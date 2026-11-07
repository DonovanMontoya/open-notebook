//! Live talk: a spoken conversation with OpenAI GPT Live about a notebook. Port of
//! `packages/engine/src/live-talk.ts`, `createLiveSession` and `recordLiveTalk` in `local.ts`,
//! and the call lifecycle in `apps/desktop/src/components/LiveTalk.tsx`.
//!
//! The Tauri app talks WebRTC. Here the same session runs over GPT Live's WebSocket
//! (the one `live.rs` uses for narration): `session.start`, mic audio streamed in with
//! `session.input_audio.append`, speech back as `session.output_audio.delta`, all raw
//! 24 kHz mono PCM16. The app supplies the microphone and speaker through [`LiveAudio`].

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::audio::{SAMPLE_RATE, base64_to_bytes, bytes_to_base64, bytes_to_pcm, pcm_to_bytes};
use super::live::LiveSocket;
use crate::Error;
use crate::backend::{Cancel, Result};
use crate::chat::{ContextNote, build_chat_prompt};
use crate::local::{AiConfig, Engine};
use crate::providers::ProviderKind;
use crate::text::clip;

pub const LIVE_MODEL: &str = "gpt-live-1";
pub const LIVE_URL: &str = "wss://api.openai.com/v1/live/sessions";
pub const VOICES: [&str; 4] = ["marin", "cedar", "alloy", "coral"];
/// The longest a conversation runs before it is ended.
pub const MAX_MINUTES: u64 = 30;
/// Characters of source overview the voice model gets, shared across the sources.
pub const LIVE_OVERVIEW_CHARS: usize = 16_000;

/// What a live session knows about the notebook.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveMaterial {
    pub notebook: String,
    /// Short: source titles and summaries, for the voice model itself.
    pub overview: String,
    /// The full chat context (sources, excerpts and notes), for the model it delegates reasoning to.
    pub full: String,
}

const VOICE_ONLY: &str = "You are an AI voice study companion. Keep replies concise, explain one idea at a time, and allow interruptions. You have no access to notebook sources. Delegate reasoning questions to the backend. Never claim to have read notebook documents.";
const VOICE_ONLY_BACKEND: &str =
    "Help with the spoken conversation. You have no notebook documents or source access. Be clear about this limitation when asked about a notebook.";
const SPOKEN: &str = "This is a spoken conversation, so answer in plain speech: no Markdown, lists or brackets. When you use a source, name it in words, like \"the FTC report says\".";

/// The session object, shared by the WebRTC body and the WebSocket `session.start`.
pub fn live_session(voice: &str, backend_model: &str, material: Option<&LiveMaterial>) -> Value {
    let instructions = match material {
        Some(m) => [
            format!(
                "You are a voice study companion for the notebook \"{}\". Keep replies concise, explain one idea at a time, and allow interruptions.",
                m.notebook
            ),
            "Below is an overview of the notebook: its sources and what each covers. For anything that needs the details of a source, a quote, a comparison or careful reasoning, delegate to the backend, which has the full text of the notebook.".into(),
            SPOKEN.into(),
            m.overview.clone(),
        ]
        .join("\n\n"),
        None => VOICE_ONLY.into(),
    };
    let backend_instructions = match material {
        Some(m) => format!("{}\n\n{SPOKEN} Keep answers short enough to say aloud, a few sentences unless asked for more.", m.full),
        None => VOICE_ONLY_BACKEND.into(),
    };
    json!({
        "model": LIVE_MODEL,
        "audio": { "output": { "voice": voice } },
        "instructions": instructions,
        "delegation": {
            "type": "responses",
            "responses": { "model": backend_model, "max_output_tokens": 4096, "instructions": backend_instructions },
        },
    })
}

/// The WebRTC session request (`livePayload`), as the Tauri app and the server send it.
pub fn live_payload(offer_sdp: &str, voice: &str, backend_model: &str, material: Option<&LiveMaterial>) -> Value {
    json!({ "session": live_session(voice, backend_model, material), "transport": { "type": "webrtc", "sdp": offer_sdp } })
}

/// The first WebSocket message: the same session, with raw 24 kHz PCM in and out.
pub fn live_start(voice: &str, backend_model: &str, material: Option<&LiveMaterial>) -> Value {
    let mut session = live_session(voice, backend_model, material);
    session["audio"]["format"] = json!({ "type": "audio/pcm", "rate": SAMPLE_RATE });
    json!({ "type": "session.start", "session": session })
}

/// The server only delegates to these OpenAI families; the app does the same.
pub fn is_live_backend(model: &str) -> bool {
    ["gpt-6", "gpt-5", "gpt-4"].iter().any(|p| model.starts_with(p))
}

/// The voice model's overview: each source's title and summary (its first insight, or its
/// text) within an even share of [`LIVE_OVERVIEW_CHARS`], then the titles of notes with content.
pub fn live_overview(sources: &[(String, String)], note_titles: &[String]) -> String {
    let each = LIVE_OVERVIEW_CHARS / sources.len().max(1);
    let overview = sources.iter().map(|(title, about)| format!("Source: {title}\n{}", clip(about, each))).collect::<Vec<_>>().join("\n\n");
    let mut parts = vec![if overview.is_empty() { "The notebook has no processed sources yet.".to_string() } else { overview }];
    if !note_titles.is_empty() {
        parts.push(format!("Notes: {}", note_titles.join(", ")));
    }
    parts.join("\n\n")
}

/// What starting a call needs: the key, the model reasoning is delegated to, and the notebook.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveSetup {
    pub api_key: String,
    pub backend_model: String,
    pub material: LiveMaterial,
}

/// The OpenAI key and the first OpenAI chat or transformation model Live can delegate to.
pub fn live_backend(config: &AiConfig) -> Result<(String, String)> {
    let key = config.providers.get(&ProviderKind::OpenAi).and_then(|p| p.api_key.clone()).filter(|k| !k.trim().is_empty());
    let key = key.ok_or_else(|| Error::other("Live needs your OpenAI API key in Settings."))?;
    let model = [&config.chat, &config.transformation]
        .into_iter()
        .flatten()
        .find(|c| c.provider.kind == ProviderKind::OpenAi && is_live_backend(&c.model))
        .ok_or_else(|| Error::other("Live needs an OpenAI chat or transformation model, such as a GPT-6 model, in Settings."))?;
    Ok((key, model.model.clone()))
}

impl Engine {
    /// Checks the key and model, then gathers the notebook's material (`createLiveSession`'s checks and `liveMaterial`).
    pub fn live_setup(&self, notebook_id: &str, cancel: &Cancel) -> Result<LiveSetup> {
        let config = self.ai_config()?;
        let (api_key, backend_model) = live_backend(&config)?;
        let material = self.live_material(notebook_id, &config, cancel)?;
        Ok(LiveSetup { api_key, backend_model, material })
    }

    /// An overview for the voice model, the full chat context for the model it delegates to.
    pub fn live_material(&self, notebook_id: &str, config: &AiConfig, cancel: &Cancel) -> Result<LiveMaterial> {
        let notebook = self.get_notebook(notebook_id)?;
        let query = [notebook.name.as_str(), notebook.description.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(". ");
        let (sources, chunks) = self.context_for(notebook_id, &query, config, cancel, None)?;
        let notes: Vec<ContextNote> = self
            .list_notes(notebook_id)?
            .into_iter()
            .map(|n| ContextNote { title: n.title.unwrap_or_else(|| "Note".into()), content: n.content.unwrap_or_default() })
            .collect();
        let full = build_chat_prompt(&notebook.name, &notebook.description, &sources, &chunks, &notes);

        let mut rows = self.list_sources(notebook_id)?;
        rows.retain(|s| s.status.as_deref().is_none_or(|s| s == "completed"));
        rows.sort_by(|a, b| a.created.cmp(&b.created));
        let mut about = Vec::new();
        for r in rows {
            let insight = self.list_insights(&r.id)?.into_iter().find(|i| i.status.as_deref() == Some("completed")).map(|i| i.content);
            let text = match insight {
                Some(t) => t,
                None => self.get_source(&r.id)?.full_text,
            };
            about.push((r.title.clone().unwrap_or_else(|| "Untitled".into()), text));
        }
        let note_titles: Vec<String> = notes.iter().filter(|n| !n.content.trim().is_empty()).map(|n| n.title.clone()).collect();
        Ok(LiveMaterial { notebook: notebook.name, overview: live_overview(&about, &note_titles), full })
    }

    /// Logs a finished conversation for the Usage view.
    pub fn record_live_talk(&self, seconds: f64) {
        if seconds > 0.0 {
            self.record_usage("live_talk", LIVE_MODEL, 0, 0, seconds.round());
        }
    }
}

// The call

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiveStatus {
    #[default]
    Idle,
    Connecting,
    Connected,
    Closing,
    Ended,
    Failed,
}

impl LiveStatus {
    /// Connecting, connected or closing: a call is running.
    pub fn live(self) -> bool {
        matches!(self, LiveStatus::Connecting | LiveStatus::Connected | LiveStatus::Closing)
    }
}

/// What the app asks of a running call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCommand {
    Mute(bool),
    End,
}

/// The microphone and speaker, at 24 kHz mono PCM16.
pub trait LiveAudio {
    /// Microphone audio captured since the last call.
    fn take_input(&mut self) -> Vec<i16>;
    /// Queues speech to play after what is already queued.
    fn play(&mut self, pcm: &[i16]);
}

/// Limits of a call. Shorter in tests.
#[derive(Clone, Copy, Debug)]
pub struct LiveTiming {
    /// For `session.started` to arrive.
    pub answer: Duration,
    /// The longest conversation.
    pub max: Duration,
    /// For `session.closed` after asking to close.
    pub close: Duration,
    /// How long one wait for an event lasts, which is also how often the mic is sent.
    pub tick: Duration,
}

impl Default for LiveTiming {
    fn default() -> Self {
        LiveTiming {
            answer: Duration::from_secs(20),
            max: Duration::from_secs(MAX_MINUTES * 60),
            close: Duration::from_secs(5),
            tick: Duration::from_millis(10),
        }
    }
}

/// How a call ended.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveEnd {
    pub status: LiveStatus,
    pub error: Option<String>,
    /// Seconds to bill: the usage OpenAI reported, or the time since the session started.
    pub seconds: f64,
}

pub const NO_ANSWER: &str = "OpenAI Live did not answer in time.";
pub const DROPPED: &str = "The connection to OpenAI Live dropped.";
pub const REPORTED: &str = "OpenAI Live reported an error and ended the conversation.";

/// 100 ms of 24 kHz audio per `session.input_audio.append`, like OpenAI's examples.
const CHUNK: usize = 2400;

/// Runs one call on an open socket until it ends. `on_status` hears each state change; the
/// returned end is final. Ending first stops the mic, then asks the session to close and
/// waits for `session.closed` for `timing.close`.
pub fn run_live_talk(
    socket: &mut dyn LiveSocket,
    start: &Value,
    audio: &mut dyn LiveAudio,
    commands: &Receiver<LiveCommand>,
    on_status: &mut dyn FnMut(LiveStatus),
    timing: LiveTiming,
) -> LiveEnd {
    let begin = Instant::now();
    let mut started: Option<Instant> = None;
    let mut closing: Option<Instant> = None;
    let mut muted = false;
    let mut billed = 0.0f64;
    let mut pending: Vec<i16> = Vec::new();
    let end = |status, error: Option<&str>, started: Option<Instant>, billed: f64| LiveEnd {
        status,
        error: error.map(String::from),
        seconds: if billed > 0.0 { billed } else { started.map_or(0.0, |t| t.elapsed().as_secs_f64()) },
    };

    on_status(LiveStatus::Connecting);
    if socket.send(&start.to_string()).is_err() {
        return end(LiveStatus::Failed, Some(DROPPED), None, 0.0);
    }
    loop {
        // Commands from the card.
        let mut close_now = false;
        loop {
            match commands.try_recv() {
                Ok(LiveCommand::Mute(m)) => muted = m,
                Ok(LiveCommand::End) => close_now = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    close_now = true;
                    break;
                }
            }
        }
        if started.is_some_and(|t| t.elapsed() >= timing.max) {
            close_now = true;
        }
        if close_now && closing.is_none() {
            if started.is_none() {
                return end(LiveStatus::Ended, None, started, billed);
            }
            let _ = socket.send(&json!({ "type": "session.close" }).to_string());
            closing = Some(Instant::now());
            on_status(LiveStatus::Closing);
        }
        if started.is_none() && begin.elapsed() >= timing.answer {
            return end(LiveStatus::Failed, Some(NO_ANSWER), started, billed);
        }
        if closing.is_some_and(|t| t.elapsed() >= timing.close) {
            return end(LiveStatus::Ended, None, started, billed);
        }

        // The mic, in 100 ms appends. Muted sends silence, as a disabled WebRTC track does.
        let input = audio.take_input();
        if started.is_some() && closing.is_none() {
            if muted {
                pending.extend(std::iter::repeat_n(0, input.len()));
            } else {
                pending.extend_from_slice(&input);
            }
            while pending.len() >= CHUNK {
                let chunk: Vec<i16> = pending.drain(..CHUNK).collect();
                let message = json!({ "type": "session.input_audio.append", "audio": bytes_to_base64(&pcm_to_bytes(&chunk)) });
                if socket.send(&message.to_string()).is_err() {
                    return end(LiveStatus::Failed, Some(DROPPED), started, billed);
                }
            }
        }

        let raw = match socket.next(timing.tick) {
            Ok(Some(raw)) => raw,
            Ok(None) => continue,
            Err(_) if closing.is_some() => return end(LiveStatus::Ended, None, started, billed),
            Err(_) => return end(LiveStatus::Failed, Some(DROPPED), started, billed),
        };
        let Ok(event) = serde_json::from_str::<Value>(&raw) else { continue };
        if let Some(seconds) = event["usage"]["seconds"].as_f64() {
            billed = billed.max(seconds);
        }
        match event["type"].as_str().unwrap_or_default() {
            "session.started" if started.is_none() => {
                started = Some(Instant::now());
                on_status(LiveStatus::Connected);
            }
            "session.output_audio.delta" => audio.play(&bytes_to_pcm(&base64_to_bytes(event["delta"].as_str().unwrap_or_default()))),
            "session.closed" => return end(LiveStatus::Ended, None, started, billed),
            "error" => {
                let _ = socket.send(&json!({ "type": "session.close" }).to_string());
                return end(LiveStatus::Failed, Some(REPORTED), started, billed);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::local::{LocalBackend, LocalOptions, ModelChoice};
    use crate::providers::ProviderConfig;
    use crate::settings::{AppSettings, ModelSetting, Secrets, provider_secret};
    use crate::types::{NewSource, SourceInput};

    fn material() -> LiveMaterial {
        LiveMaterial { notebook: "Sleep".into(), overview: "Source: Spindles\nBursts in stage 2.".into(), full: "FULL CONTEXT".into() }
    }

    #[test]
    fn the_webrtc_payload_matches_the_typescript_one() {
        let p = live_payload("v=0 offer", "cedar", "gpt-6-sol", Some(&material()));
        let s = &p["session"];
        assert_eq!(s["model"], "gpt-live-1");
        assert_eq!(s["audio"]["output"]["voice"], "cedar");
        assert_eq!(p["transport"], json!({ "type": "webrtc", "sdp": "v=0 offer" }));
        let instructions = s["instructions"].as_str().unwrap();
        assert!(instructions.starts_with("You are a voice study companion for the notebook \"Sleep\"."), "{instructions}");
        assert!(instructions.contains(SPOKEN));
        assert!(instructions.ends_with("Source: Spindles\nBursts in stage 2."));
        let d = &s["delegation"];
        assert_eq!(d["type"], "responses");
        assert_eq!(d["responses"]["model"], "gpt-6-sol");
        assert_eq!(d["responses"]["max_output_tokens"], 4096);
        assert_eq!(
            d["responses"]["instructions"].as_str().unwrap(),
            format!("FULL CONTEXT\n\n{SPOKEN} Keep answers short enough to say aloud, a few sentences unless asked for more.")
        );
    }

    #[test]
    fn without_material_the_session_is_voice_only_like_the_server() {
        let p = live_payload("v=0", "marin", "gpt-5", None);
        assert_eq!(p["session"]["instructions"], VOICE_ONLY);
        assert_eq!(p["session"]["delegation"]["responses"]["instructions"], VOICE_ONLY_BACKEND);
    }

    #[test]
    fn the_websocket_start_adds_the_pcm_format_and_no_transport() {
        let start = live_start("alloy", "gpt-6-sol", Some(&material()));
        assert_eq!(start["type"], "session.start");
        assert_eq!(start["session"]["audio"], json!({ "output": { "voice": "alloy" }, "format": { "type": "audio/pcm", "rate": 24000 } }));
        assert!(start.get("transport").is_none());
        assert_eq!(start["session"]["delegation"], live_session("alloy", "gpt-6-sol", Some(&material()))["delegation"]);
    }

    #[test]
    fn live_backends_are_the_gpt_4_5_and_6_families() {
        assert!(is_live_backend("gpt-6-sol") && is_live_backend("gpt-5.5") && is_live_backend("gpt-4o"));
        assert!(!is_live_backend("o3") && !is_live_backend("claude-sonnet-4-5") && !is_live_backend("gpt-3.5-turbo"));
    }

    #[test]
    fn the_overview_shares_its_budget_across_sources_and_lists_note_titles() {
        let long = "word ".repeat(10_000);
        let sources = vec![("A".to_string(), long.clone()), ("B".to_string(), "short".to_string())];
        let o = live_overview(&sources, &["Plan".into(), "Ideas".into()]);
        let parts: Vec<&str> = o.split("\n\n").collect();
        assert!(parts[0].starts_with("Source: A\n") && parts[0].ends_with('…'));
        assert!(parts[0].chars().count() <= "Source: A\n".len() + LIVE_OVERVIEW_CHARS / 2 + 1);
        assert_eq!(parts[1], "Source: B\nshort");
        assert_eq!(parts[2], "Notes: Plan, Ideas");
        assert_eq!(live_overview(&[], &[]), "The notebook has no processed sources yet.");
    }

    fn choice(kind: ProviderKind, model: &str, key: Option<&str>) -> ModelChoice {
        ModelChoice { provider: ProviderConfig { kind, api_key: key.map(Into::into), base_url: None }, model: model.into() }
    }

    #[test]
    fn live_backend_needs_the_openai_key_and_an_openai_gpt_model() {
        let mut config = AiConfig::default();
        assert!(live_backend(&config).unwrap_err().to_string().contains("OpenAI API key"));
        config.providers.insert(ProviderKind::OpenAi, choice(ProviderKind::OpenAi, "", Some("sk-1")).provider);
        config.chat = Some(choice(ProviderKind::Anthropic, "claude-sonnet-4-5", None));
        assert!(live_backend(&config).unwrap_err().to_string().contains("OpenAI chat or transformation model"));
        config.transformation = Some(choice(ProviderKind::OpenAi, "gpt-6-luna", Some("sk-1")));
        assert_eq!(live_backend(&config).unwrap(), ("sk-1".to_string(), "gpt-6-luna".to_string()));
    }

    #[test]
    fn live_setup_reads_the_notebook_and_live_talk_is_recorded_as_usage() {
        let dir = std::env::temp_dir().join(format!("on-live-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Secrets::new(&dir).set(&provider_secret("openai"), "sk-live").unwrap();
        let settings = AppSettings { chat: Some(ModelSetting { provider: "openai".into(), model: "gpt-6-sol".into() }), ..Default::default() };
        let b = LocalBackend::open_with(&dir, LocalOptions { settings: Some(settings), ..Default::default() }).unwrap();
        let nb = b.create_notebook("Sleep and memory", "").unwrap();
        let text =
            |content: &str, title: &str| NewSource { input: SourceInput::Text { content: content.into(), title: Some(title.into()) }, transformations: vec![] };
        b.add_source(&nb.id, text("Spindles are bursts of activity in stage 2 sleep.", "Spindles")).unwrap();
        b.idle();
        b.create_note(&nb.id, Some("My plan"), "Read the review", false).unwrap();
        b.create_note(&nb.id, Some("Empty"), "  ", false).unwrap();

        let setup = b.live_setup(&nb.id, &Cancel::new()).unwrap();
        assert_eq!((setup.api_key.as_str(), setup.backend_model.as_str()), ("sk-live", "gpt-6-sol"));
        assert_eq!(setup.material.notebook, "Sleep and memory");
        assert_eq!(setup.material.overview, "Source: Spindles\nSpindles are bursts of activity in stage 2 sleep.\n\nNotes: My plan");
        assert!(setup.material.full.contains("Spindles are bursts") && setup.material.full.contains("Read the review"));

        b.record_live_talk(0.0);
        b.record_live_talk(61.6);
        let usage = b.usage(1).unwrap();
        let text = format!("{usage:?}");
        assert!(text.contains("live_talk") && text.contains("gpt-live-1"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The call, against recorded GPT Live events.

    #[derive(Default)]
    struct Wire {
        sent: Vec<Value>,
    }

    /// Plays back server events: `on_start` after session.start, `on_close` after session.close.
    /// With `drop_when_empty`, a read with nothing queued fails as a closed socket does.
    struct Replay {
        wire: Arc<Mutex<Wire>>,
        queue: VecDeque<String>,
        on_start: Vec<String>,
        on_close: Vec<String>,
        drop_when_empty: bool,
    }

    impl LiveSocket for Replay {
        fn send(&mut self, message: &str) -> Result<()> {
            let event: Value = serde_json::from_str(message).unwrap();
            match event["type"].as_str().unwrap() {
                "session.start" => self.queue.extend(self.on_start.drain(..)),
                "session.close" => self.queue.extend(self.on_close.drain(..)),
                _ => {}
            }
            self.wire.lock().unwrap().sent.push(event);
            Ok(())
        }
        fn next(&mut self, timeout: Duration) -> Result<Option<String>> {
            if let Some(m) = self.queue.pop_front() {
                return Ok(Some(m));
            }
            if self.drop_when_empty {
                return Err(Error::Network("closed".into()));
            }
            std::thread::sleep(timeout);
            Ok(None)
        }
        fn close(&mut self) {}
    }

    #[derive(Default)]
    struct FakeAudio {
        mic: VecDeque<Vec<i16>>,
        played: Vec<i16>,
    }

    impl LiveAudio for FakeAudio {
        fn take_input(&mut self) -> Vec<i16> {
            self.mic.pop_front().unwrap_or_default()
        }
        fn play(&mut self, pcm: &[i16]) {
            self.played.extend_from_slice(pcm);
        }
    }

    fn replay(on_start: &[&str], on_close: &[&str]) -> (Replay, Arc<Mutex<Wire>>) {
        let wire: Arc<Mutex<Wire>> = Arc::default();
        let r = Replay {
            wire: wire.clone(),
            queue: VecDeque::new(),
            on_start: on_start.iter().map(|s| s.to_string()).collect(),
            on_close: on_close.iter().map(|s| s.to_string()).collect(),
            drop_when_empty: false,
        };
        (r, wire)
    }

    fn fast() -> LiveTiming {
        LiveTiming { answer: Duration::from_millis(200), max: Duration::from_secs(60), close: Duration::from_millis(200), tick: Duration::from_millis(2) }
    }

    fn call(socket: &mut Replay, audio: &mut FakeAudio, timing: LiveTiming, before: impl FnOnce(&Sender<LiveCommand>)) -> (LiveEnd, Vec<LiveStatus>) {
        let (tx, rx) = channel();
        before(&tx);
        let mut seen = Vec::new();
        let end = run_live_talk(socket, &live_start("marin", "gpt-6-sol", None), audio, &rx, &mut |s| seen.push(s), timing);
        drop(tx);
        (end, seen)
    }

    const STARTED: &str = r#"{"type":"session.started","session":{"id":"sess_123","expires_at":1782518400,"model":"gpt-live-1","audio":{"output":{"voice":"marin"}},"delegation":{"type":"responses"}}}"#;
    const CLOSED: &str = r#"{"type":"session.closed","reason":"close_requested","usage":{"seconds":128}}"#;
    const ERROR: &str = r#"{"type":"error","error":{"type":"invalid_request_error","code":"invalid_audio","message":"PCM16 audio must contain an even number of bytes","param":"audio"}}"#;

    fn delta(pcm: &[i16]) -> String {
        json!({ "type": "session.output_audio.delta", "delta": bytes_to_base64(&pcm_to_bytes(pcm)), "start_ms": 0, "end_ms": 100 }).to_string()
    }

    #[test]
    fn a_call_streams_the_mic_plays_speech_and_ends_on_request() {
        let speech: Vec<i16> = (0..480).map(|i| i as i16 * 10).collect();
        let transcript = r#"{"type":"session.output_transcript.delta","delta":"Hello","start_ms":0,"end_ms":100}"#;
        let usage = r#"{"type":"session.usage.updated","usage":{"seconds":12},"context_window":{"usage_ratio":0.42}}"#;
        let (mut socket, wire) = replay(&[STARTED, &delta(&speech), transcript, "not json", usage], &[CLOSED]);
        let mut audio = FakeAudio::default();
        audio.mic.extend([vec![], vec![7; 1000], vec![7; 2000], vec![8; 2400]]);
        let (tx, rx) = channel();
        let mut seen = Vec::new();
        let ender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            tx.send(LiveCommand::End).unwrap();
            tx
        });
        let end = run_live_talk(&mut socket, &live_start("marin", "gpt-6-sol", None), &mut audio, &rx, &mut |s| seen.push(s), fast());
        drop(ender.join());

        assert_eq!(end, LiveEnd { status: LiveStatus::Ended, error: None, seconds: 128.0 }, "billed from session.closed usage");
        assert_eq!(seen, [LiveStatus::Connecting, LiveStatus::Connected, LiveStatus::Closing]);
        assert_eq!(audio.played, speech);
        let wire = wire.lock().unwrap();
        assert_eq!(wire.sent[0]["type"], "session.start");
        let appends: Vec<Vec<i16>> = wire
            .sent
            .iter()
            .filter(|e| e["type"] == "session.input_audio.append")
            .map(|e| bytes_to_pcm(&base64_to_bytes(e["audio"].as_str().unwrap())))
            .collect();
        assert_eq!(appends.len(), 2, "5400 samples make two 100 ms appends; the rest waits");
        assert!(appends.iter().all(|a| a.len() == CHUNK));
        assert_eq!(&appends[0][..1000], &[7; 1000][..]);
        assert_eq!(appends[1][2399], 8);
        assert_eq!(wire.sent.last().unwrap()["type"], "session.close");
    }

    #[test]
    fn muted_sends_silence_instead_of_the_mic() {
        let (mut socket, wire) = replay(&[STARTED], &[CLOSED]);
        let mut audio = FakeAudio::default();
        audio.mic.extend([vec![], vec![], vec![9; 2400]]);
        let (tx, rx) = channel();
        tx.send(LiveCommand::Mute(true)).unwrap();
        let ender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            tx.send(LiveCommand::End).unwrap();
            tx
        });
        let end = run_live_talk(&mut socket, &live_start("marin", "gpt-6-sol", None), &mut audio, &rx, &mut |_| {}, fast());
        drop(ender.join());
        assert_eq!(end.status, LiveStatus::Ended);
        let wire = wire.lock().unwrap();
        let append = wire.sent.iter().find(|e| e["type"] == "session.input_audio.append").expect("an append");
        assert!(bytes_to_pcm(&base64_to_bytes(append["audio"].as_str().unwrap())).iter().all(|s| *s == 0));
    }

    #[test]
    fn no_session_started_in_time_fails_with_the_typescript_message() {
        let (mut socket, wire) = replay(&[], &[]);
        let (end, seen) = call(&mut socket, &mut FakeAudio::default(), fast(), |_| {});
        assert_eq!(end, LiveEnd { status: LiveStatus::Failed, error: Some(NO_ANSWER.into()), seconds: 0.0 });
        assert_eq!(seen, [LiveStatus::Connecting]);
        assert_eq!(wire.lock().unwrap().sent.len(), 1, "only session.start; nothing to close");
    }

    #[test]
    fn an_error_event_fails_the_call() {
        let (mut socket, _) = replay(&[STARTED, ERROR], &[]);
        let (end, seen) = call(&mut socket, &mut FakeAudio::default(), fast(), |_| {});
        assert_eq!((end.status, end.error.as_deref()), (LiveStatus::Failed, Some(REPORTED)));
        assert_eq!(seen, [LiveStatus::Connecting, LiveStatus::Connected]);
    }

    #[test]
    fn a_dropped_socket_fails_the_call() {
        let (mut socket, _) = replay(&[STARTED], &[]);
        socket.drop_when_empty = true;
        let (end, _) = call(&mut socket, &mut FakeAudio::default(), fast(), |_| {});
        assert_eq!((end.status, end.error.as_deref()), (LiveStatus::Failed, Some(DROPPED)));
    }

    #[test]
    fn ending_before_the_session_starts_just_ends() {
        let (mut socket, wire) = replay(&[STARTED], &[]);
        let (end, seen) = call(&mut socket, &mut FakeAudio::default(), fast(), |tx| tx.send(LiveCommand::End).unwrap());
        assert_eq!(end, LiveEnd { status: LiveStatus::Ended, error: None, seconds: 0.0 });
        assert_eq!(seen, [LiveStatus::Connecting]);
        assert!(wire.lock().unwrap().sent.iter().all(|e| e["type"] != "session.close"));
    }

    #[test]
    fn closing_without_session_closed_ends_after_the_grace_period_and_bills_elapsed_time() {
        let (mut socket, _) = replay(&[STARTED], &[]);
        let timing = LiveTiming { max: Duration::from_millis(30), ..fast() };
        let began = Instant::now();
        let (end, seen) = call(&mut socket, &mut FakeAudio::default(), timing, |_| {});
        assert_eq!(seen, [LiveStatus::Connecting, LiveStatus::Connected, LiveStatus::Closing], "the cap closes the call");
        assert_eq!(end.status, LiveStatus::Ended);
        assert!(began.elapsed() >= Duration::from_millis(230), "waited for the close grace");
        assert!(end.seconds > 0.2 && end.seconds < 5.0, "{}", end.seconds);
    }
}
