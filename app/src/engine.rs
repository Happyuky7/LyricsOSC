//! Main loop: player -> lyrics -> (transcription) -> VRChat chatbox.
use crate::audio::{self, AudioOption};
use crate::axis::Axis;
use crate::config::{Config, Mode};
use crate::lyrics::{self, Lyrics};
use crate::media::{self, Media, Track};
use crate::monitor::{Monitor, Usage};
use crate::osc::{truncate_chars, VrcChatbox};
use crate::whisper::{self, Gpu, Transcriber};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LyricStatus {
    #[default]
    Idle,
    Searching,
    Synced,
    Plain,
    None,
}

#[derive(Default)]
pub struct Shared {
    pub cfg: Config,
    pub cfg_dirty: bool,
    pub sessions: Vec<Track>,
    pub audio_options: Vec<AudioOption>,
    pub gpus: Vec<Gpu>,
    pub refresh_audio: bool,
    pub restart_voice: bool,
    pub install_gpu: bool,
    pub install_voice: bool,
    pub installing_voice: bool,
    pub track: Option<Track>,
    pub position: f64,
    pub lyric_status: LyricStatus,
    pub chatbox: String,
    pub logs: Vec<String>,
    pub paused: bool,
    pub usage: Usage,
    pub voice_running: bool,
    pub python_ready: bool,
}

pub type State = Arc<Mutex<Shared>>;

fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn progress_bar(pos: f64, dur: f64) -> String {
    const W: usize = 10;
    let k = if dur > 0.0 { ((pos / dur).clamp(0.0, 1.0) * W as f64) as usize } else { 0 };
    let bar: String = (0..W).map(|i| if i < k { '━' } else if i == k { '●' } else { '─' }).collect();
    format!("{} {} {}", fmt_time(pos), bar, fmt_time(dur))
}

/// Local clock so we don't depend on how often the OS updates the position.
struct Clock { key: String, base: f64, at: Instant, playing: bool }
impl Clock {
    fn sync(&mut self, t: &Track) {
        let est = self.now();
        if t.key() != self.key || (est - t.position).abs() > 1.5 || t.playing != self.playing || !t.playing {
            self.key = t.key();
            self.base = t.position;
            self.at = Instant::now();
        }
        self.playing = t.playing;
    }
    fn now(&self) -> f64 {
        if self.playing { self.base + self.at.elapsed().as_secs_f64() } else { self.base }
    }
}

pub fn spawn(state: State) {
    std::thread::spawn(move || run(state));
}

fn push_log(s: &State, path: &std::path::Path, m: String) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
        let _ = writeln!(f, "{m}");
    }
    let mut g = s.lock().unwrap();
    g.logs.push(m);
    let n = g.logs.len();
    if n > 300 { g.logs.drain(..n - 300); }
}

fn run(state: State) {
    let log_path = crate::config::data_dir().join("lyricsosc.log");
    let _ = std::fs::write(&log_path, "");
    let log = |m: String| push_log(&state, &log_path, m);
    let (log_tx, log_rx) = channel::<String>();

    let media = match Media::new() {
        Ok(m) => Some(m),
        Err(e) => { log(format!("[media] {e}")); None }
    };
    let (ly_tx, ly_rx): (Sender<(String, Option<Lyrics>)>, Receiver<_>) = channel();
    let (text_tx, text_rx) = channel::<String>();

    let mut cfg = state.lock().unwrap().cfg.clone();
    {
        let mut g = state.lock().unwrap();
        g.gpus = whisper::list_gpus();
        g.python_ready = whisper::python_ready(&cfg);
    }
    let mut chatbox: Option<VrcChatbox> = None;
    let mut chatbox_target = String::new();
    let mut axis: Option<Axis> = None;
    let mut axis_tried = String::new();
    let mut sent_axis = String::new();
    let mut transcriber: Option<Transcriber> = None;
    let mut voice_failed = false;
    let voice_setup_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut monitor = Monitor::new();
    let mut last_monitor = Instant::now() - Duration::from_secs(10);

    let mut track: Option<Track> = None;
    let mut lyrics: Option<Lyrics> = None;
    let mut lyrics_for = String::new();
    let mut lyrics_loading = false;
    let mut heard = String::new();
    let mut clock = Clock { key: String::new(), base: 0.0, at: Instant::now(), playing: false };

    let mut last_poll = Instant::now() - Duration::from_secs(10);
    let mut last_list = last_poll;
    let mut last_osc = last_poll;
    let mut sent_osc = String::new();
    let mut cleared = true;
    let mut was_paused = false;

    log(format!("LyricsOSC {} — {}", env!("CARGO_PKG_VERSION"), crate::config::data_dir().display()));
    loop {
        std::thread::sleep(Duration::from_millis(100));
        while let Ok(m) = log_rx.try_recv() { log(m); }
        let (paused, refresh_audio, restart_voice, install_gpu, install_voice) = {
            let mut g = state.lock().unwrap();
            if g.cfg_dirty { cfg = g.cfg.clone(); g.cfg_dirty = false; }
            (g.paused, std::mem::take(&mut g.refresh_audio), std::mem::take(&mut g.restart_voice),
             std::mem::take(&mut g.install_gpu), std::mem::take(&mut g.install_voice))
        };
        if install_voice && !state.lock().unwrap().installing_voice {
            state.lock().unwrap().installing_voice = true;
            voice_setup_done.store(false, std::sync::atomic::Ordering::Relaxed);
            whisper::install_voice_engine(log_tx.clone(), voice_setup_done.clone());
        }
        if voice_setup_done.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let mut g = state.lock().unwrap();
            g.installing_voice = false;
            g.python_ready = whisper::python_ready(&cfg);
            g.gpus = whisper::list_gpus();
            voice_failed = false; // try again with the fresh install
        }
        if refresh_audio {
            let opts = audio::list_options();
            let gpus = whisper::list_gpus();
            let mut g = state.lock().unwrap();
            g.audio_options = opts;
            g.gpus = gpus;
            g.python_ready = whisper::python_ready(&cfg);
        }
        if install_gpu {
            whisper::install_gpu_support(&cfg, log_tx.clone());
        }
        // Restart the voice engine if asked, or if its settings changed
        if restart_voice || transcriber.as_ref().map_or(false, |t| t.signature != whisper::signature(&cfg)) {
            if transcriber.take().is_some() { log("[voice] restarting…".into()); }
            voice_failed = false;
        }

        // ---- chatbox socket
        // With Axis enabled, Axis drives the VRChat chatbox itself: we stop sending OSC.
        let osc_on = cfg.osc_enabled && !cfg.axis_enabled;
        let target = format!("{}:{}", cfg.osc_host, cfg.osc_port);
        if osc_on && (chatbox.is_none() || chatbox_target != target) {
            chatbox = VrcChatbox::new(&cfg.osc_host, cfg.osc_port).ok();
            chatbox_target = target;
        }
        if !osc_on {
            if let Some(cb) = chatbox.take() {
                if !cleared { cb.clear(); }   // don't leave our last line stuck in the chatbox
                cleared = true;
                sent_osc.clear();
            }
        }

        // ---- Axis DLL (addon mode)
        if cfg.axis_enabled && axis.is_none() && axis_tried != cfg.axis_dll {
            axis_tried = cfg.axis_dll.clone();
            match Axis::load(&crate::config::resolve(&cfg.axis_dll)) {
                Ok(a) => { axis = Some(a); sent_axis.clear(); log("[axis] connected".into()); }
                Err(e) => log(format!("[axis] {e}")),
            }
        }
        if !cfg.axis_enabled && axis.is_some() { axis = None; axis_tried.clear(); }

        // ---- player
        if let Some(m) = &media {
            if last_list.elapsed() >= Duration::from_secs(2) {
                last_list = Instant::now();
                state.lock().unwrap().sessions = m.sessions();
            }
            if last_poll.elapsed() >= Duration::from_millis(500) {
                last_poll = Instant::now();
                let t = media::choose(m.sessions(), cfg.preferred_source.as_deref(), m.current_id());
                if let Some(t) = &t {
                    if track.as_ref().map(|o| o.key()) != Some(t.key()) {
                        log(format!("[media] {}: {} - {}", t.app, t.artist, t.title));
                        lyrics = None;
                        lyrics_for.clear();
                        heard.clear();
                        if let Some(tr) = &transcriber { tr.reset_language(); }
                    }
                    clock.sync(t);
                }
                track = t;
            }
        }

        // ---- lyrics
        if let Ok((key, ly)) = ly_rx.try_recv() {
            if track.as_ref().map(|t| t.key()) == Some(key.clone()) {
                let st = match &ly { Some(l) if l.synced => LyricStatus::Synced, Some(_) => LyricStatus::Plain, None => LyricStatus::None };
                log(format!("[lyrics] {st:?}"));
                state.lock().unwrap().lyric_status = st;
                lyrics = ly;
                lyrics_loading = false;
            }
        }
        if let Some(t) = &track {
            if cfg.mode != Mode::Audio && lyrics_for != t.key() {
                lyrics_for = t.key();
                lyrics_loading = true;
                state.lock().unwrap().lyric_status = LyricStatus::Searching;
                let (tx, t2) = (ly_tx.clone(), t.clone());
                std::thread::spawn(move || {
                    let l = lyrics::fetch(&t2.title, &t2.artist, &t2.album, t2.duration);
                    let _ = tx.send((t2.key(), l));
                });
            }
        }

        // ---- transcription
        let pos = clock.now() + cfg.lyric_offset as f64;
        let no_lyrics = !lyrics_loading && lyrics.is_none();
        let want_voice = !paused && match cfg.mode {
            Mode::Audio => true,
            Mode::Auto => track.as_ref().map_or(false, |t| t.playing) && no_lyrics,
            Mode::Lyrics => false,
        };
        if want_voice && !voice_failed {
            if transcriber.as_mut().map_or(true, |t| !t.alive()) {
                if !whisper::python_ready(&cfg) {
                    log(format!("[voice] {}", crate::i18n::t("python_missing")));
                    voice_failed = true;
                } else {
                    match Transcriber::start(&cfg, text_tx.clone(), log_tx.clone()) {
                        Ok(t) => transcriber = Some(t),
                        Err(e) => { log(format!("[voice] {e}")); voice_failed = true; }
                    }
                }
            }
            if let Some(t) = transcriber.as_mut() { t.start_capture(&cfg.audio_source, cfg.chunk_seconds); }
        } else if let Some(t) = transcriber.as_mut() {
            t.stop_capture();
        }
        while let Ok(t) = text_rx.try_recv() { heard = t; }

        // ---- compose
        let line = |i: usize| lyrics.as_ref().and_then(|l| l.lines.get(i)).map(|x| x.1.clone());
        let (cur, next) = match (&lyrics, &track) {
            (Some(l), _) if l.synced => match l.index_at(pos) {
                Some(i) => (Some(line(i).filter(|s| !s.is_empty()).unwrap_or("♪ ♪ ♪".into())), line(i + 1)),
                None => (None, line(0)),
            },
            (Some(l), Some(t)) if t.duration > 0.0 => {
                let i = ((pos / t.duration) * l.lines.len() as f64) as usize;
                (line(i.min(l.lines.len().saturating_sub(1))), None)
            }
            _ => (if heard.is_empty() { None } else { Some(heard.clone()) }, None),
        };
        let mut lines: Vec<String> = Vec::new();
        if let Some(t) = &track {
            if cfg.show_title {
                lines.push(if t.artist.is_empty() { format!("🎵 {}", t.title) } else {
                    cfg.title_template.replace("{title}", &t.title).replace("{artist}", &t.artist).replace("{album}", &t.album).replace("{app}", &t.app)
                });
            }
            if cfg.show_progress && t.duration > 1.0 {
                lines.push(progress_bar(clock.now(), t.duration));
            }
        }
        if let Some(c) = &cur { lines.push(c.clone()); }
        if cfg.show_next_line { if let Some(n) = next.filter(|n| !n.is_empty()) { lines.push(format!("↳ {n}")); } }
        let osc_text = truncate_chars(&lines.join("\n"), 144);
        // Axis relays to the VRChat chatbox: send it exactly the chatbox text (same options, 144 chars)
        let axis_text = osc_text.clone();
        let active = track.as_ref().map_or(false, |t| t.playing) || (cfg.mode == Mode::Audio && !heard.is_empty());

        // ---- send
        if let Some(cb) = &chatbox {
            let interval = Duration::from_secs_f32(cfg.osc_interval.max(1.0));
            if paused {
                if !was_paused {
                    if !cfg.paused_text.is_empty() { cb.send(&cfg.paused_text, false); }
                    else if cfg.clear_when_paused { cb.clear(); }
                    sent_osc.clear();
                }
            } else if active && !osc_text.is_empty() && osc_text != sent_osc && last_osc.elapsed() >= interval {
                if cfg.osc_typing_indicator { cb.typing(false); }
                cb.send(&osc_text, cfg.osc_sound);
                sent_osc = osc_text.clone();
                last_osc = Instant::now();
                cleared = false;
            } else if !active && !cleared && last_osc.elapsed() >= interval {
                if cfg.paused_text.is_empty() || !cfg.clear_when_paused { cb.clear(); } else { cb.send(&cfg.paused_text, false); }
                sent_osc.clear();
                cleared = true;
                last_osc = Instant::now();
            } else if cfg.osc_typing_indicator && active && lyrics_loading {
                cb.typing(true);
            }
        }
        was_paused = paused;
        if let Some(a) = &axis {
            if !paused && active && !axis_text.is_empty() && axis_text != sent_axis {
                a.send(&axis_text);
                sent_axis = axis_text.clone();
            }
        }

        // ---- resource monitor (1 Hz)
        let usage = if last_monitor.elapsed() >= Duration::from_secs(1) {
            last_monitor = Instant::now();
            let vp = transcriber.as_mut().and_then(|t| if t.alive() { Some(t.pid()) } else { None });
            Some(monitor.sample(vp, cfg.gpu_index))
        } else { None };

        let mut g = state.lock().unwrap();
        g.track = track.clone();
        g.position = clock.now();
        g.chatbox = osc_text;
        g.voice_running = transcriber.is_some();
        if let Some(u) = usage { g.usage = u; }
    }
}
