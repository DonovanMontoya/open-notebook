//! Port of the backend tests in `podcast/podcast.test.ts`: episodes written by the
//! fake chat provider and voiced by a tone generator, stored in a temp folder.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::super::LocalBackend;
use super::{Env, Fake, Pages, TempDir, both_models, options, text};
use crate::backend::{Backend, EpisodeAudio};
use crate::podcast::check::{CHECK_SYSTEM, RESEARCH_SYSTEM, REVISE_SYSTEM};
use crate::podcast::tests::{ToneVoice, script};
use crate::podcast::voices::{VoiceConfig, VoiceKind, VoiceMode, VoiceProvider};
use crate::settings::Secrets;
use crate::types::{CreateEpisode, EpisodeProfile, EpisodeStatus, PodcastEffort, Speaker, SpeakerProfile};

struct Podcasts {
    env: Env,
    voice: Arc<ToneVoice>,
    configs: Arc<Mutex<Vec<VoiceConfig>>>,
    episode_profile: EpisodeProfile,
}

fn script_json() -> String {
    serde_json::to_string(&script()).unwrap()
}

fn podcast_env(mode: VoiceMode) -> Podcasts {
    let dir = TempDir::new();
    let fake = Fake::new();
    fake.reply_text(&script_json());
    let pages = Pages::default();
    let voice = Arc::new(ToneVoice::new(mode));
    let configs: Arc<Mutex<Vec<VoiceConfig>>> = Arc::default();
    let mut opts = options(both_models(), &fake, &pages);
    let (v, c) = (voice.clone(), configs.clone());
    opts.voices = Some(Arc::new(move |config: &VoiceConfig| {
        c.lock().unwrap().push(config.clone());
        v.clone() as Arc<dyn VoiceProvider>
    }));
    Secrets::new(dir.path()).set("provider:openai", "sk-voice").unwrap();
    let backend = LocalBackend::open_with(dir.path(), opts).unwrap();
    let speakers = backend
        .save_speaker_profile(SpeakerProfile {
            id: String::new(),
            name: "Duo".into(),
            description: String::new(),
            voice_kind: Some("openai".into()),
            voice_model: "gpt-4o-mini-tts".into(),
            speakers: vec![
                Speaker { name: "Marin".into(), voice_id: "marin".into(), backstory: "Former FTC staffer".into(), personality: "Precise".into() },
                Speaker { name: "Cedar".into(), voice_id: "cedar".into(), backstory: "Product designer".into(), personality: "Skeptical".into() },
            ],
        })
        .unwrap();
    let episode_profile = backend
        .save_episode_profile(EpisodeProfile {
            id: String::new(),
            name: "Explainer".into(),
            description: String::new(),
            speaker_profile_id: speakers.id,
            language: Some("en".into()),
            briefing: "Explain it plainly.".into(),
            segments: 2,
        })
        .unwrap();
    Podcasts { env: Env { backend, fake, pages, dir }, voice, configs, episode_profile }
}

fn episode(notebook_id: &str, profile: &EpisodeProfile, effort: Option<PodcastEffort>, focus: Option<&str>) -> CreateEpisode {
    CreateEpisode {
        notebook_id: notebook_id.into(),
        episode_profile: profile.id.clone(),
        speaker_profile: None,
        focus: focus.map(String::from),
        name: None,
        effort,
    }
}

#[test]
fn the_servers_podcast_profiles_are_seeded_once() {
    let dir = TempDir::new();
    let fake = Fake::new();
    let open = || LocalBackend::open_with(dir.path(), options(both_models(), &fake, &Pages::default())).unwrap();
    let backend = open();
    let profiles = Backend::podcast_profiles(&backend).unwrap();
    assert_eq!((profiles.episode.len(), profiles.speaker.len()), (3, 3));
    for e in &profiles.episode {
        assert!(profiles.speaker.iter().any(|s| s.id == e.speaker_profile_id), "{} points at a seeded speaker profile", e.name);
    }
    let err = Backend::delete_speaker_profile(&backend, &profiles.episode[0].speaker_profile_id).unwrap_err();
    assert!(err.to_string().contains("uses these speakers"), "{err}");

    // Deleting a seeded profile sticks: opening again does not bring it back.
    Backend::delete_episode_profile(&backend, &profiles.episode[0].id).unwrap();
    drop(backend);
    assert_eq!(Backend::podcast_profiles(&open()).unwrap().episode.len(), 2);
}

#[test]
fn speaker_profiles_are_saved_in_the_tauri_apps_json_shape() {
    let p = podcast_env(VoiceMode::Line);
    let raw: String = p.env.backend.db.lock().query_row("SELECT speakers FROM speaker_profile WHERE name = 'Duo'", [], |r| r.get(0)).unwrap();
    let speakers: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(speakers[0], json!({ "name": "Marin", "voice": "marin", "backstory": "Former FTC staffer", "personality": "Precise" }));
    let err = Backend::save_speaker_profile(&p.env.backend, SpeakerProfile { name: "Empty".into(), ..Default::default() }).unwrap_err();
    assert_eq!(err.to_string(), "A speaker profile has one to four speakers.");
    let mut long = p.episode_profile.clone();
    long.segments = 40;
    assert_eq!(Backend::save_episode_profile(&p.env.backend, long).unwrap().segments, 12);
}

#[test]
fn an_episode_is_written_from_the_profiles_voiced_and_saved_with_chapters_and_a_timed_transcript() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    let nb = b.create_notebook("Research", "").unwrap();
    b.add_source(&nb.id, text("The FTC report covers four tactics.", "FTC report")).unwrap();
    b.idle();

    let created = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, None, Some("Subscription traps"))).unwrap();
    assert_eq!(created.status, EpisodeStatus::Writing);
    assert_eq!(created.voice_label.as_deref(), Some("Marin, Cedar, OpenAI speech"));
    b.idle();

    let e = Backend::get_episode(b, &created.id).unwrap();
    assert_eq!(e.status, EpisodeStatus::Completed, "{:?}", e.error);
    assert_eq!(e.name, "Dark patterns, explained");
    assert_eq!(e.summary.as_deref(), Some("Two hosts walk through the FTC report."));
    assert_eq!(e.chapters.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["What they are", "Why it matters"]);
    assert_eq!(e.transcript.iter().map(|l| l.speaker.as_str()).collect::<Vec<_>>(), ["Marin", "Cedar", "Marin"]);
    assert!(e.transcript.iter().all(|l| l.start.is_some() && l.end.is_some()));
    assert!(e.exact_timing);
    assert!(e.duration.unwrap() > 1.0);
    assert_eq!(e.progress, Some((3, 3)));
    assert_eq!(e.effort, Some(PodcastEffort::Standard));

    let chats = p.env.fake.chats.lock().unwrap().clone();
    let prompt = &chats[0].messages[0].content;
    for wanted in ["Subscription traps", "Background: Former FTC staffer", "Explain it plainly.", "in 2 chapters", "The FTC report covers four tactics"] {
        assert!(prompt.contains(wanted), "the prompt has {wanted:?}");
    }
    assert!(p.voice.calls().contains(&"cedar:four five".to_string()), "each speaker uses their own voice");
    let config = p.configs.lock().unwrap()[0].clone();
    assert_eq!((config.kind, config.model.as_str(), config.api_key.as_deref()), (VoiceKind::OpenAi, "gpt-4o-mini-tts", Some("sk-voice")));

    // The file is where the Tauri app keeps episodes, so either app can play it.
    let EpisodeAudio::File(path) = Backend::episode_audio(b, &created.id).unwrap() else { panic!("a file") };
    assert_eq!(path.parent().unwrap(), p.env.dir.path().join("podcasts"));
    let stem = created.id.replace(':', "_");
    assert!(path.file_name().unwrap().to_string_lossy().starts_with(&stem), "{}", path.display());
    if path.extension().is_some_and(|e| e == "wav") {
        assert_eq!(&std::fs::read(&path).unwrap()[..4], b"RIFF");
    }

    let kept = Backend::episode_prompt(b, &created.id).unwrap().unwrap();
    assert_eq!(&kept.prompt, prompt, "the exact prompt is saved with the episode");
    assert_eq!(kept.system, chats[0].system);
    assert_eq!(kept.model, "fake-chat");

    let usage = Backend::usage(b, 30).unwrap();
    for feature in ["podcast_writing", "podcast_check", "podcast_audio"] {
        assert!(usage.groups.iter().any(|g| g.feature == feature), "{feature} is in the usage log");
    }
    assert!(usage.groups.iter().find(|g| g.feature == "podcast_audio").unwrap().audio_seconds > 0.0);

    Backend::delete_episode(b, &created.id).unwrap();
    assert!(Backend::episode_prompt(b, &created.id).unwrap().is_none(), "the prompt goes with the episode");
    assert!(!path.exists(), "the audio file goes with the episode");
    assert!(Backend::list_episodes(b, &nb.id).unwrap().is_empty());
}

#[test]
fn a_failed_episode_records_why_and_can_be_retried_with_the_same_choices() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    p.voice.fail(Some("Voice quota exceeded"));
    let nb = b.create_notebook("N", "").unwrap();
    b.create_note(&nb.id, None, "Something to talk about", false).unwrap();
    let e = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, Some(PodcastEffort::Quick), None)).unwrap();
    b.idle();
    let failed = Backend::get_episode(b, &e.id).unwrap();
    assert_eq!(failed.status, EpisodeStatus::Failed);
    assert!(failed.error.as_deref().unwrap().contains("Voice quota exceeded"));

    p.voice.fail(None);
    Backend::retry_episode(b, &e.id).unwrap();
    b.idle();
    let done = Backend::get_episode(b, &e.id).unwrap();
    assert_eq!(done.status, EpisodeStatus::Completed, "{:?}", done.error);
    assert_eq!(done.error, None);
    assert_eq!(done.effort, Some(PodcastEffort::Quick), "the retry keeps the effort");
}

#[test]
fn an_episode_needs_a_chat_model_a_profile_and_something_to_talk_about() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    let nb = b.create_notebook("Empty", "").unwrap();
    let mut wrong = episode(&nb.id, &p.episode_profile, None, None);
    wrong.episode_profile = "episode_profile:missing".into();
    assert_eq!(Backend::create_episode(b, wrong).unwrap_err().to_string(), "Choose an episode profile.");

    let e = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, None, None)).unwrap();
    b.idle();
    let failed = Backend::get_episode(b, &e.id).unwrap();
    assert_eq!(failed.error.as_deref(), Some("This notebook has no processed sources or notes to talk about yet."));

    b.set_settings(Default::default());
    let err = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, None, None)).unwrap_err();
    assert_eq!(err.to_string(), "Choose a chat model in Settings to write episodes.");
}

#[test]
fn a_new_episode_is_checked_against_the_material_it_was_written_from() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    p.env.fake.reply(|r| {
        if r.system == CHECK_SYSTEM {
            json!({ "claims": [{ "line": 1, "claim": "Four tactics", "verdict": "supported", "quote": "The FTC report covers four tactics" }] }).to_string()
        } else {
            script_json()
        }
    });
    let nb = b.create_notebook("N", "").unwrap();
    b.add_source(&nb.id, text("The FTC report covers four tactics.", "FTC report")).unwrap();
    b.idle();
    let created = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, None, None)).unwrap();
    b.idle();

    let check = Backend::get_episode(b, &created.id).unwrap().check.unwrap();
    assert_eq!(check.status, "done", "{:?}", check.error);
    assert_eq!(check.writer.as_deref(), Some("fake-chat"));
    assert_eq!((check.supported, check.sources_used, check.sources_total), (1, 1, 1));
    assert_eq!(check.claims[0].said, "one two three");
    assert_eq!(check.claims[0].source.as_deref(), Some("FTC report"));
    assert!(check.claims[0].quote_found);
    assert_eq!(Backend::list_episodes(b, &nb.id).unwrap()[0].check.as_ref().unwrap().supported, 1);

    // The result is saved in the TypeScript engine's shape, so the Tauri app reads it too.
    let raw: String = b.db.lock().query_row("SELECT result FROM podcast_check WHERE episode_id = ?1", [&created.id], |r| r.get(0)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(saved["writer"], "fake-chat");
    assert_eq!(saved["sourcesUsed"], 1);
    assert_eq!(saved["claims"][0]["quoteFound"], true);

    // A broken check is reported on the episode, which stays playable.
    b.delete_source(&b.list_sources(&nb.id).unwrap()[0].id).unwrap();
    p.env.fake.reply(|r| if r.system == CHECK_SYSTEM { "no json here".into() } else { script_json() });
    Backend::check_episode(b, &created.id).unwrap();
    b.idle();
    let again = Backend::get_episode(b, &created.id).unwrap();
    assert_eq!(again.status, EpisodeStatus::Completed);
    let check = again.check.unwrap();
    assert_eq!(check.status, "failed");
    assert_eq!(check.error.as_deref(), Some("The claim check came back as invalid JSON."));
    assert!(p.env.fake.last().messages[0].content.contains("The FTC report covers four tactics"), "rechecks use the saved material");
}

#[test]
fn a_careful_episode_fixes_flagged_lines_and_checks_again_before_voicing() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    let mut fixed = script();
    fixed.chapters.truncate(1);
    fixed.chapters[0].lines = vec![crate::podcast::script::ScriptLine { host: 0, text: "the report covers four tactics".into() }];
    let fixed = serde_json::to_string(&fixed).unwrap();
    let checks = Arc::new(Mutex::new(0));
    let c = checks.clone();
    p.env.fake.reply(move |r| {
        if r.system == REVISE_SYSTEM {
            return fixed.clone();
        }
        if r.system != CHECK_SYSTEM {
            return script_json();
        }
        let mut n = c.lock().unwrap();
        *n += 1;
        let claim = if *n == 1 {
            json!({ "line": 1, "claim": "Ten tactics", "verdict": "overstated", "quote": "The FTC report covers four tactics", "note": "The report lists four." })
        } else {
            json!({ "line": 1, "claim": "Four tactics", "verdict": "supported", "quote": "The FTC report covers four tactics" })
        };
        json!({ "claims": [claim] }).to_string()
    });
    let nb = b.create_notebook("N", "").unwrap();
    b.add_source(&nb.id, text("The FTC report covers four tactics.", "FTC report")).unwrap();
    b.idle();
    let created = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, Some(PodcastEffort::Careful), None)).unwrap();
    b.idle();

    let e = Backend::get_episode(b, &created.id).unwrap();
    assert_eq!(e.status, EpisodeStatus::Completed, "{:?}", e.error);
    assert_eq!(e.effort, Some(PodcastEffort::Careful));
    assert_eq!(e.transcript.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["the report covers four tactics"], "the revised script is the one voiced");
    let check = e.check.unwrap();
    assert_eq!(check.supported, 1);
    assert_eq!(check.history.iter().map(|h| (h.supported, h.overstated, h.unsupported)).collect::<Vec<_>>(), [(0, 1, 0)]);
    assert_eq!(*checks.lock().unwrap(), 2, "checked before and after the fix, and not again while voicing");
    let revise = p.env.fake.chats.lock().unwrap().iter().find(|c| c.system == REVISE_SYSTEM).cloned().unwrap();
    assert!(revise.messages[0].content.contains("The report lists four."));
}

#[test]
fn a_thorough_episode_writes_from_study_notes_and_a_quick_one_is_not_checked() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    p.env.fake.reply(|r| {
        if r.system == RESEARCH_SYSTEM {
            "FTC report: four tactics. Caveat: no money figures.".into()
        } else if r.system == CHECK_SYSTEM {
            json!({ "claims": [] }).to_string()
        } else {
            script_json()
        }
    });
    let nb = b.create_notebook("N", "").unwrap();
    b.create_note(&nb.id, None, "Something to talk about", false).unwrap();
    let thorough = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, Some(PodcastEffort::Thorough), None)).unwrap();
    b.idle();
    let prompt = Backend::episode_prompt(b, &thorough.id).unwrap().unwrap().prompt;
    let notes = prompt.find("# Research notes").expect("the notes are in the prompt");
    assert!(prompt[notes..].contains("Caveat: no money figures"));
    assert_eq!(Backend::get_episode(b, &thorough.id).unwrap().check.unwrap().status, "done");

    let before = p.env.fake.chats.lock().unwrap().len();
    let quick = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, Some(PodcastEffort::Quick), None)).unwrap();
    b.idle();
    assert_eq!(p.env.fake.chats.lock().unwrap().len() - before, 1, "only the script is written");
    assert!(Backend::get_episode(b, &quick.id).unwrap().check.is_none());
}

#[test]
fn a_gemini_style_dialogue_voice_gives_estimated_timing() {
    let p = podcast_env(VoiceMode::Dialogue);
    let b = &p.env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    b.create_note(&nb.id, None, "Something to talk about", false).unwrap();
    let e = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, Some(PodcastEffort::Quick), None)).unwrap();
    b.idle();
    let e = Backend::get_episode(b, &e.id).unwrap();
    assert_eq!(e.status, EpisodeStatus::Completed, "{:?}", e.error);
    assert!(!e.exact_timing);
    assert_eq!(e.progress, Some((2, 2)), "progress counts dialogue requests");
}

#[test]
fn deleting_a_notebook_deletes_its_episodes_and_their_audio_files() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    b.create_note(&nb.id, None, "Something to talk about", false).unwrap();
    let e = Backend::create_episode(b, episode(&nb.id, &p.episode_profile, Some(PodcastEffort::Quick), None)).unwrap();
    b.idle();
    let EpisodeAudio::File(path) = Backend::episode_audio(b, &e.id).unwrap() else { panic!("a file") };
    assert!(path.exists());
    Backend::delete_notebook(b, &nb.id).unwrap();
    assert!(!path.exists(), "the audio file goes with the notebook");
    assert!(Backend::get_episode(b, &e.id).is_err());
    assert_eq!(super::count(&p.env, "podcast_prompt"), 0);
}

#[test]
fn episodes_written_by_the_tauri_app_read_the_same() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    let audio = p.env.dir.path().join("podcasts");
    std::fs::create_dir_all(&audio).unwrap();
    let file = audio.join("episode_old.m4a");
    std::fs::write(&file, b"m4a").unwrap();
    {
        let conn = b.db.lock();
        conn.execute(
            "INSERT INTO podcast_episode (id, notebook_id, name, format, length, status, progress_done, progress_total, transcript, chapters, exact_timing, duration, audio_ref, voice_label, effort, created, updated)
             VALUES ('episode:old', ?1, 'Old', 'conversation', '5', 'completed', 4, 4, ?2, ?3, 0, 12.5, ?4, 'Marin, Gemini', 'careful', '2026-10-01T10:00:00.000Z', '2026-10-01T10:00:00.000Z')",
            rusqlite::params![
                nb.id,
                r#"[{"speaker":"Marin","text":"Hi","start":0,"end":1.5}]"#,
                r#"[{"title":"Intro","start":0}]"#,
                file.to_string_lossy()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO podcast_check (episode_id, status, model, result, error, created) VALUES ('episode:old', 'done', 'gpt-5', ?1, NULL, '2026-10-01T10:00:00.000Z')",
            [r#"{"writer":"gpt-5","history":[{"supported":1,"overstated":2,"unsupported":0}],"claims":[{"line":1,"said":"Hi","claim":"C","verdict":"overstated","quote":"q","quoteFound":true,"source":"Doc","note":"n"}],"supported":0,"overstated":1,"unsupported":0,"sourcesUsed":0,"sourcesTotal":1}"#],
        )
        .unwrap();
    }
    let e = Backend::get_episode(b, "episode:old").unwrap();
    assert_eq!((e.status, e.progress, e.duration, e.exact_timing), (EpisodeStatus::Completed, Some((4, 4)), Some(12.5), false));
    assert_eq!((e.transcript[0].speaker.as_str(), e.transcript[0].end), ("Marin", Some(1.5)));
    assert_eq!(e.chapters[0].title, "Intro");
    assert_eq!(e.effort, Some(PodcastEffort::Careful));
    let check = e.check.unwrap();
    assert_eq!((check.status.as_str(), check.model.as_str(), check.overstated, check.sources_total), ("done", "gpt-5", 1, 1));
    assert_eq!((check.claims[0].source.as_deref(), check.claims[0].quote_found), (Some("Doc"), true));
    assert_eq!(check.history[0].overstated, 2);
    assert_eq!(Backend::episode_audio(b, "episode:old").unwrap(), EpisodeAudio::File(file.clone()));
    Backend::delete_episode(b, "episode:old").unwrap();
    assert!(!file.exists());
}

#[test]
fn audio_files_outside_the_podcasts_folder_are_never_deleted() {
    let p = podcast_env(VoiceMode::Line);
    let b = &p.env.backend;
    let outside = p.env.dir.path().join("keep.wav");
    std::fs::write(&outside, b"x").unwrap();
    assert!(b.remove_audio(&outside.to_string_lossy()).is_err());
    assert!(outside.exists());
    assert!(b.remove_audio("/nonexistent/episode.wav").is_ok(), "a missing file is already gone");
}
