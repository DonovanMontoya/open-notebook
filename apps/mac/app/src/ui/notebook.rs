//! The notebook: its sources on the left, chat or notes in the middle, and the source being
//! read on the right, with the passage a citation points at lit in amber.

use notebook_engine::citations::{self, CitableSource};
use notebook_engine::types::*;
use rune::{El, ScrollSpec, TextEdit, Tx, col, el, row, text};
use rune_kit::markdown::Markdown;
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, SegmentedControl, TextArea, TextInput};

use super::icons;
use crate::state::{App, NoteDraft, NotebookDetails, NotebookTab, Open, is_failed, is_processing, next_mode};
use crate::theme::{self, Look};

const RAIL: f32 = 300.0;
const READER: f32 = 380.0;

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let o = s.open.as_ref().expect("a notebook is open");
    let center = match o.tab {
        NotebookTab::Chat => chat(s, o, l, layer),
        NotebookTab::Notes => notes(o, l),
        NotebookTab::Study => super::study::view(o, l),
        NotebookTab::Listen => super::listen::view(s, o, l, layer),
    };
    const TABS: [NotebookTab; 4] = [NotebookTab::Chat, NotebookTab::Notes, NotebookTab::Study, NotebookTab::Listen];
    let tabs = SegmentedControl::new(&["Chat", "Notes", "Study", "Listen"], TABS.iter().position(|t| *t == o.tab))
        .label("View")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            if let Some(o) = s.open.as_mut() {
                o.tab = TABS[i];
            }
        })
        .build(&l.k)
        .key("tabs");
    let mut head = row().key("center-head").w_full().h(44.0).px(16.0).gap(8.0).center_y().child(tabs).child(el().grow(1.0));
    if o.tab == NotebookTab::Chat {
        head = chat_bar(o, head, l, layer);
    } else if o.tab == NotebookTab::Notes {
        head = head.child(
            Button::new("New note")
                .icon(&icons::PLUS)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        o.new_note(String::new(), String::new(), false);
                    }
                })
                .build(&l.k)
                .key("new-note"),
        );
    }
    let mut middle = col().key("center").grow(1.0).h_full().min_w(380.0).min_h(0.0).child(header(o, l));
    if o.search_open
        && let Some(hits) = &o.hits
    {
        middle = middle.child(search_results(hits, l));
    }
    let middle = middle.child(head).child(rule(l)).child(center);
    // A cited passage opens in the rail, beside the answer that cites it, as in the Tauri app.
    let rail = match &o.reading {
        Some((detail, passage)) => reader(o, detail, *passage, l, layer),
        None => sources(o, l),
    };
    let body = row()
        .key("notebook")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(rail)
        .child(vrule(l))
        .child(middle)
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| {
            if let Some(o) = s.open.as_mut() {
                o.drop_over = false;
            }
            s.open_add(files);
        })
        .accepts_extensions(&crate::state::accepted())
        .on_drop_hover(|s: &mut App, over, _: &mut Tx| {
            if let Some(o) = s.open.as_mut() {
                o.drop_over = over;
            }
        });
    if let Some(add) = &o.add {
        super::add::sheet(o, add, l, layer);
    } else if o.drop_over {
        let name = o.notebook.as_ref().map(|n| n.name.clone()).unwrap_or_else(|| "this notebook".into());
        layer.push(col().key("drop-overlay").absolute(0.0, 0.0).w_full().h_full().inert().child(
            col().w_full().h_full().center().fill(l.page.alpha(0.88)).child(text(format!("Drop to add to {name}")).font(theme::READ_HEADING).color(l.text)),
        ));
    }
    if let Some(d) = &o.details {
        details(o, d, l, layer);
    }
    body
}

/// The notebook's name and what it's for, its settings, and a search box.
fn header(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let nb = o.notebook.as_ref();
    let mut title = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(1.0)
        .child(text(nb.map(|n| n.name.clone()).unwrap_or_default()).key("notebook-name").font(theme::READ_HEADING).color(l.text).max_lines(1));
    if let Some(d) = nb.map(|n| n.description.trim()).filter(|d| !d.is_empty()) {
        title = title.child(text(d.to_owned()).font(k.ty.small).color(l.text3).max_lines(2));
    }
    let title = title.on_double_press(open_details);
    let search = TextInput::new(&o.search)
        .placeholder("Search notebook")
        .label("Search this notebook")
        .leading_icon(&rune_kit::glyphs::SEARCH)
        .width(Some(220.0))
        .on_edit(|s: &mut App| &mut s.open.as_mut().expect("open").search)
        .on_change(|s: &mut App, _: &mut Tx| s.search_changed())
        .on_key(|s: &mut App, ev: &rune::KeyEvent, _: &mut Tx| {
            if ev.key == rune::Key::Escape {
                if let Some(o) = s.open.as_mut() {
                    o.search_open = false;
                }
                return true;
            }
            false
        })
        .build(k)
        .key("notebook-search");
    row()
        .key("notebook-head")
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(8.0)
        .center_y()
        .child(title)
        .child(
            IconButton::new(&icons::GEAR)
                .variant(IconButtonVariant::GhostMuted)
                .label("Notebook settings")
                .help("Notebook settings")
                .on_press(open_details)
                .build(k)
                .key("notebook-settings"),
        )
        .child(search)
}

fn open_details(s: &mut App) {
    if let Some(o) = s.open.as_mut()
        && let Some(nb) = &o.notebook
    {
        o.details = Some(NotebookDetails { name: TextEdit::new(nb.name.clone()), description: TextEdit::new(nb.description.clone()) });
    }
}

/// The search's hits under the header: a source opens in the reader at the passage, a note in the editor.
fn search_results(hits: &crate::state::Hits, l: &Look) -> El {
    let k = &l.k;
    let (query, result) = hits;
    let mut list = col().key("search-results").w_full().gap(2.0).px(8.0).py(6.0);
    match result {
        Err(e) => list = list.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap().px(8.0).py(6.0)),
        Ok(found) if found.is_empty() => {
            list = list.child(text(format!("Nothing matches \u{201c}{query}\u{201d}")).font(k.ty.ui).color(l.text3).px(8.0).py(6.0))
        }
        Ok(found) => {
            for h in found {
                let (id, kind, q) = (h.id.clone(), h.kind, query.clone());
                let icon = if kind == HitKind::Note { &icons::SAVE } else { &icons::DOC };
                let mut lines = col().grow(1.0).min_w(0.0).gap(1.0).child(text(h.title.clone()).font(k.ty.ui_medium).color(l.text).max_lines(1));
                if !h.snippet.trim().is_empty() {
                    lines = lines.child(text(h.snippet.clone()).font(k.ty.small).color(l.text3).max_lines(2));
                }
                let r =
                    row().key(format!("hit-{}", h.id)).w_full().gap(8.0).px(8.0).py(6.0).radius(8.0).child(rune::icon(icon, 14.0).color(l.text3)).child(lines);
                list = list.child(k.interactive(r).on_press(move |s: &mut App| {
                    let Some(o) = s.open.as_mut() else { return };
                    o.search_open = false;
                    let nb = o.id.clone();
                    match kind {
                        HitKind::Source => s.read_source(id.clone(), Some(q.clone())),
                        HitKind::Note => s.open_note(nb, id.clone()),
                    }
                }));
            }
        }
    }
    col().w_full().child(list.scroll(ScrollSpec { stick_bottom: false }).max_h(320.0)).child(rule(l))
}

/// Rename, describe, archive or delete the notebook.
fn details(o: &Open, d: &NotebookDetails, l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    fn form(s: &mut App) -> &mut NotebookDetails {
        s.open.as_mut().and_then(|o| o.details.as_mut()).expect("details")
    }
    fn close(s: &mut App) {
        if let Some(o) = s.open.as_mut() {
            o.details = None;
        }
    }
    fn save(s: &mut App, archived: Option<bool>) {
        let Some(o) = s.open.as_ref() else { return };
        let Some(d) = &o.details else { return };
        let name = d.name.text.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let update = NotebookUpdate { name: Some(name), description: Some(d.description.text.trim().to_owned()), archived };
        let id = o.id.clone();
        s.update_notebook(id, update);
    }
    let archived = o.notebook.as_ref().is_some_and(|n| n.archived);
    let name = o.notebook.as_ref().map(|n| n.name.clone()).unwrap_or_default();
    let id = o.id.clone();
    let body = col()
        .w_full()
        .gap(12.0)
        .child(text("Name").font(k.ty.ui_medium).color(l.text2))
        .child(
            TextInput::new(&d.name)
                .label("Name")
                .width(None)
                .on_edit(|s: &mut App| &mut form(s).name)
                .on_submit(|s: &mut App, _: &mut Tx| save(s, None))
                .build(k)
                .key("details-name"),
        )
        .child(text("What it's for").font(k.ty.ui_medium).color(l.text2))
        .child(
            TextArea::new(&d.description)
                .label("What it's for")
                .placeholder("Optional. The assistant reads this before answering.")
                .width(None)
                .rows(3)
                .on_edit(|s: &mut App| &mut form(s).description)
                .build(k)
                .key("details-description"),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    Button::new("Delete")
                        .variant(ButtonVariant::Ghost)
                        .on_press(move |s: &mut App| {
                            let id = id.clone();
                            s.ask(format!("Delete \u{201c}{name}\u{201d}? Its sources, notes and conversations are deleted too."), move |s| {
                                s.delete_notebook(id.clone())
                            });
                        })
                        .build(k)
                        .key("details-delete"),
                )
                .child(
                    Button::new(if archived { "Unarchive" } else { "Archive" })
                        .variant(ButtonVariant::Ghost)
                        .on_press(move |s: &mut App| save(s, Some(!archived)))
                        .build(k)
                        .key("details-archive"),
                )
                .child(el().grow(1.0))
                .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(close).build(k).key("details-cancel"))
                .child(
                    Button::new("Save")
                        .variant(ButtonVariant::Contrast)
                        .disabled(d.name.text.trim().is_empty())
                        .on_press(|s: &mut App| save(s, None))
                        .build(k)
                        .key("details-save"),
                ),
        );
    Dialog::new("notebook-details", "Notebook").open(true).width(460.0).fit().child(body.px(20.0).pb(20.0)).on_close(close).build(k, layer);
}

fn rule(l: &Look) -> El {
    el().w_full().h(1.0).fill(l.line)
}

fn vrule(l: &Look) -> El {
    el().w(1.0).h_full().fill(l.line)
}

// Sources

fn sources(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let add = Button::new("Add source")
        .icon(&icons::PLUS)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(|s: &mut App| s.open_add(&[]))
        .build(k)
        .key("add-source");
    let head = row()
        .w_full()
        .h(44.0)
        .px(16.0)
        .center_y()
        .justify_between()
        .child(text(format!("Sources · {}", o.sources.len())).font(k.ty.ui_medium).color(l.text2))
        .child(add);
    let mut list = col().key("source-list").w_full().gap(2.0).px(8.0).pb(12.0);
    if o.sources.is_empty() {
        list = list.child(text("Add links, text or files. Chat answers from what you add here.").font(k.ty.ui).color(l.text3).wrap().px(8.0).py(12.0));
    }
    let reading = o.reading.as_ref().map(|(d, _)| d.summary.id.as_str());
    for src in &o.sources {
        list = list.child(source_row(o, src, reading == Some(src.id.as_str()), l));
    }
    col()
        .key("sources")
        .w(RAIL)
        .shrink()
        .min_w(240.0)
        .h_full()
        .min_h(0.0)
        .fill(l.page)
        .child(head)
        .child(list.scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0))
}

/// A source's icon by what it came from: a link, a file or pasted text.
fn kind_icon(src: &SourceSummary) -> &'static rune::Icon {
    match &src.asset {
        Some(a) if a.url.is_some() => &icons::LINK,
        Some(a) if a.file_path.is_some() => &icons::DOC,
        _ if src.file_available => &icons::DOC,
        _ => &icons::TEXT,
    }
}

/// "example.com, added 3 days ago".
fn source_meta(src: &SourceSummary) -> String {
    let host = src.asset.as_ref().and_then(|a| a.url.as_deref()).map(|u| format!("{}, ", crate::format::host_of(u))).unwrap_or_default();
    format!("{host}added {}", crate::format::relative_time(&src.created))
}

fn mode_label(m: SourceContext) -> &'static str {
    match m {
        SourceContext::Full => "Full text",
        SourceContext::Insights => "Insights",
        SourceContext::Off => "Left out",
    }
}

fn mode_hint(m: SourceContext) -> &'static str {
    match m {
        SourceContext::Full => "Chat reads the whole source",
        SourceContext::Insights => "Chat reads only this source's insights",
        SourceContext::Off => "Chat ignores this source",
    }
}

fn source_row(o: &Open, src: &SourceSummary, selected: bool, l: &Look) -> El {
    let k = &l.k;
    let id = src.id.clone();
    let title = src.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled source".into());
    let failed = is_failed(src.status.as_deref());
    let mode = o.source_mode(src);
    let off = mode == SourceContext::Off && !failed;
    let meta = if is_processing(src.status.as_deref()) {
        row().gap(6.0).center_y().child(rune_kit::Spinner::new().size(10.0).build(k)).child(text("Reading and indexing").font(k.ty.small).color(l.text3))
    } else if failed {
        row().child(text(src.error.clone().unwrap_or_else(|| "Could not be read".into())).font(k.ty.small).color(l.danger).max_lines(2).shrink().min_w(0.0))
    } else {
        row().child(text(source_meta(src)).font(k.ty.small).color(l.text3).max_lines(1).shrink().min_w(0.0))
    };
    let lines = col().grow(1.0).min_w(0.0).gap(2.0).child(text(title).font(k.ty.ui).color(if off { l.text3 } else { l.text }).max_lines(2)).child(meta);
    let mut r =
        row().key(format!("src-{}", src.id)).w_full().px(8.0).py(7.0).gap(8.0).radius(8.0).child(rune::icon(kind_icon(src), 16.0).color(l.text3)).child(lines);
    if failed {
        let rid = id.clone();
        r = r.child(
            IconButton::new(&icons::RETRY)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Try again")
                .help("Try again")
                .on_press(move |s: &mut App| s.retry_source(rid.clone()))
                .build(k)
                .key("retry"),
        );
    } else {
        let next = next_mode(mode, src.insights_count > 0);
        let sid = id.clone();
        r = r.child(
            Button::new(mode_label(mode))
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .help(format!("{}. Click for {}.", mode_hint(mode), mode_label(next).to_lowercase()))
                .on_press(move |s: &mut App| s.set_source_context(sid.clone(), next))
                .build(k)
                .key("context-mode"),
        );
    }
    if selected {
        r = r.fill(l.amber_wash);
    } else {
        r = k.interactive(r);
    }
    r.on_press(move |s: &mut App| s.read_source(id.clone(), None))
}

fn reader(o: &Open, detail: &SourceDetail, passage: Option<(usize, usize)>, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let src = &detail.summary;
    let title = src.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled source".into());
    let sid = src.id.clone();
    let title_el = match &o.renaming_source {
        Some(edit) => {
            let a = sid.clone();
            TextInput::new(edit)
                .label("Source title")
                .width(None)
                .on_edit(|s: &mut App| s.open.as_mut().and_then(|o| o.renaming_source.as_mut()).expect("renaming"))
                .on_submit(move |s: &mut App, _: &mut Tx| {
                    let t = s.open.as_ref().and_then(|o| o.renaming_source.as_ref()).map(|e| e.text.clone()).unwrap_or_default();
                    s.rename_source(a.clone(), t);
                })
                .on_key(|s: &mut App, ev: &rune::KeyEvent, _: &mut Tx| {
                    if ev.key == rune::Key::Escape {
                        if let Some(o) = s.open.as_mut() {
                            o.renaming_source = None;
                        }
                        return true;
                    }
                    false
                })
                .build(k)
                .key("rename-source")
                .grow(1.0)
                .min_w(0.0)
        }
        None => {
            let t = title.clone();
            text(title.clone()).font(k.ty.ui_medium).color(l.text).grow(1.0).min_w(0.0).max_lines(1).help("Double-click to rename").on_double_press(
                move |s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        let mut e = TextEdit::new(t.clone());
                        e.focused = true;
                        o.renaming_source = Some(e);
                    }
                },
            )
        }
    };
    let mut head = row().w_full().h(44.0).px(16.0).gap(8.0).center_y().child(title_el);
    if let Some(url) = src.asset.as_ref().and_then(|a| a.url.clone()).filter(|u| is_web(u)) {
        head = head.child(
            IconButton::new(&icons::EXTERNAL)
                .variant(IconButtonVariant::GhostMuted)
                .label("Open original")
                .help("Open original")
                .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                    let _ = tx.open_url(&url);
                })
                .build(k)
                .key("open-original"),
        );
    }
    head = head
        .child({
            let (sid, t) = (sid.clone(), title.clone());
            let on = o.scope.as_ref().is_some_and(|(id, _)| *id == sid);
            IconButton::new(&icons::CHAT)
                .variant(IconButtonVariant::GhostMuted)
                .expanded(on)
                .label(if on { "Back to notebook chat" } else { "Chat about this source" })
                .help(if on { "Back to notebook chat" } else { "Chat about this source" })
                .on_press(move |s: &mut App| s.chat_scope((!on).then(|| (sid.clone(), t.clone()))))
                .build(k)
                .key("source-chat")
        })
        .child(
            IconButton::new(&icons::CLOSE)
                .variant(IconButtonVariant::GhostMuted)
                .label("Back to sources")
                .help("Back to sources")
                .on_press(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        o.reading = None;
                        o.renaming_source = None;
                        o.active_cite = None;
                    }
                })
                .build(k)
                .key("close-reader"),
        );
    let mut meta = source_meta(src);
    if src.embedded_chunks > 0 {
        let n = src.embedded_chunks;
        meta.push_str(&format!(", {n} {} indexed", if n == 1 { "passage" } else { "passages" }));
    }
    let full = &detail.full_text;
    let body = if full.trim().is_empty() {
        let empty = if is_processing(src.status.as_deref()) { "Still reading this source" } else { "No text was found" };
        text(empty).font(theme::READ_HEADING).color(l.text3).wrap()
    } else {
        let mut t = text(full.clone()).font(theme::READ_BODY).color(l.text2).wrap().selectable(true);
        if let Some((start, end)) = passage.filter(|(s, e)| s < e && *e <= full.len()) {
            // Paint-only, so the text wraps the same; keyed so the reader can scroll to it.
            t = t
                .key(format!("source-text-{}-{start}", src.id))
                .text_highlight(rune::TextHighlight::new(start..end, l.amber_wash).color(l.text).key("passage"))
                .on_layout_tx(|s: &mut App, _, tx: &mut Tx| {
                    if let Some(o) = s.open.as_mut()
                        && std::mem::take(&mut o.scroll_to_passage)
                    {
                        tx.scroll_center("passage");
                    }
                });
        } else {
            t = t.key(format!("source-text-{}", src.id));
        }
        t
    };
    let shared = o.reading.as_ref().is_some_and(|(d, _)| d.summary.notebooks.len() > 1);
    let mut actions = row().w_full().gap(8.0);
    if src.file_available {
        let id = sid.clone();
        actions = actions.child(
            Button::new(if o.downloading { "Saving" } else { "Download original" })
                .icon(&icons::DOWNLOAD)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .disabled(o.downloading)
                .on_press(move |s: &mut App| s.download_source(id.clone()))
                .build(k)
                .key("download"),
        );
    }
    let id = sid.clone();
    actions = actions.child(
        Button::new(if shared { "Remove from this notebook" } else { "Delete source" })
            .icon(&icons::TRASH)
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press(move |s: &mut App| s.ask_delete_source(id.clone()))
            .build(k)
            .key("delete-source"),
    );
    let content = col()
        .w_full()
        .px(20.0)
        .py(16.0)
        .gap(18.0)
        .child(text(meta).font(k.ty.small).color(l.text3).wrap())
        .child(insights(o, !full.trim().is_empty(), l, layer))
        .child(body)
        .child(actions);
    col()
        .key("reader")
        .w(READER)
        .shrink()
        .min_w(260.0)
        .h_full()
        .min_h(0.0)
        .fill(l.page)
        .child(head)
        .child(rule(l))
        .child(content.scroll(ScrollSpec { stick_bottom: false }).key("reader-scroll").grow(1.0).min_h(0.0))
}

/// Links that open in the browser: web pages only.
fn is_web(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://")
}

/// The source's insights above its text, each opening to show it in full, and a menu to run a
/// transformation once there is text to run it on.
fn insights(o: &Open, has_text: bool, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let names: Vec<String> = o.transformations.iter().map(|t| t.title.clone()).collect();
    let ids: Vec<String> = o.transformations.iter().map(|t| t.id.clone()).collect();
    let mut head = row().w_full().gap(8.0).center_y().child(text("Insights").font(k.ty.ui_medium).color(l.text2).grow(1.0));
    if has_text && !names.is_empty() {
        head = head.child(
            rune_kit::Select::new("run-insight", &names)
                .label("Run a transformation")
                .placeholder("Transform")
                .current(None)
                .width(160.0)
                .open(o.insight_picker)
                .on_toggle(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        o.insight_picker = !o.insight_picker;
                    }
                })
                .on_close(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        o.insight_picker = false;
                    }
                })
                .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
                    let id = s.open.as_ref().and_then(|o| o.transformations.iter().position(|t| t.title == name)).and_then(|i| ids.get(i).cloned());
                    if let Some(id) = id {
                        if let Some(o) = s.open.as_mut() {
                            o.insights_expected = Some(std::time::Instant::now() + std::time::Duration::from_secs(120));
                        }
                        s.run_insight(id);
                    }
                })
                .build(k, layer),
        );
    }
    let mut c = col().key("insights").w_full().gap(8.0).child(head);
    if o.insights.is_empty() {
        let hint = if has_text {
            "Run a transformation to get a summary, key points or questions from this source."
        } else {
            "Insights can be made once the text is ready."
        };
        c = c.child(text(hint).font(k.ty.small).color(l.text3).wrap());
    }
    for i in &o.insights {
        let running = i.status.as_deref() == Some("running");
        let failed = i.status.as_deref() == Some("failed");
        let open = o.insight_open.as_deref() == Some(i.id.as_str());
        let status = if running {
            rune_kit::Spinner::new().size(12.0).build(k)
        } else if failed {
            text("Failed").font(k.ty.small).color(l.danger)
        } else {
            rune::icon(if open { &rune_kit::glyphs::CHEVRON_UP } else { &rune_kit::glyphs::CHEVRON_DOWN }, 14.0).color(l.text3)
        };
        let tid = i.id.clone();
        let top = k
            .interactive(
                row()
                    .w_full()
                    .gap(6.0)
                    .center_y()
                    .radius(6.0)
                    .child(text(i.kind.clone()).font(k.ty.ui_medium).color(l.text).grow(1.0).min_w(0.0).max_lines(1))
                    .child(status),
            )
            .on_press(move |s: &mut App| {
                if let Some(o) = s.open.as_mut() {
                    o.insight_open = if o.insight_open.as_deref() == Some(tid.as_str()) { None } else { Some(tid.clone()) };
                }
            })
            .key("insight-head");
        let mut card = col().key(format!("insight-{}", i.id)).w_full().gap(6.0).padding(12.0).radius(10.0).fill(l.raised).child(top);
        if failed {
            card = card.child(text(i.error.clone().unwrap_or_default()).font(k.ty.small).color(l.danger).wrap());
        } else if !running {
            let style = rune_kit::markdown::MarkdownStyle::from_kit(k).body_font(theme::READ_BODY).text(l.text2);
            let md = Markdown::new(i.content.clone()).style(style).selectable(true).build(k);
            card = card.child(if open { md } else { col().w_full().max_h(78.0).clip_fade(0.0, 0.0, 0.0, 28.0).child(md) });
        }
        if open && !running {
            let (del, save) = (i.id.clone(), i.id.clone());
            let mut acts = row().gap(4.0);
            if !failed {
                let saved = o.saved_insights.contains(&i.id);
                acts = acts.child(
                    Button::new(if saved { "Saved to notes" } else { "Save as note" })
                        .icon(&icons::SAVE)
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .disabled(saved)
                        .on_press(move |s: &mut App| s.save_insight(save.clone()))
                        .build(k)
                        .key("save"),
                );
            }
            acts = acts.child(
                Button::new("Delete")
                    .icon(&icons::TRASH)
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| {
                        let id = del.clone();
                        s.ask("Delete this insight?", move |s| s.delete_insight(id.clone()));
                    })
                    .build(k)
                    .key("delete"),
            );
            card = card.child(acts);
        }
        c = c.child(card);
    }
    c
}

// Chat

/// The conversation bar: which conversation, its rename and delete, and a new chat.
fn chat_bar(o: &Open, mut head: El, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    if let Some(edit) = &o.renaming_chat {
        let field = TextInput::new(edit)
            .label("Conversation name")
            .width(Some(220.0))
            .on_edit(|s: &mut App| s.open.as_mut().and_then(|o| o.renaming_chat.as_mut()).expect("renaming"))
            .on_submit(|s: &mut App, _: &mut Tx| submit_rename(s))
            .on_key(|s: &mut App, ev: &rune::KeyEvent, _: &mut Tx| {
                if ev.key == rune::Key::Escape {
                    if let Some(o) = s.open.as_mut() {
                        o.renaming_chat = None;
                    }
                    return true;
                }
                false
            })
            .build(k)
            .key("rename-chat");
        return head.child(field).child(Button::new("Save").control_size(ControlSize::Small).on_press(submit_rename).build(k).key("rename-save")).child(
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        o.renaming_chat = None;
                    }
                })
                .build(k)
                .key("rename-cancel"),
        );
    }
    head = head.child(session_picker(o, l, layer));
    if let Some(session) = o.session.as_ref().map(|s| s.session.clone()) {
        let sid = session.id.clone();
        // Source chats can't be renamed on a server, so only notebook chats offer it.
        if o.scope.is_none() {
            head = head.child(
                IconButton::new(&icons::PENCIL)
                    .variant(IconButtonVariant::GhostMuted)
                    .label("Rename conversation")
                    .help("Rename conversation")
                    .on_press(move |s: &mut App| {
                        if let Some(o) = s.open.as_mut() {
                            let mut e = TextEdit::new(session.title.clone());
                            e.focused = true;
                            o.renaming_chat = Some(e);
                        }
                    })
                    .build(k)
                    .key("rename-chat-button"),
            );
        }
        head = head.child(
            IconButton::new(&icons::TRASH)
                .variant(IconButtonVariant::GhostMuted)
                .label("Delete conversation")
                .help("Delete conversation")
                .on_press(move |s: &mut App| {
                    let sid = sid.clone();
                    s.ask("Delete this conversation?", move |s| s.delete_session(sid.clone()));
                })
                .build(k)
                .key("delete-chat"),
        );
    }
    head.child(
        IconButton::new(&icons::PLUS)
            .variant(IconButtonVariant::GhostMuted)
            .label("New chat")
            .help("New chat")
            .disabled(o.session.is_none())
            .on_press(|s: &mut App| s.new_chat())
            .build(k)
            .key("new-chat"),
    )
}

fn submit_rename(s: &mut App) {
    let t = s.open.as_ref().and_then(|o| o.renaming_chat.as_ref()).map(|e| e.text.clone()).unwrap_or_default();
    s.rename_chat(t);
}

fn session_picker(o: &Open, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    if o.sessions.is_empty() {
        return el();
    }
    let titles = o.sessions.iter().map(|s| if s.title.trim().is_empty() { "Untitled conversation".to_owned() } else { s.title.clone() });
    let labels = crate::state::unique_labels(titles);
    let details: Vec<Option<String>> =
        o.sessions.iter().map(|s| Some(crate::format::relative_time(if s.updated.is_empty() { &s.created } else { &s.updated }))).collect();
    let current = o.session.as_ref().and_then(|cur| o.sessions.iter().position(|s| s.id == cur.session.id)).map(|i| labels[i].as_str());
    let ids: Vec<String> = o.sessions.iter().map(|s| s.id.clone()).collect();
    let picks = labels.clone();
    rune_kit::Select::new("sessions", &labels)
        .details(&details)
        .label(format!("Conversations, {}", o.sessions.len()))
        .placeholder(format!("New chat · {}", o.sessions.len()))
        .current(current)
        .width(220.0)
        .open(o.sessions_open)
        .on_toggle(|s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                o.sessions_open = !o.sessions_open;
            }
        })
        .on_close(|s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                o.sessions_open = false;
            }
        })
        .on_choose(move |s: &mut App, label: &str, _: &mut Tx| {
            // Labels are unique, so the position names the conversation.
            if let Some(id) = picks.iter().position(|x| x == label).and_then(|i| ids.get(i).cloned()) {
                s.open_session(id);
            }
        })
        .build(k, layer)
}

fn citable(o: &Open) -> Vec<CitableSource> {
    o.sources.iter().map(|s| CitableSource { id: s.id.clone(), title: s.title.clone() }).collect()
}

fn chat(s: &App, o: &Open, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let sources = citable(o);
    let mut thread = col().key("thread").w_full().max_w(720.0).gap(18.0).px(24.0).py(20.0);
    if let Some((_, title)) = &o.scope {
        thread = thread.child(
            row()
                .key("scope-banner")
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(8.0)
                .radius(10.0)
                .fill(l.amber_wash)
                .center_y()
                .child(text(format!("Chatting about {title}")).font(k.ty.ui).color(l.text).grow(1.0).min_w(0.0).max_lines(1))
                .child(
                    Button::new("Back to notebook chat")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut App| s.chat_scope(None))
                        .build(k)
                        .key("scope-back"),
                ),
        );
    }
    let messages = o.session.as_ref().map(|s| s.messages.as_slice()).unwrap_or(&[]);
    if messages.is_empty() && o.pending.is_none() {
        let ready = o.chat_ready();
        let (title, body) = if o.scope.is_some() {
            ("Ask about this source", "Questions here are answered from this source alone.")
        } else if ready {
            ("Ask about your sources", "Answers cite the source they come from. Click a citation to read the passage.")
        } else {
            ("Add a source to ask about", "Answers come from the pages, PDFs and text you add to this notebook.")
        };
        let mut empty = col()
            .key("chat-empty")
            .w_full()
            .pt(80.0)
            .gap(8.0)
            .items_center()
            .child(text(title).font(theme::READ_HEADING).color(l.text2).align(rune::Align::Center).wrap())
            .child(text(body).font(k.ty.ui).color(l.text3).align(rune::Align::Center).wrap());
        if !ready {
            empty = empty.child(
                row().pt(6.0).child(Button::new("Add a source").icon(&icons::PLUS).on_press(|s: &mut App| s.open_add(&[])).build(k).key("chat-add-source")),
            );
        }
        thread = thread.child(empty);
    }
    let active = |id: &str| o.active_cite.as_ref().filter(|(m, _)| m == id).map(|(_, i)| *i);
    for (i, m) in messages.iter().enumerate() {
        thread = thread.child(message(&m.id, m.kind, &m.content, &sources, None, active(&m.id), l).key(format!("msg-{i}-{}", m.id)));
    }
    if let Some((question, reply)) = &o.pending {
        thread = thread.child(message("pending-q", MessageKind::Human, question, &sources, None, None, l).key("pending-q"));
        let writing = if reply.is_empty() { "Reading your sources" } else { "Writing" };
        thread = thread.child(message("pending", MessageKind::Ai, reply, &sources, Some(writing), None, l).key("pending-a"));
    }
    if let Some(e) = &o.error {
        thread = thread.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap().key("chat-error"));
    }
    let scroller = col().key("thread-scroll").w_full().items_center().child(thread).scroll(ScrollSpec { stick_bottom: true }).grow(1.0).min_h(0.0);
    col().w_full().grow(1.0).min_h(0.0).child(scroller).child(composer(s, o, l, layer))
}

/// Marks the clicked citation: its links in the answer turn bold.
fn mark_cite(markdown: &str, index: usize) -> String {
    let tail = format!("](cite:{index})");
    let mut out = String::with_capacity(markdown.len() + 8);
    let mut rest = markdown;
    while let Some(end) = rest.find(&tail) {
        let Some(open) = rest[..end].rfind('[') else { break };
        out.push_str(&rest[..open]);
        out.push_str("**");
        out.push_str(&rest[open..end + tail.len()]);
        out.push_str("**");
        rest = &rest[end + tail.len()..];
    }
    out.push_str(rest);
    out
}

/// A message. `writing` is the status while a reply streams in, when only its finished
/// blocks show; `active` is the citation last clicked in it.
fn message(id: &str, kind: MessageKind, content: &str, sources: &[CitableSource], writing: Option<&str>, active: Option<usize>, l: &Look) -> El {
    let k = &l.k;
    match kind {
        MessageKind::Human => row().w_full().justify_end().child(
            col()
                .shrink()
                .min_w(0.0)
                .max_w(520.0)
                .px(14.0)
                .py(9.0)
                .radius(14.0)
                .fill(l.raised)
                .child(text(content.to_owned()).font(k.ty.body).color(l.text).wrap().selectable(true)),
        ),
        MessageKind::Ai => {
            let shown = if writing.is_some() { notebook_engine::blocks::finished_blocks(content) } else { content };
            let linked = citations::link_citations(shown, sources);
            let cited: Vec<String> = linked.cited.iter().map(|c| c.id.clone()).collect();
            let md = linked.markdown.clone();
            let marked = match active {
                Some(i) => mark_cite(&linked.markdown, i),
                None => linked.markdown,
            };
            let style = rune_kit::markdown::MarkdownStyle::from_kit(k).body_font(theme::READ_BODY).link(l.amber).text(l.text);
            let mid = id.to_owned();
            let mut c = col().w_full().gap(6.0);
            if !marked.is_empty() {
                c = c.child(
                    Markdown::new(marked)
                        .style(style)
                        .selectable(true)
                        .on_link_tx(move |s: &mut App, url: &str, tx: &mut Tx| {
                            if let Some(i) = url.strip_prefix("cite:").and_then(|n| n.parse::<usize>().ok()) {
                                if let Some(id) = cited.get(i) {
                                    let at = md.find(&format!("](cite:{i})")).unwrap_or(0);
                                    let query = citations::sentence_around(&md, at);
                                    if let Some(o) = s.open.as_mut() {
                                        o.active_cite = Some((mid.clone(), i));
                                    }
                                    s.read_source(id.clone(), Some(query));
                                }
                            } else if is_web(url) {
                                let _ = tx.open_url(url);
                            }
                        })
                        .build(k),
                );
            }
            if let Some(w) = writing {
                return c.child(text(w).font(k.ty.small).color(l.text3).key("writing"));
            }
            let copy = content.to_owned();
            let save = content.to_owned();
            c.child(
                row()
                    .gap(2.0)
                    .child(
                        IconButton::new(&rune_kit::glyphs::COPY)
                            .variant(IconButtonVariant::GhostMuted)
                            .control_size(ControlSize::Small)
                            .label("Copy")
                            .help("Copy")
                            .on_press_tx(move |_: &mut App, tx: &mut Tx| tx.copy(copy.clone()))
                            .build(k)
                            .key("copy"),
                    )
                    .child(
                        IconButton::new(&icons::SAVE)
                            .variant(IconButtonVariant::GhostMuted)
                            .control_size(ControlSize::Small)
                            .label("Save as note")
                            .help("Save as note")
                            .on_press(move |s: &mut App| {
                                // Opens in the editor so it can be changed before it is saved.
                                if let Some(o) = s.open.as_mut() {
                                    o.new_note(notebook_engine::text::title_from(&save, 60), save.clone(), true);
                                }
                            })
                            .build(k)
                            .key("save"),
                    ),
            )
        }
    }
}

fn composer(s: &App, o: &Open, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let sending = o.sending.is_some();
    let streams = s.backend.as_ref().is_none_or(|b| b.streams_chat());
    let field = TextArea::new(&o.draft)
        .placeholder(if o.scope.is_some() { "Ask about this source" } else { "Ask about these sources" })
        .label("Message")
        .width(None)
        .auto_grow(1, 8)
        .on_key(|s: &mut App, ev: &rune::KeyEvent, _: &mut Tx| {
            if ev.key == rune::Key::Enter && !ev.mods.shift && !ev.mods.alt {
                s.send();
                return true;
            }
            false
        })
        .on_edit(|s: &mut App| &mut s.open.as_mut().expect("open").draft)
        .on_submit(|s: &mut App, _: &mut Tx| s.send())
        .build(k)
        .key("draft");
    let button = if sending && streams {
        IconButton::new(&icons::STOP).variant(IconButtonVariant::Contrast).label("Stop").help("Stop").on_press(|s: &mut App| s.stop())
    } else {
        IconButton::new(&icons::SEND)
            .variant(IconButtonVariant::Contrast)
            .label("Ask")
            .help("Ask")
            .disabled(sending || o.draft.text.trim().is_empty())
            .on_press(|s: &mut App| s.send())
    };
    let meta = row()
        .key("composer-meta")
        .w_full()
        .max_w(720.0)
        .gap(8.0)
        .center_y()
        .child(model_menu(s, o, l, layer))
        .child(el().grow(1.0))
        .child(text("Shift Return for a new line").font(k.ty.small).color(l.text3));
    col()
        .key("composer")
        .w_full()
        .items_center()
        .px(24.0)
        .pb(12.0)
        .pt(8.0)
        .gap(6.0)
        .child(row().w_full().max_w(720.0).gap(8.0).items_end().child(col().grow(1.0).min_w(0.0).child(field)).child(button.build(k).key("send")))
        .child(meta)
}

/// The model for this conversation: the default, or one of the server's or provider's models.
fn model_menu(s: &App, o: &Open, l: &Look, layer: &mut Vec<El>) -> El {
    let default = format!("Default: {}", s.default_model_label());
    let models = o.models.clone().unwrap_or_default();
    let chosen = s.model_override();
    let mut labels = vec![default.clone()];
    labels.extend(crate::state::unique_labels(models.iter().map(|(_, l)| l.clone())));
    let values: Vec<String> = std::iter::once(String::new()).chain(models.iter().map(|(v, _)| v.clone())).collect();
    let current = match &chosen {
        Some(v) => values.iter().position(|x| x == v).map(|i| labels[i].clone()).unwrap_or_else(|| v.clone()),
        None => s.default_model_label(),
    };
    let picks = labels.clone();
    rune_kit::Select::new("chat-model", &labels)
        .label("Model for this conversation")
        .placeholder(current.clone())
        .current(Some(current.as_str()).filter(|c| labels.iter().any(|x| x == c)))
        .width(240.0)
        .open(o.models_open)
        .on_toggle(|s: &mut App| {
            s.load_models();
            if let Some(o) = s.open.as_mut() {
                o.models_open = !o.models_open;
            }
        })
        .on_close(|s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                o.models_open = false;
            }
        })
        .on_choose(move |s: &mut App, label: &str, _: &mut Tx| {
            if let Some(v) = picks.iter().position(|x| x == label).and_then(|i| values.get(i).cloned()) {
                s.set_model_override(v);
            }
            if let Some(o) = s.open.as_mut() {
                o.models_open = false;
            }
        })
        .build(&l.k, layer)
}

// Notes

fn notes(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let mut list = col().key("notes-list").w_full().max_w(720.0).gap(12.0).px(24.0).py(20.0);
    if let Some(d) = &o.note {
        list = list.child(note_editor(d, l));
    }
    if o.notes.is_empty() && o.note.is_none() {
        list = list.child(text("Notes you write, and answers you save from chat, collect here.").font(theme::READ_HEADING).color(l.text3).wrap().pt(60.0));
    }
    for n in &o.notes {
        if o.note.as_ref().is_some_and(|d| d.id.as_deref() == Some(n.id.as_str())) {
            continue;
        }
        let (id, eid) = (n.id.clone(), n.id.clone());
        let (title, content) = (n.title.clone().unwrap_or_default(), n.content.clone().unwrap_or_default());
        let (et, ec) = (title.clone(), content.clone());
        let study = content.starts_with(notebook_engine::studio::STUDY_PREFIX);
        let preview = if study { "Study deck".to_owned() } else { content.chars().take(600).collect() };
        let mut card = col()
            .key(format!("note-{}", n.id))
            .w_full()
            .gap(6.0)
            .padding(14.0)
            .radius(12.0)
            .border(1.0, l.line)
            .fill(l.panel)
            .child(
                row()
                    .w_full()
                    .gap(6.0)
                    .center_y()
                    .child(text(if title.is_empty() { "Untitled note".into() } else { title }).font(theme::READ_HEADING).color(l.text).grow(1.0).max_lines(1))
                    .child(text(if n.note_type.as_deref() == Some("ai") { "From chat" } else { "" }).font(k.ty.small).color(l.text3))
                    .child(
                        IconButton::new(&icons::TRASH)
                            .variant(IconButtonVariant::GhostMuted)
                            .control_size(ControlSize::Small)
                            .label("Delete note")
                            .on_press(move |s: &mut App| {
                                let id = id.clone();
                                s.ask("Delete this note?", move |s| s.delete_note(id.clone()));
                            })
                            .build(k)
                            .key("delete"),
                    ),
            )
            .child(text(preview).font(theme::READ_BODY).color(l.text2).wrap().max_lines(8));
        if !study {
            card = k.interactive(card).on_press(move |s: &mut App| {
                if let Some(o) = s.open.as_mut() {
                    let mut content = TextEdit::new(ec.clone());
                    content.focused = true;
                    o.note = Some(NoteDraft { id: Some(eid.clone()), title: TextEdit::new(et.clone()), content });
                }
            });
        }
        list = list.child(card);
    }
    col().key("notes").w_full().items_center().child(list).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn note_editor(d: &NoteDraft, l: &Look) -> El {
    let k = &l.k;
    fn draft(s: &mut App) -> &mut NoteDraft {
        s.open.as_mut().and_then(|o| o.note.as_mut()).expect("editing a note")
    }
    col()
        .key("note-editor")
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .fill(l.panel)
        .child(
            TextInput::new(&d.title)
                .width(None)
                .placeholder("Title")
                .plain()
                .text_style(theme::READ_HEADING)
                .on_edit(move |s: &mut App| &mut draft(s).title)
                .build(k)
                .key("note-title"),
        )
        .child(
            TextArea::new(&d.content)
                .width(None)
                .placeholder("Write a note")
                .auto_grow(4, 24)
                .on_edit(move |s: &mut App| &mut draft(s).content)
                .build(k)
                .key("note-content"),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(
                    Button::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut App| {
                            if let Some(o) = s.open.as_mut() {
                                o.note = None;
                            }
                        })
                        .build(k)
                        .key("note-cancel"),
                )
                .child(Button::new("Save").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.save_note()).build(k).key("note-save")),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_clicked_citation_is_marked() {
        let md = "One [A](cite:0) two [B](cite:1) and [A](cite:0).";
        assert_eq!(super::mark_cite(md, 0), "One **[A](cite:0)** two [B](cite:1) and **[A](cite:0)**.");
        assert_eq!(super::mark_cite(md, 2), md);
    }

    #[test]
    fn only_web_links_open() {
        assert!(super::is_web("https://example.com") && super::is_web("HTTP://x.y"));
        assert!(!super::is_web("file:///etc/passwd") && !super::is_web("javascript:alert(1)"));
    }
}
