//! The system's Now Playing panel, media keys and headphone controls (macOS), through
//! MPNowPlayingInfoCenter and MPRemoteCommandCenter. Elsewhere, and in tests and headless
//! scenes, every call does nothing.

use crate::listen::NowPlaying;
use crate::state::Inbox;

/// What the system shows, kept so it is only told about changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    /// Whole seconds, so a playing episode updates once a second.
    pub elapsed: u64,
    pub rate: f32,
    pub playing: bool,
}

impl Info {
    pub fn of(p: &NowPlaying) -> Info {
        let chapter = p.chapter().and_then(|i| p.episode.chapters.get(i)).map(|c| c.title.clone());
        Info {
            title: p.episode.name.clone(),
            artist: p.notebook.clone().unwrap_or_else(|| "Open Notebook".into()),
            album: chapter.unwrap_or_else(|| "Open Notebook".into()),
            duration: p.duration(),
            elapsed: p.player.time().max(0.0) as u64,
            rate: crate::listen::RATES[p.rate],
            playing: p.is_playing(),
        }
    }
}

/// Takes the media keys for the app, once. Commands arrive through `inbox`.
pub fn install(inbox: Inbox) {
    #[cfg(all(target_os = "macos", not(test)))]
    mac::install(inbox);
    #[cfg(not(all(target_os = "macos", not(test))))]
    let _ = inbox;
}

/// Tells the system what is playing, when it changed.
pub fn update(p: &NowPlaying) {
    #[cfg(all(target_os = "macos", not(test)))]
    mac::update(Info::of(p));
    #[cfg(not(all(target_os = "macos", not(test))))]
    let _ = p;
}

/// Nothing is playing any more.
pub fn clear() {
    #[cfg(all(target_os = "macos", not(test)))]
    mac::clear();
}

/// What a media key or the Now Playing panel asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Skip(f64),
    SeekTo(f64),
    Previous,
    Next,
}

impl Command {
    pub fn apply(self, app: &mut crate::state::App) {
        match self {
            Command::Play => app.resume_playing(),
            Command::Pause => app.pause_playing(),
            Command::Toggle => app.toggle_play(),
            Command::Skip(by) => app.skip(by),
            Command::SeekTo(t) => app.seek(t),
            Command::Previous => app.previous_chapter(),
            Command::Next => app.next_chapter(),
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
mod mac {
    use std::cell::RefCell;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDictionary, NSNumber, NSString};
    use objc2_media_player::{
        MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist, MPMediaItemPropertyPlaybackDuration,
        MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
        MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus, MPSkipIntervalCommandEvent,
    };

    use super::{Command, Info};
    use crate::listen::{SKIP_BACK, SKIP_FORWARD};
    use crate::state::Inbox;

    thread_local! {
        /// Set once the commands are registered; the last info sent.
        static STATE: RefCell<Option<Option<Info>>> = const { RefCell::new(None) };
    }

    fn on(command: &MPRemoteCommand, inbox: &Inbox, f: impl Fn(&MPRemoteCommandEvent) -> Option<Command> + 'static) {
        let inbox = inbox.clone();
        let block = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            // SAFETY: the system hands the handler a valid event for the length of the call.
            let event = unsafe { event.as_ref() };
            match f(event) {
                Some(c) => {
                    inbox.send(move |app| c.apply(app));
                    MPRemoteCommandHandlerStatus::Success
                }
                None => MPRemoteCommandHandlerStatus::CommandFailed,
            }
        });
        // SAFETY: the block is retained by the command center for as long as it is registered.
        unsafe {
            command.setEnabled(true);
            let _ = command.addTargetWithHandler(&block);
        }
    }

    pub fn install(inbox: Inbox) {
        if STATE.with_borrow(|s| s.is_some()) {
            return;
        }
        STATE.with_borrow_mut(|s| *s = Some(None));
        // SAFETY: plain MediaPlayer calls on the main thread, where the app's frames run.
        unsafe {
            let c = MPRemoteCommandCenter::sharedCommandCenter();
            on(&c.playCommand(), &inbox, |_| Some(Command::Play));
            on(&c.pauseCommand(), &inbox, |_| Some(Command::Pause));
            on(&c.togglePlayPauseCommand(), &inbox, |_| Some(Command::Toggle));
            on(&c.nextTrackCommand(), &inbox, |_| Some(Command::Next));
            on(&c.previousTrackCommand(), &inbox, |_| Some(Command::Previous));
            let (back, forward) = (SKIP_BACK, SKIP_FORWARD);
            let back_cmd = c.skipBackwardCommand();
            back_cmd.setPreferredIntervals(&objc2_foundation::NSArray::from_retained_slice(&[NSNumber::new_f64(back)]));
            on(&back_cmd, &inbox, move |e| {
                let by = e.downcast_ref::<MPSkipIntervalCommandEvent>().map_or(back, |e| e.interval());
                Some(Command::Skip(-by))
            });
            let forward_cmd = c.skipForwardCommand();
            forward_cmd.setPreferredIntervals(&objc2_foundation::NSArray::from_retained_slice(&[NSNumber::new_f64(forward)]));
            on(&forward_cmd, &inbox, move |e| {
                let by = e.downcast_ref::<MPSkipIntervalCommandEvent>().map_or(forward, |e| e.interval());
                Some(Command::Skip(by))
            });
            on(&c.changePlaybackPositionCommand(), &inbox, |e| {
                e.downcast_ref::<MPChangePlaybackPositionCommandEvent>().map(|e| Command::SeekTo(e.positionTime()))
            });
        }
    }

    fn object<T: objc2::Message>(o: Retained<T>) -> Retained<AnyObject> {
        // SAFETY: every Objective-C object is an AnyObject.
        unsafe { Retained::cast_unchecked(o) }
    }

    pub fn update(info: Info) {
        let changed = STATE.with_borrow_mut(|s| match s {
            Some(last) if last.as_ref() != Some(&info) => {
                *last = Some(info.clone());
                true
            }
            _ => false,
        });
        if !changed {
            return;
        }
        // SAFETY: the keys are MediaPlayer's own constants; the values are the types it expects.
        unsafe {
            let keys: [&NSString; 6] = [
                MPMediaItemPropertyTitle,
                MPMediaItemPropertyArtist,
                MPMediaItemPropertyAlbumTitle,
                MPMediaItemPropertyPlaybackDuration,
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                MPNowPlayingInfoPropertyPlaybackRate,
            ];
            let values = [
                object(NSString::from_str(&info.title)),
                object(NSString::from_str(&info.artist)),
                object(NSString::from_str(&info.album)),
                object(NSNumber::new_f64(info.duration)),
                object(NSNumber::new_f64(info.elapsed as f64)),
                object(NSNumber::new_f64(if info.playing { info.rate as f64 } else { 0.0 })),
            ];
            let dict: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_retained_objects(&keys, &values);
            let center = MPNowPlayingInfoCenter::defaultCenter();
            center.setNowPlayingInfo(Some(&dict));
            center.setPlaybackState(if info.playing { MPNowPlayingPlaybackState::Playing } else { MPNowPlayingPlaybackState::Paused });
        }
    }

    pub fn clear() {
        let installed = STATE.with_borrow_mut(|s| match s {
            Some(last) => {
                *last = None;
                true
            }
            None => false,
        });
        if installed {
            // SAFETY: as in `update`.
            unsafe {
                let center = MPNowPlayingInfoCenter::defaultCenter();
                center.setNowPlayingInfo(None);
                center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use notebook_engine::types::{Episode, EpisodeChapter, EpisodeStatus};

    use super::{Command, Info};
    use crate::listen::NowPlaying;
    use crate::state::App;

    fn playing() -> App {
        let mut app = App::offline();
        let episode = Episode {
            id: "e1".into(),
            name: "Spindles".into(),
            duration: Some(600.0),
            chapters: vec![EpisodeChapter { title: "Intro".into(), start: 0.0 }, EpisodeChapter { title: "Studies".into(), start: 200.0 }],
            notebook_id: None,
            summary: None,
            status: EpisodeStatus::Completed,
            progress: None,
            error: None,
            created: String::new(),
            transcript: Vec::new(),
            exact_timing: true,
            voice_label: None,
            effort: None,
            check: None,
        };
        let mut p = NowPlaying::new(episode, rune::Player::audio("/nonexistent/episode.wav"), 1);
        p.notebook = Some("Sleep".into());
        app.playing = Some(p);
        app
    }

    #[test]
    fn the_system_sees_the_episode_notebook_and_chapter() {
        let app = playing();
        let info = Info::of(app.playing.as_ref().unwrap());
        assert_eq!((info.title.as_str(), info.artist.as_str(), info.album.as_str()), ("Spindles", "Sleep", "Intro"));
        assert_eq!(info.rate, 1.0);
        assert!(!info.playing);
    }

    #[test]
    fn media_key_commands_reach_the_player() {
        let mut app = playing();
        Command::SeekTo(250.0).apply(&mut app);
        Command::Previous.apply(&mut app);
        Command::Next.apply(&mut app);
        Command::Skip(-15.0).apply(&mut app);
        Command::Play.apply(&mut app);
        Command::Pause.apply(&mut app);
        Command::Toggle.apply(&mut app);
        assert!(app.playing.is_some());
    }
}
