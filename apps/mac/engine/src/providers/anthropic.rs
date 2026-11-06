//! Anthropic's Messages API, streamed over SSE. The TypeScript engine uses the
//! official SDK; this speaks the same wire protocol directly: the
//! `stream: true` Messages call (with the refusal-fallback beta on models
//! that take it) and the paged model list.

use std::time::Duration;

use serde_json::{Value, json};

use super::{Capability, ChatRequest, ModelInfo, Provider, ProviderConfig, ProviderKind, read_json, send, stream_error, trim_base};
use crate::backend::{Cancel, Result};
use crate::net::{Request, SharedHttp, encode_component, read_sse};

/// Models that accept the server-side refusal fallback ("default" routing).
const FALLBACK_MODELS: &[&str] = &["claude-fable-5-1", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"];
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2023-06-01";
const KIND: ProviderKind = ProviderKind::Anthropic;

pub struct Anthropic {
    base_url: String,
    api_key: String,
    http: SharedHttp,
}

impl Anthropic {
    pub fn new(config: &ProviderConfig, http: SharedHttp) -> Self {
        // Explicit, so nothing in the environment can redirect the key.
        let base = config.base_url.as_deref().filter(|u| !u.is_empty()).unwrap_or("https://api.anthropic.com");
        Anthropic { base_url: trim_base(base), api_key: config.api_key.clone().unwrap_or_default(), http }
    }

    fn request(&self, method: &str, path: &str) -> Request {
        Request::new(method, format!("{}{path}", self.base_url)).header("x-api-key", self.api_key.clone()).header("anthropic-version", API_VERSION)
    }
}

impl Provider for Anthropic {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn supports_embeddings(&self) -> bool {
        false
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let mut models = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut path = "/v1/models?limit=1000".to_string();
            if let Some(id) = &after {
                path.push_str(&format!("&after_id={}", encode_component(id)));
            }
            let body = read_json(send(&self.http, KIND, self.request("GET", &path).timeout(Duration::from_secs(60)), &Cancel::new())?)?;
            for m in body["data"].as_array().into_iter().flatten() {
                if let Some(id) = m["id"].as_str() {
                    models.push(ModelInfo { id: id.into(), label: m["display_name"].as_str().map(String::from), capabilities: vec![Capability::Chat] });
                }
            }
            match body["last_id"].as_str() {
                Some(last) if body["has_more"].as_bool() == Some(true) => after = Some(last.into()),
                _ => return Ok(models),
            }
        }
    }

    fn chat(&self, mut request: ChatRequest<'_>) -> Result<String> {
        let fallback = FALLBACK_MODELS.contains(&request.model.as_str());
        let messages: Vec<Value> = request.messages.iter().map(|m| json!({ "role": m.role.as_str(), "content": m.content })).collect();
        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(64_000),
            "system": request.system,
            "messages": messages,
            "stream": true,
        });
        let mut http_request = if fallback {
            body["fallbacks"] = json!("default");
            self.request("POST", "/v1/messages?beta=true").header("anthropic-beta", FALLBACK_BETA)
        } else {
            self.request("POST", "/v1/messages")
        };
        http_request = http_request.json(&body);
        let mut response = send(&self.http, KIND, http_request, &request.cancel)?;

        let mut text = String::new();
        let mut stop_reason = String::new();
        let cancel = request.cancel.clone();
        read_sse(&mut response.body, |data| {
            cancel.check()?;
            let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(true) };
            match event["type"].as_str().unwrap_or("") {
                "content_block_delta" if event["delta"]["type"] == "text_delta" => {
                    if let Some(t) = event["delta"]["text"].as_str().filter(|t| !t.is_empty()) {
                        text.push_str(t);
                        request.emit(t);
                    }
                }
                "message_start" => {
                    if let Some(r) = event["message"]["stop_reason"].as_str() {
                        stop_reason = r.into();
                    }
                }
                "message_delta" => {
                    if let Some(r) = event["delta"]["stop_reason"].as_str() {
                        stop_reason = r.into();
                    }
                }
                "message_stop" => return Ok(false),
                "error" => return Err(stream_error(KIND, event["error"]["message"].as_str().unwrap_or("Stream error"))),
                _ => {}
            }
            Ok(true)
        })?;
        if stop_reason == "refusal" {
            return Err(stream_error(KIND, "Claude declined to answer this request."));
        }
        if stop_reason == "max_tokens" {
            text.push_str("\n\n_(Reply cut off at the output limit.)_");
        }
        Ok(text)
    }

    fn embed(&self, _model: &str, _texts: &[String], _cancel: &Cancel) -> Result<Vec<Vec<f32>>> {
        Err(stream_error(KIND, "Anthropic has no embeddings API. Choose OpenAI, Gemini or an OpenAI-compatible provider for embeddings."))
    }
}
