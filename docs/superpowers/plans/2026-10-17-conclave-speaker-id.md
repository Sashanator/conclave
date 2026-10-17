# Conclave Speaker Identification (v0.6.0) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Label every transcript line with a speaker (You / known names / Speaker n), remember people by voice across meetings, split two-speaker lines at sentence ends, and pass names into the summary. Everything runs locally on DirectML with a CPU fallback.

**Architecture:**
- **Live:** during the call, the mixer records a 50 ms mic/system level track, which labels live lines You/Others.
- **After stop:** a single app-wide background job runs a Rust port of sherpa-onnx 1.13.8's pyannote-segmentation + TitaNet diarization on the saved `audio.mp4`, then centroid merge. It matches the clusters to "You" and to known voiceprints, and publishes speakers, line labels and splits in one token-checked transaction.
- **UI:** the meeting page shows a Speakers bar for naming (which teaches voices), a per-line move menu, and an Identify button. The automatic summary waits for the job.

**Tech Stack:** Rust (Tauri 2, sqlx/SQLite, `ort` 2.0.0-rc.10 `load-dynamic`, `ndarray` 0.16, `realfft`, `rubato` 0.15, `symphonia`, `sha2`, `reqwest` 0.11, whisper-rs 0.13.2), ONNX Runtime 1.22.0 DirectML build + `DirectML.dll` 1.15.4, Next.js 14 + React 18 + TypeScript, Bun tests (happy-dom), Python 3 + sherpa-onnx 1.13.8 for local parity references only.

**Spec:** `docs/superpowers/specs/2026-10-15-conclave-speaker-id-design.md`. Read it before any task. Section numbers below (§n) refer to it.

**Deviations from the spec (same behaviour, simpler or safer):**
- `levels.bin` is kept in memory during the call and written next to `audio.mp4` at stop. It is not appended live: the pipeline doesn't know the meeting folder, and the file is only ~150 KB/h. If the app crashes mid-call, there is no level track, and "You" is then found through the self voiceprints.
- The temporary 16 kHz file lives in `<app_data>/speakers-tmp/`, not in the meeting folder. So the Phase 4 folder guard and its delete list don't need to know about it, and it is cleared at startup.
- Speaker SQL lives in `database/repositories/speakers.rs`, the codebase's repository pattern. Clustering and merge live in `speakers/cluster.rs`. There is no `voices.rs`: matching is in `assign.rs`, and learning, pruning and rename/delete/merge are SQL.
- The release is v0.6.0, because v0.5.0 (Phase 4) shipped after the spec was written.

**Changes after the Codex + Antigravity plan review (2026-10-17):**
- Every terminal job state emits `speakers-updated`, and the summary gate stays closed until the first speaker lookup returns.
- `finish_without_publish` is token-checked, and replacing transcripts invalidates a running job even with the feature off.
- Sentences may not overlap and are re-validated on save.
- Recording-history reloads keep speaker fields.
- Streaming decode keeps exact timing (resampler delay, flush, trim) and deletes partial temp files.
- Features (Task 9) now come before the runtime (Task 10).
- Split rows get a correct `duration`.
- The installer size gate runs before publishing.
- `DirectML.dll` is preloaded by absolute path, and the Windows test initialises ORT from the staged DLL.
- DTW reference uses custom turbo alignment heads.
- One finding was rejected: TitaNet frames are *not* padded to 16 (sherpa pads only its buffer; the tensor shape is unpadded).

## Global Constraints

- **Start condition:** PR #8 (`fix/transcript-quality`) is merged into `main`, and `speaker-id` is rebased onto it (Task 0). The plan targets that code: `transcribe_audio_with_confidence(audio, language, policy: Option<&mut LanguagePolicy>)` and `pad_to_min_duration` in `whisper_engine/language_policy.rs`.
- Windows 10 x64 is the target. Development and tests run on Linux (AGENTS.md "Verified on Linux"). Windows CI runs only at the Phase 6 checkpoint, by manual dispatch.
- **Local builds:**
  - `export CARGO_TARGET_DIR=/workspace/target`; `nice -n 15 cargo test -j 4 ...`. Never two cargo builds at once.
  - Rust tests: `cargo test -p meetily --lib speakers::` (plus the module under test).
  - Full check before each phase review: `cargo test --workspace --exclude llama-helper`, and `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`.
- **Feature flag:** a Tauri store file `speakers.json`, key `enabled` (bool, default false), cached in a process-wide `AtomicBool`. It is not in `RecordingPreferences`, because the Recording settings page saves that whole object and would reset an unknown field. The feature is **active** only when `enabled && models_ready()`. When it is not active: no level track, no `speaker_idx`, no `speaker_jobs` row, no waiting, and a byte-identical summary payload.
- **Models**, in `<app_data>/models/speakers/`:

  | File | URL | Bytes | SHA-256 |
  |---|---|---|---|
  | `segmentation-3.0.onnx` | `https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/9403a6902bb58e3d5ae8c7e77c3422de279db2e0/model.onnx` | 5,992,913 | `220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079` |
  | `titanet-large.onnx` | `https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/0743f301363dec56491a490f6d6cbc9d67f9a3bf/nemo_en_titanet_large.onnx` | 101,405,493 | `d51abcf31717ef28162f26acb9d44dd4127c3d44c9b8624f699f3425daca8e77` |

- **ONNX Runtime (build time):**

  | Package | URL | Bytes | SHA-256 |
  |---|---|---|---|
  | `Microsoft.ML.OnnxRuntime.DirectML` 1.22.0 | `https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.22.0/microsoft.ml.onnxruntime.directml.1.22.0.nupkg` | 17,898,472 | `29f9872d786236b79aa83f94482f3a17c14297e4833768d6d0ed4883ee732e60` |
  | `Microsoft.AI.DirectML` 1.15.4 | `https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg` | 202,292,617 | `4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9` |

  Staged files:

  | Archive path | Bytes | SHA-256 |
  |---|---|---|
  | `runtimes/win-x64/native/onnxruntime.dll` | 16,471,584 | `95366724919f4e95ecc60010912ed538ad9804b6683fbd0aad389749102834b9` |
  | `runtimes/win-x64/native/onnxruntime_providers_shared.dll` | 22,048 | `dea79756b1ef0deb317115aa5da45eca8946eafcf1be2dbd9fa3b309551faae5` |
  | `bin/x64-win/DirectML.dll` | 18,527,776 | `9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1` |

- **Thresholds** (spec §8.2, exact):

  | Rule | Value |
  |---|---|
  | Clustering | complete linkage on `1 − cosine`, cut at distance 0.7 |
  | Anchors | ≥ 30 s of speech |
  | Centroids | from 3 s sub-windows of segments ≥ 1.5 s, at most 60 per cluster, evenly spaced |
  | Anchor merge | cosine ≥ 0.7 |
  | Small clusters | 3 s ≤ seconds < 30 s; attach to the nearest anchor at cosine ≥ 0.65 |
  | Known voice and self-by-voiceprint | max-cosine ≥ 0.75 |
  | Duration filter | `min_duration_on` 0.3 s, `min_duration_off` 0.5 s |
  | Self by levels | at least half of the cluster's time is mic-only |
  | Self learning | 3 s sub-windows ≥ 90 % mic-only, total ≥ 20 s |
  | Naming learns | speaker has ≥ 20 s |
  | Voiceprints | 20 newest per person |
  | Line split | sentence overlap ≥ 60 % and ≥ 1 s with a speaker different from its neighbours |
  | Embedding dimension | 192 |
  | Clustering cap | `MAX_EMBEDDINGS = 15_000` (f32 condensed matrix ≈ 0.45 GB) |

- **Level track:**
  - Slices are 50 ms; one 600 ms mix window gives exactly 12 `(mic_rms, sys_rms)` pairs.
  - Levels in dB: `20·log10(max(rms, 1e-5))`.
  - Each channel's floor is the 10th percentile of its dB over the trailing 600 slices (30 s), including the current slice.
  - **Mic-only:** `mic_db ≥ mic_floor + 12` **and** `sys_db ≤ sys_floor + 6`.
  - Live label: **You** when > 50 % of the line's slices are mic-only, else **Others**.
  - File header: magic `b"CLVL"`, `u16` version 1, `u16` slice_ms 50, `u32` sample_rate 48000, then little-endian `f32` pairs.
- **Sentences:**
  - Cut after a token whose trimmed bytes end in `.`, `?`, `!` or `…`.
  - Times are seconds with 2 decimals.
  - JSON is `[[t0, t1, "text"], ...]`, in meeting seconds.
- **Job statuses:** `pending | running | done | failed | skipped`.
  - Skip reasons: `feature_off`, `models_missing`, `no_recording`.
  - Backends: `directml | cpu | mixed`.
- **Events:**
  - `speakers-progress` `{ meeting_id, percent }`
  - `speakers-updated` `{ meeting_id }`
  - `speaker-models-progress` `{ percent, downloaded_bytes, total_bytes }`
  - `speaker-models-done` `{ ok, error }`
  - `speaker-settings-changed` `{}`
- **UI copy (exact):**
  - "Identify speakers"
  - "Re-identify"
  - "Identifying speakers… {n}%"
  - "Waiting for speaker analysis…"
  - "Summarize now"
  - "Speaker names changed. Regenerate the summary to use them"
  - "Speaker analysis failed: {reason}. Retry"
  - "No recording saved, so speakers weren't identified"
  - "Speaker labels will be rebuilt afterwards."
  - "Voiceprints are removed; past transcripts keep the name"
  - "this is you"
  - "Others"
  - "Unknown"
  - "You"
  - "Speaker {n}"
  - "New person…"
  - "Last analysis: {Backend} · {run} for a {audio} meeting"
- **Privacy:** no voiceprint, centroid or level data ever leaves the PC. Only display names go into the summary prompt.
- **Git hygiene:**
  - New outbound hosts: only the two Hugging Face URLs at runtime, and `api.nuget.org` at build time. Both are listed in `docs/network-calls.md`.
  - Never commit `.claude/settings.json`, `.devcontainer/*` or anything under `.superpowers/`. Recordings and parity fixtures stay in `.superpowers/` (gitignored).
  - Stage files by name; never `git commit -a`.
  - Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Reviews (CLAUDE.md):**
  - Every task: a `reviewer` review.
  - Tasks touching persistence, deletion, concurrency/restart or untrusted input (marked **[Codex]**) get a Codex pass.
  - Each phase end: reviewer + Codex.
  - Whole branch: reviewer + Codex + Antigravity, plus one recheck after fixes.

## Review Focus

1. **A line whose sentence timing is unusable must never break saving or analysis.** This covers:
   - text changed by `clean_repetitive_text`;
   - Cyrillic characters split across tokens;
   - padded sub-1 s chunks;
   - unordered or out-of-range token times;
   - a hand-edited JSON payload from the webview.

   Each such line is stored without sentences and later gets its majority speaker. Tests: Task 2 (`sentences_*`), Task 3 (`save_transcript_drops_bad_sentences_and_speaker_idx`).
2. **An interrupted or overtaken job must leave no stale or orphan data.** Cases: the app closes mid-analysis; the meeting is deleted mid-job; Re-transcribe or Re-identify runs mid-job. Expected: publishing does nothing, and an interrupted job re-runs at the next start. Tests: Task 15 (`publish_*`, `recover_*`) and Task 16 (`job_discards_result_when_token_changed`).
3. **DirectML missing or failing mid-run** (an old driver, no DX12, a device reset) must give a finished CPU analysis with the real backend recorded. A silent CPU fallback must never be recorded as `directml`. Tests: Task 10 (`backend_label_combines_backends`), Task 16 (`job_falls_back_to_cpu_and_records_mixed`) and Task 23 (`windows_e2e`).
4. **Feature off means today's app exactly:**
   - no `levels.bin`;
   - no `speaker_idx`;
   - no job row;
   - no waiting;
   - a byte-identical summary payload;
   - a byte-identical Copy text.

   Tests: Task 6 (`inactive_feature_writes_no_levels`), Task 15 (`save_without_feature_creates_no_job`), Task 21 (`summary payload is byte-identical when the feature is off`).
5. **Long, silent or one-person recordings must stay bounded and finish `done`.** A 3-hour meeting has bounded RAM: streaming decode plus the embedding cap. A silent file, a single window and a single embedding all finish `done`. Tests: Task 11 (`window_step_keeps_embeddings_under_cap`), Task 12 (`diarize_silence_returns_no_turns`, `diarize_single_window_skips_clustering`), Task 13 (`decode_streams_without_loading_whole_file`).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `frontend/src-tauri/migrations/20261017000000_speakers.sql` (new) | `people`, `meeting_speakers`, `voiceprints`, `speaker_jobs`, `transcripts.speaker_idx`, `transcripts.sentences` | 1 |
| `database/models.rs`, `database/repositories/{meeting,transcript}.rs`, `api/api.rs` | New columns read/written, loaders ordered, explicit deletes | 1, 3 |
| `speakers/mod.rs` (new), `lib.rs` | Module root, command registration, startup | 2, 16 |
| `speakers/sentences.rs` (new) | Timed sentences from Whisper tokens, validation, meeting-time offset | 2 |
| `whisper_engine/{whisper_engine,language_policy}.rs` | Return sentences; padding offset | 2 |
| `audio/transcription/worker.rs`, `audio/{import,retranscription,common}.rs` | Carry sentences (and live label) to all writers | 3, 6 |
| `frontend/src/types/index.ts`, `contexts/TranscriptContext.tsx` | `speaker_idx`, `sentences` on live lines | 3 |
| `speakers/levels.rs` (new) | Level slices, floors, mic-only flags, live store, file format | 5 |
| `speakers/settings.rs` (new), `audio/pipeline.rs`, `audio/recording_saver.rs`, `audio/meeting_folder.rs` | Setting, feed levels, write `levels.bin`, delete list | 6 |
| `scripts/speaker-parity/` (new: `README.md`, `make_reference.py`, `requirements.txt`) | Local reference outputs from sherpa-onnx 1.13.8 | 7 |
| `speakers/models.rs` (new) | Model catalog, SHA-256 resumable download, readiness | 8 |
| `speakers/runtime.rs` (new) | ORT sessions, DirectML/CPU, explicit backend, fallback | 10 |
| `speakers/features.rs` (new) | NeMo log-mel features (sherpa port) | 9 |
| `speakers/diarize.rs` (new) | Segmentation, powerset, overlap exclusion, embeddings, reconstruction (sherpa port) | 12 |
| `speakers/cluster.rs` (new) | Complete linkage, window-step cap, centroid merge helpers | 11 |
| `audio/decoder.rs`, `speakers/temp_audio.rs` (new) | Streaming decode to a 16 kHz mono temp file; random-access reader | 13 |
| `speakers/assign.rs` (new) | Merge, self, known voices, numbering, line labels and splits | 14 |
| `database/repositories/speakers.rs` (new) | People, voiceprints, meeting speakers, jobs, publish | 15 |
| `speakers/job.rs`, `speakers/commands.rs` (new) | Queue worker, steps, fallback, events, Tauri commands, enqueue hooks | 16 |
| `frontend/src/lib/speakers.ts` (new), `contexts/ConfigContext.tsx` | Pure UI helpers, `speakerIdActive` | 17 |
| `frontend/src/components/SpeakerSettings.tsx` (new), `app/settings/page.tsx` | Settings → Speakers tab | 18 |
| `components/VirtualizedTranscriptView.tsx`, `app/_components/TranscriptPanel.tsx` | Live You/Others label | 19 |
| `frontend/src/hooks/useMeetingSpeakers.ts` (new), `components/MeetingDetails/{SpeakersBar,SpeakerLineMenu}.tsx` (new), `TranscriptPanel.tsx`, `TranscriptButtonGroup.tsx`, `RetranscribeDialog.tsx` | Meeting page speaker UI | 20 |
| `lib/meeting-notes.ts`, `hooks/meeting-details/{useSummaryGeneration,useCopyOperations}.ts`, `app/meeting-details/page-content.tsx`, `components/MeetingDetails/SummaryPanel.tsx` | Summary wait, names in prompt and Copy, names-changed hint | 21 |
| `build/onnxruntime.rs`, `.github/workflows/{release,ci}.yml`, `docs/network-calls.md` | DirectML runtime, installer size check, CI e2e | 22, 23 |

All Rust paths are under `frontend/src-tauri/src/` unless shown otherwise.

---

## Phase 1 — Data and timed sentences (no visible change)

### Task 0: Rebase onto main with PR #8

**Files:** none (git only).

- [ ] **Step 1:** Check that PR #8 is merged: `gh pr view 8 --json state` shows `"MERGED"`. If it isn't, stop and report `BLOCKED: PR #8 not merged`.
- [ ] **Step 2:** In `/workspace/.worktrees/speaker-id`, run `git fetch origin && git rebase origin/main`. Expected: the spec and plan commits replay cleanly (docs only).
- [ ] **Step 3:** Run the baseline. Expected: all pass.

  ```bash
  cd frontend && pnpm install --frozen-lockfile && pnpm exec tsc --noEmit && bun test && pnpm run build
  mkdir -p src-tauri/binaries && touch src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu
  cd .. && nice -n 15 cargo test --workspace --exclude llama-helper -j 4
  ```

  Record the test counts in the report.

### Task 1: Migration, model fields, loader order, explicit deletes **[Codex]**

**Files:**
- Create: `frontend/src-tauri/migrations/20261017000000_speakers.sql`
- Modify:
  - `database/models.rs:30-45` (`Transcript`)
  - `database/repositories/meeting.rs:77-81` (full loader), `:164-168` (paginated), `:244-291` (delete)
  - `api/api.rs:129-142` (`MeetingTranscript`), plus both mapping sites (`api.rs:~890`, `meeting.rs:~88`)
- Test: `database/repositories/meeting.rs` (tests module), `database/repositories/speakers.rs` is not created yet; use a `#[cfg(test)] mod migration_tests` in `meeting.rs`.

**Interfaces:**
- Produces:
  - `Transcript { ..., #[sqlx(default)] pub speaker_idx: Option<i64>, #[sqlx(default)] pub sentences: Option<String> }`
  - `MeetingTranscript { ..., #[serde(skip_serializing_if = "Option::is_none")] pub speaker_idx: Option<i64> }`
  - Tables exactly as spec §9.

- [ ] **Step 1: Write the failing tests** (in `meeting.rs` tests, using `crate::database::repositories::test_pool()`):

```rust
#[tokio::test] async fn migration_creates_self_person() {
    let pool = test_pool().await;
    let (id, is_self): (String, i64) = sqlx::query_as("SELECT id, is_self FROM people").fetch_one(&pool).await.unwrap();
    assert_eq!((id.as_str(), is_self), ("self", 1));
    // second self is rejected
    assert!(sqlx::query("INSERT INTO people (id,name,is_self,created_at) VALUES ('x','X',1,'t')").execute(&pool).await.is_err());
}
#[tokio::test] async fn loaders_order_by_start_then_id() { /* insert rows with start 2.0 (id b), 1.0 (id c), 1.0 (id a) → full and paginated loaders both return a, c, b */ }
#[tokio::test] async fn delete_meeting_removes_speaker_rows_and_keeps_voiceprints() {
    /* meeting M with meeting_speakers(M,1), speaker_jobs(M), voiceprint source M → after delete_meeting:
       meeting_speakers and speaker_jobs rows for M are 0; the voiceprint still exists with source_meeting_id NULL */
}
#[tokio::test] async fn pre_feature_rows_survive_migration() { /* rows inserted before running the new migration keep text/times; speaker_idx and sentences are NULL */ }
```

`pre_feature_rows_survive_migration` must not use `test_pool()`, which already runs every migration. Connect directly with `SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:")`. Then apply the migrations up to the previous one with `Migrator::new(Path::new("./migrations"))`, filtered to version < 20261017000000. Insert a row, then run the full migrator.

- [ ] **Step 2:** `cargo test -p meetily --lib repositories::meeting` → FAIL (table `people` does not exist).
- [ ] **Step 3: Implement.**
  - The migration SQL is spec §9, verbatim.
  - Add the model fields.
  - Both loaders use `ORDER BY audio_start_time ASC, id ASC`.
  - In `delete_meeting_with_transaction`, add `DELETE FROM meeting_speakers WHERE meeting_id = ?` and `DELETE FROM speaker_jobs WHERE meeting_id = ?` before deleting transcripts, plus `UPDATE voiceprints SET source_meeting_id = NULL WHERE source_meeting_id = ?`. Each gets the comment `// Explicit, not only the FK (spec §9)`.
  - Map `speaker_idx` in both `MeetingTranscript` constructions.
- [ ] **Step 4:** Run the same command → PASS. Then `cargo test -p meetily --lib database::` → PASS.
- [ ] **Step 5: Commit** `feat(db): speaker tables, transcript speaker columns, ordered loaders`.

### Task 2: Timed sentences from Whisper tokens **[Codex]**

**Files:**
- Create: `speakers/mod.rs` (`pub mod sentences;`), `speakers/sentences.rs`
- Modify:
  - `lib.rs` (`mod speakers;`)
  - `whisper_engine/language_policy.rs` (`pad_to_min_duration`)
  - `whisper_engine/whisper_engine.rs` (`transcribe_audio_with_confidence`, and the `pad_to_min_duration` call in `transcribe_audio`)
- Test: `speakers/sentences.rs`, `whisper_engine/language_policy.rs` tests.

**Interfaces:**
- Produces:

```rust
// speakers/sentences.rs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sentence(pub f64, pub f64, pub String);           // [t0, t1, text]
pub struct TimedToken { pub bytes: Vec<u8>, pub t0: i64, pub t1: i64 } // centiseconds in the padded chunk
pub fn build_sentences(tokens: &[TimedToken], pad_secs: f64, audio_secs: f64) -> Vec<Sentence>      // chunk-local seconds
pub fn to_meeting_time(local: &[Sentence], line_text: &str, start: f64, end: f64) -> Option<Vec<Sentence>>
pub fn is_valid(s: &[Sentence], line_text: &str, start: f64, end: f64) -> bool  // meeting-time sentences vs their line
pub fn parse_json(raw: &str) -> Option<Vec<Sentence>>          // None on any malformed entry (shape/finite numbers only)
// language_policy.rs
pub fn pad_to_min_duration(audio: Vec<f32>) -> (Vec<f32>, usize) // (audio, samples added before)
// whisper_engine.rs
pub async fn transcribe_audio_with_confidence(&self, audio_data: Vec<f32>, language: Option<String>, policy: Option<&mut LanguagePolicy>)
    -> Result<(String, f32, bool, Vec<Sentence>)>               // sentences chunk-local; empty when unusable
```

- [ ] **Step 1: Write the failing tests** in `sentences.rs`:

```rust
fn tok(s: &str, t0: i64, t1: i64) -> TimedToken { TimedToken { bytes: s.as_bytes().to_vec(), t0, t1 } }
#[test] fn sentences_cut_after_terminal_punctuation() {
    let t = [tok(" Hello", 0, 40), tok(" there.", 40, 90), tok(" Yes?", 120, 160), tok(" ok", 170, 200)];
    assert_eq!(build_sentences(&t, 0.0, 2.0), vec![
        Sentence(0.0, 0.9, "Hello there.".into()), Sentence(1.2, 1.6, "Yes?".into()), Sentence(1.7, 2.0, "ok".into())]);
}
#[test] fn sentences_join_cyrillic_bytes_split_across_tokens() {
    let b = "Привет.".as_bytes();
    let t = [TimedToken { bytes: b[..3].to_vec(), t0: 0, t1: 10 }, TimedToken { bytes: b[3..].to_vec(), t0: 10, t1: 50 }];
    assert_eq!(build_sentences(&t, 0.0, 1.0), vec![Sentence(0.0, 0.5, "Привет.".into())]);
}
#[test] fn sentences_remove_padding_and_clamp() {
    // 0.3 s of padding before the real audio of 0.5 s
    let t = [tok(" Да.", 20, 90)];
    assert_eq!(build_sentences(&t, 0.3, 0.5), vec![Sentence(0.0, 0.5, "Да.".into())]);
}
#[test] fn sentences_ellipsis_cuts() { /* "Ну…" then "Потом." → 2 sentences */ }
#[test] fn sentences_to_meeting_time_offsets_and_checks_text() {
    let local = vec![Sentence(0.0, 0.9, "Hello there.".into()), Sentence(1.2, 1.6, "Yes?".into())];
    assert_eq!(to_meeting_time(&local, "Hello there. Yes?", 100.0, 102.0).unwrap()[1], Sentence(101.2, 101.6, "Yes?".into()));
    assert!(to_meeting_time(&local, "Hello there. Yes? Yes?", 100.0, 102.0).is_none()); // text was cleaned/changed
}
#[test] fn sentences_reject_unordered_or_out_of_range() {
    let bad = vec![Sentence(1.0, 0.5, "a.".into())];
    assert!(to_meeting_time(&bad, "a.", 0.0, 2.0).is_none());
    let late = vec![Sentence(0.0, 5.0, "a.".into())];
    assert!(to_meeting_time(&late, "a.", 0.0, 2.0).is_none()); // beyond end + 0.2 s tolerance
}
#[test] fn sentences_reject_overlap() {
    let s = vec![Sentence(0.0, 1.0, "a.".into()), Sentence(0.8, 1.5, "b.".into())];
    assert!(!is_valid(&s, "a. b.", 0.0, 2.0));
    let ok = vec![Sentence(0.0, 1.0, "a.".into()), Sentence(1.0, 1.5, "b.".into())];
    assert!(is_valid(&ok, "a. b.", 0.0, 2.0));
}
#[test] fn sentences_parse_json_rejects_garbage() {
    assert_eq!(parse_json(r#"[[1.0,2.0,"a."]]"#).unwrap().len(), 1);
    for raw in [r#"[[1.0,"x","a"]]"#, r#"{"a":1}"#, r#"[[NaN,1,"a"]]"#, "not json"] { assert!(parse_json(raw).is_none()); }
}
```

In `language_policy.rs` tests: `pad_reports_offset_before_audio`. A 0.5 s input (8,000 samples) returns `(17_600 samples, 4_800)`. A 1.0 s input returns `(16_000, 0)`. Adapt PR #8's existing padding tests to the tuple.

- [ ] **Step 2:** `cargo test -p meetily --lib speakers::sentences language_policy` → FAIL.
- [ ] **Step 3: Implement.**

  **`build_sentences`:**
  - Concatenate token bytes. Cut after a token whose bytes, with trailing ASCII whitespace trimmed, end in `.`, `?`, `!` or the UTF-8 bytes of `…`.
  - Sentence text is `String::from_utf8_lossy(bytes).trim()`. Drop empty sentences.
  - After the last token, emit any remaining unpunctuated tokens as a final sentence.
  - `t0` is the first token's `t0`; `t1` is the last token's `t1`. Both are `/100.0 - pad_secs`, clamped to `[0, audio_secs]` and rounded to 2 decimals.

  **`to_meeting_time`:** add `start`, clamp to `[start, end]`, and return `Some` only if `is_valid` holds.

  **`is_valid`** (the one rule set, also used by save in Task 3 and by the job in Task 16):
  - Joining the sentence texts with `' '` equals the line text after collapsing whitespace.
  - Every `t0 ≤ t1`, each sentence's `t0 ≥` the previous sentence's `t1` (no overlap), and all times are within `[start − 0.2, end + 0.2]`.

  **`transcribe_audio_with_confidence`:**
  - Use `let (audio_data, pad_before) = pad_to_min_duration(audio_data)`.
  - After `state.full`, collect tokens per segment: `full_n_tokens`, `full_get_token_id`, `full_get_token_data` (`t0`, `t1`).
  - Skip ids `>= ctx.token_eot()`.
  - Get bytes via `ctx.token_to_cstr(id)?.to_bytes()`.
  - Call `build_sentences(&tokens, pad_before as f64 / 16000.0, original_len as f64 / 16000.0)`.
  - Return an empty Vec if `clean_repetitive_text` changed the text, or if token collection errors. Never fail the transcription because of sentences.
  - Update the 4 callers to destructure the 4-tuple, ignoring sentences for now (Task 3 wires them): `worker.rs`, `whisper_provider.rs`, `import.rs`, `retranscription.rs`.
  - `transcribe_audio` uses `.0` of the new pad return.
- [ ] **Step 4:** Run the same tests → PASS. Then `cargo test -p meetily --lib` → PASS.
- [ ] **Step 5: Commit** `feat(whisper): timed sentences from token timestamps`.

### Task 3: Carry sentences to all three transcript writers **[Codex]**

**Files:**
- Modify:
  - `audio/transcription/worker.rs` (`TranscriptUpdate`, `transcribe_chunk_with_provider` return)
  - `api/api.rs:183-196` (`TranscriptSegment` DTO)
  - `database/repositories/transcript.rs:53-64` (insert)
  - `audio/common.rs:51` (`create_transcript_segments`)
  - `audio/import.rs:550,607,724`
  - `audio/retranscription.rs:341,397,458`
  - `frontend/src/types/index.ts` (`Transcript`, `TranscriptUpdate`)
  - `frontend/src/contexts/TranscriptContext.tsx` (copy the new fields from the update into the stored line, `:315-353`; also in the history rebuild after a reload, `:402`)
  - `audio/recording_saver.rs:15-25` (`recording_saver::TranscriptSegment` gains `speaker_idx`, `sentences`, both `#[serde(default)]`) and `audio/recording_commands.rs:503-525,692-` (both history listeners copy them)
- Test: `database/repositories/transcript.rs` tests; `frontend/tests/contexts/` (new `transcript-speaker-fields.test.tsx`).

**Interfaces:**
- Consumes: `Sentence`, `to_meeting_time`, `parse_json` (Task 2).
- Produces:
  - `TranscriptUpdate { ..., #[serde(skip_serializing_if = "Option::is_none")] pub sentences: Option<Vec<Sentence>>, pub speaker_idx: Option<i64> }`. `speaker_idx` is always `None` until Task 6.
  - `api::TranscriptSegment { ..., #[serde(default, deserialize_with = "lenient_sentences")] pub sentences: Option<Vec<Sentence>>, #[serde(default, deserialize_with = "lenient_speaker_idx")] pub speaker_idx: Option<i64> }`
  - `create_transcript_segments(&[(String, f64, f64, Option<Vec<Sentence>>)])`
  - TS: `Transcript.speaker_idx?: number | null; Transcript.sentences?: [number, number, string][] | null`

- [ ] **Step 1: Write the failing tests:**

```rust
#[tokio::test] async fn save_transcript_stores_sentences_and_self_label() {
    // segment with sentences [[0.0,1.0,"Hi."]] and speaker_idx Some(0) → row has speaker_idx 0 and sentences JSON equal to the input
}
#[tokio::test] async fn save_transcript_drops_bad_sentences_and_speaker_idx() {
    // DTO parsed from JSON: sentences = [[1,"x","a"]] and speaker_idx = 7 → saved row has sentences NULL and speaker_idx NULL; the meeting still saves
}
```

```ts
// transcript-speaker-fields.test.tsx: a 'transcript-update' payload with speaker_idx 0 and sentences [[1,2,"Hi."]]
// ends up in useTranscripts().transcripts[0] with both fields unchanged; after the reload path rebuilds history
// from the backend (mock the history command with the same fields), both fields are still there
```

- [ ] **Step 2:** Run the Rust and Bun tests → FAIL.
- [ ] **Step 3: Implement.**
  - **Live:** `transcribe_chunk_with_provider` returns the sentences too. The worker calls `to_meeting_time(&s, &transcript, audio_start_time, audio_end_time)` and puts the result in `TranscriptUpdate.sentences`.
  - **Lenient deserializers:**
    - `lenient_sentences` accepts only a well-formed list (same shape rules as `parse_json`, applied to a `serde_json::Value`). Anything else becomes `None`.
    - `lenient_speaker_idx` accepts only `0`. A live save can only know "You"; anything else becomes `None`.
    - After deserializing a segment, `save_transcript` keeps its sentences only if `is_valid(&s, &text, start, end)`; otherwise it stores NULL.
  - **Insert** adds `speaker_idx, sentences` (JSON via `serde_json::to_string`).
  - **Import and retranscription:** convert each Whisper result with `to_meeting_time(&s, &text, start_ms/1000, end_ms/1000)` and store it. Parakeet gives `None`.
- [ ] **Step 4:** Tests PASS. Then the full Rust lib tests and `bun test` → PASS.
- [ ] **Step 5: Commit** `feat(transcripts): store timed sentences from recording, import and re-transcribe`.

### Task 4: Measure sentence-time accuracy (local gate, no product code)

**Files:** `scripts/speaker-parity/sentence_timing.md` (new, results only; no recordings and no text from them).

This task decides whether line splitting (Task 14, `SPLIT_LINES`) ships enabled.

- [ ] **Step 1: Build a test harness** in the scratchpad, not committed. It runs `WhisperEngine::transcribe_audio_with_confidence` on 10-minute excerpts of the 4 recordings in `/workspace/.superpowers/spike-audio/`.
  - Model: `ggml-large-v3-turbo` from `.superpowers/quality-debug/`.
  - Use the app's VAD chunks: run `audio::vad` over the 16 kHz excerpt, exactly as `import.rs` does.
- [ ] **Step 2: Build the reference.** Transcribe the same chunks with whisper-rs DTW token timestamps. whisper-rs 0.13.2 has no turbo preset (`DtwModelPreset` stops at `LargeV3`), so use `DtwMode::Custom` with large-v3-turbo's alignment heads, copied from `g_aheads_large_v3_turbo` in the whisper.cpp version vendored by `whisper-rs-sys` 0.13.2. If that version lacks it, take the heads from openai-whisper's `_ALIGNMENT_HEADS["large-v3-turbo"]` and note the source. Each sentence's reference end is `WhisperTokenData.t_dtw` of its last token (centiseconds).
- [ ] **Step 3: Record the results.** Report the median and p90 of |token-time end − DTW end| over all sentence ends, plus the counts.
  - **Gate:** median ≤ 0.30 s **and** p90 ≤ 0.80 s → `SPLIT_LINES = true`. Otherwise `false`, and the controller tells the user.
- [ ] **Step 4: Commit** `docs(speakers): sentence timing measurement` (the numbers only).

**Phase 1 end:** reviewer + Codex review of the phase range.

---

## Phase 2 — Level track and live labels

### Task 5: Level slices, floors and the mic-only rule

**Files:**
- Create: `speakers/levels.rs`
- Modify: `speakers/mod.rs`
- Test: `speakers/levels.rs`

**Interfaces:**
- Produces:

```rust
pub const SLICE_SECS: f64 = 0.05;
pub const SLICES_PER_WINDOW: usize = 12;
pub type LevelPair = (f32, f32);                                   // (mic_rms, sys_rms)
pub fn window_levels(mic: &[f32], sys: &[f32]) -> [LevelPair; SLICES_PER_WINDOW]
pub struct FloorTracker { /* trailing 600-slice windows per channel */ }
impl FloorTracker { pub fn new() -> Self; pub fn push(&mut self, p: LevelPair) -> bool /* mic-only */ }
pub fn mic_only_flags(pairs: &[LevelPair]) -> Vec<bool>             // == pushing every pair through one FloorTracker
pub fn mic_only_fraction(flags: &[bool], start: f64, end: f64) -> Option<f64>
pub fn encode(pairs: &[LevelPair]) -> Vec<u8>
pub fn decode(bytes: &[u8]) -> Result<Vec<LevelPair>, String>
// live store (process-global, one recording at a time)
pub fn live_reset(active: bool)
pub fn live_push(window: [LevelPair; SLICES_PER_WINDOW])          // no-op when inactive
pub fn live_label(start: f64, end: f64) -> Option<i64>             // Some(0) = You, None = Others/inactive
pub fn live_is_active() -> bool
pub fn live_take() -> Option<Vec<LevelPair>>                        // pairs if active; resets to inactive
```

- [ ] **Step 1: Write the failing tests:**
  - `window_levels_splits_600ms_into_12_rms_pairs`: a window of 28,800 samples where slice k has constant amplitude k/12 → `mic_rms[k] ≈ k/12`. A shorter window yields RMS over its available samples, zero-padded.
  - `mic_only_needs_mic_active_and_system_quiet`: 600 quiet slices (mic 0.001, sys 0.001), then mic 0.1 + sys 0.001 → true; mic 0.1 + sys 0.1 → false (echo/overlap counts as Others).
  - `floors_adapt_to_constant_system_hum`: sys a constant 0.05 for 600 slices, then mic 0.1 → true (the hum is the floor).
  - `flags_equal_live_tracker`: random pairs; `mic_only_flags` equals the per-push results.
  - `fraction_maps_seconds_to_slices`: slices [0, 0.05) …; `mic_only_fraction(flags, 0.0, 0.1)` uses slices 0–1. An empty range → `None`.
  - `encode_decode_roundtrip_and_rejects_bad_header`: the header bytes are exactly `CLVL`, `1u16`, `50u16`, `48000u32`. A truncated or odd body → `Err`.
  - `live_store_labels_and_resets`: `live_reset(true)`, push windows, `live_label` → `Some(0)` over a mic-only range. `live_take()` returns the pairs, then `live_is_active()` is false.
- [ ] **Step 2:** `cargo test -p meetily --lib speakers::levels` → FAIL.
- [ ] **Step 3: Implement** with the Global Constraints values.
  - `live_*` uses a `static LIVE: LazyLock<Mutex<Option<Live>>>`, where `Live { pairs, flags, tracker }`.
  - The percentile uses a sorted copy of the trailing window: 600 elements, 12 pushes every 600 ms.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(speakers): level slices and mic-only rule`.

### Task 6: Setting, pipeline feed, live label, `levels.bin` at stop **[Codex]**

**Files:**
- Create: `speakers/settings.rs`
- Modify:
  - `lib.rs` setup (call `speakers::settings::load_at_startup(app)` before the audio system can start)
  - `audio/pipeline.rs:849-851` (after `extract_window`)
  - `audio/pipeline.rs` constructor (`AudioPipeline::new` calls `levels::live_reset(crate::speakers::is_active())`)
  - `audio/transcription/worker.rs` (`speaker_idx`)
  - `audio/recording_saver.rs` (write at stop, next to the merged `audio.mp4`)
  - `audio/meeting_folder.rs:17` (`MEETING_FILES`)
  - `speakers/mod.rs`
- Test: `speakers/levels.rs`, `audio/recording_saver.rs`, `audio/meeting_folder.rs` tests.

**Interfaces:**
- Consumes: Task 5 live API.
- Produces:
  - `speakers::settings::{load_at_startup<R: Runtime>(app: &AppHandle<R>), save_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String>, is_enabled() -> bool}`. These use the store `speakers.json` and key `enabled`, plus a `static ENABLED: AtomicBool`.
  - `speakers::is_active() -> bool`, which is `settings::is_enabled() && models::models_ready()`. Until Task 8 exists, use a stub `models_ready() -> bool { false }` in `speakers/models.rs`.
  - `speakers::levels::write_levels_file(folder: &Path, pairs: &[LevelPair]) -> std::io::Result<()>`, which writes `.levels.bin.tmp` and then renames it to `levels.bin`.
  - `MEETING_FILES: [&str; 6]` gains `"levels.bin"` and `".levels.bin.tmp"`.

- [ ] **Step 1: Write the failing tests:**
  - `inactive_feature_writes_no_levels`: `live_reset(false)`; push; `live_take()` is `None`, and `write` is never called (saver helper `finish_levels(folder)` returns `Ok(false)`, no file).
  - `active_feature_writes_levels_file`: `finish_levels` writes `levels.bin`, which decodes to the pushed pairs, and leaves no `.tmp`.
  - `meeting_delete_accepts_levels_file`: the Phase 4 `delete_meeting_folder` test fixture plus a `levels.bin` → the folder is removed (fake remover).
  - `speaker_setting_defaults_off`: with no `speakers.json` value, `is_enabled()` is false. Test the pure `parse_enabled(Option<serde_json::Value>) -> bool`: `None`, `"yes"` and `1` → false; `true` → true.
  - The worker logs `speakers: mic-only fraction {:.2} for {start:.1}-{end:.1}s` at `debug` for each labelled line, so Task 24 can recalibrate the margins.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement.**
  - **Pipeline:** `levels::live_push(levels::window_levels(&mic_window, &sys_window))` right after `extract_window`, before mixing.
  - **Worker:** `speaker_idx: levels::live_label(audio_start_time, audio_end_time)`.
  - **Saver:** after the merged audio is written, add `finish_levels(&meeting_folder)` → `if let Some(p) = live_take() { write_levels_file(..) }`. Write errors are logged, never fatal.
  - **Settings:** `load_at_startup` reads the store once (missing or invalid → false).
- [ ] **Step 4:** PASS, plus `cargo test -p meetily --lib audio::` → PASS.
- [ ] **Step 5: Commit** `feat(speakers): level track during recording and live You/Others label`.

**Phase 2 end:** reviewer + Codex.

---

## Phase 3 — Analysis engine

### Task 7: Parity reference kit (local only)

**Files:**
- Create: `scripts/speaker-parity/README.md`, `scripts/speaker-parity/requirements.txt` (`sherpa-onnx==1.13.8`, `numpy`, `soundfile`), `scripts/speaker-parity/make_reference.py`
- Outputs (never committed): `/workspace/.superpowers/speaker-parity/<name>/`

**Interfaces:**
- Produces, per recording (`1-2-1_Ira`, `1-2-1_Slava`, `german-lesson`, `meetup`), files the ignored Rust tests read:
  - `audio16k.f32`: 16 kHz mono, little-endian `f32`, decoded with `ffmpeg -ac 1 -ar 16000 -f f32le`.
  - `diar_step1.json`, `diar_step2.json`: `[[start, end, cluster], ...]`. From sherpa `OfflineSpeakerDiarization`, segmentation `window_shift_ratio` 0.1 / 0.2, clustering threshold 0.7, `min_duration_on` 0.3, `min_duration_off` 0.5.
  - `clips.json`: 20 fixed clips per recording, `[[start, end], ...]` (1.5–10 s, evenly spaced over speech). Plus `clip_emb.npy`: TitaNet embeddings from sherpa `SpeakerEmbeddingExtractor`, shape `[20, 192]`, little-endian `f32`.
  - `merged.json`: `{ "speakers": n, "seconds": [...] }` after the spec §8.2 steps 6–7 merge. Implemented in Python with the Global Constraints values, so the Rust merge can be compared.

- [ ] **Step 1: Create the venv and models.**

  ```bash
  python3 -m venv /workspace/.superpowers/speaker-parity/venv && /workspace/.superpowers/speaker-parity/venv/bin/pip install -r scripts/speaker-parity/requirements.txt
  ```

  Download the two models to `.superpowers/speaker-parity/models/` from the pinned URLs and check their SHA-256.
- [ ] **Step 2:** Write `make_reference.py`, taking args `--audio-dir --models --out`. It must run with `python -I`.
- [ ] **Step 3: Run it.** Expected `merged.json` speaker counts (the spike results): `1-2-1_Ira` 2, `1-2-1_Slava` 2, `german-lesson` 2, `meetup` ≥ 3, with 3 speakers ≥ 30 s. If they differ, stop and report.
- [ ] **Step 4: Commit** the scripts and README only: `chore(speakers): local parity reference kit`.

### Task 8: Model download with SHA-256 **[Codex]**

**Files:**
- Modify: `speakers/models.rs` (replace the Task 6 stub)
- Modify: `docs/network-calls.md` (a row under huggingface.co naming `speakers/models.rs`)
- Test: `speakers/models.rs`

**Interfaces:**
- Produces:

```rust
pub struct ModelFile { pub name: &'static str, pub url: &'static str, pub bytes: u64, pub sha256: &'static str }
pub const MODELS: [ModelFile; 2];                              // Global Constraints table
pub fn models_dir() -> Option<PathBuf>                         // <app_data>/models/speakers, set at startup via set_models_dir
pub fn set_models_dir(dir: PathBuf)
pub fn models_ready() -> bool                                  // both files exist with exact size AND verified hash (cached per (path,len,mtime))
pub async fn download_models(client: &reqwest::Client, dir: &Path, progress: impl FnMut(u64, u64)) -> Result<(), String>
pub fn delete_models(dir: &Path) -> Result<(), String>         // only the two known names and their .part files
```

- [ ] **Step 1: Write the failing tests.** They use a local `tokio::net::TcpListener` HTTP/1.1 stub that serves bytes and honours `Range`.
  - `download_verifies_hash_and_renames_part`: after download, the final file exists and there is no `.part`.
  - `download_resumes_from_part_with_range`: a pre-existing `.part` with the first half → the stub sees `Range: bytes=N-` and the result is correct.
  - `download_rejects_wrong_hash_and_deletes_part`: the stub serves altered bytes → `Err`, and no file and no `.part` remain.
  - `download_rejects_oversize_stream`: more bytes than expected → `Err`.
  - `models_ready_false_on_size_or_hash_mismatch`.
  - `delete_removes_only_known_files`: an unrelated file in the dir survives.

  Make URLs, bytes and hashes injectable for tests: `download_one(client, &ModelFile, dir, progress)` is pub(crate), and the tests pass a test `ModelFile`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement** following Parakeet's resume pattern (`parakeet_engine.rs:886-1045`).
  - Validate 206 `Content-Range`.
  - Abort when the size exceeds the pinned size.
  - Hash with `sha2::Sha256` (add `sha2 = "0.10"` to `[dependencies]`).
  - Rename on success.
  - Never follow a redirect to a non-https URL (`reqwest` default policy plus a scheme check on the final URL).
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(speakers): verified model download`.

### Task 9: NeMo features (port)

**Files:**
- Create: `speakers/features.rs`
- Test: `speakers/features.rs`

**Source to port:** sherpa-onnx **v1.13.8**: `sherpa-onnx/csrc/speaker-embedding-extractor-nemo-impl.h`, `features.h` / `features.cc`, plus the kaldi-native-fbank version pinned by that tag's `cmake/kaldi-native-fbank.cmake`. Download into the scratchpad from `https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/...`.

**Interfaces:**
- Produces:

```rust
pub struct NemoMeta { pub feat_dim: usize, pub sample_rate: usize, pub window_size_ms: f32, pub window_stride_ms: f32, pub window_type: String, pub normalize_type: String }
pub fn read_nemo_meta(session: &ort::session::Session) -> Result<NemoMeta, String>
pub fn nemo_features(samples: &[f32], meta: &NemoMeta) -> ndarray::Array2<f32>   // [feat_dim, frames], per-feature normalised
```

- [ ] **Step 1: Write the failing tests:**
  - `features_shape_and_normalisation`: 1 s of a 440 Hz sine → `feat_dim` rows, frames = the snip-edges count for the metadata window/stride; each row has mean ≈ 0 and std ≈ 1 (tolerance 1e-3).
  - `features_silence_is_finite`: all zeros → all values finite (the `stddev + 1e-5` guard).
  - The embedding parity test lives in Task 10, which adds `embed`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement** the port, using `realfft`. It must reproduce: pre-emphasis, the window from metadata, `snip_edges = true`, no DC removal, librosa-style mel filters, log, then per-feature normalisation with `stddev + 1e-5`. Match the sherpa code line by line. Put any constant not read from metadata in a `const` with a comment naming the sherpa line.
- [ ] **Step 4:** Unit tests PASS.
- [ ] **Step 5: Commit** `feat(speakers): NeMo feature extraction`.

### Task 10: ORT sessions on DirectML or CPU **[Codex]**

**Files:**
- Create: `speakers/runtime.rs`
- Test: `speakers/runtime.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Copy, PartialEq, Debug)] pub enum Backend { DirectMl, Cpu }
pub const TITANET_ON_DIRECTML: bool = true;   // set false if the user's profiling shows CPU is faster (Task 24)
pub struct SegMeta { pub sample_rate: usize, pub window_size: usize, pub receptive_field_size: usize, pub receptive_field_shift: usize, pub num_speakers: usize, pub powerset_max_classes: usize, pub num_classes: usize }
pub trait SpeakerModels {
    fn seg_meta(&self) -> &SegMeta;
    fn segment(&mut self, chunk: &[f32]) -> Result<ndarray::Array2<f32>, String>; // [frames, num_classes] logits
    fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, String>;    // None = invalid (NaN/zero-norm)
    fn backend_label(&self) -> &'static str;                                      // "directml" | "cpu" | "mixed"
    fn fall_back_to_cpu(&mut self) -> Result<(), String>;
}
pub struct OrtSpeakerModels { /* seg, emb sessions + backends + metas */ }
impl OrtSpeakerModels { pub fn load(dir: &Path, prefer_directml: bool) -> Result<Self, String> }
fn session(path: &Path, backend: Backend) -> Result<ort::session::Session, String>
```

- [ ] **Step 1: Write the failing tests:**
  - `cpu_session_options`: building with `Backend::Cpu` sets intra threads = max(1, logical/2), with spinning off (`with_intra_op_spinning(false)`, `with_inter_op_spinning(false)`). Test via a small pure `cpu_threads(logical: usize) -> usize` (`8 → 4`, `1 → 1`).
  - `load_on_linux_uses_cpu` (ignored; needs the parity models via `SPEAKER_MODELS_DIR`): `load(dir, true)` → `backend_label() == "cpu"`. On non-Windows, DirectML is never attempted (`#[cfg(not(windows))]` → CPU). The fallback *behaviour* is tested with fake models in Task 16, and the real registration on Windows in Task 23.
  - `backend_label_combines_backends`: a pure `label(seg: Backend, emb: Backend, any_directml_work_failed_over: bool) -> &'static str`. Both DirectML → `directml`; both CPU and never DirectML → `cpu`; mixed, or a failover after DirectML work → `mixed`.
  - `segment_shape_matches_meta` (ignored): 10 s of zeros → `cols == num_classes` from the metadata.
  - `embedding_parity_with_sherpa` (ignored, moved here from Task 9 because it needs `embed`): for each recording's 20 clips, cosine(Rust `embed`, `clip_emb.npy`) > 0.999. Report the min cosine.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement.**
  - **DirectML** (only under `#[cfg(windows)]`): `DirectMLExecutionProvider::default().build().error_on_failure()`, `with_parallel_execution(false)`, `with_memory_pattern(false)`.
  - **CPU:** `CPUExecutionProvider`, threads as tested, spinning off.
  - Call `crate::ensure_onnx_runtime_available()` first.
  - If a DirectML session fails to create, use CPU and log `warn!`.
  - Read `SegMeta` from model metadata keys `sample_rate`, `window_size`, `receptive_field_size`, `receptive_field_shift`, `num_speakers`, `powerset_max_classes`, `num_classes`, as sherpa's `offline-speaker-segmentation-pyannote-model.cc` does. Error unless `sample_rate == 16000`.
  - Segmentation input is one `f32` tensor of shape `[1, 1, window_size]` (sherpa `offline-speaker-diarization-pyannote-impl.h:365`). Output `[1, frames, num_classes]` is returned as `[frames, num_classes]`. Input/output names come from the session; don't hard-code them.
  - `embed` computes features with Task 9's `nemo_features` (`[feat_dim, frames]`). It runs TitaNet with an `f32` tensor `[1, feat_dim, frames]` and an `i64` length tensor `[1]` = `frames`. **No frame padding:** sherpa resizes its buffer to a multiple of 16 frames, but its tensor shape uses the unpadded count (`speaker-embedding-extractor-nemo-impl.h:92-100`), so the padding never reaches the model.
  - Sessions are `!Sync`; a single job thread owns them.
- [ ] **Step 4:** Unit tests PASS. Run the ignored ones with `SPEAKER_MODELS_DIR=/workspace/.superpowers/speaker-parity/models cargo test ... -- --ignored`. The embedding parity test must pass on all 80 clips.
- [ ] **Step 5: Commit** `feat(speakers): ORT sessions with explicit DirectML/CPU backend`.

### Task 11: Clustering and step cap

**Files:**
- Create: `speakers/cluster.rs`
- Test: `speakers/cluster.rs`

**Source to port:** sherpa-onnx v1.13.8 `fast-clustering.cc`, which uses `fastclustercpp::hclust_fast` with `HCLUST_METHOD_COMPLETE` and `cutree_cdist`. Implement complete linkage with the **nearest-neighbour-chain** algorithm, which is O(N²) time over an O(N²) `f32` condensed matrix. Reproduce `cutree_cdist`'s cut rule exactly; read it in fastcluster's `fastcluster.cpp`, which sherpa vendors.

**Interfaces:**
- Produces:

```rust
pub const MAX_EMBEDDINGS: usize = 15_000;
pub fn complete_linkage_labels(emb: &[Vec<f32>], threshold: f32) -> Vec<usize>   // labels 0..k, by first appearance
pub fn window_step_secs(duration_secs: f64) -> f64      // smallest whole step ≥ 1 s with 3 * windows(step) ≤ MAX_EMBEDDINGS
pub fn cosine(a: &[f32], b: &[f32]) -> f32
pub fn mean_normalized(v: &[Vec<f32>]) -> Option<Vec<f32>>  // None if empty or zero-norm
```

- [ ] **Step 1: Write the failing tests:**
  - `complete_linkage_two_tight_groups`: 2 groups of 5 near-identical vectors (cosine between groups 0.1) → 2 labels.
  - `complete_linkage_chain_is_not_single_linkage`: a chain a–b–c where cos(a,b) and cos(b,c) are 0.9 but cos(a,c) is 0.2 → complete linkage at distance 0.7 does **not** put a, b and c together.
  - `complete_linkage_matches_naive_on_random`: 60 random vectors; the labels equal a naive O(N³) complete-linkage implementation inside the test (same partition).
  - `complete_linkage_single_and_empty`: 1 vector → `[0]`; 0 → `[]`.
  - `window_step_keeps_embeddings_under_cap`: 1 h → 1.0; 3 h → 3.0 (exact values from the rule); `3 * windows ≤ 15_000` for 10 min, 1 h, 2 h, 3 h, 6 h. `windows(step) = floor((dur − 10) / step) + 1 (+1 if remainder)`, as sherpa computes chunks.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(speakers): complete-linkage clustering with size cap`.

### Task 12: Diarization port (segmentation → embeddings → reconstruction)

**Files:**
- Create: `speakers/diarize.rs`
- Test: `speakers/diarize.rs`

**Source to port:** sherpa-onnx **v1.13.8** `offline-speaker-diarization-pyannote-impl.h`. Port these functions:
- `Process`
- `RunSpeakerSegmentationModel`
- `ToMultiLabel` and `InitPowersetMapping`
- `ComputeSpeakersPerFrame`
- `GetChunkSpeakerSampleIndexes` and `ExcludeOverlap`
- `ComputeEmbeddings`
- `ConvertChunkSpeakerToCluster`
- `ReLabel`
- `ComputeSpeakerCount`
- `FinalizeLabels`
- `ComputeResult`, including `min_duration_on/off`
- `HandleOneChunkSpecialCase`

Skip the silhouette/confidence code: `compute_confidence` is off.

**Interfaces:**
- Consumes: `SpeakerModels` (Task 10), `cluster::complete_linkage_labels` (Task 11). This task defines `AudioSource` with a `Vec<f32>` impl for tests; Task 13 implements it for the temp file.
- Produces:

```rust
pub trait AudioSource { fn len(&self) -> usize; fn read(&self, start: usize, out: &mut [f32]) -> Result<(), String>; }
impl AudioSource for Vec<f32> { ... }
#[derive(Debug, Clone, PartialEq)] pub struct Turn { pub start: f64, pub end: f64, pub cluster: usize }
pub fn diarize(audio: &dyn AudioSource, models: &mut dyn SpeakerModels, step_secs: f64, progress: &mut dyn FnMut(f32)) -> Result<Vec<Turn>, String>
```

- [ ] **Step 1: Write the failing tests** with a `FakeModels` that returns scripted logits and embeddings:
  - `powerset_mapping_matches_sherpa`: for 3 speakers and max 2 classes, class index → active-speaker set equals sherpa's table (`{}`, `{0}`, `{1}`, `{2}`, `{0,1}`, `{0,2}`, `{1,2}`).
  - `overlap_frames_excluded_from_embedding_audio`: the frames where 2 speakers are active are not in the sample index ranges.
  - `diarize_silence_returns_no_turns`: all-silence logits → `Ok(vec![])`.
  - `diarize_single_window_skips_clustering`: audio ≤ 10 s → `HandleOneChunkSpecialCase` path; FakeModels `embed` is never called.
  - `diarize_drops_invalid_embeddings`: FakeModels returns `None` for one (chunk, speaker) → it is excluded and the result is still produced.
  - `turns_clamped_to_audio_length`: no turn ends after `len / 16000`.
  - `diarize_parity_with_sherpa` (`#[ignore]`): for each recording and both steps (1 s, 2 s), compare the Rust turns from `audio16k.f32` against `diar_step{1,2}.json`. Required: the same number of clusters, and ≥ 99 % of speech time agreeing under the best one-to-one cluster mapping (greedy by overlap). Print the agreement.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement** the port.
  - The window shift is `step_secs * 16000` samples (sherpa: `window_shift_ratio * window_size`, so 0.1 means 1 s).
  - Progress: segmentation 0–0.25, embeddings 0.25–0.9, the rest up to 1.0.
- [ ] **Step 4:** PASS. The parity test PASSES with agreement ≥ 99 % on all 8 runs. If a run fails, don't loosen the gate: report `DONE_WITH_CONCERNS` with the per-run numbers.
- [ ] **Step 5: Commit** `feat(speakers): pyannote diarization port`.

### Task 13: Streaming decode to a 16 kHz temp file

**Files:**
- Modify: `audio/decoder.rs` (add a function; `decode_audio_file` is unchanged)
- Create: `speakers/temp_audio.rs`
- Test: both files.

**Interfaces:**
- Produces:

```rust
// audio/decoder.rs
pub fn decode_to_mono_16k_file(input: &Path, output: &Path, progress: &mut dyn FnMut(f32)) -> anyhow::Result<f64>  // seconds written
// speakers/temp_audio.rs
pub struct TempFileGuard(PathBuf);       // deletes the file on Drop unless converted
impl TempFileGuard { pub fn new_in(dir: &Path, meeting_id: &str) -> Self; pub fn path(&self) -> &Path; }
pub struct TempAudio { file: std::fs::File, len: usize, guard: TempFileGuard }   // deletes the file on Drop
impl TempAudio { pub fn open(guard: TempFileGuard) -> std::io::Result<Self>; }
impl crate::speakers::diarize::AudioSource for TempAudio
pub fn clear_temp_dir(dir: &Path)        // startup: removes *.f32 files directly in dir only
```

- [ ] **Step 1: Write the failing tests:**
  - `decode_to_mono_16k_file_matches_whole_decode`: a generated 48 kHz stereo WAV (3 s, via `hound` in `dev-dependencies`, or the existing test fixture writer) → the file has 48,000 samples, within ±1 of `decode_audio_file(..).to_whisper_format()` length. Mean absolute difference < 1e-3.
  - `decode_streams_without_loading_whole_file`: a 60 s source. The function records the largest number of input samples it held at once in a `#[cfg(test)]` thread-local high-water mark, updated wherever samples are buffered. Assert the mark ≤ 2 s of input. A whole-file implementation would hold 60 s and fail.
  - `decode_keeps_timing_exact`: 48 kHz input of 10.37 s (not a whole number of chunks) with a click at 0.0 s, at 5.0 s (a chunk boundary) and at 10.3 s. Output length = `round(10.37 × 16000)` ± 1. Each click peak is within ±2 samples of its expected 16 kHz position.
  - `temp_guard_deletes_partial_file_on_decode_error`: decode a truncated file into a guard → `Err`, and the file no longer exists after the guard drops.
  - `temp_audio_reads_ranges_and_deletes_on_drop`; `clear_temp_dir_removes_only_f32_files`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement.**
  - The symphonia packet loop downmixes to mono and feeds a `rubato::FftFixedIn` resampler in 1 s chunks.
  - **Timing:**
    - Drop the first `resampler.output_delay()` output frames.
    - Feed the final partial chunk with `process_partial`, then flush with `process_partial(None)` until enough output exists.
    - Trim the output to exactly `round(input_frames × 16000 / input_rate)` frames.
  - Write `f32` LE through a `BufWriter` into the guard's path. The caller opens `TempAudio` only on success; on error the guard deletes the partial file.
  - The ffmpeg-only extensions reuse `convert_to_wav_with_ffmpeg` first.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(speakers): streaming 16 kHz decode to a temp file`.

**Phase 3 end:** reviewer + Codex.

---

## Phase 4 — Assignment, storage and the job

### Task 14: Speaker resolution and line assignment

**Files:**
- Create: `speakers/assign.rs`
- Test: `speakers/assign.rs`

**Interfaces:**
- Consumes: `Turn` (Task 12), `cosine` and `mean_normalized` (Task 11), `Sentence` (Task 2), `mic_only_fraction` (Task 5).
- Produces:

```rust
pub const SPLIT_LINES: bool = /* Task 4 result */;
pub struct ClusterInfo { pub id: usize, pub seconds: f64, pub turns: Vec<(f64, f64)>, pub centroid: Option<Vec<f32>> }
pub enum SelfEvidence { Levels(Vec<bool>), Prints(Vec<Vec<f32>>), None }
pub struct KnownPerson { pub person_id: String, pub prints: Vec<Vec<f32>> }
#[derive(Debug, Clone, PartialEq)] pub struct ResolvedSpeaker { pub idx: i64, pub person_id: Option<String>, pub centroid: Option<Vec<f32>>, pub seconds: f64, pub turns: Vec<(f64, f64)> }
pub fn clusters_from_turns(turns: &[Turn]) -> Vec<ClusterInfo>                 // centroid None (filled by the job)
pub fn centroid_windows(turns: &[(f64, f64)]) -> Vec<(f64, f64)>
// turns < 1.5 s: skipped; 1.5–3 s: the whole turn; ≥ 3 s: consecutive non-overlapping 3 s windows from the turn start
// (a remainder ≥ 1.5 s is its own window, a shorter one is dropped). More than 60 candidates → keep
// candidates[(i * (n - 1)) / 59] for i in 0..60, in time order.
pub fn self_learning_windows(turns: &[(f64, f64)], flags: &[bool]) -> Vec<(f64, f64)>  // 3 s windows ≥ 90 % mic-only
pub fn resolve_speakers(clusters: Vec<ClusterInfo>, self_ev: &SelfEvidence, people: &[KnownPerson]) -> Vec<ResolvedSpeaker>
pub struct Line { pub id: String, pub start: f64, pub end: f64, pub text: String, pub sentences: Option<Vec<Sentence>> }
#[derive(Debug, Clone, PartialEq)] pub struct LineOut { pub id: Option<String> /* None = new row */, pub source_id: String, pub start: f64, pub end: f64, pub text: String, pub sentences: Option<Vec<Sentence>>, pub speaker_idx: Option<i64> }
pub fn assign_lines(lines: &[Line], speakers: &[ResolvedSpeaker]) -> Vec<LineOut>
```

`resolve_speakers` order (spec §8.2 steps 6–10):
1. Anchors (≥ 30 s) merge greedily, biggest first. Each anchor joins the most similar already-kept anchor at cosine ≥ 0.7, comparing against the kept anchor's original centroid.
2. Self:
   - With `Levels`: the cluster with the most mic-only seconds, if mic-only ≥ 50 % of its seconds.
   - With `Prints`: the best max-cosine ≥ 0.75.
   - Self becomes idx 0, `person_id = "self"`.
3. Known people: every remaining cluster with a centroid (anchors and small ≥ 3 s) takes the person with the best max-cosine ≥ 0.75. Clusters that match the same person merge.
4. Remaining small clusters attach to the nearest anchor at cosine ≥ 0.65. This includes anchors that became self or known. If the meeting has no anchor at all, they stay as their own speakers; otherwise they become Unknown (dropped from the list). Clusters < 3 s become Unknown.
5. Unnamed speakers are numbered 1..n by seconds, descending.

`assign_lines` (step 11):
- For each sentence, take the speaker with the most overlap.
- Sentences with no overlap inherit the line's majority.
- If `SPLIT_LINES` is true, split where a sentence has ≥ 60 % and ≥ 1 s of overlap with a speaker different from its neighbours.
- Adjacent sentences with the same speaker stay together.
- The first piece keeps `id = Some(original)`; later pieces get `id = None`.
- Lines without sentences get the majority speaker.
- No overlap with any speaker → `speaker_idx None`.

- [ ] **Step 1: Write the failing tests** (synthetic unit vectors, 192-d):
  - `merge_two_anchors_of_same_voice`: two 60 s clusters at cos 0.8 → one speaker of 120 s.
  - `anchors_below_merge_threshold_stay_apart`: cos 0.5 → 2.
  - `self_by_levels_needs_half_mic_only`: a cluster with 40 % mic-only → not self; 60 % → idx 0.
  - `self_by_prints_when_no_levels`: cos 0.8 to a self print → idx 0; 0.7 → no self.
  - `user_silent_gives_no_self`: no cluster qualifies → no idx 0 in the output.
  - `known_person_matched_and_same_person_clusters_merge`.
  - `small_cluster_attaches_to_anchor_at_065`; `small_cluster_without_match_is_unknown_when_anchors_exist`.
  - `short_meeting_without_anchors_keeps_small_speakers`: a 25 s meeting, two 10 s clusters → Speaker 1 and Speaker 2.
  - `tiny_cluster_under_3s_is_unknown`.
  - `numbering_by_talk_time_desc`.
  - `line_split_at_sentence_with_other_speaker` (with `SPLIT_LINES` forced true through a `assign_lines_with(split: bool)` inner fn): a line "Do you agree? Yes. Then go." with sentences in A, B (1.2 s), A → 3 pieces, `ids [Some, None, None]`, speakers `[A, B, A]`, texts and sentence subsets correct.
  - `short_reply_under_1s_does_not_split`.
  - `line_without_sentences_gets_majority`; `line_with_no_overlap_is_unknown`.
  - `centroid_windows_cap_60_evenly`; `self_learning_windows_need_90_percent`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(speakers): speaker resolution and line assignment`.

### Task 15: Speaker repository: people, voiceprints, jobs, publish **[Codex]**

**Files:**
- Create: `database/repositories/speakers.rs`
- Modify: `database/repositories/mod.rs`, `database/repositories/transcript.rs` (enqueue in the save transaction)
- Test: `database/repositories/speakers.rs`

**Interfaces:**
- Consumes: `ResolvedSpeaker`, `LineOut`, `KnownPerson` (Task 14).
- Produces (all `pub async fn` on `pub struct SpeakersRepository;`):

```rust
pub struct JobRow { pub meeting_id: String, pub status: String, pub reason: Option<String>, pub backend: Option<String>, pub run_token: String, pub audio_seconds: Option<f64>, pub run_seconds: Option<f64>, pub updated_at: String }
pub struct SpeakerView { pub idx: i64, pub name: String /* resolved display */, pub person_id: Option<String>, pub is_self: bool, pub seconds: f64 }
pub struct PersonView { pub id: String, pub name: String, pub is_self: bool, pub meetings: i64, pub last_heard: Option<String> }
pub enum NameTarget { Person(String), New(String) }
pub struct Publish { pub speakers: Vec<ResolvedSpeaker>, pub lines: Vec<LineOut>, pub self_print: Option<(Vec<f32>, f64)>, pub backend: String, pub audio_seconds: f64, pub run_seconds: f64 }

enqueue_in_tx(conn: &mut SqliteConnection, meeting_id: &str) -> Result<String, SqlxError>       // upsert pending, new token
enqueue(pool, meeting_id) -> Result<String, SqlxError>
transcripts_replaced_in_tx(conn: &mut SqliteConnection, meeting_id: &str, active: bool) -> Result<(), SqlxError>
// called by EVERY transaction that replaces a meeting's transcripts (re-transcribe), whatever the feature state:
// active → enqueue_in_tx; inactive and a job row exists → status 'skipped', reason 'feature_off', NEW token
// (so a job still running can never publish against the replaced rows); no row → nothing
recover_jobs(pool) -> Result<u64, SqlxError>                     // pending|running → pending with new tokens
claim_next(pool) -> Result<Option<(String, String)>, SqlxError>  // oldest pending → running; returns (meeting_id, token)
finish_without_publish(pool, meeting_id, token, status: &str, reason: Option<&str>, backend: Option<&str>) -> Result<bool, SqlxError>
publish(pool, meeting_id, token, p: &Publish) -> Result<bool, SqlxError>
get_job(pool, meeting_id) -> Result<Option<JobRow>, SqlxError>
last_done_job(pool) -> Result<Option<JobRow>, SqlxError>
meeting_speakers(pool, meeting_id) -> Result<Vec<SpeakerView>, SqlxError>
known_people(pool) -> Result<(Vec<Vec<f32>> /* self prints */, Vec<KnownPerson>), SqlxError>
name_speaker(pool, meeting_id, idx, target: NameTarget) -> Result<(), String>
move_line(pool, transcript_id, idx: Option<i64>) -> Result<(), String>
list_people(pool) -> Result<Vec<PersonView>, SqlxError>
rename_person(pool, id, name) -> Result<(), String>
delete_person(pool, id) -> Result<(), String>
merge_people(pool, from, into) -> Result<(), String>
```

Rules (spec §9):
- **Vectors** are 192 little-endian `f32` BLOBs (768 bytes). Reading any other length is an error, and the row is skipped with a `warn!`.
- **Publish:** one transaction. First run `UPDATE speaker_jobs SET status='done', backend=?, audio_seconds=?, run_seconds=?, reason=NULL, updated_at=? WHERE meeting_id=? AND run_token=? AND status='running'`. If 0 rows are affected, roll back and return `false`. Then:
  - `DELETE FROM voiceprints WHERE source_meeting_id=?`
  - replace `meeting_speakers`
  - for each `LineOut`: `UPDATE` (id `Some`; also set `transcript`, `audio_start_time`, `audio_end_time`, `duration = end − start`, `sentences`, `speaker_idx`) or `INSERT` (new UUID `transcript-<uuid>`, same columns, copying `timestamp` from `source_id`'s row, `mark` NULL)
  - insert the self print (`source_idx 0`, `person_id 'self'`) if present, then prune to 20 newest
- **`name_speaker`:**
  - `Err("Speaker analysis is running")` if the job is pending or running.
  - `New(name)`: the trimmed name must be 1–80 characters; creates a person `person-<uuid>`.
  - Sets `meeting_speakers.person_id`.
  - If the speaker has `seconds ≥ 20` and a centroid, upserts the voiceprint for `(meeting, idx)` (`ON CONFLICT(source_meeting_id, source_idx) DO UPDATE SET person_id, vector, seconds, created_at`), then prunes that person to 20.
  - Naming idx 0 is refused.
- **`move_line`:** same running check. `idx` must exist in `meeting_speakers` for that transcript's meeting, or be `None`.
- **`rename_person`:** 1–80 characters; self can be renamed.
- **`delete_person`:** self is refused. First run `UPDATE meeting_speakers SET name = (SELECT name FROM people WHERE id=?) WHERE person_id=?`, then delete the person (the cascade removes prints and sets `meeting_speakers.person_id` NULL).
- **`merge_people`:**
  - Refused if either side is self or `from == into`.
  - Move voiceprints and `meeting_speakers.person_id` to `into`, prune `into` to 20, delete `from`.
- **`finish_without_publish`** is token-checked like publish: `UPDATE speaker_jobs SET status=?, reason=?, backend=?, updated_at=? WHERE meeting_id=? AND run_token=? AND status='running'`. It returns `false` on 0 rows, so an overtaken job can never overwrite a newer pending one.
- **Save transaction:** `TranscriptsRepository::save_transcript` gains `enqueue_speakers: bool` and calls `enqueue_in_tx` inside its transaction. This task updates its caller `api/api.rs:994` to pass `false`; Task 16 wires `speakers::is_active()`.

- [ ] **Step 1: Write the failing tests:**
  - `save_without_feature_creates_no_job`; `save_with_feature_creates_pending_job`.
  - `claim_and_publish_with_matching_token`: speakers are replaced, lines updated or inserted (each with `duration == end − start`), the job is `done`.
  - `publish_with_stale_token_does_nothing`: re-enqueue between claim and publish → returns `false`, and no row changed.
  - `finish_with_stale_token_does_nothing`: re-enqueue between claim and `finish_without_publish(.., "failed", ..)` → `false`; the job stays `pending`.
  - `replacing_transcripts_with_feature_off_invalidates_running_job`: claim; `transcripts_replaced_in_tx(.., active=false)`; publish with the old token → `false`. Status is `skipped` / `feature_off`.
  - `publish_after_meeting_deleted_does_nothing`: no orphan `meeting_speakers`.
  - `recover_requeues_running_with_new_token`.
  - `publish_removes_prints_learned_from_this_meeting_on_rerun`.
  - `name_speaker_learns_once_and_replaces_on_rename`: naming idx 1 as Alice, then as Bob → exactly one print for `(M,1)`, owned by Bob.
  - `name_speaker_under_20s_does_not_learn`; `name_refused_while_job_running`; `name_self_refused`.
  - `prints_capped_at_20_newest`.
  - `delete_person_keeps_name_on_past_meetings`; `merge_moves_prints_and_speakers_and_prunes`; `self_cannot_be_deleted_or_merged`.
  - `move_line_validates_idx_belongs_to_meeting`.
  - `bad_vector_blob_is_skipped`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement. Runtime `sqlx::query` strings only (the codebase style).
- [ ] **Step 4:** PASS. Then `cargo test -p meetily --lib database::` → PASS.
- [ ] **Step 5: Commit** `feat(db): speaker repository with token-checked publish`.

### Task 16: Job worker, Tauri commands, enqueue hooks **[Codex]**

**Files:**
- Create: `speakers/job.rs`, `speakers/commands.rs`
- Modify:
  - `speakers/mod.rs`
  - `lib.rs`: setup sets `models::set_models_dir`; calls `temp_audio::clear_temp_dir`, `SpeakersRepository::recover_jobs`, and `job::start_worker(app.handle())` after DB init (`lib.rs:~577`); registers the commands
  - `api/api.rs` (`api_save_transcript` passes `enqueue_speakers = speakers::is_active()` and calls `job::wake()` after commit)
  - `audio/import.rs` (enqueue in `create_meeting_with_transcripts`'s transaction when active, then `wake()`)
  - `audio/retranscription.rs` (call `transcripts_replaced_in_tx(conn, meeting_id, is_active())` inside its replace transaction, always; then `wake()`)
- Test: `speakers/job.rs`

**Interfaces:**
- Consumes: Tasks 8–15.
- Produces:

```rust
// job.rs
pub fn start_worker<R: Runtime>(app: AppHandle<R>)        // one tokio task; one job at a time app-wide
pub fn wake()                                              // tokio::sync::Notify
pub(crate) async fn run_one<R: Runtime>(app: &AppHandle<R>, pool: &SqlitePool, meeting_id: &str, token: &str, deps: &mut dyn JobDeps) -> Result<(), String>
pub(crate) trait JobDeps { fn models(&mut self, prefer_directml: bool) -> Result<Box<dyn SpeakerModels>, String>; fn is_active(&self) -> bool; fn models_ready(&self) -> bool; fn emit(&self, event: &str, meeting_id: &str); fn recordings_base(&self) -> Option<PathBuf>; }
// commands.rs (all #[tauri::command])
speakers_get_settings() -> SpeakerSettings { enabled, models_ready, downloading, last_analysis: Option<{backend, run_seconds, audio_seconds}> }
speakers_set_enabled(enabled: bool)                        // settings::save_enabled; enabling starts the download if models are missing; emits speaker-settings-changed
speakers_download_models()                                 // emits speaker-models-progress / speaker-models-done; one download at a time
speakers_delete_models()
speakers_get_meeting(meeting_id) -> { job: Option<{status, reason, percent: Option<u8>}>, speakers: Vec<SpeakerView> }
speakers_identify(meeting_id)                              // Err if not active, no recording, or job pending/running
speakers_name(meeting_id, idx, person_id: Option<String>, new_name: Option<String>)
speakers_move_line(transcript_id, idx: Option<i64>)
people_list() / people_rename(id, name) / people_delete(id) / people_merge(from, into)
```

`run_one` steps (spec §8.2):
0. **Preconditions** → `finish_without_publish(.., "skipped", reason)`: `feature_off`, `models_missing`, `no_recording`. The recording is found with `guarded_meeting_folder(&folder_path, &recordings_base)` + `find_audio_file`. `recordings_base` comes from the saved `save_folder` preference, the same way Phase 4's callers get it (see the callers of `guarded_meeting_folder` in `audio/meeting_folder.rs`/`retention.rs`).
1. Decode to a `TempAudio` in `<app_data>/speakers-tmp/`.
2. Pick the step with `window_step_secs`, then `diarize`.
3. Build clusters; embed `centroid_windows` for each cluster with ≥ 3 s.
4. **Self evidence:** `levels.bin` decoded → `Levels(mic_only_flags)`; if it is missing or invalid → `Prints(self prints)`, or `None` if there are no prints.
5. `resolve_speakers` with `known_people`.
6. **Self print:** if self came from `Levels`, embed `self_learning_windows`. If they total ≥ 20 s, `self_print = mean_normalized`.
7. Load the meeting's lines (`id`, times, text, `parse_json(sentences)`), then `assign_lines`.
8. `publish`. A `false` result is logged ("job overtaken") and not an error.
9. Emit `speakers-updated`. Log at `info` exactly: `speakers: stage timings decode={:.1}s diarize={:.1}s centroids={:.1}s publish={:.1}s backend={} audio={:.0}s run={:.1}s`.

Rules:
- **Every** terminal transition the worker makes (`done`, `failed`, `skipped`), and every enqueue, emits `speakers-updated { meeting_id }`. Otherwise the UI would stay on "running" and the summary gate would never open.
- `speaker-settings-changed` is emitted on toggle, after a successful model download, and after model deletion, i.e. whenever `is_active()` can change.
- `speakers_set_enabled(true)` returns immediately. A missing-model download runs in `tauri::async_runtime::spawn`; a second request while one runs is ignored.
- Progress emits `speakers-progress`: decode 0–10, diarize 10–80, centroids 80–95, publish 100. The latest percent is kept in a `Mutex<HashMap<meeting_id, u8>>` for `speakers_get_meeting`, and the entry is removed when the job reaches a terminal state.
- **DirectML failure** during a stage: call `models.fall_back_to_cpu()`, then redo that stage. The backend becomes `"mixed"` if any work ran on DirectML, else `"cpu"`.
- Any other error → `failed` with the error text, truncated to 300 characters.
- The `TempAudio` is dropped (deleted) on every path.
- Heavy work runs in `tokio::task::spawn_blocking`; the sessions live inside that closure.

- [ ] **Step 1: Write the failing tests**, with `FakeDeps`, fake models and a temp meeting folder holding a tiny WAV named `audio.wav`:
  - `job_skips_without_recording`; `job_skips_when_feature_off`; `job_skips_when_models_missing`. Each also asserts that a `speakers-updated` event was emitted (via a recording event sink in `JobDeps`, `fn emit(&self, event: &str, meeting_id: &str)`).
  - `job_failure_emits_update`.
  - `job_publishes_speakers_and_labels` (fake diarization: two clusters, one mic-only via a written `levels.bin`) → idx 0 and idx 1 exist, and the lines are labelled.
  - `job_discards_result_when_token_changed`: re-enqueue during the run (FakeModels hook) → status stays `pending`, and no speakers are written.
  - `job_falls_back_to_cpu_and_records_mixed`: fake DirectML `embed` fails once → the job is `done` with backend `mixed`.
  - `job_deletes_temp_file_on_failure`.
  - `job_silence_finishes_done_without_speakers`.
  - `identify_refused_while_running`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS. Then the full `cargo test --workspace --exclude llama-helper` → PASS.
- [ ] **Step 5: Run end to end on Linux** (`#[ignore]` test `job_end_to_end_on_meetup`, needs `SPEAKER_MODELS_DIR`):
  - Copy `meetup.mp4` into a temp meeting folder.
  - Insert a meeting with one line per 10 s.
  - Run → `done`, ≥ 3 speakers with ≥ 30 s.
  - Report run seconds per hour of audio on CPU.
- [ ] **Step 6: Commit** `feat(speakers): analysis job, commands and enqueue on save/import/re-transcribe`.

**Phase 4 end:** reviewer + Codex.

---

## Phase 5 — UI

### Task 17: Frontend speaker helpers and `speakerIdActive`

**Files:**
- Create: `frontend/src/lib/speakers.ts`
- Modify: `frontend/src/contexts/ConfigContext.tsx`, `frontend/src/types/index.ts`
- Test: `frontend/tests/lib/speakers.test.ts`

**Interfaces:**
- Produces:

```ts
export type SpeakerJobStatus = 'pending' | 'running' | 'done' | 'failed' | 'skipped';
export interface SpeakerView { idx: number; name: string; person_id: string | null; is_self: boolean; seconds: number }
export interface MeetingSpeakers { job: { status: SpeakerJobStatus; reason: string | null; percent: number | null } | null; speakers: SpeakerView[] }
export interface SpeakerSettings { enabled: boolean; models_ready: boolean; downloading: boolean; last_analysis: { backend: string; run_seconds: number; audio_seconds: number } | null }
export function isJobBusy(job: MeetingSpeakers['job']): boolean                       // pending | running
export function lineSpeakerName(idx: number | null | undefined, speakers: SpeakerView[], analysed: boolean): string  // 'You' | name | 'Others' | 'Unknown'
export function speakerChips(speakers: SpeakerView[]): { key: string; name: string; idxs: number[]; seconds: number; isSelf: boolean }[]  // grouped by person_id, self first, then seconds desc
export function formatTalkTime(seconds: number): string                               // '12m', '45s', '1h 05m'
export function lastAnalysisText(a: SpeakerSettings['last_analysis']): string | null  // 'Last analysis: DirectML · 41 s for a 58-min meeting'
// ConfigContext: speakerIdActive: boolean; refreshSpeakerSettings(): Promise<void>  (listens to 'speaker-settings-changed')
```

- [ ] **Step 1: Write the failing tests:**

```ts
test('live labels before analysis', () => { expect(lineSpeakerName(0, [], false)).toBe('You'); expect(lineSpeakerName(null, [], false)).toBe('Others'); });
test('after analysis null is Unknown and idx resolves', () => {
  const s = [{ idx: 0, name: 'You', person_id: 'self', is_self: true, seconds: 60 }, { idx: 1, name: 'Сергей', person_id: 'p1', is_self: false, seconds: 30 }];
  expect(lineSpeakerName(1, s, true)).toBe('Сергей'); expect(lineSpeakerName(null, s, true)).toBe('Unknown'); });
test('two speakers named the same person form one chip', () => { /* idx 1 and 2 both person p1 → one chip, idxs [1,2], seconds summed */ });
test('last analysis text', () => { expect(lastAnalysisText({ backend: 'directml', run_seconds: 41, audio_seconds: 3480 })).toBe('Last analysis: DirectML · 41 s for a 58-min meeting'); });
```

- [ ] **Step 2:** `cd frontend && bun test tests/lib/speakers.test.ts` → FAIL.
- [ ] **Step 3:** Implement. `speakerIdActive = enabled && models_ready`, loaded with `invoke('speakers_get_settings')` on mount, using the `RetranscribeDialog` unlisten-collection pattern.
- [ ] **Step 4:** PASS, and `pnpm exec tsc --noEmit` passes.
- [ ] **Step 5: Commit** `feat(ui): speaker helpers and active flag`.

### Task 18: Settings → Speakers tab

**Files:**
- Create: `frontend/src/components/SpeakerSettings.tsx`
- Modify: `frontend/src/app/settings/page.tsx:20-27,119-139` (a tab `{ value: 'speakers', label: 'Speakers', icon: Users }` after Transcription)
- Test: `frontend/tests/components/speaker-settings.test.tsx`

**Behaviour** (spec §10.1):
- Switch "Identify speakers", off by default.
- Turning it on calls `speakers_set_enabled(true)`, then shows a download progress bar from `speaker-models-progress`. On `speaker-models-done {ok:false}` it shows the error and **Retry** (`speakers_download_models`).
- A "Delete models" button.
- The `lastAnalysisText` line.
- **Known voices** (`people_list`): the self row is pinned first with "this is you" and offers only Rename. Other rows show name, meetings and last heard, with Rename (inline input), "Merge into…" (`Select` of the other non-self people) and Delete (confirm with "Voiceprints are removed; past transcripts keep the name").

- [ ] **Step 1: Write the failing tests:** toggle on invokes `speakers_set_enabled` with `{ enabled: true }`; the progress event renders a percentage; a failed done shows Retry, and clicking it invokes `speakers_download_models`; self has no Delete and no Merge; Delete confirms with the exact text and then invokes `people_delete`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement with `ui/switch`, `ui/button`, `ui/input`, `ui/select`, following `BetaSettings.tsx`.
- [ ] **Step 4:** PASS. Then `tsc`, `bun test`, `pnpm run build`.
- [ ] **Step 5: Commit** `feat(ui): Settings → Speakers`.

### Task 19: Live You/Others label

**Files:**
- Modify:
  - `frontend/src/components/VirtualizedTranscriptView.tsx` (an optional `speakerLabel?: string` on `TranscriptSegment`; `getSpeakerLabel?: (segment) => string | undefined` on the view props)
  - `frontend/src/types/index.ts` (`TranscriptSegmentData.speaker_idx?`)
  - `frontend/src/app/_components/TranscriptPanel.tsx` (pass `lineSpeakerName(s.speaker_idx, [], false)` when `speakerIdActive`)
- Test: `frontend/tests/components/transcript-speaker-label.test.tsx`

- [ ] **Step 1: Write the failing tests:** with a label, the row renders a small label element (`data-speaker-label="You"`) before the text; without one, the row output is unchanged from today (same markup as the existing `transcript-streaming` test snapshot).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement. The label is a `span` with `text-xs` muted styling. The `memo` comparison includes `speakerLabel`.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat(ui): live You/Others labels`.

### Task 20: Meeting page speaker UI

**Files:**
- Create:
  - `frontend/src/hooks/useMeetingSpeakers.ts`
  - `frontend/src/components/MeetingDetails/SpeakersBar.tsx`
  - `frontend/src/components/MeetingDetails/SpeakerLineMenu.tsx`
- Modify:
  - `components/MeetingDetails/TranscriptPanel.tsx`: bar above the list, `getSpeakerLabel`, row label → `SpeakerLineMenu`
  - `TranscriptButtonGroup.tsx`: the Identify/Re-identify button
  - `RetranscribeDialog.tsx`: the line "Speaker labels will be rebuilt afterwards." when active
  - `hooks/usePaginatedTranscripts.ts` and `TranscriptPanel.tsx:65-78`: map `speaker_idx`
- Test: `frontend/tests/hooks/meeting-speakers.test.tsx`, `frontend/tests/components/speakers-bar.test.tsx`

**Interfaces:**
- Produces:

```ts
export function useMeetingSpeakers(meetingId: string, active: boolean): {
  data: MeetingSpeakers | null; loaded: boolean /* first speakers_get_meeting answered (or inactive) */; busy: boolean; percent: number | null;
  identify(): Promise<void>; name(idx: number, target: { personId: string } | { newName: string }): Promise<void>;
  moveLine(transcriptId: string, idx: number | null): Promise<void>; namesChanged: boolean; clearNamesChanged(): void;
}  // reloads on 'speakers-updated' / 'speakers-progress' for this meeting; after publish it also re-fetches transcripts (calls the onTranscriptsChanged callback arg)
```

**Behaviour** (spec §10.3):
- While busy, show "Identifying speakers… {n}%" over the transcript. Editing is disabled.
- **Chips:** `● {name} {talk time}` with a ✎ on unnamed ones. Clicking opens a `Popover` with a `Command` list: Known voices from `people_list` and "New person…" (an input).
- **Row label click:** a `menuitemradio` list of this meeting's speakers plus "Unknown". This follows `TranscriptMarkButton`'s pattern, including keyboard support.
- **Button:** "Identify speakers", or "Re-identify" when the job status is `done`. Re-identify confirms with `window.confirm('Labels, including your manual fixes, will be replaced. Continue?')`. Shown only when `speakerIdActive && meetingFolderPath && !audioMissingReason`.
- **Notes:** on failure, "Speaker analysis failed: {reason}. Retry" (Retry calls `identify`). When skipped with `no_recording`, the quiet note "No recording saved, so speakers weren't identified".
- `namesChanged` is set after any successful `name` or `moveLine`.

- [ ] **Step 1: Write the failing tests:**
  - hook: `speakers-updated` for another meeting is ignored; for this meeting it reloads and calls `onTranscriptsChanged`.
  - bar: chips are grouped and ordered (self first); picking a known person invokes `speakers_name` with `{ meetingId, idx, personId }`; "New person…" with "Аня" invokes it with `newName: 'Аня'`; while busy, the chips are disabled.
  - line menu: choosing "Unknown" invokes `speakers_move_line` with `idx: null`.
  - button: hidden when inactive; Re-identify asks with the exact text and does nothing on No.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS. Then `tsc`, `bun test`, `pnpm run build`.
- [ ] **Step 5: Commit** `feat(ui): meeting speakers bar, line moves, identify`.

### Task 21: Summary waits; names in prompt and Copy

**Files:**
- Modify:
  - `frontend/src/lib/meeting-notes.ts:21-30` (`formatSummaryTranscript`)
  - `hooks/meeting-details/useSummaryGeneration.ts:349-351` (`buildSummaryTranscriptPayload`)
  - `hooks/meeting-details/useCopyOperations.ts:86-92`
  - `app/meeting-details/page-content.tsx:177-203` (the gate)
  - `components/MeetingDetails/SummaryPanel.tsx` (waiting text + "Summarize now"; the names-changed hint)
- Test: `frontend/tests/lib/meeting-notes.test.ts` (extend), `frontend/tests/hooks/summary-generation.test.tsx` (extend), `frontend/tests/components/summary-speaker-gate.test.tsx` (new)

**Interfaces:**
- Produces: `formatSummaryTranscript(transcripts: Transcript[], speakerName?: (t: Transcript) => string | undefined): string`. With a name: `${time}${mark} ${name}: ${text}`. Without one: today's exact output.

- [ ] **Step 1: Write the failing tests:**
  - `summary payload is byte-identical when the feature is off`: the existing fixture produces today's exact string with no second argument and with `speakerIdActive` false.
  - names on: `[00:05] Сергей: text`, and with a mark `[00:05] [MARKED: action] Сергей: text`.
  - Copy: feature on → `[12:04] Сергей: …`; off → unchanged.
  - gate: auto-summary does not start while the job is `pending` or `running`. It starts when the job turns `done`, `failed` or `skipped`, or when "Summarize now" is clicked. With the feature off it starts immediately, as today.
  - gate waits for the first lookup: `speakers_get_meeting` answers after 50 ms with a `pending` job → no summary starts before or after the answer until the job is terminal. The existing effect latches `autoGenerationStartedMeetingIdRef`, so starting early cannot be undone.
  - the gate opens after a `speakers-updated` event for a job that ended `failed` or `skipped`.
  - hint: after `namesChanged`, the summary panel shows "Speaker names changed. Regenerate the summary to use them". Clicking Regenerate goes through `shouldRegenerate(summaryDirty)`, the existing guard.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3: Implement.**
  - When `speakerIdActive` is false, call `formatSummaryTranscript(transcripts)` with **no** second argument (and Copy uses today's format). Only when active: names are resolved from the latest `speakers_get_meeting` result via `lineSpeakerName(t.speaker_idx, speakers, job?.status === 'done')`.
  - The gate is an extra condition in the existing effect: `speakerGateOpen`. It is true when inactive, or when `loaded` and (no job, the job is terminal, or `summarizeNow` is set). Until `loaded`, it is false.
- [ ] **Step 4:** PASS. Then `tsc`, `bun test`, `pnpm run build`.
- [ ] **Step 5: Commit** `feat(summary): wait for speakers and use names`.

**Phase 5 end:** reviewer + Codex.

---

## Phase 6 — Packaging, Windows and release

### Task 22: DirectML runtime, installer size check, network docs **[Codex]**

**Files:**
- Modify:
  - `frontend/src-tauri/build/onnxruntime.rs`: two archives, each with URL/size/SHA, and an `ARTIFACTS` list of the 3 DLLs from the Global Constraints table plus the license, taken from the DirectML nupkg (`LICENSE` or `LICENSE.txt` inside it; pin its size and SHA)
  - `.github/workflows/release.yml`: the cache key stays on `build/onnxruntime.rs`; add a step after the build
  - `docs/network-calls.md`: the build-time row now names `api.nuget.org` instead of `github.com/microsoft/onnxruntime`
- Test: the `build/onnxruntime.rs` verification functions, if they have tests; otherwise add `#[cfg(test)]` cases in a `tests` module compiled by `build.rs`'s test harness. If none exists, verify by a Windows target `cargo check` only (Linux builds skip staging).

Changes:
- **Staging:** download both `.nupkg` files (zip archives), verify archive size and SHA, extract only the listed paths, verify each file, and stage `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `DirectML.dll` and the license into `binaries/onnxruntime/`. The `tauri.windows.conf.json` resource mapping already bundles the whole directory next to the exe.
- **Preload `DirectML.dll`** (`lib.rs:467-496`, Windows only): resolve `DirectML.dll` from the resource dir the same way as `onnxruntime.dll`. Load it with `LoadLibraryExW(path, NULL, LOAD_WITH_ALTERED_SEARCH_PATH)` **before** `ort::init_from`, and log the loaded module path (`GetModuleFileNameW`). The exe's folder already precedes System32 in the search order, but the preload makes the bundled copy certain, including in test processes. Declare the two functions in an `extern "system"` block against `kernel32`, like `whisper_engine/cuda_runtime.rs:27-43`. A failed preload is logged and the app continues; speakers then run on CPU.
- **`release.yml`:** directly after "Build installer" and **before** "Publish release" (line 175), fail if any `bundle/nsis/*.exe` is > 100,000,000 bytes, and print the size. Also extend the smoke test to copy `binaries/onnxruntime/*` next to the exe. Today it copies only `cudart64_12.dll`, so an ORT load failure goes unnoticed. Then grep the app log for `Initialized bundled ONNX Runtime`.
- **Existing gap (do not fix here):** check the Windows CI log for an `ort-sys` download from `cdn.pyke.io` (default `download-binaries`). Report it to the controller, who puts it on the later list.

- [ ] **Step 1:** Run `cargo check --target x86_64-pc-windows-msvc -p meetily` if the toolchain is present. Otherwise check the build script by running its pure verification on the downloaded nupkgs in a scratch test (sizes and hashes match the table).
- [ ] **Step 2:** `actionlint` → clean; `bash scripts/check-no-phone-home.sh` → pass.
- [ ] **Step 3: Commit** `build: DirectML ONNX Runtime and installer size gate`.

### Task 23: Windows CI checkpoint (manual dispatch)

**Files:**
- Modify: `.github/workflows/ci.yml` (`windows-app` job): add `SPEAKER_E2E=1` and a step that runs `cargo test -p meetily --lib speakers::job::windows_e2e -- --ignored`
- Test: `speakers/job.rs` `windows_e2e` (`#[ignore]`)

`windows_e2e`:
- **First**, before any other ORT call in the test process: preload the staged `binaries/onnxruntime/DirectML.dll`, then `ort::init_from(<abs path of staged onnxruntime.dll>)`. Assert via `GetModuleFileNameW(GetModuleHandleW("onnxruntime.dll"))` that the loaded module is the staged one, not an `ort-sys` download in `target/`.
- Downloads both models into a temp dir with `download_models` (real network, pinned hashes).
- Loads `OrtSpeakerModels` with `prefer_directml = true`. A GPU-less runner may still create a DirectML device on Windows' software adapter, so accept `directml` or `cpu`. Assert that the label matches what actually registered (the per-session backends recorded at creation), and that loading did not panic.
- Runs `run_one` on a generated 30 s noise WAV → `done`, with the same backend label.
- **Parakeet check:** build an ORT session with Parakeet's exact options (`model.rs:120-131`: Level3, CPU EP, parallel execution on) on the 6 MB segmentation model, and run it once. A real Parakeet model (~600 MB) is not downloaded in CI. The user doesn't use Parakeet, and this checks that the new runtime accepts Parakeet's session configuration. State this limit in the task report.

- [ ] **Step 1:** Push the branch (this is the push point). Run `gh workflow run ci.yml -f full=true --ref speaker-id`.
- [ ] **Step 2:** Watch with `gh run watch`. Expected: `windows-app` passes, including `windows_e2e`. On failure, fix and re-dispatch at most twice, then report.
- [ ] **Step 3: Commit** any CI fix as `ci: speaker e2e on Windows`.

### Task 24: Release v0.6.0 and the user's checks

**Files:**
- Modify: version via `node scripts/set-version.mjs 0.6.0`; `docs/superpowers/specs/2026-10-15-conclave-speaker-id-design.md` status line ("Implemented in v0.6.0").

- [ ] **Step 1:** Run the whole-branch review: reviewer + Codex + Antigravity (CLAUDE.md commands), fix, one recheck.
- [ ] **Step 2:** The controller asks the user before tagging. Tag `v0.6.0`; `release.yml` builds the draft. The installer size printed by the new step must be < 100 MB.
- [ ] **Step 3: User's PC checklist** (the controller sends it to the user):
  1. Install, then Settings → Speakers → on. The models download, and the line shows the backend after the first analysis.
  2. Record a 5-minute test call. Live lines show You/Others.
  3. After stop: the summary waits and the lines get names. Settings shows "DirectML" and the run time.
  4. Name the other speaker. Run Identify on an older meeting with the same person: both "You" and that person are recognised.
  5. Send the app log lines `speakers: stage timings`. If TitaNet on DirectML is slower than on CPU, set `TITANET_ON_DIRECTML = false` in a follow-up.
  6. If the live You/Others labels look wrong, send the log line `speakers: mic-only fraction` and recalibrate the level margins (12 dB / 6 dB).
- [ ] **Step 4:** If the user is unhappy with label quality, remind them of the upgrade path: a temporary system-only 16 kHz track (spec §13).
