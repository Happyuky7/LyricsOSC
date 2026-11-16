use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Mode {
    /// Online lyrics; if none, transcribe the audio
    Auto,
    /// Online lyrics only
    Lyrics,
    /// Transcribe audio only
    Audio,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Config {
    // --- general
    /// UI language code (default English; "auto" = system language)
    pub ui_language: String,
    pub mode: Mode,
    /// Fixed media session id (None = automatic)
    pub preferred_source: Option<String>,

    // --- lyrics
    pub lyric_offset: f32,
    pub show_progress: bool,
    pub show_next_line: bool,
    pub title_template: String,
    pub show_title: bool,

    // --- VRChat OSC
    pub osc_enabled: bool,
    pub osc_host: String,
    pub osc_port: u16,
    /// Minimum seconds between chatbox messages (VRChat rate limit)
    pub osc_interval: f32,
    pub osc_sound: bool,
    pub osc_typing_indicator: bool,
    pub clear_when_paused: bool,
    pub paused_text: String,

    // --- Axis (addon mode, Windows)
    pub axis_enabled: bool,
    pub axis_dll: String,

    // --- voice (Python / faster-whisper)
    /// "default", "app:<exe>", "out:<name>", "in:<name>"
    pub audio_source: String,
    pub python: String,
    pub whisper_model: String,
    /// "auto" | "cpu" | "cuda"
    pub whisper_device: String,
    pub gpu_index: u32,
    /// "auto" | "float16" | "int8_float16" | "int8" | "float32"
    pub compute_type: String,
    pub beam_size: u32,
    pub vad: bool,
    pub languages: Vec<String>,
    pub chunk_seconds: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ui_language: "en".into(),
            mode: Mode::Auto,
            preferred_source: None,
            lyric_offset: 0.3,
            show_progress: true,
            show_next_line: false,
            title_template: "🎵 {title} - {artist}".into(),
            show_title: true,
            osc_enabled: true,
            osc_host: "127.0.0.1".into(),
            osc_port: 9000,
            osc_interval: 1.6,
            osc_sound: false,
            osc_typing_indicator: false,
            clear_when_paused: true,
            paused_text: String::new(),
            axis_enabled: false,
            axis_dll: "AxisOpenApi.dll".into(),
            audio_source: "default".into(),
            python: default_python(),
            whisper_model: "small".into(),
            whisper_device: "auto".into(),
            gpu_index: 0,
            compute_type: "auto".into(),
            beam_size: 1,
            vad: false,
            languages: vec!["zh".into(), "en".into(), "es".into(), "ja".into(), "ko".into()],
            chunk_seconds: 5.0,
        }
    }
}

fn default_python() -> String {
    if cfg!(windows) { "python\\.venv\\Scripts\\python.exe".into() } else { "python/.venv/bin/python".into() }
}

pub fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| ".".into())
}

/// Finds a file next to the exe, or in the working dir (handy with `cargo run`).
pub fn resolve(rel: &str) -> PathBuf {
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        return p;
    }
    let a = base_dir().join(&p);
    if a.exists() {
        return a;
    }
    let c = std::env::current_dir().map(|d| d.join(&p)).unwrap_or(a.clone());
    if c.exists() { c } else { a }
}

/// Config/cache live in the install dir if writable, else in the user's config dir.
pub fn data_dir() -> PathBuf {
    let base = base_dir();
    let probe = base.join(".write_test");
    if std::fs::write(&probe, b"").is_ok() {
        let _ = std::fs::remove_file(probe);
        return base;
    }
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(base.clone()).join("LyricsOSC")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or(base.clone()).join("lyricsosc")
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_path(), s);
        }
    }
}
