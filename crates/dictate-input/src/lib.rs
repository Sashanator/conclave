//! Dictate's input layer: hotkey parsing, the hold/tap state machine (spec §4)
//! and the Windows low-level keyboard hook that drives it.
pub mod hotkey;
pub mod machine;
pub mod win;

pub use win::{is_injected, monotonic_ms};

#[cfg(windows)]
pub use win::{start_hook, HookHandle};

// Non-Windows stubs so the app crate compiles and tests on Linux.
#[cfg(not(windows))]
mod stub {
    use crate::machine::{Action, Binding};

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
}

#[cfg(not(windows))]
pub use stub::{start_hook, HookHandle};
