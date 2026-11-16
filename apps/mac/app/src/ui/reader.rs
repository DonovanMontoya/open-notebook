//! Library materials open independently of notebook conversations.
use notebook_engine::types::SourceSummary;
use rune::{El, ScrollSpec, Tx, col, row, text};
use rune_kit::markdown::{Case, Highlight, Markdown, MarkdownStyle};
use rune_kit::{Button, ButtonVariant, TextArea, TextInput};

use crate::{
    pages::LibraryPage,
    state::App,
    theme::{self, Look},
};

pub fn metadata(src: &SourceSummary) -> String {
    let mut parts = Vec::new();
    if let Some(url) = src.asset.as_ref().and_then(|a| a.url.as_deref()) {
        parts.push(crate::format::host_of(url));
    }
    parts.push(format!("Added {}", crate::format::relative_time(&src.created)));
    parts.join(" · ")
}

pub fn close(s: &mut App) {
    if let Some(p) = s.library.as_mut() {
        if let Some(cancel) = &p.chat.sending {
            cancel.cancel();
        }
        p.reading = None;
        #[cfg(target_os = "macos")]
        {
            p.original_pdf = None;
        }
        p.opening = None;
        p.reader_error = None;
    }
}

/// Search the visible document text, skipping formatting markers and link destinations.
pub fn matches(body: &str, query: &str) -> Vec<Vec<std::ops::Range<usize>>> {
    Markdown::find(body, query, Case::Insensitive)
}

pub fn view(s: &App, p: &LibraryPage, l: &Look) -> El {
    let k = &l.k;
    let button = |label: &str| Button::new(label).variant(ButtonVariant::Ghost);
    let mut toolbar = row().w_full().gap(8.0).center_y().flex_wrap().child(button("Library").on_press(close).build(k).min_h(44.0).key("reader-back"));
    let Some(detail) = &p.reading else {
        let message = p.reader_error.clone().unwrap_or_else(|| "Opening material…".into());
        let mut body = col().w_full().gap(16.0).padding(24.0).child(text(message).font(k.ty.body).color(l.text2).wrap());
        if p.reader_error.is_some() {
            body = body.child(text("Return to Library and open the material to try again.").font(k.ty.ui).color(l.text3).wrap());
        }
        return col().w_full().h_full().child(toolbar.px(16.0)).child(body);
    };
    let src = &detail.summary;
    let title = src.title.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "Untitled material".into());
    #[cfg(target_os = "macos")]
    if p.original_pdf.is_some() {
        if s.confirm.is_some() || s.new_notebook.is_some() {
            return col().w_full().h_full();
        }
        let mut head = row()
            .w_full()
            .px(16.0)
            .gap(10.0)
            .center_y()
            .child(button("Library").on_press(close).build(k).min_h(44.0).key("reader-back"))
            .child(text(title).font(k.ty.ui_medium).color(l.text).max_lines(1).grow(1.0).min_w(0.0));
        if p.chat.visualization.is_some() {
            head = head
                .child(
                    Button::new("Original paper")
                        .variant(if p.chat.showing_visualization { ButtonVariant::Ghost } else { ButtonVariant::Contrast })
                        .on_press(|s: &mut App| {
                            if let Some(p) = &mut s.library {
                                p.chat.showing_visualization = false;
                            }
                        })
                        .build(k)
                        .min_h(44.0)
                        .key("reader-original"),
                )
                .child(
                    Button::new("Visualization")
                        .variant(if p.chat.showing_visualization { ButtonVariant::Contrast } else { ButtonVariant::Ghost })
                        .on_press(|s: &mut App| {
                            if let Some(p) = &mut s.library {
                                p.chat.showing_visualization = true;
                            }
                        })
                        .build(k)
                        .min_h(44.0)
                        .key("reader-visualization"),
                );
            if p.chat.showing_visualization {
                head = head.child(
                    button("Restart")
                        .on_press(|s: &mut App| {
                            if let Some(v) = s.library.as_mut().and_then(|p| p.chat.visualization.as_mut()) {
                                v.restart += 1;
                            }
                        })
                        .build(k)
                        .min_h(44.0)
                        .key("visualization-restart"),
                );
            }
        }
        let dark = l.dark;
        let identity = p.chat.visualization.as_ref().map(|v| format!("{}-{}", v.message_id, v.restart)).unwrap_or_default();
        let document = rune::el().key(format!("library-pdf-host-{}-{identity}", p.chat.showing_visualization)).grow(1.0).min_w(320.0).min_h(0.0).on_layout_tx(
            move |s: &mut App, rect, tx: &mut Tx| {
                let result = s.library.as_ref().and_then(|p| p.original_pdf.as_ref()).map(|pdf| pdf.show(rect));
                if let Some(Err(error)) = result
                    && let Some(p) = &mut s.library
                {
                    p.original_pdf = None;
                    p.reading = None;
                    p.reader_error = Some(error.to_string());
                } else {
                    if let Some(p) = &s.library {
                        let current = p.chat.visualization.as_ref().filter(|_| p.chat.showing_visualization);
                        crate::pdf::show_visualization(current.map(|v| v.document.html.as_str()), dark, current.map_or(0, |v| v.restart));
                    }
                    tx.live_only();
                }
            },
        );
        return col().key("library-reader").w_full().h_full().min_h(0.0).child(head).child(super::notebook::rule(l)).child(split(s, p, l, document));
    }

    let sizes = [20.0, 23.0, 26.0];
    let size = p.reader_size.min(sizes.len() - 1);
    for (label, delta) in [("Smaller text", -1isize), ("Larger text", 1)] {
        toolbar = toolbar.child(
            button(label)
                .disabled(if delta < 0 { size == 0 } else { size == 2 })
                .on_press(move |s: &mut App| {
                    if let Some(p) = s.library.as_mut() {
                        p.reader_size = p.reader_size.saturating_add_signed(delta).min(2);
                    }
                })
                .build(k)
                .min_h(44.0),
        );
    }
    if let Some(url) = src.asset.as_ref().and_then(|a| a.url.clone()).filter(|u| {
        let u = u.trim().to_ascii_lowercase();
        u.starts_with("https://") || u.starts_with("http://")
    }) {
        toolbar = toolbar.child(
            button("Open original")
                .on_press_tx(move |_: &mut App, tx: &mut Tx| {
                    let _ = tx.open_url(&url);
                })
                .build(k)
                .min_h(44.0),
        );
    }
    if !src.notebooks.is_empty() {
        let (id, nb) = (src.id.clone(), src.notebooks.first().cloned());
        toolbar = toolbar.child(
            button("Notebook workspace").on_press(move |s: &mut App| s.open_source(id.clone(), nb.clone(), None)).build(k).min_h(44.0).key("reader-workspace"),
        );
    }
    let hits = matches(&detail.full_text, &p.reader_query.text);
    let current = p.reader_match.min(hits.len().saturating_sub(1));
    let find = TextInput::new(&p.reader_query)
        .width(None)
        .height(44.0)
        .placeholder("Find in text")
        .label("Find in text")
        .on_edit(|s: &mut App| &mut s.library.as_mut().expect("library").reader_query)
        .on_change(|s: &mut App, tx: &mut Tx| {
            if let Some(p) = s.library.as_mut() {
                p.reader_match = 0;
                p.reader_jump = true;
            }
            let _ = tx;
        })
        .build(k)
        .grow(1.0)
        .min_w(120.0)
        .key("reader-find");
    let mut findbar = row().w_full().gap(8.0).center_y().child(find);
    if !p.reader_query.text.is_empty() {
        findbar = findbar
            .child(text(if hits.is_empty() { "No matches".into() } else { format!("{} / {}", current + 1, hits.len()) }).font(k.ty.small).color(l.text3));
        for (label, delta) in [("Previous", -1isize), ("Next", 1)] {
            let count = hits.len();
            findbar = findbar.child(
                button(label)
                    .disabled(count == 0)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        if let Some(p) = s.library.as_mut() {
                            p.reader_match = (current as isize + delta).rem_euclid(count.max(1) as isize) as usize;
                            p.reader_jump = true;
                        }
                        let _ = tx;
                    })
                    .build(k)
                    .min_h(44.0),
            );
        }
    }
    let font = rune::tok::TextStyle::new(sizes[size], 400.0).family(rune::tok::Family::Named("Newsreader")).lh(sizes[size] * 1.55);
    let mut style = MarkdownStyle::from_kit(k).body_font(font).text(l.text).link(l.amber);
    for (level, scale) in [(1, 1.6), (2, 1.3), (3, 1.1)] {
        style = style.heading_font(level, rune::tok::TextStyle { size: sizes[size] * scale, weight: 500.0, ..font }.lh(sizes[size] * scale * 1.25));
    }
    let highlights = hits.get(current).map(|ranges| (ranges.clone(), Highlight::current().bg(l.amber_wash).fg(l.text).key("library-match")));
    let mut body = Markdown::new(&detail.full_text)
        .style(style)
        .selectable(true)
        .tasks_read_only()
        .scroll_wide(true)
        .highlights(highlights)
        .on_link_tx(|_: &mut App, url: &str, tx: &mut Tx| {
            let scheme = url.trim().to_ascii_lowercase();
            if scheme.starts_with("https://") || scheme.starts_with("http://") {
                let _ = tx.open_url(url);
            }
        })
        .build(k)
        .key(format!("library-text-{}-{current}-{}", src.id, p.reader_query.text))
        .on_layout_tx(|s: &mut App, _, tx: &mut Tx| {
            if let Some(p) = s.library.as_mut()
                && std::mem::take(&mut p.reader_jump)
            {
                tx.scroll_center("library-match");
            }
        });
    if detail.full_text.trim().is_empty() {
        body = text(if crate::state::is_processing(src.status.as_deref()) {
            "This material is still being processed. Return to Library and reopen it when processing finishes."
        } else {
            "No readable text is available. Open the original or use the notebook workspace to access the source file."
        })
        .font(font)
        .color(l.text2)
        .wrap();
    }
    let words = detail.full_text.split_whitespace().count();
    let article = col()
        .key(format!("article-{}", src.id))
        .w_full()
        .max_w(760.0)
        .px(if l.phone() { 20.0 } else { 40.0 })
        .pt(40.0)
        .pb(64.0 + l.inset_bottom)
        .gap(24.0)
        .child(text(title).font(rune::tok::TextStyle { size: 32.0, ..theme::READ_TITLE }.lh(39.0)).color(l.text).wrap())
        .child(text(format!("{} · {} min read", metadata(src), words.div_ceil(220).max(1))).font(k.ty.small).color(l.text3).wrap())
        .child(body);
    col()
        .key("library-reader")
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(l.page)
        .child(toolbar.px(16.0).py(6.0))
        .child(findbar.px(16.0).pb(8.0).flex_wrap())
        .child(super::notebook::rule(l))
        .child(
            col()
                .key(format!("library-reading-scroll-{}", src.id))
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .items_center()
                .child(article)
                .scroll(ScrollSpec { stick_bottom: false }),
        )
}

pub(super) fn split(s: &App, p: &LibraryPage, l: &Look, document: El) -> El {
    let width = p.split.width();
    let grip = rune::el()
        .key("reader-chat-grip")
        .w(8.0)
        .h_full()
        .cursor(rune::Cursor::ResizeColumn)
        .role("separator")
        .label("Resize discussion")
        .range_value(width as f64, crate::visualization::ReaderSplit::MIN as f64, p.split.max() as f64)
        .range_step(16.0)
        .child(rune::el().w(1.0).h_full().fill(l.line))
        .on_drag(|s: &mut App, drag: &mut rune::Drag, tx: &mut Tx| {
            let Some(p) = &mut s.library else { return };
            match drag.phase {
                rune::Phase::Down => drag.data = p.split.width(),
                rune::Phase::Move => {
                    p.split.resize(drag.data - drag.dx);
                    tx.instant();
                }
                rune::Phase::Up => {
                    if drag.cancelled() {
                        p.split.resize(drag.data);
                    }
                    let _ = s.prefs.set("reader.chat-width", &p.split.preferred);
                }
            }
        })
        .on_double_press(|s: &mut App| {
            if let Some(p) = &mut s.library {
                p.split.resize(380.0);
                let _ = s.prefs.set("reader.chat-width", &p.split.preferred);
            }
        })
        .on_step(|s: &mut App, steps, _: &mut Tx| {
            if let Some(p) = &mut s.library {
                p.split.resize(p.split.width() + steps as f32 * 16.0);
                let _ = s.prefs.set("reader.chat-width", &p.split.preferred);
            }
        });
    row()
        .key("reader-split")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(document)
        .child(grip)
        .child(discussion(s, p, l).w(width).min_w(crate::visualization::ReaderSplit::MIN))
        .on_layout_tx(|s: &mut App, rect, tx: &mut Tx| {
            if let Some(p) = &mut s.library
                && (p.split.available - rect[2]).abs() > 0.5
            {
                p.split.available = rect[2];
            } else {
                tx.live_only();
            }
        })
}

fn response(id: &str, content: &str, streaming: bool, l: &Look) -> El {
    use crate::visualization::HtmlAnswer;
    match crate::visualization::parse(content) {
        Some(HtmlAnswer::Complete(document)) if !streaming => {
            let (before, after, id) = (document.before.clone(), document.after.clone(), id.to_owned());
            let key = format!("visualization-open-{id}");
            col()
                .w_full()
                .gap(10.0)
                .child(answer(&before, l))
                .child(
                    Button::new("Open interactive figure")
                        .on_press(move |s: &mut App| s.open_reading_visualization(id.clone(), document.clone()))
                        .build(&l.k)
                        .min_h(44.0)
                        .key(key),
                )
                .child(answer(&after, l))
        }
        Some(HtmlAnswer::Complete(v)) => {
            col().gap(8.0).child(answer(&v.before, l)).child(text("Building interactive figure…").font(l.k.ty.small).color(l.text3))
        }
        Some(HtmlAnswer::Incomplete(before)) => col().gap(8.0).child(answer(&before, l)).child(
            text(if streaming { "Building interactive figure…" } else { "The visualization was incomplete. Ask to generate it again." })
                .font(l.k.ty.small)
                .color(l.text3)
                .wrap(),
        ),
        Some(HtmlAnswer::TooLarge(before)) => col()
            .gap(8.0)
            .child(answer(&before, l))
            .child(text("This visualization is too large to open. Ask for a smaller explanation.").font(l.k.ty.small).color(l.danger).wrap()),
        None => answer(content, l),
    }
}

fn discussion(s: &App, p: &LibraryPage, l: &Look) -> El {
    let k = &l.k;
    let chat = &p.chat;
    let mut content = col().w_full().gap(14.0).px(16.0).py(16.0);
    if chat.loading {
        content = content.child(text("Loading conversation…").font(k.ty.ui).color(l.text3));
    }
    if let Some(session) = &chat.session {
        for message in &session.messages {
            let human = message.kind == notebook_engine::types::MessageKind::Human;
            let shown = if human {
                message.content.strip_prefix("Question: ").unwrap_or(&message.content).split("\n\nReading focus:").next().unwrap_or("").to_owned()
            } else {
                message.content.clone()
            };
            content = content.child(
                col().w_full().gap(4.0).child(text(if human { "You" } else { "Answer" }).font(k.ty.small_medium).color(l.text3)).child(if human {
                    answer(&shown, l)
                } else {
                    response(&message.id, &shown, false, l)
                }),
            );
        }
    }
    if let Some((question, reply)) = &chat.pending {
        content = content.child(text(question.clone()).font(k.ty.body).color(l.text).wrap()).child(if reply.is_empty() {
            text("Reading the material…").font(k.ty.ui).color(l.text3)
        } else {
            response("pending", reply, true, l)
        });
    } else if chat.session.as_ref().is_none_or(|s| s.messages.is_empty()) {
        content = content.child(text("Ask about this paper").font(theme::READ_HEADING).color(l.text)).child(
            text("Select a passage to discuss it. Choose a figure to include its original page, caption and related text.").font(k.ty.ui).color(l.text3).wrap(),
        );
    }
    if let Some(error) = &chat.error {
        content = content.child(text(error.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    let mut composer = col().w_full().padding(14.0).gap(10.0);
    if let Some(context) = &chat.context {
        let mut card = col()
            .w_full()
            .gap(6.0)
            .padding(10.0)
            .radius(8.0)
            .fill(l.raised)
            .child(
                row().w_full().center_y().child(text(format!("{} · p. {}", context.label, context.page)).font(k.ty.ui_medium).color(l.text).grow(1.0)).child(
                    Button::new("Clear")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut App| {
                            if let Some(p) = s.library.as_mut() {
                                p.chat.context = None;
                            }
                        })
                        .build(k),
                ),
            )
            .child(text(context.text.clone()).font(k.ty.small).color(l.text2).max_lines(4).wrap());
        if context.png.is_some() {
            card = card.child(text("Original page image included").font(k.ty.small).color(l.text3));
        }
        let figure_page = context.page;
        if context.kind == "figure" {
            let disabled = chat.sending.is_some() || chat.loading || (s.remote.is_none() && s.settings.chat.is_none());
            card = card.child(
                row()
                    .w_full()
                    .gap(6.0)
                    .flex_wrap()
                    .child(
                        Button::new("Visualize")
                            .disabled(disabled)
                            .on_press(|s: &mut App| s.visualize_reading_figure(false))
                            .build(k)
                            .min_h(44.0)
                            .key("figure-visualize"),
                    )
                    .child(
                        Button::new("Animate")
                            .disabled(disabled)
                            .on_press(|s: &mut App| s.visualize_reading_figure(true))
                            .build(k)
                            .min_h(44.0)
                            .key("figure-animate"),
                    ),
            );
            card = card.child(
                Button::new("Show figure page")
                    .variant(ButtonVariant::Ghost)
                    .on_press(move |s: &mut App| {
                        if let Some(p) = &mut s.library {
                            p.chat.showing_visualization = false;
                        }
                        #[cfg(target_os = "macos")]
                        crate::pdf::navigate(figure_page, "");
                    })
                    .build(k),
            );
        }
        for reference in context.references.iter().take(4) {
            let page = reference.page;
            let passage = reference.text.clone();
            card = card
                .child(
                    Button::new(format!("Related text · p. {page}"))
                        .variant(ButtonVariant::Ghost)
                        .on_press(move |s: &mut App| {
                            if let Some(p) = &mut s.library {
                                p.chat.showing_visualization = false;
                            }
                            #[cfg(target_os = "macos")]
                            crate::pdf::navigate(page, &passage);
                        })
                        .build(k),
                )
                .child(text(reference.text.clone()).font(k.ty.small).color(l.text3).max_lines(2).wrap());
        }
        composer = composer.child(card.max_h(260.0).scroll(ScrollSpec { stick_bottom: false }));
    }
    let needs_model = s.remote.is_none() && s.settings.chat.is_none();
    let needs_notebook = s.remote.is_none() && p.reading.as_ref().is_some_and(|d| d.summary.notebooks.is_empty());
    if needs_notebook {
        composer = composer.child(text("Add this material to a notebook in Manage materials to start a discussion.").font(k.ty.small).color(l.text3).wrap());
    }
    if needs_model {
        composer =
            composer.child(Button::new("Choose a chat model in Settings").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.open_settings()).build(k));
    }
    composer = composer.child(
        TextArea::new(&chat.draft)
            .label("Question about this material")
            .placeholder("Ask about the paper or selected context…")
            .width(None)
            .rows(3)
            .on_edit(|s: &mut App| &mut s.library.as_mut().expect("Library").chat.draft)
            .build(k)
            .key("reader-question"),
    );
    let action = if chat.sending.is_some() {
        Button::new("Stop")
            .variant(ButtonVariant::Ghost)
            .on_press(|s: &mut App| {
                if let Some(cancel) = s.library.as_ref().and_then(|p| p.chat.sending.as_ref()) {
                    cancel.cancel();
                }
            })
            .build(k)
    } else {
        Button::new("Ask")
            .variant(ButtonVariant::Contrast)
            .disabled(needs_model || needs_notebook || chat.loading || chat.draft.text.trim().is_empty())
            .on_press(App::send_reading_question)
            .build(k)
    };
    col()
        .key("reader-discussion")
        .h_full()
        .min_h(0.0)
        .border(1.0, l.line)
        .fill(l.panel)
        .child(text("Discussion").font(k.ty.ui_medium).color(l.text).px(16.0).py(14.0))
        .child(content.scroll(ScrollSpec { stick_bottom: true }).grow(1.0).min_h(0.0))
        .child(composer.child(action.key("reader-ask")))
}

fn answer(content: &str, l: &Look) -> El {
    Markdown::new(content)
        .style(MarkdownStyle::from_kit(&l.k).body_font(theme::READ_BODY).text(l.text).link(l.amber))
        .selectable(true)
        .on_link_tx(|s: &mut App, url: &str, _: &mut Tx| {
            if let Some(page) = url.strip_prefix("pdf-page:").and_then(|p| p.parse::<u32>().ok()) {
                if let Some(p) = &mut s.library {
                    p.chat.showing_visualization = false;
                }
                #[cfg(target_os = "macos")]
                crate::pdf::navigate(page, "");
            }
        })
        .build(&l.k)
}

#[cfg(test)]
mod tests {
    use super::matches;
    #[test]
    fn find_searches_visible_formatted_text() {
        let body = "# Reading\n\n前 **café** and *notes*. [Reference](https://hidden.example)";
        let hits = matches(body, "CAFÉ and notes");
        assert_eq!(hits.len(), 1);
        let visible: String = hits[0].iter().map(|r| &body[r.clone()]).collect();
        assert_eq!(visible, "café and notes");
        assert!(matches(body, "").is_empty());
        assert!(matches(body, "hidden.example").is_empty());
        assert!(matches(body, "**").is_empty());
    }

    #[test]
    fn dragging_the_discussion_edge_changes_its_width_and_saves_it() {
        use crate::{pages::LibraryPage, state::App, theme::Look};
        fn view(s: &App, _: &mut rune::Cx<App>) -> rune::El {
            super::split(s, s.library.as_ref().unwrap(), &Look::new(false), rune::el().key("paper-slot").grow(1.0).min_w(320.0))
        }
        let mut app = App::offline();
        app.library = Some(LibraryPage::default());
        let mut rt = rune::Runtime::new(app, view, (1100.0, 800.0), rune::Env::new(false, 1.0));
        for n in 0..3 {
            rt.frame(n as f64 / 60.0);
        }
        let before = rt.presented("reader-discussion").unwrap().0[2];
        let (x, y) = rt.find("reader-chat-grip").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_move(x - 160.0, y);
        rt.pointer_up(x - 160.0, y);
        rt.frame(0.2);
        let width = rt.presented("reader-discussion").unwrap().0[2];
        assert!(width > before + 140.0, "before={before} after={width}");
        assert_eq!(rt.state.prefs.get::<f32>("reader.chat-width"), Some(width));
        assert!(rt.presented("paper-slot").unwrap().0[2] >= 320.0);
    }
}
