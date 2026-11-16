//! Linux: MPRIS over D-Bus (Spotify, Firefox, Chrome, VLC, mpv, ...).
use super::{classify, Track};
use crate::lyrics::{clean_title, is_placeholder};
use std::collections::HashMap;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

const PREFIX: &str = "org.mpris.MediaPlayer2.";

pub struct Media {
    conn: Connection,
}

fn s(v: Option<&OwnedValue>) -> String {
    v.and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default()
}

fn first_of_list(v: Option<&OwnedValue>) -> String {
    v.and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
        .map(|l| l.join(", ")).unwrap_or_default()
}

impl Media {
    pub fn new() -> zbus::Result<Self> {
        Ok(Self { conn: Connection::session()? })
    }

    fn players(&self) -> Vec<String> {
        let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&self.conn) else { return vec![] };
        dbus.list_names().unwrap_or_default().into_iter()
            .map(|n| n.to_string()).filter(|n| n.starts_with(PREFIX)).collect()
    }

    fn read(&self, bus: &str) -> Option<Track> {
        let p = Proxy::new(&self.conn, bus, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").ok()?;
        let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").ok()?;
        let raw_title = s(meta.get("xesam:title"));
        if is_placeholder(&raw_title) {
            return None;
        }
        let raw_artist = first_of_list(meta.get("xesam:artist"));
        let album = s(meta.get("xesam:album"));
        let duration = meta.get("mpris:length")
            .and_then(|v| i64::try_from(v.try_clone().ok()?).ok()
                .or_else(|| u64::try_from(v.try_clone().ok()?).ok().map(|x| x as i64)))
            .map(|us| us as f64 / 1e6).unwrap_or(0.0);
        let status: String = p.get_property("PlaybackStatus").unwrap_or_default();
        let position = p.get_property::<i64>("Position").map(|us| us as f64 / 1e6).unwrap_or(0.0);
        let source_id = bus.trim_start_matches(PREFIX).to_string();
        let app = classify(&source_id);
        let (title, artist) = clean_title(&raw_title, &raw_artist, app != "spotify");
        Some(Track { title, artist, album, raw_title, app: app.into(), source_id,
                     position, duration, playing: status == "Playing" })
    }

    pub fn sessions(&self) -> Vec<Track> {
        self.players().iter().filter_map(|b| self.read(b)).collect()
    }

    pub fn current_id(&self) -> Option<String> {
        None
    }
}
