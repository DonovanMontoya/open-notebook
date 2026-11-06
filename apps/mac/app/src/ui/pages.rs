//! Ask and Search across notebooks, the Library of every source, Transformations and Usage.

use notebook_engine::citations::{self, CitableSource};
use notebook_engine::types::*;
use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::markdown::{Markdown, MarkdownStyle};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, SegmentedControl, Select, Switch, TextArea, TextInput};

use super::icons;
use crate::pages::{AskMode, TransformationDraft};
use crate::state::App;
use crate::theme::{self, Look};

/// A centred, scrolling page column with a title and an intro line.
fn page(key: &str, title: &str, intro: &str, body: El, l: &Look) -> El {
    let c = col()
        .w_full()
        .max_w(720.0)
        .gap(16.0)
        .px(24.0)
        .py(28.0)
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
        .build(k, layer)
        .key("ask-scope");
    let busy = p.busy.is_some();
    let go = Button::new(if busy {
        if asking { "Researching…" } else { "Searching…" }
    } else if asking {
        "Ask"
    } else {
        "Search"
    })
    .variant(ButtonVariant::Contrast)
    .disabled(busy || p.query.text.trim().is_empty())
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
                HitKind::Note => open_note(s, nb.clone()),
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
        open_note(s, notebook);
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

pub fn library(s: &App, l: &Look) -> El {
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
    let shown: Vec<&SourceSummary> = p.sources.iter().filter(|x| want.is_empty() || x.title.as_deref().unwrap_or("").to_lowercase().contains(&want)).collect();
    if p.loaded && shown.is_empty() {
        list = list.child(text(if p.sources.is_empty() { "No sources yet." } else { "No titles match." }).font(k.ty.ui).color(l.text3));
    }
    for src in shown {
        let names: Vec<&str> = src.notebooks.iter().filter_map(|id| s.notebooks.iter().find(|n| &n.id == id)).map(|n| n.name.as_str()).collect();
        let (id, nb) = (src.id.clone(), src.notebooks.first().cloned());
        let title = src.title.clone().unwrap_or_else(|| "Untitled source".into());
        list = list
            .child(hit_row(format!("lib-{}", src.id), &title, &names.join(" · "), l).on_press(move |s: &mut App| s.open_source(id.clone(), nb.clone(), None)));
    }
    page(
        "library",
        "Library",
        "Every source, in every notebook. A source can belong to several notebooks.",
        col().w_full().gap(14.0).child(filter).child(list),
        l,
    )
}

// Transformations

pub fn transformations(s: &App, l: &Look) -> El {
    let k = &l.k;
    let Some(p) = &s.transformations else { return col() };
    let mut body = col().w_full().gap(14.0);
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

fn transformation_editor(d: &TransformationDraft, l: &Look) -> El {
    let k = &l.k;
    fn draft(s: &mut App) -> &mut TransformationDraft {
        s.transformations.as_mut().and_then(|p| p.editing.as_mut()).expect("editing")
    }
    let field = |label: &str, el: El| col().w_full().gap(4.0).child(text(label.to_owned()).font(k.ty.small).color(l.text2)).child(el);
    col()
        .key("t-editor")
        .w_full()
        .gap(10.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .child(field("Name", TextInput::new(&d.name).width(None).on_edit(|s: &mut App| &mut draft(s).name).build(k).key("t-name")))
        .child(field(
            "Insight title",
            TextInput::new(&d.title).width(None).placeholder("Shown on the insight").on_edit(|s: &mut App| &mut draft(s).title).build(k).key("t-title"),
        ))
        .child(field("Description", TextInput::new(&d.description).width(None).on_edit(|s: &mut App| &mut draft(s).description).build(k).key("t-description")))
        .child(field("Prompt", TextArea::new(&d.prompt).width(None).auto_grow(4, 16).on_edit(|s: &mut App| &mut draft(s).prompt).build(k).key("t-prompt")))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    Switch::new(d.apply_default).label("Run on new sources").on_toggle(|s: &mut App| draft(s).apply_default ^= true).build(k).key("t-default"),
                )
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
                    Button::new("Save")
                        .variant(ButtonVariant::Contrast)
                        .disabled(d.name.text.trim().is_empty() || d.prompt.text.trim().is_empty())
                        .on_press(|s: &mut App| s.save_transformation())
                        .build(k)
                        .key("t-save"),
                ),
        )
}

// Usage

pub fn usage(s: &App, l: &Look) -> El {
    let k = &l.k;
    let Some(p) = &s.usage else { return col() };
    const DAYS: [u32; 3] = [7, 30, 90];
    let range = SegmentedControl::new(&["7 days", "30 days", "90 days"], DAYS.iter().position(|d| *d == p.days))
        .label("Period")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| s.load_usage(DAYS[i]))
        .build(k)
        .key("usage-days");
    let mut body = col().w_full().gap(14.0).child(range);
    match &p.report {
        None => body = body.child(text("Loading").font(k.ty.ui).color(l.text3)),
        Some(r) => {
            let cost = r.estimated_usd.map_or_else(|| "cost unknown".into(), |usd| format!("about ${usd:.2}"));
            body = body.child(text(format!("{} calls, {cost}", grouped(r.calls))).font(theme::READ_HEADING).color(l.text));
            if r.estimated {
                body = body.child(text("Token counts on this Mac are estimated from text length.").font(k.ty.small).color(l.text3));
            }
            let max = r.groups.iter().map(|g| g.input_tokens + g.output_tokens).max().unwrap_or(1).max(1) as f32;
            for g in &r.groups {
                let tokens = g.input_tokens + g.output_tokens;
                let mut detail = format!("{} calls · {} tokens", grouped(g.calls), grouped(tokens));
                if g.audio_seconds > 0.0 {
                    detail += &format!(" · {:.0} min audio", g.audio_seconds / 60.0);
                }
                if let Some(usd) = g.estimated_usd {
                    detail += &format!(" · ${usd:.2}");
                }
                body = body.child(
                    col()
                        .key(format!("u-{}-{}", g.model, g.feature))
                        .w_full()
                        .gap(4.0)
                        .child(
                            row()
                                .w_full()
                                .justify_between()
                                .child(text(format!("{} · {}", g.feature, g.model)).font(k.ty.ui_medium).color(l.text))
                                .child(text(detail).font(k.ty.small).color(l.text2)),
                        )
                        .child(el().w_full().h(6.0).radius(3.0).fill(l.raised).child(el().w_frac(tokens as f32 / max).h(6.0).radius(3.0).fill(l.text3))),
                );
            }
            if r.groups.is_empty() {
                body = body.child(text("Nothing used in this period.").font(k.ty.ui).color(l.text3));
            }
        }
    }
    page("usage", "Usage", "Calls to AI providers by feature and model, with an estimated cost from list prices.", body, l)
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
}
