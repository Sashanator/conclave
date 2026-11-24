//! Startup hook: back up before anything opens or migrates the database.
use super::pre_upgrade_backup;
use log::{error, info, warn};
use std::path::Path;
use tauri::Manager;

pub const BACKUP_FAILED_MESSAGE: &str =
    "Couldn't back up your data before upgrading. Continue without a backup, or quit?";

/// In the data dir itself: the backups folder may be what fails, and the failing launch has written nothing to the log.
pub const FAILURES_FILE: &str = "backup-failures.txt";
const FAILURES_MAX_BYTES: u64 = 256 * 1024;

/// true = go on to migrations; calls `ask_continue` only when the backup failed.
pub fn may_migrate(data_dir: &Path, templates: Option<&Path>, current: &str, ask_continue: impl FnOnce(&str) -> bool) -> bool {
    match pre_upgrade_backup(data_dir, templates, current) {
        Ok(Some(info)) => {
            info!("backup: pre-upgrade backup written to {}", info.path);
            true
        }
        Ok(None) => true,
        Err(e) => {
            error!("backup: {e:#}");
            // One cause per line: Windows shortens a long single line with "..."
            let causes: Vec<String> = e.chain().map(|c| c.to_string()).collect();
            let file = data_dir.join(FAILURES_FILE);
            // Written before the dialog: a launch that dies while it is open still leaves its entry
            append(&file, &failure_entry(current, &causes), true);
            let go_on = ask_continue(&format!("{BACKUP_FAILED_MESSAGE}\n\n{}\n\nDetails: {}", causes.join("\n"), file.display()));
            append(&file, &format!("choice: {}\n\n", if go_on { "Continue" } else { "Quit" }), false);
            go_on
        }
    }
}

fn failure_entry(current: &str, causes: &[String]) -> String {
    format!(
        "---- {} | pid {} | v{current}\nexe: {}\nargs: {:?}\n{}\n",
        chrono::Local::now().to_rfc3339(),
        std::process::id(),
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|e| format!("unknown ({e})")),
        // args_os: env::args panics on an argument that is not valid Unicode
        std::env::args_os().skip(1).collect::<Vec<_>>(),
        causes.iter().map(|c| format!("  {c}")).collect::<Vec<_>>().join("\n"),
    )
}

/// Appends `text`; with `may_restart`, starts the file over once it passes FAILURES_MAX_BYTES. Never fails startup.
fn append(file: &Path, text: &str, may_restart: bool) {
    use std::io::Write;
    let restart = may_restart && std::fs::metadata(file).is_ok_and(|m| m.len() >= FAILURES_MAX_BYTES);
    let written = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!restart)
        .truncate(restart)
        .open(file)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if let Err(e) = written {
        error!("backup: could not record the failure in {}: {e}", file.display());
    }
}

/// Windows: native Continue/Quit box. Close/Escape, or a dialog that can't open, means Quit.
/// No parent window: the main window is only built after this runs (see lib.rs setup).
#[cfg(windows)]
fn ask_owned(message: &str) -> bool {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    let dialog = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title("Conclave")
        .set_description(message)
        .set_buttons(MessageButtons::OkCancelCustom("Continue".into(), "Quit".into()));
    match dialog.show() {
        MessageDialogResult::Custom(s) => s == "Continue",
        MessageDialogResult::Cancel => {
            // rfd also returns Cancel when the dialog could not be shown at all
            error!("backup: Quit chosen, or the dialog could not open; treating as Quit");
            false
        }
        other => {
            error!("backup: unexpected dialog result {other:?}; treating as Quit");
            false
        }
    }
}

#[cfg(not(windows))]
fn ask_continue_without_backup(message: &str) -> bool {
    error!("backup: no dialog on this platform; treating as Quit. {message}");
    false
}

/// On Quit: exit before any pool exists. Nothing changed; the next launch retries.
pub fn run_at_startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let data_dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(e) => {
            warn!("backup: no app data dir, skipping: {e}");
            return;
        }
    };
    let templates = crate::summary::templates::templates_dir();
    #[cfg(windows)]
    let ask = ask_owned;
    #[cfg(not(windows))]
    let ask = ask_continue_without_backup;
    if !may_migrate(&data_dir, templates.as_deref(), env!("CARGO_PKG_VERSION"), ask) {
        error!("backup: user chose Quit after a failed backup; exiting without touching the database");
        log::logger().flush();
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{list_backups, write_last_run};
    use crate::database::manager::DB_FILE_NAME;

    fn data_dir() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(DB_FILE_NAME), b"db").unwrap();
        write_last_run(d.path(), "0.9.0").unwrap();
        d
    }

    #[test]
    fn may_migrate_does_not_ask_when_backup_succeeds() {
        let d = data_dir();
        assert!(may_migrate(d.path(), None, "1.0.0", |_| panic!("must not ask")));
        assert_eq!(list_backups(d.path()).unwrap().len(), 1);
    }

    #[test]
    fn may_migrate_does_not_ask_when_nothing_to_do() {
        let d = data_dir();
        assert!(may_migrate(d.path(), None, "0.9.0", |_| panic!("must not ask")));
        assert!(list_backups(d.path()).unwrap().is_empty());
    }

    // An invalid version makes the backup fail without touching the disk.
    #[test]
    fn backup_failure_asks_and_quit_blocks_migrations() {
        let d = data_dir();
        let mut msg = String::new();
        assert!(!may_migrate(d.path(), None, "bad version!", |m| {
            msg = m.to_string();
            false
        }));
        assert!(msg.starts_with(BACKUP_FAILED_MESSAGE));
        assert!(msg.contains("invalid app version"));
    }

    #[test]
    fn backup_failure_continue_proceeds() {
        let d = data_dir();
        assert!(may_migrate(d.path(), None, "bad version!", |_| true));
    }

    #[test]
    fn backup_failure_is_recorded_with_causes_and_choice() {
        let d = data_dir();
        let mut msg = String::new();
        may_migrate(d.path(), None, "bad version!", |m| {
            msg = m.to_string();
            true
        });
        assert!(msg.contains(FAILURES_FILE));
        may_migrate(d.path(), None, "bad version!", |_| false);
        let text = std::fs::read_to_string(d.path().join(FAILURES_FILE)).unwrap();
        assert_eq!(text.matches("---- ").count(), 2);
        assert!(text.contains("invalid app version") && text.contains(&format!("pid {}", std::process::id())));
        assert!(text.contains("choice: Continue") && text.contains("choice: Quit"));
    }

    #[test]
    fn failures_file_starts_over_when_too_big() {
        let d = data_dir();
        let file = d.path().join(FAILURES_FILE);
        std::fs::write(&file, vec![b'x'; FAILURES_MAX_BYTES as usize]).unwrap();
        append(&file, &failure_entry("1.0.0", &["boom".into()]), true);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("---- ") && text.contains("  boom"));
    }
}
