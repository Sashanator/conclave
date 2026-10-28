# Resume recording / Continue (v1.0.2) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A recording can be added to an earlier meeting: the Resume button on a saved meeting's page, or a Continue offer for 2 minutes after a recording is saved (on the Ask banner, the Auto banner, or in the live view). The result is one meeting with one transcript, one `audio.mp4` and one summary. When a continue fails, the recording is saved as a new meeting, and the old meeting is not touched.

**Architecture:**
- Every recording runs exactly as today. "Continue" only sets a **target meeting id in Rust** (`audio/continuation.rs`). The target is bound to the current recording's number (`recordings_started()`). `api_save_transcript` reads it only on the live save (`live: true`), never on the recovery save.
- **The meeting-page Resume uses the same mechanism.** It starts a normal auto-start recording on `/` and passes the meeting id in `sessionStorage`. Once the start succeeds, `useRecordingStart` calls `continue_set_target`. So there is no second start path, and `start_recording*` keep their signatures.
- **Audio: merged into the target's single `audio.mp4`** using the bundled ffmpeg's concat with stream copy (the same helpers as checkpoint merging). Playback, speaker analysis, retention, folder-open and retranscription keep reading one file, so none of them change. Multiple files would need a multi-file player and speaker decoder, which is far more code.
- **Timestamp offset = the container duration of the target's current `audio.mp4`** (parsed from `ffmpeg -i` stderr), not the end of the last transcript line. The new part starts at that point in the merged file, so seek stays correct.
- At save, `continue_into` does these steps in order:
  1. Check eligibility.
  2. Merge into `.audio.merge.tmp.mp4` and verify it (full decode, plus the size check).
  3. In a single DB transaction, insert the shifted lines and merged notes, and requeue speakers.
  4. Swap the files (`audio.mp4` → `.audio.prev.mp4`, tmp → `audio.mp4`).
  5. Commit.
  6. After the commit, best effort: join `levels.bin`, then recycle `.audio.prev.mp4` and the new part's folder.

  An error at any step undoes the earlier steps and returns `Err`. `api_save_transcript` logs it, saves an ordinary new meeting, and returns `continue_error`, which the frontend shows as a toast.
- **Crash recovery:** the target lives only in Rust memory, and the recovery save never passes `live`. A crash therefore recovers the part as its own new meeting (spec "My calls").
- **Summary:** after a continued save the app navigates to `?id=<target>&source=recording&continued=1`. The existing auto-summary gate (`source=recording` plus the auto-summary toggle) decides whether a summary runs. If it runs, `regenerateOnAuto` replaces an existing summary through the normal Regenerate path. While it runs, the existing `processing` state is shown.

**Tech Stack:** Rust (Tauri 2, tokio, sqlx/SQLite), bundled ffmpeg, Next.js 14 + React 18 + TypeScript, Bun tests.

**Spec:** `docs/superpowers/specs/2026-10-28-resume-recording-design.md` (read it fully before any task).

**Deviations from the spec (said here, so the reviewer can check them):**
- **Live view shows only the new lines**, plus a `Continuing "<title>"` line above the transcript. Loading the old lines into `TranscriptContext` would feed them into the IndexedDB recovery and the save, which risks saving them twice. The spec allows this.
- **The 2-minute window starts when the previous meeting is saved, not when it stops.** That is a few seconds later, because the save waits for transcription. *shortcut: the window can run up to ~10 s long; measure from `recording-stopped` if that matters.*
- **The offer lives in memory only.** After an app restart nothing is offered; the Resume button still works.

## Global Constraints

- **Branch:** `fix/v1.0.2` (or a worktree branch from it per lane, merged back). Never stage `.devcontainer/*` or `FIXES.md`. Run `git add` and `git commit` as separate Bash calls.
- **Rust gate:** `. "$HOME/.cargo/env"; export CARGO_TARGET_DIR=/workspace/target; cd /workspace && nice -n 10 cargo test -j 4 -p meetily --lib <filter>`. The full lib run is needed once per task: `nice -n 10 cargo test -j 4 -p meetily --lib`. It needs `frontend/out` and the stub sidecar (AGENTS.md). Never run two cargo builds at once.
- **Frontend gate:** `cd /workspace/frontend && pnpm exec tsc --noEmit && bun test`. The 7 failures already on `main` (summary-unsaved-guard, storage-service, saveMeeting) are not ours. Report any other failure.
- **No ffmpeg in the devcontainer.** All Rust tests inject the ffmpeg step, following the pattern of `merge_leftover_checkpoints_with` (`audio/incremental_saver.rs:356`). The real `ffmpeg_join` is checked on the user's PC (see "PC check").
- **Deletion:** only through `audio/meeting_folder.rs` helpers with a `Remover` (`recycle` in prod, a fake in tests): `remove_checked`, `delete_meeting_folder`. Never `remove_file` on user audio. The single exception is our own `.audio.merge.tmp.mp4`, which may be removed with `std::fs::remove_file`.
- **Privacy:** never log transcript text, note text or titles. Log ids, counts, byte sizes and seconds.
- **Stats:** no new counting code. Talk time comes from `meeting_speakers`, which `SpeakersRepository::publish` replaces for the whole meeting (`database/repositories/speakers.rs:202`). LLM usage rows attach once, by `recording_id`. Neither counts the earlier part twice.
- **Pinned names (every slice must match):**
  - Rust module `audio/continuation.rs`, with `pub const CONTINUE_WINDOW: Duration = Duration::from_secs(120);`
  - `#[derive(Debug, Clone, PartialEq, Eq, Serialize)] pub struct ContinueMeeting { pub meeting_id: String, pub title: String }`
  - `#[derive(Serialize)] pub struct ContinueStatus { pub offer: Option<ContinueMeeting>, pub target: Option<ContinueMeeting> }`
  - Tauri commands: `continue_status() -> ContinueStatus`, `continue_set_target(meeting_id: String) -> Result<ContinueMeeting, String>`
  - Events: `continue-target-changed` (payload `ContinueMeeting`), `continue-target-failed` (payload `{ "error": String }`)
  - Temp files in the target folder: `.audio.merge.tmp.mp4`, `.audio.prev.mp4`. Both are added to `MEETING_FILES`.
  - `api_save_transcript` gains the argument `live: Option<bool>` (JS `live`). Its JSON reply gains `"continued": bool` and `"continue_error": string | null`.
  - `MeetingAudio` gains `pub resumable: bool` (JS `resumable`).
  - Banner: `BannerView` gains `pub continue_title: Option<String>`; new `kind` value `"continue"`; new action `"continue"`.
  - `sessionStorage` key `autoStartContinueId`; meeting-page query param `continued=1`.
  - Frontend lib `frontend/src/lib/resume.ts`: `requestResume`, `takeAutoStartContinueId`, `saveOutcome`.

## Review Focus

1. **The original audio is replaced only after a verified merge. Any failure leaves the old meeting byte-for-byte as it was.** Tests: Task 3, `failed_join_leaves_the_old_meeting_untouched`, `incomplete_merge_is_rejected_and_tmp_removed`.
2. **A failed continue is saved as a new meeting, with a logged error and a toast. Nothing fails silently.** Tests: Task 3, `ineligible_target_saves_a_new_meeting_with_continue_error`; Task 5, `saveOutcome surfaces continue_error`.
3. **New lines, sentences and notes shift by the old audio's duration. Old notes and marks are kept.** Tests: Task 1, `append_shifts_lines_and_sentences_after_existing`, `append_keeps_old_notes_and_marks_and_shifts_new_notes`.
4. **A target belongs to one recording. A stale target or the recovery save never continues.** Tests: Task 2, `target_is_taken_only_by_its_recording`; Task 5, `saveMeeting sends live false by default`.
5. **Imported, retention-deleted and folderless meetings are never resumable.** Tests: Task 2, `eligibility_rejects_imported_deleted_and_folderless`.

## Lanes and order

| Lane | Tasks | Starts after |
|---|---|---|
| A | Task 1 | — |
| B | Task 2, then Task 4 | — |
| C | Task 5, then Task 6 | — (frontend tests mock `invoke`; uses only the pinned names) |
| A | Task 3 | Tasks 1 and 2 merged |

At most 3 lanes run at once. Tasks 3 and 4 can run in parallel; they share no files. Task 6 follows Task 5 in one lane because both edit `page-content.tsx`.

## PC check (after all tasks, on the Windows build)

1. Record 1 min, stop, and start again within 2 min. Click Continue in the live view, record 1 min, stop. Expect: one meeting, the lines in order, seek into the second part lands on the right words, and the player shows the full length.
2. Ask mode with Zoom: leave the call, rejoin, choose `Continue "<title>"`. Auto mode: rejoin, the recording starts by itself, then click the banner's Continue.
3. Resume from the meeting page on an old meeting. With the auto-summary toggle on, the summary is replaced; with it off, it is not.
4. Rename `ffmpeg.exe` and Continue. Expect the toast "Could not add this recording to the earlier meeting", a new meeting, and the old meeting unchanged.
5. Continue onto a meeting recorded by v0.x, to check that concat works on older encoder settings.

---

### Task 1: Append lines and notes to an existing meeting (DB)

**Tag:** judgment. **Risky:** yes (persistence). It gets a per-task review plus a Codex pass, with silent-failure focus.

**Files:**
- Modify: `frontend/src-tauri/src/database/repositories/transcript.rs`: extract the segment insert loop (lines 61-111) into `insert_segments`, and add `shift_segments` and `append_in_tx`. Tests go in the module's `mod tests` (line 349).
- Modify: `frontend/src-tauri/src/database/repositories/meeting_notes.rs:145`: make `get` take any SQLite executor.
- Modify: `frontend/src-tauri/src/api/api.rs:179`: add `Clone` to `TranscriptSegment`'s derive.

**Interfaces:**
- Consumes:
  - `MeetingNotesRepository::save(conn: &mut SqliteConnection, meeting_id: &str, notes: &[MeetingNote])`
  - `sanitize_notes(&[MeetingNote]) -> Vec<MeetingNote>`
  - `SpeakersRepository::transcripts_replaced_in_tx(conn: &mut SqliteConnection, meeting_id: &str, active: bool) -> Result<bool, SqlxError>`
  - `crate::database::repositories::test_pool()`
- Produces:
  - `async fn insert_segments(conn: &mut SqliteConnection, meeting_id: &str, transcripts: &[TranscriptSegment]) -> Result<(), SqlxError>` (private, used by `save_transcript` and `append_in_tx`; same SQL, same `is_valid` sentence check, same `sanitize_mark`).
  - `pub fn shift_segments(transcripts: &[TranscriptSegment], offset: f64) -> Vec<TranscriptSegment>`: adds `offset` to `audio_start_time`, `audio_end_time` and both times of every `Sentence`; nothing else changes.
  - `pub async fn append_in_tx(conn: &mut SqliteConnection, meeting_id: &str, transcripts: &[TranscriptSegment], offset: f64, notes: &[MeetingNote], speakers_active: bool) -> Result<(), SqlxError>`:
    1. `SELECT 1 FROM meetings WHERE id = ?`; none → `Err(SqlxError::RowNotFound)`.
    2. `insert_segments(shift_segments(..))`.
    3. If `sanitize_notes(notes)` is non-empty: existing = `MeetingNotesRepository::get(&mut *conn, id)`, new = sanitized notes with `at` shifted by `offset` (`None` stays `None`), `save(conn, id, existing ++ new)`.
    4. `UPDATE meetings SET updated_at = ? WHERE id = ?`.
    5. `transcripts_replaced_in_tx(conn, id, speakers_active)`.

    It never commits; the caller owns the transaction.
  - `pub async fn get<'e, E: sqlx::SqliteExecutor<'e>>(ex: E, meeting_id: &str) -> Result<Vec<MeetingNote>, sqlx::Error>` (existing callers passing `&SqlitePool` compile unchanged).

**Steps:**
- [ ] Write failing tests in `transcript.rs` `mod tests`:
  - `append_shifts_lines_and_sentences_after_existing`: save `M` with `seg("old", 0.0)`. Append one line with start 0.0, end 2.0, sentence `(0.5, 1.5, "new")`, offset `100.0`. Read the lines `ORDER BY audio_start_time`: texts are `["old", "new"]`; the new line has start 100.0 and end 102.0, and its sentences JSON is `[[100.5,101.5,"new"]]`.
  - `append_keeps_old_notes_and_marks_and_shifts_new_notes`: save with note `n1` at `Some(5.0)` and a line marked `"important"` (any valid mark value from `sanitize_mark`). Append note `n2` at `Some(3.0)` and `n3` at `None`, offset 100.0. The notes are `n1@5.0, n2@103.0, n3@None` (in `sort_notes` order), and the old line's mark is unchanged.
  - `append_to_missing_meeting_fails_and_writes_nothing`: `append_in_tx` on `"meeting-nope"` inside a tx returns `Err`; after rollback, `SELECT COUNT(*) FROM transcripts` is 0.
  - `append_requeues_speakers_when_active`: after append with `speakers_active = true` and a commit, the `speaker_jobs` row for the meeting has `status = 'pending'`.
  - `shift_segments_moves_only_times`: text, mark and speaker_idx are unchanged; `None` times stay `None`.
- [ ] Run `nice -n 10 cargo test -j 4 -p meetily --lib repositories::transcript::` and expect a compile failure (missing functions).
- [ ] Implement the signatures above. `save_transcript` calls `insert_segments` and keeps its rollback-on-error behaviour.
- [ ] Run the filter, then the full `-p meetily --lib`, and expect both to pass.
- [ ] Commit: `feat(resume): append shifted lines and notes to an existing meeting`

---

### Task 2: Continue target, offer and eligibility

**Tag:** judgment. **Risky:** yes (untrusted meeting id and folder from the webview). It gets a per-task review.

**Files:**
- Create: `frontend/src-tauri/src/audio/continuation.rs` (state, eligibility, two commands, tests).
- Modify: `frontend/src-tauri/src/audio/mod.rs`: add `pub mod continuation;`.
- Modify: `frontend/src-tauri/src/lib.rs:773`: register `audio::continuation::continue_status` and `audio::continuation::continue_set_target`.
- Modify: `frontend/src-tauri/src/audio/meeting_folder.rs:308-331`: add `resumable` to `MeetingAudio`, set in `meeting_audio_inner`.

**Interfaces:**
- Consumes:
  - `guarded_meeting_folder(&str, &Path) -> Option<PathBuf>` (`meeting_folder.rs:48`)
  - `is_plain_file` (make it `pub(crate)`, `meeting_folder.rs:72`)
  - `MeetingsRepository::get_meeting_metadata(pool, id)` (`database/repositories/meeting.rs:145`, returns `folder_path`, `title`, `audio_deleted_at`)
  - `recording_commands::{is_recording_now, recordings_started}`
  - `get_default_recordings_folder()`
- Produces:
  - `pub(crate) struct Eligible { pub folder: PathBuf, pub title: String }`
  - `pub(crate) fn resumable_folder(folder: &Path) -> bool`: true when `folder/audio.mp4` is a plain file and `folder/metadata.json` either does not parse or has a `"source"` other than `"import"`.
  - `pub(crate) async fn eligible(pool: &SqlitePool, meeting_id: &str, base: &Path) -> Result<Eligible, String>`. Each error string is a user-facing sentence:
    - `"The earlier meeting no longer exists."`
    - `"The earlier meeting has no recording folder."`
    - `"The earlier meeting's audio was deleted."`
    - `"Imported meetings cannot be continued."`
    - `"The earlier meeting's audio.mp4 is missing."`
  - State: `static LAST: Mutex<Option<(ContinueMeeting, Instant)>>` and `static TARGET: Mutex<Option<(u64, ContinueMeeting)>>`.
    - `pub(crate) fn remember_saved(m: ContinueMeeting, at: Instant)`
    - `pub(crate) fn offer_at(now: Instant) -> Option<ContinueMeeting>` (LAST if `now - at < CONTINUE_WINDOW`)
    - `pub(crate) fn offer() -> Option<ContinueMeeting>`
    - `pub(crate) fn set_target_for(recording: u64, m: ContinueMeeting)`
    - `pub(crate) fn current_target(recording: u64) -> Option<ContinueMeeting>`
    - `pub(crate) fn take_target(recording: u64) -> Option<ContinueMeeting>` (it always clears TARGET and returns it only when the number matches)
  - `pub(crate) async fn set_target<R: Runtime>(app: &AppHandle<R>, pool: &SqlitePool, meeting_id: &str) -> Result<ContinueMeeting, String>`:
    - not `is_recording_now()` → `Err("Start a recording first.")`
    - otherwise run `eligible`, then `set_target_for(recordings_started(), ..)`, then emit `continue-target-changed`.
  - `#[tauri::command] pub async fn continue_status() -> ContinueStatus`: `offer`, and `target = current_target(recordings_started())` while recording.
  - `#[tauri::command] pub async fn continue_set_target<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<ContinueMeeting, String>`: gets the pool from `AppState` (as `get_meeting_audio` does) and calls `set_target`.
  - `MeetingAudio.resumable = folder.is_some_and(resumable_folder) && meeting.audio_deleted_at.is_none()`

**Steps:**
- [ ] Write failing tests in `continuation.rs`, using `tempdir` plus `test_pool`, and insert meetings as in `audio/retention.rs:256`:
  - `eligibility_rejects_imported_deleted_and_folderless`: cover each of the five errors above. An imported meeting gets a `metadata.json` with `{"source":"import"}` and an `audio.mp4`. A deleted meeting has `audio_deleted_at` set. A folderless meeting has `folder_path` NULL. Also cover a folder outside the base, which gives the "no recording folder" error. A plain recording with `audio.mp4` and no `metadata.json` returns `Ok` with its title.
  - `target_is_taken_only_by_its_recording`: `set_target_for(5, m)`; `take_target(6)` is `None` and also clears it, so a later `take_target(5)` is `None` too. A fresh `set_target_for(5, m)` then `take_target(5)` returns `Some(m)`.
  - `offer_expires_after_the_window`: `remember_saved(m, t0)`; `offer_at(t0 + 119 s)` is `Some`; `offer_at(t0 + 120 s)` is `None`.
  - In `meeting_folder.rs` tests: `meeting_audio_is_resumable_only_for_recorded_mp4` checks that `resumable` is true for a recorded `audio.mp4`, false for an imported `audio.mp3`, and false when `audio_deleted_at` is set.
- [ ] Run `nice -n 10 cargo test -j 4 -p meetily --lib continuation::` and expect it to fail.
- [ ] Implement it. The statics follow the `Mutex` pattern in `banner.rs:45`, recovering from a poisoned lock with `unwrap_or_else(PoisonError::into_inner)`. Tests that touch the statics share one test lock, as `banner.rs:184` does.
- [ ] Run the filter, then the full `-p meetily --lib`, and expect both to pass.
- [ ] Commit: `feat(resume): continue target, 2-minute offer and eligibility`

---

### Task 3: Merge into the target at save, with fallback

**Tag:** judgment. **Risky:** yes (it replaces user audio, recycles a folder, and handles persistence and recovery). It gets a per-task review plus a Codex pass, with silent-failure focus.

**Files:**
- Modify: `frontend/src-tauri/src/audio/continuation.rs`: add `parse_ffmpeg_duration`, `join_levels`, `ffmpeg_join`, `continue_into_with` and `continue_into`, with tests.
- Modify: `frontend/src-tauri/src/audio/incremental_saver.rs:284-323`: make `concat_line`, `concat_args`, `merged_is_complete` and `verify_chunk` `pub(crate)`.
- Modify: `frontend/src-tauri/src/audio/meeting_folder.rs:18`: change `MEETING_FILES` to `[&str; 8]`, adding `.audio.merge.tmp.mp4` and `.audio.prev.mp4`. A crash leftover then never blocks a meeting delete.
- Modify: `frontend/src-tauri/src/api/api.rs:977-1053`: add the `live` argument to `api_save_transcript`, plus the continue branch and the fallback.

**Interfaces:**
- Consumes:
  - Task 1: `append_in_tx`
  - Task 2: `eligible`, `take_target`, `remember_saved`, `ContinueMeeting`
  - `recordings_started()`
  - `remove_checked(path, folder, remove)`, `delete_meeting_folder(path, base, remove)`, `recycle`, `Remover`
  - `speakers::levels::{decode, write_levels_file, SLICE_SECS, LevelPair}`
  - `find_ffmpeg_path()`
- Produces:
  - `pub(crate) fn parse_ffmpeg_duration(stderr: &str) -> Option<f64>`: reads the first `Duration: HH:MM:SS.ff`. `N/A`, a missing duration or a non-finite value gives `None`.
  - `pub(crate) fn join_levels(old: Option<Vec<LevelPair>>, offset_secs: f64, new: Option<Vec<LevelPair>>) -> Option<Vec<LevelPair>>`:
    - both `None` → `None`.
    - otherwise: old (or empty) padded with `(0.0, 0.0)` or truncated to `(offset_secs / SLICE_SECS).round()` slices, followed by new (or nothing).
  - `async fn ffmpeg_join(old: PathBuf, new: PathBuf, tmp: PathBuf) -> Result<f64, String>`. Everything runs in `spawn_blocking` with `CREATE_NO_WINDOW` on Windows:
    1. Probe with `ffmpeg -hide_banner -i <old>` and parse the duration from stderr; a non-zero exit is expected here.
    2. Write the concat list to `<new folder>/.checkpoints/concat_list.txt` (`create_dir_all`) using `concat_line` for old, then new.
    3. Run `concat_args(list, tmp)`, then `verify_chunk(ffmpeg, tmp)`.
    4. Return the old file's duration.
  - `pub(crate) async fn continue_into_with<J, Fut>(pool: &SqlitePool, target_id: &str, new_folder: Option<&str>, transcripts: &[TranscriptSegment], notes: &[MeetingNote], speakers_active: bool, base: &Path, remove: Remover<'_>, join: J) -> Result<ContinueMeeting, String>` where `J: FnOnce(PathBuf, PathBuf, PathBuf) -> Fut, Fut: Future<Output = Result<f64, String>>`. Pinned order:
    1. `eligible(pool, target_id, base)`. The new folder must pass `guarded_meeting_folder`, differ from the target folder, and contain a plain `audio.mp4`. Otherwise `Err("This recording has no audio file to add.")`.
    2. `offset = join(old_audio, new_audio, tmp).await`. Then require `offset.is_finite() && offset > 0.0`, and `merged_is_complete(len(tmp), len(old) + len(new))`. On any failure, `remove_file(tmp)` and `Err("Merging the audio failed: <reason>")`.
    3. `pool.begin()`, then `append_in_tx(.., offset, ..)`. On error: rollback, remove tmp, `Err`.
    4. `rename(audio.mp4 → .audio.prev.mp4)`, then `rename(tmp → audio.mp4)`. If the second rename fails, rename prev back. On either failure: rollback, remove tmp, `Err`.
    5. `commit()`. On error: `rename(.audio.prev.mp4 → audio.mp4)` to restore the original. If that restore fails, `log::error!` that the original is kept as `.audio.prev.mp4`. Return `Err`.
    6. After the commit (each step's failure is `log::warn!` only, never `Err`):
       - Write `join_levels(old levels, offset, new levels)` with `write_levels_file` when it is `Some`.
       - `remove_checked(.audio.prev.mp4, target, remove)`.
       - `delete_meeting_folder(new_folder, base, remove)`.
  - `pub(crate) async fn continue_into(...)`: the same arguments without `remove` and `join`; it passes `&recycle` and `ffmpeg_join`.
  - The `api_save_transcript` argument is `live: Option<bool>`. When it is `Some(true)` and `take_target(recordings_started())` gives `Some(t)`, call `continue_into`:
    - `Ok`: the meeting id is `t.meeting_id`; `continued = true`. If speakers are active, call `speakers::job::wake()` and emit `speakers-updated`, as today.
    - `Err(e)`: `log_error!("continue into {} failed: {}", t.meeting_id, e)`, set `continue_error = Some(e)`, and fall through to the unchanged `save_transcript` path.

    `attach_recording_usage` runs for whichever id was saved. When `live` is true, call `remember_saved(ContinueMeeting { id, title }, Instant::now())` only if the saved meeting has a folder; the title is the target's title when continued. The reply adds `"continued"` and `"continue_error"`.

**Steps:**
- [ ] Write failing tests in `continuation.rs`. They use a fake `join` that writes `old bytes ++ new bytes` to `tmp` and returns `Ok(10.0)`, and a fake `Remover` like `audio/retention.rs:236`:
  - `parse_ffmpeg_duration_reads_the_container_line`: `"  Duration: 01:02:03.45, start: 0.000000"` gives `3723.45`; `"Duration: N/A"` gives `None`; `""` gives `None`.
  - `join_levels_pads_or_trims_the_old_track`: old has 3 pairs, offset `0.25` (5 slices) and new has 2 pairs, so the result is 7 pairs with pairs 3-4 `(0.0, 0.0)`. Old with 10 pairs and offset `0.25` gives 5 old pairs plus new. `(None, _, None)` gives `None`.
  - `continue_appends_and_replaces_audio_after_commit`: the target's `audio.mp4` is `old bytes ++ new bytes`; its new lines start at `10.0 + start`; the fake bin received `.audio.prev.mp4` and then the new folder's files; no `.audio.merge.tmp.mp4` remains.
  - `failed_join_leaves_the_old_meeting_untouched`: `join` returns `Err`. Afterwards the old `audio.mp4` bytes, line count and notes are unchanged, the new folder still exists, no tmp file remains, and the result is `Err` starting with `"Merging the audio failed"`.
  - `incomplete_merge_is_rejected_and_tmp_removed`: the fake writes 10 bytes against 2×100 bytes of input, so the result is `Err`, the tmp file is gone and the old audio is unchanged.
  - `missing_target_row_rolls_back_and_restores`: delete the target row after the eligibility check, which you can do in the fake `join` before it returns. The result is `Err`; the target folder's `audio.mp4` equals the old bytes and no `.audio.prev.mp4` remains.
  - `ineligible_target_saves_a_new_meeting_with_continue_error`: test this at the save level through a small `pub(crate) async fn save_live(...)` helper. Only add the helper if `api_save_transcript` needs `State` to be testable; otherwise test the branch through `continue_into_with` returning `Err`, and check that `save_transcript` creates a second meeting while the target's rows are unchanged.
- [ ] Run `nice -n 10 cargo test -j 4 -p meetily --lib continuation::` and expect it to fail.
- [ ] Implement the pinned order exactly; each step's failure path is covered by the tests above.
- [ ] Run the filter, then `audio::meeting_folder::` (the `MEETING_FILES` change), then the full `-p meetily --lib`, and expect all to pass.
- [ ] Commit: `feat(resume): merge a continued recording into the earlier meeting at save`

---

### Task 4: Banner and detector offer Continue (Ask and Auto)

**Tag:** judgment. **Risky:** no.

**Files:**
- Modify: `frontend/src-tauri/src/banner.rs`:
  - `BannerKind::Detected` gains `offer: Option<ContinueMeeting>`.
  - New `BannerKind::Continue { call: CallApp, offer: ContinueMeeting }`.
  - `BannerView.continue_title`.
  - Handling for the action `"continue"`.
- Modify: `frontend/src-tauri/src/meeting_detector.rs:122-126` (`apply`).
- Modify: `frontend/src-tauri/src/tray.rs:84,91-108`: `request_recording_start` gains `continue_id: Option<&str>`.

**Interfaces:**
- Consumes: Task 2's `offer()`, `set_target`, `ContinueMeeting`, plus `meeting_detector::busy()` and `is_recording_now()`.
- Produces:
  - `pub fn request_recording_start<R: Runtime>(app: &AppHandle<R>, title_prefix: Option<&str>, continue_id: Option<&str>)`. When `continue_id` is `Some`, the eval also runs `sessionStorage.setItem('autoStartContinueId', <JSON-encoded id>)`. Update all three callers (`tray.rs:84`, `banner.rs:156` and `meeting_detector.rs:125`) to pass `None`, except the new continue paths.
  - `view()`: `Detected { offer: Some(o) }` → `kind "detected"`, `continue_title Some(o.title)`. `Continue { call, offer }` → `kind "continue"`, `app call.label()`, `continue_title Some(offer.title)`. The other kinds give `continue_title None`.
  - `meeting_banner_action`, action `"continue"`:
    - `Detected { offer: Some(o), call }`: the same `busy()` guard as `"record"`, then `request_recording_start(&app, Some(call.label()), Some(&o.meeting_id))`.
    - `Continue { offer }`: `set_target(&app, pool, &offer.meeting_id)`. On `Err(e)`: `log::warn!` and `emit_to("main", "continue-target-failed", {"error": e})`.
  - `apply`:
    - `Prompt` → `BannerKind::Detected { call, certain, offer: continuation::offer() }`.
    - `Start` → `request_recording_start(app, Some(call.label()), None)`; then, if `offer()` is `Some(o)`, spawn a task that polls `is_recording_now()` every 500 ms, up to 60 times. Once the recording is live it calls `banner::show(app, BannerKind::Continue { call, offer: o })`; otherwise it shows nothing. The banner appears only once the recording is live, so the click can bind the target at once.

**Steps:**
- [ ] Write failing tests in `banner.rs` `mod tests`:
  - `view_texts` (updated): `Detected { offer: None }` keeps `("detected", "Google Meet", true)` and `continue_title None`.
  - `detected_with_offer_shows_continue_title`: gives `continue_title == Some("Standup")`.
  - `continue_view`: `Continue { call: Zoom, offer }` gives `kind "continue"` and `continue_title Some("Standup")`.
- [ ] Run `nice -n 10 cargo test -j 4 -p meetily --lib banner::` and expect it to fail.
- [ ] Implement, and update every `BannerKind::Detected { .. }` pattern (use `rg "BannerKind::Detected"`).
- [ ] Run the filter, then the full `-p meetily --lib`, and `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins`. Expect all to pass.
- [ ] Commit: `feat(resume): Continue on the meeting banner in Ask and Auto mode`

---

### Task 5: Frontend save path and summary replacement

**Tag:** mechanical. **Risky:** no. It touches the save call, but every data decision is in Rust.

**Files:**
- Create: `frontend/src/lib/resume.ts`, `frontend/tests/lib/resume.test.ts`.
- Modify: `frontend/src/services/storageService.ts:18-60`: add `live` to `saveMeeting` and the two reply fields to `SaveMeetingResponse`.
- Modify: `frontend/src/hooks/useRecordingStop.ts:257-355`.
- Modify: `frontend/src/app/meeting-details/page.tsx:26,285-292`.
- Modify: `frontend/src/app/meeting-details/page-content.tsx:232-264`.
- Test: `frontend/tests/lib/storage-service.test.ts`, `frontend/tests/components/summary-speaker-gate.test.tsx`.

**Interfaces:**
- Consumes: the `api_save_transcript` reply `{ meeting_id, continued, continue_error }` (Task 3).
- Produces:
  - `SaveMeetingResponse { meeting_id: string; continued?: boolean; continue_error?: string | null }`
  - `saveMeeting(meetingTitle, transcripts, folderPath, notes = [], recordingId = null, titleOrigin = { source: 'default' }, live = false)` sends `live` in the invoke args.
  - `export function saveOutcome(r: SaveMeetingResponse): { url: string; continued: boolean; continueError: string | null }` with `url = /meeting-details?id=<id>&source=recording` plus `&continued=1` when continued.
  - `useRecordingStop`:
    - Pass `live = true`.
    - When `continueError` is set: `toast.error('Could not add this recording to the earlier meeting', { description: `${continueError} It was saved as a new meeting instead.` })`.
    - Skip `applyPinnedSummaryLanguageToMeeting` / `detectAndCacheSummaryLanguage` when continued.
    - The success toast description is `'Added to the earlier meeting.'` when continued.
    - Navigate to `saveOutcome(..).url`.
  - `PageContent` prop `regenerateOnAuto?: boolean`. `page.tsx` passes `searchParams.get('continued') === '1'`. The auto effect also runs when `regenerateOnAuto && (status === 'completed' || status === 'error')`, and then calls `regenerateSummary()` instead of `handleGenerateSummary('')`. Every other gate (`shouldAutoGenerate`, `speakerGateOpen`, the model config, the one-way latch) stays as it is.

**Steps:**
- [ ] Write failing tests:
  - `resume.test.ts`:
    - `saveOutcome builds the meeting url`: a plain save gives `'/meeting-details?id=m1&source=recording'`; a continued save gives the same plus `'&continued=1'`.
    - `saveOutcome surfaces continue_error`: `{ meeting_id: 'm2', continue_error: 'Merging the audio failed: x' }` gives `continueError === 'Merging the audio failed: x'` and `continued === false`.
  - `storage-service.test.ts`:
    - `saveMeeting sends live false by default`: `expect.objectContaining({ live: false })`.
    - `saveMeeting sends live true from the live stop`.
  - `summary-speaker-gate.test.tsx`: `a continued meeting with a completed summary regenerates it`. Render with `shouldAutoGenerate` and `regenerateOnAuto`, with an initial completed summary; `api_process_transcript` is invoked once. Without `regenerateOnAuto` it is not invoked.
- [ ] Run `cd frontend && bun test tests/lib/resume.test.ts tests/lib/storage-service.test.ts tests/components/summary-speaker-gate.test.tsx` and expect the new tests to fail.
- [ ] Implement.
- [ ] Run `pnpm exec tsc --noEmit && bun test` and expect no failures beyond the known 7.
- [ ] Commit: `feat(resume): save into the target meeting and replace its summary`

---

### Task 6: Resume button, live-view Continue, banner buttons

**Tag:** mechanical. **Risky:** no.

**Files:**
- Modify: `frontend/src/lib/resume.ts` (add two functions) and `frontend/tests/lib/resume.test.ts`.
- Create: `frontend/src/app/_components/ContinueControl.tsx`, `frontend/tests/components/continue-control.test.tsx`.
- Modify: `frontend/src/app/_components/LiveTranscriptAndNotes.tsx:55-58`: render `{isRecording && <ContinueControl />}` inside the transcript column, before `<TranscriptPanel>`.
- Modify: `frontend/src/hooks/useRecordingStart.ts:208-260`: take the continue id with the title prefix; after a successful start, set the target.
- Modify: `frontend/src/lib/meeting-audio.ts:17-21`: add `resumable: boolean` to `MeetingAudioInfo`.
- Modify: `frontend/src/components/MeetingDetails/TranscriptButtonGroup.tsx:12-27,62+`, and pass the prop through `TranscriptPanel.tsx` and `page-content.tsx:296-322`.
- Modify: `frontend/src/app/banner/page.tsx:8,30,54-78`, `frontend/tests/components/banner-mention.test.tsx` (or a new `banner-continue.test.tsx`).

**Interfaces:**
- Consumes: the `continue_status`, `continue_set_target`, `continue-target-changed` and `continue-target-failed` names (Task 2/4), `MeetingAudioInfo.resumable` (Task 2), and `BannerView.continue_title` with kind `'continue'` (Task 4).
- Produces:
  - `export function requestResume(meetingId: string, push: (url: string) => void): void` sets `autoStartContinueId = meetingId` and `autoStartRecording = 'true'` in `sessionStorage`, then calls `push('/')`.
  - `export function takeAutoStartContinueId(storage: Storage): string | null` reads the key and removes it.
  - `useRecordingStart`, auto-start branch: `const continueId = takeAutoStartContinueId(sessionStorage)`, next to `autoStartTitlePrefix` (it is removed even when the model is not ready). After `startRecordingWithDevices` resolves, if `continueId` is set: `invoke('continue_set_target', { meetingId: continueId })`, with `.catch((e) => toast.error('Could not continue the earlier meeting', { description: `${e} This recording will be saved as a new meeting.` }))`.
  - `ContinueControl`:
    - On mount, `invoke<ContinueStatus>('continue_status')`. It listens to `continue-target-changed`, which sets the target, and to `continue-target-failed`, which shows the same toast.
    - Rendering: a target gives the text `Continuing "<title>"`; otherwise an offer gives a `<button>` labelled `Continue "<title>"`, which calls `invoke('continue_set_target', { meetingId })` and on success sets the target; with neither it renders `null`.
  - `TranscriptButtonGroup` prop `onResumeRecording?: () => void`. When it is set, the group shows a `Resume recording` button.
  - `page-content.tsx` passes `onResumeRecording = audio.info?.resumable && !recordingNow ? () => requestResume(meeting.id, router.push) : undefined`, where `recordingNow = useRecordingState().isRecording` and `router = useRouter()`.
  - Banner page:
    - `kind` gains `'continue'`; `act` gains `'continue'`.
    - `detected` with `continue_title` shows `Record new` (action `record`) and `Continue "<title>"` (action `continue`).
    - `detected` without it shows `Record`, as today.
    - `continue` shows the text `<app>: still the same call?`, a `Continue "<title>"` button and the dismiss X.
    - Text is plain React text nodes.

**Steps:**
- [ ] Write failing tests:
  - `resume.test.ts`:
    - `requestResume sets both keys and opens home`.
    - `takeAutoStartContinueId reads once`: the second call is `null`.
  - `continue-control.test.tsx` (mock `invoke` and `listen` as in `banner-mention.test.tsx`):
    - `shows Continue for an offer and binds it on click`: the click invokes `continue_set_target` with `{ meetingId: 'm1' }`, after which the text contains `Continuing "Standup"`.
    - `shows Continuing when a target is already set`.
    - `renders nothing without offer or target`.
  - Banner:
    - `detected with an offer shows Record new and Continue`: clicking Continue invokes `meeting_banner_action` with `{ action: 'continue' }`.
    - `continue banner shows only Continue and dismiss`.
    - `detected without an offer still shows Record`.
  - `TranscriptButtonGroup`: `Resume recording shows only with a handler and calls it`. Put it in `tests/components/meeting-details-layout.test.tsx`, or in a new file if that one does not render the group.
- [ ] Run the new test files and expect them to fail.
- [ ] Implement.
- [ ] Run `pnpm exec tsc --noEmit && bun test` and expect no failures beyond the known 7. Also run `pnpm run build` once, since this task adds a client component.
- [ ] Commit: `feat(resume): Resume button, live-view Continue and banner buttons`

---

## Done when

- All six tasks are committed. Then run the full check: `cargo test --workspace --exclude llama-helper`; `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`; `bash scripts/check-no-phone-home.sh`. No new network calls.
- Codex passes are done on Tasks 1 and 3 and on the whole branch.
- The PC check above has been reported to the user. Items 1, 4 and 5 are the data-safety checks.
