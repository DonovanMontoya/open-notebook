//! The Listen tab (episodes and the form that makes one), the player bar, and the profiles page.

use notebook_engine::podcast::voices::VoiceKind;
use notebook_engine::types::*;
use rune::{El, PlayerState, ScrollSpec, TextEdit, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, SegmentedControl, Select, TextArea, TextInput};

use super::icons;
use crate::listen::{
    EFFORTS, EpisodeDraft, EpisodeOpen, LANGUAGES, NewEpisode, NowPlaying, PlayerTab, ProfilesPage, PromptView, RATES, SKIP_BACK, SKIP_FORWARD, SLEEPS, Sleep,
    SpeakerDraft, chapter_at, check_history, check_models, check_verdict, clock, episode_minutes, language_name, line_at, line_times,
};
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
    actions = actions.child(small("Profiles", "open-profiles", l, |s| s.open_profiles()));
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
        if let Some(v) = o.episode_views.get(&e.id).and_then(|v| v.prompt.as_ref()) {
            prompt_dialog(e, v, l, layer);
        }
    }
    col().w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

/// The cast under the form: first names and the voice provider.
fn cast_line(sp: &SpeakerProfile) -> String {
    let names: Vec<&str> = sp.speakers.iter().map(|s| s.name.split(' ').next().unwrap_or_default()).collect();
    match sp.voice_kind.as_deref().and_then(VoiceKind::parse) {
        Some(v) => format!("{}, {}", names.join(", "), v.label()),
        None => names.join(", "),
    }
}

fn new_episode(s: &App, f: &NewEpisode, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let local = s.local_podcasts();
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
    let cast = current.and_then(|ep| {
        let id = f.speaker_profile.as_deref().unwrap_or(&ep.speaker_profile_id);
        profiles.speaker.iter().find(|p| p.id == id)
    });
    let minutes = current.map(|p| episode_minutes(p.segments));
    let mut c = col()
        .key("new-episode-form")
        .w_full()
        .gap(12.0)
        .padding(16.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .child(text("New episode").font(theme::READ_HEADING).color(l.text))
        .child(row().w_full().gap(10.0).child(profile).child(speakers));
    let about = [current.map(|p| p.description.clone()).filter(|d| !d.is_empty()), minutes.map(|m| format!("About {m} min"))];
    let about: Vec<String> = about.into_iter().flatten().collect();
    if !about.is_empty() {
        c = c.child(text(about.join(". ")).font(k.ty.small).color(l.text2).wrap().key("episode-about"));
    }
    if let Some(sp) = cast {
        c = c.child(text(cast_line(sp)).font(k.ty.small).color(l.text3).key("episode-cast"));
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
        );
    if local {
        let effort_at = EFFORTS.iter().position(|(e, _, _)| *e == f.effort).unwrap_or(1);
        let effort = SegmentedControl::new(&EFFORTS.map(|(_, n, _)| n), Some(effort_at))
            .label("Checking")
            .fit()
            .on_pick(|s: &mut App, i, _: &mut Tx| s.set_effort(EFFORTS[i].0))
            .build(k)
            .key("effort");
        c = c.child(
            col()
                .gap(6.0)
                .child(text("How much checking").font(k.ty.ui_medium).color(l.text2))
                .child(effort)
                .child(text(EFFORTS[effort_at].2).font(k.ty.small).color(l.text3)),
        );
    }
    if let (Some(m), Some(sp)) = (minutes, cast)
        && sp.voice_kind.as_deref() == Some(VoiceKind::OpenAiLive.as_str())
    {
        let make = (m as f64 * 1.5).round() as u32;
        c = c.child(
            text(format!("OpenAI Live records in real time, so this episode takes about {make} minutes to make")).font(k.ty.small).color(l.text3).wrap(),
        );
    }
    let needs_chat = s.needs_chat_model();
    if needs_chat {
        c = c.child(text("Choose a chat model in Settings; it writes the script").font(k.ty.small).color(l.amber).wrap().key("episode-needs-chat"));
    }
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
                    .disabled(f.busy || needs_chat || current.is_none())
                    .on_press(|s: &mut App| s.create_episode())
                    .build(k)
                    .key("episode-make"),
            ),
    )
}

fn in_progress(e: &Episode) -> bool {
    !matches!(e.status, EpisodeStatus::Completed | EpisodeStatus::Failed)
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
            let chapters = match e.chapters.len() {
                0 => None,
                1 => Some("1 chapter".to_owned()),
                n => Some(format!("{n} chapters")),
            };
            let created = Some(crate::format::relative_time(&e.created)).filter(|t| !t.is_empty());
            return [e.duration.map(clock), chapters, e.voice_label.clone(), created].into_iter().flatten().collect::<Vec<_>>().join(" · ");
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
    let local = s.local_podcasts();
    let view = o.episode_views.get(&e.id).cloned().unwrap_or_default();
    let done = e.status == EpisodeStatus::Completed;
    let failed = e.status == EpisodeStatus::Failed;
    let busy = in_progress(e);
    let playing = s.playing.as_ref().filter(|p| p.episode.id == e.id);
    let mut head = row().w_full().gap(8.0).center_y();
    if done {
        let ep = e.clone();
        let on = playing.is_some_and(|p| p.is_playing());
        head = head.child(
            IconButton::new(if on { &icons::PAUSE } else { &icons::PLAY })
                .variant(IconButtonVariant::Contrast)
                .label(if on { "Pause" } else { "Play" })
                .on_press(move |s: &mut App| s.play(ep.clone()))
                .build(k)
                .key("play"),
        );
    }
    let mut title = text(e.name.clone()).font(theme::READ_HEADING).color(l.text).max_lines(2);
    if done {
        let ep = e.clone();
        title = k.interactive(title.key("episode-title")).on_press(move |s: &mut App| s.open_player(ep.clone()));
    }
    head = head.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(row().child(title))
            .child(text(status_line(e)).font(k.ty.small).color(if failed { l.danger } else { l.text3 }).max_lines(2)),
    );
    let mut card = col()
        .key(format!("ep-{}", e.id))
        .w_full()
        .gap(10.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, if playing.is_some() { l.line_strong } else { l.line })
        .child(head);
    if let Some((d, t)) = e.progress.filter(|_| busy)
        && t > 0
    {
        card = card.child(el().w_full().h(4.0).radius(2.0).fill(l.raised).child(el().w_frac(d as f32 / t as f32).h(4.0).radius(2.0).fill(l.amber)));
    }
    if let Some(summary) = e.summary.as_ref().filter(|x| !x.is_empty() && !busy && !failed) {
        card = card.child(text(summary.clone()).font(theme::READ_BODY).color(l.text2).wrap().max_lines(4));
    }
    if local && done {
        card = card.child(claim_check(e, &view, l));
    }
    let (tid, pid, rid, did, dlep) = (e.id.clone(), e.id.clone(), e.id.clone(), e.id.clone(), e.clone());
    let mut actions = row().w_full().gap(2.0);
    if done && !e.transcript.is_empty() {
        actions = actions.child(small(if view.transcript { "Hide transcript" } else { "Transcript" }, "transcript", l, move |s| {
            if let Some(o) = s.open.as_mut() {
                let v = o.episode_views.entry(tid.clone()).or_default();
                v.transcript = !v.transcript;
            }
        }));
    }
    if local && !busy {
        actions = actions.child(small("See the prompt", "prompt", l, move |s| s.show_prompt(pid.clone())));
    }
    if done {
        actions = actions.child(
            Button::new("Download")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.download_episode(&dlep, tx))
                .build(k)
                .key("download"),
        );
    }
    if failed {
        actions = actions.child(small("Try again", "retry", l, move |s| s.retry_episode(rid.clone())));
    }
    actions = actions.child(el().grow(1.0));
    if !busy {
        let name = e.name.clone();
        actions = actions.child(
            IconButton::new(&icons::TRASH)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Delete episode")
                .on_press(move |s: &mut App| {
                    let id = did.clone();
                    s.ask(format!("Delete “{name}” and its audio?"), move |s| s.delete_episode(id.clone()));
                })
                .build(k)
                .key("delete"),
        );
    }
    card = card.child(actions);
    if !e.chapters.is_empty() && done {
        let current = playing.and_then(|p| p.chapter());
        card = card.child(chapter_list(e, current, l, "ch"));
    }
    if view.transcript {
        card = card.child(transcript(e, playing, l, "line", false));
    }
    card
}

/// Chapter rows that seek (or start the episode); the one playing is lit.
fn chapter_list(e: &Episode, current: Option<usize>, l: &Look, key: &str) -> El {
    let k = &l.k;
    let mut ch = col().w_full().gap(2.0);
    for (i, c) in e.chapters.iter().enumerate() {
        let (ep, at) = (e.clone(), c.start);
        let lit = current == Some(i);
        let mut r = row()
            .key(format!("{key}-{i}"))
            .w_full()
            .gap(10.0)
            .px(6.0)
            .py(4.0)
            .radius(6.0)
            .child(text(clock(c.start)).font(k.ty.small).color(l.text3).w(48.0))
            .child(text(c.title.clone()).font(if lit { k.ty.ui_medium } else { k.ty.ui }).color(l.text).shrink().min_w(0.0));
        if lit {
            r = r.fill(l.amber_wash);
        }
        ch = ch.child(k.interactive(r).on_press(move |s: &mut App| {
            if s.playing.as_ref().is_some_and(|p| p.episode.id == ep.id) {
                s.seek_to(at);
            } else {
                s.play(ep.clone());
            }
        }));
    }
    ch
}

/// The script prompt: the model, its size, a copy button and the whole prompt.
fn prompt_dialog(e: &Episode, v: &PromptView, l: &Look, layer: &mut Vec<El>) {
    let k = &l.k;
    let mut body = col().key("prompt").w_full().gap(8.0);
    let mut copy = None;
    match v {
        PromptView::Loading => body = body.child(text("Loading").font(k.ty.ui).color(l.text3)),
        PromptView::Missing => {
            body = body
                .child(text("This episode was made before prompts were saved. Episodes you make from now on keep theirs.").font(k.ty.ui).color(l.text2).wrap())
        }
        PromptView::Shown { prompt, copied } => {
            let chars = prompt.prompt.chars().count();
            body = body
                .child(
                    text(format!(
                        "Sent to {}, {} characters. The notebook material is included as it was when the episode was made.",
                        prompt.model,
                        group(chars)
                    ))
                    .font(k.ty.small)
                    .color(l.text3)
                    .wrap(),
                )
                .child(
                    col()
                        .w_full()
                        .gap(6.0)
                        .child(text("System").font(k.ty.small_medium).color(l.text2))
                        .child(text(prompt.system.clone()).font(k.ty.mono).color(l.text2).wrap().selectable(true))
                        .child(text("Message").font(k.ty.small_medium).color(l.text2).pt(6.0))
                        .child(text(prompt.prompt.clone()).font(k.ty.mono).color(l.text2).wrap().selectable(true))
                        .scroll(ScrollSpec { stick_bottom: false })
                        .max_h(440.0),
                );
            let all = format!("{}\n\n{}", prompt.system, prompt.prompt);
            let id = e.id.clone();
            copy = Some(
                Button::new(if *copied { "Copied" } else { "Copy prompt" })
                    .variant(ButtonVariant::Ghost)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        tx.copy(all.clone());
                        if let Some(PromptView::Shown { copied, .. }) =
                            s.open.as_mut().and_then(|o| o.episode_views.get_mut(&id)).and_then(|v| v.prompt.as_mut())
                        {
                            *copied = true;
                        }
                    })
                    .build(k)
                    .key("prompt-copy"),
            );
        }
    }
    let mut buttons = row().gap(8.0).justify_end().w_full();
    if let Some(c) = copy {
        buttons = buttons.child(c);
    }
    buttons = buttons.child(Button::new("Done").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.close_prompt()).build(k).key("prompt-done"));
    Dialog::new("prompt-dialog", "Script prompt").open(true).width(640.0).child(body).child(buttons).on_close(|s: &mut App| s.close_prompt()).build(k, layer);
}

/// 12345 as "12,345".
fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn claims_toggle(id: &str, flag: fn(&mut EpisodeOpen) -> &mut bool) -> impl Fn(&mut App) + 'static {
    let id = id.to_owned();
    move |s: &mut App| {
        if let Some(o) = s.open.as_mut() {
            let v = flag(o.episode_views.entry(id.clone()).or_default());
            *v = !*v;
        }
    }
}

/// How closely the episode follows the notebook: a verdict and a bar of supported, overstated
/// and unsupported claims; open, the models, the fix rounds and the claims, flagged first.
fn claim_check(e: &Episode, view: &EpisodeOpen, l: &Look) -> El {
    let k = &l.k;
    let id = e.id.clone();
    let Some(c) = &e.check else {
        return row().child(small("Check claims against your sources", "check", l, move |s| s.check_episode(id.clone())));
    };
    match c.status.as_str() {
        "checking" => return text("Checking claims against your sources…").font(k.ty.small).color(l.text3),
        "failed" => {
            return row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text(format!("The claim check failed: {}", c.error.clone().unwrap_or_default()))
                        .font(k.ty.small)
                        .color(l.danger)
                        .wrap()
                        .shrink()
                        .min_w(0.0),
                )
                .child(small("Check again", "check", l, move |s| s.check_episode(id.clone())));
        }
        _ => {}
    }
    let total = c.supported + c.overstated + c.unsupported;
    let seg = |n: u32, color| el().w_frac(n as f32 / total.max(1) as f32).h(8.0).fill(color);
    let bar = row()
        .w_full()
        .h(8.0)
        .radius(4.0)
        .clip_rounded(4.0)
        .fill(l.raised)
        .child(seg(c.supported, l.ok))
        .child(seg(c.overstated, l.amber))
        .child(seg(c.unsupported, l.danger));
    let mut headline = row().gap(6.0).shrink().min_w(0.0).child(text(check_verdict(c)).font(k.ty.ui_medium).color(l.text));
    if total > 0 {
        headline = headline.child(text(format!("{} of {total} claims", c.supported)).font(k.ty.ui).color(l.text2));
    }
    let mut out = col().key("claims").w_full().gap(6.0).child(row().w_full().center_y().justify_between().child(headline).child(small(
        if view.claims { "Hide details" } else { "Details" },
        "claims-toggle",
        l,
        claims_toggle(&e.id, |v| &mut v.claims),
    )));
    out = out.child(bar);
    if !view.claims {
        return out;
    }
    out = out.child(
        text(format!("{} match a passage in your sources · {} overstated · {} not in your sources", c.supported, c.overstated, c.unsupported))
            .font(k.ty.small)
            .color(l.text2)
            .wrap(),
    );
    let mut about = format!("{total} factual claims. {}.", check_models(c));
    if c.sources_total > 0 {
        about += &format!(" Draws on {} of {} sources.", c.sources_used, c.sources_total);
    }
    about += " A claim only counts as a match if the passage given as evidence is in your sources.";
    out = out.child(text(about).font(k.ty.small).color(l.text3).wrap().key("claims-models"));
    if let Some(h) = check_history(c) {
        out = out.child(text(h).font(k.ty.small).color(l.text3).wrap().key("claims-history"));
    }
    let flagged: Vec<&CheckedClaim> = c.claims.iter().filter(|x| x.verdict != ClaimVerdict::Supported).collect();
    let matched: Vec<&CheckedClaim> = c.claims.iter().filter(|x| x.verdict == ClaimVerdict::Supported).collect();
    if !flagged.is_empty() {
        out = out.child(text("Worth a second look").font(k.ty.ui_medium).color(l.text).pt(4.0).key("claims-flagged"));
        for (i, claim) in flagged.iter().enumerate() {
            out = out.child(claim_item(claim, &format!("flagged-{i}"), l));
        }
    }
    if !matched.is_empty() {
        let label = format!("{} claims that match your sources ({})", if view.matched { "Hide" } else { "Show" }, matched.len());
        out = out.child(row().child(small(&label, "claims-matched", l, claims_toggle(&e.id, |v| &mut v.matched))));
        if view.matched {
            for (i, claim) in matched.iter().enumerate() {
                out = out.child(claim_item(claim, &format!("matched-{i}"), l));
            }
        }
    }
    let again = e.id.clone();
    out.child(row().child(small("Check again", "check", l, move |s| s.check_episode(again.clone()))))
}

/// One claim: the spoken line, the verdict and why, and the quote when it was found.
fn claim_item(claim: &CheckedClaim, key: &str, l: &Look) -> El {
    let k = &l.k;
    let (label, color) = match claim.verdict {
        ClaimVerdict::Supported => ("Matches your sources", l.ok),
        ClaimVerdict::Overstated => ("Overstated", l.amber),
        ClaimVerdict::Unsupported => ("Not in your sources", l.danger),
    };
    let said = if claim.said.trim().is_empty() { &claim.claim } else { &claim.said };
    let why = claim
        .note
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| (!claim.said.trim().is_empty() && claim.said.trim() != claim.claim.trim()).then(|| claim.claim.clone()));
    let mut verdict = row().w_full().gap(6.0).child(text(label).font(k.ty.small_medium).color(color));
    if let Some(w) = why {
        verdict = verdict.child(text(w).font(k.ty.small).color(l.text2).wrap().shrink().min_w(0.0));
    }
    let mut item = col()
        .key(key.to_owned())
        .w_full()
        .gap(4.0)
        .px(10.0)
        .py(8.0)
        .radius(8.0)
        .fill(l.raised)
        .child(text(format!("“{said}”")).font(theme::READ_BODY).color(l.text).wrap())
        .child(verdict);
    if let Some(q) = claim.quote.as_ref().filter(|q| !q.is_empty() && claim.quote_found) {
        let from = claim.source.clone().map(|s| format!(" ({s})")).unwrap_or_default();
        item = item.child(text(format!("{q}{from}")).font(k.ty.small).color(l.text3).wrap());
    }
    item
}

/// Transcript lines; the one being spoken is lit and, in the full player, earlier ones dim and
/// chapter titles sit between them. Clicking a line seeks there.
fn transcript(e: &Episode, playing: Option<&NowPlaying>, l: &Look, key: &str, full: bool) -> El {
    let k = &l.k;
    let duration = playing.map(|p| p.duration()).or(e.duration).unwrap_or(0.0);
    let times = line_times(e, duration);
    let current = playing.and_then(|p| line_at(&times, p.player.time()));
    let mut c = col().w_full().gap(8.0);
    if !e.exact_timing {
        c = c.child(text("Line times are estimated from text length").font(k.ty.small).color(l.text3));
    }
    for (i, line) in e.transcript.iter().enumerate() {
        let at = times.get(i).map_or(0.0, |t| t.0);
        if full && let Some(ch) = e.chapters.iter().find(|c| (c.start - at).abs() < 0.05 || (i == 0 && c.start <= at)) {
            c = c.child(text(ch.title.clone()).font(k.ty.small_medium).color(l.text3).pt(6.0).key(format!("np-chapter-{i}")));
        }
        let lit = current == Some(i);
        let past = full && current.is_some_and(|c| i < c);
        let ep = e.clone();
        let mut r = row()
            .key(format!("{key}-{i}"))
            .w_full()
            .gap(10.0)
            .px(6.0)
            .py(4.0)
            .radius(6.0)
            .child(text(line.speaker.clone()).font(k.ty.small_medium).color(l.text2).w(84.0).max_lines(1))
            .child(text(line.text.clone()).font(theme::READ_BODY).color(if past { l.text3 } else { l.text }).wrap().grow(1.0).min_w(0.0));
        if lit {
            r = r.fill(l.amber_wash);
        }
        r = k.interactive(r).on_press(move |s: &mut App| {
            if s.playing.as_ref().is_some_and(|p| p.episode.id == ep.id) {
                s.seek_to(at);
            } else {
                s.play(ep.clone());
            }
        });
        c = c.child(r);
    }
    c
}

/// The track: one segment per chapter, filled up to the position. Click or drag to seek.
fn scrubber(p: &NowPlaying, l: &Look, key: &str) -> El {
    let duration = p.duration();
    let shown = p.scrub.unwrap_or_else(|| p.player.time());
    let starts: Vec<f64> = if p.episode.chapters.is_empty() || duration <= 0.0 { vec![0.0] } else { p.episode.chapters.iter().map(|c| c.start).collect() };
    let mut bar = row().w_full().h(4.0).gap(2.0);
    for (i, start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(duration.max(*start));
        let span = (end - start).max(0.001);
        let fill = if duration > 0.0 { ((shown - start) / span).clamp(0.0, 1.0) as f32 } else { 0.0 };
        bar = bar.child(el().grow(span as f32).h(4.0).radius(2.0).fill(l.raised).child(el().w_frac(fill).h(4.0).radius(2.0).fill(l.amber)));
    }
    col().key(key.to_owned()).w_full().h(16.0).justify_center().child(bar).cursor(rune::Cursor::Pointer).on_drag(
        |s: &mut App, d: &mut rune::Drag, _: &mut Tx| {
            let Some(p) = s.playing.as_mut() else { return };
            let t = (d.x / d.w.max(1.0)).clamp(0.0, 1.0) as f64 * p.duration();
            match d.phase {
                rune::Phase::Down | rune::Phase::Move => p.scrub = Some(t),
                rune::Phase::Up => {
                    p.scrub = None;
                    if !d.cancelled() {
                        s.seek(t);
                    }
                }
            }
        },
    )
}

/// "1:02 · The recall studies" while dragging the track.
fn scrub_label(p: &NowPlaying, t: f64) -> String {
    match chapter_at(&p.episode.chapters, t).and_then(|i| p.episode.chapters.get(i)) {
        Some(c) => format!("{} · {}", clock(t), c.title),
        None => clock(t),
    }
}

/// The bar under the desk while an episode is loaded, or the full player when it is open.
pub fn player_bar(p: &NowPlaying, l: &Look) -> El {
    if p.expanded {
        return now_playing(p, l);
    }
    let k = &l.k;
    let state = p.player.state();
    let on = state == PlayerState::Playing;
    let duration = p.duration();
    let t = p.player.time();
    let ep = p.episode.clone();
    let status = match (&state, p.scrub) {
        (_, Some(at)) => scrub_label(p, at),
        (PlayerState::Loading, _) => "Loading".to_owned(),
        (PlayerState::Failed(e), _) => e.clone(),
        _ => format!("{} left", clock(duration - t)),
    };
    let chapter = p.chapter().and_then(|i| p.episode.chapters.get(i)).map(|c| c.title.clone());
    let has_chapters = !p.episode.chapters.is_empty();
    let mut name = row().gap(6.0).shrink().min_w(0.0).child(text(p.episode.name.clone()).font(k.ty.ui_medium).color(l.text).max_lines(1).shrink().min_w(0.0));
    if let Some(c) = chapter {
        name = name.child(text(c).font(k.ty.small).color(l.text3).max_lines(1).shrink().min_w(0.0));
    }
    let name = k.interactive(name.key("player-title")).on_press(|s: &mut App| {
        if let Some(p) = s.playing.as_mut() {
            p.expanded = true;
            p.followed = None;
        }
    });
    let mut r = row().key("player").w_full().h(56.0).px(16.0).gap(10.0).center_y().fill(l.panel).border(1.0, l.line).child(
        IconButton::new(if on { &icons::PAUSE } else { &icons::PLAY })
            .variant(IconButtonVariant::Contrast)
            .label(if on { "Pause" } else { "Play" })
            .on_press(move |s: &mut App| s.play(ep.clone()))
            .build(k)
            .key("player-play"),
    );
    if has_chapters {
        r = r.child(small("Prev", "player-prev", l, |s| s.previous_chapter()));
    }
    r = r.child(small("−15", "player-back", l, |s| s.skip(-SKIP_BACK))).child(small("+30", "player-forward", l, |s| s.skip(SKIP_FORWARD)));
    if has_chapters {
        r = r.child(small("Next", "player-next", l, |s| s.next_chapter()));
    }
    let mut r = r.child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(row().w_full().gap(8.0).justify_between().child(name).child(text(status).font(k.ty.small).color(l.text3)))
            .child(scrubber(p, l, "player-track")),
    );
    if p.sleep != Sleep::Off {
        r = r.child(text(p.sleep.label(p.now)).font(k.ty.small).color(l.text3).key("player-sleep"));
    }
    r.child(small(&format!("{}×", RATES[p.rate]), "player-rate", l, |s| s.next_rate())).child(
        IconButton::new(&icons::CLOSE)
            .variant(IconButtonVariant::GhostMuted)
            .label("Close player")
            .on_press(|s: &mut App| s.stop_playing())
            .build(k)
            .key("player-close"),
    )
}

/// The full player over the desk: the episode, the track and transport, speed and sleep, and
/// a transcript that follows along or the chapters.
fn now_playing(p: &NowPlaying, l: &Look) -> El {
    let k = &l.k;
    let e = &p.episode;
    let on = p.is_playing();
    let duration = p.duration();
    let shown = p.scrub.unwrap_or_else(|| p.player.time());
    let chapter = p.chapter();
    let ep = e.clone();
    let header = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(small("Minimize", "np-minimize", l, |s| {
            if let Some(p) = s.playing.as_mut() {
                p.expanded = false;
            }
        }))
        .child(text(p.notebook.clone().unwrap_or_default()).font(k.ty.small).color(l.text3).grow(1.0).min_w(0.0).max_lines(1).align(rune::Align::Center))
        .child(small("Stop", "np-stop", l, |s| s.stop_playing()));
    let meta = match e.chapters.len() {
        0 => clock(duration),
        n => format!("{n} chapters, {}", clock(duration)),
    };
    let now_line = chapter.and_then(|i| e.chapters.get(i)).map(|c| c.title.clone()).or_else(|| e.summary.clone()).unwrap_or_default();
    let mut stage = col()
        .key("np-stage")
        .w(460.0)
        .gap(14.0)
        .child(
            col()
                .w_full()
                .gap(6.0)
                .padding(20.0)
                .radius(14.0)
                .fill(l.raised)
                .child(text("Open Notebook").font(k.ty.small).color(l.text3))
                .child(text(e.name.clone()).font(theme::READ_TITLE).color(l.text).wrap())
                .child(text(meta).font(k.ty.small).color(l.text2)),
        )
        .child(text(now_line).font(theme::READ_HEADING).color(l.text).wrap().max_lines(3).key("np-now"));
    if let Some(v) = &e.voice_label {
        stage = stage.child(text(v.clone()).font(k.ty.small).color(l.text3));
    }
    let tip = p.scrub.map(|t| scrub_label(p, t)).unwrap_or_else(|| clock(shown));
    stage = stage.child(
        col().w_full().gap(4.0).child(scrubber(p, l, "np-track")).child(
            row()
                .w_full()
                .justify_between()
                .child(text(tip).font(k.ty.small).color(l.text2))
                .child(text(format!("−{}", clock(duration - shown))).font(k.ty.small).color(l.text3)),
        ),
    );
    let has_chapters = !e.chapters.is_empty();
    let last = chapter.is_some_and(|c| c + 1 >= e.chapters.len()) && has_chapters;
    stage = stage.child(
        row()
            .w_full()
            .gap(6.0)
            .justify_center()
            .center_y()
            .child(
                Button::new("Previous chapter")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .disabled(!has_chapters)
                    .on_press(|s: &mut App| s.previous_chapter())
                    .build(k)
                    .key("np-prev"),
            )
            .child(small("−15", "np-back", l, |s| s.skip(-SKIP_BACK)))
            .child(
                IconButton::new(if on { &icons::PAUSE } else { &icons::PLAY })
                    .variant(IconButtonVariant::Contrast)
                    .label(if on { "Pause" } else { "Play" })
                    .on_press(move |s: &mut App| s.play(ep.clone()))
                    .build(k)
                    .key("np-play"),
            )
            .child(small("+30", "np-forward", l, |s| s.skip(SKIP_FORWARD)))
            .child(
                Button::new("Next chapter")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .disabled(!has_chapters || last)
                    .on_press(|s: &mut App| s.next_chapter())
                    .build(k)
                    .key("np-next"),
            ),
    );
    let rates: Vec<String> = RATES.iter().map(|r| format!("{r}×")).collect();
    stage = stage
        .child(col().gap(6.0).child(text("Speed").font(k.ty.small).color(l.text2)).child(
            SegmentedControl::new(&rates, Some(p.rate)).label("Speed").fit().on_pick(|s: &mut App, i, _: &mut Tx| s.set_rate(i)).build(k).key("np-rate"),
        ))
        .child(
            col()
                .gap(6.0)
                .child(
                    text(if p.sleep == Sleep::Off { "Sleep timer".to_owned() } else { format!("Sleep timer: {}", p.sleep.label(p.now)) })
                        .font(k.ty.small)
                        .color(l.text2),
                )
                .child(
                    SegmentedControl::new(&SLEEPS.map(|(_, n)| n), Some(p.sleep.index()))
                        .label("Sleep timer")
                        .fit()
                        .on_pick(|s: &mut App, i, _: &mut Tx| s.set_sleep(SLEEPS[i].0))
                        .build(k)
                        .key("np-sleep"),
                ),
        );
    if let PlayerState::Failed(err) = p.player.state() {
        stage = stage.child(text(err).font(k.ty.small).color(l.danger).wrap());
    }
    let tab_at = if p.tab == PlayerTab::Transcript { 0 } else { 1 };
    let tabs = SegmentedControl::new(&["Transcript", "Chapters"], Some(tab_at))
        .label("Show")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            if let Some(p) = s.playing.as_mut() {
                p.tab = if i == 0 { PlayerTab::Transcript } else { PlayerTab::Chapters };
                p.followed = None;
            }
        })
        .build(k)
        .key("np-tabs");
    let list = match p.tab {
        PlayerTab::Transcript if e.transcript.is_empty() => text("No transcript for this episode").font(k.ty.ui).color(l.text3),
        PlayerTab::Transcript => transcript(e, Some(p), l, "np-line", true),
        PlayerTab::Chapters if e.chapters.is_empty() => text("This episode has no chapters").font(k.ty.ui).color(l.text3),
        PlayerTab::Chapters => chapter_list(e, chapter, l, "np-ch"),
    };
    let scroller = col().key("np-transcript").w_full().grow(1.0).min_h(0.0).child(list).scroll(ScrollSpec { stick_bottom: false }).on_wheel(
        |s: &mut App, _: rune::WheelEvent, _: &mut Tx| {
            if let Some(p) = s.playing.as_mut() {
                p.following = false;
            }
            false
        },
    );
    let mut side = col().key("np-side").grow(1.0).min_w(0.0).h_full().min_h(0.0).gap(10.0).child(row().child(tabs)).child(scroller);
    if !p.following && p.tab == PlayerTab::Transcript {
        side = side.child(
            row().child(
                Button::new("Follow along")
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut App| {
                        if let Some(p) = s.playing.as_mut() {
                            p.following = true;
                            p.followed = None;
                        }
                    })
                    .build(k)
                    .key("np-follow"),
            ),
        );
    }
    col()
        .key("now-playing")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(l.panel)
        .px(24.0)
        .pt(48.0)
        .pb(20.0)
        .gap(16.0)
        .child(header)
        .child(row().w_full().grow(1.0).min_h(0.0).gap(28.0).child(stage).child(side))
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
            &format!("{} · {} segments, about {} min · {}", speaker, ep.segments, episode_minutes(ep.segments), language_name(ep.language.as_deref())),
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
        c = c.child(speaker_editor(s, p, d, l, layer));
    } else {
        c = c.child(row().child(small("New speaker profile", "new-speaker-profile", l, |s| s.new_speaker())));
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
            move |s| s.edit_speaker(SpeakerDraft::from(&edit)),
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
    let language_names: Vec<&str> = LANGUAGES.iter().map(|(_, n)| *n).collect();
    let language_now = language_name(d.language.as_deref());
    let language = Select::new("ep-language", &language_names)
        .label("Language")
        .current(Some(language_now.as_str()))
        .width(200.0)
        .open(p.open_select == Some("ep-language"))
        .on_toggle(|s: &mut App| {
            let p = page(s);
            p.open_select = if p.open_select == Some("ep-language") { None } else { Some("ep-language") };
        })
        .on_close(|s: &mut App| page(s).open_select = None)
        .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
            if let Some((code, _)) = LANGUAGES.iter().find(|(_, n)| *n == name) {
                draft(s).language = Some((*code).to_owned()).filter(|c| !c.is_empty());
            }
            page(s).open_select = None;
        })
        .build(k, layer)
        .key("ep-language");
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
        .child(row().w_full().gap(12.0).center_y().child(speakers).child(language).child(segments))
        .child(field(
            "Briefing: the angle and style of the episode",
            TextArea::new(&d.briefing)
                .width(None)
                .auto_grow(4, 14)
                .placeholder("For example: explain the ideas for a newcomer, with examples")
                .on_edit(|s: &mut App| &mut draft(s).briefing)
                .build(k)
                .key("ep-briefing"),
            l,
        ))
        .child(
            text(format!("About {} minutes. Each episode can add its own focus on top.", episode_minutes(d.segments)))
                .font(k.ty.small)
                .color(l.text3)
                .key("ep-minutes"),
        )
        .child(save_row(l, |s| page(s).episode = None, |s| s.save_episode_profile(), d.can_save()))
}

fn speaker_editor(s: &App, p: &ProfilesPage, d: &SpeakerDraft, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    fn draft(s: &mut App) -> &mut SpeakerDraft {
        page(s).speaker.as_mut().expect("editing a speaker profile")
    }
    fn toggle(s: &mut App, key: &'static str) {
        let p = page(s);
        p.open_voice = None;
        p.open_select = if p.open_select == Some(key) { None } else { Some(key) };
    }
    let local = s.local_podcasts();
    let kind_now = VoiceKind::parse(&d.voice_kind).unwrap_or(VoiceKind::OpenAi);
    let mut c = editor_frame("speaker-editor", l)
        .child(field(
            "Name",
            TextInput::new(&d.name).width(None).placeholder("Two analysts").on_edit(|s: &mut App| &mut draft(s).name).build(k).key("sp-name"),
            l,
        ))
        .child(field(
            "Description",
            TextInput::new(&d.description).width(None).on_edit(|s: &mut App| &mut draft(s).description).build(k).key("sp-description"),
            l,
        ));
    if local {
        let kinds: Vec<&str> = VoiceKind::ALL.iter().map(|v| v.label()).collect();
        let kind = Select::new("sp-kind", &kinds)
            .label("Voice provider")
            .current(Some(kind_now.label()))
            .width(220.0)
            .open(p.open_select == Some("sp-kind"))
            .on_toggle(|s: &mut App| toggle(s, "sp-kind"))
            .on_close(|s: &mut App| page(s).open_select = None)
            .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
                page(s).open_select = None;
                if let Some(v) = VoiceKind::ALL.into_iter().find(|v| v.label() == name) {
                    s.set_voice_kind(v);
                }
            })
            .build(k, layer)
            .key("sp-kind");
        let model = if p.voice_models.is_empty() {
            TextInput::new(&d.voice_model).width(None).placeholder("Voice model").on_edit(|s: &mut App| &mut draft(s).voice_model).build(k).key("sp-model")
        } else {
            let mut models = p.voice_models.clone();
            let current = d.voice_model.text.trim().to_owned();
            if !current.is_empty() && !models.contains(&current) {
                models.insert(0, current.clone());
            }
            Select::new("sp-model", &models)
                .label("Voice model")
                .current(Some(current.as_str()).filter(|c| !c.is_empty()))
                .placeholder("Voice model")
                .width(260.0)
                .open(p.open_select == Some("sp-model"))
                .on_toggle(|s: &mut App| toggle(s, "sp-model"))
                .on_close(|s: &mut App| page(s).open_select = None)
                .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
                    draft(s).voice_model = TextEdit::new(name);
                    page(s).open_select = None;
                })
                .build(k, layer)
                .key("sp-model")
        };
        c = c.child(row().w_full().gap(10.0).child(field("Voice provider", kind, l).w(220.0)).child(col().grow(1.0).min_w(0.0).child(field(
            "Voice model",
            model,
            l,
        ))));
        if kind_now == VoiceKind::OpenAiLive {
            c = c.child(
                text("OpenAI Live records in real time, so episodes take about one and a half times their length to make")
                    .font(k.ty.small)
                    .color(l.text3)
                    .wrap(),
            );
        }
        match &p.voices {
            None => c = c.child(text("Loading voices").font(k.ty.small).color(l.text3).key("sp-voices-loading")),
            Some(Err(e)) => c = c.child(text(format!("Could not load voices: {e}")).font(k.ty.small).color(l.danger).wrap().key("sp-voices-error")),
            Some(Ok(_)) => {}
        }
    } else {
        let mut names = vec!["Server default".to_owned()];
        names.extend(p.server_models.iter().map(|m| format!("{} ({})", m.name, m.provider)));
        let ids: Vec<String> = p.server_models.iter().map(|m| m.id.clone()).collect();
        let current = p.server_models.iter().find(|m| m.id == d.voice_model.text.trim()).map(|m| format!("{} ({})", m.name, m.provider));
        let current = current.unwrap_or_else(|| if d.voice_model.text.trim().is_empty() { names[0].clone() } else { d.voice_model.text.trim().to_owned() });
        let model = Select::new("sp-server-model", &names)
            .label("Voice model on the server")
            .current(Some(current.as_str()))
            .width(320.0)
            .open(p.open_select == Some("sp-server-model"))
            .on_toggle(|s: &mut App| toggle(s, "sp-server-model"))
            .on_close(|s: &mut App| page(s).open_select = None)
            .on_choose({
                let names = names.clone();
                move |s: &mut App, name: &str, _: &mut Tx| {
                    let id = names.iter().position(|n| n == name).and_then(|i| i.checked_sub(1)).and_then(|i| ids.get(i).cloned()).unwrap_or_default();
                    draft(s).voice_model = TextEdit::new(id);
                    page(s).open_select = None;
                }
            })
            .build(k, layer)
            .key("sp-server-model");
        c = c.child(field("Voice model on the server", model, l));
    }
    let voice_list: Option<&Vec<notebook_engine::podcast::voices::VoiceOption>> =
        if local { p.voices.as_ref().and_then(|v| v.as_ref().ok()).filter(|v| !v.is_empty()) } else { None };
    let removable = d.speakers.len() > 1;
    for (i, sp) in d.speakers.iter().enumerate() {
        let mut r = col()
            .key(format!("speaker-{i}"))
            .w_full()
            .gap(6.0)
            .padding(10.0)
            .radius(10.0)
            .fill(l.raised)
            .child(text(format!("Speaker {}", i + 1)).font(k.ty.small_medium).color(l.text2));
        let name = TextInput::new(&sp[0])
            .width(None)
            .placeholder("Name, for example Maya Chen")
            .label("Name")
            .on_edit(move |s: &mut App| &mut draft(s).speakers[i][0])
            .build(k)
            .key(format!("sp-{i}-0"));
        let voice = match voice_list {
            Some(list) => {
                let mut labels: Vec<String> = list.iter().map(|v| v.label.clone()).collect();
                let mut ids: Vec<String> = list.iter().map(|v| v.id.clone()).collect();
                let now = sp[1].text.trim().to_owned();
                if !now.is_empty() && !ids.contains(&now) {
                    labels.insert(0, now.clone());
                    ids.insert(0, now.clone());
                }
                let current = ids.iter().position(|v| *v == now).map(|j| labels[j].clone());
                Select::new(format!("sp-voice-{i}"), &labels)
                    .label("Voice")
                    .placeholder("Choose a voice")
                    .current(current.as_deref())
                    .width(220.0)
                    .open(p.open_voice == Some(i))
                    .on_toggle(move |s: &mut App| {
                        let p = page(s);
                        p.open_select = None;
                        p.open_voice = if p.open_voice == Some(i) { None } else { Some(i) };
                    })
                    .on_close(|s: &mut App| page(s).open_voice = None)
                    .on_choose(move |s: &mut App, label: &str, _: &mut Tx| {
                        if let Some(id) = labels.iter().position(|x| x == label).and_then(|j| ids.get(j).cloned()) {
                            draft(s).speakers[i][1] = TextEdit::new(id);
                        }
                        page(s).open_voice = None;
                    })
                    .build(k, layer)
                    .key(format!("sp-voice-{i}"))
            }
            None => TextInput::new(&sp[1])
                .width(None)
                .placeholder("Voice id, for example marin")
                .label("Voice")
                .on_edit(move |s: &mut App| &mut draft(s).speakers[i][1])
                .build(k)
                .key(format!("sp-{i}-1")),
        };
        let mut top = row().w_full().gap(8.0).center_y().child(col().grow(1.0).min_w(0.0).child(name)).child(voice);
        if local {
            let busy = p.previewing == Some(i);
            top = top.child(
                Button::new(if busy { "Speaking…" } else { "Hear this voice" })
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .disabled(busy || sp[1].text.trim().is_empty())
                    .on_press(move |s: &mut App| s.preview_voice(i))
                    .build(k)
                    .key(format!("sp-{i}-preview")),
            );
        }
        r = r.child(top);
        r = r
            .child(
                TextInput::new(&sp[2])
                    .width(None)
                    .placeholder("Background: who they are and what they know")
                    .label("Background")
                    .on_edit(move |s: &mut App| &mut draft(s).speakers[i][2])
                    .build(k)
                    .key(format!("sp-{i}-2")),
            )
            .child(
                TextInput::new(&sp[3])
                    .width(None)
                    .placeholder("Personality: how they talk")
                    .label("Personality")
                    .on_edit(move |s: &mut App| &mut draft(s).speakers[i][3])
                    .build(k)
                    .key(format!("sp-{i}-3")),
            );
        if let Some((_, e)) = p.preview_error.as_ref().filter(|(j, _)| *j == i) {
            r = r.child(text(e.clone()).font(k.ty.small).color(l.danger).wrap());
        }
        if removable {
            r = r.child(row().child(small("Remove speaker", "remove-speaker", l, move |s| {
                draft(s).speakers.remove(i);
                page(s).open_voice = None;
            })));
        }
        c = c.child(r);
    }
    if d.speakers.len() < 4 {
        c = c.child(row().child(small("Add speaker", "add-speaker", l, |s| draft(s).speakers.push(std::array::from_fn(|_| TextEdit::new(""))))));
    }
    c.child(save_row(l, |s| page(s).speaker = None, |s| s.save_speaker_profile(), d.can_save()))
}
