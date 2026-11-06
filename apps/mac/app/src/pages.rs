//! State and actions for the pages away from a notebook: Ask, Library, Transformations, Usage.

use notebook_engine::Cancel;
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

#[derive(Default)]
pub struct LibraryPage {
    pub sources: Vec<SourceSummary>,
    pub filter: TextEdit,
    pub loaded: bool,
}

pub struct TransformationDraft {
    pub id: String,
    pub name: TextEdit,
    pub title: TextEdit,
    pub description: TextEdit,
    pub prompt: TextEdit,
    pub apply_default: bool,
}

impl TransformationDraft {
    pub fn from(t: &Transformation) -> Self {
        TransformationDraft {
            id: t.id.clone(),
            name: TextEdit::new(t.name.clone()),
            title: TextEdit::new(t.title.clone()),
            description: TextEdit::new(t.description.clone()),
            prompt: TextEdit::new(t.prompt.clone()),
            apply_default: t.apply_default,
        }
    }
}

#[derive(Default)]
pub struct TransformationsPage {
    pub list: Vec<Transformation>,
    pub editing: Option<TransformationDraft>,
    pub instructions: TextEdit,
    pub instructions_saved: bool,
}

#[derive(Default)]
pub struct UsagePage {
    pub days: u32,
    pub report: Option<UsageReport>,
}

impl App {
    pub fn go(&mut self, route: Route) {
        match route {
            Route::Ask if self.ask.is_none() => self.ask = Some(AskPage::default()),
            Route::Library => {
                self.library.get_or_insert_with(LibraryPage::default);
                self.run(
                    |b| b.list_all_sources(),
                    |app, r| match r {
                        Ok(sources) => {
                            if let Some(p) = app.library.as_mut() {
                                p.sources = sources;
                                p.loaded = true;
                            }
                        }
                        Err(e) => app.fail(e),
                    },
                );
            }
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
        if query.is_empty() || p.busy.is_some() {
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
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn save_transformation(&mut self) {
        let Some(d) = self.transformations.as_mut().and_then(|p| p.editing.take()) else { return };
        let name = d.name.text.trim().to_owned();
        let t = Transformation {
            id: d.id,
            title: Some(d.title.text.trim().to_owned()).filter(|t| !t.is_empty()).unwrap_or_else(|| name.clone()),
            name,
            description: d.description.text.trim().to_owned(),
            prompt: d.prompt.text.clone(),
            apply_default: d.apply_default,
        };
        self.run(
            move |b| b.save_transformation(t),
            |app, r| match r {
                Ok(_) => app.load_transformations(),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn delete_transformation(&mut self, id: String) {
        self.run(
            move |b| b.delete_transformation(&id),
            |app, r| match r {
                Ok(()) => app.load_transformations(),
                Err(e) => app.fail(e),
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
        self.run(
            move |b| {
                let mut report = b.usage(days)?;
                if b.kind() == notebook_engine::BackendKind::Local {
                    price(&mut report);
                }
                Ok(report)
            },
            move |app, r| match r {
                Ok(report) => {
                    if let Some(p) = app.usage.as_mut().filter(|p| p.days == days) {
                        p.report = Some(report);
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }
}

/// On device, cost comes from list prices (the server reports its own). Models without a
/// price stay unpriced and out of the total.
fn price(report: &mut UsageReport) {
    let overrides = notebook_engine::pricing::PriceOverrides::new();
    let mut total = None;
    for g in &mut report.groups {
        g.estimated_usd = notebook_engine::pricing::cost_of(&g.model, g.input_tokens, g.output_tokens, g.audio_seconds, &overrides);
        if let Some(c) = g.estimated_usd {
            *total.get_or_insert(0.0) += c;
        }
    }
    report.estimated_usd = total;
    report.groups.sort_by(|a, b| b.estimated_usd.unwrap_or(-1.0).total_cmp(&a.estimated_usd.unwrap_or(-1.0)).then(b.calls.cmp(&a.calls)));
}
