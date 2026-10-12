# Conclave v1 — Design

Date: 2026-10-10
Status: Draft for review

## 1. Purpose

Conclave replaces Notion AI Meeting Notes for one user on Windows 10. It records online calls, transcribes them locally, and summarizes them with a cloud LLM.

Priorities, in order: **cost**, then **control and features**, then **privacy**.

Conclave is an **independent fork** of Meetily ([Zackriya-Solutions/meeting-minutes](https://github.com/Zackriya-Solutions/meeting-minutes), MIT, v0.4.1, Tauri 2 + Rust + Next.js). Meetily already provides mic + system-audio capture on Windows, live local transcription (Whisper / Parakeet), cloud summaries (Claude, Groq, OpenRouter, OpenAI-compatible), import, retranscription, search and pause/resume. Conclave keeps all of that as-is and adds three things:

1. A Conclave-owned baseline: rename, no telemetry, no upstream updater, Windows CI on a private repo.
2. Whisper on the NVIDIA GPU via CUDA.
3. Automatic detection of Zoom, Google Meet and Microsoft Teams calls, with automatic start/stop and microphone following.

### Context

- **Machine:** Windows 10 x64, NVIDIA RTX 4070 Ti SUPER (16 GB, Ada, sm_89), several microphones.
- **Calls:** Zoom desktop app; Google Meet and Microsoft Teams in a browser; possibly the new Teams desktop app.
- **Languages:** Russian, English, and German/Russian mixed (language lessons). Whisper `large-v3` / `large-v3-turbo` with automatic language detection is the default model.
- **Development:** in a Linux container; builds and tests on GitHub-hosted Windows runners. The user's GitHub Pro (Student Pack) gives about 3,000 Actions minutes per month on private repos, and Windows minutes count double.
- **Repo:** `github.com/Sashanator/conclave`, **private**, currently empty.
- **Launch:** the user starts Conclave manually after booting. There is no autostart. Closing the window hides it to the tray (existing Meetily behaviour), and detection keeps running there.

## 2. Success criteria

v1 is done when, on the user's PC:

1. A Conclave installer, built by CI on a tag, installs and runs. The app is named Conclave everywhere the user sees it.
2. A Zoom call is recorded, transcribed live, and summarized with the user's cloud LLM key.
3. The app sends nothing to PostHog or to Meetily's servers.
4. The settings show "Whisper running on: CUDA (NVIDIA GeForce RTX 4070 Ti SUPER)", and the Task Manager "Cuda" graph is active during transcription.
5. With auto-detect on, joining a Zoom, Meet or Teams call prompts to record (Ask) or starts recording (Auto-record). Leaving the call stops the recording within about 6 seconds.
6. If the call app switches microphone mid-call, Conclave switches to the same microphone within about 5 seconds.

## 3. Phase 0: Baseline fork

### 3.1 Repository

- Import Meetily's full git history (from `main` at v0.4.1) into `Sashanator/conclave`, using a plain clone and push. It is **not** a GitHub fork: forks of public repos cannot be private, and their PRs default to targeting upstream.
- Add Meetily as a read-only remote named `upstream`, used only for reading or cherry-picking individual fixes. There will be **no regular upstream merges**, and the code may change freely.
- Keep Meetily's `LICENSE.md` copyright notice (an MIT requirement) and add a Conclave copyright line.
- The existing workspace files (`CLAUDE.md`, `.devcontainer/`, `.claude/` settings and agents, `.gitignore` entries, `.vscode/`) are merged into the repo root.
- This spec is committed as part of the first Conclave commit.

### 3.2 Rename (user-visible only)

- `tauri.conf.json`: `productName` → `Conclave`, `identifier` → `com.conclave.app`. Also the main window title and the installer and bundle names.
- The tray menu, notifications, and about 39 UI strings in `frontend/src` that say "Meetily".
- The user agent `Meetily/<ver>` in `whisper_engine.rs` becomes `Conclave/<ver>`.
- Internal names (crate names, module paths, folder names) stay as they are. They are renamed only when code is touched for another reason.
- Consequence: the new identifier gives Conclave its own data folder. It does not see existing Meetily data, and the two apps can be installed side by side.

### 3.3 Telemetry removal

Analytics is **removed entirely, not stubbed and not kept locally**. Meetily tracks only product-usage counters (`track_app_started`, `track_recording_started`, `track_model_changed`, `track_daily_active_user`, …), which are worthless for a single user. The meetings themselves are already in the local database, and diagnostics are already in the app logs.

- Delete `src-tauri/src/analytics/`, the `posthog-rs` dependency and the API key, and every `track_*` call site and analytics Tauri command (about 40 files, Rust and frontend).
- Frontend: remove the analytics consent step from onboarding, the analytics settings and data modal (`AnalyticsDataModal.tsx`), and their entry points.
- Stored analytics preferences in the settings or database are ignored. No migration is needed, because Conclave starts with its own data folder (§3.2).
- **Outbound-call audit:** list every network call in Rust and the frontend. Allowed: model downloads (Hugging Face and the existing model hosts), the user-configured LLM provider, and Ollama on localhost if the user configures it. Anything else is removed or reported to the user.

### 3.4 Updater disabled

- Remove the `updater` block (Meetily's endpoint and public key) from `tauri.conf.json`, remove the updater plugin (`tauri-plugin-updater`) and its capability, and remove the frontend update check and "check for updates" UI (`services/updateService.ts`, `components/UpdateDialog.tsx`, `@tauri-apps/plugin-updater`).
- Result: the app never contacts Meetily's release endpoint. New versions are installed manually from the private repo's Releases page.

### 3.5 CI (Windows only)

Remove: the macOS, Linux and devtest workflows; the Windows code-signing steps (`SM_*` secrets, DigiCert); and the unused `SUPABASE_*` and `MEETILY_RSA_PUBLIC_KEY` secrets. The installer is **unsigned**, so Windows SmartScreen shows "Unknown publisher" once per install, and the user clicks "More info → Run anyway".

Two workflows:

- **`ci.yml`** is built to minimise Actions minutes (the user's explicit priority):
  - On every PR push, the frontend checks and the phone-home guard run on `ubuntu-latest` (1× billing), and the small `meeting-detector` crate is tested on `windows-latest`.
  - The full Windows app tests (a CPU build plus `cargo test`) run only on manual dispatch.
  - Doc-only changes skip CI, superseded runs are cancelled, and Cargo and pnpm are cached.
  - Most verification happens for free in the devcontainer before pushing.
- **`release.yml`** runs on a `v*` tag or a manual dispatch. It builds **only the NSIS installer** (`Conclave_<version>_x64-setup.exe`) and publishes a GitHub Release on the private repo. In Phase 0 it is a CPU build. Phase 1 switches it to CUDA.
  - **No `.msi`.** Installing the MSI and the NSIS setup on the same machine creates two separate "Conclave" entries in Apps & Features. One format avoids that and saves build minutes.
  - **The version comes from the tag.** The workflow writes the version from `vX.Y.Z` into `tauri.conf.json` and `Cargo.toml` before building, so every installer carries a higher version than the last one and upgrades it in place.

**Install, update and uninstall (user flow):**
- **Update:** download the newer `…-setup.exe` from the Releases page and run it. It upgrades the existing install in place. Do not uninstall first. Updates do not touch your data, which lives in three places:
  - `%APPDATA%\com.conclave.app`: the meetings database, settings and downloaded models.
  - `%APPDATA%\Conclave`: custom summary templates, notification settings and a fallback model folder.
  - `%USERPROFILE%\Music\conclave-recordings` (falls back to Documents if Music is unavailable): recordings.
- **Uninstall:** use Windows Settings → Apps & features (or Control Panel → Programs and Features) → Conclave → Uninstall. The uninstaller has a "Delete the application data" checkbox. It is unchecked by default. Checking it removes the `%APPDATA%\com.conclave.app` folder (meetings, settings and models). It does not remove `%APPDATA%\Conclave` or `%USERPROFILE%\Music\conclave-recordings`; those must be deleted by hand if wanted. Do not delete the install folder by hand, because that leaves the Start-menu entry and the uninstall registration behind.
- **Meetily:** a separate app with its own identifier and data folder. It can stay installed next to Conclave or be uninstalled the same way, with no effect on Conclave.

The last task of Phase 0 is the `conclave-probe` diagnostic tool (§5.4).

**Phase 0 checkpoint:** the user installs the release, records one real call, and gets a transcript and a summary. In the same session, the user runs `conclave-probe.exe` and reports the §5.4 results, so Phase 2 starts with its risks already answered.

## 4. Phase 1: Whisper on CUDA

### 4.1 Build

- `release.yml` installs the CUDA 12 toolkit on the runner and builds with `--features cuda`.
- `CMAKE_CUDA_ARCHITECTURES=89` compiles only for the RTX 40 series, which keeps build time and minutes down.
- The CUDA build output (the whisper.cpp / ggml objects) is cached between release runs.
- **The big CUDA libraries are not bundled** (user decision, 2026-10-12): `cublas64_12.dll` and `cublasLt64_12.dll` (about 600 MB) come from the user's CUDA 12.x toolkit (`CUDA_SETUP.md`), found through `PATH`. Only `cudart64_12.dll` (about 0.5 MB) ships next to the exe. This was a controller ruling on 2026-10-12, after the Task 3 review found that nvcc-generated static initialisers call cudart before `main`, so cudart can't be optional. The installer stays at about 100 MB. CI minutes are unchanged, because the toolkit is still needed to compile.
- CI and the PC must use the same CUDA **major** version (12.x). The workflow pins the toolkit to 12.8 and builds on `windows-2022`, because nvcc 12.8 rejects the VS 2026 compiler on `windows-latest`.
- **A missing CUDA toolkit or NVIDIA driver must not be a cryptic crash.** `cublas64_12.dll`, `cublasLt64_12.dll` and `nvcuda.dll` (the driver) are **delay-loaded**. At startup, `LoadLibraryW` checks `cublas64_12.dll`, `cublasLt64_12.dll` and `nvcuda.dll`. If any is missing, Whisper gets `use_gpu = false` and the user sees "CUDA 12 runtime not found — see CUDA_SETUP.md". The release workflow's start-without-CUDA smoke test checks this before publishing.
- The Vulkan build is removed from releases. `ci.yml` stays CPU-only.
- If CUDA fails to start at runtime for another reason (an old driver, for example), whisper.cpp falls back to the CPU and the app keeps working.

### 4.2 Backend note

- One read-only line in the transcription settings: **"Whisper running on: CUDA (\<GPU name\>)"** or **"Whisper running on: CPU"**.
- The source is the existing detection in `whisper_engine/acceleration.rs` (the compiled backend plus the runtime-detected GPU). A Tauri command exposes it to the settings UI.
- Parakeet stays on CPU and is not changed.

### 4.3 Tests

- **CI:** a unit test asserts that a build with the `cuda` feature reports the CUDA compiled backend, and that a CPU build reports CPU. The release job fails if the CUDA build fails.
- **User's PC:** the backend note shows CUDA, and the Task Manager "Cuda" graph is active while transcribing. `large-v3-turbo` keeps up with a live call. Temporarily removing `%CUDA_PATH%\bin` from `PATH` gives the clear message plus CPU transcription, not a crash.

## 5. Phase 2: Meeting auto-detection

### 5.1 Behaviour

Setting: **Auto-detect meetings: Off / Ask (default) / Auto-record.**

| Call app is using the mic | Classified as | Certain? |
|---|---|---|
| `Zoom.exe` | Zoom | yes |
| New Teams (packaged app `MSTeams_*`) or `ms-teams.exe` | Teams | yes |
| Browser (`chrome.exe`, `msedge.exe`, `firefox.exe`, `brave.exe`) with a window title containing "Meet" or "Microsoft Teams" | Meet / Teams | yes |
| Browser with no matching title | "Browser is using the microphone" | no |
| Conclave itself | ignored | — |

- **Ask mode:** a Windows notification such as "Zoom meeting detected. Click to record". Clicking it brings Conclave's window forward with the meeting title pre-filled and a Record button. A meeting that was dismissed or ignored is not prompted again until its app releases the mic.
- **Auto-record mode:** recording starts at once for certain detections. An uncertain detection always prompts, as in Ask mode.
- **Stop:** when a recording has a detected call app attached, recording stops **5 seconds after that app releases the mic**. The grace period covers headset switches. This applies however the recording was started, including manually during a detected call. The usual Meetily flow (save, transcribe, summarize) then runs, and a notification says "\<App\> meeting ended. Notes are being generated."
- Muting in the call app does not release the mic, so it does not stop the recording. Pre-flight check A (§5.4) verifies this.
- A recording with no call app ever attached (for example, recording a video) is never stopped automatically.
- **While a recording is active, new detections are ignored.** Overlapping calls are not handled.
- **Rejoining a call** (for example, Zoom's 40-minute limit) is handled as Meetily does today. The first meeting stops when the call drops, and rejoining is detected as a new meeting. Meetily's existing Pause/Resume during a recording is unchanged.

### 5.2 Microphone following

- When a recording is attached to a call app, Conclave finds **which capture device that app is using** by listing active audio sessions on every capture endpoint (WASAPI `IAudioSessionManager2` → `IAudioSessionControl2::GetProcessId`). It records from that device.
- It re-checks on every mic-usage change notification and on the 5-second safety re-read (§5.3). If the app moved to a different device, Conclave switches its mic stream through Meetily's existing mid-recording hot-swap path (`recording_manager.rs`: take the mic stream out, then set the new one). Up to about 1 second of audio may be lost at the switch.
- The recording screen always shows the current mic, for example "🎙 Shure MV7 (following Zoom)". If the user picks a mic manually during a recording, following turns off for the rest of that recording.
- If the app's device can't be determined, or the switch fails, Conclave keeps recording on the current mic and shows a warning on the recording screen.
- Without an attached call app, mic selection works as in Meetily today.

### 5.3 Mechanism

- **Source of truth:** Windows' microphone consent store, which is updated by Windows whenever an app starts or stops using the mic, for any device.
  `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone`
  - Packaged apps are subkeys named by package family. Desktop apps are under `NonPackaged\<exe path with '#' separators>`.
  - Each entry has `LastUsedTimeStart` and `LastUsedTimeStop` (FILETIME). The app is **using the mic now** when `LastUsedTimeStop == 0` or `LastUsedTimeStart > LastUsedTimeStop`.
- **Event-driven:** a dedicated thread calls `RegNotifyChangeKeyValue` on that key (whole subtree, `REG_NOTIFY_CHANGE_LAST_SET`) and waits on the event with a **5-second timeout**. On the event or the timeout it re-reads the store. The timeout is the safety net if a notification is missed. There is no 2-second polling.
- Window titles are read with `EnumWindows` and `GetWindowTextW` (for windows of the browser's process IDs) **only when a browser starts using the mic**.
- **Units.** The detector lives in its own small library crate, `crates/meeting-detector/`, a new member of the root Cargo workspace. It builds and tests quickly, without the Tauri app or whisper.cpp.
  - `crates/meeting-detector/src/classify.rs`: pure functions with no Windows calls. Input is a snapshot (mic users, including the executable or package name, and the relevant window titles) plus the previous state. Output is the detection or end events and the start/stop/prompt decisions, including the 5-second grace rule.
  - `crates/meeting-detector/src/windows.rs`: a thin Windows layer. It reads the consent store (with the registry root as a parameter, for tests), waits for change notifications, reads window titles, and lists audio sessions per capture device. It uses the `windows` crate.
  - `crates/meeting-detector/src/bin/conclave-probe.rs`: the diagnostic tool from §5.4.
  - `frontend/src-tauri/src/meeting_detector.rs`: the app side. It runs the watcher thread, holds the setting, and wires events to Meetily's existing recording commands, notification manager and hot-swap path.
- The detector is compiled and active only on Windows. On other targets it is a no-op.

### 5.4 Pre-flight checks (built at the end of Phase 0, run at the Phase 0 checkpoint)

**`conclave-probe.exe`** is a small console tool: the binary target of `crates/meeting-detector` (§5.3). Phase 0 creates the crate with the first version of `windows.rs`, so the probe code becomes the Phase 2 Windows layer rather than being thrown away. `ci.yml` builds it and uploads it as a workflow artifact. It is not part of the installer. It has no network access and no admin rights, and it writes nothing except to the console. It prints a live, timestamped log of:

- every consent-store change: which app started or stopped using the mic (checks A and B);
- each change-notification wake-up, versus the 5-second timeout re-read (check B);
- the active audio sessions per capture device, each with its process name (check C);
- browser window titles when a browser starts using the mic.

Check D uses the installed Phase 0 build's existing test notification (`notifications::commands::show_test_notification`), clicked from the notification settings.

The user runs it in about 10 minutes during the Phase 0 checkpoint session:

- **A.** In Zoom, Meet and Teams, muting keeps the app listed as using the mic, and leaving the call releases it at once.
- **B.** `RegNotifyChangeKeyValue` fires when an app starts and stops using the mic.
- **C.** Audio-session listing shows the call app on the correct capture device, and the device changes when the user switches mic in the app.
- **D.** Meetily's notifications support click-to-open on Windows.

If A fails, the stop rule changes to "the meeting window closed" before implementation continues. If B fails, the 5-second re-read becomes the main mechanism. If D fails, Ask mode brings the Conclave window to the front with the prompt instead of using a clickable notification. Any change goes back to the user before implementation continues.

### 5.5 Tests

- **CI (`cargo test`):**
  - `classify.rs` cases: Zoom; packaged Teams; a browser with a Meet title; a browser with a Teams title; a browser with no match (uncertain → prompt even in Auto); Conclave ignored; Off/Ask/Auto decisions; no repeat prompt after dismissal; stop exactly after 5 seconds of release; a release shorter than 5 seconds does not stop; a recording without an attached app never auto-stops; detections are ignored while recording; mic following turns off after a manual pick.
  - On the Windows runner: the consent-store reader parses fake entries written to a test key under `HKCU\Software\ConclaveTest\...`, covering packaged and non-packaged entries, in-use and released.
- **User's PC:** real Zoom, Meet and Teams calls in Ask and Auto mode, including mute, leave, a mic switch mid-call, and a browser call with the call tab inactive.

## 6. Error handling summary

| Failure | Behaviour |
|---|---|
| Consent store unreadable | Auto-detect turns off. Settings shows the reason. Manual recording works. |
| Windows mic privacy blocks desktop apps | Meetily's existing permission check and message |
| Call app's mic unknown, or the switch fails | Keep the current mic and show a warning on the recording screen |
| CUDA unavailable at runtime (runtime DLLs missing, or an old driver) | CPU fallback with a message pointing to `CUDA_SETUP.md`. The backend note shows CPU. |
| LLM summary fails | Meetily as-is: the transcript is saved and the summary can be retried |
| Crash mid-recording | Meetily as-is: the incremental saver keeps the recorded data |

Detection, microphone following and the backend note never stop or block a recording.

## 7. Delivery

Phases run in order: 0 → 1 → 2. Each implementation task ends with its tests and a `reviewer` review. Each phase ends with a `reviewer` review with a Codex pass and a checkpoint on the user's PC (see `CLAUDE.md`). The §5.4 pre-flight results, gathered at the Phase 0 checkpoint, are confirmed with the user before Phase 2 begins.
