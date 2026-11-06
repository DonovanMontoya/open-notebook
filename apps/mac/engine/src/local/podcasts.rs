//! Podcasts on device: speaker and episode profiles, episodes and their claim
//! checks, and the audio files. Port of the podcast part of `local.ts`; the
//! job that writes and voices an episode is in `episodes.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Map, Value, json};

use super::{AiConfig, Engine};
use crate::backend::{Cancel, EpisodeAudio, Result};
use crate::db::int;
use crate::podcast::check::{CHECK_SYSTEM, MaterialPart, ScriptCheck, check_prompt, material_text, parse_check};
use crate::podcast::profiles::{DEFAULT_EPISODE_PROFILES, DEFAULT_SPEAKER_PROFILES};
use crate::podcast::script::PodcastScript;
use crate::podcast::voices::{VoiceConfig, VoiceKind, VoiceProvider};
use crate::providers::{ChatRequest, ChatTurn};
use crate::settings::provider_secret;
use crate::types::{
    CheckCounts, CreateEpisode, Episode, EpisodeCheck, EpisodePrompt, EpisodeProfile, EpisodeStatus, PodcastEffort, PodcastProfiles, SpeakerProfile,
};
use crate::Error;

/// Where finished episodes are kept: the Tauri app's folder, so either app can play them.
const PODCASTS: &str = "podcasts";

/// Episodes with their claim check, if any.
const EPISODE_SELECT: &str = "SELECT e.*, c.status AS check_status, c.model AS check_model, c.result AS check_result, c.error AS check_error
  FROM podcast_episode e LEFT JOIN podcast_check c ON c.episode_id = e.id";

/// What an episode job needs from the row or the request.
#[derive(Clone, Debug)]
pub(super) struct EpisodeJob {
    pub notebook_id: String,
    pub focus: Option<String>,
    pub name: Option<String>,
    pub effort: PodcastEffort,
}

/// Columns to change on an episode; `updated` is always set.
#[derive(Default)]
pub(super) struct EpisodePatch {
    pub status: Option<EpisodeStatus>,
    pub error: Option<Option<String>>,
    pub done: Option<usize>,
    pub total: Option<usize>,
    pub name: Option<String>,
    pub summary: Option<String>,
}

/// What a claim check saves alongside its result.
#[derive(Clone, Debug, Default)]
pub(super) struct CheckExtra {
    pub writer: Option<String>,
    pub history: Option<Vec<CheckCounts>>,
}

impl CheckExtra {
    fn to_map(&self) -> Map<String, Value> {
        let mut map = Map::new();
        if let Some(w) = &self.writer {
            map.insert("writer".into(), json!(w));
        }
        if let Some(h) = &self.history {
            map.insert("history".into(), serde_json::to_value(h).unwrap_or_default());
        }
        map
    }
}

pub(super) fn status_name(status: EpisodeStatus) -> String {
    serde_json::to_value(status).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

pub(super) fn effort_name(effort: PodcastEffort) -> String {
    serde_json::to_value(effort).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn parse_effort(s: Option<String>) -> Option<PodcastEffort> {
    serde_json::from_value(Value::String(s?)).ok()
}

fn json_column<T: serde::de::DeserializeOwned + Default>(text: Option<String>) -> T {
    text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn to_episode(r: &Row<'_>) -> rusqlite::Result<Episode> {
    let total = int(r, "progress_total")?;
    let check = match r.get::<_, Option<String>>("check_status")? {
        None => None,
        Some(status) => {
            let mut map: Map<String, Value> = json_column(r.get("check_result")?);
            map.insert("status".into(), json!(status));
            map.insert("model".into(), json!(r.get::<_, Option<String>>("check_model")?.unwrap_or_default()));
            if let Some(error) = r.get::<_, Option<String>>("check_error")? {
                map.insert("error".into(), json!(error));
            }
            serde_json::from_value::<EpisodeCheck>(Value::Object(map)).ok()
        }
    };
    Ok(Episode {
        id: r.get("id")?,
        notebook_id: Some(r.get("notebook_id")?),
        name: r.get("name")?,
        summary: r.get("summary")?,
        status: serde_json::from_value(json!(r.get::<_, String>("status")?)).unwrap_or(EpisodeStatus::Failed),
        progress: (total != 0).then(|| -> rusqlite::Result<(u32, u32)> { Ok((int(r, "progress_done")? as u32, total as u32)) }).transpose()?,
        error: r.get("error")?,
        created: r.get("created")?,
        duration: r.get("duration")?,
        chapters: json_column(r.get("chapters")?),
        transcript: json_column(r.get("transcript")?),
        exact_timing: int(r, "exact_timing")? != 0,
        voice_label: r.get("voice_label")?,
        effort: parse_effort(r.get("effort")?),
        check,
    })
}

fn to_speaker_profile(r: &Row<'_>) -> rusqlite::Result<SpeakerProfile> {
    Ok(SpeakerProfile {
        id: r.get("id")?,
        name: r.get("name")?,
        description: r.get::<_, Option<String>>("description")?.unwrap_or_default(),
        voice_kind: r.get("voice_kind")?,
        voice_model: r.get("voice_model")?,
        speakers: json_column(r.get("speakers")?),
    })
}

fn to_episode_profile(r: &Row<'_>) -> rusqlite::Result<EpisodeProfile> {
    Ok(EpisodeProfile {
        id: r.get("id")?,
        name: r.get("name")?,
        description: r.get::<_, Option<String>>("description")?.unwrap_or_default(),
        speaker_profile_id: r.get("speaker_profile_id")?,
        language: r.get("language")?,
        briefing: r.get("briefing")?,
        segments: int(r, "segments")? as u32,
    })
}

/// Letters, digits, `-` and `_`, as the Tauri app's `podcasts.rs` names episode files.
fn safe_name(id: &str) -> Result<String> {
    let name: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if name.is_empty() || name.len() > 128 {
        return Err(Error::other("Invalid episode id"));
    }
    Ok(name)
}

impl Engine {
    // Profiles

    /// Adds the server's default speaker and episode profiles the first time the app runs.
    pub(super) fn seed_podcast_profiles(&self) -> Result<()> {
        let seeded: Option<String> =
            self.db.lock().query_row("SELECT value FROM app_setting WHERE key = 'podcast_profiles_seeded'", [], |r| r.get(0)).optional()?;
        if seeded.is_some() {
            return Ok(());
        }
        let mut ids = std::collections::HashMap::new();
        for sp in &DEFAULT_SPEAKER_PROFILES {
            let saved = self.save_speaker_profile(SpeakerProfile {
                id: String::new(),
                name: sp.name.into(),
                description: sp.description.into(),
                voice_kind: Some(sp.voice_kind.into()),
                voice_model: sp.voice_model.into(),
                speakers: sp.speakers(),
            })?;
            ids.insert(sp.name, saved.id);
        }
        for ep in &DEFAULT_EPISODE_PROFILES {
            if let Some(speaker_profile_id) = ids.get(ep.speaker_profile_name) {
                self.save_episode_profile(EpisodeProfile {
                    id: String::new(),
                    name: ep.name.into(),
                    description: ep.description.into(),
                    speaker_profile_id: speaker_profile_id.clone(),
                    language: None,
                    briefing: ep.briefing.into(),
                    segments: ep.segments,
                })?;
            }
        }
        self.db.lock().execute("INSERT INTO app_setting (key, value) VALUES ('podcast_profiles_seeded', '1')", [])?;
        Ok(())
    }

    pub fn podcast_profiles(&self) -> Result<PodcastProfiles> {
        let conn = self.db.lock();
        let speaker = conn.prepare("SELECT * FROM speaker_profile ORDER BY name")?.query_map([], to_speaker_profile)?.collect::<rusqlite::Result<_>>()?;
        let episode = conn.prepare("SELECT * FROM episode_profile ORDER BY name")?.query_map([], to_episode_profile)?.collect::<rusqlite::Result<_>>()?;
        Ok(PodcastProfiles { episode, speaker })
    }

    /// Creates the profile when `id` is empty.
    pub fn save_speaker_profile(&self, p: SpeakerProfile) -> Result<SpeakerProfile> {
        if p.speakers.is_empty() || p.speakers.len() > 4 {
            return Err(Error::other("A speaker profile has one to four speakers."));
        }
        let id = if p.id.is_empty() { self.id("speaker_profile") } else { p.id.clone() };
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO speaker_profile (id, name, description, voice_kind, voice_model, speakers) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, voice_kind = excluded.voice_kind,
               voice_model = excluded.voice_model, speakers = excluded.speakers",
            params![
                id,
                p.name.trim(),
                p.description,
                p.voice_kind.as_deref().filter(|k| !k.is_empty()).unwrap_or("openai"),
                p.voice_model,
                serde_json::to_string(&p.speakers)?
            ],
        )?;
        Ok(conn.query_row("SELECT * FROM speaker_profile WHERE id = ?1", [&id], to_speaker_profile)?)
    }

    pub fn delete_speaker_profile(&self, id: &str) -> Result<()> {
        let conn = self.db.lock();
        let used: Option<String> = conn.query_row("SELECT name FROM episode_profile WHERE speaker_profile_id = ?1", [id], |r| r.get(0)).optional()?;
        if let Some(name) = used {
            return Err(Error::other(format!("The episode profile “{name}” uses these speakers. Change it first.")));
        }
        conn.execute("DELETE FROM speaker_profile WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Creates the profile when `id` is empty. Segments are kept between 1 and 12.
    pub fn save_episode_profile(&self, p: EpisodeProfile) -> Result<EpisodeProfile> {
        let id = if p.id.is_empty() { self.id("episode_profile") } else { p.id.clone() };
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO episode_profile (id, name, description, speaker_profile_id, language, briefing, segments) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, speaker_profile_id = excluded.speaker_profile_id,
               language = excluded.language, briefing = excluded.briefing, segments = excluded.segments",
            params![id, p.name.trim(), p.description, p.speaker_profile_id, p.language, p.briefing, p.segments.clamp(1, 12)],
        )?;
        Ok(conn.query_row("SELECT * FROM episode_profile WHERE id = ?1", [&id], to_episode_profile)?)
    }

    pub fn delete_episode_profile(&self, id: &str) -> Result<()> {
        self.db.lock().execute("DELETE FROM episode_profile WHERE id = ?1", [id])?;
        Ok(())
    }

    fn profiles_for(&self, episode_profile_id: &str, speaker_profile_id: Option<&str>) -> Result<(EpisodeProfile, SpeakerProfile)> {
        let PodcastProfiles { episode, speaker } = self.podcast_profiles()?;
        let ep = episode.into_iter().find(|p| p.id == episode_profile_id).ok_or_else(|| Error::other("Choose an episode profile."))?;
        let wanted = speaker_profile_id.filter(|s| !s.is_empty()).unwrap_or(&ep.speaker_profile_id).to_string();
        let sp = speaker
            .into_iter()
            .find(|p| p.id == wanted)
            .ok_or_else(|| Error::other("This episode profile’s speakers were deleted. Choose a speaker profile."))?;
        Ok((ep, sp))
    }

    /// The voice provider for a kind and model, with the saved key. The UI uses it to list voices and models.
    pub fn voice_provider(&self, kind: VoiceKind, model: &str) -> Result<Arc<dyn VoiceProvider>> {
        let config = VoiceConfig {
            kind,
            model: if model.is_empty() { kind.default_model().into() } else { model.into() },
            api_key: self.secrets.get(&provider_secret(kind.key_name()))?,
            voices: Vec::new(),
            hosts: Vec::new(),
        };
        Ok((self.voices)(&config))
    }

    // Episodes

    pub fn list_episodes(&self, notebook_id: &str) -> Result<Vec<Episode>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!("{EPISODE_SELECT} WHERE e.notebook_id = ?1 ORDER BY e.created DESC"))?;
        let rows = stmt.query_map([notebook_id], to_episode)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_episode(&self, id: &str) -> Result<Episode> {
        self.db
            .lock()
            .query_row(&format!("{EPISODE_SELECT} WHERE e.id = ?1"), [id], to_episode)
            .optional()?
            .ok_or_else(|| Error::NotFound("Episode".into()))
    }

    /// Saves the episode and starts writing and voicing it in the background.
    pub fn create_episode(self: &Arc<Self>, input: CreateEpisode) -> Result<Episode> {
        let config = self.ai_config()?;
        if config.chat.is_none() {
            return Err(Error::Setup("Choose a chat model in Settings to write episodes.".into()));
        }
        self.get_notebook(&input.notebook_id)?;
        let (ep, sp) = self.profiles_for(&input.episode_profile, input.speaker_profile.as_deref())?;
        let id = self.id("episode");
        let kind = VoiceKind::parse(sp.voice_kind.as_deref().unwrap_or("openai")).unwrap_or(VoiceKind::OpenAi);
        let first_names = sp.speakers.iter().map(|s| s.name.split(' ').next().unwrap_or_default()).collect::<Vec<_>>().join(", ");
        let job = EpisodeJob {
            notebook_id: input.notebook_id.clone(),
            focus: input.focus.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(String::from),
            name: input.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(String::from),
            effort: input.effort.unwrap_or_default(),
        };
        self.db.lock().execute(
            "INSERT INTO podcast_episode (id, notebook_id, name, focus, format, length, status, voice_label, episode_profile_id, speaker_profile_id, effort, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'writing', ?7, ?8, ?9, ?11, ?10, ?10)",
            params![
                id,
                job.notebook_id,
                job.name.as_deref().unwrap_or("New episode"),
                job.focus,
                if sp.speakers.len() > 1 { "conversation" } else { "monologue" },
                ep.segments.to_string(),
                format!("{first_names}, {}", kind.label()),
                ep.id,
                sp.id,
                self.now(),
                effort_name(job.effort),
            ],
        )?;
        self.start_episode(&id, job, ep, sp, config);
        self.get_episode(&id)
    }

    /// Runs a failed (or finished) episode again with the choices it was made with.
    pub fn retry_episode(self: &Arc<Self>, id: &str) -> Result<()> {
        type RetryRow = (String, Option<String>, Option<String>, Option<String>, Option<String>);
        let row: Option<RetryRow> = self
            .db
            .lock()
            .query_row("SELECT notebook_id, focus, episode_profile_id, speaker_profile_id, effort FROM podcast_episode WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .optional()?;
        let Some((notebook_id, focus, episode_profile_id, speaker_profile_id, effort)) = row else {
            return Err(Error::NotFound("Episode".into()));
        };
        let Some(episode_profile_id) = episode_profile_id else {
            return Err(Error::other("This episode was made before profiles; create a new one instead."));
        };
        let (ep, sp) = self.profiles_for(&episode_profile_id, speaker_profile_id.as_deref())?;
        let config = self.ai_config()?;
        self.set_episode(id, EpisodePatch { status: Some(EpisodeStatus::Writing), error: Some(None), done: Some(0), total: Some(0), ..Default::default() })?;
        let job = EpisodeJob { notebook_id, focus: focus.filter(|f| !f.is_empty()), name: None, effort: parse_effort(effort).unwrap_or_default() };
        self.start_episode(id, job, ep, sp, config);
        Ok(())
    }

    fn start_episode(self: &Arc<Self>, id: &str, job: EpisodeJob, ep: EpisodeProfile, sp: SpeakerProfile, config: AiConfig) {
        let (generation, cancel) = self.start_running(id);
        let engine = self.clone();
        let id = id.to_string();
        self.podcast_jobs.push(move || {
            if let Err(error) = engine.make_episode(&id, &job, &ep, &sp, &config, &cancel)
                && !cancel.is_cancelled()
            {
                let _ = engine.set_episode(&id, EpisodePatch { status: Some(EpisodeStatus::Failed), error: Some(Some(error.to_string())), ..Default::default() });
            }
            engine.finish_running(&id, generation);
        });
    }

    /// Stops the episode's work and deletes it with its audio file, prompt and check.
    pub fn delete_episode(&self, id: &str) -> Result<()> {
        self.cancel_running(id);
        self.cancel_running(&format!("check:{id}"));
        let audio: Option<String> =
            self.db.lock().query_row("SELECT audio_ref FROM podcast_episode WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        if let Some(audio) = audio {
            let _ = self.remove_audio(&audio);
        }
        let conn = self.db.lock();
        conn.execute("DELETE FROM podcast_check WHERE episode_id = ?1", [id])?;
        conn.execute("DELETE FROM podcast_prompt WHERE episode_id = ?1", [id])?;
        conn.execute("DELETE FROM podcast_episode WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn episode_audio(&self, id: &str) -> Result<EpisodeAudio> {
        let audio: Option<String> =
            self.db.lock().query_row("SELECT audio_ref FROM podcast_episode WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        let path = PathBuf::from(audio.ok_or_else(|| Error::other("This episode has no audio yet."))?);
        if !path.exists() {
            return Err(Error::other("This episode’s audio file is missing."));
        }
        Ok(EpisodeAudio::File(path))
    }

    pub fn episode_prompt(&self, id: &str) -> Result<Option<EpisodePrompt>> {
        Ok(self
            .db
            .lock()
            .query_row("SELECT model, system, prompt FROM podcast_prompt WHERE episode_id = ?1", [id], |r| {
                Ok(EpisodePrompt { model: r.get(0)?, system: r.get(1)?, prompt: r.get(2)? })
            })
            .optional()?)
    }

    pub(super) fn set_episode(&self, id: &str, patch: EpisodePatch) -> Result<()> {
        let mut sets: Vec<&str> = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        use rusqlite::types::Value as V;
        if let Some(s) = patch.status {
            sets.push("status");
            values.push(V::Text(status_name(s)));
        }
        if let Some(e) = patch.error {
            sets.push("error");
            values.push(e.map_or(V::Null, V::Text));
        }
        if let Some(d) = patch.done {
            sets.push("progress_done");
            values.push(V::Integer(d as i64));
        }
        if let Some(t) = patch.total {
            sets.push("progress_total");
            values.push(V::Integer(t as i64));
        }
        if let Some(n) = patch.name.filter(|n| !n.is_empty()) {
            sets.push("name");
            values.push(V::Text(n));
        }
        if let Some(s) = patch.summary {
            sets.push("summary");
            values.push(V::Text(s));
        }
        sets.push("updated");
        values.push(V::Text(self.now()));
        let assignments = sets.iter().enumerate().map(|(i, c)| format!("{c} = ?{}", i + 1)).collect::<Vec<_>>().join(", ");
        values.push(V::Text(id.into()));
        self.db.lock().execute(
            &format!("UPDATE podcast_episode SET {assignments} WHERE id = ?{}", values.len()),
            rusqlite::params_from_iter(values),
        )?;
        Ok(())
    }

    // Claim checks

    /// Checks the episode's script again in the background, against the material it was written from.
    pub fn check_episode(self: &Arc<Self>, id: &str) -> Result<()> {
        let row: Option<(String, Option<String>, Option<String>)> = self
            .db
            .lock()
            .query_row("SELECT notebook_id, focus, script FROM podcast_episode WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let Some((notebook_id, focus, script)) = row else { return Err(Error::NotFound("Episode".into())) };
        let Some(script) = script else { return Err(Error::other("This episode has no script to check yet.")) };
        let config = self.ai_config()?;
        if config.chat.is_none() {
            return Err(Error::Setup("Choose a chat model in Settings to check episodes.".into()));
        }
        let saved: Option<(String, Option<String>)> = self
            .db
            .lock()
            .query_row("SELECT model, material FROM podcast_prompt WHERE episode_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        // Check against the material the script was written from when it was kept.
        let material: Vec<MaterialPart> = match saved.as_ref().and_then(|(_, m)| m.as_deref()) {
            Some(m) => serde_json::from_str(m)?,
            None => {
                let query = match focus.filter(|f| !f.is_empty()) {
                    Some(f) => f,
                    None => self.get_notebook(&notebook_id)?.name,
                };
                self.podcast_material(&notebook_id, &query, &config, &Cancel::new())?
            }
        };
        let script: PodcastScript = serde_json::from_str(&script)?;
        self.start_check(id, script, material, config, saved.map(|(model, _)| model));
        Ok(())
    }

    fn save_check(&self, id: &str, status: &str, model: &str, result: Option<Map<String, Value>>, error: Option<&str>) -> Result<()> {
        let result = result.map(|r| Value::Object(r).to_string());
        self.db.lock().execute(
            "INSERT INTO podcast_check (episode_id, status, model, result, error, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(episode_id) DO UPDATE SET status = excluded.status, model = excluded.model, result = excluded.result, error = excluded.error, created = excluded.created",
            params![id, status, model, result, error, self.now()],
        )?;
        Ok(())
    }

    /// A failed check, saved on the episode. A failed check never fails the episode.
    pub(super) fn save_failed_check(&self, id: &str, model: &str, extra: &CheckExtra, message: &str) {
        let extra = extra.to_map();
        let _ = self.save_check(id, "failed", model, (!extra.is_empty()).then_some(extra), Some(message));
    }

    /// Checks a script's claims, saves the result on the episode and returns it.
    pub(super) fn run_check(&self, id: &str, script: &PodcastScript, material: &[MaterialPart], config: &AiConfig, extra: &CheckExtra, cancel: &Cancel) -> Result<ScriptCheck> {
        let choice = config.chat.as_ref().ok_or_else(|| Error::Setup("Choose a chat model in Settings to check episodes.".into()))?;
        self.save_check(id, "checking", &choice.model, Some(extra.to_map()), None)?;
        let request = ChatRequest::new(&choice.model, CHECK_SYSTEM, vec![ChatTurn::user(check_prompt(script, &material_text(material)))])
            .max_tokens(16_000)
            .cancel(cancel);
        let raw = self.chat_with(choice, "podcast_check", request)?;
        let check = parse_check(&raw, script, material)?;
        if !cancel.is_cancelled() {
            let mut result = extra.to_map();
            if let Value::Object(fields) = serde_json::to_value(&check)? {
                result.extend(fields);
            }
            self.save_check(id, "done", &choice.model, Some(result), None)?;
        }
        Ok(check)
    }

    /// Runs the claim check in the background.
    pub(super) fn start_check(self: &Arc<Self>, id: &str, script: PodcastScript, material: Vec<MaterialPart>, config: AiConfig, writer: Option<String>) {
        let key = format!("check:{id}");
        self.cancel_running(&key);
        let (generation, cancel) = self.start_running(&key);
        let engine = self.clone();
        let id = id.to_string();
        self.podcast_jobs.push(move || {
            let extra = CheckExtra { writer, history: None };
            if let Err(error) = engine.run_check(&id, &script, &material, &config, &extra, &cancel)
                && !cancel.is_cancelled()
            {
                let model = config.chat.as_ref().map(|c| c.model.clone()).unwrap_or_default();
                engine.save_failed_check(&id, &model, &extra, &error.to_string());
            }
            engine.finish_running(&key, generation);
        });
    }

    // Audio files

    fn podcast_dir(&self) -> PathBuf {
        self.data_dir.join(PODCASTS)
    }

    /// Saves a WAV recording as the Tauri app does: `<data_dir>/podcasts/<id>.wav`,
    /// converted to AAC (`.m4a`, about a tenth of the size) on macOS. Returns the absolute path.
    pub(super) fn save_audio(&self, episode_id: &str, wav: &[u8]) -> Result<String> {
        let name = safe_name(episode_id)?;
        let dir = self.podcast_dir();
        std::fs::create_dir_all(&dir)?;
        let wav_path = dir.join(format!("{name}.wav"));
        std::fs::write(&wav_path, wav)?;
        if let Some(m4a) = to_aac(&wav_path, &dir.join(format!("{name}.m4a"))) {
            let _ = std::fs::remove_file(&wav_path);
            return Ok(m4a.to_string_lossy().into_owned());
        }
        Ok(wav_path.to_string_lossy().into_owned())
    }

    /// Deletes a saved episode file. Only paths inside the podcasts folder are accepted.
    pub(super) fn remove_audio(&self, audio_ref: &str) -> Result<()> {
        let Ok(target) = Path::new(audio_ref).canonicalize() else { return Ok(()) }; // already gone
        let dir = self.podcast_dir().canonicalize()?;
        if !target.starts_with(&dir) {
            return Err(Error::other("Refusing to delete outside the podcasts folder"));
        }
        std::fs::remove_file(target)?;
        Ok(())
    }
}

/// Converts with the system's afconvert, like the Tauri app; None when it isn't available.
#[cfg(target_os = "macos")]
fn to_aac(wav: &Path, m4a: &Path) -> Option<PathBuf> {
    let ok = std::process::Command::new("/usr/bin/afconvert")
        .args(["-f", "m4af", "-d", "aac", "-b", "96000"])
        .arg(wav)
        .arg(m4a)
        .status()
        .is_ok_and(|s| s.success());
    (ok && m4a.exists()).then(|| m4a.to_path_buf())
}

#[cfg(not(target_os = "macos"))]
fn to_aac(_wav: &Path, _m4a: &Path) -> Option<PathBuf> {
    None
}
