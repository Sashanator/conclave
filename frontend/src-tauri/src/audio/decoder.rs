// Audio file decoder for retranscription feature
// Uses Symphonia to decode MP4/AAC audio files, with ffmpeg fallback for
// formats Symphonia can't handle (MKV, WebM, WMA)

use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use rayon::prelude::*;
use rubato::{FftFixedIn, Resampler};
use std::borrow::Cow;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;

use super::audio_processing::{audio_to_mono, resample, resample_audio};
use super::ffmpeg::find_ffmpeg_path;

/// Extensions requiring ffmpeg pre-conversion (Symphonia lacks these demuxers/codecs)
const FFMPEG_ONLY_EXTENSIONS: &[&str] = &["mkv", "webm", "wma"];

/// Progress callback for long-running operations
/// Returns current progress (0-100) and a message
pub type ProgressCallback = Box<dyn Fn(u32, &str) + Send>;

/// Decoded audio data from a file
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Raw audio samples (interleaved if stereo)
    pub samples: Vec<f32>,
    /// Sample rate of the decoded audio
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Duration in seconds
    pub duration_seconds: f64,
}

impl DecodedAudio {
    /// Convert decoded audio to Whisper-compatible 16kHz mono f32 format.
    ///
    /// Performs mono conversion, normalization, and resampling. Large files
    /// (>5 min at 48kHz) use chunked sinc resampling to keep memory bounded
    /// while preserving audio quality for downstream VAD and transcription.
    pub fn to_whisper_format(&self) -> Vec<f32> {
        self.to_whisper_format_with_progress(None)
    }

    /// Convert decoded audio to Whisper format with optional progress callback
    pub fn to_whisper_format_with_progress(&self, progress_callback: Option<ProgressCallback>) -> Vec<f32> {
        // Step 1: Convert to mono if needed
        let mono_samples = if self.channels > 1 {
            info!(
                "Converting {} channels to mono ({} samples)",
                self.channels,
                self.samples.len()
            );
            audio_to_mono(&self.samples, self.channels)
        } else {
            self.samples.clone()
        };

        // Step 1.5: Normalize samples to valid range (-1.0 to 1.0)
        // Some audio files may have samples slightly outside this range
        let mono_samples = normalize_audio_samples(mono_samples);

        // Step 2: Resample to 16kHz if needed
        const WHISPER_SAMPLE_RATE: u32 = 16000;
        if self.sample_rate != WHISPER_SAMPLE_RATE {
            // Large files are processed in chunks through the sinc resampler
            // to keep memory bounded while preserving audio quality.
            // Linear interpolation (fast_resample) was removed because it lacks
            // an anti-aliasing filter, causing aliasing artifacts that make VAD
            // miss ~99% of speech in long recordings.
            const LARGE_FILE_THRESHOLD: usize = 14_400_000;

            let mut resampled = if mono_samples.len() > LARGE_FILE_THRESHOLD {
                info!(
                    "Chunked sinc resampling {} samples from {}Hz to {}Hz (large file mode)",
                    mono_samples.len(),
                    self.sample_rate,
                    WHISPER_SAMPLE_RATE
                );
                chunked_resample_with_progress(&mono_samples, self.sample_rate, WHISPER_SAMPLE_RATE, progress_callback)
            } else {
                info!(
                    "Resampling {} samples from {}Hz to {}Hz",
                    mono_samples.len(),
                    self.sample_rate,
                    WHISPER_SAMPLE_RATE
                );
                resample_audio(&mono_samples, self.sample_rate, WHISPER_SAMPLE_RATE)
            };

            // Clamp after resampling: the sinc resampler can overshoot
            // slightly beyond [-1.0, 1.0] (Gibbs phenomenon), which causes
            // VAD to reject samples with "Float sample must be in the range -1.0 to 1.0"
            for s in &mut resampled {
                *s = s.clamp(-1.0, 1.0);
            }
            resampled
        } else {
            mono_samples
        }
    }
}

/// Resample large audio files in fixed-size chunks through the sinc resampler.
///
/// Processes `input` in 60-second chunks using the high-quality sinc resampler
/// from [`resample_audio`], concatenating the results. This avoids the memory
/// spike of resampling the entire file at once while preserving anti-aliasing
/// quality that is critical for downstream VAD accuracy.
///
/// Chunked resampling with optional progress callback.
///
/// Resamples `input` in parallel 60-second chunks via [`rayon`], then merges
/// the results sequentially with a 100ms cross-fade to eliminate discontinuities
/// at chunk boundaries. Each chunk's [`resample`] call is independent and
/// CPU-bound, making this ideal for data parallelism.
///
/// Falls back to [`resample_audio`] (single-pass sinc) if any chunk fails.
fn chunked_resample_with_progress(
    input: &[f32],
    from_rate: u32,
    to_rate: u32,
    progress_callback: Option<ProgressCallback>,
) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }

    // 60 seconds of audio at the source sample rate per chunk
    let chunk_samples = from_rate as usize * 60;
    // 100ms overlap in the input domain to cross-fade between chunks
    let overlap_input = from_rate as usize / 10;
    let ratio = to_rate as f64 / from_rate as f64;
    let overlap_output = (overlap_input as f64 * ratio) as usize;
    let estimated_output = (input.len() as f64 * ratio) as usize + 1024;

    // Build overlapping chunk boundaries
    let mut chunk_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while start < input.len() {
        let end = (start + chunk_samples + overlap_input).min(input.len());
        chunk_ranges.push((start, end));
        start += chunk_samples;
    }

    let total_chunks = chunk_ranges.len();
    info!(
        "Parallel chunked sinc resampling: {} chunks of ~60s each with 100ms cross-fade ({} total samples)",
        total_chunks,
        input.len()
    );

    // Resample all chunks in parallel — each is independent and CPU-bound
    let resampled_chunks: Vec<Result<Vec<f32>>> = chunk_ranges
        .par_iter()
        .map(|&(chunk_start, chunk_end)| {
            let chunk = &input[chunk_start..chunk_end];
            resample(chunk, from_rate, to_rate)
        })
        .collect();

    // Merge sequentially with cross-fade (order-dependent, must be serial)
    let mut output = Vec::with_capacity(estimated_output);
    for (chunk_idx, result) in resampled_chunks.into_iter().enumerate() {
        match result {
            Ok(resampled) => {
                if chunk_idx == 0 {
                    output.extend_from_slice(&resampled);
                } else {
                    // Cross-fade the overlap region with the tail of the previous output
                    let fade_len = overlap_output.min(resampled.len()).min(output.len());
                    if fade_len > 0 {
                        let out_start = output.len() - fade_len;
                        for i in 0..fade_len {
                            let t = i as f32 / fade_len as f32;
                            output[out_start + i] =
                                output[out_start + i] * (1.0 - t) + resampled[i] * t;
                        }
                        if fade_len < resampled.len() {
                            output.extend_from_slice(&resampled[fade_len..]);
                        }
                    } else {
                        output.extend_from_slice(&resampled);
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Resampling failed on chunk {}/{}: {}, falling back to single-pass sinc resampler",
                    chunk_idx + 1,
                    total_chunks,
                    e
                );
                return resample_audio(input, from_rate, to_rate);
            }
        }

        if let Some(callback) = &progress_callback {
            let progress_pct = ((chunk_idx + 1) as f64 / total_chunks as f64) * 100.0;
            if (chunk_idx + 1) % 10 == 0 || chunk_idx + 1 == total_chunks {
                info!(
                    "Resampling progress: {}/{} chunks ({:.0}%)",
                    chunk_idx + 1,
                    total_chunks,
                    progress_pct
                );
            }
            callback(
                progress_pct as u32,
                &format!("Resampling audio: {:.0}%", progress_pct),
            );
        }
    }

    info!(
        "Parallel chunked sinc resampling complete: {} -> {} samples",
        input.len(),
        output.len()
    );
    output
}

/// Normalize audio samples to the valid range (-1.0 to 1.0)
/// This handles audio files that may have samples slightly outside the expected range
fn normalize_audio_samples(mut samples: Vec<f32>) -> Vec<f32> {
    // First, find the maximum absolute value
    let max_abs = samples
        .iter()
        .filter(|s| s.is_finite())
        .map(|s| s.abs())
        .fold(0.0f32, |a, b| a.max(b));

    if max_abs > 1.0 {
        // Audio exceeds valid range - normalize by scaling
        info!(
            "Audio samples exceed valid range (max: {:.3}), normalizing...",
            max_abs
        );
        let scale = 1.0 / max_abs;
        for sample in &mut samples {
            *sample *= scale;
        }
    }

    // Also clamp any remaining edge cases (NaN, infinity, etc.)
    for sample in &mut samples {
        if !sample.is_finite() {
            *sample = 0.0;
        } else {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    samples
}

/// Check if a file extension requires ffmpeg pre-conversion
fn needs_ffmpeg_conversion(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| FFMPEG_ONLY_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Convert an audio file to WAV using ffmpeg for formats Symphonia can't decode.
///
/// Returns a `TempPath` that auto-deletes the temporary WAV file when dropped.
/// The caller must keep the `TempPath` alive until decoding of the WAV is complete.
fn convert_to_wav_with_ffmpeg(
    input_path: &Path,
    progress_callback: Option<&ProgressCallback>,
) -> Result<tempfile::TempPath> {
    let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| anyhow!("FFmpeg not found next to Conclave.exe — reinstall Conclave."))?;

    // Create temp file in the same directory as the input to avoid cross-device issues
    let parent_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let temp_file = tempfile::Builder::new()
        .prefix(".meetily_decode_")
        .suffix(".wav")
        .tempfile_in(parent_dir)
        .map_err(|e| anyhow!("Failed to create temporary WAV file: {}", e))?;

    let temp_path = temp_file.into_temp_path();

    info!(
        "Converting .{} to temporary WAV via ffmpeg: {} -> {}",
        input_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown"),
        input_path.display(),
        temp_path.display()
    );

    if let Some(cb) = progress_callback {
        cb(0, "Converting audio format with FFmpeg...");
    }

    let input_str = input_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid input path (non-UTF8)"))?;
    let output_str = temp_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid temp path (non-UTF8)"))?;

    let mut command = Command::new(&ffmpeg_path);
    command
        .args([
            "-i", input_str,
            "-vn",                  // Strip video tracks
            "-acodec", "pcm_s16le", // Output PCM WAV (Symphonia handles natively)
            "-y",                   // Overwrite without prompt
            output_str,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    debug!("FFmpeg conversion command: {:?}", command);

    #[allow(clippy::zombie_processes)]
    let child = command
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn ffmpeg process: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| anyhow!("Failed to wait for ffmpeg process: {}", e))?;

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    debug!("FFmpeg stderr: {}", stderr_text);

    if !output.status.success() {
        error!(
            "FFmpeg conversion failed (exit code: {}): {}",
            output.status, stderr_text
        );
        return Err(anyhow!(
            "FFmpeg conversion failed with exit code: {}. \
             The file may be corrupted or in an unsupported format.",
            output.status
        ));
    }

    // Verify output file exists and has content
    let output_meta = std::fs::metadata(&temp_path)
        .map_err(|e| anyhow!("FFmpeg output file not found: {}", e))?;

    if output_meta.len() == 0 {
        return Err(anyhow!(
            "FFmpeg produced an empty output file. The input may contain no audio."
        ));
    }

    if let Some(cb) = progress_callback {
        cb(100, "FFmpeg conversion complete");
    }

    info!(
        "FFmpeg conversion complete: {} bytes output",
        output_meta.len()
    );

    Ok(temp_path)
}

/// Decode an audio file (MP4, M4A, WAV, etc.) to raw samples.
///
/// Samples are placed on the container timeline by packet timestamp, as in
/// [`decode_to_mono_16k_file`]: the encoder's repeated segment joins are dropped and
/// forward gaps are zero-filled. Files without timestamp jumps decode unchanged.
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    decode_audio_file_with_progress(path, None)
}

/// Decode an audio file with optional progress callback
pub fn decode_audio_file_with_progress(
    path: &Path,
    progress_callback: Option<ProgressCallback>,
) -> Result<DecodedAudio> {
    info!("Decoding audio file");

    // FFmpeg pre-conversion for unsupported formats (MKV, WebM, WMA).
    // If the file is in a format Symphonia can't decode, use ffmpeg to convert
    // it to a temporary WAV file first, then decode the WAV with Symphonia.
    // The _temp_wav_guard keeps the temp file alive until decoding completes,
    // then auto-deletes it when dropped (even on error/panic).
    let (_temp_wav_guard, decode_path): (Option<tempfile::TempPath>, Cow<'_, Path>) =
        if needs_ffmpeg_conversion(path) {
            info!(
                "Format requires ffmpeg pre-conversion: .{}",
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown")
            );
            let temp_path = convert_to_wav_with_ffmpeg(path, progress_callback.as_ref())?;
            let wav_path = temp_path.to_path_buf();
            (Some(temp_path), Cow::Owned(wav_path))
        } else {
            (None, Cow::Borrowed(path))
        };

    // Open the file (use decode_path which may be the temp WAV)
    let file = std::fs::File::open(decode_path.as_ref())
        .map_err(|e| anyhow!("Failed to open audio file '{}': {}", decode_path.display(), e))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Set up format hint based on file extension
    let mut hint = Hint::new();
    if let Some(ext) = decode_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the file format
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("Failed to probe audio format: {}", e))?;

    let mut format = probed.format;

    // Find the first audio track
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("No audio track found in file"))?;

    let track_id = track.id;

    // Get audio parameters
    let mut sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Unknown sample rate"))?;

    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    debug!(
        "Audio track: {}Hz, {} channels (from metadata)",
        sample_rate, channels
    );

    // Create the decoder
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("Failed to create decoder: {}", e))?;

    // Decode all packets
    let mut all_samples: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    // Calculate expected samples for progress tracking
    let expected_duration = track.codec_params.n_frames
        .map(|frames| frames as f64 / sample_rate as f64);
    let expected_samples = expected_duration
        .map(|dur| (dur * sample_rate as f64 * channels as f64) as usize);

    let time_base = track.codec_params.time_base;
    let n_frames = track.codec_params.n_frames;
    // Container timeline position, in frames at the decoded rate.
    let mut written = 0u64;

    let mut last_progress = 0u32;

    loop {
        // Get the next packet
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // End of file
                break;
            }
            Err(e) => {
                warn!("Error reading packet: {}", e);
                break;
            }
        };

        // Skip packets from other tracks
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet
        match decoder.decode(&packet) {
            Ok(decoded) => {
                // Initialize sample buffer if needed
                if sample_buf.is_none() {
                    let spec = *decoded.spec();
                    let duration = decoded.capacity() as u64;
                    // Detect actual channel count from decoded audio (metadata may be wrong/missing)
                    let actual_channels = spec.channels.count() as u16;
                    if actual_channels != channels {
                        info!(
                            "Channel count corrected: metadata={} actual={} (using actual)",
                            channels, actual_channels
                        );
                        channels = actual_channels;
                    }
                    // Detect actual sample rate from decoded audio. The container can
                    // declare a different rate than the decoder produces: HE-AAC (SBR)
                    // files declare e.g. 48000 Hz, but Symphonia has no SBR support and
                    // decodes the AAC-LC core at half that rate. Trusting the container
                    // rate makes the audio play at 2x speed and halves the duration.
                    let actual_rate = spec.rate;
                    if actual_rate != sample_rate {
                        info!(
                            "Sample rate corrected: metadata={} actual={} (using actual)",
                            sample_rate, actual_rate
                        );
                        sample_rate = actual_rate;
                    }
                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                // Copy samples to buffer, placed on the container timeline
                if let Some(ref mut buf) = sample_buf {
                    buf.copy_interleaved_ref(decoded);
                    let ch = channels as usize;
                    let frames = (buf.samples().len() / ch) as u64;
                    let ts = ts_to_frames(packet.ts(), time_base, sample_rate);
                    let declared = n_frames.map(|n| ts_to_frames(n, time_base, sample_rate));
                    let (skip, zero_fill, write) =
                        align_packet(ts, frames, written, Bounds::new(declared, sample_rate))?;
                    all_samples.resize(all_samples.len() + zero_fill as usize * ch, 0.0);
                    all_samples.extend_from_slice(
                        &buf.samples()[skip as usize * ch..(skip + write) as usize * ch],
                    );
                    written = written.saturating_add(zero_fill).saturating_add(write);
                }

                // Emit progress updates (every 10%)
                if let (Some(callback), Some(expected)) = (&progress_callback, expected_samples) {
                    let current_progress = ((all_samples.len() as f64 / expected as f64) * 100.0) as u32;
                    if current_progress >= last_progress + 10 && current_progress <= 100 {
                        last_progress = current_progress;
                        callback(current_progress, &format!("Decoding audio: {}%", current_progress));
                    }
                }
            }
            Err(e) => {
                warn!("Error decoding packet: {}", e);
                continue;
            }
        }
    }

    // Ensure we report 100% completion
    if let Some(callback) = &progress_callback {
        callback(100, "Decoding complete");
    }

    if all_samples.is_empty() {
        return Err(anyhow!("No audio samples decoded from file"));
    }

    let total_frames = all_samples.len() / channels as usize;
    let duration_seconds = total_frames as f64 / sample_rate as f64;

    info!(
        "Decoded {} samples ({:.2}s) at {}Hz, {} channels",
        all_samples.len(),
        duration_seconds,
        sample_rate,
        channels
    );

    Ok(DecodedAudio {
        samples: all_samples,
        sample_rate,
        channels,
        duration_seconds,
    })
}

/// Output rate of the speaker pipeline (Whisper and the diarizer both take 16 kHz mono).
const SPEAKER_RATE_HZ: u32 = 16_000;
/// Silence is pushed in blocks of this size, so a long gap never needs one long buffer.
static SILENCE: [f32; 4096] = [0.0; 4096];

#[cfg(test)]
thread_local! {
    /// Largest number of input samples held at once by the streaming decode (tests only).
    static INPUT_HIGH_WATER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_input_buffered(len: usize) {
    INPUT_HIGH_WATER.with(|hw| hw.set(hw.get().max(len)));
}

#[cfg(not(test))]
fn note_input_buffered(_len: usize) {}

/// `round(value × mul ÷ div)` in integers, saturating at `u64::MAX`. `div` must be non-zero.
fn scale_round(value: u64, mul: u64, div: u64) -> u64 {
    let num = (value as u128)
        .saturating_mul(mul as u128)
        .saturating_mul(2)
        .saturating_add(div as u128);
    u64::try_from(num / (2 * div as u128)).unwrap_or(u64::MAX)
}

/// Longest forward gap accepted when the container does not declare a length, in seconds.
const MAX_GAP_SECS: u64 = 60;
/// Timestamp differences up to this many milliseconds are rounding, not gaps or overlaps.
const JITTER_MS: u64 = 5;

/// What the container allows for one track, in frames at its decoded rate.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    /// Declared end plus 1 s, or None when the container does not declare a length.
    limit: Option<u64>,
    /// Largest forward gap accepted when `limit` is None.
    max_gap: u64,
    /// Timestamp differences up to this many frames are treated as continuous.
    tolerance: u64,
    /// One second of frames: the most silence that may be inserted past the declared length.
    rate_1s: u64,
}

impl Bounds {
    /// `declared` is the track length in frames at `rate`, when the container states one.
    fn new(declared: Option<u64>, rate: u32) -> Self {
        Self {
            limit: declared.map(|d| d.saturating_add(rate as u64)),
            max_gap: MAX_GAP_SECS * rate as u64,
            tolerance: JITTER_MS * rate as u64 / 1000,
            rate_1s: rate as u64,
        }
    }
}

/// Places a decoded packet on the container timeline, in frames.
///
/// `ts` is where the packet starts, `frames` is its decoded length and `written` is the timeline
/// position reached so far. Returns `(skip, zero_fill, write)`: drop the first `skip` frames
/// (they repeat audio already written), insert `zero_fill` silent frames (a forward gap), then
/// keep `write` frames. Real decoded frames are always kept. Only inserted silence is bounded:
/// with no declared length a gap over `max_gap` is an error; past the declared length a gap over
/// one second is an error, so that a skipped packet (a short gap) does not fail the decode.
fn align_packet(ts: u64, frames: u64, written: u64, bounds: Bounds) -> Result<(u64, u64, u64)> {
    if ts.abs_diff(written) <= bounds.tolerance {
        return Ok((0, 0, frames));
    }
    if ts < written {
        let skip = (written - ts).min(frames);
        return Ok((skip, 0, frames - skip));
    }
    let gap = ts - written;
    let too_long = match bounds.limit {
        None => gap > bounds.max_gap,
        Some(limit) => ts > limit && gap > bounds.rate_1s,
    };
    if too_long {
        return Err(anyhow!(
            "Corrupt audio timestamps: a gap of {} frames after frame {}",
            gap,
            written
        ));
    }
    Ok((0, gap, frames))
}

/// Converts a packet timestamp from the track's time base to frames at `rate`.
/// Without a usable time base the timestamp is already a frame count.
fn ts_to_frames(ts: u64, time_base: Option<TimeBase>, rate: u32) -> u64 {
    match time_base {
        Some(tb) if tb.denom > 0 => scale_round(ts, tb.numer as u64 * rate as u64, tb.denom as u64),
        _ => ts,
    }
}

/// True when the decoded timeline falls short of the declared duration by more than 2 %.
fn timeline_is_short(written: u64, declared: u64) -> bool {
    (written as u128) * 100 < (declared as u128) * 98
}

/// Mono input-rate frames in, 16 kHz f32 little-endian out. The resampler works in 1 s chunks.
struct Resample16k {
    resampler: FftFixedIn<f32>,
    rate: u32,
    /// Input frames waiting for a full chunk (never more than one chunk).
    pending: Vec<f32>,
    /// Output frames still to drop: the resampler's filter delay, at the start only.
    to_drop: usize,
    /// The filter delay, which `finish` counts against the output it produces.
    delay: usize,
    /// Every output frame the resampler has produced, including the dropped ones.
    produced: u64,
    out: BufWriter<File>,
}

impl Resample16k {
    fn new(rate: u32, output: &Path) -> Result<Self> {
        let file = File::create(output)
            .map_err(|e| anyhow!("Failed to create temp audio file '{}': {}", output.display(), e))?;
        let resampler = FftFixedIn::<f32>::new(rate as usize, SPEAKER_RATE_HZ as usize, rate as usize, 2, 1)?;
        let delay = resampler.output_delay();
        Ok(Self {
            resampler,
            rate,
            pending: Vec::with_capacity(rate as usize),
            to_drop: delay,
            delay,
            produced: 0,
            out: BufWriter::new(file),
        })
    }

    /// Appends `frames` silent input frames, in blocks.
    fn push_silence(&mut self, mut frames: u64) -> Result<()> {
        while frames > 0 {
            let n = (frames as usize).min(SILENCE.len());
            self.push(&SILENCE[..n])?;
            frames -= n as u64;
        }
        Ok(())
    }

    fn push(&mut self, mut data: &[f32]) -> Result<()> {
        let chunk = self.rate as usize;
        while !data.is_empty() {
            let take = (chunk - self.pending.len()).min(data.len());
            self.pending.extend_from_slice(&data[..take]);
            data = &data[take..];
            note_input_buffered(self.pending.len());
            if self.pending.len() == chunk {
                let out = self.resampler.process(&[&self.pending], None)?;
                self.pending.clear();
                self.write_output(&out[0])?;
            }
        }
        Ok(())
    }

    fn write_output(&mut self, data: &[f32]) -> Result<()> {
        self.produced += data.len() as u64;
        let skip = self.to_drop.min(data.len());
        self.to_drop -= skip;
        let bytes: Vec<u8> = data[skip..].iter().flat_map(|s| s.to_le_bytes()).collect();
        self.out.write_all(&bytes)?;
        Ok(())
    }

    /// Flushes the resampler, then sets the file to exactly `round(timeline × 16000 / rate)`
    /// frames. Returns the seconds written.
    fn finish(mut self, timeline: u64) -> Result<f64> {
        if !self.pending.is_empty() {
            let tail = [std::mem::take(&mut self.pending)];
            let out = self.resampler.process_partial(Some(&tail[..]), None)?;
            self.write_output(&out[0])?;
        }
        let expected = scale_round(timeline, SPEAKER_RATE_HZ as u64, self.rate as u64);
        while self.produced < expected + self.delay as u64 {
            let out = self.resampler.process_partial(None::<&[Vec<f32>]>, None)?;
            if out[0].is_empty() {
                break;
            }
            self.write_output(&out[0])?;
        }
        if self.produced < expected + self.delay as u64 {
            warn!("Resampler flush ended early; the missing tail is zero-filled");
        }
        self.out.flush()?;
        let file = self.out.into_inner().map_err(|e| e.into_error())?;
        // Trims any extra frames, or zero-fills a short flush, so the length is exact.
        file.set_len(expected * 4)?;
        Ok(expected as f64 / SPEAKER_RATE_HZ as f64)
    }
}

/// Decodes an audio file straight to 16 kHz mono f32 (little-endian) at `output`.
///
/// Packets are streamed through a resampler in 1 s chunks, so memory stays small whatever the
/// file length. Each packet is placed on the container timeline by its timestamp: audio the
/// AAC encoder repeats at segment joins is dropped, and forward gaps are filled with silence.
/// The output therefore matches the container duration. `progress` gets fractions in 0..=1.
/// Returns the seconds written. On error the output may be partial: the caller's
/// `TempFileGuard` removes it.
pub fn decode_to_mono_16k_file(
    input: &Path,
    output: &Path,
    progress: &mut dyn FnMut(f32),
) -> Result<f64> {
    info!("Streaming decode to 16 kHz");

    // Formats Symphonia can't read go through ffmpeg first, as in decode_audio_file.
    let (_temp_wav_guard, decode_path) = if needs_ffmpeg_conversion(input) {
        let temp = convert_to_wav_with_ffmpeg(input, None)?;
        let path = temp.to_path_buf();
        (Some(temp), path)
    } else {
        (None, input.to_path_buf())
    };

    let file = File::open(&decode_path)
        .map_err(|e| anyhow!("Failed to open audio file '{}': {}", decode_path.display(), e))?;
    let mut hint = Hint::new();
    if let Some(ext) = decode_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("Failed to probe audio format: {}", e))?
        .format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("No audio track found in file"))?;
    let track_id = track.id;
    let time_base = track.codec_params.time_base;
    let n_frames = track.codec_params.n_frames;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("Failed to create decoder: {}", e))?;

    // Symphonia parses the mp4 edit list but does not apply it, and gapless trimming is off by
    // default, so the packet timestamps are the raw container timeline and nothing is trimmed here.
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut stream: Option<Resample16k> = None;
    let mut rate = 0u32;
    let mut channels = 0u16;
    // Container timeline position, in input-rate frames.
    let mut written = 0u64;
    let mut last_pct = 0u32;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => {
                warn!("Error reading packet: {}", e);
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(e) => {
                warn!("Error decoding packet: {}", e);
                continue;
            }
        };

        let spec = *decoded.spec();
        let capacity = decoded.capacity() as u64;
        let buf = sample_buf.get_or_insert_with(|| {
            // The decoder's rate and channels win over the container's (see decode_audio_file).
            rate = spec.rate;
            channels = spec.channels.count() as u16;
            SampleBuffer::<f32>::new(capacity, spec)
        });
        buf.copy_interleaved_ref(decoded);
        let mono = audio_to_mono(buf.samples(), channels);
        note_input_buffered(mono.len());

        let ts = ts_to_frames(packet.ts(), time_base, rate);
        let declared = n_frames.map(|n| ts_to_frames(n, time_base, rate));
        let (skip, zero_fill, write) =
            align_packet(ts, mono.len() as u64, written, Bounds::new(declared, rate))?;
        if stream.is_none() {
            stream = Some(Resample16k::new(rate, output)?);
        }
        let s = stream.as_mut().expect("stream was created above");
        s.push_silence(zero_fill)?;
        s.push(&mono[skip as usize..(skip + write) as usize])?;
        written = written.saturating_add(zero_fill).saturating_add(write);

        if let Some(n) = n_frames {
            let pct = ((packet.ts() as f64 / n as f64) * 100.0).min(100.0) as u32;
            if pct > last_pct {
                last_pct = pct;
                progress(pct as f32 / 100.0);
            }
        }
    }

    if let Some(n) = n_frames {
        let declared = ts_to_frames(n, time_base, rate);
        if timeline_is_short(written, declared) {
            warn!(
                "Decoded {:.2} s of a declared {:.2} s; the file may be truncated",
                written as f64 / rate as f64,
                declared as f64 / rate as f64
            );
        }
    }
    let stream = stream.ok_or_else(|| anyhow!("No audio samples decoded from file"))?;
    let seconds = stream.finish(written)?;
    progress(1.0);
    info!("Streamed {:.2}s of 16 kHz audio", seconds);
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_he_aac_uses_decoded_rate_not_container_rate() {
        // HE-AAC (SBR) fixture: the container declares 48 kHz stereo, but
        // Symphonia has no SBR support and decodes only the AAC-LC core at
        // 24 kHz. Trusting the container rate halves the duration and makes
        // the audio play at 2x speed, which truncated long imported
        // recordings to half their length.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/he_aac_48k_5s.m4a");
        let decoded = decode_audio_file(&path).expect("failed to decode HE-AAC fixture");

        assert_eq!(
            decoded.sample_rate, 24000,
            "sample_rate must come from the decoder output, not container metadata"
        );
        assert!(
            (decoded.duration_seconds - 5.16).abs() < 0.5,
            "duration must be ~5s (was reported as ~2.6s before the fix), got {:.2}s",
            decoded.duration_seconds
        );

        // The 16 kHz conversion must preserve the real duration as well.
        let whisper_samples = decoded.to_whisper_format();
        let duration_16k = whisper_samples.len() as f64 / 16000.0;
        assert!(
            (duration_16k - 5.16).abs() < 0.5,
            "whisper-format duration must be ~5s, got {:.2}s",
            duration_16k
        );
    }

    #[test]
    fn test_to_whisper_format_mono_16k() {
        // Already in correct format
        let audio = DecodedAudio {
            samples: vec![0.1, 0.2, 0.3],
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 0.0001875,
        };

        let result = audio.to_whisper_format();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_to_whisper_format_stereo_to_mono() {
        // Stereo input
        let audio = DecodedAudio {
            samples: vec![0.2, 0.4, 0.6, 0.8], // 2 stereo frames
            sample_rate: 16000,
            channels: 2,
            duration_seconds: 0.000125,
        };

        let result = audio.to_whisper_format();
        assert_eq!(result.len(), 2); // Should be mono now
        // Average of (0.2, 0.4) = 0.3 and (0.6, 0.8) = 0.7
        assert!((result[0] - 0.3).abs() < 0.001);
        assert!((result[1] - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_to_whisper_format_resamples_48k_to_16k() {
        // 48kHz mono input - should be downsampled to 16kHz
        // Use a larger sample to ensure resampler works correctly
        // 48000 samples at 48kHz = 1 second → 16000 samples at 16kHz
        let audio = DecodedAudio {
            samples: vec![0.5; 4800], // 0.1 seconds at 48kHz
            sample_rate: 48000,
            channels: 1,
            duration_seconds: 4800.0 / 48000.0,
        };

        let result = audio.to_whisper_format();
        // Output length should be approximately input_len / 3 (16000/48000 ratio)
        // 4800 / 3 = 1600
        assert!(!result.is_empty(), "Result should not be empty");
        assert!(result.len() > 1000 && result.len() < 2000,
            "Expected ~1600 samples, got {}", result.len());
    }

    #[test]
    fn test_chunked_resample_same_rate() {
        let input = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let result = chunked_resample_with_progress(&input, 16000, 16000, None);
        assert_eq!(result.len(), input.len());
        for (i, &sample) in result.iter().enumerate() {
            assert!((sample - input[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_chunked_resample_empty_input() {
        let input: Vec<f32> = vec![];
        let result = chunked_resample_with_progress(&input, 48000, 16000, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_chunked_resample_downsamples_correctly() {
        // 48kHz to 16kHz = 3x downsampling with a 2-second signal
        let input: Vec<f32> = (0..96000).map(|i| (i as f32 / 96000.0)).collect();
        let result = chunked_resample_with_progress(&input, 48000, 16000, None);

        // Output should be approximately 1/3 the length
        let expected_len = 96000.0 * (16000.0 / 48000.0);
        assert!(
            (result.len() as f64 - expected_len).abs() < 200.0,
            "Expected ~{} samples, got {}",
            expected_len,
            result.len()
        );
    }

    #[test]
    fn test_chunked_resample_preserves_signal_range() {
        // 1 second of sine wave at 44100Hz
        let input: Vec<f32> = (0..44100)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let result = chunked_resample_with_progress(&input, 44100, 16000, None);

        for sample in &result {
            assert!(
                *sample >= -1.1 && *sample <= 1.1,
                "Sample {} out of expected range",
                sample
            );
        }
    }

    #[test]
    fn test_chunked_resample_matches_single_pass() {
        // Verify chunked output is close to single-pass for small files
        let input: Vec<f32> = (0..48000)
            .map(|i| (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();

        let single_pass = resample_audio(&input, 48000, 16000);
        let chunked = chunked_resample_with_progress(&input, 48000, 16000, None);

        // Lengths should be very close
        let len_diff = (single_pass.len() as i64 - chunked.len() as i64).unsigned_abs();
        assert!(
            len_diff < 50,
            "Length mismatch: single_pass={}, chunked={}",
            single_pass.len(),
            chunked.len()
        );

        // Compare overlapping samples (allow some tolerance at chunk boundaries)
        let compare_len = single_pass.len().min(chunked.len());
        let mut max_diff = 0.0f32;
        for i in 0..compare_len {
            let diff = (single_pass[i] - chunked[i]).abs();
            max_diff = max_diff.max(diff);
        }
        // Chunk boundaries may introduce small discontinuities
        assert!(
            max_diff < 0.15,
            "Max sample difference too large: {}",
            max_diff
        );
    }

    #[test]
    fn test_decoded_audio_duration_calculation() {
        let audio = DecodedAudio {
            samples: vec![0.0; 48000], // 1 second at 48kHz mono
            sample_rate: 48000,
            channels: 1,
            duration_seconds: 1.0,
        };

        // Duration should be samples / sample_rate for mono
        let calculated_duration = audio.samples.len() as f64 / audio.sample_rate as f64;
        assert!((calculated_duration - audio.duration_seconds).abs() < 0.001);
    }

    #[test]
    fn test_decoded_audio_stereo_duration() {
        let audio = DecodedAudio {
            samples: vec![0.0; 96000], // 1 second at 48kHz stereo (2 channels)
            sample_rate: 48000,
            channels: 2,
            duration_seconds: 1.0,
        };

        // Duration should be samples / (sample_rate * channels) for stereo
        let frames = audio.samples.len() / audio.channels as usize;
        let calculated_duration = frames as f64 / audio.sample_rate as f64;
        assert!((calculated_duration - audio.duration_seconds).abs() < 0.001);
    }

    #[test]
    fn test_to_whisper_format_handles_large_file_threshold() {
        // Test that large files use chunked sinc resampling path
        // LARGE_FILE_THRESHOLD is 14_400_000 samples
        // We'll test with a smaller sample to verify the path selection logic works
        let audio = DecodedAudio {
            samples: vec![0.5; 1000], // Small file
            sample_rate: 48000,
            channels: 1,
            duration_seconds: 1000.0 / 48000.0,
        };

        let result = audio.to_whisper_format();
        // Should complete without error and produce valid output
        assert!(!result.is_empty());
        assert!(result.len() < 1000); // Downsampled
    }

    #[test]
    fn test_normalize_audio_samples_already_normalized() {
        let samples = vec![0.5, -0.5, 0.0, 0.9, -0.9];
        let result = normalize_audio_samples(samples.clone());
        // Should be unchanged (already in range)
        for (i, &s) in result.iter().enumerate() {
            assert!((s - samples[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_normalize_audio_samples_exceeds_range() {
        let samples = vec![0.5, -0.5, 2.0, -1.5]; // max_abs = 2.0
        let result = normalize_audio_samples(samples);
        // All samples should be scaled by 0.5 (1.0 / 2.0)
        assert!((result[0] - 0.25).abs() < 0.001);
        assert!((result[1] - -0.25).abs() < 0.001);
        assert!((result[2] - 1.0).abs() < 0.001);
        assert!((result[3] - -0.75).abs() < 0.001);
    }

    #[test]
    fn test_normalize_audio_samples_handles_nan() {
        let samples = vec![0.5, f32::NAN, 0.3];
        let result = normalize_audio_samples(samples);
        assert!((result[0] - 0.5).abs() < 0.001);
        assert_eq!(result[1], 0.0); // NaN replaced with 0
        assert!((result[2] - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_normalize_audio_samples_handles_infinity() {
        let samples = vec![0.5, f32::INFINITY, -0.3];
        let result = normalize_audio_samples(samples);
        assert!((result[0] - 0.5).abs() < 0.001); // preserved
        assert_eq!(result[1], 0.0); // infinity → 0
        assert!((result[2] - (-0.3)).abs() < 0.001); // preserved
    }

    #[test]
    fn test_needs_ffmpeg_conversion() {
        assert!(needs_ffmpeg_conversion(Path::new("video.mkv")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.webm")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.wma")));
        // Case insensitive
        assert!(needs_ffmpeg_conversion(Path::new("meeting.MKV")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.WMA")));
        assert!(needs_ffmpeg_conversion(Path::new("audio.WebM")));
        // Symphonia-native formats should NOT need ffmpeg
        assert!(!needs_ffmpeg_conversion(Path::new("audio.mp4")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.wav")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.mp3")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.flac")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.ogg")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.aac")));
        assert!(!needs_ffmpeg_conversion(Path::new("audio.m4a")));
        // No extension
        assert!(!needs_ffmpeg_conversion(Path::new("noext")));
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    const MEETUP: &str = "/workspace/.superpowers/spike-audio/meetup.mp4";

    /// Stereo 16-bit WAV from frames of `[left, right]`.
    fn write_wav(path: &Path, rate: u32, frames: &[[f32; 2]]) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for frame in frames {
            for &s in frame {
                w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    fn tone(frames: usize, rate: u32) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let v = 0.3 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 3000.0 * t).sin();
                [v, v]
            })
            .collect()
    }

    fn read_f32(path: &Path) -> Vec<f32> {
        std::fs::read(path)
            .unwrap()
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    fn exact(limit: Option<u64>, max_gap: u64) -> Bounds {
        Bounds { limit, max_gap, tolerance: 0, rate_1s: 16_000 }
    }

    fn place(ts: u64, frames: u64, written: u64) -> (u64, u64, u64) {
        align_packet(ts, frames, written, exact(None, u64::MAX)).unwrap()
    }

    #[test]
    fn align_packet_drops_backward_overlap() {
        // Packet at 100 (50 frames) starts 20 frames before the 120 already written.
        assert_eq!(place(100, 50, 120), (20, 0, 30));
    }

    #[test]
    fn align_packet_drops_a_packet_already_written() {
        assert_eq!(place(100, 10, 120), (10, 0, 0));
    }

    #[test]
    fn align_packet_zero_fills_forward_gap() {
        assert_eq!(place(200, 50, 120), (0, 80, 50));
    }

    #[test]
    fn align_packet_keeps_exact_continuity() {
        assert_eq!(place(120, 50, 120), (0, 0, 50));
    }

    #[test]
    fn align_packet_always_ends_on_the_later_of_written_and_packet_end() {
        for ts in 0..40u64 {
            for frames in 0..20u64 {
                for written in 0..40u64 {
                    let (skip, zero, write) = place(ts, frames, written);
                    assert!(skip <= frames);
                    assert_eq!(write, frames - skip);
                    assert_eq!(
                        written + zero + write,
                        written.max(ts + frames),
                        "ts={ts} frames={frames} written={written}"
                    );
                }
            }
        }
    }

    #[test]
    fn align_packet_keeps_real_frames_past_the_declared_end() {
        // An MP3 without a Xing header declares too short a length. Its decoded frames are real,
        // so a packet at the timeline position is kept whole even past the limit.
        assert_eq!(align_packet(130, 50, 130, exact(Some(120), u64::MAX)).unwrap(), (0, 0, 50));
        assert_eq!(align_packet(120, 50, 120, exact(Some(100), u64::MAX)).unwrap(), (0, 0, 50));
    }

    #[test]
    fn align_packet_zero_fills_a_long_gap_inside_the_declared_length() {
        // 200 frames of gap with a 100-frame cap for undeclared lengths: fine when declared.
        assert_eq!(align_packet(200, 50, 0, exact(Some(1000), 100)).unwrap(), (0, 200, 50));
    }

    #[test]
    fn align_packet_rejects_a_gap_past_the_declared_length() {
        // Two seconds of silence past the declared end is corrupt.
        assert!(align_packet(1000 + 32_000, 50, 1000, exact(Some(1000), u64::MAX)).is_err());
    }

    #[test]
    fn align_packet_zero_fills_a_short_gap_past_the_declared_length() {
        // A skipped packet leaves a short gap, even when the declared length has already run out.
        assert_eq!(
            align_packet(1000 + 768, 50, 1000, exact(Some(1000), u64::MAX)).unwrap(),
            (0, 768, 50)
        );
        assert_eq!(
            align_packet(1000 + 16_000, 50, 1000, exact(Some(1000), u64::MAX)).unwrap(),
            (0, 16_000, 50)
        );
    }

    #[test]
    fn align_packet_rejects_a_long_gap_when_no_length_is_declared() {
        assert_eq!(align_packet(100, 50, 0, exact(None, 100)).unwrap(), (0, 100, 50));
        assert!(align_packet(101, 50, 0, exact(None, 100)).is_err());
    }

    #[test]
    fn align_packet_rejects_huge_timestamps_without_overflow() {
        assert!(align_packet(u64::MAX, 50, 0, exact(Some(1000), 100)).is_err());
        assert!(align_packet(u64::MAX, 50, 0, exact(None, 100)).is_err());
    }

    #[test]
    fn align_packet_treats_small_offsets_as_jitter() {
        let jitter = Bounds { limit: None, max_gap: u64::MAX, tolerance: 220, rate_1s: 44_100 };
        // Up to the tolerance either side: no skip and no silence.
        assert_eq!(align_packet(900, 50, 1000, jitter).unwrap(), (0, 0, 50));
        assert_eq!(align_packet(1200, 50, 1000, jitter).unwrap(), (0, 0, 50));
        // Beyond it, the usual rules apply.
        assert_eq!(align_packet(1221, 50, 1000, jitter).unwrap(), (0, 221, 50));
        assert_eq!(align_packet(779, 300, 1000, jitter).unwrap(), (221, 0, 79));
    }

    #[test]
    fn timescale_rounding_of_44k_in_milliseconds_is_continuous() {
        // 44.1 kHz audio in a 1 kHz timescale: each timestamp rounds to the millisecond, which is
        // up to 22 frames off. Every packet must still follow on with no skip and no silence.
        let bounds = Bounds::new(None, 44_100);
        let tb = Some(TimeBase { numer: 1, denom: 1000 });
        let mut written = 0u64;
        for k in 0..200u64 {
            let ts_ms = (k * 1024 * 1000 + 22_050) / 44_100;
            let ts = ts_to_frames(ts_ms, tb, 44_100);
            let (skip, zero, write) = align_packet(ts, 1024, written, bounds).unwrap();
            assert_eq!((skip, zero, write), (0, 0, 1024), "packet {k}");
            written += zero + write;
        }
        assert_eq!(written, 200 * 1024);
    }

    #[test]
    fn bounds_declare_the_limit_plus_one_second() {
        let b = Bounds::new(Some(100), 16_000);
        assert_eq!(b.limit, Some(16_100));
        assert_eq!(b.tolerance, 80);
        assert_eq!(b.max_gap, 60 * 16_000);
        assert_eq!(b.rate_1s, 16_000);
    }

    #[test]
    fn scale_round_rounds_half_up_and_saturates() {
        assert_eq!(scale_round(1, 1, 3), 0); // 0.33
        assert_eq!(scale_round(1, 2, 3), 1); // 0.67
        assert_eq!(scale_round(3, 1, 2), 2); // 1.5 rounds up
        assert_eq!(scale_round(u64::MAX, u64::MAX, 1), u64::MAX);
    }

    #[test]
    fn timeline_is_short_below_98_percent() {
        assert!(timeline_is_short(979, 1000));
        assert!(!timeline_is_short(980, 1000));
        assert!(!timeline_is_short(1000, 1000));
    }

    #[test]
    fn ts_to_frames_scales_to_the_decoded_rate() {
        let tb = Some(TimeBase { numer: 1, denom: 48000 });
        // HE-AAC: 48 kHz container timestamps, 24 kHz decoded frames.
        assert_eq!(ts_to_frames(2048, tb, 24000), 1024);
        assert_eq!(ts_to_frames(2048, tb, 48000), 2048);
        assert_eq!(ts_to_frames(2048, None, 24000), 2048);
    }

    #[test]
    fn ts_to_frames_is_exact_at_the_extremes() {
        // A 1/3 time base at 2 Hz: 1 tick is 0.67 frames, which rounds up.
        assert_eq!(ts_to_frames(1, Some(TimeBase { numer: 1, denom: 3 }), 2), 1);
        // An invalid time base falls back to the raw timestamp.
        assert_eq!(ts_to_frames(5, Some(TimeBase { numer: 1, denom: 0 }), 48000), 5);
        assert_eq!(ts_to_frames(u64::MAX, Some(TimeBase { numer: 1, denom: 1 }), 48000), u64::MAX);
    }

    #[test]
    fn decode_to_mono_16k_file_matches_reference_tone() {
        // The whole decode is not a usable reference: it returns 47,914 frames for this 3 s input
        // (SincFixedIn, no flush) and sits about one sample off with a fractional delay. The
        // tone is analytic, so the streamed output is checked against the exact signal.
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("tone.wav");
        write_wav(&src, 48000, &tone(144_000, 48000)); // 3 s
        let out = dir.path().join("out.f32");

        let secs = decode_to_mono_16k_file(&src, &out, &mut |_| {}).unwrap();
        let streamed = read_f32(&out);
        assert_eq!(streamed.len(), 48_000);
        assert!((secs - 3.0).abs() < 1e-9);

        let mean_abs_err: f32 = streamed
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let t = i as f32 / 16000.0;
                let ideal = 0.3 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 3000.0 * t).sin();
                (v - ideal).abs()
            })
            .sum::<f32>()
            / streamed.len() as f32;
        assert!(mean_abs_err < 1e-3, "mean abs error {mean_abs_err}");
    }

    #[test]
    fn decode_streams_without_loading_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("long.wav");
        write_wav(&src, 48000, &tone(60 * 48000, 48000)); // 60 s
        let out = dir.path().join("long.f32");

        INPUT_HIGH_WATER.with(|hw| hw.set(0));
        decode_to_mono_16k_file(&src, &out, &mut |_| {}).unwrap();
        let high_water = INPUT_HIGH_WATER.with(|hw| hw.get());

        assert!(high_water <= 2 * 48000, "held {high_water} input samples at once");
        assert_eq!(read_f32(&out).len(), 60 * 16000);
    }

    #[test]
    fn decode_keeps_timing_exact() {
        // 10.37 s at 48 kHz: not a whole number of 1 s chunks.
        let frames = 497_760usize;
        let mut audio = vec![[0.0f32; 2]; frames];
        let clicks = [0usize, 240_000, 494_400]; // 0.0 s, 5.0 s (chunk boundary), 10.3 s
        for &c in &clicks {
            audio[c] = [0.8, 0.8];
        }
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("clicks.wav");
        write_wav(&src, 48000, &audio);
        let out = dir.path().join("clicks.f32");

        decode_to_mono_16k_file(&src, &out, &mut |_| {}).unwrap();
        let samples = read_f32(&out);
        let expected_len = (10.37f64 * 16000.0).round() as i64;
        assert!((samples.len() as i64 - expected_len).abs() <= 1, "len {}", samples.len());

        for &c in &clicks {
            let expected = (c as f64 * 16000.0 / 48000.0).round() as usize;
            let lo = expected.saturating_sub(20);
            let hi = (expected + 21).min(samples.len());
            let peak = (lo..hi)
                .max_by(|&a, &b| samples[a].abs().total_cmp(&samples[b].abs()))
                .unwrap();
            assert!(samples[peak].abs() > 0.1, "no click near input {c}");
            assert!(
                (peak as i64 - expected as i64).abs() <= 2,
                "click at input {c}: peak {peak}, expected {expected}"
            );
        }
    }

    #[test]
    fn decode_audio_file_keeps_plain_wav_samples_exactly() {
        // Float WAV: symphonia reads the values back bit for bit, so any change in how packets
        // are placed shows up as a difference. A WAV has no timestamp jumps, so nothing moves.
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("float.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let values: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.001).sin() * 0.5).collect();
        let mut w = hound::WavWriter::create(&src, spec).unwrap();
        for &v in &values {
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();

        let decoded = decode_audio_file(&src).unwrap();
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channels, 2);
        assert!((decoded.duration_seconds - 1.0).abs() < 1e-9);
        assert_eq!(decoded.samples, values);
    }

    #[test]
    #[ignore = "needs the gitignored synthetic VBR MP3 fixture (no Xing header, 120 s)"]
    fn vbr_mp3_without_xing_decodes_fully_with_both_decoders() {
        // symphonia estimates n_frames (~51 s) for an MP3 without a Xing header; the audio is 120 s.
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.superpowers/sdd/2026-10-17-conclave-speaker-id/fixtures/vbr_noxing.mp3");
        let whole = decode_audio_file(&src).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("vbr.f32");
        let streamed = decode_to_mono_16k_file(&src, &out, &mut |_| {}).unwrap();
        println!(
            "vbr mp3: decode_audio_file {:.3} s, decode_to_mono_16k_file {streamed:.3} s",
            whole.duration_seconds
        );
        assert!((whole.duration_seconds - 120.0).abs() <= 0.05, "{}", whole.duration_seconds);
        assert!((streamed - 120.0).abs() <= 0.05, "{streamed}");
    }

    #[test]
    #[ignore = "needs the gitignored synthetic corrupt VBR MP3 fixture (4 KB overwritten at 85 %)"]
    fn corrupt_vbr_mp3_decodes_fully_with_both_decoders() {
        // Two packets fail to decode and are zero-filled by their timestamp gap. The demuxer also
        // resyncs past about 0.5 s of frames without any error or timestamp gap, so the result is
        // short of 120 s by that amount, not by a decode error.
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../.superpowers/sdd/2026-10-17-conclave-speaker-id/fixtures/vbr_corrupt.mp3",
        );
        let whole = decode_audio_file(&src).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("corrupt.f32");
        let streamed = decode_to_mono_16k_file(&src, &out, &mut |_| {}).unwrap();
        println!(
            "corrupt mp3: decode_audio_file {:.3} s, decode_to_mono_16k_file {streamed:.3} s",
            whole.duration_seconds
        );
        assert!((119.0..=120.05).contains(&whole.duration_seconds), "{}", whole.duration_seconds);
        assert!((119.0..=120.05).contains(&streamed), "{streamed}");
    }

    #[test]
    #[ignore = "needs the real meetup.mp4 spike recording (never committed)"]
    fn meetup_decode_audio_file_matches_container_duration() {
        let src = Path::new(MEETUP);
        let decoded = decode_audio_file(src).unwrap();
        let container = container_seconds(src);
        println!(
            "meetup decode_audio_file: decoded {:.3} s, container {container:.3} s, {} channels",
            decoded.duration_seconds, decoded.channels
        );
        assert!(
            (decoded.duration_seconds - container).abs() <= 0.05,
            "decoded {:.3} s vs container {container:.3} s",
            decoded.duration_seconds
        );
    }

    #[test]
    #[ignore = "needs the real meetup.mp4 spike recording (never committed)"]
    fn meetup_decode_matches_container_duration() {
        let src = Path::new(MEETUP);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("meetup.f32");

        let decoded = decode_to_mono_16k_file(src, &out, &mut |_| {}).unwrap();
        let container = container_seconds(src);
        println!("meetup: decoded {decoded:.3} s, container {container:.3} s");
        assert!(
            (decoded - container).abs() <= 0.05,
            "decoded {decoded:.3} s vs container {container:.3} s"
        );
    }

    /// Track duration from the container: n_frames in time-base units.
    fn container_seconds(path: &Path) -> f64 {
        let file = File::open(path).unwrap();
        let mut hint = Hint::new();
        hint.with_extension("mp4");
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                MediaSourceStream::new(Box::new(file), Default::default()),
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .unwrap();
        let p = &probed.format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).unwrap().codec_params;
        let n = p.n_frames.unwrap() as f64;
        match p.time_base {
            Some(tb) => n * tb.numer as f64 / tb.denom as f64,
            None => n / p.sample_rate.unwrap() as f64,
        }
    }
}
