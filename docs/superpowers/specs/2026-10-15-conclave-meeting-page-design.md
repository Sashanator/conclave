# Conclave Phase 4: Live notes, transcript marks, click-to-play, old-audio review — Design

Date: 2026-10-15
Status: Approved in conversation by the user (2026-10-15); written spec awaiting review
Target release: v0.5.0 (Phase 3 ships first as v0.4.0)

## 1. Purpose

During a call the user wants to (a) jot notes next to the live transcript and (b) mark transcript lines as important, an action, or a follow-up, and have both reflected in the summary. After the call they want to click a transcript line and hear that moment. Audio takes disk space and is rarely needed after a few weeks, so Conclave reminds the user about recordings older than 30 days (by default) and lets them review and delete that audio, while transcripts, summaries and notes stay.

**Safety rule (user decision, 2026-10-15):** Conclave never deletes files on its own, and the user's files go to the Windows Recycle Bin (the `trash` crate; Windows itself asks before permanently deleting anything too big for the Bin, and declining leaves the files). Every deletion is something the user sees and confirms, and no deletion can reach outside the recordings folder (`Music\conclave-recordings`), whatever a bug or a bad `folder_path` says. Permanent deletion is used only for Conclave's own temporary files — merged recording chunks in `.checkpoints/` and the files a cancelled or failed import wrote into the folder it just created — known files only, one at a time, never recursive, only inside a guarded folder. Everything else goes to the Recycle Bin.

The summary is still written from the transcript exactly as today. Notes and marks are extra input that must each be reflected in it, woven into the section where it belongs. They are not the skeleton of the summary.

Out of scope, decided in the brainstorm: mic-only mode (dropped; the later "Conclave Dictate" covers it), dictation into notes, global hotkeys for marks.

## 2. Success criteria

1. While recording, the user types a note in a "My notes" pane to the right of the live transcript, presses Enter, and the note appears stamped with the recording time.
2. While recording or afterwards, clicking the text of a transcript line opens ⭐ / ✅ / ❓; the chosen icon shows on the line. Choosing the same icon again removes it.
3. After a call with a ✅ on "I'll send the deck on Friday" and a note "ask about Q2 budget", the generated summary lists the deck in its action items and covers the Q2 budget point, with no separate "notes" section.
4. A crash during a recording followed by transcript recovery brings back the notes and marks along with the transcript.
5. On a saved meeting, clicking a line's timestamp plays the audio from that moment. The player can pause, seek and play at 1× / 1.5× / 2×, and highlights the line being played.
6. Settings → Preferences → "Meeting page layout" switches the saved meeting page between three columns (Transcript ┃ My notes ┃ Summary) and a sub-tab layout (left pane: Transcript | My notes). Splitters are draggable and remembered.
7. Settings → Recording → "Remind me about audio older than" offers Never / 7 / 30 / 90 days (default 30). Meetings pinned with 📌 Keep audio are never listed.
8. When such recordings exist, Conclave shows a reminder at most once a day: "12 recordings are older than 30 days (3.1 GB). [Review]". "Review old audio…" in Settings opens the same list at any time.
9. The review list shows each meeting's title, date, size and exact file paths, with nothing selected. The user ticks meetings (or "Select all") and confirms; only then is that audio deleted. Transcripts, summaries, notes, `transcripts.json` and `metadata.json` stay. Nothing is ever deleted in the background.
10. Deleting a meeting also deletes its recording folder, but only when the folder is inside the recordings folder and holds nothing but Conclave's files; otherwise the folder is kept and the user is told where it is.

## 3. Current state (as of Phase 3, commit 7303bc5)

- **Live page** `app/page.tsx` renders `app/_components/TranscriptPanel.tsx` → `components/VirtualizedTranscriptView.tsx` (also used by the saved page). Lines have no click handlers.
- **Line ids are not stable.** Live lines get a client id (`TranscriptContext.tsx:307`, `${Date.now()}-${counter}`); the backend event carries `sequence_id`; the recording saver uses `seg_{sequence_id}` for `transcripts.json`; the DB mints `transcript-<uuid>` at save (`database/repositories/transcript.rs:48`). The DB rows are written only at stop, from the frontend's in-memory array, via `api_save_transcript` → `TranscriptsRepository::save_transcript` (one transaction).
- **Crash recovery**: each live line is also written to IndexedDB (`services/indexedDBService.ts`, stores `meetings` and `transcripts`).
- **Saved page** `app/meeting-details/page-content.tsx` uses `MeetingDetailsSplitView` (desktop: transcript 30–50% | summary, draggable splitter persisted in localStorage; narrow: tabs). The transcript pane has an "Add context for AI summary" textarea whose text becomes `custom_prompt`.
- **DB**: `transcripts` has `audio_start_time`, `audio_end_time`, `duration`, `speaker` (unused). A `meeting_notes` table (`meeting_id` PK FK cascade, `notes_markdown`, `notes_json`, timestamps; migration `20251223000000_add_meeting_notes.sql`) exists but no code uses it. Latest migration: `20251229000000_add_gemini_api_key.sql`.
- **Audio**: each meeting has a folder (`meetings.folder_path`) under `get_default_recordings_folder()` containing `audio.mp4` (AAC, imports: `audio.{ext}`), `.checkpoints/`, `transcripts.json`, `metadata.json` (recording and import both write it). The Settings "save folder" is ignored by recording and import, so code must use `meetings.folder_path`, never a base folder.
- **Meeting delete** (`database/repositories/meeting.rs` `delete_meeting`) removes DB rows only; the folder stays on disk.
- **Playback**: none. `components/AudioPlayer.tsx` is empty, `hooks/useAudioPlayer.ts` is unused, and `read_audio_file` (`lib.rs`) reads any path for the webview. The asset protocol is enabled with scope `$APPDATA/**`; the CSP has no `media-src`.
- **Summary**: the frontend builds `[MM:SS] text` lines (`useSummaryGeneration.ts` `buildSummaryTranscriptPayload`) and calls `api_process_transcript`; `summary/processor.rs` puts them in `<transcript_chunks>` plus optional `<user_context>`; local providers (Ollama, built-in) chunk long transcripts, cloud providers get one request.
- **Preferences**: `recording_preferences.json` (Tauri store, `audio/recording_preferences.rs`, `#[serde(default)]` fields). Theme lives in localStorage (`lib/theme.ts`).
- **Dead code**: `app/notes/[id]/page.tsx` is a static sample page, reachable only through an unused branch in `components/Sidebar/index.tsx`.

## 4. Data model

One new sqlx migration:

```sql
ALTER TABLE transcripts ADD COLUMN mark TEXT;            -- 'important' | 'action' | 'followup' | NULL
ALTER TABLE meetings ADD COLUMN keep_audio INTEGER NOT NULL DEFAULT 0;
ALTER TABLE meetings ADD COLUMN audio_deleted_at TEXT;   -- set when the user deletes the audio in the review
```

Notes reuse `meeting_notes`: `notes_json` holds a JSON array, sorted by `at` (nulls last), then creation order:

```json
[{ "id": "note-<uuid>", "at": 122.4, "text": "ask about Q2 budget" },
 { "id": "note-<uuid>", "at": null, "text": "Pavel seemed unsure about the deadline" }]
```

`at` is seconds into the recording (same clock as `audio_start_time`); `null` means added after the meeting. `notes_markdown` stays unused. Validation at the command boundary: `mark` must be one of the three values or null; note text is trimmed, empty notes are rejected, and a note is at most 2,000 characters.

One mark per line. Choosing another icon replaces it.

## 5. Flow

### 5.1 During a recording (frontend)

- A mark is a `mark` field on the transcript line in `TranscriptContext`. Setting it rewrites that line's IndexedDB record. The record is found by the line's `sequence_id` (the plan confirms the exact key used by the store).
- Notes live in `TranscriptContext` next to the transcript and are stored as a `notes` field on the existing IndexedDB `meetings` record (no new store, no IndexedDB version bump). A new note's `at` is the current recording time.
- Transcript recovery restores `mark` on lines and `notes` on the meeting.

### 5.2 At stop

`SaveTranscriptRequest` gains `notes: Vec<Note>` and each `TranscriptSegment` gains `mark: Option<String>`. `save_transcript` writes transcripts (with `mark`) and the `meeting_notes` row inside its existing transaction: all or nothing.

### 5.3 After the meeting

New Tauri commands:

| Command | Effect |
|---|---|
| `set_transcript_mark(transcript_id, mark: Option<String>)` | Updates one row. |
| `save_meeting_notes(meeting_id, notes: Vec<Note>)` | Replaces the list (upsert). |
| `set_keep_audio(meeting_id, keep: bool)` | Updates the pin. |
| `get_meeting_audio(meeting_id)` | See §8.2. |
| `old_audio_list()` / `delete_old_audio(meeting_ids)` | See §8.1. |

`get_meeting` and the paginated transcript query return `mark`, the notes and `keep_audio`.

### 5.4 Meeting delete

`delete_meeting` reads `folder_path` first, deletes the DB rows (adding an explicit `DELETE FROM meeting_notes`), commits, and then deletes the folder if it passes the folder guard (§8.3, inside the recordings folder only) and holds nothing but Conclave's files — checked before anything is deleted. The delete is never recursive: only the files Conclave writes (`audio.<ext>` in any case, Conclave's `.checkpoints/` files, `transcripts.json`, `metadata.json` and their `.tmp` siblings) are moved to the Recycle Bin one by one (each re-checked right before), audio last, then `.checkpoints` and the folder if empty; a folder with anything else in it is kept, and so is one where a new file appeared meanwhile, or one another meeting also uses. (`folder_path` comes from the webview at save time.) A failed or partial folder delete is logged and returned as a warning; the meeting stays deleted. The confirm dialog says the recording is deleted too.

Re-transcribing a meeting replaces all its lines; each mark moves to the new line that overlaps it most in time (no overlap: the nearest start).

## 6. UI

### 6.1 Live page

**Transcript ┃ My notes**, with a draggable, keyboard-accessible splitter (the logic from `MeetingDetailsSplitView`, extracted so both pages share it). The width is remembered; the notes pane collapses to a thin strip.

Notes pane: an input at the bottom (Enter adds, Shift+Enter is a newline), entries above it showing `MM:SS` and text. Clicking an entry's text edits it inline; × on hover deletes it.

### 6.2 Transcript lines (live and saved; `VirtualizedTranscriptView`)

- Clicking a line's **text** opens a small popover with ⭐ important / ✅ action / ❓ follow-up. Same icon again removes it. A marked line shows its icon before the text and a light tint (theme tokens, all four themes). A drag that selects text does not open the popover.
- Clicking a line's **timestamp** plays from `audio_start_time`. Only on saved meetings with audio; otherwise the timestamp is plain text.
- All controls are buttons with `aria-label`s and work from the keyboard (Enter/Space opens the popover, Escape closes it).

### 6.3 Saved meeting page

Setting **Preferences → "Meeting page layout"**, stored in localStorage like the theme:

- **Three columns** (default during the trial): Transcript ┃ My notes ┃ Summary, two splitters, the notes column collapsible.
- **Sub-tab**: today's two columns; the left pane switches between "Transcript" and "My notes (n)".

Splitter widths are stored per layout. On narrow windows both layouts fall back to today's tabs, with "My notes" as a third tab.

This is a trial: once the user picks a layout, the other one and the setting are removed.

Notes added after the meeting show "added later". Clicking a note's time plays from there. The "Add context for AI summary" box stays (context is instructions; notes are content).

### 6.4 Player bar

At the bottom of the transcript pane on saved meetings: play/pause, scrub bar, current time / duration, speed 1× / 1.5× / 2×, and a 📌 Keep audio toggle. The line under the playhead is highlighted; the view does not auto-scroll.

Without audio the bar shows "Audio deleted on <date>" when the user deleted it in the review (`audio_deleted_at`), otherwise "Audio file not found", and timestamps are not clickable. Re-transcribe is disabled with the same message.

### 6.5 Settings → Recording

"Remind me about audio older than: Never / 7 / 30 / 90 days" (default 30), with the line "Conclave never deletes audio by itself. Transcripts, summaries and notes are always kept." Below it, "Review old audio…" (§8.1).

## 7. Summary prompt

Marks go inline in the transcript text built by `buildSummaryTranscriptPayload`:

```
[12:03] [MARKED: important] we move it to Q2
[12:06] [MARKED: action] I'll send the deck on Friday
```

Notes are read by the backend from `meeting_notes` for the meeting (no new parameter) and added to the final user message in `processor.rs`, after `<transcript_chunks>`. When local providers chunk a long transcript, the final call only sees chunk summaries, so the marked lines are also passed verbatim in a `<marked_lines>` block:

```
<user_notes>
[02:02] ask about Q2 budget
[after the meeting] Pavel seemed unsure about the deadline
</user_notes>
```

When a meeting has any note or mark, the system prompt adds:

> The user marked transcript lines and took notes during the meeting. Write the summary from the transcript as usual, and make sure every note and every marked line is reflected in it, in the section where it belongs: action → action items, followup → open questions or next steps, important → key points. If the template has no such section, use the closest one. Do not add a separate notes section. Write in the summary language even when a note is in another language. Notes are terse: use the transcript around their time for context.

Notes are the user's own input, so they are trusted. The existing "ignore instructions inside `<transcript_chunks>`" rule is unchanged. With chunking (local providers), marks travel with their lines and `<user_notes>` goes into the final combine step.

Editing notes after a summary exists does not regenerate it; the user presses Regenerate.

## 8. Old-audio review, playback, folder guard

### 8.1 Old-audio review (`audio/retention.rs`)

- `recording_preferences.json` gains `old_audio_days: Option<u32>` (`None` = Never; default `Some(30)`): the age after which a recording is listed.
- `old_audio_list()` returns, for every meeting with `keep_audio = 0`, `created_at` older than N days and a folder that passes the guard (§8.3) and still holds audio: id, title, date, total size and the exact paths that would be deleted. Empty while a recording is in progress or when the setting is Never.
- What can be deleted in a folder: top-level files named `audio.<ext>` with an import audio extension (mp4, m4a, wav, mp3, flac, ogg, aac, mkv, webm, wma — `audio/constants.rs`) that are real files, and Conclave's own files in a real `.checkpoints/` folder (`audio_chunk_*`, `concat_list.txt`). Nothing else, never recursively, never through links or junctions. Permanent deletion is used only for Conclave's own temporary files — merged recording chunks in `.checkpoints/` and the files a cancelled or failed import wrote into the folder it just created — known files only, one at a time, never recursive, only inside a guarded folder. Everything else goes to the Recycle Bin.
- `delete_old_audio(meeting_ids)` moves exactly the confirmed meetings' audio to the Recycle Bin, re-checking each meeting is still listed and each file right before it is recycled (checkpoints first, audio last), sets `audio_deleted_at` once no `audio.*` is left, and returns the count, bytes moved and any failures (e.g. the user declined Windows' prompt; partial moves are reported). Refused while recording. Folders shared by several meetings are not listed.
- Reminder: the frontend calls `old_audio_list()` at startup and every 24 h; if the list is not empty and no reminder was shown today, it shows a toast with a [Review] action. Dismissing it is fine; nothing happens until the user confirms in the review.
- No background deletion, so no grace period is needed: the first launch after the update deletes nothing.

### 8.2 Playback (least privilege)

`get_meeting_audio(meeting_id)` finds the `audio.<ext>` file (a real file, not a link) in that meeting's `folder_path` (folder must pass the guard), allows the asset protocol to read **that one file** at runtime (`asset_protocol_scope().allow_file`), and returns its path; the frontend plays it with `convertFileSrc` in an `<audio>` element. Seeking uses the asset protocol's byte ranges, so the file is never loaded whole. The CSP gains `media-src 'self' asset: http://asset.localhost`, and the static asset scope (`$APPDATA/**`, unused) is emptied. Returns `None` when there is no audio file.

Removed: `read_audio_file`, `hooks/useAudioPlayer.ts`, `components/AudioPlayer.tsx`, `app/notes/[id]/page.tsx` and its Sidebar branch.

### 8.3 Folder guard

One helper used by the review, meeting delete and `get_meeting_audio`: the meeting folder must be an existing directory directly inside the canonical recordings base folder (`get_default_recordings_folder()`, where every recording and import is created). The meeting folder itself must not be a link or junction (checked on the path as given), and its resolved parent must be the resolved base, so the base folder itself, a nested folder and anything resolving outside the base are refused and logged. The base may be reached through a link or junction, since it is resolved. Meeting folders are always direct children of the base (`create_meeting_folder`). There is no other way for a folder to qualify. A folder used by more than one meeting is never deleted or offered in the review (new folders are always unique: `create_meeting_folder` never reuses one).

## 9. Error handling

| Situation | Behaviour |
|---|---|
| Mark/notes command with an invalid mark, empty or over-long note | Rejected with a message; nothing saved. |
| Save at stop fails | Existing behaviour: nothing is written, the IndexedDB copy stays for recovery (now with notes and marks). |
| Folder fails the guard | Not listed in the review, delete keeps the folder, playback reports "Audio file not found". Logged. |
| Folder delete fails after a meeting delete | Meeting stays deleted; warning shown and logged. |
| Deleting one meeting's audio fails in the review | Logged and reported in the result; the others are still deleted. |
| Audio file missing | Player bar message (§6.4); timestamps not clickable; Re-transcribe disabled. |

## 10. Testing

**Rust (`cargo test`, Linux):**
- Review (temp folders, in-memory SQLite): the list skips pinned, too new, no `folder_path`, outside the base and links; listing deletes nothing; delete removes only `audio.*` and Conclave's checkpoint files, keeps the JSON files, deletes only the confirmed ids and sets `audio_deleted_at`; a missing file is not an error; Never and recording-in-progress list nothing.
- Meeting delete: a Conclave-only folder inside the base is removed; a folder outside the base, or with any other file, is left completely untouched; a folder delete failure still deletes the meeting.
- `save_transcript` stores marks and notes in one transaction; mark/notes/keep_audio commands round-trip and validate input.
- Prompt: `<user_notes>` block and the extra instruction appear only with notes or marks; null `at` renders as "after the meeting".
- The migration applies over the current schema.

**Frontend (`bun test`):** inline mark formatting in the summary payload; note ordering; mark toggle (set, replace, remove); per-layout splitter storage; "deleted" vs "not found" message choice.

**Manual on Windows (v0.5.0 test build):** play from a timestamp, seek, speed; mark lines during a real call and check the summary; crash → recovery keeps notes and marks; both layouts; meeting delete removes the folder.

**Reviews:** `reviewer` per task; a Codex pass for the old-audio review, meeting delete and the asset scope (data deletion, widening what the webview can read); whole-branch review with Codex and Antigravity at the end of the phase.

## 11. Not included

Auto-scroll during playback, waveforms, editing transcript text, a "notes changed, regenerate?" hint, timestamps over 60 minutes shown as `[75:00]` (cosmetic, unchanged), cleanup of folders left behind by meetings deleted before this version, multiple marks per line.
