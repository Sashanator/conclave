# Conclave Phase 0 (Baseline Fork) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn Meetily v0.4.1 into a Conclave baseline in `Sashanator/conclave`:
- renamed;
- no telemetry and no updater;
- Windows CI that tests every change and builds an unsigned NSIS installer on tags;
- the `conclave-probe` diagnostic tool for the Phase 2 pre-flight checks.

**Architecture:**
- Import Meetily's git history into `/workspace` and push it to the private repo.
- Make all Phase 0 changes on branch `phase-0-baseline` and merge it through a PR.
- A Windows CI job (`ci.yml`) tests every push to the PR. A release job (`release.yml`) builds the installer.
- The detector's Windows layer lives in a new workspace crate, `crates/meeting-detector`, whose binary is the probe.

**Tech Stack:**
- Tauri 2.6 with Rust stable; Next.js 14 with pnpm 9.15.9 and Node 20; Bun (the frontend tests).
- GitHub Actions on `windows-latest`, with `tauri-apps/tauri-action@v0`.
- `winreg` 0.55 and `windows` 0.61 (meeting-detector only).

**Spec:** `docs/superpowers/specs/2026-10-10-conclave-v1-design.md`. This plan covers §3 and the probe from §5.4. Phase 1 (§4) and Phase 2 (§5) get their own plans, written at the Phase 0 checkpoint.

## Execution setup (user's choice, 2026-10-10)

The plan runs subagent-driven, with these models:

| Role | Agent | Model / effort |
|---|---|---|
| Orchestration (this plan, task hand-off, CI watching, merge decisions) | main session | Opus 5.5, high |
| Implementation of each task, and code search | `general-purpose` (implementation) / `Explore` (search) | `model: "haiku"` (Haiku 5.5), `effort: "high"` |
| Per-task and phase code review | `reviewer` | `model: "opus"` (Opus 5.5), `effort: "medium"` |
| Codex pass for hard or dangerous changes (Tasks 1, 4, 5, 6, 9, 10) | inside the `reviewer` brief | `codex review -c model="gpt-6.1-sol" -c model_reasoning_effort="medium"` (Codex Sol 6.1, medium) |

## Global Constraints

- **Target:** Windows 10 x64. Development happens in a Linux container; Windows behaviour is verified only by CI on `windows-latest` and on the user's PC.
- **Repo:** `https://github.com/Sashanator/conclave`, private, default branch `main`, with Meetily as remote `upstream` (read-only). There are no regular upstream merges.
- **Rename scope:** user-visible only. `productName` → `Conclave`; `identifier` → `com.conclave.app`; window title, tray, notifications, UI strings, the user agent, and data folder names → `Conclave`. Internal crate, module and folder names stay as they are.
- **Telemetry:** removed entirely. No PostHog, no `track_*`, no analytics module, no consent UI.
- **Updater:** removed entirely (plugin, config, capability, UI, tray item). `createUpdaterArtifacts` is `false`.
- **Installer:** NSIS only (`--bundles nsis`) and unsigned. No `.msi`, no `signCommand`, no `SM_*`, `SUPABASE_*`, `MEETILY_RSA_*` or `TAURI_SIGNING_*` secrets.
- **Versioning:** the version comes from tag `vX.Y.Z`. The first release is `v0.1.0`.
- **CI minutes (about 3,000 per month, Windows counts double): minimise them.**
  - **Verify locally first, for free:** frontend type-check, `bun test` and build (no lint: user ruling at Task 2); `cargo test` on Linux; `cargo check --target x86_64-pc-windows-msvc` for `meeting-detector`; `actionlint` for workflows; and the guard script.
  - **Push only at CI checkpoints:** Task 3 (Windows baseline), Task 9 (detector on Windows), and Task 10 (review and merge, then the tag). Tasks 4–8 commit locally and are pushed together with Task 9.
  - **`ci.yml` keeps every run cheap:**
    - The frontend checks and guard run on `ubuntu-latest`, which bills at 1×.
    - Only the small `meeting-detector` job runs on Windows for each PR push.
    - The full Windows app test job (about 60 billed minutes when cold) runs only on manual dispatch with `full=true`.
    - Doc-only changes skip CI (`paths-ignore`), runs are cancelled when superseded, every job has a tight `timeout-minutes`, and `rust-cache` keeps its cache even when a job fails.
  - **Installer builds:** only `release.yml` on a `v*` tag builds one. There are no trial release runs.
  - No CUDA in Phase 0.
  - **Budget for Phase 0:** about 250 billed minutes (one cold Windows baseline, about 3 detector runs, about 3 Linux check runs, one release build).
- **Allowed outbound hosts:** Hugging Face and the existing model hosts, the user-configured LLM provider hosts, and localhost Ollama. Nothing else.
- **Commit trailer:** every commit ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Per `CLAUDE.md`:** each task ends with its tests plus a `reviewer` review of the task's diff. Tasks 1, 4, 5, 6 and 9 also get a Codex pass, because they touch credentials, data, network or OS input. The phase ends with a `reviewer` and Codex review of the whole branch, then one recheck after fixes.

## Review Focus

1. **Secrets committed by accident.** `/workspace/.claude/` holds `.credentials.json` and session files, and `.devcontainer/.Xauthority` is a display cookie. Only `.claude/settings.json`, `.claude/statusline.sh` and `.claude/agents/reviewer.md` may be tracked. Pinned by Task 1, Step 6.
2. **Conclave reading Meetily's data on a PC that has both.** Hard-coded `"Meetily"` data paths (`%APPDATA%\Meetily\templates`, the models fallback) would make Conclave pick up Meetily's files. Pinned by the guard check in Task 7.
3. **The installer version doesn't match the tag.** If `v0.1.0` produces an installer stamped 0.4.1, the next upgrade misbehaves. Pinned by `scripts/set-version.test.mjs` in Task 8.
4. **First launch breaks after analytics removal.** Settings, About and post-recording navigation must still work with the analytics provider gone and with an old `analytics.json` store file. Pinned by the frontend type-check, `bun test` and build in Task 4, plus the checkpoint checklist in Task 10.
5. **The probe crashes on a real PC.** Multi-process audio sessions return an error from `GetProcessId`, and some devices have no friendly name. The probe must skip these, not panic. Pinned by the `capture_sessions_never_errors_on_runner` test and by `Option` handling in Task 9.

---

## File Structure (Phase 0)

| Path | Change | Responsibility |
|---|---|---|
| `AGENTS.md` | create | Agent-neutral project guide: what Conclave is, the layout, commands, CI, invariants, and Meetily's codebase guide |
| `CLAUDE.md` | replace | `@AGENTS.md` import, plus the Claude Code working rules and agent routing |
| `.gitignore` | modify | Meetily's ignores plus the workspace's secret and agent ignores |
| `LICENSE.md` | modify | Keep the Zackriya copyright and add a Conclave line |
| `README.md` | replace | A short Conclave README |
| `.devcontainer/Dockerfile`, `.devcontainer/devcontainer.json` | modify | Rust toolchain and Tauri Linux build dependencies |
| `.github/workflows/*` | delete all, create `ci.yml` and `release.yml` | Conclave CI |
| `scripts/check-no-phone-home.sh` | create | Guard against telemetry, the updater, Meetily hosts and Meetily data paths |
| `scripts/set-version.mjs`, `scripts/set-version.test.mjs` | create | Stamp the version from the tag into tauri.conf, package.json and Cargo.toml |
| `docs/network-calls.md` | create | Outbound-call audit result |
| `docs/probe.md` | create | How the user runs `conclave-probe.exe` |
| `frontend/src-tauri/src/analytics/` | delete | Telemetry |
| `frontend/src/lib/analytics.ts`, `frontend/src/components/Analytics{Provider,ConsentSwitch,DataModal}.tsx` | delete | Telemetry |
| `frontend/src/services/updateService.ts`, `frontend/src/hooks/useUpdateCheck.ts`, `frontend/src/components/Update{Dialog,Notification,CheckProvider}.tsx` | delete | Updater |
| `frontend/src-tauri/tauri.conf.json`, `frontend/src-tauri/Cargo.toml`, `frontend/src-tauri/src/lib.rs`, `frontend/src-tauri/src/tray.rs`, `frontend/src-tauri/capabilities/*.json` | modify | Rename, remove telemetry and the updater, bundle settings |
| `frontend/src-tauri/scripts/sign-windows.ps1` | delete | Paid signing |
| `Cargo.toml` (root) | modify | Add the `crates/meeting-detector` member |
| `crates/meeting-detector/` | create | `consent.rs` (pure), `windows.rs` (Win32), `bin/conclave-probe.rs` |

---

### Task 1: Bootstrap the repo from Meetily

**Files:**
- Create: `AGENTS.md`
- Replace: `/workspace/CLAUDE.md`
- Modify: `/workspace/.gitignore`, `LICENSE.md`
- Delete: `.github/workflows/*.yml`, `.github/workflows/*.md`
- Keep: `.github/force-portable-ggml.cmake`, `.github/verify-portable-ggml.cjs`, `.github/verify-portable-ggml.test.cjs`, `.github/ISSUE_TEMPLATE`, and the templates

**Interfaces:**
- Produces: the git repo at `/workspace` on `main`, pushed to `origin` (`Sashanator/conclave`), with `upstream` = Meetily. All later tasks branch from this.

- [ ] **Step 1: Back up the workspace files that collide with Meetily's**

```bash
BK=/tmp/claude-1000/-workspace/856fdc4d-db31-4468-b4a5-55dd868575b4/scratchpad/ws-backup
mkdir -p "$BK" && cp /workspace/CLAUDE.md /workspace/.gitignore "$BK"/
```

- [ ] **Step 2: Import Meetily's history into `/workspace`**

```bash
cd /workspace
git init -b main
git remote add upstream https://github.com/Zackriya-Solutions/meeting-minutes.git
git fetch --no-tags upstream main
git checkout -f -B main upstream/main
git log --oneline -1   # expect a2cb62e (or a later upstream main, which is fine)
```

The untracked workspace files (`.claude/`, `.devcontainer/`, `.vscode/`) stay in place. `CLAUDE.md` and `.gitignore` are now Meetily's versions, which the next step merges.

- [ ] **Step 3: Write `AGENTS.md` (the project description for any coding agent)**

Create `/workspace/AGENTS.md` with the content below. Then append Meetily's `CLAUDE.md` (now in the working tree) from its `## Project Overview` section to the end, under a heading `## Codebase guide (inherited from Meetily)`. While doing that: demote its headings one level, replace "Meetily" with "Conclave" in prose, and delete its "Legacy Backend Archive" and "Service Endpoints" subsections (the backend is unsupported and not shipped).

```markdown
# AGENTS.md: Conclave

Guide for coding agents (Claude Code, Codex and others) working in this repo.

## What Conclave is

Conclave is a personal Windows 10 desktop app that replaces Notion AI Meeting Notes. It:
- records online calls (Zoom desktop; Google Meet and Microsoft Teams in a browser or the Teams app), capturing the microphone and system audio;
- transcribes them **locally** with Whisper, live, with automatic language detection (Russian, English, German/Russian mixed);
- summarizes them with a **cloud LLM** using the user's own key (Claude, OpenAI, Groq, OpenRouter, or an OpenAI-compatible endpoint).

The priorities, in order, are **cost**, then **control and features**, then **privacy**.

It is an **independent fork of Meetily v0.4.1** (github.com/Zackriya-Solutions/meeting-minutes, MIT). The `upstream` remote is read-only: there are no regular merges, only occasional cherry-picks of individual fixes.

- Design spec: `docs/superpowers/specs/2026-10-10-conclave-v1-design.md`
- Plans: `docs/superpowers/plans/`

## Layout

| Path | What |
|---|---|
| `frontend/` | Next.js 14 + React 18 UI (pnpm), with Bun tests in `frontend/tests/` |
| `frontend/src-tauri/` | Tauri 2 Rust core (crate `meetily`, lib `app_lib`): audio capture, Whisper/Parakeet, storage, summaries |
| `crates/meeting-detector/` | Windows meeting detection (mic consent store, change notifications, capture sessions) and the `conclave-probe` diagnostic binary |
| `llama-helper/` | Sidecar for the local LLM (bundled; not used for summaries in v1) |
| `scripts/` | `check-no-phone-home.sh` (CI guard), `set-version.mjs` (release version stamp) |
| `.github/workflows/` | `ci.yml` (PR checks), `release.yml` (installer on `v*` tags) |
| `backend/` | Archived Meetily Python backend. Unsupported; do not touch. |

## Platform and build facts

- **Target:** Windows 10 x64 only. The user's PC has an NVIDIA RTX 4070 Ti SUPER. Development happens in a Linux devcontainer, which has no Windows toolchain and no GPU.
- **Verified on Linux (free):**
  - `cd frontend && pnpm install --frozen-lockfile && pnpm exec tsc --noEmit && bun test && pnpm run build`
  - `cargo test --workspace --exclude llama-helper`. It needs `frontend/out` (from `pnpm run build`) and a stub sidecar: `touch frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu`.
  - `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests`
  - `bash scripts/check-no-phone-home.sh`, and `actionlint` for workflows
- **Verified only on Windows:** the Windows-only Rust code, through GitHub Actions on `windows-latest`. CI minutes are scarce (about 3,000 per month, Windows counts double): verify locally, push only at checkpoints, and run the full Windows app tests only by manual dispatch (`gh workflow run ci.yml -f full=true`).
- **Releases:** tag `vX.Y.Z` → `release.yml` builds an unsigned NSIS `Conclave_X.Y.Z_x64-setup.exe` and publishes it on the private repo. The app has no updater.

## Invariants (do not break)

- **No telemetry and no phone-home.** No PostHog, no analytics, no updater, and no Meetily-owned URLs. `scripts/check-no-phone-home.sh` enforces this in CI. Allowed outbound calls are listed in `docs/network-calls.md`.
- **App identity is `Conclave` / `com.conclave.app`.** Data folders are named `Conclave`, never `Meetily`. The only user-visible "Meetily" is the MIT credit in About.
- **Rename only what users see.** Internal crate, module and folder names stay as Meetily had them unless code is being touched anyway.
- **Never commit secrets:** `.claude/` (except `settings.json`, `statusline.sh`, `agents/reviewer.md`), `.env*`, `.devcontainer/.Xauthority`.
```

- [ ] **Step 3b: Write `CLAUDE.md`**

Replace `/workspace/CLAUDE.md` with:

````markdown
@AGENTS.md

# Claude Code working rules

<the backup's CLAUDE.md body, verbatim: "$BK/CLAUDE.md">

## Agent routing (user preference)

- Orchestration: the main session (Opus 5.5, high effort).
- Implementation and code-search subagents: `model: "haiku"` (Haiku 5.5), `effort: "high"`.
- Code review: the `reviewer` agent with `model: "opus"` (Opus 5.5), `effort: "medium"`.
- Codex pass for hard or dangerous changes (security or credentials, data persistence or deletion, concurrency or restart, untrusted input): tell the reviewer to run Codex as
  `codex review -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" -c sandbox_mode='"danger-full-access"' -`.
````

Build it with:

```bash
cd /workspace
{ printf '@AGENTS.md\n\n# Claude Code working rules\n\n'; cat "$BK/CLAUDE.md"; cat <<'EOF'

## Agent routing (user preference)

- Orchestration: the main session (Opus 5.5, high effort).
- Implementation and code-search subagents: `model: "haiku"` (Haiku 5.5), `effort: "high"`.
- Code review: the `reviewer` agent with `model: "opus"` (Opus 5.5), `effort: "medium"`.
- Codex pass for hard or dangerous changes (security or credentials, data persistence or deletion, concurrency or restart, untrusted input): tell the reviewer to run Codex as
  `codex review -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" -c sandbox_mode='"danger-full-access"' -`.
EOF
} > CLAUDE.md.new
```

Then, after Step 3's AGENTS.md has absorbed Meetily's guide, run `mv CLAUDE.md.new CLAUDE.md`. Expected: `head -1 CLAUDE.md` prints `@AGENTS.md`.

- [ ] **Step 4: Merge `.gitignore`**

```bash
cd /workspace
{ printf '\n# ---- Conclave workspace ----\n'; cat "$BK/.gitignore"; } >> .gitignore
```

- [ ] **Step 5: Remove upstream workflows and add the Conclave license line**

```bash
cd /workspace
git rm -q .github/workflows/*.yml .github/workflows/*.md
sed -i 's/^Copyright (c) 2024 Zackriya Solutions$/Copyright (c) 2024 Zackriya Solutions\nCopyright (c) 2026 Sashanator (Conclave modifications)/' LICENSE.md
head -4 LICENSE.md   # expect both copyright lines
```

- [ ] **Step 6: Stage everything and verify no secrets are tracked**

```bash
cd /workspace
git add -A
git ls-files | grep -E '(^|/)\.claude/' ; echo "---"
git ls-files | grep -Ei 'credentials|\.xauthority|(^|/)\.env($|\.)|history\.jsonl|sessions/' ; echo "exit=$?"
```

Expected: the first list is exactly `.claude/agents/reviewer.md`, `.claude/settings.json` and `.claude/statusline.sh`. The second grep prints nothing and `exit=1`. If anything else appears, fix `.gitignore` and re-stage before continuing.

- [ ] **Step 7: Commit and push `main`**

```bash
cd /workspace
git commit -q -m "chore: start Conclave from Meetily v0.4.1

Imports Meetily history, adds Conclave workspace config, spec and plan,
removes upstream CI workflows, adds Conclave copyright line.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git remote add origin https://github.com/Sashanator/conclave.git
git push -u origin main
gh repo view Sashanator/conclave --json defaultBranchRef,isEmpty   # expect main, isEmpty=false
```

- [ ] **Step 8: Review**

Get a `reviewer` review with a Codex pass of commit `HEAD` (it touches credentials and ignore rules). The review covers: the secrets check (Review Focus 1), AGENTS.md being accurate (every command in it actually runs) and CLAUDE.md importing it, and LICENSE compliance.

---

### Task 2: Dev toolchain and local baseline

**Files:**
- Modify: `.devcontainer/devcontainer.json`, `.devcontainer/Dockerfile`

**Interfaces:**
- Produces: `cargo`, `rustup` (with the `x86_64-pc-windows-msvc` target) and `bun` in the container, plus a recorded baseline of which upstream checks already fail.

- [ ] **Step 1: Make the toolchain persistent across container rebuilds**

In `.devcontainer/devcontainer.json`, add this entry to `"features"`:

```json
"ghcr.io/devcontainers/features/rust:1": { "version": "stable", "targets": "x86_64-pc-windows-msvc" }
```

In `.devcontainer/Dockerfile`, append before the final lines:

```dockerfile
# Tauri 2 / Conclave Linux build dependencies (local cargo check/test; Windows is built in CI)
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake pkg-config clang libclang-dev \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
        libasound2-dev libpulse-dev \
    && rm -rf /var/lib/apt/lists/*
# Bun runs the frontend tests (bun:test); actionlint checks workflows locally (saves CI minutes)
RUN npm install -g --prefix /usr/local bun@1.2.21 \
    && curl -sSfL https://raw.githubusercontent.com/rhysd/actionlint/v1.7.7/scripts/download-actionlint.bash | bash -s -- 1.7.7 /usr/local/bin
```

- [ ] **Step 2: Install the same tools in the running container**

```bash
sudo apt-get update && sudo apt-get install -y --no-install-recommends build-essential cmake pkg-config clang libclang-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libasound2-dev libpulse-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env" && rustup target add x86_64-pc-windows-msvc
sudo npm install -g --prefix /usr/local bun@1.2.21
curl -sSfL https://raw.githubusercontent.com/rhysd/actionlint/v1.7.7/scripts/download-actionlint.bash | bash -s -- 1.7.7 /tmp && sudo mv /tmp/actionlint /usr/local/bin/
cargo --version && bun --version && actionlint -version
```

If an apt package name doesn't exist on Debian bookworm, use the name the error suggests and update the Dockerfile to match.

- [ ] **Step 3: Record the frontend baseline**

```bash
cd /workspace/frontend
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit; echo "tsc=$?"
pnpm run lint; echo "lint=$?"
bun test; echo "bun=$?"
pnpm run build; echo "build=$?"
```

- [ ] **Step 4: Record the Rust baseline (Linux)**

```bash
cd /workspace
mkdir -p frontend/src-tauri/binaries && touch frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu
cargo test --workspace --exclude llama-helper 2>&1 | tail -40; echo "cargo=${PIPESTATUS[0]}"
```

The stub sidecar exists only so that `tauri-build` finds the `externalBin` path. It is ignored by git through `.gitignore`. Check that it is: `git check-ignore frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu` should print the path. If it doesn't, add `frontend/src-tauri/binaries/llama-helper-*` to `.gitignore`.

- [ ] **Step 5: Stop if the baseline is red**

If any of `tsc`, `lint`, `bun`, `build` or `cargo` failed, **stop** and report the exact failures to the user. They are upstream failures, and the user decides whether to fix them or mark specific tests as ignored before CI enforces them. Don't silently ignore anything.

- [ ] **Step 6: Commit**

```bash
cd /workspace && git checkout -b phase-0-baseline
git add .devcontainer/ .gitignore
git commit -q -m "chore(dev): add Rust, Tauri Linux deps and Bun to devcontainer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Review**

Get a `reviewer` review of the commit, including the baseline results.

---

### Task 3: CI workflow (`ci.yml`), built to save minutes

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: the `CI` workflow. Later tasks extend it: Task 4 adds the guard step to the `checks` job, and Task 9 adds the `detector` job.
  - `checks` runs on `ubuntu-latest` (1× billing) on every PR push.
  - `windows-app` runs the full Windows app tests only on `workflow_dispatch` with `full=true`.

- [ ] **Step 1: Write the workflow**

```yaml
name: CI

on:
  pull_request:
    paths-ignore: ['docs/**', '**/*.md', '.devcontainer/**', '.claude/**', '.vscode/**']
  workflow_dispatch:
    inputs:
      full:
        description: 'Also run the full Windows app tests (slow: ~30–60 billed minutes)'
        type: boolean
        default: false

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  checks:
    name: Frontend checks (Linux)
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4

      - uses: pnpm/action-setup@v4
        with:
          version: 9.15.9
          run_install: false

      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: pnpm
          cache-dependency-path: frontend/pnpm-lock.yaml

      - uses: oven-sh/setup-bun@v2
        with:
          bun-version: 1.2.21

      - name: Frontend (install, typecheck, test, build)
        working-directory: frontend
        run: |
          pnpm install --frozen-lockfile
          pnpm exec tsc --noEmit
          bun test
          pnpm run build

  windows-app:
    name: Windows app tests (manual, full)
    if: github.event_name == 'workflow_dispatch' && inputs.full
    runs-on: windows-latest
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4

      - uses: pnpm/action-setup@v4
        with:
          version: 9.15.9
          run_install: false

      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: pnpm
          cache-dependency-path: frontend/pnpm-lock.yaml

      - uses: dtolnay/rust-toolchain@stable

      - uses: swatinem/rust-cache@v2
        with:
          workspaces: ". -> target"
          key: ci-app-v1
          cache-on-failure: true

      - name: Build frontend (tauri embeds frontend/out at compile time)
        working-directory: frontend
        run: |
          pnpm install --frozen-lockfile
          pnpm run build

      - name: Stub llama-helper sidecar (tauri-build checks externalBin exists; tests never run it)
        shell: bash
        run: |
          mkdir -p frontend/src-tauri/binaries
          touch frontend/src-tauri/binaries/llama-helper-x86_64-pc-windows-msvc.exe

      - name: Rust tests (Windows)
        run: cargo test --workspace --exclude llama-helper
```

- [ ] **Step 2: Lint the workflow locally (free)**

```bash
cd /workspace && actionlint .github/workflows/ci.yml && echo ok
```

Expected: `ok`.

- [ ] **Step 3: Commit, push and open a draft PR**

This is the first of only three push points in Phase 0.

```bash
cd /workspace
git add .github/workflows/ci.yml
git commit -q -m "ci: add minute-saving CI (Linux checks per PR, full Windows tests on demand)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push -u origin phase-0-baseline
gh pr create --draft --base main --head phase-0-baseline --title "Phase 0: Conclave baseline" --body "Implements docs/superpowers/plans/2026-10-10-conclave-phase-0-baseline.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

- [ ] **Step 4: Run the Windows baseline once (manual, full) and watch it**

```bash
cd /workspace
gh workflow run ci.yml --ref phase-0-baseline -f full=true
sleep 15
RID=$(gh run list --workflow CI --branch phase-0-baseline --event workflow_dispatch --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch --exit-status "$RID"
gh run view "$RID" --json jobs -q '.jobs[] | "\(.name): \(.conclusion) \(.startedAt) → \(.completedAt)"'
```

Expected: both jobs succeed. Record the `windows-app` duration; it's the cold baseline for minutes. This full run is the **only** Windows app test run until Task 10, and Task 10 runs it only if the reviewer flags a Windows-specific risk.

If a step fails only on Windows (it passed in Task 2 on Linux), use the systematic-debugging skill. Fix only CI configuration here. Product-code failures go to the user, as in Task 2 Step 5. Every re-run costs minutes, so read the full log (`gh run view "$RID" --log-failed`) before changing anything.

- [ ] **Step 5: Review**

Get a `reviewer` review of the commit, plus the run link.

---

### Task 4: Remove telemetry

**Files:**
- Create: `scripts/check-no-phone-home.sh`
- Modify (`.github/workflows/ci.yml`): add the guard step
- Delete: `frontend/src-tauri/src/analytics/` (whole folder), `frontend/src/lib/analytics.ts`, `frontend/src/components/AnalyticsProvider.tsx`, `frontend/src/components/AnalyticsConsentSwitch.tsx`, `frontend/src/components/AnalyticsDataModal.tsx`
- Modify (Rust):
  - `frontend/src-tauri/Cargo.toml`: remove `posthog-rs = "0.3.7"`.
  - `frontend/src-tauri/src/lib.rs`: remove `pub mod analytics;` at line 38, the comment at line 337, and the 25 `analytics::commands::*` entries at lines 619–643 of `generate_handler!`.
  - `frontend/src-tauri/src/audio/recording_commands.rs`: delete the whole "Step 3.5: Track meeting ended analytics" block, from the `// Step 3.5` comment (about line 902) through the end of the `if let Some((total_duration, …)) = analytics_data { … }` block (about line 1003). Then delete any `use` lines left unused.
- Modify (frontend): every file that imports `@/lib/analytics`, `AnalyticsProvider`, `AnalyticsConsentSwitch` or `AnalyticsDataModal`:
  - `src/app/layout.tsx`, `src/app/page.tsx`, `src/app/meeting-details/page.tsx`, `src/app/meeting-details/page-content.tsx`
  - `src/lib/recordingNotification.tsx`, `src/types/betaFeatures.ts`, `src/contexts/ConfigContext.tsx`, `src/contexts/RecordingPostProcessingProvider.tsx`
  - `src/hooks/useRecordingStart.ts`, `src/hooks/useRecordingStop.ts`, `src/hooks/useImportAudio.ts`
  - `src/hooks/meeting-details/useSummaryGeneration.ts`, `src/hooks/meeting-details/useModelConfiguration.ts`, `src/hooks/meeting-details/useCopyOperations.ts`, `src/hooks/meeting-details/useTemplates.ts`
  - `src/components/PreferenceSettings.tsx`, `src/components/About.tsx`, `src/components/Sidebar/index.tsx`, `src/components/Sidebar/SidebarProvider.tsx`, `src/components/DeviceSelection.tsx`, `src/components/LanguageSelection.tsx`, `src/components/RecordingSettings.tsx`, `src/components/RecordingControls.tsx`
  - `src/components/MeetingDetails/SummaryGeneratorButtonGroup.tsx`, `src/components/MeetingDetails/TranscriptButtonGroup.tsx`, `src/components/MeetingDetails/RetranscribeDialog.tsx`, `src/components/MeetingDetails/SummaryUpdaterButtonGroup.tsx`, `src/components/MeetingDetails/SummaryPanel.tsx`
  - The tests: `frontend/tests/hooks/*.test.tsx`, which mock `../../src/lib/analytics`.

**Interfaces:**
- Produces: `scripts/check-no-phone-home.sh`, which exits 0 when clean and 1 with the offending `file:line` otherwise. Tasks 5–7 add patterns to its `PATTERNS` array.

- [ ] **Step 1: Write the guard (the failing test)**

```bash
#!/usr/bin/env bash
# Fails if telemetry, the updater, Meetily-owned hosts or Meetily data paths reappear in product code.
set -uo pipefail
cd "$(dirname "$0")/.."

PATTERNS=(
  'posthog'
  'track_[a-z_]+\('
  '@/lib/analytics'
  'src/lib/analytics'
  'Analytics(Provider|ConsentSwitch|DataModal)'
)

SEARCH_PATHS=(frontend/src frontend/src-tauri/src frontend/src-tauri/Cargo.toml frontend/src-tauri/tauri.conf.json frontend/src-tauri/capabilities frontend/package.json frontend/tests)
EXCLUDES=(--exclude='*.backup' --exclude='*_old*.rs' --exclude='core-old.rs' --exclude='lib_old_complex.rs')

fail=0
for p in "${PATTERNS[@]}"; do
  if hits=$(grep -rnIE "${EXCLUDES[@]}" -- "$p" "${SEARCH_PATHS[@]}" 2>/dev/null); then
    echo "FORBIDDEN pattern '$p':"; echo "$hits"; fail=1
  fi
done
[ "$fail" -eq 0 ] && echo "phone-home guard: clean"
exit "$fail"
```

```bash
chmod +x /workspace/scripts/check-no-phone-home.sh
```

- [ ] **Step 2: Run the guard to confirm it fails**

Run: `bash scripts/check-no-phone-home.sh; echo "exit=$?"`
Expected: it lists hits for `posthog`, `track_…(` and `@/lib/analytics`, with `exit=1`.

- [ ] **Step 3: Remove the Rust side**

```bash
cd /workspace/frontend/src-tauri
git rm -rq src/analytics
sed -i '/^posthog-rs = /d' Cargo.toml
```

Edit `src/lib.rs` and `src/audio/recording_commands.rs` as listed under **Files**. Then:

```bash
cd /workspace && cargo check -p meetily 2>&1 | grep -E "^(error|warning: unused)" | head -20
```

Expected: no errors. Remove any newly unused imports it reports.

- [ ] **Step 4: Remove the frontend side**

```bash
cd /workspace/frontend
git rm -q src/lib/analytics.ts src/components/AnalyticsProvider.tsx src/components/AnalyticsConsentSwitch.tsx src/components/AnalyticsDataModal.tsx
```

Then apply these rules in each listed file:
- Delete the `import Analytics …` / `import AnalyticsConsentSwitch …` / `import AnalyticsProvider …` line.
- Delete every statement whose only purpose is analytics: `Analytics.track…(…)`, `await Analytics.…(…)`, `Analytics.identify…`, `Analytics.trackPageView(…)`. A multi-line call is deleted in full, including its argument object.
- If a `try { … } catch` exists only for analytics (for example `useRecordingStop.ts` lines ~347–395, from `// Track meeting completion analytics` through `} catch (analyticsError) { … }`), delete the whole block, including the `Store.load('analytics.json')` use.
- If a variable or helper is computed only to feed analytics (for example the duration or word-count calculations right before a track call), delete it too. The compiler's unused-variable warnings point to them (`tsc --noEmit`; there is no lint — see Task 2 ruling).
- `layout.tsx`: remove the `<AnalyticsProvider>` wrapper and keep its children.
- `PreferenceSettings.tsx` and `About.tsx`: remove the `{/* Analytics Section */}` block and `<AnalyticsConsentSwitch />`.
- `betaFeatures.ts`: remove `BETA_FEATURE_ANALYTICS_MAP` and its doc-comment line. Remove its uses in `ConfigContext.tsx`.
- Tests: remove the `mock.module('../../src/lib/analytics', …)` setup and restore lines, and the `originalAnalytics` capture.

**Do not** change any non-analytics behaviour. If a statement mixes analytics with real logic, keep the real logic.

- [ ] **Step 5: Verify**

```bash
cd /workspace/frontend && pnpm exec tsc --noEmit && bun test && pnpm run build
cd /workspace && cargo test --workspace --exclude llama-helper 2>&1 | tail -5
bash scripts/check-no-phone-home.sh
```

Expected: everything passes, and the guard prints `phone-home guard: clean`.

- [ ] **Step 6: Wire the guard into CI**

In `.github/workflows/ci.yml`, in the **`checks`** job, insert this right after `actions/checkout@v4`:

```yaml
      - name: Guard (no telemetry / updater / Meetily hosts)
        shell: bash
        run: bash scripts/check-no-phone-home.sh
```

- [ ] **Step 7: Commit (local only)**

```bash
cd /workspace && git add -A
git commit -q -m "feat: remove PostHog telemetry entirely

Deletes analytics module, posthog-rs, all track_* calls, consent UI and
analytics provider; adds phone-home guard to CI.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Don't push. Tasks 4–8 are pushed together with Task 9, to save CI minutes.

- [ ] **Step 8: Review**

Get a `reviewer` review with a Codex pass. Point it at Review Focus 4 (no behaviour change outside analytics; the app still works with an old `analytics.json` store present).

---

### Task 5: Remove the updater

**Files:**
- Modify: `scripts/check-no-phone-home.sh` (patterns)
- Modify: `frontend/src-tauri/tauri.conf.json`:
  - delete the `"plugins": { "updater": … }` block (if `plugins` becomes empty, delete the key);
  - set `"createUpdaterArtifacts": false`;
  - remove `"updater:default"` from the capability list.
- Modify: `frontend/src-tauri/Cargo.toml`: remove `tauri-plugin-updater = "2.3.0"`.
- Modify: `frontend/src-tauri/src/lib.rs:469`: remove `.plugin(tauri_plugin_updater::Builder::new().build())`. **Keep** `tauri_plugin_process` (it's used for relaunch and exit elsewhere).
- Modify: `frontend/src-tauri/src/tray.rs`: remove the `"check_updates" =>` match arm (line 50), `fn check_updates_handler` (line 203 onwards), and the `check_updates` menu item (line 388).
- Modify: `frontend/src-tauri/capabilities/*.json`: remove any `updater:*` permission.
- Modify: `frontend/package.json`: remove `"@tauri-apps/plugin-updater"`, then run `pnpm install` to update `pnpm-lock.yaml`.
- Delete: `frontend/src/services/updateService.ts`, `frontend/src/hooks/useUpdateCheck.ts`, `frontend/src/components/UpdateDialog.tsx`, `frontend/src/components/UpdateNotification.tsx`, `frontend/src/components/UpdateCheckProvider.tsx`
- Modify: `frontend/src/app/layout.tsx`: remove the `UpdateCheckProvider` import and wrapper, keeping its children.
- Modify: `frontend/src/components/About.tsx`: remove the update state, `handleCheckForUpdates`, the "Check for Updates" button and `<UpdateDialog …/>`. Keep the version display (`getVersion()`).

**Interfaces:**
- Consumes: `scripts/check-no-phone-home.sh` from Task 4.

- [ ] **Step 1: Extend the guard (the failing test)**

Add these to `PATTERNS`:

```bash
  'plugin-updater'
  'tauri_plugin_updater'
  'updater:default'
  'releases/latest/download/latest\.json'
  'createUpdaterArtifacts": true'
```

Run: `bash scripts/check-no-phone-home.sh; echo "exit=$?"`
Expected: hits in `Cargo.toml`, `lib.rs`, `tauri.conf.json`, `package.json` and `updateService.ts`, with `exit=1`.

- [ ] **Step 2: Apply the removals listed under Files**

```bash
cd /workspace/frontend
git rm -q src/services/updateService.ts src/hooks/useUpdateCheck.ts src/components/UpdateDialog.tsx src/components/UpdateNotification.tsx src/components/UpdateCheckProvider.tsx
sed -i '/^tauri-plugin-updater = /d' src-tauri/Cargo.toml
pnpm remove @tauri-apps/plugin-updater
```

Then make the manual edits to `tauri.conf.json`, `lib.rs`, `tray.rs`, the capabilities, `layout.tsx` and `About.tsx`.

- [ ] **Step 3: Verify**

```bash
cd /workspace/frontend && pnpm exec tsc --noEmit && bun test && pnpm run build
cd /workspace && cargo test --workspace --exclude llama-helper 2>&1 | tail -5
bash scripts/check-no-phone-home.sh
grep -rn "check-updates-from-tray" frontend/src frontend/src-tauri/src; echo "leftover=$?"
```

Expected: everything passes, the guard is clean, and `leftover=1` (no references left).

- [ ] **Step 4: Commit (local only)**

```bash
cd /workspace && git add -A
git commit -q -m "feat: remove auto-updater (never contact Meetily release endpoint)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Don't push. Tasks 4–8 are pushed together with Task 9, to save CI minutes.

- [ ] **Step 5: Review**

Get a `reviewer` review with a Codex pass. Ask it to confirm that no code path can reach `github.com/Zackriya-Solutions/.../latest.json`.

---

### Task 6: Outbound-call audit

**Files:**
- Create: `docs/network-calls.md`
- Modify: `scripts/check-no-phone-home.sh`
- Modify:
  - `frontend/src/components/About.tsx`: remove the `open_external_url` call to `meetily.zackriya.com` and its "contact" button.
  - `frontend/src/components/onboarding/steps/SetupOverviewStep.tsx:101`: remove the GitHub link to Zackriya.
  - `frontend/src/components/BluetoothPlaybackWarning.tsx:84`: remove the broken `github.com/your-org/meetily` link and keep the warning text.

**Interfaces:**
- Consumes: the guard from Tasks 4–5.

- [ ] **Step 1: List every URL in the product code**

```bash
cd /workspace/frontend
grep -rnoE "https?://[a-zA-Z0-9.:/_%?=&#-]+" src src-tauri/src --include=*.ts --include=*.tsx --include=*.rs \
  | grep -vE "\.backup|_old|core-old|lib_old_complex" | sort
```

- [ ] **Step 2: Classify each host and write `docs/network-calls.md`**

Write `docs/network-calls.md` with this table, filling in one row per host found in Step 1:

```markdown
# Outbound network calls (audited 2026-10-10, Phase 0)

| Host | Where (file:line) | Purpose | Decision |
|---|---|---|---|
| huggingface.co | src-tauri/src/whisper_engine/… | Whisper model download | Allowed (model host) |
| meetily.towardsgeneralintelligence.com | src-tauri/src/parakeet_engine/parakeet_engine.rs:127 | Parakeet model download | Allowed (existing model host) |
| api.anthropic.com, api.openai.com, api.groq.com, openrouter.ai | src-tauri/src/{anthropic,openai,groq,openrouter}/ | User-configured LLM summaries | Allowed (user-configured) |
| localhost:11434 / ollama.com | src-tauri/src/ollama/ | Local Ollama / its model library links | Allowed |
| us.i.posthog.com | — | Telemetry | Removed in Task 4 |
| github.com/Zackriya-Solutions/…/latest.json | — | Updater | Removed in Task 5 |
| meetily.zackriya.com | src/components/About.tsx | "Contact" link | Removed |
```

Any host not covered by the Global Constraints' allowed list gets **Removed**, or is reported to the user if removing it would break a feature.

- [ ] **Step 3: Extend the guard (the failing test)**

Add these to `PATTERNS`:

```bash
  'meetily\.zackriya\.com'
  'zackriya\.com'
  'github\.com/your-org'
  'github\.com/Zackriya-Solutions'
```

Run: `bash scripts/check-no-phone-home.sh; echo "exit=$?"`
Expected: hits in `About.tsx`, `SetupOverviewStep.tsx` and `BluetoothPlaybackWarning.tsx`, with `exit=1`.

- [ ] **Step 4: Remove the links listed under Files, then verify**

```bash
cd /workspace/frontend && pnpm exec tsc --noEmit && bun test && pnpm run build
cd /workspace && bash scripts/check-no-phone-home.sh
```

Expected: all pass, and the guard is clean.

- [ ] **Step 5: Commit (local only)**

```bash
cd /workspace && git add -A
git commit -q -m "chore: audit outbound calls, drop Meetily-owned links

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Don't push. Tasks 4–8 are pushed together with Task 9, to save CI minutes.

- [ ] **Step 6: Review**

Get a `reviewer` review with a Codex pass of the audit's completeness. Ask it to independently grep for `reqwest::`, `fetch(`, `Client::new` and `invoke('open_external_url'` call sites that build URLs at runtime.

---

### Task 7: Rename to Conclave (user-visible only)

**Files:**
- Modify: `frontend/src-tauri/tauri.conf.json`: `"productName": "Conclave"`, `"identifier": "com.conclave.app"`, and the main window `"title": "Conclave"`.
- Modify: `frontend/src-tauri/src/tray.rs:26`: `.tooltip("Conclave")`.
- Modify: `frontend/src-tauri/src/notifications/types.rs` (lines 122–192) and `frontend/src-tauri/src/notifications/commands.rs` (lines 335, 385): every `"Meetily"` becomes `"Conclave"`, and `"Meetily Error"` becomes `"Conclave Error"`.
- Modify: `frontend/src-tauri/src/audio/recording_commands.rs:111`: "Restart Meetily" becomes "Restart Conclave".
- Modify: `frontend/src-tauri/src/whisper_engine/whisper_engine.rs:154` and `:1144`, `frontend/src-tauri/src/summary/summary_engine/model_manager.rs:148`, `frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs:294`, `frontend/src-tauri/src/summary/templates/loader.rs:27` (plus its doc comment lines 22–24): `"Meetily"` becomes `"Conclave"` in paths and the user agent.
- Modify: user-visible strings in `frontend/src/**/*.tsx` / `*.ts`. Find them with `grep -rn "Meetily" frontend/src` (about 39 hits), and change the text shown to the user (labels, titles, alt text, toasts, dialog copy). Remove Meetily PRO promotions or upsell copy entirely rather than renaming them. Keep identifiers, type names and storage keys unchanged.
- Modify: `frontend/src/components/About.tsx`: header "Conclave", the version line, and one line "Based on Meetily (MIT) by Zackriya Solutions." This is the only allowed user-visible "Meetily".
- Replace: `README.md` with a short Conclave README (what it is, link to the spec, install/update/uninstall from spec §3.5, and the credit to Meetily with an MIT link).
- Modify: `scripts/check-no-phone-home.sh`: add the data-path guard.

**Interfaces:**
- Consumes: the guard from Tasks 4–6.

- [ ] **Step 1: Extend the guard for Meetily data paths and identity (the failing test)**

Add these to `PATTERNS`:

```bash
  'join\("Meetily"\)'
  'push\("Meetily"\)'
  '"Meetily/"'
  '"identifier": "com\.meetily\.ai"'
  '"productName": "meetily"'
  'Notification::new\("Meetily'
  '"Meetily( Error)?",'
  '= "Meetily";'
```

Run: `bash scripts/check-no-phone-home.sh; echo "exit=$?"`
Expected: hits in the files listed above, with `exit=1`.

- [ ] **Step 2: Apply the rename edits listed under Files**

- [ ] **Step 3: Check the remaining user-visible "Meetily" strings**

```bash
cd /workspace
grep -rn "Meetily" frontend/src --include=*.tsx --include=*.ts
grep -rnE '"[^"]*Meetily[^"]*"' frontend/src-tauri/src --include=*.rs | grep -vE "\.backup|_old|core-old|lib_old_complex|^\S+:\s*//"
```

Expected: the only frontend hit is the About credit line. Any Rust hit must be a log message (`log::`, `info!`, `warn!`, `error!`, `debug!`), not user-facing text. Fix anything else.

- [ ] **Step 4: Verify**

```bash
cd /workspace/frontend && pnpm exec tsc --noEmit && bun test && pnpm run build
cd /workspace && cargo test --workspace --exclude llama-helper 2>&1 | tail -5
bash scripts/check-no-phone-home.sh
```

Expected: all pass, and the guard is clean.

- [ ] **Step 5: Commit (local only)**

```bash
cd /workspace && git add -A
git commit -q -m "feat: rename user-visible app to Conclave (com.conclave.app)

Own data folders, tray, notifications, UI copy, user agent; About credits Meetily (MIT).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Don't push. Tasks 4–8 are pushed together with Task 9, to save CI minutes.

- [ ] **Step 6: Review**

Get a `reviewer` review. Point it at Review Focus 2 (no Meetily data paths remain).

---

### Task 8: Release workflow (unsigned NSIS, version from tag)

**Files:**
- Create: `scripts/set-version.mjs`, `scripts/set-version.test.mjs`, `.github/workflows/release.yml`
- Modify: `frontend/src-tauri/tauri.conf.json`:
  - `"bundle.targets": ["nsis"]`;
  - delete `"bundle.windows.signCommand"` (and `bundle.windows` if it becomes empty);
  - delete `"bundle.macOS"`.
- Delete: `frontend/src-tauri/scripts/sign-windows.ps1`

**Interfaces:**
- Produces: `node scripts/set-version.mjs <X.Y.Z> [rootDir]`, which rewrites `version` in `frontend/src-tauri/tauri.conf.json`, `frontend/package.json` and the `[package]` section of `frontend/src-tauri/Cargo.toml`. It exits 1 on a non-`X.Y.Z` argument.

- [ ] **Step 1: Write the failing test**

`scripts/set-version.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = join(dirname(fileURLToPath(import.meta.url)), 'set-version.mjs');

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'setver-'));
  mkdirSync(join(root, 'frontend/src-tauri'), { recursive: true });
  writeFileSync(join(root, 'frontend/src-tauri/tauri.conf.json'), JSON.stringify({ productName: 'Conclave', version: '0.4.1' }, null, 4) + '\n');
  writeFileSync(join(root, 'frontend/package.json'), JSON.stringify({ name: 'meetily', version: '0.4.1' }, null, 4) + '\n');
  writeFileSync(join(root, 'frontend/src-tauri/Cargo.toml'),
    '[package]\nname = "meetily"\nversion = "0.4.1"\n\n[dependencies]\nfoo = { version = "1.2.3" }\nbar = "0.4.1"\n');
  return root;
}

test('stamps version into all three files', () => {
  const root = fixture();
  execFileSync('node', [script, '1.2.3', root]);
  assert.equal(JSON.parse(readFileSync(join(root, 'frontend/src-tauri/tauri.conf.json'), 'utf8')).version, '1.2.3');
  assert.equal(JSON.parse(readFileSync(join(root, 'frontend/package.json'), 'utf8')).version, '1.2.3');
  const cargo = readFileSync(join(root, 'frontend/src-tauri/Cargo.toml'), 'utf8');
  assert.match(cargo, /^\[package\]\nname = "meetily"\nversion = "1\.2\.3"$/m);
  assert.match(cargo, /foo = \{ version = "1\.2\.3" \}/); // untouched dependency stays as-is
  assert.match(cargo, /bar = "0\.4\.1"/);                  // dependency with same old version untouched
});

test('rejects a non X.Y.Z version and changes nothing', () => {
  const root = fixture();
  const r = spawnSync('node', [script, 'v1.2', root]);
  assert.equal(r.status, 1);
  assert.equal(JSON.parse(readFileSync(join(root, 'frontend/package.json'), 'utf8')).version, '0.4.1');
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd /workspace && node --test scripts/set-version.test.mjs`
Expected: FAIL (`Cannot find module …/set-version.mjs`).

- [ ] **Step 3: Implement `scripts/set-version.mjs`**

```js
// Usage: node scripts/set-version.mjs X.Y.Z [repoRoot]
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [version, root = process.cwd()] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) {
  console.error(`set-version: expected X.Y.Z, got '${version}'`);
  process.exit(1);
}

for (const rel of ['frontend/src-tauri/tauri.conf.json', 'frontend/package.json']) {
  const path = join(root, rel);
  const json = JSON.parse(readFileSync(path, 'utf8'));
  json.version = version;
  writeFileSync(path, JSON.stringify(json, null, 4) + '\n');
}

// Only the version line inside [package], never dependency versions.
const cargoPath = join(root, 'frontend/src-tauri/Cargo.toml');
const cargo = readFileSync(cargoPath, 'utf8');
const updated = cargo.replace(/(\[package\][^\[]*?^version = ")[^"]*(")/m, `$1${version}$2`);
if (updated === cargo && !cargo.includes(`version = "${version}"`)) {
  console.error('set-version: [package] version line not found in Cargo.toml');
  process.exit(1);
}
writeFileSync(cargoPath, updated);
console.log(`set-version: ${version}`);
```

- [ ] **Step 4: Run it to confirm it passes**

Run: `cd /workspace && node --test scripts/set-version.test.mjs`
Expected: 2 tests pass. Also run `node scripts/set-version.mjs 0.1.0 && git diff --stat`. Expect exactly three files changed, with no indentation churn beyond the version line. If `git diff` shows reformatting of `tauri.conf.json` or `package.json` (key order or indentation), match the original indentation in the script. Then `git checkout -- frontend` to discard the trial.

- [ ] **Step 5: Bundle config and signing removal**

Make the `tauri.conf.json` edits listed under **Files**, and `git rm frontend/src-tauri/scripts/sign-windows.ps1`.

- [ ] **Step 6: Write `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags: ['v*']
  workflow_dispatch:

permissions:
  contents: write

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  windows:
    name: Build Windows installer
    runs-on: windows-latest
    timeout-minutes: 150
    steps:
      - uses: actions/checkout@v4

      - name: Stamp version from tag
        if: github.ref_type == 'tag'
        shell: bash
        run: node scripts/set-version.mjs "${GITHUB_REF_NAME#v}"

      - uses: pnpm/action-setup@v4
        with:
          version: 9.15.9
          run_install: false

      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: pnpm
          cache-dependency-path: frontend/pnpm-lock.yaml

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: Configure portable Windows native build
        id: portability
        shell: pwsh
        run: |
          $cmakeHook = (Join-Path $env:GITHUB_WORKSPACE ".github/force-portable-ggml.cmake").Replace('\', '/')
          "CMAKE_PROJECT_INCLUDE=$cmakeHook" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
          "RUSTFLAGS=-C target-cpu=x86-64-v2" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
          $buildDirectory = "target/x86_64-pc-windows-msvc/release"
          $configPath = (Join-Path $env:RUNNER_TEMP "tauri.windows-portable.conf.json").Replace('\', '/')
          @{ build = @{ beforeBundleCommand = @{ script = "node .github/verify-portable-ggml.cjs $buildDirectory"; cwd = $env:GITHUB_WORKSPACE } } } |
            ConvertTo-Json -Depth 4 | Set-Content -Path $configPath -Encoding utf8
          "args=--config `"$configPath`"" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append

      - uses: swatinem/rust-cache@v2
        with:
          workspaces: ". -> target"
          key: release-cpu-v1

      - name: Install frontend dependencies
        working-directory: frontend
        run: pnpm install --frozen-lockfile

      - name: Build llama-helper sidecar (CPU)
        shell: pwsh
        run: |
          cargo build --release -p llama-helper
          if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
          New-Item -ItemType Directory -Force -Path "frontend/src-tauri/binaries" | Out-Null
          Copy-Item "target/release/llama-helper.exe" -Destination "frontend/src-tauri/binaries/llama-helper-x86_64-pc-windows-msvc.exe"

      - name: Cache FFmpeg binary
        uses: actions/cache@v4
        with:
          path: frontend/src-tauri/binaries/ffmpeg-*.exe
          key: ${{ runner.os }}-ffmpeg-${{ hashFiles('frontend/src-tauri/build.rs', 'frontend/src-tauri/build/ffmpeg.rs') }}

      - name: Cache Windows ONNX Runtime
        uses: actions/cache@v4
        with:
          path: frontend/src-tauri/binaries/onnxruntime
          key: ${{ runner.os }}-onnxruntime-x86_64-pc-windows-msvc-${{ hashFiles('frontend/src-tauri/build/onnxruntime.rs') }}

      - name: Build installer (and publish release on tag)
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          projectPath: frontend
          args: --target x86_64-pc-windows-msvc --bundles nsis ${{ steps.portability.outputs.args }}
          tagName: ${{ github.ref_type == 'tag' && github.ref_name || '' }}
          releaseName: ${{ github.ref_type == 'tag' && format('Conclave {0}', github.ref_name) || '' }}
          releaseBody: 'Unsigned build: on first install choose "More info → Run anyway". Update by running the new setup over the old one.'
          releaseDraft: false
          includeUpdaterJson: false

      - name: Upload installer artifact
        uses: actions/upload-artifact@v4
        with:
          name: conclave-windows-x64-setup
          path: target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe
          if-no-files-found: error
          retention-days: 14
```

- [ ] **Step 7: Lint the workflow locally and commit (no CI run)**

```bash
cd /workspace && actionlint .github/workflows/release.yml && node --test scripts/set-version.test.mjs
git add -A
git commit -q -m "ci: add unsigned NSIS release workflow, version stamped from tag

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Don't push, and don't make a trial run. A release build is the most expensive job (about 45+ minutes ×2), so the only build is the `v0.1.0` tag in Task 10. If that run fails, Task 10 explains how to fix it and re-tag.

- [ ] **Step 8: Review**

Get a `reviewer` review. Point it at Review Focus 3 (the version stamp).

---

### Task 9: `meeting-detector` crate and `conclave-probe`

**Files:**
- Modify: `Cargo.toml` (root): add `"crates/meeting-detector"` to `members`.
- Create: `crates/meeting-detector/Cargo.toml`, `crates/meeting-detector/src/lib.rs`, `crates/meeting-detector/src/consent.rs`, `crates/meeting-detector/src/windows.rs`, `crates/meeting-detector/src/bin/conclave-probe.rs`, `docs/probe.md`
- Modify (`.github/workflows/ci.yml`): build and upload the probe.

**Interfaces:**
- Produces (used by the Phase 2 plan):
  - `meeting_detector::consent::{MicUser { app: String, packaged: bool, in_use: bool }, app_name_from_key(key: &str, packaged: bool) -> String, is_in_use(start: u64, stop: u64) -> bool, is_browser(app: &str) -> bool, CONSENT_PATH: &str}`
  - `meeting_detector::windows::{read_mic_users(path: &str) -> io::Result<Vec<MicUser>>, ChangeWatcher::open(path: &str) -> io::Result<ChangeWatcher>, ChangeWatcher::wait(&self, timeout_ms: u32) -> io::Result<Wake>, enum Wake { Changed, Timeout }, window_titles_for(exe: &str) -> Vec<String>, capture_sessions() -> windows::core::Result<Vec<CaptureSession>>, CaptureSession { device_id: String, device_name: String, process_id: u32, process_name: Option<String>, active: bool }, init_com()}` (Windows only)

- [ ] **Step 1: Create the crate skeleton**

`crates/meeting-detector/Cargo.toml`:

```toml
[package]
name = "meeting-detector"
version = "0.1.0"
edition = "2021"
publish = false

[[bin]]
name = "conclave-probe"
path = "src/bin/conclave-probe.rs"

[target.'cfg(windows)'.dependencies]
winreg = "0.55"
windows = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_System_Threading",
    "Win32_System_Registry",
    "Win32_System_Com",
    "Win32_System_Com_StructuredStorage",
    "Win32_System_Variant",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Shell_PropertiesSystem",
    "Win32_Media_Audio",
    "Win32_Devices_FunctionDiscovery",
] }
```

`crates/meeting-detector/src/lib.rs`:

```rust
//! Detects online meetings from Windows' microphone-usage record (spec §5.3).
pub mod consent;
#[cfg(windows)]
pub mod windows;
```

Add `"crates/meeting-detector"` to `members` in the root `Cargo.toml`.

- [ ] **Step 2: Write the failing tests for the pure consent logic**

`crates/meeting-detector/src/consent.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonpackaged_key_becomes_lowercase_exe_name() {
        assert_eq!(app_name_from_key(r"C:#Program Files#Zoom#bin#Zoom.exe", false), "zoom.exe");
        assert_eq!(app_name_from_key(r"C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe", false), "zoom.exe");
    }

    #[test]
    fn packaged_key_is_lowercased_family_name() {
        assert_eq!(app_name_from_key("MSTeams_8wekyb3d8bbwe", true), "msteams_8wekyb3d8bbwe");
    }

    #[test]
    fn in_use_rules() {
        assert!(!is_in_use(0, 0), "never used");
        assert!(is_in_use(100, 0), "started, not stopped");
        assert!(is_in_use(200, 100), "restarted after last stop");
        assert!(!is_in_use(100, 200), "stopped after start");
        assert!(!is_in_use(100, 100), "equal stamps = stopped");
    }

    #[test]
    fn browsers_are_recognised() {
        for b in ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe"] {
            assert!(is_browser(b), "{b}");
        }
        assert!(!is_browser("zoom.exe"));
    }
}
```

Run: `cd /workspace && cargo test -p meeting-detector`
Expected: FAIL to compile (`cannot find function app_name_from_key`).

- [ ] **Step 3: Implement the pure consent logic (above the tests in the same file)**

```rust
//! Pure helpers for Windows' microphone consent store. No Win32 calls, so they test on any OS.

/// HKCU-relative path of the microphone consent store.
pub const CONSENT_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

const BROWSERS: [&str; 4] = ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe"];

/// One app that has used the microphone, as recorded by Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicUser {
    /// Lowercase exe file name (desktop apps) or package family name (packaged apps).
    pub app: String,
    pub packaged: bool,
    pub in_use: bool,
}

/// Desktop-app keys are full exe paths with `\` replaced by `#`; packaged-app keys are package family names.
pub fn app_name_from_key(key: &str, packaged: bool) -> String {
    let name = if packaged { key } else { key.rsplit('#').next().unwrap_or(key) };
    name.to_lowercase()
}

/// FILETIME stamps: in use while the last start is newer than the last stop (stop == 0 means never stopped).
pub fn is_in_use(start: u64, stop: u64) -> bool {
    start != 0 && (stop == 0 || start > stop)
}

pub fn is_browser(app: &str) -> bool {
    BROWSERS.contains(&app)
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd /workspace && cargo test -p meeting-detector`
Expected: 4 tests pass (on Linux; the `windows` module isn't compiled there).

- [ ] **Step 5: Write the Windows layer with its Windows-only tests**

`crates/meeting-detector/src/windows.rs`:

```rust
//! Thin Win32 layer: consent-store reads, change notifications, window titles, capture sessions.
use crate::consent::{app_name_from_key, is_in_use, MicUser};
use std::io;
use windows::core::{Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{
    eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Registry::{RegNotifyChangeKeyValue, HKEY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME};
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

fn entry(key: &RegKey, name: &str, packaged: bool) -> Option<MicUser> {
    let sub = key.open_subkey(name).ok()?;
    let start: u64 = sub.get_value("LastUsedTimeStart").unwrap_or(0);
    let stop: u64 = sub.get_value("LastUsedTimeStop").unwrap_or(0);
    Some(MicUser { app: app_name_from_key(name, packaged), packaged, in_use: is_in_use(start, stop) })
}

/// Reads every app recorded under `HKCU\<path>` (packaged subkeys + `NonPackaged\*`).
pub fn read_mic_users(path: &str) -> io::Result<Vec<MicUser>> {
    let root = RegKey::predef(HKEY_CURRENT_USER).open_subkey(path)?;
    let mut users = Vec::new();
    for name in root.enum_keys().filter_map(Result::ok) {
        if name.eq_ignore_ascii_case("NonPackaged") {
            let np = root.open_subkey(&name)?;
            users.extend(np.enum_keys().filter_map(Result::ok).filter_map(|n| entry(&np, &n, false)));
        } else if let Some(u) = entry(&root, &name, true) {
            users.push(u);
        }
    }
    Ok(users)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    Changed,
    Timeout,
}

/// Wakes when anything under the key changes (values or subkeys), or after a timeout.
pub struct ChangeWatcher {
    key: RegKey,
    event: HANDLE,
}

// The registry key and event handle are only used from the thread that calls `wait`.
unsafe impl Send for ChangeWatcher {}

impl ChangeWatcher {
    pub fn open(path: &str) -> io::Result<Self> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(path)?;
        let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.map_err(io::Error::other)?;
        Ok(Self { key, event })
    }

    /// Arms a one-shot notification, then waits. Re-arms on every call.
    pub fn wait(&self, timeout_ms: u32) -> io::Result<Wake> {
        let hkey = HKEY(self.key.raw_handle() as _);
        let err = unsafe {
            RegNotifyChangeKeyValue(hkey, true, REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME, Some(self.event), true)
        };
        if err.is_err() {
            return Err(io::Error::from_raw_os_error(err.0 as i32));
        }
        match unsafe { WaitForSingleObject(self.event, timeout_ms) } {
            WAIT_OBJECT_0 => Ok(Wake::Changed),
            WAIT_TIMEOUT => Ok(Wake::Timeout),
            other => Err(io::Error::other(format!("WaitForSingleObject returned {:?}", other))),
        }
    }
}

impl Drop for ChangeWatcher {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.event) };
    }
}

/// Lowercase exe file name of a process, or None if it can't be opened (e.g. elevated/system process).
pub fn process_exe_name(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(path.rsplit('\\').next().unwrap_or(&path).to_lowercase())
    }
}

/// Titles of visible top-level windows whose process exe name equals `exe` (lowercase).
pub fn window_titles_for(exe: &str) -> Vec<String> {
    struct Ctx<'a> { exe: &'a str, titles: Vec<String> }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        if IsWindowVisible(hwnd).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if process_exe_name(pid).as_deref() == Some(ctx.exe) {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(hwnd, &mut buf);
                if n > 0 {
                    ctx.titles.push(String::from_utf16_lossy(&buf[..n as usize]));
                }
            }
        }
        true.into()
    }
    let mut ctx = Ctx { exe, titles: Vec::new() };
    let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut ctx as *mut Ctx as isize)) };
    ctx.titles
}

/// Must be called once on each thread that uses `capture_sessions`.
pub fn init_com() {
    // RPC_E_CHANGED_MODE (already initialised differently) is fine for our use.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSession {
    pub device_id: String,
    pub device_name: String,
    pub process_id: u32,
    pub process_name: Option<String>,
    pub active: bool,
}

unsafe fn take_pwstr(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as _));
    s
}

/// Audio sessions on every active capture device. Sessions without a single owning process are skipped.
pub fn capture_sessions() -> windows::core::Result<Vec<CaptureSession>> {
    let mut out = Vec::new();
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            let Ok(device) = devices.Item(i) else { continue };
            let device_id = device.GetId().map(|p| take_pwstr(p)).unwrap_or_default();
            let device_name = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                .and_then(|pv| PropVariantToStringAlloc(&pv))
                .map(|p| take_pwstr(p))
                .unwrap_or_else(|_| "(unknown device)".to_string());
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
            for j in 0..sessions.GetCount().unwrap_or(0) {
                let Ok(control) = sessions.GetSession(j) else { continue };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                // Errors here mean a multi-process or system session: skip, never fail the scan.
                let Ok(pid) = control2.GetProcessId() else { continue };
                if pid == 0 {
                    continue;
                }
                let active = control2.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false);
                out.push(CaptureSession {
                    device_id: device_id.clone(),
                    device_name: device_name.clone(),
                    process_id: pid,
                    process_name: process_exe_name(pid),
                    active,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const TEST_ROOT: &str = r"Software\ConclaveTest";

    fn test_store(name: &str) -> (RegKey, String) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(r"{TEST_ROOT}\{name}\microphone");
        let _ = hkcu.delete_subkey_all(format!(r"{TEST_ROOT}\{name}"));
        let (key, _) = hkcu.create_subkey(&path).unwrap();
        (key, path)
    }

    fn set(key: &RegKey, sub: &str, start: u64, stop: u64) {
        let (k, _) = key.create_subkey(sub).unwrap();
        k.set_value("LastUsedTimeStart", &start).unwrap();
        k.set_value("LastUsedTimeStop", &stop).unwrap();
    }

    #[test]
    fn reads_packaged_and_nonpackaged_entries() {
        let (key, path) = test_store("read");
        set(&key, "MSTeams_8wekyb3d8bbwe", 200, 0);
        set(&key, r"NonPackaged\C:#Program Files#Zoom#bin#Zoom.exe", 100, 200);
        set(&key, r"NonPackaged\C:#Program Files#Google#Chrome#Application#chrome.exe", 300, 100);

        let mut users = read_mic_users(&path).unwrap();
        users.sort_by(|a, b| a.app.cmp(&b.app));
        assert_eq!(users, vec![
            MicUser { app: "chrome.exe".into(), packaged: false, in_use: true },
            MicUser { app: "msteams_8wekyb3d8bbwe".into(), packaged: true, in_use: true },
            MicUser { app: "zoom.exe".into(), packaged: false, in_use: false },
        ]);
        RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(format!(r"{TEST_ROOT}\read")).unwrap();
    }

    #[test]
    fn watcher_wakes_on_change_and_times_out_otherwise() {
        let (key, path) = test_store("watch");
        set(&key, r"NonPackaged\C:#Zoom.exe", 1, 0);
        let watcher = ChangeWatcher::open(&path).unwrap();

        assert_eq!(watcher.wait(300).unwrap(), Wake::Timeout);

        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let k = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(format!(r"{path}\NonPackaged\C:#Zoom.exe"), winreg::enums::KEY_WRITE).unwrap();
            k.set_value("LastUsedTimeStop", &5u64).unwrap();
        });
        assert_eq!(watcher.wait(5000).unwrap(), Wake::Changed);
        writer.join().unwrap();
        drop(watcher);
        RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(format!(r"{TEST_ROOT}\watch")).unwrap();
    }

    #[test]
    fn capture_sessions_never_errors_on_runner() {
        init_com();
        // CI runners usually have no microphone: an empty list is the expected success.
        let sessions = capture_sessions().expect("enumeration must succeed even with zero devices");
        for s in sessions {
            assert_ne!(s.process_id, 0);
        }
    }

    #[test]
    fn window_titles_for_unknown_exe_is_empty() {
        assert!(window_titles_for("definitely-not-running-conclave-test.exe").is_empty());
    }
}
```

- [ ] **Step 6: Cross-check that it compiles for Windows from Linux**

Run: `cd /workspace && cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests`
Expected: no errors. If a `windows` 0.61 signature differs from the code above, fix it against `~/.cargo/registry/src/*/windows-0.61.*/src/Windows/Win32/...`, keeping the same behaviour.

- [ ] **Step 7: Write the probe binary**

`crates/meeting-detector/src/bin/conclave-probe.rs`:

```rust
//! Pre-flight diagnostic for spec §5.4. Read-only: prints mic usage, wake reasons and capture sessions.
#[cfg(not(windows))]
fn main() {
    eprintln!("conclave-probe only runs on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    use meeting_detector::consent::{is_browser, CONSENT_PATH};
    use meeting_detector::windows::*;
    use std::collections::{BTreeSet, HashMap};
    use std::time::Instant;

    let t0 = Instant::now();
    let log = |msg: String| println!("[{:>8.3}s] {msg}", t0.elapsed().as_secs_f64());

    init_com();
    let watcher = match ChangeWatcher::open(CONSENT_PATH) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Cannot open the microphone consent store (HKCU\\{CONSENT_PATH}): {e}");
            std::process::exit(2);
        }
    };
    println!("conclave-probe {} — read-only. Join/mute/leave calls and switch mics; press Ctrl+C to quit.", env!("CARGO_PKG_VERSION"));
    println!("Legend: via=notify means Windows sent a change notification; via=timeout means it was only caught by the 5 s re-read.\n");

    let mut prev: HashMap<String, bool> = HashMap::new();
    let mut prev_sessions: BTreeSet<String> = BTreeSet::new();
    let mut first = true;
    loop {
        let via = match watcher.wait(5000) {
            Ok(Wake::Changed) => "notify",
            Ok(Wake::Timeout) => "timeout",
            Err(e) => {
                log(format!("ERROR waiting for change: {e}"));
                std::thread::sleep(std::time::Duration::from_secs(5));
                "error-retry"
            }
        };
        match read_mic_users(CONSENT_PATH) {
            Ok(users) => {
                for u in users {
                    let was = prev.insert(u.app.clone(), u.in_use).unwrap_or(false);
                    if u.in_use != was && !(first && !u.in_use) {
                        let kind = if u.packaged { "packaged" } else { "desktop" };
                        log(format!("MIC {} {} ({kind}) via={via}", if u.in_use { "START" } else { "STOP " }, u.app));
                        if u.in_use && is_browser(&u.app) {
                            for t in window_titles_for(&u.app) {
                                log(format!("    window title: {t}"));
                            }
                        }
                    }
                }
            }
            Err(e) => log(format!("ERROR reading consent store: {e}")),
        }
        match capture_sessions() {
            Ok(sessions) => {
                let now: BTreeSet<String> = sessions
                    .iter()
                    .filter(|s| s.active)
                    .map(|s| format!("{} [{}] ← {} (pid {})", s.device_name, s.device_id, s.process_name.as_deref().unwrap_or("?"), s.process_id))
                    .collect();
                if now != prev_sessions {
                    log("ACTIVE CAPTURE SESSIONS:".to_string());
                    if now.is_empty() {
                        log("    (none)".to_string());
                    }
                    for s in &now {
                        log(format!("    {s}"));
                    }
                    prev_sessions = now;
                }
            }
            Err(e) => log(format!("ERROR listing capture sessions: {e}")),
        }
        first = false;
    }
}
```

Run: `cd /workspace && cargo check -p meeting-detector --target x86_64-pc-windows-msvc --bins && cargo build -p meeting-detector`
Expected: no errors. The Linux build compiles the stub `main`.

- [ ] **Step 8: Add the `detector` job to CI (small, Windows, every PR push)**

Append this job to `.github/workflows/ci.yml` under `jobs:`. It builds only this small crate (no whisper.cpp, no Tauri), so a warm run takes a few minutes:

```yaml
  detector:
    name: meeting-detector (Windows)
    runs-on: windows-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: swatinem/rust-cache@v2
        with:
          workspaces: ". -> target"
          key: ci-detector-v1
          cache-on-failure: true

      - name: Test meeting-detector (incl. Windows-only registry/COM tests)
        run: cargo test -p meeting-detector

      - name: Build conclave-probe
        run: cargo build --release -p meeting-detector --bin conclave-probe

      - name: Upload conclave-probe
        uses: actions/upload-artifact@v4
        with:
          name: conclave-probe
          path: target/release/conclave-probe.exe
          if-no-files-found: error
          retention-days: 30
```

Run: `cd /workspace && actionlint .github/workflows/ci.yml && echo ok`
Expected: `ok`.

- [ ] **Step 9: Write `docs/probe.md` (user instructions)**

```markdown
# Running conclave-probe (Phase 0 checkpoint, spec §5.4)

1. Download `conclave-probe` from the latest green **CI** run on GitHub → Actions → CI → run → Artifacts. Unzip it.
2. Open **Windows Terminal / PowerShell**, `cd` to the folder, run `.\conclave-probe.exe`. Leave it running. It is read-only and offline.
3. Do these, waiting ~10 s between steps, and keep the window's output:
   - **A (mute/leave):** join a Zoom test meeting (zoom.us/test). Mute for 30 s. Unmute. Leave. Repeat in Google Meet (meet.new) and Microsoft Teams (browser or app).
   - **B (notifications):** note whether START/STOP lines say `via=notify` (good) or `via=timeout`.
   - **C (which mic):** during a Zoom call, switch Zoom's microphone (Settings → Audio) to another mic, then back. Watch "ACTIVE CAPTURE SESSIONS" change devices.
   - Also join a Meet call with the Meet tab **not** active, to see what window titles are printed.
4. **D (notification click):** in the installed Conclave app, open Settings → Notifications → send a test notification and click it. Note whether Conclave's window comes to the front.
5. Ctrl+C to stop. Paste the whole output and the D result back to Claude.
```

- [ ] **Step 10: Commit and push (second push point: Tasks 4–9 together), then check CI**

Before pushing, run the full local suite once (free), so CI only confirms it:

```bash
cd /workspace/frontend && pnpm exec tsc --noEmit && bun test && pnpm run build
cd /workspace && cargo test --workspace --exclude llama-helper 2>&1 | tail -5
cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins
bash scripts/check-no-phone-home.sh && actionlint && node --test scripts/set-version.test.mjs
```

```bash
cd /workspace && git add -A
git commit -q -m "feat(detector): add meeting-detector crate and conclave-probe diagnostic

Consent-store reader, change watcher, window titles, capture sessions; probe for spec §5.4.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git push
sleep 15
RID=$(gh run list --branch phase-0-baseline --workflow CI --event pull_request --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch --exit-status "$RID"
gh run view "$RID" --log | grep -E "test (consent|windows)::tests::.* \.\.\. ok" | head -20
gh run view "$RID" --json jobs -q '.jobs[] | select(.name=="meeting-detector (Windows)") | .steps[] | select(.name=="Upload conclave-probe") | .conclusion'
```

Expected: the run is green (`checks` and `detector`; `windows-app` is skipped); all 8 meeting-detector tests show `ok` (4 consent tests plus 4 Windows tests); the upload step concludes `success`.

- [ ] **Step 11: Review**

Get a `reviewer` review with a Codex pass, because the code reads untrusted OS data. Point it at Review Focus 5 (no panics on odd sessions or devices; `unsafe` blocks are sound; handles get closed).

---

### Task 10: Phase review, merge, first release and checkpoint hand-off

**Files:** none new. This task covers review fixes only.

- [ ] **Step 1: Whole-branch review**

Get a `reviewer` review with a Codex pass of `main..phase-0-baseline`, against spec §3 and §5.4 and this plan's Global Constraints and Review Focus. Ask the reviewer whether any change since the Task 3 baseline is Windows-specific enough to justify one more full Windows run (`gh workflow run ci.yml --ref phase-0-baseline -f full=true`). Run it only if the answer is yes. Fix the confirmed findings, verifying locally, and push them as one commit. Then get one recheck review. After two fix/review cycles, bring any unresolved findings to the user.

- [ ] **Step 2: Ask the user before merging and tagging**

Present the final diff summary, the CI run links, test results, minutes used so far (`gh api /repos/Sashanator/conclave/actions/runs --jq '[.workflow_runs[] | .run_duration_ms] | add / 60000'` gives the wall minutes; Windows bills double), and any unresolved findings. Ask: "Merge PR and tag `v0.1.0`?" Wait for an explicit yes.

- [ ] **Step 3: Merge and tag (third push point; the only installer build)**

```bash
cd /workspace
gh pr ready && gh pr merge --merge --delete-branch
git checkout main && git pull --ff-only
git tag -a v0.1.0 -m "Conclave 0.1.0 — baseline" && git push origin v0.1.0
sleep 15; RID=$(gh run list --workflow Release --limit 1 --json databaseId -q '.[0].databaseId'); gh run watch --exit-status "$RID"
gh release view v0.1.0 --json assets -q '.assets[].name'
```

Expected: one asset, `Conclave_0.1.0_x64-setup.exe`.

If the release run fails:
1. Read `gh run view "$RID" --log-failed` in full.
2. Fix on a branch `fix/release-0.1.0`, verify locally (`actionlint`, the local suite), and open a PR. Its `checks` run is cheap. Get a `reviewer` review and merge.
3. Delete the failed release, if one was created (`gh release delete v0.1.0 --yes`), and move the tag: `git checkout main && git pull --ff-only && git tag -fa v0.1.0 -m "Conclave 0.1.0 — baseline" && git push -f origin v0.1.0`.
4. Moving a tag is acceptable only because nothing has been published or installed yet. Tell the user it happened.

- [ ] **Step 4: Hand off the checkpoint checklist to the user**

Give the user:
1. Download `Conclave_0.1.0_x64-setup.exe` from Releases, then install it (SmartScreen → "More info → Run anyway").
2. In Settings, pick the Whisper `large-v3-turbo` model and your LLM provider and key.
3. Record a real Zoom call, then check that the live transcript appears and that the summary generates after stopping.
4. Check that Settings and About open without errors, that there's no analytics or "check for updates" anywhere, and that the app is named Conclave in the window, tray and notifications.
5. Run `conclave-probe` following `docs/probe.md`, then paste the output and the D result.

The Phase 1 and Phase 2 plans get written after the user reports back.
