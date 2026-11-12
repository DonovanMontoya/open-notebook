//! The Study tab: a form for a new deck above the notebook's decks, and a viewer that runs
//! flashcards (missed cards come back) or scores a quiz.

use notebook_engine::types::*;
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, TextArea};

use crate::format;
use crate::state::{App, Open};
use crate::study::{DIFFICULTIES, DeckView, StudyForm, ready};
use crate::theme::{self, Look};

/// The deck form, made on first use so it always shows.
fn form(s: &mut App) -> &mut StudyForm {
    if s.open.as_ref().is_some_and(|o| o.study.is_none()) {
        s.new_study_form();
    }
    s.open.as_mut().and_then(|o| o.study.as_mut()).expect("study form")
}

fn viewing(s: &mut App) -> Option<&mut DeckView> {
    s.open.as_mut().and_then(|o| o.viewing.as_mut())
}

fn nouns(kind: StudyKind) -> &'static str {
    match kind {
        StudyKind::Flashcards => "cards",
        StudyKind::Quiz => "questions",
    }
}

/// "Flashcards, 8 cards, 3 days ago".
pub fn deck_details(d: &StoredDeck) -> String {
    let kind = match d.deck.kind {
        StudyKind::Flashcards => "Flashcards",
        StudyKind::Quiz => "Quiz",
    };
    let mut out = format!("{kind}, {} {}", d.deck.items.len(), nouns(d.deck.kind));
    if let Some(c) = &d.created {
        out += &format!(", {}", format::relative_time(c));
    }
    out
}

pub fn view(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let mut c = col().key("study").w_full().max_w(720.0).gap(14.0).px(l.pad()).py(20.0);
    if let Some(v) = &o.viewing
        && let Some(deck) = o.decks.get(v.index)
    {
        c = c.child(deck_view(o, &deck.deck, v, l));
    } else {
        c = c.child(study_form(o, l));
        if o.decks.is_empty() {
            c = c.child(
                text("Make flashcards to remember the key points, or a quiz to check yourself.").font(theme::READ_HEADING).color(l.text3).wrap().pt(24.0),
            );
        }
        for (i, d) in o.decks.iter().enumerate() {
            let len = d.deck.items.len();
            let card = col()
                .key(format!("deck-{i}-{}", d.note_id))
                .w_full()
                .gap(4.0)
                .padding(if l.phone() { 16.0 } else { 14.0 })
                .radius(12.0)
                .border(1.0, l.line)
                .child(text(d.deck.title.clone()).font(theme::READ_HEADING).color(l.text).max_lines(1))
                .child(text(deck_details(d)).font(k.ty.small).color(l.text3));
            c = c.child(k.interactive(card).on_press(move |s: &mut App| {
                if let Some(o) = s.open.as_mut() {
                    o.viewing = Some(DeckView::new(i, len));
                }
            }));
        }
    }
    col().w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn study_form(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let fallback;
    let f = match &o.study {
        Some(f) => f,
        None => {
            fallback = StudyForm {
                kind: StudyKind::Flashcards,
                difficulty: 1,
                count: 8,
                selected: o.sources.iter().filter(|s| ready(s.status.as_deref())).map(|s| s.id.clone()).collect(),
                focus: rune::TextEdit::new(""),
                busy: false,
                error: None,
            };
            &fallback
        }
    };
    let kind =
        super::pages::choices(&["Flashcards", "Quiz"], Some(if f.kind == StudyKind::Flashcards { 0 } else { 1 }), "Kind", l, |s: &mut App, i, _: &mut Tx| {
            form(s).kind = if i == 0 { StudyKind::Flashcards } else { StudyKind::Quiz }
        })
        .key("study-kind");
    let difficulty = super::pages::choices(&["Easy", "Medium", "Hard"], Some(f.difficulty), "Difficulty", l, |s: &mut App, i, _: &mut Tx| {
        form(s).difficulty = i.min(DIFFICULTIES.len() - 1)
    })
    .key("study-difficulty");
    let count = row()
        .gap(6.0)
        .center_y()
        .child(
            Button::new("−")
                .variant(ButtonVariant::Ghost)
                .control_size(if l.phone() { ControlSize::ExtraLarge } else { ControlSize::Small })
                .disabled(f.count <= 4)
                .on_press(|s: &mut App| form(s).count = form(s).count.saturating_sub(1).max(4))
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("count-down"),
        )
        .child(text(format!("{} {}", f.count, nouns(f.kind))).font(k.ty.ui).color(l.text))
        .child(
            Button::new("+")
                .variant(ButtonVariant::Ghost)
                .control_size(if l.phone() { ControlSize::ExtraLarge } else { ControlSize::Small })
                .disabled(f.count >= 20)
                .on_press(|s: &mut App| form(s).count = (form(s).count + 1).min(20))
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("count-up"),
        );
    let ready_sources: Vec<&SourceSummary> = o.sources.iter().filter(|s| ready(s.status.as_deref())).collect();
    let chosen = ready_sources.iter().filter(|s| f.selected.contains(&s.id)).count();
    let mut c = col().key("study-form").w_full().gap(12.0).padding(if l.phone() { 16.0 } else { 14.0 }).radius(12.0).border(1.0, l.line).child(if l.phone() {
        col().w_full().gap(16.0).child(kind).child(difficulty).child(count)
    } else {
        row().gap(12.0).center_y().flex_wrap().child(kind).child(difficulty).child(count)
    });
    if ready_sources.len() > 1 {
        let mut sources = col().w_full().gap(6.0);
        for src in &ready_sources {
            let id = src.id.clone();
            let selected = f.selected.contains(&src.id);
            let title = src.title.clone().unwrap_or_else(|| "Untitled source".into());
            let toggle = move |s: &mut App| {
                let f = form(s);
                if !f.selected.remove(&id) {
                    f.selected.insert(id.clone());
                }
            };
            let choice = if l.phone() {
                k.interactive(
                    row()
                        .w_full()
                        .min_h(52.0)
                        .gap(12.0)
                        .py(8.0)
                        .center_y()
                        .role("checkbox")
                        .checked(selected)
                        .label(title.clone())
                        .child(
                            row()
                                .w(24.0)
                                .h(24.0)
                                .center()
                                .radius(6.0)
                                .fill(if selected { l.amber_wash } else { l.raised })
                                .child(text(if selected { "✓" } else { "" }).font(k.ty.ui_medium).color(l.amber)),
                        )
                        .child(text(title).font(k.ty.ui).color(l.text).wrap().grow(1.0).min_w(0.0)),
                )
                .on_press(toggle)
            } else {
                Checkbox::new(selected).label(title).on_toggle(toggle).build(k)
            };
            sources = sources.child(choice.key(format!("pick-{}", src.id)));
        }
        c = c.child(sources);
    }
    c = c.child(
        TextArea::new(&f.focus)
            .width(None)
            .rows(2)
            .placeholder("Anything to focus on? Optional")
            .on_edit(|s: &mut App| &mut form(s).focus)
            .build(k)
            .key("study-focus"),
    );
    if let Some(e) = &f.error {
        c = c.child(text(e.clone()).font(k.ty.ui).color(l.danger).wrap());
    }
    let summary = if ready_sources.is_empty() { "Add a source first".to_owned() } else { format!("From {chosen} of {} sources", ready_sources.len()) };
    let make = if f.busy {
        "Making the deck"
    } else if f.kind == StudyKind::Quiz {
        "Make quiz"
    } else {
        "Make flashcards"
    };
    c.child(
        row().w_full().gap(8.0).center_y().child(text(summary).font(k.ty.small).color(l.text3)).child(el().grow(1.0)).child(
            Button::new(make)
                .variant(ButtonVariant::Contrast)
                .disabled(f.busy || chosen == 0)
                .on_press(|s: &mut App| {
                    form(s);
                    s.make_deck();
                })
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("study-make"),
        ),
    )
}

/// Links to the sources an item came from; each opens the source.
fn source_links(o: &Open, ids: &[String], l: &Look) -> El {
    let k = &l.k;
    let mut r = row().w_full().gap(6.0).center_y().flex_wrap();
    for id in ids {
        let title = o.sources.iter().find(|s| &s.id == id).and_then(|s| s.title.clone()).unwrap_or_else(|| "Source".into());
        let sid = id.clone();
        r = r.child(
            Button::new(title)
                .variant(ButtonVariant::Ghost)
                .control_size(if l.phone() { ControlSize::ExtraLarge } else { ControlSize::Small })
                .on_press(move |s: &mut App| s.read_source(sid.clone(), None))
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key(format!("src-{id}")),
        );
    }
    r
}

/// Space flips the card; with the answer showing, Right is Got it and Left is Again.
pub fn flashcard_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    let Some(v) = viewing(s) else { return false };
    if v.item().is_none() {
        return false;
    }
    match &ev.key {
        Key::Text(t) if t == " " => v.flipped = !v.flipped,
        Key::Right if v.flipped => v.answer(true),
        Key::Left if v.flipped => v.answer(false),
        _ => return false,
    }
    true
}

fn deck_view(o: &Open, deck: &StudyDeck, v: &DeckView, l: &Look) -> El {
    let k = &l.k;
    let total = deck.items.len();
    let back = Button::new("All decks")
        .variant(ButtonVariant::Ghost)
        .control_size(if l.phone() { ControlSize::ExtraLarge } else { ControlSize::Small })
        .on_press(|s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                o.viewing = None;
            }
        })
        .build(k)
        .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .key("deck-back");
    let item = v.item().and_then(|i| deck.items.get(i));
    let progress = match (deck.kind, item) {
        (StudyKind::Flashcards, Some(_)) => format!("Card {} of {}, {} known", v.card + 1, v.order.len(), v.known),
        (StudyKind::Flashcards, None) => format!("{total} cards, {} known", v.known),
        (StudyKind::Quiz, Some(_)) => format!("Question {} of {total}, {} right", v.card + 1, v.right),
        (StudyKind::Quiz, None) => format!("Finished, {} right", v.right),
    };
    let mut c = col()
        .w_full()
        .gap(14.0)
        .child(row().w_full().center_y().justify_between().child(back).child(text(progress).font(k.ty.small).color(l.text3)))
        .child(text(deck.title.clone()).font(k.ty.ui_medium).color(l.text2));
    let Some(item) = item else {
        let (score, again) = match deck.kind {
            StudyKind::Quiz => (format!("{} of {total} right", v.right), "Try again"),
            StudyKind::Flashcards => (format!("All {total} cards known"), "Go again"),
        };
        return c.child(text(score).font(theme::READ_TITLE).color(l.text)).child(
            row().child(
                Button::new(again)
                    .on_press(|s: &mut App| {
                        if let Some(v) = viewing(s) {
                            v.restart();
                        }
                    })
                    .build(k)
                    .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                    .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                    .key("deck-restart"),
            ),
        );
    };
    match deck.kind {
        StudyKind::Flashcards => {
            let face = if v.flipped { &item.answer } else { &item.question };
            let card = col()
                .key("flashcard")
                .w_full()
                .min_h(220.0)
                .padding(28.0)
                .radius(16.0)
                .border(1.0, l.line_strong)
                .fill(if v.flipped { l.raised } else { l.panel })
                .center()
                .child(text(face.clone()).font(theme::READ_HEADING).color(l.text).wrap().align(rune::Align::Center))
                .label(if v.flipped { "Show question" } else { "Show answer" })
                .on_press(|s: &mut App| {
                    if let Some(v) = viewing(s) {
                        v.flipped = !v.flipped;
                    }
                });
            let button = |label: &str, key: &str, contrast: bool, f: fn(&mut DeckView)| {
                Button::new(label.to_owned())
                    .variant(if contrast { ButtonVariant::Contrast } else { ButtonVariant::Neutral })
                    .on_press(move |s: &mut App| {
                        if let Some(v) = viewing(s) {
                            f(v);
                        }
                    })
                    .build(k)
                    .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                    .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                    .key(key.to_owned())
            };
            let controls = if v.flipped {
                row().gap(8.0).child(button(if l.phone() { "Again" } else { "Again  ←" }, "deck-again", false, |v| v.answer(false))).child(button(
                    if l.phone() { "Got it" } else { "Got it  →" },
                    "deck-known",
                    true,
                    |v| v.answer(true),
                ))
            } else {
                row().gap(8.0).child(button(if l.phone() { "Show answer" } else { "Show answer  Space" }, "deck-flip", false, |v| v.flipped = true))
            };
            let mut deck_col = col().key("flashcards").w_full().gap(14.0).child(k.interactive(card)).child(row().w_full().justify_center().child(controls));
            if v.flipped && !item.source_ids.is_empty() {
                deck_col = deck_col.child(source_links(o, &item.source_ids, l));
            }
            c = c.child(deck_col.focusable(true).on_key(flashcard_key));
        }
        StudyKind::Quiz => {
            c = c.child(text(item.question.clone()).font(theme::READ_HEADING).color(l.text).wrap());
            for (i, option) in item.options.iter().enumerate() {
                let right = item.correct_index == Some(i);
                let fill = match v.picked {
                    Some(_) if right => l.amber_wash,
                    Some(p) if p == i => l.danger.alpha(0.12),
                    _ => l.panel,
                };
                let choice = row()
                    .key(format!("opt-{i}"))
                    .w_full()
                    .min_h(52.0)
                    .padding(12.0)
                    .radius(10.0)
                    .border(1.0, l.line)
                    .fill(fill)
                    .child(text(option.clone()).font(k.ty.body).color(l.text).wrap());
                c = c.child(if v.picked.is_some() {
                    choice
                } else {
                    k.interactive(choice).on_press(move |s: &mut App| {
                        if let Some(v) = viewing(s) {
                            v.picked = Some(i);
                            if right {
                                v.right += 1;
                            }
                        }
                    })
                });
            }
            if v.picked.is_some() {
                c = c.child(text(item.answer.clone()).font(theme::READ_BODY).color(l.text2).wrap());
                if !item.source_ids.is_empty() {
                    c = c.child(source_links(o, &item.source_ids, l));
                }
                let last = v.card + 1 >= total;
                c = c.child(
                    row().w_full().justify_end().child(
                        Button::new(if last { "See score" } else { "Next question" })
                            .variant(ButtonVariant::Contrast)
                            .on_press(|s: &mut App| {
                                if let Some(v) = viewing(s) {
                                    v.next();
                                }
                            })
                            .build(k)
                            .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                            .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                            .key("deck-next"),
                    ),
                );
            }
        }
    }
    c
}
