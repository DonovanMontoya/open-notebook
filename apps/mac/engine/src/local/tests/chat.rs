//! Notebook chat, source chat and Ask.

use std::sync::Mutex;

use super::*;
use crate::local::FULL_CONTEXT_CHARS;
use crate::providers::Role;
use crate::types::{AskStep, MessageKind};

#[test]
fn chat_sends_small_notebooks_whole_streams_the_reply_and_saves_the_session() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Chat", "").unwrap();
    b.add_source(&nb.id, text("The launch date is 3 March.", "Plan")).unwrap();
    b.create_note(&nb.id, Some("Reminder"), "Ask about budget.", false).unwrap();
    b.idle();

    let deltas = Mutex::new(String::new());
    let mut on_text = |d: &str| deltas.lock().unwrap().push_str(d);
    let first = b.send_chat_message(SendChat { on_text: Some(&mut on_text), ..chat(&nb.id, "When is launch?") }).unwrap();
    assert_eq!(*deltas.lock().unwrap(), "Fake answer [Doc]");
    let system = env.fake.last().system;
    assert!(system.contains("<source title=\"Plan\">\nThe launch date is 3 March."), "{system}");
    assert!(system.contains("<note title=\"Reminder\">"));
    assert_eq!(first.messages.iter().map(|m| m.kind).collect::<Vec<_>>(), [MessageKind::Human, MessageKind::Ai]);
    assert_eq!(first.session.title, "When is launch?");

    b.send_chat_message(SendChat { session_id: Some(first.session.id.clone()), ..chat(&nb.id, "And budget?") }).unwrap();
    let roles: Vec<Role> = env.fake.last().messages.iter().map(|m| m.role).collect();
    assert_eq!(roles, [Role::User, Role::Assistant, Role::User], "earlier turns are sent as history");
    let sessions = b.list_chat_sessions(&nb.id).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].message_count, Some(4));
    let saved = b.get_chat_session(&first.session.id).unwrap();
    assert!(saved.messages[0].timestamp < saved.messages[1].timestamp, "the reply sorts after the question");
}

#[test]
fn chat_on_a_large_notebook_sends_short_sources_whole_and_excerpts_of_long_ones() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Big", "").unwrap();
    let filler = "lorem ipsum dolor sit amet ".repeat(FULL_CONTEXT_CHARS / 27 + 11);
    b.add_source(&nb.id, text(&filler, "Filler")).unwrap();
    b.add_source(&nb.id, text("Xylophones quickly vex wizards.", "Needle")).unwrap();
    b.idle();

    b.send_chat_message(chat(&nb.id, "xylophone wizards")).unwrap();
    let system = env.fake.last().system;
    assert!(system.contains("<source title=\"Needle\">\nXylophones quickly vex wizards."));
    assert!(system.contains("<source title=\"Filler\" excerpts=\"true\">"));
    assert!(system.chars().count() < FULL_CONTEXT_CHARS + 2_000, "prompt stays near the full-context budget");
}

#[test]
fn chat_on_a_large_notebook_includes_every_source_not_only_the_closest_matches() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Eight", "").unwrap();
    let long =
        |n: usize| format!("Source {n} opening line.\n\n{}", format!("paragraph about topic {n} with details. ").repeat(FULL_CONTEXT_CHARS / 8 / 40 + 200));
    for n in 1..=8 {
        b.add_source(&nb.id, text(&long(n), &format!("Paper {n}"))).unwrap();
    }
    b.idle();

    b.send_chat_message(chat(&nb.id, "Give me a two-sentence summary of each source")).unwrap();
    let system = env.fake.last().system;
    for n in 1..=8 {
        assert!(
            system.contains(&format!("<source title=\"Paper {n}\" excerpts=\"true\">\nSource {n} opening line")),
            "Paper {n} is represented by its opening"
        );
    }
    assert!(system.chars().count() < FULL_CONTEXT_CHARS + 4_000, "prompt stays near the full-context budget");
}

#[test]
fn a_failed_chat_request_saves_nothing() {
    let env = setup();
    let nb = env.backend.create_notebook("Chat", "").unwrap();
    env.fake.fail(Some("API key rejected"));
    let err = env.backend.send_chat_message(chat(&nb.id, "Hi")).unwrap_err();
    assert!(err.to_string().contains("API key rejected"));
    assert!(env.backend.list_chat_sessions(&nb.id).unwrap().is_empty());
}

#[test]
fn chat_without_a_configured_model_explains_what_to_do() {
    let env = setup_with(AppSettings::default());
    let nb = env.backend.create_notebook("Chat", "").unwrap();
    let err = env.backend.send_chat_message(chat(&nb.id, "Hi")).unwrap_err();
    assert!(err.to_string().contains("Choose a chat model"));
}

#[test]
fn a_model_override_picks_the_provider_and_model_for_one_message() {
    let env = setup();
    let nb = env.backend.create_notebook("Chat", "").unwrap();
    env.backend.send_chat_message(SendChat { model_override: Some("anthropic::claude-x::v2".into()), ..chat(&nb.id, "Hi") }).unwrap();
    assert_eq!(env.fake.last().model, "claude-x::v2");
    let err = env.backend.send_chat_message(SendChat { model_override: Some("nope::m".into()), ..chat(&nb.id, "Hi") }).unwrap_err();
    assert!(err.to_string().contains("No API key is saved for the model nope::m"));
}

#[test]
fn chat_sessions_can_be_renamed_and_deleted() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Chat", "").unwrap();
    let first = b.send_chat_message(chat(&nb.id, "A question that is long enough to be cut off at forty characters")).unwrap();
    assert_eq!(first.session.title, "A question that is long enough to be cu…");
    b.rename_chat_session(&first.session.id, "  ").unwrap();
    assert_eq!(b.get_chat_session(&first.session.id).unwrap().session.title, "Untitled conversation");
    b.delete_chat_session(&first.session.id).unwrap();
    assert!(b.list_chat_sessions(&nb.id).unwrap().is_empty());
    assert_eq!(count(&env, "chat_message"), 0);
}

#[test]
fn source_chat_talks_about_one_source_and_keeps_its_own_sessions() {
    let env = setup_with(AppSettings { chat: model("m"), ..Default::default() });
    let b = &env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    let a = b.add_source(&nb.id, text("ALPHA TEXT", "Alpha")).unwrap();
    b.add_source(&nb.id, text("BETA TEXT", "Beta")).unwrap();
    b.idle();

    let first = b.send_source_chat_message(source_chat(&a.id, "What is this?")).unwrap();
    let system = env.fake.last().system;
    assert!(system.contains("ALPHA TEXT"));
    assert!(!system.contains("BETA TEXT"), "other sources stay out");
    assert!(system.contains("A conversation about one source: \"Alpha\". Answer from it."));
    b.send_source_chat_message(SendSourceChat { session_id: Some(first.session.id.clone()), ..source_chat(&a.id, "More?") }).unwrap();
    assert_eq!(env.fake.last().messages.len(), 3);

    assert_eq!(b.list_source_chat_sessions(&a.id).unwrap().len(), 1);
    assert!(b.list_chat_sessions(&nb.id).unwrap().is_empty(), "source chats are not notebook chats");

    b.rename_chat_session(&first.session.id, "Alpha questions").unwrap();
    assert_eq!(Backend::get_source_chat_session(b, &a.id, &first.session.id).unwrap().session.title, "Alpha questions");

    b.delete_source(&a.id).unwrap();
    assert!(b.list_source_chat_sessions(&a.id).unwrap().is_empty());
}

#[test]
fn ask_plans_searches_answers_each_then_writes_a_cited_final_answer() {
    let env = setup_with(AppSettings { chat: model("m"), ..Default::default() });
    env.fake.reply(|req| {
        let msg = &req.messages[0].content;
        if msg.contains("plan how to find") {
            return serde_json::json!({ "reasoning": "Look for launch info", "searches": [{ "term": "launch date", "instructions": "Find the date" }] })
                .to_string();
        }
        if msg.contains("# Search") {
            let id = msg.split("<document id=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("?");
            return format!("It launches in March [{id}].");
        }
        if msg.contains("Partial answers") {
            let start = msg.find("It launches in March [").unwrap();
            let end = start + msg[start..].find("].").unwrap() + 2;
            return msg[start..end].to_string();
        }
        "?".into()
    });
    let b = &env.backend;
    let nb = b.create_notebook("Plans", "").unwrap();
    let src = b.add_source(&nb.id, text("The launch date is in March.", "Launch plan")).unwrap();
    b.create_note(&nb.id, Some("Launch worries"), "What if the launch date slips?", false).unwrap();
    b.idle();

    let mut steps = Vec::new();
    let mut on_step = |s: AskStep| {
        steps.push(match s {
            AskStep::Strategy { .. } => "strategy",
            AskStep::Answer { .. } => "answer",
            AskStep::Final { .. } => "final",
        })
    };
    let result = b.ask("When is the launch?", None, &mut on_step, &Cancel::new()).unwrap();
    assert_eq!(steps, ["strategy", "answer", "final"]);
    assert_eq!(result.answer, format!("It launches in March [{}].", src.id));
    assert!(result.documents.iter().any(|d| d.id == src.id && d.title == "Launch plan" && d.notebook_id.as_deref() == Some(nb.id.as_str())));
    assert!(result.documents.iter().any(|d| d.title == "Launch worries"), "matching notes are searched too");
}

#[test]
fn ask_runs_several_searches_and_keeps_their_order() {
    let env = setup_with(AppSettings { chat: model("m"), ..Default::default() });
    env.fake.reply(|req| {
        let msg = &req.messages[0].content;
        if msg.contains("plan how to find") {
            let searches: Vec<_> = ["alpha", "beta", "gamma"].iter().map(|t| serde_json::json!({ "term": t, "instructions": "" })).collect();
            return serde_json::json!({ "reasoning": "", "searches": searches }).to_string();
        }
        if msg.contains("Partial answers") {
            return msg.clone();
        }
        "partial".into()
    });
    let nb = env.backend.create_notebook("N", "").unwrap();
    env.backend.add_source(&nb.id, text("alpha beta gamma words", "Doc")).unwrap();
    env.backend.idle();
    let answers = Mutex::new(Vec::new());
    let mut on_step = |s: AskStep| {
        if let AskStep::Answer { term, .. } = s {
            answers.lock().unwrap().push(term)
        }
    };
    let result = env.backend.ask("q", Some(&nb.id), &mut on_step, &Cancel::new()).unwrap();
    assert_eq!(answers.lock().unwrap().len(), 3);
    let (a, b, c) = (result.answer.find("## alpha").unwrap(), result.answer.find("## beta").unwrap(), result.answer.find("## gamma").unwrap());
    assert!(a < b && b < c, "partial answers keep the plan's order");
}

#[test]
fn ask_without_a_chat_model_explains_what_to_do() {
    let env = setup_with(AppSettings::default());
    let err = env.backend.ask("q", None, &mut |_| {}, &Cancel::new()).unwrap_err();
    assert!(err.to_string().contains("Choose a chat model"));
}
