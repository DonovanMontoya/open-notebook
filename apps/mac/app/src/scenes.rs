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
