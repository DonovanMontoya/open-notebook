//! Notebook and source chat: sessions, the context sent with each question,
//! and ranking chunks by embedding similarity or keywords.

use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use super::{AiConfig, Engine, FULL_CONTEXT_CHARS, ModelChoice};
use crate::backend::{Cancel, OnText, Result, SendChat, SendChatResult, SendSourceChat};
use crate::chat::{ContextChunk, ContextNote, ContextSource, build_chat_prompt, fair_share};
use crate::db::int;
use crate::providers::{ChatRequest, ChatTurn, ProviderKind};
use crate::text::{char_len, cosine_similarity, decode_vector, keyword_score, keyword_terms, take_chars, title_from};
use crate::types::{ChatMessage, ChatSession, ChatSessionWithMessages, ContextSelection, MessageKind, SourceContext};
use crate::Error;

const HISTORY_TURNS: usize = 20;
pub(super) const RETRIEVAL_CHUNKS: usize = 40;

#[derive(Clone, Debug)]
pub(crate) struct RankedChunk {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub text: String,
    pub score: f64,
}

fn to_session(r: &Row<'_>) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: r.get("id")?,
        title: r.get("title")?,
        notebook_id: r.get("notebook_id")?,
        created: r.get("created")?,
        updated: r.get("updated")?,
        message_count: None,
        model_override: None,
    })
}

fn to_message(r: &Row<'_>) -> rusqlite::Result<ChatMessage> {
    let kind: String = r.get("type")?;
    Ok(ChatMessage {
        id: r.get("id")?,
        kind: if kind == "human" { MessageKind::Human } else { MessageKind::Ai },
        content: r.get("content")?,
        timestamp: r.get("timestamp")?,
    })
}

/// What one conversation turn needs; shared by notebook and source chat.
struct Turn<'a> {
    notebook_id: String,
    source_id: Option<String>,
    session_id: Option<String>,
    message: String,
    model_override: Option<String>,
    on_text: Option<OnText<'a>>,
    cancel: Cancel,
}

impl Engine {
    fn sessions(&self, filter: &str, key: &str) -> Result<Vec<ChatSession>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT s.*, (SELECT COUNT(*) FROM chat_message m WHERE m.session_id = s.id) AS message_count
               FROM chat_session s WHERE {filter} ORDER BY s.updated DESC"
        ))?;
        let rows = stmt
            .query_map([key], |r| {
                let mut s = to_session(r)?;
                s.message_count = Some(int(r, "message_count")? as u32);
                Ok(s)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_chat_sessions(&self, notebook_id: &str) -> Result<Vec<ChatSession>> {
        self.sessions("s.notebook_id = ?1 AND s.source_id IS NULL", notebook_id)
    }

    pub fn list_source_chat_sessions(&self, source_id: &str) -> Result<Vec<ChatSession>> {
        self.sessions("s.source_id = ?1", source_id)
    }

    pub fn get_chat_session(&self, id: &str) -> Result<ChatSessionWithMessages> {
        let conn = self.db.lock();
        let mut session = conn
            .query_row("SELECT * FROM chat_session WHERE id = ?1", [id], to_session)
            .optional()?
            .ok_or_else(|| Error::NotFound("Chat session".into()))?;
        let mut stmt = conn.prepare("SELECT id, type, content, timestamp FROM chat_message WHERE session_id = ?1 ORDER BY timestamp, id")?;
        let messages = stmt.query_map([id], to_message)?.collect::<rusqlite::Result<Vec<_>>>()?;
        session.message_count = Some(messages.len() as u32);
        Ok(ChatSessionWithMessages { session, messages })
    }

    pub fn delete_chat_session(&self, id: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM chat_message WHERE session_id = ?1", [id])?;
        conn.execute("DELETE FROM chat_session WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn rename_chat_session(&self, id: &str, title: &str) -> Result<()> {
        let title = if title.trim().is_empty() { "Untitled conversation" } else { title.trim() };
        self.db.lock().execute("UPDATE chat_session SET title = ?1 WHERE id = ?2", params![title, id])?;
        Ok(())
    }

    pub fn send_chat_message(&self, args: SendChat<'_>) -> Result<SendChatResult> {
        let notebook = self.get_notebook(&args.notebook_id)?;
        let selection = args.context.clone();
        let (notebook_id, message, cancel) = (args.notebook_id.clone(), args.message.clone(), args.cancel.clone());
        let turn = Turn {
            notebook_id: args.notebook_id,
            source_id: None,
            session_id: args.session_id,
            message: args.message,
            model_override: args.model_override,
            on_text: args.on_text,
            cancel: args.cancel,
        };
        self.converse(turn, |config| {
            let (sources, chunks) = self.context_for(&notebook_id, &message, config, &cancel, selection.as_ref())?;
            let notes: Vec<ContextNote> = self
                .list_notes(&notebook_id)?
                .into_iter()
                .filter(|n| selection.as_ref().and_then(|s| s.notes.get(&n.id)) != Some(&false))
                .map(|n| ContextNote { title: n.title.unwrap_or_else(|| "Note".into()), content: n.content.unwrap_or_default() })
                .collect();
            Ok(build_chat_prompt(&notebook.name, &notebook.description, &sources, &chunks, &notes))
        })
    }

    pub fn send_source_chat_message(&self, args: SendSourceChat<'_>) -> Result<SendChatResult> {
        let source = self.get_source(&args.source_id)?;
        let notebook_id = source.summary.notebooks.first().cloned().unwrap_or_default();
        let notebook = self.get_notebook(&notebook_id)?;
        let (source_id, message, cancel) = (args.source_id.clone(), args.message.clone(), args.cancel.clone());
        let turn = Turn {
            notebook_id: notebook_id.clone(),
            source_id: Some(args.source_id),
            session_id: args.session_id,
            message: args.message,
            model_override: args.model_override,
            on_text: args.on_text,
            cancel: args.cancel,
        };
        self.converse(turn, |config| {
            let title = source.summary.title.clone().unwrap_or_else(|| "Untitled".into());
            let insight_text = self
                .list_insights(&source_id)?
                .into_iter()
                .filter(|i| i.status.as_deref() == Some("completed"))
                .map(|i| format!("## {}\n{}", i.kind, i.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            let fits = char_len(&source.full_text) <= FULL_CONTEXT_CHARS;
            let chunks: Vec<ContextChunk> = if fits {
                Vec::new()
            } else {
                let only = HashSet::from([source_id.clone()]);
                self.rank_chunks(&message, config, Some(&notebook_id), RETRIEVAL_CHUNKS, &cancel, Some(&only))?
                    .into_iter()
                    .map(|h| ContextChunk { title: h.title, text: h.text })
                    .collect()
            };
            let mut sources = Vec::new();
            if fits {
                sources.push(ContextSource { title: title.clone(), text: source.full_text.clone() });
            }
            if !insight_text.is_empty() {
                sources.push(ContextSource { title: format!("{title} (insights)"), text: insight_text });
            }
            let about = format!("A conversation about one source: \"{title}\". Answer from it.");
            Ok(build_chat_prompt(&notebook.name, &about, &sources, &chunks, &[]))
        })
    }

    /// The chat model for a turn: the override ("provider::model") or Settings' chat model.
    fn chat_model(&self, config: &AiConfig, model_override: Option<&str>) -> Result<ModelChoice> {
        if let Some(over) = model_override.filter(|o| !o.is_empty()) {
            let (kind, model) = over.split_once("::").unwrap_or((over, ""));
            let provider = ProviderKind::parse(kind).and_then(|k| config.providers.get(&k).cloned());
            return match provider {
                Some(provider) if !model.is_empty() => Ok(ModelChoice { provider, model: model.into() }),
                _ => Err(Error::Setup(format!("No API key is saved for the model {over}."))),
            };
        }
        config.chat.clone().ok_or_else(|| Error::Setup("Choose a chat model in Settings first.".into()))
    }

    /// History, the model call, then saving both turns.
    fn converse(&self, turn: Turn<'_>, system: impl FnOnce(&AiConfig) -> Result<String>) -> Result<SendChatResult> {
        let config = self.ai_config()?;
        let chat = self.chat_model(&config, turn.model_override.as_deref())?;

        let mut messages: Vec<ChatTurn> = Vec::new();
        if let Some(id) = &turn.session_id {
            let existing = self.get_chat_session(id)?;
            let skip = existing.messages.len().saturating_sub(HISTORY_TURNS);
            for m in existing.messages.into_iter().skip(skip) {
                messages.push(if m.kind == MessageKind::Human { ChatTurn::user(m.content) } else { ChatTurn::assistant(m.content) });
            }
        }
        messages.push(ChatTurn::user(turn.message.clone()));

        let system = system(&config)?;
        let request = ChatRequest::new(&chat.model, &system, messages).cancel(&turn.cancel).on_text(turn.on_text);
        let reply = self.chat_with(&chat, "chat", request)?;

        // Persist only after a successful reply, so a failed request leaves no orphan turn.
        let now = self.now();
        let later = self.now_plus_ms(1);
        let session_id = {
            let mut conn = self.db.lock();
            let tx = conn.transaction()?;
            let session_id = match turn.session_id {
                Some(id) => id,
                None => {
                    let id = self.id("chat_session");
                    let title = title_from(&turn.message, 40);
                    tx.execute(
                        "INSERT INTO chat_session (id, notebook_id, source_id, title, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                        params![id, turn.notebook_id, turn.source_id, if title.is_empty() { "New chat".into() } else { title }, now],
                    )?;
                    id
                }
            };
            tx.execute(
                "INSERT INTO chat_message (id, session_id, type, content, timestamp) VALUES (?1, ?2, 'human', ?3, ?4)",
                params![self.id("chat_message"), session_id, turn.message, now],
            )?;
            tx.execute(
                "INSERT INTO chat_message (id, session_id, type, content, timestamp) VALUES (?1, ?2, 'ai', ?3, ?4)",
                params![self.id("chat_message"), session_id, reply, later],
            )?;
            tx.execute("UPDATE chat_session SET updated = ?1 WHERE id = ?2", params![later, session_id])?;
            tx.commit()?;
            session_id
        };
        let session = self.get_chat_session(&session_id)?;
        Ok(SendChatResult { messages: session.messages.clone(), session: session.session })
    }

    /// Small notebooks go to the model whole. Larger ones send each source's fair share:
    /// its opening plus the chunks most similar to the question (or keyword matches
    /// when no embedding model is set).
    pub(crate) fn context_for(
        &self,
        notebook_id: &str,
        question: &str,
        config: &AiConfig,
        cancel: &Cancel,
        selection: Option<&ContextSelection>,
    ) -> Result<(Vec<ContextSource>, Vec<ContextChunk>)> {
        struct Row {
            id: String,
            title: String,
            text: String,
            len: usize,
        }
        let (rows, insights) = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare(
                "SELECT id, title, full_text FROM source
                  WHERE id IN (SELECT source_id FROM notebook_source WHERE notebook_id = ?1) AND status = 'completed' ORDER BY created",
            )?;
            let rows: Vec<Row> = stmt
                .query_map([notebook_id], |r| {
                    let text: String = r.get::<_, Option<String>>(2)?.unwrap_or_default();
                    Ok(Row { id: r.get(0)?, title: r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "Untitled".into()), len: char_len(&text), text })
                })?
                .collect::<rusqlite::Result<_>>()?;
            let mut stmt = conn.prepare(
                "SELECT i.source_id, i.insight_type, i.content FROM source_insight i JOIN source s ON s.id = i.source_id
                  WHERE s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = ?1) AND i.status = 'completed' ORDER BY i.created",
            )?;
            let mut insights: HashMap<String, Vec<String>> = HashMap::new();
            for row in stmt.query_map([notebook_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
                let (source, kind, content) = row?;
                insights.entry(source).or_default().push(format!("## {kind}\n{content}"));
            }
            (rows, insights)
        };

        // Without a choice, a source contributes its insights if it has some and its full text otherwise.
        let mode = |id: &str| {
            selection.and_then(|s| s.sources.get(id).copied()).unwrap_or(if insights.contains_key(id) { SourceContext::Insights } else { SourceContext::Full })
        };
        let mut sources: Vec<ContextSource> = Vec::new();
        for r in &rows {
            if mode(&r.id) == SourceContext::Insights
                && let Some(parts) = insights.get(&r.id)
            {
                sources.push(ContextSource { title: format!("{} (insights)", r.title), text: parts.join("\n\n") });
            }
        }
        let full: Vec<&Row> = rows.iter().filter(|r| mode(&r.id) == SourceContext::Full).collect();
        let total: usize = full.iter().map(|r| r.len).sum();
        let insight_chars: usize = sources.iter().map(|s| char_len(&s.text)).sum();
        let budget = FULL_CONTEXT_CHARS.saturating_sub(insight_chars).max(FULL_CONTEXT_CHARS / 2);
        let whole = |r: &Row| ContextSource { title: r.title.clone(), text: r.text.clone() };
        if total <= budget {
            let mut out: Vec<ContextSource> = full.iter().map(|r| whole(r)).collect();
            out.extend(sources);
            return Ok((out, Vec::new()));
        }

        // Too much to send whole. Every source gets a fair share of the budget, so a question about all of
        // them sees all of them; a source longer than its share sends its opening plus its most relevant passages.
        let shares = fair_share(&full.iter().map(|r| r.len).collect::<Vec<_>>(), budget);
        let clipped: Vec<(&Row, usize)> = full.iter().zip(&shares).filter(|(r, s)| **s < r.len).map(|(r, s)| (*r, *s)).collect();
        let ids: HashSet<String> = clipped.iter().map(|(r, _)| r.id.clone()).collect();
        let scores: HashMap<String, f64> =
            self.rank_chunks(question, config, Some(notebook_id), usize::MAX, cancel, Some(&ids))?.into_iter().map(|h| (h.id, h.score)).collect();
        let mut chunk_rows: HashMap<String, Vec<(String, String)>> = HashMap::new();
        {
            let conn = self.db.lock();
            let placeholders = (1..=clipped.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
            let mut stmt = conn.prepare(&format!("SELECT id, source_id, text FROM chunk WHERE source_id IN ({placeholders}) ORDER BY position"))?;
            let rows = stmt.query_map(params_from_iter(clipped.iter().map(|(r, _)| r.id.as_str())), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            for row in rows {
                let (id, source, text) = row?;
                chunk_rows.entry(source).or_default().push((id, text));
            }
        }
        let mut chunks = Vec::new();
        for (r, share) in &clipped {
            let own = chunk_rows.remove(&r.id).unwrap_or_default();
            if own.is_empty() {
                chunks.push(ContextChunk { title: r.title.clone(), text: take_chars(&r.text, *share).to_string() });
                continue;
            }
            let mut ranked: Vec<&(String, String)> = own.iter().skip(1).collect();
            ranked.sort_by(|a, b| {
                let (sa, sb) = (scores.get(&a.0).copied().unwrap_or(0.0), scores.get(&b.0).copied().unwrap_or(0.0));
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            });
            ranked.insert(0, &own[0]);
            let mut picked: HashSet<&str> = HashSet::new();
            let mut used = 0;
            for (id, text) in ranked {
                let len = char_len(text);
                if used + len > *share {
                    continue;
                }
                picked.insert(id);
                used += len;
            }
            for (id, text) in &own {
                if picked.contains(id.as_str()) {
                    chunks.push(ContextChunk { title: r.title.clone(), text: text.clone() });
                }
            }
        }
        let mut out: Vec<ContextSource> = full.iter().zip(&shares).filter(|(r, s)| **s >= r.len).map(|(r, _)| whole(r)).collect();
        out.extend(sources);
        Ok((out, chunks))
    }

    /// Chunks most similar to the query by embedding, or by keywords without an embedding model.
    pub(crate) fn rank_chunks(
        &self,
        query: &str,
        config: &AiConfig,
        notebook_id: Option<&str>,
        limit: usize,
        cancel: &Cancel,
        only: Option<&HashSet<String>>,
    ) -> Result<Vec<RankedChunk>> {
        let keep = |source: &str| only.is_none_or(|o| o.contains(source));
        let in_notebook = "s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = ?2)";
        if let Some(embedding) = &config.embedding {
            let query_vector = self.embed_with(embedding, "search", &[query.to_string()], cancel)?.pop().unwrap_or_default();
            let rows: Vec<(RankedChunk, Option<String>)> = {
                let conn = self.db.lock();
                let sql = format!(
                    "SELECT c.id, c.source_id, c.text, c.embedding, s.title FROM chunk c JOIN source s ON s.id = c.source_id
                      WHERE c.embedding IS NOT NULL AND c.embedding_model = ?1{}",
                    if notebook_id.is_some() { format!(" AND {in_notebook}") } else { String::new() }
                );
                let mut stmt = conn.prepare(&sql)?;
                let map = |r: &Row<'_>| {
                    Ok((
                        RankedChunk {
                            id: r.get(0)?,
                            source_id: r.get(1)?,
                            text: r.get(2)?,
                            title: r.get::<_, Option<String>>(4)?.unwrap_or_else(|| "Untitled".into()),
                            score: 0.0,
                        },
                        r.get::<_, Option<String>>(3)?,
                    ))
                };
                match notebook_id {
                    Some(n) => stmt.query_map(params![embedding.model, n], map)?.collect::<rusqlite::Result<_>>()?,
                    None => stmt.query_map(params![embedding.model], map)?.collect::<rusqlite::Result<_>>()?,
                }
            };
            if !rows.is_empty() {
                let mut hits: Vec<RankedChunk> = rows
                    .into_iter()
                    .filter(|(c, _)| keep(&c.source_id))
                    .map(|(mut c, e)| {
                        c.score = e.and_then(|e| decode_vector(&e)).map_or(0.0, |v| cosine_similarity(&query_vector, &v));
                        c
                    })
                    .collect();
                hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                hits.truncate(limit);
                return Ok(hits);
            }
        }

        // Keyword fallback: score chunks by how many query terms they contain.
        let terms = keyword_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<RankedChunk> = {
            let conn = self.db.lock();
            let sql = format!(
                "SELECT c.id, c.source_id, c.text, s.title FROM chunk c JOIN source s ON s.id = c.source_id {}",
                if notebook_id.is_some() { "WHERE s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = ?1)" } else { "" }
            );
            let mut stmt = conn.prepare(&sql)?;
            let map = |r: &Row<'_>| {
                Ok(RankedChunk {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    text: r.get(2)?,
                    title: r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "Untitled".into()),
                    score: 0.0,
                })
            };
            match notebook_id {
                Some(n) => stmt.query_map([n], map)?.collect::<rusqlite::Result<_>>()?,
                None => stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?,
            }
        };
        let mut hits: Vec<RankedChunk> = rows
            .into_iter()
            .filter(|c| keep(&c.source_id))
            .map(|mut c| {
                c.score = keyword_score(&c.text, &terms);
                c
            })
            .filter(|c| c.score > 0.0)
            .collect();
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        Ok(hits)
    }
}
