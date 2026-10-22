use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow};
use log::{info, warn, error};
use super::encode::encode_single_audio;
use super::recording_state::AudioChunk;
use serde::{Serialize, Deserialize};

use super::ffmpeg::find_ffmpeg_path;
use super::meeting_folder::{audio_files, checkpoint_files, delete_checkpoint_files, guarded_meeting_folder, is_plain_dir};

/// Audio data without device type (we only store mixed audio)
#[derive(Clone)]
struct AudioData {
    data: Vec<f32>,
    // sample_rate: u32,
}

/// Incremental audio saver that writes checkpoints every 30 seconds
/// to minimize memory usage and enable crash recovery
pub struct IncrementalAudioSaver {
    checkpoint_buffer: Vec<AudioData>,
    checkpoint_interval_samples: usize,  // 30s at 48kHz = 1,440,000 samples
    next_checkpoint_samples: usize,      // buffered samples that trigger the next attempt
    checkpoint_count: u32,
    checkpoints_dir: PathBuf,
    meeting_folder: PathBuf,
    sample_rate: u32,
}

impl IncrementalAudioSaver {
    /// Create a new incremental saver
    ///
    /// # Arguments
    /// * `meeting_folder` - Path to the meeting folder (contains .checkpoints/)
    /// * `sample_rate` - Sample rate of audio (typically 48000)
    pub fn new(meeting_folder: PathBuf, sample_rate: u32) -> Result<Self> {
        let checkpoints_dir = meeting_folder.join(".checkpoints");

        // Verify checkpoints directory exists
        if !checkpoints_dir.exists() {
            return Err(anyhow!("Checkpoints directory does not exist"));
        }

        Ok(Self {
            checkpoint_buffer: Vec::new(),
            checkpoint_interval_samples: sample_rate as usize * 30, // 30 seconds
            next_checkpoint_samples: sample_rate as usize * 30,
            checkpoint_count: 0,
            checkpoints_dir,
            meeting_folder,
            sample_rate,
        })
    }

    /// Add an audio chunk to the buffer
    /// Automatically saves a checkpoint when buffer reaches 30 seconds
    pub fn add_chunk(&mut self, chunk: AudioChunk) -> Result<()> {
        let audio_data = AudioData {
            data: chunk.data,
            // sample_rate: chunk.sample_rate,
        };

        self.checkpoint_buffer.push(audio_data);

        // Calculate total samples in buffer
        let total_samples: usize = self.checkpoint_buffer
            .iter()
            .map(|c| c.data.len())
            .sum();

        // Save checkpoint when buffer reaches threshold (30 seconds)
        if total_samples >= self.next_checkpoint_samples {
            match self.save_checkpoint() {
                Ok(()) => {
                    self.checkpoint_buffer.clear();
                    self.next_checkpoint_samples = self.checkpoint_interval_samples;
                }
                Err(e) => {
                    // Keep the audio, but wait for another full interval before retrying.
                    self.next_checkpoint_samples = total_samples + self.checkpoint_interval_samples;
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    /// Writes the buffered audio as the next checkpoint now, so it is on disk before any slow
    /// stop work. Does nothing when the buffer is empty.
    pub fn checkpoint_buffered(&mut self) -> Result<()> {
        if self.checkpoint_buffer.is_empty() {
            return Ok(());
        }
        self.save_checkpoint()?;
        self.checkpoint_buffer.clear();
        self.next_checkpoint_samples = self.checkpoint_interval_samples;
        Ok(())
    }

    /// Save current buffer as a checkpoint file
    fn save_checkpoint(&mut self) -> Result<()> {
        // Concatenate all chunks in buffer
        let audio_data: Vec<f32> = self.checkpoint_buffer
            .iter()
            .flat_map(|c| &c.data)
            .cloned()
            .collect();

        if audio_data.is_empty() {
            warn!("Attempted to save empty checkpoint, skipping");
            return Ok(());
        }

        // Generate checkpoint filename
        let checkpoint_path = self.checkpoints_dir
            .join(format!("audio_chunk_{:03}.mp4", self.checkpoint_count));

        // Encode and save checkpoint
        encode_single_audio(
            bytemuck::cast_slice(&audio_data),
            self.sample_rate,
            1,  // mono
            &checkpoint_path
        )?;

        let duration_seconds = audio_data.len() as f32 / self.sample_rate as f32;
        self.checkpoint_count += 1;

        info!("Saved checkpoint {}: {:.2}s of audio ({} samples)",
              self.checkpoint_count,
              duration_seconds,
              audio_data.len());

        Ok(())
    }

    /// Finalize the recording: save final checkpoint, merge all checkpoints, cleanup
    ///
    /// Returns the path to the final merged audio.mp4 file
    pub async fn finalize(&mut self) -> Result<PathBuf> {
        info!("Finalizing incremental recording...");

        // Save final buffer if not empty
        if !self.checkpoint_buffer.is_empty() {
            info!("Saving final checkpoint with remaining {} chunks", self.checkpoint_buffer.len());
            self.save_checkpoint()?;
            self.checkpoint_buffer.clear();
        }

        if self.checkpoint_count == 0 {
            return Err(anyhow!("No audio checkpoints to merge - recording may have failed"));
        }

        // Merge all checkpoints using FFmpeg concat
        let final_audio_path = self.meeting_folder.join("audio.mp4");
        self.merge_checkpoints(&final_audio_path).await?;

        self.remove_merged_checkpoints(&crate::audio::recording_preferences::get_default_recordings_folder());

        info!("Finalized recording");

        Ok(final_audio_path)
    }

    /// Deletes Conclave's own chunk files after a merge, only inside the meeting folder when it
    /// passes the guard against `recordings_base`. Non-fatal: the user can delete them by hand.
    fn remove_merged_checkpoints(&self, recordings_base: &Path) {
        info!("Cleaning up {} checkpoint files", self.checkpoint_count);
        match guarded_meeting_folder(&self.meeting_folder.to_string_lossy(), recordings_base) {
            Some(folder) => {
                if let Err(e) = delete_checkpoint_files(&folder) {
                    warn!("Failed to clean up checkpoints directory: {}", e);
                }
            }
            None => warn!("Checkpoints kept: the meeting folder is outside the recordings folder"),
        }
    }

    /// Merge all checkpoint files into final audio.mp4 using FFmpeg concat
    /// Uses concat demuxer for fast merging without re-encoding
    async fn merge_checkpoints(&self, output: &PathBuf) -> Result<()> {
        info!("Merging {} checkpoints into final audio file...", self.checkpoint_count);

        // Create concat list file for FFmpeg
        let list_file = self.checkpoints_dir.join("concat_list.txt");
        let mut list_content = String::new();

        for i in 0..self.checkpoint_count {
            let checkpoint_path = self.checkpoints_dir
                .join(format!("audio_chunk_{:03}.mp4", i));

            // Verify checkpoint exists
            if !checkpoint_path.exists() {
                return Err(anyhow!("Checkpoint file missing: {}", checkpoint_path.file_name().and_then(|n| n.to_str()).unwrap_or("?")));
            }

            // Use absolute path for FFmpeg (required for safe mode)
            let abs_path = checkpoint_path.canonicalize()?;
            list_content.push_str(&format!("file '{}'\n", abs_path.display()));
        }

        std::fs::write(&list_file, list_content)?;

        let ffmpeg_path = find_ffmpeg_path()
            .ok_or_else(|| anyhow!("FFmpeg not found next to Conclave.exe — reinstall Conclave."))?;
        info!("Using FFmpeg at: {:?}", ffmpeg_path);

        // Run FFmpeg concat command
        // Using concat demuxer with copy codec for fast merging (no re-encoding)
        
        let mut command = std::process::Command::new(ffmpeg_path);
        
        command.args(&[
            "-f", "concat",          // Use concat demuxer
            "-safe", "0",            // Allow absolute paths
            "-i", list_file.to_str().unwrap(),
            "-c", "copy",            // Copy codec - no re-encoding!
            "-y",                    // Overwrite output file
            output.to_str().unwrap()
        ]);

        // Hide console window on Windows to prevent CMD popup during finalization
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let ffmpeg_output = command.output()?;

        if !ffmpeg_output.status.success() {
            let stderr = String::from_utf8_lossy(&ffmpeg_output.stderr);
            error!("FFmpeg merge failed: {}", stderr);
            return Err(anyhow!("FFmpeg concat failed: {}", stderr));
        }

        // Verify output file was created
        if !output.exists() {
            return Err(anyhow!("Merged audio file was not created"));
        }

        info!("Successfully merged {} checkpoints", self.checkpoint_count);

        Ok(())
    }

    /// Get the meeting folder path
    pub fn get_meeting_folder(&self) -> &PathBuf {
        &self.meeting_folder
    }

    /// Get current checkpoint count
    pub fn get_checkpoint_count(&self) -> u32 {
        self.checkpoint_count
    }
}

/// Audio recovery status for transcript recovery feature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRecoveryStatus {
    pub status: String, // "success" | "partial" | "failed" | "none"
    pub chunk_count: u32,
    pub estimated_duration_seconds: f64,
    pub audio_file_path: Option<String>,
    pub message: String,
}

/// True when the meeting has checkpoint chunks but no audio file, i.e. the Stop-time merge failed.
fn needs_checkpoint_merge(folder: &Path) -> bool {
    audio_files(folder).is_empty() && !checkpoint_files(&folder.join(".checkpoints")).is_empty()
}

/// Merges leftover checkpoint chunks before a lookup of the meeting's audio. Skipped while a
/// recording runs, because the live saver still owns those chunks.
pub async fn merge_leftover_checkpoints(folder: &Path) {
    merge_leftover_checkpoints_with(folder, |dir| async move {
        recover_audio_from_checkpoints(dir.to_string_lossy().into_owned(), 48000).await
    })
    .await;
}

/// `merge` does the actual merge. It is a parameter so the cleanup after a failed merge can be
/// tested without ffmpeg.
async fn merge_leftover_checkpoints_with<F, Fut>(folder: &Path, merge: F)
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<AudioRecoveryStatus, String>>,
{
    if super::recording_commands::is_recording_now() || !needs_checkpoint_merge(folder) {
        return;
    }
    match merge(folder.to_path_buf()).await {
        Ok(status) if status.status == "success" => {
            info!("Merged leftover checkpoints in {}: {}", folder.display(), status.message);
        }
        outcome => {
            let reason = match outcome {
                Ok(status) => status.message,
                Err(e) => e,
            };
            warn!("Merging leftover checkpoints in {} failed: {}", folder.display(), reason);
            // No audio file existed before (checked above), so this partial output is ours. The chunks stay.
            let _ = std::fs::remove_file(folder.join("audio.mp4"));
        }
    }
}

/// Recover audio from checkpoint files
/// This is called by the transcript recovery system to merge audio chunks after a crash
#[tauri::command]
pub async fn recover_audio_from_checkpoints(
    meeting_folder: String,
    _sample_rate: u32
) -> Result<AudioRecoveryStatus, String> {
    info!("Starting audio recovery");

    // The folder comes from the webview: it must pass the guard before anything is read or written.
    let folder_path = guarded_meeting_folder(&meeting_folder, &crate::audio::recording_preferences::get_default_recordings_folder())
        .ok_or_else(|| format!("Recovery refused: {meeting_folder} is not a meeting folder in the recordings folder"))?;
    let checkpoints_dir = folder_path.join(".checkpoints");

    // Check if checkpoints directory exists (a link is not ours to read or write through)
    if !is_plain_dir(&checkpoints_dir) {
        info!("No checkpoints directory found");
        return Ok(AudioRecoveryStatus {
            status: "none".to_string(),
            chunk_count: 0,
            estimated_duration_seconds: 0.0,
            audio_file_path: None,
            message: "No audio checkpoints found".to_string(),
        });
    }

    // Scan for checkpoint files
    let mut checkpoint_files: Vec<_> = std::fs::read_dir(&checkpoints_dir)
        .map_err(|e| format!("Failed to read checkpoints directory: {}", e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().and_then(|s| s.to_str()) == Some("mp4")
        })
        .collect();

    if checkpoint_files.is_empty() {
        info!("No checkpoint files found");
        return Ok(AudioRecoveryStatus {
            status: "none".to_string(),
            chunk_count: 0,
            estimated_duration_seconds: 0.0,
            audio_file_path: None,
            message: "No audio checkpoint files found".to_string(),
        });
    }

    // Sort by filename (audio_chunk_000.mp4, audio_chunk_001.mp4, etc.)
    checkpoint_files.sort_by_key(|entry| entry.path());

    let chunk_count = checkpoint_files.len() as u32;
    let estimated_duration = (chunk_count as f64) * 30.0; // 30 seconds per chunk

    info!("Found {} checkpoint files, estimated duration: {:.2}s", chunk_count, estimated_duration);

    // Create FFmpeg concat file
    let concat_file_path = checkpoints_dir.join("concat_list.txt");
    let mut concat_content = String::new();

    for entry in &checkpoint_files {
        let path = entry.path().canonicalize()
            .map_err(|e| format!("Failed to canonicalize path: {}", e))?;
        concat_content.push_str(&format!("file '{}'\n", path.display()));
    }

    std::fs::write(&concat_file_path, concat_content)
        .map_err(|e| format!("Failed to write concat file: {}", e))?;

    // Run FFmpeg to merge chunks
    let output_path = folder_path.join("audio.mp4");
    let output_path_str = output_path.to_str()
        .ok_or("Invalid output path")?
        .to_string();

    let ffmpeg_path = find_ffmpeg_path()
        .ok_or_else(|| "FFmpeg not found next to Conclave.exe — reinstall Conclave.".to_string())?;
    info!("Using FFmpeg at: {:?}", ffmpeg_path);

    let mut command = std::process::Command::new(ffmpeg_path);

    command.args(&[
        "-f", "concat",
        "-safe", "0",
        "-i", concat_file_path.to_str().unwrap(),
        "-c", "copy",
        "-y", // Overwrite if exists
        &output_path_str
    ]);

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let ffmpeg_result = command.output();

    match ffmpeg_result {
        Ok(output) if output.status.success() => {
            // Clean up concat file
            let _ = std::fs::remove_file(concat_file_path);

            info!("Successfully recovered audio");

            Ok(AudioRecoveryStatus {
                status: "success".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: Some(output_path_str),
                message: format!("Successfully recovered {} audio chunks", chunk_count),
            })
        }
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            error!("FFmpeg recovery failed: {}", error);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("FFmpeg failed: {}", error),
            })
        }
        Err(e) => {
            error!("Failed to run FFmpeg: {}", e);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("Failed to run FFmpeg: {}", e),
            })
        }
    }
}

/// Clean up checkpoint files after successful recording or recovery
/// This command is called by the frontend after successful save to clean up checkpoint files
#[tauri::command]
pub async fn cleanup_checkpoints(meeting_folder: String) -> Result<(), String> {
    info!("Cleaning up checkpoints");
    // The path comes from the webview: guarded, and only Conclave's own chunk files are deleted.
    let base = crate::audio::recording_preferences::get_default_recordings_folder();
    super::meeting_folder::cleanup_checkpoints_guarded(&meeting_folder, &base)
}

/// Check if a meeting folder has audio checkpoint files
/// Returns true if .checkpoints/ directory exists and contains .mp4 files
#[tauri::command]
pub async fn has_audio_checkpoints(meeting_folder: String) -> Result<bool, String> {
    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(".checkpoints");

    // Check if checkpoints directory exists
    if !checkpoints_dir.exists() {
        return Ok(false);
    }

    // Scan for .mp4 checkpoint files
    let has_mp4_files = std::fs::read_dir(&checkpoints_dir)
        .map_err(|e| format!("Failed to read checkpoints directory: {}", e))?
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry.path().extension().and_then(|s| s.to_str()) == Some("mp4")
        });

    Ok(has_mp4_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use super::super::recording_state::DeviceType;

    #[tokio::test]
    async fn test_checkpoint_creation() {
        // Create temp meeting folder
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Test_Meeting");
        std::fs::create_dir_all(&meeting_folder).unwrap();
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();

        let mut saver = IncrementalAudioSaver::new(
            meeting_folder.clone(),
            48000
        ).unwrap();

        // Add 60 seconds worth of audio (should create 2 checkpoints)
        for i in 0..120 {  // 120 chunks of 0.5s each
            let chunk = AudioChunk {
                data: vec![0.5f32; 24000],  // 0.5s at 48kHz
                sample_rate: 48000,
                timestamp: i as f64 * 0.5,  // timestamp in seconds
                chunk_id: i as u64,
                device_type: DeviceType::Microphone,
            };
            saver.add_chunk(chunk).unwrap();
        }

        // Verify 2 checkpoints created
        assert_eq!(saver.checkpoint_count, 2);

        // Finalize and verify merge
        let final_path = saver.finalize().await.unwrap();
        assert!(final_path.exists());

        // The temp folder is outside the recordings folder, so the guard keeps the chunks...
        assert!(meeting_folder.join(".checkpoints").exists());
        // ...and inside a base that contains it, the merged chunks and the empty folder go.
        saver.remove_merged_checkpoints(temp_dir.path());
        assert!(!meeting_folder.join(".checkpoints").exists());
    }

    #[tokio::test]
    async fn test_empty_recording() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Empty_Test");
        std::fs::create_dir_all(&meeting_folder).unwrap();
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();

        let mut saver = IncrementalAudioSaver::new(
            meeting_folder.clone(),
            48000
        ).unwrap();

        // Try to finalize without adding any chunks
        let result = saver.finalize().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No audio checkpoints"));
    }
    #[tokio::test]
    async fn recovery_refuses_a_folder_outside_the_recordings_folder() {
        let outside = tempdir().unwrap();
        let cp = outside.path().join(".checkpoints");
        std::fs::create_dir_all(&cp).unwrap();
        std::fs::write(cp.join("audio_chunk_000.mp4"), "x").unwrap();
        let result = recover_audio_from_checkpoints(outside.path().to_string_lossy().to_string(), 48000).await;
        assert!(result.is_err());
        assert!(!cp.join("concat_list.txt").exists());
        assert!(!outside.path().join("audio.mp4").exists());
    }

    #[test]
    fn failed_checkpoint_is_not_retried_until_another_interval_of_audio() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Retry_Test");
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();
        // 100 Hz makes the 30 s interval 3000 samples.
        let mut saver = IncrementalAudioSaver::new(meeting_folder, 100).unwrap();
        // A missing folder makes every checkpoint encode fail.
        saver.checkpoints_dir = temp_dir.path().join("missing");
        let chunk = |n: usize| AudioChunk {
            data: vec![0.5f32; n],
            sample_rate: 100,
            timestamp: 0.0,
            chunk_id: 0,
            device_type: DeviceType::Microphone,
        };

        assert!(saver.add_chunk(chunk(3000)).is_err());
        // Less than a full interval since the failure: no retry, and the audio is kept.
        assert!(saver.add_chunk(chunk(10)).is_ok());
        assert_eq!(saver.checkpoint_buffer.len(), 2);
    }

    #[test]
    fn leftover_checkpoints_without_audio_choose_the_merge() {
        let temp_dir = tempdir().unwrap();
        let folder = temp_dir.path().join("Failed_Merge");
        std::fs::create_dir_all(folder.join(".checkpoints")).unwrap();
        std::fs::write(folder.join(".checkpoints").join("audio_chunk_000.mp4"), b"chunk").unwrap();
        assert!(needs_checkpoint_merge(&folder));

        std::fs::write(folder.join("audio.mp4"), b"merged").unwrap();
        assert!(!needs_checkpoint_merge(&folder));

        let no_chunks = temp_dir.path().join("No_Chunks");
        std::fs::create_dir_all(no_chunks.join(".checkpoints")).unwrap();
        assert!(!needs_checkpoint_merge(&no_chunks));
    }

    #[tokio::test]
    async fn checkpoint_buffered_writes_the_tail_once_and_finalize_adds_nothing() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Tail_Test");
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();
        let checkpoints = meeting_folder.join(".checkpoints");
        // One second stays buffered: far below the 30 s checkpoint interval.
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();
        saver.add_chunk(AudioChunk {
            data: vec![0.5f32; 48000],
            sample_rate: 48000,
            timestamp: 0.0,
            chunk_id: 0,
            device_type: DeviceType::Microphone,
        }).unwrap();
        assert_eq!(saver.checkpoint_count, 0);

        saver.checkpoint_buffered().unwrap();
        assert_eq!(saver.checkpoint_count, 1);
        assert!(saver.checkpoint_buffer.is_empty());
        assert!(checkpoints.join("audio_chunk_000.mp4").exists());

        // Nothing buffered: no empty or duplicate chunk.
        saver.checkpoint_buffered().unwrap();
        assert_eq!(saver.checkpoint_count, 1);

        saver.finalize().await.unwrap();
        assert!(!checkpoints.join("audio_chunk_001.mp4").exists());
    }

    fn recovery_status(status: &str) -> AudioRecoveryStatus {
        AudioRecoveryStatus {
            status: status.to_string(),
            chunk_count: 1,
            estimated_duration_seconds: 30.0,
            audio_file_path: None,
            message: status.to_string(),
        }
    }

    #[tokio::test]
    async fn a_failed_merge_removes_its_partial_audio_and_keeps_the_chunks() {
        let temp_dir = tempdir().unwrap();
        let folder = temp_dir.path().join("Failed_Merge");
        std::fs::create_dir_all(folder.join(".checkpoints")).unwrap();
        std::fs::write(folder.join(".checkpoints").join("audio_chunk_000.mp4"), b"chunk").unwrap();

        merge_leftover_checkpoints_with(&folder, |dir| async move {
            std::fs::write(dir.join("audio.mp4"), b"partial").unwrap();
            Ok(recovery_status("failed"))
        }).await;

        assert!(!folder.join("audio.mp4").exists());
        assert!(folder.join(".checkpoints").join("audio_chunk_000.mp4").exists());
    }

    #[tokio::test]
    async fn a_successful_merge_keeps_its_audio() {
        let temp_dir = tempdir().unwrap();
        let folder = temp_dir.path().join("Good_Merge");
        std::fs::create_dir_all(folder.join(".checkpoints")).unwrap();
        std::fs::write(folder.join(".checkpoints").join("audio_chunk_000.mp4"), b"chunk").unwrap();

        merge_leftover_checkpoints_with(&folder, |dir| async move {
            std::fs::write(dir.join("audio.mp4"), b"merged").unwrap();
            Ok(recovery_status("success"))
        }).await;

        assert!(folder.join("audio.mp4").exists());
    }
}
