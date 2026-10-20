//! Speaking Practice: role-play voice practice with a Claude character, local TTS and a weak-spots report.
// Later tasks consume these items; until then they are unused.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

pub mod db;

pub const CHAT_MODEL: &str = "claude-haiku-5-5";
pub const PLAN_MODEL: &str = "claude-opus-5-5";
pub const REPORT_MODEL: &str = "claude-opus-5-5";
pub const CHAT_MAX_TOKENS: u32 = 400;
pub const LONG_MAX_TOKENS: u32 = 16_000;

pub const MAX_SESSION_SECS: f64 = 3600.0;
pub const WARN_AT_SECS: f64 = 3000.0;
pub const WRAP_UP_AT_SECS: f64 = 3300.0;
pub const MAX_TURN_SECS: f64 = 300.0;

pub const MIN_SPEECH_SECS: f64 = 0.5;
pub const LONG_PAUSE_SECS: f64 = 1.0;
pub const LIVE_VAD_REDEMPTION_MS: u64 = 700;

/// Total material bytes per setup.
pub const MATERIAL_MAX_BYTES: i64 = 20 * 1024 * 1024;

pub const TTS_STEPS: u32 = 8;
pub const TTS_SPEED: f32 = 1.05;
pub const TTS_RATE: u32 = 44_100;
pub const YOU_WAV_RATE: u32 = 16_000;

/// Counts the live holds on the Whisper model. A session and a report job can overlap.
static HOLDS: AtomicUsize = AtomicUsize::new(0);

pub fn holds_model() -> bool {
    HOLDS.load(Ordering::SeqCst) > 0
}

/// Keeps the model loaded while it is alive. Dropping it releases the hold.
pub struct ModelHold;

impl Drop for ModelHold {
    fn drop(&mut self) {
        HOLDS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn hold_model() -> ModelHold {
    HOLDS.fetch_add(1, Ordering::SeqCst);
    ModelHold
}
