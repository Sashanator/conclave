# Conclave changelog

## v2.0.0 — 2026-11-17
- **Meetings table.** All meetings in one table with saved views, search across transcripts, and grouping by day or category.
- **Create a meeting first.** Name it, write prep notes, then press Record. Calendar events with 📞 in the title become meetings on their own.
- **One AI sidebar.** Ask and Copilot open beside the table or meeting (Ctrl J); Digests replace Briefs in the menu.
- **What's new** in the menu shows these notes; version, data folder and log file moved there from About.

## v1.2.0 — 2026-11-16
- API keys are stored in Windows Credential Manager instead of plain text; existing keys move over automatically on first start.
- A backup of your data is made before every version change, and the three most recent are kept.
- The app checks for new releases and installs one only after you click Update; every update is signature-checked.

## v1.1.0 — 2026-10-30
- Improve a summary's template right from the meeting page, with suggestions after you edit a summary.
- Templates keep an undo history; a failed save or an unreadable undo copy no longer blocks you.

## v1.0.4 — 2026-10-28
- Dictate has its own microphone setting and switches to another microphone if yours is unplugged.
- The sidebar shows the real app version and the rename and delete buttons show on hover again.

## v1.0.3 — 2026-10-28
- Maintenance release; no user-facing changes.

## v1.0.2 — 2026-10-28
- Continue a finished recording: the new part is merged into the earlier meeting and its summary is replaced.
- The meeting banner offers Continue in Ask and Auto mode.

## v1.0.1 — 2026-10-25
- Deleting a task has an undo window.
- Smaller fixes to tasks and the weekly digest.

## v1.0.0 — 2026-10-22
- Tasks page that collects action items from your meetings.
- Pre-meeting briefs and titles taken from your calendar.
- Friday weekly digest.

## v0.9.0 — 2026-10-21
- Live Copilot tab: ask questions during a call and get an alert banner when your name is mentioned.

## v0.8.0 — 2026-10-21
- Speaking Practice reports show pronunciation hints, recurring patterns and your filler-word trend.
- Reference pronunciation playback.

## v0.7.0 — 2026-10-19
- Dictate: hold a shortcut to dictate into any app, with a bottom pill and tray shortcuts.
- Dictionary spelling hints are used in summaries.

## v0.6.0 — 2026-10-18
- Speaker identification: transcripts get speaker labels you can rename and correct.
- Summaries use the speaker names.

## v0.5.0 — 2026-10-16
- Notes and line marks during a recording and on the saved meeting page, with audio playback.
- Review old recordings and move chosen ones to the Recycle Bin.

## v0.4.0 — 2026-10-15
- Summary templates: create, edit and duplicate them in Settings, with the seven Notion templates seeded.
- The template is picked by hand, by title keyword or by the model; notes are written in the meeting's language.
- Conclave colour theme.

## v0.3.0 — 2026-10-14
- Meeting detection for Zoom, Meet and Teams with a top banner; Off, Ask and Auto modes.
- Follows the call app's microphone.
- System, Light and Dark theme.

## v0.2.0 — 2026-10-13
- GPU transcription falls back to CPU when the CUDA runtime is missing.
- New Conclave icon; telemetry and the auto-updater removed.

## v0.1.0 — 2026-10-10
- First Conclave build: the Meetily fork renamed, with local Whisper transcription and cloud summaries using your own key.
