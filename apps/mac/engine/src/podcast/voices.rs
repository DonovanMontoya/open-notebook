//! Text to speech for episodes: OpenAI speech, OpenAI Live, Gemini and
//! ElevenLabs, all asked for 24 kHz 16-bit PCM. Port of
//! `packages/engine/src/podcast/voices.ts`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};

use super::audio::{SAMPLE_RATE, base64_to_bytes, bytes_to_pcm, resample, trim};
use super::live::{LiveOptions, OpenSocket, render_live};
use crate::backend::{Cancel, Result};
use crate::net::{Request, Response, SharedHttp, encode_component};
use crate::providers::{ProviderKind, provider_error};
use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VoiceKind {
    OpenAi,
    OpenAiLive,
    Gemini,
    ElevenLabs,
}

impl VoiceKind {
    pub const ALL: [VoiceKind; 4] = [VoiceKind::OpenAiLive, VoiceKind::OpenAi, VoiceKind::Gemini, VoiceKind::ElevenLabs];

    /// The name saved in `speaker_profile.voice_kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            VoiceKind::OpenAi => "openai",
            VoiceKind::OpenAiLive => "openai-live",
            VoiceKind::Gemini => "gemini",
            VoiceKind::ElevenLabs => "elevenlabs",
        }
    }

    pub fn parse(s: &str) -> Option<VoiceKind> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            VoiceKind::OpenAi => "OpenAI speech",
            VoiceKind::OpenAiLive => "OpenAI Live",
            VoiceKind::Gemini => "Gemini",
            VoiceKind::ElevenLabs => "ElevenLabs",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            VoiceKind::OpenAi => "gpt-4o-mini-tts",
            VoiceKind::OpenAiLive => "gpt-live-1",
            VoiceKind::Gemini => "gemini-2.5-flash-preview-tts",
            VoiceKind::ElevenLabs => "eleven_multilingual_v2",
        }
    }

    /// Two contrasting voices, so a new show sounds like two people.
    pub fn default_voices(self) -> &'static [&'static str] {
        match self {
            VoiceKind::OpenAi | VoiceKind::OpenAiLive => &["marin", "cedar"],
            VoiceKind::Gemini => &["Kore", "Puck"],
            VoiceKind::ElevenLabs => &[],
        }
    }

    /// Which saved key (`provider:<name>` in `keys.json`) the voice provider uses.
    pub fn key_name(self) -> &'static str {
        match self {
            VoiceKind::OpenAi | VoiceKind::OpenAiLive => "openai",
            VoiceKind::Gemini => "gemini",
            VoiceKind::ElevenLabs => "elevenlabs",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceConfig {
    pub kind: VoiceKind,
    pub model: String,
    pub api_key: Option<String>,
    /// One voice per host, in host order.
    pub voices: Vec<String>,
    /// Spoken names for the hosts.
    pub hosts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceOption {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueLine {
    pub host: usize,
    pub speaker: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMode {
    /// One line per request, which gives exact timing.
    Line,
    /// A whole stretch of conversation with both hosts at once, which sounds
    /// more natural; line timing inside it is estimated.
    Dialogue,
}

pub trait VoiceProvider: Send + Sync {
    fn kind(&self) -> VoiceKind;
    fn mode(&self) -> VoiceMode;
    /// Requests that may run at the same time.
    fn concurrency(&self) -> usize;
    fn speak_line(&self, text: &str, voice: &str, cancel: &Cancel) -> Result<Vec<i16>>;
    fn speak_dialogue(&self, lines: &[DialogueLine], voices: &[String], cancel: &Cancel) -> Result<Vec<i16>> {
        let _ = (lines, voices, cancel);
        Err(Error::other(format!("{} does not support dialogue requests", self.kind().label())))
    }
    fn list_voices(&self) -> Result<Vec<VoiceOption>>;
    fn list_models(&self) -> Result<Vec<String>>;
}

const OPENAI_VOICES: [&str; 13] = ["alloy", "ash", "ballad", "cedar", "coral", "echo", "fable", "marin", "nova", "onyx", "sage", "shimmer", "verse"];
pub const LIVE_VOICES: [&str; 10] = ["alloy", "ash", "ballad", "coral", "echo", "sage", "shimmer", "verse", "marin", "cedar"];
const GEMINI_VOICES: [(&str, &str); 30] = [
    ("Zephyr", "bright"), ("Puck", "upbeat"), ("Charon", "informative"), ("Kore", "firm"), ("Fenrir", "excitable"),
    ("Leda", "youthful"), ("Orus", "firm"), ("Aoede", "breezy"), ("Callirrhoe", "easy-going"), ("Autonoe", "bright"),
    ("Enceladus", "breathy"), ("Iapetus", "clear"), ("Umbriel", "easy-going"), ("Algieba", "smooth"), ("Despina", "smooth"),
    ("Erinome", "clear"), ("Algenib", "gravelly"), ("Rasalgethi", "informative"), ("Laomedeia", "upbeat"), ("Achernar", "soft"),
    ("Alnilam", "firm"), ("Schedar", "even"), ("Gacrux", "mature"), ("Pulcherrima", "forward"), ("Achird", "friendly"),
    ("Zubenelgenubi", "casual"), ("Vindemiatrix", "gentle"), ("Sadachbia", "lively"), ("Sadaltager", "knowledgeable"), ("Sulafat", "warm"),
];

const HOST_STYLE: &str =
    "Speak as a warm, curious podcast host in a natural conversational rhythm. Vary pace slightly, sound engaged, never theatrical.";

/// A single voice request may take a while for a long line; this only stops one that hangs.
const TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub fn create_voice_provider(config: &VoiceConfig, http: SharedHttp, open_socket: Option<OpenSocket>) -> Arc<dyn VoiceProvider> {
    let base = Base { config: config.clone(), http };
    match config.kind {
        VoiceKind::OpenAi => Arc::new(OpenAiSpeech(base)),
        VoiceKind::OpenAiLive => Arc::new(OpenAiLive { base, open_socket }),
        VoiceKind::Gemini => Arc::new(GeminiSpeech(base)),
        VoiceKind::ElevenLabs => Arc::new(ElevenLabs(base)),
    }
}

fn capitalized(id: &str) -> VoiceOption {
    let mut chars = id.chars();
    let label = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    VoiceOption { id: id.into(), label }
}

struct Base {
    config: VoiceConfig,
    http: SharedHttp,
}

impl Base {
    fn key(&self) -> String {
        self.config.api_key.clone().unwrap_or_default()
    }

    /// Sends a request; error statuses become "<label>: <message>" errors.
    fn send(&self, request: Request, cancel: &Cancel, parse_as: ProviderKind) -> Result<Response> {
        cancel.check()?;
        let response = self.http.send(request).map_err(|e| Error::Network(e.to_string()))?;
        cancel.check()?;
        if response.ok() {
            return Ok(response);
        }
        Err(match provider_error(parse_as, response) {
            Error::Provider { status, message, .. } => Error::Provider { provider: self.config.kind.label().into(), status, message },
            other => other,
        })
    }

    fn json(&self, request: Request, parse_as: ProviderKind) -> Result<Value> {
        let response = self.send(request, &Cancel::new(), parse_as)?;
        Ok(serde_json::from_slice(&response.bytes()?)?)
    }

    fn pcm(&self, request: Request, cancel: &Cancel, parse_as: ProviderKind) -> Result<Vec<i16>> {
        let bytes = self.send(request.timeout(TIMEOUT), cancel, parse_as)?.bytes().map_err(|e| Error::Network(e.to_string()))?;
        Ok(bytes_to_pcm(&bytes))
    }

    /// OpenAI model ids matching a filter, sorted.
    fn openai_models(&self, keep: impl Fn(&str) -> bool) -> Result<Vec<String>> {
        let body = self.json(Request::get("https://api.openai.com/v1/models").header("Authorization", format!("Bearer {}", self.key())), ProviderKind::OpenAi)?;
        let mut ids: Vec<String> =
            body["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).filter(|id| keep(id)).map(String::from).collect();
        ids.sort();
        Ok(ids)
    }
}

fn contains_tts(id: &str) -> bool {
    id.to_ascii_lowercase().contains("tts")
}

struct OpenAiSpeech(Base);

impl VoiceProvider for OpenAiSpeech {
    fn kind(&self) -> VoiceKind {
        VoiceKind::OpenAi
    }
    fn mode(&self) -> VoiceMode {
        VoiceMode::Line
    }
    fn concurrency(&self) -> usize {
        3
    }
    fn speak_line(&self, text: &str, voice: &str, cancel: &Cancel) -> Result<Vec<i16>> {
        let body = json!({ "model": self.0.config.model, "voice": voice, "input": text, "response_format": "pcm", "instructions": HOST_STYLE });
        let request = Request::post("https://api.openai.com/v1/audio/speech").header("Authorization", format!("Bearer {}", self.0.key())).json(&body);
        Ok(trim(&self.0.pcm(request, cancel, ProviderKind::OpenAi)?))
    }
    fn list_voices(&self) -> Result<Vec<VoiceOption>> {
        Ok(OPENAI_VOICES.iter().map(|id| capitalized(id)).collect())
    }
    fn list_models(&self) -> Result<Vec<String>> {
        self.0.openai_models(contains_tts)
    }
}

struct OpenAiLive {
    base: Base,
    open_socket: Option<OpenSocket>,
}

impl VoiceProvider for OpenAiLive {
    fn kind(&self) -> VoiceKind {
        VoiceKind::OpenAiLive
    }
    fn mode(&self) -> VoiceMode {
        VoiceMode::Line
    }
    /// Live renders in real time; sessions run one after another, like the server does.
    fn concurrency(&self) -> usize {
        1
    }
    fn speak_line(&self, text: &str, voice: &str, cancel: &Cancel) -> Result<Vec<i16>> {
        let Some(open_socket) = self.open_socket.clone() else {
            return Err(Error::Setup("OpenAI Live needs the app’s WebSocket support".into()));
        };
        let Some(api_key) = self.base.config.api_key.clone().filter(|k| !k.is_empty()) else {
            return Err(Error::Setup("OpenAI Live needs your OpenAI API key".into()));
        };
        render_live(&LiveOptions::new(text, voice, &api_key, &self.base.config.model, open_socket, cancel.clone()))
    }
    fn list_voices(&self) -> Result<Vec<VoiceOption>> {
        Ok(LIVE_VOICES.iter().map(|id| capitalized(id)).collect())
    }
    fn list_models(&self) -> Result<Vec<String>> {
        let live = self.base.openai_models(|id| id.to_ascii_lowercase().starts_with("gpt-live"))?;
        Ok(if live.is_empty() { vec![VoiceKind::OpenAiLive.default_model().into()] } else { live })
    }
}

const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

static RATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rate=(\d+)").unwrap());

struct GeminiSpeech(Base);

impl GeminiSpeech {
    fn model(&self) -> String {
        let m = &self.0.config.model;
        if m.starts_with("models/") { m.clone() } else { format!("models/{m}") }
    }

    fn generate(&self, prompt: &str, speech_config: Value, cancel: &Cancel) -> Result<Vec<i16>> {
        let body = json!({
            "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
            "generationConfig": { "responseModalities": ["AUDIO"], "speechConfig": speech_config },
        });
        let request = Request::post(format!("{GEMINI_BASE}/{}:generateContent", self.model()))
            .header("x-goog-api-key", self.0.key())
            .json(&body)
            .timeout(TIMEOUT);
        let response = self.0.send(request, cancel, ProviderKind::Gemini)?;
        let body: Value = serde_json::from_slice(&response.bytes().map_err(|e| Error::Network(e.to_string()))?)?;
        let part = body
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .and_then(|parts| parts.iter().find(|p| p.pointer("/inlineData/data").and_then(Value::as_str).is_some_and(|d| !d.is_empty())))
            .map(|p| &p["inlineData"]);
        let Some(part) = part else { return Err(Error::other("Gemini returned no audio")) };
        let mime = part["mimeType"].as_str().unwrap_or_default();
        let rate = RATE.captures(mime).and_then(|c| c[1].parse().ok()).unwrap_or(SAMPLE_RATE);
        Ok(resample(bytes_to_pcm(&base64_to_bytes(part["data"].as_str().unwrap_or_default())), rate, SAMPLE_RATE))
    }
}

fn voice_config(name: &str) -> Value {
    json!({ "prebuiltVoiceConfig": { "voiceName": name } })
}

impl VoiceProvider for GeminiSpeech {
    fn kind(&self) -> VoiceKind {
        VoiceKind::Gemini
    }
    fn mode(&self) -> VoiceMode {
        VoiceMode::Dialogue
    }
    fn concurrency(&self) -> usize {
        2
    }
    fn speak_line(&self, text: &str, voice: &str, cancel: &Cancel) -> Result<Vec<i16>> {
        let pcm = self.generate(&format!("Read this aloud, warmly and naturally:\n{text}"), json!({ "voiceConfig": voice_config(voice) }), cancel)?;
        Ok(trim(&pcm))
    }
    fn speak_dialogue(&self, lines: &[DialogueLine], voices: &[String], cancel: &Cancel) -> Result<Vec<i16>> {
        // Speakers in order of first appearance, each with their host.
        let mut speakers: Vec<(&str, usize)> = Vec::new();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for l in lines {
            match seen.get(l.speaker.as_str()) {
                Some(&i) => speakers[i].1 = l.host,
                None => {
                    seen.insert(&l.speaker, speakers.len());
                    speakers.push((&l.speaker, l.host));
                }
            }
        }
        let voice_for = |host: usize| voices.get(host).or(voices.first()).cloned().unwrap_or_default();
        if speakers.len() < 2 {
            let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
            return self.speak_line(&text, &voice_for(lines.first().map_or(0, |l| l.host)), cancel);
        }
        let script = lines.iter().map(|l| format!("{}: {}", l.speaker, l.text)).collect::<Vec<_>>().join("\n");
        let names = speakers.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(" and ");
        let configs: Vec<Value> =
            speakers.iter().take(2).map(|(speaker, host)| json!({ "speaker": speaker, "voiceConfig": voice_config(&voice_for(*host)) })).collect();
        let pcm = self.generate(
            &format!("Perform this podcast conversation between {names}. Natural, relaxed and engaged, with real turn-taking:\n{script}"),
            json!({ "multiSpeakerVoiceConfig": { "speakerVoiceConfigs": configs } }),
            cancel,
        )?;
        Ok(trim(&pcm))
    }
    fn list_voices(&self) -> Result<Vec<VoiceOption>> {
        Ok(GEMINI_VOICES.iter().map(|(id, tone)| VoiceOption { id: (*id).into(), label: format!("{id}, {tone}") }).collect())
    }
    fn list_models(&self) -> Result<Vec<String>> {
        let body = self.0.json(Request::get(format!("{GEMINI_BASE}/models?pageSize=1000")).header("x-goog-api-key", self.0.key()), ProviderKind::Gemini)?;
        let mut ids: Vec<String> = body["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["name"].as_str())
            .map(|n| n.trim_start_matches("models/").to_string())
            .filter(|id| contains_tts(id))
            .collect();
        ids.sort();
        Ok(ids)
    }
}

struct ElevenLabs(Base);

impl VoiceProvider for ElevenLabs {
    fn kind(&self) -> VoiceKind {
        VoiceKind::ElevenLabs
    }
    fn mode(&self) -> VoiceMode {
        VoiceMode::Line
    }
    fn concurrency(&self) -> usize {
        2
    }
    fn speak_line(&self, text: &str, voice: &str, cancel: &Cancel) -> Result<Vec<i16>> {
        let request = Request::post(format!("https://api.elevenlabs.io/v1/text-to-speech/{}?output_format=pcm_24000", encode_component(voice)))
            .header("xi-api-key", self.0.key())
            .json(&json!({ "text": text, "model_id": self.0.config.model }));
        Ok(trim(&self.0.pcm(request, cancel, ProviderKind::OpenAi)?))
    }
    fn list_voices(&self) -> Result<Vec<VoiceOption>> {
        let body = self.0.json(Request::get("https://api.elevenlabs.io/v1/voices").header("xi-api-key", self.0.key()), ProviderKind::OpenAi)?;
        Ok(body["voices"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| VoiceOption { id: v["voice_id"].as_str().unwrap_or_default().into(), label: v["name"].as_str().unwrap_or_default().into() })
            .collect())
    }
    fn list_models(&self) -> Result<Vec<String>> {
        let body = self.0.json(Request::get("https://api.elevenlabs.io/v1/models").header("xi-api-key", self.0.key()), ProviderKind::OpenAi)?;
        Ok(body
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["can_do_text_to_speech"].as_bool() != Some(false))
            .filter_map(|m| m["model_id"].as_str().map(String::from))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Transport;
    use std::sync::Mutex;

    type Calls = Arc<Mutex<Vec<Request>>>;

    fn answering(body: Vec<u8>, calls: &Calls) -> SharedHttp {
        let calls = calls.clone();
        Arc::new(move |r: Request| -> std::result::Result<Response, Transport> {
            calls.lock().unwrap().push(r);
            Ok(Response::new(200, body.clone()))
        })
    }

    fn config(kind: VoiceKind, key: &str, voices: &[&str]) -> VoiceConfig {
        VoiceConfig {
            kind,
            model: kind.default_model().into(),
            api_key: Some(key.into()),
            voices: voices.iter().map(|v| v.to_string()).collect(),
            hosts: Vec::new(),
        }
    }

    const PCM: [u8; 4] = [0x10, 0x27, 0xf0, 0xd8]; // 10000, -10000

    #[test]
    fn voice_providers_ask_for_24_khz_pcm_in_their_own_request_formats() {
        let calls: Calls = Arc::default();
        let openai = create_voice_provider(&config(VoiceKind::OpenAi, "k", &["marin"]), answering(PCM.to_vec(), &calls), None);
        assert!(!openai.speak_line("Hello", "marin", &Cancel::new()).unwrap().is_empty());
        let r = calls.lock().unwrap().remove(0);
        assert_eq!(r.url, "https://api.openai.com/v1/audio/speech");
        let body: Value = serde_json::from_str(&r.body_text()).unwrap();
        assert_eq!((body["response_format"].as_str(), body["voice"].as_str()), (Some("pcm"), Some("marin")));
        assert_eq!(r.header_value("authorization"), Some("Bearer k"));

        let eleven = create_voice_provider(&config(VoiceKind::ElevenLabs, "x", &["v1"]), answering(PCM.to_vec(), &calls), None);
        eleven.speak_line("Hello", "voice123", &Cancel::new()).unwrap();
        let r = calls.lock().unwrap().remove(0);
        assert!(r.url.ends_with("text-to-speech/voice123?output_format=pcm_24000"), "{}", r.url);
        assert_eq!(r.header_value("xi-api-key"), Some("x"));

        let audio = super::super::audio::bytes_to_base64(&PCM);
        let reply = json!({ "candidates": [{ "content": { "parts": [{ "inlineData": { "mimeType": "audio/L16;codec=pcm;rate=24000", "data": audio } }] } }] });
        let gemini = create_voice_provider(&config(VoiceKind::Gemini, "g", &["Kore", "Puck"]), answering(reply.to_string().into_bytes(), &calls), None);
        let lines = [
            DialogueLine { host: 0, speaker: "Kore".into(), text: "First.".into() },
            DialogueLine { host: 1, speaker: "Puck".into(), text: "Second.".into() },
        ];
        let pcm = gemini.speak_dialogue(&lines, &["Kore".into(), "Puck".into()], &Cancel::new()).unwrap();
        assert!(!pcm.is_empty());
        let r = calls.lock().unwrap().remove(0);
        assert_eq!(r.url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-preview-tts:generateContent");
        let body: Value = serde_json::from_str(&r.body_text()).unwrap();
        assert_eq!(body["generationConfig"]["responseModalities"], json!(["AUDIO"]));
        let speakers: Vec<(String, String)> = body["generationConfig"]["speechConfig"]["multiSpeakerVoiceConfig"]["speakerVoiceConfigs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["speaker"].as_str().unwrap().into(), s["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"].as_str().unwrap().into()))
            .collect();
        assert_eq!(speakers, [("Kore".into(), "Kore".into()), ("Puck".into(), "Puck".into())]);
        assert!(body["contents"][0]["parts"][0]["text"].as_str().unwrap().contains("Kore: First.\nPuck: Second."));
    }

    #[test]
    fn gemini_audio_at_another_rate_is_resampled_to_24_khz() {
        let calls: Calls = Arc::default();
        let audio = super::super::audio::bytes_to_base64(&super::super::audio::pcm_to_bytes(&[5000; 1200]));
        let reply = json!({ "candidates": [{ "content": { "parts": [{ "inlineData": { "mimeType": "audio/L16;rate=12000", "data": audio } }] } }] });
        let gemini = create_voice_provider(&config(VoiceKind::Gemini, "g", &["Kore"]), answering(reply.to_string().into_bytes(), &calls), None);
        assert_eq!(gemini.speak_line("Hi", "Kore", &Cancel::new()).unwrap().len(), 2400);
        let empty = create_voice_provider(&config(VoiceKind::Gemini, "g", &["Kore"]), answering(b"{}".to_vec(), &calls), None);
        assert_eq!(empty.speak_line("Hi", "Kore", &Cancel::new()).unwrap_err().to_string(), "Gemini returned no audio");
    }

    #[test]
    fn voice_errors_name_the_voice_provider() {
        let http: SharedHttp = Arc::new(|_r: Request| -> std::result::Result<Response, Transport> {
            Ok(Response::new(401, r#"{"error":{"message":"Incorrect API key"}}"#))
        });
        let openai = create_voice_provider(&config(VoiceKind::OpenAi, "bad", &["marin"]), http, None);
        let err = openai.speak_line("Hi", "marin", &Cancel::new()).unwrap_err();
        assert_eq!(err.to_string(), "OpenAI speech: API key rejected: Incorrect API key");
        assert!(openai.speak_dialogue(&[], &[], &Cancel::new()).unwrap_err().to_string().contains("does not support dialogue"));
    }

    #[test]
    fn voice_and_model_lists_follow_each_provider() {
        let calls: Calls = Arc::default();
        let models = json!({ "data": [{ "id": "gpt-4o-mini-tts" }, { "id": "gpt-4o" }, { "id": "tts-1" }] });
        let openai = create_voice_provider(&config(VoiceKind::OpenAi, "k", &[]), answering(models.to_string().into_bytes(), &calls), None);
        assert_eq!(openai.list_models().unwrap(), ["gpt-4o-mini-tts", "tts-1"]);
        assert_eq!(openai.list_voices().unwrap()[0], VoiceOption { id: "alloy".into(), label: "Alloy".into() });
        let live = create_voice_provider(&config(VoiceKind::OpenAiLive, "k", &[]), answering(models.to_string().into_bytes(), &calls), None);
        assert_eq!(live.list_models().unwrap(), ["gpt-live-1"]);
        assert!(live.speak_line("Hi", "marin", &Cancel::new()).unwrap_err().to_string().contains("WebSocket"));
        assert_eq!(VoiceKind::parse("openai-live"), Some(VoiceKind::OpenAiLive));
        assert_eq!(VoiceKind::Gemini.key_name(), "gemini");
    }
}
