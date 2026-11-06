//! Podcasts: a notebook's episodes, the form that starts one, the player, and the episode and
//! speaker profiles episodes are made from.

use notebook_engine::backend::{EpisodeAudio, PodcastMode};
use notebook_engine::types::*;
use rune::{Player, PlayerState, TextEdit};

use crate::state::{App, Route};

pub const EFFORTS: [(PodcastEffort, &str, &str); 4] = [
    (PodcastEffort::Quick, "Quick", "Write the script and voice it."),
    (PodcastEffort::Standard, "Standard", "Check its claims against the sources while it is voiced."),
    (PodcastEffort::Careful, "Careful", "Check, fix flagged lines once and check again before voicing."),
    (PodcastEffort::Thorough, "Thorough", "Study notes on each source first, then up to two rounds of fixes."),
];

pub const RATES: [f32; 5] = [1.0, 1.25, 1.5, 2.0, 0.75];

pub struct NewEpisode {
    pub episode_profile: Option<String>,
    /// None uses the episode profile's own.
    pub speaker_profile: Option<String>,
    pub focus: TextEdit,
    pub name: TextEdit,
    pub effort: PodcastEffort,
    pub open_select: Option<&'static str>,
    pub busy: bool,
    pub error: Option<String>,
}

/// What the Listen tab shows about one episode beyond its card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpisodeOpen {
    pub transcript: bool,
    pub claims: bool,
    pub prompt: Option<EpisodePrompt>,
}

pub struct NowPlaying {
    pub episode: Episode,
    pub player: Player,
    pub rate: usize,
}

impl NowPlaying {
    /// The transcript line being spoken, by line start times.
    pub fn line(&self) -> Option<usize> {
        let t = self.player.time();
        self.episode.transcript.iter().rposition(|l| l.start.is_some_and(|s| s <= t))
    }
}

pub struct SpeakerDraft {
    pub id: String,
    pub name: TextEdit,
    pub description: TextEdit,
    pub voice_kind: String,
    pub voice_model: TextEdit,
    /// name, voice, backstory, personality
    pub speakers: Vec<[TextEdit; 4]>,
}

pub struct EpisodeDraft {
    pub id: String,
    pub name: TextEdit,
    pub description: TextEdit,
    pub speaker_profile_id: String,
    pub briefing: TextEdit,
    pub segments: u32,
}

#[derive(Default)]
pub struct ProfilesPage {
    pub profiles: PodcastProfiles,
    pub speaker: Option<SpeakerDraft>,
    pub episode: Option<EpisodeDraft>,
    pub open_select: Option<&'static str>,
}

impl App {
    pub fn load_episodes(&self) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let id = nb.clone();
        self.run(
            move |b| b.list_episodes(&id),
            move |app, r| match r {
                Ok(list) => {
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == nb) {
                        o.episodes = list;
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn load_profiles(&self) {
        self.run(
            |b| b.podcast_profiles(),
            |app, r| match r {
                Ok(p) => {
                    if let Some(page) = app.profiles.as_mut() {
                        page.profiles = p.clone();
                    }
                    app.podcast_profiles = Some(p);
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn new_episode_form(&mut self) {
        let first = self.podcast_profiles.as_ref().and_then(|p| p.episode.first()).map(|p| p.id.clone());
        if let Some(o) = self.open.as_mut() {
            o.new_episode = Some(NewEpisode {
                episode_profile: first,
                speaker_profile: None,
                focus: TextEdit::new(""),
                name: TextEdit::new(""),
                effort: PodcastEffort::Standard,
                open_select: None,
                busy: false,
                error: None,
            });
        }
        self.load_profiles();
    }

    pub fn create_episode(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(f) = o.new_episode.as_mut() else { return };
        let Some(profile) = f.episode_profile.clone() else {
            f.error = Some("Choose an episode profile.".into());
            return;
        };
        f.busy = true;
        f.error = None;
        let input = CreateEpisode {
            notebook_id: o.id.clone(),
            episode_profile: profile,
            speaker_profile: f.speaker_profile.clone(),
            focus: Some(f.focus.text.trim().to_owned()).filter(|t| !t.is_empty()),
            name: Some(f.name.text.trim().to_owned()).filter(|t| !t.is_empty()),
            effort: Some(f.effort),
        };
        let nb = o.id.clone();
        self.run(
            move |b| b.create_episode(input),
            move |app, r| {
                let Some(o) = app.open.as_mut().filter(|o| o.id == nb) else { return };
                match r {
                    Ok(e) => {
                        o.new_episode = None;
                        o.episodes.insert(0, e);
                    }
                    Err(e) => {
                        if let Some(f) = o.new_episode.as_mut() {
                            f.busy = false;
                            f.error = Some(e.to_string());
                        }
                    }
                }
            },
        );
    }

    pub fn delete_episode(&mut self, id: String) {
        if self.playing.as_ref().is_some_and(|p| p.episode.id == id) {
            self.stop_playing();
        }
        let gone = id.clone();
        self.run(
            move |b| b.delete_episode(&id),
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut() {
                        o.episodes.retain(|e| e.id != gone);
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn retry_episode(&self, id: String) {
        self.run(
            move |b| b.retry_episode(&id),
            |app, r| match r {
                Ok(()) => app.load_episodes(),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn check_episode(&self, id: String) {
        self.run(
            move |b| b.check_episode(&id),
            |app, r| match r {
                Ok(()) => app.load_episodes(),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn toggle_prompt(&mut self, id: String) {
        let Some(o) = self.open.as_mut() else { return };
        let view = o.episode_views.entry(id.clone()).or_default();
        if view.prompt.take().is_some() {
            return;
        }
        let nb = o.id.clone();
        let key = id.clone();
        self.run(
            move |b| b.episode_prompt(&id),
            move |app, r| match r {
                Ok(Some(p)) => {
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == nb) {
                        o.episode_views.entry(key).or_default().prompt = Some(p);
                    }
                }
                Ok(None) => app.toast = Some("This episode's prompt wasn't kept".into()),
                Err(e) => app.fail(e),
            },
        );
    }

    // Playing

    pub fn play(&mut self, episode: Episode) {
        if let Some(p) = self.playing.as_ref().filter(|p| p.episode.id == episode.id) {
            match p.player.state() {
                PlayerState::Playing => p.player.pause(),
                _ => p.player.play(),
            }
            return;
        }
        self.stop_playing();
        let id = episode.id.clone();
        let waker = self.waker();
        self.run(
            move |b| b.episode_audio(&id),
            move |app, r| match r {
                Ok(audio) => {
                    let player = match audio {
                        EpisodeAudio::File(path) => Player::audio(path),
                        EpisodeAudio::Url { url, password } => {
                            let mut request = Player::request(url);
                            if let Some(p) = password {
                                request = request.header("Authorization", format!("Bearer {p}"));
                            }
                            request.audio()
                        }
                    };
                    if let Some(w) = waker {
                        player.set_waker(w);
                    }
                    player.play();
                    app.playing = Some(NowPlaying { episode, player, rate: 0 });
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn stop_playing(&mut self) {
        if let Some(p) = self.playing.take() {
            p.player.pause();
        }
    }

    pub fn skip(&self, seconds: f64) {
        if let Some(p) = &self.playing {
            p.player.seek((p.player.time() + seconds).max(0.0));
        }
    }

    pub fn seek_to(&self, seconds: f64) {
        if let Some(p) = &self.playing {
            p.player.seek(seconds);
            if p.player.state() != PlayerState::Playing {
                p.player.play();
            }
        }
    }

    pub fn next_rate(&mut self) {
        if let Some(p) = self.playing.as_mut() {
            p.rate = (p.rate + 1) % RATES.len();
            p.player.set_rate(RATES[p.rate]);
        }
    }

    // Profiles

    pub fn open_profiles(&mut self) {
        self.profiles.get_or_insert_with(ProfilesPage::default);
        self.route = Route::Profiles;
        self.load_profiles();
    }

    pub fn save_speaker_profile(&mut self) {
        let Some(d) = self.profiles.as_mut().and_then(|p| p.speaker.take()) else { return };
        let p = SpeakerProfile {
            id: d.id,
            name: d.name.text.trim().to_owned(),
            description: d.description.text.trim().to_owned(),
            voice_kind: Some(d.voice_kind),
            voice_model: d.voice_model.text.trim().to_owned(),
            speakers: d
                .speakers
                .iter()
                .filter(|s| !s[0].text.trim().is_empty())
                .map(|s| Speaker {
                    name: s[0].text.trim().into(),
                    voice_id: s[1].text.trim().into(),
                    backstory: s[2].text.trim().into(),
                    personality: s[3].text.trim().into(),
                })
                .collect(),
        };
        self.run(
            move |b| b.save_speaker_profile(p),
            |app, r| match r {
                Ok(_) => app.load_profiles(),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn save_episode_profile(&mut self) {
        let Some(d) = self.profiles.as_mut().and_then(|p| p.episode.take()) else { return };
        let p = EpisodeProfile {
            id: d.id,
            name: d.name.text.trim().to_owned(),
            description: d.description.text.trim().to_owned(),
            speaker_profile_id: d.speaker_profile_id,
            language: None,
            briefing: d.briefing.text.clone(),
            segments: d.segments,
        };
        self.run(
            move |b| b.save_episode_profile(p),
            |app, r| match r {
                Ok(_) => app.load_profiles(),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn delete_profile(&mut self, speaker: bool, id: String) {
        self.run(
            move |b| if speaker { b.delete_speaker_profile(&id) } else { b.delete_episode_profile(&id) },
            |app, r| match r {
                Ok(()) => app.load_profiles(),
                Err(e) => app.fail(e),
            },
        );
    }

    /// Whether this backend writes episodes on device (its profiles can be edited here).
    pub fn local_podcasts(&self) -> bool {
        self.backend.as_ref().is_some_and(|b| b.podcast_mode() == PodcastMode::Local)
    }
}

impl SpeakerDraft {
    pub fn from(p: &SpeakerProfile) -> Self {
        SpeakerDraft {
            id: p.id.clone(),
            name: TextEdit::new(p.name.clone()),
            description: TextEdit::new(p.description.clone()),
            voice_kind: p.voice_kind.clone().unwrap_or_else(|| "openai".into()),
            voice_model: TextEdit::new(p.voice_model.clone()),
            speakers: p
                .speakers
                .iter()
                .map(|s| {
                    [TextEdit::new(s.name.clone()), TextEdit::new(s.voice_id.clone()), TextEdit::new(s.backstory.clone()), TextEdit::new(s.personality.clone())]
                })
                .collect(),
        }
    }
}

impl EpisodeDraft {
    pub fn from(p: &EpisodeProfile) -> Self {
        EpisodeDraft {
            id: p.id.clone(),
            name: TextEdit::new(p.name.clone()),
            description: TextEdit::new(p.description.clone()),
            speaker_profile_id: p.speaker_profile_id.clone(),
            briefing: TextEdit::new(p.briefing.clone()),
            segments: p.segments.max(1),
        }
    }
}

/// "12:05" or "1:02:05".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clock_reads_like_a_player() {
        assert_eq!(super::clock(0.0), "0:00");
        assert_eq!(super::clock(65.4), "1:05");
        assert_eq!(super::clock(3725.0), "1:02:05");
    }
}
