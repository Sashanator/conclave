# Conclave Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The meeting becomes the unit of work. A Notion-style meetings table at `/` replaces the record screen. A meeting exists before its call (by hand or from a 📞 calendar event), holds its prep, and is where recording happens. One right-hand AI sidebar (Ctrl J) holds Ask and Copilot. "What's new" replaces About. The update runs on existing data with no AI calls.

**Architecture:**
- **Data:** one migration (`20261116000001_redesign.sql`) adds meeting columns, rebuilds `meeting_preps` with a `meeting_id`, adds `meeting_views` and `dismissed_occurrences`, and does the one-time row fixes in SQL. A repeatable startup backfill (`redesign_backfill::run`) fills gists and gives every prep a meeting row. One meeting date: `COALESCE(started_at, calendar_start, created_at)`.
- **Recording:** every start resolves a meeting row through `recording::route` (`pick_target` → `resolve_target`). The id travels start → `RECORDING_TARGET` static → event payloads → `api_save_transcript`, which writes into that row or falls back to a new one (`fallback: true`). Recovery stores the id in IndexedDB.
- **Calendar:** after each successful refresh the knowledge scheduler runs a pure `reconcile::plan` over the expanded 📞 events and an executor whose SQL re-checks each condition in the same statement.
- **Frontend:** `lib/meetings.ts` + `@tanstack/react-table` table at `/`; a three-state meeting page (New, Recording, Finished); an `AiSidebar` shell whose body follows the screen; a single left nav; a build-time bundled `CHANGELOG.md` on `/whats-new`.

**Tech Stack:** Rust (Tauri 2, sqlx/SQLite), Next.js 14 static export + React 18 + TS, shadcn/Tailwind, `@tanstack/react-table@8.21.3` (new), `@tanstack/react-virtual`, `cmdk`, `date-fns`, Bun tests, Playwright MCP, `/impeccable`.

**Spec:** `docs/superpowers/specs/2026-11-16-conclave-redesign-design.md` (cited as §x.y). Mockups: `docs/superpowers/specs/2026-11-16-redesign-mockups/`. Read the section each task cites. Branch `redesign`, release **v1.3.0**.

## Global Constraints

- **Branch `redesign`:** one worktree per lane (`/workspace/.worktrees/rd-<lane>`), merged at phase ends. Rust paths are under `frontend/src-tauri/src/`, TS paths under `frontend/` unless written in full. Line numbers are from `7d99a857`; files edited by earlier tasks drift, so find the cited anchor text.
- **Commit step** (last step of every task): `git add <the task's files>` and `git commit -m "<message>"` as **separate** Bash calls. Never stage `.claude/`, `.devcontainer/` or `.env*`. Every message ends with a blank line and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Gates** (tasks name them):
  - **RUST:** `. "$HOME/.cargo/env"; export CARGO_TARGET_DIR=/workspace/target; cd <worktree> && flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib <filter> -j 4`. Each worktree needs `frontend/out` (`pnpm run build`) and `mkdir -p frontend/src-tauri/binaries && touch frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu`. `cargo check -p meetily --tests` (same wrapper) after signature changes.
  - **FE:** `cd <worktree>/frontend && pnpm exec tsc --noEmit && bun test <file>`, then the full `bun test` and `pnpm run build`.
  - **A11Y:** `pnpm run dev:preview` in the background, then `node frontend/scripts/a11y-check.mjs http://localhost:3118 / /meeting-details?id=preview-1` exits 0.
  - **GUARD:** `bash scripts/check-no-phone-home.sh`.
  - **PHASE:** `flock … cargo test --workspace --exclude llama-helper -j 4`, full FE, GUARD; then `harness:reviewer` phase review with a Codex pass.
- **Risky tasks** get a per-task review with a Codex pass. Deletion, persistence, I/O and background tasks: the reviewer looks hard for silent failures (an error must reach an `Err`, a toast or Settings, never only the log).
- **Content safety (§4):** calendar sync, migration and failed saves never delete a recording, transcript, note, prep, brief or summary. Only "Delete meeting" removes prep and notes.
- **UI tasks follow `DESIGN.md`** (Task 0). No new colours or fonts. Tests mock `invoke` as `tests/components/tasks-page.test.tsx:1-15` does and restore modules in `afterAll`. Date tests build inputs with local `new Date(y, m, d, h, min)`.
- **Pinned names** (use exactly):
  - Migration `migrations/20261116000001_redesign.sql`. Columns `template_id, reviewed, gist, started_at, ended_at, calendar_end, calendar_original_start, calendar_marker, calendar_cancelled_at`. Tables `meeting_views(id TEXT PRIMARY KEY, name TEXT, position INTEGER, builtin INTEGER NOT NULL DEFAULT 0, config_json TEXT NOT NULL)` seeded `all`/`week`/`unreviewed`; `dismissed_occurrences(calendar_uid, original_start, dismissed_at, PRIMARY KEY(calendar_uid, original_start))`.
  - **Backfill split:** the one-time fixes (`reviewed=1`, `template_id`, `started_at`/`ended_at`) are SQL inside the migration. `database::redesign_backfill::run(pool) -> anyhow::Result<()>` does only the repeatable parts: gists (`gist IS NULL`) and prep linking (`meeting_id IS NULL`).
  - `meeting_date_sql(alias) -> String`; `default_meeting_title(now_local: NaiveDateTime) -> String` ("Untitled · Mon 14:02") and `MeetingsRepository::create_meeting(pool: &SqlitePool, title: Option<String>, now_local: NaiveDateTime) -> Result<String, sqlx::Error>`, both in `database/repositories/meeting.rs`. `pub(crate) const UNTOUCHED_SQL: &str` (alias `m`) in the same file (Task 10).
  - `MeetingRow { id, title, title_source, template_id, emoji, reviewed, gist, started_at, ended_at, created_at, calendar_uid, calendar_start, calendar_end, calendar_cancelled_at, has_transcript, has_summary, prep_points }` (serde camelCase).
  - Commands: `api_list_meeting_rows`, `api_create_meeting(title?)`, `api_set_meeting_reviewed`, `api_set_meeting_template`, `api_list_meeting_views`, `api_save_meeting_view`, `api_delete_meeting_view`, `api_delete_recording`, `api_delete_meeting` (changed), `api_route_recording_start(source)`, `ask_meetings(…, meeting_ids)`, `brief_for_meeting`, `brief_regenerate`, `brief_meeting`, `open_log_file`.
  - **Kept-task rule** (both deletes): a task survives with `meeting_id` cleared iff `status <> 'dismissed' AND (owner = 'me' OR edited = 1)`.
  - Live target: Rust `recording_target_id() -> Option<String>`; frontend **`liveMeetingId: string | null`** from `useRecordingState()` (fed by `get_recording_state.meeting_id` and the `recording-started`/`recording-target` events). There is no other name for it.
  - `StartSource { Sidebar, Popup, Tray, AutoDetect, Page(String) }` (camelCase serde), `RouteTarget { Existing(String), New { title: Option<String> } }`, `RoutedMeeting { meetingId, title }`.
  - Routes: `/` table, `/meeting-details?id=` page (`tab=summary|transcript|notes|prep`, `t=` seconds), `/digests`, `/whats-new`, `/settings?tab=knowledge|copilot`. `/record` is temporary (Task 16 → removed in Task 51). `/ask`, `/briefs` and `/prep` are removed.
- **Routing rule:** the sidebar "Record new meeting" always creates a new meeting (§6.2). Pop-up, tray and auto-detect follow the 📞 rule (Task 47). A page records into its own meeting.
- **Calendar failure cadence:** a failed fetch keeps today's cadence (next refresh in 30 min). A failed reconcile retries on the next tick from the in-memory result of the last successful fetch, without refetching.
- **Deferred (not in this plan):** the silent-system-audio warning on the Audio line (§6.4 "New"). The Audio line ships with microphone, system audio, language and the existing mic-following line.

## Review Focus

1. **A recording lands in the wrong row, merges two recordings, or is lost.** Task 10 `save_into_meeting_with_transcripts_never_merges`, `save_into_missing_meeting_falls_back_to_new`, `continue_releases_fresh_routed_row` (both branches); Task 47 `route_cases` (`overlap_fresh_beats_closer_finished`, `finished → part 2`).
2. **The calendar reconcile deletes or cancels content.** Task 43 `concurrent_prep_save_wins_over_delete`, `started_at_written_after_snapshot_blocks_all`; Task 42 `incomplete_never_cancels_or_deletes`, `outside_window_never_cancelled`, `marker_change_leaves_old_rows`.
3. **Deletes take the wrong things.** Task 8 `kept_tasks_survive_both_deletes` (incl. the X-removed `dismissed, edited=1` task that must go) and `delete_recording_keeps_meeting_prep_notes`; Task 11 `delete_refuses_live_target`; Tasks 23 and 36 confirm-before-delete tests.
4. **Migration or backfill loses or duplicates prep data.** Task 1 `redesign_migration_on_previous_schema` (every prep column equal, `sqlite_sequence` kept); Task 2 `backfill_shared_key_links_once`, `backfill_is_idempotent`.
5. **The live meeting id is lost on a remount**, so the table, nav and page show a recording meeting as New, or recovery saves into the wrong row. Task 13 `liveMeetingId survives a remount` test; Task 14 recovery tests; Task 34 Recording-view test.

## Parallel lanes

At most 8 agents at once (reviewers count). Only one cargo runs at a time (the `flock` wrapper); waiting on the lock is fine.
- **Lane R (Rust, sequential):** Tasks 1–12 and 15 (Phase 1, Phase 2 Rust). After Phase 2 merges: 27, 29, 31, then **39–47** (E backend), then 52. These run **in parallel with Phase 3 and the Phase 4 UI**. Task 31 (prep by meeting id) must land before Task 43. Task 47 edits `recording/route.rs` (single owner after Phase 2).
- **Lane B-FE:** Tasks 13 and 14 after Task 11 merges (they mock `invoke`).
- **Lane C:** Tasks 16–25 in order after Phase 2. Task 19 has a Rust half: it takes the cargo lock like any task.
- **Lane D-UI:** Task 26 any time after Phase 2; 30 after 31 and 27 rows exist in tests (mocks only); 28 and 32–38 after Lane C merges (they import `lib/meetings.ts`, `MeetingsTable`, `meeting-target.ts`).
- **Lane E-UI:** 48 after 44; 49 after 35 and 46.
- **Phase 6 (50–56)** after everything merges.
- **Single owners:** `browser-preview.ts`: never edited by two worktrees at once (Lane C owns it during Phase 3; D/E UI add mocks after). `Sidebar/index.tsx`: 28 and 49 retarget links only, 50 rewrites it. `layout.tsx`: 26, 32, 33, 51 in that order. `RecordingStateContext.tsx`: 13 only.

## Phase 0: Design system

### Task 0: `DESIGN.md` from the current tokens and the approved mockups
**Agent:** implementer · **Risky:** no (docs only) · §6.9
**Files:** create `DESIGN.md` (repo root) and `.impeccable/design.json`. Sources: `frontend/src/app/globals.css` (`:root`, `.dark`, `.theme-conclave`), `frontend/tailwind.config.*`, the mockups.
- [ ] Run `/impeccable document` in scan mode. Cover tokens, the three themes (Conclave default), type scale (Source Sans 3), table density (comfortable 40 px, compact 32 px rows), chips (category, state, Calendar badge), the 360 px AI sidebar, the left nav, `kbd` hints.
- [ ] Check every colour token in `DESIGN.md` exists in `globals.css`; nothing invented.
- [ ] Gates: `node -e 'JSON.parse(require("fs").readFileSync(".impeccable/design.json"))'`, GUARD.
- [ ] Commit `docs(design): DESIGN.md from globals.css and the redesign mockups`.

## Phase 1: Data model (§6.1, §6.3 emoji, gist, §6.4 delete backend)

Rust tests use `repositories::test_pool()` (`database/repositories/mod.rs:10-18`). The "previous schema" helper is the `Migrator` filter pattern of `existing_rows_migrate_as_user` (`meeting.rs:660-685`).

Decisions (verified): `ended_at = created_at` (not `updated_at`, which renames and auto-titles also bump; `transcript.rs:193`, `meeting.rs:251,275`). `started_at = created_at − MAX(audio_end_time)`, else `created_at`. The backfill runs inside `DatabaseManager::new` right after `sqlx::migrate!` (`manager.rs:38`), which all three open paths reach (`setup.rs:62`, `database/commands.rs:129,181`), after the pre-upgrade backup (`lib.rs:655` before `:753`). A failed backfill is `log::error!` and not fatal (next start retries); this is a deliberate exception to the Global Constraint that errors reach an `Err`, toast or Settings (no user action fixes it, and the old data stays readable).

### Task 1: Migration `20261116000001_redesign.sql`
**Agent:** implementer · **Risky:** yes (rebuilds `meeting_preps`, rewrites every meeting row)
**Files:** create `migrations/20261116000001_redesign.sql`; tests in a new `mod redesign_migration_tests` in `database/repositories/meeting.rs` with `const REDESIGN_VERSION: i64 = 20261116000001;`.
**Interfaces:** the pinned columns and tables; seeds `('all','All meetings',0,1,'{}')`, `('week','This week',1,1,'{}')`, `('unreviewed','Not reviewed',2,1,'{}')` (Task 18 fills defaults for `'{}'`); `CREATE INDEX idx_meetings_calendar ON meetings(calendar_uid, calendar_start)`.
- [ ] Failing test `redesign_migration_on_previous_schema`. Migrate below `REDESIGN_VERSION`, insert:
  - `old`: `created_at '2026-11-01T10:00:00+00:00'`, transcripts with `audio_end_time` 1200.0 and 2700.0, `summary_processes.result` `{"markdown":"## Summary\nWe agreed. More.","template_id":"interview"}`;
  - `legacy`: `created_at '2026-10-05 12:00:00'`, one transcript with `audio_end_time` NULL, `result 'not json'`;
  - `cal`: `calendar_uid 'u1'`, `calendar_start '2026-11-20T09:00:00+00:00'`;
  - preps `p1 ('u1', …20T09…)` and `p2 ('u2','2026-11-21T09:00:00+00:00')` with material, `chat_json`, `items_json`, `approved_at`; insert and delete a third prep so `sqlite_sequence` is above the max id.

  Run all migrations, assert: `reviewed=1` everywhere; `old.template_id='interview'`, `legacy.template_id IS NULL`; `old.ended_at = old.created_at`; `old.started_at` parses to `2026-11-01T09:15:00Z`; `legacy.started_at = legacy.created_at`; every column of `p1`/`p2` unchanged and `meeting_id IS NULL`; a new prep id > the old `sqlite_sequence`; two preps with NULL `meeting_id` and NULL event columns insert, a duplicate `meeting_id` fails; three view seeds with `builtin=1`; duplicate `(calendar_uid, original_start)` refused.
- [ ] **RUST** `redesign_migration` → FAIL.
- [ ] Write the SQL in order: (a) nine `ALTER TABLE meetings ADD COLUMN` (`reviewed INTEGER NOT NULL DEFAULT 0`, the rest TEXT NULL); (b) `meeting_preps_new` (columns of `20261029000001_meeting_preps.sql` with `event_uid`, `event_start`, `event_title` nullable, plus `meeting_id TEXT UNIQUE`, keeping `UNIQUE(event_uid, event_start)`), `INSERT … SELECT` every column, copy the `sqlite_sequence` seq to `meeting_preps_new`, `DROP`, `ALTER … RENAME`; (c) the two tables and the index; (d) `UPDATE meetings SET reviewed=1`, `template_id = (SELECT CASE WHEN json_valid(s.result) THEN json_extract(s.result,'$.template_id') END FROM summary_processes s WHERE s.meeting_id=meetings.id)`, `ended_at = created_at`, `started_at = COALESCE((SELECT strftime('%Y-%m-%dT%H:%M:%f+00:00', meetings.created_at, printf('-%.3f seconds', MAX(t.audio_end_time))) FROM transcripts t WHERE t.meeting_id=meetings.id AND t.audio_end_time > 0), created_at)`.
- [ ] **RUST** `redesign_migration`, `migration_tests`, then the full `cargo test -p meetily --lib` → PASS.
- [ ] Commit `feat(db): redesign migration (meeting columns, preps rebuild, views, dismissed occurrences)`.

### Task 2: `redesign_backfill::run` and `gist_from_summary`
**Agent:** implementer · **Risky:** yes (inserts meeting rows at every startup)
**Files:** create `database/redesign_backfill.rs`; `database/mod.rs` (`pub mod redesign_backfill;`); `database/manager.rs:38`; `knowledge/extract.rs` (next to `parse_extraction`, `:149`).
**Interfaces:** `pub async fn run(pool: &SqlitePool) -> anyhow::Result<()>`; `pub(crate) fn gist_from_summary(md: &str) -> Option<String>`; `pub(crate) const GIST_MAX_CHARS: usize = 160;`.
- [ ] Failing tests:
  - `gist_from_summary`: `"## Summary\nWe agreed to ship. Then more."` → `"We agreed to ship."`; `"# T\n\n- **Ship** on Friday\n## Next"` → `"Ship on Friday"`; headings only or `""` → `None`; a 400-char sentence → 160 chars ending `…`; Cyrillic cut on char boundaries.
  - `backfill_links_and_creates`: meeting `cal (u1,s1)`, preps `p1 (u1,s1)`, `p2 (u2,s2,'Board')` → `p1.meeting_id='cal'`; `p2` points at a new row `title='Board'`, `title_source='calendar'`, `calendar_uid='u2'`, `calendar_start=s2`, `calendar_marker`/`calendar_original_start`/`started_at` NULL, `reviewed=0`.
  - `backfill_shared_key_links_once`: two meetings share `(u1,s1)` → only the older is linked, no UNIQUE error.
  - `backfill_is_idempotent`: a second run changes neither the meeting count nor any `meeting_id`.
  - `backfill_gist`: completed summary + `gist NULL` → `gist_from_summary(markdown)`; `gist 'kept'` stays.
- [ ] **RUST** `redesign_backfill`, `extract::tests::gist` → FAIL.
- [ ] Implement. Gists: meetings with `gist IS NULL` and a `json_valid` result with `$.markdown`; `UPDATE … SET gist=? WHERE id=? AND gist IS NULL`. Preps: for each `meeting_id IS NULL AND event_uid IS NOT NULL` row by id, one transaction: pick `SELECT id FROM meetings m WHERE calendar_uid=? AND calendar_start=? AND NOT EXISTS (SELECT 1 FROM meeting_preps p WHERE p.meeting_id=m.id) ORDER BY created_at, id LIMIT 1`, else insert `meeting-{uuid}` (`created_at=updated_at=Utc::now()`); then `UPDATE meeting_preps SET meeting_id=? WHERE id=? AND meeting_id IS NULL`. `gist_from_summary`: first non-blank non-heading line; strip `-`, `*`, `N.` markers and `**`, `__`, backticks; sentence ends at `.`/`!`/`?` followed by space or end; cut at 159 chars + `…`.
- [ ] Wire at `manager.rs:38`: `if let Err(e) = super::redesign_backfill::run(&pool).await { log::error!("redesign backfill failed; retried at next start: {e:#}"); }`.
- [ ] **RUST** `redesign_backfill`, `extract::`, `manager::` → PASS.
- [ ] Commit `feat(db): idempotent redesign backfill (gists, a meeting row for every prep) at startup`.

### Task 3: `meeting_date_sql` and every meeting-date reader
**Agent:** implementer · **Risky:** yes (wide cross-module; retention offers audio for deletion)
**Files:** `database/repositories/meeting.rs` (new fn near `:8`), plus each reader below.
**Interfaces:** `pub fn meeting_date_sql(alias: &str) -> String` → `COALESCE({a}.started_at, {a}.calendar_start, {a}.created_at)` (`""` → unaliased). `MeetingsRepository::meeting_date(pool, id) -> Result<Option<DateTime<Utc>>, SqlxError>`.

| Reader | Decision |
|---|---|
| `meeting.rs:43` `get_meetings` order | meeting date DESC |
| `digest.rs:490` `period_meetings` | meeting date, plus `AND EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id=meetings.id)` (a New row would block `ready_lists`, `:345`) |
| `retention.rs:61,85,98` | `COALESCE(ended_at, created_at) AS created_at`, also in ORDER BY |
| `briefs.rs:86-88`, `tasks.rs:172,177`, `stats.rs:254`, `retrieve.rs:359` | meeting date |
| `transcript.rs:331,346,360,376` search (`:401` newest first) | meeting date as the `created_at` field |
| `speakers.rs:461` | `MAX(meeting date)` |
| `summary/service.rs:369` | keep `created_at` for the user-title cutoff (`:608`); set `info.date` from `meeting_date` |
| `api.rs:864`, `api.rs:1101`, `transcript.rs:84-110` writes, `prep/upcoming.rs:73` | unchanged (not a date read; `upcoming.rs` is removed in Task 45) |

- [ ] Failing tests, one per switched reader, on a row with `created_at` 2026-11-01 and `started_at` 2026-11-09: `get_meetings` puts it before a row made 11-05; the week-of-11-09 digest includes it and leaves out a transcript-less row whose `calendar_start` is in that week; retention with `ended_at` 3 days ago and `created_at` 40 days ago offers nothing at `days=30`; briefs previous meeting, open-task order, stats day, Ask `day`, search order and the person's last date follow `started_at`; the summary prompt date is `2026-11-09`.
- [ ] **RUST** `meeting::`, `digest::`, `retention::`, `briefs::`, `tasks::`, `stats::`, `retrieve::`, `transcript::`, `speakers::`, `service::` → FAIL → switch each reader → PASS (old `created_at`-only tests still pass through COALESCE).
- [ ] Commit `feat(db): one meeting date (started_at, calendar start, created_at) for every reader`.

### Task 4: Gist from extraction; summary completion fills gist and template
**Agent:** implementer · **Risky:** yes (untrusted model JSON written to the DB)
**Files:** `knowledge/extract.rs` (`Extraction :85-89`, `SYSTEM :98-105`, `parse_extraction :149`); `knowledge/tasks.rs` (`apply_extraction :44-108`, test literal `:374`); `database/repositories/summary.rs` (`update_process_completed :149-185`).
**Interfaces:** `Extraction.gist: Option<String>` (`#[serde(default)]`); consumes `gist_from_summary` (Task 2).
- [ ] Failing tests: `"gist":"  Agreed to ship.\nextra "` → `Some("Agreed to ship.")` (first line, trimmed, ≤ `GIST_MAX_CHARS`); `""` or missing → `None`; `5` fails the parse as other bad types do; `VALID` still parses. `apply_extraction_writes_gist`: `Some` overwrites, `None` keeps the fallback. `completion_sets_fallback_gist_and_template`: result `{"markdown":"## S\nShipped it.","template_id":"interview"}` sets `gist='Shipped it.'`, `template_id='interview'` when NULL; existing `'x'` stays; a stale completion (`Ok(false)`) writes nothing.
- [ ] **RUST** `extract::`, `tasks::`, `summary::` → FAIL.
- [ ] Implement: add `"gist":"one sentence, at most 160 characters, what the meeting was about"` to `SYSTEM`; in `apply_extraction`'s tx before `:106` commit, `UPDATE meetings SET gist=? WHERE id=?` for `Some`; in the `completed` branch next to `rename_if_default` (`:181`), `UPDATE meetings SET gist=?, template_id=COALESCE(template_id, ?) WHERE id=?`.
- [ ] **RUST** → PASS. Commit `feat(knowledge): meeting gist from extraction, summary-sentence fallback, category from summary template`.

### Task 5: `Template.emoji`: field, built-ins, editor and assistant
**Agent:** implementer · **Risky:** no
**Files:** `summary/templates/types.rs` (`Template :18-40`, `validate :47`); `summary/templates/store.rs` (`get_template_in :46`, `list_templates_in :58`); `summary/templates/defaults.rs`; the nine `templates/*.json` and `templates/README.md` (field table `:13-23`, the assistant's `FIELD_DOCS`); `summary/template_commands.rs` (`TemplateInfo :7-14`, `template_info :22`); `summary/template_assist.rs` (`WRITING`, `parse_reply :283-299`, `keep_hidden_fields :306`); `emoji: None` in test literals `types.rs:129,248`, `store.rs:232`, `history.rs:388`, `template_commands.rs:93`, `template_assist.rs:1126`; `src/lib/template-draft.ts`, `src/components/TemplateSettings.tsx`, `tests/lib/template-draft.test.ts`; `src/hooks/meeting-details/useTemplates.ts:5-18` (keep `emoji`).
**Interfaces:** `Template.emoji: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`; `Some("")` = cleared on purpose); `pub fn valid_emoji(s: &str) -> bool` (trimmed, non-empty, ≤ 32 bytes, no ASCII, whitespace or control chars); `pub fn builtin_emoji(id: &str) -> Option<&'static str>`; `TemplateInfo.emoji: Option<String>` (effective; `None` when empty); `TemplateDraft.emoji: string | null`.
Built-ins: `standard_meeting` 📝, `german_lessons` 🎓, `interview` 🤝, `one_to_one` 👥, `one_two_one_rasulov` 🧭, `rasulov_lpr` 🎯, `portfolio_meetings` 📊, `scrum_of_scrums` 🔄, `delivery_management` 🚚, written into the JSON files; `get_template_in`/`list_templates_in` fill `None` from `builtin_emoji(id)` for older seeded files.
- [ ] Failing tests: `validate` rejects `"📝 Notes"`, `"ab"`, accepts `None`, `""`, `"📝"`; an old seeded `interview.json` lists `🤝`; a custom `""` lists `None`; every bundled JSON has a `valid_emoji`; `parse_reply` with `"emoji":"Notes 📝"` → `emoji None` (dropped, not a failure); `keep_hidden_fields` keeps the emoji when the revision omits it; FE `draftFromTemplate({…emoji:'🤝'}).emoji === '🤝'`, missing → `null`, `templateFromDraft` omits `null` and writes `''` when cleared; `useTemplates` keeps `emoji`.
- [ ] **RUST** `templates::`, `template_assist::`, `template_commands::` and **FE** `tests/lib/template-draft.test.ts` → FAIL.
- [ ] Implement: README row `` | `emoji` | string, optional | One emoji shown before meetings of this kind | ``; `WRITING` gains "Set `emoji` to one emoji that fits this kind of meeting."; `TemplateSettings` gets `Input id="tpl-emoji"` (label "Emoji", `maxLength` 16) beside the name.
- [ ] **RUST**, **FE** → PASS. Commit `feat(templates): category emoji (built-ins, editor field, assistant)`.

### Task 6: One-time emoji suggestion when a custom template is saved
**Agent:** implementer · **Risky:** yes (untrusted LLM reply written to a user file; background task)
**Files:** create `summary/template_emoji.rs` (`mod` in `summary/mod.rs`); `summary/template_commands.rs:57-65` (`api_save_template` gains injected `app: AppHandle<R>`; frontend unchanged).
**Interfaces:** consumes `template_assist_settings::target`, `llm_target::complete`, `usage::record` (`Purpose::TemplateAssist`), `valid_emoji`, `builtin_emoji`, `templates::{get_template, save_template}`. Produces `pub(crate) fn emoji_prompts(t: &Template) -> (String, String)`, `pub(crate) fn parse_emoji_reply(raw: &str) -> Option<String>`, `pub(crate) async fn apply_suggestion_in(dir: &Path, id: &str, emoji: &str) -> Result<bool, String>` (re-reads, saves only if still `None`, `SaveOrigin { source: HistorySource::Ai, note: "emoji suggestion" }`).
- [ ] Failing tests: `parse_emoji_reply(" 📝 ")` → `Some("📝")`; `"📝 Notes"`, `"Notes"`, `""`, `"<b>x</b>"`, a 40-byte string → `None`. `emoji_prompts` holds name and `use_when` and carries `extract::UNTRUSTED`. `apply_suggestion_in` writes when `None`; not when the user set `""` or `"🎯"` meanwhile, or the file is gone (`Ok(false)`).
- [ ] **RUST** `template_emoji::` → FAIL.
- [ ] Implement: after a successful save, spawn `suggest(app, id)` only when `template.emoji.is_none()` and `builtin_emoji(&id).is_none()`; one call, 20 s timeout, usage recorded; failure is `warn!` only, so the next save retries.
- [ ] **RUST** → PASS. Commit `feat(templates): suggest an emoji once for a custom template on save`.

### Task 7: Meeting rows, create, reviewed and template setters, views CRUD
**Agent:** implementer · **Risky:** yes (write commands with untrusted frontend input)
**Files:** create `api/meetings.rs` (style of `api/meeting_page.rs`: `*_inner(pool, …)` + thin commands; `pub mod meetings;` in `api/mod.rs:3`); `database/repositories/meeting.rs` (`default_meeting_title`, `create_meeting`); register in `lib.rs` next to `api::api_delete_meeting` (`:901`); mocks in `src/lib/browser-preview.ts` (`handlers :35`).
**Interfaces:** the pinned `MeetingRow` (`created_at` normalised through `DateTimeUtc` to RFC 3339), `MeetingView { id, name, position: i64, builtin: bool, config_json: String }`; the seven commands (each `Result<…, String>`; setters and save return `()`); `default_meeting_title`; `MeetingsRepository::create_meeting(pool, title, now_local)` (blank → default title + `title_source='default'`; else trimmed, cut to 200 chars, `title_source='user'`; `reviewed=0`, no `started_at`). `api_create_meeting(title)` calls it with `Local::now()`.
- [ ] Failing tests:
  - `default_meeting_title(Mon 2026-11-16 14:02)` → `"Untitled · Mon 14:02"`.
  - `create_meeting(None)` and `Some("  ")` → default title, `'default'`; `Some(" Board ")` → `"Board"`, `'user'`; 300 chars → 200.
  - `list_rows` on a recorded meeting with a completed summary, a New meeting with prep items `[a,b,c]`, and a prep with corrupt `items_json` → three rows with expected `has_transcript`, `has_summary` (true only for the completed summary) and `prep_points` (3, and 0 for corrupt), newest meeting date first; `emoji` from the template list.
  - Setters `Err` on unknown id; `set_template(Some("../x"))` → `Err`, `None` clears.
  - Views: save inserts with `builtin=0`; re-saving `all` with `builtin:false` keeps `builtin=1`; `config_json "{"`, > 64 KiB, empty id, name > 80 chars → `Err`; deleting `week` → `Err("Built-in views cannot be deleted")`; deleting a user view removes it; list ordered `position, id`.
- [ ] **RUST** `api::meetings::`, `meeting::` → FAIL.
- [ ] Implement: one SELECT with `EXISTS(transcripts)`, `EXISTS(SELECT 1 FROM summary_processes s WHERE s.meeting_id=m.id AND LOWER(s.status)='completed') AS has_summary`, a `json_valid`-guarded `json_array_length(items_json)` from `meeting_preps` by `meeting_id`, `ORDER BY meeting_date_sql("m") DESC, m.id`; emoji via `spawn_blocking(templates::list_templates)`; view save `INSERT … ON CONFLICT(id) DO UPDATE SET name, position, config_json`; template ids checked with `templates::is_valid_id` (`store.rs:26`).
- [ ] Mocks: `api_list_meeting_rows` (five rows: live, New with prep 3, Not recorded, Cancelled, Finished with gist and `hasSummary`); `api_list_meeting_views` (three seeds); `api_create_meeting` → `'preview-new'`; others `null`.
- [ ] **RUST**, **FE** → PASS. Commit `feat(meetings): table rows, create meeting, reviewed/category setters and saved views`.

### Task 8: Delete recording vs Delete meeting
**Agent:** implementer · **Risky:** yes (deletion, files on disk, replaces a guarantee)
**Files:** `database/repositories/meeting.rs` (`delete_meeting_with_transaction :296-392`, tasks delete `:355-359`, test `delete_meeting_deletes_tasks_and_extract :520-552`); `audio/meeting_folder.rs` (next to `delete_meeting_and_folder :281-306`); `api/meetings.rs` (command, registered in `lib.rs`); `prep/store.rs` (test `deleting_a_meeting_keeps_its_prep :868-885`).
**Interfaces:** `MeetingsRepository::delete_recording(pool, id) -> Result<bool, SqlxError>`; `delete_recording_and_audio(pool, id, recordings_base, remove) -> Result<Option<Option<String>>, sqlx::Error>` (contract of `delete_meeting_and_folder`); `api_delete_recording(meeting_id) -> Result<serde_json::Value, String>` shaped like `api_delete_meeting` (`{status, message, folder_warning}`); `pub(crate) const DISMISS_KEEP_DAYS: i64 = 30;`.
- [ ] Failing tests:
  - `delete_recording_keeps_meeting_prep_notes`: a meeting with transcripts, summary, chunks, speakers, speaker job, extracts, notes, prep by `meeting_id`, `template_id 'x'`, `reviewed=1`, gist, times, `folder_path` → row, title, `template_id`, notes, prep remain; transcripts, summary, chunks, `meeting_speakers`, `speaker_jobs`, extracts gone; `voiceprints.source_meeting_id` NULL; `started_at`, `ended_at`, `gist`, `folder_path` NULL, `reviewed=0`; `llm_usage` kept.
  - `kept_tasks_survive_both_deletes` on the rewritten `:520` data plus `tu` (`unassigned`, unedited), `te` (`unassigned`, `edited=1`) and `tx` (`owner 'me'`, `status 'dismissed'`, `edited=1`, i.e. removed with X): after either delete `t1`, `t2`, `t3`, `te` remain with `meeting_id`/`at` NULL; `tu` and `tx` are gone; `t4`, `t5` unchanged.
  - `delete_meeting_removes_prep_and_brief` (replaces `prep/store.rs:868`): prep `meeting_id='M'` gone; the brief at `(calendar_uid, calendar_start)` and its feed item gone, kept when another meeting shares the key.
  - `delete_meeting_dismisses_calendar_occurrence`: uid `'u'` + `calendar_original_start` `o` inserts `('u', o, now)`; no original start → `calendar_start`; no uid → nothing; a 31-day-old entry pruned, 29-day kept.
  - `delete_recording_and_audio` recycles the guarded folder, keeps it with a warning when another meeting uses it (mirror `meeting_folder.rs:585-624`); missing id → `Ok(None)`.
- [ ] **RUST** `meeting::`, `meeting_folder::`, `prep::store::` → FAIL.
- [ ] Implement. Task rule (both paths): `UPDATE tasks SET meeting_id=NULL, at=NULL WHERE meeting_id=? AND status <> 'dismissed' AND (owner='me' OR edited=1)`, then `DELETE FROM tasks WHERE meeting_id=?`. `delete_recording`: one tx reusing the existing statements (chunks, summary, speakers, voiceprints, extracts, transcripts, previous-meeting briefs) plus the row reset; read `folder_path` first, remove the folder after commit. `delete_meeting`: read `calendar_uid`, `calendar_start`, `calendar_original_start` first; `DELETE FROM meeting_preps WHERE meeting_id=?`; delete the event brief and feed items guarded by `NOT EXISTS` (other meeting with the key); `INSERT OR REPLACE INTO dismissed_occurrences`; prune `dismissed_at < now − 30 days` (both bound as `DateTime<Utc>`).
- [ ] **RUST** → PASS. Commit `feat(meetings): Delete recording keeps the meeting; Delete meeting removes prep, keeps kept tasks, remembers calendar occurrences`.

## Phase 2: Meeting-first recording (§6.2)

The id travels: start command → `RECORDING_TARGET` → `recording-started`/`recording-stopped` payloads → `api_save_transcript`, which writes into the row or (row gone **or already has transcripts**) creates a new one with `fallback: true`. Continue (`audio/continuation.rs`, `api.rs:1027-1040`) is unchanged: a continue start skips routing and passes `meeting_id = None`; in the save, `take_target` runs first.

### Task 9: `recording::route`, the one routing function
**Agent:** implementer · **Risky:** no (creates rows only)
**Files:** create `recording/mod.rs` (`pub mod route;`), `recording/route.rs`; `lib.rs:58-68` (`mod recording;`), `generate_handler!` next to `api::api_save_transcript` (`:912`); mock in `src/lib/browser-preview.ts`.
**Interfaces:**
```rust
pub async fn pick_target(pool: &SqlitePool, now: DateTime<Utc>, source: StartSource) -> anyhow::Result<RouteTarget>
pub async fn resolve_target(pool: &SqlitePool, now: DateTime<Utc>, source: StartSource) -> anyhow::Result<RoutedMeeting>
pub(crate) async fn mark_started(pool: &SqlitePool, meeting_id: &str, at: DateTime<Utc>) -> Result<bool, sqlx::Error>
pub(crate) fn take_fresh(meeting_id: &str) -> bool   // true once if resolve_target created this row
#[tauri::command] pub async fn api_route_recording_start(state: tauri::State<'_, AppState>, source: StartSource) -> Result<RoutedMeeting, String>
```
- `pick_target`: `Page(id)` → `Existing(id)`; every other source → `New { title: None }` (Task 47 adds the 📞 rule).
- `resolve_target`: `Existing` reads the title with `get_meeting_metadata` (`meeting.rs:145`), missing → `bail!("This meeting no longer exists")`; `New { title }` → `MeetingsRepository::create_meeting(pool, title, Local::now())`, records the id in `static FRESH_ROUTED: Mutex<Option<String>>` (`// shortcut: process-local; after a restart a routed row is only returned to New, never deleted`), reads the title back.
- `mark_started`: `UPDATE meetings SET started_at=?, updated_at=? WHERE id=?`, returns `rows_affected()==1`.
- Mock: `api_route_recording_start: () => ({ meetingId: 'preview-1', title: 'Weekly sync' })`.
- [ ] Failing tests: `pick_target_page_is_existing`; `pick_target_other_paths_are_new` (Sidebar, Popup, Tray, AutoDetect); `resolve_new_creates_default_row` (title starts `"Untitled · "`, `title_source 'default'`, `take_fresh(id)` true then false); `resolve_page_of_missing_row_errs`; `mark_started_sets_and_reports_missing`; `start_source_from_frontend_json` (`"sidebar"` → Sidebar, `{"page":"m1"}` → Page).
- [ ] **RUST** `route::` → FAIL → implement → PASS; **FE** `tsc`.
- [ ] Commit `feat(recording): one routing function picks the meeting a recording starts into`.

### Task 10: Save into an existing meeting row; release a routed row on Continue
**Agent:** implementer · **Risky:** yes (persistence; guarded write races recovery; deletes a row)
**Files:** `database/repositories/transcript.rs:84-160` (`save_transcript`: add `ended_at` to the INSERT at `:101`), tests at `:426+`; `database/repositories/meeting.rs` (`UNTOUCHED_SQL`, `release_routed_row`); `api/api.rs:978-1080` (`api_save_transcript`).
**Interfaces:**
- `TranscriptsRepository::save_into_or_new(pool, target: Option<&str>, meeting_title: &str, transcripts: &[TranscriptSegment], folder_path: Option<String>, notes: &[MeetingNote], enqueue_speakers: bool, title: &TitleOrigin) -> Result<(String, bool), SqlxError>` → `(meeting_id, fallback)`.
- `pub(crate) const UNTOUCHED_SQL` (alias `m`; Task 43 reuses it):
  ```sql
  m.started_at IS NULL AND m.title_source <> 'user' AND m.template_id IS NULL
  AND NOT EXISTS (SELECT 1 FROM transcripts t WHERE t.meeting_id = m.id)
  AND NOT EXISTS (SELECT 1 FROM meeting_notes n WHERE n.meeting_id = m.id AND TRIM(COALESCE(n.notes_markdown,'')) <> '')
  AND NOT EXISTS (SELECT 1 FROM meeting_preps p WHERE p.meeting_id = m.id
      AND (TRIM(p.material) <> '' OR p.chat_json NOT IN ('','[]') OR p.items_json NOT IN ('','[]')))
  ```
- `MeetingsRepository::release_routed_row(pool, id: &str, fresh: bool) -> Result<bool, SqlxError>` (true = deleted): one tx: `UPDATE meetings SET started_at=NULL WHERE id=? AND NOT EXISTS (transcripts)`; if `fresh`: `DELETE FROM meetings AS m WHERE m.id=? AND m.calendar_uid IS NULL AND <UNTOUCHED_SQL>`, and when it removed the row, delete its (empty) `meeting_preps`/`meeting_notes` rows.
- `api_save_transcript(..., live: Option<bool>, meeting_id: Option<String>)`; response gains `"fallback": bool`.
**Behaviour:** with `Some(id)`, one tx, guarded write first: `UPDATE meetings SET folder_path=?, ended_at=?, updated_at=?, audio_deleted_at=NULL WHERE id=? AND NOT EXISTS (SELECT 1 FROM transcripts WHERE meeting_id=?)`; 0 rows → roll back, `save_transcript(...)`, `(new_id, true)`; else `insert_segments` (`:18`), merge notes like `append_in_tx` (`:177-182`), `SpeakersRepository::enqueue_in_tx` when asked, commit, `(id, false)`. Never touches title, `title_source` or calendar columns. `None` → `save_transcript` (binds `ended_at = now`). In `api.rs:1037-1040` the non-continued branch calls `save_into_or_new(pool, meeting_id.as_deref(), …)`. When the **continue branch succeeds** and `meeting_id` is `Some(id)` (Continue chosen mid-recording, §6.2 user decision), call `release_routed_row(pool, &id, route::take_fresh(&id))`; an error is `warn!` (the row keeps `started_at`, nothing is lost). Every other save also calls `take_fresh(&id)` so the flag never outlives its recording.
- [ ] Failing tests (target row inserted as `('m-t','Untitled · Mon 14:02', now, now, 'default')`): `save_into_existing_keeps_title_and_sets_ended_at` (`("m-t", false)`, title kept, `folder_path '/f'`, `ended_at` set, 1 line); `save_into_missing_meeting_falls_back_to_new`; `save_into_meeting_with_transcripts_never_merges` (second save → `fb == true`, `m-t` still 1 line); `save_into_keeps_existing_notes_first` (`[n1]` then `[n2]` → `[n1,n2]`); `save_into_queues_speakers_when_active`; `save_without_target_sets_ended_at`; `continue_releases_fresh_routed_row`: fresh untouched row with `started_at` → deleted; the same row not fresh, or fresh with a note / `title_source 'user'` / a 📞 `calendar_uid` → kept with `started_at` NULL.
- [ ] **RUST** `transcript::`, `meeting::` → FAIL → implement and wire → **RUST** `transcript::`, `meeting::`, `continuation::` PASS (existing `save_transcript_*`, `append_*` unchanged).
- [ ] Commit `feat(recording): save a recording into its meeting row, fall back to a new meeting`.

### Task 11: Thread `meeting_id` through start and stop; refuse deleting the live meeting
**Agent:** implementer · **Risky:** yes (global start/stop state; deletion guard)
**Files:** `audio/recording_commands.rs` (statics `:38-47`; `finalize_recording_start :153-159`; `start_recording_with_meeting_name :367-548`; `start_recording_with_devices_and_meeting :562-747`; default names `:448-456`, `:643-650`; `recording-started` emits `:537`, `:731`; `IS_RECORDING.store(false)` `:1050`; `recording-stopped` emit `:1084-1091`; plain wrappers `:361-364`, `:552-559` pass `None`; `get_recording_state :1208-1233`); `lib.rs:200-260` (`start_recording` gains `meeting_id`), `lib.rs:404-470`; `api/api.rs:778` (`api_delete_meeting`), `api/meetings.rs` (`api_delete_recording`).
**Interfaces:** `static RECORDING_TARGET: Mutex<Option<String>>`; `pub fn recording_target_id() -> Option<String>`; `fn take_recording_target() -> Option<String>`; event `recording-target {meetingId}`; `recording-started` gains `"meetingId"`; `recording-stopped` gains `"meeting_id"`; `get_recording_state` gains `"meeting_id"` (the routed target, else the current continue target's meeting id via `continuation::current_target`, `continuation.rs:510`, display only); `pub(crate) fn ensure_not_live(live: Option<&str>, id: &str) -> Result<(), String>` → `Err("This meeting is being recorded. Stop the recording first.")`.
**Behaviour:** `finalize_recording_start(meeting_id)` stores the target next to `IS_RECORDING = true` (both call sites, `:488`, `:682`). Then `claim_target(app, meeting_id)`: for `Some`, `mark_started` via `app.try_state::<AppState>()` (missing state, Err or `Ok(false)` → `warn!`, recording continues; the save falls back) and emit `recording-target`. At `:1050` `take_recording_target()` goes into `recording-stopped`. Default names become `default_meeting_title(Local::now())` (folder/fallback name; `sanitize_filename` replaces `:`). Both delete commands call `ensure_not_live(recording_target_id().as_deref(), &meeting_id)?` first.
- [ ] Failing tests: `recording_target_is_set_by_finalize_and_taken_once` (finalize `Some("m-1")` → `recording_target_id()` `Some`; take → `Some`, then `None`; reset `IS_RECORDING`); `delete_refuses_live_target` (`ensure_not_live(Some("m1"),"m1")` Err, `(Some("m1"),"m2")` and `(None,"m1")` Ok).
- [ ] **RUST** `recording_commands::` → FAIL → implement → **RUST** `recording_commands::`, `route::`, `api::meetings::` PASS; `cargo check -p meetily --tests`.
- [ ] Commit `feat(recording): the recording carries its meeting id from start to stop; the live meeting cannot be deleted`.

### Task 12: Route the tray, pop-up and detector starts
**Agent:** implementer · **Risky:** yes (two starts during the routing await; ids interpolated into `window.eval`)
**Files:** `tray.rs:84`, `:89-112` (`request_recording_start`), tests `:565+`; `banner.rs:162,171`; `meeting_detector.rs:129`.
**Interfaces:** `pub fn request_recording_start<R: Runtime>(app: &AppHandle<R>, source: StartSource, continue_id: Option<&str>)` (drops `title_prefix`); `fn autostart_script(meeting_id: Option<&str>, continue_id: Option<&str>) -> String`; sessionStorage key `autoStartMeetingId`.
**Behaviour:** keep the busy check (`:93-99`), then `static ROUTING_START: AtomicBool` (`compare_exchange` failure → log and return). Tray state `Starting`, then spawn: with `continue_id`, `meeting_id = None`; else `resolve_target(pool, Utc::now(), source)`; on error retry once with `StartSource::Sidebar`; if that fails too, `error!` and start anyway with `meeting_id = None`, so the save creates the row as today (§4: a recording is never dropped; the user may not be at the screen). Then `window.eval(&autostart_script(..))`, clear `ROUTING_START`. The script JSON-encodes ids with `serde_json::to_string` (as `:103-106`), sets `autoStartMeetingId`/`autoStartContinueId` (each only when given) and `autoStartRecording`, then `window.location.assign('/')` (Task 16 → `/record`, Task 33 → the meeting page). Callers: tray `Tray`, `banner.rs:162` `Popup`, `banner.rs:171` `Popup` + `Some(&offer.meeting_id)`, detector `AutoDetect`.
- [ ] Failing tests: `autostart_script_sets_meeting_id` (contains `sessionStorage.setItem('autoStartMeetingId', "m-1")`, no `autoStartContinueId`, no `autoStartTitlePrefix`, ends `window.location.assign('/')`); `autostart_script_escapes_ids` (`x"); alert(1)//` appears JSON-escaped); `autostart_script_continue_has_no_meeting_id`.
- [ ] **RUST** `tray::` → FAIL → implement, update callers → **RUST** `tray::`, `banner::`, `meeting_detector::` PASS; `cargo check -p meetily --tests`.
- [ ] Commit `feat(recording): tray, pop-up and auto-start route through pick_target`.

### Task 13: Frontend start and stop carry the meeting id; `liveMeetingId`
**Agent:** implementer · **Risky:** yes (save path; a lost id silently creates a duplicate row)
**Files:** create `src/lib/recording-target.ts`; `src/services/recordingService.ts` (`:19-23` `RecordingStoppedPayload.meeting_id?`, `:90-101` `startRecordingWithDevices`, `:136-138`); `src/services/storageService.ts:18-22,46-66`; `src/lib/resume.ts:5-9` (`saveOutcome`); `src/hooks/useRecordingStart.ts` (`:33-68` drop `generateMeetingTitle`, `:113-200`, `:205-303`, `:305-400`); `src/hooks/useRecordingStop.ts` (`:87-102`, `:247-275`, `:321-323`); `src/contexts/TranscriptContext.tsx:147`; `src/contexts/RecordingStateContext.tsx:29-52,360-404`; tests `tests/lib/recording-target.test.ts`, `tests/lib/resume.test.ts`, `tests/lib/storage-service.test.ts`, `tests/contexts/recording-state-live-id.test.tsx`.
**Interfaces:** `type StartSourceArg = 'sidebar' | { page: string }`; `routeRecordingStart(source): Promise<{ meetingId: string; title: string }>`; `takeAutoStartMeetingId(storage): string | null`; `handleRecordingStart(meetingId?: string)`; `saveMeeting(..., live = false, meetingId: string | null = null)`; `SaveMeetingResponse.fallback?: boolean`; `saveOutcome(...).fallback: boolean`; `useRecordingState().liveMeetingId: string | null`.
**Behaviour:** every start: model check, then `routeRecordingStart` (manual/sidebar: `meetingId ? { page: meetingId } : 'sidebar'`; autostart: `{ page: takeAutoStartMeetingId(sessionStorage) }` when present, else `'sidebar'`), then `startRecordingWithDevices(mic, sys, routed.title, routed.meetingId)` and `setStartTitle({ title: routed.title, source: 'default' })`. A continue autostart skips routing (`meetingName = null`, `meetingId = null`; `continue_set_target` as today). `pickStartTitle`/`autoStartTitlePrefix` are no longer read (the key is still removed). Routing error → start anyway with `meetingId = null` (the save creates a new meeting, §4) and `toast.warning('Recording as a new meeting', { description })`; add a test for it. The stopped listener stores `meeting_id` in `last_recording_meeting_id`; the save passes it; `outcome.fallback && !outcome.continueError` → `toast.warning('Saved as a new meeting', { description: 'The meeting this recording belonged to was deleted.' })`; clear the key next to `:322`. `TranscriptContext.tsx:147` fallback → `meetingName || 'Untitled'`. `liveMeetingId` is read from `get_recording_state().meeting_id` on mount, re-read on `recording-started`, `recording-target` and after `continue_set_target` resolves, and cleared when status returns to idle.
- [ ] Failing tests: `routeRecordingStart('sidebar')` / `({page:'m1'})` invoke `api_route_recording_start` with `{ source }`; `takeAutoStartMeetingId` returns and removes the key; `saveOutcome({meeting_id:'m',fallback:true}).fallback === true`, missing → false; `saveMeeting('T',[],null,[],null,{source:'default'},true,'m-t')` sends `meetingId:'m-t', live:true`; `liveMeetingId` survives a remount: a fresh provider with `get_recording_state` → `{is_recording:true, meeting_id:'m1'}` gives `'m1'` without any event; idle → `null`.
- [ ] **FE** (these files) → FAIL → implement → **FE** PASS, full `bun test`, `pnpm run build`.
- [ ] Commit `feat(recording): start and save carry the meeting id; liveMeetingId; toast when saved as new`.

### Task 14: Crash recovery saves into the target row
**Agent:** implementer · **Risky:** yes (persistence and restart; never merges recordings)
**Files:** `src/services/indexedDBService.ts:10-20` (`MeetingMetadata`); `src/contexts/TranscriptContext.tsx:133-160`; `src/hooks/useTranscriptRecovery.ts:182-188`; `tests/hooks/transcript-recovery.test.tsx` (mocks `:17-37`).
**Interfaces:** `MeetingMetadata.targetMeetingId?: string` (`meetingId` is already the IndexedDB key).
**Behaviour:** the `recording-started` listener writes `targetMeetingId: payload?.meetingId ?? undefined`. Recovery calls `saveMeeting(metadata.title, formatted, folderPath ?? null, metadata.notes ?? [], meetingId, { source: 'default' }, false, metadata.targetMeetingId ?? null)`; on `fallback`, `toast.info('Recovered as a new meeting', { description: 'Its meeting was deleted or recorded again.' })`. The Rust rule of Task 10 covers both cases.
- [ ] Failing tests: saves into `'meeting-target'`; `fallback: true` → `toast.info` once (add `info` to the sonner mock); metadata without a target → `null`.
- [ ] **FE** → FAIL → implement → PASS, full `bun test`.
- [ ] Commit `feat(recovery): recovered recordings return to their meeting, or save as new`.

### Task 15: The category picked before recording is the manual template pick
**Agent:** mechanical · **Risky:** no
**Files:** `summary/service.rs:350-379`, tests `:616+`.
**Interfaces:** `async fn meeting_category(pool: &SqlitePool, meeting_id: &str) -> Option<String>` (`SELECT template_id FROM meetings WHERE id=?`; error → `warn!`, `None`).
- [ ] Failing test `meeting_category_reads_template_id` (`'one_on_one'` → `Some`; missing meeting → `None`).
- [ ] **RUST** `summary::service::` → FAIL. At `:379` pass `manual_template_id.clone().or(meeting_category(&pool, &meeting_id).await).as_deref()` as `manual`. → PASS; also `template_choice::`.
- [ ] Commit `feat(summary): the meeting's category is the manual template pick`.

## Phase 3: Meetings table at `/` (§6.3, §6.2 recovery mount)

Until Phase 6 the old record screen lives at `/record`, the old sidebar list stays, and the Ask button is a stub (`onAsk` prop) that Task 28 wires.

### Task 16: Record screen moves to `/record`; startup checks mount on `/`
**Agent:** mechanical · **Risky:** yes (if recovery fails to mount, recoverable recordings are never offered)
**Files:** `git mv frontend/src/app/page.tsx frontend/src/app/record/page.tsx` (imports `./_components/...` → `@/app/_components/...`); create `src/app/_components/StartupChecks.tsx` and a new `src/app/page.tsx`; edit `components/Sidebar/SidebarProvider.tsx:136,153,161`, `app/layout.tsx:157`, `lib/resume.ts:27`, `tests/lib/resume.test.ts:31`; `tray.rs` `autostart_script` URL and its test.
**Interfaces:** `StartupChecks` (no props); temporary route `/record`.
- [ ] Tests: `resume.test.ts:31` expects `['/record']`; `autostart_script_sets_meeting_id` expects `window.location.assign('/record')`. Run → FAIL.
- [ ] Move into `StartupChecks`: `cudaToastChecked` (`:26`), the CUDA effect (`:69-85`), `useTranscriptRecovery()` (`:55-64`), router, `refetchMeetings`, the recovery effects and handlers (`:88-186`, keeping the skip while recording/stopping/processing/saving, `:92-98`) and `<TranscriptRecovery …/>` (`:217-224`); recovered navigation goes to the returned `meeting_id`. New `app/page.tsx` renders `<StartupChecks />` and a `<main>` placeholder. Retarget `resume.ts:27`, `SidebarProvider.tsx:136,153,161`, `layout.tsx:157` and the autostart script to `/record`. Leave `meeting-details/page.tsx:269` and `Sidebar/index.tsx:353` on `/` (home).
- [ ] **FE** (+ `tests/hooks/transcript-recovery.test.tsx`), **RUST** `tray::` → PASS; dev:preview renders `/` and `/record`.
- [ ] Commit `refactor(home): record screen moves to /record; recovery and CUDA check mount on /`.

### Task 17: `lib/meetings.ts`: commands, `MeetingRow`, state and date
**Agent:** mechanical · **Risky:** no
**Files:** create `src/lib/meetings.ts`, `tests/lib/meetings.test.ts`.
**Interfaces:** `type MeetingRow = { id; title; titleSource; templateId: string|null; emoji: string|null; reviewed: boolean; gist: string|null; startedAt: string|null; endedAt: string|null; createdAt: string; calendarUid: string|null; calendarStart: string|null; calendarEnd: string|null; calendarCancelledAt: string|null; hasTranscript: boolean; hasSummary: boolean; prepPoints: number }`; `type MeetingState = 'recording'|'finished'|'cancelled'|'not-recorded'|'new'`; `type MeetingView = { id; name; position: number; builtin: boolean; configJson: string }`. Wrappers `listMeetingRows`, `createMeeting(title?)`, `setReviewed`, `setTemplate`, `deleteRecording`, `deleteMeeting(id): Promise<{ folderWarning: string|null }>`, `listViews`, `saveView`, `deleteView`. Pure `parseDbTime(s): Date`, `meetingDate(row)` (`startedAt ?? calendarStart ?? createdAt`), `meetingState(row, liveId, now)`, `formatMeetingDate(row, now, live = false)`, `deleteMeetingParts(row): string[]`.
- [ ] Tests: `parseDbTime('2026-11-16 10:00:00.123456789+00:00')` = `Date.UTC(2026,10,16,10,0,0,123)`, RFC 3339 `Z` parses. `meetingState`: `liveId === row.id` → `'recording'`; `hasTranscript` → `'finished'`; `calendarCancelledAt` → `'cancelled'`; date 15 Nov 10:00Z, now 16 Nov 10:00:01Z → `'not-recorded'`, 09:59:59Z → `'new'`; old `createdAt` + `calendarStart` 19 Nov → `'new'`. `formatMeetingDate`: `'Mon 16 Nov · 10:00–10:45'`; live `'Mon 16 Nov · 10:49–now'`; calendar `'Thu 19 Nov · 17:00–18:00'`; created only `'Mon 16 Nov · 14:02'`; other year `'Fri 14 Nov 2025 · 09:00–09:30'`. `deleteMeetingParts`: finished, prep 2, calendar uid → `['recording, transcript and summary','prep (2 points)','notes','brief']`; prep 1 → `'prep (1 point)'`; bare new → `['notes']`. `setReviewed` invokes `('api_set_meeting_reviewed', { meetingId:'m1', reviewed:true })`; `saveView` sends `{ view: { id, name, position, builtin, configJson } }`.
- [ ] Run → FAIL → implement (date-fns `'EEE d MMM'` + `' yyyy'` when the year differs, `'HH:mm'`; `parseDbTime` puts `T` for the first space and cuts the fraction to 3 digits) → PASS.
- [ ] Commit `feat(table): meetings data layer, row state and date label`.

### Task 18: View config, built-in views and arranging rows
**Agent:** implementer · **Risky:** no · **Difficult:** yes (grouping and order rules)
**Files:** create `src/lib/meeting-views.ts`, `tests/lib/meeting-views.test.ts`.
**Interfaces:** `ViewFilter = { categories?: (string|null)[]; reviewed?: boolean; states?: MeetingState[]; calendar?: boolean; range?: 'this-week' | { from: string; to: string } }`; `ViewConfig = { sort: { id: 'date'|'name'|'category'|'reviewed'; desc: boolean }[]; filter; group: 'none'|'day'|'category'; widths: Record<string, number>; hidden: string[]; density: 'comfortable'|'compact' }`; `BUILTIN_VIEW_DEFAULTS`, `parseViewConfig(id, json)`, `viewReducer`, `weekRange(now)`, `effectiveHidden(config)`, `arrangeRows(rows, config, { liveId, now, templates }): Section[]` (`Section = { key; label: string|null; rows }`).
Pinned: columns `name, date, category, summary, reviewed`; `name` can't be hidden; widths clamped 80–640; `all` = date desc, group category; `week` = this-week, date asc, group day; `unreviewed` = `reviewed:false`, group none, date desc; custom default group none, date desc; `effectiveHidden` adds `category` while grouped by category; the live row always sits alone in a first section `'Now'` whatever the filter; `reviewed:false` matches only rows with `hasSummary` and not reviewed; ties by id.
- [ ] Tests: `parseViewConfig('week','{}')` = defaults; `('all','not json')` → `all` defaults; `('v1','{"group":"weird","density":"compact"}')` → group `'none'`, density `'compact'`. Reducer: `setWidth name 20` → 80; `toggleHidden summary` twice → `[]`; `toggleHidden name` unchanged. `weekRange(new Date(2026,10,22,23,0))` → `{from: new Date(2026,10,16), to: new Date(2026,10,23)}`. Week view: Mon 10:00, Mon 14:02, Tue 18:00, next Mon → `['Monday 16 Nov','Tuesday 17 Nov']`, next Monday dropped; with `liveId` first section `'Now'` with only that row. Category grouping with `[{one_on_one,'1:1','🤝'},{interview,'Interview','🧑‍💻'}]` → `['🤝 1:1','🧑‍💻 Interview','No template']`. `states:['cancelled']`, `calendar:true` filter.
- [ ] Run → FAIL → implement → PASS. Commit `feat(table): per-view config, built-in views, filter/sort/group`.

### Task 19: Search reports hits per meeting; snippet labels and links
**Agent:** mechanical · **Risky:** no
**Files:** `database/repositories/transcript.rs:219-229` (`SearchHit`), `:410-422` (`search_all`), test after `:585`; `src/lib/meeting-target.ts` (`SearchHit`, `MeetingTarget.tab`); `src/lib/ask.ts:68` (`clock` moves out); tests `tests/lib/meeting-target.test.ts` and `totalInMeeting` in fixtures of `tests/components/search-hits.test.tsx:8`, `sidebar-search-order.test.tsx`, `tests/lib/ask.test.ts`, `tests/components/ask-page.test.tsx`.
**Interfaces:** `SearchHit.total_in_meeting: usize` (TS `totalInMeeting`; counted before the cap of 3); `MeetingTarget.tab` gains `'prep'`; exported `clock(seconds)`, `snippetLabel(hit)`, `snippetHref(hit)`, `groupHits(hits): { meetingId; hits; more }[]`.
- [ ] Rust test `search_reports_total_in_meeting` (the `three_hits_per_meeting_max` "бюджет" fixture → 3 hits, each `total_in_meeting == 7`). **RUST** `transcript::tests` → FAIL → implement (collect snippeted hits, count, `take(3)`) → PASS.
- [ ] Bun: `snippetLabel` → `'Name'`, `'Summary'`, `'Notes'`, `'Transcript 12:04'` (724), `'Transcript'` (null); `snippetHref` transcript 724.6 → `'/meeting-details?id=m1&t=724'`, summary → `'…&tab=summary'`, title → `'/meeting-details?id=m1'`; `groupHits([m2 t, m2 s, m1 t])` with m2 total 5 → `[{m2,2 hits,more:3},{m1,1,more:0}]`; `parseMeetingTarget('id=a&tab=prep').tab === 'prep'`. → FAIL → implement → PASS.
- [ ] Commit `feat(search): per-meeting hit totals, snippet labels and time links`.

### Task 20: Keyboard map
**Agent:** mechanical · **Risky:** no
**Files:** create `src/lib/meetings-keys.ts`, `tests/lib/meetings-keys.test.ts`.
**Interfaces:** `tableKeyAction(e: { key; ctrlKey; metaKey; shiftKey; altKey }, ctx: { inEditable; searching }): 'up'|'down'|'open'|'toggle-reviewed'|'focus-search'|'new-meeting'|'clear-search'|null`; `moveSelection(index, delta, count)`.
- [ ] Tests: ArrowDown/ArrowUp/Enter/`' '`/`'/'` → down/up/open/toggle-reviewed/focus-search; Ctrl N → `'new-meeting'` (also in editable); Ctrl+Shift+N → null; Ctrl J → null (Task 26 owns it); Escape → `'clear-search'` only while searching; non-Ctrl keys null while editable except Escape; `moveSelection(0,-1,5)=0`, `(4,1,5)=4`, `(-1,1,5)=0`. → FAIL → implement → PASS.
- [ ] Commit `feat(table): keyboard map`.

### Task 21: `@tanstack/react-table` and browser-preview mocks
**Agent:** mechanical · **Risky:** no
**Files:** `frontend/package.json`, `pnpm-lock.yaml` (`pnpm add @tanstack/react-table@8.21.3 --save-exact`); `src/lib/browser-preview.ts:36-71`.
- [ ] Replace Task 7's row mock: `api_list_meeting_rows` returns 8 rows dated from this week's Monday: finished with gist; finished unreviewed; `'Untitled · Mon 14:02'`; not recorded with `prepPoints 2`; cancelled with prep; new from calendar with `prepPoints 3`; two older finished (finished rows `hasSummary: true`; keep ids `preview-1..3`). Add `api_save_meeting_view`, `api_delete_meeting_view`, `api_set_meeting_reviewed`, `api_set_meeting_template`, `api_delete_meeting` → `{ folder_warning: null }`, `api_search_transcripts` (hits with `totalInMeeting` for queries containing "plan", else `[]`); `api_list_templates` gains `emoji` (🤝 1:1, 🧑‍💻 Interview).
- [ ] FE, GUARD; dev:preview console has no "unmocked command" warning for these. Commit `chore(table): add @tanstack/react-table; preview mocks for table commands`.

### Task 22: `useMeetingRows` and `useMeetingViews` hooks
**Agent:** implementer · **Risky:** yes (a failed save must not look saved; optimistic edits roll back)
**Files:** create `src/hooks/useMeetingRows.ts`, `src/hooks/useMeetingViews.ts`, `tests/hooks/meeting-table-hooks.test.tsx`.
**Interfaces:** `useMeetingRows()` → `{ rows, loaded, liveId, reload, toggleReviewed(id), changeTemplate(id, templateId|null), removeMeeting(id) }` (`liveId` = `useRecordingState().liveMeetingId`; reloads on mount, `recording-started`, `recording-saved`, `recording-target`, window `focus`). `useMeetingViews()` → `{ views, active, config, dispatch, addView(), renameView(id, name), removeView(id) }` (active id in localStorage `conclave.meetingsView`, every access in try/catch).
- [ ] Tests: `toggleReviewed` flips at once, flips back and `toast.error('Could not save Reviewed', …)` on reject; `dispatch({type:'setGroup',group:'none'})` sends one `api_save_meeting_view` within 600 ms (500 ms debounce), reject → `toast.error('Could not save the view', …)`; `addView()` saves `{builtin:false, position:max+1, name:'New view'}` with the active config and activates it; `removeView` on a builtin makes no call; `removeMeeting` calls `api_delete_meeting`, drops the row, calls `useSidebar().refetchMeetings()`, and shows the `Sidebar/index.tsx:345-347` warning toast on `folder_warning`; a rejected delete toasts the error and keeps the row.
- [ ] → FAIL → implement → PASS. Commit `feat(table): rows and views hooks with saved per-view config`.

### Task 23: `MeetingsTable` rows, cells, row menu and Delete meeting dialog
**Agent:** implementer · **Risky:** yes (nothing is deleted before the confirm click)
**Files:** create `src/components/MeetingsTable/{MeetingsTable,cells,RowMenu,DeleteMeetingDialog,index}.tsx|ts`, `tests/components/meetings-table.test.tsx`; use `ui/dialog`, `ui/dropdown-menu`, `ui/popover`, `ui/command`.
**Interfaces:** `MeetingsTable({ onAsk?: () => void; onScopeChange?: (s: { viewName: string; meetingIds: string[] }) => void })`; exported `DeleteMeetingDialog({ row, open, onOpenChange, onConfirm })` (Task 36 reuses it). tanstack `useReactTable` (sizing `columnResizeMode:'onEnd'` → `setWidth`; visibility `effectiveHidden`); sections from `arrangeRows`; `useVirtualizer` (pattern `components/VirtualizedTranscriptView.tsx:201`) above **200** flattened items, spacer `<tr>`s.
- Cells: Name = emoji then title. Summary by state: recording → `Recording · ${clock(activeDuration)}`; new → `Not recorded yet` + `Calendar` badge (when `calendarUid`) + `Prep: N points`; not-recorded → `Not recorded` + `Open prep` (`meetingHref({id, tab:'prep'})`); cancelled → `Cancelled in calendar` + `· has prep notes, so it stays` + `Delete` (opens the dialog); finished → gist. Reviewed checkbox only when `hasSummary` (`aria-label` `Reviewed: <title>`). Category chip = cmdk popover of templates + `No template` → `changeTemplate`. Row menu: Open, Delete meeting… (disabled on the live row).
- Dialog: title `Delete “<title>”?`, body `This removes: ` + `deleteMeetingParts(row).join(', ')` + `. Tasks you kept stay in Tasks.`, buttons Cancel / Delete meeting.
- [ ] Tests: fixture renders `Not recorded yet`, `Prep: 3 points`, `Calendar`, `Open prep`, `Cancelled in calendar`; exactly as many checkboxes as `hasSummary` rows; opening the dialog makes no `api_delete_meeting` call, Delete meeting makes one with `{ meetingId }`, Cancel none.
- [ ] → FAIL → implement → PASS. Commit `feat(table): rows, cells, category chip, row menu, delete dialog`.

### Task 24: View tabs, toolbar, filter chips, search mode, empty states, page wiring
**Agent:** implementer · **Risky:** no
**Files:** create `MeetingsTable/ViewTabs.tsx`, `Toolbar.tsx`, `SearchResults.tsx`; edit `src/app/page.tsx` (placeholder → `<MeetingsTable />` under a `Meetings` heading); extend `tests/components/meetings-table.test.tsx`.
- View tabs `role="tablist"`, counts on non-`all` views, `+ View` → `addView`, tab menu Rename and (custom only) Delete.
- Toolbar: New meeting (`Ctrl N`) → `createMeeting()` then `router.push(meetingHref({id}))`; search takes `/`, 250 ms debounce, newest request wins (`searchSeqRef` pattern, `SidebarProvider.tsx:166-181`); Filter chips (`Date: This week (Mon 16 – Sun 22 Nov) ×`, `+ Filter`); Sort and Group menus; header menus (Sort asc/desc, Hide column, Columns checklist); Ask with `Ctrl J` hint, disabled without `onAsk`.
- Keyboard: one `keydown` listener on `/` maps `tableKeyAction` to selection, open, `toggleReviewed` (rows with `hasSummary` only), focus search, new meeting (`preventDefault`, WebView2 opens a window otherwise), clear search (`preventDefault` so the sidebar's Esc does not also close, Task 26).
- Search mode: ignores view filter and grouping, backend order; header `Searching all meetings · N matches · Esc clears`; columns `Name and where it matched`, Date, Category, Reviewed; up to 3 snippets (`snippetLabel`, `snippetHref`) + `N more in this meeting`; no hits → `Nothing mentions “<q>”. Try fewer words or another spelling.` + `Clear search`.
- Empty first run: `No meetings yet`, the spec text, `New meeting (Ctrl N)`, `Connect calendar` (→ `/settings?tab=knowledge`). Empty view: `No meetings match this view.`
- [ ] Tests: no rows → `No meetings yet`; its button calls `api_create_meeting` and pushes `/meeting-details?id=preview-new`; `zzz` → the no-match sentence, `Clear search` restores rows; two hits with total 5 → `3 more in this meeting`; `onScopeChange` gets the week view's ids; Escape while searching is `defaultPrevented`.
- [ ] → FAIL → implement → PASS. Commit `feat(table): views, toolbar, search, empty state at /`.

### Task 25: Phase 3 end check
**Agent:** implementer · **Risky:** no
- [ ] PHASE gate. dev:preview Playwright screenshots of `/` in Conclave, Dark, Light: both densities; group by Category and Day; search "plan"; no matches; empty first run (temporary `[]` mock); the delete dialog. Compare with `meetings-table.html`, `states-search.html` screens 1–3. `/impeccable audit` on `/`, fix serious findings. A11Y.
- [ ] Phase review + Codex on Tasks 16, 22, 23 (silent failures: failed saves, deletes, searches must toast). Commit fixes as `fix(table): phase review findings`.

## Phase 4: Meeting page and AI sidebar (§6.4 UI, §6.5)

Test style: `bun:test` + `react-test-renderer` with `mock.module('@tauri-apps/api/core')` (`tests/components/ask-page.test.tsx:1-40`).

### Task 26: `AiSidebar` shell and `AiSidebarProvider`
**Agent:** implementer · **Risky:** no
**Files:** create `src/components/AiSidebar/{AiSidebarProvider,AiSidebar}.tsx`, `src/lib/ai-sidebar.ts`, `tests/components/ai-sidebar.test.tsx`; `src/app/layout.tsx:272-309` (provider inside `CopilotProvider`, `<AiSidebar />` after `<MainContent>` in `div.flex :285-288`).
**Interfaces:**
```ts
export type ScreenKind = 'table' | 'live' | 'meeting';
export type AskScope = { label: string; meetingIds: string[] | null }; // null = all meetings
export function readSidebarOpen(kind: ScreenKind): boolean;   // `conclave.aiSidebar.open.${kind}`; defaults live=true, table=false, meeting=false
export function writeSidebarOpen(kind: ScreenKind, open: boolean): void; // try/catch
export function useAiSidebar(): { kind: ScreenKind | null; open: boolean; scope: AskScope | null; toggle(): void; close(): void };
export function useAiSidebarScreen(kind: ScreenKind | null, scope?: AskScope): void;
```
Ctrl J toggles; Esc closes only when open and the event is not `defaultPrevented`; `kind === null` → nothing. `<aside aria-label="AI sidebar" className="w-[360px] …">` with a body slot (Tasks 28, 30).
- [ ] Tests: `readSidebarOpen('live')` true, `'table'` false; write `table` true → `meeting` still false; rendered with `useAiSidebarScreen('table')`: Ctrl J shows the aside, Escape hides, a `defaultPrevented` Escape leaves it open; `useAiSidebarScreen(null)` → no aside.
- [ ] → FAIL → implement → PASS. Commit `feat(ai-sidebar): one right-hand panel, Ctrl J, open state per screen`.

### Task 27: `ask_meetings` takes `meeting_ids`
**Agent:** mechanical · **Risky:** no (ids only filter an in-memory map)
**Files:** `knowledge/retrieve.rs:79-92` (`retrieve`), `:357-369` (`load_meetings`); `knowledge/ask.rs:217` (`run`), `:254-266`, `:270-290`; existing callers in tests (`ask.rs:483-511`, `retrieve.rs:560-645`) pass `None`.
**Interfaces:** `retrieve(pool, plan, scope: Option<&[String]>)`; `run(…, scope: Option<&[String]>, call)`; `ask_meetings(app, request_id, question, history, meeting_ids: Option<Vec<String>>)`. After `load_meetings`, `meetings.retain(|id, _| ids.contains(id))`; `filtered` (`:87`) true when scoped; `Some(vec![])` → `NOT_FOUND`.
- [ ] Failing tests: `scope_limits_retrieval` (two meetings say "бюджет": `None` → 2; scoped → only `id_a`, non-empty; `SearchPlan::default()` scoped → non-empty); `empty_scope_is_not_found_without_answer_call` (one model call, the plan).
- [ ] **RUST** `knowledge::` → FAIL → implement → PASS. Commit `feat(ask): ask_meetings limits retrieval to given meeting ids`.

### Task 28: Ask in the sidebar; `/ask` removed; table wired
**Agent:** implementer · **Risky:** no
**Files:** `src/components/Ask/AskChat.tsx:22-159`; `AiSidebar.tsx` (body for `table` and `meeting`); `src/app/page.tsx` (`onAsk={toggle}`, `onScopeChange` → `useAiSidebarScreen('table', { label: viewName, meetingIds })`); delete `src/app/ask/page.tsx`; move `tests/components/ask-page.test.tsx` → `tests/components/ask-chat.test.tsx`; drop the `/ask` items in `components/Sidebar/index.tsx:459,531,908-912`.
**Interfaces:** `AskChat({ scope, allowAll }: { scope: AskScope; allowAll: boolean })`; `invoke('ask_meetings', { requestId, question, history, meetingIds })`; chip `"${label} · ${n} meetings"` or `"All meetings"`; with `allowAll` (table) the chip is a menu (current view / All meetings); the meeting page passes `{ label: 'This meeting', meetingIds: [id] }`, `allowAll=false`. Citation chips unchanged (`lib/ask.ts:54-74`). Changing scope keeps the conversation.
- [ ] Tests: chip `'This week · 2 meetings'`, ask sends `['a','b']`; pick All meetings → `null`; `allowAll=false` → no menu; citation click → `'/meeting-details?id=m1&line=t1'`.
- [ ] → FAIL → implement → PASS; `pnpm run build` has no `/ask`. Commit `feat(ask): Ask lives in the AI sidebar with a scope chip; /ask page removed`.

### Task 29: `copilot-alert` event to the main window
**Agent:** mechanical · **Risky:** no
**Files:** `copilot/alert.rs:18-53` (`on_transcript`), `:57-70` (`mentions`).
**Interfaces:** `pub fn matched_word(words: &[String], text: &str) -> Option<String>`; `mentions` = `matched_word(..).is_some()`; event `copilot-alert { word, line, at: f64 }` (camelCase; `at = update.audio_start_time`) via `app.emit_to("main", …)` right after the cheap gate (`:25`), before the banner's focus and cooldown checks.
- [ ] Failing test `matched_word_returns_the_alert_word` (`"the DEADLINES are"` → `"deadline"`; `"Спасибо, Саше"` → `"Саша"`; `[]` → None).
- [ ] **RUST** `copilot::alert` → FAIL → implement → PASS (existing `:120-201` tests unchanged). Commit `feat(copilot): emit copilot-alert to the main window for every mention`.

### Task 30: Copilot in the sidebar
**Agent:** implementer · **Risky:** no
**Files:** `src/components/Copilot/CopilotPane.tsx:1-110`; `src/contexts/CopilotContext.tsx:10-22` (delete `LIVE_SIDE_TAB_KEY`, `readLiveSideTab`, `writeLiveSideTab`), `:24-46`; `src/lib/copilot.ts`; `src/app/_components/TranscriptPanel.tsx:31-60`; `src/app/layout.tsx:151-163` (`copilot-open`); `AiSidebar.tsx` (`live` body); `tests/lib/copilot.test.ts`, `tests/components/copilot-pane.test.tsx`.
**Interfaces:** `type TimePart = { kind: 'text'; text } | { kind: 'time'; label; seconds }`; `splitTimes(answer): TimePart[]` (`\b(\d{1,2}:)?\d{1,2}:\d{2}\b`; minutes and seconds < 60 else text); `CopilotContext` adds `alerts: { word; line; at }[]` (from `copilot-alert`, cleared at recording start), `focusAt: number | null`, `setFocusAt(t)`.
- Answers render via `splitTimes`: text as text children, never markup; times as `<button>` → `setFocusAt`. Live `TranscriptPanel` passes `focusSegmentId={lineAtTime(segments, focusAt, false)}`.
- Header "Copilot · this call", then "Alerts: deadline, budget · edit" (`copilot_get_settings().alert_words`; edit → `/settings?tab=copilot`), or "No alert words · add". Alert cards (`Alert · 23:41`) above answers, time as a chip.
- `copilot-open` pushes `/meeting-details?id=${liveMeetingId}` and opens the sidebar for `live`; it no longer pushes `/record` or writes the removed flags. Banner unchanged.
- [ ] Tests: `splitTimes('Sam takes QA. 17:30 Then 1:02:03 ok')` → text/time(1050)/text/time(3723)/text; `'ratio 3:75'` → one text part; `copilot-alert {at:1421}` → article `Alert · 23:41`; button `12:04` → `focusAt` 724; edit → push `/settings?tab=copilot`.
- [ ] → FAIL → implement → PASS. Commit `feat(copilot): Copilot in the AI sidebar with time chips, alert line and alert cards`.

### Task 31: Prep keyed by meeting id
**Agent:** mechanical · **Risky:** yes (prep rows are written) · **Lane R; must land before Task 43**
**Files:** `prep/mod.rs:43-55` (`MeetingPrep`); `prep/store.rs:121-128` (`for_meeting`), `:140-226` (`save`), `:393-413` (`prep_save`); `src/lib/prep.ts:74-84`.
**Interfaces:** `MeetingPrep.event_uid/event_start/event_title: Option<String>`; `for_meeting` reads `WHERE p.meeting_id = ?`; `prep_save` gains `meeting_id: Option<String>` (upsert keyed on `meeting_id` when given; event columns may be NULL); `savePrep` args gain `meetingId?: string`, `eventUid`/`eventStart` optional.
- [ ] Failing test `prep_saved_for_meeting_without_event`: meeting from `MeetingsRepository::create_meeting`; `save(meeting_id=Some(id), event=None)` twice → one row, `for_meeting(id)` returns the second items; an existing calendar prep is still found by `prep_get(uid, start)`.
- [ ] **RUST** `prep::` → FAIL → implement → PASS; **FE** `tsc`. Commit `feat(prep): prep saved and read by meeting id`.

### Task 32: Pure meeting-page helpers
**Agent:** mechanical · **Risky:** no
**Files:** create `src/lib/meeting-page.ts`, `tests/lib/meeting-page.test.ts`.
**Interfaces:** `type PageView = 'new'|'recording'|'finished'`; `pageView(state: MeetingState): PageView` (recording→recording, finished→finished, else new); `splitTasks(tasks: TaskRow[], meetingId): { kept; removed }` (this meeting only; removed = owner `'unassigned'` or status `'dismissed'`); `taskKeyAction(e): 'keep'|'remove'|'up'|'down'|null` (k/K, x/X, arrows; null with a modifier). The date line uses `formatMeetingDate` (Task 17).
- [ ] Tests: one per branch, incl. `taskKeyAction({key:'k',ctrlKey:true,…}) === null`. → FAIL → implement → PASS.
- [ ] Commit `feat(meeting-page): state and task-review helpers`.

### Task 33: Page shell and the New state; autostart lands on the meeting page
**Agent:** implementer · **Risky:** yes (start path: every non-page start lands here)
**Files:** `src/app/meeting-details/page.tsx:23-312`; create `src/components/MeetingPage/{MeetingHeader,NewMeetingView,AudioLine}.tsx`; `src/lib/recording-target.ts` (`queueAutoStart`); `src/lib/resume.ts:27` (`requestResume`); `tray.rs` `autostart_script`; `tests/components/meeting-page-new.test.tsx`, `tests/lib/resume.test.ts`.
**Interfaces:** `queueAutoStart(storage, meetingId): string` (sets `autoStartMeetingId` + `autoStartRecording`, returns `/meeting-details?id=<id>`). `autostart_script` → `window.location.assign('/meeting-details?id=' + encodeURIComponent(<json id>))` with the meeting id, else the continue id. `requestResume(id, push)` pushes `/meeting-details?id=<id>`.
- The page mounts `useRecordingStart` (its autostart check re-runs when `id` changes: navigating between meeting pages keeps the page mounted).
- Row from `listMeetingRows().then(r => r.find(m => m.id === id))` (`// shortcut: one list call per page open, fine at personal scale; add a get-row command if it grows slow`).
- Header: category emoji above the title field (existing title edit in `useMeetingData`), category chip (→ `setTemplate`), `formatMeetingDate(row, now, live)`, ⋯ menu (Task 36).
- New body: "Prep notes" editor (`prepForMeeting` / `savePrep({ meetingId, … })`, `PrepTickList` look), `<AudioLine disabled={false} />` (microphone and system pickers from `DeviceSelection` with `useConfig().selectedDevices/setSelectedDevices`, `LanguageSelection` with `provider={transcriptModelConfig.provider}`, the `MicFollowingLine` warning; existing components only), "Start recording" → `handleRecordingStart(id)` with hint "Recording goes into this meeting." While another meeting is live, "Recording in <title> · Open" replaces the button.
- `useAiSidebarScreen(view === 'recording' ? 'live' : 'meeting', …)`.
- [ ] Tests: a row without `startedAt` → 'Start recording', Prep notes, Microphone/System audio/Language labels; click starts with `meetingId 'm1'`; emoji '🤝' precedes the title input; `queueAutoStart` sets both keys; `requestResume('m1')` pushes `/meeting-details?id=m1`; Rust `autostart_script_sets_meeting_id` expects the encoded meeting-page URL, continue → the continue id's page.
- [ ] **FE**, **RUST** `tray::` → FAIL → implement → PASS. Commit `feat(meeting-page): header, New state with prep, Audio line and Start recording`.

### Task 34: The Recording state
**Agent:** implementer · **Risky:** yes (moves the stop/pause path off the record screen; a lost stop loses a recording)
**Files:** create `src/components/MeetingPage/RecordingView.tsx`; reuse `src/app/_components/LiveTranscriptAndNotes.tsx` (drop the Copilot tab, the collapse flag `:29,47,60-70`, `readFlag/writeFlag`; add `tabbed: boolean`), `components/RecordingControls.tsx` (stop path `:150-160` untouched), `StatusOverlays`, `useRecordingStop`; `tests/components/meeting-page-recording.test.tsx`.
- Bar: "Recording mm:ss" (`formatRecordingTime(activeDuration)`), Pause/Resume, "Stop and summarize", `<AudioLine disabled />`. Below: live transcript and My notes; `tabbed = useAiSidebar().open` → My notes is a tab next to Transcript, else two columns. Prep points under My notes (`prepForMeeting(liveMeetingId)`). Shown when `liveMeetingId === id`.
- [ ] Tests: `isRecording`, `activeDuration: 73`, `liveMeetingId: 'm1'` → 'Recording 01:13', 'Pause', 'Stop and summarize'; sidebar open → tabs 'Transcript'/'My notes'; closed → no tablist, both panes; Stop calls `stop_recording` once and the page stays mounted when the save returns the same id.
- [ ] → FAIL → implement → PASS. Commit `feat(meeting-page): Recording state with live transcript, notes and Audio line`.

### Task 35: The Finished state, tabs and task review
**Agent:** implementer · **Risky:** no
**Files:** `src/app/meeting-details/page-content.tsx:86-97`, `:220-236`, `:283-389`; replace `MeetingDetailsSplitView` with `src/components/MeetingPage/FinishedTabs.tsx`; create `MeetingPage/MeetingTasksPanel.tsx`, `PrepTab` gets `meetingId` and `briefSlot`; `tests/components/meeting-page-finished.test.tsx`.
**Interfaces:** `type MeetingDetailsTab = 'summary'|'transcript'|'notes'|'prep'` (default `'summary'`; `line`/`t` opens `'transcript'`; `tab=` opens its tab). Prep tab: `<PrepTab meetingId briefSlot={null} />` (Task 49 fills it).
- Tasks panel "Tasks from this meeting": `tasks_list('open'|'done'|'dismissed')` through `splitTasks`. Removed rows show "removed by AI" (unassigned) or "removed" + Restore (`tasks_claim`, plus `tasks_set_status(open)` when dismissed). K → `tasks_claim`; X → `tasks_set_status(id,'dismissed')`; ↑↓ move focus (only while focus is in the panel). ("Keep all mine" was dropped 2026-11-17: every kept row is already the user's, so it did nothing.) "Done reviewing" → `setReviewed(id, true)`, then reads "Reviewed ✓" and unticks on click.
- [ ] Tests: opens on Summary; `?t=24` with segments at 0,12,24 → Transcript and `focusSegmentId 't-2'`; `?tab=prep` → Prep; an unassigned task under Removed with Restore; `k` → `tasks_claim {id}`; `x` → `tasks_set_status {id, status:'dismissed'}`; Done reviewing → `api_set_meeting_reviewed {meetingId:'m1', reviewed:true}`. Existing `prep-tab.test.tsx`, `summary-speaker-gate.test.tsx` pass (imports only).
- [ ] → FAIL → implement → PASS. Commit `feat(meeting-page): Finished state with Summary, Transcript, My notes, Prep and task review`.

### Task 36: Page menu (two deletes) and category change
**Agent:** implementer · **Risky:** yes (deletion)
**Files:** `src/components/MeetingPage/MeetingHeader.tsx`; create `MeetingPage/DeleteRecordingDialog.tsx`; reuse `DeleteMeetingDialog` from `@/components/MeetingsTable`; `tests/components/meeting-page-menu.test.tsx`.
- ⋯ menu: "Delete recording" (only when finished) and "Delete meeting"; disabled while `liveMeetingId === id`.
- Delete recording dialog: "Removes the transcript, audio, summary and tasks you did not keep. Title, category, prep and notes stay." → `deleteRecording(id)`, reload (New view). Delete meeting → `deleteMeeting(id)` then `router.push('/')`. Errors toast; the page stays.
- Category change on a finished meeting: `setTemplate(id, t)`, then a toast action "Regenerate summary with <name>" → `regenerateSummary(t)` (`page-content.tsx`, `pickTemplateIfAllowed`); never automatic.
- [ ] Tests: Delete meeting makes no call until Delete is clicked, then `api_delete_meeting {meetingId:'m1'}` and push '/'; Cancel → no calls; finished offers Delete recording, New does not; category change calls `api_set_meeting_template`, shows 'Regenerate summary with 1:1', no `api_process_transcript`.
- [ ] → FAIL → implement → PASS. Commit `feat(meeting-page): Delete recording / Delete meeting with confirmation, category relabel`.

### Task 37: Remove the two layouts, their setting and the collapse flag
**Agent:** mechanical · **Risky:** no
**Files:** delete `src/components/MeetingDetails/MeetingDetailsSplitView.tsx`, `tests/components/meeting-details-layout.test.tsx`; `src/lib/meeting-page-layout.ts` (delete `:1-19`, `:35-46`; keep `readStoredRatio`/`writeStoredRatio` for `hooks/useSplitRatio.tsx:3`); `tests/lib/meeting-page-layout.test.ts` (keep `:15-24`); `src/components/PreferenceSettings.tsx:10,26-28,142-150`.
- [ ] `grep -rn "conclave.notesPaneCollapsed\|readMeetingPageLayout\|LiveSideTab" frontend/src` prints nothing; FE, `pnpm run build` PASS. Commit `refactor(meeting-page): remove the old layouts, their setting and the notes collapse flag`.

### Task 38: Browser-preview mocks and a11y for the meeting page
**Agent:** mechanical · **Risky:** no
**Files:** `src/lib/browser-preview.ts`.
- [ ] Extend Task 21's rows: `preview-1` finished (times, `standard_meeting` 📝, `hasSummary`, three tasks: me, unassigned, dismissed), `preview-new` (no times); handlers `api_delete_recording`, `tasks_list({status})`, `tasks_claim`, `tasks_set_status`, `prep_for_meeting`, `prep_save`, `ask_meetings` (one answer citing preview-1), `copilot_get_settings` (`{ alert_words: ['deadline'] }`). `get_recording_state` stays idle.
- [ ] A11Y on `/meeting-details?id=preview-1` and `?id=preview-new`; open the sidebar with Ctrl J in Playwright and rerun. GUARD. PHASE gate; phase review + Codex on Tasks 33, 34, 36.
- [ ] Commit `chore(preview): mocks for meeting page states and the AI sidebar`.

## Phase 5: Calendar 📞 meetings, pop-up routing, briefs in Prep, Digests (§6.6, §6.7)

Tasks 39–47 run in Lane R in parallel with Phases 3–4 (Task 43 after 31). Pinned: every `calendar_start/calendar_end/calendar_original_start` and `meeting_preps.event_start` the reconcile writes is `DateTime<Utc>::to_rfc3339()`; comparisons parse both sides, never compare strings; briefs keep `briefs::start_key` (`briefs.rs:67`); cross-format lookups use SQLite `datetime(a) = datetime(b)`. Window: `from = now − CALENDAR_BEHIND_DAYS`, `to = now + RECONCILE_AHEAD_DAYS` (`pub const RECONCILE_AHEAD_DAYS: i64 = 7;` in `knowledge/mod.rs` near `:24`).

### Task 39: `Occurrence.original_start` and cache version 2
**Agent:** mechanical · **Risky:** no
**Files:** `knowledge/calendar.rs` (struct `:44-53`, `Cache :65-70`, master push `:181-187`, `occurrence() :295-302`, `parse_cache :748-750`, test `:960-966`); Occurrence literals in tests (`calendar.rs:895,1599`, `scheduler.rs:144`, `briefs.rs:284`).
**Interfaces:** `pub original_start: DateTime<Utc>`; `Cache { #[serde(default)] version: u32, … }`; `const CACHE_VERSION: u32 = 2;`.
- [ ] Failing tests: `moved_instance_keeps_its_recurrence_id` (weekly 10:00 series + `RECURRENCE-ID 2026-10-29T10:00Z` override at 11:00 → `original_start` 10:00; plain instance `original_start == start`); `version_1_cache_is_dropped` (no version → `None`, `"version":2` → `Some`; replaces `old_occurrence_json_defaults_to_nonrecurring`).
- [ ] **RUST** `knowledge::calendar` → FAIL. Implement: master `original_start: utc`; `occurrence(e)` → `e.recurrence_id.unwrap_or(e.start.with_timezone(&Utc))`; `store()` writes `version`; `parse_cache` filters on `CACHE_VERSION`; `load_at_startup` logs `info!("calendar: old cache format, refetching")` and starts empty. → **RUST** `knowledge::` PASS.
- [ ] Commit `feat(calendar): occurrence original start, cache format v2`.

### Task 40: Expansion reports completeness; refresh returns it
**Agent:** mechanical · **Risky:** no
**Files:** `calendar.rs` (`expand :93-108`, `expand_here :110-215`, `refresh :655-666`, `fetch_and_expand :668-682`, `calendar_connect :796`, test calls `:892,927,987,1587,1620,1644`), `scheduler.rs:74`.
**Interfaces:** `#[derive(Clone, Debug)] pub struct ExpandResult { pub occurrences: Vec<Occurrence>, pub complete: bool }`; `expand(ics, from, to) -> Result<ExpandResult, String>`; `refresh(app, now) -> Result<ExpandResult, String>`; `fetch_and_expand(url, now)`.
- [ ] Failing tests: `budget_skip_is_incomplete` (series over `MAX_SERIES_WORK`, as `:1029`); `plain_feed_is_complete`.
- [ ] **RUST** `knowledge::calendar` → FAIL. Implement `complete = !timed_out && skipped == 0 && out.len() < MAX_FEED_OCCURRENCES` (`:204-214`, warnings stay); one `now` for expansion and reconcile; `refresh` stores `occurrences.clone()` and returns the result; `calendar_connect` passes `Utc::now()`. → PASS.
- [ ] Commit `feat(calendar): expansion reports completeness`.

### Task 41: Marker setting, `strip_marker`, 15-minute title window
**Agent:** mechanical · **Risky:** no
**Files:** `knowledge/settings.rs` (struct `:31-46`, Debug `:49-65`, Default `:67-80`, View `:86-94`, Input `:97-103`, `sanitize :113-133`, `normalise :181`, `apply_input :241-252`, `view :334-343`), `knowledge/mod.rs:28`, `calendar.rs` (`event_for_start :823-829`, `calendar_title_now :839-846`, test `:1668-1680`).
**Interfaces:** `KnowledgeSettings.calendar_marker: String` (default `"📞"`), `KnowledgeSettingsView.calendar_marker`, `KnowledgeSettingsInput.calendar_marker: Option<String>` (`#[serde(default)]`, None keeps); `pub const DEFAULT_CALENDAR_MARKER: &str = "📞";`; `pub fn strip_marker(title: &str, marker: &str) -> Option<String>`; `AUTO_TITLE_LEAD_MINS = 15`.
- [ ] Failing tests: `strip_marker("📞 Sync with Ivan")` → `"Sync with Ivan"`; `"Sync 📞  Ivan"` → `"Sync Ivan"`; `"📞"` → `"Call"`; `"Lunch"` → None; `sanitize` empty marker → `"📞"`; strict `normalise` of `"  "` → Err; `event_for_start_window`: fifteen before matches, sixteen does not.
- [ ] **RUST** `knowledge::` → FAIL. Implement: remove every occurrence, collapse whitespace, trim, empty → `"Call"`; `normalise` trims, empty or > 16 chars → Err "The calls marker must be 1–16 characters" when strict, default otherwise; `calendar_title_now` returns the stripped title. → PASS.
- [ ] Commit `feat(calendar): calls marker setting, 15-minute title window`.

### Task 42: Pure `calendar::reconcile::plan`
**Agent:** implementer · **Risky:** yes · **Difficult:** yes (deletion decisions)
**Files:** create `knowledge/calendar/reconcile.rs`; `pub mod reconcile;` at the top of `calendar.rs`.
**Interfaces:**
```rust
pub struct CalRow { pub id: String, pub title: String, pub title_source: String, pub calendar_uid: String,
  pub calendar_start: Option<String>, pub calendar_original_start: Option<String>, pub calendar_marker: Option<String>,
  pub calendar_cancelled_at: Option<String>, pub started_at: Option<String>, pub untouched: bool }
pub struct Snapshot { pub meetings: Vec<CalRow>, pub dismissed: Vec<(String, String)> }
pub struct Window { pub from: DateTime<Utc>, pub to: DateTime<Utc> }
pub enum DeleteReason { Gone, Past }
#[derive(Debug, PartialEq)] pub enum Action {
  Adopt { id: String, original_start: String }, Insert { uid: String, title: String, start: String, end: String, original_start: String },
  Move { id: String, start: String, end: String }, Rename { id: String, title: String },
  Cancel { id: String }, Uncancel { id: String }, Delete { id: String, reason: DeleteReason } }
pub fn plan(events: &ExpandResult, rows: &Snapshot, live_target: Option<&str>, marker: &str, window: &Window, now: DateTime<Utc>) -> Vec<Action>
```
Rules in order: (1) 📞 events = `strip_marker(title, marker).is_some()`. (2) Frozen: the live target or a row with `started_at` gets no action, but still consumes its matching event. (3) Adopt: `calendar_marker` NULL, same uid, parsed `calendar_start` == event start → `Adopt` with the event's `original_start`, then treated as a current-marker row. (4) Key: recurring → uid + `calendar_original_start == original_start`; one-off → uid alone; rows with another non-NULL marker are never matched or touched. (5) Matched: `Move` if start/end differ, `Rename` if the stripped title differs and `title_source != "user"`, `Uncancel` if cancelled. (6) Unmatched event: `Insert` only if `end > now && start < window.to && start + 1 day > now` and not dismissed (uid + `original_start` for recurring, uid for one-off). (7) Unmatched current-marker row, only if `events.complete`: `calendar_start + 1 day <= now` → untouched `Delete{Past}`, else nothing; `calendar_start` in `[from, to)` → untouched `Delete{Gone}`, content and not cancelled → `Cancel`; outside → nothing.
- [ ] Failing table-driven test `plan_rules` (`struct Case { name, events, complete, rows, dismissed, live, marker, want }`): `new_event_inserts`, `non_marker_event_ignored`, `moved_recurring_by_original_start_moves`, `one_off_moved_matches_on_uid`, `renamed_updates_title`, `user_title_not_renamed`, `gone_untouched_deletes`, `gone_with_content_cancels`, `already_cancelled_not_recancelled`, `reappear_uncancels`, `past_untouched_deletes`, `past_with_content_stays`, `live_target_untouched`, `started_at_row_untouched_and_not_reinserted`, `incomplete_never_cancels_or_deletes` (insert still made), `outside_window_never_cancelled`, `marker_change_leaves_old_rows`, `migrated_prep_row_adopted_no_duplicate`, `migrated_row_not_matching_left_alone`, `dismissed_recurring_not_reinserted`, `dismissed_one_off_matches_uid_alone`.
- [ ] **RUST** `calendar::reconcile` → FAIL → implement → PASS. Commit `feat(calendar): pure reconcile plan`.

### Task 43: Reconcile executor with in-statement re-checks
**Agent:** implementer · **Risky:** yes (deletes meeting rows; concurrent prep saves) · after Task 31
**Files:** `knowledge/calendar/reconcile.rs`.
**Interfaces:** `pub async fn snapshot(pool) -> Result<Snapshot, String>` (`(<UNTOUCHED_SQL>) AS untouched`, reusing `database::repositories::meeting::UNTOUCHED_SQL` from Task 10); `pub async fn apply(pool, actions, marker, now) -> Result<usize, String>`; `pub async fn run(pool, events: &ExpandResult, live_target, marker, window, now) -> Result<usize, String>`. Guard G = `started_at IS NULL AND calendar_marker = ?marker`.
- Adopt: `UPDATE meetings SET calendar_marker=?, calendar_original_start=? WHERE id=? AND calendar_marker IS NULL AND started_at IS NULL`.
- Insert: `INSERT INTO meetings (id,title,title_source,created_at,updated_at,calendar_uid,calendar_start,calendar_end,calendar_original_start,calendar_marker) SELECT ?,?,'calendar',?,?,?,?,?,?,? WHERE NOT EXISTS (dismissed match) AND NOT EXISTS (SELECT 1 FROM meetings WHERE calendar_uid=? AND calendar_original_start=?)` (id `meeting-{uuid}`, timestamps `.bind(now)`).
- Move (one tx): `UPDATE meetings SET calendar_start=?, calendar_end=? WHERE id=? AND G`; if a row changed, `UPDATE meeting_preps SET event_start=? WHERE meeting_id=? AND NOT EXISTS (SELECT 1 FROM meeting_preps q WHERE q.event_uid=meeting_preps.event_uid AND q.event_start=? AND q.meeting_id IS NOT meeting_preps.meeting_id)` (a prep that cannot take the slot keeps its `meeting_id` link).
- Rename: `… SET title=? WHERE id=? AND G AND title_source <> 'user'`. Cancel: `… SET calendar_cancelled_at=? WHERE id=? AND G AND calendar_cancelled_at IS NULL`. Uncancel: `… = NULL WHERE id=? AND G`.
- Delete (one tx): `DELETE FROM meetings AS m WHERE m.id=? AND m.calendar_marker=? AND <UNTOUCHED_SQL>`; if removed, delete its `meeting_preps` and `meeting_notes` rows (empty by the guard); if nothing removed and `Gone`, run the Cancel statement.
- [ ] Failing tests on `test_pool()`: `concurrent_prep_save_wins_over_delete` (plan `Delete{Gone}`, save prep material, apply → row kept, cancelled, material unchanged); `delete_removes_untouched_row_and_empty_prep`; `rename_after_user_rename_is_skipped`; `move_carries_prep_event_start` (one-off); `move_with_slot_taken_keeps_prep_link`; `started_at_written_after_snapshot_blocks_all` (move, rename, cancel, delete); `insert_skips_occurrence_dismissed_after_snapshot`; `adopt_then_rerun_inserts_nothing`.
- [ ] **RUST** `calendar::reconcile` → FAIL → implement → PASS. Commit `feat(calendar): reconcile executor with in-statement re-checks`.

### Task 44: Run after each successful refresh; retry a failed reconcile; last sync error
**Agent:** implementer · **Risky:** yes (decides when deletion may run)
**Files:** `knowledge/scheduler.rs:22-26` (`Clock`), `:65-87` (`tick`); `calendar.rs` (`State :72-76`, `CalendarStatus :56-63`, `record_error :711-717`, `store :692-709`, `status :777-787`).
**Interfaces:** `CalendarStatus.last_error_at: Option<String>`; `pub(crate) fn record_sync_error(problem: String)` (only while connected); `Clock.pending: Option<ExpandResult>`; `pub(crate) async fn after_refresh(pool, result: Result<ExpandResult, String>, live, marker, now) -> Result<(), String>`.
**Behaviour:** `refreshed_at` is still set before the fetch, as today, so a failed fetch waits for the next 30-min refresh. A successful fetch puts its result in `clock.pending`. Each tick, while `pending` is `Some`, run `reconcile::run` with `Window { from: now − CALENDAR_BEHIND_DAYS, to: now + RECONCILE_AHEAD_DAYS }`, `settings::current().calendar_marker` and `recording_target_id()`; success clears `pending`; failure → `record_sync_error(e)` and keeps `pending` for the next tick (no refetch). A fetch error → `record_sync_error`. `calendar_disconnect` and a marker change call nothing.
- [ ] Failing tests: `reconcile_runs_only_on_success` (`Err` → no DB change, Err; `Ok` with a 📞 event inserts one row); `failed_fetch_waits_for_next_refresh` (`refreshed_at` set after a failed fetch, `pending` None); `failed_reconcile_retries_from_pending_without_refetch` (closed pool on first run keeps `pending`; next tick with a good pool inserts the row and does not call refresh); `record_error` sets `last_error_at`, `store` clears both.
- [ ] **RUST** `knowledge::` → FAIL → implement → PASS. Commit `feat(calendar): reconcile after each refresh, retry from the last feed, sync error time`.

### Task 45: Remove the old prep mover and `/prep` backend
**Agent:** mechanical · **Risky:** no
**Files:** delete `prep/upcoming.rs` (`moves`, `relocate`, `overview`, `prep_overview` and tests); `prep/mod.rs:5`; `lib.rs:1067`; `calendar.rs:641-646` (`HAS_REFRESHED`/`has_refreshed`) and the `store` line `:707`.
- [ ] `cargo check`, **RUST** `prep::`, GUARD. Commit `refactor(prep): remove relocate and prep overview (superseded by reconcile)`.

### Task 46: Briefs only for 📞 meetings; per-meeting brief commands
**Agent:** implementer · **Risky:** no
**Files:** `scheduler.rs:90-112` (`make_due_briefs`), `knowledge/briefs.rs` (after `:265`), `lib.rs:1042-1044`.
**Interfaces:** `#[serde(rename_all="camelCase")] pub struct MeetingBrief { pub is_call: bool, pub brief_id: Option<String> }`; `brief_for_meeting(meeting_id) -> Result<MeetingBrief, String>` (`is_call = calendar_marker IS NOT NULL`; `briefs b JOIN meetings m ON b.event_uid = m.calendar_uid AND datetime(b.event_start) = datetime(m.calendar_start)`); `brief_regenerate(app, meeting_id) -> Result<Option<String>, String>`; `brief_meeting(brief_id) -> Result<Option<String>, String>` (reverse join).
- [ ] Failing tests: `only_marker_events_get_briefs` ("📞 1:1 Ivan" and "1:1 Ivan" → one brief, title "1:1 Ivan"); `brief_for_meeting_finds_by_uid_and_start` (`+00:00` row vs `Z` brief); `regenerate_replaces_brief_outside_due_window` (3 days ahead → `Some`, old brief and feed item gone); `regenerate_refuses_non_call_row` → Err "Briefs are made for 📞 calls only".
- [ ] **RUST** `knowledge::` → FAIL. Implement: `make_due_briefs` skips non-📞 occurrences and passes the stripped title; `brief_regenerate` builds `Occurrence { uid, title, start, end, recurring: false, original_start }`, needs `event_type(title, list_templates())` on `spawn_blocking` (none → Err "No template matches this title, so there is no earlier meeting to brief from", old brief kept), deletes the brief and `feed_items (kind='brief', target_id)` in one tx, calls `make_brief(app, pool, &occ, t, &|| occ.start - Duration::minutes(1))`. → PASS.
- [ ] Commit `feat(briefs): briefs for 📞 meetings, per-meeting lookup and regenerate`.

### Task 47: Pop-up routing (§6.7) inside `pick_target`
**Agent:** implementer · **Risky:** yes · **Difficult:** yes (a recording saved into the wrong meeting)
**Files:** `recording/route.rs`.
**Interfaces:** `struct CallRow { id, title, start: DateTime<Utc>, end: DateTime<Utc>, recorded: bool }` (`recorded` = has transcripts or `started_at`); `fn pick_call(rows: &[CallRow], now) -> Option<RouteTarget>`. Rows: `calendar_marker = settings::current().calendar_marker AND calendar_cancelled_at IS NULL`. Candidates `start − 15 min <= now < end`; not-recorded first, closest `|start − now|` → `Existing(id)`; all recorded → closest → `New { title: Some(format!("{title} · part 2")) }` (no calendar link: `create_meeting` never sets one); none → `None` (→ `New { title: None }`). Applies to `Popup`, `Tray`, `AutoDetect`; `Sidebar` always new; `Page` unchanged.
- [ ] Failing table test `route_cases`: for Popup, Tray, AutoDetect: `in_progress → Existing`, `starts_in_15 → Existing`, `starts_in_16 → New{None}`, `overlap_closest_wins`, `overlap_fresh_beats_closer_finished`, `finished → New{Some("Sync · part 2")}`, `cancelled_row_ignored`, `old_marker_row_ignored`; Sidebar `due_call_still_new`; Page `page_id_wins`.
- [ ] **RUST** `recording::route` → FAIL → implement → PASS. Commit `feat(recording): route pop-up, tray and auto-start into the due 📞 meeting`.

### Task 48: Settings → Calendar: marker field and last error time
**Agent:** mechanical · **Risky:** no
**Files:** `src/components/KnowledgeSettings.tsx` (`CalendarStatus ~:20-31`, status block `:225-236`, save), `tests/components/knowledge-settings.test.tsx`, `src/lib/browser-preview.ts`.
- [ ] Failing tests: "My calls marker" shows `📞` and saves `calendar_marker: '☎'` via `knowledge_save_settings`; `last_error` + `last_error_at` render `Last error (<local time>): …`; an empty marker shows the backend error toast.
- [ ] Implement: help text "Only events whose title contains it become meetings. Changing it never removes existing meetings."; `last_error_at` in the type and `NO_CALENDAR`; mocks. FE, A11Y → PASS. Commit `feat(settings): calls marker and last sync error time`.

### Task 49: Brief in the Prep tab, Digests page, `/prep` removed
**Agent:** implementer · **Risky:** no
**Files:** create `src/components/Briefs/MeetingBrief.tsx`; `PrepTab` (Task 35) renders `<MeetingBrief meetingId={id} />` in `briefSlot`; `git mv src/app/briefs/page.tsx src/app/digests/page.tsx`; `src/components/Feed/FeedBell.tsx:53-63`; delete `src/app/prep/page.tsx`, `src/components/Prep/PrepPage.tsx`; `src/lib/prep.ts` (drop `PrepBadge`, `UpcomingEvent`, `OtherPrep`, `PrepOverview`, `prepOverview`, `groupByDay` if unused); `components/Sidebar/index.tsx:49,461,502-525,880-900` (`/briefs` → `/digests`, drop `/prep`); tests `briefs-page.test.tsx` → `digests-page.test.tsx`, delete `prep-page.test.tsx`, drop `groupByDay` tests in `tests/lib/prep.test.ts`; `browser-preview.ts`.
- [ ] Failing tests: `MeetingBrief` `isCall:false` → nothing; with `briefId` → `BriefView`; none → "No brief yet"; "Regenerate brief" calls `brief_regenerate`, Err shows its text. Digests page lists only `kind==='digest'`, heading "Digests". FeedBell: brief → `brief_meeting` → `/meeting-details?id=<m>&tab=prep`; digest → `/digests?id=`.
- [ ] → FAIL → implement → FE, A11Y, GUARD PASS. PHASE gate; phase review + Codex on Tasks 42–44, 47. Commit `feat(ui): brief in meeting Prep tab, Digests page, remove /prep`.

## Phase 6: Navigation, What's new, removals, final QA (§6.8, §8)

### Task 50: One nav list for rail and expanded sidebar
**Agent:** implementer · **Risky:** no
**Files:** create `src/lib/nav.ts`, `tests/lib/nav.test.ts`, `tests/components/sidebar-nav.test.tsx`; rewrite `src/components/Sidebar/index.tsx` (one `items.map` for both widths; delete the meetings list, search box, mic button, `Logo`, `Info`, the delete/edit dialogs, the `SettingTabs` import; keep `<FeedBell>` beside Settings, collapse toggle, `go()`/`confirmLeave`); delete `tests/components/sidebar-search-order.test.tsx`.
**Interfaces:**
```ts
export type NavItem = { label: string; href: string; icon: LucideIcon; match: string[] };
export const NAV_MAIN: NavItem[]; // Meetings '/' (match '/', '/meeting-details'), Tasks '/tasks', Digests '/digests', Dictate '/dictate', Practice '/practice', Stats '/stats'
export function isNavActive(pathname: string, item: NavItem): boolean;
export function recordLabel(isRecording: boolean, activeSecs: number | null): string; // 'Record new meeting' | `Recording ${formatClock(secs)}`
export const WHATS_NEW_SEEN_KEY = 'whats-new-seen';
export function whatsNewUnseen(seen: string | null, version: string | null): boolean;
```
Bottom: Import audio → `openImportDialog()`, Settings, `What's new in v{version}` → `/whats-new` with a dot (`aria-hidden` + `sr-only` " (new)") while unseen; re-read on window event `whats-new-seen`. Expanded: visible labels; collapsed: icon + `Tooltip side="right"` + `aria-label`; active `aria-current="page"`; `<nav aria-label="Main">`. Record: not recording → `routeRecordingStart('sidebar')`, `queueAutoStart(sessionStorage, meetingId)`, `go(url)` (error → `toast.error('Could not start recording', …)`); recording → `recordLabel`, click → `go('/meeting-details?id=' + liveMeetingId)` (disabled while `liveMeetingId` is null).
- [ ] Failing tests: `NAV_MAIN` labels `['Meetings','Tasks','Digests','Dictate','Practice','Stats']`; `isNavActive('/meeting-details', NAV_MAIN[0])` true, `'/ask'` false; `recordLabel(false,null)`, `(true,192)` → `'Recording 03:12'`; `whatsNewUnseen` (null,'2.0.0') true, equal false, (null,null) false. Component (mock `SidebarProvider`, `RecordingStateContext`, `next/navigation`, `ImportDialogContext`, `invoke` with `'plugin:app|version'` → `'2.0.0'` and `api_route_recording_start`): expanded button texts in order `['Record new meeting','Meetings','Tasks','Digests','Dictate','Practice','Stats','Import audio','Settings',"What's new in v2.0.0"]`; collapsed each has its `aria-label`, nothing named `Ask`, `Prep`, `About`, `Briefs & digests`; live `m1` at 192 s → `Recording 03:12`, click → push `/meeting-details?id=m1`; idle click → `api_route_recording_start {source:'sidebar'}` then push of the meeting page; dot gone after `whats-new-seen` with `'2.0.0'`.
- [ ] → FAIL → implement → PASS, full `bun test`, `pnpm run build`. Commit `feat(nav): single left nav with Record, Digests and What's new; drop sidebar list and mic button`.

### Task 51: Remove the old sidebar state, About, and the record screen
**Agent:** implementer · **Risky:** yes (edits the start and save hooks; a wrong removal can stop a save)
**Files:** `src/components/Sidebar/SidebarProvider.tsx` (drop `currentMeeting`/`setCurrentMeeting`, `sidebarItems`, `meetings`/`setMeetings`/`fetchMeetings`/`refetchMeetings`, `isMeetingActive`/`setIsMeetingActive`, `handleRecordingToggle`, the search trio; keep `isCollapsed`, `toggleCollapse`, `serverAddress`, summary polling, the `continue-target-failed` listener); callers `meeting-details/page.tsx`, `hooks/useNavigation.ts`, `hooks/useRecordingStop.ts` (also the `'New Meeting'` fallbacks `:260`, `:341` → `'Untitled'`), `hooks/useRecordingStart.ts`, `hooks/meeting-details/useMeetingData.ts:84-89`, `ImportAudio/ImportAudioDialog.tsx`, `hooks/useMeetingRows.ts` and its test (drop the `refetchMeetings` call), `app/_components/StartupChecks.tsx`; `app/layout.tsx:130-149` (dead `request-recording-toggle` listener); delete `src/app/record/` (the temporary route), `components/{Logo,Info,About,SettingTabs,CustomDialog}.tsx`, `components/Sidebar/SearchHits.tsx` + `tests/components/search-hits.test.tsx`, `pickStartTitle` + its test if unused, and each of `app/_components/{ContinueControl,LiveTranscriptAndNotes,SettingsModal,StatusOverlays,TranscriptPanel}.tsx`, `components/{RecordingControls,MicFollowingLine}.tsx`, `hooks/useModalState.ts` (+ tests) **only if** `grep -rn "<basename>" frontend/src` finds no importer (Tasks 33–34 reuse several).
- [ ] Guard: `grep -rnw <name> frontend/src` for every dropped field; anything read for more than the old sidebar stays and is reported.
- [ ] Failing test `tests/components/sidebar-provider.test.tsx`: `invoke` never receives `api_get_meetings`; sorted context keys `['isCollapsed','serverAddress','startSummaryPolling','stopSummaryPolling','toggleCollapse']`. → FAIL.
- [ ] Remove; `tsc` finds stale callers. FE PASS (recording-stop, summary-generation, meeting-details suites unchanged), `pnpm run build` has no `/record`.
- [ ] Commit `refactor(ui): remove sidebar meeting list state, About dialogs and the old record screen`.

### Task 52: `open_log_file` command
**Agent:** mechanical · **Risky:** no
**Files:** `database/commands.rs` (after `open_database_folder :219-234`); `lib.rs` after `:1000`; `src/lib/browser-preview.ts` (`open_log_file`, `open_database_folder` → null, `'plugin:app|version'` → `'2.0.0'`).
**Interfaces:** `pub(crate) fn log_file_path(log_dir: &Path, app_name: &str) -> PathBuf` (`<dir>/<name>.log`); `#[tauri::command] pub async fn open_log_file(app: AppHandle) -> Result<(), String>` (path from `app.path().app_log_dir()` and `package_info().name`; exists → `open_folder(&path)`, else `create_dir_all` + open the dir).
- [ ] Failing test `log_file_path` (`"L"`, `"Conclave"` → `L/Conclave.log`). **RUST** → FAIL → implement, register → PASS. Commit `feat(app): open the log file from the app`.

### Task 53: CHANGELOG v2.0.0
**Decided 2026-11-17 (user):** the redesign ships as **v2.0.0**. Also add a `## v1.2.0` section above v1.1.0 for the already-merged safe updates (keyring secrets, pre-upgrade backups, signed in-app updater). What's new lists every section.
**Agent:** mechanical · **Risky:** no
**Files:** `CHANGELOG.md` (above the newest section), `scripts/release-scripts.test.mjs:62` (`'1.1.0'` → `'2.0.0'`).
```
## v2.0.0 — <commit date, YYYY-MM-DD>
- **Meetings table.** All meetings in one table with saved views, search across transcripts, and grouping by day or category.
- **Create a meeting first.** Name it, write prep notes, then press Record; calendar meetings appear on their own.
- **One AI sidebar.** Ask and Copilot open beside the table or meeting (Ctrl J); Digests replace Briefs in the menu.
- **What's new** in the menu shows these notes; version, data folder and log file moved there from About.
```
- [ ] Change the test → `node --test scripts/*.test.mjs` FAIL → add the section → PASS; `node scripts/changelog-section.mjs 2.0.0` prints the four bullets. Commit `docs(changelog): v2.0.0`.

### Task 54: What's new page with the About footer
**Agent:** implementer · **Risky:** no (bundled file, no network)
**Files:** create `src/lib/changelog.ts` (`type Release = { version; date; items: string[] }`; `parseChangelog(md)`: `/^## v(\d+\.\d+\.\d+) — (.+)$/`, bullets `/^- (.+)$/`, newest first by numeric semver); `src/components/WhatsNew/WhatsNew.tsx` (`'use client'`, `{ changelog: string }`: one `<article>` per release, `**x**` → `<b>`, everything else text; the `getVersion()` release gets `Installed` + `aria-current="true"`; on mount sets `WHATS_NEW_SEEN_KEY` (try/catch) and dispatches `whats-new-seen`; footer `Conclave v{version} · Based on Meetily (MIT) by Zackriya Solutions ·` + `Open data folder` / `Open log file`, failures `toast.error`); `src/app/whats-new/page.tsx` as a **server component** returning `<WhatsNew changelog={readFileSync(join(process.cwd(), '..', 'CHANGELOG.md'), 'utf8')} />`.
- [ ] Failing tests: older-first sample → `['2.0.0','1.1.0']`; `items[0]` keeps `**Meetings table.**`; `## Unreleased` and prose skipped; `1.10.0` above `1.9.0`; component with version `'1.1.0'`: only v1.1.0 has `Installed`, the key becomes `'1.1.0'`, footer has `Based on Meetily (MIT)`, `Open log file` invokes `open_log_file`, a rejection toasts, `<img src=x>` renders as text.
- [ ] → FAIL → implement → PASS; `pnpm run build`, `grep -c "v2.0.0" frontend/out/whats-new.html` ≥ 1. Commit `feat(whats-new): bundled changelog page with version, credit, data folder and log file`.

### Task 55: Final UI QA (§8 frontend)
**Agent:** implementer · **Risky:** no
- [ ] dev:preview; open `/`, `/meeting-details?id=preview-1`, `/tasks`, `/digests`, `/dictate`, `/practice`, `/stats`, `/settings`, `/whats-new` with Playwright MCP; every "unmocked command" warning gets a realistic handler. Commit `chore(preview): mocks for every redesign command`.
- [ ] Screenshots of each screen in `light`, `dark`, `conclave` (`localStorage.theme`), nav expanded and collapsed, into the scratchpad; compare with `DESIGN.md` and the mockups; list defects.
- [ ] `/impeccable audit` then `/impeccable polish` on the changed screens; fixes within `DESIGN.md` tokens; FE after each batch.
- [ ] `node frontend/scripts/a11y-check.mjs http://localhost:3118 / /meeting-details?id=preview-1 /whats-new` exits 0; GUARD; full FE. Commit `style(ui): redesign polish from audit` (if anything changed).

### Task 56: Checkpoint, full CI and the Windows checklist
**Agent:** mechanical (controller) · **Risky:** no
- [ ] Whole-branch review with a Codex pass; fix minors before merge.
- [ ] Push `redesign`; `gh workflow run ci.yml -f full=true --ref redesign`; watch to the end and report.
- [ ] Hand the user the PC checklist:
  1. Update from the latest installed release (v1.2.x if released, else v1.1.x) with a backup: the pre-upgrade backup appears in Settings → Updates; old meetings show with category, gist and dates, all Reviewed; preps still open.
  2. Create a meeting from the table, add prep, press Start recording on its page: the nav shows `Recording mm:ss` and opens that meeting; Stop saves into the same row (no second row).
  3. A 📞 event within 15 min: its row exists; join Zoom, press Record on the pop-up: the recording goes into that meeting and its page opens.
  4. During the call open Copilot (Ctrl J) and ask; the answer arrives while recording continues.
  5. What's new shows v2.0.0 Installed; the dot goes after opening; Open data folder and Open log file work.

## Spec coverage

| Spec | Tasks |
|---|---|
| §6.1 columns, preps rebuild, two tables | 1 |
| §6.1 backfill: reviewed, template, times (SQL) / gist, prep rows | 1 / 2, 4 |
| §6.1 meeting date and every `created_at` reader | 3 |
| §6.1 derived state | 7 (`has_transcript`, `has_summary`, times), 17 (`meetingState`) |
| §6.2 New meeting row, default title / Record new meeting | 7, 24 / 50 (+33 autostart) |
| §6.2 Start recording into the page's meeting; id start→save; `started_at` at start | 9, 11, 13, 33 |
| §6.2 save into row, `ended_at`, missing target → new + toast | 10, 13 |
| §6.2 crash recovery on `/`, target in IndexedDB, fallback | 14, 16 |
| §6.2 one routing function for sidebar, pop-up, tray, detector | 9, 12, 13, 47, 50 |
| §6.2 finished has no Start; Continue unchanged (routed row released) | 33, 35 / 10 |
| §6.2 category before recording = manual pick; new default titles | 15 / 7, 11, 13 |
| §6.3 views with per-view sort/filter/group/widths/hidden/density | 1, 7, 18, 22, 24 |
| §6.3 category emoji (built-ins, editor, assistant, one-time suggestion) | 5, 6 |
| §6.3 columns, date start–end and live, Reviewed only with a summary | 7, 17, 23 |
| §6.3 row states / toolbar / empty first run | 17, 23 / 24 / 24 |
| §6.3 search across all meetings, snippets, N more, open at time, Esc | 19, 24 |
| §6.3 keyboard / tanstack + virtualisation / one list call | 20, 24 / 21, 23 / 7, 22 |
| §6.4 title, emoji, chip, date line / New (prep, Audio line, Start) | 33 / 31, 33 (warning deferred) |
| §6.4 Recording / Finished, tabs, task review, Done reviewing | 34 / 32, 35 |
| §6.4 Delete recording, Delete meeting, kept tasks, dismissed occurrences | 8, 11 (live guard), 23, 36 |
| §6.4 category change offers regenerate / old layouts removed | 36 / 37 |
| §6.5 shell, Ctrl J, per-screen memory / Ask with scope, `/ask` removed | 26 / 27, 28 |
| §6.5 Copilot in the sidebar, time chips, alerts line and cards / Ask this meeting | 29, 30 / 28, 35 |
| §6.6 marker, stripped titles / matching key, cache refetch | 41, 48 / 39, 42 |
| §6.6 when it runs, completeness, marker-scoped / window | 40, 42, 44 / 42, 44 |
| §6.6 rules (adopt, insert, move+prep, rename, gone, reappear, past, frozen) | 42, 43 |
| §6.6 untouched, no lost edits / failures + last error / relocate removed | 10, 43 / 44, 48 / 45 |
| §6.6 briefs only 📞, in Prep, regenerate / Digests, `/prep` removed | 46, 49 / 45, 49 |
| §6.6 existing preps get rows and are adopted | 2, 42, 43 |
| §6.7 pop-up routing, part 2, 15-minute window | 47, 41 |
| §6.8 nav, live label, labels in rail, What's new dot / removals / What's new page | 50 / 50, 51 / 52, 53, 54 |
| §6.9 DESIGN.md | 0 (followed by 16–55) |
| §8 Rust list / frontend bun / mocks, screenshots, audit, a11y / Windows | 1–15, 19, 27, 29, 31, 39–47, 52 / 17–24, 26–36, 48–50, 54 / 21, 25, 38, 55 / 56 |

## Ledger

| Task | Status |
|---|---|
| 0 DESIGN.md | done (pre-crash) |
| 1 redesign migration | done (pre-crash) |
| 2 backfill + gist_from_summary | done (pre-crash) |
| 3 meeting date readers | done (pre-crash) |
| 4 gist from extraction | done (pre-crash) |
| 5 template emoji | done (pre-crash) |
| 6 emoji suggestion | done (pre-crash) |
| 7 rows, create, setters, views | done (pre-crash) |
| 8 Delete recording / Delete meeting | done |
| 9 recording::route | done (pre-crash) |
| 10 save into row + release routed row | done |
| 11 meeting id start→stop + live delete guard | done |
| 12 tray/pop-up/detector routing | done |
| 13 frontend start/save + liveMeetingId | done |
| 14 recovery into target | done |
| 15 category = manual pick | done |
| 16 /record + StartupChecks | done (pre-crash) |
| 17 lib/meetings.ts | done (pre-crash) |
| 18 view config + arrangeRows | done (pre-crash) |
| 19 search totals + snippets | done (pre-crash) |
| 20 keyboard map | done (pre-crash) |
| 21 react-table + mocks | done (pre-crash) |
| 22 rows/views hooks | done |
| 23 table rows + delete dialog | done |
| 24 tabs, toolbar, search, empty | done |
| 25 Phase 3 check | done |
| 26 AiSidebar shell | done (pre-crash) |
| 27 ask_meetings scope | done (pre-crash) |
| 28 Ask in sidebar | done |
| 29 copilot-alert | done (pre-crash) |
| 30 Copilot in sidebar | done |
| 31 prep by meeting id | done (pre-crash) |
| 32 meeting-page helpers | done |
| 33 page shell + New + autostart URL | done |
| 34 Recording state | done (rd-d, merged into rd-35) |
| 35 Finished state + tasks | done (rd-35, not merged) |
| 36 page menu + category | fix round (rd-35) |
| 37 remove old layouts | todo |
| 38 meeting page mocks + a11y | todo |
| 39 original_start + cache v2 | done |
| 40 expansion completeness | done |
| 41 marker + 15-min window | done |
| 42 reconcile plan | done |
| 43 reconcile executor | done |
| 44 reconcile scheduling + sync error | done |
| 45 remove prep mover | done (rd-49, with 49) |
| 46 📞 briefs + commands | done |
| 47 pop-up 📞 routing | done |
| 48 Settings marker + error time | done |
| 49 brief in Prep, Digests | done (rd-49) + prep chat moved into Prep tab (user, 2026-11-17) |
| 50 nav | done (rd-50, not merged) |
| 51 removals | todo |
| 52 open_log_file | done |
| 53 CHANGELOG v2.0.0 (+ v1.2.0) | done |
| 54 What's new | done (rd-50) |
| 55 final UI QA | todo |
| 56 checkpoint + Windows checklist | todo |

2026-11-17: the VM crashed mid-session; uncommitted work in rd-e (Tasks 39+) and rd-r (Task 10) was corrupted (NUL bytes) and discarded (backups in the session scratchpad). Committed lanes merged into `redesign`. "done (pre-crash)" tasks get covered by their phase-end reviews.

2026-11-17 session: lanes write code in parallel; every build/test (cargo, pnpm, tsc, bun) runs under `flock /workspace/.superpowers/cargo.lock`. Phase 2 merged into `redesign` from rd-b (Phase 2 review running). Review-found plan errors fixed in code: `UNTOUCHED_SQL` also checks `notes_json`; autostart script goes to `/record` (Task 16), not `/`; brief store and `set_extract` require a `summary_processes` row (Task 8).

## Open items for the user

1. ~~v1.2.0 and the v1.3.0 notes~~ Resolved 2026-11-17: v1.2.0 section for safe updates; the redesign is v2.0.0.

Decided while planning (2026-11-16): silent-system-audio warning deferred; a Continue mid-recording deletes the fresh routed row only if untouched, else returns it to New (Task 10); a start whose routing fails records anyway into a new meeting (Tasks 12, 13); a failed calendar fetch keeps the 30-min cadence, a failed reconcile retries next tick from the last feed (Task 44).
