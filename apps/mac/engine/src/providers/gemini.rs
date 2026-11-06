//! Google Gemini through the Generative Language REST API.

use std::time::Duration;

use serde_json::{Value, json};

use super::{Capability, ChatRequest, ModelInfo, Provider, ProviderConfig, ProviderKind, Role, read_json, send, stream_error, trim_base, vector};
use crate::backend::{Cancel, Result};
use crate::net::{Request, SharedHttp, encode_component, read_sse};

const EMBED_BATCH: usize = 100;
const KIND: ProviderKind = ProviderKind::Gemini;

pub struct Gemini {
    base_url: String,
    api_key: String,
    http: SharedHttp,
}

fn model_path(model: &str) -> String {
    if model.starts_with("models/") { model.to_string() } else { format!("models/{model}") }
}

impl Gemini {
    pub fn new(config: &ProviderConfig, http: SharedHttp) -> Self {
        let base = config.base_url.as_deref().filter(|u| !u.is_empty()).or(KIND.default_base_url()).unwrap_or_default();
        Gemini { base_url: trim_base(base), api_key: config.api_key.clone().unwrap_or_default(), http }
    }

    fn request(&self, method: &str, url: String) -> Request {
        Request::new(method, url).header("x-goog-api-key", self.api_key.clone())
    }
}

impl Provider for Gemini {
    fn kind(&self) -> ProviderKind {
        KIND
    }

    fn supports_embeddings(&self) -> bool {
        true
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let mut models = Vec::new();
        let mut page_token = String::new();
        loop {
            let mut url = format!("{}/models?pageSize=1000", self.base_url);
            if !page_token.is_empty() {
                url.push_str(&format!("&pageToken={}", encode_component(&page_token)));
            }
            let body = read_json(send(&self.http, KIND, self.request("GET", url).timeout(Duration::from_secs(60)), &Cancel::new())?)?;
            for m in body["models"].as_array().into_iter().flatten() {
                let Some(name) = m["name"].as_str() else { continue };
                let methods: Vec<&str> = m["supportedGenerationMethods"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let mut capabilities = Vec::new();
                if methods.contains(&"generateContent") {
                    capabilities.push(Capability::Chat);
                }
                if methods.contains(&"embedContent") || methods.contains(&"batchEmbedContents") {
                    capabilities.push(Capability::Embedding);
                }
                models.push(ModelInfo {
                    id: name.strip_prefix("models/").unwrap_or(name).to_string(),
                    label: m["displayName"].as_str().map(String::from),
                    capabilities,
                });
            }
            page_token = body["nextPageToken"].as_str().unwrap_or("").to_string();
            if page_token.is_empty() {
                return Ok(models);
            }
        }
    }

    fn chat(&self, mut request: ChatRequest<'_>) -> Result<String> {
        let contents: Vec<Value> = request
            .messages
            .iter()
            .map(|m| json!({ "role": if m.role == Role::Assistant { "model" } else { "user" }, "parts": [{ "text": m.content }] }))
            .collect();
        let mut body = json!({ "systemInstruction": { "parts": [{ "text": request.system }] }, "contents": contents });
        if let Some(max) = request.max_tokens {
            body["generationConfig"] = json!({ "maxOutputTokens": max });
        }
        let url = format!("{}/{}:streamGenerateContent?alt=sse", self.base_url, model_path(&request.model));
        let mut response = send(&self.http, KIND, self.request("POST", url).json(&body), &request.cancel)?;
        let mut text = String::new();
        let cancel = request.cancel.clone();
        read_sse(&mut response.body, |data| {
            cancel.check()?;
            let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(true) };
            if !event["error"].is_null() {
                return Err(stream_error(KIND, event["error"]["message"].as_str().unwrap_or("Stream error")));
            }
            if let Some(reason) = event["promptFeedback"]["blockReason"].as_str().filter(|r| !r.is_empty()) {
                return Err(stream_error(KIND, format!("Gemini blocked the request ({reason})")));
            }
            for part in event["candidates"][0]["content"]["parts"].as_array().into_iter().flatten() {
                if let Some(t) = part["text"].as_str().filter(|t| !t.is_empty())
                    && !part["thought"].as_bool().unwrap_or(false)
                {
                    text.push_str(t);
                    request.emit(t);
                }
            }
            Ok(true)
        })?;
        Ok(text)
    }

    fn embed(&self, model: &str, texts: &[String], cancel: &Cancel) -> Result<Vec<Vec<f32>>> {
        let path = model_path(model);
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH) {
            let requests: Vec<Value> = batch.iter().map(|t| json!({ "model": path, "content": { "parts": [{ "text": t }] } })).collect();
            let url = format!("{}/{path}:batchEmbedContents", self.base_url);
            let request = self.request("POST", url).json(&json!({ "requests": requests })).timeout(Duration::from_secs(300));
            let body = read_json(send(&self.http, KIND, request, cancel)?)?;
            vectors.extend(body["embeddings"].as_array().into_iter().flatten().map(|e| vector(&e["values"])));
        }
        Ok(vectors)
    }
}
