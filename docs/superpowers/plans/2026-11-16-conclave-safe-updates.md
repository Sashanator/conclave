# Conclave Safe Updates — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Conclave tells the user about a newer release in their private repo and installs it, signature-verified, only after an explicit **Update** click. Every version change first backs up the user's data, before migrations. Every stored secret moves from plain text into Windows Credential Manager without loss.

**Architecture:**
- **`secrets/`:** a `SecretStore` trait backed by `keyring` v3 (one mutex, target `Conclave/<name>`), with an in-memory fake for tests. Reads try the store first, then fall back to the plain value. A verified one-time move runs at startup.
- **`backup/`:** a file-copy pre-upgrade backup runs in `lib.rs` setup before the DB opens; if it fails, a native Continue/Quit dialog asks. Manual backups use `VACUUM INTO`. A last-run version file is written after migrations. Retention: 3 pre-upgrade, 5 manual.
- **`updates/`:** a busy state, a GitHub `releases/latest` lookup of the `latest.json` API asset URL, and `tauri-plugin-updater` driven from Rust with the token as a header. Commands, plus a start-up check about 30 s after launch.
- **Release:** `CHANGELOG.md` supplies the notes. The build signs the installer, and a workflow step uploads `latest.json` to the draft. **Frontend:** a Settings → General Updates/Backups section and one global update dialog.

**Startup order** (`lib.rs` `.setup`, before IPC runs):
1. `backup::run_at_startup`, at `lib.rs:650` above `// Initialize system tray`. This is before the JSON settings loads (`:666-673`) and before the DB.
2. `initialize_database_on_startup` (`:746`) → `open_in_data_dir` (`manager.rs:59`) → `sqlx::migrate!` (`:38`) → `backup::write_last_run` in the `Ok` arm (`manager.rs:77`).
3. An open error returns at `:747-752`. Nothing below it runs.
4. `secrets::migrate::run_at_startup` at `:758`, after `fail_interrupted_summaries`, so after `app.manage(AppState)` (`database/setup.rs:66`). Part A placed it at `:758`, Part B at `setup.rs:66`. Both come after the DB open. `:758` is also past the error return and keeps `setup.rs` untouched.
5. `updates::start_check`, after `knowledge::scheduler::start` (`:769`).

**Tech Stack:** Rust (Tauri 2.12, sqlx/SQLite, reqwest 0.11, `keyring =3.6.3`, `tauri-plugin-updater 2.14.1`, `rfd 0.16` on Windows), Next.js 14 + React 18 + TS, Bun tests, `node:test`, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-16-conclave-safe-updates-design.md`. Read the section each task cites.

## Global Constraints

- **Branch `safe-updates`:** one worktree per lane (`/workspace/.worktrees/su-<lane>`), merged at the phase end. Never stage `.claude/*`, `.devcontainer/*` or `.env*`. Run `git add`, `git commit` and `git push` as separate Bash calls.
- **Gates** (tasks name them):
  - **RUST:** `. "$HOME/.cargo/env"; export CARGO_TARGET_DIR=/workspace/target; cd /workspace && flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib <filter> -j 4`. Run each filter separately, then once without a filter. Each worktree needs `frontend/out` (`pnpm run build`) and `mkdir -p frontend/src-tauri/binaries && touch frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu`.
  - **FE:** `cd /workspace/frontend && pnpm exec tsc --noEmit && bun test <file>`, then the full `bun test` and `pnpm run build`. Record the baseline failures before Task 14.
  - **A11Y:** `pnpm run dev:preview` in the background, then `node frontend/scripts/a11y-check.mjs http://localhost:3118 / /settings` must exit 0.
  - **SCRIPTS:** `node --test scripts/`; `bash -n` / `node --check` / `actionlint` / JSON parse on changed files; `bash scripts/check-no-phone-home.sh` (the "guard").
  - **PHASE:** `flock … cargo test --workspace --exclude llama-helper -j 4`, the full FE gate, and the guard.
  - **WINCI** (at a push checkpoint): `gh workflow run ci.yml -f full=true --ref safe-updates` (`ci.yml:53-95`). The app crate can't be checked for Windows on Linux.
- **Secrets:** Never log or return a value. Format keyring errors as `{e}`, never `{e:?}`, because `BadEncoding`'s Debug prints the bytes. Errors name the secret. The token goes only to `api.github.com`. Every message leaving `updates/` passes through `github::scrub`.
- **No destructive migration:** plain columns and `calendar_url` stay, set to NULL (`''` for `licensing`).
- **Deletion:** only retention deletes backups, and only real child dirs of `backups/<kind>/` whose `backup.json` parses with that kind.
- **Silent failures:** install, backup, I/O and background errors must reach the user (an `Err`, a dialog, a toast or `lastCheck`), never only the log. Every reviewer of a risky task looks for them.
- **No new styling system and no new frontend dependency.** No webview `updater:*` permission.
- **Pinned names:**
  - `trait SecretStore: Send + Sync { fn get(&self,&str)->anyhow::Result<Option<String>>; fn set(&self,&str,&str)->anyhow::Result<()>; fn delete(&self,&str)->anyhow::Result<()>; }` and `secrets::store() -> &'static dyn SecretStore`.
  - Secret names: `GITHUB_UPDATE_TOKEN="githubUpdateToken"`, `CALENDAR_URL="calendarUrl"`, `CUSTOM_OPENAI_API_KEY="customOpenAIConfig.apiKey"`, `MAX_VALUE_BYTES=2560`, `transcript_name(col)="transcript.<col>"`.
  - `BackupInfo { kind: "pre-upgrade"|"manual", createdAt (RFC3339), fromVersion: string|null, version, sizeBytes, path }` (camelCase).
  - `backup_now -> Result<BackupInfo,String>`, `backup_list -> Result<Vec<BackupInfo>,String>` (newest first, both kinds), `backup_open_folder -> Result<(),String>`. Files: `last-run-version`, `backups/{pre-upgrade,manual}/<dir>/backup.json`.
  - Updater commands (all `Result<_,String>` except `updates_check`): `updates_get_status -> UpdatesStatus { tokenLast4, checkOnStart, lastCheck: {at,result}|null, currentVersion }`; `updates_set_token({token})`, `updates_remove_token`, `updates_set_check_on_start({enabled})`; `updates_check -> UpdateCheckResult { status: "not_set_up"|"up_to_date"|"available"|"error", version, notes, message }`. It records `lastCheck` for start-up **and** manual checks (every status except `not_set_up`). `updates_install`: while busy it returns `Err("busy: <BusyReason Display>")`. On Windows it never resolves on success.
  - Events: `update-available {version, notes}`, emitted only by Rust, through `announce_when_idle`, after the start-up check **or** a manual `updates_check` returns `available`. The frontend never emits it, so the prompt never appears while busy (spec §6.2). `updates-progress {downloaded, total}`. `updates.json` holds `checkOnStart` (default true) and `lastCheck`.
  - **Busy texts** (shown to the user): "a recording is in progress or being saved", "a Practice session is running", "a Practice report is being generated", "a summary is being generated", "a model is downloading".
  - Owner `Sashanator`, repo `conclave`. **First release with this feature: v1.2.0** (latest tag is v1.1.0).

## Review Focus

- **Secrets:** A stale plain value never overwrites a newer store value. A mismatch or store error keeps the plain value and names the secret, never the value. Clears are compare-and-clear, and the size limit counts UTF-16 bytes. Cleared values leave the DB file (checkpoint + VACUUM).
- **Backup:** No backup on a fresh install. A pre-feature DB gets one, named `<stamp>_unknown-to-v<new>`. Once per `old→new` pair. The last-run file is untrusted (`[0-9A-Za-z.+-]{1,64}`, else "unknown") and never becomes a path part. A failure blocks migrations until the user chooses, and a dialog that can't open counts as Quit. A partial dir is removed. Prune runs only after the manifest is written. A manual backup uses `VACUUM INTO`.
- **Updater:** Both the endpoint and `update.download_url` pass `is_conclave_asset` before the token is sent. `releases/latest` uses `Policy::none()`. Busy is checked before the download and again before the install. A second install is refused. The start-up check never shows a modal. `latest.json` goes only to the draft, after the existing checks.
- **Guard:** fails on a foreign `pubkey`, config `endpoints`, `.endpoints(`/`.pubkey(` outside `src/updates/`, a non-Conclave URL in non-test `src/updates/` code, and any `"updater:` permission.
- **Frontend:** The token is never rendered, logged or toasted. The dialog can't be dismissed while installing. A refusal, failure or failed backup is never shown as success.

## Parallel lanes

Only one cargo runs at a time (AGENTS.md flock rule).
- **Lane R (cargo):** Tasks 1–11 in order. They share `lib.rs` (1, 4, 5, 6, 7, 9, 11), `Cargo.toml` (1, 6, 11) and `manager.rs` (6). Task 8 runs no cargo, but it edits `tauri.conf.json` and the guard, which 10 and 11 need, so it stays in sequence.
- **Lane S:** Tasks 12 and 13, any time after Task 0. They touch `release.yml`, `CHANGELOG.md`, new scripts, `AGENTS.md` and `network-calls.md`, none of which Lane R uses.
- **Lane F:** Task 14, then 15 (`settings/page.tsx`) and 16 (`layout.tsx`) in parallel, then 17 (`README.md`). These tests mock `invoke` and don't wait for Rust.
- **Never together:** two Lane R tasks; Task 14 with 15 or 16. Single owners: guard and `tauri.conf.json` = 8, settings page = 15, `AGENTS.md` = 13. Task 18 runs alone, after everything is merged into `main`.

## Phase 0: Signing key (blocks all)

### Task 0: Signing key and GitHub secrets (user checkpoint)

**Tag:** user, [risky] (credentials). The agent never sees the private key or its password. Spec §6.2, §6.5.

The controller pastes these steps to the user:
1. In the devcontainer terminal (not in Claude): `unset CI; mkdir -p ~/.tauri && cd /workspace/frontend && pnpm exec tauri signer generate -w ~/.tauri/conclave-updater.key`. Type a strong password at the prompt. Don't use `-p`: it lands in the shell history. With `CI` set, the CLI makes a key with no password, hence the `unset`.
2. Save the key file **and** the password in a password manager. If either is lost, updates can't be signed, and the user needs one manual reinstall of a build with a new key.
3. `gh secret set TAURI_SIGNING_PRIVATE_KEY -R Sashanator/conclave < ~/.tauri/conclave-updater.key`, then `gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R Sashanator/conclave` (it prompts). `gh secret list -R Sashanator/conclave` must show both.
4. Paste `cat ~/.tauri/conclave-updater.key.pub` into the chat. The public key isn't secret.
5. Optional: delete the private key file once it's saved.

The controller records the key as `CONCLAVE_UPDATER_PUBKEY` for Task 8. No "updater disabled" mode is needed: the key exists before any code. tauri-cli 2.11.1 fails a bundle that has `createUpdaterArtifacts: true` and no private key (`bundle.rs:277-279`).

## Phase 1: Secrets (spec §6.1)

### Task 1: `keyring` dependency and the `secrets` module

**Tag:** [mechanical] [risky] (secrets). Per-task review + Codex pass.

**Files:**
- `frontend/src-tauri/Cargo.toml:70`: after `anyhow`, add `keyring = { version = "=3.6.3", features = ["windows-native"] }`. This is the last v3; v4 renamed the features. On Linux keyring falls back to its mock store, and the tests use our fake.
- Create `src/secrets/mod.rs`.
- `lib.rs:65`: add `mod secrets;` after `mod prep;`. Tasks 5 and 9 add `mod backup;` and `mod updates;` there too.

**Interfaces** (plus the pinned trait and constants):
```rust
pub fn target(name: &str) -> String;            // "Conclave/<name>"
pub fn transcript_name(column: &str) -> String;
pub fn check_size(name: &str, value: &str) -> anyhow::Result<()>; // encode_utf16().count()*2 > 2560 -> Err("secret <name> is too long (<n> bytes, limit 2560)")
/// Store first; plain only while non-empty. Store error: non-empty plain -> Ok(plain) + warn naming the secret; else Err.
pub fn read_or_plain(store: &dyn SecretStore, name: &str, plain: Option<String>) -> anyhow::Result<Option<String>>;
pub struct MemoryStore { /* Mutex<HashMap> */ }   // Default; set calls check_size
struct KeyringStore { lock: std::sync::Mutex<()> } // every call holds `lock`
```
**`KeyringStore`:**
- It uses `Entry::new_with_target(&target(name), "Conclave", name)`.
- `NoEntry` gives `Ok(None)` from `get` and `Ok(())` from `delete`. Errors are `anyhow!("secret {name}: {e}")`.

**`store()`:** in the app, a `static OnceLock<KeyringStore>`. Under `#[cfg(test)]`, a per-thread leaked `MemoryStore` with the comment `// shortcut: per-thread fake; a test reading from another thread sees an empty store. Pass a store explicitly if one ever needs to.`

**Steps:**
- [ ] Failing tests: `target_is_conclave_slash_name`; `memory_store_round_trip_and_delete_missing_is_ok`; `rejects_values_over_2560_bytes`: `"x"×1280` is OK, `"x"×1281` is an Err, `"я"×1280` is OK. With the sentinel `"SENTINEL"×200`, the error names the secret but not the value. A rejected set keeps the old value; `read_or_plain_prefers_store`; `read_or_plain_falls_back_while_plain_non_empty`: `Some("")`/`None` → `None`; `read_or_plain_store_error`: `FailingStore` + `Some("p")` → `Ok(Some("p"))`; `+ None` → `Err` naming the secret; `#[cfg(windows)] keyring_store_round_trip_on_windows`: name `test.<uuid>`. `"x"×1280` set, then read back equal by a **new** `KeyringStore`. `"x"×1281` → Err. Delete → get `None` → a second delete is Ok.
- [ ] **RUST** `secrets::`: FAIL → implement → PASS. A throwaway scratchpad crate (not committed) with the same `keyring` line and the four `Entry` calls must pass `flock … cargo check --target x86_64-pc-windows-msvc`.
- [ ] Commit `feat(secrets): Credential Manager store via keyring v3, in-memory fake, 2560-byte limit`.

### Task 2: Reroute `SettingsRepository` keys through `secrets`

**Tag:** [judgment] [risky] (secrets, persistence). Per-task review + Codex pass; silent-failure focus.

**Files:** `src/database/repositories/setting.rs` (`:70-140` save/get, `:175-235` the transcript pair, `:237-270` delete, `:280-350` custom OpenAI). Tests go in a new `mod secret_tests` using `repositories::test_pool()` (`mod.rs:11`).

**Behaviour:** signatures are unchanged, so no caller changes. A private `store_err(anyhow::Error) -> sqlx::Error` wraps errors as `Protocol(e.to_string())`.
- **Names:** a settings column's store name is the column; a transcript column's is `transcript_name(col)`. `"localWhisper"` maps to `transcript.whisperApiKey`. `"parakeet"` keeps its early return.
- **get:** `read_or_plain(store(), name, <column>)`.
- **save:** `store().set` first; on an error, return it with the DB untouched. Then the existing upsert, binding `None::<String>`.
- **delete:** the store, then `= NULL`. `custom-openai` also deletes `CUSTOM_OPENAI_API_KEY`.
- **Custom config save:** `Some(k)` → set, `None` → delete. The JSON is stored with `api_key = None`.
- **Custom config get:** fills `api_key` via `read_or_plain`.

**Steps:**
- [ ] Failing tests: `save_api_key_goes_to_store_not_column`: the column is NULL, the store holds `sk-1`, and get returns it; `get_api_key_falls_back_to_plain_column`, then the store wins after a set; `transcript_and_summary_keys_are_distinct`; `local_whisper_key_goes_to_store`; `delete_api_key_removes_store_and_column`; `custom_openai_key_lives_in_store`: the raw JSON has no `"c"` and the read returns `c`, other fields unchanged. Saving `None` or `delete_api_key("custom-openai")` removes the entry; `over_limit_key_is_rejected_and_keeps_old`: the error has no `xxxx`, and get still returns `k1`.
- [ ] These existing tests pass unchanged: `onboarding.rs:255-270`, `summary/llm_target.rs:146-185`, `practice/claude.rs:940-962`.
- [ ] **RUST** `setting::`, `llm_target::`, `onboarding::`, `practice::claude::`.
- [ ] Commit `feat(secrets): API keys read and written through Credential Manager, plain columns as fallback`.

### Task 3: Reroute the calendar URL through `secrets`

**Tag:** [judgment] [risky] (secrets, persistence).

**Files:** `src/knowledge/settings.rs`: `:367-376` `set_calendar_url`, and `:440-477` `load_at_startup` (reads at `:463`). Make `calendar_path` (`:378`), `write_calendar_at` (`:390`) and `read_calendar_at` (`:408`) `pub(crate)` for Task 4.

**Interfaces:**
- `fn load_calendar_url(store: &dyn SecretStore, path: &Path) -> Option<String>`: `read_or_plain(store, CALENDAR_URL, <file value>)`. A file error keeps today's `warn!`. A store error with no plain value gives `None` plus a warning naming `calendarUrl`.
- `fn save_calendar_url(store: &dyn SecretStore, path: &Path, url: Option<&str>) -> Result<(), String>`: `set` or `delete`, then `write_calendar_at(path, None)`.
- `set_calendar_url` keeps its signature and `SAVE_LOCK`. It calls `save_calendar_url(secrets::store(), …)` before `set_cache`, and leaves the cache unchanged on error.
- `load_at_startup` (it runs at `lib.rs:670`, before the DB; the store doesn't need one) uses `load_calendar_url`.

**Steps:**
- [ ] Failing tests: `calendar_url_saved_to_store_and_file_cleared`; `calendar_disconnect_deletes_store_entry`; `calendar_url_store_wins_then_file_fallback`: store `a` + file `b` → `a`; empty store + `b` → `b`; `FailingStore` + `b` → `b`; `FailingStore` with no file → `None`.

  The existing `calendar_file_round_trip`, `corrupt_calendar_file_is_an_error_without_content`, `view_never_contains_url` and `save_keeps_calendar_url` must still pass.
- [ ] **RUST** `knowledge::settings::`.
- [ ] Commit `feat(secrets): calendar URL kept in Credential Manager`.

### Task 4: One-time move at startup, compaction, failure notice

**Tag:** [judgment] [risky] (secrets, persistence, startup order). Per-task review + Codex pass; silent-failure focus.

**Files:**
- Create `src/secrets/migrate.rs` (`pub(crate) mod migrate;`).
- `lib.rs:758`, after `fail_interrupted_summaries`: add `tauri::async_runtime::block_on(secrets::migrate::run_at_startup(_app.handle()));`. With `block_on` in setup, no IPC command can race the move.

**Interfaces:**
```rust
pub(crate) const MOVE_FAILED_NOTICE: &str =
    "Couldn't move your keys to Windows encryption; they still work. Details in the log.";
const SETTINGS_COLUMNS: [&str; 6] = ["openaiApiKey","anthropicApiKey","ollamaApiKey","groqApiKey","openRouterApiKey","geminiApiKey"];
const TRANSCRIPT_COLUMNS: [&str; 5] = ["whisperApiKey","deepgramApiKey","elevenLabsApiKey","groqApiKey","openaiApiKey"];
pub(crate) struct MoveReport { pub not_moved: Vec<String>, pub cleared: usize }
/// Store already has a value -> Ok (it wins). Else set, get, compare; mismatch -> Err.
fn copy_verified(store: &dyn SecretStore, name: &str, plain: &str) -> anyhow::Result<()>;
pub(crate) async fn move_plain_secrets(pool: &SqlitePool, calendar_file: &Path, store: &dyn SecretStore) -> MoveReport;
/// `PRAGMA wal_checkpoint(TRUNCATE)` then `VACUUM`, only when cleared > 0; returns whether it ran.
pub(crate) async fn compact_if_cleared(pool: &SqlitePool, cleared: usize) -> anyhow::Result<bool>;
pub async fn run_at_startup(app: &tauri::AppHandle);
```
**`move_plain_secrets`:** each source is handled on its own. Any error pushes the name, logs `warn!("secrets: {name} not moved, plain-text copy kept: {e}")` and moves on. Each successful clear adds 1 to `cleared`.
1. `SETTINGS_COLUMNS` (`settings WHERE id='1'`): `copy_verified`, then `UPDATE … SET "<col>"=NULL WHERE id='1' AND "<col>"=$1`. `geminiApiKey` has no reader; it moves because the spec lists it.
2. `TRANSCRIPT_COLUMNS` (`transcript_settings`), named `transcript_name(col)`. The spec missed `whisperApiKey`.
3. `customOpenAIConfig`: invalid JSON is a failure named `customOpenAIConfig.apiKey` and changes nothing. Otherwise `copy_verified`, then compare-and-swap the JSON with `api_key: None`.
4. `licensing` (no Rust reader; the spec lists it). `LIMIT 2`, then by row count: 0 rows: nothing. 2 rows: a failure named `licensing`. 1 row: `copy_verified` `licensing.license_key` and `licensing.encrypted_key`. If both succeed, `SET license_key='', encrypted_key='' WHERE license_key=$1`.
5. Calendar: a `Some(url)` gets `copy_verified(CALENDAR_URL)`, then `write_calendar_at(path, None)`, which counts as cleared. A read error is a `calendarUrl` failure.

**`run_at_startup`:**
1. Return if there is no `AppState` (a first launch has no DB).
2. Path from `calendar_path(app)`. An Err gives a warning and a `calendarUrl` failure.
3. Move, then log `info!("secrets: move checked, {n} not moved, {c} cleared")`.
4. `compact_if_cleared`. An Err is a `warn!`, because the values are already cleared.
5. If any secret was not moved, spawn a task that: polls `NotificationManagerState<Wry>` (`read().await.is_some()`) every 500 ms for up to 30 s; it starts asynchronously at `lib.rs:676-700`. then calls `show_system_error_notification(&state, MOVE_FAILED_NOTICE.into())` (`notifications/commands.rs:460`) once.

A DB imported from Meetily (`database/commands.rs:156-164`) moves at the next start with no extra code. Until then it reads through the fallback.

**Steps:**
- [ ] Failing tests. Setup: `test_pool()`, a tempdir calendar file, an explicit `MemoryStore`; test-only `FailingStore` and `LyingStore` (its `get` returns `"other"`); `move_success_clears_plain_text`: seed three settings keys including gemini, a custom config (`apiKey 'c'`, `endpoint 'http://x/v1'`), transcript groq and whisper, one licensing row (NOT NULL columns filled), and a calendar URL. Expect: `not_moved` is empty and `cleared` equals the seed count. Every value is in the store under its pinned name. The columns are NULL (licensing `''`). The JSON keeps its endpoint with a null `apiKey`. `read_calendar_at == None`; `read_back_mismatch_keeps_plain`; `store_error_keeps_plain_and_reports`: all values unchanged, all names listed, no value in any entry, `cleared == 0`; `second_run_is_idempotent`; `existing_store_value_wins`: store `new`, column `old` → the store keeps `new` and the column becomes NULL; `empty_values_are_skipped`; `two_licensing_rows_are_reported_and_kept`; `bad_custom_config_json_is_reported_and_kept`; `compaction_runs_only_when_something_was_cleared`: `compact_if_cleared(pool, 0) == Ok(false)`; `cleared_values_leave_the_db_file`: A file DB in a tempdir via `DatabaseManager::new(db, legacy)` (as `database/setup.rs:107`), with `anthropicApiKey='sentinel-not-a-real-key'`. Move, then compact → `Ok(true)`. Neither `meeting_minutes.sqlite` nor any `-wal` contains the sentinel.
- [ ] **RUST** `secrets::migrate::`.
- [ ] Commit `feat(secrets): one-time verified move of plain-text secrets at startup`.
- [ ] Phase end: **PHASE**, a Codex pass over Tasks 1–4, a push, then **WINCI**. `keyring_store_round_trip_on_windows` must pass.

## Phase 2: Backup (spec §6.3)

### Task 5: `backup` module core

**Tag:** [judgment] [risky] (persistence, deletion, untrusted file content). Per-task review + Codex pass; silent-failure focus.

**Files:** create `src/backup/mod.rs`; `lib.rs:65` gets `mod backup;`. Tests use tempdirs (`tempfile`, `Cargo.toml:215`).

**Interfaces:**
```rust
pub const LAST_RUN_FILE: &str = "last-run-version";
pub const BACKUPS_DIR: &str = "backups"; pub const MANIFEST_FILE: &str = "backup.json";
pub const KEEP_PRE_UPGRADE: usize = 3; pub const KEEP_MANUAL: usize = 5;
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] #[serde(rename_all = "camelCase")]
pub struct BackupInfo { pub kind: String, pub created_at: String, pub from_version: Option<String>,
                        pub version: String, pub size_bytes: u64, pub path: String }
struct Manifest { kind, created_at, from_version, version, size_bytes, files: Vec<ManifestFile> } // camelCase; no path
struct ManifestFile { path: String /* relative, '/' */, size_bytes: u64 }
pub fn read_last_run(data_dir: &Path) -> Option<String>;   // trimmed; None if missing/unreadable/invalid
pub fn write_last_run(data_dir: &Path, version: &str) -> std::io::Result<()>;
fn valid_version(s: &str) -> bool;                         // [0-9A-Za-z.+-]{1,64}
pub fn pre_upgrade_backup(data_dir: &Path, templates: Option<&Path>, current: &str) -> anyhow::Result<Option<BackupInfo>>;
pub fn list_backups(data_dir: &Path) -> Vec<BackupInfo>;   // both kinds, newest first; bad manifests skipped
fn prune(kind_dir: &Path, kind: &str, keep: usize) -> anyhow::Result<()>;
fn copy_user_files(data_dir: &Path, templates: Option<&Path>, dest: &Path) -> anyhow::Result<Vec<ManifestFile>>;
fn new_backup_dir(kind_dir: &Path, name: &str) -> anyhow::Result<PathBuf>; // create_dir of name, name-2, name-3…
```
**`pre_upgrade_backup`:**
1. No `DB_FILE_NAME` → `Ok(None)`, before creating anything.
2. `last == Some(current)` → `Ok(None)`. A downgrade counts as a change.
3. A manifest for `(last, current)` already exists → `Ok(None)`.
4. Dir name: `Local::now().format("%Y-%m-%d_%H%M")` + `_v{last}-to-v{current}`, or `_unknown-to-v{current}` when `last` is None.
5. Copy the DB, then `-wal`/`-shm` if present, then `copy_user_files`: top-level regular `*.json` only (case-insensitive, no recursion); the templates tree, recursive, skipping symlinks; skipped if missing.

   On any error, `remove_dir_all` only this call's dir (`warn!` if that fails), and return the error with context.
6. Write the manifest, then prune. A prune error is a `warn!`.

**`prune`:** keeps the `keep` newest real dirs whose manifest kind matches, by parsed `createdAt`, ties broken by dir name descending.

**Steps:**
- [ ] Failing tests: `last_run_missing_is_none`, `last_run_round_trips` (a trailing newline is OK), `last_run_rejects_path_like_content` (`"../../x"`, `""` and 100 chars → `None`); `fresh_install_takes_no_backup`: also with a stale last-run file; `backups/` must not exist; `same_version_takes_no_backup`; `db_without_last_run_counts_as_change`: `from_version == None`, and the dir ends `_unknown-to-v1.1.0`; `new_version_copies_db_wal_shm_json_and_templates`: Data dir: DB, `-wal`, `-shm`, `a.json`, `B.JSON`, `notes.txt`, `models/m.json`, `backups/old.json`. Templates: `t.json`, `.seeded-v1`, `history/t/1.json`. 8 files byte-equal; `notes.txt` and `m.json` absent. The manifest has `files.len()==8`, the summed `sizeBytes` and a parseable `createdAt`. `path` is the dir; `missing_wal_and_templates_are_fine`: 1 file; `once_per_version_pair`: the second call is `Ok(None)` (1 child); a new `current` makes a new backup; `failure_returns_err_and_leaves_no_partial_dir`: `backups/pre-upgrade` as a file → Err; `-wal` as a directory → Err, no children; `prune_keeps_newest_n_and_ignores_strangers`: 7 manual manifests plus a pre-upgrade-kind dir, a dir with no manifest and a plain file. The 5 newest manual remain; the strangers are untouched; `pre_upgrade_retention_keeps_three`; `list_backups_newest_first_both_kinds`: a corrupt `backup.json` is skipped.
- [ ] **RUST** `backup::`.
- [ ] Commit `feat(backup): pre-upgrade file backup, manifest, retention, last-run version file`.

### Task 6: Startup order: hook before migrations, Continue/Quit, last-run write

**Tag:** [judgment] [risky] (startup order, persistence, native dialog on the main thread). Per-task review + Codex pass. The reviewer looks for silent failures and for any pool opened before the hook.

**Files:**
- Create `src/backup/startup.rs` (`mod startup; pub use startup::*;`).
- `lib.rs:650`, above `// Initialize system tray`:
  ```rust
  // Before anything reads or migrates user data: a pre-upgrade backup, or the user's Continue/Quit.
  backup::run_at_startup(_app.handle());
  ```
- `database/manager.rs:77-80`, in the `Ok(db_manager)` arm of `open_in_data_dir`. All three open paths pass through it: `setup.rs:62`, `initialize_fresh_database` (`commands.rs:181`) and `import_legacy_database`.
  ```rust
  // After migrations only: a failed open must leave the old version recorded.
  if let Err(e) = crate::backup::write_last_run(app_data_dir, env!("CARGO_PKG_VERSION")) {
      log::warn!("could not record the last-run version: {e}");
  }
  ```
- `Cargo.toml:197-201`, under the Windows target dependencies: `rfd = { version = "0.16", default-features = false, features = ["common-controls-v6"] }`. It matches `tauri-plugin-dialog 2.7.1`'s, so no new crate. The plugin's `blocking_show` can't run on the main thread (`database/setup.rs:18-19`), and setup runs there.

**Interfaces:**
```rust
pub const BACKUP_FAILED_MESSAGE: &str =
    "Couldn't back up your data before upgrading. Continue without a backup, or quit?"; // + "\n\n<error>"
/// true = go on to migrations; calls ask_continue only when the backup failed.
pub fn may_migrate(data_dir: &Path, templates: Option<&Path>, current: &str, ask_continue: impl FnOnce(&str) -> bool) -> bool;
/// Windows: rfd::MessageDialog (Warning, "Conclave", OkCancelCustom("Continue","Quit")); true only for Custom("Continue").
/// Close/Escape = Quit. A dialog that can't open (rfd returns Cancel) = Quit, logged with error! and the reason.
/// Other targets (dev): logs and returns false.
pub fn ask_continue_without_backup(message: &str) -> bool;
/// On Quit: log, log::logger().flush(), std::process::exit(0). Nothing changed; the next launch retries.
pub fn run_at_startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>);
```
`run_at_startup`:
- If `app_data_dir()` fails, `warn!` and return; the open then fails via `report_open_failure`.
- It passes `crate::summary::templates::templates_dir()` (`store.rs:15`) and `env!("CARGO_PKG_VERSION")` (`set-version.mjs` stamps it together with `tauri.conf.json`).

**Steps:**
- [ ] Failing tests: `may_migrate_does_not_ask_when_backup_succeeds`: `ask` panics if called; `true`; one backup exists; `may_migrate_does_not_ask_when_nothing_to_do`; `backup_failure_asks_and_quit_blocks_migrations`: the message starts with `BACKUP_FAILED_MESSAGE` and includes the error; `false`; `backup_failure_continue_proceeds`; In `manager.rs`: `open_writes_last_run_after_migrations`. `damaged_database_keeps_its_wal_file` gains `assert!(!dir.path().join(crate::backup::LAST_RUN_FILE).exists())`.
- [ ] **RUST** `backup::`, `database::manager::`. `cargo tree -p meetily -i rfd --target x86_64-pc-windows-msvc` shows one `rfd v0.16.0`.
- [ ] Commit `feat(backup): back up before migrations at startup; Continue/Quit on failure; record last-run version after migrations`.

### Task 7: Manual backup (`VACUUM INTO`) and the three commands

**Tag:** [judgment] [risky] (DB copy with a live pool; retention deletion). Per-task review; the phase-end Codex pass covers it.

**Files:** `backup/mod.rs` (`manual_backup`); create `backup/commands.rs` (`pub mod commands;`); `lib.rs` registers the three commands next to `database::commands::open_database_folder` (`:989`).

**Interfaces:**
```rust
pub async fn manual_backup(pool: &sqlx::SqlitePool, data_dir: &Path, templates: Option<&Path>, version: &str) -> anyhow::Result<BackupInfo>;
#[tauri::command] pub async fn backup_now(app: AppHandle) -> Result<BackupInfo, String>;   // no AppState -> Err("The database isn't open yet"); errors "Backup failed: {e:#}"
#[tauri::command] pub async fn backup_list(app: AppHandle) -> Result<Vec<BackupInfo>, String>;
#[tauri::command] pub async fn backup_open_folder(app: AppHandle) -> Result<(), String>;   // create_dir_all, then the explorer spawn of database/commands.rs:219-254
```
`manual_backup`:
- Dir: `backups/manual/<stamp>_v<version>`.
- Order: `VACUUM INTO ?` (bound path), `copy_user_files`, the manifest (`manual`, `from_version None`), then `prune(manual, 5)`.
- A partial dir is removed on error.

**Steps:**
- [ ] Failing tests: `manual_backup_copies_live_db_with_vacuum_into`: A `DatabaseManager::new` tempdir with a `meetings` row and the pool open, plus `x.json` and a templates dir. A new pool on the copy sees the row. The manifest is `manual` with a null `fromVersion`. `x.json` and `templates/` are copied, with no `-wal`/`-shm`; `manual_backup_twice_in_a_minute_gets_distinct_dirs`: `-2` suffix; `manual_retention_keeps_five_and_spares_pre_upgrade`: 5 manual + 4 pre-upgrade seeded → 5 + 4.
- [ ] **RUST** `backup::`.
- [ ] Commit `feat(backup): Back up now (VACUUM INTO), backup list and open-folder commands`.
- [ ] Phase end: **PHASE**, a Codex pass over Tasks 5–7, then **WINCI** at the next push (it builds the rfd path).

## Phase 3: Updater core (spec §6.2)

The guard/config task comes first, because the old guard denies the plugin and the `src/updates/` URLs that Tasks 10–11 add.

### Task 8: Updater config in `tauri.conf.json` and the guard rewrite

**Tag:** [judgment] [risky] (security guard). Per-task review + Codex pass. No cargo.

**Files:**
- **`frontend/src-tauri/tauri.conf.json`:** `:80` becomes `"createUpdaterArtifacts": true`. Add top-level `"plugins": { "updater": { "pubkey": "<CONCLAVE_UPDATER_PUBKEY>", "windows": { "installMode": "passive" } } }`. No `endpoints`; no `updater:*` capability. `requireSignedVersion` stays `false` (the default): tauri-cli 2.11.1 puts no version in the trusted comment (`updater_signature.rs:132`), so `true` would reject every update.
- **`scripts/check-no-phone-home.sh`:** The line 2 comment becomes "…telemetry, a foreign updater, Meetily-owned hosts…". Patterns: drop `plugin-updater`, `tauri_plugin_updater` and `createUpdaterArtifacts` (lines 12–14, 16); keep `releases/latest/download/latest\.json`; change `updater:default` to `"updater:`; add `\.pubkey\(` and `"endpoints"[[:space:]]*:`. Add `CONCLAVE_UPDATER_PUBKEY='<value>'` near the top. Add checks after the `PATTERNS` loop. Each sets `fail=1`, and a grep exit ≥2 fails closed: A `"pubkey"` hit without the Conclave value prints `FOREIGN updater pubkey:`. A `\.endpoints\(` hit outside `frontend/src-tauri/src/updates/` prints `updater endpoints outside src/updates:`. In each `src/updates/*.rs`, the lines before the first `#[cfg(test)]` (`awk '/#\[cfg\(test\)\]/{exit} {print}'`) are scanned. Every `https?://[^"[:space:]]+` must start with `https://api.github.com/repos/Sashanator/conclave/`, or it prints `FOREIGN URL in src/updates:`.
- **`.github/workflows/ci.yml:25`:** the step is named `Guard (no telemetry / foreign updater / Meetily hosts)`. Add a step after it, `Script tests`, running `node --test scripts/` (no workflow runs these today, so the guard and release-script tests would never run in CI).
- **Create `scripts/check-no-phone-home.test.mjs`** (`node:test`, like `set-version.test.mjs`). A `fixture(overrides)` helper builds a temp root: `frontend/src/a.ts`; `src-tauri/src/lib.rs`; `src-tauri/src/updates/mod.rs`, with `.endpoints(vec![url])` and the Conclave API URL; `Cargo.toml` with `tauri-plugin-updater`; `tauri.conf.json` with the real pubkey.

  It copies the script into the root and runs it there.

**Steps:**
- [ ] Failing tests: `passes_with_conclave_updater_config` (exit 0, `clean`); `fails_with_foreign_pubkey` (`FOREIGN updater pubkey`); `fails_with_config_endpoints`; `fails_with_endpoints_call_outside_updates`; `fails_with_foreign_url_in_updates` (`FOREIGN URL`); `fails_with_runtime_pubkey_override`; `fails_with_webview_updater_permission`; `urls_in_test_module_are_ignored`; `still_fails_on_meetily_latest_json`
- [ ] **SCRIPTS**. The real repo must print `clean`, and `actionlint` runs on `ci.yml`.
- [ ] Commit `feat(updates): Conclave updater pubkey in config; guard allows only the Conclave updater`.

### Task 9: Busy state (`updates::busy`)

**Tag:** [judgment] [risky] (concurrency). Per-task review + Codex pass.

**Files:**
- Create `src/updates/mod.rs` (`pub mod busy;`) and `updates/busy.rs`. `lib.rs:65` gets `mod updates;`.
- `practice/commands.rs`, after `running_id` (`:147`): `pub(crate) fn session_active() -> bool { !matches!(*slot(), Slot::Idle) }`. `Starting` counts as busy.
- Add one `let _busy = crate::updates::busy::hold(BusyReason::…);` at each place: `PracticeReport`: `practice/report_job.rs:91`, before `publish(&app, &session_id, "waiting")`. `Summary`: `summary/commands.rs:528`, the first line inside `api_process_transcript`'s spawned task, so a panic drops it. `ModelDownload`, as the first statement of `whisper_engine/commands.rs:422`, `parakeet_engine/commands.rs:379`, `summary/summary_engine/commands.rs:131` and `practice/commands.rs:347`. Also in `speakers/commands.rs:96-98`, after the `DOWNLOADING.compare_exchange` return.

**Interfaces (`busy.rs`):**
- `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum BusyReason { Recording, PracticeSession, PracticeReport, Summary, ModelDownload }`. Its `Display` gives the pinned busy texts, in order.
- `static HELD: [AtomicUsize; 5]`. `pub struct BusyGuard(BusyReason)` decrements on `Drop`. Plus `pub fn hold(r) -> BusyGuard` and `pub fn held(r) -> usize`.
- `pub fn current() -> Option<BusyReason>` checks, in order:
  1. `Recording` if `is_recording_now() || start_in_flight() || meeting_detector::post_processing()`. These are the sync parts of `meeting_detector::busy()` (`meeting_detector.rs:72`), covering the stop tail and the save like tray Quit (`tray.rs:161-175`).
  2. `PracticeSession` if `session_active()`.
  3. Any held counter, in enum order.
- `pub async fn wait_until_idle(busy: impl Fn() -> Option<BusyReason>, poll: Duration)`.

**Steps:**
- [ ] Failing tests. They don't use `current()`, because other tests toggle its globals in parallel; `hold_counts_until_dropped`; `guard_drops_on_panic` (`catch_unwind`); `wait_until_idle_returns_only_after_busy_ends`: busy on calls 1–3 → exactly 4 calls; `wait_until_idle_never_returns_while_busy`: `timeout(50ms)` → `Elapsed`; `reason_text_is_user_language`
- [ ] **RUST** `updates::busy`.
- [ ] Commit `feat(updates): busy state across recording, Practice, summaries and model downloads`.

### Task 10: GitHub release lookup, messages, token checks, settings

**Tag:** [judgment] [risky] (secret handling, untrusted GitHub input). Per-task review + Codex pass.

**Files:** create `updates/github.rs` and `updates/settings.rs`; declare both in `updates/mod.rs`.

**`github.rs`** (every error is user-facing text without the token):
- **Constants:** `OWNER="Sashanator"`, `REPO="conclave"`, `LATEST_RELEASE_URL="https://api.github.com/repos/Sashanator/conclave/releases/latest"`, `ASSET_PREFIX="https://api.github.com/repos/Sashanator/conclave/releases/assets/"`. They must be literals, because the Task 8 URL rule reads them. Foreign test URLs stay inside `#[cfg(test)]`.
- **`pub fn is_conclave_asset(u: &url::Url) -> bool`:** https, host `api.github.com`, no port, no userinfo, no query, path `/repos/Sashanator/conclave/releases/assets/<digits>`.
- **`pub fn latest_json_url(release: &serde_json::Value) -> Result<url::Url, String>`:** finds the asset named exactly `latest.json`. Missing: `"The latest release has no update file (latest.json)."` Foreign URL: `"The update file points outside the Conclave repository."`
- **`pub fn status_message(status: u16) -> String`:** 401/403: `"Token rejected by GitHub: check it has read access to Conclave."` 404: `"No published Conclave release found, or the token can't see the Conclave repository."` Anything else: `"GitHub answered {status}. Try again later."`
- **`pub const NETWORK_MESSAGE`** `= "Couldn't reach GitHub. Check your internet connection."`. **`pub fn scrub(msg, token) -> String`** replaces a non-empty token with `[token]`.
- **`pub async fn find_latest_json(token: &str) -> Result<url::Url, String>`:** Client: reqwest 0.11 with `user_agent("Conclave/<ver>")`, `redirect::Policy::none()` and a 30 s timeout. Headers: `Authorization: Bearer`, `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`. A non-2xx status → `status_message`. A transport error → `NETWORK_MESSAGE` plus `warn!` of `e.without_url()`. Headers are never logged.
- **`pub fn validate_token(raw) -> Result<String, String>`:** trimmed, 1–255 chars, all `is_ascii_graphic`. Empty: `"Paste a token first."` Otherwise invalid: `"That doesn't look like a GitHub token."`

**`settings.rs`** (store file `updates.json`, the pattern of `practice/settings.rs:80-107`):
- `pub struct LastCheck { pub at: String, pub result: String }` (camelCase).
- `check_on_start_from(Option<Value>) -> bool`: missing or non-bool → `true`.
- `last_check_from(Option<Value>) -> Option<LastCheck>`: lenient.
- Plus `check_on_start(app)`, `set_check_on_start(app, enabled) -> Result<(), String>` and `last_check(app)`.
- `record_check(app, result)`: `at = Utc::now().to_rfc3339()`; a store error is a `warn!`.

**Steps:**
- [ ] Failing tests. `RELEASE_JSON` is trimmed from the real v1.1.0 shape: an installer asset plus `latest.json` (id `700000001`), each with `url` and `browser_download_url`; `finds_latest_json_api_url`; `missing_latest_json_is_a_plain_error`; `foreign_asset_url_is_refused`; `is_conclave_asset_cases`: false for `http://`, `github.com/…/releases/download/…`, `repos/Other/conclave`, `?x=1`, `user@`, `:444`, `assets/abc`; `status_messages`; `messages_never_contain_the_token`: with `github_pat_SECRET123`, across every status message, `NETWORK_MESSAGE`, `latest_json_url` on a body containing the token, and `scrub`; `validate_token_trims_and_rejects`: no `Err` contains the input; `check_on_start_defaults_true`; `last_check_round_trips_camel_case`
- [ ] **RUST** `updates::`, then the guard. Its URL rule now scans the real `src/updates/`.
- [ ] Commit `feat(updates): GitHub latest.json lookup, error messages and update settings`.

### Task 11: Updater plugin, commands, start-up check and install

**Tag:** [judgment] [risky] (secrets, install/exit, concurrency, background task). Per-task review + Codex pass; silent-failure focus. Needs Tasks 1 and 8–10.

**Files:**
- `Cargo.toml`: add `tauri-plugin-updater = "2.14.1"` after `tauri-plugin-process` (`:164`), in the general dependencies. It needs tauri ^2.12 (2.12.5 is locked) and reqwest 0.13 (0.13.3 is locked).
- `lib.rs`: `:603`: `.plugin(tauri_plugin_updater::Builder::new().build())`. After `:769`: `updates::start_check(_app.handle().clone());`. `generate_handler!` (`:792`): the six `updates_*` commands.
- `updates/mod.rs`.

**Plugin facts** (2.14.1 source):
- `check()` sets `Accept: application/json` only when no Accept header is set, so our `application/octet-stream` returns the asset bytes; `download()` keeps it too.
- Builder headers go on both the check and the download, which is why `download_url` must pass `is_conclave_asset`.
- A non-2xx endpoint becomes `ReleaseNotFound`, so status texts come only from `find_latest_json`.
- `download()` verifies the signature. `install(bytes)` verifies again, then runs NSIS and exits.
- reqwest 0.13 strips `Authorization` on the cross-host 302 (`redirect.rs:239-251`).

**Interfaces:**
- The pinned camelCase types `UpdatesStatus`, `UpdateCheckResult` (status `&'static str`), `UpdateAvailable { version, notes: Option<String> }` and `Progress { downloaded: u64, total: Option<u64> }`.
- `fn token() -> Result<Option<String>, String>` reads `secrets::store().get(GITHUB_UPDATE_TOKEN)`.
- `async fn find_update<R>(app, token, timeout: Option<Duration>) -> Result<Option<Update>, String>`: `find_latest_json`, then `updater_builder().endpoints(vec![url])?.header("Authorization", Bearer)?.header("Accept", "application/octet-stream")?`, the optional timeout, `.build()?.check()`. Plugin errors become `"Couldn't read the update file: {e}"`, scrubbed. A foreign `u.download_url` → `Err("The update file points outside the Conclave repository.")`.
- `pub(crate) fn result_text(r) -> String`: "Up to date", "Version X.Y.Z is available", or the message.
- `pub(crate) fn should_emit_progress(downloaded, last, total) -> bool`: at each new 1 MiB, or when `downloaded == total`.
- `pub(crate) async fn announce_when_idle(busy: impl Fn() -> Option<BusyReason>, poll: Duration, emit: impl FnOnce())`.

**Commands:**
- `updates_get_status`: `currentVersion` from `package_info().version`; `tokenLast4` is the token's last 4 chars.
- `updates_set_token`: `validate_token`, then `set`. `updates_remove_token`: `delete`. `updates_set_check_on_start`: saves the setting.
- `updates_check`: No token → `not_set_up`, with no request and no `lastCheck`. Otherwise `find_update(.., Some(30 s))` → `available` (`notes = update.body`), `up_to_date` or `error`, then `record_check(result_text)`. On `available` it calls `announce(app, version, notes)`.
- `fn announce(app, version, notes)`: spawns `announce_when_idle(busy::current, 5 s, emit "update-available")`. A `static ANNOUNCING: AtomicBool` (swap, reset after the emit) keeps it to one waiting task, so repeated checks while busy don't stack prompts.
- `updates_install`:
  1. `static INSTALLING: AtomicBool` swap with a drop guard → `Err("An update is already being installed.")`.
  2. `busy::current()` → `Err(format!("busy: {r}"))`.
  3. No token → `Err("Updates are not set up: add a GitHub token in Settings.")`.
  4. `find_update(.., None)` (no timeout, so an 80 MB download isn't cut off). `None` → `Err("Conclave is already up to date.")`.
  5. `download(on_chunk)` emits `updates-progress` per `should_emit_progress`. A scrubbed error becomes `"Download failed: …"`. A bad signature fails here, and nothing changes.
  6. `busy::current()` **again** → `Err("busy: …")`. The bytes are dropped.
  7. `install(bytes)`. Errors are scrubbed.

**`start_check(app)`** spawns a task:
1. Sleep 30 s.
2. Return if `!check_on_start` or there is no token.
3. Run the `updates_check` body. Errors go only to `lastCheck` and a `warn!`.
4. `available` already called `announce` inside the `updates_check` body. Later persists nothing.

It is reached only after a successful DB open, so a failed open means no start-up check. That is acceptable.

**Steps:**
- [ ] Failing tests: `result_text_cases`; `progress_is_throttled`: `(500_000,0,Some(80M))` → false; `(1_048_576,0,_)` → true; `(80M,79.5M,Some(80M))` → true; `status_serializes_camel_case`: all pinned keys; `startup_announce_waits_for_idle`: the log is exactly `["busy","busy","idle","emit"]` (spec §8 gating); `startup_announce_never_emits_while_busy`: `timeout(50ms)` elapses; emit never runs; `announce_is_not_stacked`: two `announce` calls while busy → one emit after idle (test through a pure helper taking the flag, the busy fn and the emit closure).
- [ ] **RUST** `updates::`, then the guard. `cargo tree -p meetily -i reqwest@0.13.3 -e normal` lists `tauri-plugin-updater`.
- [ ] Commit `feat(updates): updater commands, start-up check and guarded install`.
- [ ] Phase end: **PHASE**, a Codex pass over Tasks 8–11, then **WINCI**.

## Phase 4: Release pipeline and docs (spec §6.2, §6.5)

### Task 12: CHANGELOG.md, release notes and `latest.json` in `release.yml`

**Tag:** [judgment] [risky] (release pipeline, signing secrets). Per-task review + Codex pass. No cargo.

**Why we write `latest.json` ourselves:**
- `tauri-action@v0` (= `action-v0.6.2`) builds the `url` from `browser_download_url`, which returns 404 on a private repo even with a token.
- v1.0.0 fixes that, but it is a breaking release.
- So we keep `@v0` with `includeUpdaterJson: false`.

**Files:**
- **Create `CHANGELOG.md`:** Header `# Conclave changelog`, then newest first: `## vX.Y.Z — YYYY-MM-DD`, each with 1–4 plain bullets. Seed from these tags: v1.1.0 10-30, v1.0.4/1.0.3/1.0.2 10-28, v1.0.1 10-25, v1.0.0 10-22, v0.9.0/0.8.0 10-21, v0.7.0 10-19, v0.6.0 10-18, v0.5.0 10-16, v0.4.0 10-15, v0.3.0 10-14, v0.2.0 10-13, v0.1.0 10-10 (all 2026). Skip the Meetily tags `v0.4.1`, `v0.2.1`, `0.1.1` and `test-v0.0.6`. Sources: `git log --oneline <prev>..<tag> --no-merges`, and `gh release view <tag> --json body` from v1.0.0 on. No v1.2.0 section (Task 18 adds it).
- **Create `scripts/changelog-section.mjs X.Y.Z [path]`:** prints the trimmed body of `## vX.Y.Z ` up to the next `## `. A missing or empty section, or a malformed version, exits 1 with `changelog-section: no section for vX.Y.Z`.
- **Create `scripts/latest-json.mjs <X.Y.Z> <notesFile> <sigFile> <assetApiUrl>`:** prints `{version, notes, pub_date, platforms: {"windows-x86_64": {signature, url}}}`. It exits 1 on a bad version, an empty signature, or a URL not matching `^https://api\.github\.com/repos/Sashanator/conclave/releases/assets/\d+$`.
- **Create `scripts/release-scripts.test.mjs`.**
- **`.github/workflows/release.yml`:**
  1. After "Stamp version from tag" (`:24-27`), add step `Release notes from CHANGELOG.md` (`id: notes`, tag-only). It writes `changelog-section.mjs "${GITHUB_REF_NAME#v}"` to `$RUNNER_TEMP/notes.md`. It outputs `body` via `body<<CONCLAVE_NOTES_EOF`: the notes, a blank line, then this footer. The footer replaces `:127`'s "uninstall the old version" instruction:
       > Unsigned build: on first install choose "More info → Run anyway". GPU transcription needs the CUDA 12 toolkit (cuBLAS) — see CUDA_SETUP.md in the repo; without it Whisper runs on CPU. To update: in Conclave open Settings → General → Updates and click Update (needs a read-only GitHub token), or quit Conclave from the tray icon (right-click → Quit) and run this setup; no uninstall needed, meetings, settings and models are kept. Summaries use your own cloud LLM key: Settings → Summary (default Claude Haiku 5.5).
  2. "Build installer" (`:117-129`): `env` gains `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from secrets. Set `releaseBody: ${{ steps.notes.outputs.body }}` and keep `includeUpdaterJson: false`.
  3. Before "Publish release" (`:201`), after the smoke test, add step `Upload latest.json to the draft` (tag-only, `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`). The release is still a draft, so `/releases/latest` can't see it yet.
     ```bash
     ver="${GITHUB_REF_NAME#v}"; exe="Conclave_${ver}_x64-setup.exe"
     sig="target/x86_64-pc-windows-msvc/release/bundle/nsis/${exe}.sig"
     [ -s "$sig" ] || { echo "::error::missing updater signature $sig"; exit 1; }
     url=$(gh release view "$GITHUB_REF_NAME" --json assets -q ".assets[] | select(.name == \"$exe\") | .apiUrl")
     node scripts/latest-json.mjs "$ver" "$RUNNER_TEMP/notes.md" "$sig" "$url" > "$RUNNER_TEMP/latest.json"
     gh release upload "$GITHUB_REF_NAME" "$RUNNER_TEMP/latest.json" --clobber
     ```

**Steps:**
- [ ] Failing tests: `section_is_extracted_between_headings`; `prefix_versions_do_not_match` (`1.2.0` vs `v1.2.01`); `missing_or_empty_section_fails`; `latest_json_has_windows_platform`; `latest_json_rejects_browser_url`; `latest_json_rejects_foreign_repo_and_empty_sig`; `real_changelog_has_current_release` (`1.1.0`)
- [ ] **SCRIPTS** (`actionlint release.yml`).
- [ ] Commit `feat(release): CHANGELOG-driven release notes; signed installer and latest.json with API asset URL`.

### Task 13: `docs/network-calls.md` and AGENTS.md

**Tag:** [mechanical].

**`docs/network-calls.md`:**
- Add two Hosts rows: `| api.github.com | src-tauri/src/updates/github.rs (LATEST_RELEASE_URL); src-tauri/src/updates/mod.rs (find_update, via tauri-plugin-updater) | Release lookup (releases/latest) and download of latest.json and the installer through API asset URLs; only when the user saved a GitHub token, on start (setting, default on) or a click. The token is sent only here | Allowed (own private repo; user's token) |` `| release-assets.githubusercontent.com | redirect target of the API asset download (reqwest strips Authorization on the cross-host redirect) | Download of latest.json and the signed installer | Allowed (GitHub asset host; no token) |`
- Rename `## Removed (updater tooling)` to `## Removed (Meetily updater tooling)`, adding: "The Meetily updater (Meetily endpoint and signing key) stays removed. Since 2026-11-16 the only updater is Conclave's own (spec `2026-11-16-conclave-safe-updates-design.md` §6.2); `scripts/check-no-phone-home.sh` fails on any other pubkey or endpoint."
- End the "Signing gate in `frontend/build.ps1`" bullet with "Superseded 2026-11-16: release builds now sign updater artifacts with the user's own `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD` secrets."

**`AGENTS.md`:**
- `:64` becomes "No PostHog, no analytics, and no Meetily-owned URLs except the Parakeet model host and the build-time ffmpeg-binaries download, both listed in `docs/network-calls.md`. The only update channel is the Conclave-owned updater: the user's private repo, the user's signing key, a read-only token, and an explicit user click." The rest of the bullet stays.
- `:43`: delete "The app has no updater." Add "Every tag needs a `## vX.Y.Z — date` section in `CHANGELOG.md` (release and update notes). The build signs the installer with the `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD` secrets and uploads `latest.json` (API asset URL) while the release is a draft."
- `:29` scripts row: add `changelog-section.mjs`, `latest-json.mjs` (release notes and update manifest).
- `:36-41`: add `` `node --test scripts/` (release and guard script tests) ``.

**Steps:**
- [ ] Edit, then run the guard. `git diff --stat` must show only these two files.
- [ ] Commit `docs: Conclave-owned updater in AGENTS invariant and network-calls audit`.

## Phase 5: Frontend and README (spec §6.2 Prompt/Install, §6.4, §6.5)

**House patterns:**
- Components call `invoke` (`@tauri-apps/api/core`) directly, like `KnowledgeSettings.tsx`.
- Inline errors are `<p className="text-sm text-red-600">` (`KnowledgeSettings.tsx:260`).
- Cards use `bg-card rounded-lg border border-border p-6 shadow-sm` (`PreferenceSettings.tsx:122`).
- The token lives in React state only between paste and Save.

### Task 14: Updates lib and browser-preview mocks

**Tag:** [mechanical].

**Files:** create `frontend/src/lib/updates.ts` and `frontend/tests/lib/updates.test.ts`; `frontend/src/lib/browser-preview.ts` gets `handlers` entries (`:35-56`) after `old_audio_list` (`:47`).

**Interfaces:**
```ts
export interface LastCheck { at: string; result: string }
export interface UpdatesStatus { tokenLast4: string | null; checkOnStart: boolean; lastCheck: LastCheck | null; currentVersion: string }
export interface UpdateCheckResult { status: 'not_set_up' | 'up_to_date' | 'available' | 'error'; version: string | null; notes: string | null; message: string | null }
export interface UpdateAvailable { version: string; notes: string | null }
export interface UpdateProgress { downloaded: number; total: number | null }
export interface BackupInfo { kind: 'pre-upgrade' | 'manual'; createdAt: string; fromVersion: string | null; version: string; sizeBytes: number; path: string }
export const UPDATE_AVAILABLE_EVENT = 'update-available';
export const UPDATES_PROGRESS_EVENT = 'updates-progress';
export const TOKEN_HELP = 'Create a fine-grained token with read-only Contents access to the Conclave repository only.';
export function checkMessage(r: UpdateCheckResult): { text: string; error: boolean }
export function testMessage(r: UpdateCheckResult): { text: string; error: boolean }
export function installError(e: unknown): string
export function progressLabel(p: UpdateProgress): string
export function backupLabel(b: BackupInfo): string
export function lastCheckLine(s: UpdatesStatus): string
```
**Copy:**
- `checkMessage`: `not_set_up` → "Add a GitHub token first." `up_to_date` → "Conclave is up to date." `available` → "Conclave v{version} is available." `error` → `message ?? 'Check failed.'` (`error: true`).
- `testMessage`: "Works" for `up_to_date`/`available`; the message for `error`; "No token saved." for `not_set_up`. The last two are errors.
- `installError`: `"busy: <reason>"` → "Conclave is busy ({reason}). Update when it finishes." Otherwise "Update failed: {e}. Nothing was changed."
- `progressLabel`: "Downloading… {floor(pct)}%"; "Installing… Conclave will restart." once `downloaded >= total`; with no total, "Downloading… {formatMegabytes(downloaded)}" (`@/lib/old-audio`).
- `backupLabel`: "Before upgrade v{fromVersion ?? '?'} → v{version}" or "Manual v{version}".
- `lastCheckLine`: "Not checked yet" or "Last checked {toLocaleString()}: {result}".

**Preview mocks:**
- `updates_get_status` → `{tokenLast4:'3f9a', checkOnStart:true, lastCheck:{at:'2026-11-16T09:00:00Z', result:'Up to date'}, currentVersion:'1.1.0'}`.
- `updates_check` → `available` 1.2.0 with notes. Void commands → `null`.
- `backup_list` → one row per kind, including `{kind:'pre-upgrade', createdAt:'2026-11-16T08:59:00Z', fromVersion:'1.0.4', version:'1.1.0', sizeBytes:48_200_000, path:'C:\\…'}`.
- `backup_now` → a manual row.

**Steps:**
- [ ] Failing tests: `checkMessage maps every status`; `testMessage reports Works for up_to_date and available`; `installError recognises busy`: `'busy: a recording is in progress or being saved'` → contains "busy (a recording is in progress or being saved)"; `'bad signature'` → the exact failure text; `progressLabel percent, done and unknown total`: `{50,100}` → "Downloading… 50%"; `{12_400_000,null}` → "Downloading… 12 MB"; `backupLabel both kinds`; `lastCheckLine never checked`
- [ ] **FE** `tests/lib/updates.test.ts`.
- [ ] Commit `feat(updates): frontend updates lib and preview mocks`.

### Task 15: Settings → General → Updates and Backups

**Tag:** [judgment] [risky] (the token passes through the UI; backup creation).

**Files:**
- Create `frontend/src/components/UpdatesSettings.tsx` and `frontend/tests/components/updates-settings.test.tsx`.
- `frontend/src/app/settings/page.tsx:130-132`: the General `TabsContent` becomes `<div className="space-y-6"><PreferenceSettings /><UpdatesSettings /></div>`. It does not go inside `PreferenceSettings`, which has early returns (`:107-114`) and is reused by `SettingsModal.tsx:83`.

**Behaviour, card "Updates":**
- "Current version v{currentVersion}".
- **No token saved:** `<Label htmlFor="update-token">GitHub token</Label>` with `<Input id="update-token" type="password" autoComplete="off">`. **Save** is disabled while empty or busy. It sends `updates_set_token({ token: value.trim() })`. Success clears the input and reloads the status. An error shows inline and keeps the input.
- **Token saved:** "Token saved ••••{tokenLast4}". **Test** calls `updates_check` and shows `testMessage`. **Remove** calls `updates_remove_token` (no confirm), then reloads.
- `TOKEN_HELP` always shows (`text-xs text-muted-foreground`).
- **Switch "Check for updates on start"** (`aria-label`): optimistic. It calls `updates_set_check_on_start({ enabled })`. On error it reverts and shows `toast.error('Could not save the setting', { description })`.
- **Check for updates** (disabled while running): calls `updates_check`, shows `checkMessage` inline, then reloads the status. It never emits `update-available`: Rust announces it once the app is idle (Task 11), so the one dialog opens then.
- **Status line:** `lastCheckLine`. If `updates_get_status` fails: "Could not load update settings".

**Behaviour, card "Backups":**
- **Back up now** ("Backing up…" while running): Success: `toast.success("Backed up ({formatMegabytes(sizeBytes)})")`, then reload the list. Error: `toast.error('Backup failed', { description })`, with no reload.
- **Open backups folder:** calls `backup_open_folder`; on error, `toast.error`.
- `<ul>` rows: `toLocaleString(createdAt)`, `backupLabel`, `formatMegabytes`, in Rust's order. "No backups yet" when empty; "Could not list backups" on error.

**Steps:**
- [ ] Failing tests. Mock and restore `@tauri-apps/api/core`, `@tauri-apps/api/event`, `sonner` and `ui/switch` as `tests/components/knowledge-settings.test.tsx:1-16,87-91` does (`afterAll` restore required); `saving a token sends it trimmed and never shows it`: Type `'  ghp_secretvalue1234  '` and Save. The call is `('updates_set_token', {token:'ghp_secretvalue1234'})`. After the status returns `tokenLast4 '1234'`: no password input, "Token saved ••••1234" shown, and no `ghp_secretvalue` in the rendered text; `failed save keeps the input and shows the error`; `Test reports Works`; `Test shows the GitHub error` ("Token rejected by GitHub: check it has read access to Conclave." verbatim); `Remove clears the saved token`; `toggle saves and reverts on failure`; `Check for updates shows an available update`: "Conclave v1.2.0 is available." and the event `emit` mock is never called; `Check for updates error shows inline`; `status line shows the last check`; `help line is verbatim`; `Back up now success reloads the list` (`backup_list` called twice); `Back up now failure shows an error and keeps the list`; `backup rows show date, label and size` ("Before upgrade v1.0.4 → v1.1.0", "48 MB"); `Open backups folder invokes the command`
- [ ] **FE** `tests/components/updates-settings.test.tsx`. Check the preview `/settings` by eye, then **A11Y**.
- [ ] Commit `feat(updates): Settings updates and backups section`.

### Task 16: Global update dialog

**Tag:** [judgment]. Install safety lives in Rust (Task 11). The UI never shows a refusal or failure as success.

**Files:**
- Create `frontend/src/components/Updates/UpdateDialog.tsx` and `frontend/tests/components/update-dialog.test.tsx`.
- `frontend/src/app/layout.tsx`: import it next to `OldAudioReminder` (`:25`) and render `<UpdateDialog />` after `<OldAudioReminder />` (`:298`). That is inside `AppShell`, so the banner and pill windows never mount it.

**Behaviour:**
- `listen` to both events on mount; unlisten on unmount.
- **`update-available`:** sets `{version, notes}`, opens the dialog, and clears any error or progress. It is ignored while installing.
- **Content:** Title "Conclave v{version} is available". Notes as plain text (`whitespace-pre-wrap max-h-64 overflow-y-auto text-sm`). **Later** (outline) closes and calls nothing.
- **Update:** sets `installing` and calls `invoke('updates_install')`. While installing, both buttons are disabled and `onOpenChange(false)` is ignored. The dialog shows `progressLabel` and `<Progress>` (a percentage only when the total is known). On reject: `installing=false`, `installError(e)` in red, and the dialog stays open for a retry. On resolve: it stays installing, because the app is about to exit. `updates-progress` applies only while installing.

**Steps:**
- [ ] Failing tests. Setup: mock `listen` into a handler map with an emit helper (`tests/components/summary-speaker-gate.test.tsx:76-95,252-253`); a passThrough `ui/dialog` that renders children only when `open` (`tests/components/old-audio-review.test.tsx:1-27`); restore in `afterAll`. Tests: `stays closed until update-available`; `shows version and notes`; `Later closes without installing`; `Update installs and shows progress` (`{50,100}` → "Downloading… 50%", Later disabled); `cannot be closed while installing`; `busy refusal shows the message and allows retry` (`'busy: a summary is being generated'`); `failed download shows error and changes nothing` ("Update failed: signature verification failed. Nothing was changed."); `unlistens on unmount`.
- [ ] **FE** `tests/components/update-dialog.test.tsx`. The preview has no Rust to emit the event: open the dialog via the Playwright MCP (`browser_evaluate` calling the preview's event emit for `update-available`, or a dev-only query flag if the preview has no emit hook), then run **A11Y** or check its title and description with `browser_snapshot`.
- [ ] Commit `feat(updates): update-available dialog with progress`.

### Task 17: README sections

**Tag:** [mechanical].

**File:** `/workspace/README.md` (today: `## Install` `:7`, `## Update` `:13-20`, `## Uninstall` `:22`, `## Credits` `:26`).
- New order: Install, Update, **Back up before you update**, **Restore from a backup**, Uninstall, Credits.
- Plain user language, no em dashes in the prose.

Each section must state these facts:

**`## Update`, in-app:**
- Create a fine-grained token with read-only **Contents** access to this repository only. Paste it in **Settings → General → Updates → GitHub token**, then click **Test**. It is stored in Windows Credential Manager.
- Conclave checks about 30 s after start (the switch is on by default) and on **Check for updates**. The dialog shows "Conclave vX.Y.Z is available" with the notes.
- **Update** downloads, verifies the signature, installs with a progress bar and restarts. **Later** asks again at the next start.
- It never asks or installs during a recording or its save, a Practice session or report, a summary being generated, or a model download.
- A failed start-up check shows nothing; the reason is in the Settings status line.

**`## Update`, manual:** keep today's paragraph ("do not uninstall first") and the data-location list (`:15-20`).

**`## Back up before you update`:**
- **Automatic:** on the first start of a new version, before the database changes. Covers `meeting_minutes.sqlite` plus `-wal`/`-shm`, every `.json` in `%APPDATA%\com.conclave.app`, and `%APPDATA%\Conclave\templates`. Not models or recordings.
- **Where:** `…\backups\pre-upgrade\<date>_v<old>-to-v<new>\`; `<date>_unknown-to-v<new>` when coming from a version before v1.2.0. `…\backups\manual\<date>_v<version>\`. Each has a `backup.json`. The last 3 pre-upgrade and the last 5 manual are kept. **Open backups folder** opens it.
- **If the backup fails:** quote the spec's dialog text. Quit changes nothing, and the next start tries again.
- **Recordings:** copy `%USERPROFILE%\Music\conclave-recordings` (or the folder set in Settings) by hand.
- **Manual backup:** use **Back up now**. By hand: quit from the tray first (closing the window keeps Conclave running), then copy the three folders.
- **Warning:** the uninstaller's "Delete the application data" checkbox also deletes `backups\`. Copy it elsewhere first.

**`## Restore from a backup`:**
1. Quit from the tray.
2. Delete the live `meeting_minutes.sqlite-wal` and `-shm`, if present. This is required: a newer journal next to an older DB corrupts it.
3. Copy the backup's `meeting_minutes.sqlite` (plus its `-wal`/`-shm`, if any) and its `.json` files into `%APPDATA%\com.conclave.app`. Copy `templates` over `%APPDATA%\Conclave\templates`. Leave out `backup.json`.
4. If the upgrade was the problem, run the older installer from Releases. The `v<old>` in the folder name says which.

Keys stay in Credential Manager, so a restore with the current version needs nothing else. **Downgrade:** "If you reinstall a version older than v1.2.0, paste your keys into Settings again."

**`## Uninstall`:** add one sentence: the data checkbox also deletes `backups\` (link to the warning).

**Steps:**
- [ ] Write the sections, then run the guard.
- [ ] Commit `docs(readme): in-app updates, backup and restore`.
- [ ] Phase end: full **FE**, **A11Y**, and a Codex pass over Tasks 12–17.

## Phase 6: First updater release (user checkpoint)

### Task 18: Release v1.2.0 and v1.2.1, end-to-end check on the PC

**Tag:** user + controller, [risky] (release). It runs after every lane is merged into `main`, preceded by a whole-branch Codex pass.

1. **Controller, v1.2.0:** Add `## v1.2.0 — <date>` to `CHANGELOG.md`: in-app updates, automatic backup, keys in Credential Manager. If the version differs, fix the README downgrade line. Commit, merge, tag. Watch the Release run to the end and report it. `gh release view v1.2.0 -R Sashanator/conclave --json assets -q '.assets[].name'` must list `Conclave_1.2.0_x64-setup.exe` and `latest.json`, and the release must be published.
2. **User, install over:** quit v1.1.0 from the tray and run the v1.2.0 setup **without uninstalling**. Meetings, settings, templates and the summary key are all still there. `backups\pre-upgrade\…_unknown-to-v1.2.0\` exists (v1.1.0 wrote no last-run file). **If installing over the old version loses data or settings,** the controller restores the uninstall instruction in the `release.yml` footer (Task 12) and in the README before the next tag.
3. **User, secrets:** Credential Manager → Windows Credentials lists `Conclave/anthropicApiKey` and the user's other keys. Summaries, Practice and the calendar still work. `calendar.json` has `"calendar_url":null`.
4. **User, token:** create a fine-grained token (repository `Sashanator/conclave` only, Contents: Read-only). Paste it → "Token saved ••••xxxx" → **Test** → "Works".
5. **User, backups:** **Back up now** adds a row. **Open backups folder** opens Explorer at `backups\`.
6. **Controller:** release `v1.2.1` (CHANGELOG-only) the same way.
7. **User, on v1.2.0:** Restart. About 30 s later the dialog shows v1.2.1 and its notes. **Later**, restart, and it asks again. Start a recording, then **Update** → the busy message, and nothing installs. Stop the recording and wait for the save, then **Update** → progress, the passive NSIS window, and a restart as v1.2.1. `backups\pre-upgrade\…_v1.2.0-to-v1.2.1\` exists.
8. **User, backup failure:** Move `backups\pre-upgrade` elsewhere and create an empty **file** named `pre-upgrade` in `backups\` (Windows ignores a folder's read-only flag), delete `last-run-version`, and launch → the Continue/Quit dialog appears. Quit → `meeting_minutes.sqlite` is byte-identical, and no `last-run-version` was written.
9. **User, token hygiene:** search the log folder and `updates.json` for `github_pat_` → no hits.
10. **User, bad token:** a wrong token, then **Test** → "Token rejected by GitHub…". Remove it → `not_set_up`, and no request is made on start.

## Spec coverage

| Spec | Tasks |
|---|---|
| §6.1 store / reads+writes / one-time move | 1 / 2, 3 / 4 |
| §6.2 signing, artifacts, passive / pipeline, `latest.json`, CHANGELOG | 0, 8 / 12, 18 |
| §6.2 check, token host, when, errors / busy, prompt, install | 10, 11 / 9, 11, 16 |
| §6.3 / §6.4 | 5, 6, 7 / 15 |
| §6.5 README+downgrades / AGENTS+network-calls / guard / signing first+checklist | 17 / 13 / 8 / 0, 18 |
| §8 secrets / backup+version / gating, lookup, token-free / guard / Windows CI / end-to-end | 1, 2, 4 / 5, 6, 7 / 9, 10, 11 / 8 / WINCI after 4, 7, 11 / 18 |

## Ledger

| Task | Status | Commit |
|---|---|---|
| 0 Signing key + secrets | pending | |
| 1 secrets module | pending | |
| 2 SettingsRepository via secrets | pending | |
| 3 calendar URL via secrets | pending | |
| 4 one-time move + compaction | pending | |
| 5 backup core | pending | |
| 6 startup order + Continue/Quit | pending | |
| 7 manual backup + commands | pending | |
| 8 updater config + guard | pending | |
| 9 busy state | pending | |
| 10 GitHub lookup + settings | pending | |
| 11 plugin, commands, install | pending | |
| 12 CHANGELOG + release.yml | pending | |
| 13 AGENTS + network-calls | pending | |
| 14 frontend updates lib | pending | |
| 15 Settings Updates/Backups | pending | |
| 16 update dialog | pending | |
| 17 README | pending | |
| 18 v1.2.0/v1.2.1 + PC check | pending | |
