//! Local pronunciation hints. The HuBERT phoneme model gives per-frame phone log-probabilities for one speech
//! segment; the transcript's words are aligned to them with CMUdict, and GOP scores flag words that *may* be off.
//! These are hints, not grades.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use super::pron_align::{log_softmax, score_words, VOCAB, WILDCARD};
use super::pron_dict::{words, PronDict, PHONES};
use super::{
    PRON_FRAME_SECS, PRON_MAX_CHUNK_SECS, PRON_MIN_CHUNK_SECS, PRON_MIN_PHONES, PRON_PAD_SECS,
    PRON_THRESHOLD, YOU_WAV_RATE,
};
use crate::speakers::runtime::{open, Backend};

const MODEL_FILE: &str = "hubert_phoneme_int8.onnx";
const DICT_FILE: &str = "cmudict.dict";

#[derive(Debug, Clone, PartialEq)]
pub struct SegmentHint {
    /// As written in the transcript.
    pub word: String,
    /// Relative to the start of the segment.
    pub start_secs: f64,
    pub end_secs: f64,
    pub score: f32,
    /// The phone with the worst GOP in the word, e.g. "TH".
    pub phone: String,
}

/// One aligned word before the hint filter. An `oov` word only holds its place in the alignment.
struct Scored {
    hint: SegmentHint,
    phones: usize,
    oov: bool,
}

pub struct PronEngine {
    session: Session,
    dict: PronDict,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl PronEngine {
    /// Opens the phoneme model on the CPU and reads the dictionary from `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        crate::ensure_onnx_runtime_available().map_err(err)?;
        let (session, _) = open(&dir.join(MODEL_FILE), Backend::Cpu)?;
        let text = std::fs::read_to_string(dir.join(DICT_FILE)).map_err(err)?;
        Ok(Self {
            session,
            dict: PronDict::parse(&text),
        })
    }

    /// Hints for one segment. Every word is aligned in order, and out-of-vocabulary words are wildcards:
    /// they hold their place in the alignment but are never hinted.
    pub fn segment_hints(
        &mut self,
        samples16k: &[f32],
        text: &str,
    ) -> Result<Vec<SegmentHint>, String> {
        Ok(self
            .scored_words(samples16k, text)?
            .into_iter()
            .filter(|s| !s.oov && s.hint.score < PRON_THRESHOLD && s.phones >= PRON_MIN_PHONES)
            .map(|s| s.hint)
            .collect())
    }

    /// Every word of the segment with its span and GOP, before the hint filter. Empty when the segment is
    /// skipped, has fewer than two dictionary words, or is too short for its phones.
    fn scored_words(&mut self, samples16k: &[f32], text: &str) -> Result<Vec<Scored>, String> {
        let secs = samples16k.len() as f64 / f64::from(YOU_WAV_RATE);
        if skip_segment(samples16k.len(), text) {
            log::info!("pronunciation: no hints for a {secs:.1} s segment");
            return Ok(vec![]);
        }
        let tokens = words(text);
        if tokens
            .iter()
            .filter(|w| self.dict.variants(w).is_some())
            .count()
            < 2
        {
            return Ok(vec![]);
        }
        let (logp, frames) = self.posteriors(samples16k)?;
        let Some(scores) = score_words(&logp, frames, &word_variants(&tokens, &self.dict)) else {
            return Ok(vec![]);
        };
        Ok(scores
            .into_iter()
            .map(|s| {
                let (start_secs, end_secs) = hint_times(s.start, s.end, secs);
                Scored {
                    oov: s.worst_phone == WILDCARD,
                    phones: s.phones,
                    hint: SegmentHint {
                        word: tokens[s.index].clone(),
                        start_secs,
                        end_secs,
                        score: s.score,
                        phone: PHONES
                            .get(s.worst_phone as usize)
                            .copied()
                            .unwrap_or("")
                            .to_string(),
                    },
                }
            })
            .collect())
    }

    /// Phone log-probabilities, row-major `[frames, VOCAB]`, and the frame count.
    fn posteriors(&mut self, samples: &[f32]) -> Result<(Vec<f32>, usize), String> {
        let input =
            Tensor::from_array(([1usize, samples.len()], normalise(samples))).map_err(err)?;
        let outputs = self
            .session
            .run(ort::inputs!["input_values" => input])
            .map_err(err)?;
        let (shape, data) = outputs["logits"].try_extract_tensor::<f32>().map_err(err)?;
        let dims: &[i64] = shape;
        let frames = match *dims {
            [1, frames, c] if c == VOCAB as i64 => usize::try_from(frames).map_err(err)?,
            _ => {
                return Err(format!(
                    "logits have shape {dims:?}, expected [1, frames, {VOCAB}]"
                ))
            }
        };
        Ok((log_softmax(data, frames), frames))
    }
}

/// Segments outside the model's useful length range, or with no text to align, get no hints.
fn skip_segment(samples: usize, text: &str) -> bool {
    let secs = samples as f64 / f64::from(YOU_WAV_RATE);
    !(PRON_MIN_CHUNK_SECS..=PRON_MAX_CHUNK_SECS).contains(&secs) || text.trim().is_empty()
}

/// One entry per word: its CMUdict variants, or a single wildcard phone when it is out of vocabulary.
fn word_variants(words: &[String], dict: &PronDict) -> Vec<Vec<Vec<u8>>> {
    words
        .iter()
        .map(|w| match dict.variants(w) {
            Some(v) => v.to_vec(),
            None => vec![vec![WILDCARD]],
        })
        .collect()
}

/// Zero mean and unit variance over the clip. The epsilon matches the model's feature extractor.
fn normalise(samples: &[f32]) -> Vec<f32> {
    let n = samples.len() as f64;
    let mean = samples.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let var = samples
        .iter()
        .map(|&x| (f64::from(x) - mean).powi(2))
        .sum::<f64>()
        / n;
    let scale = (var + 1e-7).sqrt();
    samples
        .iter()
        .map(|&x| ((f64::from(x) - mean) / scale) as f32)
        .collect()
}

/// Frames `start..=end` as seconds, padded by `PRON_PAD_SECS` and clamped to `[0, segment_secs]`.
fn hint_times(start: usize, end: usize, segment_secs: f64) -> (f64, f64) {
    let from = (start as f64 * PRON_FRAME_SECS - PRON_PAD_SECS).clamp(0.0, segment_secs);
    let to = ((end + 1) as f64 * PRON_FRAME_SECS + PRON_PAD_SECS).clamp(0.0, segment_secs);
    (from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::audio_processing::resample_audio;
    use crate::practice::tts::{load_voice, Supertonic};
    use crate::practice::{TTS_RATE, TTS_SPEED, TTS_STEPS};

    const DICT: &str = "ALPHA  AH L F AH
BETA  B EY T AH
GAMMA  G AE M AH
";

    #[test]
    fn oov_words_become_wildcards() {
        let dict = PronDict::parse(DICT);
        let got = word_variants(&words("Alpha 2026 beta"), &dict);
        assert_eq!(
            got,
            vec![
                dict.variants("alpha").unwrap().to_vec(),
                vec![vec![WILDCARD]],
                dict.variants("BETA").unwrap().to_vec(),
            ]
        );
    }

    #[test]
    fn long_segment_is_skipped() {
        let rate = YOU_WAV_RATE as usize;
        assert!(skip_segment(20 * rate + 1, "good morning"));
        assert!(!skip_segment(20 * rate, "good morning"));
        assert!(skip_segment(2 * rate, "   "));
        assert!(!skip_segment(2 * rate, "good morning"));
    }

    #[test]
    fn short_segment_is_skipped() {
        let rate = YOU_WAV_RATE as usize;
        assert!(skip_segment(0, "good morning"));
        assert!(skip_segment(rate * 3 / 10 - 1, "good morning"));
        assert!(!skip_segment(rate * 3 / 10, "good morning"));
    }

    #[test]
    fn hint_spans_are_padded_and_clamped_to_the_segment() {
        // Frames 10..=19 are 0.20..0.40 s, padded by 0.08 s on each side.
        let (from, to) = hint_times(10, 19, 1.0);
        assert!(
            (from - 0.12).abs() < 1e-9 && (to - 0.48).abs() < 1e-9,
            "{from} {to}"
        );
        // Clamped to [0, segment length].
        let (from, to) = hint_times(0, 2, 0.1);
        assert!(from == 0.0 && (to - 0.1).abs() < 1e-9, "{from} {to}");
    }

    fn pron_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("PRACTICE_PRON_DIR").expect("set PRACTICE_PRON_DIR"))
    }

    fn synthesize(text: &str) -> Vec<f32> {
        let models = std::path::PathBuf::from(
            std::env::var("PRACTICE_MODELS_DIR").expect("set PRACTICE_MODELS_DIR"),
        );
        let mut tts = Supertonic::load(&models, false).unwrap();
        let voice = load_voice(&models.join("M1.json")).unwrap();
        let audio = tts.synthesize(text, &voice, TTS_STEPS, TTS_SPEED).unwrap();
        resample_audio(&audio, TTS_RATE, YOU_WAV_RATE)
    }

    /// Needs ONNX Runtime and `PRACTICE_PRON_DIR` (hubert_phoneme_int8.onnx and cmudict.dict).
    #[test]
    #[ignore]
    fn real_model_outputs_43_logits_per_20ms() {
        let mut engine = PronEngine::load(&pron_dir()).unwrap();
        let (logp, frames) = engine.posteriors(&vec![0.0; 2 * 16_000]).unwrap();
        assert!((98..=102).contains(&frames), "{frames} frames for 2 s");
        assert_eq!(logp.len(), frames * VOCAB);
    }

    /// Needs ONNX Runtime, `PRACTICE_PRON_DIR` and `PRACTICE_MODELS_DIR` (the flat Supertonic 3 folder).
    #[test]
    #[ignore]
    fn native_tts_sentence_has_no_hints() {
        const SENTENCE: &str = "Good morning, please have a seat.";
        let samples = synthesize(SENTENCE);
        let mut engine = PronEngine::load(&pron_dir()).unwrap();
        let hints = engine.segment_hints(&samples, SENTENCE).unwrap();
        assert!(hints.is_empty(), "{hints:?}");
    }

    /// The name is out of vocabulary, so it is a wildcard. Every word keeps a short span and no word is hinted.
    /// Needs ONNX Runtime, `PRACTICE_PRON_DIR` and `PRACTICE_MODELS_DIR`.
    #[test]
    #[ignore]
    fn oov_sentence_has_no_hints_and_short_spans() {
        const SENTENCE: &str = "We met the client in Weberstrasse on Monday and agreed to finish before Friday afternoon.";
        let samples = synthesize(SENTENCE);
        let mut engine = PronEngine::load(&pron_dir()).unwrap();
        let scored = engine.scored_words(&samples, SENTENCE).unwrap();
        assert_eq!(scored.len(), 15, "every word is aligned");
        assert_eq!(scored.iter().filter(|s| s.oov).count(), 1);
        for s in &scored {
            let len = s.hint.end_secs - s.hint.start_secs;
            assert!(len <= 1.0, "{} spans {len:.2} s", s.hint.word);
        }
        let hints = engine.segment_hints(&samples, SENTENCE).unwrap();
        assert!(hints.is_empty(), "{hints:?}");
    }
}
