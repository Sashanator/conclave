# Conclave: Tasks review queue → Notion

Date: 2026-11-20 · Status: user answers folded in (2026-11-20); awaiting approval of this document
Approved mockup (open in a browser; the controls bar switches screens and themes): `docs/superpowers/specs/2026-11-20-tasks-notion-mockups/tasks-queue.html` (v5). UI follows `DESIGN.md`.

## 1. Intent

The AI pulls tasks out of every summarized meeting. Many are useless or belong to someone else. The user tracks real work in Notion, across three existing databases: **Job tasks**, **Personal tasks** and **German homework** (homework from German lessons). Today the Tasks page is a small to-do list inside Conclave that nobody acts on.

The Tasks page becomes a **review queue**: the user keeps or removes each extracted task, picks which Notion database each kept task goes to (pre-filled from the meeting's category), and one button creates the kept tasks in Notion. Notion stays the place where tasks are tracked and ticked off.

## 2. Goals, non-goals, anti-goals

**Goals**
- Review all meetings' tasks in one place, fast by keyboard (K keep, X remove, 1–9 database, ↑↓, Ctrl+Enter send).
- Send kept tasks to the right Notion database, filling the properties the user mapped for that database.
- Never create a duplicate in Notion, including after a crash or restart mid-send.
- Same Keep/Remove state on the meeting page panel and on the Tasks page.

**Non-goals**
- Two-way sync. Conclave never reads back a task's status from Notion and never edits or deletes a Notion page after creating it.
- Adding tasks by hand on the Tasks page (they are added in Notion). The manual "Add task" input goes away (user did not object, 2026-11-20).
- A Conclave-side Open/Done list. The Open/Done tabs go away.
- AI choosing the database. Defaults come from the meeting's category; the user overrides per task.
- Obsidian export, vocabulary → Anki (separate backlog items).

**Anti-goals** (failure even if it "works")
- A duplicate page in Notion, or a task silently lost between "kept" and "sent".
- Any Notion call the user did not trigger (no background sync, no polling).
- A task shown as sent when Notion did not confirm it.
- Breaking briefs, digests or Ask, which read the `tasks` table.

## 3. Constraints

- No telemetry, no phone-home (AGENTS.md). `api.notion.com` is added to `docs/network-calls.md` as allowed: only on the user's click (Send, Retry, Test, or reading a database's columns in Settings), with the user's own token. `scripts/check-no-phone-home.sh` must still pass; extend its allow-list if it checks hosts.
- The Notion token lives in Windows Credential Manager through `frontend/src-tauri/src/secrets/mod.rs` (new constant, e.g. `NOTION_TOKEN = "notionToken"`). It never reaches logs, errors, the frontend after saving (show last 4 characters only), or any host but `api.notion.com`.
- Cost first: no new LLM calls.
- The `tasks` table is read by `knowledge/briefs.rs`, `knowledge/digest.rs`, `knowledge/extract.rs` and `database/repositories/meeting.rs`. New state is additive; existing `status`/`owner` meanings stay.
- Windows-only target; Rust side testable on Linux with an HTTP mock.

## 4. Approaches considered

- **A. Direct Notion API from Rust, with a property mapping per database in Settings.** *Chosen by the user (2026-11-20).* Conclave reads each database's columns and lets the user map them; renaming a column in Notion is fixed in Settings, not by a release.
- B. Same, but the mapping hard-coded from the user's property list. Less UI, but every Notion change needs a release.
- C. Hand tasks to an AI agent with Notion access (Notion MCP). Costs an LLM call per send, unpredictable; against "cost first". Dropped.

## 5. Design

### 5.1 Review state (data model)

New migration adds to `tasks`:

| Column | Values | Meaning |
|---|---|---|
| `review` | `open` \| `keep` \| `drop` \| `sending` \| `sent` | Queue state. `open` = not decided. |
| `removed_by` | `ai` \| `user` \| NULL | Who removed it (shown as "removed by AI"). |
| `dest_id` | destination id or NULL | Chosen Notion destination (§5.3). NULL = none chosen. |
| `notion_page_id`, `notion_url` | text | Set only from Notion's success response. |
| `sent_at` | timestamp | When Notion confirmed. |
| `send_error` | text | Last failure, plain message. Cleared on success or a new attempt. |
| `due_date` | ISO date `YYYY-MM-DD` or NULL | Real due date (§5.3a). The existing free-text `due` stays for display. |
| `meeting_title`, `meeting_date` | text | Copy of the meeting's title and date, written when the meeting link is cleared (delete recording), so the task still sends with its context. *Builder's call (2026-11-20).* |

Rules:
- A new extracted task starts `review='open'`, except `owner='unassigned'` → `review='drop', removed_by='ai'` (same as today's meeting panel: unassigned shows as removed). Its `dest_id` is the default for the meeting's category (`meetings.template_id`) or NULL when that category says "Ask me".
- Keep → `review='keep'`, also `owner='me'`, `edited=1` (so today's survivors-on-delete rule and re-extraction keep it). Remove → `review='drop', removed_by='user'`, also `status='dismissed'` (so briefs/digests stop showing it). Restore → `review='open'`, `status='open'`.
- Editing the text in the queue uses the existing `tasks_edit` (sets `edited=1`).
- Re-extraction (`knowledge/tasks.rs` `apply_extraction`) must not delete or recreate rows whose `review` is not `open`. Today it only deletes untouched auto/open rows; it must also require `review='open'`.
- Delete recording (`delete_recording_data`): `sent` and `keep` rows survive with the meeting link cleared (they satisfy `owner='me' OR edited=1` already); `drop` and `open` unassigned rows go as today. Before clearing the link, the surviving rows get `meeting_title`/`meeting_date` copied from the meeting. A surviving row shows under a "Deleted meeting: <title>" group.
- `status` keeps its meaning for briefs/digests. `review='sent'` does not change `status` (the task is tracked in Notion now; briefs may keep listing it until ticked in Notion — acceptable, see §7).

### 5.2 Existing tasks at upgrade (decided by the user, 2026-11-20)

The migration sets:
- open tasks (`status='open'`) of meetings **not** marked Reviewed → `review='open'`, `dest_id` = the category default;
- open auto tasks whose recording was already deleted (`meeting_id IS NULL`, `source='auto'`) → `review='open'`, no destination, grouped under "Deleted meeting" (the title is unknown for these; the group says so);
- everything else (tasks of reviewed meetings, `done`/`dismissed` tasks, and manual tasks, which the user keeps in Notion already) → `review='drop'`, `removed_by=NULL`: hidden under Removed, restorable;
- `owner='unassigned'` open tasks follow §5.1 (`drop`, `removed_by='ai'`) whatever their meeting.

### 5.3 Destinations and mapping (Settings → Notion)

New Settings pane "Notion" in the App group (`frontend/src/lib/settings-panes.ts`), as in the mockup:
- **Integration token**: paste, masked (last 4), **Test** (calls Notion "retrieve bot user" and each database; reports Works / the exact problem), **Remove**. Help line: create an internal integration in Notion, then share each database with it (database ••• → Connections).
- **Destinations** list, up to 9 (keys 1–9). Each: a name shown in Conclave, the Notion database link (Conclave extracts the ID), a colour from a fixed destination palette, a status (Connected / Not shared with Conclave / Token missing / Not found). Seeded empty; the user adds Job tasks, Personal tasks, German homework.
- **Property mapping** per destination, filled from the database's columns read from Notion:
  - Task text → the database's title property (required, chosen automatically).
  - Due date → any `date` property (optional).
  - Meeting (title and date as text) → any `rich_text` property (optional).
  - Fixed values: for each `select`, `status`, `multi_select` or `checkbox` property the user may set a fixed value (e.g. Status = "To do"); options come from Notion.
  - Other property types are not settable in v1 (listed as "not used").
  The mapping is stored in the app settings (not a secret). If a mapped property disappears or changes type in Notion, Test and Send report it by name.
- **Default database per meeting category**: one picker per template plus "No template", each "Ask me" or a destination.

Destination colours are their own tokens, distinct from the category hues `--cat-1..5`. Mockup values for the first three (light / dark-and-Conclave): rose `340 75% 50%` / `340 75% 58%`, cyan `190 90% 32%` / `190 85% 45%`, lime `85 65% 32%` / `85 60% 45%` (each ≥3:1 against the background, distinguishable under deuteranopia/protanopia, checked 2026-11-20). Further colours are the builder's choice under the same checks; add them to `globals.css` and `DESIGN.md`.

### 5.3a Due dates (decided by the user, 2026-11-20: option a)

The extraction prompt (`knowledge/extract.rs`) also asks for `due_date`: an ISO date resolved from the spoken due text relative to the meeting's date (given in the prompt), or null when the text names no resolvable day. Same single LLM call, no extra cost. `parse_extraction` validates it (a real calendar date; otherwise null). In the queue the owner/due line shows the date (e.g. "by Thu 26 Nov"); clicking it opens a date input to change or clear it (stored in `due_date`, sets `edited=1`). The mapped Notion date property gets `due_date`; null leaves the property empty. Old tasks have no `due_date` until the user sets one.

### 5.4 What a Notion page contains

Properties as mapped. Page body: one paragraph "From *<meeting title>*, <meeting date>" (from the meeting, or the copied `meeting_title`/`meeting_date`) plus " at mm:ss" when the task has a transcript time. No transcript text, notes or summary go to Notion.

### 5.5 Sending

- **Send** (button or Ctrl+Enter) sends every task with `review='keep'` whose destination is Connected. Kept tasks with no destination ("need a database") or an unconnected one ("waiting for <name>") are counted in the bar and not sent. The button reads "Send N to Notion", "Sending…" while running, "Nothing to send" when N is 0, and is disabled while a send runs (one send at a time, enforced in Rust too).
- One Notion "create page" call per task, sequentially, honouring HTTP 429 `Retry-After`; a 30 s timeout per call.
- **No duplicates:** before each call the row is set to `review='sending'` and committed. Success → `sent` with page id/URL/time. A definite failure (4xx other than 429, or a connection error before the request was sent) → back to `keep` with `send_error`. An **ambiguous** failure (timeout or connection lost after sending) or a row found in `sending` at app start → shown in To review as "May already be in Notion" with **Open Notion database**, **Mark as sent**, and **Send again**. Conclave never retries an ambiguous one by itself.
- Result line in the bar: "Sent N to Notion · View" and "M not sent, see To review"; failed rows show their error and **Retry** (sends just that task).
- Errors are plain and specific: not shared ("Personal tasks isn't shared with Conclave. In Notion: database ••• → Connections → add Conclave"), token rejected, database not found, mapped property missing, rate-limited, offline.

### 5.6 Tasks page (mockup screens 1–3)

- Tabs: **To review** (count = open + kept-not-sent), **Sent to Notion**, **Removed**. The nav badge shows the same count as the To review tab.
- To review: grouped by meeting (title links to the meeting, date, category chip), newest first; "Keep all mine" per group; removed rows fold into "N removed · Show"; "All reviewed" empty state. Each row: Keep/Remove, editable text, owner/due line, destination picker with its colour dot; warning text for an unconnected destination. Key hint strip as in the mockup. K/X move to the next row; focus never drops to the page after any action.
- Sent: grouped by meeting, destination chip, "Sent <time>", **Open in Notion ↗** (opens `notion_url` in the default browser through the existing opener). Footnote: Conclave doesn't follow tasks once in Notion.
- Removed: Restore; "removed by AI / by you".
- The bottom send bar shows on To review only and never covers the list (it sits outside the scroll area).
- Rows are labelled groups for screen readers; Keep/Remove buttons name the task.

### 5.7 Meeting page panel

`components/MeetingPage/MeetingTasksPanel.tsx` switches to the `review` state: Keep/Remove write the same commands as the Tasks page, and kept tasks show their destination chip (picker included). "Done reviewing" keeps today's meaning (marks the meeting reviewed). Sending happens only on the Tasks page.

## 6. Testing

- Rust unit tests with a fake Notion HTTP server: mapping → request body for each property type; 200, 400 (property missing), 401, 403/404 (not shared), 429 with Retry-After, timeout → ambiguous; `sending` rows at start become "may already be in Notion"; concurrent Send is refused; token never appears in errors or logs.
- Migration test on a database built from the previous schema: new columns, the §5.2 rule, re-extraction leaves decided rows alone, delete recording keeps `sent`/`keep` rows.
- Extraction: `due_date` parsing (valid, invalid, missing) and the prompt carries the meeting date.
- Frontend (bun): queue counts, due-date edit, keyboard (K/X advance, 1–9, Ctrl+Enter not while editing text), send-bar states, focus after re-render.
- `bash scripts/check-no-phone-home.sh` and the a11y check on the browser preview (add preview fakes for the new commands in `src/lib/browser-preview.ts`).

## 7. Left to the builder

- Notion API version and endpoint details (the 2025 API moved databases to "data sources"; check the current docs with context7 and pin one version header). Use the data source of the linked database.
- Rust module layout (suggested `src/notion/`), command names, how the mapping is stored in settings.
- Colours for destinations 4–9.
- Whether briefs/digests should stop listing tasks once sent. Default: no change.

## 8. Decisions log

- 2026-11-20: approach A; dots in destination colours (not icons); option a for due dates; §5.2 upgrade rule; manual tasks not needed in Conclave (the user adds them in Notion directly); open tasks of deleted recordings are pointed out in the queue.
- The user may send each database's property list later; it serves as a test case. The mapping is built in Settings from Notion's columns, so it does not block planning.
