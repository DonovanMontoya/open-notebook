//! Sources: processing, links, files, sharing between notebooks, search and the index.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::types::HitKind;

#[test]
fn text_sources_are_chunked_embedded_and_searchable() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Research", "").unwrap();
    let s = b.add_source(&nb.id, text("Zebras zigzag across the zoo.", "Zoo facts")).unwrap();
    assert_eq!(s.status.as_deref(), Some("processing"));
    b.idle();

    let source = b.get_source(&s.id).unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("completed"));
    assert_eq!(source.summary.embedded_chunks, 1);
    assert_eq!(env.fake.embed_count(), 1);

    let hits = b.search("zebra zoo", Some(&nb.id)).unwrap();
    assert_eq!(hits[0].id, s.id);
    assert_eq!(hits[0].kind, HitKind::Source);
    assert_eq!(hits[0].notebook_id.as_deref(), Some(nb.id.as_str()));
}

#[test]
fn link_sources_take_their_text_and_title_from_the_page() {
    let env = setup();
    env.pages.lock().unwrap().insert("https://example.com/a".into(), Extracted { title: "Example article".into(), text: "Body of the article.".into(), ..Default::default() });
    let nb = env.backend.create_notebook("Links", "").unwrap();
    let s = env.backend.add_source(&nb.id, link("https://example.com/a")).unwrap();
    assert_eq!(s.title.as_deref(), Some("https://example.com/a"), "the address is the title until the page is read");
    env.backend.idle();

    let source = env.backend.get_source(&s.id).unwrap();
    assert_eq!(source.summary.title.as_deref(), Some("Example article"));
    assert_eq!(source.full_text, "Body of the article.");
    assert_eq!(source.summary.asset.unwrap().url.as_deref(), Some("https://example.com/a"));
}

#[test]
fn links_are_read_through_the_http_client_by_default() {
    let dir = TempDir::new();
    let fake = Fake::new();
    let http = |r: crate::net::Request| -> std::result::Result<crate::net::Response, crate::net::Transport> {
        assert_eq!(r.url, "https://example.com/page");
        Ok(crate::net::Response::new(200, "<html><head><title>A page</title></head><body><p>Readable words here.</p></body></html>")
            .with_header("content-type", "text/html"))
    };
    let mut opts = options(both_models(), &fake, &Pages::default());
    opts.http = Some(Arc::new(http));
    opts.extract_url = None;
    let b = LocalBackend::open_with(dir.path(), opts).unwrap();
    let nb = b.create_notebook("N", "").unwrap();
    let s = b.add_source(&nb.id, link("https://example.com/page")).unwrap();
    b.idle();
    let source = b.get_source(&s.id).unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("completed"), "{:?}", source.summary.error);
    assert_eq!(source.summary.title.as_deref(), Some("A page"));
    assert!(source.full_text.contains("Readable words here."));
}

#[test]
fn a_failed_source_records_the_error_and_can_be_retried() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Links", "").unwrap();
    let s = b.add_source(&nb.id, link("https://example.com/missing")).unwrap();
    b.idle();
    let source = b.get_source(&s.id).unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("failed"));
    assert!(source.summary.error.unwrap().contains("404"));

    env.pages.lock().unwrap().insert("https://example.com/missing".into(), Extracted { title: "Back".into(), text: "Now it exists.".into(), ..Default::default() });
    b.retry_source(&s.id).unwrap();
    b.idle();
    let source = b.get_source(&s.id).unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("completed"));
    assert_eq!(source.summary.error, None);
}

#[test]
fn sources_still_processing_when_the_app_closed_are_marked_failed_on_next_open() {
    let env = setup();
    {
        let conn = env.backend.db.lock();
        conn.execute("INSERT INTO notebook (id, name, created, updated) VALUES ('nb', 'x', 'now', 'now')", []).unwrap();
        conn.execute("INSERT INTO source (id, notebook_id, kind, status, created, updated) VALUES ('s', 'nb', 'text', 'processing', 'now', 'now')", [])
            .unwrap();
        conn.execute("INSERT INTO notebook_source (source_id, notebook_id) VALUES ('s', 'nb')", []).unwrap();
    }
    let reopened = LocalBackend::open_with(env.dir.path(), options(AppSettings::default(), &env.fake, &env.pages)).unwrap();
    let source = reopened.get_source("s").unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("failed"));
    assert!(source.summary.error.unwrap().contains("interrupted"));
    assert_eq!(reopened.list_transformations().unwrap().len(), crate::insights::DEFAULT_TRANSFORMATIONS.len(), "seeded only once");
}

#[test]
fn without_an_embedding_model_sources_complete_and_search_falls_back_to_keywords() {
    let env = setup_with(AppSettings { chat: model("fake-chat"), ..Default::default() });
    let b = &env.backend;
    let nb = b.create_notebook("Keywords", "").unwrap();
    let s = b.add_source(&nb.id, text("Photosynthesis converts light.", "Bio")).unwrap();
    b.create_note(&nb.id, Some("Idea"), "Light and plants", false).unwrap();
    b.idle();

    assert_eq!(b.get_source(&s.id).unwrap().summary.status.as_deref(), Some("completed"));
    assert_eq!(env.fake.embed_count(), 0);
    let mut kinds: Vec<HitKind> = b.search("light", None).unwrap().into_iter().map(|h| h.kind).collect();
    kinds.sort_by_key(|k| *k == HitKind::Source);
    assert_eq!(kinds, [HitKind::Note, HitKind::Source]);
}

#[test]
fn deleting_a_notebook_removes_its_sources_chunks_notes_and_chats() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Temp", "").unwrap();
    b.add_source(&nb.id, text("Text", "T")).unwrap();
    b.create_note(&nb.id, None, "n", false).unwrap();
    b.idle();
    b.send_chat_message(chat(&nb.id, "hi")).unwrap();

    b.delete_notebook(&nb.id).unwrap();
    for table in ["notebook", "source", "chunk", "note", "chat_session", "chat_message", "notebook_source"] {
        assert_eq!(count(&env, table), 0, "{table} is empty");
    }
}

#[test]
fn a_batch_of_sources_is_processed_two_at_a_time_and_all_complete() {
    let env = setup();
    // Long enough that the next source is queued while the first is still embedding.
    env.fake.slow_embeddings(Duration::from_millis(150));
    let nb = env.backend.create_notebook("Batch", "").unwrap();
    for i in 0..6 {
        env.backend.add_source(&nb.id, text(&format!("Document {i}"), &format!("Doc {i}"))).unwrap();
    }
    env.backend.idle();
    let sources = env.backend.list_sources(&nb.id).unwrap();
    assert_eq!(sources.len(), 6);
    assert!(sources.iter().all(|s| s.status.as_deref() == Some("completed")));
    assert_eq!(env.fake.peak.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn a_source_can_be_in_several_notebooks_and_deleting_one_notebook_keeps_shared_sources() {
    let env = setup();
    let b = &env.backend;
    let a = b.create_notebook("A", "").unwrap();
    let nb_b = b.create_notebook("B", "").unwrap();
    let shared = b.add_source(&a.id, text("Shared text", "Shared")).unwrap();
    let only = b.add_source(&a.id, text("Only in A", "Only")).unwrap();
    b.idle();

    b.link_source(&shared.id, &nb_b.id).unwrap();
    assert_eq!(b.list_sources(&nb_b.id).unwrap().iter().map(|s| s.title.clone().unwrap()).collect::<Vec<_>>(), ["Shared"]);
    assert_eq!(b.get_source(&shared.id).unwrap().summary.notebooks, [a.id.clone(), nb_b.id.clone()]);
    assert_eq!(b.get_notebook(&nb_b.id).unwrap().source_count, 1);
    assert_eq!(b.list_all_sources().unwrap().iter().find(|s| s.id == shared.id).unwrap().notebooks.len(), 2);

    b.rename_source(&shared.id, "Renamed").unwrap();
    b.delete_notebook(&a.id).unwrap();
    let kept = b.get_source(&shared.id).unwrap();
    assert_eq!(kept.summary.title.as_deref(), Some("Renamed"));
    assert_eq!(kept.summary.notebooks, std::slice::from_ref(&nb_b.id));
    assert!(b.get_source(&only.id).unwrap_err().to_string().contains("not found"));

    b.unlink_source(&shared.id, &nb_b.id).unwrap();
    assert!(b.list_sources(&nb_b.id).unwrap().is_empty());
}

fn document(title: &str, content: &str, file_name: &str, data: &[u8], mime: &str) -> NewSource {
    NewSource {
        input: SourceInput::Document {
            title: title.into(),
            content: content.into(),
            file_name: Some(file_name.into()),
            file: Some(data.to_vec()),
            mime: Some(mime.into()),
        },
        transformations: vec![],
    }
}

#[test]
fn document_files_are_kept_downloadable_and_deleted_with_the_source() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("Docs", "").unwrap();
    let s = b.add_source(&nb.id, document("Report", "The report text.", "report/2026.pdf", b"%PDF-1", "application/pdf")).unwrap();
    b.idle();

    let source = b.get_source(&s.id).unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("completed"));
    assert_eq!(source.full_text, "The report text.");
    assert!(source.summary.file_available);
    assert_eq!(source.summary.asset.unwrap().file_path.as_deref(), Some("report/2026.pdf"));
    let file_ref: String = b.db.lock().query_row("SELECT file_ref FROM source WHERE id = ?1", [&s.id], |r| r.get(0)).unwrap();
    assert!(file_ref.starts_with(&env.dir.path().join("originals").to_string_lossy().into_owned()), "{file_ref}");
    assert!(file_ref.ends_with("__report_2026.pdf"), "named like the Tauri app's files: {file_ref}");

    let original = b.download_source(&s.id).unwrap().unwrap();
    assert_eq!((original.data.as_slice(), original.name.as_str(), original.mime.as_str()), (&b"%PDF-1"[..], "report/2026.pdf", "application/pdf"));

    b.delete_source(&s.id).unwrap();
    assert!(!std::path::Path::new(&file_ref).exists(), "deleting the source deletes the file");
    assert!(b.download_source(&s.id).unwrap().is_none());
}

#[test]
fn audio_without_text_fails_until_transcription_arrives() {
    let env = setup();
    let nb = env.backend.create_notebook("Interviews", "").unwrap();
    let s = env.backend.add_source(&nb.id, document("Interview", "", "interview.mp3", &[1, 2, 3, 4], "audio/mpeg")).unwrap();
    env.backend.idle();
    let source = env.backend.get_source(&s.id).unwrap();
    assert_eq!(source.summary.status.as_deref(), Some("failed"));
    assert_eq!(source.summary.error.as_deref(), Some("Transcription isn't in the Mac app yet"));
    assert_eq!(env.backend.download_source(&s.id).unwrap().unwrap().data, [1, 2, 3, 4], "the recording is kept");
    let kind: String = env.backend.db.lock().query_row("SELECT kind FROM source WHERE id = ?1", [&s.id], |r| r.get(0)).unwrap();
    assert_eq!(kind, "media");
}

#[test]
fn calls_are_recorded_in_the_usage_log_and_summarized_by_feature_and_model() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    b.add_source(&nb.id, text("Text to embed", "T")).unwrap();
    b.idle();
    b.send_chat_message(chat(&nb.id, "Hello there")).unwrap();
    let report = Backend::usage(b, 30).unwrap();
    assert!(report.estimated);
    let chat = report.groups.iter().find(|g| g.feature == "chat").unwrap();
    assert_eq!((chat.model.as_str(), chat.calls), ("fake-chat", 1));
    assert!(chat.input_tokens > 0 && chat.output_tokens > 0);
    assert!(report.groups.iter().any(|g| g.feature == "embedding" && g.model == "fake-embed"));
    assert_eq!(report.calls, report.groups.iter().map(|g| g.calls).sum::<u64>());
}

#[test]
fn rebuilding_the_index_re_processes_every_source() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    b.add_source(&nb.id, text("One", "A")).unwrap();
    b.add_source(&nb.id, text("Two", "B")).unwrap();
    b.idle();
    let before = env.fake.embed_count();
    assert_eq!(Backend::rebuild_index(b).unwrap(), 2);
    b.idle();
    assert_eq!(env.fake.embed_count(), before + 2);
    assert!(b.list_sources(&nb.id).unwrap().iter().all(|s| s.status.as_deref() == Some("completed")));
}
