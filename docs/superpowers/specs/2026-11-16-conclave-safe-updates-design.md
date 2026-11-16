# Conclave: safe updates (in-app updater, automatic backup, encrypted secrets)

Date: 2026-11-16 · Status: user answers folded in; awaiting approval of this document · First of three projects (safe updates → redesign → cleanup).

## 1. Why

Today the user updates Conclave by opening GitHub, downloading the installer from the private repo's Releases page and running it. They want the app to tell them about a new version and install it after they agree, the way Notion does. The coming redesign changes the database schema, so every upgrade must first take a backup the user can restore. While touching the secrets path, the user asked to move every stored secret (AI keys, calendar link, and the new GitHub token) out of plain-text files into Windows' encrypted credential store.

**Why there was "no updater":** the v1 spec (`2026-10-10-conclave-v1-design.md` §3.4) removed the updater inherited from Meetily. It checked Meetily's server and trusted Meetily's signing key, so Meetily's releases could put code on the user's PC (supply-chain risk) and the app phoned home on every start. This project adds a Conclave-owned updater: the user's private repo, the user's signing key, a read-only token, and an explicit "Update" click. The user approved changing the invariant on 2026-11-16.

## 2. Goals

1. The app checks the private repo for a newer published release (on start, and on "Check for updates"), shows the version and its notes, and installs only after the user clicks **Update**.
2. Every installed update is cryptographically verified (Tauri updater signature) before it runs.
3. On the first launch of a new version, before any database migration, Conclave backs up the user's data. The user can also back up on demand.
4. All secrets live in Windows Credential Manager (per-user, DPAPI-encrypted), not in SQLite or JSON files. Existing plain-text secrets are moved without loss.
5. The README explains how to back up manually and how to restore.

## 3. Non-goals

- No public releases repo (user chose option B: private repo + token).
- No silent or automatic install. No downgrade/rollback button (restore is manual, from README).
- No backup of recordings or downloaded models (see §6.3).
- No change to which features use which keys, and no Settings redesign: the "AI models & keys" page belongs to the cleanup project. The existing settings UI keeps working.
- No code signing of the installer (SmartScreen behaviour is unchanged).

## 4. Anti-goals (failure even if it "works")

- Losing or corrupting a secret during the move (a key that worked yesterday stops working).
- Installing while a recording, Practice session or a recording save is in progress.
- The GitHub token appearing in logs, error messages, the frontend, or any request to a host other than `api.github.com`.
- An update that installs without a valid signature.
- A backup that is silently skipped (the user must see if a backup failed and decide whether to continue).

## 5. Constraints (from the code)

- Migrations run in `DatabaseManager::new()` via `sqlx::migrate!` on every launch (`frontend/src-tauri/src/database/manager.rs:38`), before the UI. A backup "before the new version touches data" must run in Rust before that call.
- Journal mode is not set by Conclave; the code assumes WAL in places (`manager.rs:155,186`). A raw file copy of a live DB is unsafe; the backup copies before the pool opens, and includes any `-wal`/`-shm` files next to the DB.
- Data locations: `%APPDATA%\com.conclave.app` (`meeting_minutes.sqlite` + ~11 small JSON stores: `knowledge.json`, `calendar.json`, `calendar-cache.json`, `dictate.json`, `copilot.json`, `practice.json`, `speakers.json`, `costs.json`, `template_assist.json`, `onboarding-status.json`, `recording_preferences.json`, plus Tauri plugin-store files), `%APPDATA%\Conclave\templates` (custom summary templates), recordings in `%USERPROFILE%\Music\conclave-recordings` (user-changeable; Practice audio too), models under `%APPDATA%\com.conclave.app\models` (1–3 GB each).
- Secrets today (all plain text): SQLite `settings` (`groqApiKey`, `openaiApiKey`, `anthropicApiKey`, `ollamaApiKey`, `openRouterApiKey`, `customOpenAIConfig.apiKey`), `transcript_settings` (`deepgramApiKey`, `elevenLabsApiKey`, Groq/OpenAI), `licensing` (`license_key`, `encrypted_key`), an unused `geminiApiKey` column, and `calendar.json` `calendar_url`. Accessors: `database/repositories/setting.rs:70-265`, `knowledge/settings.rs:27,40,380`, `knowledge/calendar.rs:790-797`. Raw keys reach the frontend via `api_get_model_config`, `api_get_api_key`, `api_get_transcript_api_key`, `api_get_custom_openai_config` (`api/api.rs`).
- Release pipeline: `.github/workflows/release.yml` builds NSIS with tauri-action as a draft (`includeUpdaterJson: false`, hard-coded `releaseBody` at `:127`), runs size/delay-load/no-CUDA checks, then publishes with `gh release edit --draft=false`. `tauri.conf.json` has `createUpdaterArtifacts: false`. The repo version (`0.4.1`) is stamped from the tag at build time (`scripts/set-version.mjs`).
- Phone-home guard `scripts/check-no-phone-home.sh` denies `plugin-updater`, `tauri_plugin_updater`, `updater:default`, `createUpdaterArtifacts: true`, `latest.json`. `docs/network-calls.md` is the human audit list. AGENTS.md invariant "no updater" must be reworded.
- Recording state: `IS_RECORDING` (`audio/recording_commands.rs:39`). Quit from tray stops a recording and waits up to 120 s for the save (`tray.rs:149-175`).
- Only Linux is available locally; Windows-only code (Credential Manager) is verified by `cargo check --target x86_64-pc-windows-msvc` where possible and by the manual Windows CI run (`gh workflow run ci.yml -f full=true`).

## 6. Design

### 6.1 Secret store

- Add `keyring = { version = "3", features = ["windows-native"] }` (pin v3; v4 renamed features). Wrap it in a small module `secrets` with `get(name)`, `set(name, value)`, `delete(name)`, behind a trait so tests use an in-memory fake (on Linux the crate falls back to its mock store; the fake is ours, so tests don't depend on that).
- One generic credential per secret: target `Conclave/<name>`; keys stored in both `settings` and `transcript_settings` (Groq, OpenAI) get distinct names (e.g. `Conclave/transcript.groqApiKey`) (e.g. `Conclave/anthropicApiKey`, `Conclave/calendarUrl`, `Conclave/githubUpdateToken`). Values are well under the 2560-byte blob limit; the module rejects larger values with an error instead of truncating. Calls are serialized (one mutex) because concurrent access to one credential can fail on Windows.
- Every place that reads or writes a secret goes through `secrets`. The DB columns and the `calendar_url` JSON field stay in the schema (no destructive migration) but are left empty.
- The existing Tauri commands keep their shapes, so the current Settings UI works unchanged. (Masking keys in the UI is part of the cleanup project's "AI models & keys" page.)
- Logging: secret values are never logged; errors name the secret, never its value.

**One-time move** (runs at startup after the backup and migrations, idempotent): for each known secret with a non-empty plain-text value: write it to the store, read it back, compare; only on an exact match clear the plain-text field. On any failure: keep the plain-text value, log a warning naming the secret, and show one notification ("Couldn't move your keys to Windows encryption; they still work. Details in the log."). The app keeps reading the plain-text value as a fallback until a later move succeeds. Reads prefer the store and fall back to the plain-text field only while it is non-empty.

### 6.2 Updater

- Add `tauri-plugin-updater` 2.x. Signing: the user generates a key pair once (`tauri signer generate`); the public key goes in `tauri.conf.json` `plugins.updater.pubkey`, the private key and password in GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `bundle.createUpdaterArtifacts: true`. The plugin refuses any installer whose `.sig` doesn't verify against the embedded key.
- **Release pipeline:** the build produces the NSIS installer, its `.sig`, and a `latest.json` (`version`, `notes`, `pub_date`, `platforms["windows-x86_64"] = {url, signature}`). `url` must be the GitHub **API** asset URL (`https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>`), because `github.com/.../releases/download/...` returns 404 for private repos even with a token. The builder verifies whether tauri-action's generated `latest.json` uses API asset URLs; if not, a workflow step after upload looks up the installer's asset id with `gh api` and writes/uploads `latest.json` itself. `latest.json` is uploaded while the release is still a draft; drafts are invisible to `/releases/latest`, so the app only ever sees releases that passed the existing checks and were published.
- **Release notes:** a new `CHANGELOG.md` (one `## vX.Y.Z — date` section per version, plain user language) replaces the hard-coded `releaseBody`; the workflow extracts the tag's section for the release body and `latest.json` `notes`. The redesign's "What's new" page will read the same file. Start it with a short entry per recent version from git history.
- **Check** (Rust command `updates_check`): if no token is stored, do nothing and report "not set up". Otherwise `GET https://api.github.com/repos/<owner>/<repo>/releases/latest` with `Authorization: Bearer <token>`, find the `latest.json` asset, and pass its API URL to `UpdaterBuilder::endpoints` with headers `Authorization` and `Accept: application/octet-stream`; the plugin compares versions and returns the update or none. The token is sent only to `api.github.com`; reqwest drops `Authorization` on the redirect to GitHub's download host, which needs no token. Owner/repo are compile-time constants.
- **When:** on app start (setting "Check for updates on start", default on, ~30 s after launch so it never slows startup) and from a "Check for updates" button. Errors (network, 401/403 bad token, 404) are shown in Settings as a plain message ("Token rejected by GitHub: check it has read access to Conclave") and never as a modal on start; a start-up check fails quietly into the Settings status line.
- **Busy state** (blocks the prompt and the install): a recording or its save in progress, a Practice session, a Practice report being generated, a summary being generated (it is paid for; killing it wastes the call), a model download.
- **Prompt:** if a newer version exists, show a dialog: "Conclave vX.Y.Z is available", the notes, buttons **Update** and **Later**. Later = ask again on the next start. The dialog never appears while the app is busy (above); it waits until that ends.
- **Install:** on Update, re-check the busy state (refuse with a message if something started meanwhile), download with progress, verify the signature (plugin), then install with NSIS `installMode: "passive"` (progress bar, no questions). The app exits for the installer and the installer restarts it. A failed download or bad signature shows an error and changes nothing.

### 6.3 Backup

- **When:** (a) automatically on the first launch of a version different from the last one that ran, before `sqlx::migrate!`; (b) on demand from Settings ("Back up now"). The last-run version is stored in a small file in app data, written only after migrations succeed. An existing DB with no last-run file (upgrade from a version older than this feature) counts as a version change.
- **Once per version change:** the pre-upgrade backup is taken once per `old → new` pair. If a pre-upgrade backup for the same pair already exists (a previous launch failed after backing up), it is not repeated, so the clean pre-migration copy is never replaced by a partly migrated one (sqlx commits each successful migration).
- **What:** `meeting_minutes.sqlite` with its `-wal`/`-shm` files if present, every `*.json` in `%APPDATA%\com.conclave.app`, and `%APPDATA%\Conclave\templates`. Not models (re-downloadable, GBs) and not recordings (large; updates and migrations never touch audio files). Confirmed by the user on 2026-11-16; the README says how to copy the recordings folder by hand.
- **How:** (a) runs before the pool opens, so a file copy is consistent. (b) runs while the app is open, so the DB is copied with SQLite's online backup (`VACUUM INTO` a new file), not a raw copy; JSON files and templates are plain copies.
- **Where:** `%APPDATA%\com.conclave.app\backups\pre-upgrade\<YYYY-MM-DD_HHMM>_v<old>-to-v<new>\` and `backups\manual\<YYYY-MM-DD_HHMM>_v<version>\`, each with a `backup.json` manifest (kind, versions, date, file list, sizes).
- **Retention (approved by the user 2026-11-16):** the two kinds are kept separately: the last **3** pre-upgrade backups and the last **5** manual ones. Older ones of the same kind are deleted only after a new backup of that kind completed and its manifest was written. Nothing else ever deletes a backup.
- **Failure:** if the automatic backup fails (disk full, permission), Conclave does **not** run migrations: it shows "Couldn't back up your data before upgrading. Continue without a backup, or quit?" (Continue runs migrations; Quit exits with nothing changed). Never a silent skip.
- Note: a pre-move backup contains the plain-text secrets that the user's files hold today; backups sit in the same per-user app-data folder as the DB, so exposure doesn't change.

### 6.4 Settings → Updates (new section on the existing General tab)

- GitHub token: paste field (masked after save, shows last 4 chars), **Test** (calls `updates_check` and reports "Works" / the error), **Remove**. A help line: "Create a fine-grained token with read-only Contents access to the Conclave repository only."
- "Check for updates on start" toggle, **Check for updates** button, status line (last check time and result).
- **Back up now**, **Open backups folder**, list of the last backups (date, version, size).

### 6.5 Docs and guards

- **README:** new section "Back up before you update" (what the automatic backup covers, where it is, how to copy recordings, how to back up manually: quit from the tray first, then copy the three folders) and "Restore from a backup" (quit from tray, copy the backup's files back, reinstall the matching older installer from Releases if the new version's migration was the problem). Update the "Update" section: in-app updates need the token; manual install still works.
- **AGENTS.md:** reword the invariant to "No telemetry and no phone-home. The only update channel is the Conclave-owned updater: the user's private repo, the user's signing key, a read-only token, and an explicit user click." Remove the "app has no updater" sentence under Releases.
- **`docs/network-calls.md`:** add `api.github.com` (release lookup + asset download, only with a token) and GitHub's asset download host; move the updater out of "Removed".
- **`scripts/check-no-phone-home.sh`:** drop the updater deny patterns; instead fail if any updater endpoint or `pubkey` other than the Conclave values appears (so a Meetily endpoint can never come back).
- **Downgrades:** after the move, older Conclave versions (from before this project) find empty key fields. The README restore section says: if you reinstall a version older than this project's release, paste your keys into Settings again. Restoring a backup with the current version needs nothing. (My call: keeping plain-text copies for downgrade safety would defeat the point of the move.)
- **Signing key first (my call):** the plan starts with a blocking checkpoint where the user generates the key pair and adds the two GitHub secrets, guided step by step. Until the pubkey is in the config, the updater code ships disabled (no plugin registration, no `createUpdaterArtifacts`), so `main` stays releasable throughout.
- **Release checklist (manual, user):** generate the signing key, add the two GitHub secrets, create the read-only token, paste it into Settings once.

## 7. Approaches considered

- **Hosting:** A) public releases-only repo + stock updater (least code; installers public) vs **B) private repo + read-only token** (chosen by the user; small custom check step, token stored encrypted).
- **Backup timing:** before install in the old app vs **at first launch of the new version, before migrations** (chosen: one mechanism covers in-app and manual installs, and the DB is closed so the copy is consistent).
- **Secret storage:** Windows Credential Manager via `keyring` (chosen: per-user DPAPI, no own crypto) vs app-level encryption with a key on disk (key sits next to the data; no real gain).

## 8. Testing

- Unit (Linux, fake store): secret move (success clears plain text; read-back mismatch keeps it; store error keeps it and reports; idempotent second run); reads prefer store then fallback; values over 2560 bytes rejected.
- Unit: backup (copies DB + JSON + templates, manifest correct, retention keeps 5, failure returns an error that blocks migrations); version-change detection (first ever run, same version, new version).
- Unit: update gating (no prompt while recording/practice/saving; prompt after it ends); `latest.json` asset lookup from a canned GitHub response; token never present in error strings.
- Guard: `check-no-phone-home.sh` passes with the Conclave updater config and fails with a foreign endpoint.
- Windows CI (manual dispatch) for the `keyring` Windows build; one real end-to-end update test on the user's PC from vN to vN+1 (checklist in the plan).

## 9. Left to the builder

- Exact UI copy beyond what is quoted here; the Rust module layout; whether `updates_check` lives in a new `updates` module; how the busy state is aggregated (recording, Practice, save in progress).
- Whether tauri-action's `latest.json` already uses API asset URLs (verify; otherwise write it in the workflow).
