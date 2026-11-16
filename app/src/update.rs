//! Update check for the About tab.
//! Asks the project website first, then falls back to the GitHub releases API.
use std::sync::{Arc, Mutex};

pub const WEBSITE: &str = "https://lyricsosc.happyuky7.com";
pub const AUTHOR_SITE: &str = "https://happyuky7.com";
pub const REPO: &str = "https://github.com/Happyuky7/LyricsOSC";
pub const AXIS_SITE: &str = "https://axis.void-space.com/";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version exists (version string).
    Available(String),
    Failed,
}

pub type Shared = Arc<Mutex<Status>>;

fn get_json(url: &str) -> Option<serde_json::Value> {
    ureq::get(url)
        .set("User-Agent", concat!("LyricsOSC/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(6))
        .call().ok()?.into_json().ok()
}

/// Latest published version, e.g. "0.3.0".
/// `Ok(None)` = reachable but nothing has been released yet; `Err` = could not check.
fn latest_version() -> Result<Option<String>, ()> {
    if let Some(v) = get_json(&format!("{WEBSITE}/api/releases/latest")) {
        return Ok(v.get("version").and_then(|x| x.as_str()).map(|s| s.trim_start_matches('v').to_string()));
    }
    let res = ureq::get("https://api.github.com/repos/Happyuky7/LyricsOSC/releases/latest")
        .set("User-Agent", concat!("LyricsOSC/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(6))
        .call();
    match res {
        Ok(r) => {
            let v: serde_json::Value = r.into_json().map_err(|_| ())?;
            Ok(v.get("tag_name").and_then(|t| t.as_str()).map(|s| s.trim_start_matches('v').to_string()))
        }
        // 404 = the repository has no release yet: nothing newer than what is installed
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(_) => Err(()),
    }
}

fn parse(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect()
}

/// True if `latest` is a higher semantic version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse(latest), parse(current));
    a.resize(3, 0);
    b.resize(3, 0);
    a > b
}

/// Starts a background check and stores the result in `state`.
pub fn check(state: Shared) {
    {
        let mut s = state.lock().unwrap();
        if *s == Status::Checking { return; }
        *s = Status::Checking;
    }
    std::thread::spawn(move || {
        let result = match latest_version() {
            Ok(Some(v)) if is_newer(&v, CURRENT) => Status::Available(v),
            Ok(_) => Status::UpToDate,
            Err(()) => Status::Failed,
        };
        *state.lock().unwrap() = result;
    });
}

#[cfg(test)]
mod tests {
    use super::is_newer;
    #[test]
    fn versions() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(is_newer("0.10.0", "0.9.0"));
    }
}
