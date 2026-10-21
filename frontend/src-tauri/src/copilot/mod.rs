//! Live copilot: settings, name alerts, and on-demand answers from the live transcript.
#![allow(dead_code)] // alert functions are wired in by the transcript listener (Task 3)
pub mod alert;
pub mod settings;

/// At most one "someone said your name" banner per this many seconds.
pub const ALERT_COOLDOWN_SECS: u64 = 30;
/// The banner shows at most this many characters of the matching line.
pub const MENTION_LINE_CHARS: usize = 160;
/// Alert words: at most this many, each at most MAX_ALERT_WORD_CHARS characters.
pub const MAX_ALERT_WORDS: usize = 30;
pub const MAX_ALERT_WORD_CHARS: usize = 40;
