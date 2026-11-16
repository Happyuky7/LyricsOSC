//! Synced lyrics from LRCLIB + title cleanup (YouTube, etc.)
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Lyrics {
    /// (seconds, text). When `synced == false` all times are 0.
    pub lines: Vec<(f64, String)>,
    pub synced: bool,
}

impl Lyrics {
    pub fn index_at(&self, pos: f64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let i = self.lines.partition_point(|(t, _)| *t <= pos);
        if i == 0 { None } else { Some(i - 1) }
    }
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

pub fn parse_lrc(text: &str) -> Vec<(f64, String)> {
    static TS: OnceLock<Regex> = OnceLock::new();
    let ts = re(&TS, r"\[(\d+):(\d+(?:\.\d+)?)\]");
    let mut out = Vec::new();
    for raw in text.lines() {
        let stamps: Vec<f64> = ts
            .captures_iter(raw)
            .filter_map(|c| Some(c[1].parse::<f64>().ok()? * 60.0 + c[2].parse::<f64>().ok()?))
            .collect();
        if stamps.is_empty() {
            continue;
        }
        let mut lyric = ts.replace_all(raw, "").trim().to_string();
        // "original^translation" -> original
        if let Some(i) = lyric.find('^') {
            lyric = lyric[..i].trim().to_string();
        }
        for t in stamps {
            out.push((t, lyric.clone()));
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    out
}

fn to_lyrics(v: &Value) -> Option<Lyrics> {
    if let Some(s) = v.get("syncedLyrics").and_then(|s| s.as_str()) {
        let lines = parse_lrc(s);
        if !lines.is_empty() {
            return Some(Lyrics { lines, synced: true });
        }
    }
    if let Some(s) = v.get("plainLyrics").and_then(|s| s.as_str()) {
        let lines: Vec<_> = s.lines().map(str::trim).filter(|l| !l.is_empty())
            .map(|l| (0.0, l.to_string())).collect();
        if !lines.is_empty() {
            return Some(Lyrics { lines, synced: false });
        }
    }
    None
}

fn has_lyrics(v: &Value) -> bool {
    ["syncedLyrics", "plainLyrics"].iter().any(|k| v.get(*k).and_then(|s| s.as_str()).map_or(false, |s| !s.is_empty()))
}

fn get(url: &str) -> Option<Value> {
    ureq::get(url)
        .set("User-Agent", concat!("LyricsOSC/", env!("CARGO_PKG_VERSION"), " (https://github.com/)"))
        .timeout(std::time::Duration::from_secs(5))
        .call().ok()?.into_json().ok()
}

fn pick(results: Value, duration: f64) -> Option<Value> {
    let arr = results.as_array()?.clone();
    arr.into_iter().max_by_key(|r| {
        let mut s: i64 = 0;
        if r.get("syncedLyrics").and_then(|v| v.as_str()).is_some() { s += 1000 }
        else if r.get("plainLyrics").and_then(|v| v.as_str()).is_some() { s += 100 }
        if duration > 0.0 {
            if let Some(d) = r.get("duration").and_then(|d| d.as_f64()) {
                s -= ((d - duration).abs().min(60.0) * 10.0) as i64;
            }
        }
        s
    })
}

fn cache_path(artist: &str, title: &str) -> std::path::PathBuf {
    let name: String = format!("{artist} - {title}")
        .chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).take(150).collect();
    crate::config::data_dir().join("cache").join(format!("{name}.json"))
}

pub fn fetch(title: &str, artist: &str, album: &str, duration: f64) -> Option<Lyrics> {
    if title.is_empty() {
        return None;
    }
    let cp = cache_path(artist, title);
    if let Ok(s) = std::fs::read_to_string(&cp) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            // {"none": <unix>} = no lyrics found recently: retry after 1 day
            if let Some(t) = v.get("none").and_then(|t| t.as_u64()) {
                if now_unix().saturating_sub(t) < 86_400 { return None; }
            } else {
                return to_lyrics(&v);
            }
        }
    }
    let enc = |s: &str| urlencoding::encode(s).into_owned();
    const API: &str = "https://lrclib.net/api";

    // All queries in parallel; priority order decides among good results.
    let mut urls: Vec<(u8, String, bool)> = Vec::new(); // (priority, url, is_search)
    if !artist.is_empty() {
        let mut u = format!("{API}/get?track_name={}&artist_name={}", enc(title), enc(artist));
        if !album.is_empty() { u += &format!("&album_name={}", enc(album)); }
        if duration > 0.0 { u += &format!("&duration={}", duration.round() as i64); }
        urls.push((0, u, false));
        urls.push((1, format!("{API}/search?track_name={}&artist_name={}", enc(title), enc(artist)), true));
        urls.push((3, format!("{API}/search?track_name={}", enc(title)), true));
    }
    urls.push((2, format!("{API}/search?q={}", enc(format!("{artist} {title}").trim())), true));

    let (tx, rx) = std::sync::mpsc::channel::<(u8, Option<Value>)>();
    let n = urls.len();
    for (prio, url, search) in urls {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let r = get(&url).and_then(|v| if search { pick(v, duration) } else { Some(v) }).filter(has_lyrics);
            let _ = tx.send((prio, r));
        });
    }
    drop(tx);
    // Take the best-priority result; stop early once a synced top-priority answer arrives.
    let mut best: Option<(u8, Value)> = None;
    for _ in 0..n {
        let Ok((prio, r)) = rx.recv_timeout(std::time::Duration::from_secs(6)) else { break };
        if let Some(v) = r {
            let synced = v.get("syncedLyrics").and_then(|s| s.as_str()).is_some();
            let score = prio + if synced { 0 } else { 10 };
            if best.as_ref().map_or(true, |(b, _)| score < *b) {
                best = Some((score, v));
            }
            if score <= 1 { break; }
        }
    }
    let _ = std::fs::create_dir_all(cp.parent().unwrap());
    match best {
        Some((_, item)) => {
            let _ = std::fs::write(&cp, item.to_string());
            to_lyrics(&item)
        }
        None => {
            let _ = std::fs::write(&cp, format!("{{\"none\": {}}}", now_unix()));
            None
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------- titles
pub fn is_placeholder(title: &str) -> bool {
    static P: OnceLock<Regex> = OnceLock::new();
    let p = re(&P, r"(?i)reproduciendo elementos multimedia|is playing media|正在播放媒体|^spotify( free| premium)?$|^advertisement$|^anuncio$");
    title.trim().is_empty() || p.is_match(title)
}

/// Returns a clean (title, artist). `split` = try to split "Artist - Song".
pub fn clean_title(title: &str, artist: &str, split: bool) -> (String, String) {
    static JUNK: OnceLock<Regex> = OnceLock::new();
    static EDIT: OnceLock<Regex> = OnceLock::new();
    static FEAT: OnceLock<Regex> = OnceLock::new();
    static CUT: OnceLock<Regex> = OnceLock::new();
    static CH: OnceLock<Regex> = OnceLock::new();
    let junk = re(&JUNK, r"(?i)[\(\[【（][^\)\]】）]*?(?:official|oficial|video|vídeo|audio|lyric|letra|visualizer|mv|m/v|hd|4k|remaster|live|en vivo|官方|歌词|動態歌詞|动态歌词|完整版|pinyin|拼音|sub|ver\.?|version|original|full|cover|tv size|movie|劇場版|主題歌|feat)[^\)\]】）]*[\)\]】）]");
    let edit = re(&EDIT, r"(?i)\s*[\(\[]?\b(?:slowed|reverb|sped up|speed up|nightcore|8d|bass boosted|[\w\s]*remix|tiktok version|extended)\b[^\)\]]*[\)\]]?");
    let feat = re(&FEAT, r"(?i)\s*(?:ft\.|feat\.?|featuring)\s.*$");
    let cut = re(&CUT, r"(?i)\s*(?:official\s+(?:music\s+)?video|official\s+mv|music\s+video|／|｜|\s\|\s|\s-\S|\s[~〜～]\s?\S)");
    let ch = re(&CH, r"(?i)\s*(?:-\s*Topic|VEVO|Official|官方频道|官方頻道)$");

    let mut t = junk.replace_all(title, "").to_string();
    t = edit.replace_all(&t, "").to_string();
    t = feat.replace(&t, "").to_string();
    t = t.replace(['《', '》', '「', '」', '『', '』'], " ");
    t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut a = ch.replace(artist.trim(), "").trim().to_string();
    if split {
        for sep in [" - ", " – ", " — ", " | ", "："] {
            if let Some((l, r)) = t.split_once(sep) {
                if !l.trim().is_empty() && !r.trim().is_empty() {
                    a = l.trim().to_string();
                    t = r.trim().to_string();
                    break;
                }
            }
        }
    }
    if let Some(m) = cut.find(&t) {
        if m.start() > 0 { t.truncate(m.start()); }
    }
    if !a.is_empty() && t.to_lowercase().starts_with(&(a.to_lowercase() + " ")) {
        t = t[a.len()..].to_string();
    }
    let t = t.trim_matches(|c: char| c.is_whitespace() || "-–—:".contains(c)).to_string();
    // If cleaning leaves title == artist, too much was cut: fall back to the original
    // title, only removing the bracketed tags.
    if t.is_empty() || t.eq_ignore_ascii_case(&a) {
        let base = junk.replace_all(title, "").trim().to_string();
        if base.eq_ignore_ascii_case(artist.trim()) || base.is_empty() {
            return (title.trim().to_string(), String::new());
        }
        return (base, if a.eq_ignore_ascii_case(artist.trim()) { a } else { artist.trim().to_string() });
    }
    (t, a)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn titles() {
        assert_eq!(clean_title("Romeo Santos, Prince Royce - Dardos (Official Video)", "RomeoSantosVEVO", true).0, "Dardos");
        assert_eq!(clean_title("Gata Only (DSCVR Remix) - Slowed", "DSCVR", true).0, "Gata Only");
        assert_eq!(clean_title("YOASOBI 勇者 Official Music Video／TVアニメ", "YOASOBI", true).0, "勇者");
        assert_eq!(clean_title("AC/DC - Thunderstruck", "AC/DC", true), ("Thunderstruck".into(), "AC/DC".into()));
        assert_eq!(clean_title("RADWIMPS - スパークル [original ver.] -Your name.", "RADWIMPS", true),
                   ("スパークル".into(), "RADWIMPS".into()));
        assert_eq!(clean_title("Lemon (Full Ver.)", "米津玄師", false).0, "Lemon");
    }
}
