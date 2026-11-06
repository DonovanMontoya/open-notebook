//! OpenAI and any server that speaks the OpenAI chat-completions protocol:
//! OpenRouter, Groq, Together, LM Studio, Ollama (/v1), vLLM and so on.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};

use super::{Capability, ChatRequest, ModelInfo, Provider, ProviderConfig, ProviderKind, read_json, send, stream_error, trim_base, vector};
use crate::backend::{Cancel, Result};
use crate::net::{Request, SharedHttp, read_sse};

const EMBED_BATCH: usize = 64;

/// Audio, image and moderation models are not chat models.
static NON_CHAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(whisper|tts|dall-e|image|moderation|audio|realtime|transcribe|search|embedding|embed)").unwrap());
static EMBED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)embed").unwrap());

pub struct OpenAi {
    kind: ProviderKind,
    base_url: String,
    api_key: Option<String>,
    http: SharedHttp,
}

impl OpenAi {
    pub fn new(config: &ProviderConfig, http: SharedHttp) -> Self {
        let base = config
            .base_url
            .as_deref()
            .filter(|u| !u.is_empty())
            .or(config.kind.default_base_url())
            .or(ProviderKind::OpenAi.default_base_url())
            .unwrap_or_default();
        OpenAi { kind: config.kind, base_url: trim_base(base), api_key: config.api_key.clone().filter(|k| !k.is_empty()), http }
    }

    fn request(&self, method: &str, path: &str) -> Request {
        let mut r = Request::new(method, format!("{}{path}", self.base_url));
        if let Some(key) = &self.api_key {
            r = r.header("Authorization", format!("Bearer {key}"));
        }
        if self.base_url.contains("openrouter.ai") {
            r = r.header("HTTP-Referer", "https://github.com/DonovanMontoya/open-notebook").header("X-Title", "Open Notebook");
        }
        r
    }
}

impl Provider for OpenAi {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn supports_embeddings(&self) -> bool {
        true
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let request = self.request("GET", "/models").timeout(Duration::from_secs(60));
        let body = read_json(send(&self.http, self.kind, request, &Cancel::new())?)?;
        let mut models: Vec<ModelInfo> = body["data"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|m| {
                        let id = m["id"].as_str()?.to_string();
                        let label = m["name"].as_str().filter(|n| *n != id).map(String::from);
                        let capabilities = if EMBED.is_match(&id) {
                            vec![Capability::Embedding]
                        } else if NON_CHAT.is_match(&id) {
                            vec![]
                        } else {
                            vec![Capability::Chat]
                        };
                        Some(ModelInfo { id, label, capabilities })
                    })
                    .collect()
            })
            .unwrap_or_default();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    fn chat(&self, mut request: ChatRequest<'_>) -> Result<String> {
        let mut messages = vec![json!({ "role": "system", "content": request.system })];
        messages.extend(request.messages.iter().map(|m| json!({ "role": m.role.as_str(), "content": m.content })));
        let body = json!({ "model": request.model, "stream": true, "messages": messages });
        let mut response = send(&self.http, self.kind, self.request("POST", "/chat/completions").json(&body), &request.cancel)?;
        let mut text = String::new();
        let cancel = request.cancel.clone();
        read_sse(&mut response.body, |data| {
            cancel.check()?;
            if data == "[DONE]" {
                return Ok(false);
            }
            let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(true) };
            if !event["error"].is_null() {
                return Err(stream_error(self.kind, event["error"]["message"].as_str().unwrap_or("Stream error")));
            }
            if let Some(delta) = event["choices"][0]["delta"]["content"].as_str().filter(|d| !d.is_empty()) {
                text.push_str(delta);
                request.emit(delta);
            }
            Ok(true)
        })?;
        Ok(text)
    }

    fn embed(&self, model: &str, texts: &[String], cancel: &Cancel) -> Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH) {
            let request = self.request("POST", "/embeddings").json(&json!({ "model": model, "input": batch })).timeout(Duration::from_secs(300));
            let body = read_json(send(&self.http, self.kind, request, cancel)?)?;
            let mut data: Vec<&Value> = body["data"].as_array().map(|d| d.iter().collect()).unwrap_or_default();
            data.sort_by_key(|d| d["index"].as_u64().unwrap_or(0));
            vectors.extend(data.into_iter().map(|d| vector(&d["embedding"])));
        }
        Ok(vectors)
    }
}
