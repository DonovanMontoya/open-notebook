//! The app against a real on-device backend in a temp folder, with a fake AI provider:
//! make a notebook, add a source, ask a question, and check what the window shows.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notebook_engine::Backend;
use notebook_engine::local::{LocalBackend, LocalOptions};
use notebook_engine::net::{Request, Response, Transport};
use notebook_engine::settings::{AppSettings, ModelSetting, Secrets, provider_secret};
use notebook_engine::types::MessageKind;
use rune::{Env, Runtime, TextEdit};

use crate::state::{App, Route};

/// An Anthropic stream that answers with a citation of the source.
fn fake_anthropic(req: Request) -> Result<Response, Transport> {
    fake_anthropic_reply(req, &["Spindles help ", "memory [Spindle notes]."])
}

fn fake_anthropic_reply(req: Request, chunks: &[&str]) -> Result<Response, Transport> {
    assert!(req.url.contains("anthropic.com"), "unexpected request to {}", req.url);
    let mut body = String::from("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n");
    for c in chunks {
        body += &format!(
            "event: content_block_delta\ndata: {}\n\n",
            serde_json::json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": c}})
        );
    }
    body += "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    Ok(Response { status: 200, headers: vec![("content-type".into(), "text/event-stream".into())], body: Box::new(std::io::Cursor::new(body.into_bytes())) })
}

fn wait(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let start = Instant::now();
    loop {
        app.drain();
        if done(app) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_notebook_from_empty_to_a_cited_answer() {
    let dir = std::env::temp_dir().join(format!("on-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let settings = AppSettings { chat: Some(ModelSetting { provider: "anthropic".into(), model: "claude-test".into() }), ..AppSettings::default() };
    settings.save(&dir).unwrap();
    Secrets::new(&dir).set(&provider_secret("anthropic"), "sk-test").unwrap();
    let backend = LocalBackend::open_with(&dir, LocalOptions { http: Some(Arc::new(fake_anthropic)), ..Default::default() }).unwrap();
    let mut app = App::with_backend(dir.clone(), Arc::new(backend));
    wait(&mut app, "the notebook list", |a| a.loaded);
    assert!(app.notebooks.is_empty());

    app.new_notebook = Some(TextEdit::new("Sleep"));
    app.create_notebook();
    wait(&mut app, "the new notebook to open", |a| matches!(a.route, Route::Notebook(_)) && a.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    assert_eq!(app.notebooks.len(), 1);

    app.add_source(notebook_engine::types::SourceInput::Text {
        content: "Spindle density after learning predicted recall the next morning.".into(),
        title: Some("Spindle notes".into()),
    });
    wait(&mut app, "the source to be added", |a| !a.open.as_ref().unwrap().sources.is_empty());
    // Processing runs in the background; the app refreshes the list from `poll`, as here.
    for _ in 0..200 {
        if !app.open.as_ref().unwrap().processing() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
        app.refresh_sources();
        app.drain();
    }
    assert!(!app.open.as_ref().unwrap().processing(), "the source finished: {:?}", app.open.as_ref().unwrap().sources);
    let source = app.open.as_ref().unwrap().sources[0].clone();
    assert_eq!(source.title.as_deref(), Some("Spindle notes"));
    assert_ne!(source.status.as_deref(), Some("failed"), "{:?}", source.error);

    app.open.as_mut().unwrap().draft = TextEdit::new("What do spindles do?");
    app.send();
    assert!(app.open.as_ref().unwrap().pending.is_some(), "the question shows while it is sent");
    wait(&mut app, "the answer", |a| a.open.as_ref().unwrap().sending.is_none());
    let o = app.open.as_ref().unwrap();
    assert_eq!(o.error, None);
    let messages = &o.session.as_ref().expect("a session").messages;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind, MessageKind::Human);
    assert_eq!(messages[1].content, "Spindles help memory [Spindle notes].");
    assert_eq!(o.sessions.len(), 1);

    // The window: the source in the rail, the answer with its citation as a link.
    let env = Env { system_dark: false, scale: 2.0 };
    let mut rt = Runtime::new(app, crate::ui::view, (1280.0, 800.0), env);
    rt.frame(0.0);
    assert!(rt.find(&format!("src-{}", source.id)).is_some(), "the source row is drawn");
    assert!(rt.find("draft").is_some(), "the composer is drawn");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Opens a copy of the Tauri app's data folder: `OPEN_NOTEBOOK_TEST_DATA=<copy> cargo test -- --ignored`.
#[test]
#[ignore]
fn opens_the_tauri_apps_data() {
    let Some(dir) = std::env::var_os("OPEN_NOTEBOOK_TEST_DATA").map(std::path::PathBuf::from) else { return };
    let backend = LocalBackend::open(&dir).expect("the database opens and migrates");
    let mut app = App::with_backend(dir, Arc::new(backend));
    wait(&mut app, "the notebook list", |a| a.loaded);
    assert!(app.toast.is_none(), "{:?}", app.toast);
    println!("notebooks: {}", app.notebooks.len());
    let Some(first) = app.notebooks.first().map(|n| n.id.clone()) else { return };
    app.open_notebook(first);
    wait(&mut app, "the notebook", |a| a.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    std::thread::sleep(Duration::from_millis(300));
    app.drain();
    let o = app.open.as_ref().unwrap();
    println!(
        "sources: {}, notes: {}, sessions: {}, messages: {}",
        o.sources.len(),
        o.notes.len(),
        o.sessions.len(),
        o.session.as_ref().map_or(0, |s| s.messages.len())
    );
    assert!(app.toast.is_none(), "{:?}", app.toast);
}

#[test]
fn duo_keeps_the_passage_beside_the_answer_and_clear_of_the_fold() {
    for (width, fold) in [(951.0, None), (951.0, Some([460.0, 0.0, 24.0, 669.0])), (870.0, Some([456.0, 0.0, 40.0, 669.0]))] {
        let mut app = App::offline();
        crate::scenes::SCENES.iter().find(|(name, _)| *name == "notebook").unwrap().1(&mut app);
        app.fold = fold;
        let mut rt = Runtime::new(app, crate::ui::view, (width, 669.0), Env { system_dark: false, scale: 2.0 });
        for frame in 0..3 {
            rt.frame(frame as f64 / 60.0);
        }
        let left = rt.presented("left-page").expect("source page").0;
        let right = rt.presented("right-page").expect("answer page").0;
        assert!(rt.find("draft").is_some());
        assert!(rt.find("desk").is_none());
        let new_chat = rt.presented("new-chat").expect("new conversation action").0;
        assert!(new_chat[0] >= right[0] && new_chat[0] + new_chat[2] <= right[0] + right[2] + 0.5, "conversation actions stay on the right page");
        if let Some([x, _, width, _]) = fold {
            assert!(left[0] + left[2] <= x + 0.5);
            assert!(right[0] >= x + width - 0.5);
        } else {
            assert!((left[2] - right[2]).abs() < 2.0);
        }
    }
}

#[test]
fn phone_keeps_navigation_and_composer_in_bounds() {
    let mut app = App::offline();
    crate::scenes::SCENES.iter().find(|(name, _)| *name == "chat").unwrap().1(&mut app);
    let mut rt = Runtime::new(app, crate::ui::view, (393.0, 852.0), Env { system_dark: false, scale: 2.0 });
    for frame in 0..3 {
        rt.frame(frame as f64 / 60.0);
    }
    for key in ["draft", "tab-bar", "notebook-name"] {
        let [x, y, w, h] = rt.presented(key).expect(key).0;
        assert!(x >= 0.0 && x + w <= 393.5 && y >= 0.0 && y + h <= 852.5, "{key}: {x} {y} {w} {h}");
    }
    assert!(rt.presented("tab-bar").unwrap().0[3] >= 44.0);
    rt.state.keyboard_up = true;
    rt.invalidate();
    rt.frame(1.0);
    assert!(rt.find("tab-bar").is_none());
    assert!(rt.find("draft").is_some());
}

#[test]
fn saving_a_source_name_after_dismissing_the_keyboard_returns_to_reading() {
    let mut app = App::offline();
    crate::scenes::SCENES.iter().find(|(name, _)| *name == "notebook").unwrap().1(&mut app);
    let mut name = TextEdit::new("A new source name");
    name.focused = true;
    app.open.as_mut().unwrap().renaming_source = Some(name);
    let mut rt = Runtime::new(app, crate::ui::view, (393.0, 852.0), Env::new(false, 2.0));
    rt.frame(0.0);
    rt.update(|_, tx| tx.blur());
    rt.frame(0.1);
    let (x, y) = rt.find("save-source-name").expect("visible Save name action");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert!(rt.state.open.as_ref().unwrap().renaming_source.is_none());
    assert!(rt.find("reader").is_some());
}

#[test]
fn mobile_home_states_keep_settings_and_global_navigation_reachable() {
    for state in ["empty", "no-model", "loading", "error", "archived", "populated"] {
        for (width, height) in [(393.0, 852.0), (466.0, 678.0), (951.0, 669.0)] {
            let mut app = App::offline();
            app.settings.chat = Some(ModelSetting { provider: "anthropic".into(), model: "sample".into() });
            match state {
                "populated" => crate::scenes::SCENES.iter().find(|(name, _)| *name == "home").unwrap().1(&mut app),
                "no-model" => app.settings.chat = None,
                "loading" => app.loaded = false,
                "error" => app.backend_error = Some("Server unavailable".into()),
                "archived" => {
                    crate::scenes::SCENES.iter().find(|(name, _)| *name == "archived").unwrap().1(&mut app);
                    for notebook in &mut app.notebooks {
                        notebook.archived = true;
                    }
                }
                _ => {}
            }
            let mut rt = Runtime::new(app, crate::ui::view, (width, height), Env::new(false, 2.0));
            for frame in 0..3 {
                rt.frame(frame as f64 / 60.0);
            }
            let [x, y, w, h] = rt.presented("home-navigation-settings").unwrap_or_else(|| panic!("{state}: Settings must always be available")).0;
            assert!(w >= 44.0 && h >= 44.0 && x >= 0.0 && x + w <= width && y >= 0.0 && y + h <= height);
            for key in ["home-ask", "tool-library", "tool-transformations", "tool-usage", "tool-settings"] {
                assert!(rt.find(key).is_some(), "{state}: missing {key}");
            }
            rt.pointer_down(x + w / 2.0, y + h / 2.0);
            rt.pointer_up(x + w / 2.0, y + h / 2.0);
            rt.frame(1.0);
            assert_eq!(rt.state.route, Route::Settings, "{state}: Settings opens");
            let (x, y) = rt.find("back").expect("back to notebooks");
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.1);
            assert_eq!(rt.state.route, Route::Home, "{state}: can return home");
        }
    }
}

/// A phone notebook shows what its search found, as the desk and the Duo do.
#[test]
fn phone_notebook_shows_search_results() {
    let dir = std::env::temp_dir().join(format!("on-phone-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let backend = LocalBackend::open_with(&dir, LocalOptions::default()).unwrap();
    let mut app = App::with_backend(dir.clone(), Arc::new(backend));
    wait(&mut app, "the notebook list", |a| a.loaded);
    app.new_notebook = Some(TextEdit::new("Sleep"));
    app.create_notebook();
    wait(&mut app, "the new notebook to open", |a| matches!(a.route, Route::Notebook(_)) && a.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    let o = app.open.as_mut().unwrap();
    o.search_open = true;
    o.hits = Some(("spindles".into(), Ok(Vec::new())));
    let mut rt = Runtime::new(app, crate::ui::view, (390.0, 800.0), Env { system_dark: false, scale: 3.0 });
    rt.frame(0.0);
    assert!(rt.find("search-results").is_some(), "the phone notebook shows search results");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Two notebooks on a fresh backend with the fake provider, and the app on the first. A
/// question that contains `held` waits for a value on the returned sender before it is answered.
fn two_notebooks(name: &str, held: &'static str) -> (App, Arc<dyn Backend>, String, String, Sender<()>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("on-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let settings = AppSettings { chat: Some(ModelSetting { provider: "anthropic".into(), model: "claude-test".into() }), ..AppSettings::default() };
    settings.save(&dir).unwrap();
    Secrets::new(&dir).set(&provider_secret("anthropic"), "sk-test").unwrap();
    let (release, gate) = channel::<()>();
    let gate = Mutex::new(gate);
    let http = move |req: Request| {
        if req.body.as_ref().is_some_and(|b| String::from_utf8_lossy(b).contains(held)) {
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(10));
        }
        fake_anthropic(req)
    };
    let backend: Arc<dyn Backend> = Arc::new(LocalBackend::open_with(&dir, LocalOptions { http: Some(Arc::new(http)), ..Default::default() }).unwrap());
    let a = backend.create_notebook("A", "").unwrap().id;
    let b = backend.create_notebook("B", "").unwrap().id;
    let mut app = App::with_backend(dir.clone(), backend.clone());
    wait(&mut app, "the notebook list", |a| a.loaded);
    app.open_notebook(a.clone());
    wait(&mut app, "notebook A", |app| app.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    (app, backend, a, b, release, dir)
}

/// Applies worker results for a moment, so late ones have arrived.
fn linger(app: &mut App) {
    for _ in 0..30 {
        app.drain();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_answer_that_arrives_after_leaving_stays_with_its_notebook() {
    let (mut app, backend, a, b, release, dir) = two_notebooks("leave", "held question");
    app.open.as_mut().unwrap().draft = TextEdit::new("A held question");
    app.send();
    app.open_notebook(b.clone());
    wait(&mut app, "notebook B", |app| app.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    release.send(()).unwrap();
    wait(&mut app, "the answer to be saved", |_| backend.list_chat_sessions(&a).unwrap().first().is_some_and(|s| s.message_count == Some(2)));
    linger(&mut app);
    let o = app.open.as_ref().unwrap();
    assert_eq!(o.id, b);
    assert!(o.session.is_none() && o.sessions.is_empty(), "B shows none of A's conversation");
    assert!(o.pending.is_none() && o.sending.is_none() && o.error.is_none());

    // B's first message starts B's own conversation.
    app.open.as_mut().unwrap().draft = TextEdit::new("Anything here?");
    app.send();
    wait(&mut app, "B's answer", |a| a.open.as_ref().unwrap().sending.is_none());
    let session = app.open.as_ref().unwrap().session.clone().expect("a session");
    assert_eq!(session.session.notebook_id, b);
    assert_eq!(session.messages.len(), 2);
    let theirs = backend.list_chat_sessions(&a).unwrap();
    assert_eq!(theirs.len(), 1);
    assert_ne!(theirs[0].id, session.session.id);
    assert_eq!(backend.get_chat_session(&theirs[0].id).unwrap().messages.len(), 2);

    // Back in A, its conversation is there.
    app.open_notebook(a);
    wait(&mut app, "A's conversation", |app| app.open.as_ref().unwrap().session.as_ref().is_some_and(|s| s.messages.len() == 2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_answer_to_an_earlier_visit_leaves_the_reopened_notebook_alone() {
    let (mut app, backend, a, b, release, dir) = two_notebooks("revisit", "held question");
    app.open.as_mut().unwrap().draft = TextEdit::new("A held question");
    app.send();
    app.open_notebook(b);
    app.open_notebook(a.clone());
    wait(&mut app, "notebook A again", |app| app.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    linger(&mut app);
    // The reopened view asks its own question while the first is still waiting.
    app.open.as_mut().unwrap().draft = TextEdit::new("Second question");
    app.send();
    wait(&mut app, "the second answer", |a| a.open.as_ref().unwrap().sending.is_none());
    release.send(()).unwrap();
    wait(&mut app, "the first answer to be saved", |_| backend.list_chat_sessions(&a).unwrap().len() == 2);
    linger(&mut app);
    let o = app.open.as_ref().unwrap();
    let s = o.session.as_ref().expect("a session");
    assert_eq!(s.messages[0].content, "Second question", "the view keeps the conversation it sent");
    assert_eq!(o.sessions.len(), 1, "the earlier conversation is listed once the notebook reloads, not pushed in");
    assert!(o.pending.is_none() && o.error.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_notebook_left_while_loading_does_not_open_its_chat_in_the_next() {
    let (mut app, _backend, a, b, _release, dir) = two_notebooks("reload", "never sent");
    app.open.as_mut().unwrap().draft = TextEdit::new("What do spindles do?");
    app.send();
    wait(&mut app, "the answer", |a| a.open.as_ref().unwrap().sending.is_none());
    app.open_notebook(b.clone());
    // A starts loading, and its latest conversation with it, but B opens first.
    app.open_notebook(a);
    app.open_notebook(b.clone());
    wait(&mut app, "notebook B", |app| app.open.as_ref().is_some_and(|o| o.notebook.is_some()));
    linger(&mut app);
    let o = app.open.as_ref().unwrap();
    assert_eq!(o.id, b);
    assert!(o.session.is_none() && o.sessions.is_empty(), "{:?}", o.session.as_ref().map(|s| &s.session));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn library_reader_keeps_materials_separate_from_the_workspace() {
    for (width, height) in [(393.0, 852.0), (951.0, 669.0), (1280.0, 800.0)] {
        let mut app = App::offline();
        crate::scenes::SCENES.iter().find(|(name, _)| *name == "library-reader").unwrap().1(&mut app);
        let mut rt = Runtime::new(app, crate::ui::view, (width, height), Env::new(false, 1.0));
        for frame in 0..3 {
            rt.frame(frame as f64 / 60.0);
        }
        assert!(rt.find("library-reader").is_some());
        assert!(rt.find("insights").is_none());
        assert!(rt.find("draft").is_none());
        for key in ["reader-back", "reader-find", "reader-workspace"] {
            let [x, y, w, h] = rt.presented(key).expect(key).0;
            assert!(x >= 0.0 && x + w <= width + 0.5 && y >= 0.0 && y + h <= height + 0.5, "{key} at {width}: {x} {y} {w} {h}");
        }
        let (x, y) = rt.find("reader-back").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0);
        assert!(rt.find("library-filter").is_some());
        assert!(rt.state.library.as_ref().unwrap().reading.is_none());
        assert!(matches!(rt.state.route, Route::Library));
    }
}

#[test]
fn reader_questions_send_selected_context_and_image_then_reload_the_conversation() {
    let dir = std::env::temp_dir().join(format!("on-reader-chat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    AppSettings { chat: Some(ModelSetting { provider: "anthropic".into(), model: "vision-test".into() }), ..Default::default() }.save(&dir).unwrap();
    Secrets::new(&dir).set(&provider_secret("anthropic"), "sk-test").unwrap();
    let sent = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let captured = sent.clone();
    let http = move |req: Request| {
        captured.lock().unwrap().push(serde_json::from_slice(req.body.as_ref().unwrap()).unwrap());
        fake_anthropic_reply(req, &["Here is the figure.\n```html\n<html><button>Play</button></html>\n```"])
    };
    let backend: Arc<dyn Backend> = Arc::new(LocalBackend::open_with(&dir, LocalOptions { http: Some(Arc::new(http)), ..Default::default() }).unwrap());
    let nb = backend.create_notebook("Paper", "").unwrap();
    let source = backend
        .add_source(
            &nb.id,
            notebook_engine::types::NewSource {
                input: notebook_engine::types::SourceInput::Text { content: "The paper's findings.".into(), title: Some("Paper".into()) },
                transformations: Vec::new(),
            },
        )
        .unwrap();
    let mut app = App::with_backend(dir.clone(), backend.clone());
    wait(&mut app, "source processing", |_| backend.get_source(&source.id).unwrap().summary.status.as_deref() == Some("completed"));
    app.go(Route::Library);
    app.read_library_source(source.id.clone());
    wait(&mut app, "the reading conversation", |a| a.library.as_ref().is_some_and(|p| p.reading.is_some() && !p.chat.loading));
    let chat = &mut app.library.as_mut().unwrap().chat;
    chat.context = Some(crate::reading::ReadingContext {
        kind: "figure".into(),
        label: "Figure 2".into(),
        page: 7,
        text: "Association maps".into(),
        png: Some("cGFnZQ==".into()),
        references: vec![crate::reading::ReadingReference { page: 6, text: "The paragraph discussing Figure 2.".into() }],
    });
    chat.draft = TextEdit::new("What do the axes mean?");
    app.send_reading_question();
    assert_eq!(app.library.as_ref().unwrap().chat.pending.as_ref().unwrap().0, "What do the axes mean?");
    wait(&mut app, "the reading answer", |a| a.library.as_ref().unwrap().chat.sending.is_none());
    let chat = &app.library.as_ref().unwrap().chat;
    assert!(chat.error.is_none(), "{:?}", chat.error);
    assert!(chat.pending.is_none());
    let session_id = chat.session.as_ref().unwrap().session.id.clone();
    assert!(chat.showing_visualization);
    assert_eq!(chat.visualization.as_ref().unwrap().document.html, "<html><button>Play</button></html>");
    let requests = sent.lock().unwrap();
    let blocks = requests.last().unwrap()["messages"].as_array().unwrap().last().unwrap()["content"].as_array().unwrap();
    assert!(blocks.iter().any(|b| b["type"] == "image" && b["source"]["data"] == "cGFnZQ=="));
    let question = blocks.iter().find(|b| b["type"] == "text").unwrap()["text"].as_str().unwrap();
    for part in ["What do the axes mean?", "PDF page 7", "Association maps", "The paragraph discussing Figure 2."] {
        assert!(question.contains(part), "{part}");
    }
    drop(requests);
    app.read_library_source(source.id.clone());
    wait(&mut app, "the saved reading conversation", |a| a.library.as_ref().is_some_and(|p| p.reading.is_some() && !p.chat.loading));
    let session = app.library.as_ref().unwrap().chat.session.as_ref().unwrap();
    assert_eq!(session.session.id, session_id);
    assert_eq!(session.messages.len(), 2);
    assert!(app.library.as_ref().unwrap().chat.visualization.is_some(), "the saved visualization can be opened again");
    assert!(!app.library.as_ref().unwrap().chat.showing_visualization, "reopening starts with the original material");
    assert!(matches!(app.route, Route::Library));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn settings_sections_and_actions_fit_compact_and_desktop_windows() {
    for (width, height) in [(393.0, 852.0), (951.0, 669.0), (1280.0, 800.0)] {
        for scene in ["settings", "settings-keys", "settings-models", "settings-server"] {
            let mut app = App::offline();
            crate::scenes::SCENES.iter().find(|(name, _)| *name == scene).unwrap().1(&mut app);
            let mut rt = Runtime::new(app, crate::ui::view, (width, height), Env::new(false, 1.0));
            for frame in 0..3 {
                rt.frame(frame as f64 / 60.0);
            }
            for key in ["settings-tabs", "recommend-models", "save-settings", "check-saved-keys", "check-openai", "check-anthropic", "save-connect"] {
                if let Some((rect, _)) = rt.presented(key) {
                    let [x, _, w, _] = rect;
                    assert!(x >= -0.5 && x + w <= width + 0.5, "{scene} {width} {key}: {rect:?}");
                }
            }
        }
    }
}

#[test]
fn settings_connection_shortcuts_open_the_relevant_section() {
    let mut app = App::offline();
    crate::scenes::SCENES.iter().find(|(name, _)| *name == "settings").unwrap().1(&mut app);
    let mut rt = Runtime::new(app, crate::ui::view, (1280.0, 800.0), Env::new(false, 1.0));
    rt.frame(0.0);
    let (x, y) = rt.find("settings-open-models").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.1);
    assert!(rt.find("recommend-models").is_some());
    assert_eq!(rt.state.form.as_ref().unwrap().tab, crate::configuration::SettingsTab::Models);
}

#[test]
fn rendering_settings_keeps_in_flight_checks_alive() {
    for scene in ["settings-keys", "settings-server"] {
        let mut app = App::offline();
        crate::scenes::SCENES.iter().find(|(name, _)| *name == scene).unwrap().1(&mut app);
        let form = app.form.as_mut().unwrap();
        let operation = if scene == "settings-keys" { "openai" } else { "server" };
        let ticket = form.begin(operation);
        form.checks.insert(operation.into(), crate::state::Check::Checking);
        let mut rt = Runtime::new(app, crate::ui::view, (1280.0, 800.0), Env::new(false, 1.0));
        for frame in 0..3 {
            rt.frame(frame as f64 / 60.0);
        }
        let form = rt.state.form.as_ref().unwrap();
        assert!(form.owns(operation, &ticket));
        assert_eq!(form.checks.get(operation), Some(&crate::state::Check::Checking));
    }
}
