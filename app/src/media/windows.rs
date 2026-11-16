use super::{classify, Track};
use crate::lyrics::{clean_title, is_placeholder};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

/// Ticks of 100 ns since 1601 -> now
fn now_filetime() -> i64 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    (unix.as_nanos() / 100) as i64 + 116_444_736_000_000_000
}

pub struct Media {
    mgr: Manager,
}

impl Media {
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        Ok(Self { mgr: Manager::RequestAsync()?.get()? })
    }

    fn read(s: &Session) -> Option<Track> {
        let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let raw_title = props.Title().ok()?.to_string();
        if is_placeholder(&raw_title) {
            return None;
        }
        let raw_artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
        let album = props.AlbumTitle().map(|a| a.to_string()).unwrap_or_default();
        let source_id = s.SourceAppUserModelId().map(|a| a.to_string()).unwrap_or_default();
        let app = classify(&source_id);
        let playing = s.GetPlaybackInfo().ok()
            .and_then(|i| i.PlaybackStatus().ok())
            .map_or(false, |st| st == Status::Playing);
        let tl = s.GetTimelineProperties().ok()?;
        let mut position = tl.Position().map(|t| t.Duration as f64 / 1e7).unwrap_or(0.0);
        let duration = tl.EndTime().map(|t| t.Duration as f64 / 1e7).unwrap_or(0.0);
        if playing {
            if let Ok(upd) = tl.LastUpdatedTime() {
                let dt = (now_filetime() - upd.UniversalTime) as f64 / 1e7;
                if dt > 0.0 && dt < 3600.0 {
                    position += dt;
                }
            }
        }
        let (title, artist) = clean_title(&raw_title, &raw_artist, app != "spotify");
        Some(Track { title, artist, album, raw_title, app: app.into(),
                     source_id, position, duration, playing })
    }

    pub fn sessions(&self) -> Vec<Track> {
        let Ok(list) = self.mgr.GetSessions() else { return vec![] };
        list.into_iter().filter_map(|s| Self::read(&s)).collect()
    }

    /// Id of the session Windows considers "current".
    pub fn current_id(&self) -> Option<String> {
        self.mgr.GetCurrentSession().ok()
            .and_then(|s| s.SourceAppUserModelId().ok()).map(|s| s.to_string())
    }
}
