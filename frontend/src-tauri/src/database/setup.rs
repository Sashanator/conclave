use log::info;
use tauri::{AppHandle, Emitter, Manager};

use super::manager::{DatabaseManager, DB_FILE_NAME};
use super::repositories::summary::SummaryProcessesRepository;
use crate::state::AppState;
use std::path::{Path, PathBuf};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// The text of the error dialog shown when the database cannot open. Says what was not deleted.
pub fn open_failure_message(db_path: &Path, error: &str) -> String {
    format!(
        "Conclave could not open its database.\n\nDatabase: {}\nError: {error}\n\nConclave did not delete, rename or replace any database files. Fix the cause and start Conclave again.",
        db_path.display()
    )
}

/// Shows why the database would not open, then exits with code 1. The dialog waits on its own thread:
/// blocking_show must not run on the main thread, and setup runs on it.
pub fn report_open_failure(app: &AppHandle, error: &str) {
    let db_path = app
        .path()
        .app_data_dir()
        .map(|dir| dir.join(DB_FILE_NAME))
        .unwrap_or_else(|_| PathBuf::from(DB_FILE_NAME));
    let message = open_failure_message(&db_path, error);
    log::error!("{message}");
    let handle = app.clone();
    std::thread::spawn(move || {
        handle
            .dialog()
            .message(message)
            .title("Conclave")
            .kind(MessageDialogKind::Error)
            .blocking_show();
        handle.exit(1);
    });
}

/// Initialize database on app startup
/// Handles first launch detection and conditional initialization
pub async fn initialize_database_on_startup(app: &AppHandle) -> Result<(), String> {
    // Check if this is the first launch (no database exists yet)
    let is_first_launch = DatabaseManager::is_first_launch(app)
        .await
        .map_err(|e| format!("Failed to check first launch status: {}", e))?;

    if is_first_launch {
        info!("First launch detected - will notify window when ready");

        // Delay event emission to ensure window is ready and React listeners are registered
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            app_handle
                .emit("first-launch-detected", ())
                .expect("Failed to emit first-launch-detected event");
            info!("Emitted first-launch-detected after delay");
        });
    } else {
        // Normal flow - initialize database immediately
        let db_manager = DatabaseManager::new_from_app_handle(app)
            .await
            .map_err(|e| format!("Failed to initialize database manager: {}", e))?;

        app.manage(AppState { db_manager });
        crate::speakers::job::wake();
        info!("Database initialized successfully");
    }

    Ok(())
}

/// Startup sweep: a summary left pending by a crash never finishes. Skipped on first launch, when no database is open yet.
pub async fn fail_interrupted_summaries<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else { return };
    match SummaryProcessesRepository::fail_interrupted(state.db_manager.pool()).await {
        Ok(0) => {}
        Ok(n) => info!("summary: marked {n} interrupted summaries as failed"),
        Err(e) => log::warn!("summary: could not recover interrupted summaries: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::mock_app;

    #[test]
    fn open_failure_message_names_path_error_and_keeps_files() {
        let path = Path::new(r"C:\Users\me\AppData\Roaming\com.conclave.app\meeting_minutes.sqlite");
        let msg = open_failure_message(path, "database disk image is malformed");
        assert!(msg.contains(r"C:\Users\me\AppData\Roaming\com.conclave.app\meeting_minutes.sqlite"));
        assert!(msg.contains("database disk image is malformed"));
        assert!(msg.contains("did not delete, rename or replace any database files"));
    }

    #[tokio::test]
    async fn startup_sweep_skips_first_launch() {
        // First launch: no AppState is registered yet, and the sweep must not panic
        let app = mock_app();
        fail_interrupted_summaries(app.handle()).await;
    }

    #[tokio::test]
    async fn startup_sweep_fails_pending_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("meeting_minutes.sqlite").to_string_lossy().to_string();
        let legacy = dir.path().join("meeting_minutes.db").to_string_lossy().to_string();
        let db_manager = DatabaseManager::new(&db, &legacy).await.unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 'M', 't', 't')")
            .execute(db_manager.pool()).await.unwrap();
        sqlx::query("INSERT INTO summary_processes (meeting_id, status, created_at, updated_at) VALUES ('m', 'PENDING', 't', 't')")
            .execute(db_manager.pool()).await.unwrap();
        let app = mock_app();
        app.manage(AppState { db_manager: db_manager.clone() });

        fail_interrupted_summaries(app.handle()).await;

        let (status,): (String,) = sqlx::query_as("SELECT status FROM summary_processes WHERE meeting_id = 'm'")
            .fetch_one(db_manager.pool()).await.unwrap();
        assert_eq!(status, "failed");
    }
}
