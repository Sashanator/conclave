//! Live dictation segment scheduler. Pure: no I/O, no clock, no threads. The async worker (Task 6)
//! feeds it audio and VAD segments and asks it for the next decode job.

use std::collections::VecDeque;

pub const MAX_SEGMENT_SECS: f32 = 20.0;
pub const SPLIT_WINDOW_SECS: f32 = 2.0;
pub const SPLIT_FRAME_MS: usize = 30;
pub const HOLD_SPEECH_SECS: f32 = 3.0;
pub const MIN_CUT_SPEECH_SECS: f32 = 0.25;
pub const SILENCE_KEEP_SECS: f32 = 1.0;
pub const PREVIEW_EVERY_MS: u64 = 1000;
pub const PREVIEW_EVERY_MEETING_MS: u64 = 3000;
pub const PREVIEW_MIN_GROWTH_SECS: f32 = 0.5;
pub const FIRST_PREVIEW_SECS: f32 = 1.5;
pub const PROMPT_CHARS: usize = 200;
pub const PILL_TAIL_CHARS: usize = 400;

#[derive(Debug, PartialEq)]
pub enum Job {
    Commit { index: u64, samples: Vec<f32>, prompt: String },
    Preview { from: usize, samples: Vec<f32>, prompt: String },
}

/// Positions are native-rate sample indices in "kept" coordinates: absolute session
/// positions minus `removed`, the silence that was dropped from the buffer.
pub struct LiveCore {
    rate: u32,
    total: usize,
    cut: usize,
    committed_to: usize,
    removed: usize,
    buf: Vec<f32>, // audio from `committed_to` on
    vad_end: usize,
    in_speech: bool,
    speech_mark: usize, // `total` at the last chunk flagged as speech
    silence_run: usize, // samples pushed since speech last flagged (trimming does not reset it)
    held_speech: f32,
    decided: bool,
    ready: VecDeque<Vec<f32>>,
    next_index: u64,
    in_flight: Option<u64>,
    transcript: String,
    prompt_start: usize,
    last_lang: Option<String>,
    volatile: String,
    accepted_from: Option<usize>,
    preview_in_flight: bool,
    last_preview_now: Option<u64>,
    last_preview_total: usize,
    speech_secs: f32,
    finishing: bool,
    cancelled: bool,
    failed: bool,
    stats: (usize, usize, usize),
}

impl LiveCore {
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1), // 0 would make every length 0 and the split loop spin
            total: 0,
            cut: 0,
            committed_to: 0,
            removed: 0,
            buf: Vec::new(),
            vad_end: 0,
            in_speech: false,
            speech_mark: 0,
            silence_run: 0,
            held_speech: 0.0,
            decided: false,
            ready: VecDeque::new(),
            next_index: 0,
            in_flight: None,
            transcript: String::new(),
            prompt_start: 0,
            last_lang: None,
            volatile: String::new(),
            accepted_from: None,
            preview_in_flight: false,
            last_preview_now: None,
            last_preview_total: 0,
            speech_secs: 0.0,
            finishing: false,
            cancelled: false,
            failed: false,
            stats: (0, 0, 0),
        }
    }

    fn samples(&self, secs: f32) -> usize {
        (secs * self.rate as f32) as usize
    }

    pub fn push_audio(&mut self, samples: &[f32], in_speech: bool) {
        if self.cancelled || self.finishing {
            return;
        }
        self.total += samples.len();
        self.buf.extend_from_slice(samples);
        self.in_speech = in_speech;
        self.silence_run = if in_speech { 0 } else { self.silence_run + samples.len() };
        if in_speech {
            self.speech_mark = self.total;
            let (max, win) = (self.samples(MAX_SEGMENT_SECS), self.samples(SPLIT_WINDOW_SECS));
            while self.total - self.cut >= max {
                let ws = self.cut + max - win;
                let w = &self.buf[ws - self.committed_to..ws + win - self.committed_to];
                let at = ws + quietest_frame_centre(w, self.rate as usize * SPLIT_FRAME_MS / 1000);
                let secs = (at - self.cut) as f32 / self.rate as f32;
                self.speech_secs += secs;
                self.close_range(at, secs);
            }
        } else {
            // The VAD reports a phrase only after the chunk that ends it, so never trim flagged speech
            // it has not reported yet (the core does not depend on feed order).
            let from = self.cut.max(self.vad_end);
            let unreported = self.speech_mark > from.max(self.vad_end + self.samples(SILENCE_KEEP_SECS));
            if !unreported && self.total - from > self.samples(SILENCE_KEEP_SECS + 1.0) {
                // Silence is never decoded; the held prefix before `cut` stays.
                let n = self.total - from - self.samples(SILENCE_KEEP_SECS);
                self.buf.drain(from - self.committed_to..from - self.committed_to + n);
                self.removed += n;
                self.total -= n;
                self.last_preview_total = self.last_preview_total.min(self.total);
                self.speech_mark = self.speech_mark.min(self.total);
            }
        }
    }

    pub fn on_vad_segment(&mut self, start_ms: f64, end_ms: f64) {
        if self.cancelled || self.finishing {
            return;
        }
        self.speech_secs += ((end_ms - start_ms).max(0.0) / 1000.0) as f32;
        let idx = |ms: f64| ((ms * self.rate as f64 / 1000.0) as usize).saturating_sub(self.removed).min(self.total);
        let (s, e) = (idx(start_ms), idx(end_ms));
        self.vad_end = self.vad_end.max(e);
        let overlap = e.saturating_sub(s.max(self.cut));
        if overlap >= self.samples(MIN_CUT_SPEECH_SECS) && overlap > 0 {
            self.close_range(e, overlap as f32 / self.rate as f32);
        }
    }

    fn close_range(&mut self, new_cut: usize, speech: f32) {
        self.cut = new_cut;
        self.held_speech += speech;
        if self.decided || self.held_speech >= HOLD_SPEECH_SECS {
            self.release();
        }
    }

    /// Sends `[committed_to, cut)` to the ready queue.
    fn release(&mut self) {
        let n = self.cut - self.committed_to;
        if n > 0 {
            self.ready.push_back(self.buf.drain(..n).collect());
            self.committed_to = self.cut;
        }
        self.held_speech = 0.0;
        self.decided = true;
    }

    fn prompt(&self) -> String {
        tail_chars(&self.transcript[self.prompt_start..], PROMPT_CHARS).to_string()
    }

    pub fn next_job(&mut self, now_ms: u64, meeting: bool, engine_idle: bool) -> Option<Job> {
        if self.cancelled {
            return None;
        }
        if self.in_flight.is_none() && !self.failed {
            if let Some(samples) = self.ready.pop_front() {
                let index = self.next_index;
                self.next_index += 1;
                self.in_flight = Some(index);
                self.stats.0 += 1;
                return Some(Job::Commit { index, samples, prompt: self.prompt() });
            }
        }
        if self.finishing || self.failed || self.preview_in_flight || self.buf.is_empty() {
            return None;
        }
        let held = self.cut > self.committed_to;
        if !(self.in_speech || (held && self.silence_run <= self.samples(SPLIT_WINDOW_SECS))) {
            return None;
        }
        if !self.decided && self.total - self.committed_to < self.samples(FIRST_PREVIEW_SECS) {
            return None;
        }
        if self.total - self.last_preview_total < self.samples(PREVIEW_MIN_GROWTH_SECS) {
            return None;
        }
        let every = if meeting { PREVIEW_EVERY_MEETING_MS } else { PREVIEW_EVERY_MS };
        if self.last_preview_now.is_some_and(|t| now_ms.saturating_sub(t) < every) {
            return None;
        }
        self.last_preview_now = Some(now_ms);
        if meeting && !engine_idle {
            self.stats.2 += 1;
            return None;
        }
        self.preview_in_flight = true;
        self.last_preview_total = self.total;
        self.stats.1 += 1;
        Some(Job::Preview { from: self.committed_to, samples: self.buf.clone(), prompt: self.prompt() })
    }

    pub fn on_commit_done(&mut self, index: u64, text: Result<String, ()>, language: Option<String>) {
        if self.cancelled || self.in_flight != Some(index) {
            return;
        }
        self.in_flight = None;
        let Ok(text) = text else {
            self.failed = true;
            return;
        };
        let text = text.trim();
        if !text.is_empty() && !self.transcript.is_empty() {
            self.transcript.push(' ');
        }
        let text_start = self.transcript.len();
        self.transcript.push_str(text);
        if let Some(l) = language {
            if self.last_lang.as_ref().is_some_and(|p| *p != l) {
                self.prompt_start = text_start;
            }
            self.last_lang = Some(l);
        }
        if self.accepted_from.is_some_and(|f| f < self.committed_to) {
            self.volatile.clear();
            self.accepted_from = None;
        }
    }

    pub fn on_preview_done(&mut self, from: usize, text: String) {
        if self.cancelled {
            return;
        }
        self.preview_in_flight = false;
        if from == self.committed_to {
            self.volatile = text;
            self.accepted_from = Some(from);
        }
    }

    pub fn finish(&mut self) {
        if self.cancelled || self.finishing {
            return;
        }
        // The VAD flush may report a stretch shorter than the recording-time minimum; keep it.
        self.cut = self.cut.max(self.vad_end.min(self.total));
        self.release();
        self.buf.clear();
        self.finishing = true;
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn fail(&mut self) {
        self.failed = true;
    }

    pub fn is_done(&self) -> bool {
        self.cancelled || (self.finishing && self.in_flight.is_none() && (self.ready.is_empty() || self.failed))
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    pub fn volatile(&self) -> &str {
        &self.volatile
    }

    pub fn speech_secs(&self) -> f32 {
        self.speech_secs
    }

    /// (commits, previews, previews skipped)
    pub fn stats(&self) -> (usize, usize, usize) {
        self.stats
    }

    #[cfg(test)]
    fn buffered(&self) -> usize {
        self.buf.len()
    }
}

pub fn tail_chars(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    let (i, _) = s.char_indices().nth(count - n).expect("count > n");
    &s[i..]
}

/// Index (relative to `samples`) of the centre of the quietest `frame`-sample frame (first minimum, RMS).
pub fn quietest_frame_centre(samples: &[f32], frame: usize) -> usize {
    let frame = frame.max(1);
    let mut best: Option<(f32, usize)> = None;
    for (i, f) in samples.chunks_exact(frame).enumerate() {
        let e = f.iter().map(|x| x * x).sum::<f32>() / frame as f32;
        if best.is_none_or(|(b, _)| e < b) {
            best = Some((e, i));
        }
    }
    best.map_or(samples.len() / 2, |(_, i)| i * frame + frame / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: u32 = 16_000;

    fn n(secs: f32) -> usize {
        (secs * R as f32) as usize
    }

    fn speech(core: &mut LiveCore, secs: f32) {
        core.push_audio(&vec![0.5; n(secs)], true);
    }

    /// Pushes `secs` of speech and reports it to the VAD as one closed segment.
    fn seg(core: &mut LiveCore, secs: f32) {
        let s = core.total as f64 / core.rate as f64 * 1000.0;
        speech(core, secs);
        core.on_vad_segment(s, s + secs as f64 * 1000.0);
    }

    /// Real worker order: the chunk that ends the phrase arrives flagged not-in-speech
    /// before the VAD reports the segment.
    fn seg_real(core: &mut LiveCore, secs: f32) {
        let s = core.total as f64 / core.rate as f64 * 1000.0;
        speech(core, secs);
        core.push_audio(&vec![0.0; n(0.05)], false);
        core.on_vad_segment(s, s + secs as f64 * 1000.0);
    }

    /// First decision made: 3 s of speech released, commit 0 ("zero", en) done.
    fn decided(rate: u32) -> LiveCore {
        let mut c = LiveCore::new(rate);
        seg_real(&mut c, 3.0);
        assert!(matches!(c.next_job(0, false, true), Some(Job::Commit { index: 0, .. })));
        c.on_commit_done(0, Ok("zero".into()), Some("en".into()));
        c
    }

    fn commit(c: &mut LiveCore) -> (u64, Vec<f32>, String) {
        match c.next_job(0, false, true) {
            Some(Job::Commit { index, samples, prompt }) => (index, samples, prompt),
            other => panic!("expected commit, got {other:?}"),
        }
    }

    #[test]
    fn force_split_at_20s_cuts_at_quietest_frame_in_last_2s() {
        let mut c = LiveCore::new(R);
        let mut a = vec![0.5; n(20.0)];
        a[n(19.0)..n(19.0) + n(0.03)].fill(0.0);
        c.push_audio(&a, true);
        let (_, samples, _) = commit(&mut c);
        assert!(n(19.0) <= samples.len() && samples.len() <= n(19.03), "{}", samples.len());
    }

    #[test]
    fn first_commit_waits_for_3s_of_speech() {
        let mut c = LiveCore::new(R);
        seg(&mut c, 1.0);
        seg(&mut c, 1.5);
        assert!(!matches!(c.next_job(0, false, true), Some(Job::Commit { .. })));
        seg(&mut c, 0.6);
        let (idx, samples, _) = commit(&mut c);
        assert_eq!((idx, samples.len()), (0, n(1.0) + n(1.5) + n(0.6)));
    }

    #[test]
    fn finish_releases_held_audio_under_three_seconds() {
        let mut c = LiveCore::new(R);
        seg(&mut c, 1.2);
        speech(&mut c, 0.3); // open tail, no VAD segment
        c.finish();
        let (_, samples, _) = commit(&mut c);
        assert_eq!(samples.len(), n(1.2));
        assert!(!c.is_done());
        c.on_commit_done(0, Ok("x".into()), None);
        assert!(c.is_done());
    }

    #[test]
    fn finish_keeps_a_short_vad_overlap_past_the_cut() {
        let mut c = decided(R);
        let ct = c.committed_to;
        speech(&mut c, 1.0);
        c.on_vad_segment(ct as f64 * 1000.0 / R as f64, (ct + n(1.0)) as f64 * 1000.0 / R as f64);
        commit(&mut c);
        c.on_commit_done(1, Ok("a".into()), None);
        let cut = c.cut;
        speech(&mut c, 0.1);
        let start = (cut + n(0.05)) as f64 * 1000.0 / R as f64;
        c.on_vad_segment(start, c.total as f64 * 1000.0 / R as f64); // 0.1 s past the cut: under 0.25 s
        assert!(!matches!(c.next_job(0, false, true), Some(Job::Commit { .. })));
        let total = c.total;
        c.finish();
        let (_, samples, _) = commit(&mut c);
        assert_eq!(samples.len(), total - cut);
    }

    #[test]
    fn force_cut_then_short_vad_overlap_is_kept_at_finish() {
        let mut c = LiveCore::new(R);
        speech(&mut c, 20.0);
        let (_, first, _) = commit(&mut c);
        c.on_commit_done(0, Ok("a".into()), None);
        c.on_vad_segment(19_800.0, 20_000.0); // 0.2 s past the force cut
        assert!(!matches!(c.next_job(0, false, true), Some(Job::Commit { .. })));
        c.finish();
        let (idx, rest, _) = commit(&mut c);
        assert_eq!((idx, first.len() + rest.len()), (1, n(20.0)));
    }

    #[test]
    fn silence_after_phrase_end_does_not_eat_unreported_speech() {
        let mut c = LiveCore::new(R);
        speech(&mut c, 3.0);
        c.push_audio(&vec![0.0; n(0.15)], false); // the chunk that ends the phrase
        c.on_vad_segment(0.0, 3000.0);
        c.finish();
        let (_, samples, _) = commit(&mut c);
        assert_eq!(samples.len(), n(3.0));
    }

    #[test]
    fn zero_rate_does_not_hang() {
        let mut c = LiveCore::new(0);
        c.push_audio(&[0.5; 100], true);
        c.finish();
        assert!(c.is_done() || matches!(c.next_job(0, false, true), Some(Job::Commit { .. })));
    }

    #[test]
    fn vad_ms_convert_at_48khz() {
        let mut c = LiveCore::new(48_000);
        c.push_audio(&vec![0.5; 48_000 * 3], true);
        c.on_vad_segment(1000.0, 2500.0);
        c.finish();
        let Some(Job::Commit { samples, .. }) = c.next_job(0, false, true) else { panic!() };
        assert_eq!(samples.len(), 120_000);
    }

    #[test]
    fn reversed_vad_segment_does_not_lower_speech_secs() {
        let mut c = LiveCore::new(R);
        c.on_vad_segment(1000.0, 500.0);
        assert_eq!(c.speech_secs(), 0.0);
    }

    #[test]
    fn commits_run_one_at_a_time_in_order() {
        let mut c = decided(R);
        for _ in 0..3 {
            seg(&mut c, 1.0);
        }
        let (i, _, _) = commit(&mut c);
        assert_eq!(i, 1);
        assert!(!matches!(c.next_job(0, false, true), Some(Job::Commit { .. })));
        c.on_commit_done(1, Ok(" one ".into()), Some("en".into()));
        let (i, _, p) = commit(&mut c);
        assert_eq!((i, p.as_str()), (2, "zero one"));
        c.on_commit_done(2, Ok("two".into()), None);
        assert_eq!(c.transcript(), "zero one two");
    }

    #[test]
    fn stale_commit_result_is_ignored() {
        let mut c = decided(R);
        seg(&mut c, 1.0);
        commit(&mut c);
        c.on_commit_done(5, Ok("x".into()), None);
        assert_eq!(c.transcript(), "zero");
        c.on_commit_done(1, Ok("y".into()), None);
        assert_eq!(c.transcript(), "zero y");
    }

    #[test]
    fn prompt_clears_on_language_change() {
        let mut c = decided(R);
        for _ in 0..3 {
            seg(&mut c, 1.0);
        }
        commit(&mut c);
        c.on_commit_done(1, Ok("привет".into()), Some("ru".into()));
        commit(&mut c);
        c.on_commit_done(2, Ok("hello".into()), Some("en".into()));
        let (i, _, p) = commit(&mut c);
        assert_eq!((i, p.as_str()), (3, "hello"));
    }

    #[test]
    fn tail_counts_chars_not_bytes() {
        let s = "я".repeat(250);
        assert_eq!(tail_chars(&s, 200).chars().count(), 200);
        assert_eq!(tail_chars("abc", 200), "abc");
    }

    #[test]
    fn one_preview_pending_and_cadence() {
        let mut c = decided(R);
        speech(&mut c, 0.6);
        assert!(matches!(c.next_job(0, false, true), Some(Job::Preview { from, .. }) if from == n(3.0)));
        speech(&mut c, 0.3);
        assert_eq!(c.next_job(1000, false, true), None); // in flight
        c.on_preview_done(n(3.0), "a".into());
        assert_eq!(c.next_job(1500, false, true), None); // under 1 s since the last
        assert_eq!(c.next_job(2000, false, true), None); // 0.3 s growth
        speech(&mut c, 0.3);
        assert!(matches!(c.next_job(2000, false, true), Some(Job::Preview { .. })));
        assert_eq!(c.volatile(), "a");
    }

    #[test]
    fn first_preview_needs_one_and_a_half_seconds() {
        let mut c = LiveCore::new(R);
        speech(&mut c, 1.4);
        assert_eq!(c.next_job(0, false, true), None);
        speech(&mut c, 0.1);
        assert!(matches!(c.next_job(0, false, true), Some(Job::Preview { from: 0, .. })));
    }

    #[test]
    fn meeting_previews_every_3s_and_skip_when_engine_busy() {
        let mut c = decided(R);
        speech(&mut c, 1.0);
        assert!(matches!(c.next_job(0, true, true), Some(Job::Preview { .. })));
        c.on_preview_done(n(3.0), "a".into());
        speech(&mut c, 0.6);
        assert_eq!(c.next_job(1000, true, true), None);
        assert_eq!(c.next_job(3000, true, false), None);
        assert_eq!(c.stats().2, 1);
        assert_eq!(c.next_job(4000, true, true), None); // the skip restarted the 3 s wait
        assert!(matches!(c.next_job(6000, true, true), Some(Job::Preview { .. })));
    }

    #[test]
    fn preview_after_a_commit_moved_the_boundary_is_dropped() {
        let mut c = LiveCore::new(R);
        speech(&mut c, 1.6);
        assert!(matches!(c.next_job(0, false, true), Some(Job::Preview { from: 0, .. })));
        speech(&mut c, 1.5);
        c.on_vad_segment(0.0, 3100.0);
        c.on_preview_done(0, "x".into());
        assert_eq!(c.volatile(), "");
        // the in-flight flag was cleared: the next preview can run once audio grows
        speech(&mut c, 0.6);
        assert!(matches!(c.next_job(2000, false, true), Some(Job::Commit { .. })));
        assert!(matches!(c.next_job(2000, false, true), Some(Job::Preview { .. })));
    }

    #[test]
    fn cancel_ignores_late_results() {
        let mut c = LiveCore::new(R);
        seg(&mut c, 3.2);
        commit(&mut c);
        c.cancel();
        c.on_commit_done(0, Ok("late".into()), None);
        assert_eq!(c.transcript(), "");
        assert_eq!(c.next_job(5000, false, true), None);
        assert!(c.is_done());
    }

    #[test]
    fn failed_commit_marks_fallback_and_stops_commits() {
        let mut c = decided(R);
        seg(&mut c, 1.0);
        commit(&mut c);
        c.on_commit_done(1, Err(()), None);
        assert!(c.failed());
        seg(&mut c, 1.0);
        assert_eq!(c.next_job(9000, false, true), None);
        c.finish();
        assert!(c.is_done());
    }

    #[test]
    fn silence_only_makes_no_jobs_and_keeps_buffer_short() {
        let mut c = LiveCore::new(R);
        let mut max = 0;
        for i in 0..300u64 {
            c.push_audio(&vec![0.0; n(0.1)], false);
            max = max.max(c.buffered());
            assert_eq!(c.next_job(i * 100, false, true), None);
        }
        assert!(max <= n(2.1), "{max}");
        c.finish();
        assert!(c.is_done());
        assert_eq!(c.speech_secs(), 0.0);
        assert!(c.buffered() <= n(2.1));
    }

    #[test]
    fn held_audio_then_silence_stops_previews_and_keeps_buffer_short() {
        let mut c = LiveCore::new(R);
        seg(&mut c, 1.0); // held: under 3 s
        let mut late_previews = 0;
        let mut max = 0;
        for i in 0..1200u64 {
            c.push_audio(&vec![0.0; n(0.1)], false);
            max = max.max(c.buffered());
            if let Some(Job::Preview { from, .. }) = c.next_job(1000 + i * 100, false, true) {
                if i >= 20 {
                    late_previews += 1;
                }
                c.on_preview_done(from, String::new());
            }
        }
        assert_eq!(late_previews, 0);
        assert!(max <= n(1.0) + n(2.1), "{max}");
        c.finish();
        let (_, samples, _) = commit(&mut c);
        assert_eq!(samples.len(), n(1.0));
    }

    #[test]
    fn finish_includes_audio_after_the_last_drain() {
        let mut c = decided(R);
        let ct = c.committed_to;
        speech(&mut c, 2.0);
        speech(&mut c, 0.15);
        c.on_vad_segment(ct as f64 * 1000.0 / R as f64, (ct + n(2.15)) as f64 * 1000.0 / R as f64);
        c.finish();
        let (_, samples, _) = commit(&mut c);
        assert_eq!(samples.len(), n(2.15));
    }
}
