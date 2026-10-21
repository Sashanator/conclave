//! Live copilot: settings, name alerts, and on-demand answers from the live transcript.
pub mod alert;
pub mod ask;
pub mod settings;

/// At most one "someone said your name" banner per this many seconds.
pub const ALERT_COOLDOWN_SECS: u64 = 30;
/// The banner shows at most this many characters of the matching line.
pub const MENTION_LINE_CHARS: usize = 160;
/// Alert words: at most this many, each at most MAX_ALERT_WORD_CHARS characters.
pub const MAX_ALERT_WORDS: usize = 30;
pub const MAX_ALERT_WORD_CHARS: usize = 40;
/// A copilot answer is abandoned after this many seconds.
pub const ASK_TIMEOUT_SECS: u64 = 60;
/// The most transcript and notes text sent in one call. Ollama uses its context size, up to this.
pub const MAX_CONTEXT_CHARS: usize = 60_000;
pub const MAX_QUESTION_CHARS: usize = 1000;
