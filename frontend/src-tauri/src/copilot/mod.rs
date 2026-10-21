//! Live copilot: name alerts now, the copilot prompts and actions in later tasks.
#![allow(dead_code)] // wired in by the transcript listener (Task 3)
pub mod alert;

/// At most one "someone said your name" banner per this many seconds.
pub const ALERT_COOLDOWN_SECS: u64 = 30;
/// The banner shows at most this many characters of the matching line.
pub const MENTION_LINE_CHARS: usize = 160;
