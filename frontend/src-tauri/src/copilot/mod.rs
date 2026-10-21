// Live copilot: its settings here; the alert listener and the ask flow come in later steps.

pub mod settings;

/// Alert words: at most this many, each at most MAX_ALERT_WORD_CHARS characters.
pub const MAX_ALERT_WORDS: usize = 30;
pub const MAX_ALERT_WORD_CHARS: usize = 40;
