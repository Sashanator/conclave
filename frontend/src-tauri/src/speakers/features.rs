// NeMo log-mel features for the TitaNet speaker-embedding model.
// Rust port of sherpa-onnx v1.13.8 (speaker-embedding-extractor-nemo-impl.h, features.cc) on
// kaldi-native-fbank v1.22.3 (feature-window.cc, feature-fbank.cc, mel-computations.cc).
// Each constant that is not read from the model metadata names its source line.

use std::f64::consts::TAU;

use ndarray::Array2;
use realfft::RealFftPlanner;

#[derive(Debug, Clone)]
pub struct NemoMeta {
    pub feat_dim: usize,
    pub sample_rate: usize,
    pub window_size_ms: f32,
    pub window_stride_ms: f32,
    pub window_type: String,
    pub normalize_type: String,
}

// sherpa-onnx speaker-embedding-extractor-nemo-impl.h, CreateStream: low_freq = 0.
const LOW_FREQ_HZ: f32 = 0.0;
// sherpa-onnx features.h: high_freq = -400 (not overridden by CreateStream), an offset from Nyquist.
const HIGH_FREQ_OFFSET_HZ: f32 = -400.0;
// sherpa-onnx features.h: preemph_coeff = 0.97 (default, not overridden).
const PREEMPH_COEFF: f32 = 0.97;
// kaldi-native-fbank feature-window.h: blackman_coeff = 0.42f (only used by the "blackman" window).
const BLACKMAN_COEFF: f32 = 0.42;
// kaldi-native-fbank feature-fbank.cc FbankComputer::Compute: log floor is numeric_limits<float>::epsilon().
const LOG_FLOOR: f32 = f32::EPSILON;
// sherpa-onnx speaker-embedding-extractor-nemo-impl.h NormalizePerFeature: stddev + 1e-5f.
const NORMALIZE_EPS: f32 = 1e-5;
// mel-computations.h InverseMelScaleSlaney / MelScaleSlaney constants.
const SLANEY_LINEAR_MAX_HZ: f32 = 1000.0;
const SLANEY_LINEAR_MAX_MEL: f32 = 15.0;
const SLANEY_LOG_STEP: f32 = 0.06875177742094911; // log(6.4) / 27
const SLANEY_LOG_SCALE: f32 = 14.545078505785561; // 27 / log(6.4)

pub fn read_nemo_meta(session: &ort::session::Session) -> Result<NemoMeta, String> {
    let meta = session.metadata().map_err(|e| e.to_string())?;
    nemo_meta_from(|key| {
        meta.custom(key)
            .map_err(|e| format!("metadata '{key}': {e}"))
    })
}

/// The metadata rules, on a key lookup so that they can be tested without a model.
fn nemo_meta_from(
    get: impl Fn(&str) -> Result<Option<String>, String>,
) -> Result<NemoMeta, String> {
    let req =
        |key: &str| get(key)?.ok_or_else(|| format!("'{key}' does not exist in the metadata"));
    // sherpa reads the window lengths as int32.
    let int = |key: &str| -> Result<f32, String> {
        req(key)?
            .parse::<i32>()
            .map(|v| v as f32)
            .map_err(|_| format!("invalid '{key}' in the metadata"))
    };
    let framework = req("framework")?;
    if framework != "nemo" {
        return Err(format!(
            "unsupported framework '{framework}', expected 'nemo'"
        ));
    }
    let out = NemoMeta {
        feat_dim: req("feat_dim")?
            .parse()
            .map_err(|_| "invalid 'feat_dim' in the metadata")?,
        sample_rate: req("sample_rate")?
            .parse()
            .map_err(|_| "invalid 'sample_rate' in the metadata")?,
        window_size_ms: int("window_size_ms")?,
        window_stride_ms: int("window_stride_ms")?,
        // Same defaults as sherpa's model loader.
        window_type: get("window_type")?.unwrap_or_else(|| "povey".into()),
        normalize_type: get("feature_normalize_type")?.unwrap_or_default(),
    };
    check_meta(&out)?;
    Ok(out)
}

/// Longest window the port accepts (about 65 s at 16 kHz). Bounds the allocations that a bad model could ask for.
const MAX_FRAME_LEN: usize = 1 << 20;

/// Rejects settings the port cannot compute, so that `nemo_features` never has to.
fn check_meta(m: &NemoMeta) -> Result<(), String> {
    if m.sample_rate != 16_000 {
        return Err(format!("unsupported sample_rate {}, expected 16000", m.sample_rate));
    }
    if !(m.window_size_ms.is_finite() && m.window_size_ms > 0.0)
        || !(m.window_stride_ms.is_finite() && m.window_stride_ms > 0.0)
    {
        return Err("window_size_ms and window_stride_ms must be positive".into());
    }
    let (frame_len, shift, padded) = frame_params(m)?;
    if frame_len < 2 || shift < 1 {
        return Err("window_size_ms and window_stride_ms are too small for the sample rate".into());
    }
    if m.normalize_type != "per_feature" {
        return Err(format!(
            "unsupported feature_normalize_type '{}'",
            m.normalize_type
        ));
    }
    mel_banks(m.feat_dim, m.sample_rate as f32, padded)?;
    if window_coeffs(&m.window_type, frame_len).is_none() {
        return Err(format!("unsupported window_type '{}'", m.window_type));
    }
    Ok(())
}

/// Log-mel features as `[feat_dim, frames]`, per-feature normalised over time. `samples` must already
/// be at `meta.sample_rate`. The frames are not padded to a multiple of 16; the embedder does that.
pub fn nemo_features(samples: &[f32], meta: &NemoMeta) -> Array2<f32> {
    // `check_meta` runs in `read_nemo_meta`; a hand-built NemoMeta must pass it too.
    let (frame_len, shift, padded) = frame_params(meta).expect("checked by check_meta");
    let feat_dim = meta.feat_dim;
    let window = window_coeffs(&meta.window_type, frame_len).expect("checked by check_meta");
    let bins = mel_banks(feat_dim, meta.sample_rate as f32, padded).expect("checked by check_meta");

    // kaldi `NumFrames` with snip_edges: frames must fit completely into the signal.
    let num_frames = if samples.len() < frame_len {
        0
    } else {
        1 + (samples.len() - frame_len) / shift
    };
    if num_frames == 0 {
        return Array2::zeros((feat_dim, 0));
    }

    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(padded);
    let mut buf = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut power = vec![0f32; spectrum.len()];
    let mut feats = vec![0f32; num_frames * feat_dim]; // [frame][feat]
    for t in 0..num_frames {
        let start = t * shift;
        // Zero-padded to the FFT size; the FFT overwrites the buffer, so clear it every frame.
        buf.fill(0.0);
        buf[..frame_len].copy_from_slice(&samples[start..start + frame_len]);
        // kaldi Preemphasize: from the last sample back, so each sample sees its original predecessor.
        for i in (1..frame_len).rev() {
            buf[i] -= PREEMPH_COEFF * buf[i - 1];
        }
        buf[0] -= PREEMPH_COEFF * buf[0];
        for (x, w) in buf[..frame_len].iter_mut().zip(&window) {
            *x *= w;
        }
        fft.process(&mut buf, &mut spectrum)
            .expect("buffer lengths match the plan");
        for (p, c) in power.iter_mut().zip(&spectrum) {
            *p = c.re * c.re + c.im * c.im;
        }
        for (b, bin) in bins.iter().enumerate() {
            let mut energy = 0f32;
            for (k, w) in bin.weights.iter().enumerate() {
                energy += w * power[bin.first + k];
            }
            feats[t * feat_dim + b] = energy.max(LOG_FLOOR).ln();
        }
    }

    // sherpa NormalizePerFeature: per feature over time, variance from the centred values.
    let n = num_frames as f32;
    for d in 0..feat_dim {
        let mut sum = 0f32;
        for t in 0..num_frames {
            sum += feats[t * feat_dim + d];
        }
        let mean = sum / n;
        let mut sq = 0f32;
        for t in 0..num_frames {
            let c = feats[t * feat_dim + d] - mean;
            sq += c * c;
        }
        let stddev = (sq / n).sqrt();
        for t in 0..num_frames {
            let x = &mut feats[t * feat_dim + d];
            *x = (*x - mean) / (stddev + NORMALIZE_EPS);
        }
    }
    Array2::from_shape_fn((feat_dim, num_frames), |(d, t)| feats[t * feat_dim + d])
}

/// Frame length, hop and FFT size in samples (kaldi `WindowSize`, `WindowShift`, `PaddedWindowSize`).
/// The f32 arithmetic is kept in the same order as kaldi, so the sizes truncate the same way.
fn frame_params(meta: &NemoMeta) -> Result<(usize, usize, usize), String> {
    let sr = meta.sample_rate as f32;
    let frame_len = (sr * 0.001 * meta.window_size_ms) as usize;
    let shift = (sr * 0.001 * meta.window_stride_ms) as usize;
    if frame_len > MAX_FRAME_LEN {
        return Err("window_size_ms is too large for the sample rate".into());
    }
    let padded = frame_len
        .checked_next_power_of_two()
        .ok_or("window_size_ms is too large for the sample rate")?;
    Ok((frame_len, shift, padded))
}

/// kaldi `GetWindow` (feature-window.cc). Computed in f64 and stored as f32, as kaldi does.
/// Returns None for a window type the port does not know.
fn window_coeffs(window_type: &str, n: usize) -> Option<Vec<f32>> {
    // "hann" is periodic (period n); every other type uses the symmetric period n - 1.
    let a = if window_type == "hann" {
        TAU / n as f64
    } else {
        TAU / (n as f64 - 1.0)
    };
    let b = BLACKMAN_COEFF as f64;
    let value = |i: usize| -> Option<f64> {
        let x = i as f64;
        Some(match window_type {
            "hanning" => 0.5 - 0.5 * (a * x).cos(),
            "sine" => (0.5 * a * x).sin(),
            "hamming" => 0.54 - 0.46 * (a * x).cos(),
            "hann" => 0.50 - 0.50 * (a * x).cos(),
            "povey" => (0.5 - 0.5 * (a * x).cos()).powf(0.85),
            "rectangular" => 1.0,
            "blackman" => b - 0.5 * (a * x).cos() + (0.5 - b) * (2.0 * a * x).cos(),
            _ => return None,
        })
    };
    (0..n).map(|i| value(i).map(|v| v as f32)).collect()
}

struct MelBin {
    first: usize,
    weights: Vec<f32>,
}

/// kaldi `MelBanks::InitLibrosaMelBanks` with the NeMo settings: slaney mel scale, slaney norm,
/// `low_freq` 0 and `high_freq` = Nyquist - 400.
fn mel_banks(num_bins: usize, sample_rate: f32, padded: usize) -> Result<Vec<MelBin>, String> {
    let num_fft_bins = padded / 2;
    // Checked before `with_capacity`, so a huge `feat_dim` from a model cannot ask for a huge allocation.
    if !(3..=num_fft_bins).contains(&num_bins) {
        return Err(format!(
            "feat_dim {num_bins} is outside 3..={num_fft_bins} for this window"
        ));
    }
    let nyquist = 0.5 * sample_rate;
    let low = LOW_FREQ_HZ;
    let high = nyquist + HIGH_FREQ_OFFSET_HZ;
    if low < 0.0 || low >= nyquist || high <= 0.0 || high > nyquist || high <= low {
        return Err(format!(
            "bad mel range {low}..{high} Hz for sample rate {sample_rate}"
        ));
    }
    let fft_bin_width = sample_rate / padded as f32;
    let mel_low = mel_scale_slaney(low);
    let mel_delta = (mel_scale_slaney(high) - mel_low) / (num_bins as f32 + 1.0);

    let mut bins = Vec::with_capacity(num_bins);
    for bin in 0..num_bins {
        let left = inv_mel_scale_slaney(mel_low + bin as f32 * mel_delta);
        let center = inv_mel_scale_slaney(mel_low + (bin + 1) as f32 * mel_delta);
        let right = inv_mel_scale_slaney(mel_low + (bin + 2) as f32 * mel_delta);
        let active: Vec<(usize, f32)> = (0..=num_fft_bins)
            .filter_map(|i| {
                let hz = fft_bin_width * i as f32;
                if hz > left && hz < right {
                    let w = if hz <= center {
                        (hz - left) / (center - left)
                    } else {
                        (right - hz) / (right - center)
                    };
                    Some((i, w * (2.0 / (right - left))))
                } else {
                    None
                }
            })
            .collect();
        let (first, _) = *active
            .first()
            .ok_or("num_mel_bins is too large for the window")?;
        let last = active.last().map_or(first, |&(i, _)| i);
        // The active bins of a triangle are contiguous, so the dense range matches kaldi's slice.
        let mut weights = vec![0f32; last + 1 - first];
        for (i, w) in active {
            weights[i - first] = w;
        }
        bins.push(MelBin { first, weights });
    }
    Ok(bins)
}

fn mel_scale_slaney(freq: f32) -> f32 {
    if freq <= SLANEY_LINEAR_MAX_HZ {
        freq * 3.0 / 200.0
    } else {
        SLANEY_LINEAR_MAX_MEL + SLANEY_LOG_SCALE * (freq / SLANEY_LINEAR_MAX_HZ).ln()
    }
}

fn inv_mel_scale_slaney(mel: f32) -> f32 {
    if mel <= SLANEY_LINEAR_MAX_MEL {
        200.0 / 3.0 * mel
    } else {
        SLANEY_LINEAR_MAX_HZ * ((mel - SLANEY_LINEAR_MAX_MEL) * SLANEY_LOG_STEP).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn meta() -> NemoMeta {
        NemoMeta {
            feat_dim: 80,
            sample_rate: 16_000,
            window_size_ms: 25.0,
            window_stride_ms: 10.0,
            window_type: "hann".into(),
            normalize_type: "per_feature".into(),
        }
    }

    #[test]
    fn features_shape_and_normalisation() {
        // A bare tone keeps the bins near 440 Hz almost constant over time, so the 1e-5 guard pulls their
        // std below 1. Noise at amplitude 0.3 keeps every bin varying enough for the 1e-3 check.
        let mut seed = 1u32;
        let signal: Vec<f32> = (0..16_000)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
                (2.0 * PI * 440.0 * i as f32 / 16_000.0).sin() + 0.3 * noise
            })
            .collect();
        let f = nemo_features(&signal, &meta());
        // snip-edges: 1 + (16000 - 400) / 160 = 98 frames
        assert_eq!(f.dim(), (80, 98));
        for row in f.rows() {
            let n = row.len() as f32;
            let mean = row.sum() / n;
            let std = (row.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n).sqrt();
            assert!(mean.abs() < 1e-3, "mean {mean}");
            assert!((std - 1.0).abs() < 1e-3, "std {std}");
        }
    }

    #[test]
    fn features_silence_is_finite() {
        let f = nemo_features(&vec![0.0; 16_000], &meta());
        assert_eq!(f.dim(), (80, 98));
        assert!(f.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn features_shorter_than_one_frame_is_empty() {
        let f = nemo_features(&[0.1; 100], &meta());
        assert_eq!(f.dim(), (80, 0));
    }

    /// Reads the real TitaNet metadata. Needs ONNX Runtime and `TITANET_ONNX=<path to titanet-large.onnx>`.
    #[test]
    #[ignore]
    fn titanet_metadata_matches_port() {
        let path = std::env::var("TITANET_ONNX").expect("set TITANET_ONNX");
        let session = ort::session::Session::builder()
            .unwrap()
            .commit_from_file(path)
            .unwrap();
        let m = read_nemo_meta(&session).unwrap();
        eprintln!("{m:?}");
        assert_eq!(m.feat_dim, 80);
        assert_eq!(m.sample_rate, 16_000);
        assert_eq!(m.window_size_ms, 25.0);
        assert_eq!(m.window_stride_ms, 10.0);
        assert_eq!(m.window_type, "hann");
        assert_eq!(m.normalize_type, "per_feature");
    }

    #[test]
    fn check_meta_rejects_settings_the_port_cannot_compute() {
        assert!(check_meta(&meta()).is_ok());
        let mut m = meta();
        m.normalize_type = "all_features".into();
        assert!(check_meta(&m).is_err());
        let mut m = meta();
        m.window_type = "kaiser".into();
        assert!(check_meta(&m).is_err());
        let mut m = meta();
        m.feat_dim = 2;
        assert!(check_meta(&m).is_err());
        let mut m = meta();
        m.sample_rate = 0;
        assert!(check_meta(&m).is_err());
    }

    #[test]
    fn check_meta_rejects_bad_window_sizes_before_allocating() {
        for ms in [f32::NAN, f32::INFINITY, 0.0, -25.0, 1e30] {
            let mut m = meta();
            m.window_size_ms = ms;
            assert!(check_meta(&m).is_err(), "window_size_ms {ms}");
        }
        for ms in [f32::NAN, f32::INFINITY, 0.0, -10.0] {
            let mut m = meta();
            m.window_stride_ms = ms;
            assert!(check_meta(&m).is_err(), "window_stride_ms {ms}");
        }
    }

    fn meta_from(pairs: &[(&str, &str)]) -> Result<NemoMeta, String> {
        nemo_meta_from(|key| {
            Ok(pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string()))
        })
    }

    const NEMO_PAIRS: [(&str, &str); 7] = [
        ("feat_dim", "80"),
        ("sample_rate", "16000"),
        ("window_size_ms", "25"),
        ("window_stride_ms", "10"),
        ("window_type", "hann"),
        ("feature_normalize_type", "per_feature"),
        ("framework", "nemo"),
    ];

    #[test]
    fn nemo_meta_requires_nemo_framework() {
        assert!(meta_from(&NEMO_PAIRS).is_ok());
        let without: Vec<_> = NEMO_PAIRS
            .iter()
            .copied()
            .filter(|(k, _)| *k != "framework")
            .collect();
        assert!(meta_from(&without).is_err());
        let mut other = NEMO_PAIRS.to_vec();
        other[6] = ("framework", "pytorch");
        assert!(meta_from(&other).is_err());
    }

    #[test]
    fn nemo_meta_rejects_other_sample_rates() {
        let mut other = NEMO_PAIRS.to_vec();
        other[1] = ("sample_rate", "8000");
        assert!(meta_from(&other).is_err());
        assert!(meta_from(&NEMO_PAIRS).is_ok());
    }

    #[test]
    fn nemo_meta_parses_window_ms_as_integers() {
        let mut fractional = NEMO_PAIRS.to_vec();
        fractional[2] = ("window_size_ms", "25.5");
        assert!(meta_from(&fractional).is_err());
        let mut junk = NEMO_PAIRS.to_vec();
        junk[3] = ("window_stride_ms", "nan");
        assert!(meta_from(&junk).is_err());
        let m = meta_from(&NEMO_PAIRS).unwrap();
        assert_eq!((m.window_size_ms, m.window_stride_ms), (25.0, 10.0));
    }
}
