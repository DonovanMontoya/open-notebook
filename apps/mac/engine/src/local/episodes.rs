//! The episode job: material, optional study notes, script, claim checks and
//! fixes by effort level, voices, then one saved recording with chapter and
//! line timing. Port of `makeEpisode` and `podcastMaterial` in `local.ts`.

use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use serde_json::json;

use super::podcasts::{CheckExtra, EpisodeJob, EpisodePatch};
use super::{AiConfig, Engine};
use crate::Error;
use crate::backend::{Cancel, Result};
use crate::podcast::WORDS_PER_SEGMENT;
use crate::podcast::audio::{SAMPLE_RATE, encode_wav};
use crate::podcast::check::{MaterialPart, RESEARCH_SYSTEM, REVISE_SYSTEM, material_text, research_prompt, revise_prompt};
use crate::podcast::produce::{ProduceInput, produce};
use crate::podcast::script::{Persona, SCRIPT_SYSTEM, ScriptInput, parse_script, script_prompt};
use crate::podcast::voices::{VoiceConfig, VoiceKind};
use crate::providers::{ChatRequest, ChatTurn};
use crate::settings::provider_secret;
use crate::types::{CheckCounts, ClaimVerdict, ContextSelection, EpisodeProfile, EpisodeStatus, PodcastEffort, SourceContext, SpeakerProfile};

fn status(s: EpisodeStatus) -> EpisodePatch {
    EpisodePatch { status: Some(s), ..Default::default() }
}

impl Engine {
    /// Script, then voices, then one saved recording with chapter and line timing.
    pub(super) fn make_episode(
        self: &Arc<Self>,
        id: &str,
        job: &EpisodeJob,
        ep: &EpisodeProfile,
        sp: &SpeakerProfile,
        config: &AiConfig,
        cancel: &Cancel,
    ) -> Result<()> {
        let kind_name = sp.voice_kind.as_deref().unwrap_or("openai");
        let kind = VoiceKind::parse(kind_name).ok_or_else(|| Error::Setup(format!("Unknown voice provider “{kind_name}”. Edit the speaker profile.")))?;
        let hosts: Vec<String> = sp.speakers.iter().map(|s| s.name.clone()).collect();
        let voices: Vec<String> = sp.speakers.iter().map(|s| s.voice_id.clone()).collect();
        let notebook = self.get_notebook(&job.notebook_id)?;
        let writer = config.chat.as_ref().ok_or_else(|| Error::Setup("Choose a chat model in Settings to write episodes.".into()))?;
        let model = writer.model.clone();
        let focus = job.focus.as_deref();

        let parts = self.podcast_material(&job.notebook_id, focus.unwrap_or(&notebook.name), config, cancel)?;
        let material = material_text(&parts);
        if material.trim().is_empty() {
            return Err(Error::other("This notebook has no processed sources or notes to talk about yet."));
        }

        let mut notes = None;
        if job.effort == PodcastEffort::Thorough {
            self.set_episode(id, status(EpisodeStatus::Researching))?;
            let request = ChatRequest::new(&model, RESEARCH_SYSTEM, vec![ChatTurn::user(research_prompt(&material, focus))]).max_tokens(8_000).cancel(cancel);
            notes = Some(self.chat_with(writer, "podcast_research", request)?);
            self.set_episode(id, status(EpisodeStatus::Writing))?;
        }

        let prompt = script_prompt(&ScriptInput {
            notebook_name: &notebook.name,
            material: &material,
            speakers: sp.speakers.iter().map(|s| Persona { name: &s.name, backstory: &s.backstory, personality: &s.personality }).collect(),
            briefing: &ep.briefing,
            segments: ep.segments,
            language: ep.language.as_deref(),
            focus,
            notes: notes.as_deref(),
        });
        {
            let conn = self.db.lock();
            conn.execute(
                "INSERT INTO podcast_prompt (episode_id, model, system, prompt, material, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(episode_id) DO UPDATE SET model = excluded.model, system = excluded.system, prompt = excluded.prompt,
                   material = excluded.material, created = excluded.created",
                params![id, model, SCRIPT_SYSTEM, prompt, serde_json::to_string(&parts)?, self.now()],
            )?;
            conn.execute("DELETE FROM podcast_check WHERE episode_id = ?1", [id])?;
        }
        let max_tokens = ep.segments * WORDS_PER_SEGMENT * 4;
        let request = ChatRequest::new(&model, SCRIPT_SYSTEM, vec![ChatTurn::user(prompt)]).max_tokens(max_tokens).cancel(cancel);
        let raw = self.chat_with(writer, "podcast_writing", request)?;
        let mut script = parse_script(&raw, hosts.len())?;

        // Careful and thorough episodes are checked, and flagged lines fixed, before any voice is recorded.
        let rounds = match job.effort {
            PodcastEffort::Thorough => 2,
            PodcastEffort::Careful => 1,
            _ => 0,
        };
        if rounds > 0 {
            let mut extra = CheckExtra { writer: Some(model.clone()), history: Some(Vec::new()) };
            // A round that fails partway keeps the last revision that was checked.
            let mut fix = || -> Result<()> {
                self.set_episode(id, status(EpisodeStatus::Checking))?;
                let mut check = self.run_check(id, &script, &parts, config, &extra, cancel)?;
                for _ in 0..rounds {
                    if check.overstated + check.unsupported == 0 {
                        break;
                    }
                    self.set_episode(id, status(EpisodeStatus::Revising))?;
                    let flagged: Vec<_> = check.claims.iter().filter(|c| c.verdict != ClaimVerdict::Supported).collect();
                    let request = ChatRequest::new(&model, REVISE_SYSTEM, vec![ChatTurn::user(revise_prompt(&script, &flagged, &material))])
                        .max_tokens(max_tokens)
                        .cancel(cancel);
                    let revised = self.chat_with(writer, "podcast_writing", request)?;
                    // Keep the checked script rather than lose the episode to a bad revision.
                    let Ok(next) = parse_script(&revised, hosts.len()) else { break };
                    if let Some(history) = extra.history.as_mut() {
                        history.push(CheckCounts { supported: check.supported, overstated: check.overstated, unsupported: check.unsupported });
                    }
                    self.set_episode(id, status(EpisodeStatus::Checking))?;
                    check = self.run_check(id, &next, &parts, config, &extra, cancel)?;
                    script = next;
                }
                Ok(())
            };
            match fix() {
                Ok(()) => {}
                Err(_) if cancel.is_cancelled() => return Err(Error::Cancelled),
                // A failed check is shown on the episode; the script is still voiced.
                Err(error) => self.save_failed_check(id, &model, &extra, &error.to_string()),
            }
        }
        self.db.lock().execute("UPDATE podcast_episode SET script = ?1 WHERE id = ?2", params![serde_json::to_string(&script)?, id])?;
        if job.effort == PodcastEffort::Standard {
            self.start_check(id, script.clone(), parts.clone(), config.clone(), Some(model.clone()));
        }
        self.set_episode(
            id,
            EpisodePatch {
                status: Some(EpisodeStatus::Voicing),
                name: Some(job.name.clone().unwrap_or_else(|| script.title.clone())),
                summary: Some(script.summary.clone()),
                done: Some(0),
                total: Some(script.line_count()),
                ..Default::default()
            },
        )?;

        let voice = VoiceConfig {
            kind,
            model: sp.voice_model.clone(),
            api_key: self.secrets.get(&provider_secret(kind.key_name()))?,
            voices: voices.clone(),
            hosts: hosts.clone(),
        };
        let provider = (self.voices)(&voice);
        let on_progress = |done: usize, total: usize| {
            let _ = self.set_episode(id, EpisodePatch { done: Some(done), total: Some(total), ..Default::default() });
        };
        let result = produce(ProduceInput { script: &script, hosts: &hosts, voices: &voices, provider: &*provider, on_progress: Some(&on_progress), cancel })?;
        if cancel.is_cancelled() {
            return Ok(());
        }
        self.record_usage("podcast_audio", &sp.voice_model, 0, 0, result.duration.round());

        self.set_episode(id, status(EpisodeStatus::Saving))?;
        let old: Option<String> = self.db.lock().query_row("SELECT audio_ref FROM podcast_episode WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        if let Some(old) = old {
            let _ = self.remove_audio(&old);
        }
        let path = self.save_audio(id, &encode_wav(&result.pcm, SAMPLE_RATE))?;
        if cancel.is_cancelled() {
            // Deleted while saving: don't leave the file behind.
            let _ = self.remove_audio(&path);
            return Ok(());
        }
        let transcript: Vec<_> = result.lines.iter().map(|l| json!({ "speaker": l.speaker, "text": l.text, "start": l.start, "end": l.end })).collect();
        self.db.lock().execute(
            "UPDATE podcast_episode SET status = 'completed', error = NULL, transcript = ?1, chapters = ?2, exact_timing = ?3,
                    duration = ?4, audio_ref = ?5, updated = ?6 WHERE id = ?7",
            params![
                serde_json::to_string(&transcript)?,
                serde_json::to_string(&result.chapters)?,
                result.exact_timing as i64,
                result.duration,
                path,
                self.now(),
                id
            ],
        )?;
        Ok(())
    }

    /// What an episode is written and checked from: each source's own text (not its insights, so claims
    /// can be traced to the paper), the most relevant passages of long sources, and the notes.
    pub(super) fn podcast_material(&self, notebook_id: &str, query: &str, config: &AiConfig, cancel: &Cancel) -> Result<Vec<MaterialPart>> {
        let ids: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT source_id FROM notebook_source WHERE notebook_id = ?1")?;
            stmt.query_map([notebook_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        let selection = ContextSelection { sources: ids.into_iter().map(|id| (id, SourceContext::Full)).collect(), ..Default::default() };
        let (sources, chunks) = self.context_for(notebook_id, query, config, cancel, Some(&selection))?;
        let notes = self.list_notes(notebook_id)?;
        let mut parts: Vec<MaterialPart> = sources.into_iter().map(|s| MaterialPart { title: s.title, text: s.text, ..Default::default() }).collect();
        parts.extend(chunks.into_iter().map(|c| MaterialPart { title: c.title, text: c.text, excerpt: true, ..Default::default() }));
        parts.extend(notes.into_iter().filter_map(|n| {
            let text = n.content.filter(|c| !c.trim().is_empty())?;
            Some(MaterialPart { title: n.title.unwrap_or_else(|| "Untitled".into()), text, note: true, ..Default::default() })
        }));
        Ok(parts)
    }
}
