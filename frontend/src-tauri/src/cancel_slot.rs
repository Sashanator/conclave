//! The single in-flight LLM call of a feature, cancellable by request id.

use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio::task::AbortHandle;

/// Holds one in-flight call: its request id and how to abort it.
pub struct CancelSlot {
    held: Mutex<Option<(u64, AbortHandle)>>,
    busy: &'static str,
}

impl CancelSlot {
    /// `busy` is the error a second call gets while one is in flight.
    pub const fn new(busy: &'static str) -> Self {
        Self { held: Mutex::new(None), busy }
    }

    /// Takes the slot for `id`. The claim frees it on drop, but only while it still holds this id.
    pub fn begin(&self, id: u64, abort: AbortHandle) -> Result<Claim<'_>, String> {
        let mut slot = self.lock();
        if slot.is_some() {
            return Err(self.busy.to_string());
        }
        *slot = Some((id, abort));
        Ok(Claim { slot: self, id })
    }

    /// Aborts and frees the slot only when it holds `id`. Freeing it here lets the next call start
    /// at once, without waiting for the cancelled call to return.
    pub fn cancel(&self, id: u64) {
        let mut slot = self.lock();
        if slot.as_ref().is_some_and(|(held, _)| *held == id) {
            if let Some((_, handle)) = slot.take() {
                handle.abort();
            }
        }
    }

    /// Frees the slot only when it holds `id`, without aborting the call.
    pub fn finish(&self, id: u64) {
        let mut slot = self.lock();
        if slot.as_ref().is_some_and(|(held, _)| *held == id) {
            *slot = None;
        }
    }

    /// Aborts whatever call is in flight.
    pub fn abort_all(&self) {
        let taken = self.lock().take();
        if let Some((_, handle)) = taken {
            handle.abort();
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<(u64, AbortHandle)>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
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
