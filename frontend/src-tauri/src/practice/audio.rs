// Speaking Practice audio: the user's microphone turns, the you.wav turn recorder and speaker playback.
// The recorder, the span reader and PlayQueue are pure and tested; the cpal parts follow dictate/capture.rs.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;
use log::warn;
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

use super::{TTS_RATE, YOU_WAV_RATE};
use crate::audio::audio_processing::resample_audio;
use crate::audio::{default_input_device, get_device_and_config, AudioDevice, DeviceType};
use crate::dictate::capture::{choose_device, downmix_into, i16_to_f32, u16_to_f32};

/// A cpal stream owned by its own thread. Dropping it stops the stream and joins the thread.
struct StreamThread {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for StreamThread {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Runs `open` on a new thread, because cpal streams are created and dropped on one thread. Returns what
/// `open` reports, and the owner that keeps the stream playing.
fn start_stream<T: Send + 'static>(
    name: &str,
    open: impl FnOnce() -> Result<(cpal::Stream, T), String> + Send + 'static,
) -> Result<(T, StreamThread), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || match open() {
            Ok((stream, value)) => {
                let _ = ready_tx.send(Ok(value));
                // Returns when the StreamThread is dropped and the stop sender is gone.
                let _ = stop_rx.recv();
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| format!("Could not start the audio thread: {e}"))?;
    match ready_rx.recv() {
        Ok(Ok(value)) => Ok((value, StreamThread { stop: Some(stop_tx), thread: Some(thread) })),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err("The audio thread stopped before it opened".into()),
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The user's microphone for practice turns. Samples reach the receiver only while recording is on.
pub struct TurnMic {
    recording: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    _stream: StreamThread,
}

impl TurnMic {
    /// Opens `preferred` when it is listed, else the default input. Returns the receiver and the device rate.
    pub fn open(preferred: Option<String>) -> Result<(TurnMic, Receiver<Vec<f32>>, u32), String> {
        let (tx, rx) = mpsc::channel();
        let recording = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let (rate, stream) = start_stream("practice-mic", {
            let (recording, alive) = (recording.clone(), alive.clone());
            move || open_input(preferred, tx, recording, alive)
        })?;
        Ok((TurnMic { recording, alive, _stream: stream }, rx, rate))
    }

    pub fn set_recording(&self, on: bool) {
        self.recording.store(on, Ordering::SeqCst);
    }

    /// False once the stream reports an error. Reopening is the caller's job.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}

fn open_input(
    preferred: Option<String>,
    tx: Sender<Vec<f32>>,
    recording: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32), String> {
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
    let rate = config.sample_rate().0;
    let channels = config.channels();
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_input::<f32>(&device, &stream_config, channels, tx, recording, alive, |s| s),
        cpal::SampleFormat::I16 => build_input::<i16>(&device, &stream_config, channels, tx, recording, alive, i16_to_f32),
        cpal::SampleFormat::U16 => build_input::<u16>(&device, &stream_config, channels, tx, recording, alive, u16_to_f32),
        other => Err(format!("Unsupported microphone sample format: {other:?}")),
    }?;
    stream.play().map_err(|e| format!("Microphone stream did not start: {e}"))?;
    Ok((stream, rate))
}

fn build_input<T: cpal::SizedSample + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    tx: Sender<Vec<f32>>,
    recording: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    to_f32: fn(T) -> f32,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if !recording.load(Ordering::SeqCst) {
                    return;
                }
                let mut mono = Vec::with_capacity(data.len() / channels.max(1) as usize);
                downmix_into(data, channels, to_f32, |x| mono.push(x));
                // A dropped receiver means the session has ended; the samples are discarded.
                let _ = tx.send(mono);
            },
            move |e: cpal::StreamError| {
                warn!("Practice microphone stream error: {e}");
                alive.store(false, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| format!("Could not open the microphone: {e}"))
}

fn write_err(e: hound::Error) -> String {
    format!("Could not write the turn audio: {e}")
}

/// Writes one turn's 16 kHz mono audio into you.wav. The header is rewritten after every append,
/// so a crash keeps everything appended so far.
pub struct TurnRecorder {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    samples_written: i64,
}

impl TurnRecorder {
    pub fn create(path: &Path) -> Result<Self, String> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: YOU_WAV_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(path, spec)
            .map_err(|e| format!("Could not create {}: {e}", path.display()))?;
        Ok(Self { writer, samples_written: 0 })
    }

    /// Returns the (offset, length) of the appended samples, in samples from the start of the file.
    pub fn append(&mut self, a16: &[f32]) -> Result<(i64, i64), String> {
        let offset = self.samples_written;
        for &s in a16 {
            self.writer.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).map_err(write_err)?;
        }
        self.writer.flush().map_err(write_err)?;
        let len = a16.len() as i64;
        self.samples_written += len;
        Ok((offset, len))
    }

    pub fn finish(self) -> Result<(), String> {
        self.writer.finalize().map_err(write_err)
    }
}

/// Reads `len` samples starting at `offset` (both in samples) from you.wav, as f32 in -1..1.
pub fn read_span(path: &Path, offset: i64, len: i64) -> Result<Vec<f32>, String> {
    let fail = |e: &dyn std::fmt::Display| format!("Could not read {}: {e}", path.display());
    if offset < 0 || len < 0 {
        return Err(fail(&"negative span"));
    }
    let mut reader = hound::WavReader::open(path).map_err(|e| fail(&e))?;
    reader.seek(offset as u32).map_err(|e| fail(&e))?;
    let samples = reader
        .samples::<i16>()
        .take(len as usize)
        .map(|s| s.map(i16_to_f32))
        .collect::<Result<Vec<f32>, _>>()
        .map_err(|e| fail(&e))?;
    if samples.len() as i64 != len {
        return Err(fail(&"the turn is shorter than its recorded span"));
    }
    Ok(samples)
}

/// Speech to play, as (id, samples at the device rate). One mono sample is written to every channel.
#[derive(Default)]
pub struct PlayQueue {
    items: VecDeque<(u64, Vec<f32>)>,
    position: usize,
}

impl PlayQueue {
    pub fn push(&mut self, id: u64, samples: Vec<f32>) {
        self.items.push_back((id, samples));
    }

    /// Drops everything queued. The next fill writes silence.
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.items.clear();
        self.position = 0;
    }

    /// Writes the next samples into `out`, and returns the ids whose last sample was written in this fill.
    pub fn fill(&mut self, out: &mut [f32], channels: usize) -> Vec<u64> {
        let mut finished = Vec::new();
        for frame in out.chunks_mut(channels.max(1)) {
            frame.fill(self.next_sample(&mut finished));
        }
        finished
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn next_sample(&mut self, finished: &mut Vec<u64>) -> f32 {
        while let Some((id, samples)) = self.items.front() {
            if let Some(&s) = samples.get(self.position) {
                self.position += 1;
                if self.position == samples.len() {
                    finished.push(*id);
                    self.items.pop_front();
                    self.position = 0;
                }
                return s;
            }
            // An empty buffer has no samples to write; it still finishes.
            finished.push(*id);
            self.items.pop_front();
            self.position = 0;
        }
        0.0
    }
}

/// The default speaker. `stop_all` takes effect on the next callback.
pub struct Player {
    queue: Arc<Mutex<PlayQueue>>,
    alive: Arc<AtomicBool>,
    rate: u32,
    _stream: StreamThread,
}

impl Player {
    /// Returns the player and the receiver of finished ids.
    pub fn open() -> Result<(Player, Receiver<u64>), String> {
        let queue = Arc::new(Mutex::new(PlayQueue::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let (finished_tx, finished_rx) = mpsc::channel();
        let (rate, stream) = start_stream("practice-play", {
            let (queue, alive) = (queue.clone(), alive.clone());
            move || open_output(queue, finished_tx, alive)
        })?;
        Ok((Player { queue, alive, rate, _stream: stream }, finished_rx))
    }

    /// False once the output stream reports an error. Reopening is the caller's job.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Queues speech at 44.1 kHz; it is resampled to the device rate here, off the audio thread.
    pub fn play(&self, id: u64, samples_44k: Vec<f32>) {
        let samples = resample_audio(&samples_44k, TTS_RATE, self.rate);
        lock(&self.queue).push(id, samples);
    }

    pub fn stop_all(&self) {
        // The queued buffers are freed after the lock is released.
        let dropped = std::mem::take(&mut *lock(&self.queue));
        drop(dropped);
    }
}

fn open_output(
    queue: Arc<Mutex<PlayQueue>>,
    finished: Sender<u64>,
    alive: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "No speaker found".to_string())?;
    let config = device
        .default_output_config()
        .map_err(|e| format!("Speaker unavailable: {e}"))?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_output::<f32>(&device, &stream_config, channels, queue, finished, alive),
        cpal::SampleFormat::I16 => build_output::<i16>(&device, &stream_config, channels, queue, finished, alive),
        cpal::SampleFormat::U16 => build_output::<u16>(&device, &stream_config, channels, queue, finished, alive),
        other => Err(format!("Unsupported speaker sample format: {other:?}")),
    }?;
    stream.play().map_err(|e| format!("Speaker stream did not start: {e}"))?;
    Ok((stream, rate))
}

fn build_output<T: cpal::SizedSample + FromSample<f32> + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    queue: Arc<Mutex<PlayQueue>>,
    finished: Sender<u64>,
    alive: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                scratch.resize(data.len(), 0.0);
                // Never wait on the audio thread: a busy queue means one buffer of silence.
                let done = match queue.try_lock() {
                    Ok(mut q) => q.fill(&mut scratch, channels),
                    Err(_) => {
                        scratch.fill(0.0);
                        Vec::new()
                    }
                };
                for (o, s) in data.iter_mut().zip(&scratch) {
                    *o = T::from_sample(*s);
                }
                for id in done {
                    let _ = finished.send(id);
                }
            },
            move |e: cpal::StreamError| {
                warn!("Practice speaker stream error: {e}");
                alive.store(false, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| format!("Could not open the speaker: {e}"))
}

/// Input frames per resampler call. The resampler takes exactly this many, so the turn's chunks are buffered to it.
const TURN_RESAMPLE_CHUNK: usize = 512;

/// Converts one turn's microphone chunks to 16 kHz with one resampler for the whole turn, so the filter keeps
/// its state across the ~10 ms callbacks. A device already at 16 kHz passes through.
pub struct TurnResampler {
    resampler: Option<SincFixedIn<f32>>,
    ratio: f64,
    /// Input held back until a whole chunk has arrived.
    input: Vec<f32>,
    /// Input frames pushed so far.
    fed: usize,
    /// Output frames returned so far.
    emitted: usize,
}

impl TurnResampler {
    pub fn new(from_rate: u32) -> Result<Self, String> {
        let ratio = YOU_WAV_RATE as f64 / from_rate as f64;
        // The filter of resample_audio for a downsampling ratio of 0.5 or less (44.1 and 48 kHz devices).
        let resampler = if from_rate == YOU_WAV_RATE {
            None
        } else {
            let params = SincInterpolationParameters {
                sinc_len: 512,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Cubic,
                oversampling_factor: 512,
                window: WindowFunction::BlackmanHarris2,
            };
            let resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, TURN_RESAMPLE_CHUNK, 1)
                .map_err(|e| format!("resampler: {e}"))?;
            Some(resampler)
        };
        Ok(Self { resampler, ratio, input: Vec::new(), fed: 0, emitted: 0 })
    }

    /// Returns the 16 kHz samples that are ready. Input is held back until a whole chunk has arrived.
    pub fn push(&mut self, chunk: &[f32]) -> Result<Vec<f32>, String> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(chunk.to_vec());
        };
        self.fed += chunk.len();
        self.input.extend_from_slice(chunk);
        let mut out = Vec::new();
        while self.input.len() >= TURN_RESAMPLE_CHUNK {
            let waves_in = vec![self.input.drain(..TURN_RESAMPLE_CHUNK).collect::<Vec<f32>>()];
            let waves_out = resampler.process(&waves_in, None).map_err(|e| format!("resampler: {e}"))?;
            out.extend(waves_out.into_iter().flatten());
        }
        self.emitted += out.len();
        Ok(out)
    }

    /// Ends the turn. The held-back tail and the filter's delay are flushed, so the output covers the whole turn.
    pub fn finish(&mut self) -> Result<Vec<f32>, String> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(Vec::new());
        };
        let tail = vec![std::mem::take(&mut self.input)];
        let mut out: Vec<f32> = resampler
            .process_partial(Some(tail.as_slice()), None)
            .map_err(|e| format!("resampler: {e}"))?
            .into_iter()
            .flatten()
            .collect();
        // The output lags the input by output_delay frames, so the flush runs until that lag has drained.
        let target = (self.fed as f64 * self.ratio).round() as usize + resampler.output_delay();
        while self.emitted + out.len() < target {
            let more = resampler
                .process_partial(None::<&[Vec<f32>]>, None)
                .map_err(|e| format!("resampler: {e}"))?;
            out.extend(more.into_iter().flatten());
        }
        out.truncate(target.saturating_sub(self.emitted));
        self.emitted += out.len();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_resampler_keeps_every_sample_across_chunks() {
        // 1 s of a 440 Hz tone at 48 kHz, fed in the ~10 ms callbacks the microphone sends.
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        let mut resampler = TurnResampler::new(48_000).unwrap();
        let mut out = Vec::new();
        for chunk in tone.chunks(480) {
            out.extend(resampler.push(chunk).unwrap());
        }
        out.extend(resampler.finish().unwrap());

        assert!((out.len() as f64 - 16_000.0).abs() <= 160.0, "got {} samples, want about 16000", out.len());
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let (got, want) = (rms(&out), rms(&tone));
        assert!((got - want).abs() / want < 0.02, "rms {got} vs input {want}: samples lost");
    }

    #[test]
    fn turn_resampler_passes_16k_through() {
        let mut resampler = TurnResampler::new(YOU_WAV_RATE).unwrap();
        assert_eq!(resampler.push(&[0.1, 0.2, 0.3]).unwrap(), vec![0.1, 0.2, 0.3]);
        assert!(resampler.finish().unwrap().is_empty());
    }

    fn tmp_wav() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("you.wav");
        (dir, path)
    }

    #[test]
    fn recorder_offsets_accumulate_and_header_is_valid_after_each_append() {
        let (_dir, path) = tmp_wav();
        let mut rec = TurnRecorder::create(&path).unwrap();

        assert_eq!(rec.append(&vec![0.0; 16_000]).unwrap(), (0, 16_000));
        let r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.spec().channels, 1);
        assert_eq!(r.spec().sample_rate, YOU_WAV_RATE);
        assert_eq!(r.spec().bits_per_sample, 16);
        assert_eq!(r.duration(), 16_000);

        assert_eq!(rec.append(&vec![0.0; 8_000]).unwrap(), (16_000, 8_000));
        let r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.duration(), 24_000);

        rec.finish().unwrap();
        assert_eq!(hound::WavReader::open(&path).unwrap().duration(), 24_000);
    }

    #[test]
    fn read_span_returns_the_turn() {
        let (_dir, path) = tmp_wav();
        let mut rec = TurnRecorder::create(&path).unwrap();
        rec.append(&vec![0.1; 16_000]).unwrap();
        rec.append(&vec![-0.5; 8_000]).unwrap();
        rec.finish().unwrap();

        let turn = read_span(&path, 16_000, 8_000).unwrap();
        assert_eq!(turn.len(), 8_000);
        assert!(turn.iter().all(|s| (s + 0.5).abs() < 1e-3));

        let first = read_span(&path, 0, 4).unwrap();
        assert!(first.iter().all(|s| (s - 0.1).abs() < 1e-3));

        assert!(read_span(&path, 24_000, 1).is_err());
    }

    #[test]
    fn play_queue_reports_finished_ids_and_writes_silence_when_empty() {
        let mut q = PlayQueue::default();
        q.push(1, vec![0.5, 0.25]);
        q.push(2, vec![-0.5]);

        // Stereo: each mono sample fills one whole frame.
        let mut out = vec![9.0; 4];
        assert_eq!(q.fill(&mut out, 2), vec![1]);
        assert_eq!(out, vec![0.5, 0.5, 0.25, 0.25]);

        let mut out = vec![9.0; 4];
        assert_eq!(q.fill(&mut out, 2), vec![2]);
        assert_eq!(out, vec![-0.5, -0.5, 0.0, 0.0]);

        let mut out = vec![9.0; 4];
        assert!(q.fill(&mut out, 2).is_empty());
        assert_eq!(out, vec![0.0; 4]);
        assert!(q.is_empty());

        // A buffer that spans two fills reports its id on the second one.
        q.push(3, vec![0.1, 0.2, 0.3]);
        let mut out = vec![9.0; 2];
        assert!(q.fill(&mut out, 1).is_empty());
        assert_eq!(out, vec![0.1, 0.2]);
        let mut out = vec![9.0; 2];
        assert_eq!(q.fill(&mut out, 1), vec![3]);
        assert_eq!(out, vec![0.3, 0.0]);
    }

    #[test]
    fn play_queue_clear_stops_immediately() {
        let mut q = PlayQueue::default();
        q.push(1, vec![0.5; 100]);
        let mut out = vec![9.0; 10];
        q.fill(&mut out, 1);
        assert_eq!(out, vec![0.5; 10]);

        q.clear();
        assert!(q.is_empty());
        let mut out = vec![9.0; 10];
        assert!(q.fill(&mut out, 1).is_empty());
        assert_eq!(out, vec![0.0; 10]);
    }
}
