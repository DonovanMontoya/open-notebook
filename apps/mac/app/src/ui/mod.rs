//! The window: a sidebar of notebooks beside the desk, which shows the open notebook,
//! settings or the home screen.

mod add;
mod icons;
mod listen;
mod live;
mod notebook;
mod pages;
mod server;
mod settings;
pub mod study;

use rune::{Cx, El, Spring, TextEdit, Tx, col, row, text};
use rune_kit::shell::{PaneMotion, Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView, Toolbar};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, TextInput};

use crate::state::{App, Route};
use crate::theme::{self, Look};

pub fn look(cx: &Cx<App>, s: &App) -> Look {
    let dark = match s.settings.appearance.as_str() {
        "dark" => true,
        "light" => false,
        _ => cx.system_dark(),
    };
    Look::new(dark)
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let l = look(cx, s);
    let mut layer = Vec::new();
    // Every page has its own heading, so the title bar stays empty.
    let main = match &s.route {
        Route::Notebook(_) if s.open.is_some() => {
            // The notebook draws its own name and description.
            notebook::view(s, &l, &mut layer)
        }
        Route::Settings => settings::view(s, &l, &mut layer),
        Route::Ask => pages::ask(s, &l, &mut layer),
        Route::Library => pages::library(s, &l, &mut layer),
        Route::Transformations => pages::transformations(s, &l),
        Route::Usage => pages::usage(s, &l, &mut layer),
        Route::Server => server::view(s, &l, &mut layer),
        Route::Profiles => listen::profiles(s, &l, &mut layer),
        _ => home(s, &l),
    };
    if let Some(c) = &s.confirm {
        Dialog::new("confirm", c.question.clone())
            .open(true)
            .width(360.0)
            .child(
                row()
                    .gap(8.0)
                    .justify_end()
                    .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.confirm = None).build(&l.k).key("confirm-no"))
                    .child(
                        Button::new(c.verb)
                            .variant(if c.verb == "Delete" || c.verb == "Remove" { ButtonVariant::Danger } else { ButtonVariant::Contrast })
                            .on_press(|s: &mut App| {
                                if let Some(c) = s.confirm.take() {
                                    (c.yes)(s);
                                }
                            })
                            .build(&l.k)
                            .key("confirm-yes"),
                    ),
            )
            .on_close(|s: &mut App| s.confirm = None)
            .build(&l.k, &mut layer);
    }
    if s.new_notebook.is_some() {
        new_notebook(s, &l, &mut layer);
    }
    if let Some(message) = &s.toast {
        layer.push(toast(message, &l));
    }
    let mut desk = col().key("desk").w_full().h_full().min_h(0.0).fill(l.panel).child(col().w_full().grow(1.0).min_h(0.0).child(main));
    if let Some(p) = &s.playing {
        desk = desk.child(listen::player_bar(p, &l));
    }
    let split = SplitView::new(desk)
        .sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar(s, &l))
        .toolbar(Toolbar::new())
        .titlebar_inset(cx.titlebar_inset())
        .sidebar_padding(12.0, 8.0)
        .sidebar_toggle(SidebarToggle::new().size(ControlSize::Large).past_lights(16.0).edge(16.0).gap(4.0).in_header())
        .pane_motion(PaneMotion::new().spring(Spring::new(0.3, 0.0)))
        .reduced_motion(cx.is_reduced_motion())
        .build(&l.k);
    rune_kit::root(col().w_full().h_full().fill(l.page).child(split), layer)
}

fn sidebar(s: &App, l: &Look) -> El {
    let k = &l.k;
    let mut sb = Sidebar::new().label("Notebooks").row_gap(2.0).header_padding(0.0, 16.0);
    // The product, and where its notebooks are kept: on this Mac, or the server's host.
    let place = match &s.remote {
        Some(_) => crate::format::host_of(&s.settings.server_url).split('.').next().unwrap_or_default().to_owned(),
        None => "This device".to_owned(),
    };
    let place_help = if s.remote.is_some() { s.settings.server_url.clone() } else { "Stored on this device".into() };
    let brand = row()
        .key("data-place")
        .w_full()
        .px(8.0)
        .pb(12.0)
        .gap(8.0)
        .center_y()
        .help(place_help)
        .child(text("Open Notebook").font(k.ty.ui_medium).color(l.text).max_lines(1))
        .child(rune::el().grow(1.0))
        .child(rune::icon(if s.remote.is_some() { &icons::SERVER } else { &icons::DEVICE }, 12.0).color(l.text3))
        .child(text(place).font(k.ty.small).color(l.text3).max_lines(1));
    let nav = |label: &str, icon: &'static rune::Icon, route: Route, key: &str| {
        let selected = s.route == route;
        SidebarRow::new(label.to_owned())
            .icon(icon)
            .height(32.0)
            .selected(selected)
            .on_select(move |s: &mut App, _| s.go(route.clone()))
            .build(k)
            .key(key.to_owned())
    };
    sb = sb
        .header(brand)
        .header(nav("Ask", &rune_kit::glyphs::SEARCH, Route::Ask, "nav-ask"))
        .header(nav("Library", &icons::DOC, Route::Library, "nav-library"))
        .header(nav("Transformations", &rune_kit::glyphs::SPARKLE, Route::Transformations, "nav-transformations"))
        .header(nav("Usage", &icons::GAUGE, Route::Usage, "nav-usage"));
    let add = rune_kit::IconButton::new(&icons::PLUS)
        .label("New notebook")
        .variant(rune_kit::IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .on_press(open_new_notebook)
        .build(k)
        .key("new-notebook");
    let open: Vec<El> = s.notebooks.iter().filter(|n| !n.archived).map(|nb| notebook_row(s, nb, l)).collect();
    sb = sb.section(SidebarSection::new("Notebooks").key("notebooks").action(add).rows(open));
    let archived: Vec<&notebook_engine::types::Notebook> = s.notebooks.iter().filter(|n| n.archived).collect();
    if !archived.is_empty() {
        let rows: Vec<El> = archived.iter().map(|nb| notebook_row(s, nb, l)).collect();
        sb = sb.section(
            SidebarSection::new("Archived")
                .key("archived")
                .count(rows.len())
                .collapsed(!s.show_archived)
                .on_toggle(|s: &mut App, _| s.show_archived = !s.show_archived)
                .rows(rows),
        );
    }
    if s.remote.is_some() {
        sb = sb.footer(nav("Server", &icons::SERVER, Route::Server, "nav-server"));
    }
    sb.footer(
        SidebarRow::new("Settings")
            .icon(&icons::GEAR)
            .height(32.0)
            .selected(s.route == Route::Settings)
            .on_select(|s: &mut App, _| s.open_settings())
            .build(k)
            .key("settings"),
    )
    .build(k)
}

/// A name and what it's for, which the assistant reads before answering.
fn new_notebook(s: &App, l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    let Some(name) = &s.new_notebook else { return };
    let body = col()
        .w_full()
        .gap(12.0)
        .child(text("Name").font(k.ty.ui_medium).color(l.text2))
        .child(
            TextInput::new(name)
                .label("Name")
                .placeholder("Dark patterns literature review")
                .width(None)
                .on_edit(|s: &mut App| s.new_notebook.as_mut().expect("naming"))
                .on_submit(|s: &mut App, _: &mut Tx| s.create_notebook())
                .build(k)
                .key("new-notebook-name"),
        )
        .child(text("What it's for").font(k.ty.ui_medium).color(l.text2))
        .child(
            rune_kit::TextArea::new(&s.new_notebook_description)
                .label("What it's for")
                .placeholder("Optional. The assistant reads this before answering.")
                .width(None)
                .rows(3)
                .on_edit(|s: &mut App| &mut s.new_notebook_description)
                .build(k)
                .key("new-notebook-description"),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.new_notebook = None).build(k).key("new-notebook-cancel"))
                .child(
                    Button::new("Create notebook")
                        .variant(ButtonVariant::Contrast)
                        .disabled(name.text.trim().is_empty())
                        .on_press(|s: &mut App| s.create_notebook())
                        .build(k)
                        .key("new-notebook-create"),
                ),
        );
    Dialog::new("new-notebook", "New notebook")
        .open(true)
        .width(440.0)
        .fit()
        .child(body.px(20.0).pb(20.0))
        .on_close(|s: &mut App| s.new_notebook = None)
        .build(k, layer);
}

fn notebook_row(s: &App, nb: &notebook_engine::types::Notebook, l: &Look) -> El {
    let id = nb.id.clone();
    let selected = matches!(&s.route, Route::Notebook(open) if *open == nb.id);
    let detail = match nb.source_count {
        0 => String::new(),
        n => n.to_string(),
    };
    SidebarRow::new(nb.name.clone())
        .icon(&icons::BOOK)
        .detail(detail)
        .height(32.0)
        .selected(selected)
        .on_select(move |s: &mut App, _| s.open_notebook(id.clone()))
        .build(&l.k)
        .key(format!("nb-{}", nb.id))
}

fn open_new_notebook(s: &mut App) {
    let mut edit = TextEdit::new("");
    edit.focused = true;
    s.new_notebook = Some(edit);
    s.new_notebook_description = TextEdit::new("");
}

/// A centred message for the states with nothing to list.
fn home_message(title: &str, body: &str, action: Option<El>, l: &Look) -> El {
    let mut c = col().key("home").gap(10.0).items_center().max_w(420.0).child(text(title.to_owned()).font(theme::READ_HEADING).color(l.text));
    if !body.is_empty() {
        c = c.child(text(body.to_owned()).font(l.k.ty.ui).color(l.text2).wrap().align(rune::Align::Center));
    }
    if let Some(a) = action {
        c = c.child(col().pt(8.0).child(a));
    }
    col().w_full().h_full().center().window_drag().child(c)
}

fn home(s: &App, l: &Look) -> El {
    let k = &l.k;
    let settings_button =
        || Button::new("Open settings").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.open_settings()).build(k).key("home-settings");
    let new_button = |variant| Button::new("New notebook").icon(&icons::PLUS).variant(variant).on_press(open_new_notebook).build(k).key("home-new");
    if let Some(e) = &s.backend_error {
        return home_message("Couldn't open your notebooks", e, Some(settings_button()), l);
    }
    if !s.loaded {
        return home_message("Opening", "", None, l);
    }
    let needs_model = s.settings.mode == notebook_engine::settings::Mode::Local && s.settings.chat.is_none();
    let mut open: Vec<&notebook_engine::types::Notebook> = s.notebooks.iter().filter(|n| !n.archived).collect();
    if open.is_empty() {
        return if needs_model {
            home_message(
                "Add a model to start asking",
                "Notebooks live on this device. Questions go to the AI provider you choose, with your own API key.",
                Some(settings_button()),
                l,
            )
        } else {
            home_message("Start a notebook", "A notebook holds the sources for one question or project.", Some(new_button(ButtonVariant::Contrast)), l)
        };
    }
    open.sort_by(|a, b| b.updated.cmp(&a.updated));
    let mut c = col()
        .key("home")
        .w_full()
        .max_w(760.0)
        .px(32.0)
        .pt(40.0)
        .pb(32.0)
        .gap(24.0)
        .child(row().w_full().center_y().child(text("Notebooks").font(theme::READ_TITLE).color(l.text).grow(1.0)).child(new_button(ButtonVariant::Ghost)));
    if needs_model {
        c = c.child(
            row()
                .key("home-model")
                .w_full()
                .gap(16.0)
                .padding(16.0)
                .radius(12.0)
                .fill(l.raised)
                .center_y()
                .child(
                    text("Add a chat model to start asking. Questions go to the provider you choose, with your own key.")
                        .font(k.ty.ui)
                        .color(l.text2)
                        .wrap()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0),
                )
                .child(settings_button()),
        );
    }
    let mut list = col().key("home-list").w_full();
    for (i, nb) in open.into_iter().enumerate() {
        let mut meta = vec![match nb.source_count {
            1 => "1 source".to_owned(),
            n => format!("{n} sources"),
        }];
        if nb.note_count > 0 {
            meta.push(if nb.note_count == 1 { "1 note".into() } else { format!("{} notes", nb.note_count) });
        }
        let when = crate::format::relative_time(&nb.updated);
        if !when.is_empty() {
            meta.push(format!("updated {when}"));
        }
        let mut info = col().grow(1.0).shrink().min_w(0.0).gap(4.0).child(text(nb.name.clone()).font(theme::READ_HEADING).color(l.text).max_lines(1));
        if !nb.description.trim().is_empty() {
            info = info.child(text(nb.description.trim().to_owned()).font(k.ty.ui).color(l.text2).max_lines(2).wrap());
        }
        info = info.child(text(meta.join(" · ")).font(theme::tnum(k.ty.small)).color(l.text3));
        let id = nb.id.clone();
        let item = row().key(format!("home-nb-{}", nb.id)).w_full().px(12.0).py(16.0).radius(10.0).child(info);
        if i > 0 {
            list = list.child(row().key(format!("home-rule-{i}")).w_full().px(12.0).child(rune::el().grow(1.0).h(1.0).fill(l.line)));
        }
        list = list.child(k.interactive(item).on_press(move |s: &mut App| s.open_notebook(id.clone())));
    }
    col().w_full().h_full().items_center().child(c.child(list)).scroll(rune::ScrollSpec { stick_bottom: false }).window_drag()
}

fn toast(message: &str, l: &Look) -> El {
    col().key("toast-layer").absolute(0.0, 0.0).w_full().h_full().items_center().justify_end().pb(24.0).inert().child(
        row()
            .key("toast")
            .px(14.0)
            .py(9.0)
            .radius(10.0)
            .fill(l.raised)
            .border(1.0, l.line_strong)
            .shadow(8.0, 24.0, rune::Color::BLACK.alpha(if l.dark { 0.45 } else { 0.08 }))
            .child(text(message.to_owned()).font(l.k.ty.ui).color(l.text).max_w(520.0).wrap()),
    )
}

/// Clears the toast a few seconds after it appears. True when it went.
pub fn poll_toast(s: &mut App, tx: &mut Tx, now: f64) -> bool {
    match (&s.toast, s.toast_since) {
        (Some(_), None) => {
            s.toast_since = Some(now);
            tx.wake_after(std::time::Duration::from_secs(4));
            false
        }
        (Some(_), Some(t)) if now - t >= 3.9 => {
            s.toast = None;
            s.toast_since = None;
            true
        }
        (None, Some(_)) => {
            s.toast_since = None;
            false
        }
        _ => false,
    }
}
