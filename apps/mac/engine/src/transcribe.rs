//! Speech to text for audio and video sources, with the user's own key.
//! Port of `packages/engine/src/transcribe.ts`: OpenAI (or an OpenAI-compatible
//! server) through /audio/transcriptions, Gemini through generateContent with
//! the recording inline.

use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use crate::Error;
use crate::backend::{Cancel, Result};
use crate::net::{Http, Multipart, Request, Response, Transport};

const OPENAI_LIMIT: usize = 25 * 1024 * 1024;
const GEMINI_INLINE_LIMIT: usize = 19 * 1024 * 1024;
/// Long recordings take a while; this only stops a request that hangs.
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

const MEDIA_EXTENSIONS: [&str; 12] = ["mp3", "m4a", "wav", "aac", "ogg", "flac", "webm", "mp4", "mov", "mkv", "mpeg", "mpga"];

/// Audio or video, by MIME type or file name.
pub fn is_media(mime: &str, name: &str) -> bool {
    let ty = mime.to_ascii_lowercase();
    if ty.starts_with("audio/") || ty.starts_with("video/") {
        return true;
    }
    name.rsplit_once('.').is_some_and(|(_, ext)| MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The model used when the settings name none.
pub fn default_model(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("gpt-4o-transcribe"),
        "gemini" => Some("gemini-2.5-flash"),
        "openai-compatible" => Some("whisper-1"),
        _ => None,
    }
}

/// A provider's default API root.
pub fn default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("https://api.openai.com/v1"),
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta"),
        "openai-compatible" => Some("https://openrouter.ai/api/v1"),
        _ => None,
    }
}

/// Who transcribes: "openai", "openai-compatible", "gemini" (or "anthropic", which can't).
#[derive(Clone, Copy, Debug)]
pub struct SpeechProvider<'a> {
    pub kind: &'a str,
    /// None or empty uses the provider's default.
    pub base_url: Option<&'a str>,
    pub api_key: Option<&'a str>,
}

/// A recording to transcribe.
#[derive(Clone, Copy, Debug)]
pub struct Recording<'a> {
    pub data: &'a [u8],
    pub name: &'a str,
    /// May be empty.
    pub mime: &'a str,
}

fn mb(bytes: usize) -> String {
    format!("{} MB", (bytes as f64 / 1024.0 / 1024.0).round())
}

fn provider_error(kind: &str, status: u16, message: impl Into<String>) -> Error {
    Error::Provider { provider: kind.to_string(), status, message: message.into() }
}

/// Transcribes a recording and returns the text.
pub fn transcribe(http: &dyn Http, provider: SpeechProvider<'_>, model: &str, file: Recording<'_>, cancel: &Cancel) -> Result<String> {
    let kind = provider.kind;
    if kind == "anthropic" {
        return Err(provider_error("anthropic", 0, "Anthropic cannot transcribe audio. Choose OpenAI or Gemini for speech to text."));
    }
    cancel.check()?;
    let base = provider
        .base_url
        .filter(|b| !b.is_empty())
        .or_else(|| default_base_url(kind))
        .or_else(|| default_base_url("openai"))
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();

    if kind == "gemini" {
        if file.data.len() > GEMINI_INLINE_LIMIT {
            return Err(provider_error("gemini", 0, format!("This recording is {}; Gemini accepts up to 19 MB here.", mb(file.data.len()))));
        }
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [
                    { "inlineData": { "mimeType": if file.mime.is_empty() { "audio/mpeg" } else { file.mime }, "data": base64::engine::general_purpose::STANDARD.encode(file.data) } },
                    { "text": "Transcribe this recording word for word. Start a new paragraph when the speaker or topic changes. Output only the transcript." },
                ],
            }],
        });
        let req = Request::post(format!("{base}/models/{model}:generateContent"))
            .header("x-goog-api-key", provider.api_key.unwrap_or_default())
            .json(&body)
            .timeout(TIMEOUT);
        let out = send_json(http, kind, req)?;
        cancel.check()?;
        let parts = out.pointer("/candidates/0/content/parts").and_then(Value::as_array).cloned().unwrap_or_default();
        return Ok(parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<String>().trim().to_string());
    }

    if file.data.len() > OPENAI_LIMIT {
        return Err(provider_error(kind, 0, format!("This recording is {}; OpenAI transcription accepts up to 25 MB.", mb(file.data.len()))));
    }
    let (content_type, body) = Multipart::new().file("file", file.name, file.mime, file.data).text("model", model).text("response_format", "json").finish();
    let mut req = Request::post(format!("{base}/audio/transcriptions")).header("Content-Type", content_type).body(body).timeout(TIMEOUT);
    if let Some(key) = provider.api_key.filter(|k| !k.is_empty()) {
        req = req.header("Authorization", format!("Bearer {key}"));
    }
    let out = send_json(http, kind, req)?;
    cancel.check()?;
    Ok(out.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string())
}

fn send_json(http: &dyn Http, kind: &str, req: Request) -> Result<Value> {
    let response = http.send(req).map_err(|e| match e {
        Transport::Timeout => Error::Network(format!("{kind} took too long to answer")),
        Transport::Failed(m) => Error::Network(m),
    })?;
    if !response.ok() {
        return Err(error_from(kind, response));
    }
    let text = response.text().map_err(|e| Error::Network(e.to_string()))?;
    Ok(serde_json::from_str(&text)?)
}

/// A failed provider response as a readable error, like `providerError`.
fn error_from(kind: &str, response: Response) -> Error {
    let status = response.status;
    let reason = ureq::http::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("");
    let mut message = format!("{status} {reason}").trim().to_string();
    if let Ok(text) = response.text() {
        match serde_json::from_str::<Value>(&text) {
            Ok(body) => {
                let detail = body
                    .pointer("/error/message")
                    .or_else(|| body.get("error"))
                    .or_else(|| body.get("message"))
                    .or_else(|| body.get("detail"))
                    .and_then(Value::as_str);
                if let Some(d) = detail {
                    message = d.to_string();
                } else if let Some(m) = body.pointer("/0/error/message").and_then(Value::as_str) {
                    message = m.to_string();
                }
            }
            Err(_) if !text.trim().is_empty() => message = text.trim().chars().take(300).collect(),
            Err(_) => {}
        }
    }
    if status == 401 || status == 403 {
        message = format!("API key rejected: {message}");
    }
    provider_error(kind, status, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn recording(data: &[u8]) -> Recording<'_> {
        Recording { data, name: "talk.m4a", mime: "audio/mp4" }
    }

    #[test]
    fn is_media_recognizes_audio_and_video_by_type_or_name() {
        assert!(is_media("audio/mpeg", ""));
        assert!(is_media("", "interview.M4A"));
        assert!(is_media("video/mp4", ""));
        assert!(!is_media("application/pdf", "paper.pdf"));
        assert!(!is_media("", "mp3"));
    }

    #[test]
    fn openai_sends_a_multipart_upload_with_the_key() {
        let seen = Mutex::new(None);
        let http = |r: Request| {
            *seen.lock().unwrap() = Some(r);
            Ok(Response::new(200, r#"{"text":"  Hello world. "}"#))
        };
        let p = SpeechProvider { kind: "openai", base_url: None, api_key: Some("sk-1") };
        assert_eq!(transcribe(&http, p, "gpt-4o-transcribe", recording(b"AUDIO"), &Cancel::new()).unwrap(), "Hello world.");
        let r = seen.lock().unwrap().take().unwrap();
        assert_eq!(r.url, "https://api.openai.com/v1/audio/transcriptions");
        assert_eq!(r.header_value("authorization"), Some("Bearer sk-1"));
        let body = r.body_text();
        assert!(body.contains("name=\"file\"; filename=\"talk.m4a\"\r\nContent-Type: audio/mp4\r\n\r\nAUDIO\r\n"));
        assert!(body.contains("name=\"model\"\r\n\r\ngpt-4o-transcribe\r\n"));
        assert!(body.contains("name=\"response_format\"\r\n\r\njson\r\n"));
        assert!(body.find("name=\"file\"").unwrap() < body.find("name=\"model\"").unwrap());
    }

    #[test]
    fn compatible_servers_use_their_base_url_and_may_have_no_key() {
        let seen = Mutex::new(None);
        let http = |r: Request| {
            *seen.lock().unwrap() = Some(r);
            Ok(Response::new(200, r#"{"text":"ok"}"#))
        };
        let p = SpeechProvider { kind: "openai-compatible", base_url: Some("http://localhost:1234/v1/"), api_key: None };
        transcribe(&http, p, "whisper-1", recording(b"x"), &Cancel::new()).unwrap();
        let r = seen.lock().unwrap().take().unwrap();
        assert_eq!(r.url, "http://localhost:1234/v1/audio/transcriptions");
        assert_eq!(r.header_value("authorization"), None);
    }

    #[test]
    fn gemini_sends_the_recording_inline() {
        let seen = Mutex::new(None);
        let http = |r: Request| {
            *seen.lock().unwrap() = Some(r);
            Ok(Response::new(200, r#"{"candidates":[{"content":{"parts":[{"text":"Part one. "},{"text":"Part two."}]}}]}"#))
        };
        let p = SpeechProvider { kind: "gemini", base_url: None, api_key: Some("g-key") };
        let file = Recording { data: b"hi", name: "a.mp3", mime: "" };
        assert_eq!(transcribe(&http, p, "gemini-2.5-flash", file, &Cancel::new()).unwrap(), "Part one. Part two.");
        let r = seen.lock().unwrap().take().unwrap();
        assert_eq!(r.url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent");
        assert_eq!(r.header_value("x-goog-api-key"), Some("g-key"));
        let body: Value = serde_json::from_slice(r.body.as_deref().unwrap()).unwrap();
        assert_eq!(body.pointer("/contents/0/parts/0/inlineData"), Some(&json!({ "mimeType": "audio/mpeg", "data": "aGk=" })));
    }

    #[test]
    fn size_limits_and_anthropic_are_refused_before_sending() {
        let http = |_: Request| -> std::result::Result<Response, Transport> { panic!("nothing should be sent") };
        let big = vec![0u8; OPENAI_LIMIT + 1];
        let err = transcribe(&http, SpeechProvider { kind: "openai", base_url: None, api_key: None }, "m", recording(&big), &Cancel::new()).unwrap_err();
        assert_eq!(err.to_string(), "openai: This recording is 25 MB; OpenAI transcription accepts up to 25 MB.");
        let err = transcribe(&http, SpeechProvider { kind: "gemini", base_url: None, api_key: None }, "m", recording(&big), &Cancel::new()).unwrap_err();
        assert!(err.to_string().contains("Gemini accepts up to 19 MB"));
        let err = transcribe(&http, SpeechProvider { kind: "anthropic", base_url: None, api_key: None }, "m", recording(b"x"), &Cancel::new()).unwrap_err();
        assert!(err.to_string().contains("Anthropic cannot transcribe audio"));
    }

    #[test]
    fn provider_errors_are_readable() {
        let http = |_: Request| Ok(Response::new(401, r#"{"error":{"message":"Incorrect API key"}}"#));
        let err = transcribe(&http, SpeechProvider { kind: "openai", base_url: None, api_key: Some("bad") }, "m", recording(b"x"), &Cancel::new()).unwrap_err();
        match err {
            Error::Provider { provider, status, message } => {
                assert_eq!((provider.as_str(), status), ("openai", 401));
                assert_eq!(message, "API key rejected: Incorrect API key");
            }
            other => panic!("{other:?}"),
        }
        let http = |_: Request| Ok(Response::new(500, ""));
        let err = transcribe(&http, SpeechProvider { kind: "gemini", base_url: None, api_key: None }, "m", recording(b"x"), &Cancel::new()).unwrap_err();
        assert_eq!(err.to_string(), "gemini: 500 Internal Server Error");
    }
}
