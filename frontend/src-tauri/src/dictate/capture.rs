// Dictate's microphone: one cpal stream stays open and keeps a PRE_ROLL_MS pre-roll, so the first
// word of a dictation is not clipped. Samples live in memory only and are never written anywhere.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{info, warn};

use super::{MAX_DICTATION_SECS, PRE_ROLL_MS};
use crate::audio::{
    default_input_device, get_device_and_config, parse_audio_device, AudioDevice, DeviceType,
};

/// Window for the level meter.
const LEVEL_WINDOW_MS: usize = 50;
/// The session buffer is reserved this long at `begin`, so it does not reallocate under the lock.
const SESSION_RESERVE_SECS: usize = 30;

fn session_reserve(rate: u32) -> usize {
    SESSION_RESERVE_SECS * rate as usize
}

/// The newest samples, up to a fixed capacity. Oldest samples are dropped first.
pub struct PreRollBuffer {
    samples: VecDeque<f32>,
    capacity: usize,
}

impl PreRollBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { samples: VecDeque::with_capacity(capacity), capacity }
    }

    /// Test helper: the callback pushes one sample at a time.
    #[cfg(test)]
    pub fn push(&mut self, s: &[f32]) {
        for &x in s {
            self.push_one(x);
        }
    }

    pub fn push_one(&mut self, x: f32) {
        self.samples.push_back(x);
        if self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
    }

    /// Appends everything buffered, oldest first, to `out`. The buffer is empty afterwards.
    pub fn move_into(&mut self, out: &mut Vec<f32>) {
        out.extend(self.samples.drain(..));
    }

    /// RMS of the newest `n` samples, or 0 when there are none.
    fn rms_newest(&self, n: usize) -> f32 {
        let take = n.min(self.samples.len());
        if take == 0 {
            return 0.0;
        }
        let sum: f32 = self.samples.iter().rev().take(take).map(|s| s * s).sum();
        (sum / take as f32).sqrt()
    }
}

/// Averages each interleaved frame to one mono sample and hands it to `sink`. Allocates nothing,
/// because it runs in the audio callback. A trailing partial frame is ignored.
pub fn downmix_into<T: Copy>(
    interleaved: &[T],
    channels: u16,
    to_f32: fn(T) -> f32,
    mut sink: impl FnMut(f32),
) {
    let ch = channels.max(1) as usize;
    for frame in interleaved.chunks_exact(ch) {
        sink(frame.iter().map(|&s| to_f32(s)).sum::<f32>() / ch as f32);
    }
}

pub fn i16_to_f32(s: i16) -> f32 {
    s as f32 / 32768.0
}

pub fn u16_to_f32(s: u16) -> f32 {
    (s as f32 - 32768.0) / 32768.0
}

/// The device to open: the preferred one when it is enumerated, else the default input.
pub fn choose_device(preferred: Option<&str>, available: &[String], default: Option<&str>) -> Option<String> {
    match preferred {
        Some(p) if available.iter().any(|a| a == p) => Some(p.to_string()),
        _ => default.map(str::to_string),
    }
}

/// The pre-roll plus the live samples of one dictation. Samples past the session cap are dropped.
/// The pre-roll is refilled during a session too, so it holds the newest audio at every `begin`.
pub struct SessionBuffer {
    pre_roll: PreRollBuffer,
    session: Option<Vec<f32>>,
    cap: usize,
}

impl SessionBuffer {
    pub fn new(pre_roll_capacity: usize, cap: usize) -> Self {
        Self { pre_roll: PreRollBuffer::new(pre_roll_capacity), session: None, cap }
    }

    /// Test helper: the callback pushes one sample at a time.
    #[cfg(test)]
    pub fn push(&mut self, s: &[f32]) {
        for &x in s {
            self.push_one(x);
        }
    }

    pub fn push_one(&mut self, x: f32) {
        self.pre_roll.push_one(x);
        if let Some(buf) = &mut self.session {
            if buf.len() < self.cap {
                buf.push(x);
            }
        }
    }

    /// Starts a session with the pre-roll, appended to `session`. The caller allocates `session` before
    /// taking the lock. Calling this during a session discards that session.
    pub fn begin(&mut self, mut session: Vec<f32>) {
        self.pre_roll.move_into(&mut session);
        self.session = Some(session);
    }

    pub fn end(&mut self) -> Vec<f32> {
        self.session.take().unwrap_or_default()
    }

    pub fn cancel(&mut self) {
        self.session = None;
    }

    pub fn level(&self, window: usize) -> f32 {
        self.pre_roll.rms_newest(window)
    }
}

pub struct CapturedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

type Shared = Arc<Mutex<SessionBuffer>>;

fn lock(buf: &Shared) -> MutexGuard<'_, SessionBuffer> {
    buf.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the capture thread reports once the stream is playing.
struct Ready {
    name: String,
    rate: u32,
    buf: Shared,
}

pub struct MicCapture {
    name: String,
    rate: u32,
    buf: Shared,
    alive: Arc<AtomicBool>,
    // Dropping this sender tells the capture thread to drop its stream and exit.
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl MicCapture {
    /// Opens the preferred input, or the default input when it is missing or `preferred` is None.
    /// `device_name()` reports the device actually opened, so a caller can compare it with its
    /// preference and reopen later.
    pub fn open(preferred: Option<String>) -> Result<MicCapture, String> {
        let alive = Arc::new(AtomicBool::new(true));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("dictate-mic".into())
            .spawn({
                let alive = alive.clone();
                move || run(preferred, alive, ready_tx, stop_rx)
            })
            .map_err(|e| format!("Could not start the microphone thread: {e}"))?;
        let ready = match ready_rx.recv() {
            Ok(Ok(ready)) => ready,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(_) => return Err("The microphone thread stopped before it opened".into()),
        };
        Ok(MicCapture {
            name: ready.name,
            rate: ready.rate,
            buf: ready.buf,
            alive,
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }

    pub fn device_name(&self) -> String {
        self.name.clone()
    }

    /// False once the stream reports an error. Reopening is the caller's job.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// The session starts with the pre-roll, then collects live samples. Calling this during an
    /// active session discards that session.
    pub fn begin(&self) {
        // Allocated before the lock: the audio callback takes the same lock.
        let session = Vec::with_capacity(session_reserve(self.rate));
        lock(&self.buf).begin(session);
    }

    pub fn end(&self) -> CapturedAudio {
        CapturedAudio { samples: lock(&self.buf).end(), sample_rate: self.rate }
    }

    pub fn cancel(&self) {
        lock(&self.buf).cancel();
    }

    /// RMS of the last 50 ms, 0..1.
    pub fn level(&self) -> f32 {
        lock(&self.buf).level(self.rate as usize * LEVEL_WINDOW_MS / 1000)
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(preferred: Option<String>, alive: Arc<AtomicBool>, ready: Sender<Result<Ready, String>>, stop: Receiver<()>) {
    match start(preferred, alive) {
        Ok((stream, r)) => {
            let _ = ready.send(Ok(r));
            // Returns when MicCapture is dropped and the stop sender is gone.
            let _ = stop.recv();
            drop(stream);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
}

/// Runs on the capture thread: the cpal stream must be created and dropped here.
fn start(preferred: Option<String>, alive: Arc<AtomicBool>) -> Result<(cpal::Stream, Ready), String> {
    // Names from Recording preferences carry an " (input)" suffix; the device name is the bare part.
    let preferred = preferred.and_then(|name| match parse_audio_device(&name) {
        Ok(device) => Some(device.name),
        Err(e) => {
            warn!("Dictate: requested microphone name is unusable ({e}), using the default input");
            None
        }
    });
    // Same existence check as the recording path (resolve_mic_or_default), on every platform.
    let available: Vec<String> = cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    let default = default_input_device().ok().map(|d| d.name);
    let chosen = choose_device(preferred.as_deref(), &available, default.as_deref())
        .ok_or_else(|| "No microphone found".to_string())?;

    let (device, config) = tauri::async_runtime::block_on(get_device_and_config(&AudioDevice::new(
        chosen.clone(),
        DeviceType::Input,
    )))
    .map_err(|e| format!("Microphone '{chosen}' unavailable: {e}"))?;
    let name = device.name().map_err(|e| format!("Microphone name unreadable: {e}"))?;

    let rate = config.sample_rate().0;
    let channels = config.channels();
    let buf: Shared = Arc::new(Mutex::new(SessionBuffer::new(
        PRE_ROLL_MS as usize * rate as usize / 1000,
        (MAX_DICTATION_SECS as usize + 1) * rate as usize,
    )));
    let stream = build_stream(&device, &config, channels, buf.clone(), alive)?;
    stream.play().map_err(|e| format!("Microphone stream did not start: {e}"))?;
    match preferred.as_deref() {
        Some(p) if p != chosen => info!(
            "Dictate: requested microphone '{p}' is not available, opened '{name}' at {rate} Hz (fallback)"
        ),
        _ => info!("Dictate: opened microphone '{name}' at {rate} Hz, {channels} channel(s)"),
    }
    Ok((stream, Ready { name, rate, buf }))
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    channels: u16,
    buf: Shared,
    alive: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let stream_config = config.config();
    let built = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(device, &stream_config, channels, buf, alive, |s| s),
        cpal::SampleFormat::I16 => build::<i16>(device, &stream_config, channels, buf, alive, i16_to_f32),
        cpal::SampleFormat::U16 => build::<u16>(device, &stream_config, channels, buf, alive, u16_to_f32),
        other => return Err(format!("Unsupported microphone sample format: {other:?}")),
    };
    built.map_err(|e| format!("Could not open the microphone: {e}"))
}

fn build<T: cpal::SizedSample + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    buf: Shared,
    alive: Arc<AtomicBool>,
    to_f32: fn(T) -> f32,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut sb = lock(&buf);
            downmix_into(data, channels, to_f32, |x| sb.push_one(x));
        },
        move |e: cpal::StreamError| {
            warn!("Dictate microphone stream error: {e}");
            alive.store(false, Ordering::SeqCst);
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preroll_keeps_last_n() {
        let mut p = PreRollBuffer::new(4);
        p.push(&[1.0, 2.0, 3.0]);
        p.push(&[4.0, 5.0, 6.0]);
        let mut out = Vec::new();
        p.move_into(&mut out);
        assert_eq!(out, vec![3.0, 4.0, 5.0, 6.0]);
        out.clear();
        p.move_into(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn downmix_averages_channels() {
        let mut out = Vec::new();
        downmix_into(&[1.0, 3.0, 2.0, 4.0], 2, |s| s, |x| out.push(x));
        assert_eq!(out, vec![2.0, 3.0]);

        let mut mono = Vec::new();
        downmix_into(&[1.0, 3.0], 1, |s| s, |x| mono.push(x));
        assert_eq!(mono, vec![1.0, 3.0]);
    }

    #[test]
    fn session_includes_preroll() {
        let mut s = SessionBuffer::new(2, 100);
        s.push(&[1.0, 2.0]);
        s.begin(Vec::new());
        s.push(&[3.0]);
        assert_eq!(s.end(), vec![1.0, 2.0, 3.0]);

        s.begin(Vec::new());
        s.push(&[7.0]);
        s.cancel();
        assert!(s.end().is_empty());
    }

    #[test]
    fn session_keeps_preroll_rolling() {
        // The pre-roll is refilled while a session runs, so the next dictation starts with fresh audio.
        let mut s = SessionBuffer::new(2, 100);
        s.begin(Vec::new());
        s.push(&[3.0, 4.0]);
        s.cancel();
        s.begin(Vec::new());
        assert_eq!(s.end(), vec![3.0, 4.0]);
    }

    #[test]
    fn session_cap() {
        let mut s = SessionBuffer::new(0, 3);
        s.begin(Vec::new());
        s.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(s.end().len(), 3);
    }

    #[test]
    fn begin_appends_preroll_to_the_given_vec() {
        let mut s = SessionBuffer::new(2, 5000);
        s.push(&[1.0, 2.0, 3.0]);
        s.begin(Vec::with_capacity(1000));
        let session = s.session.as_ref().unwrap();
        assert_eq!(session, &vec![2.0, 3.0]);
        assert!(session.capacity() >= 1000);
    }

    #[test]
    fn level_is_rms_of_newest_window() {
        let mut s = SessionBuffer::new(10, 100);
        s.push(&[0.5, 0.5]);
        assert_eq!(s.level(2), 0.5);
        assert_eq!(s.level(0), 0.0);
    }

    #[test]
    fn choose_device_prefers_available_then_default() {
        let avail = vec!["Mic A".to_string(), "Mic B".to_string()];
        assert_eq!(choose_device(Some("Mic B"), &avail, Some("Mic A")), Some("Mic B".into()));
        assert_eq!(choose_device(Some("Gone"), &avail, Some("Mic A")), Some("Mic A".into()));
        assert_eq!(choose_device(None, &avail, Some("Mic A")), Some("Mic A".into()));
        assert_eq!(choose_device(Some("Gone"), &avail, None), None);
        assert_eq!(choose_device(None, &[], None), None);
    }

    #[test]
    fn i16_and_u16_convert() {
        assert_eq!(i16_to_f32(i16::MIN), -1.0);
        assert_eq!(i16_to_f32(0), 0.0);
        assert_eq!(u16_to_f32(32768), 0.0);
        assert_eq!(u16_to_f32(0), -1.0);
    }
}
