# Conclave: redesign (meetings table, meeting-first pages, one AI sidebar)

Date: 2026-11-16. Status: draft for the user's review. Release: the next minor after safe updates (v1.3.0 if safe updates ships as v1.2.0).
Approved mockups (open in a browser; each has a theme switch, Conclave is the reference): `docs/superpowers/specs/2026-11-16-redesign-mockups/`
- `meetings-table.html`: the meetings table, views, toolbar, grouping, density.
- `meeting-page.html`: the meeting page in its three states (New, Recording, Finished) with the tasks review.
- `part2-screens.html`: screens 1 (calendar rows in This week), 2 (Prep tab), 5 (pop-up states, **behaviour only**, see Non-goals), 6 (the Audio line, now on the meeting page; there is no record screen) and 7 (What's new). Screens 3 (Settings AI) and 4 (onboarding) belong to the later cleanup project.
- `states-search.html`: search results, special row states, empty first run, Ask mid-answer.
- `recording-copilot.html`: the meeting page while recording, with the Copilot sidebar open and closed.

Where a mockup and this text disagree, this text wins (several decisions came after the mockups; they are listed in §6 where they apply). Known differences: the nav's record button is "Record new meeting" (not "Record now"); the nav item is "Digests" (not "Briefs & digests"); the Date column shows start–end times; there is no "Record as a new meeting instead" link (the record screen is gone; to record separately, use Record new meeting); the category emoji sits before the name in the table and above the title on the meeting page.

## 1. Why

Conclave grew screen by screen out of Meetily: a record screen, a sidebar list of meetings, a meeting page with two layouts, separate Briefs and Prep pages, Ask on its own page, Copilot in a block on the record screen. The user works meeting by meeting: prepares a call, records it, reads the summary, reviews the tasks. The app makes them jump between screens for each step, and the history is a flat list they cannot filter, group or review.

The redesign makes the **meeting** the unit of work and the **meetings table** the home screen:
- a Notion-style database table of all meetings with saved views, search across transcripts, filter, sort and group;
- a meeting exists before its call (created by hand or from a 📞 calendar event), holds the prep, and is where recording happens;
- one right-hand AI sidebar (Ctrl J): Ask over the table, Copilot during a call, Ask about one meeting after it.

## 2. Goals

1. The meetings table at `/` replaces the record screen as the home screen, with the views, columns, toolbar and keyboard below.
2. A meeting can be created empty, named, given a category and prep, and recorded later from its own page. Recording always goes into a meeting page.
3. Calendar events whose title contains the 📞 marker become meetings by themselves and follow the calendar (move, rename, cancel) without ever deleting the user's content.
4. The pop-up's Record goes into the right meeting (a 📞 meeting now or soon, else a new one).
5. Copilot moves into the shared right sidebar; Ask moves from its own page into the same sidebar.
6. Briefs move into the meeting's Prep tab; the Briefs page becomes Digests; the `/prep` page goes away.
7. "What's new in vX" replaces the two About entries and shows the release notes of every version.
8. The update runs on existing data with no AI calls: categories, end times and one-line summaries for old meetings come from data already stored; every existing meeting starts as Reviewed.

## 3. Non-goals (this project)

These are the later **cleanup** project (`.superpowers/brainstorm/ux-audit.md` lists them):
- Settings "AI models & keys" (one tab for all model pickers), the grouped settings sub-nav.
- Onboarding (audio check, AI key step).
- The pop-up's new two-line look (`part2-screens.html` screen 5 visuals). This project changes only where its Record goes.
- The Tasks page, contrast fixes outside the screens built here, and the other audit items.

Also not in scope: Obsidian export, Tasks → Notion (the "Done reviewing" button keeps its current meaning, see §6.5), full-text index for search, a global "start recording" hotkey, new themes.

## 4. Anti-goals (failure even if it "works")

- **Losing user content.** Calendar sync, migration or a failed save must never delete or overwrite a recording, transcript, note, prep, brief or summary. A cancelled or moved event never takes the user's content with it. Only the user's explicit "Delete meeting" removes prep and notes; "Delete recording" never does (§6.4).
- **A recording with nowhere to go.** If the meeting it targets is gone or unwritable at save time, the recording is saved as a new meeting (the current behaviour), never dropped.
- **New AI cost.** No extra LLM call per meeting: the one-line summary rides on the existing task-extraction call; old meetings are backfilled without AI. The only new LLM call is a one-time emoji suggestion for a custom template (§6.3).
- **A slower table.** The table must stay instant with the user's full history (hundreds of meetings): no per-row backend calls, no transcript loading for the list.
- **Breaking the invariants** in `AGENTS.md`: no telemetry, no new outbound hosts (What's new reads a bundled file), app identity unchanged.

## 5. Constraints (facts from the code)

- **A meeting row is created only at save.** `TranscriptsRepository::save_transcript` (`frontend/src-tauri/src/database/repositories/transcript.rs:84-110`) inserts the `meetings` row with its segments, reached from `api_save_transcript` (`api/api.rs:978`) at stop. Recording start (`audio/recording_commands.rs:562`, `lib.rs:200`) takes only a name.
- **Migrations** are sqlx-embedded (`database/manager.rs:38`), files `frontend/src-tauri/migrations/YYYYMMDDHHMMSS_name.sql`.
- **`meetings` today**: `id, title, created_at, updated_at, folder_path, title_source ('user' default), calendar_uid, calendar_start, keep_audio, audio_deleted_at`. No template, reviewed, gist or end-time columns.
- **Template used** lives only in `summary_processes.result` JSON (`summary/service.rs:104-131`) and the meeting folder's `metadata.json`. Templates are JSON files (`summary/templates/types.rs:6-38`) with no emoji field. Choice order: manual pick → stored choice → prep template → title keywords → LLM auto-pick (`summary/template_choice.rs:24-65`).
- **Calendar**: iCal URL, fetched by the knowledge scheduler (60 s tick, refresh every 30 min, window now−1 day to now+14 days; `knowledge/scheduler.rs`, `knowledge/calendar.rs:578-636`), events `uid, title, start, end, recurring`, cached in `calendar-cache.json`. `calendar_title_now` (`calendar.rs:823-839`) matches an event in progress or starting within 10 min.
- **Prep** (`meeting_preps`) and **briefs** are keyed by `(event_uid, event_start)` with no `meeting_id`; a meeting links to them through `calendar_uid/calendar_start` (`prep/upcoming.rs:86`). Briefs are made when an event is within 60 min (`scheduler.rs:90`).
- **Tasks** already carry `owner` (`me` | `unassigned`) and `status` (`open|done|dismissed`); extraction is one LLM call per finished summary returning JSON (`knowledge/extract.rs:99-157`).
- **Search** exists: `api_search_transcripts` → `search_all` (`transcript.rs:231-395`), `LIKE` over titles, summary markdown and transcript text, 50 meetings × 3 hits.
- **Ask**: `ask_meetings` / `ask_cancel` (`knowledge/ask.rs:254-293`, command at `:270`), plan → local retrieval → answer with citations; no scope parameter today.
- **Copilot**: `components/Copilot/CopilotPane.tsx` + `contexts/CopilotContext.tsx`, mounted in the record screen's notes/copilot block (`app/_components/LiveTranscriptAndNotes.tsx`).
- **Continue the same call** (`audio/continuation.rs`, `api.rs:1027-1040`) appends a recording to the last meeting saved within 120 s. It stays as it is.
- **Frontend**: Next.js 14 static export, shadcn (new-york) + Tailwind tokens in `app/globals.css` (`:root`, `.dark`, `.theme-conclave`), Source Sans 3, lucide icons. `@tanstack/react-virtual`, `cmdk`, `date-fns` are installed; `@tanstack/react-table` is not (approved to add). Sidebar is `components/Sidebar/index.tsx` (two render paths, collapsed and expanded); meetings list state is `Sidebar/SidebarProvider.tsx`.
- **CHANGELOG.md** (`## vX.Y.Z — YYYY-MM-DD`) is read only at build time today.
- Linux devcontainer verifies Rust tests and the frontend; Windows-only code is verified in CI (see `AGENTS.md`).

## 6. Design

### 6.1 Data model (one migration plus a one-time Rust backfill)

New `meetings` columns (all nullable or defaulted, so old rows stay valid):

| Column | Meaning |
|---|---|
| `template_id TEXT` | The meeting's category = its summary template. NULL = "No template". |
| `reviewed INTEGER NOT NULL DEFAULT 0` | The Reviewed checkbox. |
| `gist TEXT` | One-line summary shown in the table. |
| `started_at TEXT`, `ended_at TEXT` | Real start and end of the recording (UTC, same format as `created_at`). |
| `calendar_end TEXT` | Event end, for 📞 meetings. |
| `calendar_original_start TEXT` | The occurrence's original start, part of the calendar matching key (§6.6). |
| `calendar_cancelled_at TEXT` | Set when the event vanished from the calendar but the meeting holds user content. |

`meeting_preps` is **rebuilt** in the migration (SQLite cannot drop NOT NULL): `event_uid`, `event_start`, `event_title` become nullable, a `meeting_id` column is added with a UNIQUE constraint, and `UNIQUE(event_uid, event_start)` stays for rows that have them. Every existing prep row keeps its data and gets `meeting_id` filled (§6.6 "Existing preps"). A hand-made meeting's prep has only `meeting_id`. New table `dismissed_occurrences (calendar_uid, original_start, dismissed_at)` (§6.4). New table `meeting_views (id, name, position, builtin, config_json)` stores views (§6.3).

Backfill, run once after the migration, idempotent, no AI:
- `reviewed = 1` for every existing meeting (user decision).
- `template_id` from `summary_processes.result` JSON when present.
- `ended_at = created_at` (today `created_at` is the save time, i.e. when recording stopped; `transcript.rs:84-110` binds now); `started_at = created_at` minus the last transcript segment's audio end time, else `started_at = created_at`.
- `gist` = the first sentence of the summary's first section (trimmed to ~160 chars), else NULL.

**Which date a meeting has.** From now on a meeting's date is `COALESCE(started_at, calendar_start, created_at)`, because a row can now exist days before its call. Every reader that uses `created_at` as the meeting date switches to that expression: the list order (`meeting.rs:43`), digests (`digest.rs:490`), audio retention (`retention.rs:61,85`; it must count from `ended_at`, never from row creation), briefs' previous meeting (`briefs.rs:86`), tasks, stats and Ask retrieval. The builder greps every `created_at` read on `meetings` and lists the decisions in the plan.

A meeting's **state** is derived, not stored: Recording (it is the live recording target), Finished (has transcripts), Cancelled in calendar (`calendar_cancelled_at` set), Not recorded (no transcripts and its start time + 1 day has passed), New (no transcripts otherwise; shown as "Not recorded yet").

### 6.2 Meeting-first recording (approach A)

- **New meeting** (toolbar, Ctrl N, or "+ New meeting" row) inserts a row now: title "Untitled · Mon 14:02" (local weekday and time), `title_source='default'`, and opens its page in the New state.
- **Record new meeting** (sidebar button) inserts the same kind of row and opens its page already recording.
- **Start recording** on a meeting page records into that meeting. The meeting id travels with the recording from start to save: `start_recording…` accepts an optional `meeting_id`; `started_at` is written when recording starts (so the calendar reconcile leaves the row alone from then on, §6.6); `api_save_transcript` writes the segments into that existing row and sets `ended_at`; with no id it creates a row as today.
- **Crash recovery** (`TranscriptRecovery`, today mounted only in `app/page.tsx`, the record screen being removed) moves to the new `/` screen. The recording's target `meeting_id` is stored in the recovery metadata (IndexedDB) at start, and recovery saves into that row (`useTranscriptRecovery.ts:182` today always creates a new meeting), falling back to a new meeting if the row is gone.
- **Every recording start is routed the same way**: the sidebar button, the pop-up (§6.7), the tray menu (`tray.rs:84`) and the detector's auto-start (`meeting_detector.rs:129`) all go through one routing function that picks the target meeting.
- If the target row no longer exists at save, save as a new meeting and tell the user (toast), as the continue path already does on failure.
- A finished meeting has no Start recording button (as in the approved mockup). A dropped call is still joined through the pop-up's existing "Continue".
- The category chosen on the page before recording is the summary's manual template pick (first in the choice order).
- Default titles change from `Meeting DD_MM_YY_HH_MM_SS` to the "Untitled · Mon 14:02" form.

### 6.3 Meetings table (`/`)

Layout, columns, toolbar and look as in `meetings-table.html`, with these later decisions:
- **Views**: All meetings (group by Category), This week (Mon–Sun, group by Day), Not reviewed, and "+ View" for the user's own. **Each view remembers its own sort, filter, grouping, column widths, hidden columns and density** across navigation and restarts, saved in `meeting_views` (so backups keep them). Built-in views can be changed but not deleted.
- **Category emoji**: templates gain an optional `emoji` field (template JSON). Built-in templates ship with one; the template assistant suggests one when it creates or improves a template; the user can edit it in Settings → Templates. A custom template without one gets a suggestion once, from a short LLM call when it is next saved (no backfill sweep). "No template" has no emoji and is its own group when grouping by Category.
- **Columns**: Name (category emoji before it; untitled rows read "Untitled · Mon 14:02"), Date (weekday, date, **start–end time**: "Mon 16 Nov · 10:00–10:45"; a live meeting "10:49–now"; a 📞 meeting its calendar times), Category (editable chip; hidden when grouping by category), Summary (gist, or the row state), Reviewed (checkbox; empty for meetings without a summary). The date column shows in every view, also when grouped by day.
- **Row states** as in `states-search.html` screen 2: Now (live, pinned on top), Not recorded yet (with "Calendar" badge and "Prep: N points"), Not recorded (with "Open prep"), Cancelled in calendar (with a Delete button), finished.
- **Toolbar**: New meeting (Ctrl N), one search box (`/`), Filter (category, reviewed, state, from calendar or not, date range), Sort, Group (none, day, category), Ask (Ctrl J).
- **Search** (`states-search.html` screen 1): searches names, summaries and transcripts of **all** meetings whatever the view; grouping off and newest first while searching; up to 3 snippets per meeting labelled Name / Summary / Transcript mm:ss, "N more in this meeting"; a transcript snippet opens the meeting at that moment; Esc clears; no matches shows "Nothing mentions "…". Try fewer words or another spelling." with Clear search. Uses the existing `search_all`, extended to return segment times if it does not.
- **Empty first run** as in `states-search.html` screen 3.
- **Keyboard**: arrows move rows, Enter opens, Space ticks Reviewed, `/` search, Ctrl N new meeting, Ctrl J sidebar, Esc clears search or closes the sidebar.
- Built on `@tanstack/react-table` (headless; the look is the mockup's), rows virtualised with the installed `@tanstack/react-virtual` when long. Data comes from one list command that returns every column above for all meetings in one call.

### 6.4 Meeting page (`/meeting-details?id=`)

As in `meeting-page.html` (three states) and `recording-copilot.html`, with:
- Title field first, category emoji above it (Notion page-icon style), category chip, date line with start–end time.
- **New**: prep notes, Brief (for 📞 meetings, see §6.6), the **Audio line** (microphone, system audio, language, silent-system-audio warning; `part2-screens.html` screen 6 content without the separate screen) and the big Start recording.
- **Recording**: Recording mm:ss, Pause, Stop and summarize, the Audio line, live transcript and My notes. With the sidebar open, My notes becomes a tab next to Transcript; closed, the two columns return.
- **Finished**: opens on Summary with the "Tasks from this meeting" panel; tabs Summary, Transcript, My notes, Prep. Tasks with `owner='unassigned'` are shown as removed (restorable); K keeps, X removes; "Keep all mine"; **Done reviewing** ticks Reviewed.
- **Deleting** (user decision) is two separate actions:
  - **Delete recording** (Finished meetings): removes the transcript, audio, summary, extracted tasks that were not kept, and speaker data; keeps the meeting row, its title, category, prep and notes. The meeting returns to New (or Not recorded if its day has passed). This replaces the old "deleting a recording never deletes its prep" rule with the same guarantee.
  - **Delete meeting** (meeting page menu and the table's row menu; also the Delete button on Cancelled in calendar rows): a confirmation dialog names what goes (recording, prep, notes, brief); then everything tied to the meeting is removed, prep included. A deleted 📞 occurrence is remembered by `(calendar_uid, original start)` in a small `dismissed_occurrences` table so the reconcile does not bring it back; entries older than 30 days are pruned. Kept tasks (`owner='me'` or edited) survive with `meeting_id` cleared, like manual tasks; the current delete removes all of a meeting's tasks (`meeting.rs:355`), so this is a deliberate change with its own test. Delete recording follows the same task rule.
- Changing the category of a finished meeting relabels it and offers "Regenerate summary with <template>" (no automatic re-summary; cost).
- The old two layouts, their setting and the notes-pane collapse flag are removed.

### 6.5 The AI sidebar (Ctrl J)

One right-hand panel (~360 px) with one look (`states-search.html` screen 4, `recording-copilot.html`), whose content follows the screen:
- **Meetings table → Ask your meetings.** Scope chip follows the current view ("This week · 6 meetings") with "All meetings" in its menu. `ask_meetings` gains an optional list of meeting ids that limits retrieval. Citations are chips that open the meeting.
- **Meeting page while recording → Copilot · this call.** Today's Copilot (quick actions, free question, answer cards with Add to notes and Copy, alert words) moves here from the record screen's block. Added: timestamp chips in answers that scroll the transcript, the "Alerts: … · edit" line linking to Settings → Copilot, and alert cards in the panel (the banner stays).
- **Finished meeting page → Ask about this meeting** (scope = that one meeting).
- Open/closed is remembered per screen kind. The `/ask` page is removed.

### 6.6 Calendar 📞 meetings (approach A: the backend scheduler)

- **Marker**: Settings → Calendar gets a "My calls marker" field (default 📞). Only events whose title contains it become meetings. The marker is stripped from the shown and stored title.
- **Matching key.** A 📞 row is identified by `calendar_uid` plus `calendar_original_start` (new column, §6.1): the occurrence's original start (iCal `RECURRENCE-ID` for a moved occurrence of a series, else its start at the time the row was made). One-off events (not recurring) match on `calendar_uid` alone. `calendar_start`/`calendar_end` hold the current times. `Occurrence` (`calendar.rs:45-53`) gains the original start; the cache format changes with it (an old cache is refetched).
- **When it runs.** After each calendar refresh, only if the fetch succeeded, a calendar is connected, and the expansion was **complete**: `calendar::expand` must report when it hit its time or occurrence limits or skipped a series (`calendar.rs:205,211` only log today). An incomplete expansion allows inserts and updates but no cancel or delete. A failed fetch, disconnecting the calendar, or changing the marker never cancels or deletes anything. The marker last reconciled is stored with the cache; when it changes, rows made under the old marker are left alone (they age out through the "past and untouched" rule) and only new matches are inserted.
- **The window.** Events in progress or starting within the next **7 days**. A row counts as "gone from the feed" only if its `calendar_start` lies inside the window that was fetched and expanded; rows outside it are never cancelled.
- **Rules** (a pure function: events + rows + live target → actions, with table-driven unit tests):
  - new 📞 event → insert a row (`title_source='calendar'`, `calendar_start/end/original_start` set);
  - moved (key matches, times differ) → update `calendar_start/end`; the row's prep (its `meeting_preps.event_start`, unique key included) and notes move with it;
  - renamed → update the title unless `title_source='user'`;
  - gone from the feed: untouched row → delete; otherwise set `calendar_cancelled_at`;
  - an event that reappears clears `calendar_cancelled_at`;
  - past and untouched after its day → delete; with content it stays as Not recorded.
  - **Never touched by the reconcile:** the live recording target and any row with `started_at` set (recording has started or finished): no move, rename, cancel or delete. `started_at` is written when recording starts, not at save.
- **"Untouched"** means: no transcript, `started_at` NULL, no prep content (material, chat or checklist items), no notes, `title_source` not `'user'`, and `template_id` NULL (reconcile never sets it, so a set category means the user chose it). A brief does not count: it is made automatically. *(My call.)*
- **No lost edits.** Each action re-checks its condition in the same SQL statement (the "untouched" test inside the `DELETE … WHERE`, the `title_source` test inside the rename `UPDATE`), so prep or a rename saved between the snapshot and the action wins.
- **Failures** are retried on the next tick and the last sync error (with its time) shows in Settings → Calendar, not only in the log.
- **Briefs** are made only for 📞 meetings (they are what the Prep tab shows) and appear in the Prep tab; "Regenerate brief" calls the existing generator. The Briefs page becomes **Digests** (weekly digests only); `/prep` is removed.
- **Existing preps.** The migration gives **every** `meeting_preps` row without a linked meeting (upcoming, past or unreadable, the `other` list in `prep/upcoming.rs:172,235` included) a meeting row with its event's title, uid, start and original start, so no prep becomes unreachable. Such rows for events without the marker are not reconciled: they keep their times and age out only if untouched (they are not, since they hold prep), so they stay until the user deletes them.

### 6.7 Pop-up Record routing

`meeting_banner_action` "record" (`banner.rs:144`) records into a 📞 meeting that is in progress or starts within **15 minutes** and has **no recording yet** (the closest to now when several overlap), opening its page in the Recording state. If that 📞 meeting is already Finished (a dropped call rejoined after the 120 s Continue window), it creates a new meeting with the event's title plus " · part 2" and no calendar link, so the reconcile key stays unique. *(My call.)* Otherwise it creates a new "Untitled · Thu 17:02" meeting and records into it. "Continue" is unchanged. The `calendar_title_now` window becomes 15 minutes to match.

### 6.8 Navigation and What's new

- Left nav as in `part2-screens.html`: Record new meeting at the top (becomes "Recording mm:ss" while live and opens that meeting), then Meetings, Tasks, Digests, Dictate, Practice, Stats; Import audio and Settings below; "What's new in vX" at the bottom with a dot until that version's notes are opened. Every item has a visible label (also in the collapsed rail: icon plus tooltip and accessible name).
- The sidebar mic button, the meetings list in the sidebar, the record screen, both About triggers (`Logo.tsx`, `Info.tsx`) and the Settings About tab are removed; their content (version, MIT credit to Meetily, Open data folder, Open log file) moves to the What's new page footer.
- **What's new** (`part2-screens.html` screen 7) renders `CHANGELOG.md`, bundled into the app at build time; newest first, the installed version marked. No network.

### 6.9 Design system

The first plan task writes `DESIGN.md` (via `/impeccable document`) from `app/globals.css` and the approved mockups: tokens, the three themes (Conclave default), type scale, table density, chips, sidebar, keyboard hints. UI tasks follow it.

## 7. Approaches considered

1. **Meeting-first recording.** A (chosen): create the row up front, save into it. B: reuse the continuation append path (rejected: it joins audio and has a 120 s window; bending it would make both features fragile). C: a separate "planned meetings" table merged at save (rejected: every list, search and Ask query would read two places).
2. **Who makes 📞 meetings.** A (chosen): the backend scheduler after each calendar refresh, so rows are right while the window is hidden and the pop-up can find them. B: the frontend when the table opens (rejected: the pop-up would miss meetings on days the table was not opened).
3. **Shipping.** A (chosen): one branch in phases (data → table → meeting page and sidebar → calendar and pop-up → What's new and removals), one release, with the pre-upgrade backup from the safe-updates project. B: several releases (rejected: half-old, half-new screens in between).

## 8. Testing

- Rust: migration + backfill on a database built from the previous schema with sample rows (template from JSON, start/end times from `created_at` minus audio length, gist, reviewed, every prep row gets a meeting row, the `meeting_preps` rebuild keeps all data); save-into-existing-meeting, missing-target fallback, crash recovery into the target row; calendar reconcile as table-driven tests covering every rule in §6.6, including: never deletes a row with content, incomplete expansion never cancels, rows outside the window never cancel, the live target and rows with `started_at` are never touched, reappearing events clear the cancel flag, one-off moves keep their prep, marker change touches nothing, a concurrent prep save wins over a delete; Delete recording vs Delete meeting (prep and notes kept or removed, kept tasks survive, dismissed occurrence not re-inserted); routing for all four start paths (window, overlap, already-finished 📞 meeting); Ask scoping by ids; every `created_at` reader that switched to the meeting date.
- Frontend (bun): view state save/restore, filter/sort/group, keyboard map, row-state rendering, search snippets, sidebar content per screen, What's new parsing of `CHANGELOG.md`.
- Browser preview mocks for every new command, then Playwright screenshots of each screen in the three themes, `/impeccable audit`, and the a11y check (`frontend/scripts/a11y-check.mjs`) with no serious or critical issues on `/` and the meeting page.
- Windows: the user's PC checklist (record from a page, pop-up into a 📞 meeting, Copilot during a call, update from v1.2.x with a backup).

## 9. Left to the builder

- Exact command names and payloads, component split, how the meeting id is threaded through the recording state.
- The gist field in the extraction prompt and its validation (fallback: first summary sentence).
- Column width limits, debounce of search, virtualisation threshold.
- Where "open at transcript time" scrolls and highlights.
- Whether the backfill runs as SQL (`json_extract`) or Rust, as long as it is idempotent and needs no AI.
