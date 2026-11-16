//! Tauri commands for the Backups settings section.
use super::{list_backups, manual_backup, BackupInfo, BACKUPS_DIR};
use crate::state::AppState;
use tauri::{AppHandle, Manager};

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map_err(|e| format!("Failed to get app data dir: {e}"))
}

#[tauri::command]
pub async fn backup_now(app: AppHandle) -> Result<BackupInfo, String> {
    let dir = data_dir(&app)?;
    let state = app.try_state::<AppState>().ok_or("The database isn't open yet")?;
    let templates = crate::summary::templates::templates_dir();
    manual_backup(state.db_manager.pool(), &dir, templates.as_deref(), env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|e| format!("Backup failed: {e:#}"))
}

#[tauri::command]
pub async fn backup_list(app: AppHandle) -> Result<Vec<BackupInfo>, String> {
    list_backups(&data_dir(&app)?).map_err(|e| format!("Couldn't read the backups folder: {e:#}"))
}

#[tauri::command]
pub async fn backup_open_folder(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?.join(BACKUPS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create directory: {e}"))?;
    crate::database::commands::open_folder(&dir)
}
