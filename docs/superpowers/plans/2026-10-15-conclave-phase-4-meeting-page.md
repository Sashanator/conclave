# Conclave Phase 4 (v0.5.0): Live Notes, Transcript Marks, Click-to-Play, Old-Audio Review — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Notes and ⭐/✅/❓ line marks during and after a call that are woven into the summary; click a timestamp to play the recording from there; a daily reminder and a review list for recordings older than N days (default 30, per-meeting pin) whose confirmed audio goes to the Recycle Bin; deleting a meeting moves its folder to the Recycle Bin. Conclave never deletes anything by itself and never outside `Music\conclave-recordings`.

**Architecture:** Marks are a `transcripts.mark` column and notes reuse the unused `meeting_notes` table. During a recording both live in `TranscriptContext` and in the IndexedDB recovery record, and are saved with the transcript at stop in the existing single transaction. One Rust folder guard (canonical path strictly inside the recordings folder) protects every file operation, and one `recycle` function (the `trash` crate) is the only way anything is deleted. Playback grants the asset protocol one file at runtime and plays it in an `<audio>` element. Old audio is only listed by the backend; the frontend reminds once a day and deletes exactly what the user ticks. The summary gets marks inline in the transcript text and notes as a `<user_notes>` block read from the DB.

**User decisions after the plan review (2026-10-15):** no automatic deletion — a reminder plus a review list where the user ticks what to delete; deletions go to the Windows Recycle Bin; the folder guard is the recordings folder only (the `metadata.json` rule is gone); this ships as **v0.5.0** (Phase 3 is tagged v0.4.0 first).

**Changes after the Codex plan review (2026-10-15), also reflected in the spec:** meeting-folder deletion is never recursive (known files only, one by one, then the empty folder); re-transcription carries marks to the best-overlapping new line; with chunked local summaries the marked lines are passed again in `<marked_lines>`; the asset-protocol scope is emptied; the review deletes exactly the confirmed meetings.

**Deviations from the spec (implementation detail, same behaviour):** live marks are kept on the IndexedDB *meeting* record keyed by `sequence_id` (not by rewriting each line's record); notes and `keep_audio` reach the saved page through `get_meeting_notes` and `get_meeting_audio` rather than `get_meeting`.

**Tech Stack:** Rust (Tauri 2.11, sqlx/SQLite, serde, tokio, chrono, `dunce`, `trash` (new, Recycle Bin), `tempfile` in tests), Next.js 14 + React 18 + TypeScript, Radix popover/tabs/select/switch from `components/ui`, Bun tests (happy-dom).

**Spec:** `docs/superpowers/specs/2026-10-15-conclave-meeting-page-design.md` (read it before any task).

## Global Constraints

- Target Windows 10 x64; development and tests run on Linux (AGENTS.md "Verified on Linux").
- Local builds: `nice -n 15 cargo test ... -j 4` with `CARGO_TARGET_DIR=/workspace/target`. Never two cargo builds at once.
- No new outbound network calls. No telemetry.
- Mark values: exactly `important`, `action`, `followup` (or null). UI icons: ⭐ important, ✅ action, ❓ follow-up.
- Note: `{ id, at, text }`; `at` = seconds into the recording (`null` = added after the meeting); text trimmed, non-empty, at most 2,000 characters; id non-empty, at most 64 characters, generated as `note-<uuid>`.
- Prompt label for marks: `[MARKED: important]` / `[MARKED: action]` / `[MARKED: followup]`. Notes block tag: `<user_notes>`; a note without a time renders as `[after the meeting]`.
- Old-audio reminder options: Never / 7 / 30 / 90 days, default 30 (`old_audio_days`). Settings line: "Conclave never deletes audio by itself. Transcripts, summaries and notes are always kept." Reminder at most once a day.
- Conclave never deletes on its own: only `delete_old_audio` (after the user ticks meetings in the review) and meeting delete remove files, always through `recycle` (Recycle Bin), and only inside the canonical recordings folder. The review deletes only top-level `audio.<ext>` (ext from the existing `crate::audio::constants::AUDIO_EXTENSIONS`: mp4, m4a, wav, mp3, flac, ogg, aac, mkv, webm, wma) and Conclave's own files in `.checkpoints/` (`audio_chunk_*`, `concat_list.txt`). Never `transcripts.json`, `metadata.json` or anything else. Files are recycled one at a time after a per-file re-check, folders only when empty; nothing is recursive, production code never calls `fs::remove_*`, links/junctions are never followed, and a folder used by two meetings is never touched: `folder_path` is supplied by the webview at save time.
- Every file operation on a meeting folder goes through `guarded_meeting_folder` (Task 3).
- localStorage keys: `conclave.meetingPageLayout`, `conclave.notesPaneCollapsed`, `conclave.live.transcriptRatio`, `conclave.threeCol.transcriptRatio`, `conclave.threeCol.notesRatio`; the existing `meetily.meetingDetails.transcriptPaneRatio` stays the sub-tab layout's key.
- Layout values: `three-columns` (default) | `subtab`.
- Never commit `.claude/settings.json`, `.devcontainer/*` or anything under `.superpowers/`. Stage files by name; never `git commit -a`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Save at stop must never fail because of a note or mark.** An empty note, a 5,000-character note, a `NaN` time or an unknown mark string arriving in `api_save_transcript` must be dropped/truncated, and the meeting still saved — test in Task 1 (`save_transcript_sanitizes_bad_notes_and_marks`).
2. **A `folder_path` pointing outside the recordings folder** (`C:\`, the user's home, a folder with a `metadata.json` elsewhere, a path with `..`, a link inside the base pointing out, a file instead of a folder) must never be deleted, recycled or exposed to the webview — tests in Task 3.
3. **Nothing is deleted without the user ticking it, and nothing while recording**: the review deletes only confirmed ids that are still listed, refuses while `is_recording_now()` is true, and a Recycle Bin failure (e.g. the user declines Windows' "too big for the Recycle Bin" prompt) leaves the files — tests in Task 4 (`delete_removes_only_confirmed_audio_and_marks_the_meeting`, `delete_is_refused_while_recording_and_reports_bin_failures`) and Task 11.
4. **An app reload during a recording** must keep notes and marks (reload sync rebuilds transcripts from the backend, which knows nothing about them) — `collectMarks`/`applyMarks` round-trip test in Task 6, restore wiring in Task 9 Step 5.
5. **Selecting transcript text to copy it** must not open the mark popover, and Enter/Space/Escape must work from the keyboard — test in Task 8.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `frontend/src-tauri/migrations/20261015000000_meeting_page.sql` (new) | `transcripts.mark`, `meetings.keep_audio` | 1 |
| `frontend/src-tauri/src/database/repositories/meeting_notes.rs` (new) | Note type, validation/sanitizing, notes read/write | 1 |
| `frontend/src-tauri/src/database/repositories/{transcript,meeting,mod}.rs`, `database/models.rs`, `api/api.rs` | Save marks+notes at stop, mark/pin updates, explicit notes delete | 1 |
| `frontend/src-tauri/src/api/meeting_page.rs` (new), `api/mod.rs`, `lib.rs` | Tauri commands for marks, notes, pin | 2 |
| `frontend/src-tauri/src/audio/meeting_folder.rs` (new) | Folder guard, `recycle`, audio-file lookup, folder delete, `get_meeting_audio` | 3, 5 |
| `frontend/src-tauri/src/audio/retention.rs` (new), `audio/recording_preferences.rs` | `old_audio_days`, list old audio, delete confirmed audio | 4 |
| `frontend/src-tauri/tauri.conf.json`, `lib.rs`, dead frontend files | CSP `media-src`, remove `read_audio_file`, dead player and notes page | 5 |
| `frontend/src-tauri/src/summary/user_notes.rs` (new), `summary/processor.rs`, `summary/service.rs` | `<user_notes>` block and instruction | 6 |
| `frontend/src/lib/meeting-notes.ts` (new), `frontend/src/types/index.ts`, `hooks/meeting-details/useSummaryGeneration.ts` | Shared note/mark helpers, inline marks in the summary payload | 6 |
| `frontend/src/lib/meeting-page-layout.ts` (new), `frontend/src/hooks/useSplitRatio.tsx` (new), `components/MeetingDetails/MeetingDetailsSplitView.tsx` | Layout setting, reusable splitter | 7 |
| `frontend/src/components/VirtualizedTranscriptView.tsx`, `frontend/src/components/TranscriptMarkButton.tsx` (new) | Mark popover, clickable timestamps, active line | 8 |
| `frontend/src/components/MeetingNotes/NotesPane.tsx` (new), `contexts/TranscriptContext.tsx`, `services/{indexedDBService,storageService}.ts`, `hooks/{useRecordingStop,useTranscriptRecovery}.ts`, `app/page.tsx`, `app/_components/TranscriptPanel.tsx` | Live notes and marks | 9 |
| `frontend/src/hooks/{useMeetingNotes,useMeetingAudio,usePaginatedTranscripts}.ts`, `components/MeetingDetails/{AudioPlayerBar,TranscriptPanel,TranscriptButtonGroup,MeetingDetailsSplitView}.tsx`, `app/meeting-details/page-content.tsx`, `lib/meeting-audio.ts` (new), `components/PreferenceSettings.tsx` | Saved page: notes, marks, player, both layouts | 10 |
| `frontend/src/lib/old-audio.ts` (new), `components/OldAudio/*` (new), `components/RecordingSettings.tsx`, `app/layout.tsx`, `components/Sidebar/index.tsx` | Review dialog, daily reminder, setting, delete warning | 11 |
| `.github/workflows/release.yml` | 0.5.0 release notes | 12 |

---

### Task 1: Data layer — marks, notes, pin

**Files:**
- Create: `frontend/src-tauri/migrations/20261015000000_meeting_page.sql`
- Create: `frontend/src-tauri/src/database/repositories/meeting_notes.rs`
- Modify: `frontend/src-tauri/src/database/repositories/mod.rs`, `database/repositories/transcript.rs`, `database/repositories/meeting.rs`, `database/models.rs`, `api/api.rs` (`TranscriptSegment`, `MeetingTranscript`, `api_save_transcript`, the two `MeetingTranscript` mappings), `audio/import.rs` (test literals), `audio/common.rs` (production literal at line ~59), `audio/retranscription.rs` (keep marks, Step 8)
- Test: `#[cfg(test)] mod tests` in `meeting_notes.rs` and `transcript.rs`

**Interfaces:**
- Produces:
  - `pub struct MeetingNote { pub id: String, pub at: Option<f64>, pub text: String }` (`Serialize, Deserialize, Clone, Debug, PartialEq`)
  - `pub const NOTE_MAX_CHARS: usize = 2000;` `pub const MARKS: [&str; 3]`
  - `validate_mark(Option<&str>) -> Result<(), String>`, `validate_notes(&[MeetingNote]) -> Result<Vec<MeetingNote>, String>`, `sanitize_notes(&[MeetingNote]) -> Vec<MeetingNote>`, `sanitize_mark(Option<&str>) -> Option<String>`, `sort_notes(&mut [MeetingNote])`
  - `parse_notes(values: Vec<serde_json::Value>) -> Vec<MeetingNote>` (drops entries that do not decode, logs them)
  - `pub struct TimedMark { pub start: f64, pub end: f64, pub mark: String }` and `carry_marks(old: &[TimedMark], new: &[(String, f64, f64)]) -> Vec<(String, String)>` (new segment id, mark)
  - `MeetingNotesRepository::get(&SqlitePool, &str) -> Result<Vec<MeetingNote>, sqlx::Error>`, `MeetingNotesRepository::save(&mut SqliteConnection, &str, &[MeetingNote]) -> Result<(), sqlx::Error>`
  - `TranscriptsRepository::save_transcript(pool, title, transcripts, folder_path, notes: &[MeetingNote]) -> Result<String, SqlxError>`
  - `TranscriptsRepository::set_mark(&SqlitePool, transcript_id: &str, mark: Option<&str>) -> Result<bool, SqlxError>`
  - `MeetingsRepository::set_keep_audio(&SqlitePool, meeting_id: &str, keep: bool) -> Result<bool, SqlxError>`
  - `MeetingModel.keep_audio: bool`, `MeetingModel.audio_deleted_at: Option<String>` (RFC 3339), `Transcript.mark: Option<String>`, `api::TranscriptSegment.mark: Option<String>`, `api::MeetingTranscript.mark: Option<String>`
  - `#[cfg(test)] pub(crate) async fn test_pool() -> SqlitePool` in `database/repositories/mod.rs`

- [ ] **Step 1: Migration**

`frontend/src-tauri/migrations/20261015000000_meeting_page.sql`:

```sql
-- Phase 4 (spec §4): one mark per transcript line, the per-meeting "Keep audio" pin,
-- and when the user deleted the audio in the old-audio review.
ALTER TABLE transcripts ADD COLUMN mark TEXT;
ALTER TABLE meetings ADD COLUMN keep_audio INTEGER NOT NULL DEFAULT 0;
ALTER TABLE meetings ADD COLUMN audio_deleted_at TEXT;
```

- [ ] **Step 2: Test pool helper** — append to `database/repositories/mod.rs`:

```rust
pub mod meeting_notes;

/// One connection, so every query in a test sees the same in-memory database.
#[cfg(test)]
pub(crate) async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}
```

- [ ] **Step 3: Write the failing tests** — `meeting_notes.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, at: Option<f64>, text: &str) -> MeetingNote {
        MeetingNote { id: id.into(), at, text: text.into() }
    }

    #[test]
    fn validate_notes_trims_and_rejects_bad_input() {
        let ok = validate_notes(&[note("note-1", Some(3.0), "  ask about Q2  ")]).unwrap();
        assert_eq!(ok[0].text, "ask about Q2");
        assert!(validate_notes(&[note("note-1", None, "   ")]).is_err());
        assert!(validate_notes(&[note("note-1", None, &"x".repeat(2001))]).is_err());
        assert!(validate_notes(&[note("note-1", None, &"x".repeat(2000))]).is_ok());
        assert!(validate_notes(&[note("", None, "a")]).is_err());
        assert!(validate_notes(&[note(&"i".repeat(65), None, "a")]).is_err());
        assert!(validate_notes(&[note("note-1", Some(f64::NAN), "a")]).is_err());
        assert!(validate_notes(&[note("note-1", Some(-1.0), "a")]).is_err());
    }

    #[test]
    fn sanitize_notes_never_fails() {
        let out = sanitize_notes(&[
            note("note-1", Some(f64::NAN), " keep "),
            note("note-2", None, "   "),
            note("", Some(5.0), &"y".repeat(5000)),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], note("note-1", None, "keep"));
        assert_eq!(out[1].text.chars().count(), NOTE_MAX_CHARS);
        assert!(out[1].id.starts_with("note-"));
        assert_eq!(out[1].at, Some(5.0));
    }

    #[test]
    fn marks_are_checked() {
        assert!(validate_mark(None).is_ok());
        for m in MARKS { assert!(validate_mark(Some(m)).is_ok()); }
        assert!(validate_mark(Some("IMPORTANT")).is_err());
        assert_eq!(sanitize_mark(Some("action")), Some("action".into()));
        assert_eq!(sanitize_mark(Some("bogus")), None);
    }

    #[test]
    fn parse_notes_drops_entries_that_do_not_decode() {
        let v = vec![
            serde_json::json!({"id": "n1", "at": 1.0, "text": "ok"}),
            serde_json::json!({"id": "n2", "at": "soon", "text": "bad time"}),
            serde_json::json!({"id": "n3"}),
            serde_json::json!({"id": "n4", "at": null, "text": "later"}),
        ];
        let ids: Vec<_> = parse_notes(v).into_iter().map(|n| n.id).collect();
        assert_eq!(ids, ["n1", "n4"]);
    }

    #[test]
    fn carry_marks_moves_each_mark_to_the_best_overlapping_segment() {
        let old = vec![
            TimedMark { start: 10.0, end: 14.0, mark: "action".into() },
            TimedMark { start: 30.0, end: 31.0, mark: "important".into() },
            TimedMark { start: 100.0, end: 101.0, mark: "followup".into() }, // no overlap: nearest start
        ];
        let new = vec![
            ("a".to_string(), 9.0, 11.0),
            ("b".to_string(), 11.0, 15.0),
            ("c".to_string(), 29.0, 33.0),
            ("d".to_string(), 90.0, 95.0),
        ];
        assert_eq!(
            carry_marks(&old, &new),
            vec![("b".to_string(), "action".to_string()), ("c".to_string(), "important".to_string()), ("d".to_string(), "followup".to_string())]
        );
        assert!(carry_marks(&old, &[]).is_empty());
    }

    #[test]
    fn carry_marks_keeps_the_first_mark_when_two_land_on_one_segment() {
        let old = vec![
            TimedMark { start: 1.0, end: 2.0, mark: "action".into() },
            TimedMark { start: 2.0, end: 3.0, mark: "important".into() },
        ];
        let new = vec![("a".to_string(), 0.0, 5.0)];
        assert_eq!(carry_marks(&old, &new), vec![("a".to_string(), "action".to_string())]);
    }

    #[test]
    fn sort_puts_untimed_notes_last_and_keeps_order() {
        let mut v = vec![note("a", None, "a"), note("b", Some(9.0), "b"), note("c", None, "c"), note("d", Some(2.0), "d")];
        sort_notes(&mut v);
        let ids: Vec<_> = v.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["d", "b", "a", "c"]);
    }

    #[tokio::test]
    async fn save_replaces_and_get_reads_back() {
        let pool = crate::database::repositories::test_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        MeetingNotesRepository::save(&mut conn, "m1", &[note("n1", None, "later"), note("n2", Some(1.0), "first")]).await.unwrap();
        MeetingNotesRepository::save(&mut conn, "m1", &[note("n3", Some(2.0), "only")]).await.unwrap();
        drop(conn);
        assert_eq!(MeetingNotesRepository::get(&pool, "m1").await.unwrap(), vec![note("n3", Some(2.0), "only")]);
        assert!(MeetingNotesRepository::get(&pool, "missing").await.unwrap().is_empty());
    }
}
```

`transcript.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::meeting_notes::{MeetingNote, MeetingNotesRepository};

    fn seg(text: &str, start: f64, mark: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: "x".into(), text: text.into(), timestamp: "t".into(),
            audio_start_time: Some(start), audio_end_time: Some(start + 1.0), duration: Some(1.0),
            mark: mark.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn save_transcript_stores_marks_and_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![MeetingNote { id: "note-1".into(), at: Some(1.5), text: "ask".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, Some("action")), seg("b", 2.0, None)], None, &notes).await.unwrap();
        let marks: Vec<(Option<String>,)> = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time")
            .bind(&id).fetch_all(&pool).await.unwrap();
        assert_eq!(marks, vec![(Some("action".into()),), (None,)]);
        assert_eq!(MeetingNotesRepository::get(&pool, &id).await.unwrap(), notes);
    }

    #[tokio::test]
    async fn save_transcript_sanitizes_bad_notes_and_marks() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![
            MeetingNote { id: "note-1".into(), at: Some(f64::NAN), text: "  ".into() },
            MeetingNote { id: "note-2".into(), at: None, text: "z".repeat(5000) },
        ];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, Some("nonsense"))], None, &notes).await.unwrap();
        let (mark,): (Option<String>,) = sqlx::query_as("SELECT mark FROM transcripts WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert_eq!(mark, None);
        let saved = MeetingNotesRepository::get(&pool, &id).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].text.chars().count(), 2000);
    }

    #[tokio::test]
    async fn set_mark_and_keep_audio_round_trip() {
        let pool = crate::database::repositories::test_pool().await;
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, None)], None, &[]).await.unwrap();
        let (tid,): (String,) = sqlx::query_as("SELECT id FROM transcripts WHERE meeting_id = ?").bind(&id).fetch_one(&pool).await.unwrap();
        assert!(TranscriptsRepository::set_mark(&pool, &tid, Some("important")).await.unwrap());
        assert!(!TranscriptsRepository::set_mark(&pool, "nope", Some("important")).await.unwrap());
        let (m,): (Option<String>,) = sqlx::query_as("SELECT mark FROM transcripts WHERE id = ?").bind(&tid).fetch_one(&pool).await.unwrap();
        assert_eq!(m.as_deref(), Some("important"));
        assert!(crate::database::repositories::meeting::MeetingsRepository::set_keep_audio(&pool, &id, true).await.unwrap());
        let meta = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(&pool, &id).await.unwrap().unwrap();
        assert!(meta.keep_audio);
    }

    #[tokio::test]
    async fn delete_meeting_removes_notes() {
        let pool = crate::database::repositories::test_pool().await;
        let notes = vec![MeetingNote { id: "note-1".into(), at: None, text: "x".into() }];
        let id = TranscriptsRepository::save_transcript(&pool, "M", &[seg("a", 0.0, None)], None, &notes).await.unwrap();
        assert!(crate::database::repositories::meeting::MeetingsRepository::delete_meeting(&pool, &id).await.unwrap());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meeting_notes").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- meeting_notes repositories::transcript`
Expected: compile errors (`meeting_notes` module, `mark` field, new `save_transcript` argument do not exist).

- [ ] **Step 5: Implement `meeting_notes.rs`**

```rust
//! User notes for a meeting (spec §4). Stored as JSON in the existing `meeting_notes` table.
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};
use std::cmp::Ordering;
use tracing::warn;

pub const NOTE_MAX_CHARS: usize = 2000;
pub const MARKS: [&str; 3] = ["important", "action", "followup"];
const NOTE_ID_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingNote {
    pub id: String,
    /// Seconds into the recording; `None` = added after the meeting.
    pub at: Option<f64>,
    pub text: String,
}

fn valid_at(at: f64) -> bool {
    at.is_finite() && at >= 0.0
}

/// For commands after the meeting: reject bad input with a message.
pub fn validate_mark(mark: Option<&str>) -> Result<(), String> {
    match mark {
        Some(m) if !MARKS.contains(&m) => Err(format!("Unknown mark: {m}")),
        _ => Ok(()),
    }
}

/// For commands after the meeting: trims text, rejects anything invalid.
pub fn validate_notes(notes: &[MeetingNote]) -> Result<Vec<MeetingNote>, String> {
    notes
        .iter()
        .map(|n| {
            let text = n.text.trim();
            if text.is_empty() {
                return Err("A note cannot be empty".to_string());
            }
            if text.chars().count() > NOTE_MAX_CHARS {
                return Err(format!("A note can be at most {NOTE_MAX_CHARS} characters"));
            }
            if n.id.trim().is_empty() || n.id.len() > NOTE_ID_MAX {
                return Err("Invalid note id".to_string());
            }
            if n.at.is_some_and(|a| !valid_at(a)) {
                return Err("Invalid note time".to_string());
            }
            Ok(MeetingNote { id: n.id.clone(), at: n.at, text: text.to_string() })
        })
        .collect()
}

/// For the save at stop: never lose a meeting over a bad note. Drops empty notes,
/// truncates long ones, drops invalid times, replaces invalid ids.
pub fn sanitize_notes(notes: &[MeetingNote]) -> Vec<MeetingNote> {
    notes
        .iter()
        .filter_map(|n| {
            let text = n.text.trim();
            if text.is_empty() {
                return None;
            }
            let id = if n.id.trim().is_empty() || n.id.len() > NOTE_ID_MAX {
                format!("note-{}", uuid::Uuid::new_v4())
            } else {
                n.id.clone()
            };
            Some(MeetingNote {
                id,
                at: n.at.filter(|a| valid_at(*a)),
                text: text.chars().take(NOTE_MAX_CHARS).collect(),
            })
        })
        .collect()
}

/// For the save at stop: an unknown mark is dropped, not an error.
pub fn sanitize_mark(mark: Option<&str>) -> Option<String> {
    mark.filter(|m| MARKS.contains(m)).map(str::to_string)
}

/// By recording time; notes added after the meeting go last. Stable.
pub fn sort_notes(notes: &mut [MeetingNote]) {
    notes.sort_by(|a, b| match (a.at, b.at) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
}

/// Decodes notes one by one, so one malformed note never fails the save at stop.
pub fn parse_notes(values: Vec<serde_json::Value>) -> Vec<MeetingNote> {
    values
        .into_iter()
        .filter_map(|v| match serde_json::from_value::<MeetingNote>(v) {
            Ok(n) => Some(n),
            Err(e) => {
                warn!("Dropping a note that could not be read: {e}");
                None
            }
        })
        .collect()
}

/// A mark on a transcript line before re-transcription.
pub struct TimedMark {
    pub start: f64,
    pub end: f64,
    pub mark: String,
}

/// Re-transcription replaces every line; each old mark moves to the new line that overlaps
/// it most (no overlap: nearest start). If two marks land on one line, the first one wins.
pub fn carry_marks(old: &[TimedMark], new: &[(String, f64, f64)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for m in old {
        let overlap = |s: f64, e: f64| (e.min(m.end) - s.max(m.start)).max(0.0);
        let best = new
            .iter()
            .max_by(|a, b| {
                overlap(a.1, a.2)
                    .partial_cmp(&overlap(b.1, b.2))
                    .unwrap_or(Ordering::Equal)
                    // equal overlap: prefer the nearer start (max_by keeps the later max, so invert)
                    .then_with(|| (b.1 - m.start).abs().partial_cmp(&(a.1 - m.start).abs()).unwrap_or(Ordering::Equal))
            });
        if let Some((id, _, _)) = best {
            if !out.iter().any(|(existing, _)| existing == id) {
                out.push((id.clone(), m.mark.clone()));
            }
        }
    }
    out
}

pub struct MeetingNotesRepository;

impl MeetingNotesRepository {
    pub async fn get(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<MeetingNote>, sqlx::Error> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT notes_json FROM meeting_notes WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;
        Ok(match row.and_then(|(json,)| json) {
            Some(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
                warn!("Unreadable notes for meeting {meeting_id}: {e}");
                Vec::new()
            }),
            None => Vec::new(),
        })
    }

    /// Replaces the meeting's notes. Callers validate or sanitize first.
    pub async fn save(conn: &mut SqliteConnection, meeting_id: &str, notes: &[MeetingNote]) -> Result<(), sqlx::Error> {
        let mut sorted = notes.to_vec();
        sort_notes(&mut sorted);
        let json = serde_json::to_string(&sorted).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        let now = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO meeting_notes (meeting_id, notes_json, created_at, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(meeting_id) DO UPDATE SET notes_json = excluded.notes_json, updated_at = excluded.updated_at",
        )
        .bind(meeting_id)
        .bind(json)
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
```

- [ ] **Step 6: Models and API types**

`database/models.rs` — add to `MeetingModel` and `Transcript`:

```rust
    // Phase 4; `default` keeps older explicit SELECTs without these columns working.
    #[sqlx(default)]
    pub keep_audio: bool,
    #[sqlx(default)]
    pub audio_deleted_at: Option<String>,
```

```rust
    #[sqlx(default)]
    pub mark: Option<String>,
```

`api/api.rs` — add to `TranscriptSegment` and `MeetingTranscript`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
```

Set `mark: t.mark,` in both `.map(|t| MeetingTranscript { ... })` blocks (`api/api.rs` `api_get_meeting_transcripts` and `database/repositories/meeting.rs` `get_meeting`). Add `mark: None,` to every other struct literal of `crate::api::TranscriptSegment` (the compiler lists them; known: `audio/common.rs:59` in `create_transcript_segments`, and the two in `audio/import.rs` tests).

`database/repositories/meeting.rs` — `get_meeting_metadata` SELECT becomes `SELECT id, title, created_at, updated_at, folder_path, keep_audio, audio_deleted_at FROM meetings WHERE id = ?`. Add `api::MeetingMetadata.keep_audio: bool` (`#[serde(default)]`) and set it from `meeting.keep_audio` in `api_get_meeting_metadata`. Add:

```rust
    pub async fn set_keep_audio(pool: &SqlitePool, meeting_id: &str, keep: bool) -> Result<bool, SqlxError> {
        let r = sqlx::query("UPDATE meetings SET keep_audio = ? WHERE id = ?")
            .bind(keep)
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
```

In `delete_meeting_with_transaction`, before "3. Delete from transcripts", add:

```rust
    // Explicit, not only the FK cascade (spec §5.4).
    sqlx::query("DELETE FROM meeting_notes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;
```

- [ ] **Step 7: `save_transcript` and `set_mark`** (`transcript.rs`)

Signature gains `notes: &[MeetingNote]` (import `crate::database::repositories::meeting_notes::{sanitize_mark, sanitize_notes, MeetingNote, MeetingNotesRepository}`). The INSERT becomes:

```rust
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, mark)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
```

with `.bind(sanitize_mark(segment.mark.as_deref()))` after `.bind(segment.duration)`. After the segment loop, before commit:

```rust
        let notes = sanitize_notes(notes);
        if !notes.is_empty() {
            if let Err(e) = MeetingNotesRepository::save(&mut *transaction, &meeting_id, &notes).await {
                error!("Failed to save notes for meeting {}: {}", meeting_id, e);
                transaction.rollback().await?;
                return Err(e);
            }
        }
```

Add:

```rust
    pub async fn set_mark(pool: &SqlitePool, transcript_id: &str, mark: Option<&str>) -> Result<bool, SqlxError> {
        let r = sqlx::query("UPDATE transcripts SET mark = ? WHERE id = ?")
            .bind(mark)
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }
```

`api_save_transcript` gains the parameter `notes: Option<Vec<serde_json::Value>>` (after `folder_path`) and passes `&parse_notes(notes.unwrap_or_default())` — decoded per note so a malformed one is dropped, not fatal (the transcripts are already taken as `Vec<serde_json::Value>` for the same reason).

- [ ] **Step 8: Re-transcription keeps marks** (`audio/retranscription.rs`, the transaction at ~line 430)

Inside the existing transaction, before `DELETE FROM transcripts WHERE meeting_id = ?`:

```rust
    let old_marks: Vec<(Option<f64>, Option<f64>, String)> = sqlx::query_as(
        "SELECT audio_start_time, audio_end_time, mark FROM transcripts WHERE meeting_id = ? AND mark IS NOT NULL ORDER BY audio_start_time",
    )
    .bind(&meeting_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| anyhow!("Failed to read marks: {}", e))?;
    let old_marks: Vec<TimedMark> = old_marks
        .into_iter()
        .filter_map(|(s, e, mark)| s.map(|s| TimedMark { start: s, end: e.unwrap_or(s), mark }))
        .collect();
```

After the insert loop, before `tx.commit()`:

```rust
    let new_times: Vec<(String, f64, f64)> = segments
        .iter()
        .filter_map(|s| s.audio_start_time.map(|st| (s.id.clone(), st, s.audio_end_time.unwrap_or(st))))
        .collect();
    for (id, mark) in carry_marks(&old_marks, &new_times) {
        sqlx::query("UPDATE transcripts SET mark = ? WHERE id = ?")
            .bind(mark)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| anyhow!("Failed to keep a mark: {}", e))?;
    }
```

(import `crate::database::repositories::meeting_notes::{carry_marks, TimedMark}`). Marks without a recording time cannot be placed and are dropped (lines saved by Conclave always have one).

- [ ] **Step 9: Run tests to verify they pass**

Run: `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- meeting_notes repositories::transcript`
Expected: PASS (all new tests). Then the full `nice -n 15 cargo test --workspace --exclude llama-helper -j 4` from the repo root: PASS.

- [ ] **Step 10: Commit**

```bash
git add frontend/src-tauri/migrations/20261015000000_meeting_page.sql frontend/src-tauri/src/database frontend/src-tauri/src/api/api.rs frontend/src-tauri/src/audio/import.rs frontend/src-tauri/src/audio/common.rs frontend/src-tauri/src/audio/retranscription.rs
git commit -m "feat(db): transcript marks, meeting notes and keep-audio pin; re-transcription keeps marks"
```

---

### Task 2: Tauri commands for marks, notes and the pin

**Files:**
- Create: `frontend/src-tauri/src/api/meeting_page.rs`
- Modify: `frontend/src-tauri/src/api/mod.rs` (`pub mod meeting_page;`), `frontend/src-tauri/src/lib.rs` (`invoke_handler`)
- Test: `#[cfg(test)] mod tests` in `meeting_page.rs`

**Interfaces:**
- Consumes: Task 1 repository functions and validators.
- Produces (Tauri commands, camelCase args from JS):
  - `set_transcript_mark(transcriptId: string, mark: 'important'|'action'|'followup'|null) -> ()`
  - `get_meeting_notes(meetingId: string) -> MeetingNote[]`
  - `save_meeting_notes(meetingId: string, notes: MeetingNote[]) -> MeetingNote[]` (returns the stored, trimmed, sorted list)
  - `set_keep_audio(meetingId: string, keep: boolean) -> ()`
  - Inner testable fns: `set_mark_inner(&SqlitePool, &str, Option<String>) -> Result<(), String>`, `save_notes_inner(&SqlitePool, &str, Vec<MeetingNote>) -> Result<Vec<MeetingNote>, String>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;

    async fn meeting(pool: &SqlitePool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z')")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES ('t1', 'm1', 'hi', 'x')")
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn set_mark_validates_and_reports_missing_line() {
        let pool = test_pool().await;
        meeting(&pool).await;
        assert!(set_mark_inner(&pool, "t1", Some("bogus".into())).await.is_err());
        assert!(set_mark_inner(&pool, "missing", Some("action".into())).await.is_err());
        set_mark_inner(&pool, "t1", Some("action".into())).await.unwrap();
        set_mark_inner(&pool, "t1", None).await.unwrap();
    }

    #[tokio::test]
    async fn save_notes_rejects_bad_notes_and_unknown_meeting() {
        let pool = test_pool().await;
        meeting(&pool).await;
        let bad = vec![MeetingNote { id: "n".into(), at: None, text: " ".into() }];
        assert!(save_notes_inner(&pool, "m1", bad).await.is_err());
        let good = vec![MeetingNote { id: "n".into(), at: None, text: " later ".into() }];
        assert!(save_notes_inner(&pool, "nope", good.clone()).await.is_err());
        let stored = save_notes_inner(&pool, "m1", good).await.unwrap();
        assert_eq!(stored[0].text, "later");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- api::meeting_page`
Expected: compile error (module missing).

- [ ] **Step 3: Implement `api/meeting_page.rs`**

```rust
//! Commands for the meeting page: marks, notes, the "Keep audio" pin (spec §5.3).
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::meeting_notes::{validate_mark, validate_notes, MeetingNote, MeetingNotesRepository};
use crate::database::repositories::transcript::TranscriptsRepository;
use crate::state::AppState;
use sqlx::SqlitePool;

pub(crate) async fn set_mark_inner(pool: &SqlitePool, transcript_id: &str, mark: Option<String>) -> Result<(), String> {
    validate_mark(mark.as_deref())?;
    match TranscriptsRepository::set_mark(pool, transcript_id, mark.as_deref()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("Transcript line not found: {transcript_id}")),
        Err(e) => Err(format!("Failed to save mark: {e}")),
    }
}

pub(crate) async fn save_notes_inner(pool: &SqlitePool, meeting_id: &str, notes: Vec<MeetingNote>) -> Result<Vec<MeetingNote>, String> {
    let notes = validate_notes(&notes)?;
    let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if exists.is_none() {
        return Err(format!("Meeting not found: {meeting_id}"));
    }
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    MeetingNotesRepository::save(&mut conn, meeting_id, &notes).await.map_err(|e| format!("Failed to save notes: {e}"))?;
    drop(conn);
    MeetingNotesRepository::get(pool, meeting_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_transcript_mark(state: tauri::State<'_, AppState>, transcript_id: String, mark: Option<String>) -> Result<(), String> {
    set_mark_inner(state.db_manager.pool(), &transcript_id, mark).await
}

#[tauri::command]
pub async fn get_meeting_notes(state: tauri::State<'_, AppState>, meeting_id: String) -> Result<Vec<MeetingNote>, String> {
    MeetingNotesRepository::get(state.db_manager.pool(), &meeting_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_meeting_notes(state: tauri::State<'_, AppState>, meeting_id: String, notes: Vec<MeetingNote>) -> Result<Vec<MeetingNote>, String> {
    save_notes_inner(state.db_manager.pool(), &meeting_id, notes).await
}

#[tauri::command]
pub async fn set_keep_audio(state: tauri::State<'_, AppState>, meeting_id: String, keep: bool) -> Result<(), String> {
    match MeetingsRepository::set_keep_audio(state.db_manager.pool(), &meeting_id, keep).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!("Meeting not found: {meeting_id}")),
        Err(e) => Err(e.to_string()),
    }
}
```

Register in `lib.rs` `generate_handler![...]` next to `api::api_get_meeting`: `api::meeting_page::set_transcript_mark, api::meeting_page::get_meeting_notes, api::meeting_page::save_meeting_notes, api::meeting_page::set_keep_audio,`. 

- [ ] **Step 4: Run tests** — same command as Step 2. Expected: PASS. Then the full workspace test run: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/api/meeting_page.rs frontend/src-tauri/src/api/mod.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(api): commands for transcript marks, meeting notes and keep-audio"
```

---

### Task 3: Folder guard, Recycle Bin, and meeting delete removes the folder

**Codex pass required** (data deletion).

**Files:**
- Create: `frontend/src-tauri/src/audio/meeting_folder.rs` (add `pub mod meeting_folder;` to `audio/mod.rs`)
- Modify: `frontend/src-tauri/Cargo.toml` (`dunce = "1.0.5"`, already in `Cargo.lock`; `trash = "5.2"`, new — the user approved it for Recycle Bin deletes), `frontend/src-tauri/src/api/api.rs` (`api_delete_meeting`), `frontend/src-tauri/src/audio/audio_processing.rs` (`create_meeting_folder` never reuses a folder)
- Test: `#[cfg(test)] mod tests` in `meeting_folder.rs` and `audio_processing.rs` (uses `tempfile`)

**Safety rules (spec §1, §8.3 — user decision):**
- A folder qualifies only if its canonical path is strictly inside the canonical recordings base (`get_default_recordings_folder()`); there is no other rule.
- Everything Conclave deletes goes to the Recycle Bin, **one checked item at a time**: each file is re-checked (real file, parent resolves to exactly the guarded folder or its `.checkpoints`) right before it is passed to the remover; folders are recycled only when empty. Nothing is ever deleted with `fs::remove_*` in production code.
- The remover is a parameter (`Remover`) so tests never touch the real trash; production passes `recycle`.
- A folder used by more than one meeting is never deleted.
- Known remaining window (documented in the code): a local process running as the user could swap a path between the per-file re-check and the Recycle Bin call; the window is one file's recycle call.

**Interfaces:**
- Produces:
  - `pub use crate::audio::constants::AUDIO_EXTENSIONS;` (mp4, m4a, wav, mp3, flac, ogg, aac, mkv, webm, wma — imports are stored as `audio.<source ext>`, case as typed)
  - `pub type Remover<'a> = &'a (dyn Fn(&[PathBuf]) -> Result<(), String> + Sync);` (`Sync` so async Tauri commands holding it across `.await` stay `Send`)
  - `pub fn recycle(paths: &[PathBuf]) -> Result<(), String>` — `trash::delete_all` on its own thread; the only production remover.
  - `pub fn guarded_meeting_folder(folder_path: &str, recordings_base: &Path) -> Option<PathBuf>` — canonical (`dunce`) path of an existing directory strictly inside the canonical base; `None` otherwise (logged at warn).
  - `pub fn audio_files(folder: &Path) -> Vec<PathBuf>` — real top-level files named `audio.<ext>` (ext compared case-insensitively against `AUDIO_EXTENSIONS`), in `AUDIO_EXTENSIONS` order.
  - `pub fn find_audio_file(folder: &Path) -> Option<PathBuf>` — `audio_files(folder).into_iter().next()`.
  - `pub fn checkpoint_files(checkpoints: &Path) -> Vec<PathBuf>` — real files named `audio_chunk_*` or `concat_list.txt` inside a real (non-link) `.checkpoints` folder.
  - `pub const MEETING_FILES: [&str; 4] = ["transcripts.json", "metadata.json", ".transcripts.json.tmp", ".metadata.json.tmp"];`
  - `pub fn recycle_checked(path: &Path, folder: &Path, remove: Remover) -> Result<(), String>` — the per-file re-check, then `remove(&[path])`.
  - `pub fn recycle_if_empty(dir: &Path, remove: Remover) -> Result<bool, String>` — recycles a real, empty directory; `Ok(false)` if it is missing, a link or not empty.
  - `pub fn delete_meeting_folder(folder_path: &str, recordings_base: &Path, remove: Remover) -> Result<(), String>` — missing folder: `Ok(())`; guard failure or foreign content: `Err("Recording folder kept: …")` with nothing removed; otherwise recycles checkpoint files, the JSON files and the audio (audio last), then `.checkpoints` and the folder if empty; a file that appeared meanwhile keeps the folder (`Err`).
  - `pub async fn delete_meeting_and_folder(pool: &SqlitePool, meeting_id: &str, recordings_base: &Path, remove: Remover<'_>) -> Result<Option<Option<String>>, sqlx::Error>` — `Ok(None)` = meeting not found; `Ok(Some(warning))` = meeting deleted, `warning` set when its folder was kept (including "another meeting uses this folder").
  - `api_delete_meeting` response gains `"folder_warning": string | null`.

- [ ] **Step 1: Write the failing tests** — `meeting_folder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    fn base() -> tempfile::TempDir { tempfile::tempdir().unwrap() }

    /// Stand-in for the Recycle Bin. Removes files, and directories only when empty
    /// (`remove_dir` fails otherwise), so a non-empty folder ever reaching it fails the test.
    struct FakeBin(Mutex<Vec<PathBuf>>);
    impl FakeBin {
        fn new() -> Self { FakeBin(Mutex::new(Vec::new())) }
        fn remove(&self, paths: &[PathBuf]) -> Result<(), String> {
            for p in paths {
                if p.is_dir() { fs::remove_dir(p).map_err(|e| e.to_string())? } else { fs::remove_file(p).map_err(|e| e.to_string())? }
                self.0.lock().unwrap().push(p.clone());
            }
            Ok(())
        }
        fn names(&self) -> Vec<String> {
            self.0.lock().unwrap().iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect()
        }
    }

    #[test]
    fn guard_accepts_only_folders_strictly_inside_the_base() {
        let b = base();
        let inside = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&inside).unwrap();
        assert!(guarded_meeting_folder(inside.to_str().unwrap(), b.path()).is_some());
        let dotdot_inside = inside.join("..").join("Standup_2026-10-15_10-00");
        assert!(guarded_meeting_folder(dotdot_inside.to_str().unwrap(), b.path()).is_some());

        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("metadata.json"), "{}").unwrap(); // not a pass any more
        assert!(guarded_meeting_folder(elsewhere.path().to_str().unwrap(), b.path()).is_none());
    }

    #[test]
    fn guard_rejects_base_itself_files_missing_and_dotdot_escape() {
        let b = base();
        assert!(guarded_meeting_folder(b.path().to_str().unwrap(), b.path()).is_none());
        let file = b.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        assert!(guarded_meeting_folder(file.to_str().unwrap(), b.path()).is_none());
        assert!(guarded_meeting_folder(b.path().join("nope").to_str().unwrap(), b.path()).is_none());
        let sub = b.path().join("m");
        fs::create_dir(&sub).unwrap();
        assert!(guarded_meeting_folder(sub.join("..").join("..").to_str().unwrap(), b.path()).is_none());
        assert!(guarded_meeting_folder("", b.path()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn guard_rejects_a_link_inside_the_base_that_points_outside() {
        let b = base();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), b.path().join("sneaky")).unwrap();
        assert!(guarded_meeting_folder(b.path().join("sneaky").to_str().unwrap(), b.path()).is_none());
    }

    #[test]
    fn audio_files_prefer_mp4_and_find_imported_formats_in_any_case() {
        let b = base();
        assert!(find_audio_file(b.path()).is_none());
        fs::write(b.path().join("audio.MP3"), "x").unwrap();
        fs::write(b.path().join("other.mp4"), "x").unwrap();
        assert_eq!(find_audio_file(b.path()).unwrap().file_name().unwrap(), "audio.MP3");
        fs::write(b.path().join("audio.mp4"), "x").unwrap();
        assert_eq!(find_audio_file(b.path()).unwrap().file_name().unwrap(), "audio.mp4");
        assert_eq!(audio_files(b.path()).len(), 2);
    }

    #[test]
    fn delete_folder_recycles_conclave_files_one_by_one_audio_last() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir_all(m.join(".checkpoints")).unwrap();
        for f in ["audio.MKV", "transcripts.json", "metadata.json", ".checkpoints/audio_chunk_000.mp4", ".checkpoints/concat_list.txt"] {
            fs::write(m.join(f), "x").unwrap();
        }
        let bin = FakeBin::new();
        delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap();
        let names = bin.names();
        assert_eq!(names.len(), 7); // 5 files + .checkpoints + the folder
        assert_eq!(names[names.len() - 3], "audio.MKV"); // audio is the last file
        assert_eq!(names.last().unwrap(), "m");
        assert!(!m.exists());
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_ok()); // already gone
    }

    #[test]
    fn delete_folder_touches_nothing_outside_the_base_or_with_other_files() {
        let b = base();
        let bin = FakeBin::new();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("metadata.json"), "{}").unwrap();
        fs::write(outside.path().join("audio.mp4"), "theirs").unwrap();
        assert!(delete_meeting_folder(outside.path().to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());

        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        fs::write(m.join("thesis.docx"), "precious").unwrap();
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).unwrap_err().contains("kept"));

        assert!(bin.names().is_empty());
        for f in [outside.path().join("metadata.json"), outside.path().join("audio.mp4"), m.join("audio.mp4"), m.join("thesis.docx")] {
            assert!(f.exists(), "{}", f.display());
        }
    }

    #[test]
    fn a_file_appearing_during_the_delete_keeps_the_folder() {
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("metadata.json"), "{}").unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        let bin = FakeBin::new();
        let intruder = m.join("new-file.txt");
        let remove = |p: &[PathBuf]| {
            if !intruder.exists() { fs::write(&intruder, "appeared").unwrap(); }
            bin.remove(p)
        };
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &remove).unwrap_err().contains("kept"));
        assert!(intruder.exists());
        assert!(!bin.names().contains(&"new-file.txt".to_string()));
        assert!(!bin.names().contains(&"m".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn links_inside_a_meeting_folder_are_never_followed() {
        use std::os::unix::fs::symlink;
        let b = base();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("audio_chunk_000.mp4"), "victim").unwrap();
        fs::write(target.path().join("secret.mp4"), "secret").unwrap();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        symlink(target.path(), m.join(".checkpoints")).unwrap();
        symlink(target.path().join("secret.mp4"), m.join("audio.mp4")).unwrap();
        assert!(checkpoint_files(&m.join(".checkpoints")).is_empty());
        assert!(find_audio_file(&m).is_none());
        let bin = FakeBin::new();
        assert!(delete_meeting_folder(m.to_str().unwrap(), b.path(), &|p| bin.remove(p)).is_err());
        assert!(recycle_checked(&m.join(".checkpoints").join("audio_chunk_000.mp4"), &dunce::canonicalize(&m).unwrap(), &|p| bin.remove(p)).is_err());
        assert!(bin.names().is_empty());
        assert!(target.path().join("audio_chunk_000.mp4").exists());
        assert!(target.path().join("secret.mp4").exists());
    }

    async fn meeting(pool: &sqlx::SqlitePool, id: &str, folder: &Path) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path) VALUES (?, 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?)")
            .bind(id).bind(folder.to_string_lossy().to_string())
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn meeting_stays_deleted_when_its_folder_cannot_be() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let outside = tempfile::tempdir().unwrap(); // fails the guard
        meeting(&pool, "m1", outside.path()).await;
        let bin = FakeBin::new();
        let result = delete_meeting_and_folder(&pool, "m1", b.path(), &|p| bin.remove(p)).await.unwrap();
        assert!(matches!(result, Some(Some(_))));
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 0);
        assert!(outside.path().exists());
        assert!(bin.names().is_empty());
        assert!(delete_meeting_and_folder(&pool, "m1", b.path(), &|p| bin.remove(p)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_folder_shared_by_two_meetings_is_kept() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let m = b.path().join("Standup_2026-10-15_10-00");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        meeting(&pool, "a", &m).await;
        meeting(&pool, "b", &m).await;
        let bin = FakeBin::new();
        let w = delete_meeting_and_folder(&pool, "a", b.path(), &|p| bin.remove(p)).await.unwrap().unwrap().unwrap();
        assert!(w.contains("another meeting"));
        assert!(m.join("audio.mp4").exists());
        assert!(bin.names().is_empty());
    }
}
```

`audio_processing.rs` test (add a `#[cfg(test)]` module if there is none):

```rust
#[cfg(test)]
mod meeting_folder_name_tests {
    use super::*;

    #[test]
    fn two_meetings_never_share_a_folder() {
        let base = tempfile::tempdir().unwrap();
        let a = create_meeting_folder(&base.path().to_path_buf(), "Standup", true).unwrap();
        let b = create_meeting_folder(&base.path().to_path_buf(), "Standup", false).unwrap();
        assert_ne!(a, b);
        assert!(a.is_dir() && b.is_dir());
        assert!(a.join(".checkpoints").is_dir());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- audio::meeting_folder meeting_folder_name_tests`
Expected: compile error (module missing) and, once it compiles, `two_meetings_never_share_a_folder` FAILS (same minute → same folder).

- [ ] **Step 3: Implement `meeting_folder.rs`**

```rust
//! The one place that decides whether a meeting folder may be read or deleted (spec §8.3).
//! Conclave never deletes outside the recordings folder and never permanently: see `recycle`.
//! Remaining window: another process running as the user could swap a path between the
//! per-file re-check in `recycle_checked` and the Recycle Bin call for that one file.
use log::warn;
use std::fs;
use std::path::{Path, PathBuf};

/// The import formats (`import.rs` copies a file in as `audio.<ext>`), so every format is found.
pub use crate::audio::constants::AUDIO_EXTENSIONS;

pub const MEETING_FILES: [&str; 4] = ["transcripts.json", "metadata.json", ".transcripts.json.tmp", ".metadata.json.tmp"];

/// Every deleting function takes this, so tests never touch the real Recycle Bin.
/// `Sync` keeps the async commands that hold it across `.await` `Send`.
pub type Remover<'a> = &'a (dyn Fn(&[PathBuf]) -> Result<(), String> + Sync);

/// Moves paths to the Windows Recycle Bin. With `trash`'s flags Windows asks the user before
/// permanently deleting anything too big for the Bin; declining returns an error here.
/// Runs on its own thread so `trash` gets a fresh COM apartment.
/// shortcut: the join blocks one async worker while Windows shows a prompt; move to spawn_blocking if that ever matters.
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let owned = paths.to_vec();
    std::thread::spawn(move || trash::delete_all(&owned).map_err(|e| format!("Could not move to the Recycle Bin: {e}")))
        .join()
        .map_err(|_| "The Recycle Bin call failed unexpectedly".to_string())?
}

/// Canonical path of a meeting folder strictly inside the recordings base, or `None`.
/// Canonicalizing first resolves `..` and links, so nothing outside the base can qualify.
pub fn guarded_meeting_folder(folder_path: &str, recordings_base: &Path) -> Option<PathBuf> {
    if folder_path.trim().is_empty() {
        return None;
    }
    let (Ok(folder), Ok(base)) = (dunce::canonicalize(folder_path), dunce::canonicalize(recordings_base)) else {
        warn!("Meeting folder guard: cannot resolve {folder_path}");
        return None;
    };
    if folder.is_dir() && folder != base && folder.starts_with(&base) {
        Some(folder)
    } else {
        warn!("Meeting folder guard: refusing {} (not inside {})", folder.display(), base.display());
        None
    }
}

/// A real file, not a symlink/junction (links could point anywhere on disk).
fn is_plain_file(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file())
}

fn is_plain_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// Position of `name`'s extension in `AUDIO_EXTENSIONS` if it is `audio.<ext>`, any case
/// (imports keep the source file's case, e.g. `audio.MP3`).
fn audio_rank(name: &str) -> Option<usize> {
    let (stem, ext) = name.split_once('.')?;
    if !stem.eq_ignore_ascii_case("audio") {
        return None;
    }
    AUDIO_EXTENSIONS.iter().position(|e| e.eq_ignore_ascii_case(ext))
}

fn is_checkpoint_name(name: &str) -> bool {
    name.starts_with("audio_chunk_") || name == "concat_list.txt"
}

pub fn audio_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<(usize, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| audio_rank(&e.file_name().to_string_lossy()).map(|r| (r, e.path())))
        .collect();
    found.sort_by_key(|(r, _)| *r);
    found.into_iter().map(|(_, p)| p).collect()
}

pub fn find_audio_file(folder: &Path) -> Option<PathBuf> {
    audio_files(folder).into_iter().next()
}

/// Conclave's own files in `.checkpoints/` (`audio_chunk_NNN.mp4`, `concat_list.txt`, see
/// `incremental_saver.rs`). Empty when `.checkpoints` is missing or is a link.
pub fn checkpoint_files(checkpoints: &Path) -> Vec<PathBuf> {
    if !is_plain_dir(checkpoints) {
        return Vec::new();
    }
    fs::read_dir(checkpoints)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter(|e| is_checkpoint_name(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Re-checks one file right before recycling it: a real file whose parent resolves to exactly
/// the (canonical) meeting folder or its `.checkpoints`.
pub fn recycle_checked(path: &Path, folder: &Path, remove: Remover) -> Result<(), String> {
    let parent_ok = path
        .parent()
        .and_then(|p| dunce::canonicalize(p).ok())
        .is_some_and(|p| p == folder || p == folder.join(".checkpoints"));
    if !parent_ok || !is_plain_file(path) {
        return Err(format!("Skipped {}: it is no longer a Conclave file in the meeting folder", path.display()));
    }
    remove(&[path.to_path_buf()])
}

/// Recycles a real, empty directory. `Ok(false)` if it is missing, a link or not empty.
pub fn recycle_if_empty(dir: &Path, remove: Remover) -> Result<bool, String> {
    let empty = is_plain_dir(dir) && fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_none());
    if !empty {
        return Ok(false);
    }
    remove(&[dir.to_path_buf()]).map(|_| true)
}

/// The files to recycle for a whole-folder delete, checkpoint files first and audio last,
/// or `None` if the folder holds anything Conclave did not write (links never count as ours).
fn conclave_files(folder: &Path) -> Option<Vec<PathBuf>> {
    let mut checkpoints = Vec::new();
    let mut json = Vec::new();
    for e in fs::read_dir(folder).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        match e.file_type().ok()? {
            t if t.is_file() && audio_rank(&name).is_some() => {}
            t if t.is_file() && MEETING_FILES.contains(&name.as_str()) => json.push(e.path()),
            t if t.is_dir() && name == ".checkpoints" => {
                let files = checkpoint_files(&e.path());
                if fs::read_dir(e.path()).ok()?.count() != files.len() {
                    return None;
                }
                checkpoints = files;
            }
            _ => return None,
        }
    }
    Some([checkpoints, json, audio_files(folder)].concat())
}

/// Moves a meeting folder to the Recycle Bin item by item, but only a folder inside the
/// recordings base that holds nothing but Conclave's files. Checked completely first.
pub fn delete_meeting_folder(folder_path: &str, recordings_base: &Path, remove: Remover) -> Result<(), String> {
    if !Path::new(folder_path).exists() {
        return Ok(());
    }
    let folder = guarded_meeting_folder(folder_path, recordings_base)
        .ok_or_else(|| format!("Recording folder kept: it is outside the Conclave recordings folder ({folder_path})"))?;
    let files = conclave_files(&folder)
        .ok_or_else(|| format!("Recording folder kept: it contains other files ({})", folder.display()))?;
    for f in &files {
        recycle_checked(f, &folder, remove)?;
    }
    recycle_if_empty(&folder.join(".checkpoints"), remove)?;
    if guarded_meeting_folder(&folder.to_string_lossy(), recordings_base).as_ref() != Some(&folder)
        || !recycle_if_empty(&folder, remove)?
    {
        return Err(format!("Recording folder kept: new files appeared in it ({})", folder.display()));
    }
    Ok(())
}

/// DB delete first, then the folder; the meeting stays deleted if the folder cannot be.
pub async fn delete_meeting_and_folder(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    recordings_base: &Path,
    remove: Remover<'_>,
) -> Result<Option<Option<String>>, sqlx::Error> {
    use crate::database::repositories::meeting::MeetingsRepository;
    let folder_path = MeetingsRepository::get_meeting_metadata(pool, meeting_id).await?.and_then(|m| m.folder_path);
    if !MeetingsRepository::delete_meeting(pool, meeting_id).await? {
        return Ok(None);
    }
    let warning = match folder_path {
        None => None,
        Some(p) => {
            let (others,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM meetings WHERE folder_path = ?")
                .bind(&p)
                .fetch_one(pool)
                .await?;
            if others > 0 {
                Some(format!("Recording folder kept: another meeting uses it ({p})"))
            } else {
                delete_meeting_folder(&p, recordings_base, remove).err()
            }
        }
    };
    if let Some(w) = &warning {
        warn!("Meeting {meeting_id} deleted, folder kept: {w}");
    }
    Ok(Some(warning))
}
```

- [ ] **Step 4: `create_meeting_folder` never reuses a folder** (`audio_processing.rs`) — replace the `create_dir_all(&meeting_folder)` line and the `let meeting_folder = …` before it with:

```rust
    std::fs::create_dir_all(base_path)?;
    // `create_dir` (not `_all`) fails on an existing folder, so two meetings never share one;
    // the name only has minute resolution.
    let mut meeting_folder = base_path.join(&folder_name);
    let mut n = 2;
    loop {
        match std::fs::create_dir(&meeting_folder) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n <= 99 => {
                meeting_folder = base_path.join(format!("{folder_name}_{n}"));
                n += 1;
            }
            Err(e) => return Err(e.into()),
        }
    }
```

(Callers: `recording_saver.rs:229` and `import.rs:344`, each once per new meeting — check neither expects to reopen an existing folder.)

- [ ] **Step 5: Wire `api_delete_meeting`** — replace the `match MeetingsRepository::delete_meeting(...)` with:

```rust
    let base = crate::audio::recording_preferences::get_default_recordings_folder();
    match crate::audio::meeting_folder::delete_meeting_and_folder(pool, &meeting_id, &base, &crate::audio::meeting_folder::recycle).await {
        Ok(Some(folder_warning)) => {
            log_info!("Successfully deleted meeting {}", meeting_id);
            Ok(serde_json::json!({
                "status": "success",
                "message": "Meeting deleted successfully",
                "folder_warning": folder_warning,
            }))
        }
        Ok(None) => {
            log_warn!("Meeting not found or already deleted: {}", meeting_id);
            Err(format!("Meeting not found or could not be deleted: {}", meeting_id))
        }
        Err(e) => {
            log_error!("Error deleting meeting {}: {}", meeting_id, e);
            Err(format!("Failed to delete meeting: {}", e))
        }
    }
```

- [ ] **Step 6: Run tests** — Step 2 command: PASS; full workspace tests: PASS. `trash` on Windows is first compiled by the Windows CI run at the end of the phase — note it in the report.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/Cargo.toml Cargo.lock frontend/src-tauri/src/audio/meeting_folder.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/audio/audio_processing.rs frontend/src-tauri/src/api/api.rs
git commit -m "feat(storage): guard meeting folders to the recordings folder; meeting delete moves its files to the Recycle Bin"
```

---

### Task 4: Old-audio review (backend)

**Codex pass required** (data deletion).

**Files:**
- Create: `frontend/src-tauri/src/audio/retention.rs` (add `pub mod retention;` to `audio/mod.rs`)
- Modify: `frontend/src-tauri/src/audio/recording_preferences.rs`, `frontend/src-tauri/src/lib.rs` (register commands)
- Test: `#[cfg(test)] mod tests` in `retention.rs` and `recording_preferences.rs`

There is **no background deletion and no grace period** (spec §8.1): the backend only lists old audio and moves exactly what the user confirmed in the review to the Recycle Bin, re-checking each meeting and each file right before.

**Interfaces:**
- Consumes (Task 3): `guarded_meeting_folder`, `audio_files`, `checkpoint_files`, `recycle_checked`, `recycle_if_empty`, `Remover`, `recycle`; (Task 1) `keep_audio`, `audio_deleted_at`; `crate::audio::recording_commands::is_recording_now()`.
- Produces:
  - `RecordingPreferences.old_audio_days: Option<u32>` (`#[serde(default = "default_old_audio_days")]` → `Some(30)`; JSON `null` = Never)
  - `#[derive(Serialize)] pub struct OldAudio { pub meeting_id: String, pub title: String, pub created_at: String, pub folder: String, pub bytes: u64, pub files: Vec<String> }`
  - `#[derive(Serialize, Default)] pub struct DeleteResult { pub meetings: u32, pub bytes: u64, pub failed: Vec<String> }`
  - `pub fn is_old(created_at: DateTime<Utc>, days: u32, now: DateTime<Utc>) -> bool`
  - `pub fn audio_paths(folder: &Path) -> Vec<PathBuf>` — `checkpoint_files(.checkpoints)` then `audio_files(folder)` (audio last; never a directory)
  - `pub async fn list_old_audio(pool, days: Option<u32>, now, recording: bool, base: &Path) -> Result<Vec<OldAudio>, String>` — skips pinned, too new, no folder, guard failures, folders shared by several meetings, and folders without audio.
  - `pub async fn delete_old_audio_inner(pool, confirmed: &[String], days: Option<u32>, now, recording: bool, base: &Path, remove: Remover<'_>) -> Result<DeleteResult, String>`
  - Commands: `old_audio_list() -> OldAudio[]`, `delete_old_audio(meetingIds: string[]) -> DeleteResult`.

- [ ] **Step 1: Write the failing tests** — `recording_preferences.rs`, new test module:

```rust
#[cfg(test)]
mod old_audio_pref_tests {
    use super::*;

    #[test]
    fn old_file_defaults_to_30_days() {
        let json = r#"{"save_folder":"C:\\rec","auto_save":true,"file_format":"mp4"}"#;
        let p: RecordingPreferences = serde_json::from_str(json).unwrap();
        assert_eq!(p.old_audio_days, Some(30));
    }

    #[test]
    fn explicit_null_means_never() {
        let json = r#"{"save_folder":"C:\\rec","auto_save":true,"file_format":"mp4","old_audio_days":null}"#;
        let p: RecordingPreferences = serde_json::from_str(json).unwrap();
        assert_eq!(p.old_audio_days, None);
    }
}
```

`retention.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::test_pool;
    use chrono::{Duration, TimeZone, Utc};
    use std::fs;
    use std::sync::Mutex;

    fn now() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap() }

    /// Recycle Bin stand-in: files, and directories only when empty.
    struct FakeBin(Mutex<Vec<PathBuf>>);
    impl FakeBin {
        fn new() -> Self { FakeBin(Mutex::new(Vec::new())) }
        fn remove(&self, paths: &[PathBuf]) -> Result<(), String> {
            for p in paths {
                if p.is_dir() { fs::remove_dir(p).map_err(|e| e.to_string())? } else { fs::remove_file(p).map_err(|e| e.to_string())? }
                self.0.lock().unwrap().push(p.clone());
            }
            Ok(())
        }
    }

    fn folder(base: &Path, name: &str) -> PathBuf {
        let f = base.join(name);
        fs::create_dir_all(f.join(".checkpoints")).unwrap();
        fs::write(f.join("audio.mp4"), vec![0u8; 1000]).unwrap();
        fs::write(f.join(".checkpoints/audio_chunk_000.mp4"), vec![0u8; 10]).unwrap();
        fs::write(f.join("transcripts.json"), "[]").unwrap();
        fs::write(f.join("metadata.json"), "{}").unwrap();
        f
    }

    async fn insert(pool: &SqlitePool, id: &str, created: DateTime<Utc>, folder: Option<&Path>, keep: bool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path, keep_audio) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id).bind(format!("Meeting {id}")).bind(created).bind(created)
            .bind(folder.map(|f| f.to_string_lossy().to_string())).bind(keep)
            .execute(pool).await.unwrap();
    }

    async fn deleted_at(pool: &SqlitePool, id: &str) -> Option<String> {
        let (d,): (Option<String>,) = sqlx::query_as("SELECT audio_deleted_at FROM meetings WHERE id = ?").bind(id).fetch_one(pool).await.unwrap();
        d
    }

    #[test]
    fn is_old_uses_the_cutoff() {
        assert!(is_old(now() - Duration::days(31), 30, now()));
        assert!(!is_old(now() - Duration::days(29), 30, now()));
    }

    #[tokio::test]
    async fn list_selects_the_right_meetings_and_deletes_nothing() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let old = now() - Duration::days(40);
        let f_old = folder(b.path(), "old");
        let f_new = folder(b.path(), "new");
        let f_pin = folder(b.path(), "pinned");
        let f_shared = folder(b.path(), "shared");
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("audio.mp4"), "x").unwrap();
        insert(&pool, "old", old, Some(&f_old), false).await;
        insert(&pool, "new", now() - Duration::days(5), Some(&f_new), false).await;
        insert(&pool, "pinned", old, Some(&f_pin), true).await;
        insert(&pool, "nofolder", old, None, false).await;
        insert(&pool, "outside", old, Some(outside.path()), false).await;
        insert(&pool, "shared1", old, Some(&f_shared), false).await;
        insert(&pool, "shared2", now() - Duration::days(1), Some(&f_shared), false).await;

        let list = list_old_audio(&pool, Some(30), now(), false, b.path()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].meeting_id, "old");
        assert_eq!(list[0].title, "Meeting old");
        assert_eq!(list[0].bytes, 1010);
        assert_eq!(list[0].files.len(), 2);
        assert!(list[0].files.last().unwrap().ends_with("audio.mp4")); // audio last
        assert!(f_old.join("audio.mp4").exists());
        assert!(list_old_audio(&pool, None, now(), false, b.path()).await.unwrap().is_empty());
        assert!(list_old_audio(&pool, Some(30), now(), true, b.path()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_recycles_only_confirmed_audio_and_marks_the_meeting() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let old = now() - Duration::days(40);
        let f1 = folder(b.path(), "one");
        let f2 = folder(b.path(), "two");
        insert(&pool, "one", old, Some(&f1), false).await;
        insert(&pool, "two", old, Some(&f2), false).await;
        let bin = FakeBin::new();

        let r = delete_old_audio_inner(&pool, &["one".into(), "ghost".into()], Some(30), now(), false, b.path(), &|p| bin.remove(p)).await.unwrap();
        assert_eq!((r.meetings, r.bytes), (1, 1010));
        assert!(r.failed.is_empty());
        assert!(!f1.join("audio.mp4").exists());
        assert!(!f1.join(".checkpoints").exists()); // emptied, then recycled
        for keep in ["transcripts.json", "metadata.json"] { assert!(f1.join(keep).exists(), "{keep}"); }
        assert!(f2.join("audio.mp4").exists()); // not confirmed
        assert!(deleted_at(&pool, "one").await.is_some());
        assert!(deleted_at(&pool, "two").await.is_none());
        assert_eq!(bin.0.lock().unwrap().len(), 3); // chunk, audio, then the empty .checkpoints
    }

    #[tokio::test]
    async fn delete_is_refused_while_recording_and_reports_partial_bin_failures() {
        let pool = test_pool().await;
        let b = tempfile::tempdir().unwrap();
        let f = folder(b.path(), "one");
        insert(&pool, "one", now() - Duration::days(40), Some(&f), false).await;
        let bin = FakeBin::new();
        // the user declines Windows' prompt for the big audio file; the chunk was already moved
        let declines_audio = |p: &[PathBuf]| {
            if p.iter().any(|x| x.file_name().is_some_and(|n| n == "audio.mp4")) { Err("user declined".to_string()) } else { bin.remove(p) }
        };
        assert!(delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), true, b.path(), &declines_audio).await.is_err());
        let r = delete_old_audio_inner(&pool, &["one".into()], Some(30), now(), false, b.path(), &declines_audio).await.unwrap();
        assert_eq!(r.meetings, 0);
        assert_eq!(r.bytes, 10);
        assert_eq!(r.failed.len(), 1);
        assert!(f.join("audio.mp4").exists());
        assert!(deleted_at(&pool, "one").await.is_none()); // audio still there
    }

    #[test]
    fn foreign_files_in_checkpoints_are_neither_listed_nor_deleted() {
        let b = tempfile::tempdir().unwrap();
        let f = b.path().join("m");
        fs::create_dir_all(f.join(".checkpoints")).unwrap();
        fs::write(f.join(".checkpoints/mine.txt"), vec![0u8; 500]).unwrap();
        assert!(audio_paths(&f).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn linked_checkpoints_are_not_listed() {
        let b = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("audio_chunk_000.mp4"), "x").unwrap();
        let f = b.path().join("m");
        fs::create_dir(&f).unwrap();
        std::os::unix::fs::symlink(target.path(), f.join(".checkpoints")).unwrap();
        assert!(audio_paths(&f).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- retention old_audio_pref`
Expected: compile errors (field and module missing).

- [ ] **Step 3: Preferences** — in `RecordingPreferences` add:

```rust
    /// List recordings older than this many days in the old-audio review; `None` = Never (spec §8.1).
    #[serde(default = "default_old_audio_days")]
    pub old_audio_days: Option<u32>,
```

with `fn default_old_audio_days() -> Option<u32> { Some(30) }` and `old_audio_days: default_old_audio_days()` in `Default`. `set_recording_preferences` is unchanged.

- [ ] **Step 4: Implement `retention.rs`**

```rust
//! Old-audio review (spec §8.1): lists recordings older than N days and moves exactly the
//! confirmed ones to the Recycle Bin. Never runs by itself; never touches transcripts,
//! summaries, notes or the JSON files in the folder.
use crate::audio::meeting_folder::{audio_files, checkpoint_files, guarded_meeting_folder, recycle, recycle_checked, recycle_if_empty, Remover};
use crate::audio::recording_preferences::{get_default_recordings_folder, load_recording_preferences};
use crate::database::models::DateTimeUtc;
use chrono::{DateTime, Duration, Utc};
use log::{info, warn};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OldAudio {
    pub meeting_id: String,
    pub title: String,
    pub created_at: String,
    pub folder: String,
    pub bytes: u64,
    pub files: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct DeleteResult {
    pub meetings: u32,
    pub bytes: u64,
    pub failed: Vec<String>,
}

pub fn is_old(created_at: DateTime<Utc>, days: u32, now: DateTime<Utc>) -> bool {
    created_at < now - Duration::days(i64::from(days))
}

/// Everything the review may delete in one folder, checkpoint files first and audio last.
pub fn audio_paths(folder: &Path) -> Vec<PathBuf> {
    [checkpoint_files(&folder.join(".checkpoints")), audio_files(folder)].concat()
}

fn size_of(path: &Path) -> u64 {
    fs::symlink_metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub async fn list_old_audio(
    pool: &SqlitePool,
    days: Option<u32>,
    now: DateTime<Utc>,
    recording: bool,
    base: &Path,
) -> Result<Vec<OldAudio>, String> {
    let Some(days) = days.filter(|d| *d > 0) else { return Ok(Vec::new()) };
    if recording {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, DateTimeUtc, String, bool)> = sqlx::query_as(
        "SELECT id, title, created_at, folder_path, keep_audio FROM meetings WHERE folder_path IS NOT NULL ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    // A folder used by several meetings is never offered (deleting it would hit the others).
    let mut uses: HashMap<String, usize> = HashMap::new();
    for (_, _, _, path, _) in &rows {
        *uses.entry(path.clone()).or_default() += 1;
    }
    Ok(rows
        .into_iter()
        .filter(|(_, _, created, path, keep)| !keep && uses[path] == 1 && is_old(created.0, days, now))
        .filter_map(|(id, title, created, path, _)| {
            let folder = guarded_meeting_folder(&path, base)?;
            let files = audio_paths(&folder);
            if audio_files(&folder).is_empty() {
                return None;
            }
            Some(OldAudio {
                meeting_id: id,
                title,
                created_at: created.0.to_rfc3339(),
                folder: folder.to_string_lossy().to_string(),
                bytes: files.iter().map(|f| size_of(f)).sum(),
                files: files.iter().map(|f| f.to_string_lossy().to_string()).collect(),
            })
        })
        .collect())
}

/// Moves exactly the confirmed meetings' audio to the Recycle Bin, re-listing first and
/// re-checking every file right before it is recycled.
pub async fn delete_old_audio_inner(
    pool: &SqlitePool,
    confirmed: &[String],
    days: Option<u32>,
    now: DateTime<Utc>,
    recording: bool,
    base: &Path,
    remove: Remover<'_>,
) -> Result<DeleteResult, String> {
    if recording {
        return Err("Stop the recording first.".into());
    }
    let mut result = DeleteResult::default();
    for item in list_old_audio(pool, days, now, false, base).await? {
        if !confirmed.contains(&item.meeting_id) {
            continue;
        }
        // Re-guard this folder now: earlier recycles may have waited minutes on a Windows prompt.
        let Some(folder) = guarded_meeting_folder(&item.folder, base) else {
            result.failed.push(format!("{}: the folder is no longer inside the recordings folder", item.title));
            continue;
        };
        let mut error = None;
        for f in audio_paths(&folder) {
            let size = size_of(&f);
            match recycle_checked(&f, &folder, remove) {
                Ok(()) => result.bytes += size,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        if let Err(e) = recycle_if_empty(&folder.join(".checkpoints"), remove) {
            warn!("Old audio: could not recycle the empty .checkpoints of {}: {e}", item.meeting_id);
        }
        if audio_files(&folder).is_empty() {
            if let Err(e) = sqlx::query("UPDATE meetings SET audio_deleted_at = ? WHERE id = ?")
                .bind(now.to_rfc3339())
                .bind(&item.meeting_id)
                .execute(pool)
                .await
            {
                warn!("Old audio: could not record the deletion for {}: {e}", item.meeting_id);
            }
            result.meetings += 1;
        }
        if let Some(e) = error {
            warn!("Old audio: {} not fully moved: {e}", item.meeting_id);
            result.failed.push(format!("{}: {e}", item.title));
        }
    }
    info!("Old audio: moved {} recordings ({} MB) to the Recycle Bin", result.meetings, result.bytes / 1_000_000);
    Ok(result)
}

#[tauri::command]
pub async fn old_audio_list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<OldAudio>, String> {
    let state = app.try_state::<crate::state::AppState>().ok_or("Database not ready")?;
    let prefs = load_recording_preferences(&app).await.unwrap_or_default();
    let recording = crate::audio::recording_commands::is_recording_now();
    list_old_audio(state.db_manager.pool(), prefs.old_audio_days, Utc::now(), recording, &get_default_recordings_folder()).await
}

#[tauri::command]
pub async fn delete_old_audio<R: Runtime>(app: AppHandle<R>, meeting_ids: Vec<String>) -> Result<DeleteResult, String> {
    let state = app.try_state::<crate::state::AppState>().ok_or("Database not ready")?;
    let prefs = load_recording_preferences(&app).await.unwrap_or_default();
    let recording = crate::audio::recording_commands::is_recording_now();
    delete_old_audio_inner(
        state.db_manager.pool(), &meeting_ids, prefs.old_audio_days, Utc::now(), recording,
        &get_default_recordings_folder(), &recycle,
    )
    .await
}
```

Register `audio::retention::old_audio_list, audio::retention::delete_old_audio` in `generate_handler!`. Nothing is spawned at startup.

- [ ] **Step 5: Run tests** — Step 2 command: PASS. Full workspace tests: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/audio/retention.rs frontend/src-tauri/src/audio/mod.rs frontend/src-tauri/src/audio/recording_preferences.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(storage): list old recordings and move the confirmed ones to the Recycle Bin"
```

---


### Task 5: Playback backend and dead-code removal

**Codex pass required** (widens what the webview can read).

**Files:**
- Modify: `frontend/src-tauri/src/audio/meeting_folder.rs` (command), `frontend/src-tauri/src/lib.rs` (remove `read_audio_file` fn and registration; register `get_meeting_audio`), `frontend/src-tauri/tauri.conf.json` (CSP)
- Delete: `frontend/src/hooks/useAudioPlayer.ts`, `frontend/src/components/AudioPlayer.tsx`, `frontend/src/app/notes/` (whole folder)
- Modify: `frontend/src/components/Sidebar/index.tsx` (route branch)
- Test: `meeting_folder.rs` tests

**Interfaces:**
- Consumes: Task 3 guard and `find_audio_file`; Task 1 `MeetingModel.keep_audio`, `audio_deleted_at`.
- Produces:
  - `#[derive(Serialize)] pub struct MeetingAudio { pub path: Option<String>, pub keep_audio: bool, pub audio_deleted_at: Option<String> }`
  - Command `get_meeting_audio(meetingId: string) -> MeetingAudio`: on success the asset protocol may read exactly `path`.

- [ ] **Step 1: Write the failing test** (append to `meeting_folder.rs` tests):

```rust
    #[tokio::test]
    async fn meeting_audio_reports_file_pin_and_deletion() {
        let pool = crate::database::repositories::test_pool().await;
        let b = base();
        let m = b.path().join("m");
        fs::create_dir(&m).unwrap();
        fs::write(m.join("audio.mp4"), "x").unwrap();
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at, folder_path, keep_audio, audio_deleted_at) VALUES ('m1', 't', '2026-10-15T00:00:00Z', '2026-10-15T00:00:00Z', ?, 1, '2026-11-20T10:00:00+00:00')")
            .bind(m.to_string_lossy().to_string())
            .execute(&pool).await.unwrap();
        let a = meeting_audio_inner(&pool, "m1", b.path()).await.unwrap();
        assert!(a.path.unwrap().ends_with("audio.mp4"));
        assert!(a.keep_audio);
        assert_eq!(a.audio_deleted_at.as_deref(), Some("2026-11-20T10:00:00+00:00"));
        fs::remove_file(m.join("audio.mp4")).unwrap();
        assert!(meeting_audio_inner(&pool, "m1", b.path()).await.unwrap().path.is_none());
        assert!(meeting_audio_inner(&pool, "nope", b.path()).await.is_err());
    }
```

- [ ] **Step 2: Run to verify failure** — `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- audio::meeting_folder`: compile error.

- [ ] **Step 3: Implement** (append to `meeting_folder.rs`):

```rust
use crate::audio::recording_preferences::get_default_recordings_folder;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Serialize)]
pub struct MeetingAudio {
    pub path: Option<String>,
    pub keep_audio: bool,
    /// Set when the user deleted the audio in the old-audio review; picks the player message.
    pub audio_deleted_at: Option<String>,
}

pub(crate) async fn meeting_audio_inner(pool: &sqlx::SqlitePool, meeting_id: &str, base: &Path) -> Result<MeetingAudio, String> {
    let meeting = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(pool, meeting_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Meeting not found: {meeting_id}"))?;
    let file = meeting
        .folder_path
        .as_deref()
        .and_then(|p| guarded_meeting_folder(p, base))
        .and_then(|folder| find_audio_file(&folder));
    Ok(MeetingAudio {
        path: file.map(|f| f.to_string_lossy().to_string()),
        keep_audio: meeting.keep_audio,
        audio_deleted_at: meeting.audio_deleted_at,
    })
}

/// Finds the meeting's audio and lets the asset protocol read that one file (spec §8.2).
#[tauri::command]
pub async fn get_meeting_audio<R: Runtime>(app: AppHandle<R>, meeting_id: String) -> Result<MeetingAudio, String> {
    let state = app.try_state::<crate::state::AppState>().ok_or("Database not ready")?;
    let audio = meeting_audio_inner(state.db_manager.pool(), &meeting_id, &get_default_recordings_folder()).await?;
    if let Some(p) = &audio.path {
        app.asset_protocol_scope().allow_file(p).map_err(|e| e.to_string())?;
    }
    Ok(audio)
}
```

Register `audio::meeting_folder::get_meeting_audio` in `generate_handler!`. Delete the `read_audio_file` function and its `read_audio_file,` registration in `lib.rs`.

`tauri.conf.json` `security.csp` gains:

```json
                "media-src": "'self' asset: http://asset.localhost https://asset.localhost",
```

and `assetProtocol.scope` becomes `[]` (nothing in `frontend/src` uses `convertFileSrc` or `asset:` today; recordings are added one file at a time at runtime). Confirm with `grep -rn "convertFileSrc\|asset:" frontend/src` before the change — the only hit afterwards must be `useMeetingAudio` (Task 10).

- [ ] **Step 4: Dead frontend code**

```bash
git rm frontend/src/hooks/useAudioPlayer.ts frontend/src/components/AudioPlayer.tsx
git rm -r frontend/src/app/notes
```

In `components/Sidebar/index.tsx` replace the `basePath` expression with:

```tsx
              const basePath = item.id.startsWith('intro-call') ? '/' : `/meeting-details?id=${item.id}`;
```

Confirm nothing else references them: `grep -rn "useAudioPlayer\|components/AudioPlayer\|/notes/\|read_audio_file" frontend/src frontend/src-tauri/src --exclude=lib_old_complex.rs` → no hits (`lib_old_complex.rs` is not compiled).

- [ ] **Step 5: Run tests** — Rust Step 2 command: PASS; full workspace tests: PASS; `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`: PASS; `bash scripts/check-no-phone-home.sh`: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/audio/meeting_folder.rs frontend/src-tauri/src/lib.rs frontend/src-tauri/tauri.conf.json frontend/src/components/Sidebar/index.tsx
git commit -m "feat(playback): serve one meeting's audio to the player; remove read_audio_file and dead pages"
```

---

### Task 6: Notes and marks in the summary prompt

**Files:**
- Create: `frontend/src-tauri/src/summary/user_notes.rs` (add `pub mod user_notes;` to `summary/mod.rs`)
- Modify: `frontend/src-tauri/src/summary/processor.rs`, `frontend/src-tauri/src/summary/service.rs`
- Create: `frontend/src/lib/meeting-notes.ts`; Modify: `frontend/src/types/index.ts`, `frontend/src/hooks/meeting-details/useSummaryGeneration.ts`
- Test: `user_notes.rs` tests, `processor.rs` tests, `frontend/tests/lib/meeting-notes.test.ts`

**Interfaces:**
- Consumes: `MeetingNotesRepository::get`, `MeetingNote` (Task 1).
- Produces (Rust):
  - `pub const NOTES_INSTRUCTION: &str`
  - `pub fn render_notes_block(notes: &[MeetingNote]) -> Option<String>` (`None` when empty)
  - `pub fn transcript_has_marks(text: &str) -> bool` (contains `"[MARKED: "`)
  - `pub fn marked_lines_block(original_text: &str) -> Option<String>` (`<marked_lines>` block with every transcript line containing `[MARKED: `; `None` if none)
  - `pub(crate) fn build_final_prompts(template: &Template, language_name: &str, content: &str, original_text: &str, chunked: bool, custom_prompt: &str, notes_block: Option<&str>) -> (String, String)` in `processor.rs`
  - `generate_meeting_summary(..., custom_prompt, notes_block: Option<&str>, template, ...)` — new argument right after `custom_prompt`.
- Produces (TS, `lib/meeting-notes.ts`):
  - `type TranscriptMark = 'important' | 'action' | 'followup'`, `interface MeetingNote { id: string; at: number | null; text: string }` (exported from `types/index.ts`; `Transcript.mark?: TranscriptMark | null`; `TranscriptSegmentData.mark?: TranscriptMark | null`)
  - `MARKS: ReadonlyArray<{ value: TranscriptMark; icon: string; label: string }>`, `NOTE_MAX_CHARS = 2000`
  - `toggleMark(current: TranscriptMark | null | undefined, chosen: TranscriptMark): TranscriptMark | null`
  - `formatRecordingTime(seconds: number | null | undefined): string` → `"MM:SS"`, `"--:--"` for missing
  - `formatSummaryTranscript(transcripts: Transcript[]): string`
  - `sortNotes(notes: MeetingNote[]): MeetingNote[]` (new array)
  - `cleanNoteText(text: string): { ok: true; text: string } | { ok: false; error: string }`
  - `newNote(text: string, at: number | null): MeetingNote` (id `note-${crypto.randomUUID()}`)
  - `collectMarks(transcripts: Transcript[]): Record<string, TranscriptMark>` (key `String(sequence_id)`)
  - `applyMarks(transcripts: Transcript[], marks: Record<string, TranscriptMark> | undefined): Transcript[]`

- [ ] **Step 1: Write the failing Rust tests** — `user_notes.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_block_formats_times_and_after_meeting() {
        let notes = vec![
            MeetingNote { id: "a".into(), at: Some(122.9), text: "ask about Q2 budget".into() },
            MeetingNote { id: "b".into(), at: None, text: "Pavel seemed unsure".into() },
        ];
        assert_eq!(
            render_notes_block(&notes).unwrap(),
            "<user_notes>\n[02:02] ask about Q2 budget\n[after the meeting] Pavel seemed unsure\n</user_notes>"
        );
        assert!(render_notes_block(&[]).is_none());
    }

    #[test]
    fn detects_marks() {
        assert!(transcript_has_marks("[00:01] [MARKED: action] send deck"));
        assert!(!transcript_has_marks("[00:01] send deck"));
    }

    #[test]
    fn marked_lines_block_keeps_only_marked_lines() {
        let text = "[00:01] hi\n[00:05] [MARKED: action] send deck\n[00:09] bye";
        assert_eq!(
            marked_lines_block(text).unwrap(),
            "<marked_lines>\n[00:05] [MARKED: action] send deck\n</marked_lines>"
        );
        assert!(marked_lines_block("[00:01] hi").is_none());
    }
}
```

`processor.rs` tests (add):

```rust
    #[test]
    fn final_prompts_add_notes_only_when_present() {
        let t: crate::summary::templates::Template =
            serde_json::from_value(serde_json::json!({"name": "T", "description": "", "sections": []})).unwrap();
        let (sys, user) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "", None);
        assert!(!sys.contains("marked transcript lines"));
        assert!(!user.contains("<user_notes>"));

        let marked = "[00:01] [MARKED: action] hi";
        let (sys, user) = build_final_prompts(&t, "English", marked, marked, false, "", None);
        assert!(sys.contains("marked transcript lines"));
        assert!(!user.contains("<user_notes>"));
        assert!(!user.contains("<marked_lines>")); // not chunked: the lines are already in the transcript

        let (sys, user) = build_final_prompts(&t, "English", "[00:01] hi", "[00:01] hi", false, "ctx", Some("<user_notes>\n[00:01] x\n</user_notes>"));
        assert!(sys.contains("marked transcript lines"));
        assert!(user.contains("</transcript_chunks>\n\n<user_notes>"));
        assert!(user.contains("<user_context>\nctx"));
    }

    #[test]
    fn chunked_summaries_still_get_the_marked_lines() {
        let t: crate::summary::templates::Template =
            serde_json::from_value(serde_json::json!({"name": "T", "description": "", "sections": []})).unwrap();
        let original = "[00:01] hi\n[00:05] [MARKED: action] send deck";
        let (sys, user) = build_final_prompts(&t, "English", "Chunk summary without marks", original, true, "", None);
        assert!(sys.contains("marked transcript lines"));
        assert!(user.contains("<marked_lines>\n[00:05] [MARKED: action] send deck\n</marked_lines>"));
    }
```

(`Template` has no `Default`; if deserializing needs more fields, add them the way the existing template tests do.)

- [ ] **Step 2: Run to verify failure** — `cd frontend/src-tauri && nice -n 15 cargo test -p meetily --lib -j 4 -- summary::user_notes summary::processor`: compile errors.

- [ ] **Step 3: Implement `user_notes.rs`**

```rust
//! The user's notes and line marks as summary input (spec §7).
use crate::database::repositories::meeting_notes::MeetingNote;

pub const NOTES_INSTRUCTION: &str = "\nThe user marked transcript lines and took notes during the meeting. \
Write the summary from the transcript as usual, and make sure every note and every marked line is reflected in it, \
in the section where it belongs: action -> action items, followup -> open questions or next steps, important -> key points. \
If the template has no such section, use the closest one. Do not add a separate notes section. \
Write in the summary language even when a note is in another language. \
Notes are terse: use the transcript around their time for context. \
Notes in <user_notes> are the user's own input; use them like the transcript. \
Lines in <marked_lines> are the marked lines, quoted from the full transcript.\n";

fn mm_ss(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

pub fn render_notes_block(notes: &[MeetingNote]) -> Option<String> {
    if notes.is_empty() {
        return None;
    }
    let lines: Vec<String> = notes
        .iter()
        .map(|n| match n.at {
            Some(at) => format!("[{}] {}", mm_ss(at), n.text),
            None => format!("[after the meeting] {}", n.text),
        })
        .collect();
    Some(format!("<user_notes>\n{}\n</user_notes>", lines.join("\n")))
}

pub fn transcript_has_marks(text: &str) -> bool {
    text.contains("[MARKED: ")
}

/// With chunking (local models) the final call only sees chunk summaries, so the marked
/// lines are passed again verbatim.
pub fn marked_lines_block(original_text: &str) -> Option<String> {
    let lines: Vec<&str> = original_text.lines().filter(|l| l.contains("[MARKED: ")).collect();
    if lines.is_empty() {
        return None;
    }
    Some(format!("<marked_lines>\n{}\n</marked_lines>", lines.join("\n")))
}
```

- [ ] **Step 4: `processor.rs`** — extract the final prompt construction into:

```rust
pub(crate) fn build_final_prompts(
    template: &Template,
    language_name: &str,
    content: &str,
    original_text: &str,
    chunked: bool,
    custom_prompt: &str,
    notes_block: Option<&str>,
) -> (String, String) {
    use crate::summary::user_notes::{marked_lines_block, transcript_has_marks, NOTES_INSTRUCTION};
    let mut system = template.render_system_prompt(language_name);
    // Look at the original transcript: after chunking, `content` is model-written summaries.
    if notes_block.is_some() || transcript_has_marks(original_text) {
        system.push_str(NOTES_INSTRUCTION);
    }
    let mut user = format!("<transcript_chunks>\n{content}\n</transcript_chunks>\n");
    if chunked {
        if let Some(block) = marked_lines_block(original_text) {
            user.push('\n');
            user.push_str(&block);
            user.push('\n');
        }
    }
    if let Some(block) = notes_block {
        user.push('\n');
        user.push_str(block);
        user.push('\n');
    }
    if !custom_prompt.is_empty() {
        user.push_str("\n\nUser Provided Context:\n\n<user_context>\n");
        user.push_str(custom_prompt);
        user.push_str("\n</user_context>");
    }
    (system, user)
}
```

and replace the inline `final_system_prompt` / `final_user_prompt` code in `generate_meeting_summary` with `let (final_system_prompt, final_user_prompt) = build_final_prompts(template, language_name, &content_to_summarize, text, chunked, custom_prompt, notes_block);`, where `chunked` is true in the branch that ran chunk summaries (`processor.rs` ~341-363; set a `let mut chunked = false;` before the branch and `chunked = true;` inside it). Add the `notes_block: Option<&str>` parameter after `custom_prompt`.

- [ ] **Step 5: `service.rs`** — just before the `generate_meeting_summary(` call:

```rust
        let notes = crate::database::repositories::meeting_notes::MeetingNotesRepository::get(&pool, &meeting_id)
            .await
            .unwrap_or_else(|e| {
                warn!("Could not read notes for {}: {}", meeting_id, e);
                Vec::new()
            });
        let notes_block = crate::summary::user_notes::render_notes_block(&notes);
```

and pass `notes_block.as_deref()` right after `&custom_prompt`. (Use `log::warn!`/`tracing::warn!` as already imported in `service.rs`.)

- [ ] **Step 6: Run Rust tests** — Step 2 command: PASS; full workspace: PASS.

- [ ] **Step 7: Write the failing TS tests** — `frontend/tests/lib/meeting-notes.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import {
  applyMarks, cleanNoteText, collectMarks, formatRecordingTime, formatSummaryTranscript,
  NOTE_MAX_CHARS, newNote, sortNotes, toggleMark,
} from '../../src/lib/meeting-notes';
import type { Transcript } from '../../src/types';

const t = (p: Partial<Transcript>): Transcript => ({ id: 'x', text: 'hi', timestamp: '10:00:00', ...p });

describe('meeting-notes', () => {
  test('toggleMark sets, replaces and removes', () => {
    expect(toggleMark(null, 'action')).toBe('action');
    expect(toggleMark('action', 'important')).toBe('important');
    expect(toggleMark('important', 'important')).toBeNull();
  });

  test('formatRecordingTime', () => {
    expect(formatRecordingTime(0)).toBe('00:00');
    expect(formatRecordingTime(122.9)).toBe('02:02');
    expect(formatRecordingTime(null)).toBe('--:--');
    expect(formatRecordingTime(undefined)).toBe('--:--');
  });

  test('summary transcript puts marks inline, words not emoji', () => {
    const text = formatSummaryTranscript([
      t({ audio_start_time: 183, text: 'we move it to Q2', mark: 'important' }),
      t({ audio_start_time: 366, text: 'send the deck', mark: 'action' }),
      t({ audio_start_time: 400, text: 'plain' }),
      t({ timestamp: '10:07:00', text: 'no time', mark: 'followup' }),
    ]);
    expect(text).toBe(
      '[03:03] [MARKED: important] we move it to Q2\n' +
      '[06:06] [MARKED: action] send the deck\n' +
      '[06:40] plain\n' +
      '10:07:00 [MARKED: followup] no time',
    );
  });

  test('sortNotes: by time, untimed last, stable, new array', () => {
    const notes = [
      { id: 'a', at: null, text: 'a' }, { id: 'b', at: 9, text: 'b' },
      { id: 'c', at: null, text: 'c' }, { id: 'd', at: 2, text: 'd' },
    ];
    const sorted = sortNotes(notes);
    expect(sorted.map(n => n.id)).toEqual(['d', 'b', 'a', 'c']);
    expect(sorted).not.toBe(notes);
  });

  test('cleanNoteText trims and limits', () => {
    expect(cleanNoteText('  hi ')).toEqual({ ok: true, text: 'hi' });
    expect(cleanNoteText('   ').ok).toBe(false);
    expect(cleanNoteText('x'.repeat(NOTE_MAX_CHARS)).ok).toBe(true);
    expect(cleanNoteText('x'.repeat(NOTE_MAX_CHARS + 1)).ok).toBe(false);
  });

  test('newNote ids are prefixed and unique', () => {
    const a = newNote('a', 1), b = newNote('b', null);
    expect(a.id.startsWith('note-')).toBe(true);
    expect(a.id).not.toBe(b.id);
    expect(b.at).toBeNull();
  });

  test('collectMarks / applyMarks round-trip by sequence_id', () => {
    const live = [t({ id: '1', sequence_id: 1, mark: 'action' }), t({ id: '2', sequence_id: 2 }), t({ id: '3', mark: 'important' })];
    const marks = collectMarks(live);
    expect(marks).toEqual({ '1': 'action' });
    const reloaded = [t({ id: 'seg_1', sequence_id: 1 }), t({ id: 'seg_2', sequence_id: 2 })];
    expect(applyMarks(reloaded, marks).map(x => x.mark ?? null)).toEqual(['action', null]);
    expect(applyMarks(reloaded, undefined)).toEqual(reloaded);
  });
});
```

- [ ] **Step 8: Run to verify failure** — `cd frontend && bun test tests/lib/meeting-notes.test.ts`: FAIL (module not found).

- [ ] **Step 9: Implement** — `types/index.ts`: add

```ts
export type TranscriptMark = 'important' | 'action' | 'followup';

export interface MeetingNote {
  id: string;
  at: number | null; // seconds into the recording; null = added after the meeting
  text: string;
}
```

plus `mark?: TranscriptMark | null;` on `Transcript` and `TranscriptSegmentData`, and `keep_audio?: boolean;` on `MeetingMetadata`.

`frontend/src/lib/meeting-notes.ts`:

```ts
import type { MeetingNote, Transcript, TranscriptMark } from '@/types';

export const NOTE_MAX_CHARS = 2000;

export const MARKS: ReadonlyArray<{ value: TranscriptMark; icon: string; label: string }> = [
  { value: 'important', icon: '⭐', label: 'Important' },
  { value: 'action', icon: '✅', label: 'Action item' },
  { value: 'followup', icon: '❓', label: 'Follow-up' },
];

export function toggleMark(current: TranscriptMark | null | undefined, chosen: TranscriptMark): TranscriptMark | null {
  return current === chosen ? null : chosen;
}

export function formatRecordingTime(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '--:--';
  const total = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(total / 60)).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`;
}

/** Transcript text for the summary; marks go inline as words (spec §7). */
export function formatSummaryTranscript(transcripts: Transcript[]): string {
  return transcripts
    .map((t) => {
      const time = t.audio_start_time === undefined ? t.timestamp : `[${formatRecordingTime(t.audio_start_time)}]`;
      const mark = t.mark ? ` [MARKED: ${t.mark}]` : '';
      return `${time}${mark} ${t.text}`;
    })
    .join('\n');
}

export function sortNotes(notes: MeetingNote[]): MeetingNote[] {
  return [...notes].sort((a, b) => {
    if (a.at === null && b.at === null) return 0;
    if (a.at === null) return 1;
    if (b.at === null) return -1;
    return a.at - b.at;
  });
}

export function cleanNoteText(text: string): { ok: true; text: string } | { ok: false; error: string } {
  const trimmed = text.trim();
  if (!trimmed) return { ok: false, error: 'A note cannot be empty' };
  if ([...trimmed].length > NOTE_MAX_CHARS) return { ok: false, error: `A note can be at most ${NOTE_MAX_CHARS} characters` };
  return { ok: true, text: trimmed };
}

export function newNote(text: string, at: number | null): MeetingNote {
  return { id: `note-${crypto.randomUUID()}`, at, text };
}

/** Live marks keyed by sequence_id, the one line key that survives a reload (spec §5.1). */
export function collectMarks(transcripts: Transcript[]): Record<string, TranscriptMark> {
  const marks: Record<string, TranscriptMark> = {};
  for (const t of transcripts) {
    if (t.mark && t.sequence_id !== undefined) marks[String(t.sequence_id)] = t.mark;
  }
  return marks;
}

export function applyMarks(transcripts: Transcript[], marks: Record<string, TranscriptMark> | undefined): Transcript[] {
  if (!marks) return transcripts;
  return transcripts.map((t) => {
    const mark = t.sequence_id === undefined ? undefined : marks[String(t.sequence_id)];
    return mark ? { ...t, mark } : t;
  });
}
```

In `useSummaryGeneration.ts`, `buildSummaryTranscriptPayload` becomes:

```ts
  const buildSummaryTranscriptPayload = useCallback((allTranscripts: Transcript[]) => ({
    transcriptText: formatSummaryTranscript(allTranscripts),
  }), []);
```

(import `formatSummaryTranscript` from `@/lib/meeting-notes`; the transcripts come from `api_get_meeting_transcripts`, which now returns `mark`).

- [ ] **Step 10: Run tests** — `cd frontend && pnpm exec tsc --noEmit && bun test`: PASS (including the existing `summary-generation` tests; update their expected payload only if they assert the exact text and the expectation is unchanged for unmarked lines).

- [ ] **Step 11: Commit**

```bash
git add frontend/src-tauri/src/summary frontend/src/lib/meeting-notes.ts frontend/src/types/index.ts frontend/src/hooks/meeting-details/useSummaryGeneration.ts frontend/tests/lib/meeting-notes.test.ts
git commit -m "feat(summary): weave the user's notes and line marks into the summary"
```

---

### Task 7: Layout setting and a reusable splitter

**Files:**
- Create: `frontend/src/lib/meeting-page-layout.ts`, `frontend/src/hooks/useSplitRatio.tsx`
- Modify: `frontend/src/components/MeetingDetails/MeetingDetailsSplitView.tsx` (use the hook; no behaviour change), `frontend/src/components/PreferenceSettings.tsx` (setting)
- Test: `frontend/tests/lib/meeting-page-layout.test.ts`

**Interfaces:**
- Produces:
  - `type MeetingPageLayout = 'three-columns' | 'subtab'`; `parseMeetingPageLayout(v: unknown): MeetingPageLayout`; `readMeetingPageLayout(): MeetingPageLayout`; `writeMeetingPageLayout(v: MeetingPageLayout): void` (key `conclave.meetingPageLayout`, default `three-columns`)
  - `readStoredRatio(key: string, fallback: number, min: number, max: number): number`, `writeStoredRatio(key: string, value: number): void`, `readFlag(key: string): boolean`, `writeFlag(key: string, value: boolean): void` (all try/catch)
  - `useSplitRatio({ storageKey, defaultRatio, min, max, offset = 0, label }: SplitOptions)` → `{ ratio: number; containerRef: MutableRefObject<HTMLDivElement | null>; containerHandlers: { onPointerMove; onPointerUp; onPointerCancel }; separatorProps: object }` where `separatorProps` spreads onto the separator `<div>` (role, aria-*, tabIndex, onPointerDown, onKeyDown). `offset` is subtracted from the pointer fraction (for a second splitter).

- [ ] **Step 1: Failing test** — `frontend/tests/lib/meeting-page-layout.test.ts`:

```ts
import { beforeEach, describe, expect, test } from 'bun:test';
import { parseMeetingPageLayout, readMeetingPageLayout, writeMeetingPageLayout, readStoredRatio, writeStoredRatio, readFlag, writeFlag } from '../../src/lib/meeting-page-layout';

beforeEach(() => localStorage.clear());

describe('meeting page layout', () => {
  test('defaults to three columns and rejects junk', () => {
    expect(readMeetingPageLayout()).toBe('three-columns');
    expect(parseMeetingPageLayout('subtab')).toBe('subtab');
    expect(parseMeetingPageLayout('nonsense')).toBe('three-columns');
    writeMeetingPageLayout('subtab');
    expect(readMeetingPageLayout()).toBe('subtab');
  });

  test('ratios are stored per key and clamped to their range', () => {
    writeStoredRatio('conclave.threeCol.transcriptRatio', 0.4);
    writeStoredRatio('meetily.meetingDetails.transcriptPaneRatio', 0.45);
    expect(readStoredRatio('conclave.threeCol.transcriptRatio', 0.3, 0.2, 0.5)).toBe(0.4);
    expect(readStoredRatio('meetily.meetingDetails.transcriptPaneRatio', 0.3, 0.3, 0.5)).toBe(0.45);
    writeStoredRatio('k', 0.9);
    expect(readStoredRatio('k', 0.3, 0.2, 0.5)).toBe(0.3);
    expect(readStoredRatio('missing', 0.3, 0.2, 0.5)).toBe(0.3);
  });

  test('flags', () => {
    expect(readFlag('conclave.notesPaneCollapsed')).toBe(false);
    writeFlag('conclave.notesPaneCollapsed', true);
    expect(readFlag('conclave.notesPaneCollapsed')).toBe(true);
  });
});
```

- [ ] **Step 2: Verify failure** — `cd frontend && bun test tests/lib/meeting-page-layout.test.ts`: FAIL.

- [ ] **Step 3: Implement `lib/meeting-page-layout.ts`**

```ts
export type MeetingPageLayout = 'three-columns' | 'subtab';

const LAYOUT_KEY = 'conclave.meetingPageLayout';

export function parseMeetingPageLayout(v: unknown): MeetingPageLayout {
  return v === 'subtab' ? 'subtab' : 'three-columns';
}

export function readMeetingPageLayout(): MeetingPageLayout {
  try {
    return parseMeetingPageLayout(localStorage.getItem(LAYOUT_KEY));
  } catch {
    return 'three-columns';
  }
}

export function writeMeetingPageLayout(v: MeetingPageLayout): void {
  try { localStorage.setItem(LAYOUT_KEY, v); } catch { /* optional */ }
}

export function readStoredRatio(key: string, fallback: number, min: number, max: number): number {
  try {
    const raw = localStorage.getItem(key);
    const n = raw == null ? NaN : Number(raw);
    return Number.isFinite(n) && n >= min && n <= max ? n : fallback;
  } catch {
    return fallback;
  }
}

export function writeStoredRatio(key: string, value: number): void {
  try { localStorage.setItem(key, String(value)); } catch { /* layout persistence is optional */ }
}

export function readFlag(key: string): boolean {
  try { return localStorage.getItem(key) === '1'; } catch { return false; }
}

export function writeFlag(key: string, value: boolean): void {
  try { localStorage.setItem(key, value ? '1' : '0'); } catch { /* optional */ }
}
```

- [ ] **Step 4: `hooks/useSplitRatio.tsx`** — move the ratio state, pointer and keyboard logic out of `MeetingDetailsSplitView` unchanged in behaviour:

```ts
'use client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { readStoredRatio, writeStoredRatio } from '@/lib/meeting-page-layout';

interface SplitOptions {
  storageKey: string;
  defaultRatio: number;
  min: number;
  max: number;
  /** Subtracted from the pointer position, for a second splitter in the same container. */
  offset?: number;
  label: string;
}

export function useSplitRatio({ storageKey, defaultRatio, min, max, offset = 0, label }: SplitOptions) {
  // Mutable so two splitters can share one container (Task 10).
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [ratio, setRatio] = useState(defaultRatio);
  const dragging = useRef(false);
  const clamp = useCallback((v: number) => Math.min(max, Math.max(min, v)), [min, max]);

  useEffect(() => { setRatio(readStoredRatio(storageKey, defaultRatio, min, max)); }, [storageKey, defaultRatio, min, max]);

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    dragging.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
  }, []);
  const onPointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragging.current || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    setRatio(clamp((e.clientX - rect.left) / rect.width - offset));
  }, [clamp, offset]);
  const onPointerUp = useCallback(() => {
    if (!dragging.current) return;
    dragging.current = false;
    setRatio((current) => { writeStoredRatio(storageKey, current); return current; });
  }, [storageKey]);
  const onKeyDown = useCallback((e: React.KeyboardEvent) => {
    const next =
      e.key === 'ArrowLeft' ? clamp(ratio - 0.05) :
      e.key === 'ArrowRight' ? clamp(ratio + 0.05) :
      e.key === 'Home' ? min :
      e.key === 'End' ? max : null;
    if (next === null) return;
    e.preventDefault();
    setRatio(next);
    writeStoredRatio(storageKey, next);
  }, [ratio, clamp, min, max, storageKey]);

  return {
    ratio,
    containerRef,
    containerHandlers: { onPointerMove, onPointerUp, onPointerCancel: onPointerUp },
    separatorProps: {
      role: 'separator' as const,
      'aria-orientation': 'vertical' as const,
      'aria-valuenow': Math.round(ratio * 100),
      'aria-valuemin': Math.round(min * 100),
      'aria-valuemax': Math.round(max * 100),
      'aria-valuetext': `${label} ${Math.round(ratio * 100)} percent`,
      'aria-label': `Resize ${label}`,
      tabIndex: 0,
      onPointerDown,
      onKeyDown,
    },
  };
}
```

A shared separator element (in the same `useSplitRatio.tsx`, exported):

```tsx
export function SplitHandle(props: Record<string, unknown>) {
  return (
    <div
      {...props}
      className="group relative z-10 hidden w-2 flex-shrink-0 cursor-col-resize items-stretch justify-center focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-inset md:flex"
    >
      <div className="h-full w-px bg-border transition-[width,background-color] duration-150 ease-out group-hover:w-1 group-hover:bg-blue-400 group-active:w-1 group-active:bg-blue-500" />
    </div>
  );
}
```

In `MeetingDetailsSplitView.tsx` delete `readStoredRatio`/`writeStoredRatio`/the pointer and key handlers and use `useSplitRatio({ storageKey: 'meetily.meetingDetails.transcriptPaneRatio', defaultRatio: 0.3, min: 0.3, max: 0.5, label: 'transcript and summary' })`, spreading `containerHandlers` on the container and `<SplitHandle {...separatorProps} />` for the separator. Keep `aria-label="Resize transcript and summary"` (the hook produces exactly that).

- [ ] **Step 5: Settings** — in `PreferenceSettings.tsx`, below the Theme row, add a row with the same markup as Theme:

```tsx
        <div className="flex items-center justify-between">
          <div>
            <h4 className="font-medium">Meeting page layout</h4>
            <p className="text-sm text-muted-foreground">Where your notes sit on a saved meeting</p>
          </div>
          <Select value={layout} onValueChange={(v) => { const next = parseMeetingPageLayout(v); setLayout(next); writeMeetingPageLayout(next); }}>
            <SelectTrigger className="w-56" aria-label="Meeting page layout"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="three-columns">Three columns</SelectItem>
              <SelectItem value="subtab">Notes as a tab</SelectItem>
            </SelectContent>
          </Select>
        </div>
```

with `const [layout, setLayout] = useState<MeetingPageLayout>('three-columns'); useEffect(() => setLayout(readMeetingPageLayout()), []);`. Match the heading/description element types the Theme row uses.

- [ ] **Step 6: Run** — `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`: PASS (existing `meeting-details-refresh` tests still pass).

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/meeting-page-layout.ts frontend/src/hooks/useSplitRatio.tsx frontend/src/components/MeetingDetails/MeetingDetailsSplitView.tsx frontend/src/components/PreferenceSettings.tsx frontend/tests/lib/meeting-page-layout.test.ts
git commit -m "feat(ui): meeting page layout setting and a reusable splitter"
```

---

### Task 8: Transcript lines — mark popover, clickable timestamp, active line

**Files:**
- Create: `frontend/src/components/TranscriptMarkButton.tsx`, `frontend/src/lib/meeting-audio.ts` (only `findActiveSegmentId` in this task)
- Modify: `frontend/src/components/VirtualizedTranscriptView.tsx`
- Test: `frontend/tests/components/transcript-marks.test.tsx`, `frontend/tests/lib/meeting-audio.test.ts`

**Interfaces:**
- Consumes: `MARKS`, `toggleMark`, `formatRecordingTime` (Task 6); `PopoverAnchor`, `PopoverContent`, `Popover` from `components/ui/popover`.
- Produces:
  - `VirtualizedTranscriptView` new optional props: `onMarkChange?: (segmentId: string, mark: TranscriptMark | null) => void`, `onPlayFrom?: (seconds: number) => void`, `playheadSeconds?: number | null`.
  - `findActiveSegmentId(segments: TranscriptSegmentData[], t: number | null | undefined): string | null` — the last segment with `timestamp <= t` whose `endTime` (or the next segment's timestamp) is after `t`; `null` when none.
  - `TranscriptMarkButton({ text, mark, onChange })`: renders the line text as the popover anchor.

Behaviour (spec §6.2): clicking the line text opens the popover unless the click ended a text selection (`window.getSelection()?.toString()` non-empty). Enter/Space open it, Escape closes. Picking an icon calls `onChange(toggleMark(mark, value))` and closes. A marked line shows the icon before the text and the classes `bg-amber-50 dark:bg-amber-950/30 rounded` (theme tokens follow Phase 3's palette remap). Without `onMarkChange`, lines render exactly as today. Clicking the timestamp calls `onPlayFrom(segment.timestamp)` only when `onPlayFrom` is set; it is then a `<button aria-label="Play from MM:SS">`; otherwise the current plain span stays. The active segment (from `playheadSeconds`) gets `ring-1 ring-blue-400 rounded`.

- [ ] **Step 1: Failing tests** — `frontend/tests/lib/meeting-audio.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import { findActiveSegmentId } from '../../src/lib/meeting-audio';

const segs = [
  { id: 'a', timestamp: 0, endTime: 4, text: 'a' },
  { id: 'b', timestamp: 5, text: 'b' },
  { id: 'c', timestamp: 9, endTime: 12, text: 'c' },
];

describe('findActiveSegmentId', () => {
  test('finds the segment under the playhead', () => {
    expect(findActiveSegmentId(segs, 1)).toBe('a');
    expect(findActiveSegmentId(segs, 6)).toBe('b'); // no endTime: until the next start
    expect(findActiveSegmentId(segs, 10)).toBe('c');
  });
  test('gaps, before start, after end and no playhead', () => {
    expect(findActiveSegmentId(segs, 4.5)).toBeNull();
    expect(findActiveSegmentId(segs, 13)).toBeNull();
    expect(findActiveSegmentId(segs, null)).toBeNull();
    expect(findActiveSegmentId([], 1)).toBeNull();
  });
});
```

`frontend/tests/components/transcript-marks.test.tsx` (react-test-renderer, mock `components/ui/popover` as pass-through like `summary-panel-language.test.tsx` does, restoring it in `afterAll`):

```tsx
import { afterAll, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create } from 'react-test-renderer';

const originalPopover = { ...await import('../../src/components/ui/popover') };
afterAll(() => mock.module('../../src/components/ui/popover', () => originalPopover));
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/popover', () => ({
  Popover: ({ open, children }: { open: boolean; children?: ReactNode }) => <div data-open={String(open)}>{children}</div>,
  PopoverAnchor: pass, PopoverTrigger: pass,
  PopoverContent: pass,
}));
const { TranscriptMarkButton } = await import('../../src/components/TranscriptMarkButton');

function render(mark: 'important' | 'action' | 'followup' | null, onChange = mock(() => {})) {
  let r!: ReturnType<typeof create>;
  act(() => { r = create(<TranscriptMarkButton text="send the deck" mark={mark} onChange={onChange} />); });
  return { r, onChange };
}
const isOpen = (r: ReturnType<typeof create>) => r.root.findByProps({ 'data-open': 'true' }) !== undefined;

describe('TranscriptMarkButton', () => {
  test('click opens, choosing a mark reports it and closes', () => {
    const { r, onChange } = render(null);
    const anchor = r.root.findByProps({ role: 'button', 'aria-haspopup': 'menu' });
    act(() => anchor.props.onClick());
    expect(isOpen(r)).toBe(true);
    act(() => r.root.findByProps({ 'aria-label': 'Mark as Action item' }).props.onClick());
    expect(onChange).toHaveBeenCalledWith('action');
    expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(0);
  });

  test('choosing the current mark removes it', () => {
    const { r, onChange } = render('action');
    act(() => r.root.findByProps({ 'aria-haspopup': 'menu' }).props.onClick());
    act(() => r.root.findByProps({ 'aria-label': 'Mark as Action item' }).props.onClick());
    expect(onChange).toHaveBeenCalledWith(null);
  });

  test('a click that ends a text selection does not open', () => {
    const { r } = render(null);
    const sel = window.getSelection;
    (window as any).getSelection = () => ({ toString: () => 'send' });
    act(() => r.root.findByProps({ 'aria-haspopup': 'menu' }).props.onClick());
    (window as any).getSelection = sel;
    expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(0);
  });

  test('keyboard: Enter and Space open, Escape closes', () => {
    const { r } = render(null);
    const anchor = () => r.root.findByProps({ 'aria-haspopup': 'menu' });
    for (const key of ['Enter', ' ']) {
      act(() => anchor().props.onKeyDown({ key, preventDefault() {} }));
      expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(1);
      act(() => anchor().props.onKeyDown({ key: 'Escape', preventDefault() {} }));
      expect(r.root.findAllByProps({ 'data-open': 'true' }).length).toBe(0);
    }
  });

  test('shows the mark icon before the text', () => {
    const { r } = render('important');
    expect(JSON.stringify(r.toJSON())).toContain('⭐');
  });
});
```

- [ ] **Step 2: Verify failure** — `cd frontend && bun test tests/lib/meeting-audio.test.ts tests/components/transcript-marks.test.tsx`: FAIL.

- [ ] **Step 3: Implement** — `lib/meeting-audio.ts`:

```ts
import type { TranscriptSegmentData } from '@/types';

export function findActiveSegmentId(segments: TranscriptSegmentData[], t: number | null | undefined): string | null {
  if (t === null || t === undefined) return null;
  for (let i = segments.length - 1; i >= 0; i--) {
    const s = segments[i];
    if (s.timestamp > t) continue;
    const end = s.endTime ?? segments[i + 1]?.timestamp ?? Infinity;
    return t < end ? s.id : null;
  }
  return null;
}
```

`components/TranscriptMarkButton.tsx`:

```tsx
'use client';
import { useState } from 'react';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { MARKS, toggleMark } from '@/lib/meeting-notes';
import type { TranscriptMark } from '@/types';

interface Props {
  text: string;
  mark: TranscriptMark | null | undefined;
  onChange: (mark: TranscriptMark | null) => void;
}

export function TranscriptMarkButton({ text, mark, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const icon = MARKS.find((m) => m.value === mark)?.icon;

  const openUnlessSelecting = () => {
    if (window.getSelection()?.toString()) return; // the user is copying text
    setOpen(true);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <div
          role="button"
          tabIndex={0}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Mark line: ${text}`}
          onClick={openUnlessSelecting}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setOpen(true); }
            else if (e.key === 'Escape') setOpen(false);
          }}
          className={`cursor-pointer select-text ${mark ? 'bg-amber-50 dark:bg-amber-950/30 rounded px-1 -mx-1' : ''}`}
        >
          <p className="text-base text-foreground leading-relaxed">
            {icon && <span aria-hidden="true" className="mr-1">{icon}</span>}
            {text}
          </p>
        </div>
      </PopoverAnchor>
      <PopoverContent className="w-auto p-1" align="start">
        <div role="menu" className="flex gap-1">
          {MARKS.map((m) => (
            <button
              key={m.value}
              type="button"
              role="menuitemradio"
              aria-checked={mark === m.value}
              aria-label={`Mark as ${m.label}`}
              title={m.label}
              className={`rounded px-2 py-1 text-lg hover:bg-accent ${mark === m.value ? 'bg-accent' : ''}`}
              onClick={() => { onChange(toggleMark(mark, m.value)); setOpen(false); }}
            >
              {m.icon}
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}
```

`VirtualizedTranscriptView.tsx`: add the three props to `VirtualizedTranscriptViewProps` and the destructuring; compute `const activeId = findActiveSegmentId(segments, playheadSeconds);` once per render. Extend the memo `TranscriptSegment` with props `mark`, `isActive`, `onMarkChange?: (mark: TranscriptMark | null) => void`, `onPlay?: () => void`:

- This file already has a local `formatRecordingTime` returning `[MM:SS]`; keep it for the display and import the library one under an alias: `import { formatRecordingTime as formatMmSs } from '@/lib/meeting-notes';`.
- timestamp: when `onPlay` is set, render `<TooltipTrigger asChild><button type="button" onClick={onPlay} aria-label={`Play from ${formatMmSs(timestamp)}`} className="text-xs text-blue-600 dark:text-blue-400 hover:underline mt-1 flex-shrink-0 min-w-[50px] text-left">{formatRecordingTime(timestamp)}</button></TooltipTrigger>` — `asChild` is required, because the current `TooltipTrigger` (line ~88) renders its own `<button>` and a button inside a button is invalid. Without `onPlay`, keep today's `<TooltipTrigger><span …>` unchanged.
- text (non-streaming branch): `onMarkChange ? <TranscriptMarkButton text={displayText} mark={mark} onChange={onMarkChange} /> : <p ...>{displayText}</p>`. The streaming branch stays as is (the newest live line can be marked once it stops streaming).
- outer div: add `isActive ? 'ring-1 ring-blue-400 rounded' : ''`.

At both render sites pass `mark={segment.mark}`, `isActive={segment.id === activeId}`, `onMarkChange={onMarkChange ? (m) => onMarkChange(segment.id, m) : undefined}`, `onPlay={onPlayFrom ? () => onPlayFrom(segment.timestamp) : undefined}`. Because inline closures defeat `memo`, wrap the parent callbacks once: create them inside the map is acceptable here (segments are already re-rendered on every streaming update); do not add `useCallback` gymnastics.

- [ ] **Step 4: Run** — `cd frontend && pnpm exec tsc --noEmit && bun test`: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/TranscriptMarkButton.tsx frontend/src/components/VirtualizedTranscriptView.tsx frontend/src/lib/meeting-audio.ts frontend/tests/lib/meeting-audio.test.ts frontend/tests/components/transcript-marks.test.tsx
git commit -m "feat(transcript): mark lines, play from a timestamp, highlight the playing line"
```

---

### Task 9: Live notes and marks during a recording

**Files:**
- Create: `frontend/src/components/MeetingNotes/NotesPane.tsx`
- Modify: `frontend/src/contexts/TranscriptContext.tsx`, `frontend/src/services/indexedDBService.ts`, `frontend/src/services/storageService.ts`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src/hooks/useTranscriptRecovery.ts`, `frontend/src/app/page.tsx`, `frontend/src/app/_components/TranscriptPanel.tsx`
- Test: `frontend/tests/components/notes-pane.test.tsx`

**Interfaces:**
- Consumes: Task 6 helpers (`newNote`, `cleanNoteText`, `sortNotes`, `collectMarks`, `applyMarks`, `formatRecordingTime`); Task 7 `useSplitRatio`, `SplitHandle`, `readFlag`, `writeFlag`; Task 8 `onMarkChange`; Task 1/2 `api_save_transcript` `notes` argument.
- Produces:
  - `NotesPane({ notes, onAdd, onUpdate, onDelete, onTimeClick?, collapsed, onToggleCollapsed, disabled?, disabledHint? })` — `onAdd(text: string)`, `onUpdate(id: string, text: string)`, `onDelete(id: string)`, `onTimeClick?(at: number)`.
  - `TranscriptContext` adds: `notes: MeetingNote[]`, `notesRef: MutableRefObject<MeetingNote[]>`, `addNote(text: string, at: number | null): void`, `updateNote(id: string, text: string): void`, `deleteNote(id: string): void`, `setTranscriptMark(transcriptId: string, mark: TranscriptMark | null): void`.
  - `indexedDBService.patchMeetingMetadata(meetingId: string, patch: Partial<MeetingMetadata>): Promise<void>` — read-modify-write in **one** `readwrite` transaction, so concurrent updates (folder path, transcript count, notes, marks) never overwrite each other; `MeetingMetadata` (IndexedDB) gains `notes?: MeetingNote[]; marks?: Record<string, TranscriptMark>`.
  - `storageService.saveMeeting(meetingTitle, transcripts, folderPath, notes: MeetingNote[] = [])` → invokes `api_save_transcript` with `{ meetingTitle, transcripts, folderPath, notes }`.

- [ ] **Step 1: Failing test** — `frontend/tests/components/notes-pane.test.tsx`:

```tsx
import { describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { NotesPane } from '../../src/components/MeetingNotes/NotesPane';

const notes = [
  { id: 'n2', at: null, text: 'after' },
  { id: 'n1', at: 122, text: 'ask about Q2 budget' },
];

function render(extra: Partial<Parameters<typeof NotesPane>[0]> = {}) {
  const props = {
    notes, onAdd: mock(() => {}), onUpdate: mock(() => {}), onDelete: mock(() => {}),
    collapsed: false, onToggleCollapsed: mock(() => {}), ...extra,
  };
  let r!: ReturnType<typeof create>;
  act(() => { r = create(<NotesPane {...props} />); });
  return { r, props };
}

describe('NotesPane', () => {
  test('lists notes sorted, with times and "added later"', () => {
    const { r } = render();
    const text = JSON.stringify(r.toJSON());
    expect(text.indexOf('ask about Q2 budget')).toBeLessThan(text.indexOf('after'));
    expect(text).toContain('02:02');
    expect(text).toContain('added later');
  });

  test('Enter adds a trimmed note, Shift+Enter does not, empty is ignored', () => {
    const { r, props } = render();
    const input = r.root.findByProps({ 'aria-label': 'New note' });
    act(() => input.props.onChange({ target: { value: '  new idea ' } }));
    act(() => r.root.findByProps({ 'aria-label': 'New note' }).props.onKeyDown({ key: 'Enter', shiftKey: true, preventDefault() {} }));
    expect(props.onAdd).not.toHaveBeenCalled();
    act(() => r.root.findByProps({ 'aria-label': 'New note' }).props.onKeyDown({ key: 'Enter', shiftKey: false, preventDefault() {} }));
    expect(props.onAdd).toHaveBeenCalledWith('new idea');
    act(() => r.root.findByProps({ 'aria-label': 'New note' }).props.onKeyDown({ key: 'Enter', shiftKey: false, preventDefault() {} }));
    expect(props.onAdd).toHaveBeenCalledTimes(1);
  });

  test('delete and time click', () => {
    const onTimeClick = mock(() => {});
    const { r, props } = render({ onTimeClick });
    act(() => r.root.findByProps({ 'aria-label': 'Delete note: ask about Q2 budget' }).props.onClick());
    expect(props.onDelete).toHaveBeenCalledWith('n1');
    act(() => r.root.findByProps({ 'aria-label': 'Play from 02:02' }).props.onClick());
    expect(onTimeClick).toHaveBeenCalledWith(122);
  });

  test('collapsed shows only the expand button; disabled shows the hint', () => {
    const { r } = render({ collapsed: true });
    expect(r.root.findAllByProps({ 'aria-label': 'New note' }).length).toBe(0);
    r.root.findByProps({ 'aria-label': 'Show notes' });
    const d = render({ disabled: true, disabledHint: 'Notes start with a recording' });
    expect(d.r.root.findByProps({ 'aria-label': 'New note' }).props.disabled).toBe(true);
    expect(JSON.stringify(d.r.toJSON())).toContain('Notes start with a recording');
  });
});
```

- [ ] **Step 2: Verify failure** — `cd frontend && bun test tests/components/notes-pane.test.tsx`: FAIL.

- [ ] **Step 3: Implement `NotesPane.tsx`**

```tsx
'use client';
import { useState } from 'react';
import { ChevronLeft, ChevronRight, X } from 'lucide-react';
import { cleanNoteText, formatRecordingTime, NOTE_MAX_CHARS, sortNotes } from '@/lib/meeting-notes';
import type { MeetingNote } from '@/types';

interface NotesPaneProps {
  notes: MeetingNote[];
  onAdd: (text: string) => void;
  onUpdate: (id: string, text: string) => void;
  onDelete: (id: string) => void;
  onTimeClick?: (at: number) => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  disabled?: boolean;
  disabledHint?: string;
}

function NoteRow({ note, onUpdate, onDelete, onTimeClick }: { note: MeetingNote } & Pick<NotesPaneProps, 'onUpdate' | 'onDelete' | 'onTimeClick'>) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note.text);
  const commit = () => {
    setEditing(false);
    const cleaned = cleanNoteText(draft);
    if (cleaned.ok && cleaned.text !== note.text) onUpdate(note.id, cleaned.text);
    else setDraft(note.text);
  };
  const time = note.at === null ? 'added later' : formatRecordingTime(note.at);
  return (
    <li className="group flex gap-2 py-1.5">
      {note.at !== null && onTimeClick ? (
        <button type="button" className="text-xs text-blue-600 dark:text-blue-400 hover:underline mt-0.5 min-w-[44px] text-left"
          aria-label={`Play from ${time}`} onClick={() => onTimeClick(note.at!)}>{time}</button>
      ) : (
        <span className="text-xs text-muted-foreground mt-0.5 min-w-[44px]">{time}</span>
      )}
      {editing ? (
        <textarea
          autoFocus
          aria-label="Edit note"
          maxLength={NOTE_MAX_CHARS}
          className="flex-1 resize-none rounded border border-input bg-background px-2 py-1 text-sm"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); commit(); }
            else if (e.key === 'Escape') { setDraft(note.text); setEditing(false); }
          }}
        />
      ) : (
        <button type="button" className="flex-1 whitespace-pre-wrap break-words text-left text-sm text-foreground"
          aria-label={`Edit note: ${note.text}`} onClick={() => setEditing(true)}>{note.text}</button>
      )}
      <button type="button" className="opacity-0 group-hover:opacity-100 focus:opacity-100 text-muted-foreground hover:text-foreground"
        aria-label={`Delete note: ${note.text}`} onClick={() => onDelete(note.id)}>
        <X className="h-4 w-4" />
      </button>
    </li>
  );
}

export function NotesPane({ notes, onAdd, onUpdate, onDelete, onTimeClick, collapsed, onToggleCollapsed, disabled, disabledHint }: NotesPaneProps) {
  const [draft, setDraft] = useState('');

  if (collapsed) {
    return (
      <div className="flex w-8 flex-shrink-0 flex-col items-center border-l border-border bg-background pt-3">
        <button type="button" aria-label="Show notes" aria-expanded={false} onClick={onToggleCollapsed}
          className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
          <ChevronLeft className="h-4 w-4" />
        </button>
      </div>
    );
  }

  const submit = () => {
    const cleaned = cleanNoteText(draft);
    if (!cleaned.ok) return;
    onAdd(cleaned.text);
    setDraft('');
  };

  return (
    <section aria-label="My notes" className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <h3 className="text-sm font-medium">My notes ({notes.length})</h3>
        <button type="button" aria-label="Hide notes" aria-expanded={true} onClick={onToggleCollapsed}
          className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
          <ChevronRight className="h-4 w-4" />
        </button>
      </div>
      <ul className="flex-1 overflow-y-auto px-3">
        {sortNotes(notes).map((n) => (
          <NoteRow key={n.id} note={n} onUpdate={onUpdate} onDelete={onDelete} onTimeClick={onTimeClick} />
        ))}
      </ul>
      <div className="border-t border-border p-2 pb-24">
        <textarea
          aria-label="New note"
          disabled={disabled}
          maxLength={NOTE_MAX_CHARS}
          placeholder={disabled ? disabledHint : 'Type a note, press Enter'}
          className="w-full resize-none rounded-md border border-input bg-background px-3 py-2 text-sm min-h-[60px] disabled:opacity-60"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); }
          }}
        />
        {disabled && disabledHint && <p className="mt-1 text-xs text-muted-foreground">{disabledHint}</p>}
      </div>
    </section>
  );
}
```

(`pb-24` keeps the input above the fixed recording controls on the live page.)

- [ ] **Step 4: IndexedDB and storage service** — `indexedDBService.ts`: add `notes?: MeetingNote[]; marks?: Record<string, TranscriptMark>;` to its `MeetingMetadata` and:

```ts
  /** Read-modify-write in one transaction (spec §5.1). Fails silently like saveTranscript. */
  async patchMeetingMetadata(meetingId: string, patch: Partial<MeetingMetadata>): Promise<void> {
    try {
      if (!this.db) await this.init();
      const tx = this.db!.transaction(['meetings'], 'readwrite');
      const store = tx.objectStore('meetings');
      await new Promise<void>((resolve, reject) => {
        const get = store.get(meetingId);
        get.onsuccess = () => {
          const meeting = get.result as MeetingMetadata | undefined;
          if (!meeting) { resolve(); return; }
          const put = store.put({ ...meeting, ...patch, meetingId, lastUpdated: Date.now() });
          put.onsuccess = () => resolve();
          put.onerror = () => reject(put.error);
        };
        get.onerror = () => reject(get.error);
      });
    } catch (error) {
      console.warn('Failed to update meeting in IndexedDB:', error);
    }
  }
```

Use it in `TranscriptContext.tsx` for the two existing folder-path updates (recording-started and recording-stopped: replace the `getMeetingMetadata` + `saveMeetingMetadata` pairs with `patchMeetingMetadata(id, { folderPath })`), so they can no longer drop notes/marks written in between.

`storageService.saveMeeting` gains `notes: MeetingNote[] = []` and passes `notes` in the invoke args.

- [ ] **Step 5: `TranscriptContext.tsx`**
  - State: `const [notes, setNotes] = useState<MeetingNote[]>([]); const notesRef = useRef<MeetingNote[]>([]);` kept in sync with an effect like `transcriptsRef`.
  - `const [annotationsVersion, bumpAnnotations] = useReducer((x: number) => x + 1, 0);` and an effect on `[annotationsVersion]` that, when `annotationsVersion > 0`, takes `const id = currentMeetingId || sessionStorage.getItem('indexeddb_current_meeting_id')` and calls `indexedDBService.patchMeetingMetadata(id, { notes: notesRef.current, marks: collectMarks(transcriptsRef.current) })`. Declare this effect after the two ref-sync effects so the refs are current.
  - `addNote(text, at)`: `setNotes(prev => [...prev, newNote(text, at)]); bumpAnnotations();`
  - `updateNote(id, text)`: map; bump. `deleteNote(id)`: filter; bump.
  - `setTranscriptMark(transcriptId, mark)`: `setTranscripts(prev => prev.map(t => t.id === transcriptId ? { ...t, mark } : t)); bumpAnnotations();`
  - `clearTranscripts`: also `setNotes([])`.
  - Reload sync (the `syncFromBackend` effect): after building `formattedTranscripts`, read the recovery record and restore:

```ts
          const recoveryId = sessionStorage.getItem('indexeddb_current_meeting_id');
          const saved = recoveryId ? await indexedDBService.getMeetingMetadata(recoveryId) : null;
          setTranscripts(applyMarks(formattedTranscripts, saved?.marks));
          setNotes(saved?.notes ?? []);
```

  (replacing the plain `setTranscripts(formattedTranscripts)`).
  - Export the new members in `TranscriptContextType` and `value`.

- [ ] **Step 6: Save at stop and recovery**
  - `useRecordingStop.ts`: take `notesRef` from `useTranscripts()` and call `storageService.saveMeeting(savedMeetingName || meetingTitle || 'New Meeting', freshTranscripts, folderPath, [...notesRef.current])`.
  - `useTranscriptRecovery.ts`: in step 5 add `mark: metadata.marks?.[String((t as any).sequence_id)] ?? null,` to each formatted transcript (IndexedDB records carry the `TranscriptUpdate` field `sequence_id`, not `sequenceId`), and pass `metadata.notes ?? []` as the fourth `saveMeeting` argument.

- [ ] **Step 7: Live layout** — `app/page.tsx`: replace the `<TranscriptPanel ... />` inside `<div className="flex flex-1 overflow-hidden">` with:

```tsx
      <LiveTranscriptAndNotes isProcessingStop={isProcessingStop} isStopping={isStopping} showModal={showModal} />
```

and create `frontend/src/app/_components/LiveTranscriptAndNotes.tsx`:

```tsx
'use client';
import { useState } from 'react';
import { TranscriptPanel } from './TranscriptPanel';
import { NotesPane } from '@/components/MeetingNotes/NotesPane';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';
import { readFlag, writeFlag } from '@/lib/meeting-page-layout';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import type { ModalType } from '@/hooks/useModalState';

export function LiveTranscriptAndNotes(props: { isProcessingStop: boolean; isStopping: boolean; showModal: (name: ModalType, message?: string) => void }) {
  const { notes, addNote, updateNote, deleteNote } = useTranscripts();
  const { isRecording, activeDuration } = useRecordingState();
  const [collapsed, setCollapsed] = useState(() => readFlag('conclave.notesPaneCollapsed'));
  const split = useSplitRatio({ storageKey: 'conclave.live.transcriptRatio', defaultRatio: 0.7, min: 0.5, max: 0.8, label: 'transcript and notes' });
  const toggle = () => setCollapsed((c) => { writeFlag('conclave.notesPaneCollapsed', !c); return !c; });

  return (
    <div ref={split.containerRef} className="flex flex-1 min-w-0 overflow-hidden" {...split.containerHandlers}>
      <div className="flex min-w-0" style={collapsed ? { flex: 1 } : { width: `${split.ratio * 100}%`, flex: 'none' }}>
        <TranscriptPanel {...props} />
      </div>
      {!collapsed && <SplitHandle {...split.separatorProps} />}
      <NotesPane
        notes={notes}
        onAdd={(text) => addNote(text, activeDuration ?? 0)}
        onUpdate={updateNote}
        onDelete={deleteNote}
        collapsed={collapsed}
        onToggleCollapsed={toggle}
        disabled={!isRecording}
        disabledHint="Notes start with a recording"
      />
    </div>
  );
}
```

(`activeDuration` is seconds of active recording excluding pauses — `RecordingState::get_active_recording_duration`, `as_secs_f64` — the same clock as `audio_start_time`.)

`app/_components/TranscriptPanel.tsx`: take `setTranscriptMark` from `useTranscripts()`; include `mark: t.mark` in the `segments` map; pass `onMarkChange={setTranscriptMark}` to `VirtualizedTranscriptView`; change the content wrapper `w-2/3 max-w-[750px]` to `w-full max-w-[750px] px-6` (the pane is narrower now).

- [ ] **Step 8: Run** — `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`: PASS.

- [ ] **Step 9: Commit**

```bash
git add frontend/src/components/MeetingNotes/NotesPane.tsx frontend/src/app/_components/LiveTranscriptAndNotes.tsx frontend/src/app/_components/TranscriptPanel.tsx frontend/src/app/page.tsx frontend/src/contexts/TranscriptContext.tsx frontend/src/services/indexedDBService.ts frontend/src/services/storageService.ts frontend/src/hooks/useRecordingStop.ts frontend/src/hooks/useTranscriptRecovery.ts frontend/tests/components/notes-pane.test.tsx
git commit -m "feat(live): notes pane and line marks during a recording, kept through reload and recovery"
```

---

### Task 10: Saved meeting page — notes, marks, player, both layouts

**Files:**
- Create: `frontend/src/hooks/useMeetingNotes.ts`, `frontend/src/hooks/useMeetingAudio.ts`, `frontend/src/components/MeetingDetails/AudioPlayerBar.tsx`
- Modify: `frontend/src/lib/meeting-audio.ts` (`audioMissingMessage`), `frontend/src/hooks/usePaginatedTranscripts.ts` (`mark` in segments, `setTranscriptMark`), `frontend/src/components/MeetingDetails/MeetingDetailsSplitView.tsx`, `components/MeetingDetails/TranscriptPanel.tsx`, `components/MeetingDetails/TranscriptButtonGroup.tsx`, `app/meeting-details/page-content.tsx`, `app/meeting-details/page.tsx` (pass `setTranscriptMark` through)
- Test: `frontend/tests/lib/meeting-audio.test.ts` (extend), `frontend/tests/hooks/paginated-transcripts.test.tsx` (extend), `frontend/tests/hooks/meeting-notes.test.tsx` (new)

**Interfaces:**
- Consumes: commands from Tasks 2 and 5; Tasks 7–9 components.
- Produces:
  - `interface MeetingAudioInfo { path: string | null; keep_audio: boolean; audio_deleted_at: string | null }`
  - `AUDIO_DELETED_EVENT = 'conclave:audio-deleted'` (in `lib/meeting-audio.ts`; Task 11 dispatches it)
  - `audioMissingMessage(info: MeetingAudioInfo | null): string | null` — `null` when `path` is set or info not loaded; `"Audio deleted on <local date>"` when `audio_deleted_at` is set; else `"Audio file not found"`.
  - `useMeetingAudio(meetingId: string)` → `{ info, src: string | null, audioRef, playhead: number | null, duration: number | null, isPlaying, rate, setRate(r: 1 | 1.5 | 2), playFrom(seconds), togglePlay(), seek(seconds), setKeepAudio(keep: boolean), audioEvents }`. The `<audio>` element is rendered **once in `page-content.tsx`, outside every Tabs**, so switching tabs never unmounts it.
  - `useMeetingNotes(meetingId: string)` → `{ notes, loaded: boolean, addNote(text), updateNote(id, text), deleteNote(id) }` (after-meeting notes get `at: null`; edits are ignored until `loaded`; each change saves the whole list via `save_meeting_notes`; only the latest save's response is applied; on error the last confirmed list is restored and `toast.error` shown). `NotesPane` gets `disabled={!loaded}`.
  - `usePaginatedTranscripts` returns `setTranscriptMark(id: string, mark: TranscriptMark | null): Promise<void>` (invoke `set_transcript_mark`, then update local state; toast on error) and segments carry `mark`.
  - `MeetingDetailsSplitView` props: `transcript`, `notes: ReactNode`, `notesCount: number`, `summary`, `layout: MeetingPageLayout`, `activeTab`, `onTabChange`; `MeetingDetailsTab = 'transcript' | 'notes' | 'summary'`.
  - `TranscriptButtonGroup` prop `audioMissingReason?: string | null` — when set, the Re-transcribe button is disabled with `title={audioMissingReason}`.

- [ ] **Step 1: Failing tests** — extend `frontend/tests/lib/meeting-audio.test.ts`:

```ts
import { audioMissingMessage } from '../../src/lib/meeting-audio';

describe('audioMissingMessage', () => {
  const base = { path: null, keep_audio: false, audio_deleted_at: null };
  test('messages', () => {
    expect(audioMissingMessage(null)).toBeNull();
    expect(audioMissingMessage({ ...base, path: 'C:\\x\\audio.mp4' })).toBeNull();
    const deleted = '2026-11-20T10:00:00+00:00';
    expect(audioMissingMessage({ ...base, audio_deleted_at: deleted })).toBe(`Audio deleted on ${new Date(deleted).toLocaleDateString()}`);
    expect(audioMissingMessage(base)).toBe('Audio file not found');
  });
});
```

Create `tests/hooks/meeting-notes.test.tsx` (same invoke-mock pattern as `paginated-transcripts.test.tsx`, rendering the hook in a tiny test component with react-test-renderer):
- while `get_meeting_notes` is still pending, `loaded` is false and `addNote('x')` does **not** call `save_meeting_notes`;
- after it resolves with `[{ id: 'n1', at: 3, text: 'old' }]`, `addNote('new')` saves `[old, new]` (the stored note is never lost);
- two quick edits: the second `save_meeting_notes` call starts only after the first resolved (saves are sequential), and the UI ends with the second edit's list;
- save A succeeds, then save B fails: the UI shows A's list (not the list from before A);
- a rejected save restores the last confirmed list.

Extend `tests/hooks/paginated-transcripts.test.tsx` following its existing invoke-mock pattern: a test that loads one transcript `{ id: 't1', mark: 'action', ... }`, checks `segments[0].mark === 'action'`, calls `setTranscriptMark('t1', null)`, asserts the mocked invoke got `('set_transcript_mark', { transcriptId: 't1', mark: null })` and `segments[0].mark` is `null`; and a test where invoke rejects, asserting the mark stays `'action'`.

- [ ] **Step 2: Verify failure** — `cd frontend && bun test tests/lib/meeting-audio.test.ts tests/hooks/paginated-transcripts.test.tsx tests/hooks/meeting-notes.test.tsx`: FAIL.

- [ ] **Step 3: Implement the lib and hooks**

`lib/meeting-audio.ts` (append):

```ts
/** Dispatched on `window` after the old-audio review deleted audio, so an open player refetches. */
export const AUDIO_DELETED_EVENT = 'conclave:audio-deleted';

export interface MeetingAudioInfo {
  path: string | null;
  keep_audio: boolean;
  audio_deleted_at: string | null;
}

export function audioMissingMessage(info: MeetingAudioInfo | null): string | null {
  if (!info || info.path) return null;
  if (info.audio_deleted_at) return `Audio deleted on ${new Date(info.audio_deleted_at).toLocaleDateString()}`;
  return 'Audio file not found';
}
```

`usePaginatedTranscripts.ts`: in `convertTranscriptsToSegments` add `mark: t.mark ?? null`; add to the return type and object:

```ts
    const setTranscriptMark = useCallback(async (id: string, mark: TranscriptMark | null) => {
        try {
            await invoke('set_transcript_mark', { transcriptId: id, mark });
            setTranscripts(prev => prev.map(t => (t.id === id ? { ...t, mark } : t)));
        } catch (err) {
            console.error('Failed to save mark:', err);
            toast.error('Could not save the mark', { description: String(err) });
        }
    }, []);
```

(import `toast` from `sonner`).

`hooks/useMeetingNotes.ts`:

```ts
'use client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { newNote } from '@/lib/meeting-notes';
import type { MeetingNote } from '@/types';

export function useMeetingNotes(meetingId: string) {
  const [notes, setNotes] = useState<MeetingNote[]>([]);
  const [loaded, setLoaded] = useState(false);
  const notesRef = useRef<MeetingNote[]>([]);      // what the UI shows (optimistic)
  const confirmedRef = useRef<MeetingNote[]>([]);  // what the backend last stored
  const loadedRef = useRef(false);
  const saveSeq = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve()); // saves run one at a time

  useEffect(() => {
    let cancelled = false;
    loadedRef.current = false;
    setLoaded(false);
    invoke<MeetingNote[]>('get_meeting_notes', { meetingId })
      .then((n) => {
        if (cancelled) return;
        notesRef.current = n; confirmedRef.current = n; setNotes(n);
        loadedRef.current = true; setLoaded(true);
      })
      .catch((e) => {
        console.error('Failed to load notes:', e);
        toast.error('Could not load notes');
      });
    return () => { cancelled = true; };
  }, [meetingId]);

  const save = useCallback((next: MeetingNote[]) => {
    if (!loadedRef.current) return Promise.resolve(); // never overwrite notes we have not read yet
    const seq = ++saveSeq.current;
    notesRef.current = next;
    setNotes(next);
    // Chained: the backend sees saves in order, and every success updates confirmedRef.
    queue.current = queue.current.then(async () => {
      try {
        const stored = await invoke<MeetingNote[]>('save_meeting_notes', { meetingId, notes: next });
        confirmedRef.current = stored;
        if (seq === saveSeq.current) { notesRef.current = stored; setNotes(stored); }
      } catch (e) {
        if (seq === saveSeq.current) { notesRef.current = confirmedRef.current; setNotes(confirmedRef.current); }
        toast.error('Could not save notes', { description: String(e) });
      }
    });
    return queue.current;
  }, [meetingId]);

  return {
    notes,
    loaded,
    addNote: (text: string) => save([...notesRef.current, newNote(text, null)]),
    updateNote: (id: string, text: string) => save(notesRef.current.map((n) => (n.id === id ? { ...n, text } : n))),
    deleteNote: (id: string) => save(notesRef.current.filter((n) => n.id !== id)),
  };
}
```

`hooks/useMeetingAudio.ts`:

```ts
'use client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { AUDIO_DELETED_EVENT, type MeetingAudioInfo } from '@/lib/meeting-audio';

export type PlaybackRate = 1 | 1.5 | 2;

export function useMeetingAudio(meetingId: string) {
  const [info, setInfo] = useState<MeetingAudioInfo | null>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playhead, setPlayhead] = useState<number | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState<number | null>(null);
  const [rate, setRateState] = useState<PlaybackRate>(1);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      invoke<MeetingAudioInfo>('get_meeting_audio', { meetingId })
        .then((i) => { if (!cancelled) setInfo(i); })
        .catch((e) => console.error('Failed to locate meeting audio:', e));
    void load();
    // The old-audio review (mounted in the layout) may delete this meeting's audio while it is open.
    window.addEventListener(AUDIO_DELETED_EVENT, load);
    return () => { cancelled = true; window.removeEventListener(AUDIO_DELETED_EVENT, load); };
  }, [meetingId]);

  const src = info?.path ? convertFileSrc(info.path) : null;

  const playFrom = useCallback((seconds: number) => {
    const el = audioRef.current;
    if (!el) return;
    el.currentTime = seconds;
    void el.play().catch((e) => toast.error('Could not play the recording', { description: String(e) }));
  }, []);
  const togglePlay = useCallback(() => {
    const el = audioRef.current;
    if (!el) return;
    if (el.paused) void el.play().catch((e) => toast.error('Could not play the recording', { description: String(e) }));
    else el.pause();
  }, []);
  const seek = useCallback((seconds: number) => { if (audioRef.current) audioRef.current.currentTime = seconds; }, []);
  const setRate = useCallback((r: PlaybackRate) => { setRateState(r); if (audioRef.current) audioRef.current.playbackRate = r; }, []);
  const setKeepAudio = useCallback(async (keep: boolean) => {
    try {
      await invoke('set_keep_audio', { meetingId, keep });
      setInfo((i) => (i ? { ...i, keep_audio: keep } : i));
    } catch (e) {
      toast.error('Could not change Keep audio', { description: String(e) });
    }
  }, [meetingId]);

  return {
    info, src, audioRef, playhead, duration, isPlaying, rate, setRate, playFrom, togglePlay, seek, setKeepAudio,
    // spread onto the single <audio> element in page-content.tsx
    audioEvents: {
      onLoadedMetadata: (e: React.SyntheticEvent<HTMLAudioElement>) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : null),
      onDurationChange: (e: React.SyntheticEvent<HTMLAudioElement>) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : null),
      onTimeUpdate: (e: React.SyntheticEvent<HTMLAudioElement>) => setPlayhead(e.currentTarget.currentTime),
      onPlay: () => setIsPlaying(true),
      onPause: () => setIsPlaying(false),
      onEnded: () => setIsPlaying(false),
    },
  };
}
```

- [ ] **Step 4: `AudioPlayerBar.tsx`**

```tsx
'use client';
import { Pause, Pin, Play } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { audioMissingMessage } from '@/lib/meeting-audio';
import { formatRecordingTime } from '@/lib/meeting-notes';
import type { useMeetingAudio, PlaybackRate } from '@/hooks/useMeetingAudio';

const RATES: PlaybackRate[] = [1, 1.5, 2];

export function AudioPlayerBar({ audio }: { audio: ReturnType<typeof useMeetingAudio> }) {
  const missing = audioMissingMessage(audio.info);
  if (!audio.info) return null;
  const duration = audio.duration;
  return (
    <div className="flex items-center gap-3 border-t border-border bg-background px-3 py-2 text-sm">
      {missing ? (
        <span className="text-muted-foreground">{missing}</span>
      ) : (
        <>
          <button type="button" onClick={audio.togglePlay} aria-label={audio.isPlaying ? 'Pause' : 'Play'} className="rounded p-1 hover:bg-accent">
            {audio.isPlaying ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
          </button>
          <input
            type="range" min={0} max={duration ?? 0} step={0.1}
            value={audio.playhead ?? 0} onChange={(e) => audio.seek(Number(e.target.value))}
            aria-label="Playback position" className="flex-1 accent-blue-600"
          />
          <span className="tabular-nums text-muted-foreground">
            {formatRecordingTime(audio.playhead ?? 0)} / {formatRecordingTime(duration)}
          </span>
          <button type="button" className="rounded px-1.5 py-0.5 hover:bg-accent tabular-nums" aria-label={`Playback speed ${audio.rate}x`}
            onClick={() => audio.setRate(RATES[(RATES.indexOf(audio.rate) + 1) % RATES.length])}>
            {audio.rate}×
          </button>
        </>
      )}
      <label className="flex items-center gap-1.5 whitespace-nowrap" title="Never delete this meeting's audio automatically">
        <Pin className="h-3.5 w-3.5" aria-hidden="true" />
        Keep audio
        <Switch checked={audio.info.keep_audio} onCheckedChange={(v) => void audio.setKeepAudio(v)} aria-label="Keep audio" />
      </label>
    </div>
  );
}
```

- [ ] **Step 5: `MeetingDetailsSplitView.tsx`** — new props as in Interfaces. `TABS` gains `{ value: 'notes', label: 'My notes', icon: StickyNote }` between transcript and summary (narrow screens show three tabs in both layouts). Desktop:
  - `three-columns`: `const t = useSplitRatio({ storageKey: 'conclave.threeCol.transcriptRatio', defaultRatio: 0.3, min: 0.2, max: 0.5, label: 'transcript' }); const n = useSplitRatio({ storageKey: 'conclave.threeCol.notesRatio', defaultRatio: 0.2, min: 0.15, max: 0.35, offset: t.ratio, label: 'notes' });`. Share one container: set `ref={t.containerRef}` on the row and also assign `n.containerRef.current` to the same element (`ref={(el) => { t.containerRef.current = el; n.containerRef.current = el; }}`), and combine pointer handlers (`onPointerMove={(e) => { t.containerHandlers.onPointerMove(e); n.containerHandlers.onPointerMove(e); }}`, same for up/cancel). Order: transcript `TabsContent` (width `t.ratio`), `<SplitHandle {...t.separatorProps} />`, notes `TabsContent` (width `n.ratio`, collapsible via the NotesPane button: when the `conclave.notesPaneCollapsed` flag is set, render the notes content without a width and hide the second handle), `<SplitHandle {...n.separatorProps} />`, summary `TabsContent` (flex-1). Use inline `style={{ width: ... }}` on `md:` only by applying the width through CSS variables as the current file does (`--transcript-pane-width`, add `--notes-pane-width`, classes `md:w-[var(--notes-pane-width)] md:flex-none`).
  - `subtab`: today's two panes with the existing `meetily.meetingDetails.transcriptPaneRatio` hook; the left `TabsContent` renders an inner `Tabs` (`defaultValue="transcript"`) with triggers "Transcript" and `My notes (${notesCount})` and the two contents.
  - All three `TabsContent` keep `forceMount` and the `data-[state=inactive]:hidden md:data-[state=inactive]:flex` pattern so desktop shows every pane.

- [ ] **Step 6: Wire `page-content.tsx`**
  - `const layout = useMemo(readMeetingPageLayout, []);`, `const notes = useMeetingNotes(meeting.id);`, `const audio = useMeetingAudio(meeting.id);`, `const [notesCollapsed, setNotesCollapsed] = useState(() => readFlag('conclave.notesPaneCollapsed'));`
  - Render the one audio element next to `MeetingDetailsSplitView`, outside it: `{audio.src && <audio ref={audio.audioRef} src={audio.src} preload="metadata" className="hidden" {...audio.audioEvents} />}`. Clicking a note's time in the sub-tab layout (transcript tab unmounted) must still play — this is why it lives here.
  - Pass to `TranscriptPanel`: `onMarkChange={onSetTranscriptMark}` (new prop received from `page.tsx`, which takes `setTranscriptMark` from `usePaginatedTranscripts`), `onPlayFrom={audio.info?.path ? audio.playFrom : undefined}`, `playheadSeconds={audio.playhead}`, `footer={<AudioPlayerBar audio={audio} />}`, `audioMissingReason={audioMissingMessage(audio.info)}`.
  - `notes={<NotesPane disabled={!notes.loaded} disabledHint="Loading notes…" notes={notes.notes} onAdd={notes.addNote} onUpdate={notes.updateNote} onDelete={notes.deleteNote} onTimeClick={audio.info?.path ? audio.playFrom : undefined} collapsed={layout === 'three-columns' && notesCollapsed} onToggleCollapsed={...writeFlag + setState} />}`, `notesCount={notes.notes.length}`, `layout={layout}`.
  - `components/MeetingDetails/TranscriptPanel.tsx`: accept and forward `onMarkChange`, `onPlayFrom`, `playheadSeconds` to `VirtualizedTranscriptView`; render `footer` below the transcript list (above the custom-prompt box); include `mark: t.mark` in the non-paginated segment mapping; forward `audioMissingReason` to `TranscriptButtonGroup`.
  - `TranscriptButtonGroup.tsx`: Re-transcribe button `disabled={transcriptCount === 0 || !!audioMissingReason}` with `title={audioMissingReason ?? undefined}`.

- [ ] **Step 7: Run** — `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`: PASS.

- [ ] **Step 8: Commit**

```bash
git add frontend/src/hooks/useMeetingNotes.ts frontend/src/hooks/useMeetingAudio.ts frontend/src/hooks/usePaginatedTranscripts.ts frontend/src/lib/meeting-audio.ts frontend/src/components/MeetingDetails frontend/src/app/meeting-details frontend/tests/lib/meeting-audio.test.ts frontend/tests/hooks/paginated-transcripts.test.tsx frontend/tests/hooks/meeting-notes.test.tsx
git commit -m "feat(meeting): notes, marks and playback on the saved meeting page, in both layouts"
```

---

### Task 11: Old-audio review UI, reminder, settings and the meeting-delete warning

**Files:**
- Create: `frontend/src/lib/old-audio.ts`, `frontend/src/components/OldAudio/OldAudioReview.tsx`, `frontend/src/components/OldAudio/OldAudioReminder.tsx`
- Modify: `frontend/src/components/RecordingSettings.tsx`, `frontend/src/app/layout.tsx` (mount the reminder next to `ConditionalImportDialog`), `frontend/src/components/Sidebar/index.tsx`
- Test: `frontend/tests/lib/old-audio.test.ts`, `frontend/tests/components/old-audio-review.test.tsx`

**Interfaces:**
- Consumes: Task 4 commands `old_audio_list() -> OldAudio[]`, `delete_old_audio({ meetingIds }) -> DeleteResult`; `RecordingPreferences.old_audio_days`.
- Produces (`lib/old-audio.ts`):
  - `interface OldAudio { meeting_id: string; title: string; created_at: string; bytes: number; files: string[] }`, `interface DeleteResult { meetings: number; bytes: number; failed: string[] }`
  - `OLD_AUDIO_OPTIONS = [{ value: 'never', label: 'Never' }, { value: '7', label: '7 days' }, { value: '30', label: '30 days' }, { value: '90', label: '90 days' }]`
  - `daysToSelect(days: number | null | undefined): string` (`null` → `'never'`, `undefined` → `'30'`), `selectToDays(v: string): number | null`
  - `formatMegabytes(bytes: number): string` (`"0 MB"`, `"12 MB"`, `"1.5 GB"` at ≥ 1000 MB)
  - `toggleId(selected: string[], id: string): string[]`
  - `shouldRemind(lastShownDay: string | null, today: string, count: number): boolean` (`count > 0 && lastShownDay !== today`)
  - `todayKey(d = new Date()): string` (`YYYY-MM-DD`, local time)
  - `REMINDER_DAY_KEY = 'conclave.oldAudioReminderDay'`. The review dispatches `AUDIO_DELETED_EVENT` (from `lib/meeting-audio.ts`, Task 10) on `window` after a delete; `useMeetingAudio` refetches on it.
- `OldAudioReview({ open, onOpenChange })`: loads `old_audio_list` when opened; nothing selected by default.

Behaviour (spec §2.8–2.9, §8.1): the review lists each meeting with title, local date, size and its exact file paths (small monospace text), a checkbox per meeting, "Select all" / "Select none", and a footer line "Selected recordings go to the Windows Recycle Bin. Space is freed when you empty it. Transcripts, summaries and notes stay." The **Move N to Recycle Bin** button is disabled until at least one meeting is ticked. After the call: `toast.success("Moved N recordings (X) to the Recycle Bin")`; each `failed` entry → one `toast.warning`; the list reloads. Empty list → "No recordings are old enough to review." The reminder checks on mount and every 24 h and shows at most one toast per day: `"N recordings are older than D days (X)."` (D from `get_recording_preferences().old_audio_days`) with action **Review** that opens the dialog.

- [ ] **Step 1: Failing tests** — `frontend/tests/lib/old-audio.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import { daysToSelect, formatMegabytes, selectToDays, shouldRemind, todayKey, toggleId } from '../../src/lib/old-audio';

describe('old-audio helpers', () => {
  test('select mapping', () => {
    expect(daysToSelect(null)).toBe('never');
    expect(daysToSelect(undefined)).toBe('30');
    expect(daysToSelect(7)).toBe('7');
    expect(selectToDays('never')).toBeNull();
    expect(selectToDays('90')).toBe(90);
  });
  test('sizes', () => {
    expect(formatMegabytes(0)).toBe('0 MB');
    expect(formatMegabytes(12_400_000)).toBe('12 MB');
    expect(formatMegabytes(1_500_000_000)).toBe('1.5 GB');
  });
  test('selection toggles', () => {
    expect(toggleId([], 'a')).toEqual(['a']);
    expect(toggleId(['a', 'b'], 'a')).toEqual(['b']);
  });
  test('reminder at most once a day and only when there is something', () => {
    expect(shouldRemind(null, '2026-11-20', 3)).toBe(true);
    expect(shouldRemind('2026-11-20', '2026-11-20', 3)).toBe(false);
    expect(shouldRemind('2026-11-19', '2026-11-20', 0)).toBe(false);
    expect(todayKey(new Date(2026, 10, 5))).toBe('2026-11-05');
  });
});
```

`frontend/tests/components/old-audio-review.test.tsx` (mock `@tauri-apps/api/core` `invoke`, `sonner`, and `components/ui/dialog` as pass-through, restoring them in `afterAll` like `summary-panel-language.test.tsx`):
- with two items returned, nothing is checked and the delete button is disabled;
- ticking one enables the button; clicking it calls `invoke('delete_old_audio', { meetingIds: ['m1'] })` with **only** that id;
- "Select all" then delete sends both ids;
- the rendered text contains each item's file paths;
- a result with `failed: ['Standup: user declined']` shows a warning toast;
- a successful delete dispatches `AUDIO_DELETED_EVENT` on `window`.

- [ ] **Step 2: Verify failure** — `cd frontend && bun test tests/lib/old-audio.test.ts tests/components/old-audio-review.test.tsx`: FAIL.

- [ ] **Step 3: Implement `lib/old-audio.ts`**

```ts
export interface OldAudio { meeting_id: string; title: string; created_at: string; bytes: number; files: string[] }
export interface DeleteResult { meetings: number; bytes: number; failed: string[] }

export const REMINDER_DAY_KEY = 'conclave.oldAudioReminderDay';

export const OLD_AUDIO_OPTIONS = [
  { value: 'never', label: 'Never' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '90', label: '90 days' },
] as const;

export function daysToSelect(days: number | null | undefined): string {
  if (days === null) return 'never';
  if (days === undefined) return '30';
  return String(days);
}

export function selectToDays(v: string): number | null {
  return v === 'never' ? null : Number(v);
}

export function formatMegabytes(bytes: number): string {
  const mb = Math.round(bytes / 1_000_000);
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
}

export function toggleId(selected: string[], id: string): string[] {
  return selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
}

export function shouldRemind(lastShownDay: string | null, today: string, count: number): boolean {
  return count > 0 && lastShownDay !== today;
}

export function todayKey(d = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
```

- [ ] **Step 4: `OldAudioReview.tsx`**

```tsx
'use client';
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { formatMegabytes, toggleId, type DeleteResult, type OldAudio } from '@/lib/old-audio';
import { AUDIO_DELETED_EVENT } from '@/lib/meeting-audio';

export function OldAudioReview({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [items, setItems] = useState<OldAudio[] | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setSelected([]);
    try {
      setItems(await invoke<OldAudio[]>('old_audio_list'));
    } catch (e) {
      setItems([]);
      toast.error('Could not list old recordings', { description: String(e) });
    }
  }, []);

  useEffect(() => { if (open) void load(); }, [open, load]);

  const remove = async () => {
    setBusy(true);
    try {
      const r = await invoke<DeleteResult>('delete_old_audio', { meetingIds: selected });
      if (r.meetings > 0) toast.success(`Moved ${r.meetings} recordings (${formatMegabytes(r.bytes)}) to the Recycle Bin`);
      for (const f of r.failed) toast.warning('Not moved to the Recycle Bin', { description: f });
      window.dispatchEvent(new Event(AUDIO_DELETED_EVENT));
      await load();
    } catch (e) {
      toast.error('Could not delete the selected audio', { description: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const total = (items ?? []).filter((i) => selected.includes(i.meeting_id)).reduce((s, i) => s + i.bytes, 0);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Review old recordings</DialogTitle>
          <DialogDescription>
            Selected recordings go to the Windows Recycle Bin. Space is freed when you empty it. Transcripts, summaries and notes stay.
          </DialogDescription>
        </DialogHeader>
        {items === null ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : items.length === 0 ? (
          <p className="text-sm text-muted-foreground">No recordings are old enough to review.</p>
        ) : (
          <>
            <div className="flex gap-3 text-sm">
              <button type="button" className="text-blue-600 dark:text-blue-400 hover:underline" onClick={() => setSelected(items.map((i) => i.meeting_id))}>Select all</button>
              <button type="button" className="text-blue-600 dark:text-blue-400 hover:underline" onClick={() => setSelected([])}>Select none</button>
            </div>
            <ul className="max-h-[50vh] space-y-2 overflow-y-auto">
              {items.map((i) => (
                <li key={i.meeting_id} className="rounded border border-border p-2">
                  <label className="flex cursor-pointer items-start gap-2">
                    <input
                      type="checkbox"
                      className="mt-1"
                      checked={selected.includes(i.meeting_id)}
                      onChange={() => setSelected((s) => toggleId(s, i.meeting_id))}
                      aria-label={`Select ${i.title}`}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block text-sm font-medium">{i.title}</span>
                      <span className="block text-xs text-muted-foreground">
                        {new Date(i.created_at).toLocaleDateString()} · {formatMegabytes(i.bytes)}
                      </span>
                      {i.files.map((f) => (
                        <span key={f} className="block break-all font-mono text-[11px] text-muted-foreground">{f}</span>
                      ))}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          </>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Close</Button>
          <Button variant="destructive" disabled={busy || selected.length === 0} onClick={remove}>
            Move {selected.length} to Recycle Bin{selected.length ? ` (${formatMegabytes(total)})` : ''}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

(If `Button` has no `destructive` variant, use the variant the Sidebar delete dialog uses.)

- [ ] **Step 5: `OldAudioReminder.tsx`** and mount it in `app/layout.tsx` next to `<ConditionalImportDialog … />`:

```tsx
'use client';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { OldAudioReview } from './OldAudioReview';
import { formatMegabytes, REMINDER_DAY_KEY, shouldRemind, todayKey, type OldAudio } from '@/lib/old-audio';

const DAY_MS = 24 * 60 * 60 * 1000;

export function OldAudioReminder() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const check = async () => {
      let last: string | null = null;
      try { last = localStorage.getItem(REMINDER_DAY_KEY); } catch { /* reminder still works */ }
      let items: OldAudio[] = [];
      try { items = await invoke<OldAudio[]>('old_audio_list'); } catch { return; }
      const today = todayKey();
      if (!shouldRemind(last, today, items.length)) return;
      try { localStorage.setItem(REMINDER_DAY_KEY, today); } catch { /* optional */ }
      const bytes = items.reduce((s, i) => s + i.bytes, 0);
      let days: number | null = null;
      try { days = (await invoke<{ old_audio_days: number | null }>('get_recording_preferences')).old_audio_days; } catch { /* generic text */ }
      const age = days ? `older than ${days} days` : 'old';
      toast(`${items.length} recordings are ${age} (${formatMegabytes(bytes)}).`, {
        description: 'Nothing is deleted until you review and confirm.',
        action: { label: 'Review', onClick: () => setOpen(true) },
        duration: 15000,
      });
    };
    void check();
    const id = setInterval(check, DAY_MS);
    return () => clearInterval(id);
  }, []);

  return <OldAudioReview open={open} onOpenChange={setOpen} />;
}
```

- [ ] **Step 6: `RecordingSettings.tsx`**
  - Interface gains `old_audio_days: number | null;`; the initial state gets `old_audio_days: 30`.
  - Handler, same shape as `handleDetectionChange`:

```tsx
  const handleOldAudioChange = async (value: string) => {
    const previous = preferences.old_audio_days;
    const old_audio_days = selectToDays(value);
    setPreferences({ ...preferences, old_audio_days });
    try {
      await invoke('set_recording_preferences', { preferences: { ...preferences, old_audio_days } });
      toast.success(old_audio_days === null ? 'No reminders about old audio' : `Reminders for audio older than ${old_audio_days} days`);
    } catch (error) {
      setPreferences((prev) => ({ ...prev, old_audio_days: previous }));
      toast.error('Failed to save the setting', { description: error instanceof Error ? error.message : String(error) });
    }
  };
```

  - Markup after Meeting Detection, same structure; `const [reviewOpen, setReviewOpen] = useState(false);`:

```tsx
      <div className="p-4 border rounded-lg space-y-3">
        <div className="flex items-center justify-between gap-4">
          <div className="flex-1">
            <div className="font-medium">Remind me about audio older than</div>
            <div className="text-sm text-muted-foreground">Conclave never deletes audio by itself. Transcripts, summaries and notes are always kept. Pin a meeting with “Keep audio” to leave it out.</div>
          </div>
          <Select value={daysToSelect(preferences.old_audio_days)} onValueChange={handleOldAudioChange}>
            <SelectTrigger className="w-56" aria-label="Remind me about audio older than"><SelectValue /></SelectTrigger>
            <SelectContent>
              {OLD_AUDIO_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
        <button type="button" onClick={() => setReviewOpen(true)} disabled={preferences.old_audio_days === null || isRecording}
          className="text-sm text-blue-600 dark:text-blue-400 hover:underline disabled:opacity-50 disabled:no-underline">
          Review old audio…
        </button>
        <OldAudioReview open={reviewOpen} onOpenChange={setReviewOpen} />
      </div>
```

- [ ] **Step 7: Sidebar** — confirm text becomes `"Are you sure you want to delete this meeting? Its recording folder goes to the Recycle Bin."`. In `handleDelete`, read the result: `const res = await invoke<{ folder_warning?: string | null }>('api_delete_meeting', {...});` and after the success toast, `if (res?.folder_warning) toast.warning('The meeting was deleted, but its folder was kept', { description: res.folder_warning });`.

- [ ] **Step 8: Run** — `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`: PASS.

- [ ] **Step 9: Commit**

```bash
git add frontend/src/lib/old-audio.ts frontend/src/components/OldAudio frontend/src/components/RecordingSettings.tsx frontend/src/app/layout.tsx frontend/src/components/Sidebar/index.tsx frontend/tests/lib/old-audio.test.ts frontend/tests/components/old-audio-review.test.tsx
git commit -m "feat(settings): review old recordings and move the chosen ones to the Recycle Bin; daily reminder"
```

---


### Task 12: Release notes and full verification

**Files:**
- Modify: `.github/workflows/release.yml` (`releaseBody`)

- [ ] **Step 1: Release notes** — in `releaseBody`, replace the sentence that starts "New in 0.4.0:" with: "New in 0.5.0: take notes next to the live transcript and mark lines as important, action or follow-up — both go into the summary. Click a timestamp to hear that moment. Conclave reminds you about recordings older than 30 days (Settings → Recording) and lets you review them; only the ones you tick go to the Recycle Bin, and transcripts, summaries and notes stay. Deleting a meeting moves its recording folder to the Recycle Bin." Keep the rest of the body. Then `actionlint .github/workflows/release.yml`.

- [ ] **Step 2: Full verification**

```bash
cd frontend && pnpm install --frozen-lockfile && pnpm exec tsc --noEmit && bun test && pnpm run build
cd .. && nice -n 15 cargo test --workspace --exclude llama-helper -j 4
nice -n 15 cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins -j 4
bash scripts/check-no-phone-home.sh
actionlint
```

All pass.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "docs: 0.5.0 release notes for notes, marks, playback and the old-audio review"
```

- [ ] **Step 4: Manual check on Windows (v0.5.0 test build, by the user)** — play from a timestamp, seek, 1.5×/2×; mark lines during a real call and read the summary; reload the window during a recording and see notes/marks survive; kill the app during a recording and recover; both layouts and the splitters; notes input stays above the recording controls; delete a meeting and find its folder in the Recycle Bin; Settings → Review old audio: tick one meeting, move it, restore it from the Recycle Bin; check the daily reminder; a folder inside `conclave-recordings` with an extra file is kept on meeting delete.
