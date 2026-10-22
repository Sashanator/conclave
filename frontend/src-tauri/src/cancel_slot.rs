//! The single in-flight LLM call of a feature, cancellable by request id.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tokio::task::AbortHandle;

/// A cancel that arrives before its call begins is kept this long. The two IPC calls are milliseconds apart.
const EARLY_CANCEL_TTL: Duration = Duration::from_secs(10);
/// Bounds the early cancels kept, so stale ids cannot grow the list.
const EARLY_CANCEL_MAX: usize = 16;

struct State {
    held: Option<(u64, AbortHandle)>,
    /// Cancels for calls that have not begun (or have ended), with their arrival time.
    early: Vec<(u64, Instant)>,
}

impl State {
    fn prune(&mut self) {
        self.early.retain(|(_, at)| at.elapsed() < EARLY_CANCEL_TTL);
    }
}

/// Holds one in-flight call: its request id and how to abort it.
pub struct CancelSlot {
    state: Mutex<State>,
    busy: &'static str,
}

impl CancelSlot {
    /// `busy` is the error a second call gets while one is in flight.
    pub const fn new(busy: &'static str) -> Self {
        Self { state: Mutex::new(State { held: None, early: Vec::new() }), busy }
    }

    /// Takes the slot for `id`. The claim frees it on drop, but only while it still holds this id.
    /// A cancel for `id` that arrived first refuses the call: it is aborted and the slot stays free.
    pub fn begin(&self, id: u64, abort: AbortHandle) -> Result<Claim<'_>, String> {
        let mut state = self.lock();
        state.prune();
        if let Some(at) = state.early.iter().position(|(early, _)| *early == id) {
            state.early.remove(at);
            abort.abort();
            return Err("Cancelled".to_string());
        }
        if state.held.is_some() {
            return Err(self.busy.to_string());
        }
        state.held = Some((id, abort));
        Ok(Claim { slot: self, id })
    }

    /// Aborts and frees the slot when it holds `id`. Any other cancel is kept for a call that begins later.
    pub fn cancel(&self, id: u64) {
        let mut state = self.lock();
        if state.held.as_ref().is_some_and(|(held, _)| *held == id) {
            if let Some((_, handle)) = state.held.take() {
                handle.abort();
            }
            return;
        }
        state.prune();
        state.early.retain(|(early, _)| *early != id);
        state.early.push((id, Instant::now()));
        let excess = state.early.len().saturating_sub(EARLY_CANCEL_MAX);
        state.early.drain(..excess);
    }

    /// Frees the slot only when it holds `id`, without aborting the call.
    pub fn finish(&self, id: u64) {
        let mut state = self.lock();
        if state.held.as_ref().is_some_and(|(held, _)| *held == id) {
            state.held = None;
        }
    }

    /// Aborts whatever call is in flight.
    pub fn abort_all(&self) {
        let taken = self.lock().held.take();
        if let Some((_, handle)) = taken {
            handle.abort();
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Frees the slot on drop, so a call that is dropped or returns early cannot hold it.
pub struct Claim<'a> {
    slot: &'a CancelSlot,
    id: u64,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.slot.finish(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> tokio::task::JoinHandle<()> {
        tokio::spawn(std::future::pending())
    }

    #[tokio::test]
    async fn cancel_before_begin_refuses_the_call() {
        let slot = CancelSlot::new("busy");
        slot.cancel(7);
        let work = task();
        assert_eq!(slot.begin(7, work.abort_handle()).err().as_deref(), Some("Cancelled"));
        assert!(work.await.unwrap_err().is_cancelled());
        // The refused call did not take the slot
        assert!(slot.begin(8, task().abort_handle()).is_ok());
    }

    #[tokio::test]
    async fn cancel_after_begin_aborts_the_call() {
        let slot = CancelSlot::new("busy");
        let work = task();
        let _claim = slot.begin(7, work.abort_handle()).unwrap();
        slot.cancel(7);
        assert!(work.await.unwrap_err().is_cancelled());
        // The slot is free again
        assert!(slot.begin(8, task().abort_handle()).is_ok());
    }

    #[tokio::test]
    async fn stale_early_cancel_expires() {
        let slot = CancelSlot::new("busy");
        slot.cancel(9);
        // A cancel for a call that never began must not refuse a later call that reuses its id
        slot.lock().early[0].1 = Instant::now() - (EARLY_CANCEL_TTL + Duration::from_secs(1));
        assert!(slot.begin(9, task().abort_handle()).is_ok());
    }

    #[tokio::test]
    async fn cancel_for_another_id_leaves_the_call_running() {
        let slot = CancelSlot::new("busy");
        let work = task();
        let _claim = slot.begin(7, work.abort_handle()).unwrap();
        slot.cancel(8);
        tokio::task::yield_now().await;
        assert!(!work.is_finished());
    }
}
