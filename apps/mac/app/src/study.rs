//! State and actions inside a notebook beyond chat and notes: a source's insights, and study
//! decks (flashcards and quizzes) made from chosen sources.

use std::collections::BTreeSet;

use notebook_engine::types::*;
use rune::TextEdit;

use crate::state::App;

pub const DIFFICULTIES: [&str; 3] = ["easy", "medium", "hard"];

pub struct StudyForm {
    pub kind: StudyKind,
    /// Index into [`DIFFICULTIES`].
    pub difficulty: usize,
    pub count: u32,
    pub selected: BTreeSet<String>,
    pub focus: TextEdit,
    pub busy: bool,
    pub error: Option<String>,
}

/// A deck being studied: which card, whether its answer shows, and the quiz score.
pub struct DeckView {
    pub index: usize,
    pub card: usize,
    pub flipped: bool,
    pub picked: Option<usize>,
    pub right: u32,
}

impl App {
    // Insights

    pub fn load_insights(&self, source_id: String) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let sid = source_id.clone();
        self.run(move |b| b.list_insights(&sid), move |app, r| match r {
            Ok(list) => {
                if let Some(o) = app.open.as_mut().filter(|o| o.id == nb && o.reading.as_ref().is_some_and(|(d, _)| d.summary.id == source_id)) {
                    o.insights = list;
                }
            }
            Err(e) => app.fail(e),
        });
    }

    pub fn run_insight(&mut self, transformation_id: String) {
        let Some(o) = self.open.as_mut() else { return };
        o.insight_picker = false;
        let Some(source_id) = o.reading.as_ref().map(|(d, _)| d.summary.id.clone()) else { return };
        let sid = source_id.clone();
        self.run(move |b| b.create_insight(&sid, &transformation_id), move |app, r| match r {
            Ok(()) => app.load_insights(source_id),
            Err(e) => app.fail(e),
        });
    }

    pub fn delete_insight(&mut self, id: String) {
        let gone = id.clone();
        self.run(move |b| b.delete_insight(&id), move |app, r| match r {
            Ok(()) => {
                if let Some(o) = app.open.as_mut() {
                    o.insights.retain(|i| i.id != gone);
                }
            }
            Err(e) => app.fail(e),
        });
    }

    pub fn insight_to_note(&mut self, id: String) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(move |b| b.save_insight_as_note(&id, &nb), |app, r| match r {
            Ok(_) => {
                app.toast = Some("Saved to notes".into());
                app.refresh_notes();
            }
            Err(e) => app.fail(e),
        });
    }

    // Study decks

    pub fn load_decks(&self) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let id = nb.clone();
        self.run(move |b| b.list_study_decks(&id), move |app, r| match r {
            Ok(decks) => {
                if let Some(o) = app.open.as_mut().filter(|o| o.id == nb) {
                    o.decks = decks;
                }
            }
            Err(e) => app.fail(e),
        });
    }

    pub fn new_study_form(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let ready = o.sources.iter().filter(|s| !matches!(s.status.as_deref(), Some("failed" | "new" | "queued" | "running")));
        o.study = Some(StudyForm {
            kind: StudyKind::Flashcards,
            difficulty: 1,
            count: 10,
            selected: ready.map(|s| s.id.clone()).collect(),
            focus: TextEdit::new(""),
            busy: false,
            error: None,
        });
    }

    pub fn make_deck(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(f) = o.study.as_mut() else { return };
        if f.busy {
            return;
        }
        if f.selected.is_empty() {
            f.error = Some("Choose at least one source.".into());
            return;
        }
        f.busy = true;
        f.error = None;
        let request = StudyRequest {
            kind: f.kind,
            source_ids: f.selected.iter().cloned().collect(),
            count: f.count,
            difficulty: DIFFICULTIES[f.difficulty].into(),
            language: None,
            instructions: Some(f.focus.text.trim().to_owned()).filter(|t| !t.is_empty()),
        };
        let nb = o.id.clone();
        let id = nb.clone();
        self.run(move |b| b.generate_study_deck(&id, request), move |app, r| {
            let Some(o) = app.open.as_mut().filter(|o| o.id == nb) else { return };
            match r {
                Ok(_) => {
                    o.study = None;
                    app.load_decks();
                    app.refresh_notes();
                }
                Err(e) => {
                    if let Some(f) = o.study.as_mut() {
                        f.busy = false;
                        f.error = Some(e.to_string());
                    }
                }
            }
        });
    }
}
