// Speaker resolution (spec 8.2 steps 6-10) and line assignment (step 11). Pure logic: no I/O, no models.
// The anchor merge mirrors scripts/speaker-parity/make_reference.py, so merged.json can be checked against it.

use std::collections::BTreeMap;

use super::cluster::{cosine, mean_normalized};
use super::diarize::Turn;
use super::levels::mic_only_fraction;
use super::sentences::Sentence;

/// Lines are split between speakers at sentence boundaries. The DTW sentence times make that split accurate.
pub const SPLIT_LINES: bool = true;

const ANCHOR_SECS: f64 = 30.0;
const SMALL_SECS: f64 = 3.0;
const ANCHOR_MERGE: f64 = 0.7;
const ATTACH: f64 = 0.65;
const KNOWN_MATCH: f64 = 0.75;
const SELF_MIC_SHARE: f64 = 0.5;
const SELF_WINDOW_MIC_SHARE: f64 = 0.9;
const SUB_SECS: f64 = 3.0;
const SEG_MIN_SECS: f64 = 1.5;
const MAX_CENTROIDS: usize = 60;
const SPLIT_MIN_SHARE: f64 = 0.6;
const SPLIT_MIN_SECS: f64 = 1.0;

/// One diarization cluster: its speech time, its turns and the centroid the job embeds (None until then).
pub struct ClusterInfo {
    pub id: usize,
    pub seconds: f64,
    pub turns: Vec<(f64, f64)>,
    pub centroid: Option<Vec<f32>>,
}

/// Where the self speaker comes from: the level track, stored voiceprints, or nothing.
pub enum SelfEvidence {
    Levels(Vec<bool>),
    Prints(Vec<Vec<f32>>),
    None,
}

/// A saved person and their voiceprints.
pub struct KnownPerson {
    pub person_id: String,
    pub prints: Vec<Vec<f32>>,
}

/// A speaker after resolution. `idx` 0 is self; unnamed speakers take 1..n by talk time, named ones come after them.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSpeaker {
    pub idx: i64,
    pub person_id: Option<String>,
    pub centroid: Option<Vec<f32>>,
    pub seconds: f64,
    pub turns: Vec<(f64, f64)>,
}

/// One transcript line. `sentences` are its Whisper sentence times, if the webview sent valid ones.
pub struct Line {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub sentences: Option<Vec<Sentence>>,
}

/// A row to write back. `id: None` means a new row, for the pieces after the first of a split line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineOut {
    pub id: Option<String>,
    pub source_id: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub sentences: Option<Vec<Sentence>>,
    pub speaker_idx: Option<i64>,
}

/// Groups turns by cluster id. Centroids are left for the job to fill.
pub fn clusters_from_turns(turns: &[Turn]) -> Vec<ClusterInfo> {
    let mut by_id: BTreeMap<usize, ClusterInfo> = BTreeMap::new();
    for t in turns {
        let c = by_id.entry(t.cluster).or_insert_with(|| ClusterInfo {
            id: t.cluster,
            seconds: 0.0,
            turns: Vec::new(),
            centroid: None,
        });
        c.seconds += t.end - t.start;
        c.turns.push((t.start, t.end));
    }
    by_id.into_values().collect()
}

/// Sub-windows for the centroid embeddings. Same rules as `scripts/speaker-parity/make_reference.py::sub_windows`.
pub fn centroid_windows(turns: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut cands = Vec::new();
    for &(s, e) in turns {
        let d = e - s;
        if d < SEG_MIN_SECS {
            continue;
        }
        if d <= SUB_SECS {
            cands.push((s, e));
            continue;
        }
        let n_full = (d / SUB_SECS).floor() as usize;
        cands.extend((0..n_full).map(|k| (s + k as f64 * SUB_SECS, s + (k + 1) as f64 * SUB_SECS)));
        let rest = s + n_full as f64 * SUB_SECS;
        if e - rest >= SEG_MIN_SECS {
            cands.push((rest, e));
        }
    }
    let n = cands.len();
    if n <= MAX_CENTROIDS {
        return cands;
    }
    (0..MAX_CENTROIDS)
        .map(|i| cands[i * (n - 1) / (MAX_CENTROIDS - 1)])
        .collect()
}

/// Whole 3 s windows from each turn's start that are at least 90 % mic-only. The caller checks the total.
pub fn self_learning_windows(turns: &[(f64, f64)], flags: &[bool]) -> Vec<(f64, f64)> {
    turns
        .iter()
        .flat_map(|&(s, e)| {
            let n_full = ((e - s) / SUB_SECS).floor() as usize;
            (0..n_full).map(move |k| (s + k as f64 * SUB_SECS, s + (k + 1) as f64 * SUB_SECS))
        })
        .filter(|&(a, b)| {
            mic_only_fraction(flags, a, b).map_or(false, |f| f >= SELF_WINDOW_MIC_SHARE)
        })
        .collect()
}

/// A speaker in the making: a kept anchor, a small cluster, or several merged.
#[derive(Default)]
struct Unit {
    /// Original centroid of the first member. All matching uses it.
    centroid: Option<Vec<f32>>,
    /// Centroids of every member that has one. The output centroid is their mean.
    parts: Vec<Vec<f32>>,
    seconds: f64,
    turns: Vec<(f64, f64)>,
    anchor: bool,
    is_self: bool,
    person: Option<String>,
    dead: bool,
}

fn unit(c: ClusterInfo) -> Unit {
    Unit {
        parts: c.centroid.iter().cloned().collect(),
        anchor: c.seconds >= ANCHOR_SECS,
        seconds: c.seconds,
        turns: c.turns,
        centroid: c.centroid,
        ..Default::default()
    }
}

fn absorb_unit(u: &mut Unit, from: Unit) {
    u.seconds += from.seconds;
    u.turns.extend(from.turns);
    u.parts.extend(from.parts);
    u.anchor |= from.anchor;
}

/// Merges `units[from]` into `units[into]`. The matching centroid stays the one of `into`.
fn absorb(units: &mut [Unit], into: usize, from: usize) {
    let f = std::mem::take(&mut units[from]);
    absorb_unit(&mut units[into], f);
    units[from].dead = true;
}

/// Cosine of two centroids; None when either is missing, so such a pair never matches.
fn cos_of(a: &Option<Vec<f32>>, b: &Option<Vec<f32>>) -> Option<f64> {
    Some(cosine(a.as_ref()?, b.as_ref()?) as f64)
}

fn max_cos(c: &[f32], prints: &[Vec<f32>]) -> f64 {
    prints
        .iter()
        .map(|p| cosine(c, p) as f64)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Index and score of the best candidate, the first on a tie. Missing and NaN scores never win.
fn best_index(scores: impl Iterator<Item = Option<f64>>) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, s) in scores.enumerate() {
        if let Some(s) = s.filter(|s| !s.is_nan()) {
            if best.map_or(true, |(_, b)| s > b) {
                best = Some((i, s));
            }
        }
    }
    best
}

fn mic_seconds(u: &Unit, flags: &[bool]) -> f64 {
    u.turns
        .iter()
        .map(|&(s, e)| mic_only_fraction(flags, s, e).unwrap_or(0.0) * (e - s))
        .sum()
}

fn to_speaker(u: &Unit, idx: i64, person_id: Option<String>) -> ResolvedSpeaker {
    ResolvedSpeaker {
        idx,
        person_id,
        centroid: mean_normalized(&u.parts),
        seconds: u.seconds,
        turns: u.turns.clone(),
    }
}

/// Spec 8.2 steps 6-10. Speakers come back sorted by idx.
pub fn resolve_speakers(
    clusters: Vec<ClusterInfo>,
    self_ev: &SelfEvidence,
    people: &[KnownPerson],
) -> Vec<ResolvedSpeaker> {
    let has_anchor = clusters.iter().any(|c| c.seconds >= ANCHOR_SECS);
    let mut sorted = clusters;
    sorted.sort_by(|a, b| b.seconds.total_cmp(&a.seconds).then(a.id.cmp(&b.id)));
    let (anchors, smalls): (Vec<ClusterInfo>, Vec<ClusterInfo>) = sorted
        .into_iter()
        .filter(|c| c.seconds >= SMALL_SECS)
        .partition(|c| c.seconds >= ANCHOR_SECS);

    // Step 6: anchors merge biggest first into the most similar kept anchor, by its original centroid.
    let mut units: Vec<Unit> = Vec::new();
    for c in anchors.into_iter().map(unit) {
        match best_index(units.iter().map(|k| cos_of(&k.centroid, &c.centroid))) {
            Some((j, s)) if s >= ANCHOR_MERGE => absorb_unit(&mut units[j], c),
            _ => units.push(c),
        }
    }
    units.extend(smalls.into_iter().map(unit));

    // Step 8: self, by levels (most mic-only time, at least half of it) or by voiceprint.
    let self_ix = match self_ev {
        SelfEvidence::Levels(flags) => {
            best_index(units.iter().map(|u| Some(mic_seconds(u, flags))))
                .filter(|&(i, mic)| mic >= SELF_MIC_SHARE * units[i].seconds)
                .map(|(i, _)| i)
        }
        SelfEvidence::Prints(prints) => best_index(
            units
                .iter()
                .map(|u| u.centroid.as_deref().map(|c| max_cos(c, prints))),
        )
        .filter(|&(_, s)| s >= KNOWN_MATCH)
        .map(|(i, _)| i),
        SelfEvidence::None => None,
    };
    if let Some(i) = self_ix {
        units[i].is_self = true;
    }

    // Step 9: each unit takes its best known person; units that match the same person merge.
    let matched: Vec<Option<String>> = units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            if Some(i) == self_ix {
                return None;
            }
            best_index(
                people
                    .iter()
                    .map(|p| u.centroid.as_deref().map(|c| max_cos(c, &p.prints))),
            )
            .filter(|&(_, s)| s >= KNOWN_MATCH)
            .map(|(k, _)| people[k].person_id.clone())
        })
        .collect();
    for (i, pid) in matched.into_iter().enumerate() {
        let Some(pid) = pid else { continue };
        match (0..i).find(|&j| !units[j].dead && units[j].person.as_deref() == Some(pid.as_str())) {
            Some(j) => absorb(&mut units, j, i),
            None => units[i].person = Some(pid),
        }
    }

    // Step 4: what is still unmatched and under 30 s attaches to the nearest anchor at >= 0.65.
    for i in 0..units.len() {
        if units[i].dead || units[i].anchor || units[i].is_self || units[i].person.is_some() {
            continue;
        }
        let target = best_index(units.iter().map(|t| {
            if t.dead || !t.anchor {
                None
            } else {
                cos_of(&units[i].centroid, &t.centroid)
            }
        }))
        .filter(|&(_, s)| s >= ATTACH);
        if let Some((j, _)) = target {
            absorb(&mut units, j, i);
        }
    }

    // Unmatched small units survive only when the meeting has no anchor at all. Otherwise they are Unknown.
    let kept: Vec<usize> = (0..units.len())
        .filter(|&i| {
            let u = &units[i];
            !u.dead && (u.anchor || u.is_self || u.person.is_some() || !has_anchor)
        })
        .collect();
    let by_talk = |mut v: Vec<usize>| {
        v.sort_by(|&a, &b| units[b].seconds.total_cmp(&units[a].seconds));
        v
    };
    let unnamed = by_talk(
        kept.iter()
            .copied()
            .filter(|&i| !units[i].is_self && units[i].person.is_none())
            .collect(),
    );
    let named = by_talk(
        kept.iter()
            .copied()
            .filter(|&i| !units[i].is_self && units[i].person.is_some())
            .collect(),
    );

    // Step 10: self is 0, unnamed speakers are 1..n by talk time, and named ones follow them.
    let mut out: Vec<ResolvedSpeaker> = kept
        .iter()
        .filter(|&&i| units[i].is_self)
        .map(|&i| to_speaker(&units[i], 0, Some("self".into())))
        .collect();
    out.extend(
        unnamed
            .iter()
            .enumerate()
            .map(|(k, &i)| to_speaker(&units[i], k as i64 + 1, None)),
    );
    let n = unnamed.len() as i64;
    out.extend(
        named
            .iter()
            .enumerate()
            .map(|(k, &i)| to_speaker(&units[i], n + k as i64 + 1, units[i].person.clone())),
    );
    out
}

/// Assigns each line (and its sentences) to a speaker, splitting lines where `SPLIT_LINES` allows.
pub fn assign_lines(lines: &[Line], speakers: &[ResolvedSpeaker]) -> Vec<LineOut> {
    assign_lines_with(lines, speakers, SPLIT_LINES)
}

fn overlap(turns: &[(f64, f64)], start: f64, end: f64) -> f64 {
    turns
        .iter()
        .map(|&(s, e)| (e.min(end) - s.max(start)).max(0.0))
        .sum()
}

/// The speaker with the most overlap with `[start, end)`, and that overlap. None when nobody overlaps.
fn best_speaker(speakers: &[ResolvedSpeaker], start: f64, end: f64) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (k, sp) in speakers.iter().enumerate() {
        let ov = overlap(&sp.turns, start, end);
        if ov > 0.0 && best.map_or(true, |(_, b)| ov > b) {
            best = Some((k, ov));
        }
    }
    best
}

fn assign_lines_with(lines: &[Line], speakers: &[ResolvedSpeaker], split: bool) -> Vec<LineOut> {
    let mut out = Vec::new();
    for line in lines {
        let idx_of = |k: Option<usize>| k.map(|k| speakers[k].idx);
        // The webview's sentence times may sit up to 0.2 s outside the line, so clamp them first.
        let sents: Vec<Sentence> = line
            .sentences
            .iter()
            .flatten()
            .map(|Sentence(t0, t1, text)| {
                Sentence(
                    t0.max(line.start).min(line.end),
                    t1.max(line.start).min(line.end),
                    text.clone(),
                )
            })
            .collect();
        let majority = best_speaker(speakers, line.start, line.end).map(|(k, _)| k);
        if sents.is_empty() {
            out.push(LineOut {
                id: Some(line.id.clone()),
                source_id: line.id.clone(),
                start: line.start,
                end: line.end,
                text: line.text.clone(),
                sentences: None,
                speaker_idx: idx_of(majority),
            });
            continue;
        }
        // A sentence takes its own speaker only with >= 60 % and >= 1 s of it. Otherwise it inherits the majority.
        let finals: Vec<Option<usize>> = sents
            .iter()
            .map(|Sentence(t0, t1, _)| {
                let strong = split
                    .then(|| best_speaker(speakers, *t0, *t1))
                    .flatten()
                    .filter(|&(_, ov)| ov >= SPLIT_MIN_SHARE * (t1 - t0) && ov >= SPLIT_MIN_SECS)
                    .map(|(k, _)| k);
                strong.or(majority)
            })
            .collect();
        let mut starts = vec![0];
        starts.extend((1..sents.len()).filter(|&i| finals[i] != finals[i - 1]));
        if starts.len() == 1 {
            out.push(LineOut {
                id: Some(line.id.clone()),
                source_id: line.id.clone(),
                start: line.start,
                end: line.end,
                text: line.text.clone(),
                sentences: Some(sents),
                speaker_idx: idx_of(finals[0]),
            });
            continue;
        }
        for (p, &a) in starts.iter().enumerate() {
            let b = starts.get(p + 1).copied().unwrap_or(sents.len());
            let piece = &sents[a..b];
            out.push(LineOut {
                id: (p == 0).then(|| line.id.clone()),
                source_id: line.id.clone(),
                start: piece[0].0,
                end: piece[piece.len() - 1].1,
                text: piece
                    .iter()
                    .map(|Sentence(_, _, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                sentences: Some(piece.to_vec()),
                speaker_idx: idx_of(finals[a]),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIM: usize = 192;

    /// A 192-d vector with the given components, scaled to unit length.
    fn v(parts: &[(usize, f32)]) -> Vec<f32> {
        let mut out = vec![0.0f32; DIM];
        for &(i, x) in parts {
            out[i] = x;
        }
        let norm = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        out.iter().map(|x| x / norm).collect()
    }

    /// A unit vector with cosine `c` to the axis e0.
    fn with_cos(c: f32) -> Vec<f32> {
        v(&[(0, c), (1, (1.0 - c * c).sqrt())])
    }

    fn e(i: usize) -> Vec<f32> {
        v(&[(i, 1.0)])
    }

    fn cl(id: usize, secs: f64, centroid: Option<Vec<f32>>) -> ClusterInfo {
        // One turn per cluster, placed far apart so that flags never overlap two clusters.
        let start = id as f64 * 1000.0;
        ClusterInfo {
            id,
            seconds: secs,
            turns: vec![(start, start + secs)],
            centroid,
        }
    }

    fn line(id: &str, start: f64, end: f64, text: &str, sentences: Option<Vec<Sentence>>) -> Line {
        Line {
            id: id.into(),
            start,
            end,
            text: text.into(),
            sentences,
        }
    }

    fn sent(t0: f64, t1: f64, text: &str) -> Sentence {
        Sentence(t0, t1, text.into())
    }

    /// Flags for `slices` 50 ms slices, true where `mic(k)`.
    fn flags(slices: usize, mic: impl Fn(usize) -> bool) -> Vec<bool> {
        (0..slices).map(mic).collect()
    }

    fn names(out: &[ResolvedSpeaker]) -> Vec<(i64, Option<String>, i64)> {
        out.iter()
            .map(|s| (s.idx, s.person_id.clone(), s.seconds.round() as i64))
            .collect()
    }

    #[test]
    fn clusters_from_turns_groups_by_id() {
        let turns = [
            Turn {
                start: 0.0,
                end: 2.0,
                cluster: 3,
            },
            Turn {
                start: 5.0,
                end: 6.5,
                cluster: 1,
            },
            Turn {
                start: 7.0,
                end: 9.0,
                cluster: 3,
            },
        ];
        let got = clusters_from_turns(&turns);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].id, got[0].seconds), (1, 1.5));
        assert_eq!(got[0].turns, vec![(5.0, 6.5)]);
        assert_eq!((got[1].id, got[1].seconds), (3, 4.0));
        assert_eq!(got[1].turns, vec![(0.0, 2.0), (7.0, 9.0)]);
        assert!(got.iter().all(|c| c.centroid.is_none()));
    }

    #[test]
    fn centroid_windows_rules() {
        // under 1.5 s skipped; 1.5-3 s whole; 4.5 s -> 3 s window + own 1.5 s remainder; 4.2 s -> remainder 1.2 s dropped
        assert_eq!(centroid_windows(&[(0.0, 1.0)]), vec![]);
        assert_eq!(centroid_windows(&[(10.0, 12.0)]), vec![(10.0, 12.0)]);
        assert_eq!(
            centroid_windows(&[(0.0, 4.5)]),
            vec![(0.0, 3.0), (3.0, 4.5)]
        );
        assert_eq!(centroid_windows(&[(0.0, 4.2)]), vec![(0.0, 3.0)]);
    }

    #[test]
    fn centroid_windows_cap_60_evenly() {
        let spans: Vec<(f64, f64)> = (0..100)
            .map(|i| (2.0 * i as f64, 2.0 * i as f64 + 2.0))
            .collect();
        let got = centroid_windows(&spans);
        assert_eq!(got.len(), 60);
        let want: Vec<(f64, f64)> = (0..60).map(|i| spans[(i * 99) / 59]).collect();
        assert_eq!(got, want);
        assert!(got.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn self_learning_windows_need_90_percent() {
        // turn 0-6 s = 120 slices. First window: 54 of 60 mic-only (90 %). Second: 53 of 60 (88 %).
        let f = flags(120, |k| k < 54 || (60..113).contains(&k));
        assert_eq!(self_learning_windows(&[(0.0, 6.0)], &f), vec![(0.0, 3.0)]);
    }

    #[test]
    fn merge_two_anchors_of_same_voice() {
        let clusters = vec![cl(0, 60.0, Some(e(0))), cl(1, 60.0, Some(with_cos(0.8)))];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(names(&out), vec![(1, None, 120)]);
    }

    #[test]
    fn anchors_below_merge_threshold_stay_apart() {
        let clusters = vec![cl(0, 60.0, Some(e(0))), cl(1, 60.0, Some(with_cos(0.5)))];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn anchor_merge_matches_parity_kit() {
        // make_reference.py self_check: merge({0: 40, 1: 35, 2: 40, 3: 5, 4: 1}) -> [75, 40], unknown [5, 1].
        let clusters = vec![
            cl(0, 40.0, Some(e(0))),
            cl(1, 35.0, Some(v(&[(0, 1.0), (2, 0.5)]))),
            cl(2, 40.0, Some(e(1))),
            cl(3, 5.0, Some(v(&[(2, 1.0), (1, 0.2)]))),
            cl(4, 1.0, Some(e(2))),
        ];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(names(&out), vec![(1, None, 75), (2, None, 40)]);
    }

    #[test]
    fn anchor_merge_compares_original_centroids() {
        // make_reference.py self_check: C is 0.64 from A but 0.80 from the A+B mean, so it stays apart.
        let b = v(&[(0, 0.9), (1, (1.0f32 - 0.81).sqrt())]);
        let c = v(&[(0, 50f32.to_radians().cos()), (1, 50f32.to_radians().sin())]);
        let clusters = vec![
            cl(0, 60.0, Some(e(0))),
            cl(1, 50.0, Some(b)),
            cl(2, 40.0, Some(c)),
        ];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(names(&out), vec![(1, None, 110), (2, None, 40)]);
    }

    #[test]
    fn self_by_levels_needs_half_mic_only() {
        let turns_40 = |mic_slices: usize| {
            let f = flags(800, |k| k < mic_slices); // 40 s = 800 slices
            let clusters = vec![cl(0, 40.0, Some(e(0)))];
            resolve_speakers(clusters, &SelfEvidence::Levels(f), &[])
        };
        // 40 % mic-only: no self, the cluster is Speaker 1.
        assert_eq!(names(&turns_40(320)), vec![(1, None, 40)]);
        // 60 % mic-only: self.
        assert_eq!(names(&turns_40(480)), vec![(0, Some("self".into()), 40)]);
    }

    #[test]
    fn self_by_prints_when_no_levels() {
        let prints = vec![e(5)];
        let matching = vec![cl(0, 40.0, Some(v(&[(5, 0.8), (0, 0.6)])))];
        let out = resolve_speakers(matching, &SelfEvidence::Prints(prints.clone()), &[]);
        assert_eq!(names(&out), vec![(0, Some("self".into()), 40)]);
        let weak = vec![cl(0, 40.0, Some(v(&[(5, 0.7), (0, 0.714_142_8)])))];
        let out = resolve_speakers(weak, &SelfEvidence::Prints(prints), &[]);
        assert_eq!(names(&out), vec![(1, None, 40)]);
    }

    #[test]
    fn user_silent_gives_no_self() {
        let clusters = vec![cl(0, 40.0, Some(e(0))), cl(1, 40.0, Some(e(1)))];
        let silent = flags(1600, |_| false);
        let out = resolve_speakers(clusters, &SelfEvidence::Levels(silent), &[]);
        assert!(out.iter().all(|s| s.idx != 0));
    }

    #[test]
    fn known_person_matched_and_same_person_clusters_merge() {
        let person = KnownPerson {
            person_id: "p1".into(),
            prints: vec![e(5)],
        };
        // A and B both match the print at 0.8, but are 0.64 apart, so the anchor merge keeps them separate.
        let a = v(&[(5, 0.8), (0, 0.6)]);
        let b = v(&[(5, 0.8), (1, 0.6)]);
        let clusters = vec![
            cl(0, 60.0, Some(e(2))),
            cl(1, 60.0, Some(a)),
            cl(2, 60.0, Some(b)),
        ];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[person]);
        // The unnamed cluster takes idx 1; the named person comes after the unnamed speakers.
        assert_eq!(
            names(&out),
            vec![(1, None, 60), (2, Some("p1".into()), 120)]
        );
    }

    #[test]
    fn small_cluster_attaches_to_anchor_at_065() {
        let anchor = || cl(0, 60.0, Some(e(0)));
        let attached = resolve_speakers(
            vec![anchor(), cl(1, 10.0, Some(with_cos(0.651)))],
            &SelfEvidence::None,
            &[],
        );
        assert_eq!(names(&attached), vec![(1, None, 70)]);
        let apart = resolve_speakers(
            vec![anchor(), cl(1, 10.0, Some(with_cos(0.649)))],
            &SelfEvidence::None,
            &[],
        );
        assert_eq!(names(&apart), vec![(1, None, 60)]);
    }

    #[test]
    fn small_cluster_without_match_is_unknown_when_anchors_exist() {
        let clusters = vec![cl(0, 60.0, Some(e(0))), cl(1, 10.0, Some(e(1)))];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(names(&out), vec![(1, None, 60)]);
    }

    #[test]
    fn short_meeting_without_anchors_keeps_small_speakers() {
        let clusters = vec![cl(0, 10.0, Some(e(0))), cl(1, 10.0, Some(e(1)))];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(names(&out), vec![(1, None, 10), (2, None, 10)]);
    }

    #[test]
    fn tiny_cluster_under_3s_is_unknown() {
        // Even a perfect voice match does not attach a cluster under 3 s.
        let clusters = vec![cl(0, 60.0, Some(e(0))), cl(1, 2.0, Some(e(0)))];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(names(&out), vec![(1, None, 60)]);
        assert!(
            resolve_speakers(vec![cl(0, 2.0, Some(e(0)))], &SelfEvidence::None, &[]).is_empty()
        );
    }

    #[test]
    fn numbering_by_talk_time_desc() {
        let clusters = vec![
            cl(0, 40.0, Some(e(0))),
            cl(1, 90.0, Some(e(1))),
            cl(2, 50.0, Some(e(2))),
        ];
        let out = resolve_speakers(clusters, &SelfEvidence::None, &[]);
        assert_eq!(
            names(&out),
            vec![(1, None, 90), (2, None, 50), (3, None, 40)]
        );
    }

    /// Speakers A (idx 1, talks 0-2 and 3.2-6) and B (idx 2, talks 2-3.2).
    fn two_speakers() -> Vec<ResolvedSpeaker> {
        vec![
            ResolvedSpeaker {
                idx: 1,
                person_id: None,
                centroid: None,
                seconds: 4.8,
                turns: vec![(0.0, 2.0), (3.2, 6.0)],
            },
            ResolvedSpeaker {
                idx: 2,
                person_id: None,
                centroid: None,
                seconds: 1.2,
                turns: vec![(2.0, 3.2)],
            },
        ]
    }

    #[test]
    fn line_split_at_sentence_with_other_speaker() {
        let l = line(
            "L1",
            0.0,
            6.0,
            "Do you agree? Yes. Then go.",
            Some(vec![
                sent(0.0, 2.0, "Do you agree?"),
                sent(2.0, 3.2, "Yes."),
                sent(3.2, 6.0, "Then go."),
            ]),
        );
        let got = assign_lines_with(&[l], &two_speakers(), true);
        assert_eq!(
            got,
            vec![
                LineOut {
                    id: Some("L1".into()),
                    source_id: "L1".into(),
                    start: 0.0,
                    end: 2.0,
                    text: "Do you agree?".into(),
                    sentences: Some(vec![sent(0.0, 2.0, "Do you agree?")]),
                    speaker_idx: Some(1),
                },
                LineOut {
                    id: None,
                    source_id: "L1".into(),
                    start: 2.0,
                    end: 3.2,
                    text: "Yes.".into(),
                    sentences: Some(vec![sent(2.0, 3.2, "Yes.")]),
                    speaker_idx: Some(2),
                },
                LineOut {
                    id: None,
                    source_id: "L1".into(),
                    start: 3.2,
                    end: 6.0,
                    text: "Then go.".into(),
                    sentences: Some(vec![sent(3.2, 6.0, "Then go.")]),
                    speaker_idx: Some(1),
                },
            ]
        );
    }

    #[test]
    fn short_reply_under_1s_does_not_split() {
        // "Yes." overlaps B for only 0.8 s, so the whole line stays with A.
        let speakers = vec![
            ResolvedSpeaker {
                idx: 1,
                person_id: None,
                centroid: None,
                seconds: 5.2,
                turns: vec![(0.0, 2.0), (2.8, 6.0)],
            },
            ResolvedSpeaker {
                idx: 2,
                person_id: None,
                centroid: None,
                seconds: 0.8,
                turns: vec![(2.0, 2.8)],
            },
        ];
        let sents = vec![
            sent(0.0, 2.0, "Do you agree?"),
            sent(2.0, 2.8, "Yes."),
            sent(2.8, 6.0, "Then go."),
        ];
        let l = line(
            "L1",
            0.0,
            6.0,
            "Do you agree? Yes. Then go.",
            Some(sents.clone()),
        );
        let got = assign_lines_with(&[l], &speakers, true);
        assert_eq!(
            got,
            vec![LineOut {
                id: Some("L1".into()),
                source_id: "L1".into(),
                start: 0.0,
                end: 6.0,
                text: "Do you agree? Yes. Then go.".into(),
                sentences: Some(sents),
                speaker_idx: Some(1),
            }]
        );
    }

    #[test]
    fn line_without_sentences_gets_majority() {
        let l = line("L1", 0.0, 6.0, "Do you agree?", None);
        let got = assign_lines(&[l], &two_speakers());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].speaker_idx, Some(1));
        assert_eq!(got[0].sentences, None);
    }

    #[test]
    fn line_with_no_overlap_is_unknown() {
        let l = line(
            "L1",
            100.0,
            105.0,
            "Hello.",
            Some(vec![sent(100.0, 105.0, "Hello.")]),
        );
        let got = assign_lines(&[l], &two_speakers());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].speaker_idx, None);
    }
}
