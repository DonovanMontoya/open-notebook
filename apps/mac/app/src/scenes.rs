//! Named states for headless snapshots (`--snap out.png --scene notebook`). They fill the state
//! with sample data and never open a backend.

use notebook_engine::types::*;
use rune::TextEdit;

use crate::state::{AddSource, App, NoteDraft, NotebookTab, Route, SettingsForm};

type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("home", home),
    ("notebook", notebook),
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

fn home(s: &mut App) {
    s.notebooks = notebooks();
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
    o.viewing = Some(crate::study::DeckView { index: 0, card: 0, flipped: false, picked: Some(0), right: 0 });
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
            writer: None,
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
            history: Vec::new(),
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
    o.episode_views.insert("e1".into(), crate::listen::EpisodeOpen { transcript: true, claims: true, prompt: None });
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
