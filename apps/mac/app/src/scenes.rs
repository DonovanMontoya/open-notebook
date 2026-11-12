//! Named states for headless snapshots (`--snap out.png --scene notebook`). They fill the state
//! with sample data and never open a backend.

use notebook_engine::types::*;
use rune::TextEdit;

use crate::state::{AddSource, App, NoteDraft, NotebookTab, Route, SettingsForm};

type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("home", home),
    ("home-empty", home_empty),
    ("home-error", home_error),
    ("notebook", notebook),
    ("notebook-dark", notebook_dark),
    ("streaming", streaming),
    ("notes", notes),
    ("add-source", add_source),
    ("settings", settings),
    ("ask", ask),
    ("library", library),
    ("transformations", transformations),
    ("usage", usage),
    ("study", study),
    ("quiz", quiz),
    ("insights", insights),
    ("source-chat", source_chat),
    ("server", server),
    ("listen", listen),
    ("profiles", profiles),
    ("live-idle", live_idle),
    ("live-connected", live_connected),
    ("live-failed", live_failed),
    ("now-playing", now_playing),
    ("profile-editor", profile_editor),
    ("speaker-editor", speaker_editor),
    ("settings-keys", settings_keys),
    ("settings-picker", settings_picker),
    ("settings-server", settings_server),
    ("usage-price", usage_price),
    ("transformations-try", transformations_try),
    ("server-processing", server_processing),
    ("flashcards", flashcards),
    ("flashcards-done", flashcards_done),
    ("add-files", add_files),
    ("add-batch", add_batch),
    ("add-library", add_library),
    ("notebook-details", notebook_details),
    ("notebook-search", notebook_search),
    ("new-notebook", new_notebook),
    ("archived", archived),
    ("drop", dropping),
    ("reader", reader),
    ("reader-passage", reader_passage),
    ("chat-models", chat_models),
    ("chat-rename", chat_rename),
    ("chat-empty", chat_empty),
    ("notes-list", notes_list),
    ("note-read", note_read),
    ("chat", chat),
    ("sources-tab", sources_tab),
    ("fold", fold),
];

/// Whether this run renders a scene, so `init` skips opening the real backend.
pub fn requested() -> bool {
    std::env::args().any(|a| a == "--scene" || a.starts_with("--scene="))
}

const T: &str = "2026-11-05T10:00:00Z";

fn notebooks() -> Vec<Notebook> {
    let nb = |id: &str, name: &str, sources| Notebook {
        id: id.into(),
        name: name.into(),
        created: T.into(),
        updated: T.into(),
        source_count: sources,
        ..Default::default()
    };
    vec![nb("nb1", "Sleep and memory", 3), nb("nb2", "Heat pumps", 5), nb("nb3", "Reading list", 0)]
}

fn sources() -> Vec<SourceSummary> {
    let s = |id: &str, title: &str, url: Option<&str>, status: Option<&str>, insights| SourceSummary {
        id: id.into(),
        title: Some(title.into()),
        asset: url.map(|u| SourceAsset { url: Some(u.into()), file_path: None }),
        insights_count: insights,
        created: T.into(),
        updated: T.into(),
        status: status.map(Into::into),
        notebooks: vec!["nb1".into()],
        ..Default::default()
    };
    let mut failed = s("s4", "Paywalled article", Some("https://example.com/a"), Some("failed"), 0);
    failed.error = Some("Could not download the page (403)".into());
    vec![
        s("s1", "Sleep spindles and memory consolidation", None, None, 2),
        s("s2", "Why we sleep: a review of the evidence", Some("https://example.com/review"), None, 0),
        s("s3", "Lecture notes, week 4", None, Some("running"), 0),
        failed,
    ]
}

const SOURCE_TEXT: &str = "Sleep spindles are bursts of activity in the thalamocortical network during stage 2 sleep.\n\nIn several studies, spindle density after learning predicted how much of a word list people recalled the next morning. The effect was strongest for declarative memories, such as facts and word pairs.\n\nProcedural skills, like finger tapping sequences, also improved after sleep, but the link to spindles was weaker and depended on the task.";

fn open(s: &mut App) {
    s.notebooks = notebooks();
    s.open_offline("nb1");
    let o = s.open.as_mut().unwrap();
    o.notebook = Some(s.notebooks[0].clone());
    o.sources = sources();
    o.sessions = vec![ChatSession {
        id: "c1".into(),
        title: "What do spindles do?".into(),
        notebook_id: "nb1".into(),
        created: T.into(),
        updated: T.into(),
        ..Default::default()
    }];
}

fn home_empty(s: &mut App) {
    s.settings.chat = Some(notebook_engine::settings::ModelSetting { provider: "anthropic".into(), model: "sample".into() });
}

fn home_error(s: &mut App) {
    s.backend_error = Some("Could not connect to the notebook server. Check the server address in Settings.".into());
}

fn home(s: &mut App) {
    s.notebooks = notebooks();
}

fn notebook_dark(s: &mut App) {
    notebook(s);
    s.settings.appearance = "dark".into();
}

fn notebook(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.session = Some(ChatSessionWithMessages {
        session: o.sessions[0].clone(),
        messages: vec![
            ChatMessage { id: "m1".into(), kind: MessageKind::Human, content: "What do sleep spindles have to do with memory?".into(), timestamp: None },
            ChatMessage {
                id: "m2".into(),
                kind: MessageKind::Ai,
                content: "Spindle density after learning predicted how much of a word list people recalled the next morning [Sleep spindles and memory consolidation]. The effect was strongest for **declarative** memories such as facts and word pairs.\n\nThe review is more cautious: procedural skills improved after sleep too, but the link to spindles was weaker [Why we sleep: a review of the evidence].".into(),
                timestamp: None,
            },
        ],
    });
    let detail = SourceDetail { summary: o.sources[0].clone(), full_text: SOURCE_TEXT.into() };
    let passage = notebook_engine::citations::best_passage(SOURCE_TEXT, "spindle density after learning predicted recall of a word list");
    o.reading = Some((detail, passage));
}

fn streaming(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.pending = Some(("Summarise the lecture notes".into(), "The notes cover three ideas. First, spindles group into **trains** during".into()));
    o.sending = Some(notebook_engine::Cancel::new());
}

fn notes(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.tab = NotebookTab::Notes;
    o.notes = vec![
        Note {
            id: "n1".into(),
            title: Some("Questions for Thursday".into()),
            content: Some("Does spindle density change with age? Ask about the 2019 replication.".into()),
            note_type: Some("human".into()),
            created: T.into(),
            updated: T.into(),
        },
        Note {
            id: "n2".into(),
            title: Some("Spindles and declarative memory".into()),
            content: Some("Spindle density after learning predicted recall the next morning.".into()),
            note_type: Some("ai".into()),
            created: T.into(),
            updated: T.into(),
        },
    ];
    o.note = Some(NoteDraft { id: None, title: TextEdit::new("Draft"), content: TextEdit::new("Spindles cluster in trains every 3 to 6 seconds.") });
}

fn add_source(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.add = Some(AddSource { url: TextEdit::new("https://en.wikipedia.org/wiki/Sleep_spindle"), ..Default::default() });
}

fn settings(s: &mut App) {
    s.notebooks = notebooks();
    s.form = Some(SettingsForm::from(&s.settings, &s.secrets));
    s.route = Route::Settings;
}

fn ask(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Ask;
    let mut p = crate::pages::AskPage { query: TextEdit::new("Do spindles matter for memory?"), ..Default::default() };
    p.steps.push(AskStep::Strategy {
        reasoning: "Look for studies linking spindle density to recall, then for limits.".into(),
        searches: vec![
            AskSearch { term: "spindle density recall".into(), instructions: String::new() },
            AskSearch { term: "procedural memory sleep".into(), instructions: String::new() },
        ],
    });
    p.result = Some(AskResult {
        answer: "Yes, for declarative memory: spindle density after learning predicted next-day recall [s1]. The link is weaker for skills [s2].".into(),
        documents: vec![
            AskDocument { id: "s1".into(), title: "Sleep spindles and memory consolidation".into(), notebook_id: Some("nb1".into()) },
            AskDocument { id: "s2".into(), title: "Why we sleep: a review of the evidence".into(), notebook_id: Some("nb1".into()) },
        ],
    });
    s.ask = Some(p);
}

fn library(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Library;
    s.library = Some(crate::pages::LibraryPage { sources: sources(), loaded: true, ..Default::default() });
}

fn transformations(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Transformations;
    let t = |id: &str, title: &str, d: &str, on| Transformation {
        id: id.into(),
        name: title.into(),
        title: title.into(),
        description: d.into(),
        prompt: "...".into(),
        apply_default: on,
    };
    s.transformations = Some(crate::pages::TransformationsPage {
        list: vec![
            t("t1", "Dense Summary", "A short summary that keeps every key fact.", true),
            t("t2", "Key Insights", "The ideas worth remembering.", false),
        ],
        instructions: TextEdit::new("Write in plain English. Quote the source when it matters."),
        ..Default::default()
    });
}

fn usage(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Usage;
    let g = |feature: &str, model: &str, calls, i, o, usd| UsageGroup {
        model: model.into(),
        feature: feature.into(),
        calls,
        input_tokens: i,
        output_tokens: o,
        audio_seconds: 0.0,
        estimated_usd: Some(usd),
    };
    s.usage = Some(crate::pages::UsagePage {
        days: 30,
        report: Some(UsageReport {
            days: 30,
            calls: 64,
            estimated_usd: Some(1.84),
            groups: vec![
                g("chat", "claude-sonnet-4-5", 41, 380_000, 52_000, 1.92),
                g("embedding", "text-embedding-3-small", 18, 210_000, 0, 0.01),
                g("insights", "claude-sonnet-4-5", 5, 60_000, 9_000, 0.31),
            ],
            estimated: true,
        }),
        ..Default::default()
    });
}

fn study(s: &mut App) {
    open(s);
    s.new_study_form();
    s.open.as_mut().unwrap().tab = NotebookTab::Study;
}

fn quiz(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.tab = NotebookTab::Study;
    o.decks = vec![StoredDeck {
        note_id: "n9".into(),
        created: None,
        deck: StudyDeck {
            version: 1,
            kind: StudyKind::Quiz,
            title: "Spindles and memory".into(),
            items: vec![StudyItem {
                question: "Which memories did spindle density predict best?".into(),
                answer: "Declarative memories, such as facts and word pairs.".into(),
                options: vec!["Motor skills".into(), "Declarative memories".into(), "Emotional memories".into(), "None".into()],
                correct_index: Some(1),
                source_ids: vec!["s1".into()],
            }],
        },
    }];
    o.viewing = Some(crate::study::DeckView { picked: Some(0), ..crate::study::DeckView::new(0, 1) });
}

fn insights(s: &mut App) {
    notebook(s);
    let o = s.open.as_mut().unwrap();
    o.transformations = vec![Transformation { id: "t1".into(), name: "Dense Summary".into(), title: "Dense Summary".into(), ..Default::default() }];
    o.insights = vec![Insight {
        id: "i1".into(),
        source_id: "s1".into(),
        kind: "Dense Summary".into(),
        content: "Spindle density after learning predicts **declarative** recall; the effect on skills is weaker.".into(),
        created: None,
        status: Some("completed".into()),
        error: None,
    }];
}

fn source_chat(s: &mut App) {
    notebook(s);
    let o = s.open.as_mut().unwrap();
    o.scope = Some(("s1".into(), "Sleep spindles and memory consolidation".into()));
}

fn server(s: &mut App) {
    s.notebooks = notebooks();
    s.settings.mode = notebook_engine::settings::Mode::Remote;
    s.remote = Some(std::sync::Arc::new(notebook_engine::remote::RemoteBackend::new("https://notebook.example.ts.net", None)));
    s.route = Route::Server;
    let m = |id: &str, name: &str, provider: &str, kind: &str| ServerModel { id: id.into(), name: name.into(), provider: provider.into(), kind: kind.into() };
    let mut p = crate::server::ServerPage {
        models: vec![
            m("model:1", "qwen3:14b", "ollama", "language"),
            m("model:2", "nomic-embed-text", "ollama", "embedding"),
            m("model:3", "gpt-4o-mini-tts", "openai", "text_to_speech"),
        ],
        credentials: serde_json::from_value(
            serde_json::json!([{"id": "credential:1", "name": "OpenAI", "provider": "openai", "has_api_key": true, "model_count": 12}]),
        )
        .unwrap(),
        jobs: serde_json::from_value(serde_json::json!([{"job_id": "command:9", "name": "process_source", "status": "running"}])).unwrap(),
        ..Default::default()
    };
    p.defaults.insert("default_chat_model".into(), Some("model:1".into()));
    p.defaults.insert("default_embedding_model".into(), Some("model:2".into()));
    p.notes.insert("credential:1".into(), "Key works".into());
    s.server = Some(p);
}

fn profiles_sample() -> PodcastProfiles {
    let speakers = SpeakerProfile {
        id: "sp1".into(),
        name: "Two hosts".into(),
        description: String::new(),
        voice_kind: Some("openai".into()),
        voice_model: "gpt-4o-mini-tts".into(),
        speakers: vec![
            Speaker { name: "Maya".into(), voice_id: "coral".into(), backstory: "A sleep researcher".into(), personality: "Curious".into() },
            Speaker { name: "Leo".into(), voice_id: "ash".into(), backstory: "A science writer".into(), personality: "Skeptical".into() },
        ],
    };
    let episode = EpisodeProfile {
        id: "ep1".into(),
        name: "Deep dive".into(),
        description: "Two hosts walk through the sources.".into(),
        speaker_profile_id: "sp1".into(),
        language: None,
        briefing: "Explain, then question.".into(),
        segments: 4,
    };
    PodcastProfiles { episode: vec![episode], speaker: vec![speakers] }
}

fn listen(s: &mut App) {
    open(s);
    s.podcast_profiles = Some(profiles_sample());
    let o = s.open.as_mut().unwrap();
    o.tab = NotebookTab::Listen;
    let line = |speaker: &str, t: &str, start| EpisodeLine { speaker: speaker.into(), text: t.into(), start: Some(start), end: None };
    let claim = |c: &str, v, q: &str| CheckedClaim {
        line: 1,
        said: c.into(),
        claim: c.into(),
        verdict: v,
        quote: Some(q.into()),
        source: Some("Sleep spindles and memory consolidation".into()),
        quote_found: true,
        note: None,
    };
    let done = Episode {
        id: "e1".into(),
        notebook_id: Some("nb1".into()),
        name: "What spindles do for memory".into(),
        summary: Some("Maya and Leo go through the spindle studies and where the evidence is thin.".into()),
        status: EpisodeStatus::Completed,
        progress: None,
        error: None,
        created: T.into(),
        duration: Some(754.0),
        chapters: vec![EpisodeChapter { title: "What a spindle is".into(), start: 0.0 }, EpisodeChapter { title: "The recall studies".into(), start: 212.0 }],
        transcript: vec![line("Maya", "So what is a sleep spindle?", 0.0), line("Leo", "A burst of activity in stage 2 sleep, about a second long.", 4.0)],
        exact_timing: true,
        voice_label: Some("OpenAI speech".into()),
        effort: Some(PodcastEffort::Careful),
        check: Some(EpisodeCheck {
            status: "done".into(),
            model: "claude-sonnet-4-5".into(),
            writer: Some("claude-opus-4-1".into()),
            error: None,
            claims: vec![
                claim(
                    "Spindle density predicted next-day recall.",
                    ClaimVerdict::Supported,
                    "spindle density after learning predicted how much of a word list people recalled",
                ),
                claim("Spindles cause better memory.", ClaimVerdict::Overstated, "predicted how much of a word list people recalled"),
            ],
            supported: 14,
            overstated: 2,
            unsupported: 1,
            sources_used: 2,
            sources_total: 3,
            history: vec![CheckCounts { supported: 11, overstated: 4, unsupported: 2 }],
        }),
    };
    let mut making = done.clone();
    making.id = "e2".into();
    making.name = "Lecture notes, week 4".into();
    making.status = EpisodeStatus::Voicing;
    making.progress = Some((7, 18));
    making.check = None;
    making.summary = None;
    o.episodes = vec![making, done.clone()];
    o.episode_views.insert("e1".into(), crate::listen::EpisodeOpen { transcript: true, claims: true, matched: false, prompt: None });
    let episode = done.clone();
    play_scene(s, episode, false);
}

/// A short silent WAV standing in for an episode's audio, so the player has a duration.
fn scene_player(seconds: f64) -> rune::Player {
    let rate = 1000;
    let path = std::env::temp_dir().join("open-notebook-scene-episode.wav");
    let pcm = vec![0i16; (seconds * rate as f64) as usize];
    let _ = std::fs::write(&path, notebook_engine::podcast::audio::encode_wav(&pcm, rate));
    rune::Player::audio(path)
}

fn play_scene(s: &mut App, episode: Episode, expanded: bool) {
    let player = scene_player(episode.duration.unwrap_or(60.0));
    player.seek(230.0);
    let mut p = crate::listen::NowPlaying::new(episode, player, 1);
    p.notebook = Some("Sleep and memory".into());
    p.expanded = expanded;
    p.sleep = crate::listen::Sleep::Minutes { minutes: 30, ends_at: None };
    s.playing = Some(p);
}

fn profiles(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Profiles;
    s.profiles = Some(crate::listen::ProfilesPage { profiles: profiles_sample(), ..Default::default() });
}

fn live_idle(s: &mut App) {
    listen(s);
}

fn live_connected(s: &mut App) {
    listen(s);
    s.live.status = notebook_engine::podcast::live_talk::LiveStatus::Connected;
    s.live.muted = true;
}

fn live_failed(s: &mut App) {
    listen(s);
    s.live.status = notebook_engine::podcast::live_talk::LiveStatus::Failed;
    s.live.error = Some(crate::live::MIC_DENIED.into());
}

fn now_playing(s: &mut App) {
    listen(s);
    let mut e = s.open.as_ref().unwrap().episodes[1].clone();
    let line = |speaker: &str, t: &str, start| EpisodeLine { speaker: speaker.into(), text: t.into(), start: Some(start), end: None };
    e.transcript = vec![
        line("Maya", "So what is a sleep spindle?", 0.0),
        line("Leo", "A burst of activity in stage 2 sleep, about a second long.", 4.0),
        line("Maya", "And people have tried to link them to memory.", 9.0),
        line("Leo", "Right. The recall studies are where it gets interesting.", 212.0),
        line("Maya", "Spindle density after learning predicted how much of a word list people recalled the next morning.", 220.0),
        line("Leo", "Predicted, which is not the same as caused.", 236.0),
        line("Maya", "Fair. And the effect was strongest for facts and word pairs.", 241.0),
    ];
    play_scene(s, e, true);
}

fn profile_editor(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Profiles;
    let profiles = profiles_sample();
    let mut episode = crate::listen::EpisodeDraft::from(&profiles.episode[0]);
    episode.language = Some("de".into());
    s.profiles = Some(crate::listen::ProfilesPage { profiles, episode: Some(episode), open_select: Some("ep-language"), ..Default::default() });
}

fn speaker_editor(s: &mut App) {
    s.notebooks = notebooks();
    s.route = Route::Profiles;
    let mut profiles = profiles_sample();
    profiles.episode.clear();
    let speaker = crate::listen::SpeakerDraft::from(&profiles.speaker[0]);
    let voice = |id: &str| notebook_engine::podcast::voices::VoiceOption { id: id.into(), label: id[..1].to_uppercase() + &id[1..] };
    s.profiles = Some(crate::listen::ProfilesPage {
        profiles,
        speaker: Some(speaker),
        voices: Some(Ok(["alloy", "ash", "coral", "marin", "cedar"].map(voice).to_vec())),
        voice_models: vec!["gpt-4o-mini-tts".into(), "tts-1".into(), "tts-1-hd".into()],
        open_voice: Some(1),
        preview_error: Some((0, "OpenAI said the key is missing".into())),
        ..Default::default()
    });
}

fn sample_transformations() -> Vec<Transformation> {
    let t = |id: &str, title: &str, on| Transformation { id: id.into(), name: title.into(), title: title.into(), apply_default: on, ..Default::default() };
    vec![t("t1", "Dense Summary", true), t("t2", "Key Insights", false), t("t3", "Questions", false)]
}

fn add_files(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.transformations = sample_transformations();
    let mut a = AddSource { kind: crate::state::AddKind::File, ..Default::default() };
    a.files = vec![("/Users/me/Downloads/spindles-review.pdf".into(), 2_400_000), ("/Users/me/Music/lecture-week-4.m4a".into(), 31_000_000)];
    o.add = Some(a);
}

fn add_batch(s: &mut App) {
    use crate::state::{BatchItem, ItemState};
    open(s);
    let item = |label: &str, state| BatchItem { label: label.into(), state };
    let o = s.open.as_mut().unwrap();
    o.add = Some(AddSource {
        items: Some(vec![
            item("en.wikipedia.org/wiki/Sleep_spindle", ItemState::Added),
            item("example.com/paywalled", ItemState::Failed("Could not download the page (403)".into())),
            item("arxiv.org/pdf/2203.02155", ItemState::Adding),
            item("youtube.com/watch?v=abc", ItemState::Waiting),
        ]),
        busy: true,
        ..Default::default()
    });
}

/// Settings on this Mac with keys saved and checked: one working, one rejected, one checking.
fn settings_keys(s: &mut App) {
    use crate::state::{Check, ModelList};
    use notebook_engine::providers::{Capability, ModelInfo, ProviderKind};
    settings(s);
    let f = s.form.as_mut().unwrap();
    for kind in ["anthropic", "openai", "gemini"] {
        f.saved_keys.insert(notebook_engine::settings::provider_secret(kind));
    }
    f.chat_provider = ProviderKind::Anthropic;
    f.chat_model = TextEdit::new("claude-sonnet-5-5");
    f.embedding_provider = ProviderKind::OpenAi;
    f.embedding_model = TextEdit::new("text-embedding-3-small");
    f.transcription_provider = ProviderKind::OpenAi;
    f.transcription_model = TextEdit::new("gpt-4o-transcribe");
    let model = |id: &str| ModelInfo { id: id.into(), label: None, capabilities: vec![Capability::Chat] };
    f.models.insert(ProviderKind::Anthropic, ModelList::Ready(vec![model("claude-sonnet-5-5"), model("claude-opus-5-5"), model("claude-haiku-5-5")]));
    f.models.insert(ProviderKind::OpenAi, ModelList::Failed("API key rejected: Incorrect API key provided".into()));
    f.checks.insert("anthropic".into(), Check::Working("Working, 9 models".into()));
    f.checks.insert("openai".into(), Check::Failed("API key rejected: Incorrect API key provided".into()));
    f.checks.insert("gemini".into(), Check::Checking);
    f.keys[3].1 = TextEdit::new("sk-or-pasted");
    f.sync = Some(Err(notebook_engine::settings::SyncError::Unsigned));
    f.rebuild = Some(Check::Working("Re-indexing 4 sources".into()));
}

/// The chat model picker open on Anthropic's models.
fn settings_picker(s: &mut App) {
    settings_keys(s);
    let f = s.form.as_mut().unwrap();
    f.pickers.entry("chat").or_default().open = true;
}

/// Server mode after Save and connect.
fn settings_server(s: &mut App) {
    s.settings.mode = notebook_engine::settings::Mode::Remote;
    settings(s);
    let f = s.form.as_mut().unwrap();
    f.saved_keys.insert(notebook_engine::settings::SERVER_PASSWORD.into());
    f.checks.insert("server".into(), crate::state::Check::Working("Connected, 12 notebooks".into()));
}

/// Usage with an unpriced model and an audio row, and the price dialog open.
fn usage_price(s: &mut App) {
    usage(s);
    let p = s.usage.as_mut().unwrap();
    let r = p.report.as_mut().unwrap();
    r.groups.push(UsageGroup {
        model: "gpt-6.1-sol".into(),
        feature: "podcast_research".into(),
        calls: 3,
        input_tokens: 90_000,
        output_tokens: 4_000,
        audio_seconds: 0.0,
        estimated_usd: None,
    });
    r.groups.push(UsageGroup {
        model: "gpt-4o-mini-tts".into(),
        feature: "podcast_audio".into(),
        calls: 12,
        input_tokens: 0,
        output_tokens: 0,
        audio_seconds: 754.0,
        estimated_usd: Some(0.004),
    });
    let mut d = crate::pages::PriceDraft::new("gpt-6.1-sol", false, &s.settings.prices);
    d.input = TextEdit::new("1.5");
    p.pricing = Some(d);
}

/// Editing a saved transformation, with a Try it result.
fn transformations_try(s: &mut App) {
    transformations(s);
    let p = s.transformations.as_mut().unwrap();
    let mut d = crate::pages::TransformationDraft::from(&p.list[1]);
    d.prompt = TextEdit::new("List the ideas worth remembering, one line each.");
    d.sample = TextEdit::new("Sleep spindles are bursts of activity during stage 2 sleep. Spindle density after learning predicted recall.");
    d.output = Some(Ok("- Spindles are short bursts in **stage 2** sleep\n- More spindles after learning, better recall the next day".into()));
    p.editing = Some(d);
}

/// The server page with its processing settings and a mix of jobs.
fn server_processing(s: &mut App) {
    server(s);
    let p = s.server.as_mut().unwrap();
    crate::server::apply_settings(
        p,
        serde_json::from_value(serde_json::json!({
            "default_content_processing_engine_doc": "docling",
            "default_content_processing_engine_url": "auto",
            "default_embedding_option": "always",
            "auto_delete_files": "yes",
            "docling_ocr": true,
            "docling_formulas": false,
            "docling_vision": true,
            "youtube_preferred_languages": ["en", "es"]
        }))
        .unwrap(),
    );
    p.jobs = serde_json::from_value(serde_json::json!([
        {"job_id": "command:9", "command": "open_notebook:process_source", "status": "running", "created": T},
        {"job_id": "command:8", "command": "embed_note", "status": "failed", "created": T, "error_message": "No embedding model"},
        {"job_id": "command:7", "command": "generate_podcast", "status": "completed", "created": T}
    ]))
    .unwrap();
}

fn flashcard_deck(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    o.tab = NotebookTab::Study;
    let item = |q: &str, a: &str| StudyItem {
        question: q.into(),
        answer: a.into(),
        options: Vec::new(),
        correct_index: None,
        source_ids: vec!["s1".into(), "s2".into()],
    };
    o.decks = vec![StoredDeck {
        note_id: "n8".into(),
        created: Some(T.into()),
        deck: StudyDeck {
            version: 1,
            kind: StudyKind::Flashcards,
            title: "Spindles".into(),
            items: vec![
                item("What is a sleep spindle?", "A burst of activity in the thalamocortical network during stage 2 sleep."),
                item("Which memories did spindles predict best?", "Declarative memories, such as facts and word pairs."),
                item("Is the link as strong for skills?", "No. It was weaker and depended on the task."),
            ],
        },
    }];
}

/// A flashcard flipped, after one card was missed and one known.
fn flashcards(s: &mut App) {
    flashcard_deck(s);
    let mut v = crate::study::DeckView::new(0, 3);
    v.answer(false);
    v.answer(true);
    v.flipped = true;
    s.open.as_mut().unwrap().viewing = Some(v);
}

fn flashcards_done(s: &mut App) {
    flashcard_deck(s);
    let mut v = crate::study::DeckView::new(0, 3);
    for _ in 0..3 {
        v.answer(true);
    }
    s.open.as_mut().unwrap().viewing = Some(v);
}

fn add_library(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    let mut elsewhere = sources();
    for (i, src) in elsewhere.iter_mut().enumerate() {
        src.id = format!("x{i}");
        src.notebooks = vec!["nb2".into()];
    }
    elsewhere[1].title = Some("Heat pump sizing guide".into());
    let mut a = AddSource { kind: crate::state::AddKind::Library, library: Some(elsewhere), ..Default::default() };
    a.picked.insert("x1".into());
    o.add = Some(a);
}

fn described(s: &mut App) {
    open(s);
    let o = s.open.as_mut().unwrap();
    if let Some(nb) = o.notebook.as_mut() {
        nb.description = "Reading for the sleep seminar: what spindles do for memory, and how strong the evidence is.".into();
    }
}

fn notebook_details(s: &mut App) {
    described(s);
    let o = s.open.as_mut().unwrap();
    let nb = o.notebook.clone().unwrap();
    o.details = Some(crate::state::NotebookDetails { name: TextEdit::new(nb.name), description: TextEdit::new(nb.description) });
}

fn notebook_search(s: &mut App) {
    described(s);
    let o = s.open.as_mut().unwrap();
    o.search = TextEdit::new("spindle density");
    o.search_open = true;
    let hit = |kind, id: &str, title: &str, snippet: &str| SearchHit {
        kind,
        id: id.into(),
        title: title.into(),
        snippet: snippet.into(),
        score: 1.0,
        notebook_id: Some("nb1".into()),
    };
    o.hits = Some((
        "spindle density".into(),
        Ok(vec![
            hit(
                HitKind::Source,
                "s1",
                "Sleep spindles and memory consolidation",
                "In several studies, spindle density after learning predicted how much of a word list people recalled",
            ),
            hit(HitKind::Note, "n2", "Spindles and declarative memory", "Spindle density after learning predicted recall the next morning."),
        ]),
    ));
}

fn new_notebook(s: &mut App) {
    s.notebooks = notebooks();
    s.new_notebook = Some(TextEdit::new("Heat pump retrofit"));
    s.new_notebook_description = TextEdit::new("Quotes and sizing guides for the house.");
}

fn archived(s: &mut App) {
    s.notebooks = notebooks();
    let mut old = s.notebooks[1].clone();
    old.id = "nb4".into();
    old.name = "Last year's courses".into();
    old.archived = true;
    s.notebooks.push(old);
    s.show_archived = true;
}

fn dropping(s: &mut App) {
    open(s);
    s.open.as_mut().unwrap().drop_over = true;
}

fn reader(s: &mut App) {
    insights(s);
    let o = s.open.as_mut().unwrap();
    o.context.sources.insert("s2".into(), SourceContext::Off);
    if let Some((d, _)) = o.reading.as_mut() {
        d.summary.file_available = true;
        d.summary.embedded_chunks = 6;
        d.summary.notebooks = vec!["nb1".into(), "nb2".into()];
    }
    let more = |id: &str, kind: &str, status: &str, error: Option<&str>| Insight {
        id: id.into(),
        source_id: "s1".into(),
        kind: kind.into(),
        content: String::new(),
        created: None,
        status: Some(status.into()),
        error: error.map(Into::into),
    };
    o.insights.push(more("i2", "Key Insights", "running", None));
    o.insights.push(more("i3", "Questions", "failed", Some("The model returned nothing")));
    o.insight_open = Some("i1".into());
    o.saved_insights.insert("i1".into());
}

fn reader_passage(s: &mut App) {
    notebook(s);
    let o = s.open.as_mut().unwrap();
    let filler = "Earlier sections cover how sleep stages are scored, the history of EEG and how spindles were first described.\n\n".repeat(40);
    let text = format!("{filler}{SOURCE_TEXT}");
    let passage = notebook_engine::citations::best_passage(&text, "spindle density after learning predicted recall of a word list");
    let summary = o.sources[0].clone();
    o.reading = Some((SourceDetail { summary, full_text: text }, passage));
    o.scroll_to_passage = true;
}

fn chat_models(s: &mut App) {
    notebook(s);
    s.settings.chat = Some(notebook_engine::settings::ModelSetting { provider: "anthropic".into(), model: "claude-sonnet-4-5".into() });
    let o = s.open.as_mut().unwrap();
    o.reading = None;
    o.active_cite = Some(("m2".into(), 0));
    o.models = Some(vec![("anthropic::claude-opus-4-1".into(), "Claude Opus 4.1".into()), ("anthropic::claude-haiku-4-5".into(), "Claude Haiku 4.5".into())]);
}

fn chat_rename(s: &mut App) {
    notebook(s);
    let o = s.open.as_mut().unwrap();
    o.renaming_chat = Some(TextEdit::new("What do spindles do?"));
    let mut twin = o.sessions[0].clone();
    twin.id = "c2".into();
    twin.updated = "2026-10-01T10:00:00Z".into();
    o.sessions.push(twin);
}

fn chat_empty(s: &mut App) {
    open(s);
    for src in &mut s.open.as_mut().unwrap().sources {
        src.status = Some("processing".into());
    }
}

fn notes_list(s: &mut App) {
    notes(s);
    let o = s.open.as_mut().unwrap();
    o.note = None;
    o.context.notes.insert("n1".into(), false);
    let deck = format!("{}{{}}", notebook_engine::studio::STUDY_PREFIX);
    o.notes.push(Note { id: "n9".into(), title: Some("Spindle quiz".into()), content: Some(deck), created: T.into(), updated: T.into(), ..Default::default() });
}

fn note_read(s: &mut App) {
    notes(s);
    let o = s.open.as_mut().unwrap();
    let mut n = o.notes[1].clone();
    n.content = Some("## What the studies found\n\nSpindle density after learning predicted recall the next morning [Sleep spindles and memory consolidation](cite:0).\n\n- Strongest for **declarative** memory\n- Weaker for skills".into());
    o.edit_note(&n);
}

/// The notebook's chat with nothing open in the rail.
fn chat(s: &mut App) {
    notebook(s);
    let o = s.open.as_mut().unwrap();
    o.reading = None;
    o.active_cite = None;
}

/// A phone's Sources tab.
fn sources_tab(s: &mut App) {
    chat(s);
    s.open.as_mut().unwrap().sources_tab = true;
}

/// A partly folded iPhone Duo: a vertical fold 24 points wide, 400 points in.
fn fold(s: &mut App) {
    notebook(s);
    s.fold = Some([400.0, 0.0, 24.0, 2000.0]);
}
