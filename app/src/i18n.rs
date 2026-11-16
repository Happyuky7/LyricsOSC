//! UI translations. Files live in `locales/*.json` (embedded at compile time).
//! To add a language: copy locales/en.json, translate it and add it to LOCALES.
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub const LOCALES: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en.json")),
    ("es", include_str!("../locales/es.json")),
    ("zh-CN", include_str!("../locales/zh-CN.json")),
    ("zh-TW", include_str!("../locales/zh-TW.json")),
    ("ja", include_str!("../locales/ja.json")),
    ("ko", include_str!("../locales/ko.json")),
    ("pt-BR", include_str!("../locales/pt-BR.json")),
    ("fr", include_str!("../locales/fr.json")),
    ("ru", include_str!("../locales/ru.json")),
    ("fil", include_str!("../locales/fil.json")),
    ("th", include_str!("../locales/th.json")),
    ("de", include_str!("../locales/de.json")),
    ("it", include_str!("../locales/it.json")),
    ("vi", include_str!("../locales/vi.json")),
    ("id", include_str!("../locales/id.json")),
    ("tr", include_str!("../locales/tr.json")),
    ("pl", include_str!("../locales/pl.json")),
    ("uk", include_str!("../locales/uk.json")),
];

type Table = HashMap<String, String>;

fn tables() -> &'static HashMap<&'static str, Table> {
    static T: OnceLock<HashMap<&'static str, Table>> = OnceLock::new();
    T.get_or_init(|| LOCALES.iter().map(|(c, j)| (*c, serde_json::from_str(j).unwrap_or_default())).collect())
}

static CURRENT: RwLock<&'static str> = RwLock::new("en");

/// Language list for the selector: (code, native name)
pub fn languages() -> Vec<(&'static str, String)> {
    LOCALES.iter().map(|(c, _)| (*c, tables()[c].get("_name").cloned().unwrap_or(c.to_string()))).collect()
}

/// Maps a system locale ("es-MX", "zh_TW", "pt_BR.UTF-8") to one of ours.
pub fn detect() -> &'static str {
    let sys = sys_locale::get_locale().unwrap_or_default().replace('_', "-").to_lowercase();
    let sys = sys.split('.').next().unwrap_or("").to_string();
    if sys.starts_with("zh") {
        return if sys.contains("tw") || sys.contains("hk") || sys.contains("hant") || sys.contains("mo") { "zh-TW" } else { "zh-CN" };
    }
    if sys.starts_with("tl") || sys.starts_with("fil") { return "fil"; }
    let primary = sys.split('-').next().unwrap_or("");
    LOCALES.iter().map(|(c, _)| *c)
        .find(|c| c.to_lowercase() == sys || c.split('-').next() == Some(primary))
        .unwrap_or("en")
}

pub fn set_language(code: &str) {
    let code = if code == "auto" { detect() } else { code };
    if let Some((c, _)) = LOCALES.iter().find(|(c, _)| *c == code) {
        *CURRENT.write().unwrap() = c;
    }
}

pub fn current() -> &'static str {
    *CURRENT.read().unwrap()
}

/// Translate a key (falls back to English, then to the key itself).
pub fn t(key: &str) -> String {
    let cur = current();
    tables().get(cur).and_then(|m| m.get(key))
        .or_else(|| tables()["en"].get(key))
        .cloned().unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_locales_complete() {
        let en = &super::tables()["en"];
        for (code, table) in super::tables() {
            for k in en.keys() {
                assert!(table.contains_key(k), "{code} missing {k}");
            }
        }
    }
}
