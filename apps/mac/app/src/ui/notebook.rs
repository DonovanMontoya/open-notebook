//! The notebook: its sources on the left, chat or notes in the middle, and the source being
//! read on the right, with the passage a citation points at lit in amber.

use notebook_engine::citations::{self, CitableSource};
use notebook_engine::types::*;
use rune::file_dialog::FileDialogOptions;
use rune::{El, ScrollSpec, TextEdit, Tx, col, el, rich, row, text};
use rune_kit::markdown::Markdown;
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, SegmentedControl, TextArea, TextInput};

use super::icons;
use crate::state::{AddKind, AddSource, App, NoteDraft, NotebookTab, Open};
use crate::theme::{self, Look};

const RAIL: f32 = 300.0;
const READER: f32 = 380.0;

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let o = s.open.as_ref().expect("a notebook is open");
    let center = match o.tab {
        NotebookTab::Chat => chat(o, l),
        NotebookTab::Notes => notes(o, l),
    };
    let tabs = SegmentedControl::new(&["Chat", "Notes"], Some(if o.tab == NotebookTab::Chat { 0 } else { 1 }))
        .label("View")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            if let Some(o) = s.open.as_mut() {
                o.tab = if i == 0 { NotebookTab::Chat } else { NotebookTab::Notes };
            }
        })
        .build(&l.k)
        .key("tabs");
    let mut head = row().key("center-head").w_full().h(44.0).px(16.0).gap(8.0).center_y().child(tabs).child(el().grow(1.0));
    if o.tab == NotebookTab::Chat {
        if let Some(sid) = o.session.as_ref().map(|s| s.session.id.clone()) {
            head = head.child(
                IconButton::new(&icons::TRASH)
                    .variant(IconButtonVariant::GhostMuted)
                    .label("Delete this chat")
                    .help("Delete this chat")
                    .on_press(move |s: &mut App| {
                        let sid = sid.clone();
                        s.ask("Delete this chat?", move |s| s.delete_session(sid.clone()));
                    })
                    .build(&l.k)
                    .key("delete-chat"),
            );
        }
        head = head.child(session_picker(o, l, layer)).child(
            IconButton::new(&icons::PLUS)
                .variant(IconButtonVariant::GhostMuted)
                .label("New chat")
                .help("New chat")
                .on_press(|s: &mut App| s.new_chat())
                .build(&l.k)
                .key("new-chat"),
        );
    } else {
        head = head.child(
            Button::new("New note")
                .icon(&icons::PLUS)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        let mut content = TextEdit::new("");
                        content.focused = true;
                        o.note = Some(NoteDraft { id: None, title: TextEdit::new(""), content });
                    }
                })
                .build(&l.k)
                .key("new-note"),
        );
    }
    let middle = col().key("center").grow(1.0).h_full().min_w(380.0).min_h(0.0).child(head).child(rule(l)).child(center);
    // A cited passage opens in the rail, beside the answer that cites it, as in the Tauri app.
    let rail = match &o.reading {
        Some((detail, passage)) => reader(detail, *passage, l),
        None => sources(o, l),
    };
    let body = row().key("notebook").w_full().h_full().min_h(0.0).child(rail).child(vrule(l)).child(middle);
    if let Some(add) = &o.add {
        add_sheet(add, l, layer);
    }
    body
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
        .on_press(|s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                let mut a = AddSource::default();
                a.url.focused = true;
                o.add = Some(a);
            }
        })
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
        list = list.child(
            text("Add links, text or files. Chat answers from what you add here.").font(k.ty.ui).color(l.text3).wrap().px(8.0).py(12.0),
        );
    }
    let reading = o.reading.as_ref().map(|(d, _)| d.summary.id.as_str());
    for src in &o.sources {
        list = list.child(source_row(src, reading == Some(src.id.as_str()), l));
    }
    let nb = o.id.clone();
    let delete = Button::new("Delete notebook")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut App| {
            let nb = nb.clone();
            s.ask("Delete this notebook? Its sources, notes and chats go too.", move |s| s.delete_notebook(nb.clone()));
        })
        .build(k)
        .key("delete-notebook");
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
        .child(row().w_full().px(12.0).py(8.0).child(delete))
}

fn source_row(src: &SourceSummary, selected: bool, l: &Look) -> El {
    let k = &l.k;
    let id = src.id.clone();
    let title = src.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled source".into());
    let icon = if src.asset.as_ref().is_some_and(|a| a.url.is_some()) { &icons::LINK } else { &icons::DOC };
    let (status, failed) = match src.status.as_deref() {
        Some("failed") => (src.error.clone().unwrap_or_else(|| "Couldn't read this source".into()), true),
        Some("new" | "queued" | "running" | "processing") => ("Reading…".into(), false),
        _ if src.insights_count > 0 => (format!("{} insights", src.insights_count), false),
        _ => (String::new(), false),
    };
    let mut lines = col().grow(1.0).min_w(0.0).gap(1.0).child(text(title).font(k.ty.ui).color(l.text).max_lines(2));
    if !status.is_empty() {
        lines = lines.child(text(status).font(k.ty.small).color(if failed { l.danger } else { l.text3 }).max_lines(2));
    }
    let mut r = row()
        .key(format!("src-{}", src.id))
        .w_full()
        .px(8.0)
        .py(7.0)
        .gap(8.0)
        .radius(8.0)
        .child(rune::icon(icon, 16.0).color(l.text3))
        .child(lines);
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
    }
    let did = id.clone();
    r = r.child(
        IconButton::new(&icons::TRASH)
            .variant(IconButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .label("Delete source")
            .help("Delete source")
            .on_press(move |s: &mut App| {
                let id = did.clone();
                s.ask("Delete this source? Its text and insights go too.", move |s| s.delete_source(id.clone()));
            })
            .build(k)
            .key("delete"),
    );
    if selected {
        r = r.fill(l.amber_wash);
    } else {
        r = k.interactive(r);
    }
    r.on_press(move |s: &mut App| s.read_source(id.clone(), None))
}

fn reader(detail: &SourceDetail, passage: Option<(usize, usize)>, l: &Look) -> El {
    let k = &l.k;
    let title = detail.summary.title.clone().unwrap_or_else(|| "Untitled source".into());
    let head = row()
        .w_full()
        .h(44.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(text(title).font(k.ty.ui_medium).color(l.text).grow(1.0).min_w(0.0).max_lines(1))
        .child(
            IconButton::new(&icons::CLOSE)
                .variant(IconButtonVariant::GhostMuted)
                .label("Back to sources")
                .help("Back to sources")
                .on_press(|s: &mut App| {
                    if let Some(o) = s.open.as_mut() {
                        o.reading = None;
                    }
                })
                .build(k)
                .key("close-reader"),
        );
    let full = &detail.full_text;
    let body = if full.trim().is_empty() {
        text("No text yet. It appears here once the source has been read.").font(k.ty.ui).color(l.text3).wrap()
    } else if let Some((start, end)) = passage.filter(|(s, e)| s < e && *e <= full.len()) {
        // The passage is a span in the same text block, so it wraps with the paragraph around it.
        let spans = vec![rune::Span::new(start, end).bg(l.amber_wash, 2.0).color(l.text)];
        rich(full.clone(), spans).key("source-text").font(theme::READ_BODY).color(l.text2).wrap().selectable(true)
    } else {
        text(full.clone()).key("source-text").font(theme::READ_BODY).color(l.text2).wrap().selectable(true)
    };
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
        .child(col().w_full().px(20.0).py(16.0).child(body).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0))
}

fn add_sheet(a: &AddSource, l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    let kinds = SegmentedControl::new(&["Link", "Text", "File"], Some(a.kind as usize))
        .label("Source type")
        .width(None)
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            if let Some(a) = s.open.as_mut().and_then(|o| o.add.as_mut()) {
                a.kind = [AddKind::Link, AddKind::Text, AddKind::File][i];
                a.error = None;
            }
        })
        .build(k)
        .key("add-kind");
    fn add(s: &mut App) -> &mut AddSource {
        s.open.as_mut().and_then(|o| o.add.as_mut()).expect("adding")
    }
    let mut body = col().w_full().gap(12.0).child(kinds);
    match a.kind {
        AddKind::Link => {
            body = body.child(
                TextInput::new(&a.url)
                    .placeholder("https://… (one per line for several)")
                    .label("Link")
                    .width(None)
                    .on_edit(move |s: &mut App| &mut add(s).url)
                    .on_submit(|s: &mut App, _: &mut Tx| submit_add(s))
                    .build(k)
                    .key("add-url"),
            );
        }
        AddKind::Text => {
            body = body
                .child(TextInput::new(&a.title).width(None).placeholder("Title (optional)").on_edit(move |s: &mut App| &mut add(s).title).build(k).key("add-title"))
                .child(TextArea::new(&a.text).width(None).placeholder("Paste text").rows(8).on_edit(move |s: &mut App| &mut add(s).text).build(k).key("add-text"));
        }
        AddKind::File => {
            body = body.child(text("PDF, Word, PowerPoint, Excel, EPUB, HTML, Markdown or plain text. The text is read on this Mac.").font(k.ty.ui).color(l.text2).wrap());
        }
    }
    if let Some(e) = &a.error {
        body = body.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    let action = if a.kind == AddKind::File { "Choose file…" } else { "Add" };
    let mut primary = Button::new(if a.busy { "Adding…" } else { action }).variant(ButtonVariant::Contrast).disabled(a.busy);
    primary = if a.kind == AddKind::File { primary.on_press_tx(|s: &mut App, tx: &mut Tx| choose_file(s, tx)) } else { primary.on_press(submit_add) };
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(close_add).build(k).key("add-cancel"))
            .child(primary.build(k).key("add-submit")),
    );
    Dialog::new("add-source", "Add a source").open(true).width(480.0).fit().child(body.px(20.0).pb(20.0)).on_close(close_add).build(k, layer);
}

fn close_add(s: &mut App) {
    if let Some(o) = s.open.as_mut() {
        o.add = None;
    }
}

fn submit_add(s: &mut App) {
    let Some(a) = s.open.as_ref().and_then(|o| o.add.as_ref()) else { return };
    if a.busy {
        return;
    }
    match a.kind {
        AddKind::Link => {
            let urls = notebook_engine::text::parse_links(&a.url.text);
            if urls.is_empty() {
                set_add_error(s, "Enter a link that starts with http:// or https://");
                return;
            }
            for url in urls {
                s.add_source(SourceInput::Link { url, title: None });
            }
        }
        AddKind::Text => {
            let content = a.text.text.trim().to_owned();
            if content.is_empty() {
                set_add_error(s, "Paste some text first");
                return;
            }
            let title = Some(a.title.text.trim().to_owned()).filter(|t| !t.is_empty());
            s.add_source(SourceInput::Text { content, title });
        }
        AddKind::File => {}
    }
}

fn set_add_error(s: &mut App, e: &str) {
    if let Some(a) = s.open.as_mut().and_then(|o| o.add.as_mut()) {
        a.error = Some(e.into());
    }
}

fn choose_file(_: &mut App, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Add a source".into(), ..Default::default() }.filter(
        "Documents",
        &["pdf", "docx", "pptx", "xlsx", "epub", "html", "htm", "md", "markdown", "txt", "csv", "json"],
    );
    tx.open_file(options, |s: &mut App, result, _| match result {
        Ok(Some(path)) => read_file(s, path),
        Ok(None) => {}
        Err(e) => set_add_error(s, &e.to_string()),
    });
}

/// Reads and extracts the file on a worker, then adds it as a document.
fn read_file(s: &mut App, path: std::path::PathBuf) {
    if let Some(a) = s.open.as_mut().and_then(|o| o.add.as_mut()) {
        a.busy = true;
        a.error = None;
    }
    let inbox = s.inbox();
    std::thread::spawn(move || {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "File".into());
        let result = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
            notebook_engine::extract::extract(&name, None, &bytes).map(|x| (x, bytes)).map_err(|e| e.to_string())
        });
        inbox.send(move |app| match result {
            Ok((x, bytes)) => app.add_source(SourceInput::Document {
                title: Some(x.title).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| name.clone()),
                content: x.text,
                file_name: Some(name),
                file: Some(bytes),
                mime: None,
            }),
            Err(e) => {
                if let Some(a) = app.open.as_mut().and_then(|o| o.add.as_mut()) {
                    a.busy = false;
                    a.error = Some(e);
                }
            }
        });
    });
}

// Chat

fn session_picker(o: &Open, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let labels: Vec<String> = o.sessions.iter().map(|s| s.title.clone()).collect();
    if labels.is_empty() {
        return el();
    }
    let current = o.session.as_ref().map(|s| s.session.title.as_str());
    let ids: Vec<String> = o.sessions.iter().map(|s| s.id.clone()).collect();
    rune_kit::Select::new("sessions", &labels)
        .label("Conversation")
        .current(current)
        .width(200.0)
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
            let picked = s.open.as_ref().and_then(|o| o.sessions.iter().position(|x| x.title == label)).and_then(|i| ids.get(i).cloned());
            if let Some(id) = picked {
                s.open_session(id);
            }
        })
        .build(k, layer)
        .key("session-picker")
}

fn citable(o: &Open) -> Vec<CitableSource> {
    o.sources.iter().map(|s| CitableSource { id: s.id.clone(), title: s.title.clone() }).collect()
}

fn chat(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let sources = citable(o);
    let mut thread = col().key("thread").w_full().max_w(720.0).gap(18.0).px(24.0).py(20.0);
    let messages = o.session.as_ref().map(|s| s.messages.as_slice()).unwrap_or(&[]);
    if messages.is_empty() && o.pending.is_none() {
        let hint = if o.sources.is_empty() { "Add a source, then ask about it." } else { "Ask about your sources. Answers cite the passages they come from." };
        thread = thread.child(col().w_full().pt(80.0).items_center().child(text(hint).font(theme::READ_HEADING).color(l.text3).align(rune::Align::Center).wrap()));
    }
    for (i, m) in messages.iter().enumerate() {
        thread = thread.child(message(m.kind, &m.content, &sources, false, l).key(format!("msg-{i}-{}", m.id)));
    }
    if let Some((question, reply)) = &o.pending {
        thread = thread.child(message(MessageKind::Human, question, &sources, false, l).key("pending-q"));
        let shown = if reply.is_empty() { "…" } else { reply.as_str() };
        thread = thread.child(message(MessageKind::Ai, shown, &sources, true, l).key("pending-a"));
    }
    if let Some(e) = &o.error {
        thread = thread.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap().key("chat-error"));
    }
    let scroller = col()
        .key("thread-scroll")
        .w_full()
        .items_center()
        .child(thread)
        .scroll(ScrollSpec { stick_bottom: true })
        .grow(1.0)
        .min_h(0.0);
    col().w_full().grow(1.0).min_h(0.0).child(scroller).child(composer(o, l))
}

fn message(kind: MessageKind, content: &str, sources: &[CitableSource], streaming: bool, l: &Look) -> El {
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
            let linked = citations::link_citations(content, sources);
            let cited: Vec<String> = linked.cited.iter().map(|c| c.id.clone()).collect();
            let md = linked.markdown.clone();
            let style = rune_kit::markdown::MarkdownStyle::from_kit(k).body_font(theme::READ_BODY).link(l.amber).text(l.text);
            let body = Markdown::new(linked.markdown)
                .style(style)
                .streaming(streaming)
                .selectable(true)
                .on_link_tx(move |s: &mut App, url: &str, tx: &mut Tx| {
                    if let Some(i) = url.strip_prefix("cite:").and_then(|n| n.parse::<usize>().ok()) {
                        if let Some(id) = cited.get(i) {
                            let at = md.find(&format!("](cite:{i})")).unwrap_or(0);
                            let query = citations::sentence_around(&md, at);
                            s.read_source(id.clone(), Some(query));
                        }
                    } else {
                        let _ = tx.open_url(url);
                    }
                })
                .build(k);
            let mut c = col().w_full().gap(6.0).child(body);
            if !streaming {
                let copy = content.to_owned();
                let save = content.to_owned();
                c = c.child(
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
                                .label("Save to notes")
                                .help("Save to notes")
                                .on_press(move |s: &mut App| s.save_reply_as_note(save.clone()))
                                .build(k)
                                .key("save"),
                        ),
                );
            }
            c
        }
    }
}

fn composer(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let sending = o.sending.is_some();
    let field = TextArea::new(&o.draft)
        .placeholder(if o.sources.is_empty() { "Ask anything" } else { "Ask about your sources" })
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
    let button = if sending {
        IconButton::new(&icons::STOP).variant(IconButtonVariant::Contrast).label("Stop").on_press(|s: &mut App| s.stop())
    } else {
        IconButton::new(&icons::SEND).variant(IconButtonVariant::Contrast).label("Send").disabled(o.draft.text.trim().is_empty()).on_press(|s: &mut App| s.send())
    };
    col().key("composer").w_full().items_center().px(24.0).pb(16.0).pt(8.0).child(
        row().w_full().max_w(720.0).gap(8.0).items_end().child(col().grow(1.0).min_w(0.0).child(field)).child(button.build(k).key("send")),
    )
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
        .child(TextInput::new(&d.title).width(None).placeholder("Title").plain().text_style(theme::READ_HEADING).on_edit(move |s: &mut App| &mut draft(s).title).build(k).key("note-title"))
        .child(TextArea::new(&d.content).width(None).placeholder("Write a note").auto_grow(4, 24).on_edit(move |s: &mut App| &mut draft(s).content).build(k).key("note-content"))
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
