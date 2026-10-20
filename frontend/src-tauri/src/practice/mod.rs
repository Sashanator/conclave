//! Speaking Practice: role-play voice practice with a Claude character, local TTS and a weak-spots report.
//!
//! Removal checklist (checked against the tree on 2026-10-20):
//! 1. Delete `frontend/src-tauri/src/practice/`, `frontend/src/app/practice/`, `frontend/src/components/Practice/`,
//!    `frontend/src/lib/practice.ts`, `frontend/tests/components/practice-page.test.tsx`,
//!    `frontend/tests/lib/practice.test.ts` and `frontend/src-tauri/tests/fixtures/practice/`.
//!    Keep `frontend/src-tauri/migrations/20261020000000_practice.sql`: an applied migration must stay in the source.
//! 2. `frontend/src-tauri/src/lib.rs`: the `mod practice;` line; the `practice::models::set_dir` line in setup, and the
//!    "speaker ID and Practice stay off" log text next to it; the `practice::commands::recover_at_startup` spawn with its
//!    comment; the 18 `practice::commands::practice_*` entries in `generate_handler!`.
//! 3. `frontend/src-tauri/src/dictate/mod.rs`, `keeps_model_loaded`: drop the `|| crate::practice::holds_model()` clause and
//!    the Practice part of its doc comment.
//! 3a. `frontend/src-tauri/src/whisper_engine/whisper_engine.rs`: remove `transcribe_audio_raw` and fold `transcribe_inner`
//!    back into `transcribe_audio_with_confidence` (the raw path exists only for the report pass).
//! 4. `frontend/src-tauri/src/speakers/runtime.rs`, `open`: back to private (optional; `practice/tts.rs` is its only other user).
//! 5. `frontend/src/components/Sidebar/index.tsx`: the two Practice entries (desktop and mobile).
//! 6. `frontend/src-tauri/Cargo.toml`: remove `base64` and `unicode-normalization` (used only by practice); move `hound`
//!    back to `[dev-dependencies]` (test code in `audio/decoder.rs` and `speakers/` still uses it); then let cargo refresh
//!    `Cargo.lock`.
//! 7. A new migration: `DROP TABLE practice_turns; DROP TABLE practice_sessions; DROP TABLE practice_materials;
//!    DROP TABLE practice_setups;` (never edit the old one).
//! 8. `docs/network-calls.md`: the two Practice rows (huggingface.co, api.anthropic.com); `AGENTS.md`: the Practice Layout row.
//! 9. Data left behind: session folders in `conclave-recordings`, and the voices in `models/practice` under app data.
//!    Both stay on disk for the user to delete.

use std::sync::atomic::{AtomicUsize, Ordering};

pub mod audio;
pub mod commands;
pub mod db;
pub mod claude;
pub mod session;
pub mod models;
pub mod files;
pub mod report;
pub mod report_job;
pub mod prompts;
pub mod pron_dict;
pub mod runner;
pub mod tts_text;
pub mod tts;

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
pub struct ModelHold(());

impl Drop for ModelHold {
    fn drop(&mut self) {
        HOLDS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn hold_model() -> ModelHold {
    HOLDS.fetch_add(1, Ordering::SeqCst);
    ModelHold(())
}
