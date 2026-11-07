//! Ask and Search across notebooks, the Library of every source, Transformations and Usage.

use notebook_engine::citations::{self, CitableSource};
use notebook_engine::types::*;
use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::markdown::{Markdown, MarkdownStyle};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, SegmentedControl, Select, Spinner, Switch, TextArea, TextInput};

use super::icons;
use crate::format;
use crate::pages::{AskMode, MIN_SEARCH_CHARS, PriceDraft, TransformationDraft};
use crate::state::App;
use crate::theme::{self, Look};

/// A centred, scrolling page column with a title and an intro line.
fn page(key: &str, title: &str, intro: &str, body: El, l: &Look) -> El {
    let c = col()
        .w_full()
        .max_w(720.0)
        .gap(16.0)
        .px(l.pad())
        .py(l.pad_top())
        .child(text(title.to_owned()).font(theme::READ_TITLE).color(l.text))
        .child(text(intro.to_owned()).font(l.k.ty.ui).color(l.text2).wrap())
        .child(body);
    col().key(key.to_owned()).w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn hit_row(key: String, title: &str, snippet: &str, l: &Look) -> El {
    let mut c = col()
        .key(key)
        .w_full()
        .gap(3.0)
        .padding(12.0)
        .radius(10.0)
        .border(1.0, l.line)
        .child(text(title.to_owned()).font(l.k.ty.ui_medium).color(l.text).max_lines(1));
    if !snippet.is_empty() {
        c = c.child(text(snippet.to_owned()).font(theme::READ_BODY).color(l.text2).wrap().max_lines(3));
    }
    l.k.interactive(c)
}

// Ask

pub fn ask(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(p) = &s.ask else { return col() };
    let asking = p.mode == AskMode::Ask;
    let mode = SegmentedControl::new(&["Ask", "Search"], Some(if asking { 0 } else { 1 }))
        .label("Mode")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            if let Some(p) = s.ask.as_mut() {
                p.mode = if i == 0 { AskMode::Ask } else { AskMode::Search };
            }
        })
        .build(k)
        .key("ask-mode");
    let mut scopes = vec!["All notebooks".to_owned()];
    scopes.extend(s.notebooks.iter().map(|n| n.name.clone()));
    let ids: Vec<String> = s.notebooks.iter().map(|n| n.id.clone()).collect();
    let current = p.scope.as_ref().and_then(|id| s.notebooks.iter().find(|n| &n.id == id)).map_or("All notebooks", |n| n.name.as_str());
    let scope = Select::new("ask-scope", &scopes)
        .label("Where to look")
        .current(Some(current))
        .width(200.0)
        .open(p.scope_open)
        .on_toggle(|s: &mut App| {
            if let Some(p) = s.ask.as_mut() {
                p.scope_open = !p.scope_open;
            }
        })
        .on_close(|s: &mut App| {
            if let Some(p) = s.ask.as_mut() {
                p.scope_open = false;
            }
        })
        .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
            let names: Vec<String> = s.notebooks.iter().map(|n| n.name.clone()).collect();
            if let Some(p) = s.ask.as_mut() {
                p.scope = names.iter().position(|n| n == name).and_then(|i| ids.get(i).cloned());
                p.scope_open = false;
            }
        })
        .build(k, layer);
    let busy = p.busy.is_some();
    let go = Button::new(if busy {
        if asking { "Researching…" } else { "Searching…" }
    } else if asking {
        "Research"
    } else {
        "Search"
    })
    .variant(ButtonVariant::Contrast)
    .disabled(busy || p.query.text.trim().chars().count() < if asking { 1 } else { MIN_SEARCH_CHARS })
    .on_press(|s: &mut App| s.ask_submit())
    .build(k)
    .key("ask-go");
    let field = TextInput::new(&p.query)
        .width(None)
        .large()
        .placeholder(if asking { "What do my sources say about…" } else { "Words to find" })
        .label(if asking { "Question" } else { "Search" })
        .on_edit(|s: &mut App| &mut s.ask.as_mut().expect("ask").query)
        .on_submit(|s: &mut App, _: &mut Tx| s.ask_submit())
        .build(k)
        .key("ask-query");
    let mut body = col().w_full().gap(14.0).child(field).child(row().w_full().gap(8.0).center_y().child(mode).child(scope).child(el().grow(1.0)).child(go));
    if let Some(e) = &p.error {
        body = body.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    // The research plan, each search ticked off as it is answered.
    for step in &p.steps {
        if let AskStep::Strategy { reasoning, searches } = step {
            let mut plan = col().w_full().gap(6.0).padding(14.0).radius(12.0).fill(l.raised);
            if !reasoning.is_empty() {
                plan = plan.child(text(reasoning.clone()).font(k.ty.ui).color(l.text2).wrap());
            }
            for search in searches {
                let done = p.result.is_some() || p.steps.iter().any(|x| matches!(x, AskStep::Answer { term, .. } if *term == search.term));
                plan = plan.child(text(format!("{} {}", if done { "✓" } else { "·" }, search.term)).font(k.ty.ui).color(if done { l.text } else { l.text3 }));
            }
            body = body.child(plan);
        }
    }
    if busy && asking && p.result.is_none() {
        body = body.child(text(if p.steps.is_empty() { "Planning the research" } else { "Reading what the searches found" }).font(k.ty.ui).color(l.text3));
    }
    if let Some(r) = &p.result {
        let docs: Vec<CitableSource> = r.documents.iter().map(|d| CitableSource { id: d.id.clone(), title: Some(d.title.clone()) }).collect();
        let linked = citations::link_citations(&r.answer, &docs);
        let cited: Vec<(String, Option<String>)> =
            linked.cited.iter().map(|c| (c.id.clone(), r.documents.iter().find(|d| d.id == c.id).and_then(|d| d.notebook_id.clone()))).collect();
        let style = MarkdownStyle::from_kit(k).body_font(theme::READ_BODY).link(l.amber).text(l.text);
        body = body.child(
            Markdown::new(linked.markdown)
                .style(style)
                .selectable(true)
                .on_link_tx(move |s: &mut App, url: &str, tx: &mut Tx| match url.strip_prefix("cite:").and_then(|n| n.parse::<usize>().ok()) {
                    Some(i) => {
                        if let Some((id, nb)) = cited.get(i).cloned() {
                            open_document(s, id, nb, None);
                        }
                    }
                    None => {
                        let _ = tx.open_url(url);
                    }
                })
                .build(k)
                .key("ask-answer"),
        );
        if !r.documents.is_empty() {
            let mut list = col().w_full().gap(8.0).child(text("Documents it drew on").font(k.ty.ui_medium).color(l.text2));
            for d in &r.documents {
                let (id, nb) = (d.id.clone(), d.notebook_id.clone());
                list =
                    list.child(hit_row(format!("doc-{}", d.id), &d.title, "", l).on_press(move |s: &mut App| open_document(s, id.clone(), nb.clone(), None)));
            }
            body = body.child(list);
        }
    }
    if let Some(hits) = &p.hits {
        if hits.is_empty() {
            body = body.child(text("Nothing matched.").font(k.ty.ui).color(l.text3));
        }
        let query = p.query.text.clone();
        for h in hits {
            let (id, nb, kind, q) = (h.id.clone(), h.notebook_id.clone(), h.kind, query.clone());
            body = body.child(hit_row(format!("hit-{}", h.id), &h.title, &h.snippet, l).on_press(move |s: &mut App| match kind {
                HitKind::Source => s.open_source(id.clone(), nb.clone(), Some(q.clone())),
                HitKind::Note => match nb.clone() {
                    Some(nb) => s.open_note(nb, id.clone()),
                    None => open_note(s, None),
                },
            }));
        }
    }
    let intro = if asking {
        "Researches a question across your notebooks: plans searches, reads what they find, then answers with citations."
    } else {
        "Finds sources and notes by meaning and by keyword."
    };
    page("ask", if asking { "Ask your notebooks" } else { "Search your notebooks" }, intro, body, l)
}

/// Ask's documents are sources, or notes with ids starting `note:`.
fn open_document(s: &mut App, id: String, notebook: Option<String>, query: Option<String>) {
    if id.starts_with("note:") {
        match notebook {
            Some(nb) => s.open_note(nb, id),
            None => open_note(s, None),
        }
    } else {
        s.open_source(id, notebook, query);
    }
}

fn open_note(s: &mut App, notebook: Option<String>) {
    if let Some(nb) = notebook {
        s.open_notebook(nb);
        if let Some(o) = s.open.as_mut() {
            o.tab = crate::state::NotebookTab::Notes;
        }
    }
}

// Library

pub fn library(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(p) = &s.library else { return col() };
    let filter = TextInput::new(&p.filter)
        .width(None)
        .placeholder("Filter by title")
        .leading_icon(&rune_kit::glyphs::SEARCH)
        .on_edit(|s: &mut App| &mut s.library.as_mut().expect("library").filter)
        .build(k)
        .key("library-filter");
    let want = p.filter.text.trim().to_lowercase();
    let mut list = col().w_full().gap(8.0);
    if !p.loaded {
        list = list.child(row().gap(8.0).center_y().child(Spinner::new().build(k)).child(text("Loading").font(k.ty.ui).color(l.text3)));
    }
    if let Some(e) = &p.error {
        list = list.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    let shown: Vec<&SourceSummary> = p.sources.iter().filter(|x| want.is_empty() || x.title.as_deref().unwrap_or("").to_lowercase().contains(&want)).collect();
    if p.loaded && shown.is_empty() && p.error.is_none() {
        list = list.child(text(if p.sources.is_empty() { "No sources yet" } else { "No titles match" }).font(k.ty.ui).color(l.text3));
    }
    for src in shown {
        list = list.child(library_row(s, src, p.add_open.as_deref() == Some(src.id.as_str()), l, layer));
    }
    page(
        "library",
        "Library",
        "Every source, in every notebook. A source can be in more than one notebook; removing it from one keeps it here.",
        col().w_full().gap(14.0).child(filter).child(list),
        l,
    )
}

/// "example.com, added 3 days ago, 2 insights".
pub fn source_details(src: &SourceSummary) -> String {
    let mut parts = Vec::new();
    if let Some(url) = src.asset.as_ref().and_then(|a| a.url.as_deref()) {
        parts.push(format::host_of(url));
    }
    parts.push(format!("added {}", format::relative_time(&src.created)));
    match src.insights_count {
        0 => {}
        1 => parts.push("1 insight".into()),
        n => parts.push(format!("{n} insights")),
    }
    parts.join(", ")
}

fn library_row(s: &App, src: &SourceSummary, add_open: bool, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let title = src.title.clone().unwrap_or_else(|| "Untitled source".into());
    let (id, first) = (src.id.clone(), src.notebooks.first().cloned());
    let mut head = k.interactive(col().w_full().gap(3.0).child(text(title).font(k.ty.ui_medium).color(l.text).max_lines(1)));
    if first.is_some() {
        head = head.on_press(move |s: &mut App| s.open_source(id.clone(), first.clone(), None));
    }
    let mut chips = row().w_full().gap(6.0).center_y().flex_wrap();
    let several = src.notebooks.len() > 1;
    for nb in &src.notebooks {
        let name = s.notebooks.iter().find(|n| &n.id == nb).map_or("Notebook", |n| n.name.as_str());
        let (sid, nid) = (src.id.clone(), nb.clone());
        let mut chip = row()
            .key(format!("chip-{nb}"))
            .gap(2.0)
            .center_y()
            .child(
                Button::new(name.to_owned())
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.open_source(sid.clone(), Some(nid.clone()), None))
                    .build(k)
                    .key("open"),
            )
            .radius(8.0)
            .border(1.0, l.line);
        if several {
            let (sid, nid) = (src.id.clone(), nb.clone());
            chip = chip.child(
                IconButton::new(k.glyph.close)
                    .variant(IconButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .label(format!("Remove from {name}"))
                    .on_press(move |s: &mut App| s.link_source(sid.clone(), nid.clone(), false))
                    .build(k)
                    .key("unlink"),
            );
        }
        chips = chips.child(chip);
    }
    let addable: Vec<&notebook_engine::types::Notebook> = s.notebooks.iter().filter(|n| !src.notebooks.contains(&n.id)).collect();
    if !addable.is_empty() {
        let names: Vec<String> = addable.iter().map(|n| n.name.clone()).collect();
        let ids: Vec<String> = addable.iter().map(|n| n.id.clone()).collect();
        let (toggle_id, sid) = (src.id.clone(), src.id.clone());
        chips = chips.child(
            Select::new(format!("add-{}", src.id), &names)
                .label("Add to notebook")
                .placeholder("Add to")
                .current(None)
                .width(150.0)
                .open(add_open)
                .on_toggle(move |s: &mut App| {
                    if let Some(p) = s.library.as_mut() {
                        p.add_open = if p.add_open.as_deref() == Some(toggle_id.as_str()) { None } else { Some(toggle_id.clone()) };
                    }
                })
                .on_close(|s: &mut App| {
                    if let Some(p) = s.library.as_mut() {
                        p.add_open = None;
                    }
                })
                .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
                    if let Some(nb) = names.iter().position(|n| n == name).and_then(|i| ids.get(i).cloned()) {
                        s.link_source(sid.clone(), nb, true);
                    }
                })
                .build(k, layer),
        );
    }
    col()
        .key(format!("lib-{}", src.id))
        .w_full()
        .gap(6.0)
        .padding(12.0)
        .radius(10.0)
        .border(1.0, l.line)
        .child(head)
        .child(text(source_details(src)).font(k.ty.small).color(l.text3).max_lines(1))
        .child(chips)
}

// Transformations

pub fn transformations(s: &App, l: &Look) -> El {
    let k = &l.k;
    let Some(p) = &s.transformations else { return col() };
    let mut body = col().w_full().gap(14.0);
    if let Some(e) = &p.error {
        body = body.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    body = body.child(
        Button::new("New transformation")
            .icon(&icons::PLUS)
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut App| {
                if let Some(p) = s.transformations.as_mut() {
                    p.editing = Some(TransformationDraft::from(&Transformation::default()));
                }
            })
            .build(k)
            .key("new-transformation"),
    );
    if let Some(d) = &p.editing {
        body = body.child(transformation_editor(d, l));
    }
    for t in &p.list {
        if p.editing.as_ref().is_some_and(|d| d.id == t.id) {
            continue;
        }
        let (edit, del) = (t.clone(), t.id.clone());
        let mut head =
            row().w_full().gap(8.0).center_y().child(text(t.title.clone()).font(theme::READ_HEADING).color(l.text).grow(1.0).min_w(0.0).max_lines(1));
        if t.apply_default {
            head = head.child(text("Runs on new sources").font(k.ty.small).color(l.text3));
        }
        head = head.child(
            IconButton::new(&icons::TRASH)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Delete transformation")
                .on_press(move |s: &mut App| {
                    let id = del.clone();
                    s.ask("Delete this transformation?", move |s| s.delete_transformation(id.clone()));
                })
                .build(k)
                .key("delete"),
        );
        let mut card = col().key(format!("t-{}", t.id)).w_full().gap(6.0).padding(14.0).radius(12.0).border(1.0, l.line).child(head);
        if !t.description.is_empty() {
            card = card.child(text(t.description.clone()).font(k.ty.ui).color(l.text2).wrap());
        }
        body = body.child(k.interactive(card).on_press(move |s: &mut App| {
            if let Some(p) = s.transformations.as_mut() {
                p.editing = Some(TransformationDraft::from(&edit));
            }
        }));
    }
    body = body.child(
        col()
            .w_full()
            .gap(8.0)
            .pt(10.0)
            .child(text("Instructions before every transformation").font(k.ty.ui_medium).color(l.text))
            .child(
                TextArea::new(&p.instructions)
                    .width(None)
                    .auto_grow(3, 12)
                    .on_edit(|s: &mut App| {
                        let p = s.transformations.as_mut().expect("transformations");
                        p.instructions_saved = false;
                        &mut p.instructions
                    })
                    .build(k)
                    .key("instructions"),
            )
            .child(
                row()
                    .gap(10.0)
                    .center_y()
                    .child(Button::new("Save instructions").on_press(|s: &mut App| s.save_instructions()).build(k).key("save-instructions"))
                    .child(text(if p.instructions_saved { "Saved" } else { "" }).font(k.ty.ui).color(l.text3)),
            ),
    );
    page("transformations", "Transformations", "Prompts that turn a source into an insight: a summary, key points, questions. Run them from a source.", body, l)
}

fn draft(s: &mut App) -> &mut TransformationDraft {
    s.transformations.as_mut().and_then(|p| p.editing.as_mut()).expect("editing")
}

fn transformation_editor(d: &TransformationDraft, l: &Look) -> El {
    let k = &l.k;
    let field = |label: &str, el: El| col().w_full().gap(4.0).child(text(label.to_owned()).font(k.ty.small).color(l.text2)).child(el);
    let mut c = col()
        .key("t-editor")
        .w_full()
        .gap(10.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .child(field(
            "Title",
            TextInput::new(&d.title).width(None).placeholder("For example Key arguments").on_edit(|s: &mut App| &mut draft(s).title).build(k).key("t-title"),
        ))
        .child(field(
            "What it does",
            TextInput::new(&d.description)
                .width(None)
                .placeholder("Shown as a hint when choosing it")
                .on_edit(|s: &mut App| &mut draft(s).description)
                .build(k)
                .key("t-description"),
        ))
        .child(field(
            "Prompt",
            TextArea::new(&d.prompt)
                .width(None)
                .auto_grow(4, 16)
                .placeholder("Tell the model what to produce from the source text")
                .on_edit(|s: &mut App| &mut draft(s).prompt)
                .build(k)
                .key("t-prompt"),
        ));
    if let Some(e) = &d.error {
        c = c.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    c = c.child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(Switch::new(d.apply_default).label("Run on new sources").on_toggle(|s: &mut App| draft(s).apply_default ^= true).build(k).key("t-default"))
            .child(el().grow(1.0))
            .child(
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut App| {
                        if let Some(p) = s.transformations.as_mut() {
                            p.editing = None;
                        }
                    })
                    .build(k)
                    .key("t-cancel"),
            )
            .child(
                Button::new(if d.saving {
                    "Saving"
                } else if d.id.is_empty() {
                    "Create transformation"
                } else {
                    "Save changes"
                })
                .variant(ButtonVariant::Contrast)
                .disabled(!d.can_save())
                .on_press(|s: &mut App| s.save_transformation())
                .build(k)
                .key("t-save"),
            ),
    );
    if !d.id.is_empty() {
        c = c.child(try_it(d, l));
    }
    c
}

/// Runs the saved transformation on pasted text.
fn try_it(d: &TransformationDraft, l: &Look) -> El {
    let k = &l.k;
    let run = Button::new(if d.running { "Running" } else { "Run" })
        .icon(&icons::PLAY)
        .disabled(d.running || d.sample.text.trim().is_empty())
        .on_press(|s: &mut App| s.try_transformation());
    let mut c = col()
        .key("t-try")
        .w_full()
        .gap(8.0)
        .pt(8.0)
        .child(text("Try it").font(k.ty.ui_medium).color(l.text))
        .child(
            TextArea::new(&d.sample)
                .width(None)
                .auto_grow(4, 10)
                .placeholder("Paste some text to run this transformation on")
                .on_edit(|s: &mut App| &mut draft(s).sample)
                .build(k)
                .key("t-sample"),
        )
        .child(row().w_full().justify_end().child(run.build(k).key("t-run")));
    match &d.output {
        Some(Ok(out)) => {
            let style = MarkdownStyle::from_kit(k).body_font(theme::READ_BODY).link(l.amber).text(l.text);
            c = c.child(
                col()
                    .w_full()
                    .padding(14.0)
                    .radius(10.0)
                    .fill(l.raised)
                    .child(Markdown::new(out.clone()).style(style).selectable(true).build(k).key("t-output")),
            );
        }
        Some(Err(e)) => c = c.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap()),
        None => {}
    }
    c
}

// Usage

/// Readable names for the usage log's feature keys.
pub fn feature_label(feature: &str) -> &str {
    match feature {
        "chat" => "Chat",
        "ask" => "Ask",
        "search" => "Search",
        "embedding" => "Indexing sources",
        "transformation" | "insights" => "Insights",
        "study" => "Study decks",
        "podcast_writing" => "Podcast scripts",
        "podcast_research" => "Podcast study notes",
        "podcast_check" => "Podcast claim checks",
        "podcast_audio" => "Podcast voices",
        "podcast_live_audio" => "Podcast voices (Live)",
        "transcription" => "Transcription",
        "live_talk" => "Live talk, voice minutes",
        other => other,
    }
}

/// Dollars with cents; amounts under a cent read "<$0.01".
pub fn money(usd: f64) -> String {
    if usd > 0.0 && usd < 0.01 { "<$0.01".into() } else { format!("${usd:.2}") }
}

/// 90 → "2 min", 40 → "40 s".
pub fn audio_time(seconds: f64) -> String {
    if seconds >= 60.0 { format!("{} min", grouped((seconds / 60.0).round() as u64)) } else { format!("{} s", seconds.round() as u64) }
}

pub fn usage(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(p) = &s.usage else { return col() };
    let local = s.backend.as_ref().is_none_or(|b| b.kind() == notebook_engine::BackendKind::Local);
    const DAYS: [u32; 3] = [7, 30, 90];
    let range = SegmentedControl::new(&["7 days", "30 days", "90 days"], DAYS.iter().position(|d| *d == p.days))
        .label("Period")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| s.load_usage(DAYS[i]))
        .build(k)
        .key("usage-days");
    let mut body = col().w_full().gap(14.0).child(row().child(range));
    match (&p.report, &p.error) {
        (_, Some(e)) => body = body.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap()),
        (None, None) => body = body.child(row().gap(8.0).center_y().child(Spinner::new().build(k)).child(text("Loading").font(k.ty.ui).color(l.text3))),
        (Some(r), None) => {
            let mut unpriced: Vec<&str> = Vec::new();
            for g in r.groups.iter().filter(|g| g.estimated_usd.is_none()) {
                if !unpriced.contains(&g.model.as_str()) {
                    unpriced.push(&g.model);
                }
            }
            let mut caption = format!("Estimated cost, last {} days", p.days);
            if r.estimated_usd.is_some() && !unpriced.is_empty() {
                let what = if unpriced.len() == 1 { unpriced[0].to_owned() } else { format!("{} models", unpriced.len()) };
                caption += &format!(", not counting {what} without a price");
            }
            let tokens: u64 = r.groups.iter().map(|g| g.input_tokens + g.output_tokens).sum();
            let audio: f64 = r.groups.iter().map(|g| g.audio_seconds).sum();
            let mut facts = format!("{} calls, {} {}", grouped(r.calls), grouped(tokens), if r.estimated { "tokens (estimated)" } else { "tokens" });
            if audio > 0.0 {
                facts += &format!(" and {} of audio", audio_time(audio));
            }
            body = body.child(
                col()
                    .w_full()
                    .gap(2.0)
                    .child(text(r.estimated_usd.map_or_else(|| "No estimate".into(), |usd| format!("${usd:.2}"))).font(theme::READ_TITLE).color(l.text))
                    .child(text(caption).font(k.ty.ui).color(l.text2).wrap())
                    .child(text(facts).font(k.ty.small).color(l.text3).wrap()),
            );
            if !r.groups.is_empty() && !l.phone() {
                let head = |t: &str| text(t.to_owned()).font(k.ty.small).color(l.text3);
                body = body.child(
                    row()
                        .w_full()
                        .gap(12.0)
                        .child(head("Used for").grow(1.0).min_w(0.0))
                        .child(head("Calls").w(56.0).align(rune::Align::End))
                        .child(head("Tokens in").w(84.0).align(rune::Align::End))
                        .child(head("Tokens out").w(96.0).align(rune::Align::End))
                        .child(head("Cost").w(72.0).align(rune::Align::End)),
                );
            }
            let max = r.groups.iter().map(|g| g.input_tokens + g.output_tokens).max().unwrap_or(1).max(1) as f32;
            // The row that cost the most leads in ink; the rest stay quiet.
            let top = r
                .groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.estimated_usd.is_some_and(|c| c > 0.0))
                .max_by(|a, b| a.1.estimated_usd.partial_cmp(&b.1.estimated_usd).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i);
            for (gi, g) in r.groups.iter().enumerate() {
                let used = g.input_tokens + g.output_tokens;
                let audio = g.audio_seconds > 0.0;
                let num = |t: String, w: f32| text(t).font(theme::tnum(k.ty.ui)).color(l.text2).w(w).align(rune::Align::End).max_lines(1);
                let cost: El = if local {
                    let (model, label) = (g.model.clone(), g.estimated_usd.map_or_else(|| "Set price".into(), money));
                    row().w(72.0).justify_end().child(
                        Button::new(label)
                            .variant(ButtonVariant::Ghost)
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut App| s.edit_price(&model, audio))
                            .build(k)
                            .key("price"),
                    )
                } else {
                    num(g.estimated_usd.map(money).unwrap_or_default(), 72.0)
                };
                body = body.child(
                    col()
                        .key(format!("u-{}-{}", g.model, g.feature))
                        .w_full()
                        .gap(4.0)
                        .child(
                            row()
                                .w_full()
                                .gap(12.0)
                                .center_y()
                                .child(
                                    col()
                                        .grow(1.0)
                                        .min_w(0.0)
                                        .child(text(feature_label(&g.feature).to_owned()).font(k.ty.ui_medium).color(l.text).max_lines(1))
                                        .child(text(g.model.clone()).font(k.ty.small).color(l.text3).max_lines(1)),
                                )
                                .children(if l.phone() {
                                    Vec::new()
                                } else {
                                    vec![
                                        num(grouped(g.calls), 56.0),
                                        num(if audio { String::new() } else { grouped(g.input_tokens) }, 84.0),
                                        num(if audio { format!("{} audio", audio_time(g.audio_seconds)) } else { grouped(g.output_tokens) }, 96.0),
                                    ]
                                })
                                .child(cost),
                        )
                        .children(l.phone().then(|| {
                            // A phone has no room for columns: the figures go on a line of their own.
                            let used = if audio {
                                format!("{} calls, {} audio", grouped(g.calls), audio_time(g.audio_seconds))
                            } else {
                                format!("{} calls, {} in, {} out", grouped(g.calls), grouped(g.input_tokens), grouped(g.output_tokens))
                            };
                            text(used).font(theme::tnum(k.ty.small)).color(l.text3)
                        }))
                        .child(
                            el().w_full().h(4.0).radius(2.0).fill(l.raised).child(el().w_frac(used as f32 / max).h(4.0).radius(2.0).fill(if top == Some(gi) {
                                l.text
                            } else {
                                l.line_strong
                            })),
                        ),
                );
            }
            if r.groups.is_empty() {
                body = body.child(text("Nothing in this period").font(k.ty.ui).color(l.text3));
            }
        }
    }
    if let Some(d) = &p.pricing {
        price_dialog(d, l, layer);
    }
    let intro = if local {
        format!(
            "What model calls from {} cost, at list prices or your own. Tokens are estimated from text length, so treat totals as close, not exact.",
            theme::THIS_DEVICE
        )
    } else {
        "What the server spent on models, with its price estimates.".to_owned()
    };
    page("usage", "Usage", &intro, body, l)
}

fn pricing(s: &mut App) -> &mut PriceDraft {
    s.usage.as_mut().and_then(|p| p.pricing.as_mut()).expect("price dialog")
}

/// Edit what a model costs, per million tokens or per minute of audio.
fn price_dialog(d: &PriceDraft, l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    let field = |label: &str, edit: &rune::TextEdit, placeholder: &str, key: &str, at: fn(&mut App) -> &mut rune::TextEdit| {
        col().grow(1.0).min_w(0.0).gap(4.0).child(text(label.to_owned()).font(k.ty.small).color(l.text2)).child(
            TextInput::new(edit)
                .width(None)
                .placeholder(placeholder.to_owned())
                .on_edit(at)
                .on_submit(|s: &mut App, _: &mut Tx| {
                    let price = s.usage.as_ref().and_then(|p| p.pricing.as_ref()).and_then(|d| d.price());
                    if price.is_some() {
                        s.save_price(price);
                    }
                })
                .build(k)
                .key(key.to_owned()),
        )
    };
    let fields = if d.audio {
        row().w_full().child(field("Dollars per minute of audio", &d.per_minute, "0.015", "price-minute", |s| &mut pricing(s).per_minute))
    } else {
        row().w_full().gap(10.0).child(field("Input, $ per million tokens", &d.input, "2.00", "price-input", |s| &mut pricing(s).input)).child(field(
            "Output, $ per million tokens",
            &d.output,
            "10.00",
            "price-output",
            |s| &mut pricing(s).output,
        ))
    };
    let next = d.price();
    let mut buttons = row().w_full().gap(8.0).center_y();
    if d.custom {
        buttons = buttons.child(
            Button::new(if d.listed { "Use list price" } else { "Clear price" })
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.save_price(None))
                .build(k)
                .key("price-clear"),
        );
    }
    buttons = buttons
        .child(el().grow(1.0))
        .child(
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| {
                    if let Some(p) = s.usage.as_mut() {
                        p.pricing = None;
                    }
                })
                .build(k)
                .key("price-cancel"),
        )
        .child(
            Button::new("Save price")
                .variant(ButtonVariant::Contrast)
                .disabled(next.is_none())
                .on_press(move |s: &mut App| s.save_price(next))
                .build(k)
                .key("price-save"),
        );
    Dialog::new("price", format!("Price for {}", d.model))
        .open(true)
        .width(l.dialog(460.0))
        .fit()
        .child(
            col()
                .w_full()
                .gap(14.0)
                .px(16.0)
                .pb(16.0)
                .child(
                    text(format!("Copy the rate from your provider's pricing page. It applies to every use of this model on {}.", theme::THIS_DEVICE))
                        .font(k.ty.ui)
                        .color(l.text2)
                        .wrap(),
                )
                .child(fields)
                .child(buttons),
        )
        .on_close(|s: &mut App| {
            if let Some(p) = s.usage.as_mut() {
                p.pricing = None;
            }
        })
        .build(k, layer);
}

/// 432000 → "432,000".
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_thousands() {
        assert_eq!(super::grouped(0), "0");
        assert_eq!(super::grouped(999), "999");
        assert_eq!(super::grouped(432_000), "432,000");
        assert_eq!(super::grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn money_labels_and_audio_time() {
        assert_eq!(super::money(0.004), "<$0.01");
        assert_eq!(super::money(0.0), "$0.00");
        assert_eq!(super::money(1.926), "$1.93");
        assert_eq!(super::audio_time(40.0), "40 s");
        assert_eq!(super::audio_time(754.0), "13 min");
        assert_eq!(super::feature_label("podcast_research"), "Podcast study notes");
        assert_eq!(super::feature_label("new_thing"), "new_thing");
    }
}
