//! Windows audio capture (16 kHz mono f32) with WASAPI:
//!   "default"     -> whatever the default output plays (loopback)
//!   "app:<exe>"   -> a single application, like OBS (Windows 10 2004+)
//!   "out:<name>"  -> loopback of a specific output (headphones, Voicemeeter...)
//!   "in:<name>"   -> an input (microphone, VB-Cable Output...)
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use wasapi::*;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::*;

use super::{AudioOption, RATE};
use crate::i18n::t;

/// pid -> (exe name, parent pid)
fn processes() -> HashMap<u32, (String, u32)> {
    let mut out = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.insert(e.th32ProcessID, (String::from_utf16_lossy(&e.szExeFile[..n]), e.th32ParentProcessID));
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Walks up to the parent with the same exe (browsers play audio in child processes).
fn root_pid(mut pid: u32, procs: &HashMap<u32, (String, u32)>) -> u32 {
    let name = procs.get(&pid).map(|p| p.0.to_lowercase()).unwrap_or_default();
    let mut seen = HashSet::new();
    while seen.insert(pid) {
        match procs.get(&pid).and_then(|(_, pp)| procs.get(pp).map(|p| (*pp, p))) {
            Some((pp, (n, _))) if n.to_lowercase() == name => pid = pp,
            _ => break,
        }
    }
    pid
}

/// (exe, root pid, playing) of the apps that have an audio session.
fn audio_apps() -> Vec<(String, u32, bool)> {
    let _ = initialize_mta();
    let procs = processes();
    let mut apps: HashMap<u32, (String, bool)> = HashMap::new();
    let Ok(en) = DeviceEnumerator::new() else { return vec![] };
    let Ok(coll) = en.get_device_collection(Direction::Render) else { return vec![] };
    for dev in &coll {
        let Ok(dev) = dev else { continue };
        let Ok(mgr) = dev.get_iaudiosessionmanager() else { continue };
        let Ok(ss) = mgr.get_audiosessionenumerator() else { continue };
        for i in 0..ss.get_count().unwrap_or(0) {
            let Ok(c) = ss.get_session(i) else { continue };
            let Ok(pid) = c.get_process_id() else { continue };
            if pid == 0 { continue }
            let active = matches!(c.get_state(), Ok(SessionState::Active));
            let root = root_pid(pid, &procs);
            let name = procs.get(&root).map(|p| p.0.clone()).unwrap_or_else(|| format!("pid {root}"));
            let e = apps.entry(root).or_insert((name, false));
            e.1 |= active;
        }
    }
    let mut v: Vec<_> = apps.into_iter().map(|(pid, (n, a))| (n, pid, a)).collect();
    v.sort_by_key(|(n, _, a)| (!a, n.to_lowercase()));
    v
}

pub fn list_options() -> Vec<AudioOption> {
    let _ = initialize_mta();
    let mut v = vec![AudioOption { id: "default".into(), label: format!("🔊 {}", t("src_default")) }];
    for (exe, _, active) in audio_apps() {
        v.push(AudioOption { id: format!("app:{exe}"), label: format!("🎯 {}: {exe}{}", t("src_app"), if active { "  ▶" } else { "" }) });
    }
    if let Ok(en) = DeviceEnumerator::new() {
        for (dir, prefix, icon) in [(Direction::Render, "out", format!("🎧 {}", t("src_out"))), (Direction::Capture, "in", format!("🎤 {}", t("src_in")))] {
            if let Ok(coll) = en.get_device_collection(dir) {
                for d in &coll {
                    if let Ok(name) = d.and_then(|d| d.get_friendlyname()) {
                        v.push(AudioOption { id: format!("{prefix}:{name}"), label: format!("{icon}: {name}") });
                    }
                }
            }
        }
    }
    v
}

fn find_device(dir: Direction, name: &str) -> Option<Device> {
    let en = DeviceEnumerator::new().ok()?;
    let coll = en.get_device_collection(dir).ok()?;
    for d in &coll {
        if let Ok(d) = d {
            if d.get_friendlyname().ok().as_deref() == Some(name) {
                return Some(d);
            }
        }
    }
    None
}

fn open_client(source: &str) -> Result<AudioClient, String> {
    let e = |x: WasapiError| x.to_string();
    if let Some(exe) = source.strip_prefix("app:") {
        let (_, pid, _) = audio_apps().into_iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(exe))
            .ok_or_else(|| format!("{exe} is not playing audio"))?;
        return AudioClient::new_application_loopback_client(pid, true).map_err(e);
    }
    let en = DeviceEnumerator::new().map_err(e)?;
    let dev = if let Some(n) = source.strip_prefix("out:") {
        find_device(Direction::Render, n).ok_or(format!("output not found: {n}"))?
    } else if let Some(n) = source.strip_prefix("in:") {
        find_device(Direction::Capture, n).ok_or(format!("input not found: {n}"))?
    } else {
        en.get_default_device(Direction::Render).map_err(e)?
    };
    dev.get_iaudioclient().map_err(e)
}

/// Captures on a thread, sending blocks of `chunk_secs` seconds. Stops when `stop` is set.
pub fn spawn_capture(source: String, chunk_secs: f32, tx: Sender<Vec<f32>>,
                     stop: Arc<AtomicBool>, log: Sender<String>) {
    std::thread::spawn(move || {
        if let Err(err) = capture_loop(&source, chunk_secs, &tx, &stop) {
            let _ = log.send(format!("[audio] {err}"));
        }
    });
}

fn capture_loop(source: &str, chunk_secs: f32, tx: &Sender<Vec<f32>>, stop: &AtomicBool) -> Result<(), String> {
    let _ = initialize_mta();
    let e = |x: WasapiError| x.to_string();
    let fmt = WaveFormat::new(32, 32, SampleType::Float, RATE, 1, None);
    let mut client = open_client(source)?;
    client.initialize_client(&fmt, Direction::Capture,
        StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 }, None).map_err(e)?;
    let event = client.set_get_eventhandle().map_err(e)?;
    let cap = client.get_audiocaptureclient().map_err(e)?;
    let mut q: VecDeque<u8> = VecDeque::new();
    let mut acc: Vec<f32> = Vec::new();
    let chunk = (chunk_secs.max(1.0) * RATE as f32) as usize;
    client.start_stream().map_err(e)?;
    while !stop.load(Ordering::Relaxed) {
        let _ = event.wait_for_event(200); // timeout = silence, keep going
        while let Ok(Some(n)) = cap.get_next_packet_size() {
            if n == 0 { break }
            cap.read_from_device_to_deque(&mut q).map_err(e)?;
        }
        let whole = q.len() / 4 * 4;
        let bytes: Vec<u8> = q.drain(..whole).collect();
        acc.extend(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        if acc.len() >= chunk {
            let block: Vec<f32> = acc.drain(..chunk).collect();
            let peak = block.iter().fold(0f32, |m, s| m.max(s.abs()));
            if peak > 0.01 && tx.send(block).is_err() {
                break;
            }
        }
    }
    let _ = client.stop_stream();
    Ok(())
}
