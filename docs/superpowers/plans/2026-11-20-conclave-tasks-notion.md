# Tasks review queue → Notion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Tasks page into a review queue (keep/remove, pick a Notion database per task) and create kept tasks as pages in the user's Notion databases, never twice.

**Architecture:** One additive migration gives `tasks` a `review` state plus Notion columns and adds `notion_destinations` / `notion_category_defaults`. A new Rust module `src/notion/` (reqwest client with a pinned API version, mapping, sequential send engine that commits `sending` before each call) is driven by Tauri commands; the frontend replaces `TaskList` with a queue (`components/Tasks/`), a Settings → Notion pane and a meeting-panel switch to `review`.

**Tech Stack:** Tauri 2 / Rust (crate `meetily`, lib `app_lib`), sqlx 0.8 SQLite, reqwest 0.11, chrono, uuid; Next.js 14 / React 18, date-fns 4, bun tests with happy-dom.

**Spec:** `docs/superpowers/specs/2026-11-20-conclave-tasks-notion-design.md` (mockup: `docs/superpowers/specs/2026-11-20-tasks-notion-mockups/tasks-queue.html`, v5; UI follows `DESIGN.md`).

## Global Constraints

- No telemetry, no phone-home. `api.notion.com` is called only on the user's click (Send, Retry, Send again, Test, Read columns), with the user's own token. No background sync, no polling, no Notion call on page load.
- Token: Windows Credential Manager via `frontend/src-tauri/src/secrets/mod.rs`, `pub const NOTION_TOKEN: &str = "notionToken";`. Never in logs, errors, the frontend after saving (last 4 characters only), or any host but `api.notion.com`. No struct holding it derives `Debug`; reqwest `bearer_auth`; Notion's error text is scrubbed of the token and cut to 300 chars.
- Cost first: no new LLM calls (the due date rides on the existing extraction call).
- `tasks` is read by `knowledge/briefs.rs`, `knowledge/digest.rs`, `knowledge/extract.rs`, `database/repositories/meeting.rs`: new state is additive; `status`/`owner` keep their meaning. `review='sent'` does not change `status`.
- Errors to the UI are plain `String`s (`Result<_, String>`); pool functions return `anyhow::Result` or `sqlx::Error`.
- No new dependencies.
- **Rust gate** (one cargo at a time across worktrees): `cd /workspace/frontend/src-tauri && . "$HOME/.cargo/env" && export CARGO_TARGET_DIR=/workspace/target && flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib -j 4 <filter>`. Needs `frontend/out` (from `pnpm run build`) and the stub sidecar `frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu`. Phase gate: same prefix with `cargo test --workspace --exclude llama-helper -j 4` from `/workspace`.
- **Frontend gate** (same lock): `cd /workspace/frontend && flock /workspace/.superpowers/cargo.lock sh -c 'pnpm exec tsc --noEmit && bun test && pnpm run build'`. One file: `cd /workspace/frontend && flock /workspace/.superpowers/cargo.lock bun test <path>`. pnpm 9.15.9.
- `bash scripts/check-no-phone-home.sh` must print `phone-home guard: clean` (it is a deny-list; no change needed).
- Frontend: colours from tokens only (`tests/theme-guard.test.ts`); warning text `text-amber-700 dark:text-amber-400`, error `text-destructive`, ok `text-emerald-700 dark:text-emerald-400`. Never `console.error` anything that can echo task text; log the command name only. Tauri args are camelCase.
- Each `git commit` is its own Bash call and ends with the session's `Co-Authored-By` trailer.

## Rulings

The controller rulings R1–R12 in `.superpowers/sdd/2026-11-20-tasks-notion/contract.md` are binding and are already applied below: R1 persisted destination status (Tasks 1, 12–14, 20, 23), R2 send re-check (Task 13), R3 Rust assigns ids (Task 12), R4 value shapes (Task 11), R5 status/messages (Task 10), R6 token entry first (Tasks 14, 23), R7 re-extraction rule (Task 3), R8 `Result` return (Task 3), R9 mapping in `mapping_json` (Task 12), R10 `tasks-changed` Tauri event (Tasks 4, 14, 27), R11 dead-code removal (Task 29), R12 colour checks (Tasks 21, 28).

Shared definitions used by several tasks:
- **Blocking destination states** (R1/R2): `not_shared`, `not_found`, `token_missing`, `token_rejected`, `mapping_broken`. `connected`, `offline` and NULL (never tested) do not block. Rust `pub(crate) const BLOCKING_STATES: [&str; 5]` in `notion/mod.rs`; TS `BLOCKING_STATES` in `lib/notion.ts`.
- **Event**: Rust `pub(crate) const TASKS_CHANGED: &str = "tasks-changed";` and `pub(crate) fn tasks_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>)` (emit `()`, a failure logged at `warn`) in `knowledge/tasks.rs`; TS `TASKS_CHANGED_EVENT = 'tasks-changed'` in `lib/tasks-queue.ts`, received with `listen` from `@tauri-apps/api/event`.

## Review Focus

1. **Notion confirmed, the app dies (or the DB write fails) before `sent` is stored.** Expected: the row stays `sending`, and at the next start it shows "May already be in Notion", never resent by itself. Test `outcome_not_saved_stays_sending_then_recovers` (Task 13).
2. **Token pasted with spaces, a trailing newline or a `Bearer ` prefix.** Expected: stored clean, Test works. Test `token_is_cleaned_and_only_last4_leaves` (Task 14).
3. **Database link in another form**: public `*.notion.site` link, a view link with `?v=<view>&p=<page>` (peek), a `notion://` desktop link. Expected: the database id from the path, never the view or page id. Test `parses_database_links_and_ids` (Task 11).
4. **A mapped column renamed in Notion after setup.** Expected: the first send fails naming the column, the destination becomes `mapping_broken`, the remaining tasks for it are not sent (no burst of failing calls) and wait with "Fix in Settings". Test `renamed_column_blocks_the_destination` (Task 13); UI waiting state in Task 20 `sendCheck`.
5. **Time zones.** A meeting stored `2026-11-20 23:30:00` (UTC) seen in UTC+1 is "21 Nov 2026" in the Notion page; the prompt carries the local meeting date; a due date `2026-11-26` is sent as a date-only `start` and labelled "by Thu 26 Nov" whatever the zone. Tests `meeting_day_uses_the_local_zone` (Task 11), `prompt_carries_the_meeting_date` (Task 2), `dueLabel` (Task 20).

## Execution lanes

Three worktrees. Cargo and every frontend build/test share `/workspace/.superpowers/cargo.lock`, so lanes edit in parallel but run gates one at a time.

| Lane | Tasks, in order | Starts when |
|---|---|---|
| A: data (Rust) | 1 → 2 → 3 → 4 → 5 | now |
| B: Notion (Rust) | 10, 11 (no DB) → 12 → 13 → 14 | 10/11 now; 12 after Task 1 is merged into B; 14 after Task 4 (uses `tasks_changed`) |
| C: frontend | 20 → (21 ∥ 22) → (23 ∥ 24) → 25 → 26 → 27 | now (all Rust is mocked) |
| Final | 28 (UI check, browser preview), then 29 | 28 after lane C; 29 after A, B and C are merged |

`lib.rs` is touched by Tasks 4, 10 and 14 (registration lines and `mod notion;`); expect trivial merge conflicts there. Codex pass (risky): Tasks 1, 3, 4, 5, 10, 13, 14, 26.

---

### Task 1: Migration `20261120000001_tasks_notion.sql` — mechanical, risky (persistence)

**Files:**
- Create: `frontend/src-tauri/migrations/20261120000001_tasks_notion.sql`
- Test: `frontend/src-tauri/src/knowledge/tasks.rs` (new `#[cfg(test)] mod notion_migration_tests` at the end; pattern of `redesign_migration_tests`, `database/repositories/meeting.rs:949`)

**Interfaces:**
- Produces: `tasks` columns `review TEXT NOT NULL DEFAULT 'open'`, `removed_by`, `dest_id`, `due_date`, `notion_page_id`, `notion_url`, `sent_at`, `send_error`, `send_ambiguous INTEGER NOT NULL DEFAULT 0`, `meeting_title`, `meeting_date`; tables `notion_destinations` (with R1 `last_status`, `last_message`) and `notion_category_defaults`.

- [ ] **Step 1: Write the failing test** `tasks_notion_migration_on_previous_schema`: run only migrations with `version < 20261120000001` on `sqlite::memory:` (`Migrator::new(Path::new("./migrations"))`, filter `m.migrations`), insert meetings `R` (reviewed=1) and `U` (reviewed=0) and these tasks `(id, meeting_id, owner, status, source, edited)`: `a U me open auto 0`, `b R me open auto 0`, `c U me done auto 0`, `d U me dismissed auto 1`, `e NULL me open auto 0`, `f NULL me open manual 0`, `g U unassigned open auto 0`, `h R unassigned open auto 0`, `i U unassigned done auto 0`; run all migrations; assert `SELECT id, review, removed_by, status, dest_id, send_ambiguous ORDER BY id` equals:

```text
a open NULL open | b drop NULL open | c drop NULL done | d drop NULL dismissed | e open NULL open
f drop NULL open | g drop ai open   | h drop ai open   | i drop NULL done        (dest_id NULL, send_ambiguous 0 everywhere)
```
and both new tables are empty; `SELECT last_status, last_message FROM notion_destinations` prepares (columns exist). Bind expected rows with an explicit `Vec<(String, String, Option<String>, String, Option<String>, i64)>` (inference cannot pick the element type).

- [ ] **Step 2: Run** — Rust gate, filter `notion_migration_tests`. Expected: FAIL `no such column: review`.

- [ ] **Step 3: Create the migration** (exact; the `UPDATE` is spec §5.2):

```sql
-- Tasks review queue → Notion (spec 2026-11-20 §5.1, §5.2). Additive: status/owner keep their meaning.
ALTER TABLE tasks ADD COLUMN review TEXT NOT NULL DEFAULT 'open'; -- open | keep | drop | sending | sent
ALTER TABLE tasks ADD COLUMN removed_by TEXT;  -- ai | user | NULL
ALTER TABLE tasks ADD COLUMN dest_id TEXT;     -- notion_destinations.id; NULL = none chosen
ALTER TABLE tasks ADD COLUMN due_date TEXT;    -- YYYY-MM-DD
ALTER TABLE tasks ADD COLUMN notion_page_id TEXT;
ALTER TABLE tasks ADD COLUMN notion_url TEXT;
ALTER TABLE tasks ADD COLUMN sent_at TEXT;
ALTER TABLE tasks ADD COLUMN send_error TEXT;
ALTER TABLE tasks ADD COLUMN send_ambiguous INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN meeting_title TEXT; -- copied when the meeting link is cleared
ALTER TABLE tasks ADD COLUMN meeting_date TEXT;

CREATE TABLE notion_destinations (id TEXT PRIMARY KEY, name TEXT NOT NULL, colour INTEGER NOT NULL, -- 1..9
  position INTEGER NOT NULL, database_id TEXT NOT NULL, data_source_id TEXT, mapping_json TEXT NOT NULL,
  created_at TEXT NOT NULL, last_status TEXT, last_message TEXT); -- last_*: last Test/send outcome, NULL = never tested
-- template_key: a template id, or '' for "No template". dest_id NULL = "Ask me".
CREATE TABLE notion_category_defaults (template_key TEXT PRIMARY KEY, dest_id TEXT);

-- Existing tasks (§5.2). dest_id stays NULL: no default exists yet; setting one later fills open/keep tasks.
UPDATE tasks SET
  review = CASE
    WHEN status = 'open' AND owner = 'unassigned' THEN 'drop'
    WHEN status = 'open' AND meeting_id IS NULL AND source = 'auto' THEN 'open'
    WHEN status = 'open' AND EXISTS (SELECT 1 FROM meetings m WHERE m.id = tasks.meeting_id AND m.reviewed = 0) THEN 'open'
    ELSE 'drop' END,
  removed_by = CASE WHEN status = 'open' AND owner = 'unassigned' THEN 'ai' END;
```

- [ ] **Step 4: Run** the filter, then the Rust gate with no filter (`knowledge::` and `database::` stay green). Expected: PASS.
- [ ] **Step 5: Commit** `git add frontend/src-tauri/migrations/20261120000001_tasks_notion.sql frontend/src-tauri/src/knowledge/tasks.rs` — `feat(tasks): review-queue and Notion columns, upgrade rule for existing tasks`

---

### Task 2: Extraction asks for `due_date` — mechanical

**Files:**
- Modify: `frontend/src-tauri/src/knowledge/extract.rs` (`ExtractedTask`, `SYSTEM`, `build_prompts`, `parse_extraction`, `extract_once` at the `build_prompts` call, tests; update the existing `build_prompts` test calls to pass `None`)
- Modify (test helper only): `frontend/src-tauri/src/knowledge/tasks.rs` `fn task` (:416) gets `due_date: None`

**Interfaces:**
- Produces: `ExtractedTask.due_date: Option<String>`; `pub(crate) fn iso_date(s: &str) -> Option<String>` (trimmed, exactly 10 chars, parses `%Y-%m-%d` to a real date → canonical string; length check first because chrono accepts `2026-1-5`); `pub fn build_prompts(summary_md: &str, my_names: &[String], own_lines: &[String], meeting_date: Option<chrono::NaiveDate>) -> (String, String)`.

- [ ] **Step 1: Write the failing tests** in `extract.rs` `mod tests`:

```rust
#[test]
fn prompt_carries_the_meeting_date() {
    let (system, user) = build_prompts("S", &[], &[], chrono::NaiveDate::from_ymd_opt(2026, 11, 20));
    assert!(system.contains("\"due_date\""));
    assert!(user.contains("Meeting date: 2026-11-20 (Friday)"));
    assert!(build_prompts("S", &[], &[], None).1.contains("Meeting date: unknown"));
}
#[test]
fn due_date_must_be_a_real_calendar_date() {
    // tasks a..e with due_date "2026-11-26", "2026-02-30", missing, "Thursday", "2026-1-5"
    assert_eq!(dates, vec![Some("2026-11-26".to_string()), None, None, None, None]);
    assert_eq!(iso_date(" 2028-02-29 "), Some("2028-02-29".to_string()));
}
```

- [ ] **Step 2: Run** — filter `knowledge::extract`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** `parse_extraction`: `due_date: task.due_date.as_deref().and_then(iso_date)`. `SYSTEM` exact copy: task schema `{\"text\":\"...\",\"owner\":\"me\" or \"unassigned\",\"due\":\"...\",\"due_date\":\"YYYY-MM-DD\",\"at\":seconds}`; the "Leave out" sentence becomes `Leave out \"due\", \"due_date\" and \"at\" when unknown. No other text.`; after the `"at"` line add `\"due_date\" is the day \"due\" names, worked out from the meeting date given below (\"Thursday\" means the first Thursday after the meeting). Leave it out unless \"due\" names one day.`. User prompt starts `{UNTRUSTED}\n\nMeeting date: {date}\n\nNames of the user: ...`, `date` = `d.format("%Y-%m-%d (%A)")` or `unknown`. In `extract_once`: `let date = MeetingsRepository::meeting_date(pool, meeting_id).await.ok().flatten().map(|d| d.with_timezone(&chrono::Local).date_naive());` — Local, so a late-evening meeting resolves "tomorrow" from the user's day; `.ok()` on purpose (an unparsable stored date only makes the prompt say "unknown").
- [ ] **Step 4: Run** — filter `knowledge::`. Expected: PASS.
- [ ] **Step 5: Commit** `extract.rs`, `tasks.rs` — `feat(extract): ask for an ISO due_date resolved from the meeting date`

---

### Task 3: `TaskRow` fields, category default, re-extraction respects decisions — judgment, risky

**Files:**
- Modify: `frontend/src-tauri/src/knowledge/tasks.rs` (`TaskRow` :15-27, `apply_extraction` :44-123, `list` :187, `add` :242, tests)

**Interfaces:**
- Consumes: Task 1 columns; Task 2 `ExtractedTask.due_date`.
- Produces:
  - `TaskRow` gains after `created_at`: `review: String, removed_by: Option<String>, dest_id: Option<String>, due_date: Option<String>, notion_url: Option<String>, sent_at: Option<String>, send_error: Option<String>, send_ambiguous: bool, template_id: Option<String>`.
  - `fn select_sql() -> String`: `SELECT … FROM tasks t LEFT JOIN meetings m ON m.id = t.meeting_id` with `COALESCE(m.title, t.meeting_title) AS meeting_title`, `COALESCE({meeting_date_sql("m")}, t.meeting_date) AS meeting_date`, `m.template_id`, every other `TaskRow` column; callers append WHERE/ORDER. Used by `list` and Task 4.
  - `pub(crate) async fn default_dest_for_meeting<'c>(exec: impl sqlx::SqliteExecutor<'c>, meeting_id: &str) -> Result<Option<String>, sqlx::Error>` (R8): `SELECT d.dest_id FROM meetings m JOIN notion_category_defaults d ON d.template_key = COALESCE(m.template_id, '') JOIN notion_destinations n ON n.id = d.dest_id WHERE m.id = ?` (a dangling default yields None).

- [ ] **Step 1: Write the failing tests** (helper `add_dest(pool, id, template_key, dest: Option<&str>)` inserts a destination row if missing and a default row):
  - `category_default_destination`: meeting `M` (no template) → None; after `add_dest("job", "", Some("job"))` → `Some("job")`; meeting `S` with `template_id='standup'` and `add_dest("job","standup",None)` → None ("Ask me"); after `DELETE FROM notion_destinations` → `M` gives None.
  - `new_tasks_get_review_state_and_default_destination`: extraction `[task("Mine"), unassigned "Theirs" with due_date "2026-11-26"]` → rows `(Mine, open, NULL, job, NULL)`, `(Theirs, drop, ai, job, 2026-11-26)`.
  - `reextract_replaces_only_untouched_ai_output` (R7): rows of `M`, all `status='open', source='auto', edited=0`: `k` keep, `x` drop/`removed_by='user'`, `s` sent, `o` open, `a` drop/`removed_by='ai'` (texts K, X, S, O, A). Extract `[K, X, S, N]`. Assert texts = `[K keep, N open, S sent, X drop]` (O and A replaced/gone) and ids of K, S, X unchanged (`k`, `s`, `x`).
- [ ] **Step 2: Run** — filter `knowledge::tasks`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** The DELETE at :63 becomes (R7): `DELETE FROM tasks WHERE meeting_id = ? AND source = 'auto' AND status = 'open' AND edited = 0 AND (review = 'open' OR (review = 'drop' AND removed_by = 'ai'))`. Then `let dest = default_dest_for_meeting(&mut *tx, meeting_id).await?;`; the INSERT also writes `review` (`drop` when `task.owner == "unassigned"`, else `open`), `removed_by` (`ai` for unassigned), `dest_id` = `dest`, `due_date`. The `kept`/`seen` query stays (decided rows are never duplicated). `list` = `select_sql()` + `WHERE t.status = ? ORDER BY` (order unchanged); `add` fills `review: "open"`, `send_ambiguous: false`, the rest None.
- [ ] **Step 4: Run** — filter `knowledge::`. Expected: PASS (incl. `reextract_keeps_user_touched_tasks`, briefs tests reading `tasks::list`).
- [ ] **Step 5: Commit** — `feat(tasks): review state and category destination on extraction; re-extraction keeps decided rows`

---

### Task 4: Queue commands and the `tasks-changed` event — judgment, risky

**Files:**
- Modify: `frontend/src-tauri/src/knowledge/tasks.rs` (new fns after `remove` :272, commands after `tasks_delete` :338; `tasks_edit` :313 and `tasks_set_status` :302 gain `app: tauri::AppHandle` and emit)
- Modify: `frontend/src-tauri/src/knowledge/extract.rs` (`start_worker` :298 emits after `run_one` returns Ok; `extraction_retry` :541 gains `app: tauri::AppHandle` and emits after Ok)
- Modify: `frontend/src-tauri/src/lib.rs` (register after `knowledge::tasks::tasks_delete,` :1111)

**Interfaces:**
- Consumes: Task 3 `TaskRow`, `select_sql`; Task 2 `iso_date`; `stamp()` :38.
- Produces: `TASKS_CHANGED`, `tasks_changed(app)` (see Rulings). Pool fns (`pub(crate)`, `anyhow::Result`) and commands (`Result<_, String>`, each write command takes `app: tauri::AppHandle` and calls `tasks_changed(&app)` after success):
  - `queue_list(pool) -> Vec<TaskRow>` / `tasks_queue_list()`: every task, `ORDER BY julianday(COALESCE({d}, t.meeting_date)) DESC, t.created_at, t.rowid`.
  - `set_review(pool, id, review: &str)` / `tasks_set_review(id: String, review: String)`.
  - `set_dest(pool, id, dest_id: Option<&str>)` / `tasks_set_dest(id: String, dest_id: Option<String>)`.
  - `set_due_date(pool, id, due_date: Option<&str>)` / `tasks_set_due_date(id: String, due_date: Option<String>)`.
  - `mark_sent(pool, id)` / `tasks_mark_sent(id: String)`.
- Rule: these writes change only rows with `review IN ('open','keep','drop')` (a `sending`/`sent` row must never return to `keep`); `mark_sent` has its own guard. Zero rows → error `task not found or already sent`.

- [ ] **Step 1: Write the failing tests:**
  - `keep_remove_restore`: an AI-dropped unassigned row; `keep` → `(review keep, status open, owner me, edited 1, removed_by NULL)`; `drop` → `(drop, dismissed, me, 1, user)`; `open` → `(open, open, me, 1, NULL)`; `set_review(.., "sent")` errs; once `review='sent'`, `set_review keep`, `set_dest None`, `set_due_date None` all err.
  - `destination_and_due_date`: `set_dest Some("job")` ok, `Some("nope")` errs; `set_due_date "2026-11-26"` ok and sets `edited=1`, `"2026-02-30"` errs; `None` clears both.
  - `mark_sent_only_for_ambiguous_rows`: keep rows `a` (ambiguous) and `b`; `mark_sent(a)` → `(sent, send_ambiguous 0, sent_at set)`; `mark_sent(b)` errs and `b` stays keep.
  - `queue_lists_every_task_with_meeting_context`: `a` on meeting `M` (`template_id='standup'`, dismissed), orphan `b` with copied `meeting_title='Gone'`, `meeting_date='2026-01-01'`, `send_ambiguous=1` → `[(b, Gone, None, open, true), (a, M, standup, open, false)]` (test meetings have `created_at='t'`, which `julianday` reads as NULL, so dated `b` sorts first).
- [ ] **Step 2: Run** — filter `knowledge::tasks`. Expected: compile FAIL.
- [ ] **Step 3: Implement** (SQL values are spec §5.1):
  - `set_review`: `keep` → `review='keep', owner='me', edited=1, status='open', removed_by=NULL`; `drop` → `review='drop', removed_by='user', status='dismissed', edited=1`; `open` → `review='open', status='open', removed_by=NULL`; other → `bail!("unknown review state")`.
  - `set_dest`: `… AND (?1 IS NULL OR EXISTS (SELECT 1 FROM notion_destinations WHERE id = ?1))`; zero rows → `task not found, already sent, or unknown database`.
  - `set_due_date`: `iso_date` or `due date must be YYYY-MM-DD`; writes `edited = 1`.
  - `mark_sent`: `UPDATE tasks SET review='sent', send_ambiguous=0, send_error=NULL, sent_at=? WHERE id=? AND send_ambiguous=1 AND review <> 'sending'`; zero rows → `only a task that may already be in Notion can be marked sent`.
  - Register the five commands after `knowledge::tasks::tasks_delete,`.
- [ ] **Step 4: Run** — filter `knowledge::`, then `cargo check -p meetily --lib -j 4` under the lock. Expected: PASS, check clean.
- [ ] **Step 5: Commit** `tasks.rs`, `extract.rs`, `lib.rs` — `feat(tasks): queue commands, tasks-changed event`

---

### Task 5: Delete keeps decided tasks with the meeting's title and date — mechanical, risky (deletion)

**Files:**
- Modify: `frontend/src-tauri/src/database/repositories/meeting.rs` (`delete_recording_data` :364-390; test in `mod migration_tests` :501 after `kept_tasks_survive_both_deletes` :657)

**Interfaces:**
- Consumes: Task 1 columns; `meeting_date_sql` (:42).
- Produces: after `delete_recording` and `delete_meeting`, surviving tasks have `meeting_id = NULL`, `at = NULL`, `meeting_title`/`meeting_date` copied (raw `meeting_date_sql` text).

- [ ] **Step 1: Write the failing test** `decided_tasks_survive_with_meeting_copy`: for both deletes, meeting `M` (`insert_meeting` :511: title `M`, `created_at '2026-10-17 12:00:00'`) with tasks `k` (me, open, edited 1, keep), `s` (unassigned, open, edited 0, sent), `u` (me, dismissed, edited 1, drop/user), `a` (unassigned, open, drop/ai). After delete: rows are exactly `k`, `s`, each `(meeting_id NULL, meeting_title "M", meeting_date "2026-10-17 12:00:00")`.
- [ ] **Step 2: Run** — filter `decided_tasks_survive_with_meeting_copy`. Expected: FAIL.
- [ ] **Step 3: Implement** before the `for sql in [...]` loop:

```rust
// Survivors keep their meeting's title and date so they still send with context (spec §5.1).
let copy = format!("UPDATE tasks SET meeting_title = m.title, meeting_date = {d} FROM meetings m \
    WHERE m.id = tasks.meeting_id AND tasks.meeting_id = ?", d = meeting_date_sql("m"));
sqlx::query(&copy).bind(meeting_id).execute(&mut *tx).await?;
```
(`UPDATE … FROM` needs SQLite ≥ 3.33, which sqlx 0.8 bundles; else use correlated subqueries.) The survivor statement becomes `UPDATE tasks SET meeting_id = NULL, at = NULL WHERE meeting_id = ? AND ((status <> 'dismissed' AND (owner = 'me' OR edited = 1)) OR review IN ('keep', 'sending', 'sent'))`; the doc comment adds "or are kept, being sent or sent to Notion".
- [ ] **Step 4: Run** — filter `database::repositories::meeting`, then the phase gate. Expected: PASS.
- [ ] **Step 5: Commit** — `fix(tasks): deleting a recording keeps kept and sent tasks with the meeting's title and date`

---

## Pinned Notion API facts (checked 2026-11-20; used by Tasks 10–14)

| Fact | Value | Source |
|---|---|---|
| Version header | `Notion-Version: 2026-03-11` (its breaking changes — block `position`, `archived`→`in_trash`, `transcription`→`meeting_notes` — are not used here) | https://developers.notion.com/guides/get-started/upgrade-guide-2026-03-11 , https://developers.notion.com/reference/versioning |
| Base URL, auth | `https://api.notion.com/v1`, `Authorization: Bearer <token>` | https://developers.notion.com/reference/intro |
| Token test | `GET /v1/users/me` → bot user | https://developers.notion.com/reference/get-self |
| Retrieve database | `GET /v1/databases/{id}` → `{"id","title":[rich text],"data_sources":[{"id","name"}]}`; no `properties` | https://developers.notion.com/reference/retrieve-database |
| Retrieve data source | `GET /v1/data_sources/{id}` → `{"id","properties":{"<name>":{"id","name","type","<type>":{…}}}}`; select/status/multi_select carry `options:[{"name"}]` | https://developers.notion.com/reference/retrieve-a-data-source , https://developers.notion.com/reference/property-object |
| Create page | `POST /v1/pages` `{"parent":{"data_source_id":"…"},"properties":{…},"children":[…]}` → `id`, `url`; property keys may be names | https://developers.notion.com/reference/post-page |
| Property writes | title `{"title":[{"type":"text","text":{"content":"…"}}]}`; rich_text same shape under `rich_text`; date `{"date":{"start":"2026-11-26"}}`; select `{"select":{"name":"…"}}`; status `{"status":{"name":"…"}}`; multi_select `{"multi_select":[{"name":"…"}]}`; checkbox `{"checkbox":true}` | https://developers.notion.com/reference/page-property-values |
| Paragraph block | `{"object":"block","type":"paragraph","paragraph":{"rich_text":[…]}}`, italic via `"annotations":{"italic":true}` | https://developers.notion.com/reference/block |
| Limits | `text.content` ≤ 2000 chars; ~3 requests/s | https://developers.notion.com/reference/request-limits |
| 429 | `rate_limited`, `Retry-After` integer seconds | https://developers.notion.com/reference/request-limits |
| Errors | `{"object":"error","status","code","message"}`: 400 `validation_error` (names the property), 401 `unauthorized`, 403 `restricted_resource`, 404 `object_not_found` (missing **or** not shared; Notion does not tell them apart), 409 `conflict_error`, 5xx incl. 503 ("a write may have saved anyway") | https://developers.notion.com/reference/status-codes |

Decisions: non-2xx < 500 (incl. 429 after retries) is **definite**; ≥ 500, a timeout, a connection lost after writing, or a 2xx without `id` is **ambiguous**; reqwest `is_connect()` is definite (not sent). Mapping keys are property names. A database's first `data_sources` entry is used (`shortcut: multi-source databases always use the first source; add a picker if the user ever has one.`). HTTP tests use a hand-rolled `tokio::net::TcpListener` server like `knowledge/calendar.rs` `serve_once`, clients with `.no_proxy()` like `practice/claude.rs`.

---

### Task 10: Notion HTTP client and token constant — judgment, risky (credentials, untrusted input)

**Files:**
- Modify: `frontend/src-tauri/src/secrets/mod.rs` (`pub const NOTION_TOKEN: &str = "notionToken";` after `CUSTOM_OPENAI_API_KEY` :14)
- Create: `frontend/src-tauri/src/notion/mod.rs` (`pub mod client;`), `frontend/src-tauri/src/notion/client.rs`
- Modify: `frontend/src-tauri/src/lib.rs` (`mod notion;` after `mod updates;` :69)

**Interfaces** (`pub(crate)` in `notion::client`):
- `const NOTION_API: &str = "https://api.notion.com/v1"; const NOTION_VERSION: &str = "2026-03-11";`
- `struct NotionClient { base, token, http }` (no `Debug`); `NotionClient::new(token: String)`; `#[cfg(test)] NotionClient::with_base(base: &str, token: &str, timeout: Duration)` (`.no_proxy()`). Builder: `redirect(Policy::none())`, `connect_timeout(10 s)`, `timeout(30 s)`.
- `async fn me(&self) -> Result<(), CallError>`, `database(&self, id) -> Result<Value, CallError>`, `data_source(&self, id) -> Result<Value, CallError>`, `create_page(&self, body: &Value) -> Result<CreatedPage, CallError>`; `struct CreatedPage { id: String, url: Option<String> }`.
- `enum CallError { Http { status: u16, code: String, message: String }, Offline, NoAnswer }` with `is_ambiguous(&self) -> bool`, `state(&self) -> &'static str`, `message(&self, what: &str) -> String`.
- `#[cfg(test)] pub(crate) mod fake`: `enum Reply { Http(String), Hang, Drop }`, `fn reply(status: u16, extra_headers: &str, body: &str) -> Reply` (adds `Content-Length`, `Connection: close`), `async fn serve(replies: Vec<Reply>) -> Fake`, `struct Fake { base: String, requests: Arc<Mutex<Vec<Req>>> }`, `struct Req { head: String, body: String }`, `async fn Fake::wait_requests(&self, n: usize)` (poll 10 ms, panic after 5 s). Accepts connections one at a time, reads head and `Content-Length` body, records, then `Http` writes and closes, `Hang` parks the socket in a task for 10 s and moves on, `Drop` closes silently.

- [ ] **Step 1: Add the constant.**
- [ ] **Step 2: Write the failing tests** (token `"ntn_SECRET123"`):
  - `sends_auth_and_version_headers`: head starts `get /users/me `, contains `authorization: bearer ntn_secret123` and `notion-version: 2026-03-11` (lowercased).
  - `create_page_posts_body_and_reads_id_and_url`: `POST /pages `, body JSON round-trips, returns `("p1", Some("https://www.notion.so/p1"))`.
  - `retries_429_after_retry_after_then_succeeds` (2 requests) and `gives_up_after_three_429_retries_as_definite` (4 requests, `Http{status:429}`, not ambiguous).
  - `classifies_statuses` (R5), each body carrying the token in `message`:

```rust
for (status, code, ambiguous, state) in [
    (400, "validation_error", false, "mapping_broken"), (401, "unauthorized", false, "token_rejected"),
    (403, "restricted_resource", false, "not_shared"), (404, "object_not_found", false, "not_shared"),
    (409, "conflict_error", false, "offline"), (429, "rate_limited", false, "offline"),
    (500, "internal_server_error", true, "offline"), (503, "service_unavailable", true, "offline"),
] { /* assert is_ambiguous, state, and !e.message("Personal tasks").contains("ntn_SECRET123") */ }
```
  - `messages_use_the_spec_copy`: 400 message contains `Due is not a property that exists.`; 404 → exactly `Personal tasks isn't shared with Conclave, or was deleted. In Notion: database ••• → Connections → add Conclave`; 429 → `Notion asked to slow down. Try again in a minute.`
  - `timeout_and_dropped_connection_are_ambiguous_refused_connect_is_definite`: `Hang` with 300 ms timeout → `NoAnswer`; `Drop` → ambiguous; a closed port → `Offline`, not ambiguous, state `offline`.
  - `ok_without_id_is_ambiguous`.
- [ ] **Step 3: Run** — filter `notion::client`. Expected: compile FAIL.
- [ ] **Step 4: Implement.** One private `call(method, path, body)`: up to 3 retries on 429 sleeping `Retry-After` (missing/unparsable → 1 s, cap 30 s). reqwest error: `is_connect()` → `Offline` (check before `is_timeout()`), else `NoAnswer`. Non-2xx: `code`/`message` from the body (missing → ""), token replaced by `[token]`, cut to 300 chars. 2xx unparsable or `create_page` without string `id` → `NoAnswer`. `is_ambiguous` = `NoAnswer` or status ≥ 500. `state`: 400 → `mapping_broken`, except a message mentioning `parent` → `not_found`; 401 → `token_rejected`; 403/404 → `not_shared`; everything else (409, 429, other 4xx, ≥ 500, `Offline`, `NoAnswer`) → `offline`. `message(what)` exact copy:
  - 400: `{what}: Notion refused it: {message}` (empty message: `{what}: Notion refused it (HTTP 400).`; same form for other 4xx not listed)
  - 401: `Notion rejected the token. Paste a new one in Settings → Notion.`
  - 403: `{what}: the Conclave connection isn't allowed to add pages here. In Notion, give it the Insert content capability.`
  - 404: `{what} isn't shared with Conclave, or was deleted. In Notion: database ••• → Connections → add Conclave`
  - 429: `Notion asked to slow down. Try again in a minute.`
  - ≥ 500: `Notion had a problem (HTTP {status}).` · `Offline`: `Notion could not be reached. Check your internet connection.` · `NoAnswer`: `Notion did not answer in time.`
- [ ] **Step 5: Run** — filter `notion::client`. Expected: PASS.
- [ ] **Step 6: Commit** `secrets/mod.rs`, `notion/`, `lib.rs` — `feat(notion): HTTP client with pinned API version, 429 retry and definite/ambiguous errors`

---

### Task 11: Types, link parsing, schema and page body — judgment, risky (untrusted input)

**Files:**
- Modify: `frontend/src-tauri/src/notion/mod.rs` (types, `pub mod mapping;`, `BLOCKING_STATES`)
- Create: `frontend/src-tauri/src/notion/mapping.rs`

**Interfaces:**
- `notion/mod.rs`, all `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] #[serde(rename_all = "camelCase")]`:
  `Destination { id: String, name: String, colour: u8, position: i64, database_id: String, data_source_id: Option<String>, mapping: Mapping, last_status: Option<String>, last_message: Option<String> }` (R1; `last_*` read-only from the UI, `#[serde(default)]`),
  `Mapping { title_prop: String, due_prop: Option<String>, meeting_prop: Option<String>, fixed: Vec<FixedValue> }`,
  `FixedValue { prop: String, kind: String, value: serde_json::Value }` (R4: string for select/status, string array for multi_select, bool for checkbox),
  `DbSchema { database_id: String, data_source_id: Option<String>, title: String, properties: Vec<PropInfo> }`, `PropInfo { id, name, kind: String, options: Vec<String> }`,
  `DestStatus { dest_id: String, state: String, message: Option<String> }`, `NotionConfig { token_last4: Option<String>, destinations: Vec<Destination>, category_defaults: Vec<CategoryDefault> }`, `CategoryDefault { template_key: String, dest_id: Option<String> }`, `SendReport { sent: u32, failed: u32, ambiguous: u32 }` (+ `Default`).
  `pub(crate) const BLOCKING_STATES: [&str; 5] = ["not_shared", "not_found", "token_missing", "token_rejected", "mapping_broken"];`
- `notion::mapping` (`pub(crate)`): `parse_uuid(s) -> Option<String>` (32 hex digits, dashes ignored → lowercase 8-4-4-4-12); `parse_database_id(link) -> Result<String, String>`; `first_data_source(db: &Value) -> Option<String>`; `schema_from(database_id, db: &Value, ds: &Value) -> DbSchema`; `check_mapping(m: &Mapping, s: &DbSchema) -> Vec<String>`; `struct SendTask { id, text, at: Option<f64>, due_date: Option<String>, meeting_title: Option<String>, meeting_date: Option<String>, dest_id: String }` (`sqlx::FromRow`); `display_date<Tz: TimeZone>(raw: &str, tz: &Tz) -> Option<String> where Tz::Offset: Display`; `page_body(data_source_id: &str, m: &Mapping, t: &SendTask) -> Value`.

- [ ] **Step 1: Write the failing tests** (`ID = "1a2b3c4d5e6f47a8b9c0d1e2f3a4b5c6"`, `DASHED` = its 8-4-4-4-12 form; `VIEW`, `PAGE` = two other 32-hex ids):
  - `parses_database_links_and_ids`: each of `https://www.notion.so/myws/Job-tasks-{ID}?v={VIEW}&pvs=4`, `https://www.notion.so/{ID}`, `https://www.notion.so/myws/{ID}?v={VIEW}&p={PAGE}&pm=s`, `https://acme.notion.site/Job-tasks-{ID}?pvs=4`, `notion://www.notion.so/myws/Job-tasks-{ID}`, `"  {DASHED}  "`, `ID.to_uppercase()` → `DASHED`; `https://www.notion.so/myws/Job-tasks`, `""`, `../../users/me` → Err with copy `That doesn't look like a Notion database link. In Notion: open the database as a full page, ••• → Copy link.`
  - `reads_schema_title_first`: db title runs "Job " + "tasks" → `Job tasks`; `data_source_id` = `ds.id`; property order `[Name, Due, Meeting, Status, Tags, Urgent]` (title first, rest by name); Status options `["To do","Done"]`; `first_data_source` of a non-hex id → None.
  - `check_mapping_reports_by_name`: the full mapping (Name/Due/Meeting, Status=`"To do"`, Tags=`["Work"]`, Urgent=`true`) → empty; with `due_prop "Deadline"`, `meeting_prop "Due"`, Status `"Later"` → `["“Deadline” is missing", "“Due” is no longer a text property", "“Status” has no option “Later”"]`.
  - `page_body_maps_every_property_kind`: task "Send the deck", `at 754.0`, due `2026-11-26`, meeting `Weekly sync` / `2026-11-20T10:00:00+00:00` → `parent = {"data_source_id":"ds-1"}`, Name/Due/Status/Tags/Urgent shapes as in the facts table, `Due == {"date":{"start":"2026-11-26"}}` (no time part), Meeting `"Weekly sync, 20 Nov 2026"`, paragraph text `From Weekly sync, 20 Nov 2026 at 12:34` with run 1 italic.
  - `page_body_without_due_or_meeting`: no `Due` key; paragraph `From a deleted meeting`.
  - `meeting_day_uses_the_local_zone` (Review Focus 5): with `FixedOffset::east_opt(3600)`: `display_date("2026-11-20 23:30:00", tz)` and `display_date("2026-11-20T23:30:00+00:00", tz)` → `Some("21 Nov 2026")`; `display_date("2026-11-20", tz)` → `Some("20 Nov 2026")`; `"t"` → None.
- [ ] **Step 2: Run** — filter `notion::mapping`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** Link parsing (exact rule): trim; if `parse_uuid` of the whole input succeeds, use it; else cut at the first `?` or `#` (query ids are views/pages, never the database), take the last `/` segment, strip dashes, and `parse_uuid` its last 32 characters (title slugs end in `-<id>`). `display_date`: replace the first space with `T`, parse RFC 3339; else parse `%Y-%m-%dT%H:%M:%S%.f` as UTC (DB naive times are UTC, as `DateTimeUtc` decodes them); convert to `tz`; else a 10-char date as `NaiveDate`; format `%-d %b %Y`. `page_body` calls it with `&chrono::Local`. Paragraph: `"From "` + italic title + `", <date>"` (when known) + `" at m:ss"` (`h:mm:ss` from 3600 s) when `at` is set; no title → plain `From a deleted meeting` (+ date when known). Meeting property: `"<title>, <date>"` or `Deleted meeting`. Every `content` cut to 2000 chars; only `Some` `due_prop`/`meeting_prop` are written (null due leaves the property out). `check_mapping` order: title, due, meeting, fixed in order; expected kinds title→`title`, due→`date`, meeting→`rich_text`, fixed→its `kind`; labels in "is no longer a {label} property": `title`, `date`, `text`, `select`, `status`, `multi-select`, `checkbox`; option checks for select/status (the string) and multi_select (each string).
- [ ] **Step 4: Run** — filter `notion::mapping`. Expected: PASS.
- [ ] **Step 5: Commit** `notion/` — `feat(notion): destination types, database-link parsing, schema check and page body`

---

### Task 12: Destination and category-default storage — judgment

**Files:**
- Modify: `frontend/src-tauri/src/notion/mod.rs`

**Interfaces** (`pub(crate)` in `notion`):
- Consumes: Task 11 types and `parse_database_id`, `parse_uuid`; Task 1 tables; `crate::knowledge::tasks::stamp()`; `crate::database::repositories::test_pool()` (`database/repositories/mod.rs:11`).
- Produces: `const MAX_DESTINATIONS: usize = 9;` `async fn list_destinations(pool) -> anyhow::Result<Vec<Destination>>` (by `position`, `mapping_json` via serde); `async fn save_destination(pool, dest: Destination) -> Result<Destination, String>`; `async fn delete_destination(pool, id: &str) -> Result<(), String>`; `async fn list_category_defaults(pool) -> anyhow::Result<Vec<CategoryDefault>>`; `async fn set_category_default(pool, template_key: &str, dest_id: Option<&str>) -> Result<(), String>`; `async fn set_dest_status(pool, dest_id: &str, state: &str, message: Option<&str>) -> anyhow::Result<()>` (R1).

- [ ] **Step 1: Write the failing tests** (fixture `dest(id, name)`: colour 1, a link with `ID` as `database_id`, `data_source_id` = 32 hex, mapping title `Name`, `last_* None`):
  - `save_inserts_normalises_and_updates`: `id ""` → UUID assigned (R3), name trimmed, `position` 1 then 2, ids normalised to dashed form; an update renames and keeps position; list order `["Work", "Personal"]`.
  - `save_resets_status_and_ignores_incoming_status`: after `set_dest_status(id, "mapping_broken", Some("x"))`, saving the destination (with `last_status: Some("connected")` in the input) stores `last_status NULL` (a fixed mapping must not stay blocked until the next Test).
  - `save_refuses_bad_input_and_a_tenth`: colour 10, blank name, `data_source_id None` (`Read the database's columns first.`), checkbox fixed value `"yes"` all Err; the tenth → `Conclave supports up to 9 Notion databases.`
  - `category_default_fills_undecided_tasks_and_delete_clears_references`: tasks `t1` open/`m1` standup, `t2` keep/`m1`, `t3` sent/`m1`, `t4` open/`m2` no template; `set_category_default("standup", Some(d))` fills `t1`, `t2` only; unknown id Err; `delete_destination` clears `t1`'s dest, the default's `dest_id`, and the row.
- [ ] **Step 2: Run** — filter `notion::tests`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** Validation copy: name 1–60 chars trimmed (`Give the database a name.`); colour 1–9 (`Pick a colour.`); `database_id` via `parse_database_id`; `data_source_id` via `parse_uuid`, else `Read the database's columns first.`; `title_prop` non-empty (`Choose the task text column.`); each fixed kind ∈ select/status/multi_select/checkbox with the R4 shape (`“{prop}” has a value Conclave can't send.`). Empty id → insert `Uuid::new_v4()`, `position = COALESCE(MAX(position),0)+1`, `created_at = stamp()`; else `UPDATE` by id (position kept; unknown id → `That Notion database is no longer in Settings.`). Both write `last_status = NULL, last_message = NULL`. `delete_destination` in one transaction: delete row; `UPDATE tasks SET dest_id = NULL WHERE dest_id = ? AND review IN ('open','keep')`; `UPDATE notion_category_defaults SET dest_id = NULL WHERE dest_id = ?`. `set_category_default`: `Some` id must exist; upsert `ON CONFLICT(template_key) DO UPDATE SET dest_id = excluded.dest_id`; when `Some`, also `UPDATE tasks SET dest_id = ?1 WHERE dest_id IS NULL AND review IN ('open','keep') AND meeting_id IN (SELECT id FROM meetings WHERE COALESCE(template_id,'') = ?2)`.
- [ ] **Step 4: Run** — filter `notion::tests`. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(notion): destination and category-default storage`

---

### Task 13: Send engine and startup recovery — judgment, risky (persistence, concurrency/restart), difficult

**Files:**
- Create: `frontend/src-tauri/src/notion/send.rs`; Modify: `notion/mod.rs` (`pub mod send;`)

**Interfaces** (`pub(crate)` in `notion::send`):
- Consumes: Task 10 `NotionClient`, `CallError`, `fake`; Task 11 `SendTask`, `page_body`, `BLOCKING_STATES`; Task 12 `list_destinations`, `set_dest_status`; `meeting_date_sql`; `stamp()`.
- Produces: `async fn send(pool, client: &NotionClient, task_ids: Option<Vec<String>>) -> Result<SendReport, String>`; `async fn recover_sending(pool) -> anyhow::Result<u64>`; `pub async fn recover_at_startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>)`; `const INTERRUPTED: &str = "Conclave closed while sending this task.";`

- [ ] **Step 1: Write the failing tests.** Each takes `SEND_TEST_LOCK` (a `static tokio::sync::Mutex<()>`; the one-send flag is process-wide). Helpers: `setup` (meeting `m1` "Weekly sync"; destination `d1` "Job tasks", `data_source_id 'ds-1'`, mapping title `Name`), `task(id, review, ambiguous)` (auto task of `m1`, `dest_id 'd1'`), `row(id) -> (review, notion_url, send_error, ambiguous)`, `ok(id)` = 200 with `id`/`url`, client timeout 500 ms.
  - `sends_kept_tasks_in_order_and_marks_them_sent`: `t1`, `t2` keep, `t3` open → `{sent:2}`; `t1` = `(sent, Some(url p1), None, false)`; 2 requests, body has `"data_source_id":"ds-1"`; `d1.last_status == "connected"`.
  - `row_is_committed_as_sending_before_the_call_and_timeout_is_ambiguous`: `Hang`; while the request is in flight `row(t1).0 == "sending"`; result `{ambiguous:1}`; then `(keep, ambiguous=true)`, error contains `may already be in Notion`.
  - `definite_failure_returns_to_keep_with_error_and_no_token`: 400 with token in message → `{failed:1}`, `(keep, false)`, error starts `Job tasks: `, names `Due`, no token.
  - `server_error_is_ambiguous`: 503 → `ambiguous 1`, flag set.
  - `ambiguous_rows_are_sent_only_when_named` (R2): ambiguous `t1`; `send(None)` → `SendReport::default()` and no request; `send(Some([t1]))` → `sent 1`, flag 0.
  - `named_ids_are_rechecked` (R2): `send(Some([open_row, no_dest_row, blocked_row, missing_id]))` with `d2.last_status='not_shared'` → `{failed:4}`, no request; `no_dest_row.send_error == "Choose a Notion database first."`; `blocked_row.send_error` starts `Personal tasks`.
  - `renamed_column_blocks_the_destination` (Review Focus 4): three keep rows on `d1`, reply 400 `"Due is not a property that exists."` → exactly 1 request, `{failed:3}`, `d1.last_status == "mapping_broken"`, rows 2–3 carry the waiting error and stay `keep`.
  - `rejected_token_stops_the_run`: 401 → 1 request; every destination `token_rejected`.
  - `outcome_not_saved_stays_sending_then_recovers` (Review Focus 1): `CREATE TRIGGER t BEFORE UPDATE OF review ON tasks WHEN NEW.review = 'sent' BEGIN SELECT RAISE(ABORT, 'disk'); END` → `send` returns `Err("Couldn't save the send result. Restart Conclave; unsent tasks will show as may already be in Notion.")`, row is `sending`; drop the trigger, `recover_sending` → 1, row `(keep, None, Some(INTERRUPTED), true)`.
  - `second_send_is_refused_while_one_runs`: second call during a `Hang` → `Err("A send is already running")`; after the first returns, a new send is `Ok`.
  - `startup_turns_sending_rows_into_may_already_be_in_notion` and `startup_hook_skips_first_launch` (`tauri::test::mock_app()`, no `AppState`: no panic).
- [ ] **Step 2: Run** — filter `notion::send`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** One-at-a-time: `static SENDING: AtomicBool`, `swap(true)` plus a `Drop` guard, as `INSTALLING`/`InstallGuard` in `updates/mod.rs:240-258`. Keep this order; it is the no-duplicate guarantee:

```text
guard = acquire SENDING or Err("A send is already running")
dests = list_destinations(pool) keyed by id            -- kept in memory, updated as outcomes arrive
ids   = task_ids, or SELECT id FROM tasks WHERE review='keep' AND send_ambiguous=0 AND dest_id IS NOT NULL
          ORDER BY created_at, rowid
for id in ids (sequential):
    t = SELECT t.id, t.text, t.at, t.due_date, t.dest_id, COALESCE(m.title, t.meeting_title) AS meeting_title,
               COALESCE(<meeting_date_sql("m")>, t.meeting_date) AS meeting_date
        FROM tasks t LEFT JOIN meetings m ON m.id = t.meeting_id WHERE t.id = ? AND t.review = 'keep'
    missing                       -> failed += 1; continue                    (not keep, sent, or unknown id)
    dest_id NULL or not in dests  -> write send_error "Choose a Notion database first."; failed += 1; continue
    dest.last_status in BLOCKING  -> write send_error dest.last_message or "{name} isn't connected. Check it in Settings → Notion.";
                                     failed += 1; continue
    body = page_body(dest.data_source_id, &dest.mapping, &t)
    claimed = UPDATE tasks SET review='sending', send_error=NULL, send_ambiguous=0 WHERE id=? AND review='keep'  -- autocommit
    rows_affected != 1 -> continue (the UI changed it meanwhile)
    match client.create_page(&body):
      Ok(p)  -> UPDATE review='sent', notion_page_id, notion_url, sent_at=stamp(), send_error=NULL WHERE id=? AND review='sending'
                sent += 1; dest status -> connected (DB + memory)
      Err(e) if e.is_ambiguous() -> UPDATE review='keep', send_ambiguous=1,
                send_error = e.message(&dest.name) + " The task may already be in Notion."; ambiguous += 1
      Err(e) -> UPDATE review='keep', send_error = e.message(&dest.name); failed += 1
                if e.state() == "token_rejected": every destination -> token_rejected (DB + memory)
                else if e.state() in BLOCKING: this destination -> e.state(), e.message(&dest.name)
    an outcome UPDATE that fails: log::error!("notion: outcome of task {id} not saved: {e}") and return
      Err("Couldn't save the send result. Restart Conclave; unsent tasks will show as may already be in Notion.")
log::info!("notion: send finished: {sent} sent, {failed} failed, {ambiguous} unsure")   -- counts only
```
`recover_sending`: `UPDATE tasks SET review='keep', send_ambiguous=1, send_error=? WHERE review='sending'` bound to `INTERRUPTED`. `recover_at_startup`: shape of `database::setup::fail_interrupted_summaries` (`database/setup.rs:75-82`).
- [ ] **Step 4: Run** — filter `notion::send`. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(notion): sequential send with sending-row commit, ambiguous outcomes and startup recovery`

---

### Task 14: Notion commands, registration, startup hook, network doc — judgment, risky (credentials)

**Files:**
- Create: `frontend/src-tauri/src/notion/commands.rs`; Modify: `notion/mod.rs` (`pub mod commands;`)
- Modify: `frontend/src-tauri/src/lib.rs` (startup hook after `fail_interrupted_summaries` :798; registration after `knowledge::tasks::tasks_mark_sent,` from Task 4)
- Modify: `docs/network-calls.md` (Hosts table after the calendar-host row; request-sites table; "Test-only clients" sentence)

**Interfaces:**
- Consumes: Tasks 10–13; Task 4 `tasks_changed`; `crate::state::AppState` (`state.db_manager.pool()`); `crate::secrets::{store, NOTION_TOKEN}`.
- Produces commands (`#[tauri::command] pub async fn … -> Result<_, String>`): `notion_get_config() -> NotionConfig`, `notion_set_token(token: String)`, `notion_remove_token()`, `notion_test() -> Vec<DestStatus>`, `notion_read_columns(link: String) -> DbSchema`, `notion_save_destination(dest: Destination) -> Destination`, `notion_delete_destination(id: String)`, `notion_set_category_default(template_key: String, dest_id: Option<String>)`, `notion_send(task_ids: Option<Vec<String>>) -> SendReport`. `notion_delete_destination`, `notion_set_category_default` and `notion_send` take `app: tauri::AppHandle` and call `tasks_changed(&app)` (send: also on `Err`, since rows may have changed). Cores (`pub(crate)`): `validate_token(raw) -> Result<String, String>`, `last4(token) -> String`, `clear_token_states(pool) -> anyhow::Result<()>`, `test_all(pool, client: Option<&NotionClient>) -> anyhow::Result<Vec<DestStatus>>`, `read_columns(client, link) -> Result<DbSchema, String>`.
- **`notion_test` shape (R6):** element 0 is the token, `dest_id: ""` (`connected` | `token_missing` | `token_rejected` | `offline`), then one entry per destination in position order. Each destination's result is persisted with `set_dest_status` except `offline` (transient).

- [ ] **Step 1: Write the failing tests:**
  - `token_is_cleaned_and_only_last4_leaves` (Review Focus 2): `validate_token("  ntn_abc123XYZ \n")`, `"Bearer ntn_abc123XYZ"` and `"bearer  ntn_abc123XYZ"` → `"ntn_abc123XYZ"`; `" "` → `Paste a token first.`; `"ntn abc"` and 201 chars → Err; `last4` → `"3XYZ"`.
  - `setting_a_token_clears_token_states`: destinations with `token_rejected` and `not_shared`; `clear_token_states` → first NULL, second unchanged.
  - `test_all_without_token_marks_everything_missing`: states `["token_missing","token_missing"]`, `s[0].dest_id == ""`.
  - `test_all_reports_rejected_token_without_database_calls`: 401 on `/users/me` → all `token_rejected`, 1 request.
  - `test_all_checks_each_database_and_mapping`: due mapped to `Deadline` → `s[1] = (d.id, "mapping_broken", "Job tasks: “Deadline” is missing")`, requests `GET /databases/{id} ` then `GET /data_sources/{ds} `, and `list_destinations()[0].last_status == Some("mapping_broken")`.
  - `test_all_maps_404_to_not_shared`: message = the Task 10 404 copy for `Personal tasks`.
  - `read_columns_reads_database_then_its_first_data_source`: link with `?v=1` → title `Job tasks`, dashed ids; `"not a link"` Err.
- [ ] **Step 2: Run** — filter `notion::commands`. Expected: compile FAIL.
- [ ] **Step 3: Implement.** `token()` reads `NOTION_TOKEN` (`Couldn't read the saved Notion token: {e}`, as `updates/mod.rs`); `last4` as `updates/mod.rs:201-204`. `validate_token`: trim, strip a leading case-insensitive `Bearer` word and trim again; non-empty (`Paste a token first.`), ≤ 200 bytes, all `is_ascii_graphic` (`That doesn't look like a Notion token.`). `notion_set_token` stores then `clear_token_states` (`UPDATE notion_destinations SET last_status=NULL, last_message=NULL WHERE last_status IN ('token_missing','token_rejected')`); save/remove errors `Couldn't save the Notion token: {e}` / `Couldn't remove the Notion token: {e}`. `test_all`: `None` → all `token_missing`, message `Add your Notion token first.`; else `me()` failing with `token_rejected`/`offline` → every entry that state, `e.message("Notion")`, no more calls; else per destination: `database` (error → `e.state()`, `e.message(&d.name)`); the stored `data_source_id` must still be in `db.data_sources` (compare via `parse_uuid`) else `not_found` `{name}: the database changed in Notion. Read its columns again in Settings.`; `data_source` → `schema_from` → `check_mapping`; problems → `mapping_broken`, `{name}: ` + problems joined `; `; else `connected`. `read_columns`: `parse_database_id` → `database` → `first_data_source` (none → `This database has no data source Conclave can use.`) → `data_source` → `schema_from`; errors `e.message("This database")`. Commands needing a token without one: test → `test_all(pool, None)`; read/send → `Err("Add your Notion token in Settings → Notion first.")`.
- [ ] **Step 4: Wire `lib.rs`.** After :798: `// A Notion send cut off by a crash or quit may already have created the page: never resend it silently.` + `tauri::async_runtime::block_on(notion::send::recover_at_startup(_app.handle()));`. Register the nine `notion::commands::…` after `knowledge::tasks::tasks_mark_sent,`.
- [ ] **Step 5: Document the host.** Hosts row: `| api.notion.com | src-tauri/src/notion/client.rs (NOTION_API; me, database, data_source, create_page), called from notion/commands.rs and notion/send.rs | Tasks → Notion: token test, reading a database's columns, creating one page per kept task (task text, due date, meeting title and date, mapped fixed values); only on the user's click (Test, Read columns, Send, Retry), with the user's own integration token from Credential Manager | Allowed (user-configured) |`. Request-sites row: `| src-tauri/src/notion/client.rs (NotionClient::new, Client::builder; redirects off) | https://api.notion.com/v1 + /users/me, /databases/{id}, /data_sources/{id}, /pages | Allowed (as above) |`. Add `notion/client.rs` `fake` to the "Test-only clients" sentence.
- [ ] **Step 6: Run** — filter `notion::`; `cd /workspace && bash scripts/check-no-phone-home.sh` → `phone-home guard: clean`; the phase gate. Expected: PASS.
- [ ] **Step 7: Commit** `notion/`, `lib.rs`, `docs/network-calls.md` — `feat(notion): settings and send commands, startup recovery hook, api.notion.com documented`

---

## Frontend (Tasks 20–28)

Every test mocks `@tauri-apps/api/core` (and `@tauri-apps/api/event` where `listen` is used) the way `tests/components/sidebar-nav.test.tsx` and `tests/components/summary-speaker-gate.test.tsx` do: keep originals, restore in `afterAll`. Copy and layout come from the mockup.

### Task 20: Queue types and pure logic — mechanical

**Files:**
- Modify: `frontend/src/lib/tasks.ts:2-14` (`TaskRow`); create `frontend/src/lib/notion.ts`, `frontend/src/lib/tasks-queue.ts`
- Test: `frontend/tests/lib/tasks-queue.test.ts`, `frontend/tests/lib/notion.test.ts`; add the new fields to the `TaskRow` factories in `tests/lib/tasks.test.ts`, `tests/lib/meeting-page.test.ts`, `tests/components/meeting-page-finished.test.tsx`, `tests/components/tasks-page.test.tsx`

**Interfaces:**
- `TaskRow` gains `review: 'open'|'keep'|'drop'|'sending'|'sent'; removedBy: 'ai'|'user'|null; destId: string|null; dueDate: string|null; notionUrl: string|null; sentAt: string|null; sendError: string|null; sendAmbiguous: boolean; templateId: string|null` (fixture defaults `'open'`, nulls, `false`).
- `lib/notion.ts`: TS mirrors of Task 11 types — `FixedKind`, `FixedValue { prop; kind: FixedKind; value: string | string[] | boolean }`, `Mapping`, `Destination { id; name; colour; position; databaseId; dataSourceId: string|null; mapping; lastStatus: DestState|null; lastMessage: string|null }`, `PropInfo`, `DbSchema`, `DestState` (7 states), `DestStatus { destId; state; message }`, `CategoryDefault`, `NotionConfig`, `SendReport`; `MAX_DESTINATIONS = 9`; `BLOCKING_STATES: DestState[]`; `DEST_STATE_LABEL` (`Connected`, `Not shared with Conclave`, `Not found`, `Token missing`, `Token rejected`, `Mapping broken`, `Offline`); `notionDbUrl(databaseId)` (`https://www.notion.so/` + id without dashes); `nextColour(dests)` (first unused 1..9, else 1); `mappingChoices(s) -> { title; dates; texts; fixed; unused }`.
- `lib/tasks-queue.ts`: `TASKS_CHANGED_EVENT = 'tasks-changed'`; `DELETED_UNKNOWN = 'Deleted meeting (title unknown)'`; `QueueGroup { key; title; meetingId; date; templateId; tasks }`; `DestLookup = { dests: Destination[]; tokenSet: boolean }`; `SendCheck = {kind:'notKept'} | {kind:'ambiguous'} | {kind:'needDb'} | {kind:'waiting'; dest; state: DestState} | {kind:'sendable'; dest}`; `SendSummary { kept; byDest: {dest; n}[]; needDb; waiting; waitingNames: string[]; undecided; sendableIds: string[] }`; functions `isToReview` (open|keep|sending), `toReviewCount`, `groupQueue`, `destOf`, `sendCheck`, `sendSummary`, `sendButtonLabel(n, sending)`, `resultLine(r) -> {sent; notSent}`, `pickFocus(before, after, wanted)`, `destForKey(key, dests)`, `dueLabel(t)`, `waitingText(name, state)`, `isEditingText(el)`.

- [ ] **Step 1: Write the failing tests** (factory `t(over)`; `job` = d1 "Job tasks" colour 1 position 0, `pers` = d2 "Personal tasks" colour 2 position 1 `lastStatus:'not_shared'`):
  - `toReviewCount` of open, keep, sending, sent, drop → 3.
  - `groupQueue` → titles `['New', 'Deleted meeting: Gone', 'Old', 'Deleted meeting (title unknown)']` (newest first by `parseDbTime` from `lib/meetings.ts`, null last), rows keep input order.
  - `sendCheck`: open → `notKept`; ambiguous keep → `ambiguous`; keep with null or deleted dest → `needDb`; keep on `pers` → `{kind:'waiting', state:'not_shared'}`; keep on `job` (`lastStatus` null) → `sendable`; `job` with `lastStatus:'offline'` → `sendable`; `tokenSet:false` → `{kind:'waiting', state:'token_missing'}`.
  - `sendSummary` → `{ kept:5, needDb:1, waiting:1, waitingNames:['Personal tasks'], undecided:1, sendableIds:['a','b'] }`, `byDest` `[['Job tasks', 2]]`.
  - Copy: `sendButtonLabel` → `Send 3 to Notion` / `Nothing to send` / `Sending…`; `resultLine({sent:2,failed:1,ambiguous:1})` → `{sent:'Sent 2 to Notion', notSent:'2 not sent, see To review'}`; zeros → nulls.
  - `pickFocus` cases `b→b`, `b removed→c`, `c removed (last)→a`, `[]→null`; `destForKey('2',[pers,job])` → d2, `'3'`/`'0'` → null.
  - `dueLabel` (Review Focus 5): `dueDate '2026-11-26'` → `by Thu 26 Nov` (date-fns `format(parseISO(d), 'EEE d MMM')`, date-only, so no zone shift); `due 'next week'` → `by next week`; neither → `no due date`.
  - `waitingText('Personal tasks','not_shared')` → `Personal tasks isn't shared with Conclave yet, so this task waits.`; `('Job tasks','mapping_broken')` → `Job tasks isn't connected (Mapping broken), so this task waits.`
  - `isEditingText`: input, textarea, contenteditable true; select, div false.
  - `notion.test.ts`: `mappingChoices` on the 8-property schema (Name title, Due date, Notes rich_text, Status status, Tags multi_select, Urgent checkbox, Owner people, Area select) → title Name, dates `[Due]`, texts `[Notes]`, fixed `[Status, Tags, Urgent, Area]`, unused `[Owner]`; `notionDbUrl('ab-cd')`; `nextColour` `[]→1`, `[1,3]→2`.
- [ ] **Step 2: Run** the two files. Expected: FAIL (modules missing).
- [ ] **Step 3: Implement.** `groupQueue` key `m:<meetingId>` or `d:<title>|<date>`; `templateId` from the first row. `sendCheck` order: not keep → notKept; ambiguous; no dest → needDb; `!tokenSet` → waiting token_missing; `lastStatus ∈ BLOCKING_STATES` → waiting; else sendable. `sendSummary.kept` counts every keep (ambiguous included); `byDest` by position; `sendableIds` never contains ambiguous rows.
- [ ] **Step 4: Run** the files, then the frontend gate. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(tasks): review-queue types and pure queue logic`

---

### Task 21: Destination colour tokens — mechanical

**Files:**
- Modify: `frontend/src/app/globals.css` (after each `--cat-5` in `:root`, `.dark`, `.theme-conclave`); `DESIGN.md` (token list, `## Chips`)
- Test: `frontend/tests/lib/db-tokens.test.ts`

**Interfaces:** CSS vars `--db-1..--db-9` (HSL triplets), used as `hsl(var(--db-${colour}))`.

| | light | dark + Conclave |
|---|---|---|
| 1 rose | `340 75% 50%` | `340 75% 58%` |
| 2 cyan | `190 90% 32%` | `190 85% 45%` |
| 3 lime | `85 65% 32%` | `85 60% 45%` |
| 4 violet | `265 65% 55%` | `265 80% 72%` |
| 5 amber | `35 90% 35%` | `38 90% 55%` |
| 6 blue | `215 85% 45%` | `215 90% 65%` |
| 7 green | `150 65% 28%` | `150 55% 50%` |
| 8 orange | `20 85% 45%` | `20 90% 62%` |
| 9 slate | `215 15% 42%` | `215 15% 65%` |

(1–3 fixed by spec §5.3; 4–9 builder's choice under R12.)

- [ ] **Step 1: Write the failing test**: parse the three blocks of `src/app/globals.css`, HSL → sRGB → WCAG contrast; `every destination colour has >= 3:1 against the background in all three themes` (asserts `[block, n, true]` so a failure names the slot) and `spec values for 1-3` (`root db-1 = 340 75% 50%`, `dark db-2 = 190 85% 45%`, `theme-conclave db-3 = 85 60% 45%`).
- [ ] **Step 2: Run** it. Expected: FAIL.
- [ ] **Step 3: Add the 27 lines;** `DESIGN.md`: `--db-1..9` in the token list, the value table, and under `## Chips`: "**Destination chip / picker**: pill, 12px, an 8px dot `hsl(var(--db-n))` with a 1px `--border` ring; dashed muted border when no destination; amber border and text when the destination is not connected. `--db-*` is only for Notion destinations, never for categories (`--cat-*`)."
- [ ] **Step 4: Run** it. Expected: PASS (a failing 4–9 value moves lightness in 5 % steps; update the table).
- [ ] **Step 5: Commit** — `feat(design): destination colour tokens --db-1..9`

---

### Task 22: Browser-preview fakes — mechanical

**Files:**
- Modify: `frontend/src/lib/browser-preview.ts` (task seed :74-89, task handlers :144-150); Test: `frontend/tests/lib/browser-preview.test.ts`

**Interfaces:** handlers for `tasks_queue_list, tasks_set_review, tasks_set_dest, tasks_set_due_date, tasks_mark_sent, tasks_edit, tasks_set_status, notion_get_config, notion_set_token, notion_remove_token, notion_test, notion_read_columns, notion_save_destination, notion_delete_destination, notion_set_category_default, notion_send`; remove the `tasks_list, tasks_claim, tasks_add, tasks_delete` fakes. `PreviewTask` becomes `TaskRow`.

Seed: destinations `d-job` "Job tasks" (colour 1, position 0), `d-personal` "Personal tasks" (2, 1, `lastStatus:'not_shared'`, `lastMessage` = the Task 10 404 copy), `d-german` "German homework" (3, 2); mapping `{ titleProp:'Name', dueProp:'Due', meetingProp:null, fixed:[{ prop:'Status', kind:'status', value:'To do' }] }`; `tokenLast4:'7Qx2'`; defaults `standard_meeting→d-job`, `interview→d-job`, `''→null`. Tasks: preview-1 "Weekly product sync" (`standard_meeting`): 2 open me `d-job` (one `dueDate:'2026-11-26'`), 1 open me `d-personal`, 1 drop/`removedBy:'ai'` unassigned, 2 sent (`notionUrl:'https://www.notion.so/preview'`, `sentAt:'2026-11-16T12:04:00Z'`); preview-5 "Candidate interview" (`interview`): keep `d-job`, keep `d-job` `sendAmbiguous:true`, keep `d-job` `sendError:"Notion did not answer in time."`; deleted meeting (`meetingId:null, meetingTitle:'Call with Max', meetingDate:'2026-11-09T19:30:00Z'`): 1 open, no dest. `notion_test` → token connected, `d-job`/`d-german` connected, `d-personal` not_shared, written into `lastStatus`. `notion_send({ taskIds })` mirrors Task 13: blocked or no-dest rows get `sendError`, count failed; others become `sent` with `sentAt` now. `tasks_set_review` applies §5.1 fields; `notion_save_destination` assigns `` `d-${n}` `` for `id ''`; `notion_read_columns` returns Task 20's 8-property schema.

- [ ] **Step 1: Write the failing tests** `the queue seed covers every review state` (`{open, keep, drop, sent}`, an ambiguous row, a `meetingId === null` row) and `send fails the unshared destination and sends the rest` (keep every open row with a dest, send them → `{ sent: n - 1, failed: 1, ambiguous: 0 }`).
- [ ] **Step 2: Run.** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** the file, then the gate. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(preview): fake review-queue and Notion commands`

---

### Task 23: Settings → Notion pane — judgment

**Files:**
- Modify: `frontend/src/lib/settings-panes.ts` (`SettingsPane` + `'notion'`; App group `{ id:'notion', label:'Notion' }` after `app`; `TAB_ALIASES` `notion:'notion'`; `SETTINGS_INDEX` entries `Integration token`, `Notion databases`, `Default database per meeting category`)
- Modify: `frontend/src/app/settings/page.tsx:86` (`case 'notion': body = <NotionSettings />; break;`)
- Create: `frontend/src/components/Settings/NotionSettings.tsx`
- Test: `frontend/tests/components/notion-settings.test.tsx`; `tests/lib/settings-panes.test.ts` (labels gain `'Notion'` last); `tests/components/settings-page.test.tsx` (stub `Settings/NotionSettings`; `tab=notion` renders h1 "Notion")

**Interfaces:**
- Consumes: `lib/notion.ts`; `SettingsRow`, `useSavedTick` (`components/Settings/SettingsRow.tsx`); `confirmAction` (`lib/confirm.ts:12`); `api_list_templates` → `{ templates: { id, name }[] }`; `open_external_url({ url })`.
- Produces: `export function NotionSettings(): JSX.Element`, reached at `/settings?tab=notion` (R10).

Layout and copy (spec §5.3, mockup screen 4; token row like `components/UpdatesSettings.tsx`):
- Intro: "Conclave creates tasks in your Notion databases only when you click Send. The token is kept in Windows Credential Manager."
- "Integration token", helper "Create an internal integration in Notion's settings, then share each database below with it (database ••• → Connections)." No token: password input + **Save** → `notion_set_token({ token })`, reload config, `markSaved`. Token set: "Token saved ••••{tokenLast4}", **Test** → `notion_test()`; the first entry (`destId === ''`, R6) shows next to the token ("Works" when connected, else its label and message); then reload config so each destination shows its persisted `lastStatus` (R1). **Remove** → `notion_remove_token()`.
- "Notion databases" (block): table Name (dot chip) / Notion database ("Open ↗" → `open_external_url({ url: notionDbUrl(databaseId) })`) / Status (`DEST_STATE_LABEL[lastStatus]`, "Not tested" when null, `lastMessage` appended when not connected; Connected in ok colour, others amber) / **Edit**, **Delete**. **Add database** disabled at `MAX_DESTINATIONS`.
- Inline Add/Edit form: Name, "Notion database link", **Read columns** → `notion_read_columns({ link })`; then "Task text → {title.name}" (read-only), "Due date" (None + `dates`), "Meeting" (None + `texts`), one select per `fixed` property (Not set + options; checkbox Not set / Checked / Unchecked; multi_select stores `[option]`), "Not used: {names}". **Save** → `notion_save_destination({ dest })` with `id: ''` for new (R3), `colour: nextColour(dests)`, `position: dests.length`, ids from the schema, `lastStatus: null, lastMessage: null`; Edit keeps id/colour/position and the stored mapping until Read columns is pressed again. Save disabled until a name and (new) a schema with a title.
- Delete: `confirmAction({ title: \`Delete ${name}?\`, body: 'Tasks waiting for it will need a database again.', confirmLabel: 'Delete', destructive: true })` → `notion_delete_destination({ id })`.
- "Default database per meeting category" (block), helper "A new task starts with this destination. You can change it on each task before sending.": a row per template plus "No template" (key `''`), select "Ask me" (`destId: null`) + destinations → `notion_set_category_default({ templateKey, destId })`.
- Every failure: the plain error string in a `role="alert"` line by the control.

- [ ] **Step 1: Write the failing tests** (react-test-renderer, `invoke` mock with a `calls` log, as `tests/components/updates-settings-token.test.tsx`):
  - saving a token → `['notion_set_token', { token: 'ntn_secret7Qx2' }]`, text `Token saved ••••7Qx2`.
  - Test → token line `Works`; after the reload the table shows `Not shared with Conclave` for `d-personal`; the `destId ''` entry is not rendered as a table row.
  - Add database: Read columns, choose Due and Status=To do, Save → `['notion_save_destination', { dest: { id: '', name: 'Job tasks', colour: 3, position: 2, databaseId: 'abc-def', dataSourceId: 'ds1', mapping: { titleProp: 'Name', dueProp: 'Due', meetingProp: null, fixed: [{ prop: 'Status', kind: 'status', value: 'To do' }] }, lastStatus: null, lastMessage: null } }]`; Due options `['None', 'Due']`.
  - Add database disabled with nine destinations; a read-columns failure shows its text in the alert; "Ask me" for No template → `['notion_set_category_default', { templateKey: '', destId: null }]`; Delete confirms first (`confirmAction` mocked false then true → exactly one `notion_delete_destination`).
- [ ] **Step 2: Run** the three files. Expected: FAIL. **Step 3: Implement.** **Step 4: Run**, then the gate. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(settings): Notion pane with token, destinations, mapping and category defaults`

---

### Task 24: Destination picker and meeting-page panel on `review` — judgment

**Files:**
- Create: `frontend/src/components/Tasks/DestPicker.tsx`
- Modify: `frontend/src/lib/meeting-page.ts:14-35` (`splitTasks`), `frontend/src/components/MeetingPage/MeetingTasksPanel.tsx` (load :21-27, keep/remove :51-58, rows, heading)
- Test: `frontend/tests/lib/meeting-page.test.ts`, `frontend/tests/components/meeting-page-finished.test.tsx`, new `frontend/tests/components/dest-picker.test.tsx`

**Interfaces:**
- `export function DestPicker(p: { task: TaskRow; dests: Destination[]; disabled?: boolean; onPick(destId: string | null): void }): JSX.Element` — mockup `.dest`: an 8px `aria-hidden` dot `hsl(var(--db-${colour}))` and a native `<select aria-label={\`Notion database for: ${task.text}\`}>` in position order; no or deleted dest → first option `""` "Choose database…", dashed muted border, no dot; selected dest's `lastStatus ∈ BLOCKING_STATES` → amber border and text. `onChange` → `onPick(value || null)`.
- `splitTasks(tasks, meetingId) -> { kept, removed }`: removed = review `drop`; kept = every other review of this meeting.

Panel (spec §5.7): load `tasks_queue_list()` and `notion_get_config()` (config failure → no destinations, silent). Keep → step `['tasks_set_review', { id, review:'keep' }, { review:'keep', owner:'me' }]` (no-op for keep/sending/sent); Remove → `['tasks_set_review', { id, review:'drop' }, { review:'drop', removedBy:'user' }]` (no-op for drop/sending/sent); both through the existing `edit` rollback helper (Rust emits `tasks-changed`; the panel does not dispatch anything). Keep rows show `DestPicker` (`onPick` → `edit(task, [['tasks_set_dest', { id, destId }, { destId }]])`); `sending` shows "Sending…", `sent` shows "Sent to Notion" and no Keep/Remove. `aria-pressed`: Keep for keep/sending/sent, Remove for drop. Removed line `removed by AI` / `removed`. Heading count and `onCountChange` = kept rows. No Send button.

- [ ] **Step 1: Write the failing tests:** `splitTasks` (drop → removed whatever the owner; open/keep/sending/sent → kept; other meetings ignored). Panel (mock answers `tasks_queue_list` and two destinations): an AI-removed task sits under Removed and `k` → `['tasks_set_review', { id:'u1', review:'keep' }]`; `x` removes an open task, a failure toasts and rolls back; a kept task's picker → `['tasks_set_dest', { id:'t1', destId:'d2' }]`; a sent task has no Keep/Remove; an open task has neither pressed. Keep "Done reviewing", "a failed edit rolls back only its own task" (now `tasks_set_review`) and "every task shows Keep and Remove with K and X hints"; delete "a restore whose status step fails keeps the saved claim". `dest-picker.test.tsx`: no destination → "Choose database…" selected, no dot; `lastStatus:'not_shared'` → amber class; choosing "" → `onPick(null)`.
- [ ] **Step 2: Run** the three files. Expected: FAIL. **Step 3: Implement.** **Step 4: Run**, then the gate. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(meeting): tasks panel uses the review state and the destination picker`

---

### Task 25: Tasks page review queue (tabs, groups, rows, keyboard, focus) — judgment, difficult

**Files:**
- Create: `frontend/src/components/Tasks/TasksQueue.tsx`, `frontend/src/components/Tasks/ReviewRow.tsx`
- Modify: `frontend/src/app/tasks/page.tsx` (`<TasksQueue />` directly in `PageFrame variant="list" title="Tasks"`, no overflow wrapper)
- Delete: `frontend/src/components/Tasks/TaskList.tsx`, `frontend/tests/components/tasks-page.test.tsx`, `groupByMeeting`/`TaskGroup`/`MANUAL_GROUP_TITLE` in `frontend/src/lib/tasks.ts` and `frontend/tests/lib/tasks.test.ts` (first `grep -rn "groupByMeeting\|MANUAL_GROUP_TITLE\|TaskList\b" frontend/src frontend/tests` → only `TaskList` hits)
- Test: `frontend/tests/components/tasks-queue.test.tsx`

**Interfaces:**
- Consumes: Task 20 functions; `taskKeyAction` (`lib/meeting-page.ts:46`); `DestPicker`; `formatRowDate` (`lib/dates.ts:6`); `parseDbTime` (`lib/meetings.ts:33`); `meetingHref` (`lib/meeting-target.ts:15`); `rememberOrigin` (`lib/back-nav.ts:8`, `{ path:'/tasks', label:'Tasks' }`); `open_external_url({ url })` (https only).
- Produces: `export function TasksQueue(): JSX.Element` with `reload(): Promise<void>` and the To review panel Task 26 extends; rows carry `data-row-id={task.id}`, `tabIndex={0}`; To review tab button `id="tab-review"`.

Structure (spec §5.6, mockup screens 1–3):
- Fragment: a scroll area `<div className="min-h-0 flex-1 overflow-y-auto">` (subtitle "Tasks the AI found in your meetings. Keep the ones you want, then send them to Notion.", tablist, panels) and, outside it, the send-bar slot (Task 26).
- Mount: `tasks_queue_list()`, `notion_get_config()`, `api_list_templates()`; `reload()` re-reads tasks and config (statuses can change after a send). Load failure: `role="alert"` "Could not load tasks.".
- Tabs `role="tab"`/`aria-selected`/`aria-controls`: "To review {toReviewCount||''}", "Sent to Notion {n}", "Removed {n}"; panels `role="tabpanel"`.
- **To review**: hint strip "K keep · X remove · 1 {dest 1} · 2 … · ↑↓ move · Ctrl+Enter send · Click a task's text to edit it". Groups from `groupQueue` holding any `isToReview` row. Header: title `Link` to `meetingHref({ id })` with `rememberOrigin` (plain text for deleted meetings), `formatRowDate(parseDbTime(date))`, category chip as `components/MeetingsTable/cells.tsx:62-64` ("No template" grey), "Keep all mine" when the group has open rows owned by me (keeps each in sequence, reload, focus the group's first row). Drop rows fold into "{n} removed{ (AI thinks they aren't yours) if any removedBy==='ai'} · Show/Hide". Empty: "All reviewed" / "New tasks appear here after each meeting is summarized."
- **ReviewRow**: `role="group"`, `aria-label` = text + ", kept"/", removed"; Keep ✓ / Remove ✕ buttons `aria-label="Keep: {text}"` / `"Remove: {text}"`, `aria-pressed`; text button → `Input` (Enter or blur saves via `tasks_edit({ id, text })` when changed and non-blank; Escape cancels; focus back to the row); who line "You" or "Unassigned · AI isn't sure it's yours", then the due button `dueLabel(task)` (`aria-label="Due date: {label}. Change"`) → `<input type="date">` (change → `tasks_set_due_date({ id, dueDate: value || null })`; Escape/blur closes); drop rows add " · removed by AI"/" · removed by you"; `DestPicker` on non-drop rows (`tasks_set_dest`); when `sendCheck` (evaluated as if kept, for open rows too) is `waiting`: `waitingText(dest.name, state)` + Link "Fix in Settings" to `/settings?tab=notion`.
- **Actions** — one `act(command, args, wanted)`: `before` = `[data-row-id]` ids in DOM order; invoke; failure → `toast.error('Could not save that change.')`; `reload()`; after re-render focus `pickFocus(before, after, wanted)`, `null` → `#tab-review`. Keep/Remove clicks toggle (pressing the pressed one sets `open`), wanted = same row. Restore → `tasks_set_review({ id, review:'open' })`.
- **Row keys** (only when `e.target === e.currentTarget`): `taskKeyAction` keep/remove → that review, wanted = next row in `before` (same when last); up/down move focus (preventDefault); `destForKey` on a non-drop row → `tasks_set_dest`, focus stays.
- **Sent to Notion**: `groupQueue(sent)`; rows: text, who, dest chip, "Sent {format(parseDbTime(sentAt),'EEE d MMM, HH:mm')}" (ok colour), "Open in Notion ↗" → `open_external_url({ url: notionUrl })` (hidden when null). Footnote "Conclave doesn't follow tasks once they are in Notion. Tick them off there."
- **Removed**: flat list, **Restore**, line "{group title} · {who} · removed by AI | removed by you" (no suffix when `removedBy` null). Empty "Nothing removed." Footnote "Removed tasks are never sent. They are deleted with their meeting's recording."

- [ ] **Step 1: Write the failing tests** (real DOM via `createRoot` + `act`, as `tests/components/meetings-row-menu.test.tsx`; mocks: `@tauri-apps/api/core` with an in-memory `rows` store applying the four task writes, `sonner`, `next/link`). Seed: m2 (newer) open `a`, `b`; m1 open `c`, drop `z` (`removedBy:'ai'`); deleted meeting `d`; sent `s`.
  - tabs and groups: To review shows `4`; group titles `['M2', 'M1', 'Deleted meeting: Gone']`.
  - `K` on `a` → `['tasks_set_review', { id:'a', review:'keep' }]`, focus `b`; `X` on `b` folds it, focus `c`; `X` on the last row `d` → focus `c`; clicking Keep on a kept row → `open`, focus stays.
  - `2` on `a` → `['tasks_set_dest', { id:'a', destId:'d2' }]`, focus stays; keys typed in the text input → no `tasks_set_review`.
  - Enter saves `['tasks_edit', { id:'a', text:'New text' }]`; due date set then cleared → `{ dueDate:'2026-11-26' }` then `{ dueDate:null }`.
  - Keep all mine; fold line `1 removed (AI thinks they aren't yours) · Show`; a failed change toasts and `document.activeElement !== document.body`; Restore → `{ id:'z', review:'open' }`; Open in Notion → `['open_external_url', { url:'https://www.notion.so/p1' }]`; empty queue → `All reviewed`.
- [ ] **Step 2: Run** the file. Expected: FAIL. **Step 3: Implement** (page change and deletions included). **Step 4: Run**, then the gate. Expected: PASS; `grep -rn "tasks_add\|tasks_delete\|tasks_claim\|'tasks_list'" frontend/src` → hits only in `lib/browser-preview.ts` comments, if any.
- [ ] **Step 5: Commit** (`git add -A frontend/src/app/tasks frontend/src/components/Tasks frontend/src/lib/tasks.ts frontend/tests`) — `feat(tasks): review queue page with tabs, groups and keyboard review`

---

### Task 26: Send bar, Ctrl+Enter, Retry and "May already be in Notion" — judgment, risky

**Files:**
- Create: `frontend/src/components/Tasks/SendBar.tsx`; Modify: `TasksQueue.tsx` (send state, Ctrl+Enter, bar slot), `ReviewRow.tsx` (error / ambiguous line)
- Test: `frontend/tests/components/tasks-queue.test.tsx` (`describe('sending')`)

**Interfaces:**
- `export function SendBar(p: { summary: SendSummary; sending: boolean; result: { sent: string | null; notSent: string | null; error?: string } | null; onSend(): void; onView(): void }): JSX.Element`.

Behaviour (spec §5.5):
- Bar `border-t bg-card`, rendered only on To review, a sibling after the scroll area. Summary "**{kept}** kept", "{name} **{n}**" per destination, amber "{needDb} need a database", amber "{waiting} waiting for {names}", "{undecided} not decided". Status `role="status" aria-live="polite"`: "Sent N to Notion" + "View" (→ Sent tab), "M not sent, see To review", or the rejection text; cleared after 8 s. Button `sendButtonLabel(sendableIds.length, sending)`, disabled while sending or at 0.
- `send(ids)`: return when sending or empty; `sending=true`; `invoke<SendReport>('notion_send', { taskIds: ids })` — always explicit ids (R2), bulk = `summary.sendableIds`; result `resultLine(r)` or `{ error: String(e) }`; `finally`: `sending=false`, `reload()`, focus the first `[data-row-id]` or `#tab-review`. The UI never marks a row sent itself.
- Ctrl+Enter: `window` keydown while To review is shown: `e.ctrlKey && e.key === 'Enter' && !isEditingText(e.target)` → `preventDefault`, `send(summary.sendableIds)`.
- Row line (replaces the waiting line): `sendAmbiguous` → amber "May already be in Notion. Check the database before sending again." + **Open Notion database** (`notionDbUrl(dest.databaseId)`, hidden without dest) · **Mark as sent** (`tasks_mark_sent({ id })` via `act`) · **Send again** (`send([id])`); else `sendError` → destructive "Not sent: {sendError}" + **Retry** (`send([id])`). Send-type buttons disabled while sending.

- [ ] **Step 1: Write the failing tests** (`notion_send` mock resolves a configurable report or rejects; a gate promise holds a send in flight):
  - bar counts `3 kept`, `1 need a database`, `1 waiting for Personal tasks` (dest `d2.lastStatus:'not_shared'`).
  - Send → `['notion_send', { taskIds: ['a'] }]`, status `Sent 1 to Notion` + `View`.
  - while in flight: button `Sending…` and disabled; a second click and Ctrl+Enter add no call (`notion_send` called once).
  - `Nothing to send` disabled when none sendable; Ctrl+Enter from a row sends, from the text input does not.
  - failed report → `1 not sent, see To review`, row "Not sent: …" with Retry → `['notion_send', { taskIds: ['b'] }]`.
  - rejected send → status `A send is already running`; no row shows "Sent".
  - ambiguous row: `['open_external_url', { url: 'https://www.notion.so/abcdef' }]`, `['tasks_mark_sent', { id: 'm' }]`, `['notion_send', { taskIds: ['m'] }]`, and `m` is not in the bulk send.
  - the bar is absent on Sent and Removed; after a send focus is on a row or `#tab-review`.
- [ ] **Step 2: Run** the file. Expected: new tests FAIL. **Step 3: Implement.** **Step 4: Run**, then the gate. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(tasks): send bar, Retry and may-already-be-in-Notion actions`

---

### Task 27: Nav badge = To review count — mechanical

**Files:**
- Modify: `frontend/src/components/Sidebar/index.tsx` (count load near the `useEffect` at :34, `row` gains `count`, pass it for `/tasks`)
- Test: `frontend/tests/components/sidebar-nav.test.tsx`

**Interfaces:** consumes `toReviewCount`, `TASKS_CHANGED_EVENT`, `tasks_queue_list`; `listen` from `@tauri-apps/api/event` (R10: Rust emits after every task write, a send and an extraction).

Behaviour: `toReview = toReviewCount((await invoke<TaskRow[]>('tasks_queue_list')) ?? [])`, re-read on mount, on each `pathname` change and on the `tasks-changed` event (unlisten on unmount); a failure keeps the last value and logs `'tasks_queue_list failed'`. `count > 0`: `aria-label` `` `${label}, ${count} to review` ``, expanded pill `ml-auto rounded-full bg-primary px-[7px] text-xs font-bold text-primary-foreground` (`aria-hidden`), collapsed the existing dot style. Zero → `aria-label="Tasks"`.

- [ ] **Step 1: Write the failing tests** (mock `tasks_queue_list` → `queue`, default `null`; mock `listen` to capture the handler): `[open, keep, sent, drop]` → a button labelled `Tasks, 2 to review`; set `queue = []`, call the captured `tasks-changed` handler → `aria-label` `Tasks`. Existing tests stay green.
- [ ] **Step 2: Run.** Expected: FAIL. **Step 3: Implement.** **Step 4: Run**, then the gate. Expected: PASS.
- [ ] **Step 5: Commit** — `feat(nav): Tasks badge shows the To review count`

---

### Task 28: UI check in the browser preview — judgment

**Files:** only files from Tasks 21–27 that the findings touch.

- [ ] **Step 1:** `cd /workspace/frontend && pnpm run dev:preview` in the background (port 3118; no other build meanwhile).
- [ ] **Step 2: a11y** — `node /workspace/frontend/scripts/a11y-check.mjs http://localhost:3118 /tasks /settings?tab=notion /meeting-details?id=preview-1` → exit 0; repeat with the Sent and Removed tabs opened via Playwright.
- [ ] **Step 3: Screenshots** (Playwright MCP, 1280×800; `document.documentElement.className = '' | 'dark' | 'dark theme-conclave'`): `/tasks` To review (error and ambiguous rows), after Send, Sent, Removed, empty state, `/settings?tab=notion` after Read columns, the meeting panel. Compare with mockup screens 1–4: copy, order, send bar outside the scroll area, dot colours, amber waiting text.
- [ ] **Step 4: Colour-vision check (R12)** — a scratchpad Node script (not committed) reads `--db-1..9` and `--cat-1..5` from `globals.css` for each theme, applies the Machado 2009 deuteranopia and protanopia matrices (severity 1.0) in linear sRGB, and prints the smallest CIE76 ΔE between any two `--db` colours and between any `--db` and `--cat`. Pass: every pair ≥ 10. A failing 4–9 slot changes hue (keep Task 21's contrast test green) and the table in `DESIGN.md`.
- [ ] **Step 5: Keyboard pass**: Tab into the first row, K K X, 1/2/3, ↑↓, click text and type K (nothing), Ctrl+Enter from a row (sends) and in the input (does not); after each, `document.activeElement` is never `body`.
- [ ] **Step 6:** `/impeccable audit` on `components/Tasks/`, `Settings/NotionSettings.tsx`, `MeetingPage/MeetingTasksPanel.tsx`; fix what breaks `DESIGN.md` or the mockup, list the rest.
- [ ] **Step 7:** frontend gate, stop the dev server. Expected: PASS. Commit only if anything changed (`git add frontend/src DESIGN.md`) — `fix(tasks): UI check findings for the review queue`

---

### Task 29: Remove the old task commands (R11) — mechanical

**Files:**
- Modify: `frontend/src-tauri/src/knowledge/tasks.rs` (delete commands `tasks_list` :292, `tasks_claim` :324, `tasks_add` :331, `tasks_delete` :338, the pool fns `claim` :233, `add` :242, `remove` :272, and their tests)
- Modify: `frontend/src-tauri/src/lib.rs` (drop the four registrations at :1106-1111)

Keep `tasks_edit` (queue text edit), `tasks_set_status` (`components/Briefs/BriefView.tsx:68`), `list` (`knowledge/briefs.rs:284`) and `set_status` (`briefs.rs:693`).

- [ ] **Step 1: Confirm no caller** — `grep -rn "tasks_list\|tasks_claim\|tasks_add\|tasks_delete" frontend/src frontend/tests` → no hits; `grep -rn "tasks::add\|tasks::claim\|tasks::remove" frontend/src-tauri/src` → no hits.
- [ ] **Step 2: Delete** the items above.
- [ ] **Step 3: Run** the phase gate, the frontend gate and `bash scripts/check-no-phone-home.sh`. Expected: PASS, `phone-home guard: clean`.
- [ ] **Step 4: Commit** `tasks.rs`, `lib.rs` — `refactor(tasks): drop the old list/claim/add/delete commands`

---

## Self-review notes

- Spec coverage: §5.1 → Tasks 3, 4, 5; §5.2 → 1; §5.3 → 11, 12, 14, 21, 23; §5.3a → 2, 4, 25; §5.4 → 11; §5.5 → 10, 13, 26; §5.6 → 25, 26, 27; §5.7 → 24; §6 testing → per task plus 28; §3 network doc → 14.
- Names reconciled across the three parts: `Destination.lastStatus/lastMessage` replace Part 3's in-memory `rememberDestStatus`/`lastDestStatus` and `DestLookup.status`; `DestPicker` drops its `state` prop; Rust emits `tasks-changed` and the frontend listens with Tauri `listen` instead of window events; 400 maps to `mapping_broken` and 404/429 copy follows R5; Notion commands register after `tasks_mark_sent` (Task 29 removes `tasks_delete`); the preview keeps a `tasks_set_status` fake for `BriefView`.
