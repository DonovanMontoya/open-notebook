//! The add-source sheet: web pages, files, pasted text or sources from other notebooks, and
//! the list of how each one went once Add is pressed.

use rune::file_dialog::FileDialogOptions;
use rune::{El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Dialog, IconButton, IconButtonVariant, SegmentedControl, TextArea, TextInput};

use super::icons;
use crate::state::{AddKind, AddSource, App, BatchItem, ItemState, Open};
use crate::theme::Look;

const KINDS: [AddKind; 4] = [AddKind::Link, AddKind::File, AddKind::Text, AddKind::Library];

fn add(s: &mut App) -> &mut AddSource {
    s.open.as_mut().and_then(|o| o.add.as_mut()).expect("adding")
}

pub fn sheet(o: &Open, a: &AddSource, l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    if let Some(items) = &a.items {
        return batch(a, items, l, layer);
    }
    let kinds = SegmentedControl::new(&["Web pages", "Files", "Text", "Other notebooks"], KINDS.iter().position(|x| *x == a.kind))
        .label("Source type")
        .width(None)
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            let a = add(s);
            a.kind = KINDS[i];
            a.error = None;
            if a.kind == AddKind::Library && a.library.is_none() {
                s.load_add_library();
            }
        })
        .build(k)
        .key("add-kind");
    let count = a.count(&o.id);
    let mut body = col().w_full().gap(12.0).child(kinds);
    match a.kind {
        AddKind::Link => {
            let links = notebook_engine::text::parse_links(&a.url.text).len();
            let help = if links > 1 {
                format!("{links} links found. Each becomes its own source.")
            } else {
                "Paste one or more links, one per line. Articles, PDF links and YouTube videos (from their captions) all work.".into()
            };
            body = body.child(
                TextArea::new(&a.url)
                    .placeholder("https://www.ftc.gov/reports/bringing-dark-patterns-light\nhttps://arxiv.org/pdf/2203.02155")
                    .label("Addresses")
                    .width(None)
                    .rows(5)
                    .on_edit(move |s: &mut App| &mut add(s).url)
                    .build(k)
                    .key("add-url"),
            );
            body = body.child(text(help).font(k.ty.small).color(l.text3).wrap());
        }
        AddKind::File => body = body.child(files(a, l)),
        AddKind::Text => {
            body = body.child(
                TextArea::new(&a.text)
                    .width(None)
                    .placeholder("Paste a transcript, an email or your own notes")
                    .label("Text")
                    .rows(8)
                    .on_edit(move |s: &mut App| &mut add(s).text)
                    .build(k)
                    .key("add-text"),
            );
        }
        AddKind::Library => body = body.child(library(o, a, l)),
    }
    if count <= 1 && a.kind != AddKind::Library {
        let hint = if a.kind == AddKind::Text { "Title (optional, taken from the first line)" } else { "Title (optional, taken from the source)" };
        body = body.child(
            TextInput::new(&a.title)
                .width(None)
                .label("Title")
                .placeholder(hint)
                .on_edit(move |s: &mut App| &mut add(s).title)
                .on_submit(|s: &mut App, _: &mut Tx| s.submit_add())
                .build(k)
                .key("add-title"),
        );
    }
    if !o.transformations.is_empty() && a.kind != AddKind::Library {
        body = body.child(insight_picks(o, a, l));
    }
    if let Some(e) = &a.error {
        body = body.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    let label = if count > 1 { format!("Add {count} sources") } else { "Add source".into() };
    body =
        body.child(
            row().w_full().gap(8.0).justify_end().child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(close).build(k).key("add-cancel")).child(
                Button::new(label).variant(ButtonVariant::Contrast).disabled(count == 0).on_press(|s: &mut App| s.submit_add()).build(k).key("add-submit"),
            ),
        );
    Dialog::new("add-source", "Add sources").open(true).width(520.0).fit().child(body.px(20.0).pb(20.0)).on_close(close).build(k, layer);
}

/// The chosen files, a button to choose more, and a drop target for them.
fn files(a: &AddSource, l: &Look) -> El {
    let k = &l.k;
    let pick = row()
        .w_full()
        .gap(10.0)
        .center_y()
        .child(Button::new("Choose file").control_size(ControlSize::Small).on_press_tx(|_: &mut App, tx: &mut Tx| choose_file(tx)).build(k).key("add-choose"))
        .child(
            text("or drop files here. PDF, Word, PowerPoint, Excel, EPUB, Markdown, text, HTML, audio and video.")
                .font(k.ty.small)
                .color(l.text3)
                .wrap()
                .shrink()
                .min_w(0.0),
        );
    let mut list = col().key("file-list").w_full().gap(2.0);
    for (i, (path, size)) in a.files.iter().enumerate() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        list = list.child(
            row()
                .key(format!("file-{i}"))
                .w_full()
                .gap(8.0)
                .py(2.0)
                .center_y()
                .child(rune::icon(&icons::DOC, 14.0).color(l.text3))
                .child(text(name.clone()).font(k.ty.ui).color(l.text).max_lines(1).grow(1.0).min_w(0.0))
                .child(text(crate::format::file_size(*size)).font(k.ty.small).color(l.text3))
                .child(
                    IconButton::new(&icons::CLOSE)
                        .variant(IconButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .label(format!("Remove {name}"))
                        .on_press(move |s: &mut App| {
                            let a = add(s);
                            if i < a.files.len() {
                                a.files.remove(i);
                            }
                        })
                        .build(k)
                        .key("remove"),
                ),
        );
    }
    col()
        .key("add-drop")
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .radius(10.0)
        .border(1.0, l.line_strong)
        .child(pick)
        .child(list)
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| add(s).add_files(files))
        .accepts_extensions(&crate::state::accepted())
}

fn choose_file(tx: &mut Tx) {
    let options = FileDialogOptions { title: "Add a source".into(), ..Default::default() }.filter("Documents, audio and video", &crate::state::accepted());
    tx.open_file(options, |s: &mut App, result, _| match result {
        Ok(Some(path)) => add(s).add_files(&[path]),
        Ok(None) => {}
        Err(e) => add(s).error = Some(e.to_string()),
    });
}

/// A checklist of sources in other notebooks.
fn library(o: &Open, a: &AddSource, l: &Look) -> El {
    let k = &l.k;
    let mut c = col().key("add-library").w_full().gap(4.0).child(text("Sources in your other notebooks").font(k.ty.ui_medium).color(l.text2));
    let Some(_) = &a.library else {
        return c.child(rune_kit::Spinner::new().size(16.0).build(k));
    };
    let mut any = false;
    let mut list = col().w_full().gap(2.0);
    for src in a.elsewhere(&o.id) {
        any = true;
        let id = src.id.clone();
        list = list.child(
            Checkbox::new(a.picked.contains(&src.id))
                .label(src.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled source".into()))
                .on_toggle(move |s: &mut App| {
                    let a = add(s);
                    if !a.picked.remove(&id) {
                        a.picked.insert(id.clone());
                    }
                })
                .build(k)
                .key(format!("pick-{}", src.id)),
        );
    }
    if !any {
        c = c.child(text("Every source is already in this notebook.").font(k.ty.small).color(l.text3));
    }
    c.child(list.scroll(rune::ScrollSpec { stick_bottom: false }).max_h(260.0))
}

/// Which transformations run on the new sources, preselected from their defaults.
fn insight_picks(o: &Open, a: &AddSource, l: &Look) -> El {
    let k = &l.k;
    let selected = a.selected(&o.transformations);
    let mut checks = row().w_full().gap(14.0).flex_wrap();
    for t in &o.transformations {
        let id = t.id.clone();
        checks = checks.child(
            Checkbox::new(selected.contains(&t.id))
                .label(t.title.clone())
                .on_toggle(move |s: &mut App| {
                    let Some(o) = s.open.as_mut() else { return };
                    let all = o.transformations.clone();
                    let Some(a) = o.add.as_mut() else { return };
                    let mut chosen = a.selected(&all);
                    if !chosen.remove(&id) {
                        chosen.insert(id.clone());
                    }
                    a.chosen = Some(chosen);
                })
                .build(k)
                .key(format!("insight-{}", t.id)),
        );
    }
    col().key("add-insights").w_full().gap(6.0).child(text("Make insights").font(k.ty.ui_medium).color(l.text2)).child(checks)
}

/// The sources of a batch, each with how it went, while they are added and after.
fn batch(a: &AddSource, items: &[BatchItem], l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    let mut list = col().key("batch").w_full().gap(8.0);
    for (i, item) in items.iter().enumerate() {
        let mark = match &item.state {
            ItemState::Waiting => el().w(16.0).h(16.0),
            ItemState::Adding => rune_kit::Spinner::new().size(14.0).build(k),
            ItemState::Added => rune::icon(&rune_kit::glyphs::CHECK, 16.0).color(l.ok),
            ItemState::Failed(_) => rune::icon(&icons::CLOSE, 16.0).color(l.danger),
        };
        let mut lines = col().grow(1.0).min_w(0.0).gap(1.0).child(text(item.label.clone()).font(k.ty.ui).color(l.text).max_lines(1));
        if let ItemState::Failed(e) = &item.state {
            lines = lines.child(text(e.clone()).font(k.ty.small).color(l.danger).wrap());
        }
        list = list.child(row().key(format!("item-{i}")).w_full().gap(8.0).child(col().w(16.0).pt(2.0).items_center().child(mark)).child(lines));
    }
    let failed = a.failed();
    let mut foot = row().w_full().gap(8.0).center_y();
    if !a.busy && failed > 0 {
        foot = foot.child(text(format!("{failed} could not be added")).font(k.ty.small).color(l.text3));
    }
    foot = foot.child(el().grow(1.0)).child(Button::new("Close").variant(ButtonVariant::Ghost).disabled(a.busy).on_press(close).build(k).key("add-cancel"));
    if let Some(p) = a.progress() {
        foot = foot.child(Button::new(p).variant(ButtonVariant::Contrast).disabled(true).build(k).key("add-submit"));
    }
    let body = col().w_full().gap(16.0).child(list).child(foot);
    Dialog::new("add-source", "Add sources").open(true).width(520.0).fit().child(body.px(20.0).pb(20.0)).on_close(close).build(k, layer);
}

fn close(s: &mut App) {
    if let Some(o) = s.open.as_mut()
        && !o.add.as_ref().is_some_and(|a| a.busy)
    {
        o.add = None;
    }
}
