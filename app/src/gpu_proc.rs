//! Per-process GPU usage (utilization % and dedicated VRAM).
//!
//! Windows: the "GPU Engine" / "GPU Process Memory" performance counters, the same source Task
//! Manager uses, so it works for any vendor (NVIDIA, AMD, Intel).
//! Linux: `nvidia-smi` (NVIDIA only).
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcGpu {
    /// Busiest engine type (3D, Compute, Copy…), in %, like Task Manager's "GPU" column
    pub util: f32,
    pub vram_mb: f64,
}

/// Instance names look like `pid_1234_luid_0x…_phys_0_eng_0_engtype_3D`.
fn parse_pid(name: &str) -> Option<u32> {
    name.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

fn engtype(name: &str) -> &str {
    name.rsplit_once("engtype_").map_or("", |(_, t)| t)
}

/// Sums per process and engine type, then keeps each process's busiest engine type.
fn fold(util: &[(String, f64)], mem: &[(String, f64)]) -> HashMap<u32, ProcGpu> {
    let mut eng: HashMap<(u32, String), f64> = HashMap::new();
    for (name, v) in util {
        if let Some(pid) = parse_pid(name) {
            *eng.entry((pid, engtype(name).to_string())).or_default() += v;
        }
    }
    let mut out: HashMap<u32, ProcGpu> = HashMap::new();
    for ((pid, _), v) in eng {
        let e = out.entry(pid).or_default();
        e.util = e.util.max(v.min(100.0) as f32);
    }
    for (name, bytes) in mem {
        if let Some(pid) = parse_pid(name) {
            out.entry(pid).or_default().vram_mb += bytes / 1_048_576.0;
        }
    }
    out
}

#[cfg(windows)]
pub struct GpuProcs {
    query: isize,
    util: isize,
    mem: isize,
}

#[cfg(windows)]
impl GpuProcs {
    pub fn new() -> Option<Self> {
        use windows::core::w;
        use windows::Win32::System::Performance::*;
        unsafe {
            let mut query = 0isize;
            if PdhOpenQueryW(None, 0, &mut query) != 0 {
                return None;
            }
            let (mut util, mut mem) = (0isize, 0isize);
            if PdhAddEnglishCounterW(query, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut util) != 0
                || PdhAddEnglishCounterW(query, w!("\\GPU Process Memory(*)\\Dedicated Usage"), 0, &mut mem) != 0
            {
                PdhCloseQuery(query);
                return None;
            }
            PdhCollectQueryData(query); // rate counters need a first sample
            Some(Self { query, util, mem })
        }
    }

    fn array(counter: isize, fmt: u32) -> Vec<(String, f64)> {
        use windows::Win32::System::Performance::*;
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            let st = PdhGetFormattedCounterArrayW(counter, PDH_FMT(fmt), &mut size, &mut count, None);
            if st != PDH_MORE_DATA || size == 0 {
                return vec![];
            }
            // u64 storage keeps the buffer aligned for the item structs
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(counter, PDH_FMT(fmt), &mut size, &mut count, Some(items)) != 0 {
                return vec![];
            }
            std::slice::from_raw_parts(items, count as usize).iter()
                .filter(|i| i.FmtValue.CStatus == 0)
                .map(|i| (i.szName.to_string().unwrap_or_default(), i.FmtValue.Anonymous.doubleValue))
                .collect()
        }
    }

    /// Call every second or two; utilization is the average since the previous call.
    pub fn sample(&mut self) -> HashMap<u32, ProcGpu> {
        unsafe { windows::Win32::System::Performance::PdhCollectQueryData(self.query); }
        const DOUBLE_NOCAP: u32 = 0x200 | 0x8000; // PDH_FMT_DOUBLE | PDH_FMT_NOCAP100
        fold(&Self::array(self.util, DOUBLE_NOCAP), &Self::array(self.mem, 0x200))
    }
}

#[cfg(windows)]
impl Drop for GpuProcs {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Performance::PdhCloseQuery(self.query); }
    }
}

#[cfg(not(windows))]
pub struct GpuProcs;

#[cfg(not(windows))]
impl GpuProcs {
    pub fn new() -> Option<Self> {
        Some(Self)
    }

    pub fn sample(&mut self) -> HashMap<u32, ProcGpu> {
        let mut out: HashMap<u32, ProcGpu> = HashMap::new();
        // `pmon` columns: gpu pid type sm mem enc dec … ("-" when idle)
        for l in crate::monitor::smi(&["pmon", "-c", "1", "-s", "u"]).lines().filter(|l| !l.starts_with('#')) {
            let c: Vec<&str> = l.split_whitespace().collect();
            if let (Some(pid), Some(sm)) = (c.get(1).and_then(|p| p.parse().ok()), c.get(3)) {
                let e = out.entry(pid).or_default();
                e.util = e.util.max(sm.parse().unwrap_or(0.0));
            }
        }
        let apps = crate::monitor::smi(&["--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"]);
        for l in apps.lines() {
            let mut it = l.split(',').map(str::trim);
            if let (Some(Ok(pid)), Some(Ok(m))) = (it.next().map(str::parse), it.next().map(str::parse::<f64>)) {
                out.entry(pid).or_default().vram_mb += m;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_engines() {
        let util = vec![
            ("pid_10_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(), 20.0),
            ("pid_10_luid_0x0_0x1_phys_0_eng_1_engtype_3D".to_string(), 15.0),
            ("pid_10_luid_0x0_0x1_phys_0_eng_2_engtype_Compute_0".to_string(), 30.0),
            ("pid_20_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(), 5.0),
        ];
        let mem = vec![("pid_10_luid_0x0_0x1_phys_0".to_string(), 512.0 * 1_048_576.0)];
        let m = fold(&util, &mem);
        assert_eq!(m[&10].util, 35.0);
        assert_eq!(m[&10].vram_mb, 512.0);
        assert_eq!(m[&20].util, 5.0);
    }

    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live() {
        let mut g = GpuProcs::new().expect("pdh");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut v: Vec<_> = g.sample().into_iter().collect();
        v.sort_by(|a, b| b.1.vram_mb.total_cmp(&a.1.vram_mb));
        for (pid, p) in v.iter().take(10) {
            println!("{pid}: {:.1}% {:.0} MB", p.util, p.vram_mb);
        }
        assert!(!v.is_empty());
    }
}
