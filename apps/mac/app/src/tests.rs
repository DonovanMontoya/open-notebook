//! The app against a real on-device backend in a temp folder, with a fake AI provider:
//! make a notebook, add a source, ask a question, and check what the window shows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use notebook_engine::local::{LocalBackend, LocalOptions};
use notebook_engine::net::{Request, Response, Transport};
use notebook_engine::settings::{AppSettings, ModelSetting, Secrets, provider_secret};
use notebook_engine::types::MessageKind;
use rune::{Env, Runtime, TextEdit};

use crate::state::{App, Route};

/// An Anthropic stream that answers with a citation of the source.
fn fake_anthropic(req: Request) -> Result<Response, Transport> {
    assert!(req.url.contains("anthropic.com"), "unexpected request to {}", req.url);
    let chunks = ["Spindles help ", "memory [Spindle notes]."];
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
