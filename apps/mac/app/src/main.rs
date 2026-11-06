//! Open Notebook for macOS, drawn with Rune.
//!
//! `cargo run -p open-notebook-mac` opens the window. Headless:
//! `cargo run -p open-notebook-mac -- --snap notebook.png --scene notebook --size 1280x800`.

mod scenes;
#[cfg(test)]
mod tests;
mod state;
mod theme;
mod ui;

use rune::{AppSpec, Chrome, KeyEvent, Tx};
use state::App;

fn poll(s: &mut App, now: f64, tx: &mut Tx) -> bool {
    let mut changed = s.drain();
    changed |= ui::poll_toast(s, tx, now);
    // Sources being read: check again every two seconds until they finish.
    if s.open.as_ref().is_some_and(|o| o.processing()) {
        let due = s.next_refresh.is_none_or(|t| now >= t);
        if due {
            s.next_refresh = Some(now + 2.0);
            s.refresh_sources();
        }
        tx.wake_after(std::time::Duration::from_secs(2));
    } else {
        s.next_refresh = None;
    }
    changed
}

fn on_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    if rune_kit::shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.is_command('n') {
        s.new_chat();
        return true;
    }
    if ev.is_command(',') {
        s.open_settings();
        return true;
    }
    false
}

fn init() -> App {
    if scenes::requested() { App::offline() } else { App::init() }
}

fn main() {
    let spec = AppSpec {
        min_size: Some((720.0, 480.0)),
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        scenes: scenes::SCENES,
        setup: Some(|s: &mut App, waker| s.set_waker(waker)),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Open Notebook", (1280.0, 800.0), init, ui::view)
    };
    let chrome = Chrome { full_size_content: true, traffic_lights: Some((16.0, 18.0)), drag_regions: true };
    rune::App::new(spec).chrome(chrome).run();
}
