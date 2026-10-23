# Outbound network calls (audited 2026-10-10, Phase 0)

Source: the Step 1 grep over `frontend/src` and `frontend/src-tauri/src`, plus the `reqwest::`, `Client::new`, `Client::builder`, `fetch(` and `open_external_url` call sites. Line numbers are from the tree after Task 7 (`ee96a45`), except where a row says "was".

`scripts/check-no-phone-home.sh` enforces that the removed hosts stay out of product code.

## Hosts

| Host | Where (file:line) | Purpose | Decision |
|---|---|---|---|
| huggingface.co | `src-tauri/src/whisper_engine/whisper_engine.rs:1104-1115` (download via `Client::builder` at `:1172`); `src-tauri/src/parakeet_engine/parakeet_engine.rs:136` (Parakeet v2 base URL); `src-tauri/src/summary/summary_engine/models.rs:172,185,198,211,342,355,369,383` (GGUF summary models, downloaded by `model_manager.rs:473`); `src/lib/parakeet.ts:78` (comment only) | Whisper, Parakeet v2 and local summary model downloads | Allowed (model host) |
| huggingface.co | `src-tauri/src/speakers/models.rs` (`MODELS` URLs, pinned to commit hashes; `download_models`) | Speaker identification models (pyannote segmentation 3.0, TitaNet-large), downloaded only on the user's request and SHA-256 verified | Allowed (model host) |
| huggingface.co | `src-tauri/src/practice/models.rs` (`FILES`: 16 files, each URL pinned to commit `724fb5ab…` and SHA-256 checked; `ensure_voices`) | Speaking Practice voices (Supertonic 3 TTS), downloaded on first use with the speaker downloader and SHA-256 verified | Allowed (model host) |
| huggingface.co | `src-tauri/src/practice/models.rs` (`PRON_FILES`, `hubert_phoneme_int8.onnx`, pinned to commit `bbc27947…`) | Speaking Practice pronunciation hints (HuBERT phoneme model), downloaded on the first pronunciation report and SHA-256 verified | Allowed (model host) |
| raw.githubusercontent.com | `src-tauri/src/practice/models.rs` (`PRON_FILES`, `cmudict.dict`, pinned to commit `74790861…`) | Speaking Practice pronunciation dictionary (CMU dict), downloaded on the first pronunciation report and SHA-256 verified | Allowed (model host) |
| meetily.towardsgeneralintelligence.com | `src-tauri/src/parakeet_engine/parakeet_engine.rs:127` (Parakeet v3 base URL, used at `:865-912`) | Parakeet v3 model download; contacted only when the user downloads a Parakeet model | Allowed — Meetily-owned, reported to user |
| api.anthropic.com | `src-tauri/src/anthropic/anthropic.rs:103` (model list, `:100`); `src-tauri/src/summary/llm_client.rs:345` (messages, POST at `:398`/`:457`) | User-configured LLM summaries | Allowed (user-configured) |
| api.openai.com | `src-tauri/src/openai/openai.rs:130` (`:127`); `src-tauri/src/summary/llm_client.rs:303` | User-configured LLM summaries | Allowed (user-configured) |
| api.groq.com | `src-tauri/src/groq/groq.rs:98` (`:95`); `src-tauri/src/summary/llm_client.rs:307` | User-configured LLM summaries | Allowed (user-configured) |
| openrouter.ai | `src-tauri/src/openrouter/openrouter.rs:45` (`:43`); `src-tauri/src/summary/llm_client.rs:311` | User-configured LLM summaries | Allowed (user-configured) |
| Same LLM hosts as the rows above (the user's chosen provider) | `src-tauri/src/copilot/ask.rs` (`copilot_ask`, `:247`) | Live copilot: sends the live transcript and notes to the user's LLM, only when the user clicks | Allowed (user-configured) |
| Same LLM hosts as the rows above (the Knowledge model's provider) | `src-tauri/src/knowledge/extract.rs` (`run_one` `:250`, `call_model` `:258`, `complete` `:264`) | Task extraction: after each summary and on Retry, or when a weekly digest or a pre-meeting brief needs it; sends the summary and the user's own lines to the Knowledge model | Allowed (user-configured) |
| Same LLM hosts as the rows above (the Knowledge model's provider) | `src-tauri/src/knowledge/digest.rs` (`call_model` `:452`, `complete` `:458`) | Friday weekly digest: sends the head of each summary from the week to the Knowledge model, once a week | Allowed (user-configured) |
| Same LLM hosts as the rows above (the Knowledge model's provider) | `src-tauri/src/knowledge/ask.rs` (`ask`, `complete` `:259`) | Ask: sends the question, the chat so far and retrieved excerpts of the user's meetings to the Knowledge model, only when the user asks | Allowed (user-configured) |
| The user's calendar host (the iCal "secret address" the user pastes; https only) | `src-tauri/src/knowledge/calendar.rs` (`get_body`, `fetch`; `CALENDAR_REFRESH_MINS` in `knowledge/mod.rs`) | GET of the user's calendar feed: once on connect, then every 30 min while running, only while connected. The URL is never logged or put in an error; only the status shows the host | Allowed (user-configured; no Meetily-owned host) |
| localhost:11434 (Ollama, default; the user can set another endpoint) | `src-tauri/src/ollama/metadata.rs:145`; `src-tauri/src/ollama/ollama.rs:163,282,441`; `src-tauri/src/summary/llm_client.rs:317`; `src/components/ModelSettingsModal.tsx:1149,1203` | Local Ollama: tags, pull, delete, show, chat | Allowed (localhost Ollama) |
| localhost:11434 test fixtures | `src-tauri/src/summary/service.rs:798-990` (inside `mod tests`, from `:689`) | Cache-key test values, not requests | Not a call |
| ollama.com | `src/components/ModelSettingsModal.tsx:739,1236` (`open_external_url`, opens the user's browser) | "Download Ollama" link | Allowed (Ollama download page; user-initiated, see Reported to user) |
| github.com/Sashanator/conclave | `src/app/page.tsx:78` (`open_external_url`, opens the user's browser) | "Open guide" button on the CUDA-missing warning, opens `CUDA_SETUP.md` | Allowed (own repo; user-initiated) |
| localhost:8000 | `src-tauri/src/summary/mod.rs:16` (doc comment); `src/components/ModelSettingsModal.tsx:956` (placeholder) | Example for the user-configured OpenAI-compatible endpoint | Allowed (user-configured) |
| x.yy.zz:11434 | `src/components/ModelSettingsModal.tsx:1134` | Example text only | Not a call |
| custom.example | `src-tauri/src/summary/service.rs:991` (inside `mod tests`) | Cache-key test value | Not a call |
| localhost:11500 | `src-tauri/src/summary/service.rs:976` (inside `mod tests`) | Cache-key test value | Not a call |
| localhost:5167 | `src-tauri/src/api/api.rs:20` (`APP_SERVER_URL`), used by `make_api_request` (`:247-250`), called from the profile commands (`api.rs:401`, `:421`, `:455`), `test_backend_connection` (`:1085`) and `debug_backend_connection` (`:1127`); `src/components/Sidebar/SidebarProvider.tsx:115` (setter) | Address of the archived FastAPI backend; loopback, nothing listens | Reported to user (unreachable from the UI) |
| 127.0.0.1:8178 | was `src/components/Sidebar/SidebarProvider.tsx:119` and CSP `connect-src` | Archived transcript stream; no consumer | Removed (dead state and CSP entry) |
| api.ollama.ai | was `src-tauri/tauri.conf.json` CSP `connect-src` | None (no consumer) | Removed (CSP entry) |
| meetily.zackriya.com | was `src/components/About.tsx:17` | "Contact" button (open_external_url) | Removed (button and CTA block) |
| github.com/your-org/meetily | was `src/components/BluetoothPlaybackWarning.tsx:84` | "Learn why" link (broken) | Removed (link only; warning text kept) |
| github.com/Zackriya-Solutions/meeting-minutes | was `src/components/onboarding/steps/SetupOverviewStep.tsx:101`; was `src-tauri/Cargo.toml:7` (`repository`) | "Report issues" link; package metadata | Removed |
| gyan.dev (Windows); evermeet.cx, osxexperts.net, johnvansickle.com (macOS, Linux) | was `src-tauri/src/audio/ffmpeg.rs:164-182` (`ffmpeg_sidecar::download`) | Runtime fallback download of an ffmpeg executable | Removed (runtime ffmpeg auto-download); ffmpeg is bundled next to Conclave.exe |
| us.i.posthog.com | none | Telemetry | Removed in Task 4 |
| github.com/Zackriya-Solutions/…/latest.json | none | Updater | Removed in Task 5 |
| www.w3.org/2000/svg | `src/app/_components/SettingsModal.tsx:74,178,219,257`; `src/components/AISummary/index.tsx:715,736,756,785`; `src/components/EditableTitle.tsx:73,93` | SVG XML namespace identifier | Not a request |

## Runtime-built URLs

These are the request sites, with the base URL each one builds from.

| Call site | Built URL | Decision |
|---|---|---|
| `src-tauri/src/summary/llm_client.rs:303-345` (`api_url` match), POST at `:398` and `:457` | Fixed provider URLs, or Ollama `{host}/v1/chat/completions` (`:319`), or custom `{endpoint}/chat/completions` (`:327`) | Allowed (user-configured) |
| `src-tauri/src/summary/service.rs:546` (`Client::new`) | Passed to `generate_meeting_summary` (llm_client) | Allowed (as above) |
| `src-tauri/src/dictate/cleanup.rs` (Dictate cleanup; `summary/llm_target.rs` `complete` → `summary/llm_client.rs` `generate_summary`) | The same URLs as the summary rows above: the user's configured summary provider, or the cleanup model if one is set (the built-in local AI sidecar is not used) | Allowed (user-configured; no new hosts) |
| `src-tauri/src/ollama/ollama.rs:162,281,440` (`Client::new`) with `{base_url}/api/tags` (`:164`), `/api/pull` (`:283`), `/api/delete` (`:442`) | Ollama endpoint (default localhost:11434) | Allowed (localhost Ollama) |
| `src-tauri/src/ollama/metadata.rs:144` (`Client::new`), `{base_url}/api/show` (`:146`) | Ollama endpoint | Allowed (localhost Ollama) |
| `src-tauri/src/api/api.rs:247-250` (`make_api_request`), called by `api.rs:401`, `:421`, `:455`; `:1085`, `:1127` (`/docs`) | `APP_SERVER_URL` (localhost:5167) plus endpoint | Reported to user (unreachable from the UI) |
| `src-tauri/src/api/api.rs:1345` (`{endpoint}/chat/completions`, `Client::builder` at `:1359`) | User's custom OpenAI endpoint (test connection) | Allowed (user-configured) |
| `src-tauri/src/summary/summary_engine/model_manager.rs:473` (`Client::builder`) | GGUF URLs from `models.rs` | Allowed (model host) |
| `src-tauri/src/whisper_engine/whisper_engine.rs:1103` (`model_url` match), client at `:1172`, user agent `Conclave/<version>` at `:1173` | Hugging Face URLs (`:1104-1115`) | Allowed (model host) |
| `src-tauri/src/parakeet_engine/parakeet_engine.rs:865` (`Client::builder`), `{base_url}/{filename}` at `:912`, `get(file_url)` at `:699` | Parakeet v2 or v3 base URL (`:127`, `:136`) | Allowed (model host; v3 is Meetily-owned, see Hosts) |
| `src-tauri/src/anthropic/anthropic.rs:100`, `groq.rs:95`, `openai.rs:127`, `openrouter.rs:43` | Provider model-list URLs (table above) | Allowed (user-configured) |
| `src-tauri/src/practice/claude.rs` (`MESSAGES_URL`, POST via `stream_with_idle`) | `https://api.anthropic.com/v1/messages` with the user's own Claude key; carries the Practice prompts, transcribed text and the user's materials (PDF materials as base64 documents), never audio | Allowed (user-configured; no new hosts) |
| `src/components/ModelSettingsModal.tsx:739,1236` (`invoke('open_external_url')`) | `https://ollama.com/download` | Allowed (see above) |

Auto-pick of a summary template: one short request to the same provider before the summary (first 4,000 characters of the transcript plus template names and "Use when" lines).

Dictate sends only the transcribed text to the LLM, never audio, and only when a cloud mode (Cleaned up or a custom mode) is used. The As spoken mode makes no request.

Token usage (for Costs and Stats) is read from the `usage` object of replies the app already receives. OpenRouter is asked to include usage in its reply (`summary/llm_client.rs:96`, same host), and Speaking Practice reads usage from its Claude stream (`practice/claude.rs`, same host). These add no host.

Test-only clients (`reqwest::Client::new()` at `llm_client.rs:698,784,860`, `whisper_engine.rs:1601,1641,1796`, `parakeet_engine.rs:1341`) talk only to local mock servers in `mod tests`.

No `fetch(` calls reach the network in `frontend/src` (the only match is `refetch`).

## Build-time downloads (CI only, not the app)

These run while the installer is built, not when Conclave runs. They are not in the app's network surface.

| Host | Where (file:line) | Purpose | Decision |
|---|---|---|---|
| github.com/Zackriya-Solutions/ffmpeg-binaries | `src-tauri/build/ffmpeg.rs:134-166` | ffmpeg archive bundled as `externalBin` in the installer | Allowed — Meetily-owned, reported to user; archive pinned by SHA-256 |
| api.nuget.org | `src-tauri/build/onnxruntime.rs` (`PACKAGES` URLs) | ONNX Runtime 1.22.0 DirectML build and DirectML 1.15.4 NuGet packages (`build.rs`); both archives SHA-256 pinned | Allowed (build-time only) |
| cdn.pyke.io | `ort-sys` build script (`ort-sys-2.0.0-rc.10/dist.txt`, `build.rs` `real_main`), reached through `ort`'s default `download-binaries` on Windows; URL `https://cdn.pyke.io/0/pyke:ort-rs/ms@1.22.0/x86_64-pc-windows-msvc.tgz` | `ort-sys`'s prebuilt ONNX Runtime 1.22.0 tarball, fetched in the CI build only. Hash-checked by ort-sys (SHA-256 `540d19b3…190d`); extracted to the cargo cache and copied to `OUT_DIR`; not bundled, not used by Conclave | Allowed (build-time only); to be removed by the later fix: set `ORT_SKIP_DOWNLOAD=1` or `ORT_LIB_LOCATION` in the Windows build. Not yet applied |
| fonts.googleapis.com (via `next/font/google`) | `src/app/layout.tsx:4` | Source Sans 3 font, fetched by Next.js at build time and self-hosted | Allowed (build-time only) |

## Removed (updater tooling)

- `scripts/generate-update-manifest-github.js`
- `scripts/test-update-locally.js`
- `frontend/build.ps1`: the "Updater artifacts have been signed" line. The signing-credential check is still there (see Reported to user).

## Removed (runtime ffmpeg auto-download)

- `frontend/src-tauri/src/audio/ffmpeg.rs`: the fallback in `find_ffmpeg_path_internal` no longer downloads ffmpeg from the internet when it is not found. It returns `None` with the error "ffmpeg not found next to Conclave.exe; reinstall Conclave". The `ffmpeg-sidecar` dependency stays, for `sidecar_dir`.

## Removed (unused config)

- `frontend/src-tauri/config/backend_config.json`: held 8178, 5167 and 11434 endpoints. Nothing referenced it.

## Reported to user

- **localhost:5167 (legacy backend address).** `api/api.rs:20` is hardcoded. The profile commands (`api_get_profile`, `api_save_profile`, `api_update_profile`) and the connection tests (`test_backend_connection`, `debug_backend_connection`) send requests there. None of them is invoked from `frontend/src`, so they are unreachable from the UI. The local-DB commands (`api_get_meetings`, `api_get_model_config`) do not use it. Kept as localhost-only legacy cleanup, out of Phase 0 scope.
- **ollama.com/download.** Kept as the "Download Ollama" link, as the brief's template lists it. The Global Constraints allowed list names only localhost Ollama, so the user should confirm.
- **Parakeet v3 host `meetily.towardsgeneralintelligence.com`.** Meetily-owned. Kept as the Parakeet v3 model host (the spec allows existing model hosts), contacted only on a Parakeet model download.
- **ffmpeg-binaries host `github.com/Zackriya-Solutions/ffmpeg-binaries`.** Meetily-owned, used only at build time to bundle ffmpeg. Kept for Phase 0, not changed.
- **Signing gate in `frontend/build.ps1`.** Lines 12-50 required `TAURI_SIGNING_PRIVATE_KEY`, which conflicted with the "no TAURI_SIGNING_* secrets" constraint. Removed in Task 8 (no signing secrets).
