# Outbound network calls (audited 2026-10-10, Phase 0)

Source: the Step 1 grep over `frontend/src` and `frontend/src-tauri/src` (excluding `*_old`, `*.backup`, `core-old`, `lib_old_complex`), plus the `reqwest::`, `Client::new`, `Client::builder`, `fetch(` and `open_external_url` call sites. Line numbers are from the tree after Task 6 edits, except where a row says "was".

`scripts/check-no-phone-home.sh` enforces that the removed hosts stay out of product code.

## Hosts

| Host | Where (file:line) | Purpose | Decision |
|---|---|---|---|
| huggingface.co | `src-tauri/src/whisper_engine/whisper_engine.rs:1077-1088` (download via `Client::builder` at `:1143`); `src-tauri/src/parakeet_engine/parakeet_engine.rs:136` (Parakeet v2 base URL); `src-tauri/src/summary/summary_engine/models.rs:172,185,198,211,342,355,369,383` (GGUF summary models, downloaded by `model_manager.rs:473`); `src/lib/parakeet.ts:78` (comment only) | Whisper, Parakeet v2 and local summary model downloads | Allowed (model host) |
| meetily.towardsgeneralintelligence.com | `src-tauri/src/parakeet_engine/parakeet_engine.rs:127` (Parakeet v3 base URL, used at `:865-912`) | Parakeet v3 model download | Allowed (existing model host) |
| api.anthropic.com | `src-tauri/src/anthropic/anthropic.rs:103` (model list, `:100`); `src-tauri/src/summary/llm_client.rs:345` (messages, POST at `:398`/`:457`) | User-configured LLM summaries | Allowed (user-configured) |
| api.openai.com | `src-tauri/src/openai/openai.rs:130` (`:127`); `src-tauri/src/summary/llm_client.rs:303` | User-configured LLM summaries | Allowed (user-configured) |
| api.groq.com | `src-tauri/src/groq/groq.rs:98` (`:95`); `src-tauri/src/summary/llm_client.rs:307` | User-configured LLM summaries | Allowed (user-configured) |
| openrouter.ai | `src-tauri/src/openrouter/openrouter.rs:45` (`:43`); `src-tauri/src/summary/llm_client.rs:311` | User-configured LLM summaries | Allowed (user-configured) |
| localhost:11434 (Ollama, default; the user can set another endpoint) | `src-tauri/src/ollama/metadata.rs:145`; `src-tauri/src/ollama/ollama.rs:163,282,441`; `src-tauri/src/summary/llm_client.rs:317`; `src/components/ModelSettingsModal.tsx:1149,1203` | Local Ollama: tags, pull, delete, show, chat | Allowed (localhost Ollama) |
| localhost:11434 test fixtures | `src-tauri/src/summary/service.rs:798-990` (inside `mod tests`, from `:689`) | Cache-key test values, not requests | Not a call |
| ollama.com | `src/components/ModelSettingsModal.tsx:739,1236` (`open_external_url`, opens the user's browser) | "Download Ollama" link | Allowed (Ollama download page; user-initiated, see Reported to user) |
| localhost:8000 | `src-tauri/src/summary/mod.rs:16` (doc comment); `src/components/ModelSettingsModal.tsx:956` (placeholder) | Example for the user-configured OpenAI-compatible endpoint | Allowed (user-configured) |
| x.yy.zz:11434 | `src/components/ModelSettingsModal.tsx:1134` | Example text only | Not a call |
| custom.example | `src-tauri/src/summary/service.rs:991` (inside `mod tests`) | Cache-key test value | Not a call |
| localhost:11500 | `src-tauri/src/summary/service.rs:976` (inside `mod tests`) | Cache-key test value | Not a call |
| localhost:5167 | `src-tauri/src/api/api.rs:20` (`APP_SERVER_URL`), used by `make_api_request` (`:247-250`), `test_backend_connection` (`:1085`) and `debug_backend_connection` (`:1127`); `src/components/Sidebar/SidebarProvider.tsx:115` | Address of the archived FastAPI backend; loopback, nothing listens. Still called by the `api_*` commands | Reported to user |
| 127.0.0.1:8178 | was `src/components/Sidebar/SidebarProvider.tsx:119` and CSP `connect-src` | Archived transcript stream; no consumer | Removed (dead state and CSP entry) |
| api.ollama.ai | was `src-tauri/tauri.conf.json` CSP `connect-src` | None (no consumer) | Removed (CSP entry) |
| meetily.zackriya.com | was `src/components/About.tsx:17` | "Contact" button (open_external_url) | Removed (button and CTA block) |
| github.com/your-org/meetily | was `src/components/BluetoothPlaybackWarning.tsx:84` | "Learn why" link (broken) | Removed (link only; warning text kept) |
| github.com/Zackriya-Solutions/meeting-minutes | was `src/components/onboarding/steps/SetupOverviewStep.tsx:101`; was `src-tauri/Cargo.toml:7` (`repository`) | "Report issues" link; package metadata | Removed |
| us.i.posthog.com | none | Telemetry | Removed in Task 4 |
| github.com/Zackriya-Solutions/…/latest.json | none | Updater | Removed in Task 5 |
| www.w3.org/2000/svg | `src/app/_components/SettingsModal.tsx:74,178,219,257`; `src/components/AISummary/index.tsx:715,736,756,785`; `src/components/EditableTitle.tsx:73,93` | SVG XML namespace identifier | Not a request |

## Runtime-built URLs

These are the request sites, with the base URL each one builds from.

| Call site | Built URL | Decision |
|---|---|---|
| `src-tauri/src/summary/llm_client.rs:303-345` (`api_url` match), POST at `:398` and `:457` | Fixed provider URLs, or Ollama `{host}/v1/chat/completions` (`:319`), or custom `{endpoint}/chat/completions` (`:327`) | Allowed (user-configured) |
| `src-tauri/src/summary/service.rs:546` (`Client::new`) | Passed to `generate_meeting_summary` (llm_client) | Allowed (as above) |
| `src-tauri/src/ollama/ollama.rs:162,281,440` (`Client::new`) with `{base_url}/api/tags` (`:164`), `/api/pull` (`:283`), `/api/delete` (`:442`) | Ollama endpoint (default localhost:11434) | Allowed (localhost Ollama) |
| `src-tauri/src/ollama/metadata.rs:144` (`Client::new`), `{base_url}/api/show` (`:146`) | Ollama endpoint | Allowed (localhost Ollama) |
| `src-tauri/src/api/api.rs:247-250` (`make_api_request`), `:1085`, `:1127` | `APP_SERVER_URL` (localhost:5167) plus endpoint | Reported to user |
| `src-tauri/src/api/api.rs:1292` (`{endpoint}/chat/completions`, `Client::builder` at `:1306`) | User's custom OpenAI endpoint (test connection) | Allowed (user-configured) |
| `src-tauri/src/summary/summary_engine/model_manager.rs:473` (`Client::builder`) | GGUF URLs from `models.rs` | Allowed (model host) |
| `src-tauri/src/whisper_engine/whisper_engine.rs:1076` (`model_url` match), client at `:1143` | Hugging Face URLs (`:1077-1088`) | Allowed (model host) |
| `src-tauri/src/parakeet_engine/parakeet_engine.rs:865` (`Client::builder`), `{base_url}/{filename}` at `:912`, `get(file_url)` at `:699` | Parakeet v2 or v3 base URL (`:127`, `:136`) | Allowed (model host) |
| `src-tauri/src/anthropic/anthropic.rs:100`, `groq.rs:95`, `openai.rs:127`, `openrouter.rs:43` | Provider model-list URLs (table above) | Allowed (user-configured) |
| `src/components/ModelSettingsModal.tsx:739,1236` (`invoke('open_external_url')`) | `https://ollama.com/download` | Allowed (see above) |

Test-only clients (`reqwest::Client::new()` at `llm_client.rs:698,784,860`, `whisper_engine.rs:1601,1641,1796`, `parakeet_engine.rs:1341`) talk only to local mock servers in `mod tests`.

No `fetch(` calls reach the network in `frontend/src` (the only match is `refetch`).

## Removed (updater tooling)

- `scripts/generate-update-manifest-github.js`
- `scripts/test-update-locally.js`
- `frontend/build.ps1`: the "Updater artifacts have been signed" line. The signing-credential check is still there (see Reported to user).

## Reported to user

- **localhost:5167 (legacy backend address).** `api/api.rs:20` and the `api_*` commands (`api_get_meetings`, `api_get_profile`, `api_get_model_config`, etc.) still send requests there. Nothing listens there, since the FastAPI backend is archived. Removing it means removing those commands and their callers, which breaks the features they serve. Not removed.
- **ollama.com/download.** Kept as the "Download Ollama" link, as the brief's template lists it. The Global Constraints allowed list names only localhost Ollama, so the user should confirm.
- **Whisper user agent.** `whisper_engine/whisper_engine.rs:1141` sends `Meetily/<version>` to Hugging Face. This is the user-agent rename, left for Task 7.
- **Signing gate in `frontend/build.ps1`.** Lines 12-50 still require `TAURI_SIGNING_PRIVATE_KEY`, which conflicts with the "no TAURI_SIGNING_* secrets" constraint. Not changed, as the ruling was limited to the updater line.
