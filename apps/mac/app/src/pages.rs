//! State and actions for the pages away from a notebook: Ask, Library, Transformations, Usage.

use notebook_engine::Cancel;
use notebook_engine::pricing::{self, Price, PriceOverrides};
use notebook_engine::types::*;
use rune::TextEdit;

use crate::state::{App, Route};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AskMode {
    #[default]
    Ask,
    Search,
}

#[derive(Default)]
pub struct AskPage {
    pub mode: AskMode,
    pub query: TextEdit,
    /// None looks in every notebook.
    pub scope: Option<String>,
    pub scope_open: bool,
    pub steps: Vec<AskStep>,
    pub result: Option<AskResult>,
    pub hits: Option<Vec<SearchHit>>,
    pub busy: Option<Cancel>,
    pub error: Option<String>,
}

/// Search needs this many characters, so one letter doesn't match everything.
pub const MIN_SEARCH_CHARS: usize = 2;

#[derive(Default)]
pub struct LibraryPage {
    pub sources: Vec<SourceSummary>,
    pub filter: TextEdit,
    pub loaded: bool,
    /// Loading, adding or removing failed.
    pub error: Option<String>,
    /// The source whose "Add to" menu is open.
    pub add_open: Option<String>,
}

/// A transformation being written or edited. It stays open until its save succeeds.
pub struct TransformationDraft {
    pub id: String,
    pub title: TextEdit,
    pub description: TextEdit,
    pub prompt: TextEdit,
    pub apply_default: bool,
    /// What it was when opened, for the name and to tell whether anything changed.
    pub original: Transformation,
    pub saving: bool,
    pub error: Option<String>,
    /// The "Try it" panel: text to run it on, and the result or error.
    pub sample: TextEdit,
    pub running: bool,
    pub output: Option<Result<String, String>>,
}

impl TransformationDraft {
    pub fn from(t: &Transformation) -> Self {
        TransformationDraft {
            id: t.id.clone(),
            title: TextEdit::new(t.title.clone()),
            description: TextEdit::new(t.description.clone()),
            prompt: TextEdit::new(t.prompt.clone()),
            apply_default: t.apply_default,
            original: t.clone(),
            saving: false,
            error: None,
            sample: TextEdit::new(""),
            running: false,
            output: None,
        }
    }

    /// The transformation as it would be saved. The name falls back to the title.
    pub fn value(&self) -> Transformation {
        let title = self.title.text.trim().to_owned();
        Transformation {
            id: self.id.clone(),
            name: Some(self.original.name.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| title.clone()),
            title,
            description: self.description.text.trim().to_owned(),
            prompt: self.prompt.text.clone(),
            apply_default: self.apply_default,
        }
    }

    pub fn can_save(&self) -> bool {
        let v = self.value();
        !self.saving && !v.title.is_empty() && !v.prompt.trim().is_empty() && (self.id.is_empty() || v != self.original)
    }
}

#[derive(Default)]
pub struct TransformationsPage {
    pub list: Vec<Transformation>,
    pub editing: Option<TransformationDraft>,
    pub instructions: TextEdit,
    pub instructions_saved: bool,
    /// Loading or deleting failed.
    pub error: Option<String>,
}

#[derive(Default)]
pub struct UsagePage {
    pub days: u32,
    pub report: Option<UsageReport>,
    pub error: Option<String>,
    /// The price dialog, when open.
    pub pricing: Option<PriceDraft>,
}

/// The price dialog's fields: dollars per million tokens, or per minute for audio models.
pub struct PriceDraft {
    pub model: String,
    pub audio: bool,
    pub input: TextEdit,
    pub output: TextEdit,
    pub per_minute: TextEdit,
    /// The user has set their own price for this model.
    pub custom: bool,
    /// The model is on the built-in list, so clearing goes back to it.
    pub listed: bool,
}

impl PriceDraft {
    pub fn new(model: &str, audio: bool, prices: &PriceOverrides) -> Self {
        let p = pricing::price_for(model, prices).unwrap_or_default();
        let field = |v: Option<f64>| TextEdit::new(v.map(|v| v.to_string()).unwrap_or_default());
        PriceDraft {
            model: model.into(),
            audio,
            input: field(p.input),
            output: field(p.output),
            per_minute: field(p.per_minute),
            custom: prices.contains_key(model),
            listed: pricing::MODEL_PRICES.iter().any(|(id, _)| *id == model),
        }
    }

    /// The price the fields describe, or None while they don't make one.
    pub fn price(&self) -> Option<Price> {
        let num = |e: &TextEdit| e.text.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
        if self.audio {
            num(&self.per_minute).map(|m| Price { per_minute: Some(m), ..Default::default() })
        } else {
            let (input, output) = (num(&self.input), num(&self.output));
            (input.is_some() || output.is_some()).then_some(Price { input, output, per_minute: None })
        }
    }
}

impl App {
    pub fn go(&mut self, route: Route) {
        match route {
            Route::Ask if self.ask.is_none() => self.ask = Some(AskPage::default()),
            Route::Library => self.load_library(),
            Route::Transformations => {
                self.transformations.get_or_insert_with(TransformationsPage::default);
                self.load_transformations();
                self.run(
                    |b| b.transformation_instructions(),
                    |app, r| match r {
                        Ok(text) => {
                            if let Some(p) = app.transformations.as_mut() {
                                p.instructions = TextEdit::new(text);
                            }
                        }
                        Err(e) => app.fail(e),
                    },
                );
            }
            Route::Usage => self.load_usage(self.usage.as_ref().map_or(30, |u| u.days)),
            Route::Server => self.load_server(),
            _ => {}
        }
        self.route = route;
    }

    // Ask and search

    pub fn ask_submit(&mut self) {
        let Some(p) = self.ask.as_mut() else { return };
        let query = p.query.text.trim().to_owned();
        let short = p.mode == AskMode::Search && query.chars().count() < MIN_SEARCH_CHARS;
        if query.is_empty() || short || p.busy.is_some() {
            return;
        }
        let cancel = Cancel::new();
        p.busy = Some(cancel.clone());
        p.error = None;
        p.steps.clear();
        p.result = None;
        p.hits = None;
        let scope = p.scope.clone();
        match p.mode {
            AskMode::Search => self.run(
                move |b| b.search(&query, scope.as_deref()),
                |app, r| {
                    let Some(p) = app.ask.as_mut() else { return };
                    p.busy = None;
                    match r {
                        Ok(hits) => p.hits = Some(hits),
                        Err(e) => p.error = Some(e.to_string()),
                    }
                },
            ),
            AskMode::Ask => {
                let inbox = self.inbox();
                self.run(
                    move |b| {
                        let mut on_step = |step: AskStep| {
                            inbox.send(move |app| {
                                if let Some(p) = app.ask.as_mut() {
                                    p.steps.push(step);
                                }
                            })
                        };
                        b.ask(&query, scope.as_deref(), &mut on_step, &cancel)
                    },
                    |app, r| {
                        let Some(p) = app.ask.as_mut() else { return };
                        p.busy = None;
                        match r {
                            Ok(result) => p.result = Some(result),
                            Err(notebook_engine::Error::Cancelled) => {}
                            Err(e) => p.error = Some(e.to_string()),
                        }
                    },
                );
            }
        }
    }

    /// Opens a source from Ask, Search or the Library in its notebook.
    pub fn open_source(&mut self, source_id: String, notebook_id: Option<String>, query: Option<String>) {
        if let Some(nb) = notebook_id {
            self.open_notebook(nb);
            self.read_source(source_id, query);
            return;
        }
        let id = source_id.clone();
        self.run(
            move |b| b.get_source(&id),
            move |app, r| match r {
                Ok(detail) => match detail.summary.notebooks.first().cloned() {
                    Some(nb) => {
                        app.open_notebook(nb);
                        app.read_source(source_id, query);
                    }
                    None => app.fail("That source isn't in a notebook"),
                },
                Err(e) => app.fail(e),
            },
        );
    }

    // Transformations

    pub fn load_transformations(&self) {
        self.run(
            |b| b.list_transformations(),
            |app, r| match r {
                Ok(list) => {
                    if let Some(o) = app.open.as_mut() {
                        o.transformations = list.clone();
                    }
                    if let Some(p) = app.transformations.as_mut() {
                        p.list = list;
                        p.error = None;
                    }
                }
                Err(e) => match app.transformations.as_mut() {
                    Some(p) => p.error = Some(e.to_string()),
                    None => app.fail(e),
                },
            },
        );
    }

    /// Saves the open draft. It stays open, with the error, until the save succeeds.
    pub fn save_transformation(&mut self) {
        let Some(d) = self.transformations.as_mut().and_then(|p| p.editing.as_mut()) else { return };
        if !d.can_save() {
            return;
        }
        d.saving = true;
        d.error = None;
        let t = d.value();
        let id = t.id.clone();
        self.run(
            move |b| b.save_transformation(t),
            move |app, r| {
                let Some(p) = app.transformations.as_mut() else { return };
                let draft = p.editing.as_mut().filter(|d| d.id == id);
                match r {
                    Ok(_) => {
                        if draft.is_some() {
                            p.editing = None;
                        }
                        app.load_transformations();
                    }
                    Err(e) => match draft {
                        Some(d) => {
                            d.saving = false;
                            d.error = Some(e.to_string());
                        }
                        None => app.fail(e),
                    },
                }
            },
        );
    }

    pub fn delete_transformation(&mut self, id: String) {
        let gone = id.clone();
        self.run(
            move |b| b.delete_transformation(&id),
            move |app, r| match r {
                Ok(()) => {
                    if let Some(p) = app.transformations.as_mut()
                        && p.editing.as_ref().is_some_and(|d| d.id == gone)
                    {
                        p.editing = None;
                    }
                    app.load_transformations();
                }
                Err(e) => match app.transformations.as_mut() {
                    Some(p) => p.error = Some(e.to_string()),
                    None => app.fail(e),
                },
            },
        );
    }

    /// Runs the saved transformation in the editor on the "Try it" text.
    pub fn try_transformation(&mut self) {
        let Some(d) = self.transformations.as_mut().and_then(|p| p.editing.as_mut()) else { return };
        let text = d.sample.text.trim().to_owned();
        if d.id.is_empty() || text.is_empty() || d.running {
            return;
        }
        d.running = true;
        d.output = None;
        let id = d.id.clone();
        let tid = id.clone();
        self.run(
            move |b| b.run_transformation(&tid, &text, &Cancel::new()),
            move |app, r| {
                if let Some(d) = app.transformations.as_mut().and_then(|p| p.editing.as_mut()).filter(|d| d.id == id) {
                    d.running = false;
                    d.output = Some(r.map_err(|e| e.to_string()));
                }
            },
        );
    }

    pub fn save_instructions(&mut self) {
        let Some(text) = self.transformations.as_ref().map(|p| p.instructions.text.clone()) else { return };
        self.run(
            move |b| b.set_transformation_instructions(&text),
            |app, r| match r {
                Ok(()) => {
                    if let Some(p) = app.transformations.as_mut() {
                        p.instructions_saved = true;
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    // Usage

    pub fn load_usage(&mut self, days: u32) {
        let p = self.usage.get_or_insert_with(UsagePage::default);
        p.days = days;
        p.error = None;
        let prices = self.settings.prices.clone();
        self.run(
            move |b| {
                let mut report = b.usage(days)?;
                if b.kind() == notebook_engine::BackendKind::Local {
                    price(&mut report, &prices);
                }
                sort_groups(&mut report);
                Ok(report)
            },
            move |app, r| {
                let Some(p) = app.usage.as_mut().filter(|p| p.days == days) else { return };
                match r {
                    Ok(report) => p.report = Some(report),
                    Err(e) => {
                        p.report = None;
                        p.error = Some(e.to_string());
                    }
                }
            },
        );
    }

    /// Opens the price dialog for a model.
    pub fn edit_price(&mut self, model: &str, audio: bool) {
        let draft = PriceDraft::new(model, audio, &self.settings.prices);
        if let Some(p) = self.usage.as_mut() {
            p.pricing = Some(draft);
        }
    }

    /// Saves (Some) or clears (None) the user's price for the model in the dialog, then
    /// prices the report again.
    pub fn save_price(&mut self, new: Option<Price>) {
        let Some(d) = self.usage.as_mut().and_then(|p| p.pricing.take()) else { return };
        match new {
            Some(p) => self.settings.prices.insert(d.model, p),
            None => self.settings.prices.remove(&d.model),
        };
        if let Err(e) = self.settings.save(&self.data_dir) {
            self.fail(e);
        }
        let local = self.backend.as_ref().is_some_and(|b| b.kind() == notebook_engine::BackendKind::Local);
        if let Some(report) = self.usage.as_mut().and_then(|p| p.report.as_mut())
            && local
        {
            price(report, &self.settings.prices);
            sort_groups(report);
        }
    }

    // Library

    pub fn load_library(&mut self) {
        self.library.get_or_insert_with(LibraryPage::default);
        self.run(
            |b| b.list_all_sources(),
            |app, r| {
                let Some(p) = app.library.as_mut() else { return };
                p.loaded = true;
                match r {
                    Ok(sources) => {
                        p.sources = sources;
                        p.error = None;
                    }
                    Err(e) => p.error = Some(e.to_string()),
                }
            },
        );
    }

    /// Adds a source to another notebook (`link`) or removes it from one.
    pub fn link_source(&mut self, source_id: String, notebook_id: String, link: bool) {
        if let Some(p) = self.library.as_mut() {
            p.add_open = None;
        }
        self.run(
            move |b| if link { b.link_source(&source_id, &notebook_id) } else { b.unlink_source(&source_id, &notebook_id) },
            |app, r| match r {
                Ok(()) => {
                    app.load_library();
                    app.refresh_notebooks();
                }
                Err(e) => match app.library.as_mut() {
                    Some(p) => p.error = Some(e.to_string()),
                    None => app.fail(e),
                },
            },
        );
    }
}

/// On device, cost comes from the user's prices, then list prices (the server reports its
/// own). Models without a price stay unpriced and out of the total.
fn price(report: &mut UsageReport, prices: &PriceOverrides) {
    let mut total = None;
    for g in &mut report.groups {
        g.estimated_usd = pricing::cost_of(&g.model, g.input_tokens, g.output_tokens, g.audio_seconds, prices);
        if let Some(c) = g.estimated_usd {
            *total.get_or_insert(0.0) += c;
        }
    }
    report.estimated_usd = total;
}

/// Most expensive first, then most calls; unpriced rows last.
fn sort_groups(report: &mut UsageReport) {
    report.groups.sort_by(|a, b| b.estimated_usd.unwrap_or(-1.0).total_cmp(&a.estimated_usd.unwrap_or(-1.0)).then(b.calls.cmp(&a.calls)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(model: &str, calls: u64, input: u64, usd: Option<f64>) -> UsageGroup {
        UsageGroup { model: model.into(), feature: "chat".into(), calls, input_tokens: input, output_tokens: 0, audio_seconds: 0.0, estimated_usd: usd }
    }

    #[test]
    fn custom_prices_count_and_rows_sort_by_cost_then_calls() {
        let mut r = UsageReport { groups: vec![group("mine-1", 9, 1_000_000, None), group("claude-opus-5-5", 2, 1_000_000, None)], ..Default::default() };
        price(&mut r, &PriceOverrides::new());
        sort_groups(&mut r);
        assert_eq!(r.estimated_usd, Some(4.0));
        assert_eq!(r.groups[1].model, "mine-1");
        let mine = PriceOverrides::from([("mine-1".to_string(), Price { input: Some(10.0), ..Default::default() })]);
        price(&mut r, &mine);
        sort_groups(&mut r);
        assert_eq!(r.estimated_usd, Some(14.0));
        assert_eq!(r.groups[0].model, "mine-1");
        let mut server = UsageReport { groups: vec![group("a", 1, 0, Some(1.0)), group("b", 5, 0, Some(1.0)), group("c", 9, 0, None)], ..Default::default() };
        sort_groups(&mut server);
        assert_eq!(server.groups.iter().map(|g| g.model.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
    }

    #[test]
    fn price_draft_reads_and_validates_fields() {
        let mut d = PriceDraft::new("claude-opus-5-5", false, &PriceOverrides::new());
        assert!(d.listed && !d.custom);
        assert_eq!(d.input.text, "4");
        d.input = TextEdit::new("x");
        d.output = TextEdit::new("");
        assert_eq!(d.price(), None);
        d.output = TextEdit::new("2.5");
        assert_eq!(d.price(), Some(Price { input: None, output: Some(2.5), per_minute: None }));
        let mut audio = PriceDraft::new("my-tts", true, &PriceOverrides::new());
        assert!(!audio.listed && audio.price().is_none());
        audio.per_minute = TextEdit::new("0.02");
        assert_eq!(audio.price(), Some(Price { per_minute: Some(0.02), ..Default::default() }));
    }

    #[test]
    fn a_draft_saves_once_title_and_prompt_are_set_and_something_changed() {
        let saved = Transformation { id: "t1".into(), name: "Dense".into(), title: "Dense Summary".into(), prompt: "Sum up".into(), ..Default::default() };
        let mut d = TransformationDraft::from(&saved);
        assert!(!d.can_save());
        d.title = TextEdit::new("Dense summary");
        assert!(d.can_save());
        assert_eq!(d.value().name, "Dense");
        let mut new = TransformationDraft::from(&Transformation::default());
        new.title = TextEdit::new("Key points");
        assert!(!new.can_save());
        new.prompt = TextEdit::new("List them");
        assert!(new.can_save());
        assert_eq!(new.value().name, "Key points");
    }
}
