//! The Listen tab (episodes and the form that makes one), the player bar, and the profiles page.

use notebook_engine::podcast::voices::VoiceKind;
use notebook_engine::types::*;
use rune::{El, PlayerState, ScrollSpec, TextEdit, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, SegmentedControl, Select, TextArea, TextInput};

use super::icons;
use crate::listen::{EFFORTS, EpisodeDraft, NewEpisode, NowPlaying, ProfilesPage, RATES, SpeakerDraft, clock};
use crate::state::{App, Open};
use crate::theme::{self, Look};

fn form(s: &mut App) -> &mut NewEpisode {
    s.open.as_mut().and_then(|o| o.new_episode.as_mut()).expect("new episode form")
}

fn small(label: &str, key: &str, l: &Look, f: impl Fn(&mut App) + 'static) -> El {
    Button::new(label.to_owned()).variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press(f).build(&l.k).key(key.to_owned())
}

pub fn view(s: &App, o: &Open, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let mut c = col().key("listen").w_full().max_w(720.0).gap(14.0).px(24.0).py(20.0);
    let mut actions = row().gap(4.0);
    if o.new_episode.is_none() {
        actions = actions.child(
            Button::new("New episode")
                .icon(&icons::PLUS)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| s.new_episode_form())
                .build(k)
                .key("new-episode"),
        );
    }
    if s.local_podcasts() {
        actions = actions.child(small("Profiles", "open-profiles", l, |s| s.open_profiles()));
    }
    c = c.child(super::live::card(s, l, layer));
    c = c.child(actions);
    if let Some(f) = &o.new_episode {
        c = c.child(new_episode(s, f, l, layer));
    }
    if o.episodes.is_empty() && o.new_episode.is_none() {
        c = c.child(text("Turn this notebook's sources into a conversation you can listen to.").font(theme::READ_HEADING).color(l.text3).wrap().pt(40.0));
    }
    for e in &o.episodes {
        c = c.child(episode_card(s, o, e, l));
    }
    col().w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn new_episode(s: &App, f: &NewEpisode, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let profiles = s.podcast_profiles.clone().unwrap_or_default();
    let names: Vec<String> = profiles.episode.iter().map(|p| p.name.clone()).collect();
    let current = f.episode_profile.as_ref().and_then(|id| profiles.episode.iter().find(|p| &p.id == id));
    let episode_ids: Vec<String> = profiles.episode.iter().map(|p| p.id.clone()).collect();
    let profile = Select::new("episode-profile", &names)
        .label("Episode profile")
        .placeholder("Episode profile")
        .current(current.map(|p| p.name.as_str()))
        .width(260.0)
        .open(f.open_select == Some("episode"))
        .on_toggle(|s: &mut App| {
            let f = form(s);
            f.open_select = if f.open_select == Some("episode") { None } else { Some("episode") };
        })
        .on_close(|s: &mut App| form(s).open_select = None)
        .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
            let names: Vec<String> = s.podcast_profiles.as_ref().map(|p| p.episode.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
            let f = form(s);
            f.episode_profile = names.iter().position(|n| n == name).and_then(|i| episode_ids.get(i).cloned());
            f.speaker_profile = None;
            f.open_select = None;
        })
        .build(k, layer)
        .key("episode-profile");
    let mut speaker_names = vec!["Profile's own voices".to_owned()];
    speaker_names.extend(profiles.speaker.iter().map(|p| p.name.clone()));
    let speaker_ids: Vec<String> = profiles.speaker.iter().map(|p| p.id.clone()).collect();
    let speaker_current =
        f.speaker_profile.as_ref().and_then(|id| profiles.speaker.iter().find(|p| &p.id == id)).map_or("Profile's own voices", |p| p.name.as_str());
    let speakers = Select::new("speaker-profile", &speaker_names)
        .label("Voices")
        .current(Some(speaker_current))
        .width(260.0)
        .open(f.open_select == Some("speaker"))
        .on_toggle(|s: &mut App| {
            let f = form(s);
            f.open_select = if f.open_select == Some("speaker") { None } else { Some("speaker") };
        })
        .on_close(|s: &mut App| form(s).open_select = None)
        .on_choose({
            let names = speaker_names.clone();
            move |s: &mut App, name: &str, _: &mut Tx| {
                let f = form(s);
                f.speaker_profile = names.iter().position(|n| n == name).and_then(|i| i.checked_sub(1)).and_then(|i| speaker_ids.get(i).cloned());
                f.open_select = None;
            }
        })
        .build(k, layer)
        .key("speaker-profile");
    let effort_at = EFFORTS.iter().position(|(e, _, _)| *e == f.effort).unwrap_or(1);
    let effort = SegmentedControl::new(&EFFORTS.map(|(_, n, _)| n), Some(effort_at))
        .label("Checking")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| form(s).effort = EFFORTS[i].0)
        .build(k)
        .key("effort");
    let mut c = col()
        .key("new-episode-form")
        .w_full()
        .gap(12.0)
        .padding(16.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .child(text("New episode").font(theme::READ_HEADING).color(l.text))
        .child(row().w_full().gap(10.0).child(profile).child(speakers));
    if let Some(p) = current
        && !p.description.is_empty()
    {
        c = c.child(text(p.description.clone()).font(k.ty.small).color(l.text2).wrap());
    }
    c = c
        .child(TextInput::new(&f.name).width(None).placeholder("Title (optional)").on_edit(|s: &mut App| &mut form(s).name).build(k).key("episode-name"))
        .child(
            TextArea::new(&f.focus)
                .width(None)
                .rows(2)
                .placeholder("What should it focus on? Optional")
                .on_edit(|s: &mut App| &mut form(s).focus)
                .build(k)
                .key("episode-focus"),
        )
        .child(
            col()
                .gap(6.0)
                .child(text("How much checking").font(k.ty.ui_medium).color(l.text2))
                .child(effort)
                .child(text(EFFORTS[effort_at].2).font(k.ty.small).color(l.text3)),
        );
    if let Some(e) = &f.error {
        c = c.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    c.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut App| {
                        if let Some(o) = s.open.as_mut() {
                            o.new_episode = None;
                        }
                    })
                    .build(k)
                    .key("episode-cancel"),
            )
            .child(
                Button::new(if f.busy { "Starting…" } else { "Make episode" })
                    .variant(ButtonVariant::Contrast)
                    .disabled(f.busy)
                    .on_press(|s: &mut App| s.create_episode())
                    .build(k)
                    .key("episode-make"),
            ),
    )
}

fn status_line(e: &Episode) -> String {
    let step = match e.status {
        EpisodeStatus::Queued => "Waiting",
        EpisodeStatus::Researching => "Studying the sources",
        EpisodeStatus::Writing => "Writing the script",
        EpisodeStatus::Checking => "Checking claims",
        EpisodeStatus::Revising => "Fixing flagged lines",
        EpisodeStatus::Voicing => "Recording voices",
        EpisodeStatus::Saving => "Saving",
        EpisodeStatus::Completed => {
            return [e.duration.map(clock), e.voice_label.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        }
        EpisodeStatus::Failed => return e.error.clone().unwrap_or_else(|| "Failed".into()),
    };
    match e.progress {
        Some((done, total)) if total > 0 => format!("{step}… {done} of {total}"),
        _ => format!("{step}…"),
    }
}

fn episode_card(s: &App, o: &Open, e: &Episode, l: &Look) -> El {
    let k = &l.k;
    let view = o.episode_views.get(&e.id).cloned().unwrap_or_default();
    let done = e.status == EpisodeStatus::Completed;
    let failed = e.status == EpisodeStatus::Failed;
    let playing = s.playing.as_ref().filter(|p| p.episode.id == e.id);
    let mut head = row().w_full().gap(8.0).center_y();
    if done {
        let ep = e.clone();
        let on = playing.is_some_and(|p| p.player.state() == PlayerState::Playing);
        head = head.child(
            IconButton::new(if on { &icons::PAUSE } else { &icons::PLAY })
                .variant(IconButtonVariant::Contrast)
                .label(if on { "Pause" } else { "Play" })
                .on_press(move |s: &mut App| s.play(ep.clone()))
                .build(k)
                .key("play"),
        );
    }
    head = head.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(text(e.name.clone()).font(theme::READ_HEADING).color(l.text).max_lines(2))
            .child(text(status_line(e)).font(k.ty.small).color(if failed { l.danger } else { l.text3 }).max_lines(2)),
    );
    let mut card = col().key(format!("ep-{}", e.id)).w_full().gap(10.0).padding(14.0).radius(12.0).border(1.0, l.line).child(head);
    if let Some((d, t)) = e.progress.filter(|_| !done && !failed)
        && t > 0
    {
        card = card.child(el().w_full().h(4.0).radius(2.0).fill(l.raised).child(el().w_frac(d as f32 / t as f32).h(4.0).radius(2.0).fill(l.amber)));
    }
    if let Some(summary) = e.summary.as_ref().filter(|x| !x.is_empty()) {
        card = card.child(text(summary.clone()).font(theme::READ_BODY).color(l.text2).wrap().max_lines(4));
    }
    if let Some(check) = &e.check {
        card = card.child(claim_meter(e, check, view.claims, l));
    }
    let (tid, pid, cid, rid, did) = (e.id.clone(), e.id.clone(), e.id.clone(), e.id.clone(), e.id.clone());
    let mut actions = row().w_full().gap(2.0);
    if done && !e.transcript.is_empty() {
        actions = actions.child(small(if view.transcript { "Hide transcript" } else { "Transcript" }, "transcript", l, move |s| {
            if let Some(o) = s.open.as_mut() {
                let v = o.episode_views.entry(tid.clone()).or_default();
                v.transcript = !v.transcript;
            }
        }));
    }
    if s.local_podcasts() {
        actions = actions.child(small(if view.prompt.is_some() { "Hide prompt" } else { "See prompt" }, "prompt", l, move |s| s.toggle_prompt(pid.clone())));
        if done {
            actions = actions.child(small("Check again", "check", l, move |s| s.check_episode(cid.clone())));
        }
    }
    if failed {
        actions = actions.child(small("Try again", "retry", l, move |s| s.retry_episode(rid.clone())));
    }
    actions = actions.child(el().grow(1.0)).child(
        IconButton::new(&icons::TRASH)
            .variant(IconButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .label("Delete episode")
            .on_press(move |s: &mut App| {
                let id = did.clone();
                s.ask("Delete this episode and its audio?", move |s| s.delete_episode(id.clone()));
            })
            .build(k)
            .key("delete"),
    );
    card = card.child(actions);
    if !e.chapters.is_empty() && done {
        let mut ch = col().w_full().gap(2.0);
        for (i, c) in e.chapters.iter().enumerate() {
            let (ep, at) = (e.clone(), c.start);
            ch = ch.child(
                k.interactive(
                    row()
                        .key(format!("ch-{i}"))
                        .w_full()
                        .gap(10.0)
                        .px(6.0)
                        .py(4.0)
                        .radius(6.0)
                        .child(text(clock(c.start)).font(k.ty.small).color(l.text3).w(48.0))
                        .child(text(c.title.clone()).font(k.ty.ui).color(l.text)),
                )
                .on_press(move |s: &mut App| {
                    if s.playing.as_ref().is_some_and(|p| p.episode.id == ep.id) {
                        s.seek_to(at);
                    } else {
                        s.play(ep.clone());
                    }
                }),
            );
        }
        card = card.child(ch);
    }
    if view.transcript {
        card = card.child(transcript(e, playing, l));
    }
    if let Some(p) = &view.prompt {
        card = card.child(
            col()
                .key("prompt")
                .w_full()
                .gap(6.0)
                .padding(12.0)
                .radius(10.0)
                .fill(l.raised)
                .child(text(format!("Written by {}", p.model)).font(k.ty.small).color(l.text3))
                .child(text(p.system.clone()).font(k.ty.mono).color(l.text2).wrap().selectable(true))
                .child(text(p.prompt.chars().take(20_000).collect::<String>()).font(k.ty.mono).color(l.text2).wrap().selectable(true)),
        );
    }
    card
}

/// How closely the episode follows the notebook: a bar of supported, overstated and
/// unsupported claims, then the claims themselves with the quote each was checked against.
fn claim_meter(e: &Episode, c: &EpisodeCheck, open: bool, l: &Look) -> El {
    let k = &l.k;
    match c.status.as_str() {
        "checking" => return text("Checking claims against the sources…").font(k.ty.small).color(l.text3),
        "failed" => return text(format!("Claim check failed: {}", c.error.clone().unwrap_or_default())).font(k.ty.small).color(l.danger).wrap(),
        _ => {}
    }
    let total = (c.supported + c.overstated + c.unsupported).max(1) as f32;
    let seg = |n: u32, color| el().w_frac(n as f32 / total).h(8.0).fill(color);
    let bar = row()
        .w_full()
        .h(8.0)
        .radius(4.0)
        .clip_rounded(4.0)
        .fill(l.raised)
        .child(seg(c.supported, l.ok))
        .child(seg(c.overstated, l.amber))
        .child(seg(c.unsupported, l.danger));
    let summary = format!(
        "{} supported · {} overstated · {} unsupported · {} of {} sources used",
        c.supported, c.overstated, c.unsupported, c.sources_used, c.sources_total
    );
    let id = e.id.clone();
    let mut col_ = col().key("claims").w_full().gap(6.0).child(bar).child(
        row().w_full().center_y().justify_between().child(text(summary).font(k.ty.small).color(l.text2)).child(small(
            if open { "Hide claims" } else { "Claims" },
            "claims-toggle",
            l,
            move |s| {
                if let Some(o) = s.open.as_mut() {
                    let v = o.episode_views.entry(id.clone()).or_default();
                    v.claims = !v.claims;
                }
            },
        )),
    );
    if open {
        for (i, claim) in c.claims.iter().enumerate() {
            let (label, color) = match claim.verdict {
                ClaimVerdict::Supported => ("Supported", l.ok),
                ClaimVerdict::Overstated => ("Overstated", l.amber),
                ClaimVerdict::Unsupported => ("Unsupported", l.danger),
            };
            let mut item = col().key(format!("claim-{i}")).w_full().gap(3.0).px(10.0).py(8.0).radius(8.0).fill(l.raised).child(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(text(label).font(k.ty.small_medium).color(color))
                    .child(text(claim.claim.clone()).font(k.ty.ui).color(l.text).wrap().grow(1.0).min_w(0.0)),
            );
            if let Some(q) = claim.quote.as_ref().filter(|q| !q.is_empty()) {
                let found = if claim.quote_found { "" } else { " (quote not found in the source)" };
                let from = claim.source.clone().map(|s| format!(" from {s}")).unwrap_or_default();
                item = item.child(text(format!("“{q}”{from}{found}")).font(theme::READ_BODY).color(l.text2).wrap());
            }
            if let Some(n) = claim.note.as_ref().filter(|n| !n.is_empty()) {
                item = item.child(text(n.clone()).font(k.ty.small).color(l.text3).wrap());
            }
            col_ = col_.child(item);
        }
    }
    col_
}

fn transcript(e: &Episode, playing: Option<&NowPlaying>, l: &Look) -> El {
    let k = &l.k;
    let current = playing.and_then(|p| p.line());
    let mut c = col().key("transcript").w_full().gap(8.0);
    if !e.exact_timing {
        c = c.child(text("Line times are estimated from text length.").font(k.ty.small).color(l.text3));
    }
    for (i, line) in e.transcript.iter().enumerate() {
        let lit = current == Some(i);
        let (ep, at) = (e.clone(), line.start);
        let mut r = row()
            .key(format!("line-{i}"))
            .w_full()
            .gap(10.0)
            .px(6.0)
            .py(4.0)
            .radius(6.0)
            .child(text(line.speaker.clone()).font(k.ty.small_medium).color(l.text2).w(84.0).max_lines(1))
            .child(text(line.text.clone()).font(theme::READ_BODY).color(l.text).wrap().grow(1.0).min_w(0.0));
        if lit {
            r = r.fill(l.amber_wash);
        }
        if let Some(at) = at {
            r = k.interactive(r).on_press(move |s: &mut App| {
                if s.playing.as_ref().is_some_and(|p| p.episode.id == ep.id) {
                    s.seek_to(at);
                } else {
                    s.play(ep.clone());
                }
            });
        }
        c = c.child(r);
    }
    c
}

/// The bar under the desk while an episode is loaded.
pub fn player_bar(p: &NowPlaying, l: &Look) -> El {
    let k = &l.k;
    let state = p.player.state();
    let on = state == PlayerState::Playing;
    let duration = p.player.duration().or(p.episode.duration).unwrap_or(0.0);
    let t = p.player.time();
    let ep = p.episode.clone();
    let status = match &state {
        PlayerState::Loading => "Loading".to_owned(),
        PlayerState::Failed(e) => e.clone(),
        _ => format!("{} / {}", clock(t), clock(duration)),
    };
    let progress = if duration > 0.0 { (t / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
    row()
        .key("player")
        .w_full()
        .h(56.0)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .fill(l.panel)
        .border(1.0, l.line)
        .child(
            IconButton::new(if on { &icons::PAUSE } else { &icons::PLAY })
                .variant(IconButtonVariant::Contrast)
                .label(if on { "Pause" } else { "Play" })
                .on_press(move |s: &mut App| s.play(ep.clone()))
                .build(k)
                .key("player-play"),
        )
        .child(small("−15", "player-back", l, |s| s.skip(-15.0)))
        .child(small("+30", "player-forward", l, |s| s.skip(30.0)))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(
                    row()
                        .w_full()
                        .justify_between()
                        .child(text(p.episode.name.clone()).font(k.ty.ui_medium).color(l.text).max_lines(1).shrink().min_w(0.0))
                        .child(text(status).font(k.ty.small).color(l.text3)),
                )
                .child(el().key("player-track").w_full().h(4.0).radius(2.0).fill(l.raised).child(el().w_frac(progress).h(4.0).radius(2.0).fill(l.amber))),
        )
        .child(small(&format!("{}×", RATES[p.rate]), "player-rate", l, |s| s.next_rate()))
        .child(
            IconButton::new(&icons::CLOSE)
                .variant(IconButtonVariant::GhostMuted)
                .label("Close player")
                .on_press(|s: &mut App| s.stop_playing())
                .build(k)
                .key("player-close"),
        )
}

// Profiles

fn page(s: &mut App) -> &mut ProfilesPage {
    s.profiles.as_mut().expect("profiles page")
}

pub fn profiles(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(p) = &s.profiles else { return col() };
    let mut c = col().w_full().max_w(720.0).gap(14.0).px(24.0).py(28.0).child(text("Podcast profiles").font(theme::READ_TITLE).color(l.text)).child(
        text("An episode profile sets the angle, length and voices of an episode. A speaker profile is the cast: who speaks and in which voice.")
            .font(k.ty.ui)
            .color(l.text2)
            .wrap(),
    );
    c = c.child(text("Episode profiles").font(theme::READ_HEADING).color(l.text));
    if let Some(d) = &p.episode {
        c = c.child(episode_editor(p, d, l, layer));
    } else {
        c = c.child(row().child(small("New episode profile", "new-episode-profile", l, |s| {
            let speaker = s.profiles.as_ref().and_then(|p| p.profiles.speaker.first()).map(|x| x.id.clone()).unwrap_or_default();
            page(s).episode = Some(EpisodeDraft::from(&EpisodeProfile { speaker_profile_id: speaker, segments: 4, ..Default::default() }));
        })));
    }
    for ep in &p.profiles.episode {
        let (edit, del) = (ep.clone(), ep.id.clone());
        let speaker = p.profiles.speaker.iter().find(|x| x.id == ep.speaker_profile_id).map_or("No voices", |x| x.name.as_str());
        c = c.child(profile_row(
            &format!("ep-{}", ep.id),
            &ep.name,
            &format!("{} · {} segments", speaker, ep.segments),
            l,
            move |s| page(s).episode = Some(EpisodeDraft::from(&edit)),
            move |s| {
                let id = del.clone();
                s.ask("Delete this episode profile?", move |s| s.delete_profile(false, id.clone()));
            },
        ));
    }
    c = c.child(text("Speaker profiles").font(theme::READ_HEADING).color(l.text).pt(10.0));
    if let Some(d) = &p.speaker {
        c = c.child(speaker_editor(p, d, l, layer));
    } else {
        c = c.child(row().child(small("New speaker profile", "new-speaker-profile", l, |s| {
            let mut d = SpeakerDraft::from(&SpeakerProfile {
                voice_kind: Some("openai".into()),
                voice_model: VoiceKind::OpenAi.default_model().into(),
                ..Default::default()
            });
            d.speakers.push(std::array::from_fn(|_| TextEdit::new("")));
            page(s).speaker = Some(d);
        })));
    }
    for sp in &p.profiles.speaker {
        let (edit, del) = (sp.clone(), sp.id.clone());
        let kind = sp.voice_kind.as_deref().and_then(VoiceKind::parse).map_or("Server voices", |v| v.label());
        let names: Vec<&str> = sp.speakers.iter().map(|x| x.name.as_str()).collect();
        c = c.child(profile_row(
            &format!("sp-{}", sp.id),
            &sp.name,
            &format!("{kind} · {}", names.join(", ")),
            l,
            move |s| page(s).speaker = Some(SpeakerDraft::from(&edit)),
            move |s| {
                let id = del.clone();
                s.ask("Delete this speaker profile?", move |s| s.delete_profile(true, id.clone()));
            },
        ));
    }
    col().key("profiles").w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn profile_row(key: &str, name: &str, detail: &str, l: &Look, edit: impl Fn(&mut App) + 'static, delete: impl Fn(&mut App) + 'static) -> El {
    let k = &l.k;
    let r = row()
        .key(key.to_owned())
        .w_full()
        .gap(8.0)
        .padding(12.0)
        .radius(10.0)
        .border(1.0, l.line)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(text(name.to_owned()).font(k.ty.ui_medium).color(l.text).max_lines(1))
                .child(text(detail.to_owned()).font(k.ty.small).color(l.text3).max_lines(1)),
        )
        .child(
            IconButton::new(&icons::TRASH)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Delete")
                .on_press(delete)
                .build(k)
                .key("delete"),
        );
    k.interactive(r).on_press(edit)
}

fn field(label: &str, e: El, l: &Look) -> El {
    col().w_full().gap(4.0).child(text(label.to_owned()).font(l.k.ty.small).color(l.text2)).child(e)
}

fn editor_frame(key: &str, l: &Look) -> El {
    col().key(key.to_owned()).w_full().gap(10.0).padding(14.0).radius(12.0).border(1.0, l.line_strong)
}

fn save_row(l: &Look, cancel: impl Fn(&mut App) + 'static, save: impl Fn(&mut App) + 'static, can_save: bool) -> El {
    let k = &l.k;
    row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(cancel).build(k).key("cancel"))
        .child(Button::new("Save").variant(ButtonVariant::Contrast).disabled(!can_save).on_press(save).build(k).key("save"))
}

fn episode_editor(p: &ProfilesPage, d: &EpisodeDraft, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    fn draft(s: &mut App) -> &mut EpisodeDraft {
        page(s).episode.as_mut().expect("editing an episode profile")
    }
    let names: Vec<String> = p.profiles.speaker.iter().map(|x| x.name.clone()).collect();
    let ids: Vec<String> = p.profiles.speaker.iter().map(|x| x.id.clone()).collect();
    let current = p.profiles.speaker.iter().find(|x| x.id == d.speaker_profile_id).map(|x| x.name.as_str());
    let speakers = Select::new("ep-speakers", &names)
        .label("Voices")
        .placeholder("Speaker profile")
        .current(current)
        .width(260.0)
        .open(p.open_select == Some("ep-speakers"))
        .on_toggle(|s: &mut App| {
            let p = page(s);
            p.open_select = if p.open_select == Some("ep-speakers") { None } else { Some("ep-speakers") };
        })
        .on_close(|s: &mut App| page(s).open_select = None)
        .on_choose({
            let names = names.clone();
            move |s: &mut App, name: &str, _: &mut Tx| {
                if let Some(id) = names.iter().position(|n| n == name).and_then(|i| ids.get(i).cloned()) {
                    draft(s).speaker_profile_id = id;
                }
                page(s).open_select = None;
            }
        })
        .build(k, layer)
        .key("ep-speakers");
    let segments = row()
        .gap(6.0)
        .center_y()
        .child(small("−", "seg-down", l, |s| draft(s).segments = draft(s).segments.saturating_sub(1).max(1)))
        .child(text(format!("{} segments", d.segments)).font(k.ty.ui).color(l.text))
        .child(small("+", "seg-up", l, |s| draft(s).segments = (draft(s).segments + 1).min(12)));
    editor_frame("episode-editor", l)
        .child(field("Name", TextInput::new(&d.name).width(None).on_edit(|s: &mut App| &mut draft(s).name).build(k).key("ep-name"), l))
        .child(field(
            "Description",
            TextInput::new(&d.description).width(None).on_edit(|s: &mut App| &mut draft(s).description).build(k).key("ep-description"),
            l,
        ))
        .child(row().w_full().gap(12.0).center_y().child(speakers).child(segments))
        .child(field(
            "Briefing: the angle and style of the episode",
            TextArea::new(&d.briefing).width(None).auto_grow(4, 14).on_edit(|s: &mut App| &mut draft(s).briefing).build(k).key("ep-briefing"),
            l,
        ))
        .child(save_row(l, |s| page(s).episode = None, |s| s.save_episode_profile(), !d.name.text.trim().is_empty() && !d.speaker_profile_id.is_empty()))
}

fn speaker_editor(p: &ProfilesPage, d: &SpeakerDraft, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    fn draft(s: &mut App) -> &mut SpeakerDraft {
        page(s).speaker.as_mut().expect("editing a speaker profile")
    }
    let kinds: Vec<&str> = VoiceKind::ALL.iter().map(|v| v.label()).collect();
    let current = VoiceKind::parse(&d.voice_kind).map(|v| v.label());
    let kind = Select::new("sp-kind", &kinds)
        .label("Voice provider")
        .current(current)
        .width(220.0)
        .open(p.open_select == Some("sp-kind"))
        .on_toggle(|s: &mut App| {
            let p = page(s);
            p.open_select = if p.open_select == Some("sp-kind") { None } else { Some("sp-kind") };
        })
        .on_close(|s: &mut App| page(s).open_select = None)
        .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
            if let Some(v) = VoiceKind::ALL.into_iter().find(|v| v.label() == name) {
                let d = draft(s);
                d.voice_kind = v.as_str().into();
                d.voice_model = TextEdit::new(v.default_model());
            }
            page(s).open_select = None;
        })
        .build(k, layer)
        .key("sp-kind");
    let voices = VoiceKind::parse(&d.voice_kind).map(|v| v.default_voices().join(", ")).unwrap_or_default();
    let mut c = editor_frame("speaker-editor", l)
        .child(field("Name", TextInput::new(&d.name).width(None).on_edit(|s: &mut App| &mut draft(s).name).build(k).key("sp-name"), l))
        .child(field(
            "Description",
            TextInput::new(&d.description).width(None).on_edit(|s: &mut App| &mut draft(s).description).build(k).key("sp-description"),
            l,
        ))
        .child(row().w_full().gap(10.0).child(kind).child(col().grow(1.0).min_w(0.0).child(
            TextInput::new(&d.voice_model).width(None).placeholder("Voice model").on_edit(|s: &mut App| &mut draft(s).voice_model).build(k).key("sp-model"),
        )));
    if !voices.is_empty() {
        c = c.child(text(format!("Voices: {voices}")).font(k.ty.small).color(l.text3).wrap());
    }
    const LABELS: [&str; 4] = ["Name", "Voice", "Backstory", "Personality"];
    for (i, sp) in d.speakers.iter().enumerate() {
        let mut r = col().key(format!("speaker-{i}")).w_full().gap(6.0).padding(10.0).radius(10.0).fill(l.raised);
        for (j, edit) in sp.iter().enumerate() {
            r = r.child(
                TextInput::new(edit)
                    .width(None)
                    .placeholder(LABELS[j])
                    .label(LABELS[j])
                    .on_edit(move |s: &mut App| &mut draft(s).speakers[i][j])
                    .build(k)
                    .key(format!("sp-{i}-{j}")),
            );
        }
        r = r.child(row().child(small("Remove speaker", "remove-speaker", l, move |s| {
            draft(s).speakers.remove(i);
        })));
        c = c.child(r);
    }
    if d.speakers.len() < 4 {
        c = c.child(row().child(small("Add speaker", "add-speaker", l, |s| draft(s).speakers.push(std::array::from_fn(|_| TextEdit::new(""))))));
    }
    c.child(save_row(l, |s| page(s).speaker = None, |s| s.save_speaker_profile(), !d.name.text.trim().is_empty() && !d.speakers.is_empty()))
}
