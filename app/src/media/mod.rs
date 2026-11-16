//! Current player: Windows SMTC / Linux MPRIS.
//! Spotify, browsers (YouTube, YouTube Music, Spotify Web, SoundCloud...), etc.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::Media;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::Media;

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub raw_title: String,
    pub app: String,
    pub source_id: String,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
}

impl Track {
    pub fn key(&self) -> String {
        format!("{}|{}", self.artist, self.title).to_lowercase()
    }
}

const BROWSERS: [&str; 9] = ["chrome", "msedge", "firefox", "opera", "brave", "vivaldi", "arc", "chromium", "zen"];
const CHINA: [&str; 5] = ["cloudmusic", "netease", "qqmusic", "kugou", "kuwo"];

pub fn classify(source_id: &str) -> &'static str {
    let a = source_id.to_lowercase();
    if CHINA.iter().any(|c| a.contains(c)) { "china" }
    else if a.contains("spotify") { "spotify" }
    else if BROWSERS.iter().any(|b| a.contains(b)) { "browser" }
    else { "otro" }
}


/// Picks the track to show among all sessions.
pub fn choose(all: Vec<Track>, preferred: Option<&str>, current_id: Option<String>) -> Option<Track> {
    if let Some(p) = preferred {
        return all.into_iter().find(|t| t.source_id == p);
    }
    let mut c = all;
    c.sort_by_key(|t| (!t.playing, Some(&t.source_id) != current_id.as_ref()));
    c.into_iter().next()
}
