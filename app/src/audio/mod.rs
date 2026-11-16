//! Audio capture (16 kHz mono f32) for transcription.
//!   "default"      -> whatever the default output plays (loopback)
//!   "app:<name>"   -> a single application, like OBS
//!   "out:<name>"   -> loopback of a specific output
//!   "in:<name>"    -> an input (microphone, virtual cable...)
pub const RATE: usize = 16000;

#[derive(Clone, Debug)]
pub struct AudioOption {
    pub id: String,
    pub label: String,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{list_options, spawn_capture};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{list_options, spawn_capture};
