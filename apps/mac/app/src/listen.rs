//! Podcasts: a notebook's episodes, the form that starts one, the player, and the episode and
//! speaker profiles episodes are made from.

use notebook_engine::backend::{EpisodeAudio, PodcastMode};
use notebook_engine::podcast::WORDS_PER_SEGMENT;
use notebook_engine::podcast::voices::{VoiceKind, VoiceOption};
use notebook_engine::types::*;
use rune::{Player, PlayerState, TextEdit};

use crate::state::{App, Route};

pub const EFFORTS: [(PodcastEffort, &str, &str); 4] = [
    (PodcastEffort::Quick, "Quick", "Write the script and voice it."),
    (PodcastEffort::Standard, "Standard", "Check its claims against the sources while it is voiced."),
    (PodcastEffort::Careful, "Careful", "Check, fix flagged lines once and check again before voicing."),
    (PodcastEffort::Thorough, "Thorough", "Study notes on each source first, then up to two rounds of fixes."),
];

/// Languages an episode can be written in: (code, name). An empty code follows the sources.
pub const LANGUAGES: [(&str, &str); 16] = [
    ("", "Same as the sources"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("ru", "Russian"),
    ("ja", "Japanese"),
    ("zh", "Chinese"),
    ("hi", "Hindi"),
    ("bn", "Bengali"),
    ("ca", "Catalan"),
];

/// The name of a language code; an unknown code shows as itself.
pub fn language_name(code: Option<&str>) -> String {
    match code.filter(|c| !c.is_empty()) {
        None => LANGUAGES[0].1.into(),
        Some(c) => LANGUAGES.iter().find(|(k, _)| *k == c).map_or_else(|| c.to_owned(), |(_, n)| (*n).to_owned()),
    }
}

pub const RATES: [f32; 8] = [0.8, 1.0, 1.1, 1.2, 1.35, 1.5, 1.75, 2.0];
pub const SKIP_BACK: f64 = 15.0;
pub const SKIP_FORWARD: f64 = 30.0;

const RATE_KEY: &str = "listen.rate";
const EFFORT_KEY: &str = "listen.effort";
fn position_key(id: &str) -> String {
    format!("listen.position.{id}")
}

/// The line a voice preview speaks.
pub const SAMPLE_LINE: &str =
    "Here is the short version: the report found four tricks that keep people subscribed, and the cancel button is the worst of them.";

/// Spoken minutes for a number of segments, at 150 words a minute.
pub fn episode_minutes(segments: u32) -> u32 {
    ((segments * WORDS_PER_SEGMENT) as f64 / 150.0).round() as u32
}

/// When playback stops by itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sleep {
    Off,
    /// When the chapter playing now ends.
    Chapter,
    /// After some minutes; `ends_at` (app clock) is set on the next poll.
    Minutes {
        minutes: u32,
        ends_at: Option<f64>,
    },
}

pub const SLEEPS: [(Sleep, &str); 5] = [
    (Sleep::Off, "Off"),
    (Sleep::Chapter, "Chapter"),
    (Sleep::Minutes { minutes: 15, ends_at: None }, "15 min"),
    (Sleep::Minutes { minutes: 30, ends_at: None }, "30 min"),
    (Sleep::Minutes { minutes: 60, ends_at: None }, "1 hour"),
];

impl Sleep {
    /// Which of `SLEEPS` this is.
    pub fn index(self) -> usize {
        match self {
            Sleep::Off => 0,
            Sleep::Chapter => 1,
            Sleep::Minutes { minutes, .. } => SLEEPS.iter().position(|(s, _)| matches!(s, Sleep::Minutes { minutes: m, .. } if *m == minutes)).unwrap_or(0),
        }
    }

    /// "Sleep", "End of chapter" or "12 min" left.
    pub fn label(self, now: f64) -> String {
        match self {
            Sleep::Off => "Sleep".into(),
            Sleep::Chapter => "End of chapter".into(),
            Sleep::Minutes { minutes, ends_at } => {
                let left = ends_at.map_or(minutes as f64 * 60.0, |e| e - now);
                format!("{} min", (left / 60.0).ceil().max(1.0) as u32)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayerTab {
    #[default]
    Transcript,
    Chapters,
}

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
    /// The claims that match the sources, folded away by default.
    pub matched: bool,
    pub prompt: Option<PromptView>,
}

/// The prompt dialog: loading, missing (older episodes kept none) or the prompt itself.
#[derive(Clone, Debug, PartialEq)]
pub enum PromptView {
    Loading,
    Missing,
    Shown { prompt: EpisodePrompt, copied: bool },
}

pub struct NowPlaying {
    pub episode: Episode,
    pub player: Player,
    /// Index into `RATES`.
    pub rate: usize,
    /// The notebook it came from, for the system's Now Playing.
    pub notebook: Option<String>,
    /// The full player instead of the bar.
    pub expanded: bool,
    pub tab: PlayerTab,
    /// The transcript scrolls to the line being spoken until you scroll it yourself.
    pub following: bool,
    pub sleep: Sleep,
    /// Where the track is being dragged to, before the seek.
    pub scrub: Option<f64>,
    /// App clock as of the last poll.
    pub now: f64,
    /// When the position was last remembered (app clock).
    pub saved_at: f64,
    /// The chapter at the last poll, so the sleep timer sees one end.
    pub chapter: Option<usize>,
    /// The line the transcript last scrolled to.
    pub followed: Option<usize>,
}

impl NowPlaying {
    pub fn new(episode: Episode, player: Player, rate: usize) -> Self {
        NowPlaying {
            episode,
            player,
            rate,
            notebook: None,
            expanded: false,
            tab: PlayerTab::Transcript,
            following: true,
            sleep: Sleep::Off,
            scrub: None,
            now: 0.0,
            saved_at: 0.0,
            chapter: None,
            followed: None,
        }
    }

    pub fn duration(&self) -> f64 {
        self.player.duration().or(self.episode.duration).unwrap_or(0.0)
    }

    pub fn is_playing(&self) -> bool {
        self.player.state() == PlayerState::Playing
    }

    /// The transcript line being spoken.
    pub fn line(&self) -> Option<usize> {
        line_at(&line_times(&self.episode, self.duration()), self.player.time())
    }

    /// The chapter playing now.
    pub fn chapter(&self) -> Option<usize> {
        chapter_at(&self.episode.chapters, self.player.time())
    }
}

/// The chapter playing at `t`; None before the first.
pub fn chapter_at(chapters: &[EpisodeChapter], t: f64) -> Option<usize> {
    chapters.iter().rposition(|c| c.start <= t + 0.05)
}

/// Where "previous chapter" goes: the start of this chapter when more than 3 s into it, as a CD
/// player does, otherwise the start of the one before.
pub fn previous_chapter_start(chapters: &[EpisodeChapter], t: f64) -> f64 {
    match chapter_at(chapters, t) {
        Some(i) if t - chapters[i].start > 3.0 => chapters[i].start,
        Some(i) => chapters[i.saturating_sub(1)].start,
        None => 0.0,
    }
}

pub fn next_chapter_start(chapters: &[EpisodeChapter], t: f64) -> Option<f64> {
    let next = chapter_at(chapters, t).map_or(0, |i| i + 1);
    chapters.get(next).map(|c| c.start)
}

/// The line spoken at `t`, from `line_times`.
pub fn line_at(times: &[(f64, f64)], t: f64) -> Option<usize> {
    times.iter().position(|&(s, e)| t >= s && t < e + 0.3)
}

/// Start and end of each transcript line. Measured starts are used as they are; episodes with
/// none (server episodes) get times spread by text length over the duration.
pub fn line_times(e: &Episode, duration: f64) -> Vec<(f64, f64)> {
    let lines = &e.transcript;
    if !lines.is_empty() && lines.iter().all(|l| l.start.is_some()) {
        return lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let start = l.start.unwrap_or_default();
                let end = l.end.or_else(|| lines.get(i + 1).and_then(|n| n.start)).unwrap_or(duration.max(start));
                (start, end)
            })
            .collect();
    }
    let total = lines.iter().map(|l| l.text.chars().count()).sum::<usize>().max(1) as f64;
    let mut at = 0.0;
    lines
        .iter()
        .map(|l| {
            let span = duration * l.text.chars().count() as f64 / total;
            let t = (at, at + span);
            at += span;
            t
        })
        .collect()
}

/// A plain verdict from the share of claims that match a passage in the sources.
pub fn check_verdict(c: &EpisodeCheck) -> &'static str {
    let total = c.supported + c.overstated + c.unsupported;
    if total == 0 {
        return "No factual claims to check";
    }
    let share = c.supported as f64 / total as f64;
    if share >= 0.9 {
        "Closely follows your sources"
    } else if share >= 0.75 {
        "Mostly follows your sources"
    } else {
        "Check before relying on it"
    }
}

/// "Written by A, checked by B" or "Written and checked by B".
pub fn check_models(c: &EpisodeCheck) -> String {
    match c.writer.as_deref().filter(|w| !w.is_empty() && *w != c.model) {
        Some(w) => format!("Written by {w}, checked by {}", c.model),
        None => format!("Written and checked by {}", c.model),
    }
}

/// "Before recording, 2 rounds of fixes took the flagged claims from 7 to 3", or None.
pub fn check_history(c: &EpisodeCheck) -> Option<String> {
    let first = c.history.first()?;
    let rounds = if c.history.len() == 1 { "one round".to_owned() } else { format!("{} rounds", c.history.len()) };
    Some(format!(
        "Before recording, {rounds} of fixes took the flagged claims from {} to {}",
        first.overstated + first.unsupported,
        c.overstated + c.unsupported
    ))
}

/// The episode's name as a file name.
pub fn file_name(name: &str, ext: &str) -> String {
    let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{}.{ext}", if clean.is_empty() { "Episode" } else { &clean })
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
    /// None writes in the sources' language.
    pub language: Option<String>,
}

#[derive(Default)]
pub struct ProfilesPage {
    pub profiles: PodcastProfiles,
    pub speaker: Option<SpeakerDraft>,
    pub episode: Option<EpisodeDraft>,
    pub open_select: Option<&'static str>,
    /// The speaker whose voice list is open.
    pub open_voice: Option<usize>,
    /// The voice provider's voices for the draft's provider: None while loading.
    pub voices: Option<Result<Vec<VoiceOption>, String>>,
    /// Voice models the provider offers, as suggestions.
    pub voice_models: Vec<String>,
    /// The server's text to speech models (server mode).
    pub server_models: Vec<ServerModel>,
    /// The speaker whose voice is being previewed.
    pub previewing: Option<usize>,
    pub preview_error: Option<(usize, String)>,
    /// Kept alive while it plays.
    pub preview: Option<Player>,
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
                effort: self.prefs.get(EFFORT_KEY).unwrap_or(PodcastEffort::Standard),
                open_select: None,
                busy: false,
                error: None,
            });
        }
        self.load_profiles();
    }

    pub fn create_episode(&mut self) {
        let local = self.local_podcasts();
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
            effort: local.then_some(f.effort),
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

    pub fn set_effort(&mut self, effort: PodcastEffort) {
        if let Some(f) = self.open.as_mut().and_then(|o| o.new_episode.as_mut()) {
            f.effort = effort;
        }
        let _ = self.prefs.set(EFFORT_KEY, &effort);
    }

    /// On device, episodes need a chat model to write the script.
    pub fn needs_chat_model(&self) -> bool {
        self.local_podcasts() && self.settings.chat.as_ref().is_none_or(|c| c.model.trim().is_empty())
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

    /// Opens the prompt dialog for an episode and loads its prompt.
    pub fn show_prompt(&mut self, id: String) {
        let Some(o) = self.open.as_mut() else { return };
        o.episode_views.entry(id.clone()).or_default().prompt = Some(PromptView::Loading);
        let nb = o.id.clone();
        let key = id.clone();
        self.run(
            move |b| b.episode_prompt(&id),
            move |app, r| {
                let Some(view) = app.open.as_mut().filter(|o| o.id == nb).and_then(|o| o.episode_views.get_mut(&key)).filter(|v| v.prompt.is_some()) else {
                    return;
                };
                match r {
                    Ok(Some(prompt)) => view.prompt = Some(PromptView::Shown { prompt, copied: false }),
                    Ok(None) => view.prompt = Some(PromptView::Missing),
                    Err(e) => {
                        view.prompt = None;
                        app.fail(e);
                    }
                }
            },
        );
    }

    pub fn close_prompt(&mut self) {
        if let Some(o) = self.open.as_mut() {
            for v in o.episode_views.values_mut() {
                v.prompt = None;
            }
        }
    }

    // Playing

    /// Plays an episode, or pauses or resumes it when it is the one loaded.
    pub fn play(&mut self, episode: Episode) {
        self.start_episode(episode, false);
    }

    /// Plays an episode and opens the full player.
    pub fn open_player(&mut self, episode: Episode) {
        if let Some(p) = self.playing.as_mut().filter(|p| p.episode.id == episode.id) {
            p.expanded = true;
            if !p.is_playing() {
                p.player.play();
            }
            return;
        }
        self.start_episode(episode, true);
    }

    fn start_episode(&mut self, episode: Episode, expanded: bool) {
        if self.playing.as_ref().is_some_and(|p| p.episode.id == episode.id) {
            self.toggle_play();
            return;
        }
        self.stop_playing();
        let id = episode.id.clone();
        let waker = self.waker();
        let notebook = self.open.as_ref().and_then(|o| o.notebook.as_ref()).map(|n| n.name.clone());
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
                    let rate = app.saved_rate();
                    player.set_rate(RATES[rate]);
                    if let Some(at) = app.prefs.get::<f64>(&position_key(&episode.id)).filter(|t| *t > 0.0) {
                        player.seek(at);
                    }
                    player.play();
                    let mut p = NowPlaying::new(episode, player, rate);
                    p.notebook = notebook;
                    p.expanded = expanded;
                    app.playing = Some(p);
                    crate::nowplaying::install(app.inbox());
                }
                Err(e) => app.fail(e),
            },
        );
    }

    /// Play or pause the loaded episode.
    pub fn toggle_play(&mut self) {
        let Some(p) = &self.playing else { return };
        if p.is_playing() {
            p.player.pause();
            self.save_position();
        } else {
            p.player.play();
        }
    }

    pub fn pause_playing(&mut self) {
        if let Some(p) = self.playing.as_ref().filter(|p| p.is_playing()) {
            p.player.pause();
            self.save_position();
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn resume_playing(&self) {
        if let Some(p) = self.playing.as_ref().filter(|p| !p.is_playing()) {
            p.player.play();
        }
    }

    pub fn stop_playing(&mut self) {
        self.save_position();
        if let Some(p) = self.playing.take() {
            p.player.pause();
        }
        crate::nowplaying::clear();
    }

    pub fn skip(&self, seconds: f64) {
        if let Some(p) = &self.playing {
            p.player.seek((p.player.time() + seconds).max(0.0));
        }
    }

    /// Seek, and play if paused (chapter and transcript rows).
    pub fn seek_to(&mut self, seconds: f64) {
        if let Some(p) = self.playing.as_mut() {
            p.player.seek(seconds);
            p.following = true;
            if p.player.state() != PlayerState::Playing {
                p.player.play();
            }
        }
    }

    /// Seek without changing play or pause (the track, media keys).
    pub fn seek(&self, seconds: f64) {
        if let Some(p) = &self.playing {
            p.player.seek(seconds.clamp(0.0, p.duration().max(0.0)));
        }
    }

    pub fn previous_chapter(&self) {
        if let Some(p) = &self.playing {
            p.player.seek(previous_chapter_start(&p.episode.chapters, p.player.time()));
        }
    }

    pub fn next_chapter(&self) {
        if let Some(p) = &self.playing
            && let Some(t) = next_chapter_start(&p.episode.chapters, p.player.time())
        {
            p.player.seek(t);
        }
    }

    /// The remembered rate's index in `RATES` (1×, the second, when none).
    fn saved_rate(&self) -> usize {
        self.prefs.get::<f32>(RATE_KEY).and_then(|r| RATES.iter().position(|x| (x - r).abs() < 0.01)).unwrap_or(1)
    }

    pub fn set_rate(&mut self, index: usize) {
        let index = index.min(RATES.len() - 1);
        if let Some(p) = self.playing.as_mut() {
            p.rate = index;
            p.player.set_rate(RATES[index]);
        }
        let _ = self.prefs.set(RATE_KEY, &RATES[index]);
    }

    /// The bar's rate button: the next rate, round to the slowest after the fastest.
    pub fn next_rate(&mut self) {
        if let Some(i) = self.playing.as_ref().map(|p| (p.rate + 1) % RATES.len()) {
            self.set_rate(i);
        }
    }

    /// One rate faster or slower (`>` and `<`), stopping at the ends.
    pub fn step_rate(&mut self, faster: bool) {
        if let Some(i) = self.playing.as_ref().map(|p| if faster { p.rate + 1 } else { p.rate.saturating_sub(1) }) {
            self.set_rate(i);
        }
    }

    pub fn set_sleep(&mut self, sleep: Sleep) {
        if let Some(p) = self.playing.as_mut() {
            p.sleep = sleep;
            p.chapter = p.chapter();
        }
    }

    /// Remembers where the episode was left; forgets it near the end.
    fn save_position(&mut self) {
        let Some(p) = &self.playing else { return };
        let (t, d, key) = (p.player.time(), p.duration(), position_key(&p.episode.id));
        if p.player.state() == PlayerState::Ended || (d > 0.0 && t > d - 10.0) {
            let _ = self.prefs.remove(&key);
        } else if t >= 1.0 {
            let _ = self.prefs.set(&key, &t.floor());
        }
    }

    /// Runs from `poll` while an episode is loaded: remembers the position every 5 s, runs the
    /// sleep timer and keeps the system's Now Playing current. True when something changed.
    pub fn tick_player(&mut self, now: f64) -> bool {
        let Some(p) = self.playing.as_mut() else { return false };
        p.now = now;
        let mut stop = false;
        match &mut p.sleep {
            Sleep::Minutes { ends_at: at @ None, minutes } => *at = Some(now + *minutes as f64 * 60.0),
            Sleep::Minutes { ends_at: Some(at), .. } => stop = now >= *at,
            Sleep::Chapter => {
                let c = p.chapter();
                stop = p.chapter.is_some() && c != p.chapter;
                p.chapter = c;
            }
            Sleep::Off => {}
        }
        let mut changed = false;
        if stop {
            p.sleep = Sleep::Off;
            self.pause_playing();
            changed = true;
        }
        let Some(p) = self.playing.as_mut() else { return changed };
        if p.player.state() == PlayerState::Ended {
            let _ = self.prefs.remove(&position_key(&p.episode.id));
        } else if p.is_playing() && now - p.saved_at >= 5.0 {
            p.saved_at = now;
            self.save_position();
        }
        if let Some(p) = &self.playing {
            crate::nowplaying::update(p);
        }
        changed
    }

    /// The transcript line the full player should scroll to, once per line while following.
    pub fn follow_line(&mut self) -> Option<String> {
        let p = self.playing.as_mut().filter(|p| p.expanded && p.following && p.tab == PlayerTab::Transcript)?;
        let line = p.line()?;
        if p.followed == Some(line) {
            return None;
        }
        p.followed = Some(line);
        Some(format!("np-line-{line}"))
    }

    /// Asks where to save an episode's audio, then copies or downloads it there.
    pub fn download_episode(&mut self, e: &Episode, tx: &mut rune::Tx) {
        let ext = if self.local_podcasts() { "wav" } else { "mp3" };
        let options = rune::FileDialogOptions { title: "Save episode audio".into(), file_name: Some(file_name(&e.name, ext)), ..Default::default() };
        let id = e.id.clone();
        tx.save_file(options, move |s: &mut App, result, _| match result {
            Ok(Some(path)) => s.save_episode_audio(id, path),
            Ok(None) => {}
            Err(e) => s.fail(e),
        });
    }

    fn save_episode_audio(&self, id: String, path: std::path::PathBuf) {
        self.run(
            move |b| {
                let written = match b.episode_audio(&id)? {
                    EpisodeAudio::File(from) => {
                        std::fs::copy(&from, &path).map_err(|e| notebook_engine::Error::other(e.to_string()))?;
                        path
                    }
                    EpisodeAudio::Url { url, password } => download(&url, password.as_deref(), path)?,
                };
                Ok(written.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            },
            |app, r| match r {
                Ok(name) => app.toast = Some(format!("Saved {name}")),
                Err(e) => app.fail(e),
            },
        );
    }

    // Profiles

    pub fn open_profiles(&mut self) {
        self.profiles.get_or_insert_with(ProfilesPage::default);
        self.route = Route::Profiles;
        self.load_profiles();
    }

    /// Opens a speaker profile in the editor and loads what its pickers offer.
    pub fn edit_speaker(&mut self, d: SpeakerDraft) {
        let Some(page) = self.profiles.as_mut() else { return };
        page.speaker = Some(d);
        page.preview_error = None;
        page.open_voice = None;
        if self.local_podcasts() {
            self.load_voices();
        } else {
            self.load_server_models();
        }
    }

    /// A new speaker profile: on device with OpenAI speech and one speaker, on a server with
    /// the server's default voice model.
    pub fn new_speaker(&mut self) {
        let local = self.local_podcasts();
        let mut d = SpeakerDraft::from(&SpeakerProfile {
            voice_kind: local.then(|| VoiceKind::OpenAi.as_str().into()),
            voice_model: if local { VoiceKind::OpenAi.default_model().into() } else { String::new() },
            ..Default::default()
        });
        d.speakers.push([TextEdit::new(""), TextEdit::new(if local { "marin" } else { "" }), TextEdit::new(""), TextEdit::new("")]);
        self.edit_speaker(d);
    }

    /// Switches the draft's voice provider: its default model, and no voices, since voice
    /// names differ between providers.
    pub fn set_voice_kind(&mut self, kind: VoiceKind) {
        let Some(d) = self.profiles.as_mut().and_then(|p| p.speaker.as_mut()) else { return };
        if d.voice_kind == kind.as_str() {
            return;
        }
        d.voice_kind = kind.as_str().into();
        d.voice_model = TextEdit::new(kind.default_model());
        for sp in &mut d.speakers {
            sp[1] = TextEdit::new("");
        }
        self.load_voices();
    }

    /// The voice provider for the draft's provider and model, with the saved key.
    fn draft_voice_provider(&self) -> Option<std::sync::Arc<dyn notebook_engine::podcast::voices::VoiceProvider>> {
        use notebook_engine::podcast::voices::{VoiceConfig, create_voice_provider};
        let d = self.profiles.as_ref()?.speaker.as_ref()?;
        let kind = VoiceKind::parse(&d.voice_kind).unwrap_or(VoiceKind::OpenAi);
        let model = d.voice_model.text.trim();
        let config = VoiceConfig {
            kind,
            model: if model.is_empty() { kind.default_model().into() } else { model.into() },
            api_key: self.secrets.get(&notebook_engine::settings::provider_secret(kind.key_name())).ok().flatten(),
            voices: Vec::new(),
            hosts: Vec::new(),
        };
        Some(create_voice_provider(&config, notebook_engine::net::UreqHttp::shared(), Some(notebook_engine::podcast::live::websocket())))
    }

    /// Loads the provider's voices and models for the speaker editor (on device).
    pub fn load_voices(&mut self) {
        let Some(provider) = self.draft_voice_provider() else { return };
        let Some(page) = self.profiles.as_mut() else { return };
        page.voices = None;
        page.voice_models.clear();
        let kind = provider.kind();
        let inbox = self.inbox();
        std::thread::spawn(move || {
            let voices = provider.list_voices().map_err(|e| e.to_string());
            let models = provider.list_models().unwrap_or_default();
            inbox.send(move |app| {
                let Some(page) = app.profiles.as_mut() else { return };
                if page.speaker.as_ref().is_some_and(|d| d.voice_kind == kind.as_str()) {
                    page.voices = Some(voices);
                    page.voice_models = models;
                }
            });
        });
    }

    /// The server's text to speech models, for the server's speaker profiles.
    pub fn load_server_models(&self) {
        self.run(
            |b| b.list_models(),
            |app, r| match r {
                Ok(models) => {
                    if let Some(page) = app.profiles.as_mut() {
                        page.server_models = models.into_iter().filter(|m| m.kind == "text_to_speech").collect();
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    /// Speaks the sample line in speaker `i`'s voice.
    pub fn preview_voice(&mut self, i: usize) {
        let Some(provider) = self.draft_voice_provider() else { return };
        let Some(page) = self.profiles.as_mut() else { return };
        let Some(voice) = page.speaker.as_ref().and_then(|d| d.speakers.get(i)).map(|s| s[1].text.trim().to_owned()).filter(|v| !v.is_empty()) else {
            return;
        };
        page.previewing = Some(i);
        page.preview_error = None;
        if let Some(p) = page.preview.take() {
            p.pause();
        }
        let inbox = self.inbox();
        let waker = self.waker();
        std::thread::spawn(move || {
            use notebook_engine::podcast::audio::{SAMPLE_RATE, encode_wav};
            let result = provider.speak_line(SAMPLE_LINE, &voice, &notebook_engine::Cancel::new()).map_err(|e| e.to_string()).and_then(|pcm| {
                let path = std::env::temp_dir().join(format!("open-notebook-voice-{}.wav", std::process::id()));
                std::fs::write(&path, encode_wav(&pcm, SAMPLE_RATE)).map(|_| path).map_err(|e| e.to_string())
            });
            inbox.send(move |app| {
                let Some(page) = app.profiles.as_mut() else { return };
                page.previewing = None;
                match result {
                    Ok(path) => {
                        let player = Player::audio(path);
                        if let Some(w) = waker {
                            player.set_waker(w);
                        }
                        player.play();
                        page.preview = Some(player);
                    }
                    Err(e) => page.preview_error = Some((i, e)),
                }
            });
        });
    }

    pub fn save_speaker_profile(&mut self) {
        let local = self.local_podcasts();
        let Some(d) = self.profiles.as_mut().and_then(|p| p.speaker.take()) else { return };
        if let Some(p) = self.profiles.as_mut().and_then(|p| p.preview.take()) {
            p.pause();
        }
        let p = SpeakerProfile {
            id: d.id,
            name: d.name.text.trim().to_owned(),
            description: d.description.text.trim().to_owned(),
            voice_kind: local.then_some(d.voice_kind),
            voice_model: d.voice_model.text.trim().to_owned(),
            speakers: d
                .speakers
                .iter()
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
            language: d.language,
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
    /// With no backend (scenes), the settings' mode says.
    pub fn local_podcasts(&self) -> bool {
        match &self.backend {
            Some(b) => b.podcast_mode() == PodcastMode::Local,
            None => self.settings.mode == notebook_engine::settings::Mode::Local,
        }
    }
}

impl SpeakerDraft {
    /// Saving needs a name and at least one speaker, each with a name and a voice.
    pub fn can_save(&self) -> bool {
        !self.name.text.trim().is_empty()
            && !self.speakers.is_empty()
            && self.speakers.iter().all(|s| !s[0].text.trim().is_empty() && !s[1].text.trim().is_empty())
    }

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
    /// Saving needs a name, a briefing and a speaker profile.
    pub fn can_save(&self) -> bool {
        !self.name.text.trim().is_empty() && !self.briefing.text.trim().is_empty() && !self.speaker_profile_id.is_empty()
    }

    pub fn from(p: &EpisodeProfile) -> Self {
        EpisodeDraft {
            id: p.id.clone(),
            name: TextEdit::new(p.name.clone()),
            description: TextEdit::new(p.description.clone()),
            speaker_profile_id: p.speaker_profile_id.clone(),
            briefing: TextEdit::new(p.briefing.clone()),
            segments: p.segments.max(1),
            language: p.language.clone().filter(|l| !l.is_empty()),
        }
    }
}

/// Downloads server audio to `path`, fixing the extension when the server sent another format.
fn download(url: &str, password: Option<&str>, path: std::path::PathBuf) -> notebook_engine::Result<std::path::PathBuf> {
    use notebook_engine::net::{Http, Request, UreqHttp};
    let mut request = Request::get(url);
    if let Some(p) = password {
        request = request.header("Authorization", format!("Bearer {p}"));
    }
    let fail = |e: String| notebook_engine::Error::other(format!("Could not download the audio: {e}"));
    let mut response = UreqHttp::new().send(request).map_err(|e| fail(format!("{e:?}")))?;
    if response.status >= 400 {
        return Err(fail(format!("the server answered {}", response.status)));
    }
    let kind = response.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.to_ascii_lowercase()).unwrap_or_default();
    let ext = if kind.contains("wav") {
        "wav"
    } else if kind.contains("mp4") || kind.contains("m4a") || kind.contains("aac") {
        "m4a"
    } else {
        "mp3"
    };
    let path = if path.extension().is_some_and(|x| x == "mp3") && ext != "mp3" { path.with_extension(ext) } else { path };
    let mut file = std::fs::File::create(&path).map_err(|e| fail(e.to_string()))?;
    std::io::copy(&mut response.body, &mut file).map_err(|e| fail(e.to_string()))?;
    Ok(path)
}

/// "12:05" or "1:02:05".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

#[cfg(test)]
mod tests {
    use notebook_engine::types::*;

    #[test]
    fn clock_reads_like_a_player() {
        assert_eq!(super::clock(0.0), "0:00");
        assert_eq!(super::clock(65.4), "1:05");
        assert_eq!(super::clock(3725.0), "1:02:05");
    }

    #[test]
    fn editing_an_episode_profile_keeps_its_language() {
        use notebook_engine::types::EpisodeProfile;
        let p = EpisodeProfile { id: "ep1".into(), language: Some("de".into()), segments: 4, ..Default::default() };
        let d = super::EpisodeDraft::from(&p);
        assert_eq!(d.language.as_deref(), Some("de"));
        assert_eq!(super::language_name(d.language.as_deref()), "German");
        assert_eq!(super::language_name(None), "Same as the sources");
        assert_eq!(super::language_name(Some("eo")), "eo");
    }

    fn ch(title: &str, start: f64) -> notebook_engine::types::EpisodeChapter {
        notebook_engine::types::EpisodeChapter { title: title.into(), start }
    }

    #[test]
    fn chapters_move_like_a_cd_player() {
        let c = [ch("a", 0.0), ch("b", 100.0), ch("c", 200.0)];
        assert_eq!(super::chapter_at(&c, 150.0), Some(1));
        assert_eq!(super::chapter_at(&[ch("a", 5.0)], 1.0), None);
        // More than 3 s in: back to this chapter's start; at its start: the one before.
        assert_eq!(super::previous_chapter_start(&c, 150.0), 100.0);
        assert_eq!(super::previous_chapter_start(&c, 101.0), 0.0);
        assert_eq!(super::previous_chapter_start(&c, 1.0), 0.0);
        assert_eq!(super::next_chapter_start(&c, 150.0), Some(200.0));
        assert_eq!(super::next_chapter_start(&c, 250.0), None);
    }

    fn episode(lines: &[(&str, Option<f64>)]) -> Episode {
        Episode {
            id: "e".into(),
            notebook_id: None,
            name: "E".into(),
            summary: None,
            status: EpisodeStatus::Completed,
            progress: None,
            error: None,
            created: String::new(),
            duration: Some(100.0),
            chapters: Vec::new(),
            transcript: lines.iter().map(|(t, s)| EpisodeLine { speaker: "A".into(), text: (*t).into(), start: *s, end: None }).collect(),
            exact_timing: false,
            voice_label: None,
            effort: None,
            check: None,
        }
    }

    #[test]
    fn lines_without_times_are_spread_by_length() {
        let e = episode(&[("aaaa", None), ("aaaaaaaaaaaaaaaa", None)]);
        assert_eq!(super::line_times(&e, 100.0), vec![(0.0, 20.0), (20.0, 100.0)]);
        let timed = episode(&[("a", Some(0.0)), ("b", Some(30.0))]);
        assert_eq!(super::line_times(&timed, 100.0), vec![(0.0, 30.0), (30.0, 100.0)]);
    }

    fn check(supported: u32, flagged: u32) -> EpisodeCheck {
        EpisodeCheck {
            status: "done".into(),
            model: "m".into(),
            writer: None,
            error: None,
            claims: Vec::new(),
            supported,
            overstated: flagged,
            unsupported: 0,
            sources_used: 0,
            sources_total: 0,
            history: Vec::new(),
        }
    }

    #[test]
    fn the_claim_check_reads_as_a_plain_verdict() {
        assert_eq!(super::check_verdict(&check(0, 0)), "No factual claims to check");
        assert_eq!(super::check_verdict(&check(9, 1)), "Closely follows your sources");
        assert_eq!(super::check_verdict(&check(8, 2)), "Mostly follows your sources");
        assert_eq!(super::check_verdict(&check(5, 5)), "Check before relying on it");
        let mut c = check(9, 1);
        assert_eq!(super::check_models(&c), "Written and checked by m");
        assert_eq!(super::check_history(&c), None);
        c.writer = Some("w".into());
        c.history = vec![CheckCounts { supported: 4, overstated: 4, unsupported: 2 }, CheckCounts::default()];
        assert_eq!(super::check_models(&c), "Written by w, checked by m");
        assert_eq!(super::check_history(&c).unwrap(), "Before recording, 2 rounds of fixes took the flagged claims from 6 to 1");
    }

    #[test]
    fn small_labels() {
        use super::Sleep;
        assert_eq!(super::file_name("A/B: c?", "mp3"), "A B c.mp3");
        assert_eq!(super::file_name("  ", "wav"), "Episode.wav");
        assert_eq!(super::episode_minutes(4), 7);
        assert_eq!(Sleep::Minutes { minutes: 30, ends_at: Some(1900.0) }.label(100.0 + 60.0 * 12.5), "18 min");
        assert_eq!(Sleep::Minutes { minutes: 60, ends_at: None }.index(), 4);
        assert_eq!(Sleep::Chapter.label(0.0), "End of chapter");
    }

    #[test]
    fn rate_and_effort_are_remembered() {
        let mut app = crate::state::App::offline();
        app.playing = Some(super::NowPlaying::new(episode(&[]), rune::Player::audio("/nonexistent.wav"), 1));
        app.step_rate(true);
        app.step_rate(true);
        assert_eq!(app.prefs.get::<f32>("listen.rate"), Some(1.2));
        app.next_rate();
        assert_eq!(app.saved_rate(), 4);
        app.set_rate(99);
        assert_eq!(app.playing.as_ref().unwrap().rate, 7);
        app.set_effort(PodcastEffort::Thorough);
        assert_eq!(app.prefs.get::<PodcastEffort>("listen.effort"), Some(PodcastEffort::Thorough));
    }

    #[test]
    fn drafts_need_what_an_episode_needs() {
        let mut d = super::EpisodeDraft::from(&EpisodeProfile { name: "N".into(), speaker_profile_id: "sp".into(), ..Default::default() });
        assert!(!d.can_save(), "a briefing is required");
        d.briefing = rune::TextEdit::new("Explain");
        assert!(d.can_save());
        let sp = SpeakerProfile {
            name: "S".into(),
            speakers: vec![Speaker { name: "Maya".into(), voice_id: String::new(), ..Default::default() }],
            ..Default::default()
        };
        assert!(!super::SpeakerDraft::from(&sp).can_save(), "every speaker needs a voice");
    }
}
