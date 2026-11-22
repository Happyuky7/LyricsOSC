//! Updates: check the latest GitHub release, download the right package for this install,
//! verify its SHA-256 against the release's SHA256SUMS.txt and apply it.
//!
//!   Windows installer  → run the new setup.exe silently over the same folder (/UPDATE), it relaunches the app
//!   Windows portable   → a helper waits for the app to exit, extracts the .zip over the folder, relaunches
//!   Linux              → same with the .tar.gz
//!
//! The app never restarts by itself: applying an update is always a user action.
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const WEBSITE: &str = "https://lyricsosc.happyuky7.com";
pub const AUTHOR_SITE: &str = "https://happyuky7.com";
pub const REPO: &str = "https://github.com/Happyuky7/LyricsOSC";
pub const AXIS_SITE: &str = "https://axis.void-space.com/";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const API_LATEST: &str = "https://api.github.com/repos/Happyuky7/LyricsOSC/releases/latest";
const UA: &str = concat!("LyricsOSC/", env!("CARGO_PKG_VERSION"));

/// How this copy of LyricsOSC was installed (decides which package to download).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InstallKind {
    WindowsInstaller,
    WindowsPortable,
    Linux,
}

impl InstallKind {
    pub fn detect() -> Self {
        if cfg!(target_os = "linux") {
            InstallKind::Linux
        } else if crate::config::base_dir().join("unins000.exe").exists() {
            InstallKind::WindowsInstaller
        } else {
            InstallKind::WindowsPortable
        }
    }
    fn asset_suffix(self) -> &'static str {
        match self {
            InstallKind::WindowsInstaller => "-setup.exe",
            InstallKind::WindowsPortable => "-windows-portable.zip",
            InstallKind::Linux => "-linux-x86_64.tar.gz",
        }
    }
    fn website_kind(self) -> &'static str {
        match self {
            InstallKind::WindowsInstaller => "windows-setup",
            InstallKind::WindowsPortable => "windows-portable",
            InstallKind::Linux => "linux",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub asset_name: String,
    pub url: String,
    pub size: u64,
    /// Expected SHA-256 (hex) from SHA256SUMS.txt, when the release has one.
    pub sha256: Option<String>,
    pub notes_url: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version exists.
    Available(Release),
    Downloading { release: Release, done: u64, total: u64 },
    /// Downloaded and verified, ready to install.
    Ready { release: Release, file: PathBuf },
    Installing,
    /// Error message (already human readable, may be an i18n key prefixed with "@").
    Failed(String),
}

pub type Shared = Arc<Mutex<Status>>;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(8)).build()
}

/// Latest release with the package for `kind`. `Ok(None)` = no release (or nothing newer is published).
fn latest(kind: InstallKind) -> Result<Option<Release>, String> {
    let res = agent().get(API_LATEST).set("User-Agent", UA).set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10)).call();
    match res {
        Ok(r) => {
            let v: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
            let version = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
            let assets = v["assets"].as_array().cloned().unwrap_or_default();
            let find = |pred: &dyn Fn(&str) -> bool| assets.iter().find(|a| pred(a["name"].as_str().unwrap_or("")));
            let Some(asset) = find(&|n| n.ends_with(kind.asset_suffix())) else {
                return Err(format!("the release has no {} package", kind.asset_suffix()));
            };
            let name = asset["name"].as_str().unwrap_or("").to_string();
            let sha256 = find(&|n| n.eq_ignore_ascii_case("SHA256SUMS.txt"))
                .and_then(|s| s["browser_download_url"].as_str())
                .and_then(|u| agent().get(u).set("User-Agent", UA).call().ok()?.into_string().ok())
                .and_then(|txt| parse_sums(&txt, &name));
            Ok(Some(Release {
                version,
                url: asset["browser_download_url"].as_str().unwrap_or("").to_string(),
                size: asset["size"].as_u64().unwrap_or(0),
                asset_name: name,
                sha256,
                notes_url: v["html_url"].as_str().unwrap_or(REPO).to_string(),
            }))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => {
            // GitHub unreachable or rate-limited: ask the project website (same files, streamed by it)
            let v: serde_json::Value = agent().get(&format!("{WEBSITE}/api/releases/latest")).set("User-Agent", UA)
                .call().map_err(|_| e.to_string())?.into_json().map_err(|x| x.to_string())?;
            let Some(version) = v["version"].as_str() else { return Ok(None) };
            let asset = v["assets"].as_array().and_then(|a| a.iter().find(|x| x["kind"] == kind.website_kind()).cloned());
            let Some(asset) = asset else { return Err(format!("the release has no {} package", kind.asset_suffix())) };
            Ok(Some(Release {
                version: version.to_string(),
                asset_name: asset["name"].as_str().unwrap_or("update").to_string(),
                url: format!("{WEBSITE}/api/download/{}", kind.website_kind()),
                size: asset["size"].as_u64().unwrap_or(0),
                sha256: None,
                notes_url: format!("{WEBSITE}/download"),
            }))
        }
    }
}

/// SHA-256 for `name` from a "hash  name" / "hash *name" checksum file.
fn parse_sums(txt: &str, name: &str) -> Option<String> {
    txt.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64).then(|| hash.to_lowercase())
    })
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

/// Background check. With `auto_download`, a newer version is downloaded right away
/// (it is still only installed when the user clicks "Restart and update").
pub fn check(state: Shared, auto_download: bool) {
    {
        let mut s = state.lock().unwrap();
        if matches!(*s, Status::Checking | Status::Downloading { .. } | Status::Installing) { return; }
        *s = Status::Checking;
    }
    std::thread::spawn(move || {
        let result = match latest(InstallKind::detect()) {
            Ok(Some(r)) if is_newer(&r.version, CURRENT) => Status::Available(r),
            Ok(_) => Status::UpToDate,
            Err(e) => Status::Failed(e),
        };
        let start = matches!(result, Status::Available(_)) && auto_download;
        *state.lock().unwrap() = result;
        if start { download(state); }
    });
}

/// Downloads the available release (Status::Available → Downloading → Ready / Failed).
pub fn download(state: Shared) {
    let release = match &*state.lock().unwrap() {
        Status::Available(r) => r.clone(),
        _ => return,
    };
    std::thread::spawn(move || {
        let set = |s: Status| *state.lock().unwrap() = s;
        set(Status::Downloading { release: release.clone(), done: 0, total: release.size });
        match fetch(&release, &state) {
            Ok(file) => set(Status::Ready { release, file }),
            Err(e) => set(Status::Failed(e)),
        }
    });
}

fn fetch(release: &Release, state: &Shared) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("lyricsosc-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // only the file name part, never a path from the server
    let safe_name: String = release.asset_name.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).collect();
    let path = dir.join(if safe_name.is_empty() { "update.bin".into() } else { safe_name });
    let resp = agent().get(&release.url).set("User-Agent", UA).call().map_err(|e| e.to_string())?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(release.size);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let (mut buf, mut done) = (vec![0u8; 64 * 1024], 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if let Status::Downloading { done: d, total: t, .. } = &mut *state.lock().unwrap() {
            *d = done;
            *t = total;
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && done != total {
        return Err(format!("incomplete download ({done} of {total} bytes)"));
    }
    if let Some(expected) = &release.sha256 {
        let got = format!("{:x}", hasher.finalize());
        if &got != expected {
            let _ = std::fs::remove_file(&path);
            return Err("@update_bad_checksum".into());
        }
    }
    Ok(path)
}

/// Installs a downloaded update and exits the app. Returns an error message if it could not start.
pub fn apply(state: &Shared) -> Result<(), String> {
    let file = match &*state.lock().unwrap() {
        Status::Ready { file, .. } => file.clone(),
        _ => return Err("no update ready".into()),
    };
    *state.lock().unwrap() = Status::Installing;
    let dir = crate::config::base_dir();
    let pid = std::process::id();
    match InstallKind::detect() {
        InstallKind::WindowsInstaller => {
            // Inno Setup: silent update over the same folder; /UPDATE skips the voice-engine
            // setup (already installed) and relaunches LyricsOSC at the end.
            let mut c = std::process::Command::new(&file);
            c.args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/UPDATE"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.raw_arg(format!("/DIR=\"{}\"", dir.display()));
            }
            c.spawn().map_err(|e| e.to_string())?;
        }
        InstallKind::WindowsPortable => {
            let script = file.with_extension("ps1");
            let exe = dir.join("LyricsOSC.exe");
            let ps = portable_script(pid, &file, &dir, &exe);
            std::fs::write(&script, ps).map_err(|e| e.to_string())?;
            let mut c = std::process::Command::new("powershell.exe");
            c.env_remove("PSModulePath");
            c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"]).arg(&script);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            c.spawn().map_err(|e| e.to_string())?;
        }
        InstallKind::Linux => {
            let script = file.with_extension("sh");
            let sh = format!(
                "#!/bin/sh\nwhile kill -0 {pid} 2>/dev/null; do sleep 0.5; done\n\
                 tar -xzf {tgz} -C {dir} --strip-components=1 && chmod +x {dir}/lyricsosc\n\
                 cd {dir} && nohup ./lyricsosc >/dev/null 2>&1 &\n",
                tgz = sh_quote(&file), dir = sh_quote(&dir));
            std::fs::write(&script, sh).map_err(|e| e.to_string())?;
            let launcher = if std::path::Path::new("/usr/bin/setsid").exists() { "setsid" } else { "sh" };
            let mut c = std::process::Command::new(launcher);
            if launcher == "setsid" { c.arg("sh"); }
            c.arg(&script).spawn().map_err(|e| e.to_string())?;
        }
    }
    // Give the helper a moment to start, then quit so the files can be replaced.
    std::thread::sleep(std::time::Duration::from_millis(400));
    std::process::exit(0);
}

/// PowerShell helper for portable installs: waits for the app to exit, extracts the .zip over the
/// folder (with .NET directly: Expand-Archive breaks when Windows PowerShell inherits PowerShell 7's
/// module path), retries while the old exe is still locked, then starts the new version.
fn portable_script(pid: u32, file: &std::path::Path, dir: &std::path::Path, exe: &std::path::Path) -> String {
    format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue\r\n\
         Add-Type -AssemblyName System.IO.Compression.FileSystem\r\n\
         $dest = '{dir}'\r\n\
         for ($try = 0; $try -lt 20; $try++) {{\r\n\
         \x20 try {{\r\n\
         \x20   $zip = [System.IO.Compression.ZipFile]::OpenRead('{zip}')\r\n\
         \x20   try {{\r\n\
         \x20     foreach ($e in $zip.Entries) {{\r\n\
         \x20       $target = [System.IO.Path]::GetFullPath([System.IO.Path]::Combine($dest, $e.FullName))\r\n\
         \x20       if (-not $target.StartsWith([System.IO.Path]::GetFullPath($dest))) {{ continue }}\r\n\
         \x20       if ($e.Name -eq '') {{ [System.IO.Directory]::CreateDirectory($target) | Out-Null; continue }}\r\n\
         \x20       [System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($target)) | Out-Null\r\n\
         \x20       [System.IO.Compression.ZipFileExtensions]::ExtractToFile($e, $target, $true)\r\n\
         \x20     }}\r\n\
         \x20   }} finally {{ $zip.Dispose() }}\r\n\
         \x20   break\r\n\
         \x20 }} catch {{ Start-Sleep -Milliseconds 500 }}\r\n\
         }}\r\n\
         Start-Process -FilePath '{exe}' -WorkingDirectory '{dir}'\r\n",
        zip = ps_quote(file), dir = ps_quote(dir), exe = ps_quote(exe))
}

fn ps_quote(p: &std::path::Path) -> String {
    p.display().to_string().replace('\'', "''")
}
fn sh_quote(p: &std::path::Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(is_newer("0.10.0", "0.9.0"));
    }
    /// Network test against the real latest release: `cargo test --release -- --ignored update_live`
    #[test]
    #[ignore]
    fn update_live() {
        for kind in [InstallKind::WindowsInstaller, InstallKind::WindowsPortable, InstallKind::Linux] {
            let r = latest(kind).expect("lookup failed").expect("no release");
            println!("{kind:?}: v{} {} {} bytes sha256={:?}", r.version, r.asset_name, r.size, r.sha256);
            assert!(r.sha256.is_some(), "SHA256SUMS.txt should list {}", r.asset_name);
            let state: Shared = Arc::new(Mutex::new(Status::Available(r.clone())));
            let file = fetch(&r, &state).expect("download/verify failed");
            assert_eq!(std::fs::metadata(&file).unwrap().len(), r.size);
            println!("  downloaded and verified -> {}", file.display());
        }
    }

    /// Writes the real portable helper script for a fake app (pid from FAKE_APP_PID) so it can be
    /// run by hand: `cargo test --release -- --ignored portable_script --nocapture`
    #[test]
    #[ignore]
    fn portable_script_file() {
        let tmp = std::env::temp_dir();
        let zip = tmp.join("lyricsosc-update").join("LyricsOSC-1.0.0-windows-portable.zip");
        let dir = tmp.join("LyricsOSC portable test5");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("LyricsOSC.exe"), b"old build").unwrap();
        let pid: u32 = std::env::var("FAKE_APP_PID").ok().and_then(|p| p.parse().ok()).unwrap_or(999_999);
        let script = tmp.join("lyricsosc-update").join("portable-test5.ps1");
        std::fs::write(&script, portable_script(pid, &zip, &dir, &dir.join("LyricsOSC.exe"))).unwrap();
        println!("SCRIPT={}", script.display());
    }

    #[test]
    fn sums() {
        let txt = "8fc2c424e5578fdd63c8b9e372cce680c443f7e95a2f971e8582e62b595ee2da  LyricsOSC-1.0.0-linux-x86_64.tar.gz\n\
                   f1375a722949d3b261cf775de7b2134592aab5360294b3d54d28b04c7f5a20dd *LyricsOSC-1.0.0-setup.exe\n";
        assert_eq!(parse_sums(txt, "LyricsOSC-1.0.0-setup.exe").as_deref(),
                   Some("f1375a722949d3b261cf775de7b2134592aab5360294b3d54d28b04c7f5a20dd"));
        assert_eq!(parse_sums(txt, "nope.zip"), None);
    }
}
