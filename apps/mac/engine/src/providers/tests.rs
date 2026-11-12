//! Port of `providers.test.ts`: every provider against canned HTTP responses.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::net::{Request, Response, Transport};

type Calls = Arc<Mutex<Vec<Request>>>;

fn fake(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> (SharedHttp, Calls) {
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let http = move |r: Request| -> std::result::Result<Response, Transport> {
        let response = handler(&r);
        seen.lock().unwrap().push(r);
        Ok(response)
    };
    (Arc::new(http), calls)
}

fn sse(events: &[String]) -> Vec<u8> {
    events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>().into_bytes()
}

fn body(r: &Request) -> Value {
    serde_json::from_str(&r.body_text()).unwrap()
}

#[test]
fn openai_compatible_chat_streams_deltas_and_sends_the_system_prompt_first() {
    let events = [
        json!({ "choices": [{ "delta": { "role": "assistant" } }] }).to_string(),
        json!({ "choices": [{ "delta": { "content": "Hel" } }] }).to_string(),
        json!({ "choices": [{ "delta": { "content": "lo" } }] }).to_string(),
        "[DONE]".to_string(),
        json!({ "choices": [{ "delta": { "content": "ignored" } }] }).to_string(),
    ];
    let (http, calls) = fake(move |_| Response::new(200, sse(&events)));
    let config = ProviderConfig { kind: ProviderKind::OpenAiCompatible, base_url: Some("https://openrouter.ai/api/v1/".into()), api_key: Some("k".into()) };
    let provider = create_provider(&config, http);
    let mut deltas = Vec::new();
    let mut on_text = |d: &str| deltas.push(d.to_string());
    let text = provider.chat(ChatRequest::new("m", "sys", vec![ChatTurn::user("hi")]).on_text(Some(&mut on_text))).unwrap();
    assert_eq!(text, "Hello");
    assert_eq!(deltas, ["Hel", "lo"]);
    let calls = calls.lock().unwrap();
    assert_eq!(calls[0].url, "https://openrouter.ai/api/v1/chat/completions");
    assert_eq!(body(&calls[0])["messages"][0], json!({ "role": "system", "content": "sys" }));
    assert_eq!(calls[0].header_value("Authorization"), Some("Bearer k"));
    assert_eq!(calls[0].header_value("X-Title"), Some("Open Notebook"));
}

#[test]
fn openai_embeddings_keep_input_order_and_batch_large_inputs() {
    let (http, calls) = fake(|r| {
        let input: Vec<String> = serde_json::from_value(body(r)["input"].clone()).unwrap();
        let mut data: Vec<Value> = input.iter().enumerate().map(|(i, t)| json!({ "index": i, "embedding": [t.len()] })).collect();
        data.reverse();
        Response::new(200, json!({ "data": data }).to_string())
    });
    let provider = create_provider(&ProviderConfig::with_key(ProviderKind::OpenAi, "k"), http);
    let texts: Vec<String> = (0..70).map(|i| "x".repeat(i + 1)).collect();
    let vectors = provider.embed("text-embedding-3-small", &texts, &Cancel::new()).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert_eq!(vectors.iter().map(|v| v[0] as usize).collect::<Vec<_>>(), texts.iter().map(String::len).collect::<Vec<_>>());
}

#[test]
fn openai_model_list_marks_embedding_and_non_chat_models() {
    let (http, _) = fake(|_| {
        Response::new(200, json!({ "data": [{ "id": "whisper-1" }, { "id": "gpt-x", "name": "GPT X" }, { "id": "text-embedding-3-small" }] }).to_string())
    });
    let models = create_provider(&ProviderConfig::with_key(ProviderKind::OpenAi, "k"), http).list_models().unwrap();
    let got: Vec<(&str, Vec<Capability>)> = models.iter().map(|m| (m.id.as_str(), m.capabilities.clone())).collect();
    assert_eq!(got, [("gpt-x", vec![Capability::Chat]), ("text-embedding-3-small", vec![Capability::Embedding]), ("whisper-1", vec![])]);
    assert_eq!(models[0].label.as_deref(), Some("GPT X"));
}

#[test]
fn gemini_chat_maps_roles_skips_thought_parts_and_reads_the_sse_stream() {
    let events = [
        json!({ "candidates": [{ "content": { "parts": [{ "text": "thinking", "thought": true }] } }] }).to_string(),
        json!({ "candidates": [{ "content": { "parts": [{ "text": "Answer" }] } }] }).to_string(),
    ];
    let (http, calls) = fake(move |_| Response::new(200, sse(&events)));
    let provider = create_provider(&ProviderConfig::with_key(ProviderKind::Gemini, "g"), http);
    let text = provider.chat(ChatRequest::new("gemini-x", "sys", vec![ChatTurn::user("a"), ChatTurn::assistant("b"), ChatTurn::user("c")])).unwrap();
    assert_eq!(text, "Answer");
    let calls = calls.lock().unwrap();
    assert!(calls[0].url.ends_with("models/gemini-x:streamGenerateContent?alt=sse"));
    let roles: Vec<Value> = body(&calls[0])["contents"].as_array().unwrap().iter().map(|c| c["role"].clone()).collect();
    assert_eq!(roles, [json!("user"), json!("model"), json!("user")]);
    assert_eq!(calls[0].header_value("x-goog-api-key"), Some("g"));
}

#[test]
fn gemini_model_list_keeps_chat_and_embedding_capabilities_across_pages() {
    let (http, calls) = fake(|r| {
        if r.url.contains("pageToken") {
            Response::new(200, json!({ "models": [{ "name": "models/embed-1", "supportedGenerationMethods": ["embedContent"] }] }).to_string())
        } else {
            Response::new(
                200,
                json!({ "models": [{ "name": "models/chat-1", "supportedGenerationMethods": ["generateContent"] }], "nextPageToken": "p/2" }).to_string(),
            )
        }
    });
    let models = create_provider(&ProviderConfig::with_key(ProviderKind::Gemini, "g"), http).list_models().unwrap();
    let got: Vec<(&str, Vec<Capability>)> = models.iter().map(|m| (m.id.as_str(), m.capabilities.clone())).collect();
    assert_eq!(got, [("chat-1", vec![Capability::Chat]), ("embed-1", vec![Capability::Embedding])]);
    assert!(calls.lock().unwrap()[1].url.ends_with("&pageToken=p%2F2"));
}

#[test]
fn gemini_embeddings_batch_by_one_hundred() {
    let (http, calls) = fake(|r| {
        let n = body(r)["requests"].as_array().unwrap().len();
        Response::new(200, json!({ "embeddings": vec![json!({ "values": [1.0, 2.0] }); n] }).to_string())
    });
    let texts: Vec<String> = (0..150).map(|i| i.to_string()).collect();
    let vectors = create_provider(&ProviderConfig::with_key(ProviderKind::Gemini, "g"), http).embed("embed-1", &texts, &Cancel::new()).unwrap();
    assert_eq!(vectors.len(), 150);
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(calls.lock().unwrap()[0].url.ends_with("/models/embed-1:batchEmbedContents"));
}

#[test]
fn provider_errors_carry_the_api_message_and_flag_rejected_keys() {
    let (http, _) = fake(|_| Response::new(401, json!({ "error": { "message": "Incorrect API key provided" } }).to_string()));
    let err = create_provider(&ProviderConfig::with_key(ProviderKind::OpenAi, "bad"), http).list_models().unwrap_err();
    assert!(err.to_string().contains("API key rejected: Incorrect API key provided"), "{err}");
    assert!(matches!(err, Error::Provider { status: 401, .. }));

    let (http, _) = fake(|_| Response::new(500, "  upstream exploded  "));
    let err = create_provider(&ProviderConfig::with_key(ProviderKind::Gemini, "g"), http).list_models().unwrap_err();
    assert!(err.to_string().ends_with("upstream exploded"), "{err}");

    let (http, _) = fake(|_| Response::new(429, ""));
    let err = create_provider(&ProviderConfig::with_key(ProviderKind::OpenAi, "k"), http).list_models().unwrap_err();
    assert!(err.to_string().ends_with("429 Too Many Requests"), "{err}");
}

#[test]
fn stream_errors_inside_the_body_are_reported() {
    let events = [json!({ "error": { "message": "context too long" } }).to_string()];
    let (http, _) = fake(move |_| Response::new(200, sse(&events)));
    let err =
        create_provider(&ProviderConfig::with_key(ProviderKind::OpenAi, "k"), http).chat(ChatRequest::new("m", "s", vec![ChatTurn::user("x")])).unwrap_err();
    assert!(err.to_string().contains("context too long"));
}

fn anthropic_stream(stop_reason: &str) -> String {
    let events = [
        json!({ "type": "message_start", "message": { "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5", "content": [], "stop_reason": null, "usage": { "input_tokens": 5, "output_tokens": 0 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hi " } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "there" } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": stop_reason, "stop_sequence": null }, "usage": { "output_tokens": 2 } }),
        json!({ "type": "message_stop" }),
    ];
    events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap())).collect()
}

#[test]
fn anthropic_chat_streams_with_the_refusal_fallback() {
    let (http, calls) = fake(|_| Response::new(200, anthropic_stream("end_turn")).with_header("content-type", "text/event-stream"));
    let provider = create_provider(&ProviderConfig::with_key(ProviderKind::Anthropic, "sk-ant-test"), http);
    let mut deltas = Vec::new();
    let mut on_text = |d: &str| deltas.push(d.to_string());
    let text = provider.chat(ChatRequest::new("claude-opus-5-5", "sys", vec![ChatTurn::user("hello")]).on_text(Some(&mut on_text))).unwrap();
    assert_eq!(text, "Hi there");
    assert_eq!(deltas, ["Hi ", "there"]);
    let calls = calls.lock().unwrap();
    assert!(calls[0].url.starts_with("https://api.anthropic.com/v1/messages"));
    assert_eq!(calls[0].header_value("x-api-key"), Some("sk-ant-test"));
    assert_eq!(calls[0].header_value("anthropic-beta"), Some("server-side-fallback-2026-07-01"));
    let sent = body(&calls[0]);
    assert_eq!(sent["fallbacks"], "default");
    assert_eq!(sent["system"], "sys");
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["max_tokens"], 64_000);
}

#[test]
fn anthropic_marks_cut_off_replies_and_reports_refusals() {
    let (http, calls) = fake(|r| Response::new(200, anthropic_stream(if body(r)["model"] == "refuse" { "refusal" } else { "max_tokens" })));
    let provider = create_provider(&ProviderConfig::with_key(ProviderKind::Anthropic, "k"), http);
    let text = provider.chat(ChatRequest::new("claude-haiku-5-5", "s", vec![ChatTurn::user("x")]).max_tokens(10)).unwrap();
    assert_eq!(text, "Hi there\n\n_(Reply cut off at the output limit.)_");
    {
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].header_value("anthropic-beta"), None, "no fallback beta for other models");
        assert_eq!(body(&calls[0])["max_tokens"], 10);
    }
    let err = provider.chat(ChatRequest::new("refuse", "s", vec![ChatTurn::user("x")])).unwrap_err();
    assert!(err.to_string().contains("declined"));
}

#[test]
fn anthropic_lists_models_across_pages_and_refuses_to_embed() {
    let (http, _) = fake(|r| {
        if r.url.contains("after_id") {
            Response::new(200, json!({ "data": [{ "id": "claude-b" }], "has_more": false }).to_string())
        } else {
            Response::new(200, json!({ "data": [{ "id": "claude-a", "display_name": "Claude A" }], "has_more": true, "last_id": "claude-a" }).to_string())
        }
    });
    let provider = create_provider(&ProviderConfig::with_key(ProviderKind::Anthropic, "k"), http);
    let ids: Vec<String> = provider.list_models().unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["claude-a", "claude-b"]);
    assert!(!provider.supports_embeddings());
    assert!(provider.embed("x", &["y".into()], &Cancel::new()).unwrap_err().to_string().contains("no embeddings API"));
}

#[test]
fn a_cancelled_request_stops_with_cancelled() {
    let (http, calls) = fake(|_| Response::new(200, ""));
    let cancel = Cancel::new();
    cancel.cancel();
    let err = create_provider(&ProviderConfig::with_key(ProviderKind::OpenAi, "k"), http).chat(ChatRequest::new("m", "s", vec![]).cancel(&cancel)).unwrap_err();
    assert!(matches!(err, Error::Cancelled));
    assert!(calls.lock().unwrap().is_empty());
}
