//! Detects online meetings from Windows' microphone-usage record (spec §5.3).
pub mod classify;
pub mod consent;
pub mod runner;
#[cfg(windows)]
pub mod windows;
