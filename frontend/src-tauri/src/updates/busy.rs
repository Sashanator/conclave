//! What may not be interrupted by installing an update: a recording, a Practice session or report, a summary,
//! a model download. Long jobs hold a `BusyGuard` (RAII, so a panic or early return releases it).

use crate::audio::recording_commands::{is_recording_now, start_in_flight};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusyReason {
    Recording,
    PracticeSession,
    PracticeReport,
    Summary,
    ModelDownload,
}

impl fmt::Display for BusyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Recording => "a recording is in progress or being saved",
            Self::PracticeSession => "a Practice session is running",
            Self::PracticeReport => "a Practice report is being generated",
            Self::Summary => "a summary is being generated",
            Self::ModelDownload => "a model is downloading",
        })
    }
}

const ORDER: [BusyReason; 5] = [
    BusyReason::Recording,
    BusyReason::PracticeSession,
    BusyReason::PracticeReport,
    BusyReason::Summary,
    BusyReason::ModelDownload,
];

static HELD: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];

pub struct BusyGuard(BusyReason);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        HELD[self.0 as usize].fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn hold(r: BusyReason) -> BusyGuard {
    HELD[r as usize].fetch_add(1, Ordering::SeqCst);
    BusyGuard(r)
}

pub fn held(r: BusyReason) -> usize {
    HELD[r as usize].load(Ordering::SeqCst)
}

/// Pure order of the checks: recording first, then a Practice session, then the held counters in enum order.
fn first_busy(recording: bool, session: bool, held: impl Fn(BusyReason) -> usize) -> Option<BusyReason> {
    if recording {
        return Some(BusyReason::Recording);
    }
    if session {
        return Some(BusyReason::PracticeSession);
    }
    ORDER.into_iter().find(|r| held(*r) > 0)
}

/// Covers the stop tail and the save too, like tray Quit (the sync parts of `meeting_detector::busy`).
pub fn current() -> Option<BusyReason> {
    first_busy(
        is_recording_now() || start_in_flight() || crate::meeting_detector::post_processing(),
        crate::practice::commands::session_active(),
        held,
    )
}

/// Returns once `busy` has reported idle. Generic so tests can script it.
pub async fn wait_until_idle(busy: impl Fn() -> Option<BusyReason>, poll: Duration) {
    while busy().is_some() {
        tokio::time::sleep(poll).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn hold_counts_until_dropped() {
        // ModelDownload is only held by this test within the module.
        let before = held(BusyReason::ModelDownload);
        let a = hold(BusyReason::ModelDownload);
        let b = hold(BusyReason::ModelDownload);
        assert_eq!(held(BusyReason::ModelDownload), before + 2);
        drop(a);
        assert_eq!(held(BusyReason::ModelDownload), before + 1);
        drop(b);
        assert_eq!(held(BusyReason::ModelDownload), before);
    }

    #[test]
    fn guard_drops_on_panic() {
        let before = held(BusyReason::Summary);
        let r = std::panic::catch_unwind(|| {
            let _g = hold(BusyReason::Summary);
            panic!("boom");
        });
        assert!(r.is_err());
        assert_eq!(held(BusyReason::Summary), before);
    }

    #[test]
    fn first_busy_checks_in_order() {
        let none = |_| 0;
        assert_eq!(first_busy(false, false, none), None);
        let all = |_| 1;
        assert_eq!(first_busy(true, true, all), Some(BusyReason::Recording));
        assert_eq!(first_busy(false, true, all), Some(BusyReason::PracticeSession));
        assert_eq!(first_busy(false, false, all), Some(BusyReason::Recording));
        let rest = |r| usize::from(matches!(r, BusyReason::Summary | BusyReason::ModelDownload));
        assert_eq!(first_busy(false, false, rest), Some(BusyReason::Summary));
        let dl = |r| usize::from(r == BusyReason::ModelDownload);
        assert_eq!(first_busy(false, false, dl), Some(BusyReason::ModelDownload));
    }

    #[tokio::test]
    async fn wait_until_idle_returns_only_after_busy_ends() {
        let calls = Cell::new(0);
        wait_until_idle(
            || {
                calls.set(calls.get() + 1);
                (calls.get() <= 3).then_some(BusyReason::Summary)
            },
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(calls.get(), 4);
    }

    #[tokio::test]
    async fn wait_until_idle_never_returns_while_busy() {
        let r = tokio::time::timeout(
            Duration::from_millis(50),
            wait_until_idle(|| Some(BusyReason::Recording), Duration::from_millis(1)),
        )
        .await;
        assert!(r.is_err());
    }

    #[test]
    fn reason_text_is_user_language() {
        let texts: Vec<String> = ORDER.iter().map(|r| r.to_string()).collect();
        assert_eq!(
            texts,
            [
                "a recording is in progress or being saved",
                "a Practice session is running",
                "a Practice report is being generated",
                "a summary is being generated",
                "a model is downloading",
            ]
        );
    }

    #[tokio::test]
    async fn guard_taken_before_spawn_is_held_until_the_task_ends() {
        let before = held(BusyReason::PracticeReport);
        let busy = hold(BusyReason::PracticeReport);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _busy = busy;
            let _ = rx.await;
        });
        // Held with the task not yet run, and while it waits.
        assert_eq!(held(BusyReason::PracticeReport), before + 1);
        tx.send(()).unwrap();
        task.await.unwrap();
        assert_eq!(held(BusyReason::PracticeReport), before);
    }
}
