// Pyannote diarization: a port of sherpa-onnx v1.13.8 offline-speaker-diarization-pyannote-impl.h
// (Process and its helpers) on top of the SpeakerModels backend. Silhouettes are skipped because
// sherpa's compute_confidence is off. Float arithmetic keeps sherpa's f32 order so that frame and
// sample indexes match sherpa's.

use std::collections::HashMap;

use ndarray::{s, Array2};

use super::cluster::complete_linkage_labels;
use super::runtime::{SegMeta, SpeakerModels};

/// Cut distance for complete linkage on `1 - cosine` (spec §8.2). sherpa's default 0.5 is overridden.
const CLUSTER_THRESHOLD: f32 = 0.7;
/// sherpa OfflineSpeakerDiarizationConfig min_duration_on, offline-speaker-diarization.h:25.
const MIN_DURATION_ON: f32 = 0.3;
/// sherpa OfflineSpeakerDiarizationConfig min_duration_off, offline-speaker-diarization.h:30.
const MIN_DURATION_OFF: f32 = 0.5;
/// A speaker takes part in a window only with at least this many non-overlapping frames
/// (sherpa GetChunkSpeakerSampleIndexes, offline-speaker-diarization-pyannote-impl.h:455).
const MIN_SPEAKER_FRAMES: i32 = 10;

/// Source of the audio. It is read in windows, so a long meeting is never held in memory at once.
pub trait AudioSource {
    fn len(&self) -> usize;
    /// Fills `out` with the samples that start at `start`.
    fn read(&self, start: usize, out: &mut [f32]) -> Result<(), String>;
}

impl AudioSource for Vec<f32> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn read(&self, start: usize, out: &mut [f32]) -> Result<(), String> {
        let src = start
            .checked_add(out.len())
            .and_then(|end| self.get(start..end))
            .ok_or("read past the end of the audio")?;
        out.copy_from_slice(src);
        Ok(())
    }
}

/// One speaker turn in seconds. `cluster` is the speaker index.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start: f64,
    pub end: f64,
    pub cluster: usize,
}

/// Runs the whole pipeline on `audio`. `step_secs` is the hop between 10 s windows.
/// `progress` gets fractions in 0..=1: segmentation up to 0.25, embeddings up to 0.9, then the rest.
pub fn diarize(
    audio: &dyn AudioSource,
    models: &mut dyn SpeakerModels,
    step_secs: f64,
    progress: &mut dyn FnMut(f32),
) -> Result<Vec<Turn>, String> {
    let meta = models.seg_meta().clone();
    let g = Geometry::new(&meta, step_secs)?;
    let mapping = powerset_mapping(meta.num_classes, meta.num_speakers, meta.powerset_max_classes)?;
    let n = audio.len();
    if n == 0 {
        progress(1.0);
        return Ok(vec![]);
    }
    let labels = segment_all(audio, models, &g, &mapping, progress)?;
    let mut turns = if labels.len() == 1 {
        handle_one_chunk(&labels[0], n, &g)
    } else {
        multi_chunk_turns(audio, models, &labels, &g, progress)?
    };
    // The last window is padded with silence, so a speaker can be placed past the end of the audio.
    let len = n as f64 / g.sample_rate as f64;
    turns.retain(|t| t.start < len);
    for t in &mut turns {
        t.end = t.end.min(len);
    }
    turns.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.cluster.cmp(&b.cluster)));
    progress(1.0);
    Ok(turns)
}

/// Segmentation windows of `g.window` samples, `g.shift` apart. The last one is zero-padded
/// (sherpa RunSpeakerSegmentationModel, offline-speaker-diarization-pyannote-impl.h:297-356).
/// Returns one `[frames, speakers]` 0/1 label matrix per window (ToMultiLabel, :379-391).
fn segment_all(
    audio: &dyn AudioSource,
    models: &mut dyn SpeakerModels,
    g: &Geometry,
    mapping: &Array2<u8>,
    progress: &mut dyn FnMut(f32),
) -> Result<Vec<Array2<u8>>, String> {
    let n = audio.len();
    let total = num_chunks(n, g);
    let mut labels: Vec<Array2<u8>> = Vec::with_capacity(total);
    let mut buf = vec![0f32; g.window];
    for i in 0..total {
        let start = i * g.shift;
        let avail = n.saturating_sub(start).min(g.window);
        buf.fill(0.0);
        audio.read(start, &mut buf[..avail])?;
        let logits = models.segment(&buf)?;
        if logits.ncols() != mapping.nrows() {
            return Err(format!(
                "segmentation output has {} classes, expected {}",
                logits.ncols(),
                mapping.nrows()
            ));
        }
        if labels.first().is_some_and(|l| l.nrows() != logits.nrows()) {
            return Err("segmentation windows have different frame counts".into());
        }
        labels.push(to_multi_label(&logits, mapping));
        progress(0.25 * (i + 1) as f32 / total as f32);
    }
    Ok(labels)
}

/// Number of segmentation windows (sherpa RunSpeakerSegmentationModel, :333-353).
fn num_chunks(n: usize, g: &Geometry) -> usize {
    if n <= g.window {
        1
    } else {
        (n - g.window) / g.shift + 1 + has_last_chunk(n, g) as usize
    }
}

/// Whether a padded window is needed after the full ones (sherpa, :334).
fn has_last_chunk(n: usize, g: &Geometry) -> bool {
    n > g.window && (n - g.window) % g.shift > 0
}

/// Frames in the reconstruction (sherpa ComputeSpeakersPerFrame, :395-425).
fn frames_total(num_chunks: usize, g: &Geometry) -> usize {
    (g.window + (num_chunks - 1) * g.shift) / g.rf_shift + 1
}

/// The first frame of window `chunk`. sherpa: `static_cast<float>(i) * window_shift / receptive_field_shift + 0.5`
/// (:414-415), truncated. The `+ 0.5` is double.
fn chunk_frame_start(chunk: usize, g: &Geometry) -> usize {
    ((chunk as f32 * g.shift as f32 / g.rf_shift as f32) as f64 + 0.5) as usize
}

/// Sample index of a frame inside a window, with the window's sample offset. Sherpa computes it in f32
/// (GetChunkSpeakerSampleIndexes, :476-477) and truncates.
fn sample_at(frame: usize, frames: usize, g: &Geometry, offset: usize) -> i32 {
    (frame as f32 / frames as f32 * g.window as f32 + offset as f32) as i32
}

/// Geometry of the segmentation model and of the reconstruction. Mirrors the sherpa metadata.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    sample_rate: usize,
    window: usize,
    shift: usize,
    rf_size: usize,
    rf_shift: usize,
}

impl Geometry {
    /// `step_secs` is the hop between windows. sherpa clamps the window shift to [1, window]
    /// (offline-speaker-segmentation-pyannote-model.cc:96-112).
    fn new(meta: &SegMeta, step_secs: f64) -> Result<Self, String> {
        let zero = [
            meta.sample_rate,
            meta.window_size,
            meta.receptive_field_size,
            meta.receptive_field_shift,
            meta.num_speakers,
            meta.num_classes,
        ];
        if zero.contains(&0) {
            return Err("segmentation metadata has a zero size".into());
        }
        let raw = step_secs * meta.sample_rate as f64;
        let shift = if raw.is_nan() || raw < 1.0 {
            1
        } else {
            (raw as usize).min(meta.window_size)
        };
        Ok(Self {
            sample_rate: meta.sample_rate,
            window: meta.window_size,
            shift,
            rf_size: meta.receptive_field_size,
            rf_shift: meta.receptive_field_shift,
        })
    }
}

/// Powerset class k is the set of speakers active in it (sherpa InitPowersetMapping,
/// offline-speaker-diarization-pyannote-impl.h:262-295). Only 1 or 2 simultaneous speakers are supported.
fn powerset_mapping(
    num_classes: usize,
    speakers: usize,
    max_classes: usize,
) -> Result<Array2<u8>, String> {
    if !(1..=2).contains(&max_classes) {
        return Err(format!("powerset_max_classes {max_classes} is not supported"));
    }
    let mut sets: Vec<Vec<usize>> = vec![vec![]];
    sets.extend((0..speakers).map(|j| vec![j]));
    if max_classes == 2 {
        for j in 0..speakers {
            for m in j + 1..speakers {
                sets.push(vec![j, m]);
            }
        }
    }
    if sets.len() != num_classes {
        return Err(format!(
            "segmentation has {num_classes} classes, expected {}",
            sets.len()
        ));
    }
    let mut map = Array2::zeros((num_classes, speakers));
    for (k, set) in sets.iter().enumerate() {
        for &s in set {
            map[[k, s]] = 1;
        }
    }
    Ok(map)
}

/// The most likely powerset class per frame, mapped to speakers (sherpa ToMultiLabel, :379-391).
/// Ties go to the lower class, as Eigen's maxCoeff does.
fn to_multi_label(logits: &Array2<f32>, mapping: &Array2<u8>) -> Array2<u8> {
    let mut out = Array2::zeros((logits.nrows(), mapping.ncols()));
    for (f, row) in logits.rows().into_iter().enumerate() {
        let mut best = 0;
        for j in 1..row.len() {
            if row[j] > row[best] {
                best = j;
            }
        }
        out.row_mut(f).assign(&mapping.row(best));
    }
    out
}

/// The number of speakers in each frame, rounded (sherpa ComputeSpeakersPerFrame, :395-425).
fn speakers_per_frame(labels: &[Array2<u8>], g: &Geometry) -> Vec<i32> {
    let total = frames_total(labels.len(), g);
    let mut count = vec![0f32; total];
    let mut weight = vec![0f32; total];
    for (i, l) in labels.iter().enumerate() {
        let start = chunk_frame_start(i, g);
        for (f, row) in l.rows().into_iter().enumerate() {
            count[start + f] += row.iter().map(|&x| x as f32).sum::<f32>();
            weight[start + f] += 1.0;
        }
    }
    (0..total)
        .map(|f| ((count[f] / (weight[f] + 1e-12f32)) + 0.5f32) as i32)
        .collect()
}

/// A (window, speaker) pair with its sample intervals, from the labels with overlapping frames removed.
struct Pair {
    chunk: usize,
    speaker: usize,
    intervals: Vec<(i32, i32)>,
}

/// The sample intervals where each speaker talks alone in each window (sherpa GetChunkSpeakerSampleIndexes,
/// :432-503). Frames with two or more speakers are excluded (ExcludeOverlap, :506-527).
fn chunk_speaker_intervals(labels: &[Array2<u8>], g: &Geometry) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for (c, label) in labels.iter().enumerate() {
        let alone = exclude_overlap(label);
        let frames = alone.nrows();
        let offset = c * g.shift;
        for s in 0..alone.ncols() {
            let active: i32 = (0..frames).map(|f| alone[[f, s]] as i32).sum();
            if active < MIN_SPEAKER_FRAMES {
                continue;
            }
            let mut intervals = Vec::new();
            let mut open: Option<usize> = None;
            for k in 0..frames {
                if alone[[k, s]] != 0 {
                    open.get_or_insert(k);
                } else if let Some(st) = open.take() {
                    intervals.push((
                        sample_at(st, frames, g, offset),
                        sample_at(k, frames, g, offset),
                    ));
                }
            }
            if let Some(st) = open {
                intervals.push((
                    sample_at(st, frames, g, offset),
                    sample_at(frames - 1, frames, g, offset),
                ));
            }
            pairs.push(Pair {
                chunk: c,
                speaker: s,
                intervals,
            });
        }
    }
    pairs
}

/// Keeps only the frames where at most one speaker is active (sherpa ExcludeOverlap, :506-527).
fn exclude_overlap(label: &Array2<u8>) -> Array2<u8> {
    let mut out = Array2::zeros(label.dim());
    for (f, row) in label.rows().into_iter().enumerate() {
        if row.iter().map(|&x| x as u32).sum::<u32>() < 2 {
            out.row_mut(f).assign(&row);
        }
    }
    out
}

/// The `k` largest indexes of `vals`, as the set that libstdc++'s `std::partial_sort` returns (sherpa
/// TopkIndex, math.h:112-133). Ties are broken as the library breaks them, so the result matches sherpa.
fn topk_index(vals: &[i32], k: usize) -> Vec<usize> {
    let k = k.min(vals.len());
    if k == 0 {
        return vec![];
    }
    let greater = |a: usize, b: usize| vals[a] > vals[b];
    let mut idx: Vec<usize> = (0..vals.len()).collect();
    // Only the set matters, and sort_heap does not change the set in the first k slots, so the
    // heap select alone is enough.
    make_heap(&mut idx[..k], &greater);
    for i in k..idx.len() {
        if greater(idx[i], idx[0]) {
            let value = idx[i];
            idx[i] = idx[0];
            adjust_heap(&mut idx[..k], 0, value, &greater);
        }
    }
    idx.truncate(k);
    idx
}

/// libstdc++ `__make_heap` on index values.
fn make_heap(h: &mut [usize], greater: &impl Fn(usize, usize) -> bool) {
    if h.len() < 2 {
        return;
    }
    let mut parent = (h.len() - 2) / 2;
    loop {
        let value = h[parent];
        adjust_heap(h, parent, value, greater);
        if parent == 0 {
            return;
        }
        parent -= 1;
    }
}

/// libstdc++ `__adjust_heap`: sift the hole down to a leaf, then push `value` back up.
fn adjust_heap(h: &mut [usize], mut hole: usize, value: usize, greater: &impl Fn(usize, usize) -> bool) {
    let len = h.len();
    let top = hole;
    let mut second = hole;
    while second < (len - 1) / 2 {
        second = 2 * (second + 1);
        if greater(h[second], h[second - 1]) {
            second -= 1;
        }
        h[hole] = h[second];
        hole = second;
    }
    if len % 2 == 0 && second == (len - 2) / 2 {
        second = 2 * (second + 1);
        h[hole] = h[second - 1];
        hole = second - 1;
    }
    push_heap(h, hole, top, value, greater);
}

/// libstdc++ `__push_heap`.
fn push_heap(h: &mut [usize], mut hole: usize, top: usize, value: usize, greater: &impl Fn(usize, usize) -> bool) {
    let mut parent = hole.saturating_sub(1) / 2;
    while hole > top && greater(h[parent], value) {
        h[hole] = h[parent];
        hole = parent;
        parent = hole.saturating_sub(1) / 2;
    }
    h[hole] = value;
}

/// Speakers per frame, as 0/1 (sherpa FinalizeLabels, :697-720): the top-k clusters by vote count.
fn finalize_labels(count: &Array2<i32>, speakers_per_frame: &[i32]) -> Array2<u8> {
    let mut out = Array2::zeros(count.dim());
    for i in 0..count.nrows() {
        let k = speakers_per_frame[i];
        if k <= 0 {
            continue;
        }
        let row: Vec<i32> = count.row(i).to_vec();
        for m in topk_index(&row, k as usize) {
            out[[i, m]] = 1;
        }
    }
    out
}

/// Speaker clustering over the windows (sherpa Process, :155-240). Returns no turns when nothing is clustered.
fn multi_chunk_turns(
    audio: &dyn AudioSource,
    models: &mut dyn SpeakerModels,
    labels: &[Array2<u8>],
    g: &Geometry,
    progress: &mut dyn FnMut(f32),
) -> Result<Vec<Turn>, String> {
    let n = audio.len();
    let frames = labels[0].nrows();
    if chunk_frame_start(labels.len() - 1, g) + frames > frames_total(labels.len(), g) {
        return Err("segmentation windows do not fit the frame grid".into());
    }
    let per_frame = speakers_per_frame(labels, g);
    if per_frame.iter().copied().max().unwrap_or(0) == 0 {
        return Ok(vec![]);
    }
    let pairs = chunk_speaker_intervals(labels, g);

    // sherpa ComputeEmbeddings (:536-590): one embedding per pair, from its clean samples only.
    // A pair without a usable embedding is dropped, as sherpa does for NaN.
    let mut kept: Vec<usize> = Vec::new();
    let mut embeddings: Vec<Vec<f32>> = Vec::new();
    for (k, pair) in pairs.iter().enumerate() {
        let clip = clip_samples(audio, &pair.intervals)?;
        if !clip.is_empty() {
            if let Some(e) = models.embed(&clip)? {
                if e.iter().all(|x| x.is_finite()) {
                    kept.push(k);
                    embeddings.push(e);
                }
            }
        }
        progress(0.25 + 0.65 * (k + 1) as f32 / pairs.len() as f32);
    }
    if embeddings.is_empty() {
        return Ok(vec![]);
    }

    // sherpa FastClustering::Cluster normalises the rows first (fast-clustering.cc:44-62), as the
    // complete-linkage port does itself.
    let cluster = complete_linkage_labels(&embeddings, CLUSTER_THRESHOLD);
    let clusters = cluster.iter().max().map_or(0, |m| m + 1);
    // sherpa ConvertChunkSpeakerToCluster (:592-609).
    let map: HashMap<(usize, usize), usize> = kept
        .iter()
        .zip(&cluster)
        .map(|(&k, &c)| ((pairs[k].chunk, pairs[k].speaker), c))
        .collect();
    let count = speaker_count(labels, &map, clusters, n, g);
    let fin = finalize_labels(&count, &per_frame);
    Ok(compute_result(&fin, g))
}

/// The samples of a pair's intervals, joined. Parts past the end of the audio are dropped.
fn clip_samples(audio: &dyn AudioSource, intervals: &[(i32, i32)]) -> Result<Vec<f32>, String> {
    let n = audio.len();
    let mut clip = Vec::new();
    for &(start, end) in intervals {
        let (start, end) = (start as usize, (end as usize).min(n));
        if end > start {
            let at = clip.len();
            clip.resize(at + (end - start), 0.0);
            audio.read(start, &mut clip[at..])?;
        }
    }
    Ok(clip)
}

/// How many speakers each frame has, counting one cluster once per window (sherpa ReLabel and
/// ComputeSpeakerCount, :611-695). Windows are relabelled one at a time, so only one is held at a time.
fn speaker_count(
    labels: &[Array2<u8>],
    map: &HashMap<(usize, usize), usize>,
    clusters: usize,
    n: usize,
    g: &Geometry,
) -> Array2<i32> {
    let total = frames_total(labels.len(), g);
    let mut count = Array2::<i32>::zeros((total, clusters));
    for (c, label) in labels.iter().enumerate() {
        let start = chunk_frame_start(c, g);
        for ((f, cl), &v) in relabel_chunk(label, c, clusters, map).indexed_iter() {
            count[[start + f, cl]] += v as i32;
        }
    }
    if !has_last_chunk(n, g) {
        return count;
    }
    let last = (n / g.rf_shift).min(total - 1);
    count.slice(s![..=last, ..]).to_owned()
}

/// One window with its speakers renamed to clusters. Two speakers in one cluster count once.
fn relabel_chunk(
    label: &Array2<u8>,
    chunk: usize,
    clusters: usize,
    map: &HashMap<(usize, usize), usize>,
) -> Array2<u8> {
    let mut out = Array2::zeros((label.nrows(), clusters));
    for s in 0..label.ncols() {
        let Some(&cl) = map.get(&(chunk, s)) else {
            continue;
        };
        for f in 0..label.nrows() {
            if label[[f, s]] == 1 {
                out[[f, cl]] = 1;
            }
        }
    }
    out
}

/// Frame runs become turns, with `min_duration_off` merging and `min_duration_on` filtering (sherpa
/// ComputeResult, :722-795; MergeSegments, :863-880; Segment::Merge, offline-speaker-diarization-result.cc:38-56).
fn compute_result(fin: &Array2<u8>, g: &Geometry) -> Vec<Turn> {
    let frames = fin.nrows();
    let mut turns = Vec::new();
    if frames == 0 {
        return turns;
    }
    // sherpa computes these in f32 (:737-738). The offset is a double before the cast.
    let scale = g.rf_shift as f32 / g.sample_rate as f32;
    let scale_offset = (0.5 * g.rf_size as f64 / g.sample_rate as f64) as f32;
    let span = |from: usize, to: usize| (from as f32 * scale + scale_offset, to as f32 * scale + scale_offset);
    for s in 0..fin.ncols() {
        let mut segs: Vec<(f32, f32)> = Vec::new();
        let mut open: Option<usize> = (fin[[0, s]] > 0).then_some(0);
        for f in 1..frames {
            match open {
                Some(st) => {
                    if fin[[f, s]] == 0 {
                        segs.push(span(st, f));
                        open = None;
                    }
                }
                None => {
                    if fin[[f, s]] == 1 {
                        open = Some(f);
                    }
                }
            }
        }
        if let Some(st) = open {
            segs.push(span(st, frames - 1));
        }
        merge_segments(&mut segs);
        for (start, end) in segs {
            if end - start > MIN_DURATION_ON {
                turns.push(Turn {
                    start: start as f64,
                    end: end as f64,
                    cluster: s,
                });
            }
        }
    }
    turns
}

/// Joins same-speaker segments whose gap is below `MIN_DURATION_OFF`, until none is left.
fn merge_segments(segs: &mut Vec<(f32, f32)>) {
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..segs.len().saturating_sub(1) {
            if let Some(m) = merge_pair(segs[i], segs[i + 1], MIN_DURATION_OFF) {
                segs[i] = m;
                segs.remove(i + 1);
                changed = true;
                break;
            }
        }
    }
}

/// sherpa OfflineSpeakerDiarizationSegment::Merge (offline-speaker-diarization-result.cc:38-56).
fn merge_pair(a: (f32, f32), b: (f32, f32), gap: f32) -> Option<(f32, f32)> {
    if a.1 < b.0 && a.1 + gap >= b.0 {
        Some((a.0, b.1))
    } else if b.1 < a.0 && b.1 + gap >= a.0 {
        Some((b.0, a.1))
    } else {
        None
    }
}

/// sherpa HandleOneChunkSpecialCase (:841-861). A single window is the only case here: the trim
/// applies only when a padded window follows it.
fn handle_one_chunk(labels: &Array2<u8>, n: usize, g: &Geometry) -> Vec<Turn> {
    if !has_last_chunk(n, g) {
        return compute_result(labels, g);
    }
    let keep = (n / g.rf_shift).min(labels.nrows());
    compute_result(&labels.slice(s![..keep, ..]).to_owned(), g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    const CHUNK_FRAMES: usize = 589;

    fn meta() -> SegMeta {
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

    fn geometry() -> Geometry {
        Geometry::new(&meta(), 1.0).unwrap()
    }

    /// Logits where every frame picks powerset class `class`.
    fn logits_all(class: usize) -> Array2<f32> {
        let mut m = Array2::zeros((CHUNK_FRAMES, 7));
        for f in 0..CHUNK_FRAMES {
            m[[f, class]] = 10.0;
        }
        m
    }

    /// A scripted backend. `logits(i)` answers the i-th segment call, `embedding(i, clip)` the i-th embed call.
    struct FakeModels {
        meta: SegMeta,
        logits: Box<dyn FnMut(usize) -> Array2<f32>>,
        embedding: Box<dyn FnMut(usize, &[f32]) -> Option<Vec<f32>>>,
        segment_calls: usize,
        embed_calls: usize,
    }

    impl FakeModels {
        fn new(
            logits: impl FnMut(usize) -> Array2<f32> + 'static,
            embedding: impl FnMut(usize, &[f32]) -> Option<Vec<f32>> + 'static,
        ) -> Self {
            Self {
                meta: meta(),
                logits: Box::new(logits),
                embedding: Box::new(embedding),
                segment_calls: 0,
                embed_calls: 0,
            }
        }
    }

    impl SpeakerModels for FakeModels {
        fn seg_meta(&self) -> &SegMeta {
            &self.meta
        }

        fn segment(&mut self, _chunk: &[f32]) -> Result<Array2<f32>, String> {
            let i = self.segment_calls;
            self.segment_calls += 1;
            Ok((self.logits)(i))
        }

        fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, String> {
            let i = self.embed_calls;
            self.embed_calls += 1;
            Ok((self.embedding)(i, samples))
        }

        fn backend_label(&self) -> &'static str {
            "cpu"
        }

        fn fall_back_to_cpu(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    fn unit(axis: usize) -> Vec<f32> {
        let mut v = vec![0.0; 192];
        v[axis] = 1.0;
        v
    }

    fn active_sets(m: &Array2<u8>) -> Vec<Vec<usize>> {
        m.rows()
            .into_iter()
            .map(|r| {
                r.iter()
                    .enumerate()
                    .filter(|(_, &v)| v == 1)
                    .map(|(s, _)| s)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn powerset_mapping_matches_sherpa() {
        let m = powerset_mapping(7, 3, 2).unwrap();
        assert_eq!(
            active_sets(&m),
            vec![
                vec![],
                vec![0],
                vec![1],
                vec![2],
                vec![0, 1],
                vec![0, 2],
                vec![1, 2],
            ]
        );
        assert!(powerset_mapping(6, 3, 2).is_err());
        assert!(powerset_mapping(7, 3, 3).is_err());
    }

    #[test]
    fn overlap_frames_excluded_from_embedding_audio() {
        // Speaker 0 is active in frames 0..300, speaker 1 in 200..500. Frames 200..300 have two speakers.
        let mut labels = Array2::<u8>::zeros((CHUNK_FRAMES, 3));
        for f in 0..300 {
            labels[[f, 0]] = 1;
        }
        for f in 200..500 {
            labels[[f, 1]] = 1;
        }
        let g = geometry();
        let sample = |f: usize| (f as f32 / CHUNK_FRAMES as f32 * 160_000.0) as i32;
        let pairs = chunk_speaker_intervals(&[labels], &g);
        assert_eq!(pairs.len(), 2);
        for p in &pairs {
            for &(start, end) in &p.intervals {
                match p.speaker {
                    0 => assert!(end <= sample(200), "speaker 0 reaches {end}"),
                    1 => assert!(start >= sample(300), "speaker 1 starts at {start}"),
                    s => panic!("unexpected speaker {s}"),
                }
            }
        }
    }

    #[test]
    fn diarize_silence_returns_no_turns() {
        let mut m = FakeModels::new(|_| logits_all(0), |_, _| panic!("no speech, no embeddings"));
        let audio = vec![0.0f32; 16_000 * 35];
        let turns = diarize(&audio, &mut m, 1.0, &mut |_| {}).unwrap();
        assert_eq!(turns, Vec::<Turn>::new());
        assert_eq!(m.embed_calls, 0);
    }

    #[test]
    fn diarize_single_window_skips_clustering() {
        let mut m = FakeModels::new(|_| logits_all(1), |_, _| panic!("one window needs no embedding"));
        let audio = vec![0.1f32; 160_000];
        let turns = diarize(&audio, &mut m, 1.0, &mut |_| {}).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].cluster, 0);
        assert!(turns[0].end > 9.5 && turns[0].end <= 10.0, "{:?}", turns[0]);
        assert_eq!(m.embed_calls, 0);
    }

    #[test]
    fn diarize_drops_invalid_embeddings() {
        // Speaker 0 is active in every 10 s window. Embedding 3 is None and embedding 5 has a NaN.
        let mut m = FakeModels::new(
            |_| logits_all(1),
            |i, _| match i {
                3 => None,
                5 => {
                    let mut v = unit(0);
                    v[7] = f32::NAN;
                    Some(v)
                }
                _ => Some(unit(0)),
            },
        );
        let audio = vec![0.1f32; 16_000 * 30];
        let turns = diarize(&audio, &mut m, 1.0, &mut |_| {}).unwrap();
        assert_eq!(m.embed_calls, 21);
        assert!(!turns.is_empty());
        assert!(turns.iter().all(|t| t.cluster == 0));
    }

    #[test]
    fn diarize_separates_two_voices() {
        // Windows 10 s apart: speaker 0 in window 0 and 2, speaker 1 in window 1. Orthogonal embeddings.
        let mut m = FakeModels::new(
            |i| logits_all(if i % 2 == 0 { 1 } else { 2 }),
            |i, _| Some(unit(if i % 2 == 0 { 0 } else { 1 })),
        );
        let audio = vec![0.1f32; 16_000 * 30];
        let turns = diarize(&audio, &mut m, 10.0, &mut |_| {}).unwrap();
        assert_eq!(turns.len(), 3, "{turns:?}");
        assert_ne!(turns[0].cluster, turns[1].cluster);
        assert_eq!(turns[0].cluster, turns[2].cluster);
    }

    #[test]
    fn turns_clamped_to_audio_length() {
        // A 5 s clip is one window that is padded with silence, but speaker 0 covers every frame.
        let mut m = FakeModels::new(|_| logits_all(1), |_, _| panic!("one window needs no embedding"));
        let audio = vec![0.1f32; 80_000];
        let turns = diarize(&audio, &mut m, 1.0, &mut |_| {}).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].end, 5.0);
    }

    #[test]
    fn topk_matches_libstdc_partial_sort() {
        // Expected sets come from std::partial_sort with vec[a] > vec[b] (libstdc++, g++ -O2).
        assert_eq!(sorted(topk_index(&[1, 1, 0], 1)), vec![0]);
        assert_eq!(sorted(topk_index(&[0, 2, 2, 1, 2], 2)), vec![1, 2]);
        assert_eq!(sorted(topk_index(&[3, 3, 3, 3], 1)), vec![0]);
        assert_eq!(sorted(topk_index(&[0; 7], 2)), vec![0, 1]);
        assert_eq!(sorted(topk_index(&[1, 2, 1, 2, 1, 2, 1, 2, 1], 3)), vec![1, 3, 5]);
        assert_eq!(
            sorted(topk_index(&[5, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9], 4)),
            vec![5, 11, 12, 14]
        );
        assert_eq!(sorted(topk_index(&[2, 2, 1], 1)), vec![0]);
        assert_eq!(sorted(topk_index(&[1, 1, 1, 2, 2, 2, 0, 0], 3)), vec![3, 4, 5]);
        assert_eq!(sorted(topk_index(&[4, 1], 5)), vec![0, 1]);
        assert!(topk_index(&[4, 1], 0).is_empty());
    }

    fn sorted(mut v: Vec<usize>) -> Vec<usize> {
        v.sort();
        v
    }

    #[test]
    fn geometry_rejects_zero_sizes() {
        let mut m = meta();
        m.receptive_field_shift = 0;
        assert!(Geometry::new(&m, 1.0).is_err());
        let mut m = meta();
        m.num_speakers = 0;
        assert!(Geometry::new(&m, 1.0).is_err());
    }

    #[test]
    fn geometry_clamps_the_window_shift() {
        // step 1 s is 16 000 samples, and a tiny or NaN step clamps to one sample.
        assert_eq!(Geometry::new(&meta(), 1.0).unwrap().shift, 16_000);
        assert_eq!(Geometry::new(&meta(), 0.0).unwrap().shift, 1);
        assert_eq!(Geometry::new(&meta(), f64::NAN).unwrap().shift, 1);
        assert_eq!(Geometry::new(&meta(), 100.0).unwrap().shift, 160_000);
    }

    #[test]
    fn handle_one_chunk_keeps_frames_within_audio() {
        // A window longer than one chunk has a second chunk, so the trim applies. Here it keeps every frame.
        let g = geometry();
        let mut labels = Array2::<u8>::zeros((CHUNK_FRAMES, 3));
        labels[[0, 0]] = 1;
        let full = compute_result(&labels, &g);
        assert_eq!(handle_one_chunk(&labels, 160_100, &g), full);
    }

    /// Needs ONNX Runtime and `SPEAKER_MODELS_DIR` (both models) and `SPEAKER_PARITY_DIR` (the recordings).
    /// Each run must agree with sherpa's turns on at least 99 % of speech time, with the same cluster count.
    #[test]
    #[ignore]
    fn diarize_parity_with_sherpa() {
        let models_dir = std::env::var("SPEAKER_MODELS_DIR").expect("set SPEAKER_MODELS_DIR");
        let parity = std::env::var("SPEAKER_PARITY_DIR").expect("set SPEAKER_PARITY_DIR");
        let mut models = super::super::runtime::OrtSpeakerModels::load(
            std::path::Path::new(&models_dir),
            true,
        )
        .unwrap();
        let mut worst = 1f64;
        for rec in ["1-2-1_Ira", "1-2-1_Slava", "german-lesson", "meetup"] {
            let dir = std::path::Path::new(&parity).join(rec);
            let audio = read_f32(&dir.join("audio16k.f32"));
            for (step, file) in [(1.0, "diar_step1.json"), (2.0, "diar_step2.json")] {
                let reference: Vec<(f64, f64, usize)> =
                    serde_json::from_slice(&std::fs::read(dir.join(file)).unwrap()).unwrap();
                let started = std::time::Instant::now();
                let turns = diarize(&audio, &mut models, step, &mut |_| {}).unwrap();
                let secs = started.elapsed().as_secs_f64();
                let ours: Vec<(f64, f64, usize)> =
                    turns.iter().map(|t| (t.start, t.end, t.cluster)).collect();
                let (agree, ours_k, ref_k) = agreement(&ours, &reference);
                eprintln!(
                    "{rec} step {step}s: clusters {ours_k} (sherpa {ref_k}), agreement {agree:.5}, {} turns, {secs:.1} s",
                    turns.len()
                );
                assert_eq!(ours_k, ref_k, "{rec} step {step}s: cluster count");
                worst = worst.min(agree);
            }
        }
        eprintln!("worst agreement over 8 runs: {worst:.5}");
        assert!(worst >= 0.99, "worst agreement {worst:.5}");
    }

    fn read_f32(path: &std::path::Path) -> Vec<f32> {
        std::fs::read(path)
            .unwrap()
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    /// Speech time that agrees under the best one-to-one cluster mapping (greedy by overlap). Returns the
    /// smaller of the agreement over sherpa's speech and over ours, with the two cluster counts.
    fn agreement(ours: &[(f64, f64, usize)], reference: &[(f64, f64, usize)]) -> (f64, usize, usize) {
        let ours_by = by_cluster(ours);
        let ref_by = by_cluster(reference);
        let mut pairs = Vec::new();
        for (rc, rv) in &ref_by {
            for (oc, ov) in &ours_by {
                let o = overlap(rv, ov);
                if o > 0.0 {
                    pairs.push((o, *rc, *oc));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (mut used_ref, mut used_ours) = (HashSet::new(), HashSet::new());
        let mut matched = 0.0;
        for (o, rc, oc) in pairs {
            if used_ref.contains(&rc) || used_ours.contains(&oc) {
                continue;
            }
            used_ref.insert(rc);
            used_ours.insert(oc);
            matched += o;
        }
        // A cluster's turns never overlap each other, so its speech time is the sum of its turn lengths.
        let total = |by: &BTreeMap<usize, Vec<(f64, f64)>>| -> f64 {
            by.values().flatten().map(|(s, e)| e - s).sum()
        };
        let agree = (matched / total(&ref_by)).min(matched / total(&ours_by));
        (agree, ours_by.len(), ref_by.len())
    }

    fn by_cluster(turns: &[(f64, f64, usize)]) -> BTreeMap<usize, Vec<(f64, f64)>> {
        let mut by: BTreeMap<usize, Vec<(f64, f64)>> = BTreeMap::new();
        for &(s, e, c) in turns {
            by.entry(c).or_default().push((s, e));
        }
        for v in by.values_mut() {
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        by
    }

    /// Overlap of two sorted lists of disjoint intervals.
    fn overlap(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
        let (mut i, mut j, mut total) = (0, 0, 0.0);
        while i < a.len() && j < b.len() {
            total += (a[i].1.min(b[j].1) - a[i].0.max(b[j].0)).max(0.0);
            if a[i].1 < b[j].1 {
                i += 1;
            } else {
                j += 1;
            }
        }
        total
    }
}
