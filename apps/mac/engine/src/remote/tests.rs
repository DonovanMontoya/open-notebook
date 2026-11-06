use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::net::{Request, Response, Transport};

type Route = Box<dyn Fn(&Request) -> std::result::Result<Response, Transport> + Send + Sync>;

/// Answers each request from `route` and records it.
struct Fake {
    calls: Mutex<Vec<Request>>,
    route: Route,
}

impl crate::net::Http for Fake {
    fn send(&self, request: Request) -> std::result::Result<Response, Transport> {
        let response = (self.route)(&request);
        self.calls.lock().unwrap().push(request);
        response
    }
}

fn fake(route: impl Fn(&Request) -> std::result::Result<Response, Transport> + Send + Sync + 'static) -> Arc<Fake> {
    Arc::new(Fake { calls: Mutex::new(Vec::new()), route: Box::new(route) })
}

fn json_ok(v: Value) -> std::result::Result<Response, Transport> {
    Ok(Response::new(200, v.to_string()))
}

/// `METHOD /path?query` with the base URL removed.
fn target(r: &Request) -> String {
    format!("{} {}", r.method, r.url.trim_start_matches("https://h"))
}

fn backend(f: &Arc<Fake>, password: Option<&str>) -> RemoteBackend {
    RemoteBackend::with_http("https://h", password, f.clone())
}

fn calls(f: &Fake) -> Vec<Request> {
    f.calls.lock().unwrap().clone()
}

fn body(r: &Request) -> Value {
    serde_json::from_slice(r.body.as_deref().unwrap_or(b"null")).unwrap()
}

fn source(id: &str, insights: u32) -> Value {
    json!({ "id": id, "title": "T", "topics": null, "asset": null, "embedded": true, "embedded_chunks": 1, "insights_count": insights, "created": "c", "updated": "u" })
}

fn note(id: &str, content: &str) -> Value {
    json!({ "id": id, "title": null, "content": content, "note_type": "human", "created": "c", "updated": "u" })
}

#[test]
fn normalize_base_url_accepts_bare_hosts_trailing_slashes_and_api() {
    assert_eq!(normalize_base_url("host.ts.net:8502/"), "https://host.ts.net:8502");
    assert_eq!(normalize_base_url(" https://host:8502/api/ "), "https://host:8502");
    assert_eq!(normalize_base_url("http://10.0.0.2:5055"), "http://10.0.0.2:5055");
    assert_eq!(normalize_base_url("HTTP://x/API"), "HTTP://x");
}

#[test]
fn requests_go_to_api_with_query_params_and_a_bearer_password() {
    let f = fake(|_| json_ok(json!([])));
    backend(&f, Some("pw")).list_notes("notebook:abc").unwrap();
    let c = &calls(&f)[0];
    assert_eq!(c.url, "https://h/api/notes?notebook_id=notebook%3Aabc");
    assert_eq!(c.header_value("authorization"), Some("Bearer pw"));
    assert_eq!(c.header_value("accept"), Some("application/json"));
    assert_eq!(c.header_value("content-type"), None, "no body, no content type");
    assert_eq!(c.timeout, Some(DEFAULT_TIMEOUT));
}

#[test]
fn no_password_sends_no_authorization_header() {
    let f = fake(|_| json_ok(json!([])));
    backend(&f, Some("  ")).list_notebooks().unwrap();
    let c = &calls(&f)[0];
    assert_eq!(c.header_value("authorization"), None);
    assert_eq!(target(c), "GET /api/notebooks?order_by=updated+desc");
}

#[test]
fn new_sources_are_linked_to_the_notebook_and_processed_in_the_background() {
    let f = fake(|_| json_ok(source("source:1", 0)));
    let b = backend(&f, None);
    b.add_source("notebook:1", NewSource { input: SourceInput::Link { url: "https://example.com".into(), title: None }, transformations: vec![] }).unwrap();
    let c = &calls(&f)[0];
    assert_eq!(target(c), "POST /api/sources/json");
    assert_eq!(c.header_value("content-type"), Some("application/json"));
    assert_eq!(body(c), json!({ "type": "link", "url": "https://example.com", "notebooks": ["notebook:1"], "embed": true, "async_processing": true }));
}

#[test]
fn a_document_without_a_file_is_sent_as_text() {
    let f = fake(|_| json_ok(source("source:1", 0)));
    let input = SourceInput::Document { title: "Notes".into(), content: "Hello".into(), file_name: None, file: None, mime: None };
    backend(&f, None).add_source("notebook:1", NewSource { input, transformations: vec!["transformation:a".into()] }).unwrap();
    assert_eq!(
        body(&calls(&f)[0]),
        json!({ "type": "text", "content": "Hello", "title": "Notes", "transformations": ["transformation:a"], "notebooks": ["notebook:1"], "embed": true, "async_processing": true })
    );
}

#[test]
fn files_are_uploaded_as_multipart_for_the_server_to_read() {
    let f = fake(|_| json_ok(source("source:9", 0)));
    let input = SourceInput::Document {
        title: "Paper".into(),
        content: String::new(),
        file_name: Some("paper.pdf".into()),
        file: Some(b"%PDF-1.4 data".to_vec()),
        mime: Some("application/pdf".into()),
    };
    let s = backend(&f, Some("pw")).add_source("notebook:1", NewSource { input, transformations: vec!["t:1".into()] }).unwrap();
    assert_eq!(s.id, "source:9");
    let c = &calls(&f)[0];
    assert_eq!(target(c), "POST /api/sources");
    let ty = c.header_value("content-type").unwrap();
    let boundary = ty.strip_prefix("multipart/form-data; boundary=").expect("multipart");
    let text = c.body_text();
    let part = |name: &str, value: &str| format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n");
    for (name, value) in [
        ("type", "upload"),
        ("notebooks", "[\"notebook:1\"]"),
        ("title", "Paper"),
        ("transformations", "[\"t:1\"]"),
        ("embed", "true"),
        ("async_processing", "true"),
    ] {
        assert!(text.contains(&part(name, value)), "missing {name} in {text}");
    }
    assert!(text.contains("name=\"file\"; filename=\"paper.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF-1.4 data\r\n"));
    // Field order follows the web app's FormData.
    assert!(text.find("name=\"type\"").unwrap() < text.find("name=\"file\"").unwrap());
    assert!(text.find("name=\"file\"").unwrap() < text.find("name=\"embed\"").unwrap());
    assert_eq!(c.header_value("authorization"), Some("Bearer pw"));
}

#[test]
fn api_errors_surface_the_fastapi_detail_message() {
    let f = fake(|_| Ok(Response::new(404, json!({ "detail": "Notebook not found" }).to_string())));
    match backend(&f, None).get_notebook("x") {
        Err(Error::Server { status, message }) => {
            assert_eq!(status, 404);
            assert_eq!(message, "Notebook not found");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn error_messages_fall_back_to_validation_errors_body_and_status() {
    assert_eq!(error_message(422, r#"{"detail":[{"msg":"field required"}]}"#), "field required");
    assert_eq!(error_message(502, "  Bad gateway  "), "Bad gateway");
    assert_eq!(error_message(500, ""), "Request failed with status 500");
    assert_eq!(error_message(500, r#"{"other":1}"#), "Request failed with status 500");
    assert_eq!(error_message(500, &"x".repeat(300)).len(), 200);
}

#[test]
fn transport_failures_name_the_server() {
    let f = fake(|_| Err(Transport::Failed("dns".into())));
    let err = backend(&f, None).list_notebooks().unwrap_err();
    assert_eq!(err.to_string(), "Network: Could not reach https://h");
    let f = fake(|_| Err(Transport::Timeout));
    let err = backend(&f, None).list_notebooks().unwrap_err();
    assert_eq!(err.to_string(), "Network: The server took too long to respond");
}

#[test]
fn audio_urls_are_absolute_so_the_player_can_stream_them() {
    let f = fake(|_| json_ok(json!({ "url": "/api/podcasts/episodes/e1/audio?media_ticket=t" })));
    let b = RemoteBackend::with_http("https://h:8502", Some("pw"), f.clone());
    assert_eq!(
        b.episode_audio("e1").unwrap(),
        EpisodeAudio::Url { url: "https://h:8502/api/podcasts/episodes/e1/audio?media_ticket=t".into(), password: Some("pw".into()) }
    );
    assert_eq!(calls(&f)[0].url, "https://h:8502/api/podcasts/episodes/e1/audio-access");
}

#[test]
fn default_context_prefers_insights_and_applies_the_selection() {
    let sources: Vec<SourceSummary> =
        [("s1", 2), ("s2", 0), ("s3", 0)].iter().map(|(id, n)| SourceSummary { id: id.to_string(), insights_count: *n, ..Default::default() }).collect();
    let notes = vec![Note { id: "n1".into(), ..Default::default() }, Note { id: "n2".into(), ..Default::default() }];
    assert_eq!(
        server_context(&sources, &notes[..1], None),
        json!({ "sources": { "s1": "insights", "s2": "full content", "s3": "full content" }, "notes": { "n1": "full content" } })
    );
    let mut sel = ContextSelection::default();
    sel.sources.insert("s1".into(), SourceContext::Full);
    sel.sources.insert("s2".into(), SourceContext::Off);
    sel.sources.insert("gone".into(), SourceContext::Full);
    sel.notes.insert("n2".into(), false);
    assert_eq!(
        server_context(&sources, &notes, Some(&sel)),
        json!({ "sources": { "s1": "full content", "s2": "not in", "s3": "full content" }, "notes": { "n1": "full content", "n2": "not in" } })
    );
}

#[test]
fn chat_creates_a_session_builds_context_and_returns_the_reply() {
    let f = fake(|r| match target(r).as_str() {
        "POST /api/chat/sessions" => json_ok(json!({ "id": "chat_session:1", "title": "t", "notebook_id": "notebook:1", "created": "c", "updated": "u" })),
        t if t.starts_with("GET /api/sources?") => json_ok(json!([source("source:a", 1)])),
        t if t.starts_with("GET /api/notes?") => json_ok(json!([note("note:n", "x")])),
        "POST /api/chat/context" => json_ok(json!({ "context": { "sources": [{ "id": "source:a" }], "notes": [] }, "token_count": 1, "char_count": 2 })),
        "POST /api/chat/execute" => json_ok(json!({ "session_id": "chat_session:1", "messages": [
            { "id": "m1", "type": "human", "content": "hi" },
            { "id": "m2", "type": "ai", "content": "Hello [source:a]" },
        ] })),
        "GET /api/chat/sessions/chat_session%3A1" => {
            json_ok(json!({ "id": "chat_session:1", "title": "t", "notebook_id": null, "created": "c", "updated": "u", "messages": [
            { "id": "m1", "type": "human", "content": "hi", "timestamp": null },
            { "id": "m2", "type": "ai", "content": "Hello [source:a]" },
        ] }))
        }
        other => panic!("unexpected {other}"),
    });
    let mut streamed = Vec::new();
    let mut on_text = |t: &str| streamed.push(t.to_string());
    let long = format!("{}\nsecond line", "word ".repeat(20));
    let out = backend(&f, None)
        .send_chat_message(SendChat {
            notebook_id: "notebook:1".into(),
            session_id: None,
            message: long,
            context: None,
            model_override: Some("model:x".into()),
            on_text: Some(&mut on_text),
            cancel: Cancel::new(),
        })
        .unwrap();
    assert_eq!(streamed, ["Hello [source:a]"]);
    assert_eq!(out.session.id, "chat_session:1");
    assert_eq!(out.session.notebook_id, "");
    assert_eq!(out.messages.len(), 2);
    assert_eq!(out.messages[1].kind, MessageKind::Ai);
    let calls = calls(&f);
    let create = calls.iter().find(|c| target(c) == "POST /api/chat/sessions").unwrap();
    let title = body(create)["title"].as_str().unwrap().to_string();
    assert_eq!(title.chars().count(), 40);
    assert!(title.ends_with("word…"), "{title}");
    let ctx = calls.iter().find(|c| target(c) == "POST /api/chat/context").unwrap();
    assert_eq!(
        body(ctx),
        json!({ "notebook_id": "notebook:1", "context_config": { "sources": { "source:a": "insights" }, "notes": { "note:n": "full content" } } })
    );
    let exec = calls.iter().find(|c| target(c) == "POST /api/chat/execute").unwrap();
    assert_eq!(
        body(exec),
        json!({ "session_id": "chat_session:1", "message": format!("{}\nsecond line", "word ".repeat(20)), "context": { "sources": [{ "id": "source:a" }], "notes": [] }, "model_override": "model:x" })
    );
}

#[test]
fn a_cancelled_chat_sends_nothing() {
    let f = fake(|_| panic!("no request expected"));
    let cancel = Cancel::new();
    cancel.cancel();
    let err = backend(&f, None)
        .send_chat_message(SendChat {
            notebook_id: "n".into(),
            session_id: Some("s".into()),
            message: "m".into(),
            context: None,
            model_override: None,
            on_text: None,
            cancel,
        })
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled));
}

#[test]
fn source_chat_streams_server_sent_events() {
    let f = fake(|r| {
        match target(r).as_str() {
        "POST /api/sources/source%3A1/chat/sessions/s1/messages" => Ok(Response::new(
            200,
            "data: {\"type\":\"user_message\",\"content\":\"q\"}\n\ndata: {\"type\":\"ai_message\",\"content\":\"Hel\"}\n\ndata: {\"type\":\"ai_message\",\"content\":\"lo\"}\n\ndata: {\"type\":\"complete\"}\n\n",
        )
        .with_header("content-type", "text/event-stream")),
        "GET /api/sources/source%3A1/chat/sessions/s1" => {
            json_ok(json!({ "id": "s1", "title": "t", "source_id": "source:1", "created": "c", "updated": "u", "messages": [{ "id": "m", "type": "ai", "content": "Hello" }] }))
        }
        other => panic!("unexpected {other}"),
    }
    });
    let mut chunks = Vec::new();
    let mut on_text = |t: &str| chunks.push(t.to_string());
    let out = backend(&f, None)
        .send_source_chat_message(SendSourceChat {
            source_id: "source:1".into(),
            session_id: Some("s1".into()),
            message: "q".into(),
            model_override: None,
            on_text: Some(&mut on_text),
            cancel: Cancel::new(),
        })
        .unwrap();
    assert_eq!(chunks, ["Hel", "lo"]);
    assert_eq!(out.messages[0].content, "Hello");
    assert_eq!(body(&calls(&f)[0]), json!({ "message": "q" }));
}

#[test]
fn source_chat_error_events_become_errors() {
    let f = fake(|_| Ok(Response::new(200, "data: {\"type\":\"error\",\"message\":\"No model\"}\n\n")));
    let err = backend(&f, None)
        .send_source_chat_message(SendSourceChat {
            source_id: "s".into(),
            session_id: Some("x".into()),
            message: "q".into(),
            model_override: None,
            on_text: None,
            cancel: Cancel::new(),
        })
        .unwrap_err();
    assert_eq!(err.to_string(), "Server: No model");
}

#[test]
fn search_maps_results_to_hits() {
    let f = fake(|_| {
        json_ok(json!({ "results": [
            { "id": "source_embedding:1", "parent_id": "source:1", "title": "S", "similarity": 0.8, "matches": ["first", "second"] },
            { "id": "note:2", "parent_id": "", "title": "N", "relevance": 3.0 },
        ], "total_count": 2, "search_type": "text" }))
    });
    let hits = backend(&f, None).search("q", Some("notebook:1")).unwrap();
    assert_eq!(hits[0], SearchHit { kind: HitKind::Source, id: "source:1".into(), title: "S".into(), snippet: "first".into(), score: 0.8, notebook_id: None });
    assert_eq!(hits[1].kind, HitKind::Note);
    assert_eq!(hits[1].id, "note:2");
    assert_eq!(hits[1].score, 3.0);
    assert_eq!(
        body(&calls(&f)[0]),
        json!({ "query": "q", "type": "text", "limit": 30, "notebook_id": "notebook:1", "search_sources": true, "search_notes": true })
    );
}

#[test]
fn ask_uses_the_default_model_and_resolves_citations() {
    let f = fake(|r| match target(r).as_str() {
        "GET /api/models/defaults" => json_ok(json!({ "default_chat_model": "model:m", "default_embedding_model": null })),
        "POST /api/search/ask/simple" => json_ok(json!({ "answer": "A [source:a] B [note:n] C [source:a] [source:gone] [other:x]", "question": "q" })),
        "GET /api/sources/source%3Aa" => json_ok(
            json!({ "id": "source:a", "title": "Paper", "embedded": true, "embedded_chunks": 0, "created": "c", "updated": "u", "notebooks": ["notebook:9"] }),
        ),
        "GET /api/notes/note%3An" => json_ok(note("note:n", "x")),
        "GET /api/sources/source%3Agone" => Ok(Response::new(404, "{}")),
        other => panic!("unexpected {other}"),
    });
    let mut steps = Vec::new();
    let out = backend(&f, None).ask("q", None, &mut |s| steps.push(s), &Cancel::new()).unwrap();
    assert_eq!(steps.len(), 1);
    assert!(matches!(&steps[0], AskStep::Final { .. }));
    assert_eq!(
        out.documents,
        vec![
            AskDocument { id: "source:a".into(), title: "Paper".into(), notebook_id: Some("notebook:9".into()) },
            AskDocument { id: "note:n".into(), title: "Note".into(), notebook_id: None },
            AskDocument { id: "source:gone".into(), title: "source:gone".into(), notebook_id: None },
        ]
    );
    let ask = calls(&f).into_iter().find(|c| c.url.ends_with("/ask/simple")).unwrap();
    assert_eq!(body(&ask), json!({ "question": "q", "strategy_model": "model:m", "answer_model": "model:m", "final_answer_model": "model:m" }));
}

#[test]
fn ask_without_a_default_model_says_what_to_set_up() {
    let f = fake(|_| json_ok(json!({ "default_chat_model": null })));
    let err = backend(&f, None).ask("q", None, &mut |_| {}, &Cancel::new()).unwrap_err();
    assert_eq!(err.to_string(), "The server has no default chat model. Set one in its Models settings.");
}

#[test]
fn cited_ids_match_the_typescript_pattern() {
    assert_eq!(cited_ids("[[source:x]] [source:a b] [note:] [note:1][source:2]"), ["source:x", "note:1", "source:2"]);
    let many: String = (0..20).map(|i| format!("[source:{i}]")).collect();
    assert_eq!(cited_ids(&many).len(), 12);
}

#[test]
fn sources_map_nulls_and_processing_errors() {
    let f = fake(|_| {
        json_ok(json!([{ "id": "source:1", "title": null, "topics": null, "asset": { "url": "https://x" }, "embedded": false, "embedded_chunks": 0,
            "insights_count": 0, "created": "c", "updated": "u", "file_available": null, "command_id": "command:1", "status": "failed",
            "processing_info": { "error": "Could not download" } }]))
    });
    let s = &backend(&f, None).list_sources("notebook:1").unwrap()[0];
    assert_eq!(s.status.as_deref(), Some("failed"));
    assert_eq!(s.error.as_deref(), Some("Could not download"));
    assert_eq!(s.asset.as_ref().unwrap().url.as_deref(), Some("https://x"));
    assert!(s.topics.is_empty() && !s.file_available);
    assert_eq!(target(&calls(&f)[0]), "GET /api/sources?notebook_id=notebook%3A1&sort_by=updated&sort_order=desc&limit=100");
}

#[test]
fn downloads_read_the_file_name_and_treat_404_as_none() {
    let f = fake(|r| {
        if r.url.contains("missing") {
            return Ok(Response::new(404, r#"{"detail":"No file"}"#));
        }
        Ok(Response::new(200, b"bytes".to_vec())
            .with_header("Content-Disposition", "attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf")
            .with_header("Content-Type", "application/pdf"))
    });
    let b = backend(&f, None);
    let file = b.download_source("source:1").unwrap().unwrap();
    assert_eq!((file.name.as_str(), file.mime.as_str(), file.data.as_slice()), ("résumé.pdf", "application/pdf", b"bytes".as_slice()));
    assert_eq!(b.download_source("source:missing").unwrap(), None);
    assert_eq!(client::disposition_name("attachment; filename=\"a b.txt\""), Some("a b.txt".into()));
    assert_eq!(client::disposition_name("inline"), None);
}

#[test]
fn episodes_map_server_status_transcript_and_voice() {
    let f = fake(|_| {
        json_ok(json!([
            { "id": "episode:1", "name": "E", "briefing": "b", "audio_file": "/x.mp3", "job_status": null, "episode_profile": {}, "speaker_profile": { "name": "two_hosts-casual" },
              "transcript": { "transcript": [{ "speaker": "A", "dialogue": "  Hi  " }, { "speaker": "B", "dialogue": " " }] } },
            { "id": "episode:2", "name": "F", "briefing": "b", "job_status": "processing", "error_message": "", "episode_profile": {}, "speaker_profile": {} },
            { "id": "episode:3", "name": "G", "briefing": "b", "job_status": "error", "error_message": "TTS failed", "episode_profile": {}, "speaker_profile": {} },
        ]))
    });
    let eps = backend(&f, None).list_episodes("ignored").unwrap();
    assert_eq!(eps[0].status, EpisodeStatus::Completed);
    assert_eq!(eps[0].transcript, vec![EpisodeLine { speaker: "A".into(), text: "Hi".into(), start: None, end: None }]);
    assert_eq!(eps[0].voice_label.as_deref(), Some("Two hosts casual"));
    assert_eq!((eps[1].status, eps[1].error.clone()), (EpisodeStatus::Voicing, None));
    assert_eq!((eps[2].status, eps[2].error.as_deref()), (EpisodeStatus::Failed, Some("TTS failed")));
}

#[test]
fn create_episode_generates_by_profile_name() {
    let f = fake(|r| match target(r).as_str() {
        "GET /api/episode-profiles" => {
            json_ok(json!([{ "id": "ep:1", "name": "tech", "description": "", "speaker_config": "sp:1", "default_briefing": "b", "num_segments": 3 }]))
        }
        "GET /api/speaker-profiles" => json_ok(json!([{ "id": "sp:1", "name": "duo", "description": "", "speakers": [] }])),
        "GET /api/notebooks/notebook%3A1" => json_ok(
            json!({ "id": "notebook:1", "name": "Rust", "description": "", "archived": false, "created": "c", "updated": "u", "source_count": 0, "note_count": 0 }),
        ),
        "POST /api/podcasts/generate" => {
            json_ok(json!({ "job_id": "command:7", "status": "submitted", "message": "", "episode_profile": "tech", "episode_name": "Rust episode" }))
        }
        other => panic!("unexpected {other}"),
    });
    let e = backend(&f, None)
        .create_episode(CreateEpisode {
            notebook_id: "notebook:1".into(),
            episode_profile: "ep:1".into(),
            speaker_profile: None,
            focus: Some("  ".into()),
            name: None,
            effort: None,
        })
        .unwrap();
    assert_eq!((e.id.as_str(), e.name.as_str(), e.status), ("job:command:7", "Rust episode", EpisodeStatus::Queued));
    assert!(e.created.ends_with('Z') && e.created.len() == 24, "{}", e.created);
    let generate = calls(&f).into_iter().find(|c| c.url.ends_with("/generate")).unwrap();
    assert_eq!(
        body(&generate),
        json!({ "episode_profile": "tech", "speaker_profile": "duo", "episode_name": "Rust episode", "notebook_id": "notebook:1", "briefing_suffix": null })
    );
}

#[test]
fn create_episode_needs_a_known_profile() {
    let f = fake(|_| json_ok(json!([])));
    let err = backend(&f, None)
        .create_episode(CreateEpisode { notebook_id: "n".into(), episode_profile: "ep:x".into(), speaker_profile: None, focus: None, name: None, effort: None })
        .unwrap_err();
    assert_eq!(err.to_string(), "Choose an episode profile.");
}

#[test]
fn editing_an_episode_profile_keeps_the_server_models() {
    let f = fake(|r| match target(r).as_str() {
        "GET /api/episode-profiles" => json_ok(json!([{ "id": "ep:1", "name": "old", "description": "", "speaker_config": "sp:1", "outline_llm": "model:o",
            "transcript_llm": "model:t", "default_briefing": "b", "num_segments": 3, "max_tokens": null }])),
        "PUT /api/episode-profiles/ep%3A1" => {
            json_ok(json!({ "id": "ep:1", "name": "new", "description": "d", "speaker_config": "sp:2", "default_briefing": "B", "num_segments": 4 }))
        }
        "POST /api/episode-profiles" => {
            json_ok(json!({ "id": "ep:2", "name": "x", "description": "", "speaker_config": "sp:2", "default_briefing": "", "num_segments": 1 }))
        }
        other => panic!("unexpected {other}"),
    });
    let b = backend(&f, None);
    let p = EpisodeProfile {
        id: "ep:1".into(),
        name: "new".into(),
        description: "d".into(),
        speaker_profile_id: "sp:2".into(),
        language: None,
        briefing: "B".into(),
        segments: 4,
    };
    assert_eq!(b.save_episode_profile(p.clone()).unwrap(), p);
    let put = calls(&f).into_iter().find(|c| c.method == "PUT").unwrap();
    assert_eq!(
        body(&put),
        json!({ "name": "new", "description": "d", "speaker_config": "sp:2", "outline_llm": "model:o", "transcript_llm": "model:t", "language": null,
            "default_briefing": "B", "num_segments": 4, "max_tokens": null })
    );
    let created = b.save_episode_profile(EpisodeProfile { id: String::new(), ..p }).unwrap();
    assert_eq!(created.id, "ep:2");
    let post = calls(&f).into_iter().find(|c| c.method == "POST").unwrap();
    assert!(body(&post).get("outline_llm").is_none(), "new profiles leave the models to the server");
}

#[test]
fn speaker_profiles_round_trip() {
    let f = fake(|r| match target(r).as_str() {
        "GET /api/episode-profiles" => json_ok(json!([])),
        "GET /api/speaker-profiles" => json_ok(json!([{ "id": "sp:1", "name": "duo", "description": "d", "voice_model": null,
            "speakers": [{ "name": "A", "voice_id": "alloy", "backstory": "b", "personality": "p" }] }])),
        "POST /api/speaker-profiles" => json_ok(json!({ "id": "sp:9", "name": "n", "description": "", "speakers": [] })),
        other => panic!("unexpected {other}"),
    });
    let b = backend(&f, None);
    let profiles = b.podcast_profiles().unwrap();
    let sp = profiles.speaker[0].clone();
    assert_eq!(sp.voice_model, "");
    assert_eq!(sp.speakers[0].voice_id, "alloy");
    let saved = b.save_speaker_profile(SpeakerProfile { id: String::new(), ..sp }).unwrap();
    assert_eq!(saved.id, "sp:9");
    let post = calls(&f).into_iter().find(|c| c.method == "POST").unwrap();
    assert_eq!(
        body(&post),
        json!({ "name": "duo", "description": "d", "voice_model": null, "speakers": [{ "name": "A", "voice_id": "alloy", "backstory": "b", "personality": "p" }] })
    );
}

#[test]
fn transformations_and_insights_map_field_names() {
    let f = fake(|r| match target(r).as_str() {
        "GET /api/transformations" => json_ok(
            json!([{ "id": "t:1", "name": "n", "title": "T", "description": "d", "prompt": "p", "apply_default": true, "created": "c", "updated": "u" }]),
        ),
        "POST /api/transformations" => json_ok(json!({ "id": "t:2", "name": "n", "title": "T", "description": "d", "prompt": "p", "apply_default": false })),
        "GET /api/sources/s/insights" => json_ok(json!([{ "id": "i:1", "source_id": "s", "insight_type": "Dense Summary", "content": "c", "created": null }])),
        "POST /api/transformations/execute" => json_ok(json!({ "output": "done" })),
        other => panic!("unexpected {other}"),
    });
    let b = backend(&f, None);
    assert!(b.list_transformations().unwrap()[0].apply_default);
    let saved = b
        .save_transformation(Transformation {
            id: String::new(),
            name: "n".into(),
            title: "T".into(),
            description: "d".into(),
            prompt: "p".into(),
            apply_default: false,
        })
        .unwrap();
    assert_eq!(saved.id, "t:2");
    let i = &b.list_insights("s").unwrap()[0];
    assert_eq!((i.kind.as_str(), i.status.as_deref()), ("Dense Summary", Some("completed")));
    assert_eq!(b.run_transformation("t:1", "text", &Cancel::new()).unwrap(), "done");
    let exec = calls(&f).into_iter().find(|c| c.url.ends_with("/execute")).unwrap();
    assert_eq!(body(&exec), json!({ "transformation_id": "t:1", "input_text": "text" }));
}

#[test]
fn study_decks_come_from_marked_notes() {
    let deck = json!({ "version": 1, "kind": "quiz", "title": "Q", "items": [{ "question": "q", "answer": "a", "options": ["1","2","3","4"], "correct_index": 0, "source_ids": ["s"] }] });
    let content = format!("{STUDY_PREFIX}{deck}");
    let damaged = format!("{STUDY_PREFIX}{{nope");
    let f = fake(move |_| json_ok(json!([note("note:1", &content), note("note:2", "plain"), note("note:3", &damaged)])));
    let decks = backend(&f, None).list_study_decks("notebook:1").unwrap();
    assert_eq!(decks.len(), 1);
    assert_eq!((decks[0].note_id.as_str(), decks[0].deck.kind), ("note:1", StudyKind::Quiz));
}

#[test]
fn notebook_updates_send_only_what_changed_and_deletes_take_exclusive_sources() {
    let f = fake(|r| {
        if r.method == "DELETE" {
            return Ok(Response::new(200, ""));
        }
        json_ok(
            json!({ "id": "notebook:1", "name": "N", "description": "", "archived": true, "created": "c", "updated": "u", "source_count": 0, "note_count": 0 }),
        )
    });
    let b = backend(&f, None);
    b.update_notebook("notebook:1", NotebookUpdate { archived: Some(true), ..Default::default() }).unwrap();
    b.delete_notebook("notebook:1").unwrap();
    let calls = calls(&f);
    assert_eq!(body(&calls[0]), json!({ "archived": true }));
    assert_eq!(target(&calls[1]), "DELETE /api/notebooks/notebook%3A1?delete_exclusive_sources=true");
}

#[test]
fn usage_maps_groups() {
    let f = fake(|_| {
        json_ok(
            json!({ "days": 7, "estimated_usd": null, "calls": 3, "groups": [{ "model": "m", "feature": "chat", "calls": 3, "input_tokens": 10, "output_tokens": 5, "audio_seconds": 0, "estimated_usd": 0.01 }] }),
        )
    });
    let u = backend(&f, None).usage(7).unwrap();
    assert_eq!((u.days, u.calls, u.estimated, u.groups[0].output_tokens), (7, 3, false, 5));
    assert_eq!(target(&calls(&f)[0]), "GET /api/usage?days=7");
}

#[test]
fn claim_checks_are_device_only_and_prompts_are_not_exposed() {
    let f = fake(|_| panic!("no request expected"));
    let b = backend(&f, None);
    assert_eq!(b.check_episode("e").unwrap_err().to_string(), "Claim checks run on episodes made on this device.");
    assert_eq!(b.episode_prompt("e").unwrap(), None);
    assert_eq!((b.kind(), b.streams_chat(), b.podcast_mode()), (BackendKind::Remote, false, PodcastMode::Server));
}

#[test]
fn per_request_timeouts_follow_the_client() {
    let f = fake(|_| json_ok(json!({ "auth_enabled": true })));
    let b = backend(&f, None);
    assert!(b.client().auth_status().unwrap().auth_enabled);
    assert_eq!(calls(&f)[0].timeout, Some(std::time::Duration::from_secs(15)));
}

#[test]
fn title_from_shortens_the_first_line() {
    assert_eq!(title_from("  short\nmore", 40), "short");
    assert_eq!(title_from("abcdef", 4), "abc…");
    assert_eq!(title_from("ab  cdef", 4), "ab…");
}
