//! Transformations, insights, chat context modes and study decks.

use std::collections::BTreeMap;

use super::*;
use crate::insights::DEFAULT_TRANSFORMATIONS;
use crate::studio::STUDY_PREFIX;
use crate::types::{ContextSelection, SourceContext, StudyKind, StudyRequest, Transformation};

fn insight_env() -> Env {
    let env = setup_with(AppSettings { chat: model("m"), ..Default::default() });
    env.fake.reply(|req| if req.system.contains("SUMMARY-PROMPT") { "A short summary.".into() } else { "Chat answer".into() });
    env
}

fn summary(apply_default: bool) -> Transformation {
    Transformation { id: String::new(), name: "s".into(), title: "Summary".into(), description: String::new(), prompt: "SUMMARY-PROMPT".into(), apply_default }
}

#[test]
fn the_servers_default_transformations_are_added_once() {
    let env = insight_env();
    let b = &env.backend;
    let list = b.list_transformations().unwrap();
    assert_eq!(list.len(), DEFAULT_TRANSFORMATIONS.len());
    assert_eq!(list[0].name, "Dense Summary", "the default-on transformation sorts first");
    b.delete_transformation(&list[1].id).unwrap();
    b.seed_transformations().unwrap();
    assert_eq!(b.list_transformations().unwrap().len(), DEFAULT_TRANSFORMATIONS.len() - 1, "deleted ones stay deleted");
    assert!(count(&env, "app_setting") >= 1);
    assert!(b.transformation_instructions().unwrap().starts_with("# INSTRUCTIONS You are my learning assistant"));
}

#[test]
fn transformations_can_be_created_edited_and_run_with_the_shared_instructions() {
    let env = insight_env();
    let b = &env.backend;
    let t = b.save_transformation(Transformation { name: "tldr".into(), title: "TL;DR".into(), description: "d".into(), ..summary(false) }).unwrap();
    assert!(t.id.starts_with("transformation:"));
    let edited = b.save_transformation(Transformation { title: "Too long".into(), ..t.clone() }).unwrap();
    assert_eq!(edited.title, "Too long");
    b.set_transformation_instructions("HOUSE RULES").unwrap();
    assert_eq!(b.run_transformation(&t.id, "Some text", &Cancel::new()).unwrap(), "A short summary.");
    let call = env.fake.last();
    assert_eq!(call.system, "HOUSE RULES\n\nSUMMARY-PROMPT");
    assert_eq!(call.messages[0].content, "Some text");
}

#[test]
fn the_transformation_model_is_used_when_set() {
    let env = setup_with(AppSettings { chat: model("chat-model"), transformation: model("writer-model"), ..Default::default() });
    let t = env.backend.list_transformations().unwrap().remove(0);
    env.backend.run_transformation(&t.id, &"x".repeat(400_010), &Cancel::new()).unwrap();
    let call = env.fake.last();
    assert_eq!(call.model, "writer-model");
    assert!(call.messages[0].content.ends_with("\n\n[Text shortened to fit]"));
}

#[test]
fn transformations_chosen_when_adding_a_source_become_insights_which_can_be_saved_as_notes() {
    let env = insight_env();
    let b = &env.backend;
    let t = b.save_transformation(summary(true)).unwrap();
    let nb = b.create_notebook("N", "").unwrap();
    let s = b.add_source(&nb.id, NewSource { transformations: vec![t.id.clone()], ..text("Long text", "Doc") }).unwrap();
    b.idle();
    let insights = b.list_insights(&s.id).unwrap();
    let got: Vec<(&str, &str, Option<&str>)> = insights.iter().map(|i| (i.kind.as_str(), i.content.as_str(), i.status.as_deref())).collect();
    assert_eq!(got, [("Summary", "A short summary.", Some("completed"))]);
    assert_eq!(b.get_source(&s.id).unwrap().summary.insights_count, 1);
    let pending: Option<String> = b.db.lock().query_row("SELECT pending_transformations FROM source WHERE id = ?1", [&s.id], |r| r.get(0)).unwrap();
    assert_eq!(pending, None, "pending transformations run once");

    let note = b.save_insight_as_note(&insights[0].id, &nb.id).unwrap();
    assert_eq!(note.title.as_deref(), Some("Summary: Doc"));
    assert_eq!(note.note_type.as_deref(), Some("ai"));

    b.delete_insight(&insights[0].id).unwrap();
    assert!(b.list_insights(&s.id).unwrap().is_empty());
}

#[test]
fn a_failing_transformation_leaves_a_failed_insight_with_the_reason() {
    let env = insight_env();
    let b = &env.backend;
    let t = b.list_transformations().unwrap().remove(0);
    let nb = b.create_notebook("N", "").unwrap();
    let s = b.add_source(&nb.id, text("Text", "Doc")).unwrap();
    b.idle();
    env.fake.fail(Some("Rate limited"));
    b.create_insight(&s.id, &t.id).unwrap();
    b.idle();
    let insight = b.list_insights(&s.id).unwrap().remove(0);
    assert_eq!(insight.status.as_deref(), Some("failed"));
    assert!(insight.error.unwrap().contains("Rate limited"));
}

#[test]
fn chat_context_follows_the_chosen_modes_for_sources_and_notes() {
    let env = insight_env();
    let b = &env.backend;
    let t = b.save_transformation(summary(false)).unwrap();
    let nb = b.create_notebook("N", "").unwrap();
    let a = b.add_source(&nb.id, NewSource { transformations: vec![t.id.clone()], ..text("ALPHA FULL TEXT", "Alpha") }).unwrap();
    let beta = b.add_source(&nb.id, text("BETA FULL TEXT", "Beta")).unwrap();
    let note = b.create_note(&nb.id, Some("Private"), "SECRET NOTE", false).unwrap();
    b.idle();

    b.send_chat_message(chat(&nb.id, "q")).unwrap();
    let system = env.fake.last().system;
    assert!(system.contains("A short summary."), "a source with insights sends its insights by default");
    assert!(system.contains("<source title=\"Alpha (insights)\">"));
    assert!(!system.contains("ALPHA FULL TEXT"));
    assert!(system.contains("BETA FULL TEXT"), "a source without insights sends its full text");
    assert!(system.contains("SECRET NOTE"));

    let context = ContextSelection {
        sources: BTreeMap::from([(a.id.clone(), SourceContext::Full), (beta.id.clone(), SourceContext::Off)]),
        notes: BTreeMap::from([(note.id.clone(), false)]),
    };
    b.send_chat_message(SendChat { context: Some(context), ..chat(&nb.id, "q") }).unwrap();
    let system = env.fake.last().system;
    assert!(system.contains("ALPHA FULL TEXT"));
    assert!(!system.contains("BETA FULL TEXT"));
    assert!(!system.contains("SECRET NOTE"));
}

fn quiz(source_id: &str, count: usize) -> String {
    let items: Vec<_> = (0..count)
        .map(|i| {
            serde_json::json!({ "question": format!("Q{i}"), "answer": "Because the report says so.", "options": ["a", "b", "c", "d"], "correct_index": 1, "source_ids": [source_id] })
        })
        .collect();
    serde_json::json!({ "version": 1, "kind": "quiz", "title": "Dark patterns quiz", "items": items }).to_string()
}

fn quiz_request(source_ids: Vec<String>) -> StudyRequest {
    StudyRequest { kind: StudyKind::Quiz, source_ids, count: 2, difficulty: "medium".into(), language: None, instructions: None }
}

#[test]
fn a_generated_deck_is_saved_as_a_note_the_server_would_recognize_and_listed_back() {
    let env = setup_with(AppSettings { chat: model("m"), ..Default::default() });
    let b = &env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    let s = b.add_source(&nb.id, text("The FTC names four tactics.", "FTC")).unwrap();
    b.idle();
    env.fake.reply_text(&quiz(&s.id, 2));

    let deck = b.generate_study_deck(&nb.id, quiz_request(vec![s.id.clone()])).unwrap();
    assert_eq!(deck.title, "Dark patterns quiz");
    let call = env.fake.last();
    assert!(call.system.contains("Exactly 2 items"));
    assert!(call.messages[0].content.contains("\"source_id\":"));

    let notes = b.list_notes(&nb.id).unwrap();
    assert!(notes[0].content.as_deref().unwrap().starts_with(STUDY_PREFIX));
    assert_eq!(notes[0].note_type.as_deref(), Some("ai"));
    let decks = b.list_study_decks(&nb.id).unwrap();
    assert_eq!(decks.len(), 1);
    assert_eq!(decks[0].note_id, notes[0].id);
    assert_eq!(decks[0].deck, deck);

    let err = b.generate_study_deck(&nb.id, quiz_request(vec!["source:elsewhere".into()])).unwrap_err();
    assert!(err.to_string().contains("from this notebook"));
}
