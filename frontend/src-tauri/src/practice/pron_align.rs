//! CTC forced alignment (Viterbi) and goodness-of-pronunciation word scores over HuBERT phoneme log-probabilities.
//! Pure: no I/O, no model. Frames are 20 ms; `logp` is row-major `[frames, VOCAB]`.
#![allow(dead_code)] // remove in Task 4, once the runner calls score_words

use super::PRON_VARIANT_MARGIN_FRAMES;

pub const BLANK: usize = 40;
pub const VOCAB: usize = 43;

/// Expected phones are ids 0..=38; 39 is [UNK] and is excluded from GOP's max.
const PHONES: usize = 39;

#[derive(Debug, Clone)]
pub struct Aligned {
    /// Inclusive frame span per expected phone.
    pub phone_spans: Vec<(usize, usize)>,
    /// Total path log-probability.
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct WordScore {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    pub score: f32,
    pub worst_phone: u8,
}

pub fn log_softmax(logits: &[f32], frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * VOCAB);
    for row in logits.chunks_exact(VOCAB).take(frames) {
        let max = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let lse = max + row.iter().map(|x| (x - max).exp()).sum::<f32>().ln();
        out.extend(row.iter().map(|x| x - lse));
    }
    out
}

/// Path state `s`: even = blank, odd `2k+1` = expected phone `k`.
pub fn viterbi(logp: &[f32], frames: usize, phones: &[u8]) -> Option<Aligned> {
    let n = phones.len();
    if n == 0 || frames < 2 * n + 1 || logp.len() < frames * VOCAB {
        return None;
    }
    let states = 2 * n + 1;
    let emit = |s: usize| {
        if s % 2 == 0 {
            BLANK
        } else {
            phones[s / 2] as usize
        }
    };
    // back[t * states + s]: 0 = stayed, 1 = came from s-1, 2 = came from s-2.
    let mut back = vec![0u8; frames * states];
    let mut prev = vec![f32::NEG_INFINITY; states];
    prev[0] = logp[BLANK];
    prev[1] = logp[emit(1)];
    for t in 1..frames {
        let row = &logp[t * VOCAB..(t + 1) * VOCAB];
        let mut cur = vec![f32::NEG_INFINITY; states];
        for s in 0..states {
            let (mut best, mut from) = (prev[s], 0u8);
            if s >= 1 && prev[s - 1] > best {
                (best, from) = (prev[s - 1], 1);
            }
            // Skipping the blank between two different phones.
            if s >= 3 && s % 2 == 1 && phones[s / 2] != phones[s / 2 - 1] && prev[s - 2] > best {
                (best, from) = (prev[s - 2], 2);
            }
            cur[s] = best + row[emit(s)];
            back[t * states + s] = from;
        }
        prev = cur;
    }
    let last = if prev[states - 2] > prev[states - 1] {
        states - 2
    } else {
        states - 1
    };
    let score = prev[last];
    let mut path = vec![0usize; frames];
    let mut s = last;
    for t in (0..frames).rev() {
        path[t] = s;
        if t > 0 {
            s -= back[t * states + s] as usize;
        }
    }
    let mut phone_spans = vec![(0, 0); n];
    for (t, &s) in path.iter().enumerate() {
        if s % 2 == 1 {
            let k = s / 2;
            if t == 0 || path[t - 1] != s {
                phone_spans[k].0 = t;
            }
            phone_spans[k].1 = t;
        }
    }
    Some(Aligned { phone_spans, score })
}

/// Per-word, per-phone spans of one forced alignment of `words` (each one pronunciation).
fn align(logp: &[f32], frames: usize, words: &[Vec<u8>]) -> Option<Vec<Vec<(usize, usize)>>> {
    let phones: Vec<u8> = words.concat();
    let a = viterbi(logp, frames, &phones)?;
    let mut spans = a.phone_spans.into_iter();
    Some(
        words
            .iter()
            .map(|w| spans.by_ref().take(w.len()).collect())
            .collect(),
    )
}

/// Mean over frames `start..=end` of `logp[p] - max over the expected phones`.
fn gop(logp: &[f32], start: usize, end: usize, phone: u8) -> f32 {
    let p = phone as usize;
    let sum: f32 = (start..=end)
        .map(|t| {
            let row = &logp[t * VOCAB..(t + 1) * VOCAB];
            let best = row[..PHONES]
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max);
            row[p] - best
        })
        .sum();
    sum / (end - start + 1) as f32
}

/// `words[i]` lists word i's pronunciation variants, each a phone-id sequence.
pub fn score_words(logp: &[f32], frames: usize, words: &[Vec<Vec<u8>>]) -> Option<Vec<WordScore>> {
    let bad_phone = |p: &u8| *p as usize >= PHONES;
    if words.is_empty()
        || words
            .iter()
            .any(|v| v.is_empty() || v.iter().any(|p| p.is_empty() || p.iter().any(bad_phone)))
    {
        return None;
    }
    let mut chosen: Vec<Vec<u8>> = words.iter().map(|v| v[0].clone()).collect();
    let first = align(logp, frames, &chosen)?;
    for (i, variants) in words.iter().enumerate().filter(|(_, v)| v.len() > 1) {
        let (start, end) = (first[i][0].0, first[i].last()?.1);
        // The window must fit the longest variant, or a shorter wrong variant wins uncompared.
        let need = 2 * variants.iter().map(|v| v.len()).max().unwrap_or(0) + 1;
        if need > frames {
            continue;
        }
        let (mut lo, mut hi) = (
            start.saturating_sub(PRON_VARIANT_MARGIN_FRAMES),
            (end + PRON_VARIANT_MARGIN_FRAMES).min(frames - 1),
        );
        while hi + 1 - lo < need {
            if lo > 0 {
                lo -= 1;
            } else {
                hi += 1;
            }
        }
        let window = &logp[lo * VOCAB..(hi + 1) * VOCAB];
        let mut best: Option<(f32, usize)> = None;
        for (vi, v) in variants.iter().enumerate() {
            if let Some(a) = viterbi(window, hi + 1 - lo, v) {
                if best.map_or(true, |(b, _)| a.score > b) {
                    best = Some((a.score, vi));
                }
            }
        }
        if let Some((_, vi)) = best {
            chosen[i] = variants[vi].clone();
        }
    }
    let spans = align(logp, frames, &chosen)?;
    Some(
        chosen
            .iter()
            .zip(&spans)
            .enumerate()
            .map(|(index, (phones, phone_spans))| {
                let gops: Vec<f32> = phones
                    .iter()
                    .zip(phone_spans)
                    .map(|(&p, &(s, e))| gop(logp, s, e, p))
                    .collect();
                let (k, min) =
                    gops.iter()
                        .copied()
                        .enumerate()
                        .fold(
                            (0, f32::INFINITY),
                            |acc, (k, g)| if g < acc.1 { (k, g) } else { acc },
                        );
                WordScore {
                    index,
                    start: phone_spans[0].0,
                    end: phone_spans[phone_spans.len() - 1].1,
                    // A NaN or infinite GOP (bad model output) reports 0.0, so the word is never flagged.
                    score: if gops.iter().all(|g| g.is_finite()) {
                        min
                    } else {
                        0.0
                    },
                    worst_phone: phones[k],
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Log-probs with `labels[t]` at 0 and every other id at -10.
    fn peaks(labels: &[usize]) -> Vec<f32> {
        labels
            .iter()
            .flat_map(|&l| (0..VOCAB).map(move |q| if q == l { 0.0 } else { -10.0 }))
            .collect()
    }

    #[test]
    fn viterbi_recovers_known_spans() {
        // blank, p1 p1, blank, p2 p2 p2, blank, p3 p3, blank blank
        let labels = [BLANK, 1, 1, BLANK, 2, 2, 2, BLANK, 3, 3, BLANK, BLANK];
        let a = viterbi(&peaks(&labels), labels.len(), &[1, 2, 3]).unwrap();
        assert_eq!(a.phone_spans, vec![(1, 2), (4, 6), (8, 9)]);
        assert!(a.score.abs() < 1e-5);
    }

    #[test]
    fn viterbi_handles_repeated_phones() {
        // Two identical phones need a blank frame between them.
        let labels = [BLANK, 5, BLANK, 5, BLANK];
        let a = viterbi(&peaks(&labels), labels.len(), &[5, 5]).unwrap();
        assert_eq!(a.phone_spans, vec![(1, 1), (3, 3)]);
    }

    #[test]
    fn viterbi_skips_the_blank_between_different_phones() {
        // Different neighbours may go straight from one phone to the next, so no blank frame is needed.
        let labels = [1, 2, 3, 3, 3, 3, 3];
        let a = viterbi(&peaks(&labels), labels.len(), &[1, 2, 3]).unwrap();
        assert_eq!(a.phone_spans, vec![(0, 0), (1, 1), (2, 6)]);
        assert!(a.score.abs() < 1e-5);
    }

    #[test]
    fn too_few_frames_returns_none() {
        // 3 phones need 2*3+1 = 7 frames.
        assert!(viterbi(&peaks(&[BLANK; 6]), 6, &[1, 2, 3]).is_none());
        assert!(viterbi(&peaks(&[BLANK; 6]), 6, &[]).is_none());
    }

    #[test]
    fn gop_is_zero_when_expected_phone_wins_and_drops_when_swapped() {
        let words = vec![vec![vec![1u8, 2]]];
        let right = [BLANK, 1, 1, BLANK, 2, 2, BLANK];
        let ok = score_words(&peaks(&right), right.len(), &words).unwrap();
        assert!(ok[0].score.abs() < 1e-5);
        assert_eq!((ok[0].start, ok[0].end), (1, 5));

        // The model hears phone 3 where phone 1 was expected.
        let swapped = [BLANK, 3, 3, BLANK, 2, 2, BLANK];
        let bad = score_words(&peaks(&swapped), swapped.len(), &words).unwrap();
        assert!(bad[0].score < -5.0, "score {}", bad[0].score);
        assert_eq!(bad[0].worst_phone, 1);
    }

    #[test]
    fn variant_pick_prefers_the_matching_pronunciation() {
        // Variant 1 is [1, 2] and variant 2 is [1, 3]. The audio says 3.
        let words = vec![vec![vec![1u8, 2], vec![1, 3]]];
        let labels = [BLANK, 1, 1, BLANK, 3, 3, BLANK];
        let s = score_words(&peaks(&labels), labels.len(), &words).unwrap();
        assert!(s[0].score.abs() < 1e-5, "score {}", s[0].score);
    }

    #[test]
    fn a_short_window_does_not_let_a_wrong_variant_win() {
        // Variant 0 needs 17 frames, and the segment is exactly 17 frames long, one frame per phone.
        // The 3-phone wrong variant fits the usual ±4 window and would win uncompared, so the window must grow.
        let words = vec![vec![(1..=8).collect::<Vec<u8>>(), vec![1, 2, 30]]];
        let mut labels = vec![BLANK];
        labels.extend(1..=8usize);
        labels.extend(vec![BLANK; 8]);
        let s = score_words(&peaks(&labels), labels.len(), &words).unwrap();
        assert!(s[0].score.abs() < 1e-5, "score {}", s[0].score);
        assert_eq!((s[0].start, s[0].end), (1, 8));
    }

    #[test]
    fn a_non_finite_gop_reports_zero_so_the_word_is_not_flagged() {
        // The only phone's column is NaN in every frame, so its GOP is NaN wherever it is aligned.
        let words = vec![vec![vec![1u8]]];
        let mut logp = peaks(&[BLANK, BLANK, BLANK]);
        for t in 0..3 {
            logp[t * VOCAB + 1] = f32::NAN;
        }
        let s = score_words(&logp, 3, &words).unwrap();
        assert_eq!(s[0].score, 0.0);
    }

    #[test]
    fn log_softmax_rows_sum_to_one() {
        let logits: Vec<f32> = (0..2 * VOCAB).map(|i| (i % 7) as f32 - 3.0).collect();
        let out = log_softmax(&logits, 2);
        assert_eq!(out.len(), 2 * VOCAB);
        for row in out.chunks(VOCAB) {
            let sum: f32 = row.iter().map(|x| x.exp()).sum();
            assert!((sum - 1.0).abs() < 1e-4);
        }
    }
}
