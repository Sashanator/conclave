// DTW token times for sentence timing. whisper.cpp computes them only without flash attention
// (whisper.cpp:3660), so a context with DTW turns flash attention off. Only while speaker ID is enabled.

use crate::speakers::sentences::TimedToken;
use whisper_rs::{DtwAhead, DtwMode, DtwModelPreset};

/// large-v3-turbo alignment heads: `g_aheads_large_v3_turbo` in whisper.cpp (whisper-rs-sys 0.11.1, whisper.cpp:384).
const TURBO_AHEADS: [DtwAhead; 6] = [
    DtwAhead { n_text_layer: 2, n_head: 4 },
    DtwAhead { n_text_layer: 2, n_head: 11 },
    DtwAhead { n_text_layer: 3, n_head: 3 },
    DtwAhead { n_text_layer: 3, n_head: 6 },
    DtwAhead { n_text_layer: 3, n_head: 11 },
    DtwAhead { n_text_layer: 3, n_head: 14 },
];

/// What a loaded context was built with. Written only under the context write lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContextMode {
    pub speakers_enabled: bool,
    pub dtw: bool,
    pub flash_attn: bool,
}

/// DTW mode for a model name; None when the model has no alignment heads here.
pub fn dtw_mode_for_model(model_name: &str) -> Option<DtwMode<'static>> {
    // Any turbo name, quantised or not, shares the turbo alignment heads
    if model_name.contains("large-v3-turbo") {
        return Some(DtwMode::Custom { aheads: &TURBO_AHEADS });
    }
    let model_preset = match strip_quantisation(model_name) {
        "tiny" => DtwModelPreset::Tiny,
        "base" => DtwModelPreset::Base,
        "small" => DtwModelPreset::Small,
        "medium" => DtwModelPreset::Medium,
        "large-v3" => DtwModelPreset::LargeV3,
        _ => return None,
    };
    Some(DtwMode::ModelPreset { model_preset })
}

/// "base-q5_1" → "base": quantised files share their model's alignment preset.
fn strip_quantisation(model_name: &str) -> &str {
    match model_name.rsplit_once("-q") {
        Some((base, q)) if q.len() == 3 && q.as_bytes()[0].is_ascii_digit() && q.as_bytes()[1] == b'_' && q.as_bytes()[2].is_ascii_digit() => base,
        _ => model_name,
    }
}

/// (flash_attn, DTW mode) for a context. Flash attention is turned off only when DTW is on.
pub fn dtw_context_options(flash_attn: bool, speakers_enabled: bool, model_name: &str) -> (bool, DtwMode<'static>) {
    match dtw_mode_for_model(model_name).filter(|_| speakers_enabled) {
        Some(mode) => (false, mode),
        None => (flash_attn, DtwMode::None),
    }
}

/// Sentences come from DTW only: token-end times are too late to store (sentence_timing.md).
pub fn sentences_enabled(context: ContextMode, text_unchanged: bool) -> bool {
    context.dtw && text_unchanged
}

/// Text tokens with their DTW start times to sentence-timing tokens: each token ends where the next
/// text token starts, and the last one ends at `end_cs` (centiseconds of the padded chunk), or at its
/// own start when that start lies in the padding past the audio end.
/// A negative DTW time means DTW failed for the chunk, so there are no tokens.
pub fn dtw_timed_tokens(tokens: Vec<(Vec<u8>, i64)>, end_cs: i64) -> Vec<TimedToken> {
    if tokens.iter().any(|(_, t0)| *t0 < 0) {
        return Vec::new();
    }
    let next_starts: Vec<i64> = tokens.iter().skip(1).map(|(_, t0)| *t0).collect();
    tokens
        .into_iter()
        .enumerate()
        .map(|(i, (bytes, t0))| {
            let t1 = next_starts.get(i).copied().unwrap_or_else(|| end_cs.max(t0));
            TimedToken { bytes, t0, t1 }
        })
        .collect()
}

/// The job asking for a speaker-ID context change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Recording,
    Import,
    Retranscription,
}

/// A context change waits for every other job to finish: each job sees its own flag set, so only the others count.
pub fn should_apply(recording: bool, importing: bool, retranscribing: bool, caller: Job) -> bool {
    let others_running = match caller {
        Job::Recording => importing || retranscribing,
        Job::Import => recording || retranscribing,
        Job::Retranscription => recording || importing,
    };
    !others_running
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtw_mode_for_model_maps_turbo_to_custom_heads_and_others_to_presets() {
        match dtw_mode_for_model("large-v3-turbo") {
            Some(DtwMode::Custom { aheads }) => {
                let heads: Vec<(i32, i32)> = aheads.iter().map(|a| (a.n_text_layer, a.n_head)).collect();
                assert_eq!(heads, vec![(2, 4), (2, 11), (3, 3), (3, 6), (3, 11), (3, 14)]);
            }
            other => panic!("turbo should use custom heads, got {other:?}"),
        }
        assert!(matches!(dtw_mode_for_model("large-v3-turbo-q5_0"), Some(DtwMode::Custom { .. })));
        assert!(matches!(
            dtw_mode_for_model("large-v3"),
            Some(DtwMode::ModelPreset { model_preset: DtwModelPreset::LargeV3 })
        ));
        assert!(matches!(
            dtw_mode_for_model("small"),
            Some(DtwMode::ModelPreset { model_preset: DtwModelPreset::Small })
        ));
        assert!(dtw_mode_for_model("unknown").is_none());
    }

    #[test]
    fn dtw_mode_for_model_maps_quantised_presets() {
        for name in ["tiny-q5_1", "base-q5_1", "small-q5_1", "medium-q5_0", "large-v3-q5_0"] {
            assert!(
                matches!(dtw_mode_for_model(name), Some(DtwMode::ModelPreset { .. })),
                "{name} should get its preset"
            );
        }
        assert!(dtw_mode_for_model("unknown-q5_1").is_none());
    }

    #[test]
    fn should_apply_only_when_no_other_job_runs() {
        // Each job sees its own flag as set, so only the other two matter
        assert!(should_apply(false, false, false, Job::Import));
        assert!(!should_apply(true, false, false, Job::Import));
        assert!(!should_apply(false, false, true, Job::Import));
        assert!(should_apply(false, true, false, Job::Import));
        assert!(!should_apply(true, true, false, Job::Recording));
        assert!(!should_apply(false, true, true, Job::Retranscription));
        assert!(should_apply(true, false, false, Job::Recording));
        assert!(should_apply(false, false, false, Job::Recording));
    }

    #[test]
    fn context_params_disable_flash_attn_only_with_dtw() {
        // Speaker ID off: today's options, flash attention kept, no DTW
        assert!(matches!(dtw_context_options(true, false, "large-v3-turbo"), (true, DtwMode::None)));
        assert!(matches!(dtw_context_options(false, false, "large-v3-turbo"), (false, DtwMode::None)));
        // Speaker ID on with a DTW model: flash attention off
        let (flash_attn, mode) = dtw_context_options(true, true, "large-v3-turbo");
        assert!(!flash_attn);
        assert!(matches!(mode, DtwMode::Custom { .. }));
        // Speaker ID on, no DTW for the model: nothing changes
        assert!(matches!(dtw_context_options(true, true, "unknown"), (true, DtwMode::None)));
    }

    #[test]
    fn dtw_tokens_to_timed_tokens() {
        let tokens = vec![(b" Hello".to_vec(), 10), (b" world.".to_vec(), 40), (b" Bye".to_vec(), 70)];
        let timed = dtw_timed_tokens(tokens, 120);
        let spans: Vec<(i64, i64)> = timed.iter().map(|t| (t.t0, t.t1)).collect();
        assert_eq!(spans, vec![(10, 40), (40, 70), (70, 120)]);
        assert_eq!(timed[1].bytes, b" world.".to_vec());
        // A negative t_dtw means DTW failed for the chunk: no tokens, whichever token it is on
        assert!(dtw_timed_tokens(vec![(b"a".to_vec(), -1), (b"b".to_vec(), 5)], 10).is_empty());
        assert!(dtw_timed_tokens(vec![(b"a".to_vec(), 0), (b"b".to_vec(), -1)], 10).is_empty());
        assert!(dtw_timed_tokens(vec![], 10).is_empty());
    }

    #[test]
    fn dtw_last_token_never_ends_before_it_starts() {
        // A sub-1 s chunk: the last token starts inside the padding, past the audio end
        let timed = dtw_timed_tokens(vec![(b" hi.".to_vec(), 130)], 120);
        assert_eq!((timed[0].t0, timed[0].t1), (130, 130));
        let timed = dtw_timed_tokens(vec![(b" a".to_vec(), 0), (b" b.".to_vec(), 130)], 120);
        let spans: Vec<(i64, i64)> = timed.iter().map(|t| (t.t0, t.t1)).collect();
        assert_eq!(spans, vec![(0, 130), (130, 130)]);
    }

    #[test]
    fn sentences_need_dtw_and_unchanged_text() {
        let off = ContextMode { speakers_enabled: false, dtw: false, flash_attn: true };
        let on = ContextMode { speakers_enabled: true, dtw: true, flash_attn: false };
        assert!(!sentences_enabled(off, true));
        assert!(sentences_enabled(on, true));
        assert!(!sentences_enabled(on, false));
    }
}
