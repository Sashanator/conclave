// Level track: per-50 ms mic and system RMS, used to tell the user's own voice (mic only) from others.
// A slice is mic-only when the mic is loud over its own noise floor and the system channel is quiet over its floor.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

pub const SLICE_SECS: f64 = 0.05;
pub const SLICES_PER_WINDOW: usize = 12;
/// 50 ms at 48 kHz.
const SLICE_SAMPLES: usize = 2_400;
/// Trailing window for the floor: 600 slices, 30 s.
const FLOOR_SLICES: usize = 600;
const FLOOR_PERCENTILE_DIV: usize = 10; // 10th percentile
const DB_MIN_RMS: f32 = 1e-5;
const MIC_MARGIN_DB: f32 = 12.0;
const SYS_MARGIN_DB: f32 = 6.0;
const SAMPLE_RATE: u32 = 48_000;
const VERSION: u16 = 1;
const SLICE_MS: u16 = 50;
const MAGIC: &[u8; 4] = b"CLVL";
const HEADER_LEN: usize = 12;

pub type LevelPair = (f32, f32); // (mic_rms, sys_rms)

fn slice_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Splits one 600 ms mix window into 12 `(mic_rms, sys_rms)` pairs. A short window
/// gives RMS over the samples it has, and zero for slices it has none of.
pub fn window_levels(mic: &[f32], sys: &[f32]) -> [LevelPair; SLICES_PER_WINDOW] {
    std::array::from_fn(|k| {
        let part = |x: &[f32]| {
            let a = (k * SLICE_SAMPLES).min(x.len());
            let b = ((k + 1) * SLICE_SAMPLES).min(x.len());
            slice_rms(&x[a..b])
        };
        (part(mic), part(sys))
    })
}

fn db(rms: f32) -> f32 {
    20.0 * rms.max(DB_MIN_RMS).log10()
}

/// The 10th percentile of the trailing window, from a sorted copy.
fn floor_db(window: &VecDeque<f32>) -> f32 {
    let mut sorted: Vec<f32> = window.iter().copied().collect();
    sorted.sort_unstable_by(f32::total_cmp);
    sorted[(sorted.len() - 1) / FLOOR_PERCENTILE_DIV]
}

fn push_capped(window: &mut VecDeque<f32>, value: f32) {
    if window.len() == FLOOR_SLICES {
        window.pop_front();
    }
    window.push_back(value);
}

#[derive(Debug, Default)]
pub struct FloorTracker {
    mic: VecDeque<f32>,
    sys: VecDeque<f32>,
}

impl FloorTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one slice and returns whether it is mic-only against the floors that include it.
    pub fn push(&mut self, p: LevelPair) -> bool {
        let (mic_db, sys_db) = (db(p.0), db(p.1));
        push_capped(&mut self.mic, mic_db);
        push_capped(&mut self.sys, sys_db);
        let mic_floor = floor_db(&self.mic);
        let sys_floor = floor_db(&self.sys);
        mic_db >= mic_floor + MIC_MARGIN_DB && sys_db <= sys_floor + SYS_MARGIN_DB
    }
}

/// Mic-only flag for each slice, as if every pair were pushed through one `FloorTracker`.
pub fn mic_only_flags(pairs: &[LevelPair]) -> Vec<bool> {
    let mut tracker = FloorTracker::new();
    pairs.iter().map(|&p| tracker.push(p)).collect()
}

/// Share of mic-only slices among the slices overlapping `[start, end)` seconds. `None` when that range has no slices.
pub fn mic_only_fraction(flags: &[bool], start: f64, end: f64) -> Option<f64> {
    // the epsilon stops float error (0.3 / 0.05 = 5.999…) from moving a boundary slice
    let lo = ((start.max(0.0) / SLICE_SECS + 1e-9).floor() as usize).min(flags.len());
    let hi = ((end.max(0.0) / SLICE_SECS - 1e-9).ceil() as usize).min(flags.len());
    if hi <= lo {
        return None;
    }
    let mic_only = flags[lo..hi].iter().filter(|&&f| f).count();
    Some(mic_only as f64 / (hi - lo) as f64)
}

/// Header `CLVL`, u16 version, u16 slice_ms, u32 sample_rate (all little-endian), then f32 pairs.
pub fn encode(pairs: &[LevelPair]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + pairs.len() * 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&SLICE_MS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    for &(mic, sys) in pairs {
        out.extend_from_slice(&mic.to_le_bytes());
        out.extend_from_slice(&sys.to_le_bytes());
    }
    out
}

/// Writes `levels.bin` in the meeting folder through a temp file, so a crash never leaves a half file.
pub fn write_levels_file(folder: &Path, pairs: &[LevelPair]) -> std::io::Result<()> {
    let tmp = folder.join(".levels.bin.tmp");
    fs::write(&tmp, encode(pairs))?;
    fs::rename(&tmp, folder.join("levels.bin"))
}

pub fn decode(bytes: &[u8]) -> Result<Vec<LevelPair>, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!("levels file too short: {} bytes", bytes.len()));
    }
    if &bytes[0..4] != MAGIC {
        return Err("levels file has a bad magic".into());
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    let slice_ms = u16::from_le_bytes([bytes[6], bytes[7]]);
    let sample_rate = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if version != VERSION || slice_ms != SLICE_MS || sample_rate != SAMPLE_RATE {
        return Err(format!(
            "unsupported levels header: version {version}, slice {slice_ms} ms, {sample_rate} Hz"
        ));
    }
    let body = &bytes[HEADER_LEN..];
    if body.len() % 8 != 0 {
        return Err(format!(
            "levels file body is {} bytes, not whole pairs",
            body.len()
        ));
    }
    Ok(body
        .chunks_exact(8)
        .map(|c| {
            let mic = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            let sys = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
            (mic, sys)
        })
        .collect())
}

// Live store: one recording at a time, process-global. Written by the mix worker, read at save time.

struct Live {
    pairs: Vec<LevelPair>,
    flags: Vec<bool>,
    tracker: FloorTracker,
}

static LIVE: LazyLock<Mutex<Option<Live>>> = LazyLock::new(|| Mutex::new(None));
/// Every test that touches `LIVE` holds this, since cargo runs tests in parallel.
#[cfg(test)]
pub(crate) static LIVE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn live() -> MutexGuard<'static, Option<Live>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts a fresh track when `active`, or drops any track when not.
pub fn live_reset(active: bool) {
    *live() = active.then(|| Live {
        pairs: Vec::new(),
        flags: Vec::new(),
        tracker: FloorTracker::new(),
    });
}

pub fn live_push(window: [LevelPair; SLICES_PER_WINDOW]) {
    if let Some(live) = live().as_mut() {
        for p in window {
            live.flags.push(live.tracker.push(p));
            live.pairs.push(p);
        }
    }
}

/// Mic-only share of `[start, end)` in the live track; `None` when inactive or the range has no slices.
pub fn live_fraction(start: f64, end: f64) -> Option<f64> {
    let guard = live();
    mic_only_fraction(&guard.as_ref()?.flags, start, end)
}

/// `Some(0)` (You) when over half of `[start, end)` is mic-only, else `None` (Others, or inactive).
pub fn live_label(start: f64, end: f64) -> Option<i64> {
    live_fraction(start, end).filter(|&f| f > 0.5).map(|_| 0)
}

pub fn live_is_active() -> bool {
    live().is_some()
}

/// The recorded pairs when a track is active. Always leaves the store inactive.
pub fn live_take() -> Option<Vec<LevelPair>> {
    live().take().map(|l| l.pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet(n: usize) -> Vec<LevelPair> {
        vec![(0.001, 0.001); n]
    }

    #[test]
    fn window_levels_splits_600ms_into_12_rms_pairs() {
        let mic: Vec<f32> = (0..28_800)
            .map(|i| (i / SLICE_SAMPLES) as f32 / 12.0)
            .collect();
        let sys = vec![0.0f32; 28_800];
        let levels = window_levels(&mic, &sys);
        for (k, (m, s)) in levels.iter().enumerate() {
            assert!((m - k as f32 / 12.0).abs() < 1e-6, "slice {k}: {m}");
            assert_eq!(*s, 0.0);
        }
        // a short window: RMS over the samples it has, zero for the slices it lacks
        let short = window_levels(&vec![0.5f32; 1_200], &[]);
        assert!((short[0].0 - 0.5).abs() < 1e-6);
        assert!(short[1..].iter().all(|p| p.0 == 0.0));
    }

    #[test]
    fn mic_only_needs_mic_active_and_system_quiet() {
        let mut t = FloorTracker::new();
        for p in quiet(600) {
            t.push(p);
        }
        assert!(t.push((0.1, 0.001)));
        assert!(!t.push((0.1, 0.1)), "echo or overlap counts as Others");
    }

    #[test]
    fn floors_adapt_to_constant_system_hum() {
        let mut t = FloorTracker::new();
        for _ in 0..600 {
            t.push((0.001, 0.05));
        }
        assert!(
            t.push((0.1, 0.05)),
            "the hum is the floor, so mic speech still counts"
        );
    }

    #[test]
    fn flags_equal_live_tracker() {
        // deterministic pseudo-random levels, no rand dependency
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 40) as f32 / (1u64 << 24) as f32) * 0.2
        };
        let pairs: Vec<LevelPair> = (0..1_500).map(|_| (next(), next())).collect();
        let _guard = LIVE_TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        live_reset(true);
        for window in pairs.chunks_exact(SLICES_PER_WINDOW) {
            live_push(window.try_into().unwrap());
        }
        // a slice is mic-only exactly when its own 50 ms range is labelled You
        let live_flags: Vec<bool> = (0..pairs.len())
            .map(|k| live_label(k as f64 * SLICE_SECS, (k + 1) as f64 * SLICE_SECS).is_some())
            .collect();
        assert_eq!(live_flags, mic_only_flags(&pairs));
        assert!(live_flags.contains(&true) && live_flags.contains(&false));
        assert_eq!(live_take(), Some(pairs));
    }

    #[test]
    fn fraction_maps_seconds_to_slices() {
        let flags = [true, false, true, true, false, false, true, false];
        // [0, 0.1) is slices 0 and 1
        assert_eq!(mic_only_fraction(&flags, 0.0, 0.1), Some(0.5));
        // [0.1, 0.2) is slices 2 and 3
        assert_eq!(mic_only_fraction(&flags, 0.1, 0.2), Some(1.0));
        // 0.3 / 0.05 is 5.999… in floats; the epsilon keeps this at slices 6 and 7
        assert_eq!(mic_only_fraction(&flags, 0.3, 0.4), Some(0.5));
        assert_eq!(mic_only_fraction(&flags, 0.1, 0.1), None);
        assert_eq!(mic_only_fraction(&flags, 5.0, 6.0), None);
    }

    #[test]
    fn encode_decode_roundtrip_and_rejects_bad_header() {
        let pairs = vec![(0.25f32, 0.5f32), (0.0, 1.0)];
        let bytes = encode(&pairs);
        assert_eq!(&bytes[0..4], b"CLVL");
        assert_eq!(&bytes[4..6], &1u16.to_le_bytes());
        assert_eq!(&bytes[6..8], &50u16.to_le_bytes());
        assert_eq!(&bytes[8..12], &48_000u32.to_le_bytes());
        assert_eq!(decode(&bytes).unwrap(), pairs);
        assert_eq!(decode(&encode(&[])).unwrap(), Vec::<LevelPair>::new());

        assert!(decode(&bytes[..bytes.len() - 4]).is_err(), "truncated body");
        let mut odd = bytes.clone();
        odd.push(0);
        assert!(decode(&odd).is_err(), "odd body");
        assert!(decode(&bytes[..10]).is_err(), "truncated header");
        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        assert!(decode(&bad_magic).is_err());
        let mut bad_rate = bytes.clone();
        bad_rate[8] = 0x00;
        bad_rate[9] = 0x7d; // 32000 Hz
        assert!(decode(&bad_rate).is_err());
    }

    #[test]
    fn live_store_labels_and_resets() {
        let _guard = LIVE_TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(!live_is_active());
        live_push([(0.1, 0.1); SLICES_PER_WINDOW]); // no-op while inactive
        assert_eq!(live_take(), None);

        live_reset(true);
        assert!(live_is_active());
        for _ in 0..50 {
            live_push([(0.001, 0.001); SLICES_PER_WINDOW]);
        }
        live_push([(0.1, 0.001); SLICES_PER_WINDOW]);
        // the last window is mic-only over [30, 30.6) (slices 600..612 of the track)
        assert_eq!(live_label(30.0, 30.6), Some(0));
        assert_eq!(live_label(0.0, 0.6), None);

        let pairs = live_take().expect("active track");
        assert_eq!(pairs.len(), 51 * SLICES_PER_WINDOW);
        assert!(!live_is_active());
        assert_eq!(live_label(30.0, 30.6), None);
    }
}
