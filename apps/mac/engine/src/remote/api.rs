//! The server's endpoints, one method each, as in `packages/api-client/src/client.ts`.
//! The Backend methods use most of them; the server settings screens use the rest
//! (credentials, models, jobs, settings).

use std::time::Duration;

use serde_json::{Map, Value, json};

use super::client::{Body, Client, Download, seg};
use super::wire::*;
use crate::Error;
use crate::backend::Result;
use crate::net::Multipart;
use crate::types::*;

fn some(v: impl ToString) -> Option<String> {
    Some(v.to_string())
}

/// A file for the server to read itself (PDF, audio, video, Office...).
pub struct Upload<'a> {
    pub data: &'a [u8],
    pub file_name: &'a str,
    pub mime: &'a str,
    pub title: Option<&'a str>,
    pub transformations: &'a [String],
}

impl Client {
    pub fn auth_status(&self) -> Result<AuthStatus> {
        self.json("GET", "/auth/status", &[], Body::None, Some(Duration::from_secs(15)))
    }

    // Notebooks

    pub fn list_notebooks(&self, archived: Option<bool>) -> Result<Vec<Notebook>> {
        self.get("/notebooks", &[("archived", archived.map(|a| a.to_string())), ("order_by", some("updated desc"))])
    }

    pub fn get_notebook(&self, id: &str) -> Result<Notebook> {
        self.get(&format!("/notebooks/{}", seg(id)), &[])
    }

    pub fn create_notebook(&self, name: &str, description: Option<&str>) -> Result<Notebook> {
        let mut body = json!({ "name": name });
        if let Some(d) = description {
            body["description"] = json!(d);
        }
        self.post("/notebooks", Some(body))
    }

    pub fn update_notebook(&self, id: &str, update: &NotebookUpdate) -> Result<Notebook> {
        let mut body = Map::new();
        if let Some(v) = &update.name {
            body.insert("name".into(), json!(v));
        }
        if let Some(v) = &update.description {
            body.insert("description".into(), json!(v));
        }
        if let Some(v) = update.archived {
            body.insert("archived".into(), json!(v));
        }
        self.put(&format!("/notebooks/{}", seg(id)), Value::Object(body))
    }

    /// Deletes the notebook, its notes, and sources that belong to no other notebook.
    pub fn delete_notebook(&self, id: &str) -> Result<()> {
        self.call("DELETE", &format!("/notebooks/{}", seg(id)), &[("delete_exclusive_sources", some(true))], None)
    }

    // Sources

    pub fn list_sources(&self, notebook_id: &str) -> Result<Vec<ServerSource>> {
        self.get(
            "/sources",
            &[("notebook_id", some(notebook_id)), ("sort_by", some("updated")), ("sort_order", some("desc")), ("limit", some(100))],
        )
    }

    pub fn list_all_sources(&self) -> Result<Vec<ServerSource>> {
        self.get("/sources", &[("sort_by", some("updated")), ("sort_order", some("desc")), ("limit", some(500))])
    }

    pub fn get_source(&self, id: &str) -> Result<ServerSource> {
        self.get(&format!("/sources/{}", seg(id)), &[])
    }

    pub fn source_insights(&self, id: &str) -> Result<Vec<ServerInsight>> {
        self.get(&format!("/sources/{}/insights", seg(id)), &[])
    }

    /// Queues a link or pasted text for background processing and embedding.
    /// `source` is `{type: "link", url, title?}` or `{type: "text", content, title?}`, plus `transformations`.
    pub fn create_source(&self, notebook_id: &str, mut source: Map<String, Value>) -> Result<ServerSource> {
        source.insert("notebooks".into(), json!([notebook_id]));
        source.insert("embed".into(), json!(true));
        source.insert("async_processing".into(), json!(true));
        self.post("/sources/json", Some(Value::Object(source)))
    }

    /// Uploads a file as multipart form data, the way the web app's FormData does.
    pub fn upload_source(&self, notebook_id: &str, file: Upload<'_>) -> Result<ServerSource> {
        let mut form = Multipart::new()
            .text("type", "upload")
            .text("notebooks", &json!([notebook_id]).to_string())
            .file("file", file.file_name, file.mime, file.data);
        if let Some(t) = file.title.filter(|t| !t.is_empty()) {
            form = form.text("title", t);
        }
        if !file.transformations.is_empty() {
            form = form.text("transformations", &json!(file.transformations).to_string());
        }
        form = form.text("embed", "true").text("async_processing", "true");
        self.json("POST", "/sources", &[], Body::Form(form), Some(Duration::from_secs(600)))
    }

    pub fn retry_source(&self, id: &str) -> Result<()> {
        self.call("POST", &format!("/sources/{}/retry", seg(id)), &[], None)
    }

    /// The original uploaded file.
    pub fn download_source(&self, id: &str) -> Result<Download> {
        self.download(&format!("/sources/{}/download", seg(id)))
    }

    pub fn update_source(&self, id: &str, title: &str) -> Result<ServerSource> {
        self.put(&format!("/sources/{}", seg(id)), json!({ "title": title }))
    }

    pub fn link_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.call("POST", &format!("/notebooks/{}/sources/{}", seg(notebook_id), seg(source_id)), &[], None)
    }

    pub fn unlink_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.call("DELETE", &format!("/notebooks/{}/sources/{}", seg(notebook_id), seg(source_id)), &[], None)
    }

    pub fn delete_source(&self, id: &str) -> Result<()> {
        self.delete(&format!("/sources/{}", seg(id)))
    }

    // Transformations and insights

    pub fn list_transformations(&self) -> Result<Vec<ServerTransformation>> {
        self.get("/transformations", &[])
    }

    pub fn create_transformation(&self, body: Value) -> Result<ServerTransformation> {
        self.post("/transformations", Some(body))
    }

    pub fn update_transformation(&self, id: &str, body: Value) -> Result<ServerTransformation> {
        self.put(&format!("/transformations/{}", seg(id)), body)
    }

    pub fn delete_transformation(&self, id: &str) -> Result<()> {
        self.delete(&format!("/transformations/{}", seg(id)))
    }

    pub fn execute_transformation(&self, transformation_id: &str, input_text: &str) -> Result<String> {
        let out: Value = self.post("/transformations/execute", Some(json!({ "transformation_id": transformation_id, "input_text": input_text })))?;
        Ok(out.get("output").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    pub fn transformation_instructions(&self) -> Result<String> {
        let out: Value = self.get("/transformations/default-prompt", &[])?;
        Ok(out.get("transformation_instructions").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    pub fn set_transformation_instructions(&self, text: &str) -> Result<()> {
        self.call("PUT", "/transformations/default-prompt", &[], Some(json!({ "transformation_instructions": text })))
    }

    pub fn create_insight(&self, source_id: &str, transformation_id: &str) -> Result<()> {
        self.call("POST", &format!("/sources/{}/insights", seg(source_id)), &[], Some(json!({ "transformation_id": transformation_id })))
    }

    pub fn delete_insight(&self, id: &str) -> Result<()> {
        self.delete(&format!("/insights/{}", seg(id)))
    }

    pub fn save_insight_as_note(&self, id: &str, notebook_id: &str) -> Result<Note> {
        self.post(&format!("/insights/{}/save-as-note", seg(id)), Some(json!({ "notebook_id": notebook_id })))
    }

    // Notes

    pub fn list_notes(&self, notebook_id: &str) -> Result<Vec<Note>> {
        self.get("/notes", &[("notebook_id", some(notebook_id))])
    }

    pub fn get_note(&self, id: &str) -> Result<Note> {
        self.get(&format!("/notes/{}", seg(id)), &[])
    }

    pub fn create_note(&self, notebook_id: &str, title: Option<&str>, content: &str, note_type: Option<&str>) -> Result<Note> {
        let mut body = json!({ "content": content, "notebook_id": notebook_id });
        if let Some(t) = title {
            body["title"] = json!(t);
        }
        if let Some(t) = note_type {
            body["note_type"] = json!(t);
        }
        self.post("/notes", Some(body))
    }

    pub fn update_note(&self, id: &str, title: Option<&str>, content: Option<&str>) -> Result<Note> {
        let mut body = Map::new();
        if let Some(t) = title {
            body.insert("title".into(), json!(t));
        }
        if let Some(c) = content {
            body.insert("content".into(), json!(c));
        }
        self.put(&format!("/notes/{}", seg(id)), Value::Object(body))
    }

    pub fn delete_note(&self, id: &str) -> Result<()> {
        self.delete(&format!("/notes/{}", seg(id)))
    }

    // Chat

    pub fn list_chat_sessions(&self, notebook_id: &str) -> Result<Vec<ServerChatSession>> {
        self.get("/chat/sessions", &[("notebook_id", some(notebook_id))])
    }

    pub fn get_chat_session(&self, id: &str) -> Result<ServerChatSession> {
        self.get(&format!("/chat/sessions/{}", seg(id)), &[])
    }

    pub fn create_chat_session(&self, notebook_id: &str, title: Option<&str>) -> Result<ServerChatSession> {
        self.post("/chat/sessions", Some(json!({ "notebook_id": notebook_id, "title": title })))
    }

    /// `config` is `{sources: {id: mode}, notes: {id: mode}}` with the server's mode names.
    pub fn build_context(&self, notebook_id: &str, config: Value) -> Result<BuildContextResponse> {
        self.post("/chat/context", Some(json!({ "notebook_id": notebook_id, "context_config": config })))
    }

    pub fn execute_chat(&self, session_id: &str, message: &str, context: Value, model_override: Option<&str>) -> Result<ExecuteChatResponse> {
        let mut body = json!({ "session_id": session_id, "message": message, "context": context });
        if let Some(m) = model_override {
            body["model_override"] = json!(m);
        }
        self.post("/chat/execute", Some(body))
    }

    pub fn delete_chat_session(&self, id: &str) -> Result<()> {
        self.delete(&format!("/chat/sessions/{}", seg(id)))
    }

    pub fn rename_chat_session(&self, id: &str, title: &str) -> Result<()> {
        self.call("PUT", &format!("/chat/sessions/{}", seg(id)), &[], Some(json!({ "title": title })))
    }

    // Source chat

    fn source_session_path(source_id: &str, session_id: &str) -> String {
        format!("/sources/{}/chat/sessions/{}", seg(source_id), seg(session_id))
    }

    pub fn list_source_chat_sessions(&self, source_id: &str) -> Result<Vec<ServerChatSession>> {
        self.get(&format!("/sources/{}/chat/sessions", seg(source_id)), &[])
    }

    pub fn create_source_chat_session(&self, source_id: &str, title: Option<&str>) -> Result<ServerChatSession> {
        self.post(&format!("/sources/{}/chat/sessions", seg(source_id)), Some(json!({ "source_id": source_id, "title": title })))
    }

    pub fn get_source_chat_session(&self, source_id: &str, session_id: &str) -> Result<ServerChatSession> {
        self.get(&Self::source_session_path(source_id, session_id), &[])
    }

    pub fn rename_source_chat_session(&self, source_id: &str, session_id: &str, title: &str) -> Result<()> {
        self.call("PUT", &Self::source_session_path(source_id, session_id), &[], Some(json!({ "title": title })))
    }

    pub fn delete_source_chat_session(&self, source_id: &str, session_id: &str) -> Result<()> {
        self.delete(&Self::source_session_path(source_id, session_id))
    }

    /// The server answers as server-sent events. Each `ai_message` chunk goes to
    /// `on_chunk` as it arrives; the whole reply is returned. Stops with
    /// `Error::Cancelled` between events once `cancel` is set.
    pub fn send_source_chat(
        &self,
        source_id: &str,
        session_id: &str,
        message: &str,
        model_override: Option<&str>,
        cancel: &crate::Cancel,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let mut body = json!({ "message": message });
        if let Some(m) = model_override {
            body["model_override"] = json!(m);
        }
        let path = format!("{}/messages", Self::source_session_path(source_id, session_id));
        let mut response = self.send("POST", &path, &[], Body::Json(body), None)?;
        let mut reply = String::new();
        crate::net::read_sse(&mut response.body, |data| {
            cancel.check()?;
            let event: Value = serde_json::from_str(data.trim())?;
            match event.get("type").and_then(Value::as_str) {
                Some("ai_message") => {
                    if let Some(c) = event.get("content").and_then(Value::as_str).filter(|c| !c.is_empty()) {
                        reply.push_str(c);
                        on_chunk(c);
                    }
                }
                Some("error") => {
                    let message = event.get("message").and_then(Value::as_str).unwrap_or("The source chat failed");
                    return Err(Error::Server { status: 500, message: message.to_string() });
                }
                _ => {}
            }
            Ok(true)
        })
        .map_err(|e| match e {
            Error::Io(io) => super::client::read_error(io),
            other => other,
        })?;
        Ok(reply)
    }

    // Study decks, search, usage

    pub fn generate_study_deck(&self, notebook_id: &str, request: &StudyRequest) -> Result<StudyDeck> {
        let body = json!({
            "kind": request.kind,
            "source_ids": request.source_ids,
            "count": request.count,
            "difficulty": request.difficulty,
            "language": request.language.as_deref().unwrap_or("en"),
            "instructions": request.instructions.as_deref().unwrap_or(""),
        });
        self.json("POST", &format!("/notebooks/{}/studio/generate", seg(notebook_id)), &[], Body::Json(body), Some(Duration::from_secs(600)))
    }

    pub fn search(&self, query: &str, notebook_id: Option<&str>, vector: bool, limit: Option<u32>) -> Result<SearchResponse> {
        let mut body = json!({
            "query": query,
            "type": if vector { "vector" } else { "text" },
            "limit": limit.unwrap_or(30),
            "search_sources": true,
            "search_notes": true,
        });
        if let Some(id) = notebook_id {
            body["notebook_id"] = json!(id);
        }
        self.post("/search", Some(body))
    }

    /// The server's Ask, one model for each step. Returns the answer.
    pub fn ask(&self, question: &str, strategy_model: &str, answer_model: &str, final_answer_model: &str, notebook_id: Option<&str>) -> Result<String> {
        let mut body = json!({
            "question": question,
            "strategy_model": strategy_model,
            "answer_model": answer_model,
            "final_answer_model": final_answer_model,
        });
        if let Some(id) = notebook_id {
            body["notebook_id"] = json!(id);
        }
        let out: Value = self.json("POST", "/search/ask/simple", &[], Body::Json(body), Some(Duration::from_secs(600)))?;
        Ok(out.get("answer").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    pub fn usage(&self, days: u32) -> Result<ServerUsage> {
        self.get("/usage", &[("days", some(days))])
    }

    // Settings, credentials, jobs, embeddings and models

    pub fn settings(&self) -> Result<Map<String, Value>> {
        self.get("/settings", &[])
    }

    pub fn update_settings(&self, data: Map<String, Value>) -> Result<Map<String, Value>> {
        self.put("/settings", Value::Object(data))
    }

    pub fn list_credentials(&self) -> Result<Vec<ServerCredential>> {
        self.get("/credentials", &[])
    }

    pub fn create_credential(&self, data: &NewCredential) -> Result<ServerCredential> {
        self.post("/credentials", Some(serde_json::to_value(data)?))
    }

    pub fn delete_credential(&self, id: &str) -> Result<()> {
        self.delete(&format!("/credentials/{}", seg(id)))
    }

    pub fn test_credential(&self, id: &str) -> Result<TestResult> {
        self.post(&format!("/credentials/{}/test", seg(id)), None)
    }

    pub fn register_credential_models(&self, id: &str) -> Result<()> {
        self.call("POST", &format!("/credentials/{}/register-models", seg(id)), &[], None)
    }

    pub fn list_jobs(&self) -> Result<Vec<ServerJob>> {
        self.get("/commands/jobs", &[("limit", some(50))])
    }

    pub fn cancel_job(&self, id: &str) -> Result<()> {
        self.delete(&format!("/commands/jobs/{}", seg(id)))
    }

    /// `all` re-embeds everything; otherwise only items that already have embeddings.
    pub fn rebuild_embeddings(&self, all: bool) -> Result<RebuildStarted> {
        let mode = if all { "all" } else { "existing" };
        self.post("/embeddings/rebuild", Some(json!({ "mode": mode, "include_sources": true, "include_notes": true, "include_insights": true })))
    }

    pub fn rebuild_status(&self, command_id: &str) -> Result<Map<String, Value>> {
        self.get(&format!("/embeddings/rebuild/{}/status", seg(command_id)), &[])
    }

    /// `kind` is language, embedding, text_to_speech or speech_to_text.
    pub fn create_model(&self, name: &str, provider: &str, kind: &str) -> Result<()> {
        self.call("POST", "/models", &[], Some(json!({ "name": name, "provider": provider, "type": kind })))
    }

    pub fn delete_model(&self, id: &str) -> Result<()> {
        self.delete(&format!("/models/{}", seg(id)))
    }

    pub fn test_model(&self, id: &str) -> Result<TestResult> {
        self.json("POST", &format!("/models/{}/test", seg(id)), &[], Body::None, Some(Duration::from_secs(60)))
    }

    /// Keys such as `default_chat_model`; a None value clears it.
    pub fn set_model_defaults(&self, data: &std::collections::BTreeMap<String, Option<String>>) -> Result<()> {
        self.call("PUT", "/models/defaults", &[], Some(serde_json::to_value(data)?))
    }

    pub fn model_providers(&self) -> Result<ModelProviders> {
        self.get("/models/providers", &[])
    }

    pub fn sync_models(&self) -> Result<()> {
        self.send("POST", "/models/sync", &[], Body::None, Some(Duration::from_secs(120))).and_then(super::client::read_text)?;
        Ok(())
    }

    pub fn list_models(&self) -> Result<Vec<ServerModel>> {
        self.get("/models", &[])
    }

    pub fn model_defaults(&self) -> Result<std::collections::BTreeMap<String, Option<String>>> {
        let map: Map<String, Value> = self.get("/models/defaults", &[])?;
        Ok(map.into_iter().map(|(k, v)| (k, v.as_str().map(str::to_string))).collect())
    }

    // Podcasts

    pub fn list_podcast_episodes(&self) -> Result<Vec<PodcastEpisode>> {
        self.get("/podcasts/episodes", &[])
    }

    pub fn get_podcast_episode(&self, id: &str) -> Result<PodcastEpisode> {
        self.get(&format!("/podcasts/episodes/{}", seg(id)), &[])
    }

    pub fn delete_podcast_episode(&self, id: &str) -> Result<()> {
        self.delete(&format!("/podcasts/episodes/{}", seg(id)))
    }

    pub fn generate_podcast(&self, body: Value) -> Result<PodcastGenerationResponse> {
        self.post("/podcasts/generate", Some(body))
    }

    pub fn episode_profiles(&self) -> Result<Vec<ServerEpisodeProfile>> {
        self.get("/episode-profiles", &[])
    }

    pub fn speaker_profiles(&self) -> Result<Vec<ServerSpeakerProfile>> {
        self.get("/speaker-profiles", &[])
    }

    /// Creates the profile when `id` is None.
    pub fn save_episode_profile(&self, body: Value, id: Option<&str>) -> Result<ServerEpisodeProfile> {
        match id {
            Some(id) => self.put(&format!("/episode-profiles/{}", seg(id)), body),
            None => self.post("/episode-profiles", Some(body)),
        }
    }

    pub fn delete_episode_profile(&self, id: &str) -> Result<()> {
        self.delete(&format!("/episode-profiles/{}", seg(id)))
    }

    /// Creates the profile when `id` is None.
    pub fn save_speaker_profile(&self, body: Value, id: Option<&str>) -> Result<ServerSpeakerProfile> {
        match id {
            Some(id) => self.put(&format!("/speaker-profiles/{}", seg(id)), body),
            None => self.post("/speaker-profiles", Some(body)),
        }
    }

    pub fn delete_speaker_profile(&self, id: &str) -> Result<()> {
        self.delete(&format!("/speaker-profiles/{}", seg(id)))
    }

    pub fn retry_podcast_episode(&self, id: &str) -> Result<()> {
        self.call("POST", &format!("/podcasts/episodes/{}/retry", seg(id)), &[], None)
    }

    pub fn languages(&self) -> Result<Vec<Language>> {
        self.get("/languages", &[])
    }

    /// A short-lived URL the audio player can stream without an auth header.
    pub fn episode_audio_url(&self, id: &str) -> Result<String> {
        let out: Value = self.post(&format!("/podcasts/episodes/{}/audio-access", seg(id)), None)?;
        let url = out.get("url").and_then(Value::as_str).unwrap_or_default();
        Ok(format!("{}{}", self.base_url(), url))
    }
}
