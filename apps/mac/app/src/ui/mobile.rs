//! The app without a sidebar: an iPhone, a folded iPhone Duo, or an open one.
//!
//! On a phone it is one column: home lists the notebooks, and a notebook has a back bar at the
//! top and its tabs at the bottom (Chat, Sources, Notes, Study, Listen). A source opens over
//! the notebook with its cited passage lit.
//!
//! An open Duo shows a notebook as two facing pages, like the book it is named after: the
//! sources, or the source being read, on the left; the conversation and the other tabs on the
//! right. When the Duo is partly folded the pages meet at the fold, so nothing sits in it.

use rune::{El, col, el, row, text};
use rune_kit::{IconButton, IconButtonVariant};

use super::{icons, listen, notebook};
use crate::state::{App, NotebookTab, Open, Route};
use crate::theme::{Look, Size};

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    if let Some(p) = &s.playing
        && p.expanded
    {
        return col().w_full().h_full().pb(s.inset_bottom).fill(l.panel).child(listen::player_bar(p, l));
    }
    // The last row pads for the home indicator: the player bar when one shows, else the content.
    let content_look = Look { inset_bottom: if s.playing.is_some() { 0.0 } else { s.inset_bottom }, ..l.clone() };
    let l = &content_look;
    let main = match &s.route {
        Route::Notebook(_) if s.open.is_some() => match l.size {
            Size::Phone => notebook_phone(s, l, layer),
            _ => notebook_pages(s, l, layer),
        },
        // Pages scroll under the home indicator and pad their own ends for it.
        Route::Home => super::page(s, l, layer),
        // Every other page keeps its own title; the bar only leads back.
        _ => col().w_full().h_full().min_h(0.0).child(back_bar("Notebooks", go_home, None, l)).child(super::page(s, l, layer)),
    };
    let mut c = col().key("mobile").w_full().h_full().min_h(0.0).fill(l.panel).child(col().w_full().grow(1.0).min_h(0.0).child(main));
    if let Some(p) = &s.playing {
        c = c.child(col().w_full().pb(s.inset_bottom).fill(l.panel).child(listen::player_bar(p, l)));
    }
    c
}

fn go_home(s: &mut App) {
    s.go(Route::Home);
}

/// A bar with a back button naming where it goes, and an optional trailing control.
fn back_bar(to: &str, on_back: fn(&mut App), trailing: Option<El>, l: &Look) -> El {
    let k = &l.k;
    let back = row()
        .key("back")
        .h(44.0)
        .pl(4.0)
        .pr(12.0)
        .gap(2.0)
        .radius(10.0)
        .center_y()
        .role("button")
        .label(format!("Back to {to}"))
        .child(rune::icon(&icons::BACK, 20.0).color(l.text2))
        .child(text(to.to_owned()).font(k.ty.body).color(l.text2).max_lines(1).shrink().min_w(0.0));
    let mut bar = row().key("back-bar").w_full().h(52.0).px(8.0).gap(4.0).center_y().child(k.interactive(back).on_press(on_back)).child(el().grow(1.0));
    if let Some(t) = trailing {
        bar = bar.child(t);
    }
    bar
}

fn notebook_settings(l: &Look) -> El {
    IconButton::new(&icons::GEAR)
        .variant(IconButtonVariant::GhostMuted)
        .label("Notebook settings")
        .on_press(notebook::open_details)
        .build(&l.k)
        .key("notebook-settings")
        .w(44.0)
        .h(44.0)
}

/// Back from a notebook on a phone: out of the source being read first, then home.
fn back_in_notebook(s: &mut App) {
    if let Some(o) = s.open.as_mut()
        && o.reading.is_some()
    {
        o.reading = None;
        o.active_cite = None;
        return;
    }
    s.go(Route::Home);
}

/// A notebook on a phone: back bar, one view, tabs at the bottom.
fn notebook_phone(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let o = s.open.as_ref().expect("a notebook is open");
    let name = o.notebook.as_ref().map(|n| n.name.clone()).unwrap_or_default();
    let reading = o.reading.is_some();
    let nav = if reading {
        back_bar(&name, back_in_notebook, None, l)
    } else {
        let back =
            IconButton::new(&icons::BACK).variant(IconButtonVariant::GhostMuted).label("Back to notebooks").on_press(go_home).build(&l.k).w(44.0).h(44.0);
        let search = IconButton::new(if o.search_open { &icons::CLOSE } else { &rune_kit::glyphs::SEARCH })
            .variant(IconButtonVariant::GhostMuted)
            .label(if o.search_open { "Close search" } else { "Search notebook" })
            .on_press_tx(|s: &mut App, tx: &mut rune::Tx| {
                if let Some(o) = s.open.as_mut() {
                    o.search_open = !o.search_open;
                    o.search.focused = o.search_open;
                }
                tx.blur();
            })
            .build(&l.k)
            .w(44.0)
            .h(44.0);
        row()
            .key("phone-navigation")
            .w_full()
            .h(56.0)
            .px(4.0)
            .gap(4.0)
            .center_y()
            .child(back)
            .child(text(name.clone()).key("notebook-name").font(crate::theme::READ_HEADING).color(l.text).grow(1.0).shrink().min_w(0.0).max_lines(1))
            .child(search)
            .child(notebook_settings(l))
    };
    let mut c = col().key("notebook").w_full().h_full().min_h(0.0).child(nav);
    if !reading && o.search_open {
        c = c.child(
            rune_kit::TextInput::new(&o.search)
                .height(48.0)
                .placeholder("Search notebook")
                .label("Search notebook")
                .width(None)
                .on_edit(|s: &mut App| &mut s.open.as_mut().expect("open").search)
                .on_change(|s: &mut App, _: &mut rune::Tx| s.search_changed())
                .build(&l.k)
                .key("notebook-search")
                .mx(16.0)
                .mb(8.0),
        );
        if let Some(hits) = &o.hits {
            c = c.child(notebook::search_results(hits, l));
        }
    }
    if reading || o.sources_tab {
        // The source being read, or the list of sources.
        c = c.child(col().w_full().grow(1.0).min_h(0.0).fill(l.page).pb(if reading { l.inset_bottom } else { 0.0 }).child(notebook::rail(o, l, layer)));
    } else {
        // Chat and Notes have controls above their content; the other tabs don't.
        if matches!(o.tab, NotebookTab::Chat | NotebookTab::Notes) {
            c = c.child(notebook::tab_head(o, l, layer));
        }
        c = c.child(notebook::rule(l)).child(col().w_full().grow(1.0).min_h(0.0).child(notebook::tab_body(s, o, l, layer)));
    }
    // The keyboard takes the bottom of the screen while it's up; the tabs come back after.
    if !reading && !s.keyboard_up {
        c = c.child(tab_bar(o, l));
    }
    notebook::overlays(o, l, layer);
    notebook::droppable(c)
}

/// A phone's tabs, along the bottom where a thumb reaches them.
fn tab_bar(o: &Open, l: &Look) -> El {
    let k = &l.k;
    const TABS: [(&str, Option<NotebookTab>, &rune::Icon); 5] = [
        ("Chat", Some(NotebookTab::Chat), &icons::CHAT),
        ("Sources", None, &icons::PAGES),
        ("Notes", Some(NotebookTab::Notes), &icons::PENCIL),
        ("Study", Some(NotebookTab::Study), &icons::CARDS),
        ("Listen", Some(NotebookTab::Listen), &icons::HEADPHONES),
    ];
    let mut bar = row().key("tab-bar").w_full().h(64.0).px(4.0).fill(l.panel);
    for (label, tab, icon) in TABS {
        let on = match tab {
            Some(t) => !o.sources_tab && o.tab == t,
            None => o.sources_tab,
        };
        let color = if on { l.text } else { l.text3 };
        let item = col()
            .key(format!("tab-{}", label.to_lowercase()))
            .grow(1.0)
            .h_full()
            .gap(4.0)
            .center()
            .role("tab")
            .label(label)
            .child(rune::icon(icon, 20.0).color(color))
            .child(text(label).font(if on { k.ty.small_medium } else { k.ty.small }).color(color));
        bar = bar.child(k.interactive(item).on_press(move |s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                match tab {
                    Some(t) => {
                        o.sources_tab = false;
                        o.tab = t;
                    }
                    None => o.sources_tab = true,
                }
            }
        }));
    }
    col().w_full().pb(l.inset_bottom).fill(l.panel).child(notebook::rule(l)).child(bar)
}

/// A notebook as two facing pages: what you read on the left, what you write on the right.
fn notebook_pages(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let o = s.open.as_ref().expect("a notebook is open");
    let (left_width, right_width) = match s.fold {
        Some([x, _, w, h]) if h >= w => (x, (l.width - x - w).max(0.0)),
        Some(_) => (l.width, l.width),
        None => ((l.width - 1.0) / 2.0, (l.width - 1.0) / 2.0),
    };
    let left_look = l.clone().sized(left_width);
    let right_look = l.clone().sized(right_width);
    let left = col()
        .key("left-page")
        .h_full()
        .min_h(0.0)
        .fill(l.page)
        .pb(l.inset_bottom)
        .child(back_bar("Notebooks", go_home, None, l))
        .child(col().w_full().grow(1.0).min_h(0.0).child(notebook::rail(o, &left_look, layer)));
    let mut right = col().key("right-page").h_full().min_h(0.0).pb(l.inset_bottom).child(notebook::header(o, l));
    if o.search_open
        && let Some(hits) = &o.hits
    {
        right = right.child(notebook::search_results(hits, l));
    }
    let right = right.child(notebook::tab_head(o, l, layer)).child(notebook::rule(l)).child(col().w_full().grow(1.0).min_h(0.0).child(notebook::tab_body(
        s,
        o,
        &right_look,
        layer,
    )));
    notebook::overlays(o, l, layer);
    // Meet at the fold when the Duo is partly folded; otherwise the left page takes two fifths.
    let body = match s.fold {
        Some([x, _, w, h]) if h >= w => {
            row().key("pages").w_full().h_full().min_h(0.0).child(left.w(x)).child(el().w(w).h_full().fill(l.page)).child(right.grow(1.0).min_w(0.0))
        }
        Some([_, y, _, h]) => {
            // Folded like a laptop: read on the top half, write on the bottom.
            col().key("pages").w_full().h_full().child(left.w_full().h(y)).child(el().w_full().h(h).fill(l.page)).child(right.w_full().grow(1.0).min_h(0.0))
        }
        None => {
            row().key("pages").w_full().h_full().min_h(0.0).child(left.w_frac(0.5)).child(el().w(1.0).h_full().fill(l.line)).child(right.grow(1.0).min_w(0.0))
        }
    };
    notebook::droppable(body)
}
