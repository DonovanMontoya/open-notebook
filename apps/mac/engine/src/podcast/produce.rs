//! Voices every line and joins them into one track, recording when each line
//! and chapter starts so the player can follow along.
//! Port of `packages/engine/src/podcast/produce.ts`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use super::audio::{SAMPLE_RATE, Timeline};
use super::script::PodcastScript;
use super::voices::{DialogueLine, VoiceMode, VoiceProvider};
use crate::backend::{Cancel, Result};
use crate::Error;

#[derive(Clone, Debug, PartialEq)]
pub struct TimedLine {
    pub speaker: String,
    pub host: usize,
    pub text: String,
    pub chapter: usize,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start: f64,
}

pub struct Production {
    pub pcm: Vec<i16>,
    pub duration: f64,
    pub lines: Vec<TimedLine>,
    pub chapters: Vec<Chapter>,
    /// False when line timing inside a dialogue request was estimated.
    pub exact_timing: bool,
}

const BETWEEN_LINES: f64 = 0.32;
const BETWEEN_CHAPTERS: f64 = 0.9;
const DIALOGUE_CHARS: usize = 2800;

/// Called with (done, total) after each voice request.
pub type OnProgress<'a> = &'a (dyn Fn(usize, usize) + Sync);

/// Runs `job(i)` for every index, at most `concurrency` at once, and returns the
/// results in index order. The first failure stops new requests and is returned.
fn run_limited<T: Send>(count: usize, concurrency: usize, cancel: &Cancel, on_progress: Option<OnProgress<'_>>, job: impl Fn(usize) -> Result<T> + Sync) -> Result<Vec<T>> {
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let done = Mutex::new(0usize);
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
    let error: Mutex<Option<Error>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..concurrency.clamp(1, count.max(1)) {
            scope.spawn(|| {
                loop {
                    if failed.load(Ordering::SeqCst) || cancel.is_cancelled() {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= count {
                        return;
                    }
                    match job(i) {
                        Ok(value) => {
                            results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(value);
                            let mut d = done.lock().unwrap_or_else(|e| e.into_inner());
                            *d += 1;
                            if let Some(f) = on_progress {
                                f(*d, count);
                            }
                        }
                        Err(e) => {
                            failed.store(true, Ordering::SeqCst);
                            error.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = error.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e);
    }
    cancel.check()?;
    // Every slot is filled unless a request was skipped after a cancel.
    results.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().collect::<Option<Vec<T>>>().ok_or(Error::Cancelled)
}

struct Flat<'a> {
    host: usize,
    text: &'a str,
    chapter: usize,
    speaker: String,
}

pub struct ProduceInput<'a> {
    pub script: &'a PodcastScript,
    pub hosts: &'a [String],
    pub voices: &'a [String],
    pub provider: &'a dyn VoiceProvider,
    pub on_progress: Option<OnProgress<'a>>,
    pub cancel: &'a Cancel,
}

pub fn produce(input: ProduceInput<'_>) -> Result<Production> {
    let ProduceInput { script, hosts, voices, provider, on_progress, cancel } = input;
    let first_host = hosts.first().cloned().unwrap_or_default();
    let flat: Vec<Flat<'_>> = script
        .chapters
        .iter()
        .enumerate()
        .flat_map(|(chapter, c)| {
            let first_host = &first_host;
            c.lines.iter().map(move |l| Flat {
                host: l.host,
                text: &l.text,
                chapter,
                speaker: hosts.get(l.host).cloned().unwrap_or_else(|| first_host.clone()),
            })
        })
        .collect();
    let mut timeline = Timeline::new(SAMPLE_RATE);
    let mut lines: Vec<TimedLine> = Vec::new();
    let mut chapters: Vec<Chapter> = Vec::new();

    // Two-speaker dialogue requests can't voice three or four people, so larger casts go line by line.
    if provider.mode() == VoiceMode::Dialogue && hosts.len() <= 2 {
        // Whole stretches of conversation per request; line times are spread by length.
        let mut groups: Vec<Vec<&Flat<'_>>> = Vec::new();
        for c in 0..script.chapters.len() {
            let mut group: Vec<&Flat<'_>> = Vec::new();
            let mut size = 0;
            for line in flat.iter().filter(|l| l.chapter == c) {
                if !group.is_empty() && size + line.text.chars().count() > DIALOGUE_CHARS {
                    groups.push(std::mem::take(&mut group));
                    size = 0;
                }
                group.push(line);
                size += line.text.chars().count();
            }
            if !group.is_empty() {
                groups.push(group);
            }
        }
        let audio = run_limited(groups.len(), provider.concurrency(), cancel, on_progress, |i| {
            let dialogue: Vec<DialogueLine> =
                groups[i].iter().map(|l| DialogueLine { host: l.host, speaker: l.speaker.clone(), text: l.text.to_string() }).collect();
            provider.speak_dialogue(&dialogue, voices, cancel)
        })?;
        let mut last_chapter: Option<usize> = None;
        for (group, pcm) in groups.iter().zip(&audio) {
            if last_chapter != Some(group[0].chapter) {
                if last_chapter.is_some() {
                    timeline.pause(BETWEEN_CHAPTERS);
                }
                last_chapter = Some(group[0].chapter);
                chapters.push(Chapter { title: script.chapters[group[0].chapter].title.clone(), start: timeline.seconds() });
            } else {
                timeline.pause(BETWEEN_LINES);
            }
            let span = timeline.append(pcm);
            let total: usize = group.iter().map(|l| l.text.chars().count()).sum();
            let mut at = span.start;
            for l in group {
                let share = (span.end - span.start) * l.text.chars().count() as f64 / total.max(1) as f64;
                lines.push(TimedLine { speaker: l.speaker.clone(), host: l.host, text: l.text.to_string(), chapter: l.chapter, start: at, end: at + share });
                at += share;
            }
        }
        let duration = timeline.seconds();
        return Ok(Production { pcm: timeline.into_pcm(), duration, lines, chapters, exact_timing: false });
    }

    let audio = run_limited(flat.len(), provider.concurrency(), cancel, on_progress, |i| {
        let line = &flat[i];
        let voice = voices.get(line.host).or(voices.first()).map(String::as_str).unwrap_or_default();
        provider.speak_line(line.text, voice, cancel)
    })?;
    for (i, (line, pcm)) in flat.iter().zip(&audio).enumerate() {
        let previous = i.checked_sub(1).map(|p| &flat[p]);
        if previous.is_none_or(|p| p.chapter != line.chapter) {
            if previous.is_some() {
                timeline.pause(BETWEEN_CHAPTERS);
            }
            chapters.push(Chapter { title: script.chapters[line.chapter].title.clone(), start: timeline.seconds() });
        } else {
            timeline.pause(BETWEEN_LINES);
        }
        let span = timeline.append(pcm);
        lines.push(TimedLine {
            speaker: line.speaker.clone(),
            host: line.host,
            text: line.text.to_string(),
            chapter: line.chapter,
            start: span.start,
            end: span.end,
        });
    }
    let duration = timeline.seconds();
    Ok(Production { pcm: timeline.into_pcm(), duration, lines, chapters, exact_timing: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::podcast::script::{ScriptChapter, ScriptLine};
    use crate::podcast::tests::{ToneVoice, script};

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn line_by_line_production_keeps_script_order_and_exact_timing_despite_parallel_requests() {
        let provider = ToneVoice::new(VoiceMode::Line);
        let progress = Mutex::new(Vec::new());
        let on_progress = |done: usize, _total: usize| progress.lock().unwrap().push(done);
        let script = script();
        let result = produce(ProduceInput {
            script: &script,
            hosts: &names(&["Marin", "Cedar"]),
            voices: &names(&["marin", "cedar"]),
            provider: &provider,
            on_progress: Some(&on_progress),
            cancel: &Cancel::new(),
        })
        .unwrap();
        assert_eq!(*progress.lock().unwrap(), [1, 2, 3]);
        let spoken: Vec<(&str, &str)> = result.lines.iter().map(|l| (l.speaker.as_str(), l.text.as_str())).collect();
        assert_eq!(spoken, [("Marin", "one two three"), ("Cedar", "four five"), ("Marin", "six seven eight nine")]);
        let [a, b, c] = &result.lines[..] else { panic!() };
        assert_eq!(a.start, 0.0);
        assert!((a.end - 0.3).abs() < 1e-9);
        assert!((b.start - (0.3 + 0.32)).abs() < 1e-9, "a short pause between lines");
        assert!((c.start - (b.end + 0.9)).abs() < 1e-9, "a longer pause between chapters");
        assert_eq!(result.chapters.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["What they are", "Why it matters"]);
        assert_eq!(result.chapters[1].start, c.start);
        assert!(result.exact_timing);
        assert!((result.duration - result.pcm.len() as f64 / SAMPLE_RATE as f64).abs() < 1e-9);
        assert!(provider.calls().contains(&"cedar:four five".to_string()), "each host uses their own voice");
    }

    #[test]
    fn dialogue_production_voices_a_chapter_at_once_and_spreads_line_times_by_length() {
        let provider = ToneVoice::new(VoiceMode::Dialogue);
        let script = script();
        let result = produce(ProduceInput {
            script: &script,
            hosts: &names(&["Kore", "Puck"]),
            voices: &names(&["Kore", "Puck"]),
            provider: &provider,
            on_progress: None,
            cancel: &Cancel::new(),
        })
        .unwrap();
        assert!(!result.exact_timing);
        let (a, b) = (&result.lines[0], &result.lines[1]);
        assert_eq!(a.start, 0.0);
        assert!((a.end - b.start).abs() < 1e-9);
        assert!(a.end - a.start > b.end - b.start, "the longer line gets more time");
        assert_eq!(provider.calls().len(), 2, "one request per chapter");
    }

    #[test]
    fn three_speakers_are_voiced_line_by_line_even_with_a_two_voice_dialogue_provider() {
        let provider = ToneVoice::new(VoiceMode::Dialogue);
        let line = |host, text: &str| ScriptLine { host, text: text.into() };
        let script = PodcastScript {
            title: "Panel".into(),
            summary: String::new(),
            chapters: vec![ScriptChapter { title: "One".into(), lines: vec![line(0, "a b"), line(1, "c d"), line(2, "e f")] }],
        };
        let result = produce(ProduceInput {
            script: &script,
            hosts: &names(&["A", "B", "C"]),
            voices: &names(&["v1", "v2", "v3"]),
            provider: &provider,
            on_progress: None,
            cancel: &Cancel::new(),
        })
        .unwrap();
        assert!(result.exact_timing);
        let mut calls = provider.calls();
        calls.sort();
        assert_eq!(calls, ["v1:a b", "v2:c d", "v3:e f"]);
    }

    #[test]
    fn a_failed_line_fails_the_production() {
        let provider = ToneVoice::new(VoiceMode::Line);
        provider.fail(Some("Voice quota exceeded"));
        let script = script();
        let err = produce(ProduceInput {
            script: &script,
            hosts: &names(&["Marin"]),
            voices: &names(&["marin"]),
            provider: &provider,
            on_progress: None,
            cancel: &Cancel::new(),
        })
        .err()
        .unwrap();
        assert_eq!(err.to_string(), "Voice quota exceeded");
    }
}
