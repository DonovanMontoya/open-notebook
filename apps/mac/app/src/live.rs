//! Live talk: the call's state, and the microphone and speaker it streams through.
//! Port of `apps/desktop/src/components/LiveTalk.tsx`. The protocol lives in the engine
//! (`podcast::live_talk`); here a worker thread opens the socket and the audio, runs the
//! call, records its usage and reports each state change back to the card.
// Parts of this only run on macOS; elsewhere they are compiled but unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use notebook_engine::Cancel;
use notebook_engine::local::LocalBackend;
use notebook_engine::podcast::live::websocket;
use notebook_engine::podcast::live_talk::{LIVE_URL, LiveAudio, LiveCommand, LiveEnd, LiveStatus, LiveTiming, VOICES, live_start, run_live_talk};
use notebook_engine::settings::Mode;

use crate::state::{App, Inbox, NotebookTab, Route};

pub const MIC_DENIED: &str = "Allow microphone access for Open Notebook in System Settings, Privacy & Security.";
pub const SERVER_MODE: &str = "Live talk runs on device for now. Switch to on-device mode and add your OpenAI key in Settings to use it.";
pub const NOT_MAC: &str = "Live talk needs macOS.";
const VOICE_PREF: &str = "live.voice";

/// The Live talk card's state.
#[derive(Default)]
pub struct LiveTalk {
    /// Index into [`VOICES`] once chosen in this run; before that, the remembered one.
    pub voice: Option<usize>,
    pub voice_open: bool,
    pub status: LiveStatus,
    pub error: Option<String>,
    pub muted: bool,
    /// The notebook the call is about.
    pub notebook: Option<String>,
    /// When `session.started` arrived, for the 30-minute clock.
    pub connected_at: Option<Instant>,
    /// Bumped by each start and by ending while connecting, so a stale call's reports are ignored.
    call: u64,
    commands: Option<Sender<LiveCommand>>,
}

impl LiveTalk {
    /// Seconds since the conversation started.
    pub fn elapsed(&self) -> f64 {
        self.connected_at.map_or(0.0, |t| t.elapsed().as_secs_f64())
    }
}

/// Why live talk can't run here, if it can't.
pub fn unavailable(s: &App) -> Option<&'static str> {
    if !cfg!(target_os = "macos") {
        Some(NOT_MAC)
    } else if s.settings.mode == Mode::Remote {
        Some(SERVER_MODE)
    } else {
        None
    }
}

impl App {
    pub fn live_choose_voice(&mut self, i: usize) {
        let i = i.min(VOICES.len() - 1);
        self.live.voice = Some(i);
        self.live.voice_open = false;
        let _ = self.prefs.set(VOICE_PREF, VOICES[i]);
    }

    /// The voice the next call uses: this run's choice, else the remembered one, else marin.
    pub fn live_voice(&self) -> usize {
        self.live.voice.or_else(|| self.prefs.get::<String>(VOICE_PREF).and_then(|v| VOICES.iter().position(|x| *x == v))).unwrap_or(0)
    }

    pub fn live_start(&mut self) {
        let Some(notebook) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        if let Some(why) = unavailable(self) {
            self.live.status = LiveStatus::Failed;
            self.live.error = Some(why.into());
            return;
        }
        let Some(local) = self.backend.as_ref().and_then(|b| b.local().cloned()) else {
            self.live.status = LiveStatus::Failed;
            self.live.error = Some(SERVER_MODE.into());
            return;
        };
        self.live.call += 1;
        let call = self.live.call;
        let (tx, rx) = channel();
        self.live = LiveTalk {
            voice: self.live.voice,
            status: LiveStatus::Connecting,
            notebook: Some(notebook.clone()),
            call,
            commands: Some(tx),
            ..Default::default()
        };
        let voice = VOICES[self.live_voice()];
        let inbox = self.inbox();
        rune::Recorder::request_permission(move |answer| {
            if answer != rune::MicPermission::Granted {
                let end = LiveEnd { status: LiveStatus::Failed, error: Some(MIC_DENIED.into()), seconds: 0.0 };
                inbox.send(move |app| app.live_finish(call, end));
                return;
            }
            std::thread::spawn(move || {
                let end = session(&local, &notebook, voice, &rx, &inbox, call);
                local.record_live_talk(end.seconds);
                inbox.send(move |app| app.live_finish(call, end));
            });
        });
    }

    /// End, like the TypeScript `stop`: a started call asks the session to close and the worker
    /// reports Closing then Ended; one still connecting ends at once.
    pub fn live_stop(&mut self) {
        match self.live.status {
            LiveStatus::Connected => {
                if let Some(tx) = &self.live.commands {
                    let _ = tx.send(LiveCommand::Mute(true));
                    let _ = tx.send(LiveCommand::End);
                }
            }
            LiveStatus::Connecting => {
                // Dropping the sender tells the worker to stop as soon as it looks.
                self.live.call += 1;
                self.live.commands = None;
                self.live.status = LiveStatus::Ended;
                self.live.connected_at = None;
                self.live.muted = false;
            }
            _ => {}
        }
    }

    pub fn live_mute(&mut self) {
        self.live.muted = !self.live.muted;
        if let Some(tx) = &self.live.commands {
            let _ = tx.send(LiveCommand::Mute(self.live.muted));
        }
    }

    pub fn live_status(&mut self, call: u64, status: LiveStatus) {
        if call != self.live.call {
            return;
        }
        self.live.status = status;
        if status == LiveStatus::Connected {
            self.live.connected_at = Some(Instant::now());
        }
    }

    pub fn live_finish(&mut self, call: u64, end: LiveEnd) {
        if call != self.live.call {
            return;
        }
        self.live.status = end.status;
        self.live.error = end.error;
        self.live.commands = None;
        self.live.connected_at = None;
        self.live.muted = false;
    }

    /// Like the TypeScript card unmounting: leaving the notebook's Listen tab ends the call.
    pub fn live_left(&self) -> bool {
        self.live.status.live()
            && (self.open.as_ref().is_none_or(|o| Some(&o.id) != self.live.notebook.as_ref() || o.tab != NotebookTab::Listen)
                || !matches!(self.route, Route::Notebook(_)))
    }
}

/// Called from `poll`: ends a call whose card went away and keeps the clock ticking.
pub fn poll(s: &mut App, tx: &mut rune::Tx) -> bool {
    if s.live_left() {
        s.live_stop();
        return true;
    }
    if s.live.status == LiveStatus::Connected {
        tx.wake_after(std::time::Duration::from_secs(1));
        return true;
    }
    false
}

/// One call on the worker thread: the notebook's material, the socket, the audio, the session.
fn session(local: &LocalBackend, notebook: &str, voice: &str, commands: &Receiver<LiveCommand>, inbox: &Inbox, call: u64) -> LiveEnd {
    let failed = |e: String| LiveEnd { status: LiveStatus::Failed, error: Some(e), seconds: 0.0 };
    let setup = match local.live_setup(notebook, &Cancel::new()) {
        Ok(s) => s,
        Err(e) => return failed(e.to_string()),
    };
    let mut socket = match websocket()(LIVE_URL, &[("Authorization".into(), format!("Bearer {}", setup.api_key))]) {
        Ok(s) => s,
        Err(e) => return failed(e.to_string()),
    };
    let mut audio = match open_audio() {
        Ok(a) => a,
        Err(e) => {
            socket.close();
            return failed(e);
        }
    };
    let start = live_start(voice, &setup.backend_model, Some(&setup.material));
    let report = |status: LiveStatus| inbox.send(move |app| app.live_status(call, status));
    let end = run_live_talk(&mut *socket, &start, &mut *audio, commands, &mut |s| report(s), LiveTiming::default());
    socket.close();
    end
}

#[cfg(target_os = "macos")]
fn open_audio() -> Result<Box<dyn LiveAudio>, String> {
    Ok(Box::new(mac::MacAudio::start()?))
}

#[cfg(not(target_os = "macos"))]
fn open_audio() -> Result<Box<dyn LiveAudio>, String> {
    Err(NOT_MAC.into())
}

// Audio formats

/// Streaming linear resampler. Keeps its phase and the last sample between chunks, so
/// 10 ms mic buffers join without clicks.
pub struct Resampler {
    step: f64,
    /// Position of the next output, in input samples from the start of the next chunk; -1..0
    /// falls between the previous chunk's last sample and this chunk's first.
    t: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(from: f64, to: f64) -> Self {
        Resampler { step: from / to, t: 0.0, prev: 0.0 }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        loop {
            let i = self.t.floor();
            let next = (i + 1.0) as isize;
            if next >= input.len() as isize {
                break;
            }
            let a = if i < 0.0 { self.prev } else { input[i as usize] };
            let b = input[next as usize];
            out.push(a + (b - a) * (self.t - i) as f32);
            self.t += self.step;
        }
        self.t -= input.len() as f64;
        self.prev = input[input.len() - 1];
        out
    }
}

pub fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|x| (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16).collect()
}

pub fn to_float(pcm: &[i16]) -> Vec<f32> {
    pcm.iter().map(|s| *s as f32 / 32768.0).collect()
}

/// The first channel of a buffer whose channels are `stride` samples apart.
pub fn first_channel(data: &[f32], frames: usize, stride: usize) -> Vec<f32> {
    (0..frames).filter_map(|i| data.get(i * stride.max(1)).copied()).collect()
}

#[cfg(target_os = "macos")]
mod mac {
    //! AVAudioEngine with voice processing on the input node, which gives echo cancellation
    //! against what the engine itself plays. Everything here stays on the call's worker thread;
    //! only the tap block runs on the audio thread, and it only sends samples down a channel.

    use std::ptr::NonNull;
    use std::sync::mpsc::{Receiver, channel};

    use block2::RcBlock;
    use notebook_engine::podcast::audio::SAMPLE_RATE;
    use notebook_engine::podcast::live_talk::LiveAudio;
    use objc2::AllocAnyThread;
    use objc2::rc::Retained;
    use objc2_avf_audio::{AVAudioEngine, AVAudioFormat, AVAudioPCMBuffer, AVAudioPlayerNode, AVAudioTime};

    use super::{Resampler, first_channel, to_float, to_pcm16};

    pub struct MacAudio {
        engine: Retained<AVAudioEngine>,
        player: Retained<AVAudioPlayerNode>,
        format: Retained<AVAudioFormat>,
        mic: Receiver<Vec<f32>>,
        resampler: Resampler,
        _tap: RcBlock<dyn Fn(NonNull<AVAudioPCMBuffer>, NonNull<AVAudioTime>)>,
    }

    impl MacAudio {
        pub fn start() -> Result<MacAudio, String> {
            let fail = |what: &str, e: Option<String>| format!("Could not start {what}{}", e.map(|e| format!(": {e}")).unwrap_or_default());
            unsafe {
                let engine = AVAudioEngine::new();
                let input = engine.inputNode();
                input.setVoiceProcessingEnabled_error(true).map_err(|e| fail("echo cancellation", Some(e.localizedDescription().to_string())))?;
                let mic_format = input.outputFormatForBus(0);
                let mic_rate = mic_format.sampleRate();
                if mic_rate <= 0.0 || mic_format.channelCount() == 0 {
                    return Err("No microphone is available".into());
                }

                let format = AVAudioFormat::initStandardFormatWithSampleRate_channels(AVAudioFormat::alloc(), SAMPLE_RATE as f64, 1)
                    .ok_or_else(|| fail("the speaker", None))?;
                let player = AVAudioPlayerNode::new();
                engine.attachNode(&player);
                engine.connect_to_format(&player, &engine.mainMixerNode(), Some(&format));

                let (tx, mic) = channel();
                let tap = RcBlock::new(move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
                    let buffer = buffer.as_ref();
                    let channels = buffer.floatChannelData();
                    if channels.is_null() {
                        return;
                    }
                    let frames = buffer.frameLength() as usize;
                    let stride = buffer.stride();
                    let data = std::slice::from_raw_parts((*channels).as_ptr(), frames * stride.max(1));
                    let _ = tx.send(first_channel(data, frames, stride));
                });
                input.installTapOnBus_bufferSize_format_block(0, (mic_rate / 50.0) as u32, None, RcBlock::as_ptr(&tap));
                engine.prepare();
                if let Err(e) = engine.startAndReturnError() {
                    input.removeTapOnBus(0);
                    return Err(fail("the microphone", Some(e.localizedDescription().to_string())));
                }
                player.play();
                Ok(MacAudio { engine, player, format, mic, resampler: Resampler::new(mic_rate, SAMPLE_RATE as f64), _tap: tap })
            }
        }
    }

    impl LiveAudio for MacAudio {
        fn take_input(&mut self) -> Vec<i16> {
            let mut out = Vec::new();
            while let Ok(chunk) = self.mic.try_recv() {
                out.extend(to_pcm16(&self.resampler.process(&chunk)));
            }
            out
        }

        fn play(&mut self, pcm: &[i16]) {
            if pcm.is_empty() {
                return;
            }
            unsafe {
                let Some(buffer) = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(AVAudioPCMBuffer::alloc(), &self.format, pcm.len() as u32) else { return };
                let channels = buffer.floatChannelData();
                if channels.is_null() {
                    return;
                }
                let out = std::slice::from_raw_parts_mut((*channels).as_ptr(), pcm.len());
                out.copy_from_slice(&to_float(pcm));
                buffer.setFrameLength(pcm.len() as u32);
                self.player.scheduleBuffer_completionHandler(&buffer, std::ptr::null_mut());
            }
        }
    }

    impl Drop for MacAudio {
        fn drop(&mut self) {
            unsafe {
                self.engine.inputNode().removeTapOnBus(0);
                self.player.stop();
                self.engine.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_48k_to_24k_keeps_every_other_sample_across_chunks() {
        let signal: Vec<f32> = (0..20).map(|i| i as f32).collect();
        let mut r = Resampler::new(48_000.0, 24_000.0);
        let mut out = r.process(&signal[..5]);
        out.extend(r.process(&signal[5..12]));
        out.extend(r.process(&[]));
        out.extend(r.process(&signal[12..]));
        assert_eq!(out, (0..10).map(|i| (i * 2) as f32).collect::<Vec<_>>());
    }

    #[test]
    fn resampling_up_interpolates_between_chunks() {
        let mut r = Resampler::new(16_000.0, 24_000.0);
        let mut out = r.process(&[0.0, 3.0]);
        out.extend(r.process(&[6.0, 9.0]));
        // Outputs every 2/3 of an input sample: 0, 2, 4, 6, 8.
        assert_eq!(out.len(), 5);
        for (got, want) in out.iter().zip([0.0, 2.0, 4.0, 6.0, 8.0]) {
            assert!((got - want).abs() < 1e-4, "{out:?}");
        }
    }

    #[test]
    fn a_second_of_44_1k_mic_becomes_a_second_of_24k() {
        let mut r = Resampler::new(44_100.0, 24_000.0);
        let total: usize = (0..100).map(|_| r.process(&[0.25; 441]).len()).sum();
        assert!((23_999..=24_000).contains(&total), "{total}");
    }

    #[test]
    fn pcm16_conversion_clamps_and_round_trips() {
        assert_eq!(to_pcm16(&[0.0, 1.0, -1.0, 2.0, -2.0, 0.5]), [0, 32767, -32768, 32767, -32768, 16384]);
        let back = to_float(&[0, 16384, -32768, 32767]);
        assert_eq!(back, [0.0, 0.5, -1.0, 32767.0 / 32768.0]);
        let pcm: Vec<i16> = vec![-20000, -1, 0, 1, 12345];
        assert_eq!(to_pcm16(&to_float(&pcm)), pcm);
    }

    #[test]
    fn the_first_channel_is_read_at_its_stride() {
        let interleaved = [1.0, 9.0, 2.0, 9.0, 3.0, 9.0];
        assert_eq!(first_channel(&interleaved, 3, 2), [1.0, 2.0, 3.0]);
        assert_eq!(first_channel(&[4.0, 5.0], 2, 1), [4.0, 5.0]);
    }

    fn connecting(app: &mut App) -> u64 {
        app.open_offline("nb1");
        app.open.as_mut().unwrap().tab = NotebookTab::Listen;
        app.live.call += 1;
        app.live.status = LiveStatus::Connecting;
        app.live.notebook = Some("nb1".into());
        app.live.call
    }

    #[test]
    fn reports_move_the_card_through_its_states_and_stale_calls_are_ignored() {
        let mut app = App::offline();
        let call = connecting(&mut app);
        app.live_status(call, LiveStatus::Connected);
        assert_eq!(app.live.status, LiveStatus::Connected);
        assert!(app.live.connected_at.is_some());
        app.live_status(call - 1, LiveStatus::Failed);
        assert_eq!(app.live.status, LiveStatus::Connected, "an older call's report is ignored");
        app.live_status(call, LiveStatus::Closing);
        app.live_finish(call, LiveEnd { status: LiveStatus::Ended, error: None, seconds: 3.0 });
        assert_eq!(app.live.status, LiveStatus::Ended);
        assert!(app.live.connected_at.is_none());
    }

    #[test]
    fn ending_while_connecting_ends_at_once_and_the_late_worker_is_ignored() {
        let mut app = App::offline();
        let call = connecting(&mut app);
        app.live_stop();
        assert_eq!(app.live.status, LiveStatus::Ended);
        app.live_status(call, LiveStatus::Connected);
        app.live_finish(call, LiveEnd { status: LiveStatus::Failed, error: Some("late".into()), seconds: 0.0 });
        assert_eq!((app.live.status, app.live.error.as_deref()), (LiveStatus::Ended, None));
    }

    #[test]
    fn ending_a_connected_call_asks_the_worker_to_mute_and_close() {
        let mut app = App::offline();
        let call = connecting(&mut app);
        let (tx, rx) = channel();
        app.live.commands = Some(tx);
        app.live_status(call, LiveStatus::Connected);
        app.live_mute();
        assert!(app.live.muted);
        app.live_stop();
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [LiveCommand::Mute(true), LiveCommand::Mute(true), LiveCommand::End]);
        assert_eq!(app.live.status, LiveStatus::Connected, "the worker reports Closing");
    }

    #[test]
    fn a_denied_microphone_fails_with_the_typescript_message() {
        let mut app = App::offline();
        let call = connecting(&mut app);
        app.live_finish(call, LiveEnd { status: LiveStatus::Failed, error: Some(MIC_DENIED.into()), seconds: 0.0 });
        assert_eq!(app.live.error.as_deref(), Some("Allow microphone access for Open Notebook in System Settings, Privacy & Security."));
    }

    #[test]
    fn leaving_the_listen_tab_or_the_notebook_ends_the_call() {
        let mut app = App::offline();
        connecting(&mut app);
        assert!(!app.live_left());
        app.open.as_mut().unwrap().tab = NotebookTab::Chat;
        assert!(app.live_left());
        app.open.as_mut().unwrap().tab = NotebookTab::Listen;
        app.open_offline("nb2");
        assert!(app.live_left());
        app.live.status = LiveStatus::Ended;
        assert!(!app.live_left(), "nothing to end");
    }

    #[test]
    fn server_mode_explains_that_live_talk_runs_on_device() {
        let mut app = App::offline();
        assert_eq!(unavailable(&app), None);
        app.settings.mode = Mode::Remote;
        assert_eq!(unavailable(&app), Some(SERVER_MODE));
        app.open_offline("nb1");
        app.live_start();
        assert_eq!((app.live.status, app.live.error.as_deref()), (LiveStatus::Failed, Some(SERVER_MODE)));
    }

    #[test]
    fn the_voice_choice_is_remembered() {
        let mut app = App::offline();
        assert_eq!(app.live_voice(), 0);
        app.live_choose_voice(3);
        assert_eq!(app.live_voice(), 3);
        app.live.voice = None;
        assert_eq!(VOICES[app.live_voice()], "coral", "remembered across runs");
    }
}
