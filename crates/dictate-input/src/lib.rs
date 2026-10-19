//! Dictate's input layer: hotkey parsing, the hold/tap state machine (spec §4)
//! and the Windows low-level keyboard hook that drives it.
pub mod hotkey;
pub mod machine;
pub mod paste_logic;
pub mod win;

pub use win::is_injected;

pub use paste_logic::{decide_paste, should_restore, CopiedReason, PasteOutcome, PasteTarget};

#[cfg(windows)]
pub use win::{clipboard, paste, start_hook, HookHandle};

// Non-Windows stubs so the app crate compiles and tests on Linux.
#[cfg(not(windows))]
mod stub {
    use std::time::Duration;

    use crate::machine::{Action, Binding};
    use crate::paste_logic::PasteOutcome;

    pub struct HookHandle;

    impl HookHandle {
        pub fn set_bindings(&self, _bindings: Vec<Binding>) {}
        pub fn reset(&self) {}
        pub fn stop(self) {}
    }

    pub fn start_hook(
        _bindings: Vec<Binding>,
        _hold_threshold_ms: u64,
        _on_action: Box<dyn Fn(Action) + Send + 'static>,
    ) -> Result<HookHandle, String> {
        Err("Dictate needs Windows".into())
    }

    pub mod clipboard {
        pub struct SavedClipboard;

        pub fn save() -> Result<SavedClipboard, String> {
            Err("Dictate needs Windows".into())
        }

        pub fn set_private_text(
            _text: &str,
            _fallback: Option<&SavedClipboard>,
        ) -> Result<(), String> {
            Err("Dictate needs Windows".into())
        }

        pub fn restore(_saved: &SavedClipboard, _expected_seq: u32) -> Result<bool, String> {
            Err("Dictate needs Windows".into())
        }

        pub fn sequence_number() -> u32 {
            0
        }
    }

    pub mod paste {
        use super::*;
        use crate::paste_logic::PasteTarget;

        pub fn foreground_target(_own_pid: u32) -> PasteTarget {
            PasteTarget::NoWindow
        }

        pub fn foreground_monitor_point() -> Option<(i32, i32)> {
            None
        }

        pub fn wait_keys_released(_timeout: Duration) -> bool {
            false
        }

        pub fn send_ctrl_v() -> Result<(), String> {
            Err("Dictate needs Windows".into())
        }

        pub fn paste_text(
            _text: &str,
            _own_pid: u32,
            _restore_delay: Duration,
            _proceed: &dyn Fn() -> bool,
        ) -> PasteOutcome {
            PasteOutcome::Failed
        }
    }
}

#[cfg(not(windows))]
pub use stub::{clipboard, paste, start_hook, HookHandle};
