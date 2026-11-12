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
    /// Position in `order`.
    pub card: usize,
    pub flipped: bool,
    pub picked: Option<usize>,
    pub right: u32,
    /// Item indexes in the order they come up; flashcards missed with Again are added again at the end.
    pub order: Vec<usize>,
    /// Flashcards answered with Got it.
    pub known: u32,
}

impl DeckView {
    /// Deck `index` from its first card, with `len` items.
    pub fn new(index: usize, len: usize) -> Self {
        DeckView { index, card: 0, flipped: false, picked: None, right: 0, order: (0..len).collect(), known: 0 }
    }

    /// The item showing, or None when the deck is done.
    pub fn item(&self) -> Option<usize> {
        self.order.get(self.card).copied()
    }

    /// A flashcard answered: known moves on, Again puts it back at the end.
    pub fn answer(&mut self, knew_it: bool) {
        let Some(item) = self.item() else { return };
        if knew_it {
            self.known += 1;
        } else {
            self.order.push(item);
        }
        self.next();
    }

    pub fn next(&mut self) {
        self.card += 1;
        self.flipped = false;
        self.picked = None;
    }

    pub fn restart(&mut self) {
        *self = DeckView::new(self.index, self.order.iter().max().map_or(0, |m| m + 1).max(self.known as usize));
    }
}

/// Sources a deck can be made from: those that finished reading.
pub fn ready(status: Option<&str>) -> bool {
    matches!(status, None | Some("completed"))
}

/// The first part of a locale such as "en_US.UTF-8" or "pt-BR": "en", "pt".
pub fn language_of(locale: &str) -> Option<String> {
    let tag = locale.split(['.', '@', '_', '-']).next().unwrap_or("").trim().to_lowercase();
    (!tag.is_empty() && tag != "c" && tag != "posix" && tag.chars().all(|c| c.is_ascii_alphabetic())).then_some(tag)
}

/// The system language, for deck questions: the locale variables, then macOS's own setting,
/// then English.
pub fn system_language() -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(l) = std::env::var(var).ok().as_deref().and_then(language_of) {
            return l;
        }
    }
    if cfg!(target_os = "macos")
        && let Ok(out) = std::process::Command::new("defaults").args(["read", "-g", "AppleLocale"]).output()
        && let Some(l) = language_of(&String::from_utf8_lossy(&out.stdout))
    {
        return l;
    }
    "en".into()
}

impl App {
    // Insights

    pub fn load_insights(&self, source_id: String) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let sid = source_id.clone();
        self.run(
            move |b| b.list_insights(&sid),
            move |app, r| match r {
                Ok(list) => {
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == nb && o.reading.as_ref().is_some_and(|(d, _)| d.summary.id == source_id)) {
                        o.insights = list;
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn run_insight(&mut self, transformation_id: String) {
        let Some(o) = self.open.as_mut() else { return };
        o.insight_picker = false;
        let Some(source_id) = o.reading.as_ref().map(|(d, _)| d.summary.id.clone()) else { return };
        let sid = source_id.clone();
        self.run(
            move |b| b.create_insight(&sid, &transformation_id),
            move |app, r| match r {
                Ok(()) => app.load_insights(source_id),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn delete_insight(&mut self, id: String) {
        let gone = id.clone();
        self.run(
            move |b| b.delete_insight(&id),
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut() {
                        o.insights.retain(|i| i.id != gone);
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn insight_to_note(&mut self, id: String) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(
            move |b| b.save_insight_as_note(&id, &nb),
            |app, r| match r {
                Ok(_) => {
                    app.toast = Some("Saved to notes".into());
                    app.refresh_notes();
                }
                Err(e) => app.fail(e),
            },
        );
    }

    // Study decks

    pub fn load_decks(&self) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let id = nb.clone();
        self.run(
            move |b| b.list_study_decks(&id),
            move |app, r| match r {
                Ok(decks) => {
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == nb) {
                        o.decks = decks;
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn new_study_form(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let ready = o.sources.iter().filter(|s| ready(s.status.as_deref()));
        o.study = Some(StudyForm {
            kind: StudyKind::Flashcards,
            difficulty: 1,
            count: 8,
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
        let mut request = StudyRequest {
            kind: f.kind,
            source_ids: f.selected.iter().cloned().collect(),
            count: f.count,
            difficulty: DIFFICULTIES[f.difficulty].into(),
            language: None,
            instructions: Some(f.focus.text.trim().to_owned()).filter(|t| !t.is_empty()),
        };
        let nb = o.id.clone();
        let id = nb.clone();
        self.run(
            move |b| {
                request.language = Some(system_language());
                b.generate_study_deck(&id, request)
            },
            move |app, r| {
                let Some(o) = app.open.as_mut().filter(|o| o.id == nb) else { return };
                match r {
                    Ok(deck) => {
                        // Open the new deck now; the reload finds it again in the stored list.
                        o.study = None;
                        o.viewing = Some(DeckView::new(0, deck.items.len()));
                        o.decks.insert(0, StoredDeck { deck: deck.clone(), note_id: String::new(), created: None });
                        app.reload_decks_showing(deck);
                        app.refresh_notes();
                    }
                    Err(e) => {
                        if let Some(f) = o.study.as_mut() {
                            f.busy = false;
                            f.error = Some(e.to_string());
                        }
                    }
                }
            },
        );
    }
}

impl App {
    /// Reloads the decks and keeps viewing `deck` wherever it lands in the list.
    fn reload_decks_showing(&self, deck: StudyDeck) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let id = nb.clone();
        self.run(
            move |b| b.list_study_decks(&id),
            move |app, r| match r {
                Ok(decks) => {
                    let Some(o) = app.open.as_mut().filter(|o| o.id == nb) else { return };
                    let viewing_new = o.viewing.as_ref().is_some_and(|v| o.decks.get(v.index).is_some_and(|d| d.deck == deck));
                    let at = decks.iter().position(|d| d.deck == deck);
                    o.decks = decks;
                    if viewing_new && let Some(v) = o.viewing.as_mut() {
                        match at {
                            Some(i) => v.index = i,
                            None => o.viewing = None,
                        }
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missed_cards_come_back_at_the_end() {
        let mut v = DeckView::new(0, 3);
        v.flipped = true;
        v.answer(false); // card 0 again
        assert_eq!(v.order, [0, 1, 2, 0]);
        assert!(!v.flipped);
        v.answer(true);
        v.answer(true);
        assert_eq!(v.item(), Some(0));
        v.answer(true);
        assert_eq!((v.item(), v.known), (None, 3));
        v.restart();
        assert_eq!((v.order.clone(), v.card, v.known), (vec![0, 1, 2], 0, 0));
    }

    #[test]
    fn language_is_the_first_locale_part() {
        assert_eq!(language_of("en_US.UTF-8").as_deref(), Some("en"));
        assert_eq!(language_of("pt-BR").as_deref(), Some("pt"));
        assert_eq!(language_of("C"), None);
        assert_eq!(language_of(""), None);
        assert!(ready(None) && ready(Some("completed")) && !ready(Some("running")));
    }
}
