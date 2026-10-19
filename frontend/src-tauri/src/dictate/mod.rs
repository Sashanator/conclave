// Dictate: hold a hotkey, speak, and the text is pasted where the cursor is.
// The tunable values from the spec live here so they can be found and changed in one place.

pub mod cleanup;
pub mod settings;

/// A press shorter than this is a tap (hands-free), not a push-to-talk hold.
pub const HOLD_THRESHOLD_MS: u64 = 300;
/// Audio kept from just before the hotkey, so the first word is not clipped.
pub const PRE_ROLL_MS: u64 = 500;
/// Meeting transcription stays muted this long after a dictation ends.
pub const MUTE_TAIL_MS: u64 = 300;
/// The cleanup model gets this long before the as-spoken text is pasted instead.
pub const CLEANUP_TIMEOUT_SECS: u64 = 10;
/// The clipboard is put back this long after the paste.
pub const RESTORE_DELAY_MS: u64 = 500;
/// A dictation is cut off at this length.
pub const MAX_DICTATION_SECS: u64 = 600;
/// Shorter recordings are treated as silence and not sent to Whisper.
pub const MIN_SPEECH_SECS: f32 = 0.5;
/// The pill's "Pasted" state fades out after this long.
pub const PASTED_FADE_MS: u64 = 1000;
/// The pill's "Copied" state stays up this long.
pub const COPIED_SHOW_MS: u64 = 4000;
