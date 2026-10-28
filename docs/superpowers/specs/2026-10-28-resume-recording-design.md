# Resume recording (v1.0.2)

## Why

A Zoom call drops (network) or the host ends it and the user rejoins. Meeting detection auto-stops the recording 5 s after Zoom releases the mic (`crates/meeting-detector/src/classify.rs:241-252`), so the rest of the call lands in a second, unrelated meeting. The user wants the rest of the call in the **same meeting**: one transcript, one summary.

## What the user decided

- A **"Resume recording"** button on a saved meeting's page.
- Resuming continues **inside the same meeting**: one meeting row, one transcript, one summary.
- More than one audio file per meeting is acceptable if it is easier.
- After a resumed recording stops, the summary is **always regenerated over the whole transcript and replaces the old one**, even if the old one was hand-edited.
- **Continue offer (2-minute window):** if a recording starts less than 2 minutes after the previous one stopped, the user can pick "new meeting" or "continue the last one":
  - Ask mode: the detection banner shows **Record new** and **Continue "<last title>"**.
  - Auto mode: the recording starts at once as today; the banner shows **Continue "<last title>"**. Ignored → it stays a new meeting.
  - Manual Start Recording within the window: the live view shows the same Continue button.
  - After 2 minutes nothing is offered. The meeting-page Resume button stays for older meetings.
- Every recording runs as a normal recording; "continue" only sets the target meeting used when it is saved at stop. A failed or cancelled continue falls back to an ordinary new meeting, and the old meeting is never touched.

## My calls (told to the user)

- A crash during a resumed/continued recording recovers that part as its own new meeting; the original is never touched.
- Resuming obeys the auto-generate-summary setting (cost first): off → no new summary.
- Resume/Continue is hidden when the meeting's `audio.mp4` was deleted by retention.
- Imported-audio meetings are out of scope.

## Behaviour

- The button shows on the meeting details page when no recording is in progress and the meeting has a recording folder. Imported-audio meetings are out of scope (my call).
- Pressing it starts a normal recording with the same devices, language and live view as Start Recording, bound to that meeting. The live view shows the existing transcript followed by the new lines (my call; the builder may show only new lines if the existing panel makes that hard, but say so).
- New transcript lines are appended after the old ones. Their timestamps continue from the end of the previous part, so playback/seek and the order stay correct.
- Existing user notes and marks are kept. New notes are added to the same meeting.
- Stop works as it does today: from the record bar, the tray, or meeting-detector auto-stop. A meeting can be resumed any number of times.
- After stop: speaker analysis (if enabled) and the summary run over the whole meeting. The new summary replaces the old one.
- Meeting title, folder and id are unchanged.

## Constraints

- **Never lose existing data.** A failed or cancelled resume must leave the original transcript, audio and summary intact. If audio parts are merged, the original file is replaced only after the merged file is verified. If the app crashes mid-resume, the existing recovery path must still recover the new part without damaging the old.
- Audio: either merge into the single `audio.mp4` at stop (ffmpeg is already bundled and used for checkpoint merging) or keep `audio.mp4` + `audio_2.mp4`…; the builder picks. Every consumer of the audio (playback, speaker analysis, retention/deletion, the folder-open action) must handle the result.
- Usage/stats must not double-count the earlier part.
- No new network calls. Windows 10 only; follow the existing Tauri command/event patterns.

## Non-goals

- Silently folding a rejoin into the old meeting with no click (the Continue offer covers it).
- Resuming imported-audio meetings.
- Changing what Start Recording does.

## Left to the builder

- Where the meeting id is threaded through (`start_recording*` in `audio/recording_commands.rs`, `TranscriptContext.tsx:138`, the save path `useRecordingStop.ts` → `storageService.ts` → `api.rs:978`, `database/repositories/transcript.rs:30`).
- Merge vs. multiple files, and the timestamp offset source (previous audio duration vs. last transcript end).
- How the "regenerating summary" state is shown.
