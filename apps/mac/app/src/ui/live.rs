//! The Live talk card at the top of the Listen tab. Port of `LiveTalk.tsx`'s markup.

use notebook_engine::podcast::live_talk::{LiveStatus, MAX_MINUTES, VOICES};
use rune::{El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Select, Spinner};

use super::icons;
use crate::listen::clock;
use crate::live::unavailable;
use crate::state::App;
use crate::theme::{self, Look};

fn title_case(v: &str) -> String {
    let mut c = v.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The line under the title, as in the TypeScript card.
pub fn status_line(s: &App) -> String {
    let live = &s.live;
    match live.status {
        LiveStatus::Connected => format!("Listening. Speak naturally; you can interrupt. {} of {MAX_MINUTES}:00", clock(live.elapsed())),
        LiveStatus::Connecting => "Connecting".into(),
        LiveStatus::Closing => "Ending".into(),
        _ => unavailable(s).map_or("Talk through this notebook out loud. It knows your sources and notes.", |why| why).into(),
    }
}

pub fn card(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let live = &s.live;
    let on = live.status == LiveStatus::Connected;
    let running = live.status.live();
    let blocked = unavailable(s).is_some();

    let title = row()
        .gap(8.0)
        .center_y()
        .child(el().w(8.0).h(8.0).radius(4.0).fill(if on { l.amber } else { l.text3 }))
        .child(text("Talk it through").font(theme::READ_HEADING).color(l.text));
    let words = col().grow(1.0).min_w(0.0).gap(2.0).child(title).child(text(status_line(s)).font(k.ty.small).color(l.text3).wrap());

    let mut actions = row().gap(6.0).center_y();
    if !running && !blocked {
        let names: Vec<String> = VOICES.iter().map(|v| title_case(v)).collect();
        let current = names[s.live_voice()].clone();
        actions = actions.child(
            Select::new("live-voice", &names)
                .current(Some(current.as_str()))
                .width(110.0)
                .open(live.voice_open)
                .on_toggle(|s: &mut App| s.live.voice_open = !s.live.voice_open)
                .on_close(|s: &mut App| s.live.voice_open = false)
                .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
                    if let Some(i) = VOICES.iter().position(|v| title_case(v) == name) {
                        s.live_choose_voice(i);
                    }
                })
                .build(k, layer),
        );
    }
    if on {
        actions = actions.child(
            Button::new(if live.muted { "Unmute" } else { "Mute" })
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| s.live_mute())
                .build(k)
                .key("live-mute"),
        );
    }
    if running {
        if live.status == LiveStatus::Connecting {
            actions = actions.child(Spinner::new().size(12.0).build(k));
        }
        actions = actions.child(
            Button::new("End")
                .icon(&icons::STOP)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .disabled(live.status == LiveStatus::Closing)
                .on_press(|s: &mut App| s.live_stop())
                .build(k)
                .key("live-end"),
        );
    } else {
        actions = actions.child(
            Button::new("Start talking").control_size(ControlSize::Small).disabled(blocked).on_press(|s: &mut App| s.live_start()).build(k).key("live-start"),
        );
    }

    let mut c = col()
        .key("live-talk")
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, if on { l.amber } else { l.line })
        .child(row().w_full().gap(10.0).center_y().child(words).child(actions));
    if let Some(e) = &live.error {
        c = c.child(text(e.clone()).font(k.ty.small).color(l.danger).wrap());
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use notebook_engine::settings::Mode;

    #[test]
    fn the_status_line_follows_the_call() {
        let mut s = App::offline();
        assert_eq!(status_line(&s), "Talk through this notebook out loud. It knows your sources and notes.");
        s.live.status = LiveStatus::Connecting;
        assert_eq!(status_line(&s), "Connecting");
        s.live.status = LiveStatus::Connected;
        assert_eq!(status_line(&s), "Listening. Speak naturally; you can interrupt. 0:00 of 30:00");
        s.live.status = LiveStatus::Closing;
        assert_eq!(status_line(&s), "Ending");
        s.live.status = LiveStatus::Idle;
        s.settings.mode = Mode::Remote;
        assert_eq!(status_line(&s), crate::live::SERVER_MODE);
        assert_eq!(title_case("marin"), "Marin");
    }
}
