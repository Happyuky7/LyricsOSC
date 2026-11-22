//! Online translation of lyric lines: MyMemory (free, no key), DeepL (API key) or LibreTranslate.
//! The source language is detected offline with `whatlang`. Results are cached on disk per song
//! (`cache/translations/<sha256>.json`), so each song is translated only once per provider/target.
use crate::config::Config;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Original line -> translated line.
pub type Translations = HashMap<String, String>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Error {
    /// Free daily limit reached (MyMemory) or quota exceeded (DeepL)
    Quota,
    /// The provider does not support the target language
    Unsupported,
    /// The provider needs an API key
    NoKey,
    /// Network / server error (raw message, for the log and the status line)
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Quota => write!(f, "quota exceeded"),
            Error::Unsupported => write!(f, "target language not supported"),
            Error::NoKey => write!(f, "API key required"),
            Error::Other(m) => write!(f, "{m}"),
        }
    }
}

/// Everything a translation depends on (taken from the config).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    pub provider: String,
    /// One of the UI language codes (already resolved from "auto")
    pub target: String,
    pub deepl_key: String,
    pub libre_url: String,
    pub libre_key: String,
    pub mymemory_email: String,
}

impl Settings {
    pub fn from_cfg(cfg: &Config) -> Self {
        let target = if cfg.translate_to == "auto" { crate::i18n::current().to_string() } else { cfg.translate_to.clone() };
        Self {
            provider: cfg.translate_provider.clone(),
            target,
            deepl_key: cfg.deepl_key.trim().to_string(),
            libre_url: cfg.libre_url.trim().trim_end_matches('/').to_string(),
            libre_key: cfg.libre_key.trim().to_string(),
            mymemory_email: cfg.mymemory_email.trim().to_string(),
        }
    }

    /// Changes whenever a new translation is needed.
    pub fn signature(&self) -> String {
        format!("{}|{}|{}|{}|{}|{}", self.provider, self.target, self.deepl_key, self.libre_url, self.libre_key, self.mymemory_email)
    }
}

// ------------------------------------------------------------------ language detection

/// ISO 639-3 (whatlang) -> code understood by MyMemory (ISO 639-1 / RFC 3066).
fn iso1(code3: &str) -> Option<&'static str> {
    Some(match code3 {
        "eng" => "en", "spa" => "es", "cmn" => "zh-CN", "jpn" => "ja", "kor" => "ko", "por" => "pt",
        "fra" => "fr", "rus" => "ru", "tgl" => "tl", "tha" => "th", "deu" => "de", "ita" => "it",
        "vie" => "vi", "ind" => "id", "tur" => "tr", "pol" => "pl", "ukr" => "uk", "nld" => "nl",
        "swe" => "sv", "dan" => "da", "nob" => "no", "fin" => "fi", "ces" => "cs", "slk" => "sk",
        "hun" => "hu", "ron" => "ro", "ell" => "el", "ara" => "ar", "heb" => "he", "hin" => "hi",
        "ben" => "bn", "cat" => "ca", "bul" => "bg", "hrv" => "hr", "srp" => "sr", "lit" => "lt",
        "lav" => "lv", "est" => "et", "slv" => "sl", "pes" => "fa", "urd" => "ur", "tam" => "ta",
        "tel" => "te", "mar" => "mr", "guj" => "gu", "kan" => "kn", "mal" => "ml", "pan" => "pa",
        "bel" => "be", "aze" => "az", "kat" => "ka", "hye" => "hy", "afr" => "af", "epo" => "eo",
        "lat" => "la", "yid" => "yi", "zul" => "zu", "jav" => "jv", "khm" => "km", "mya" => "my",
        "nep" => "ne", "sin" => "si", "uzb" => "uz", "amh" => "am", "mkd" => "mk", "tuk" => "tk",
        _ => return None,
    })
}

/// Source language of a text (whole song or one line), as a MyMemory-style code.
pub fn detect(text: &str) -> Option<&'static str> {
    // Kana anywhere means Japanese, even if most of the song is kanji (whatlang may say Chinese)
    if crate::romanize::japanese_context([text]) {
        return Some("ja");
    }
    // Short or mixed texts give unreliable guesses: let the provider auto-detect those
    whatlang::detect(text).filter(|i| i.is_reliable()).and_then(|i| iso1(i.lang().code()))
}

/// "zh-CN" -> "zh", "fil" -> "tl", to compare source and target.
pub fn primary(code: &str) -> String {
    let p = code.split('-').next().unwrap_or(code).to_lowercase();
    if p == "fil" { "tl".into() } else { p }
}

pub fn same_language(src: &str, target: &str) -> bool {
    primary(src) == primary(target)
}

fn has_letters(s: &str) -> bool {
    s.chars().any(char::is_alphabetic)
}

fn is_latin_only(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphabetic()).all(|c| (c as u32) < 0x0250)
}

// ------------------------------------------------------------------ disk cache

fn cache_path(s: &Settings, song_key: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("{}|{}|{}", s.provider, s.target, song_key).as_bytes());
    let name: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    crate::config::data_dir().join("cache").join("translations").join(format!("{name}.json"))
}

pub fn load_cache(s: &Settings, song_key: &str) -> Option<Translations> {
    serde_json::from_str(&std::fs::read_to_string(cache_path(s, song_key)).ok()?).ok()
}

fn save_cache(s: &Settings, song_key: &str, tr: &Translations) {
    let p = cache_path(s, song_key);
    if let Some(dir) = p.parent() { let _ = std::fs::create_dir_all(dir); }
    if let Ok(j) = serde_json::to_string_pretty(tr) { let _ = std::fs::write(p, j); }
}

// ------------------------------------------------------------------ public entry points

pub enum Outcome {
    /// Translations for the song (may be empty)
    Done(Translations),
    /// The song is already in the target language
    Same,
}

/// Translates all lines of a song (blocking: call it from a background thread).
pub fn translate_song(s: &Settings, song_key: &str, lines: &[String]) -> Result<Outcome, Error> {
    let whole = lines.join("\n");
    let src = detect(&whole);
    if src.is_some_and(|l| same_language(l, &s.target)) {
        return Ok(Outcome::Same);
    }
    if let Some(c) = load_cache(s, song_key) {
        return Ok(Outcome::Done(c));
    }
    let tr = translate_lines(s, src, lines)?;
    save_cache(s, song_key, &tr);
    Ok(Outcome::Done(tr))
}

/// Translates one line of transcribed text. None = nothing to translate (same language, no letters).
pub fn translate_text(s: &Settings, text: &str) -> Result<Option<String>, Error> {
    let src = detect(text);
    if !has_letters(text) || src.is_some_and(|l| same_language(l, &s.target)) {
        return Ok(None);
    }
    let mut tr = translate_lines(s, src, &[text.to_string()])?;
    Ok(tr.remove(text))
}

/// Distinct, non-empty lines -> translations. `src` = detected language of the whole text.
fn translate_lines(s: &Settings, src: Option<&str>, lines: &[String]) -> Result<Translations, Error> {
    let mut distinct: Vec<String> = Vec::new();
    for l in lines {
        let l = l.trim();
        if has_letters(l) && !distinct.iter().any(|d| d == l) {
            distinct.push(l.to_string());
        }
    }
    if distinct.is_empty() {
        return Ok(Translations::new());
    }
    let out = match s.provider.as_str() {
        "deepl" => deepl(s, &distinct)?,
        "libretranslate" => libre(s, &distinct)?,
        _ => mymemory(s, src, &distinct)?,
    };
    // Drop translations that just repeat the line (already in the target language)
    Ok(distinct.into_iter().zip(out)
        .filter_map(|(o, t)| t.map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && t.to_lowercase() != o.to_lowercase()).map(|t| (o, t)))
        .collect())
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(timeout))
        .user_agent(concat!("LyricsOSC/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Turns a ureq error into a short message (with the server's error text when there is one).
fn http_error(e: ureq::Error) -> (u16, String) {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body).ok()
                .and_then(|v| v.get("error").or_else(|| v.get("message")).and_then(|m| m.as_str()).map(str::to_string))
                .unwrap_or_else(|| body.chars().take(160).collect());
            (code, format!("HTTP {code}: {msg}"))
        }
        e => (0, e.to_string()),
    }
}

// ------------------------------------------------------------------ MyMemory

fn mymemory_code(ui: &str) -> &str {
    if ui == "fil" { "tl" } else { ui }
}

/// One GET per line, up to 4 at a time.
fn mymemory(s: &Settings, src: Option<&str>, lines: &[String]) -> Result<Vec<Option<String>>, Error> {
    let target = mymemory_code(&s.target).to_string();
    let src_is_latin = !src.is_some_and(|l| matches!(primary(l).as_str(), "ja" | "zh" | "ko" | "ru" | "uk" | "th" | "el" | "ar" | "he" | "hi" | "bg" | "sr" | "be" | "mk"));
    let agent = agent(15);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let results: Mutex<Vec<Option<String>>> = Mutex::new(vec![None; lines.len()]);
    let error: Mutex<Option<Error>> = Mutex::new(None);
    std::thread::scope(|sc| {
        for _ in 0..4.min(lines.len()) {
            sc.spawn(|| loop {
                if stop.load(Ordering::Relaxed) { break; }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(line) = lines.get(i) else { break };
                // A Latin-only line inside a Japanese/Chinese/… song is usually English: detect it alone
                let line_src = if !src_is_latin && is_latin_only(line) {
                    whatlang::detect(line).filter(|i| i.is_reliable()).and_then(|i| iso1(i.lang().code())).unwrap_or("en")
                } else {
                    src.unwrap_or("autodetect")
                };
                if same_language(line_src, &target) { continue; }
                let mut url = format!("https://api.mymemory.translated.net/get?q={}&langpair={}|{}",
                                      urlencoding::encode(line), line_src, target);
                if !s.mymemory_email.is_empty() {
                    url.push_str(&format!("&de={}", urlencoding::encode(&s.mymemory_email)));
                }
                match mymemory_one(&agent, &url) {
                    Ok(t) => results.lock().unwrap()[i] = t,
                    Err(e) => {
                        let fatal = e == Error::Quota || e == Error::Unsupported;
                        let mut err = error.lock().unwrap();
                        if err.is_none() || fatal { *err = Some(e); }
                        if fatal { stop.store(true, Ordering::Relaxed); }
                    }
                }
            });
        }
    });
    let results = results.into_inner().unwrap();
    match error.into_inner().unwrap() {
        // Partial results are fine for a network hiccup on one line, not when nothing worked
        Some(e) if matches!(e, Error::Quota | Error::Unsupported) || results.iter().all(Option::is_none) => Err(e),
        _ => Ok(results),
    }
}

fn mymemory_one(agent: &ureq::Agent, url: &str) -> Result<Option<String>, Error> {
    let v: Value = agent.get(url).call().map_err(|e| Error::Other(http_error(e).1))?
        .into_json().map_err(|e| Error::Other(e.to_string()))?;
    let status = match &v["responseStatus"] {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    };
    let text = v["responseData"]["translatedText"].as_str().unwrap_or("").to_string();
    let upper = text.to_uppercase();
    if v["quotaFinished"].as_bool() == Some(true) || status == 429 || upper.contains("MYMEMORY WARNING") {
        return Err(Error::Quota);
    }
    if upper.contains("INVALID TARGET LANGUAGE") {
        return Err(Error::Unsupported);
    }
    if status != 200 {
        let details = v["responseDetails"].as_str().unwrap_or(&text);
        return Err(Error::Other(format!("MyMemory {status}: {details}")));
    }
    Ok(Some(html_unescape(&text)).filter(|t| !t.is_empty()))
}

/// MyMemory sometimes returns HTML entities (&#39; &quot; …).
fn html_unescape(s: &str) -> String {
    s.replace("&#39;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

// ------------------------------------------------------------------ DeepL

fn deepl_code(ui: &str) -> Option<&'static str> {
    Some(match ui {
        "en" => "EN-US", "es" => "ES", "zh-CN" => "ZH-HANS", "zh-TW" => "ZH-HANT", "ja" => "JA", "ko" => "KO",
        "pt-BR" => "PT-BR", "fr" => "FR", "ru" => "RU", "de" => "DE", "it" => "IT", "id" => "ID",
        "tr" => "TR", "pl" => "PL", "uk" => "UK", "vi" => "VI",
        _ => return None,
    })
}

/// All lines in one request (`text` repeated).
fn deepl(s: &Settings, lines: &[String]) -> Result<Vec<Option<String>>, Error> {
    if s.deepl_key.is_empty() {
        return Err(Error::NoKey);
    }
    let target = deepl_code(&s.target).ok_or(Error::Unsupported)?;
    let url = if s.deepl_key.ends_with(":fx") { "https://api-free.deepl.com/v2/translate" } else { "https://api.deepl.com/v2/translate" };
    let mut form: Vec<(&str, &str)> = lines.iter().map(|l| ("text", l.as_str())).collect();
    form.push(("target_lang", target));
    let v: Value = agent(30).post(url)
        .set("Authorization", &format!("DeepL-Auth-Key {}", s.deepl_key))
        .send_form(&form)
        .map_err(|e| match http_error(e) {
            (456, _) => Error::Quota,
            (403, _) => Error::NoKey,
            (400, m) if m.to_lowercase().contains("target_lang") => Error::Unsupported,
            (_, m) => Error::Other(m),
        })?
        .into_json().map_err(|e| Error::Other(e.to_string()))?;
    let tr = v["translations"].as_array().ok_or_else(|| Error::Other("DeepL: unexpected response".into()))?;
    Ok(lines.iter().enumerate().map(|(i, _)| tr.get(i).and_then(|t| t["text"].as_str()).map(str::to_string)).collect())
}

// ------------------------------------------------------------------ LibreTranslate

fn libre_code(ui: &str) -> &str {
    match ui { "zh-CN" => "zh", "zh-TW" => "zt", "pt-BR" => "pt", "fil" => "tl", c => c }
}

fn libre(s: &Settings, lines: &[String]) -> Result<Vec<Option<String>>, Error> {
    let base = if s.libre_url.is_empty() { "https://libretranslate.com" } else { s.libre_url.as_str() };
    let mut body = json!({ "q": lines, "source": "auto", "target": libre_code(&s.target), "format": "text" });
    if !s.libre_key.is_empty() {
        body["api_key"] = json!(s.libre_key);
    }
    let v: Value = agent(30).post(&format!("{base}/translate")).send_json(body)
        .map_err(|e| match http_error(e) {
            (_, m) if m.to_lowercase().contains("api key") || m.to_lowercase().contains("api_key") => Error::NoKey,
            (400, m) if m.to_lowercase().contains("not supported") => Error::Unsupported,
            (_, m) => Error::Other(m),
        })?
        .into_json().map_err(|e| Error::Other(e.to_string()))?;
    let out = match &v["translatedText"] {
        Value::Array(a) => a.iter().map(|t| t.as_str().map(str::to_string)).collect(),
        Value::String(t) if lines.len() == 1 => vec![Some(t.clone())],
        _ => return Err(Error::Other("LibreTranslate: unexpected response".into())),
    };
    Ok(out)
}

// ------------------------------------------------------------------ background worker

pub enum Msg {
    /// Song translation finished. `sig` = song key + settings signature it was started with.
    Song { sig: String, result: Result<Outcome, Error> },
    /// Transcribed-text translation finished.
    Heard { text: String, sig: String, result: Result<Option<String>, Error> },
}

pub fn spawn_song(s: Settings, sig: String, song_key: String, lines: Vec<String>, tx: std::sync::mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        let result = translate_song(&s, &song_key, &lines);
        let _ = tx.send(Msg::Song { sig, result });
    });
}

pub fn spawn_text(s: Settings, sig: String, text: String, tx: std::sync::mpsc::Sender<Msg>, busy: Arc<AtomicBool>) {
    busy.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        let result = translate_text(&s, &text);
        busy.store(false, Ordering::Relaxed);
        let _ = tx.send(Msg::Heard { text, sig, result });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages() {
        assert_eq!(detect("夜に駆ける 沈むように溶けてゆくように"), Some("ja"));
        assert_eq!(detect("Hola, ¿cómo estás? Hoy es un buen día para cantar juntos"), Some("es"));
        assert!(same_language("zh-CN", "zh-TW"));
        assert!(same_language("tl", "fil"));
        assert!(!same_language("ja", "en"));
        assert_eq!(libre_code("zh-TW"), "zt");
        assert_eq!(deepl_code("pt-BR"), Some("PT-BR"));
        assert_eq!(deepl_code("fil"), None);
    }

    /// Real requests to MyMemory: `cargo test --release -- --ignored mymemory_live --nocapture`
    #[test]
    #[ignore]
    fn mymemory_live() {
        let base = Settings { provider: "mymemory".into(), target: "en".into(), deepl_key: String::new(),
                              libre_url: String::new(), libre_key: String::new(), mymemory_email: String::new() };
        let ja: Vec<String> = ["沈むように溶けてゆくように", "二人だけの空が広がる夜に"].iter().map(|s| s.to_string()).collect();
        let r = translate_lines(&base, detect(&ja.join("\n")), &ja).unwrap();
        println!("ja -> en: {r:#?}");
        assert_eq!(r.len(), 2);
        let es: Vec<String> = ["Te quiero mucho", "La noche es larga"].iter().map(|s| s.to_string()).collect();
        let zh = Settings { target: "zh-CN".into(), ..base };
        let r = translate_lines(&zh, detect(&es.join("\n")), &es).unwrap();
        println!("es -> zh-CN: {r:#?}");
        assert_eq!(r.len(), 2);
    }
}
