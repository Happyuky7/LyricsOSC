//! Resource monitor: CPU / RAM of LyricsOSC + its voice process, and GPU VRAM.
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub app_cpu: f32,
    pub app_ram_mb: f64,
    pub voice_cpu: f32,
    pub voice_ram_mb: f64,
    pub voice_vram_mb: f64,
    pub gpu_util: Option<f32>,
    pub gpu_vram_used_mb: Option<f64>,
    pub gpu_vram_total_mb: Option<f64>,
}

pub struct Monitor {
    sys: System,
    last_gpu: Instant,
    gpu: (Option<f32>, Option<f64>, Option<f64>, Vec<(u32, f64)>),
    cpus: f32,
}

fn smi(args: &[&str]) -> String {
    let mut c = Command::new("nvidia-smi");
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

impl Monitor {
    pub fn new() -> Self {
        let cpus = std::thread::available_parallelism().map(|n| n.get() as f32).unwrap_or(1.0);
        Self { sys: System::new(), last_gpu: Instant::now() - Duration::from_secs(60), gpu: (None, None, None, vec![]), cpus }
    }

    /// Call ~once per second. `voice_pid` = Python process, if running.
    pub fn sample(&mut self, voice_pid: Option<u32>, gpu_index: u32) -> Usage {
        let me = Pid::from_u32(std::process::id());
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true,
            ProcessRefreshKind::new().with_cpu().with_memory());
        let mut u = Usage::default();
        if let Some(p) = self.sys.process(me) {
            u.app_cpu = p.cpu_usage() / self.cpus;
            u.app_ram_mb = p.memory() as f64 / 1_048_576.0;
        }
        // voice process + its children (the venv launcher spawns the real python)
        let mut voice_pids: Vec<u32> = Vec::new();
        if let Some(v) = voice_pid {
            voice_pids.push(v);
            for (pid, p) in self.sys.processes() {
                if p.parent().map(|pp| pp.as_u32()) == Some(v) {
                    voice_pids.push(pid.as_u32());
                }
            }
            for vp in &voice_pids {
                if let Some(p) = self.sys.process(Pid::from_u32(*vp)) {
                    u.voice_cpu += p.cpu_usage() / self.cpus;
                    u.voice_ram_mb += p.memory() as f64 / 1_048_576.0;
                }
            }
        }
        // GPU every 3 s (nvidia-smi is slow-ish)
        if self.last_gpu.elapsed() > Duration::from_secs(3) {
            self.last_gpu = Instant::now();
            let id = format!("--id={gpu_index}");
            let line = smi(&[&id, "--query-gpu=utilization.gpu,memory.used,memory.total", "--format=csv,noheader,nounits"]);
            let p: Vec<f64> = line.trim().split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let apps = smi(&[&id, "--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"]);
            let apps: Vec<(u32, f64)> = apps.lines().filter_map(|l| {
                let mut it = l.split(',').map(str::trim);
                Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
            }).collect();
            self.gpu = (p.first().map(|x| *x as f32), p.get(1).copied(), p.get(2).copied(), apps);
        }
        u.gpu_util = self.gpu.0;
        u.gpu_vram_used_mb = self.gpu.1;
        u.gpu_vram_total_mb = self.gpu.2;
        u.voice_vram_mb = self.gpu.3.iter().filter(|(pid, _)| voice_pids.contains(pid)).map(|(_, m)| m).sum();
        u
    }
}
