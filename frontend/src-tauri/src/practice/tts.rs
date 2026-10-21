//! Supertonic 3 text-to-speech on ONNX Runtime: a port of the reference SDK's `Supertonic.__call__`
//! (supertonic 1.3.1, batch 1). DirectML when asked on Windows, CPU otherwise.

use std::path::Path;

use ndarray::Array3;
use ort::session::Session;
use ort::value::Tensor;
use rand::Rng;
use serde::Deserialize;

use super::tts_text::{preprocess, supported_only, Indexer};
use super::TTS_RATE;
use crate::speakers::runtime::{open, Backend};

const TTL_DIMS: [usize; 3] = [1, 50, 256];
const DP_DIMS: [usize; 3] = [1, 8, 16];
// The reference SDK's MIN_SPEED and MAX_SPEED.
const MIN_SPEED: f32 = 0.7;
const MAX_SPEED: f32 = 2.0;

pub struct Voice {
    ttl: Array3<f32>,
    dp: Array3<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TtsConfig {
    pub sample_rate: u32,
    pub base_chunk_size: usize,
    pub chunk_compress_factor: usize,
    pub latent_dim: usize,
}

pub struct Supertonic {
    dp: Session,
    text_enc: Session,
    vector_est: Session,
    vocoder: Session,
    indexer: Indexer,
    cfg: TtsConfig,
}

#[derive(Deserialize)]
struct StyleJson {
    dims: Vec<usize>,
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct VoiceJson {
    style_ttl: StyleJson,
    style_dp: StyleJson,
}

#[derive(Deserialize)]
struct ConfigJson {
    ae: AeJson,
    ttl: TtlJson,
}

#[derive(Deserialize)]
struct AeJson {
    sample_rate: u32,
    base_chunk_size: usize,
}

#[derive(Deserialize)]
struct TtlJson {
    chunk_compress_factor: usize,
    latent_dim: usize,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn read(dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = dir.join(name);
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Loads a voice style JSON (`style_ttl` and `style_dp`, each with `dims` and nested `data`).
pub fn load_voice(path: &Path) -> Result<Voice, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_voice(&bytes)
}

fn parse_voice(bytes: &[u8]) -> Result<Voice, String> {
    let v: VoiceJson =
        serde_json::from_slice(bytes).map_err(|e| format!("voice style is malformed: {e}"))?;
    Ok(Voice {
        ttl: style(v.style_ttl, TTL_DIMS)?,
        dp: style(v.style_dp, DP_DIMS)?,
    })
}

fn style(s: StyleJson, dims: [usize; 3]) -> Result<Array3<f32>, String> {
    if s.dims != dims {
        return Err(format!(
            "voice style has dims {:?}, expected {dims:?}",
            s.dims
        ));
    }
    let mut flat = Vec::with_capacity(dims.iter().product());
    flatten(&s.data, &mut flat)?;
    Array3::from_shape_vec(dims, flat)
        .map_err(|e| format!("voice style data does not match its dims: {e}"))
}

/// Flattens nested JSON arrays in row-major order, as numpy's reshape does.
fn flatten(v: &serde_json::Value, out: &mut Vec<f32>) -> Result<(), String> {
    match v {
        serde_json::Value::Array(items) => items.iter().try_for_each(|x| flatten(x, out)),
        serde_json::Value::Number(n) => {
            let x = n.as_f64().map(|x| x as f32).filter(|x| x.is_finite());
            out.push(x.ok_or("voice style values must be finite")?);
            Ok(())
        }
        _ => Err("voice style data must hold only numbers".into()),
    }
}

fn parse_config(bytes: &[u8]) -> Result<TtsConfig, String> {
    let c: ConfigJson =
        serde_json::from_slice(bytes).map_err(|e| format!("tts.json is malformed: {e}"))?;
    let cfg = TtsConfig {
        sample_rate: c.ae.sample_rate,
        base_chunk_size: c.ae.base_chunk_size,
        chunk_compress_factor: c.ttl.chunk_compress_factor,
        latent_dim: c.ttl.latent_dim,
    };
    // Zero sizes would divide by zero in `latent_shape`; the output is only ever `TTS_RATE`.
    if cfg.sample_rate != TTS_RATE
        || [
            cfg.base_chunk_size,
            cfg.chunk_compress_factor,
            cfg.latent_dim,
        ]
        .contains(&0)
    {
        return Err(format!("tts.json has unsupported values: {cfg:?}"));
    }
    Ok(cfg)
}

/// `(latent_dim * chunk_compress_factor, ceil(duration * sample_rate / (base_chunk_size * chunk_compress_factor)))`.
pub fn latent_shape(duration_secs: f32, c: &TtsConfig) -> (usize, usize) {
    let chunk = (c.base_chunk_size * c.chunk_compress_factor) as f64;
    let frames = (f64::from(duration_secs) * f64::from(c.sample_rate) / chunk).ceil() as usize;
    (c.latent_dim * c.chunk_compress_factor, frames)
}

impl Supertonic {
    pub fn load(dir: &Path, prefer_directml: bool) -> Result<Self, String> {
        crate::ensure_onnx_runtime_available().map_err(err)?;
        let cfg = parse_config(&read(dir, "tts.json")?)?;
        let indexer = Indexer::from_json(&read(dir, "unicode_indexer.json")?)?;
        // DirectML is Windows-only, so it is never asked for elsewhere.
        let pref = if prefer_directml && cfg!(windows) {
            Backend::DirectMl
        } else {
            Backend::Cpu
        };
        let (dp, b0) = open(&dir.join("duration_predictor.onnx"), pref)?;
        let (text_enc, b1) = open(&dir.join("text_encoder.onnx"), pref)?;
        let (vector_est, b2) = open(&dir.join("vector_estimator.onnx"), pref)?;
        let (vocoder, b3) = open(&dir.join("vocoder.onnx"), pref)?;
        log::info!("practice tts: Supertonic graphs on {:?}", [b0, b1, b2, b3]);
        Ok(Self {
            dp,
            text_enc,
            vector_est,
            vocoder,
            indexer,
            cfg,
        })
    }

    /// Predicted speaking time in seconds, after dividing by `speed`.
    #[cfg(test)]
    pub fn duration(&mut self, text: &str, voice: &Voice, speed: f32) -> Result<f32, String> {
        let ids = self.text_ids(text);
        self.predict_duration(&ids, voice, speed)
    }

    /// Mono audio at `TTS_RATE`, trimmed to the predicted duration.
    pub fn synthesize(
        &mut self,
        text: &str,
        voice: &Voice,
        steps: u32,
        speed: f32,
    ) -> Result<Vec<f32>, String> {
        if steps == 0 {
            return Err("steps must be at least 1".into());
        }
        let ids = self.text_ids(text);
        let t = ids.len();
        let duration = self.predict_duration(&ids, voice, speed)?;
        let (ldim, frames) = latent_shape(duration, &self.cfg);
        let wav_len = (f64::from(duration) * f64::from(self.cfg.sample_rate)) as usize;
        let chunk = self.cfg.base_chunk_size * self.cfg.chunk_compress_factor;
        // The reference zeroes the noise after the frames that hold the speech.
        let speech_frames = wav_len.div_ceil(chunk);
        let latent_mask: Vec<f32> = (0..frames)
            .map(|l| if l < speech_frames { 1.0 } else { 0.0 })
            .collect();
        let mut rng = rand::thread_rng();
        let mut x: Vec<f32> = standard_normal(ldim * frames, &mut rng)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v * latent_mask[i % frames])
            .collect();

        let enc = self
            .text_enc
            .run(ort::inputs![
                "text_ids" => Tensor::from_array(([1usize, t], ids.clone())).map_err(err)?,
                "style_ttl" => Tensor::from_array(voice.ttl.clone()).map_err(err)?,
                "text_mask" => Tensor::from_array(([1usize, 1, t], vec![1f32; t])).map_err(err)?,
            ])
            .map_err(err)?;
        let (emb_shape, emb) = enc[0].try_extract_tensor::<f32>().map_err(err)?;
        let (emb_shape, emb) = (emb_shape.to_vec(), emb.to_vec());

        for step in 0..steps {
            let outputs = self.vector_est.run(ort::inputs![
                "noisy_latent" => Tensor::from_array(([1usize, ldim, frames], std::mem::take(&mut x))).map_err(err)?,
                "text_emb" => Tensor::from_array((emb_shape.clone(), emb.clone())).map_err(err)?,
                "style_ttl" => Tensor::from_array(voice.ttl.clone()).map_err(err)?,
                "text_mask" => Tensor::from_array(([1usize, 1, t], vec![1f32; t])).map_err(err)?,
                "latent_mask" => Tensor::from_array(([1usize, 1, frames], latent_mask.clone())).map_err(err)?,
                "current_step" => Tensor::from_array(([1usize], vec![step as f32])).map_err(err)?,
                "total_step" => Tensor::from_array(([1usize], vec![steps as f32])).map_err(err)?,
            ]).map_err(err)?;
            let (_, next) = outputs[0].try_extract_tensor::<f32>().map_err(err)?;
            x = next.to_vec();
        }

        let outputs = self
            .vocoder
            .run(ort::inputs![
                "latent" => Tensor::from_array(([1usize, ldim, frames], x)).map_err(err)?,
            ])
            .map_err(err)?;
        let (_, wav) = outputs[0].try_extract_tensor::<f32>().map_err(err)?;
        wav.get(..wav_len)
            .map(<[f32]>::to_vec)
            .ok_or_else(|| format!("vocoder returned {} samples, expected {wav_len}", wav.len()))
    }

    /// Preprocessing and unicode indexing. Unsupported chars are dropped before the ids are built.
    fn text_ids(&self, text: &str) -> Vec<i64> {
        self.indexer
            .ids(&preprocess(&supported_only(text, &self.indexer), "en"))
    }

    fn predict_duration(&mut self, ids: &[i64], voice: &Voice, speed: f32) -> Result<f32, String> {
        check_speed(speed)?;
        let t = ids.len();
        let outputs = self
            .dp
            .run(ort::inputs![
                "text_ids" => Tensor::from_array(([1usize, t], ids.to_vec())).map_err(err)?,
                "style_dp" => Tensor::from_array(voice.dp.clone()).map_err(err)?,
                "text_mask" => Tensor::from_array(([1usize, 1, t], vec![1f32; t])).map_err(err)?,
            ])
            .map_err(err)?;
        let (_, d) = outputs[0].try_extract_tensor::<f32>().map_err(err)?;
        let d = match d {
            [d] => *d,
            _ => {
                return Err(format!(
                    "duration output has {} values, expected 1",
                    d.len()
                ))
            }
        };
        if !(d.is_finite() && d > 0.0) {
            return Err(format!("predicted duration {d} is not positive"));
        }
        Ok(d / speed)
    }
}

/// Box-Muller over `rand`: `n` independent standard normal values.
fn standard_normal(n: usize, rng: &mut impl Rng) -> Vec<f32> {
    let mut out = Vec::with_capacity(n + 1);
    while out.len() < n {
        let u1 = 1.0 - rng.gen::<f64>(); // in (0, 1], so the log is finite
        let u2 = rng.gen::<f64>();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        out.push((r * theta.cos()) as f32);
        out.push((r * theta.sin()) as f32);
    }
    out.truncate(n);
    out
}

fn check_speed(speed: f32) -> Result<(), String> {
    if (MIN_SPEED..=MAX_SPEED).contains(&speed) {
        Ok(())
    } else {
        Err(format!(
            "speed {speed} is outside {MIN_SPEED}..={MAX_SPEED}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::{TTS_SPEED, TTS_STEPS};
    use rand::{rngs::StdRng, SeedableRng};
    use std::path::PathBuf;

    const DURATIONS: &str = include_str!("../../tests/fixtures/practice/supertonic_durations.json");
    const GOOD_MORNING: &str = "Good morning, please have a seat.";

    fn cfg() -> TtsConfig {
        TtsConfig {
            sample_rate: 44_100,
            base_chunk_size: 512,
            chunk_compress_factor: 6,
            latent_dim: 24,
        }
    }

    fn voice_json(ttl_dims: &[usize], dp_dims: &[usize]) -> Vec<u8> {
        let n = |d: &[usize]| d.iter().product::<usize>();
        serde_json::to_vec(&serde_json::json!({
            "style_ttl": { "dims": ttl_dims, "data": vec![0.5f32; n(ttl_dims)] },
            "style_dp": { "dims": dp_dims, "data": vec![0.5f32; n(dp_dims)] },
        }))
        .unwrap()
    }

    #[test]
    fn flatten_reads_nested_data() {
        let mut out = Vec::new();
        flatten(&serde_json::json!([[1.0, 2.0], [3.0]]), &mut out).unwrap();
        assert_eq!(out, vec![1.0, 2.0, 3.0]);
        assert!(flatten(&serde_json::json!([1.0, "x"]), &mut out).is_err());
    }

    fn models_dir() -> PathBuf {
        PathBuf::from(std::env::var("PRACTICE_MODELS_DIR").expect("set PRACTICE_MODELS_DIR"))
    }

    #[test]
    fn latent_shape_math() {
        assert_eq!(latent_shape(3.0, &cfg()), (144, 44));
    }

    #[test]
    fn voice_json_dims_are_checked() {
        let parse = |ttl: &[usize], dp: &[usize]| parse_voice(&voice_json(ttl, dp)).map(|_| ());
        assert!(parse(&TTL_DIMS, &DP_DIMS).is_ok());
        assert!(parse(&[1, 50, 255], &DP_DIMS).is_err());
        assert!(parse(&TTL_DIMS, &[1, 8, 15]).is_err());
        assert!(parse(&[1, 50], &DP_DIMS).is_err());
    }

    #[test]
    fn standard_normal_is_standard() {
        let mut rng = StdRng::seed_from_u64(7);
        let xs = standard_normal(20_000, &mut rng);
        assert_eq!(xs.len(), 20_000);
        let mean = xs.iter().sum::<f32>() / xs.len() as f32;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / xs.len() as f32;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "variance {var}");
        assert!(xs.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn speed_is_checked() {
        assert!(check_speed(TTS_SPEED).is_ok());
        assert!(check_speed(0.7).is_ok());
        assert!(check_speed(0.5).is_err());
        assert!(check_speed(2.5).is_err());
        assert!(check_speed(f32::NAN).is_err());
    }

    #[test]
    fn config_rejects_unusable_values() {
        let json = |rate: u32, base: u32| {
            format!(
                r#"{{"ae":{{"sample_rate":{rate},"base_chunk_size":{base}}},"ttl":{{"chunk_compress_factor":6,"latent_dim":24}}}}"#
            )
        };
        assert_eq!(parse_config(json(44_100, 512).as_bytes()).unwrap(), cfg());
        assert!(parse_config(json(16_000, 512).as_bytes()).is_err());
        assert!(parse_config(json(44_100, 0).as_bytes()).is_err());
        assert!(parse_config(b"{}").is_err());
    }

    /// Needs ONNX Runtime and `PRACTICE_MODELS_DIR` (the flat Supertonic 3 folder).
    #[test]
    #[ignore]
    fn durations_match_reference() {
        let dir = models_dir();
        let mut m = Supertonic::load(&dir, false).unwrap();
        let cases: Vec<serde_json::Value> = serde_json::from_str(DURATIONS).unwrap();
        assert_eq!(cases.len(), 7);
        let mut voices = std::collections::HashMap::new();
        for case in cases {
            let text = case["text"].as_str().unwrap();
            let voice_name = case["voice"].as_str().unwrap();
            let speed = case["speed"].as_f64().unwrap() as f32;
            let want = case["duration"].as_f64().unwrap() as f32;
            let voice = voices
                .entry(voice_name.to_string())
                .or_insert_with(|| load_voice(&dir.join(format!("{voice_name}.json"))).unwrap());
            let got = m.duration(text, voice, speed).unwrap();
            assert!(
                (got - want).abs() < 1e-3,
                "{text:?}: rust {got}, python {want}"
            );
        }
    }

    /// Needs ONNX Runtime and `PRACTICE_MODELS_DIR` (the flat Supertonic 3 folder).
    #[test]
    #[ignore]
    fn synthesizes_finite_audio() {
        let dir = models_dir();
        let mut m = Supertonic::load(&dir, false).unwrap();
        let voice = load_voice(&dir.join("M1.json")).unwrap();
        let duration = m.duration(GOOD_MORNING, &voice, TTS_SPEED).unwrap();
        let samples = m
            .synthesize(GOOD_MORNING, &voice, TTS_STEPS, TTS_SPEED)
            .unwrap();
        let want = duration * TTS_RATE as f32;
        assert!(
            (samples.len() as f32 - want).abs() <= 0.02 * TTS_RATE as f32,
            "{} samples for {duration} s",
            samples.len()
        );
        assert!(samples.iter().all(|x| x.is_finite()));
        let peak = samples.iter().fold(0f32, |p, x| p.max(x.abs()));
        assert!(peak > 0.01, "peak {peak}");
    }
}
