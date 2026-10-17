# Sentence timing measurement (Task 4 gate)

Decides whether line splitting ships enabled (`SPLIT_LINES`, Task 14).

**Result: gate FAILED (median 1.50 s, p90 4.84 s). The gate alone recommends `SPLIT_LINES = false`. The user then decided otherwise (see Decision).**

Gate (plan, Task 4): median ≤ 0.30 s **and** p90 ≤ 0.80 s of |token-time sentence end − DTW end|, over all sentence ends.

Recordings are labelled rec-A to rec-D: rec-A and rec-B are the two 1-2-1 calls, rec-C the German lesson, rec-D the meetup.

## Method

- Model: `ggml-large-v3-turbo.bin`, CPU only, `n_threads = 4`, Linux build.
- Audio: first 600 s of each of the 4 spike recordings, decoded by the app's own decoder (`audio::decoder::decode_audio_file`, symphonia) and converted by `to_whisper_format()`. This is the same 16 kHz mono path `audio/import.rs` uses.
- Chunks, as in `import.rs`: `get_speech_chunks` with redemption 2000 ms, segments over 25 s split by `split_segment_at_silence`, segments under 1600 samples skipped.
- Transcription: the engine's parameters (beam search, `no_timestamps`, `token_timestamps`, suppression, adaptive temperature, `threads = 4`), language policy with allowed languages `ru, de, en`, `pad_to_min_duration`, the `clean_repetitive_text` rule (a changed text keeps no sentences), `timed_tokens` (special tokens skipped) and `build_sentences` (cut rule unchanged).
- Reference: one pass per chunk with a DTW-enabled context (`DtwMode::Custom`, large-v3-turbo alignment heads `(2,4) (2,11) (3,3) (3,6) (3,11) (3,14)`, from `g_aheads_large_v3_turbo` in the vendored whisper.cpp). The same pass gives `t1` (the app's token end) and `t_dtw`. Adding DTW does not change `t0`/`t1` in whisper.cpp; `t_dtw` is written separately.
- Each sentence end is the token where the app's cut rule ends the sentence. The chunk's final unpunctuated remainder also counts as an end (reported separately).
- Compared values are the app's stored times: `clamp(t/100 − pad, 0, chunk_len)`, rounded to 2 decimals, for both `t1` and `t_dtw`.
- Statistics: median is the mean of the two middle values for an even n. p90 is nearest-rank.

Cross-check: the harness output matched the real `WhisperEngine::transcribe_audio_with_confidence` (text and sentences) on 2 chunks of rec-A and 3 chunks of rec-C, 0 mismatches.

## Results (10-minute excerpts, all 4 recordings completed; no 5-minute excerpts were needed)

| Recording | Chunks | Ends | Final-remainder ends | Median | p90 | Max | Punctuated only (n, median, p90) |
|---|---|---|---|---|---|---|---|
| rec-A (1-2-1 call) | 27 | 131 | 6 | 1.80 | 4.71 | 8.32 | 125, 1.96, 5.11 |
| rec-B (1-2-1 call) | 26 | 117 | 7 | 1.60 | 5.13 | 8.20 | 110, 1.70, 5.13 |
| rec-C (German lesson) | 30 | 98 | 3 | 1.32 | 4.98 | 10.29 | 95, 1.41, 4.98 |
| rec-D (meetup) | 28 | 31 | 21 | 0.62 | 2.38 | 5.67 | 10, 1.26, 2.46 |
| **Overall** | 111 | **377** | 37 | **1.50** | **4.84** | 10.29 | 340, 1.72, 5.06 |

Invalid token timing: 0 chunks. Unusable DTW times: 0 ends. Chunks whose text changed (no sentences kept): 0, 1, 5, 4 for rec-A to rec-D.

Signed difference (app end − DTW end), overall: median +1.42 s, interquartile range 0.32 to 3.15 s. 90% of ends have the app end later than DTW. The spread is wide, so this is not a constant offset that could be corrected.

Ratio (app end / DTW end) over the same 377 ends: median 1.12, IQR 1.02 to 1.30.

## Cause of the lag

With `no_timestamps = true` (`whisper_engine.rs` ~631), whisper.cpp ends the single segment at `seek + 30 s`. It forces `seek_delta = 100*WHISPER_CHUNK_SIZE` (`whisper.cpp` ~5939–5949, and the segment `t1 = seek + seek_delta` at ~6219). `whisper_exp_compute_token_level_timestamps` then spreads the token times over that 30 s window in proportion to each token's character length (`voice_length`). Token ends therefore run late relative to the audio. DTW aligns to the audio, so the gap is systematic.

Sensitivity check (not the gate): the whisper.cpp DTW code sets `t_dtw` when the alignment first reaches a token, so `t_dtw` is that token's start, not its end. The brief's reference is therefore the last token's start. As a variant, I also compared against the start of the following token (n=276, median 1.87 s, p90 4.69 s). It fails the gate too.

## Decision

User decision 2026-10-17: sentence times come from DTW (`t_dtw`) when speaker ID is enabled; Whisper then loads without flash attention, which whisper.cpp requires for DTW. Line splitting ships enabled.

The gate result above is kept as measured. It is the reason sentence times come from DTW rather than from the token end times.

Caveats:
- Chunk-final remainder ends agree much better (median 0.14 to 0.56 s by recording), but they are a small share of the ends.
- The allowed-language set `ru, de, en` was assumed for the policy. It was not checked against the user's own setting.
- The DTW heads come from the whisper-rs-sys 0.11.1 that whisper-rs 0.13.2 links (`Cargo.lock`). The brief named whisper-rs-sys 0.13.2, which does not exist in the lock file.
