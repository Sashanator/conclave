use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use anyhow::Result;
use log::{error, info, warn};
use super::recording_state::RecordingState;
use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct AudioLevelData {
    pub device_name: String,
    pub device_type: String, // "input" or "output"
    pub rms_level: f32,     // RMS level (0.0 to 1.0)
    pub peak_level: f32,    // Peak level (0.0 to 1.0)
    pub is_active: bool,    // Whether audio is being detected
}

#[derive(Debug, Serialize, Clone)]
pub struct AudioLevelUpdate {
    pub timestamp: u64,
    pub levels: Vec<AudioLevelData>,
}

// Simple global monitoring state
static IS_MONITORING: AtomicBool = AtomicBool::new(false);

/// Start audio level monitoring for specified devices
pub async fn start_monitoring<R: Runtime>(
    app_handle: AppHandle<R>,
    device_names: Vec<String>,
) -> Result<()> {
    info!("Starting simplified audio level monitoring for devices: {:?}", device_names);

    // Stop any existing monitoring
    IS_MONITORING.store(false, Ordering::SeqCst);

    // Wait a bit for any existing tasks to stop
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Start new monitoring
    IS_MONITORING.store(true, Ordering::SeqCst);

    // For now, create fake level data to test the UI
    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        let mut counter: f32 = 0.0;

        while IS_MONITORING.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            counter += 0.1;
            let fake_level = (counter.sin().abs() * 0.8) as f32; // Simulate varying levels

            let levels: Vec<AudioLevelData> = device_names.iter().map(|name| {
                AudioLevelData {
                    device_name: name.clone(),
                    device_type: "input".to_string(),
                    rms_level: fake_level,
                    peak_level: fake_level * 1.2,
                    is_active: fake_level > 0.1,
                }
            }).collect();

            let update = AudioLevelUpdate {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                levels,
            };

            if let Err(e) = app_handle_clone.emit("audio-levels", &update) {
                error!("Failed to emit audio levels: {}", e);
                break;
            }
        }

        info!("Audio level monitoring task ended");
    });

    Ok(())
}

/// Stop audio level monitoring
pub async fn stop_monitoring() -> Result<()> {
    info!("Stopping simplified audio level monitoring");
    IS_MONITORING.store(false, Ordering::SeqCst);
    Ok(())
}

/// Check if currently monitoring
pub fn is_monitoring() -> bool {
    IS_MONITORING.load(Ordering::SeqCst)
}
/// The two meter rows for one reading: microphone and system audio, each clamped to 0..1.
pub fn level_update(mic: f32, system: f32, timestamp: u64) -> AudioLevelUpdate {
    let row = |name: &str, device_type: &str, v: f32| {
        let v = v.clamp(0.0, 1.0);
        AudioLevelData { device_name: name.to_string(), device_type: device_type.to_string(), rms_level: v, peak_level: v, is_active: v > 0.01 }
    };
    AudioLevelUpdate { timestamp, levels: vec![row("microphone", "input", mic), row("system", "output", system)] }
}

/// Emits the real `audio-levels` (the pipeline's own RMS) at most 10 Hz while this recording runs.
/// The task ends with the recording: every recording has its own `RecordingState`.
/// Ticks (100 ms each) of an unchanged reading after which the meter drops to zero: a lost device must not freeze it.
const STALE_TICKS: u32 = 10;

/// Decides what to emit on each tick: a changed reading once, then zeros once if it stays unchanged for a second.
#[derive(Default)]
struct Throttle {
    seen: Option<(f32, f32)>,
    emitted: Option<(f32, f32)>,
    stale: u32,
}

impl Throttle {
    fn step(&mut self, now: (f32, f32)) -> Option<(f32, f32)> {
        if self.seen != Some(now) {
            self.seen = Some(now);
            self.stale = 0;
        } else {
            self.stale += 1;
            if self.stale == STALE_TICKS && self.emitted != Some((0.0, 0.0)) {
                self.emitted = Some((0.0, 0.0));
                return self.emitted;
            }
            return None;
        }
        if self.emitted == Some(now) { None } else { self.emitted = Some(now); Some(now) }
    }
}

pub fn spawn_recording_levels<R: Runtime>(app: AppHandle<R>, state: Arc<RecordingState>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(100));
        let mut throttle = Throttle::default();
        let mut warned = false;
        while state.is_recording() {
            tick.tick().await;
            // Paused: show silence once, then stay quiet until audio flows again.
            let now = if state.is_paused() { (0.0, 0.0) } else { state.levels() };
            let Some(now) = throttle.step(now) else { continue };
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
            if let Err(e) = app.emit("audio-levels", &level_update(now.0, now.1, ts)) {
                if !warned {
                    warn!("Could not emit audio-levels: {}", e);
                    warned = true;
                }
            }
        }
        // Stopped: one last zero so the meter does not freeze on its final reading.
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let _ = app.emit("audio-levels", &level_update(0.0, 0.0, ts));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_emits_changes_once_and_zeros_after_a_stale_second() {
        let mut t = Throttle::default();
        assert_eq!(t.step((0.2, 0.1)), Some((0.2, 0.1)));
        for _ in 0..(STALE_TICKS - 1) {
            assert_eq!(t.step((0.2, 0.1)), None);
        }
        assert_eq!(t.step((0.2, 0.1)), Some((0.0, 0.0)), "frozen reading drops to zero once");
        assert_eq!(t.step((0.2, 0.1)), None);
        assert_eq!(t.step((0.3, 0.1)), Some((0.3, 0.1)), "audio flowing again is emitted");
        assert_eq!(t.step((0.0, 0.0)), Some((0.0, 0.0)));
        for _ in 0..STALE_TICKS + 2 {
            assert_eq!(t.step((0.0, 0.0)), None, "silence is not re-emitted");
        }
    }

    #[test]
    fn level_update_shape() {
        let u = level_update(1.5, 0.0, 7);
        assert_eq!(u.timestamp, 7);
        assert_eq!(u.levels.len(), 2);
        assert_eq!(u.levels[0].device_type, "input");
        assert_eq!(u.levels[0].rms_level, 1.0);
        assert!(u.levels[0].is_active);
        assert_eq!(u.levels[1].device_type, "output");
        assert!(!u.levels[1].is_active);
    }
}
