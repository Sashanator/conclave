// Speaker clustering: complete linkage on cosine distance, and the cap on how many embeddings we cluster.
// Port of sherpa-onnx 1.13.8 `FastClustering` (fast-clustering.cc), which calls fastcluster's hclust_fast
// (HCLUST_METHOD_COMPLETE) and cutree_cdist.

/// Clustering cap: the f32 condensed distance matrix for this many embeddings is about 0.45 GB.
pub const MAX_EMBEDDINGS: usize = 15_000;

/// Length of one embedding window in seconds; the step is the hop between windows.
const WINDOW_SECS: f64 = 10.0;

/// Labels 0..k by first appearance. Cut at `threshold` on the distance `1 - cosine`. The inputs must be finite.
pub fn complete_linkage_labels(emb: &[Vec<f32>], threshold: f32) -> Vec<usize> {
    let n = emb.len();
    if n <= 1 {
        return vec![0; n];
    }
    let mut merges = nn_chain_complete(n, condensed_distances(emb));
    // sherpa's cutree_cdist (fastcluster.cpp:99-108): merges in height order, stop at the first height >= threshold.
    merges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut parent: Vec<usize> = (0..n).collect();
    for (height, a, b) in merges {
        if height >= threshold {
            break;
        }
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    }
    let mut label_of_root = vec![usize::MAX; n];
    let mut next = 0;
    (0..n)
        .map(|i| {
            let root = find(&mut parent, i);
            if label_of_root[root] == usize::MAX {
                label_of_root[root] = next;
                next += 1;
            }
            label_of_root[root]
        })
        .collect()
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Cosine distance `max(0, 1 - cos)` for each pair i < j, in row-major upper-triangle order. Like sherpa
/// (fast-clustering.cc:46-62) the rows are L2-normalised first. f32 keeps N = 15 000 at about 0.45 GB.
fn condensed_distances(emb: &[Vec<f32>]) -> Vec<f32> {
    let n = emb.len();
    let unit: Vec<Vec<f32>> = emb.iter().map(|v| unit_vector(v)).collect();
    let mut out = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in i + 1..n {
            // f32::max returns the other operand for NaN, so a NaN similarity becomes distance 0.
            out.push((1.0 - dot(&unit[i], &unit[j])).max(0.0));
        }
    }
    out
}

/// Index of pair (i, j), i < j, in the condensed upper-triangle matrix.
fn condensed_index(n: usize, i: usize, j: usize) -> usize {
    let (i, j) = if i < j { (i, j) } else { (j, i) };
    i * n - i * (i + 1) / 2 + (j - i - 1)
}

/// Complete-linkage merges as (height, a, b), where a and b are cluster representatives (member indices).
/// Nearest-neighbour chain: merge reciprocal nearest neighbours, then update with max (Lance-Williams).
fn nn_chain_complete(n: usize, mut dist: Vec<f32>) -> Vec<(f32, usize, usize)> {
    let mut active = vec![true; n];
    let mut merges = Vec::with_capacity(n - 1);
    let mut chain: Vec<usize> = Vec::new();
    let mut first_active = 0;
    while merges.len() < n - 1 {
        if chain.is_empty() {
            while !active[first_active] {
                first_active += 1;
            }
            chain.push(first_active);
        }
        let x = chain[chain.len() - 1];
        let prev = (chain.len() >= 2).then(|| chain[chain.len() - 2]);
        // Prefer the previous chain element on ties, so a reciprocal pair is always detected.
        let mut best = prev.map(|p| (dist[condensed_index(n, x, p)], p));
        for j in 0..n {
            if j == x || !active[j] {
                continue;
            }
            let d = dist[condensed_index(n, x, j)];
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, j));
            }
        }
        let (height, y) = best.expect("at least two active clusters remain");
        if prev == Some(y) {
            chain.truncate(chain.len() - 2);
            let (lo, hi) = (x.min(y), x.max(y));
            merges.push((height, lo, hi));
            for k in 0..n {
                if active[k] && k != lo && k != hi {
                    let d = dist[condensed_index(n, lo, k)].max(dist[condensed_index(n, hi, k)]);
                    dist[condensed_index(n, lo, k)] = d;
                }
            }
            active[hi] = false;
        } else {
            chain.push(y);
        }
    }
    merges
}

/// Smallest whole step (1 s, 2 s, ...) with `3 * windows(step) <= MAX_EMBEDDINGS`.
pub fn window_step_secs(duration_secs: f64) -> f64 {
    // A non-finite duration has no window count; the 1 s step is the fallback (and keeps the loop finite).
    if !duration_secs.is_finite() {
        return 1.0;
    }
    let mut step = 1.0;
    while 3.0 * window_count(duration_secs, step) > MAX_EMBEDDINGS as f64 {
        step += 1.0;
    }
    step
}

/// Number of windows for a meeting: `floor((dur - 10) / step) + 1`, plus one for a partial last window.
fn window_count(duration_secs: f64, step: f64) -> f64 {
    let span = duration_secs - WINDOW_SECS;
    if span <= 0.0 {
        return 1.0;
    }
    let full = (span / step).floor();
    let partial = if span - full * step > 1e-9 { 1.0 } else { 0.0 };
    full + 1.0 + partial
}

/// Cosine similarity; 0 when either vector has zero norm.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (na, nb) = (dot(a, a).sqrt(), dot(b, b).sqrt());
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot(a, b) / (na * nb)
    }
}

/// Mean of the unit-normalised vectors, normalised again. None if empty or the mean has zero norm.
/// Each vector counts equally, whatever its length. The brief leaves this choice open.
pub fn mean_normalized(v: &[Vec<f32>]) -> Option<Vec<f32>> {
    // A vector with a NaN or infinity is skipped, so that it cannot poison the whole mean.
    let finite = |x: &&Vec<f32>| x.iter().all(|f| f.is_finite());
    let first = v.iter().find(finite)?;
    let mut sum = vec![0.0f32; first.len()];
    for x in v.iter().filter(finite) {
        for (s, u) in sum.iter_mut().zip(unit_vector(x)) {
            *s += u;
        }
    }
    let norm = dot(&sum, &sum).sqrt();
    (norm > 0.0).then(|| sum.iter().map(|s| s / norm).collect())
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `v` scaled to unit length. A zero vector stays zero, so it has distance 1 to everything.
fn unit_vector(v: &[f32]) -> Vec<f32> {
    let norm = dot(v, v).sqrt();
    if norm == 0.0 {
        v.to_vec()
    } else {
        v.iter().map(|x| x / norm).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Naive O(N^3) complete linkage on f64 cosine distance: merge the closest pair of clusters while it is below the threshold.
    fn naive_complete_linkage(emb: &[Vec<f32>], threshold: f32) -> Vec<usize> {
        let n = emb.len();
        let dist = |i: usize, j: usize| -> f64 {
            let dot: f64 = emb[i]
                .iter()
                .zip(&emb[j])
                .map(|(x, y)| (*x as f64) * (*y as f64))
                .sum();
            let ni: f64 = emb[i]
                .iter()
                .map(|x| (*x as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            let nj: f64 = emb[j]
                .iter()
                .map(|x| (*x as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            (1.0 - dot / (ni * nj)).max(0.0)
        };
        let mut clusters: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        loop {
            let mut best: Option<(f64, usize, usize)> = None;
            for a in 0..clusters.len() {
                for b in a + 1..clusters.len() {
                    let mut d = f64::MIN;
                    for &i in &clusters[a] {
                        for &j in &clusters[b] {
                            d = d.max(dist(i, j));
                        }
                    }
                    if best.map_or(true, |(bd, _, _)| d < bd) {
                        best = Some((d, a, b));
                    }
                }
            }
            match best {
                Some((d, a, b)) if d < threshold as f64 => {
                    let moved = clusters.remove(b);
                    clusters[a].extend(moved);
                }
                _ => break,
            }
        }
        let cluster_of: Vec<usize> = (0..n)
            .map(|i| clusters.iter().position(|c| c.contains(&i)).unwrap())
            .collect();
        let mut label_of = vec![usize::MAX; clusters.len()];
        let mut next = 0;
        cluster_of
            .iter()
            .map(|&c| {
                if label_of[c] == usize::MAX {
                    label_of[c] = next;
                    next += 1;
                }
                label_of[c]
            })
            .collect()
    }

    /// Deterministic xorshift64 so the random test is reproducible without a crate.
    fn rng(state: &mut u64) -> f32 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        ((*state >> 11) as f64 / (1u64 << 53) as f64) as f32 * 2.0 - 1.0
    }

    #[test]
    fn complete_linkage_two_tight_groups() {
        // Group A near e1, group B has cosine 0.1 to e1, so the between-group distance is 0.9.
        let mut emb = Vec::new();
        for i in 0..5 {
            emb.push(vec![1.0, 0.01 * i as f32, 0.0]);
        }
        for i in 0..5 {
            emb.push(vec![0.1, 0.995, 0.01 * i as f32]);
        }
        let labels = complete_linkage_labels(&emb, 0.7);
        assert_eq!(labels, vec![0, 0, 0, 0, 0, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn complete_linkage_chain_is_not_single_linkage() {
        // Unit vectors cannot have cos(a,b) = cos(b,c) = 0.9 with cos(a,c) = 0.2 (the angles break the
        // triangle inequality), so use cos 0.6, 0.6 and -0.28. Distances: ab 0.4, bc 0.4, ac 1.28.
        let a = vec![0.6, -0.8];
        let b = vec![1.0, 0.0];
        let c = vec![0.6, 0.8];
        let labels = complete_linkage_labels(&[a, b, c], 0.7);
        assert_eq!(labels, vec![0, 0, 1]);
    }

    #[test]
    fn complete_linkage_matches_naive_on_random() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut emb = Vec::new();
        for _ in 0..4 {
            let centre: Vec<f32> = (0..8).map(|_| rng(&mut state)).collect();
            let spread = 0.1 + 0.4 * (rng(&mut state) + 1.0);
            for _ in 0..15 {
                emb.push(
                    centre
                        .iter()
                        .map(|c| c + spread * rng(&mut state))
                        .collect::<Vec<f32>>(),
                );
            }
        }
        let labels = complete_linkage_labels(&emb, 0.7);
        let k = labels.iter().max().unwrap() + 1;
        assert!(
            k > 1 && k < emb.len(),
            "test data should give a non-trivial partition, got {k}"
        );
        assert_eq!(labels, naive_complete_linkage(&emb, 0.7));
    }

    #[test]
    fn complete_linkage_cut_is_strict_at_threshold() {
        // Orthogonal unit vectors have distance exactly 1.0. cutree_cdist merges only heights < threshold.
        let emb = [vec![1.0, 0.0], vec![0.0, 1.0]];
        assert_eq!(complete_linkage_labels(&emb, 1.0), vec![0, 1]);
        assert_eq!(complete_linkage_labels(&emb, 1.0001), vec![0, 0]);
    }

    #[test]
    fn complete_linkage_single_and_empty() {
        assert_eq!(complete_linkage_labels(&[vec![1.0, 2.0]], 0.7), vec![0]);
        assert_eq!(complete_linkage_labels(&[], 0.7), Vec::<usize>::new());
    }

    #[test]
    fn window_step_keeps_embeddings_under_cap() {
        assert_eq!(window_step_secs(3600.0), 1.0);
        assert_eq!(window_step_secs(10800.0), 3.0);
        for dur in [600.0, 3600.0, 7200.0, 10800.0, 21600.0] {
            let step = window_step_secs(dur);
            assert!(
                3.0 * window_count(dur, step) <= MAX_EMBEDDINGS as f64,
                "dur {dur} step {step}"
            );
        }
    }

    #[test]
    fn window_step_non_finite_duration_is_one_second() {
        for dur in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(window_step_secs(dur), 1.0, "duration {dur}");
        }
    }

    #[test]
    fn mean_normalized_skips_non_finite_vectors() {
        assert_eq!(
            mean_normalized(&[vec![1.0, 0.0], vec![f32::NAN, 0.0]]),
            Some(vec![1.0, 0.0])
        );
        assert_eq!(
            mean_normalized(&[vec![f32::INFINITY, 0.0], vec![0.0, 2.0]]),
            Some(vec![0.0, 1.0])
        );
        assert_eq!(mean_normalized(&[vec![f32::NAN, 1.0]]), None);
    }

    #[test]
    fn cosine_and_mean_helpers() {
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        let m = mean_normalized(&[vec![1.0, 0.0], vec![0.0, 5.0]]).unwrap();
        assert!((m[0] - m[1]).abs() < 1e-6 && m[0] > 0.0);
        assert_eq!(mean_normalized(&[]), None);
        assert_eq!(mean_normalized(&[vec![1.0, 0.0], vec![-1.0, 0.0]]), None);
    }
}
