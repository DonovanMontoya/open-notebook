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
];

/// Whether this run renders a scene, so `init` skips opening the real backend.
pub fn requested() -> bool {
    std::env::args().any(|a| a == "--scene" || a.starts_with("--scene="))
}

const T: &str = "2026-11-05T10:00:00Z";

fn notebooks() -> Vec<Notebook> {
    let nb = |id: &str, name: &str, sources| Notebook { id: id.into(), name: name.into(), created: T.into(), updated: T.into(), source_count: sources, ..Default::default() };
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
    o.sessions = vec![ChatSession { id: "c1".into(), title: "What do spindles do?".into(), notebook_id: "nb1".into(), created: T.into(), updated: T.into(), ..Default::default() }];
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
        Note { id: "n1".into(), title: Some("Questions for Thursday".into()), content: Some("Does spindle density change with age? Ask about the 2019 replication.".into()), note_type: Some("human".into()), created: T.into(), updated: T.into() },
        Note { id: "n2".into(), title: Some("Spindles and declarative memory".into()), content: Some("Spindle density after learning predicted recall the next morning.".into()), note_type: Some("ai".into()), created: T.into(), updated: T.into() },
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
