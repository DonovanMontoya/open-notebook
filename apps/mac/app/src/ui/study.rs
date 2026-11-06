//! The Study tab: decks made from the notebook's sources, a form for a new one, and a viewer
//! that flips flashcards or scores a quiz.

use notebook_engine::types::*;
use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, SegmentedControl, TextArea};

use super::icons;
use crate::state::{App, Open};
use crate::study::{DIFFICULTIES, DeckView, StudyForm};
use crate::theme::{self, Look};

fn form(s: &mut App) -> &mut StudyForm {
    s.open.as_mut().and_then(|o| o.study.as_mut()).expect("study form")
}

fn viewing(s: &mut App) -> Option<&mut DeckView> {
    s.open.as_mut().and_then(|o| o.viewing.as_mut())
}

pub fn view(o: &Open, l: &Look) -> El {
    let k = &l.k;
    let mut c = col().key("study").w_full().max_w(720.0).gap(14.0).px(24.0).py(20.0);
    if let Some(v) = &o.viewing
        && let Some(deck) = o.decks.get(v.index)
    {
        c = c.child(deck_view(&deck.deck, v, l));
    } else if let Some(f) = &o.study {
        c = c.child(study_form(o, f, l));
    } else {
        c = c.child(
            Button::new("New deck")
                .icon(&icons::PLUS)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| s.new_study_form())
                .build(k)
                .key("new-deck"),
        );
        if o.decks.is_empty() {
            c = c.child(
                text("Make flashcards to remember the key points, or a quiz to check yourself.").font(theme::READ_HEADING).color(l.text3).wrap().pt(40.0),
            );
        }
        for (i, d) in o.decks.iter().enumerate() {
            let kind = match d.deck.kind {
                StudyKind::Flashcards => "Flashcards",
                StudyKind::Quiz => "Quiz",
            };
            let card = col()
                .key(format!("deck-{}", d.note_id))
                .w_full()
                .gap(4.0)
                .padding(14.0)
                .radius(12.0)
                .border(1.0, l.line)
                .child(text(d.deck.title.clone()).font(theme::READ_HEADING).color(l.text).max_lines(1))
                .child(text(format!("{kind} · {} items", d.deck.items.len())).font(k.ty.small).color(l.text3));
            c = c.child(k.interactive(card).on_press(move |s: &mut App| {
                if let Some(o) = s.open.as_mut() {
                    o.viewing = Some(DeckView { index: i, card: 0, flipped: false, picked: None, right: 0 });
                }
            }));
        }
    }
    col().w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn study_form(o: &Open, f: &StudyForm, l: &Look) -> El {
    let k = &l.k;
    let kind = SegmentedControl::new(&["Flashcards", "Quiz"], Some(if f.kind == StudyKind::Flashcards { 0 } else { 1 }))
        .label("Kind")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| form(s).kind = if i == 0 { StudyKind::Flashcards } else { StudyKind::Quiz })
        .build(k)
        .key("study-kind");
    let difficulty = SegmentedControl::new(&["Easy", "Medium", "Hard"], Some(f.difficulty))
        .label("Difficulty")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| form(s).difficulty = i.min(DIFFICULTIES.len() - 1))
        .build(k)
        .key("study-difficulty");
    let count = row()
        .gap(6.0)
        .center_y()
        .child(
            Button::new("−")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| form(s).count = form(s).count.saturating_sub(2).max(4))
                .build(k)
                .key("count-down"),
        )
        .child(text(format!("{} {}", f.count, if f.kind == StudyKind::Quiz { "questions" } else { "cards" })).font(k.ty.ui).color(l.text))
        .child(
            Button::new("+")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| form(s).count = (form(s).count + 2).min(20))
                .build(k)
                .key("count-up"),
        );
    let mut sources = col().w_full().gap(6.0).child(text("From these sources").font(k.ty.ui_medium).color(l.text2));
    for src in &o.sources {
        let id = src.id.clone();
        let ready = !matches!(src.status.as_deref(), Some("failed" | "new" | "queued" | "running"));
        sources = sources.child(
            Checkbox::new(f.selected.contains(&src.id))
                .label(src.title.clone().unwrap_or_else(|| "Untitled source".into()))
                .disabled(!ready)
                .on_toggle(move |s: &mut App| {
                    let f = form(s);
                    if !f.selected.remove(&id) {
                        f.selected.insert(id.clone());
                    }
                })
                .build(k)
                .key(format!("pick-{}", src.id)),
        );
    }
    let mut c = col()
        .w_full()
        .gap(14.0)
        .child(text("New deck").font(theme::READ_HEADING).color(l.text))
        .child(row().gap(12.0).center_y().child(kind).child(difficulty))
        .child(count)
        .child(sources)
        .child(
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
                            o.study = None;
                        }
                    })
                    .build(k)
                    .key("study-cancel"),
            )
            .child(
                Button::new(if f.busy { "Making…" } else { "Make deck" })
                    .variant(ButtonVariant::Contrast)
                    .disabled(f.busy)
                    .on_press(|s: &mut App| s.make_deck())
                    .build(k)
                    .key("study-make"),
            ),
    )
}

fn deck_view(deck: &StudyDeck, v: &DeckView, l: &Look) -> El {
    let k = &l.k;
    let total = deck.items.len();
    let back = Button::new("All decks")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(|s: &mut App| {
            if let Some(o) = s.open.as_mut() {
                o.viewing = None;
            }
        })
        .build(k)
        .key("deck-back");
    let mut c = col().w_full().gap(14.0).child(
        row().w_full().center_y().justify_between().child(back).child(text(format!("{} of {total}", (v.card + 1).min(total))).font(k.ty.small).color(l.text3)),
    );
    let Some(item) = deck.items.get(v.card) else {
        let score = if deck.kind == StudyKind::Quiz { format!("{} of {total} right.", v.right) } else { "Deck done.".into() };
        return c.child(text(score).font(theme::READ_TITLE).color(l.text)).child(
            Button::new("Start again")
                .on_press(|s: &mut App| {
                    if let Some(v) = viewing(s) {
                        *v = DeckView { index: v.index, card: 0, flipped: false, picked: None, right: 0 };
                    }
                })
                .build(k)
                .key("deck-restart"),
        );
    };
    c = c.child(text(deck.title.clone()).font(k.ty.ui_medium).color(l.text2));
    let next = |label: &str| {
        Button::new(label.to_owned())
            .variant(ButtonVariant::Contrast)
            .on_press(|s: &mut App| {
                if let Some(v) = viewing(s) {
                    v.card += 1;
                    v.flipped = false;
                    v.picked = None;
                }
            })
            .build(k)
            .key("deck-next")
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
                .fill(if v.flipped { l.amber_wash } else { l.panel })
                .center()
                .child(text(face.clone()).font(theme::READ_HEADING).color(l.text).wrap().align(rune::Align::Center))
                .on_press(|s: &mut App| {
                    if let Some(v) = viewing(s) {
                        v.flipped = !v.flipped;
                    }
                });
            c = c.child(k.interactive(card)).child(
                row()
                    .w_full()
                    .gap(8.0)
                    .justify_end()
                    .child(text(if v.flipped { "" } else { "Click the card to see the answer" }).font(k.ty.small).color(l.text3))
                    .child(el().grow(1.0))
                    .child(next("Next")),
            );
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
                c = c
                    .child(text(item.answer.clone()).font(theme::READ_BODY).color(l.text2).wrap())
                    .child(row().w_full().justify_end().child(next("Next question")));
            }
        }
    }
    c
}
