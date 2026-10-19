//! Dictate's input layer: hotkey parsing and the hold/tap state machine (spec §4).
//! No OS calls here; the Windows keyboard hook is added in a later task.
pub mod hotkey;
pub mod machine;
