//! AI providers for on-device use: OpenAI (and any OpenAI-compatible server),
//! Anthropic and Gemini. Port of `packages/engine/src/providers/`.
//!
//! Every request goes through [`crate::net::Http`], so tests answer with canned
//! responses and never touch the network.

mod anthropic;
mod gemini;
mod openai;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backend::{Cancel, OnText, Result};
use crate::net::{Request, Response, SharedHttp};
use crate::Error;

pub use anthropic::DEFAULT_ANTHROPIC_MODEL;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 4] = [ProviderKind::OpenAi, ProviderKind::Anthropic, ProviderKind::Gemini, ProviderKind::OpenAiCompatible];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn parse(s: &str) -> Option<ProviderKind> {
        ProviderKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "OpenAI",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            ProviderKind::OpenAi => Some("https://api.openai.com/v1"),
            ProviderKind::Gemini => Some("https://generativelanguage.googleapis.com/v1beta"),
            ProviderKind::OpenAiCompatible => Some("https://openrouter.ai/api/v1"),
            ProviderKind::Anthropic => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// Required for openai-compatible (OpenRouter, Groq, LM Studio, Ollama...).
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

impl ProviderConfig {
    pub fn new(kind: ProviderKind) -> Self {
        ProviderConfig { kind, base_url: None, api_key: None }
    }
    pub fn with_key(kind: ProviderKind, key: &str) -> Self {
        ProviderConfig { kind, base_url: None, api_key: Some(key.into()) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurn {
    pub role: Role,
    pub content: String,
}

impl ChatTurn {
    pub fn user(content: impl Into<String>) -> Self {
        ChatTurn { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        ChatTurn { role: Role::Assistant, content: content.into() }
    }
}

pub struct ChatRequest<'a> {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatTurn>,
    pub max_tokens: Option<u32>,
    pub cancel: Cancel,
    /// Called with each text delta as it streams in.
    pub on_text: Option<OnText<'a>>,
}

impl<'a> ChatRequest<'a> {
    pub fn new(model: &str, system: &str, messages: Vec<ChatTurn>) -> Self {
        ChatRequest { model: model.into(), system: system.into(), messages, max_tokens: None, cancel: Cancel::new(), on_text: None }
    }
    pub fn cancel(mut self, cancel: &Cancel) -> Self {
        self.cancel = cancel.clone();
        self
    }
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }
    pub fn on_text(mut self, f: Option<OnText<'a>>) -> Self {
        self.on_text = f;
        self
    }

    fn emit(&mut self, delta: &str) {
        if let Some(f) = self.on_text.as_mut() {
            f(delta);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Chat,
    Embedding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub label: Option<String>,
    /// What the model can be used for, when the provider says.
    pub capabilities: Vec<Capability>,
}

pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn supports_embeddings(&self) -> bool;
    fn list_models(&self) -> Result<Vec<ModelInfo>>;
    /// Streams a reply and returns the full text.
    fn chat(&self, request: ChatRequest<'_>) -> Result<String>;
    fn embed(&self, model: &str, texts: &[String], cancel: &Cancel) -> Result<Vec<Vec<f32>>>;
}

pub fn create_provider(config: &ProviderConfig, http: SharedHttp) -> Arc<dyn Provider> {
    match config.kind {
        ProviderKind::Anthropic => Arc::new(anthropic::Anthropic::new(config, http)),
        ProviderKind::Gemini => Arc::new(gemini::Gemini::new(config, http)),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => Arc::new(openai::OpenAi::new(config, http)),
    }
}

/// Sends a request, turning transport failures and error statuses into readable errors.
pub(crate) fn send(http: &SharedHttp, kind: ProviderKind, request: Request, cancel: &Cancel) -> Result<Response> {
    cancel.check()?;
    let response = http.send(request).map_err(|e| Error::Network(e.to_string()))?;
    cancel.check()?;
    if response.ok() { Ok(response) } else { Err(provider_error(kind, response)) }
}

pub(crate) fn read_json(response: Response) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&response.bytes()?)?)
}

/// Turns a failed provider response into a readable error, like `providerError` in http.ts.
pub fn provider_error(kind: ProviderKind, response: Response) -> Error {
    let status = response.status;
    let mut message = format!("{status} {}", reason(status)).trim().to_string();
    if let Ok(text) = response.text() {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(body) => {
                let detail = [&body["error"]["message"], &body["error"], &body["message"], &body["detail"]].into_iter().find(|v| !v.is_null());
                if let Some(s) = detail.and_then(|d| d.as_str()) {
                    message = s.to_string();
                } else if let Some(s) = body.get(0).and_then(|b| b["error"]["message"].as_str()) {
                    message = s.to_string();
                }
            }
            Err(_) => {
                let t = text.trim();
                if !t.is_empty() {
                    message = t.chars().take(300).collect();
                }
            }
        }
    }
    if status == 401 || status == 403 {
        message = format!("API key rejected: {message}");
    }
    Error::Provider { provider: kind.as_str().into(), status, message }
}

/// A provider error that came without an HTTP failure (stream errors, refusals).
pub(crate) fn stream_error(kind: ProviderKind, message: impl Into<String>) -> Error {
    Error::Provider { provider: kind.as_str().into(), status: 0, message: message.into() }
}

fn reason(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        529 => "Overloaded",
        _ => "",
    }
}

/// Embedding values as f32.
pub(crate) fn vector(values: &serde_json::Value) -> Vec<f32> {
    values.as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

pub(crate) fn trim_base(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}
