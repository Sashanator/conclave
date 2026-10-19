// Keeps a dictation out of a recorded meeting: while any MuteGuard is alive, the meeting pipeline
// silences the microphone. The mic is muted for the whole dictation, recording or not.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use super::MUTE_TAIL_MS;

/// `until_ms` value while at least one guard is alive.
const ACTIVE: u64 = u64::MAX;

/// Count of live guards, plus the time (monotonic ms) until which the mic stays muted.
/// `enter` and `leave` take `write_lock` so the depth and `until_ms` updates cannot interleave;
/// `muted` only reads `until_ms`, so the pipeline's hot path is one atomic load.
pub struct MuteState {
    depth: AtomicU32,
    until_ms: AtomicU64,
    write_lock: Mutex<()>,
}

impl MuteState {
    pub const fn new() -> Self {
        Self {
            depth: AtomicU32::new(0),
            until_ms: AtomicU64::new(0),
            write_lock: Mutex::new(()),
        }
    }

    pub fn muted(&self, now_ms: u64) -> bool {
        self.until_ms.load(Ordering::Acquire) > now_ms
    }

    pub fn enter(&self) {
        let _lock = self
            .write_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.depth.fetch_add(1, Ordering::Relaxed);
        self.until_ms.store(ACTIVE, Ordering::Release);
    }

    pub fn leave(&self, now_ms: u64) {
        let _lock = self
            .write_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let depth = self.depth.load(Ordering::Relaxed).saturating_sub(1);
        self.depth.store(depth, Ordering::Relaxed);
        if depth == 0 {
            self.until_ms
                .store(now_ms + MUTE_TAIL_MS, Ordering::Release);
        }
    }
}

static GLOBAL: MuteState = MuteState::new();

/// Monotonic milliseconds since the first call; wall-clock jumps cannot shorten the tail.
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// True while a dictation holds the meeting mic muted, or within its tail.
pub fn meeting_mic_muted() -> bool {
    GLOBAL.muted(now_ms())
}

/// Holds the meeting mic muted for as long as it lives. Dropping it (including while unwinding)
/// releases the hold after `MUTE_TAIL_MS`.
pub struct MuteGuard;

impl MuteGuard {
    pub fn new() -> MuteGuard {
        GLOBAL.enter();
        MuteGuard
    }
}

impl Drop for MuteGuard {
    fn drop(&mut self) {
        GLOBAL.leave(now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn state_mutes_and_tail_releases() {
        let state = MuteState::new();
        state.enter();
        assert!(state.muted(5));
        state.leave(1000);
        assert!(state.muted(1299));
        assert!(!state.muted(1300));

        let fresh = MuteState::new();
        assert!(!fresh.muted(1));
    }

    #[test]
    fn nested_enters() {
        let state = MuteState::new();
        state.enter();
        state.enter();
        state.leave(1000);
        assert!(state.muted(99999));
        state.leave(2000);
        assert!(state.muted(2299));
        assert!(!state.muted(2300));
    }

    #[test]
    fn guard_drop_on_panic_releases() {
        let muted_while_alive = AtomicBool::new(false);
        // The panic message on stderr is expected; the hook is not swapped, which would race other tests.
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = MuteGuard::new();
            muted_while_alive.store(meeting_mic_muted(), Ordering::SeqCst);
            panic!("simulated panic while dictating");
        }));

        assert!(result.is_err());
        assert!(muted_while_alive.load(Ordering::SeqCst));
        std::thread::sleep(std::time::Duration::from_millis(MUTE_TAIL_MS + 100));
        assert!(!meeting_mic_muted());
    }
}
