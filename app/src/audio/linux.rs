//! Linux: PipeWire / PulseAudio through `pactl` + `parec` (pulseaudio-utils / pipewire-pulse).
//!   default     -> monitor of the default sink
//!   app:<name>  -> a sink-input (application stream), recorded with `parec --monitor-stream`
//!   out:<sink>  -> monitor of that sink
//!   in:<source> -> that source
use super::{AudioOption, RATE};
use crate::i18n::t;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

fn pactl(args: &[&str]) -> String {
    Command::new("pactl").args(args).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// (index, application name) of playing streams
fn sink_inputs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut idx = String::new();
    for line in pactl(&["list", "sink-inputs"]).lines() {
        let l = line.trim();
        if let Some(i) = l.strip_prefix("Sink Input #") {
            idx = i.to_string();
        } else if let Some(n) = l.strip_prefix("application.name = ") {
            out.push((idx.clone(), n.trim_matches('"').to_string()));
        }
    }
    out
}

fn short_list(kind: &str) -> Vec<String> {
    pactl(&["list", "short", kind]).lines()
        .filter_map(|l| l.split('\t').nth(1).map(str::to_string)).collect()
}

pub fn list_options() -> Vec<AudioOption> {
    let mut v = vec![AudioOption { id: "default".into(), label: format!("🔊 {}", t("src_default")) }];
    let mut seen = std::collections::HashSet::new();
    for (_, name) in sink_inputs() {
        if seen.insert(name.clone()) {
            v.push(AudioOption { id: format!("app:{name}"), label: format!("🎯 {}: {name}", t("src_app")) });
        }
    }
    for s in short_list("sinks") {
        v.push(AudioOption { id: format!("out:{s}"), label: format!("🎧 {}: {s}", t("src_out")) });
    }
    for s in short_list("sources").into_iter().filter(|s| !s.ends_with(".monitor")) {
        v.push(AudioOption { id: format!("in:{s}"), label: format!("🎤 {}: {s}", t("src_in")) });
    }
    v
}

fn parec_args(source: &str) -> Result<Vec<String>, String> {
    let base = vec!["--format=float32le".to_string(), format!("--rate={RATE}"), "--channels=1".into(),
                    "--latency-msec=100".into()];
    let mut a = base;
    if let Some(app) = source.strip_prefix("app:") {
        let (idx, _) = sink_inputs().into_iter().find(|(_, n)| n == app)
            .ok_or_else(|| format!("{app} is not playing audio"))?;
        a.push(format!("--monitor-stream={idx}"));
    } else if let Some(s) = source.strip_prefix("out:") {
        a.push(format!("--device={s}.monitor"));
    } else if let Some(s) = source.strip_prefix("in:") {
        a.push(format!("--device={s}"));
    } else {
        a.push("--device=@DEFAULT_MONITOR@".into());
    }
    Ok(a)
}

pub fn spawn_capture(source: String, chunk_secs: f32, tx: Sender<Vec<f32>>,
                     stop: Arc<AtomicBool>, log: Sender<String>) {
    std::thread::spawn(move || {
        let args = match parec_args(&source) {
            Ok(a) => a,
            Err(e) => { let _ = log.send(format!("[audio] {e}")); return; }
        };
        let mut child = match Command::new("parec").args(&args).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            Ok(c) => c,
            Err(e) => { let _ = log.send(format!("[audio] parec: {e} (install pulseaudio-utils)")); return; }
        };
        let mut out = child.stdout.take().unwrap();
        let chunk = (chunk_secs.max(1.0) * RATE as f32) as usize;
        let mut buf = vec![0u8; chunk * 4];
        while !stop.load(Ordering::Relaxed) {
            if out.read_exact(&mut buf).is_err() { break; }
            let block: Vec<f32> = buf.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
            let peak = block.iter().fold(0f32, |m, s| m.max(s.abs()));
            if peak > 0.01 && tx.send(block).is_err() { break; }
        }
        let _ = child.kill();
    });
}
