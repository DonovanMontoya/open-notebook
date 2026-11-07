//! Open Notebook for macOS, drawn with Rune.
//!
//! `cargo run -p open-notebook-mac` opens the window. Headless:
//! `cargo run -p open-notebook-mac -- --snap notebook.png --scene notebook --size 1280x800`.

mod format;
mod listen;
mod nowplaying;
mod pages;
mod scenes;
mod server;
mod state;
mod study;
#[cfg(test)]
mod tests;
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
    // A loaded episode: remember the position, run the sleep timer and tell the system.
    changed |= s.tick_player(now);
    if let Some(line) = s.follow_line() {
        tx.scroll_center_in("np-transcript", line);
        changed = true;
    }
    // A playing episode: redraw the clock and transcript a few times a second; a sleep timer
    // counts down while paused too.
    let playing = s.playing.as_ref().is_some_and(|p| p.is_playing());
    if playing {
        tx.wake_after(std::time::Duration::from_millis(250));
        changed = true;
    } else if s.playing.as_ref().is_some_and(|p| p.sleep != listen::Sleep::Off) {
        tx.wake_after(std::time::Duration::from_secs(15));
    }
    changed
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if rune_kit::shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.is_command('n') {
        s.new_chat();
        return true;
    }
    if s.playing.is_some() && tx.focused().is_none() && player_key(s, ev) {
        return true;
    }
    if ev.is_command(',') {
        s.open_settings();
        return true;
    }
    false
}

/// The player's keys, unless a field has focus or a modifier is held: Space, ← back 15 s,
/// → forward 30 s, `>`/`.` faster, `<`/`,` slower and Esc to shrink the full player.
fn player_key(s: &mut App, ev: &KeyEvent) -> bool {
    let m = ev.mods;
    if m.cmd || m.ctrl || m.alt || m.logo {
        return false;
    }
    match &ev.key {
        rune::Key::Text(t) => match t.as_str() {
            " " => s.toggle_play(),
            ">" | "." => s.step_rate(true),
            "<" | "," => s.step_rate(false),
            _ => return false,
        },
        rune::Key::Left => s.skip(-listen::SKIP_BACK),
        rune::Key::Right => s.skip(listen::SKIP_FORWARD),
        rune::Key::Escape if s.confirm.is_none() && s.playing.as_ref().is_some_and(|p| p.expanded) => {
            if let Some(p) = s.playing.as_mut() {
                p.expanded = false;
            }
        }
        _ => return false,
    }
    true
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
