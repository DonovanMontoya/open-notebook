//! Search, Ask (research across notebooks) and study decks.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::notebooks::to_note;
use super::Engine;
use crate::ask::{AskDoc, MAX_SEARCHES, final_answer_prompt, parse_strategy, search_answer_prompt, strategy_prompt};
use crate::backend::{Cancel, Result};
use crate::providers::{ChatRequest, ChatTurn};
use crate::studio::{STUDY_INPUT_LIMIT, STUDY_PREFIX, decks_from_notes, parse_deck, study_input, study_prompt};
use crate::text::{char_len, keyword_score, keyword_terms, snippet};
use crate::types::{AskDocument, AskResult, AskSearch, AskStep, HitKind, Note, SearchHit, StoredDeck, StudyDeck, StudyRequest};
use crate::Error;

const SEARCH_LIMIT: usize = 20;
/// Searches an Ask runs at the same time.
const ASK_CONCURRENCY: usize = 2;

/// Each search's (term, answer), filled in as the searches finish.
type Answers = Mutex<Vec<Option<Result<(String, String)>>>>;

impl Engine {
    fn source_notebooks(&self) -> Result<HashMap<String, String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT id, notebook_id FROM source")?;
        let map = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(map)
    }

    /// Notes with the notebook each is in, in table order.
    fn notes_in(&self, notebook_id: Option<&str>) -> Result<Vec<(Note, String)>> {
        let conn = self.db.lock();
        let map = |r: &rusqlite::Row<'_>| Ok((to_note(r)?, r.get::<_, String>("notebook_id")?));
        let rows = match notebook_id {
            Some(n) => conn.prepare("SELECT * FROM note WHERE notebook_id = ?1")?.query_map([n], map)?.collect::<rusqlite::Result<_>>()?,
            None => conn.prepare("SELECT * FROM note")?.query_map([], map)?.collect::<rusqlite::Result<_>>()?,
        };
        Ok(rows)
    }

    pub fn search(&self, query: &str, notebook_id: Option<&str>) -> Result<Vec<SearchHit>> {
        let config = self.ai_config()?;
        let chunks = self.rank_chunks(query, &config, notebook_id, SEARCH_LIMIT * 3, &Cancel::new(), None)?;
        let notebook_of = self.source_notebooks()?;
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut seen = HashSet::new();
        for c in chunks {
            if seen.insert(c.source_id.clone()) {
                hits.push(SearchHit {
                    kind: HitKind::Source,
                    snippet: snippet(&c.text, query, 220),
                    notebook_id: notebook_of.get(&c.source_id).cloned(),
                    id: c.source_id,
                    title: c.title,
                    score: c.score,
                });
            }
        }

        let terms = keyword_terms(query);
        if !terms.is_empty() {
            for (n, notebook) in self.notes_in(notebook_id)? {
                let content = n.content.clone().unwrap_or_default();
                let score = keyword_score(&format!("{}\n{content}", n.title.as_deref().unwrap_or("")), &terms);
                if score > 0.0 {
                    hits.push(SearchHit {
                        kind: HitKind::Note,
                        id: n.id,
                        title: n.title.unwrap_or_else(|| "Note".into()),
                        snippet: snippet(&content, query, 220),
                        score,
                        notebook_id: Some(notebook),
                    });
                }
            }
        }
        hits.truncate(SEARCH_LIMIT * 2);
        Ok(hits)
    }

    /// Plans searches, answers each from what it finds, then writes one cited answer.
    pub fn ask(&self, question: &str, notebook_id: Option<&str>, on_step: &mut (dyn FnMut(AskStep) + Send), cancel: &Cancel) -> Result<AskResult> {
        let config = self.ai_config()?;
        let chat_model = config.chat.clone().ok_or_else(|| Error::Setup("Choose a chat model in Settings first.".into()))?;
        let chat = |content: String| {
            let request = ChatRequest::new(&chat_model.model, "Follow the instructions in the message exactly.", vec![ChatTurn::user(content)]).cancel(cancel);
            self.chat_with(&chat_model, "ask", request)
        };

        let (reasoning, mut searches) = parse_strategy(&chat(strategy_prompt(question, MAX_SEARCHES))?, MAX_SEARCHES);
        if searches.is_empty() {
            searches.push(AskSearch { term: question.into(), instructions: "Answer the question.".into() });
        }
        on_step(AskStep::Strategy { reasoning: reasoning.clone(), searches: searches.clone() });

        let on_step = Mutex::new(on_step);
        let documents: Mutex<Vec<AskDocument>> = Mutex::default();
        let answers: Answers = Mutex::new((0..searches.len()).map(|_| None).collect());
        let next = AtomicUsize::new(0);
        let research = |search: &AskSearch| -> Result<(String, String)> {
            let hits = self.rank_chunks(&format!("{}\n{}", search.term, search.instructions), &config, notebook_id, 8, cancel, None)?;
            let notebook_of = self.source_notebooks()?;
            let mut results: Vec<(String, String, String)> = hits.iter().map(|h| (h.source_id.clone(), h.title.clone(), h.text.clone())).collect();
            let mut found: Vec<AskDocument> = hits
                .iter()
                .map(|h| AskDocument { id: h.source_id.clone(), title: h.title.clone(), notebook_id: notebook_of.get(&h.source_id).cloned() })
                .collect();
            let terms = keyword_terms(&search.term);
            if !terms.is_empty() {
                let mut notes: Vec<(f64, Note, String)> = self
                    .notes_in(notebook_id)?
                    .into_iter()
                    .map(|(n, nb)| (keyword_score(&format!("{}\n{}", n.title.as_deref().unwrap_or(""), n.content.as_deref().unwrap_or("")), &terms), n, nb))
                    .filter(|(score, _, _)| *score > 0.0)
                    .collect();
                notes.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                for (_, n, nb) in notes.into_iter().take(3) {
                    let title = n.title.clone().unwrap_or_else(|| "Note".into());
                    results.push((n.id.clone(), title.clone(), n.content.clone().unwrap_or_default()));
                    found.push(AskDocument { id: n.id, title, notebook_id: Some(nb) });
                }
            }
            {
                let mut docs = documents.lock().unwrap_or_else(|e| e.into_inner());
                for d in found {
                    match docs.iter_mut().find(|x| x.id == d.id) {
                        Some(existing) => *existing = d,
                        None => docs.push(d),
                    }
                }
            }
            let answer = if results.is_empty() {
                "Nothing in the notebooks matched this search.".to_string()
            } else {
                let docs: Vec<AskDoc<'_>> = results.iter().map(|(id, title, text)| AskDoc { id, title, text }).collect();
                chat(search_answer_prompt(question, search, &docs))?
            };
            (on_step.lock().unwrap_or_else(|e| e.into_inner()))(AskStep::Answer { term: search.term.clone(), answer: answer.clone() });
            Ok((search.term.clone(), answer))
        };
        std::thread::scope(|scope| {
            for _ in 0..ASK_CONCURRENCY.min(searches.len()) {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        let Some(search) = searches.get(i) else { return };
                        let result = if cancel.is_cancelled() { Err(Error::Cancelled) } else { research(search) };
                        answers.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(result);
                    }
                });
            }
        });
        let answers: Vec<(String, String)> = answers
            .into_inner()
            .unwrap_or_else(|e| e.into_inner())
            .into_iter()
            .map(|a| a.unwrap_or(Err(Error::Cancelled)))
            .collect::<Result<_>>()?;

        let answer = chat(final_answer_prompt(question, &reasoning, &answers))?;
        (on_step.into_inner().unwrap_or_else(|e| e.into_inner()))(AskStep::Final { answer: answer.clone() });
        Ok(AskResult { answer, documents: documents.into_inner().unwrap_or_else(|e| e.into_inner()) })
    }

    // Studio

    /// Flashcards or a quiz from chosen sources, saved as a note in the notebook.
    pub fn generate_study_deck(&self, notebook_id: &str, request: StudyRequest) -> Result<StudyDeck> {
        let config = self.ai_config()?;
        let model = config.writer().cloned().ok_or_else(|| Error::Setup("Choose a chat model in Settings first.".into()))?;
        let in_notebook: HashSet<String> = self.list_sources(notebook_id)?.into_iter().map(|s| s.id).collect();
        if request.source_ids.is_empty() || request.source_ids.iter().any(|id| !in_notebook.contains(id)) {
            return Err(Error::Setup("Choose sources from this notebook.".into()));
        }
        let mut ids: Vec<&String> = Vec::new();
        for id in &request.source_ids {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let sources = ids.into_iter().map(|id| self.get_source(id)).collect::<Result<Vec<_>>>()?;
        if sources.iter().any(|s| s.full_text.trim().is_empty()) {
            return Err(Error::Setup("Wait for every selected source to finish processing.".into()));
        }
        let input = study_input(&sources.iter().map(|s| (s.summary.id.clone(), s.summary.title.clone(), s.full_text.clone())).collect::<Vec<_>>());
        if char_len(&input) > STUDY_INPUT_LIMIT {
            return Err(Error::Setup("Select fewer sources; together they are longer than 300,000 characters.".into()));
        }
        let raw = self.chat_with(&model, "study", ChatRequest::new(&model.model, &study_prompt(&request), vec![ChatTurn::user(input)]))?;
        let deck = parse_deck(&raw, request.kind, request.count, &request.source_ids)?;
        self.create_note(notebook_id, Some(&deck.title), &format!("{STUDY_PREFIX}{}", serde_json::to_string(&deck)?), true)?;
        Ok(deck)
    }

    pub fn list_study_decks(&self, notebook_id: &str) -> Result<Vec<StoredDeck>> {
        Ok(decks_from_notes(&self.list_notes(notebook_id)?))
    }
}
