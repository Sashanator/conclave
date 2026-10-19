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

/// `clipboard_changed`: the clipboard changed after our write, so Ctrl+V would paste someone else's copy.
pub fn decide_paste(
    target: PasteTarget,
    clipboard_set: bool,
    keys_released: bool,
    clipboard_changed: bool,
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
        PasteTarget::App if clipboard_changed => PasteOutcome::Copied(CopiedReason::ClipboardBusy),
        PasteTarget::App if !ctrl_v_sent => PasteOutcome::Copied(CopiedReason::InjectFailed),
        PasteTarget::App => PasteOutcome::Pasted,
    }
}

/// Restore only a paste whose clipboard nobody changed since our write.
pub fn should_restore(outcome: PasteOutcome, seq_after_set: u32, seq_now: u32) -> bool {
    outcome == PasteOutcome::Pasted && seq_after_set == seq_now
}

/// Window classes that draw the desktop, where a paste goes nowhere.
pub fn is_desktop_class(name: &str) -> bool {
    matches!(name, "WorkerW" | "Progman")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_only_for_app_with_all_ok() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, true, false, true),
            PasteOutcome::Pasted
        );
    }

    #[test]
    fn non_app_targets_copy() {
        for changed in [false, true] {
            assert_eq!(
                decide_paste(PasteTarget::OwnWindow, true, true, changed, true),
                PasteOutcome::Copied(CopiedReason::NoTarget)
            );
            assert_eq!(
                decide_paste(PasteTarget::NoWindow, true, true, changed, true),
                PasteOutcome::Copied(CopiedReason::NoTarget)
            );
            assert_eq!(
                decide_paste(PasteTarget::Elevated, true, true, changed, true),
                PasteOutcome::Copied(CopiedReason::Elevated)
            );
        }
    }

    #[test]
    fn held_keys_copy() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, false, false, false),
            PasteOutcome::Copied(CopiedReason::KeysHeld)
        );
    }

    #[test]
    fn keys_held_wins_over_clipboard_changed() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, false, true, false),
            PasteOutcome::Copied(CopiedReason::KeysHeld)
        );
    }

    #[test]
    fn clipboard_changed_copies_busy() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, true, true, false),
            PasteOutcome::Copied(CopiedReason::ClipboardBusy)
        );
    }

    #[test]
    fn inject_failure_copies() {
        assert_eq!(
            decide_paste(PasteTarget::App, true, true, false, false),
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
                for changed in [false, true] {
                    for sent in [false, true] {
                        assert_eq!(
                            decide_paste(target, false, keys, changed, sent),
                            PasteOutcome::Failed
                        );
                    }
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

    #[test]
    fn desktop_classes_are_the_desktop() {
        assert!(is_desktop_class("WorkerW"));
        assert!(is_desktop_class("Progman"));
        assert!(!is_desktop_class("Chrome_WidgetWin_1"));
        assert!(!is_desktop_class("Shell_TrayWnd"));
        assert!(!is_desktop_class(""));
    }
}
