//! On-device podcasts: script writing, the quote-verified claim check, voices
//! and joining the audio. Port of `packages/engine/src/podcast/`. The episode
//! jobs that use these live in `local/podcasts.rs`.

pub mod audio;
pub mod check;
pub mod live;
pub mod produce;
pub mod profiles;
pub mod script;
pub mod voices;

/// About 280 spoken words per segment, roughly two minutes.
pub const WORDS_PER_SEGMENT: u32 = 280;

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::audio::SAMPLE_RATE;
    use super::script::{PodcastScript, ScriptChapter, ScriptLine};
    use super::voices::{DialogueLine, VoiceKind, VoiceMode, VoiceOption, VoiceProvider};
    use crate::Error;
    use crate::backend::{Cancel, Result};

    /// The script the TypeScript tests use.
    pub fn script() -> PodcastScript {
        let line = |host, text: &str| ScriptLine { host, text: text.into() };
        PodcastScript {
            title: "Dark patterns, explained".into(),
            summary: "Two hosts walk through the FTC report.".into(),
            chapters: vec![
                ScriptChapter { title: "What they are".into(), lines: vec![line(0, "one two three"), line(1, "four five")] },
                ScriptChapter { title: "Why it matters".into(), lines: vec![line(0, "six seven eight nine")] },
            ],
        }
    }

    /// A line voiced as 0.1 s of tone per word, so durations are predictable.
    pub struct ToneVoice {
        mode: VoiceMode,
        calls: Mutex<Vec<String>>,
        fail: Mutex<Option<String>>,
    }

    impl ToneVoice {
        pub fn new(mode: VoiceMode) -> Self {
            ToneVoice { mode, calls: Mutex::default(), fail: Mutex::default() }
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        pub fn fail(&self, message: Option<&str>) {
            *self.fail.lock().unwrap() = message.map(String::from);
        }
        fn tone(words: usize) -> Vec<i16> {
            vec![1000; (words as f64 * 0.1 * SAMPLE_RATE as f64).round() as usize]
        }
        fn check_fail(&self) -> Result<()> {
            match self.fail.lock().unwrap().clone() {
                Some(m) => Err(Error::other(m)),
                None => Ok(()),
            }
        }
    }

    impl VoiceProvider for ToneVoice {
        fn kind(&self) -> VoiceKind {
            if self.mode == VoiceMode::Line { VoiceKind::OpenAi } else { VoiceKind::Gemini }
        }
        fn mode(&self) -> VoiceMode {
            self.mode
        }
        fn concurrency(&self) -> usize {
            3
        }
        fn speak_line(&self, text: &str, voice: &str, _cancel: &Cancel) -> Result<Vec<i16>> {
            self.check_fail()?;
            self.calls.lock().unwrap().push(format!("{voice}:{text}"));
            std::thread::sleep(std::time::Duration::from_millis(text.len() as u64 % 5));
            Ok(Self::tone(text.split(' ').count()))
        }
        fn speak_dialogue(&self, lines: &[DialogueLine], _voices: &[String], _cancel: &Cancel) -> Result<Vec<i16>> {
            self.check_fail()?;
            self.calls.lock().unwrap().push(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("|"));
            Ok(Self::tone(lines.iter().map(|l| l.text.split(' ').count()).sum()))
        }
        fn list_voices(&self) -> Result<Vec<VoiceOption>> {
            Ok(Vec::new())
        }
        fn list_models(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }
}
