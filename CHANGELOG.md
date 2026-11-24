# Conclave changelog

## v2.5.3 — 2026-11-24
- **Live dictation keeps your language.** If the first phrase sounded a bit like another language, the whole dictation used to stick to it, so English could come out translated into Russian. Each unsure phrase now uses its own best guess; a clear phrase of 3 seconds or more still sets the language.
- **Wider Dictate page.** On wide windows, General and Modes sit side by side, with Dictionary across the full width below.

## v2.5.2 — 2026-11-24
- **Summaries survive a restart.** If Conclave closes or updates before a meeting's summary starts, opening the meeting later (within 24 hours) generates it. If no summary model is set up, you see a message instead of an empty page.
- **Claude prices update themselves.** Once a week Conclave fetches current Claude prices from OpenRouter's public model list, so cost figures stay right without manual edits. Prices you set yourself in Settings → Costs still win; the page shows when prices were last updated.

## v2.5.1 — 2026-11-24
- **Date column set to today.** In Settings → Notion, any date column other than the due date can be set to "Today (day you send)". Every task you send then gets that day, and the AI leaves the column alone.
- **Clearer backup warning.** If the pre-upgrade backup fails, the warning now names the file or folder it couldn't copy. Backups also copy nested template folders more reliably.

## v2.5.0 — 2026-11-23
- **See your words while you dictate.** The Dictate pill now shows what Conclave hears as you speak: final words in normal text, words still being guessed in grey. Pick Panel (three lines above the pill) or One line in Dictate → Live text.
- **Faster paste after long dictations.** Phrases are transcribed while you speak, so only the last one is left when you release the key.

## v2.4.0 — 2026-11-23
- **AI fills your Notion columns.** When you click Send, the AI reads the meeting's summary and transcript and fills every column of the Notion database (date, topic, status, notes and the rest), choosing only options that already exist. Columns you mapped or fixed in Settings keep your values.
- **An emoji for every row.** Give each Notion database an emoji in Settings → Notion; every task Conclave creates there gets it as its icon.
- **A switch per database.** "Let AI fill the other columns" can be turned off for any database.
- If the AI can't fill the columns (no cloud model, no key, or no answer within two minutes), the tasks are still sent and the Send bar says which databases weren't filled and why. A task is never sent twice.

## v2.3.0 — 2026-11-23
- **Resize every panel.** Drag the edge between the sidebar, lists, the meeting text, the side panel and the Ask/Copilot panel to the width you like; each width is remembered. Stats, Practice and What's new use the whole window.
- **Hide calendar events you didn't record.** A new switch in Settings → Appearance & app removes upcoming and missed calendar events from the Meetings table. Search still finds them.
- **Dictate page reworked.** Press the shortcut you want to record it (the Win key works too), pick a mode from chips and make any mode the default.
- **Dictation stays in your language.** Each dictation detects its language fresh, so English no longer comes out in Russian.
- **Ask about a meeting answers better.** Asking within one meeting or a few now always uses their summaries.
- Changing a meeting's category no longer pops up a regenerate offer; a note on the summary lets you regenerate when you want. Digests show full meeting names in the side panel, and What's new uses the full width.
- **Fix.** Conclave no longer fails with "state not managed" when the pre-upgrade backup asks a question at start.

## v2.2.0 — 2026-11-22
- **Tasks review queue.** The Tasks page lists the tasks found in your meetings, grouped by meeting. Keep or remove each one (K / X), fix its text or due date, and pick its Notion database (1–9).
- **Send to Notion.** One click (or Ctrl Enter) creates the kept tasks in your Notion databases: Job tasks, Personal tasks, German homework or any you add. A task is never sent twice; if Notion's answer is lost, it says "May already be in Notion" and waits for you.
- **Settings → Notion.** Paste your Notion token (kept in Windows Credential Manager), add databases by link, choose which columns get the task, owner, due date and meeting, and set a default database per meeting category.
- The meeting page uses the same Keep / Remove, the Tasks menu shows how many tasks wait for review, and deleting a meeting keeps the tasks you kept or sent.

## v2.1.1 — 2026-11-19
- **The summary no longer waits forever.** If speaker identification stops making progress for 10 minutes, it gives up and the automatic summary is made without speaker names.
- **Better logs for troubleshooting.** The log now names the app version, shows speaker identification, automatic summary and practice report steps, and drops the repeated settings and speech lines. Logs stay on your PC.
- The Summary row in Models & keys no longer sits in an extra card.

## v2.1.0 — 2026-11-19
- **Wide screens.** Pages use the full width with one left-aligned layout; the sidebar collapses from the bottom row or Ctrl B and remembers its state.
- **Back to where you were.** A breadcrumb, Esc or Alt+Left return from a meeting to the same view, groups, search and row.
- **Settings in one place.** Grouped settings with search; every AI model choice and API key lives in Models & keys.
- **Practice in German.** Each practice setup has a language; German sessions are transcribed and reviewed in German.
- Onboarding says what runs on your PC and asks for an AI key; the meeting-ended pop-up has an Open button.
- **Fixes.** View Meeting after stopping a recording no longer spins forever, the automatic summary runs again, and re-saving the same recording no longer creates a duplicate meeting.

## v2.0.0 — 2026-11-18
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
