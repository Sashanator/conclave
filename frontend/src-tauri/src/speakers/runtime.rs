// ONNX Runtime sessions for the two speaker models: pyannote segmentation and TitaNet embeddings.
// DirectML on Windows when asked, CPU otherwise. The backend that really ran is recorded, never assumed.

use std::path::{Path, PathBuf};

use ndarray::Array2;
use ort::execution_providers::CPUExecutionProvider;
#[cfg(windows)]
use ort::execution_providers::DirectMLExecutionProvider;
use ort::session::Session;
use ort::value::Tensor;

use super::features::{nemo_features, read_nemo_meta, NemoMeta};

const SEG_FILE: &str = "segmentation-3.0.onnx";
const EMB_FILE: &str = "titanet-large.onnx";
const EMB_DIM: usize = 192;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Backend {
    DirectMl,
    Cpu,
}

/// Set to false if profiling shows the CPU is faster for TitaNet (Task 24).
pub const TITANET_ON_DIRECTML: bool = true;

#[derive(Clone, Debug, PartialEq)]
pub struct SegMeta {
    pub sample_rate: usize,
    pub window_size: usize,
    pub receptive_field_size: usize,
    pub receptive_field_shift: usize,
    pub num_speakers: usize,
    pub powerset_max_classes: usize,
    pub num_classes: usize,
}

pub trait SpeakerModels {
    fn seg_meta(&self) -> &SegMeta;
    /// `chunk` is one window of `window_size` samples. Returns `[frames, num_classes]` logits.
    fn segment(&mut self, chunk: &[f32]) -> Result<Array2<f32>, String>;
    /// `None` means the clip gave no usable embedding (no frames, NaN, or zero norm).
    fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, String>;
    /// "directml" | "cpu" | "mixed".
    fn backend_label(&self) -> &'static str;
    /// Rebuilds every DirectML session on the CPU after a run failed on DirectML.
    fn fall_back_to_cpu(&mut self) -> Result<(), String>;
}

pub struct OrtSpeakerModels {
    seg_path: PathBuf,
    emb_path: PathBuf,
    seg: Session,
    emb: Session,
    seg_backend: Backend,
    emb_backend: Backend,
    seg_meta: SegMeta,
    nemo: NemoMeta,
    /// A DirectML session was replaced by the CPU after it had been in use.
    dml_failed_over: bool,
}

/// Intra-op threads for the CPU session: half the logical cores, at least one.
pub fn cpu_threads(logical: usize) -> usize {
    (logical / 2).max(1)
}

/// The label for the backends in use. A failover after DirectML work is always `mixed`.
pub fn label(seg: Backend, emb: Backend, any_directml_work_failed_over: bool) -> &'static str {
    match (seg, emb) {
        _ if any_directml_work_failed_over => "mixed",
        (Backend::DirectMl, Backend::DirectMl) => "directml",
        (Backend::Cpu, Backend::Cpu) => "cpu",
        _ => "mixed",
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn session(path: &Path, backend: Backend) -> Result<Session, String> {
    let builder = Session::builder().map_err(err)?;
    let builder = match backend {
        #[cfg(windows)]
        Backend::DirectMl => builder
            // error_on_failure: a DirectML that fails to register must error, not silently run on the CPU.
            .with_execution_providers([DirectMLExecutionProvider::default()
                .build()
                .error_on_failure()])
            .map_err(err)?
            // DirectML requires sequential execution and no memory pattern.
            .with_parallel_execution(false)
            .map_err(err)?
            .with_memory_pattern(false)
            .map_err(err)?,
        #[cfg(not(windows))]
        Backend::DirectMl => return Err("DirectML is only available on Windows".into()),
        Backend::Cpu => {
            let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
            builder
                .with_execution_providers([CPUExecutionProvider::default().build()])
                .map_err(err)?
                .with_intra_threads(cpu_threads(logical))
                .map_err(err)?
                .with_intra_op_spinning(false)
                .map_err(err)?
                .with_inter_op_spinning(false)
                .map_err(err)?
        }
    };
    builder
        .commit_from_file(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Opens on `preferred`. A DirectML session that fails to start is replaced by a CPU one, and the
/// backend that actually opened is returned.
fn open(path: &Path, preferred: Backend) -> Result<(Session, Backend), String> {
    if preferred == Backend::DirectMl {
        match session(path, Backend::DirectMl) {
            Ok(s) => return Ok((s, Backend::DirectMl)),
            Err(e) => log::warn!("speaker model on DirectML failed, using CPU: {e}"),
        }
    }
    Ok((session(path, Backend::Cpu)?, Backend::Cpu))
}

/// sherpa's pyannote segmentation metadata (offline-speaker-segmentation-pyannote-model.cc).
fn read_seg_meta(session: &Session) -> Result<SegMeta, String> {
    let meta = session.metadata().map_err(err)?;
    let get = |key: &str| -> Result<usize, String> {
        meta.custom(key)
            .map_err(err)?
            .ok_or_else(|| format!("'{key}' does not exist in the metadata"))?
            .parse()
            .map_err(|_| format!("invalid '{key}' in the metadata"))
    };
    let out = SegMeta {
        sample_rate: get("sample_rate")?,
        window_size: get("window_size")?,
        receptive_field_size: get("receptive_field_size")?,
        receptive_field_shift: get("receptive_field_shift")?,
        num_speakers: get("num_speakers")?,
        powerset_max_classes: get("powerset_max_classes")?,
        num_classes: get("num_classes")?,
    };
    check_seg_meta(&out)?;
    Ok(out)
}

/// Rejects segmentation metadata the diarizer cannot use. The 60 s cap bounds the window buffer.
fn check_seg_meta(m: &SegMeta) -> Result<(), String> {
    if m.sample_rate != 16_000 {
        return Err(format!(
            "segmentation sample_rate {} is not 16000",
            m.sample_rate
        ));
    }
    if [
        m.window_size,
        m.num_classes,
        m.receptive_field_size,
        m.receptive_field_shift,
        m.num_speakers,
    ]
    .contains(&0)
    {
        return Err("segmentation metadata has a zero size".into());
    }
    if m.window_size > 60 * m.sample_rate {
        return Err(format!(
            "segmentation window of {} samples is longer than 60 s",
            m.window_size
        ));
    }
    Ok(())
}

/// Checks the input and output counts that the calls rely on: segmentation needs one input and one
/// output, and TitaNet needs features and lengths in, and logits and an embedding out. Tuples are (inputs, outputs).
fn check_model_io(seg: (usize, usize), emb: (usize, usize)) -> Result<(), String> {
    if seg.0 < 1 || seg.1 < 1 {
        return Err("segmentation model needs one input and one output".into());
    }
    if emb.0 < 2 || emb.1 < 2 {
        return Err("TitaNet model needs two inputs and two outputs".into());
    }
    Ok(())
}

impl OrtSpeakerModels {
    pub fn load(dir: &Path, prefer_directml: bool) -> Result<Self, String> {
        crate::ensure_onnx_runtime_available().map_err(err)?;
        let seg_path = dir.join(SEG_FILE);
        let emb_path = dir.join(EMB_FILE);
        // DirectML is Windows-only; elsewhere it is never attempted.
        let dml = prefer_directml && cfg!(windows);
        let seg_pref = if dml { Backend::DirectMl } else { Backend::Cpu };
        let emb_pref = if dml && TITANET_ON_DIRECTML {
            Backend::DirectMl
        } else {
            Backend::Cpu
        };
        let (seg, seg_backend) = open(&seg_path, seg_pref)?;
        let (emb, emb_backend) = open(&emb_path, emb_pref)?;
        check_model_io(
            (seg.inputs.len(), seg.outputs.len()),
            (emb.inputs.len(), emb.outputs.len()),
        )?;
        Ok(Self {
            seg_meta: read_seg_meta(&seg)?,
            nemo: read_nemo_meta(&emb)?,
            seg_path,
            emb_path,
            seg,
            emb,
            seg_backend,
            emb_backend,
            dml_failed_over: false,
        })
    }
}

impl SpeakerModels for OrtSpeakerModels {
    fn seg_meta(&self) -> &SegMeta {
        &self.seg_meta
    }

    fn segment(&mut self, chunk: &[f32]) -> Result<Array2<f32>, String> {
        let window = self.seg_meta.window_size;
        if chunk.len() != window {
            return Err(format!(
                "segment expects {window} samples, got {}",
                chunk.len()
            ));
        }
        let name = self.seg.inputs[0].name.to_string();
        let input = Tensor::from_array(([1usize, 1, window], chunk.to_vec())).map_err(err)?;
        let outputs = self
            .seg
            .run(ort::inputs![name.as_str() => input])
            .map_err(err)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(err)?;
        let classes = self.seg_meta.num_classes;
        let dims: &[i64] = shape;
        let frames = match *dims {
            [1, frames, c] if c == classes as i64 => usize::try_from(frames).map_err(err)?,
            _ => {
                return Err(format!(
                    "segmentation output has shape {dims:?}, expected [1, frames, {classes}]"
                ))
            }
        };
        Array2::from_shape_vec((frames, classes), data.to_vec()).map_err(err)
    }

    fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, String> {
        let feats = nemo_features(samples, &self.nemo);
        let (dim, frames) = feats.dim();
        if frames == 0 {
            return Ok(None);
        }
        // Unpadded: sherpa pads only its own buffer, so the model sees exactly `frames`
        // (speaker-embedding-extractor-nemo-impl.h:92-100).
        let x = Tensor::from_array((
            [1usize, dim, frames],
            feats.iter().copied().collect::<Vec<f32>>(),
        ))
        .map_err(err)?;
        let x_lens = Tensor::from_array(([1usize], vec![frames as i64])).map_err(err)?;
        let names = [
            self.emb.inputs[0].name.to_string(),
            self.emb.inputs[1].name.to_string(),
        ];
        let outputs = self
            .emb
            .run(ort::inputs![names[0].as_str() => x, names[1].as_str() => x_lens])
            .map_err(err)?;
        // Output 0 is the classification logits and output 1 the embedding (sherpa nemo-model.cc:57-63).
        let (_, emb) = outputs[1].try_extract_tensor::<f32>().map_err(err)?;
        if emb.len() != EMB_DIM {
            return Err(format!(
                "embedding has {} values, expected {EMB_DIM}",
                emb.len()
            ));
        }
        let norm = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        // NaN and infinity make the norm non-finite, so one check covers them.
        if !norm.is_finite() || norm == 0.0 {
            return Ok(None);
        }
        Ok(Some(emb.to_vec()))
    }

    fn backend_label(&self) -> &'static str {
        label(self.seg_backend, self.emb_backend, self.dml_failed_over)
    }

    fn fall_back_to_cpu(&mut self) -> Result<(), String> {
        if self.seg_backend == Backend::DirectMl {
            self.seg = session(&self.seg_path, Backend::Cpu)?;
            self.seg_backend = Backend::Cpu;
            self.dml_failed_over = true;
        }
        if self.emb_backend == Backend::DirectMl {
            self.emb = session(&self.emb_path, Backend::Cpu)?;
            self.emb_backend = Backend::Cpu;
            self.dml_failed_over = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_session_threads_are_half_the_cores() {
        assert_eq!(cpu_threads(8), 4);
        assert_eq!(cpu_threads(1), 1);
        assert_eq!(cpu_threads(0), 1);
    }

    fn seg_meta() -> SegMeta {
        SegMeta {
            sample_rate: 16_000,
            window_size: 160_000,
            receptive_field_size: 991,
            receptive_field_shift: 270,
            num_speakers: 3,
            powerset_max_classes: 2,
            num_classes: 7,
        }
    }

    #[test]
    fn seg_meta_rejects_zero_sizes_and_long_windows() {
        assert!(check_seg_meta(&seg_meta()).is_ok());
        let zeroed: [fn(&mut SegMeta); 5] = [
            |m| m.receptive_field_size = 0,
            |m| m.receptive_field_shift = 0,
            |m| m.num_speakers = 0,
            |m| m.window_size = 0,
            |m| m.num_classes = 0,
        ];
        for set in zeroed {
            let mut m = seg_meta();
            set(&mut m);
            assert!(check_seg_meta(&m).is_err(), "{m:?}");
        }
        let mut m = seg_meta();
        m.window_size = 60 * 16_000;
        assert!(check_seg_meta(&m).is_ok());
        m.window_size = 60 * 16_000 + 1;
        assert!(check_seg_meta(&m).is_err());
        let mut m = seg_meta();
        m.sample_rate = 8_000;
        assert!(check_seg_meta(&m).is_err());
    }

    #[test]
    fn io_counts_must_cover_the_calls() {
        assert!(check_model_io((1, 1), (2, 2)).is_ok());
        assert!(check_model_io((1, 3), (2, 2)).is_ok());
        assert!(check_model_io((0, 1), (2, 2)).is_err());
        assert!(check_model_io((1, 0), (2, 2)).is_err());
        // TitaNet without the length input, or without the embedding output, would index out of range.
        assert!(check_model_io((1, 1), (1, 2)).is_err());
        assert!(check_model_io((1, 1), (2, 1)).is_err());
    }

    #[test]
    fn backend_label_combines_backends() {
        use Backend::{Cpu, DirectMl};
        assert_eq!(label(DirectMl, DirectMl, false), "directml");
        assert_eq!(label(Cpu, Cpu, false), "cpu");
        assert_eq!(label(DirectMl, Cpu, false), "mixed");
        assert_eq!(label(Cpu, DirectMl, false), "mixed");
        assert_eq!(label(Cpu, Cpu, true), "mixed");
        assert_eq!(label(DirectMl, DirectMl, true), "mixed");
    }

    /// Needs ONNX Runtime and `SPEAKER_MODELS_DIR` (a folder with both models).
    #[test]
    #[ignore]
    #[cfg(not(windows))]
    fn load_on_linux_uses_cpu() {
        let dir = std::env::var("SPEAKER_MODELS_DIR").expect("set SPEAKER_MODELS_DIR");
        let models = OrtSpeakerModels::load(Path::new(&dir), true).unwrap();
        assert_eq!(models.backend_label(), "cpu");
    }

    fn load_from_env() -> OrtSpeakerModels {
        let dir = std::env::var("SPEAKER_MODELS_DIR").expect("set SPEAKER_MODELS_DIR");
        OrtSpeakerModels::load(Path::new(&dir), true).unwrap()
    }

    /// Needs ONNX Runtime and `SPEAKER_MODELS_DIR`.
    #[test]
    #[ignore]
    fn segment_shape_matches_meta() {
        let mut m = load_from_env();
        let window = m.seg_meta().window_size;
        assert_eq!(window, 160_000, "pyannote 3.0 windows are 10 s");
        let out = m.segment(&vec![0.0; window]).unwrap();
        assert_eq!(out.ncols(), m.seg_meta().num_classes);
        assert!(out.nrows() > 0);
    }

    fn read_f32(path: &Path) -> Vec<f32> {
        std::fs::read(path)
            .unwrap()
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b))
    }

    /// Rust embeddings against sherpa's, for the 20 clips of each parity recording (Task 7).
    /// Needs `SPEAKER_MODELS_DIR` and `SPEAKER_PARITY_DIR` (the folder with the recordings).
    #[test]
    #[ignore]
    fn embedding_parity_with_sherpa() {
        let parity = std::env::var("SPEAKER_PARITY_DIR").expect("set SPEAKER_PARITY_DIR");
        let mut m = load_from_env();
        let mut overall = 1f32;
        for rec in ["1-2-1_Ira", "1-2-1_Slava", "german-lesson", "meetup"] {
            let dir = Path::new(&parity).join(rec);
            let audio = read_f32(&dir.join("audio16k.f32"));
            let clips: Vec<[f64; 2]> =
                serde_json::from_slice(&std::fs::read(dir.join("clips.json")).unwrap()).unwrap();
            let reference = read_f32(&dir.join("clip_emb.f32"));
            assert_eq!(clips.len(), 20);
            assert_eq!(reference.len(), 20 * 192);
            let mut min = 1f32;
            for (i, [start, end]) in clips.iter().enumerate() {
                let from = (start * 16_000.0).round() as usize;
                let to = (end * 16_000.0).round() as usize;
                let emb = m
                    .embed(&audio[from..to])
                    .unwrap()
                    .expect("a valid embedding");
                let c = cosine(&emb, &reference[i * 192..(i + 1) * 192]);
                min = min.min(c);
                assert!(c > 0.999, "{rec} clip {i}: cosine {c}");
            }
            eprintln!("{rec}: min cosine {min}");
            overall = overall.min(min);
        }
        eprintln!("min cosine over 80 clips: {overall}");
    }
}
