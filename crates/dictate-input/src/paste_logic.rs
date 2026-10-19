//! Pure paste decision (spec §10). `win/paste.rs` gathers the facts and asks these functions what to do.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PasteTarget {
    App,
    OwnWindow,
    NoWindow,
    Elevated,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CopiedReason {
    NoTarget,
    Elevated,
    ClipboardBusy,
    InjectFailed,
    KeysHeld,
}

/// `Failed` means the text could not even be put on the clipboard.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PasteOutcome {
    Pasted,
    Copied(CopiedReason),
    Failed,
}

pub fn decide_paste(
    target: PasteTarget,
    clipboard_set: bool,
    keys_released: bool,
    ctrl_v_sent: bool,
) -> PasteOutcome {
    if !clipboard_set {
        return PasteOutcome::Failed;
    }
    match target {
        PasteTarget::OwnWindow | PasteTarget::NoWindow => {
            PasteOutcome::Copied(CopiedReason::NoTarget)
        }
        PasteTarget::Elevated => PasteOutcome::Copied(CopiedReason::Elevated),
        PasteTarget::App if !keys_released => PasteOutcome::Copied(CopiedReason::KeysHeld),
        PasteTarget::App if !ctrl_v_sent => PasteOutcome::Copied(CopiedReason::InjectFailed),
        PasteTarget::App => PasteOutcome::Pasted,
    }
}

/// Restore only a paste whose clipboard nobody changed since our write.
pub fn should_restore(outcome: PasteOutcome, seq_after_set: u32, seq_now: u32) -> bool {
    outcome == PasteOutcome::Pasted && seq_after_set == seq_now
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_only_for_app_with_all_ok() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, true, true),
            PasteOutcome::Pasted
        );
    }

    #[test]
    fn non_app_targets_copy() {
        assert_eq!(
            decide_paste(PasteTarget::OwnWindow, true, true, true),
            PasteOutcome::Copied(CopiedReason::NoTarget)
        );
        assert_eq!(
            decide_paste(PasteTarget::NoWindow, true, true, true),
            PasteOutcome::Copied(CopiedReason::NoTarget)
        );
        assert_eq!(
            decide_paste(PasteTarget::Elevated, true, true, true),
            PasteOutcome::Copied(CopiedReason::Elevated)
        );
    }

    #[test]
    fn held_keys_copy() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, false, false),
            PasteOutcome::Copied(CopiedReason::KeysHeld)
        );
    }

    #[test]
    fn inject_failure_copies() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, true, false),
            PasteOutcome::Copied(CopiedReason::InjectFailed)
        );
    }

    #[test]
    fn clipboard_failure_fails() {
        for target in [
            PasteTarget::App,
            PasteTarget::OwnWindow,
            PasteTarget::NoWindow,
            PasteTarget::Elevated,
        ] {
            for keys in [false, true] {
                for sent in [false, true] {
                    assert_eq!(
                        decide_paste(target, false, keys, sent),
                        PasteOutcome::Failed
                    );
                }
            }
        }
    }

    #[test]
    fn restore_only_after_paste_and_unchanged() {
        assert!(should_restore(PasteOutcome::Pasted, 7, 7));
        assert!(!should_restore(PasteOutcome::Pasted, 7, 8));
        for outcome in [
            PasteOutcome::Copied(CopiedReason::NoTarget),
            PasteOutcome::Copied(CopiedReason::Elevated),
            PasteOutcome::Copied(CopiedReason::ClipboardBusy),
            PasteOutcome::Copied(CopiedReason::InjectFailed),
            PasteOutcome::Copied(CopiedReason::KeysHeld),
            PasteOutcome::Failed,
        ] {
            assert!(!should_restore(outcome, 7, 7));
        }
    }
}
