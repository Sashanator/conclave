// Tauri commands for speaker identification: the settings switch, model downloads, a meeting's speakers,
// re-identify, naming and moving lines, and the people list.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Runtime, State};

use super::{job, models, settings};
use crate::database::repositories::speakers::{NameTarget, PersonView, SpeakerView, SpeakersRepository};
use crate::state::AppState;

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
pub struct SpeakerSettings {
    pub enabled: bool,
    pub models_ready: bool,
    pub downloading: bool,
    pub last_analysis: Option<LastAnalysis>,
}

#[derive(Serialize)]
pub struct LastAnalysis {
    pub backend: String,
    pub run_seconds: f64,
    pub audio_seconds: f64,
}

#[derive(Serialize)]
pub struct JobView {
    pub status: String,
    pub reason: Option<String>,
    pub percent: Option<u8>,
}

#[derive(Serialize)]
pub struct MeetingSpeakers {
    pub job: Option<JobView>,
    pub speakers: Vec<SpeakerView>,
}

fn pool_of(state: &State<'_, AppState>) -> SqlitePool {
    state.db_manager.pool().clone()
}

/// Whether the feature runs. The models hash is computed on a blocking thread.
async fn active_now() -> bool {
    tokio::task::spawn_blocking(super::is_active).await.unwrap_or(false)
}

#[tauri::command]
pub async fn speakers_get_settings(state: State<'_, AppState>) -> Result<SpeakerSettings, String> {
    let models_ready = tokio::task::spawn_blocking(models::models_ready).await.map_err(|e| e.to_string())?;
    let last = SpeakersRepository::last_done_job(&pool_of(&state)).await.map_err(|e| e.to_string())?;
    let last_analysis = last.and_then(|j| {
        Some(LastAnalysis { backend: j.backend?, run_seconds: j.run_seconds?, audio_seconds: j.audio_seconds? })
    });
    Ok(SpeakerSettings {
        enabled: settings::is_enabled(),
        models_ready,
        downloading: DOWNLOADING.load(Ordering::SeqCst),
        last_analysis,
    })
}

/// Turns the feature on or off. Switching on with missing models starts the download and returns at once.
#[tauri::command]
pub async fn speakers_set_enabled<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    settings::save_enabled(&app, enabled)?;
    let _ = app.emit("speaker-settings-changed", json!({}));
    if enabled {
        tauri::async_runtime::spawn(async move {
            let ready = tokio::task::spawn_blocking(models::models_ready).await.unwrap_or(false);
            if !ready {
                download(app).await;
            }
        });
    }
    Ok(())
}

/// Starts the model download and returns at once. A second request while one runs is ignored.
#[tauri::command]
pub async fn speakers_download_models<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    tauri::async_runtime::spawn(download(app));
    Ok(())
}

async fn download<R: Runtime>(app: AppHandle<R>) {
    if DOWNLOADING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    let _ = app.emit("speaker-settings-changed", json!({}));
    let result = fetch_models(&app).await;
    DOWNLOADING.store(false, Ordering::SeqCst);
    let (ok, error) = match result {
        Ok(()) => (true, None),
        Err(e) => {
            log::warn!("speakers: model download failed: {e}");
            (false, Some(e))
        }
    };
    let _ = app.emit("speaker-models-done", json!({ "ok": ok, "error": error }));
    let _ = app.emit("speaker-settings-changed", json!({}));
}

async fn fetch_models<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let dir = models::models_dir().ok_or("the speaker models folder is not set")?;
    let progress_app = app.clone();
    models::download_models(&models::download_client(), &dir, move |done, total| {
        let percent = if total == 0 { 0 } else { done * 100 / total };
        let _ = progress_app.emit(
            "speaker-models-progress",
            json!({ "percent": percent, "downloaded_bytes": done, "total_bytes": total }),
        );
    })
    .await
}

#[tauri::command]
pub async fn speakers_delete_models<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if DOWNLOADING.load(Ordering::SeqCst) {
        return Err("A model download is running".into());
    }
    if job::is_running() {
        return Err("Speaker analysis is running".into());
    }
    let dir = models::models_dir().ok_or("the speaker models folder is not set")?;
    models::delete_models(&dir)?;
    let _ = app.emit("speaker-settings-changed", json!({}));
    Ok(())
}

#[tauri::command]
pub async fn speakers_get_meeting(state: State<'_, AppState>, meeting_id: String) -> Result<MeetingSpeakers, String> {
    let pool = pool_of(&state);
    let job = SpeakersRepository::get_job(&pool, &meeting_id)
        .await
        .map_err(|e| e.to_string())?
        .map(|j| JobView { status: j.status, reason: j.reason, percent: job::percent_of(&meeting_id) });
    let speakers = SpeakersRepository::meeting_speakers(&pool, &meeting_id).await.map_err(|e| e.to_string())?;
    Ok(MeetingSpeakers { job, speakers })
}

#[tauri::command]
pub async fn speakers_identify<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, meeting_id: String) -> Result<(), String> {
    let pool = pool_of(&state);
    let active = active_now().await;
    let base = job::recordings_base(&app).await;
    let recording = job::recording_of(&pool, &meeting_id, Some(base)).await.map_err(|e| e.to_string())?.is_some();
    job::identify(&pool, &meeting_id, active, recording).await?;
    job::wake();
    let _ = app.emit("speakers-updated", json!({ "meeting_id": meeting_id }));
    Ok(())
}

/// Names a speaker of this meeting: an existing person (`person_id`) or a new one (`new_name`). Returns the person's id.
#[tauri::command]
pub async fn speakers_name(
    state: State<'_, AppState>,
    meeting_id: String,
    idx: i64,
    person_id: Option<String>,
    new_name: Option<String>,
) -> Result<String, String> {
    let target = match (person_id, new_name) {
        (Some(id), None) => NameTarget::Person(id),
        (None, Some(name)) => NameTarget::New(name),
        _ => return Err("Name a speaker with either an existing person or a new name".into()),
    };
    SpeakersRepository::name_speaker(&pool_of(&state), &meeting_id, idx, target).await
}

#[tauri::command]
pub async fn speakers_move_line(state: State<'_, AppState>, transcript_id: String, idx: Option<i64>) -> Result<(), String> {
    SpeakersRepository::move_line(&pool_of(&state), &transcript_id, idx).await
}

#[tauri::command]
pub async fn people_list(state: State<'_, AppState>) -> Result<Vec<PersonView>, String> {
    SpeakersRepository::list_people(&pool_of(&state)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn people_rename(state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    SpeakersRepository::rename_person(&pool_of(&state), &id, &name).await
}

#[tauri::command]
pub async fn people_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    SpeakersRepository::delete_person(&pool_of(&state), &id).await
}

#[tauri::command]
pub async fn people_merge(state: State<'_, AppState>, from: String, into: String) -> Result<(), String> {
    SpeakersRepository::merge_people(&pool_of(&state), &from, &into).await
}
