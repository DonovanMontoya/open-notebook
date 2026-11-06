//! The window: a sidebar of notebooks beside the desk, which shows the open notebook,
//! settings or the home screen.

mod icons;
mod notebook;
mod pages;
mod server;
mod settings;
mod study;

use rune::{Cx, El, Spring, TextEdit, Tx, col, row, text};
use rune_kit::shell::{PaneMotion, Sidebar, SidebarRow, SidebarToggle, SplitView, Toolbar};
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
    let (main, title) = match &s.route {
        Route::Notebook(_) if s.open.is_some() => (notebook::view(s, &l, &mut layer), s.open.as_ref().and_then(|o| o.notebook.as_ref()).map(|n| n.name.clone()).unwrap_or_default()),
        Route::Settings => (settings::view(s, &l, &mut layer), "Settings".to_owned()),
        Route::Ask => (pages::ask(s, &l, &mut layer), "Ask".to_owned()),
        Route::Library => (pages::library(s, &l), "Library".to_owned()),
        Route::Transformations => (pages::transformations(s, &l), "Transformations".to_owned()),
        Route::Usage => (pages::usage(s, &l), "Usage".to_owned()),
        Route::Server => (server::view(s, &l, &mut layer), "Server".to_owned()),
        _ => (home(s, &l), String::new()),
    };
    if let Some((question, _)) = &s.confirm {
        Dialog::new("confirm", question.clone())
            .open(true)
            .width(360.0)
            .child(
                row()
                    .gap(8.0)
                    .justify_end()
                    .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| s.confirm = None).build(&l.k).key("confirm-no"))
                    .child(
                        Button::new("Delete")
                            .variant(ButtonVariant::Danger)
                            .on_press(|s: &mut App| {
                                if let Some((_, yes)) = s.confirm.take() {
                                    yes(s);
                                }
                            })
                            .build(&l.k)
                            .key("confirm-yes"),
                    ),
            )
            .on_close(|s: &mut App| s.confirm = None)
            .build(&l.k, &mut layer);
    }
    if let Some(message) = &s.toast {
        layer.push(toast(message, &l));
    }
    let desk = col().key("desk").w_full().h_full().min_h(0.0).fill(l.panel).child(main);
    let split = SplitView::new(desk)
        .sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar(s, &l))
        .toolbar(Toolbar::new().title(title))
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
    let mut sb = Sidebar::new().label("Notebooks").row_gap(2.0);
    let new_row = if let Some(edit) = &s.new_notebook {
        TextInput::new(edit)
            .placeholder("Notebook name")
            .on_edit(|s: &mut App| s.new_notebook.as_mut().expect("editing"))
            .on_submit(|s: &mut App, _: &mut Tx| s.create_notebook())
            .on_key(|s: &mut App, ev: &rune::KeyEvent, _: &mut Tx| {
                if ev.key == rune::Key::Escape {
                    s.new_notebook = None;
                    return true;
                }
                false
            })
            .build(k)
            .key("new-notebook-name")
    } else {
        SidebarRow::new("New notebook")
            .icon(&icons::PLUS)
            .height(30.0)
            .on_select(|s: &mut App, _| {
                let mut edit = TextEdit::new("");
                edit.focused = true;
                s.new_notebook = Some(edit);
            })
            .build(k)
            .key("new-notebook")
    };
    let nav = |label: &str, icon: &'static rune::Icon, route: Route, key: &str| {
        let selected = s.route == route;
        SidebarRow::new(label.to_owned()).icon(icon).height(30.0).selected(selected).on_select(move |s: &mut App, _| s.go(route.clone())).build(k).key(key.to_owned())
    };
    sb = sb
        .header(nav("Ask", &rune_kit::glyphs::SEARCH, Route::Ask, "nav-ask"))
        .header(nav("Library", &icons::DOC, Route::Library, "nav-library"))
        .header(nav("Transformations", &rune_kit::glyphs::SPARKLE, Route::Transformations, "nav-transformations"))
        .header(nav("Usage", &icons::GAUGE, Route::Usage, "nav-usage"))
        .header(new_row);
    for nb in s.notebooks.iter().filter(|n| !n.archived) {
        let id = nb.id.clone();
        let selected = matches!(&s.route, Route::Notebook(open) if *open == nb.id);
        let detail = match nb.source_count {
            0 => String::new(),
            n => n.to_string(),
        };
        sb = sb.child(
            SidebarRow::new(nb.name.clone())
                .icon(&icons::BOOK)
                .detail(detail)
                .height(30.0)
                .selected(selected)
                .on_select(move |s: &mut App, _| s.open_notebook(id.clone()))
                .build(k)
                .key(format!("nb-{}", nb.id)),
        );
    }
    if s.remote.is_some() {
        sb = sb.footer(nav("Server", &icons::SERVER, Route::Server, "nav-server"));
    }
    sb.footer(
        SidebarRow::new("Settings")
            .icon(&icons::GEAR)
            .height(30.0)
            .selected(s.route == Route::Settings)
            .on_select(|s: &mut App, _| s.open_settings())
            .build(k)
            .key("settings"),
    )
    .build(k)
}

fn home(s: &App, l: &Look) -> El {
    let k = &l.k;
    let (title, body, action): (&str, String, Option<El>) = if let Some(e) = &s.backend_error {
        (
            "Couldn't open your notebooks",
            e.clone(),
            Some(Button::new("Open settings").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.open_settings()).build(k).key("home-settings")),
        )
    } else if !s.loaded {
        ("Opening", String::new(), None)
    } else if s.settings.mode == notebook_engine::settings::Mode::Local && s.settings.chat.is_none() {
        (
            "Add a model to start asking",
            "Notebooks live on this device. Questions go to the AI provider you choose, with your own API key.".into(),
            Some(Button::new("Open settings").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.open_settings()).build(k).key("home-settings")),
        )
    } else if let Some(latest) = s.notebooks.iter().find(|n| !n.archived) {
        let id = latest.id.clone();
        (
            "Pick a notebook",
            "Or continue where you left off.".into(),
            Some(Button::new(format!("Open {}", latest.name)).on_press(move |s: &mut App| s.open_notebook(id.clone())).build(k).key("home-open")),
        )
    } else {
        ("Start a notebook", "A notebook holds the sources for one question or project. Use New notebook in the sidebar.".into(), None)
    };
    let mut c = col().key("home").gap(10.0).items_center().max_w(420.0).child(text(title).font(theme::READ_HEADING).color(l.text));
    if !body.is_empty() {
        c = c.child(text(body).font(l.k.ty.ui).color(l.text2).wrap().align(rune::Align::Center));
    }
    if let Some(a) = action {
        c = c.child(col().pt(6.0).child(a));
    }
    col().w_full().h_full().center().window_drag().child(c)
}

fn toast(message: &str, l: &Look) -> El {
    col().key("toast-layer").w_full().h_full().items_center().justify_end().pb(24.0).inert().child(
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
