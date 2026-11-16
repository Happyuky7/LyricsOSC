//! Bridge to the Python voice process (faster-whisper).
use crate::audio;
use crate::config::Config;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct Gpu {
    pub index: u32,
    pub name: String,
    pub memory_mb: u64,
}

fn no_window(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = c;
}

fn python_cmd(cfg: &Config) -> Command {
    let py = crate::config::resolve(&cfg.python);
    let exe: std::ffi::OsString = if py.exists() { py.into() } else if cfg!(windows) { "python".into() } else { "python3".into() };
    let mut c = Command::new(exe);
    c.env("PYTHONIOENCODING", "utf-8");
    // Whisper models downloaded by the installer live in <install>/models
    let models = crate::config::base_dir().join("models");
    if models.exists() {
        c.env("HF_HOME", models);
    }
    // Linux: pip-installed CUDA libs must be on LD_LIBRARY_PATH before Python starts
    #[cfg(target_os = "linux")]
    {
        let venv = crate::config::resolve("python/.venv/lib");
        let mut dirs: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&venv) {
            for py in rd.flatten() {
                if let Ok(nv) = std::fs::read_dir(py.path().join("site-packages/nvidia")) {
                    for lib in nv.flatten() {
                        let p = lib.path().join("lib");
                        if p.exists() { dirs.push(p.display().to_string()); }
                    }
                }
            }
        }
        if !dirs.is_empty() {
            let old = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
            c.env("LD_LIBRARY_PATH", format!("{}:{old}", dirs.join(":")));
        }
    }
    no_window(&mut c);
    c
}

fn script() -> std::path::PathBuf {
    crate::config::resolve("python/whisper_sidecar.py")
}

/// Is the Python voice environment installed?
pub fn python_ready(cfg: &Config) -> bool {
    crate::config::resolve(&cfg.python).exists()
}

/// NVIDIA GPUs (via nvidia-smi; works without Python).
pub fn list_gpus() -> Vec<Gpu> {
    let mut c = Command::new("nvidia-smi");
    c.args(["--query-gpu=index,name,memory.total", "--format=csv,noheader,nounits"]);
    no_window(&mut c);
    let Ok(o) = c.output() else { return vec![] };
    String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| {
        let p: Vec<&str> = l.split(',').map(str::trim).collect();
        Some(Gpu { index: p.first()?.parse().ok()?, name: p.get(1)?.to_string(),
                   memory_mb: p.get(2).and_then(|m| m.parse().ok()).unwrap_or(0) })
    }).collect()
}

/// Runs the voice-engine setup script (Python + faster-whisper + model, CUDA if NVIDIA)
/// in a visible console so the user sees the progress. Logs when it finishes.
pub fn install_voice_engine(log: Sender<String>, done: Arc<AtomicBool>) {
    let dir = crate::config::base_dir();
    std::thread::spawn(move || {
        let _ = log.send("[voice] installing the voice engine… (see the console window)".into());
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::CommandExt;
            Command::new("powershell.exe")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(dir.join("scripts").join("setup_voice.ps1"))
                .arg("-InstallDir").arg(&dir)
                .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
                .status()
        };
        #[cfg(not(windows))]
        let status = Command::new("bash").arg(dir.join("scripts").join("setup_voice.sh")).arg(&dir).status();
        match status {
            Ok(s) if s.success() => { let _ = log.send("[voice] ✔ voice engine installed".into()); }
            Ok(s) => { let _ = log.send(format!("[voice] ✖ setup failed ({s})")); }
            Err(e) => { let _ = log.send(format!("[voice] ✖ {e}")); }
        }
        done.store(true, Ordering::Relaxed);
    });
}

/// Installs the CUDA runtime libs (cuBLAS + cuDNN) into the voice venv.
pub fn install_gpu_support(cfg: &Config, log: Sender<String>) {
    let mut c = python_cmd(cfg);
    c.args(["-m", "pip", "install", "nvidia-cublas-cu12", "nvidia-cudnn-cu12==9.*"])
        .stdout(Stdio::piped()).stderr(Stdio::piped());
    std::thread::spawn(move || {
        let _ = log.send("[gpu] installing CUDA libraries (~1 GB)…".into());
        match c.output() {
            Ok(o) if o.status.success() => { let _ = log.send("[gpu] ✔ CUDA libraries installed".into()); }
            Ok(o) => { let _ = log.send(format!("[gpu] ✖ {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or(""))); }
            Err(e) => { let _ = log.send(format!("[gpu] ✖ {e}")); }
        }
    });
}

/// Puts the child in a Job that Windows kills when LyricsOSC exits (even if killed),
/// so no orphan Python processes are left behind.
#[cfg(windows)]
fn kill_with_parent(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::*;
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let Ok(job) = CreateJobObjectW(None, None) else { return 0 };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let _ = SetInformationJobObject(job, JobObjectExtendedLimitInformation,
            &info as *const _ as *const _, std::mem::size_of_val(&info) as u32);
        job.0 as usize
    });
    if job != 0 {
        unsafe {
            let _ = AssignProcessToJobObject(HANDLE(job as _), HANDLE(child.as_raw_handle() as _));
        }
    }
}
#[cfg(not(windows))]
fn kill_with_parent(_: &Child) {}

pub struct Transcriber {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    capture_stop: Arc<AtomicBool>,
    pub source: String,
    /// Settings the process was started with (restart if they change)
    pub signature: String,
    log: Sender<String>,
}

pub fn signature(cfg: &Config) -> String {
    format!("{}|{}|{}|{}|{}|{}|{}", cfg.whisper_model, cfg.whisper_device, cfg.gpu_index,
            cfg.compute_type, cfg.beam_size, cfg.vad, cfg.languages.join(","))
}

impl Transcriber {
    /// Starts Python (model loaded once). Text arrives through `on_text`.
    pub fn start(cfg: &Config, on_text: Sender<String>, log: Sender<String>) -> Result<Self, String> {
        let mut cmd = python_cmd(cfg);
        cmd.arg("-u").arg(script())
            .args(["--model", &cfg.whisper_model, "--device", &cfg.whisper_device,
                   "--gpu", &cfg.gpu_index.to_string(), "--compute-type", &cfg.compute_type,
                   "--beam", &cfg.beam_size.to_string(), "--languages", &cfg.languages.join(",")]);
        if cfg.vad {
            cmd.arg("--vad");
        }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|e| format!("Python: {e}"))?;
        kill_with_parent(&child);

        let stdout = child.stdout.take().unwrap();
        let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
        let log2 = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                    let _ = on_text.send(t.to_string());
                } else if v.get("ready").is_some() {
                    let dev = v.get("device").and_then(|d| d.as_str()).unwrap_or("?");
                    let _ = log2.send(format!("[voice] Whisper ready ({dev})"));
                } else if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
                    let _ = log2.send(format!("[voice] {e}"));
                }
            }
        });
        let _ = log.send(format!("[voice] loading Whisper {}…", cfg.whisper_model));
        Ok(Self { child, stdin, capture_stop: Arc::new(AtomicBool::new(true)), source: String::new(),
                  signature: signature(cfg), log })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn capturing(&self) -> bool {
        !self.capture_stop.load(Ordering::Relaxed)
    }

    /// Starts capturing `source` (switches if it was capturing something else).
    pub fn start_capture(&mut self, source: &str, chunk_secs: f32) {
        if self.capturing() && self.source == source {
            return;
        }
        self.stop_capture();
        let stop = Arc::new(AtomicBool::new(false));
        let (atx, arx) = channel::<Vec<f32>>();
        audio::spawn_capture(source.to_string(), chunk_secs, atx, stop.clone(), self.log.clone());
        let w = self.stdin.clone();
        std::thread::spawn(move || {
            for block in arx {
                let mut buf = Vec::with_capacity(4 + block.len() * 4);
                buf.extend_from_slice(&(block.len() as u32).to_le_bytes());
                for s in block {
                    buf.extend_from_slice(&s.to_le_bytes());
                }
                let mut w = w.lock().unwrap();
                if w.write_all(&buf).and_then(|_| w.flush()).is_err() {
                    break;
                }
            }
        });
        let _ = self.log.send(format!("[voice] listening: {source}"));
        self.capture_stop = stop;
        self.source = source.to_string();
    }

    pub fn stop_capture(&mut self) {
        self.capture_stop.store(true, Ordering::Relaxed);
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// New song: let Python detect the language again.
    pub fn reset_language(&self) {
        if let Ok(mut w) = self.stdin.lock() {
            let _ = w.write_all(&0u32.to_le_bytes()).and_then(|_| w.flush());
        }
    }
}

impl Drop for Transcriber {
    fn drop(&mut self) {
        self.capture_stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
    }
}
