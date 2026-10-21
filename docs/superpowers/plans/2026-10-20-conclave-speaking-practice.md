# Conclave Speaking Practice, phase 1 (v0.8.0) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Practice page where the user saves role-play setups (character, situation, optional material). They hold a spoken session with that character (local Whisper → Claude Haiku stream → local Supertonic voice), then get a report on their English: grammar, unnatural phrasing, clarity, fillers and fluency.

**Architecture:**
- **New app module `frontend/src-tauri/src/practice/`:**
  - pure logic (prompts, sentence splitting, SSE parsing, the report metrics and quote validation, the session state machine), unit-tested on Linux;
  - a thin executor that wires the mic, Whisper, Claude, the Supertonic ONNX voice and playback;
  - its own SQLite tables and its own folder delete.
- **Existing code is called, never changed in behaviour.** The only exception is one added condition in `dictate::keeps_model_loaded()`, and making `speakers::runtime::open` reusable.
- **UI:** a single `/practice` route with four views (list, editor, call, report) and a sidebar entry.

**Tech Stack:** Rust (Tauri 2.6.2, tokio, reqwest 0.11 stream, sqlx/SQLite, `ort` 2.0.0-rc.10 with DirectML/CPU, ndarray 0.16, cpal 0.15, whisper-rs 0.13.2, hound, rand 0.8, sha2, base64), Next.js 14 + React 18 + TypeScript, Bun tests.

**Spec:** `docs/superpowers/specs/2026-10-20-conclave-speaking-practice-design.md`. Read it before any task. Section numbers below (§n) refer to it.

**Decisions this plan adds (same behaviour as the spec, settled here):**
- **The character opens the scene from a fixed first user message.** The 55-minute wrap-up is a mid-conversation `{"role":"system"}` message (supported on Claude Haiku 5.5, no beta header).
- **Barge-in history:**
  - The assistant turn stores only the sentences actually played, plus `" —"` so the model knows it was cut off.
  - If nothing was played, no assistant turn is stored. The API accepts consecutive user messages.
- **Live transcription while the user talks:**
  - Practice runs its own small cpal capture (reusing `dictate::capture` helpers) feeding a `ContinuousVadProcessor`.
  - Each finished speech segment is transcribed at once. Only the last segment is left when the user taps.
- **Material upload:** the bytes go to Rust as a raw IPC body (`tauri::ipc::Request`), never as a JSON number array.
- **Voices:** the Supertonic files download on first need (a Start or a ▶ preview), with progress. The engine is loaded per session or per preview, and dropped afterwards.
- **No live texts in logs:** only lengths and timings (the same rule as Dictate). Texts are stored only in the `practice_*` tables.

## Global Constraints

- **Platform:** Windows 10 x64 is the target. Development and tests run on Linux (AGENTS.md "Verified on Linux"). Windows CI runs only at the Phase 5 checkpoint (Task 18), by manual dispatch.
- **Local builds:**
  - `export CARGO_TARGET_DIR=/workspace/target`; `nice -n 15 cargo test -j 4 ...`. Never run two cargo builds at once.
  - App tests: `cargo test -p meetily --lib practice::`.
  - Full check before each phase review: `cargo test --workspace --exclude llama-helper`, and `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`.
- **Self-contained (spec §3).** New code lives only in:
  - `frontend/src-tauri/src/practice/`
  - `frontend/src/app/practice/`
  - `frontend/src/components/Practice/`
  - `frontend/src/lib/practice.ts`
  - `frontend/tests/**/practice*`
  - `frontend/src-tauri/tests/fixtures/practice/`
  - one migration file

  **Touch points outside them**, exactly these: `lib.rs` (mod, commands, setup hooks), `Cargo.toml` (`hound` moved to dependencies, `base64`, `unicode-normalization`), `dictate/mod.rs` (`keeps_model_loaded`), `speakers/runtime.rs` (`open` becomes `pub(crate)`), `components/Sidebar/index.tsx`, `docs/network-calls.md`, `AGENTS.md` (layout row). Task 17 writes the removal checklist.
- **Constants in `practice/mod.rs` (spec values):**
  - `CHAT_MODEL = "claude-haiku-5-5"`, `PLAN_MODEL = "claude-opus-5-5"`, `REPORT_MODEL = "claude-opus-5-5"`
  - `CHAT_MAX_TOKENS = 400`, `LONG_MAX_TOKENS = 16000`
  - `MAX_SESSION_SECS = 3600`, `WARN_AT_SECS = 3000`, `WRAP_UP_AT_SECS = 3300`, `MAX_TURN_SECS = 300`
  - `MIN_SPEECH_SECS = 0.5`, `LONG_PAUSE_SECS = 1.0`, `LIVE_VAD_REDEMPTION_MS = 700`
  - `MATERIAL_MAX_BYTES = 20 * 1024 * 1024` (total per setup)
  - `TTS_STEPS = 8`, `TTS_SPEED = 1.05`, `TTS_RATE = 44_100`, `YOU_WAV_RATE = 16_000`
- **Claude requests:**
  - `POST https://api.anthropic.com/v1/messages`, headers `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`, always `"stream": true`.
  - The key comes from `SettingsRepository::get_api_key(pool, "claude")`. Never log it, and never put it in an error string.
- **Exact user-visible copy:**
  - phases: "Preparing {name}'s questions…", "Loading…", "{name} is speaking…", "Your turn — tap to talk", "Listening… tap when done", "Transcribing…", "Thinking…"
  - notices: "I didn't catch that — tap and try again", "10 minutes left", "Answer cut at 5 minutes", "Couldn't reach Claude — tap to talk and try again", "A meeting recording started, so the session ended"
  - blocked: "Add your Claude API key in Settings to use Practice", "Practice needs Whisper as the transcription engine (Settings → Transcription)", "Practice is unavailable while a meeting is recording, importing or re-transcribing"
  - voices: "Downloading voices… {percent}%"
- **Codex pass** on tasks marked **[Codex]** (persistence, deletion, untrusted input, credentials, concurrency).
- **Release:** **v0.8.0**.

## Review Focus

1. **Taps at awkward moments:** a double tap, a tap during Transcribing, Preparing or Loading, a tap after the session ended. Never two captures, never a stuck phase. Tests in Task 10.
2. **Claude fails mid-reply** (connection drop, `overloaded_error` event, HTTP 529/401). The sentences already played are kept. The notice shows, and the next tap starts a new user turn. No hang. Tests in Task 3 (SSE error, truncated stream) and Task 10 (`ReplyFailed` after sentences).
3. **The app closes or crashes during a session or a report.** On the next start the session is listed as ended, `you.wav` is intact up to the last finished turn, and the report shows Retry. Tests in Task 1 (`recover_interrupted`) and Task 8 (WAV header valid after each append).
4. **Character text the TTS or splitter may trip on:** "Dr. Smith", "3.5 GB", "e.g.", an ellipsis, quotes, an emoji, a Cyrillic name. No cut mid-sentence, and unsupported characters are dropped, never sent as `-1` ids. Tests in Task 2 and Task 4.
5. **A session folder holding a foreign file, or swapped for a link:** delete refuses and nothing is removed. Tests in Task 9.

## File Structure

| Path | Responsibility |
|---|---|
| `frontend/src-tauri/migrations/20261020000000_practice.sql` | `practice_*` tables and the three seed setups |
| `practice/mod.rs` | Constants, `hold_model()`/`holds_model()`, the removal checklist comment |
| `practice/db.rs` | Row types, CRUD, plan fingerprint, crash recovery |
| `practice/claude.rs` | SSE parser, request bodies, streaming call with cancellation |
| `practice/prompts.rs` | Character, plan and report prompts; `SentenceSplitter` |
| `practice/report.rs` | Report types, fillers, WPM, pauses, loop check, quote validation |
| `practice/models.rs` | Supertonic file list (pinned), `ensure_voices` |
| `practice/tts_text.rs` | Supertonic text front-end (preprocess, index) |
| `practice/tts.rs` | Supertonic ONNX engine and voice styles |
| `practice/audio.rs` | Mic capture for turns, `TurnRecorder` (`you.wav`), `Player` and `PlayQueue` |
| `practice/session.rs` | Pure session state machine (`Core`, `Event`, `Effect`) |
| `practice/runner.rs` | Executor: one tokio task per session |
| `practice/report_job.rs` | Word-for-word pass, metrics, Opus report, retry |
| `practice/files.rs` | Session folder create, delete, asset access |
| `practice/commands.rs` | Tauri commands and events |
| `frontend/src/lib/practice.ts` | Types and pure helpers |
| `frontend/src/app/practice/page.tsx` | Route; view switching |
| `frontend/src/components/Practice/{SetupList,SessionList,SetupEditor,CallView,ReportView}.tsx` | Views |

---

## Phase 1 — Foundations (pure, Linux-testable)

### Task 1: Tables, seeds and the data layer **[Codex]**

**Files:**
- Create: `frontend/src-tauri/migrations/20261020000000_practice.sql`, `practice/mod.rs`, `practice/db.rs`
- Modify: `lib.rs` (add `mod practice;` after `mod speakers;` at `:59`)
- Test: in `practice/db.rs` (`#[cfg(test)]`, in-memory pool + `sqlx::migrate!("./migrations")` as in `onboarding.rs:236-237`)

**Interfaces — Produces:**
```rust
// mod.rs: constants (Global Constraints), plus
pub fn holds_model() -> bool;               // static AtomicUsize HOLDS > 0
pub struct ModelHold;                       // RAII: hold_model() increments HOLDS, Drop decrements (a session and a report job can overlap)
pub fn hold_model() -> ModelHold;
// db.rs
pub struct SetupRow { pub id: String, pub name: String, pub character: String, pub situation: String, pub voice: String,
  pub seeded: bool, pub plan: Option<String>, pub plan_fingerprint: Option<String>, pub created_at: String, pub updated_at: String }
pub struct MaterialMeta { pub id: String, pub setup_id: String, pub kind: MaterialKind, pub name: String, pub bytes: i64 }
pub enum MaterialKind { Pdf, Text }               // DB text "pdf" | "text"
pub struct Material { pub meta: MaterialMeta, pub content: Vec<u8> }
pub struct SetupInput { pub id: Option<String>, pub name: String, pub character: String, pub situation: String, pub voice: String }
pub struct SessionRow { pub id: String, pub setup_id: Option<String>, pub setup_name: String, pub character: String, pub voice: String,
  pub started_at: String, pub duration_secs: f64, pub folder_path: Option<String>, pub plan: Option<String>, pub status: String,
  pub end_reason: Option<String>, pub report_json: Option<String>, pub report_status: String, pub report_error: Option<String> }
pub struct TurnRow { pub session_id: String, pub idx: i64, pub speaker: String, pub live_text: String, pub verbatim_text: Option<String>,
  pub at_secs: f64, pub wav_offset: Option<i64>, pub wav_len: Option<i64> }
pub async fn list_setups(pool) -> Result<Vec<SetupRow>, String>;          // ORDER BY updated_at DESC
pub async fn get_setup(pool, id) -> Result<Option<SetupRow>, String>;
pub async fn save_setup(pool, input: SetupInput) -> Result<SetupRow, String>; // validate: name/character non-empty after trim, ≤ 200 chars; situation ≤ 4000; voice ∈ VOICES; new id = uuid v4
pub async fn delete_setup(pool, id) -> Result<(), String>;                 // sessions keep their snapshot (FK SET NULL)
pub async fn list_materials(pool, setup_id) -> Result<Vec<MaterialMeta>, String>;
pub async fn load_materials(pool, setup_id) -> Result<Vec<Material>, String>;
pub async fn add_material(pool, setup_id, kind: MaterialKind, name: &str, content: Vec<u8>) -> Result<MaterialMeta, String>; // total bytes per setup ≤ MATERIAL_MAX_BYTES else Err("Material is limited to 20 MB per setup")
pub async fn delete_material(pool, id) -> Result<(), String>;
pub fn plan_fingerprint(situation: &str, materials: &[Material]) -> String; // sha256 hex of situation + "\0" + each (id, content) sorted by id
pub async fn set_plan(pool, setup_id, plan: Option<&str>, fingerprint: Option<&str>) -> Result<(), String>;
pub async fn create_session(pool, setup: &SetupRow, folder_path: &str, plan: Option<&str>) -> Result<SessionRow, String>; // status 'running', report_status 'pending'
pub async fn add_turn(pool, turn: &TurnRow) -> Result<(), String>;
pub async fn end_session(pool, id, duration_secs: f64, reason: &str) -> Result<(), String>; // status 'ended'
pub async fn set_report(pool, id, status: &str, json: Option<&str>, error: Option<&str>) -> Result<(), String>;
pub async fn set_verbatim(pool, session_id, idx: i64, text: &str) -> Result<(), String>;
pub async fn list_sessions(pool) -> Result<Vec<SessionRow>, String>;      // ORDER BY started_at DESC
pub async fn get_session(pool, id) -> Result<Option<(SessionRow, Vec<TurnRow>)>, String>;
pub async fn delete_session_rows(pool, id) -> Result<(), String>;
pub async fn recover_interrupted(pool) -> Result<u32, String>; // status 'running' → 'ended', end_reason 'interrupted'; report_status 'running'|'waiting' → 'failed', error "Interrupted — Retry"
pub const VOICES: [(&str, &str); 10] = [("M1","Male voice 1"),…,("M5","Male voice 5"),("F1","Female voice 1"),…,("F5","Female voice 5")];
```

**Migration (exact):**
```sql
CREATE TABLE practice_setups (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, character TEXT NOT NULL, situation TEXT NOT NULL, voice TEXT NOT NULL,
  seeded INTEGER NOT NULL DEFAULT 0, plan TEXT, plan_fingerprint TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE practice_materials (
  id TEXT PRIMARY KEY, setup_id TEXT NOT NULL REFERENCES practice_setups(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('pdf','text')), name TEXT NOT NULL, content BLOB NOT NULL, bytes INTEGER NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE practice_sessions (
  id TEXT PRIMARY KEY, setup_id TEXT REFERENCES practice_setups(id) ON DELETE SET NULL,
  setup_name TEXT NOT NULL, character TEXT NOT NULL, voice TEXT NOT NULL, started_at TEXT NOT NULL,
  duration_secs REAL NOT NULL DEFAULT 0, folder_path TEXT, plan TEXT,
  status TEXT NOT NULL CHECK (status IN ('running','ended')), end_reason TEXT,
  report_json TEXT, report_status TEXT NOT NULL DEFAULT 'pending' CHECK (report_status IN ('pending','waiting','running','ready','failed')), report_error TEXT);
CREATE TABLE practice_turns (
  session_id TEXT NOT NULL REFERENCES practice_sessions(id) ON DELETE CASCADE, idx INTEGER NOT NULL,
  speaker TEXT NOT NULL CHECK (speaker IN ('user','character')), live_text TEXT NOT NULL, verbatim_text TEXT,
  at_secs REAL NOT NULL, wav_offset INTEGER, wav_len INTEGER, PRIMARY KEY (session_id, idx));
```
Seeds (`seeded = 1`, `datetime('now')`):
- `seed-interview`: "Job interview" / "Anna Keller, engineering manager" / "Interview for a backend developer role at a mid-size software company. A mix of behavioural and technical questions. About 30 minutes." / voice `F2`
- `seed-thesis`: "Thesis defence" / "Prof. Weber, strict examiner" / "Master's thesis defence. The professor is sceptical about the evaluation method and likes follow-up questions. About 20 minutes." / voice `M1`
- `seed-casual`: "Casual chat" / "Mike, a friendly car mechanic" / "I bring my car to Mike's garage because of a strange noise. Small talk and explaining the problem." / voice `M3`

- [ ] **Step 1: Write the failing tests:**
  - `seeds_are_three_editable_setups` (ids, voices `F2`/`M1`/`M3`, `seeded = true`)
  - `save_setup_validates_and_updates` (empty name → Err; unknown voice → Err; an update keeps `created_at` and bumps `updated_at`)
  - `material_limit_is_per_setup` (19 MB + 2 MB → Err with the exact text; another setup is unaffected)
  - `deleting_a_setup_keeps_its_sessions` (session `setup_id` → NULL, `setup_name` kept; materials gone)
  - `fingerprint_changes_with_situation_or_material` (same inputs → same hex; an edited situation or an added material → a different one)
  - `deleting_a_session_removes_its_turns`
  - `recover_interrupted_ends_running_sessions_and_fails_running_reports` (returns 2 for one running session plus one `waiting` report; a `ready` report is untouched)
  - `model_holds_overlap` (two holds, drop one → `holds_model()` still true; drop both → false)
- [ ] **Step 2:** `cargo test -p meetily --lib practice::db` → FAIL (module missing).
- [ ] **Step 3:** Implement the migration, `mod.rs` constants and the model hold, and `db.rs`. Use `sqlx::query`/`query_as` like `database/repositories/*`, and `chrono::Utc::now().to_rfc3339()` for times.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(practice): tables, seed setups and data layer`.

### Task 2: Supertonic text front-end

**Files:**
- Create: `practice/tts_text.rs`, `frontend/src-tauri/tests/fixtures/practice/supertonic_text.json`
- Modify: `Cargo.toml` (`unicode-normalization = "0.1"`)

**Interfaces — Produces:**
```rust
pub fn preprocess(text: &str, lang: &str) -> String;           // exact port of supertonic 1.3.1 UnicodeProcessor._preprocess_text, steps 1-10, then "<{lang}>…</{lang}>"
pub struct Indexer(Vec<i64>);                                  // unicode_indexer.json: index = code point, value = id or -1
impl Indexer { pub fn from_json(bytes: &[u8]) -> Result<Self, String>; pub fn ids(&self, preprocessed: &str) -> Vec<i64>; } // chars with -1 or out of range are DROPPED (never -1)
pub fn supported_only(text: &str, idx: &Indexer) -> String;    // removes unsupported chars before preprocess; logs only the count
```
**Port reference:** `supertonic/core.py` `UnicodeProcessor`.
- NFKD normalisation
- emoji ranges removed
- `_SYMBOL_REPLACEMENTS`
- `[♥☆♡©\\]` removed
- `@`→" at ", `e.g.,`→"for example, ", `i.e.,`→"that is, "
- " ," " ." " !" " ?" " ;" " :" " '" spacing fixed
- duplicate quotes collapsed
- whitespace collapsed and trimmed
- "." added when the text doesn't end in `[.!?;:,'"')\]}…。」』】〉》›»]`
- language tags added

- [ ] **Step 1: Generate the parity fixture with the reference SDK** (network needed once):
  ```bash
  cd frontend/src-tauri/tests/fixtures/practice && uv run --python 3.12 --with supertonic==1.3.1 --with huggingface_hub python -c "
  import json; from supertonic.core import UnicodeProcessor; from huggingface_hub import hf_hub_download as d
  p=d('Supertone/supertonic-3','onnx/unicode_indexer.json',revision='724fb5abbf5502583fb520898d45929e62f02c0b')
  u=UnicodeProcessor(p); T=['Good morning, please have a seat.','Hmm — interesting “choice”… e.g., why?','Dr. Smith said 3.5 GB @ TU Munich','No period at the end','Weird  spacing , here !','emoji 😀 and ♥ hearts','Привет, Weber']
  json.dump([{'text':t,'pre':u._preprocess_text(t,'en'),'ids':u([t],'en')[0][0].tolist()} for t in T],open('supertonic_text.json','w'),ensure_ascii=False,indent=1)"
  ```
  Commit the JSON only. Do not commit the indexer: tests that need it are `#[ignore]` unless `PRACTICE_MODELS_DIR` is set.
- [ ] **Step 2: Write the failing tests:**
  - `preprocess_matches_reference` (every fixture `pre` equals `preprocess(text, "en")`)
  - `ids_never_contain_minus_one` (a synthetic indexer with `-1` at '😀' and short length; ids for "a😀b" skip it)
  - `ids_match_reference` (`#[ignore]`; needs `PRACTICE_MODELS_DIR/unicode_indexer.json`; fixture `ids` with any `-1` entries removed equals `Indexer::ids`)
- [ ] **Step 3:** Run → FAIL. **Step 4:** Implement with the `regex` and `unicode-normalization` crates. **Step 5:** Run → PASS (and the ignored test with the env var after Task 6 has downloaded the files).
- [ ] **Step 6:** Commit `feat(practice): Supertonic text front-end`.

### Task 3: Claude streaming client **[Codex]**

**Files:**
- Create: `practice/claude.rs`
- Modify: `Cargo.toml` (`base64 = "0.22"`)

**Interfaces — Produces:**
```rust
pub enum SseEvent { Text(String), Stop, Error { kind: String, message: String }, Ignore }
#[derive(Default)] pub struct SseParser { buf: String }
impl SseParser { pub fn push(&mut self, chunk: &str) -> Vec<SseEvent>; }  // handles events split across chunks, \r\n, multiple per chunk;
  // content_block_delta + text_delta → Text; message_stop → Stop; event "error" → Error; thinking/ping/other deltas → Ignore
pub struct ChatMsg { pub role: &'static str /* "user"|"assistant"|"system" */, pub content: String }
pub fn chat_body(system: &str, msgs: &[ChatMsg]) -> serde_json::Value;
  // {model: CHAT_MODEL, max_tokens: CHAT_MAX_TOKENS, stream: true, system, messages,
  //  thinking: {"type":"disabled"}, output_config: {"effort":"low"}, cache_control: {"type":"ephemeral"}}
pub fn plan_body(system: &str, user: &str, materials: &[crate::practice::db::Material]) -> serde_json::Value;
  // model PLAN_MODEL, max_tokens LONG_MAX_TOKENS, stream true, output_config {"effort":"high"};
  // one user message: each PDF as {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":<b64, no newlines>}},
  // each text material as {"type":"text","text":"Material: <name>\n<utf8 lossy>"}, then the instruction text block last
pub fn report_body(system: &str, user: &str, schema: &serde_json::Value) -> serde_json::Value;
  // model REPORT_MODEL, max_tokens LONG_MAX_TOKENS, stream true, output_config {"effort":"high","format":{"type":"json_schema","schema":schema}}
pub async fn stream(client: &reqwest::Client, key: &str, body: &serde_json::Value, cancel: &tokio_util::sync::CancellationToken,
                    on_text: impl FnMut(&str)) -> Result<String, ClaudeError>;  // returns the full text; cancel → Err(Cancelled)
pub enum ClaudeError { Cancelled, NoKey, Rejected /*401/403*/, Busy /*429/529/overloaded_error*/, Http(u16), Network(String), Stream(String) }
impl ClaudeError { pub fn user_message(&self) -> &'static str; } // never includes the key or response bodies
pub async fn api_key(pool: &sqlx::SqlitePool) -> Result<String, ClaudeError>; // SettingsRepository::get_api_key(pool,"claude"); None/blank → NoKey
pub fn http_client() -> reqwest::Client;  // connect_timeout 30 s; no overall timeout (streams); the caller applies a 20 s first-byte timeout
```
- [ ] **Step 1: Write the failing tests** (pure, canned SSE text):
  - `parser_joins_events_split_across_chunks`
  - `parser_handles_crlf_and_several_events_per_chunk`
  - `parser_ignores_thinking_and_ping`
  - `parser_reports_error_event` (`overloaded_error` → `Error{kind:"overloaded_error",..}`, mapped to `Busy` by `stream`'s mapper `fn map_error_kind`, unit-tested)
  - `chat_body_turns_thinking_off_at_low_effort` (asserts every field above, `stream == true`)
  - `plan_body_puts_pdfs_before_the_instruction` (block order, `media_type`, base64 without `\n`)
  - `report_body_sets_json_schema_format`
  - `user_messages_never_echo_key` (every `ClaudeError::user_message()` is free of a sample key)
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. `stream` posts with `bytes_stream()` (reqwest `stream` feature is on), feeds `SseParser`, and stops on `Stop`. If the stream ends without `Stop`, that is `Err(Stream("ended early"))`. **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(practice): streaming Claude client`.

### Task 4: Prompts and the sentence splitter

**Files:** Create `practice/prompts.rs`.

**Interfaces — Produces:**
```rust
pub struct Persona<'a> { pub character: &'a str, pub situation: &'a str, pub plan: Option<&'a str> }
pub fn character_system(p: &Persona, variety_seed: u32) -> String; // rules from spec §5.1 verbatim in spirit; plan section only when Some;
  // variety line: "For variety, start today with question {1 + seed % 5} of your pool and move through the rest in a different order."
pub const OPENING_USER: &str = "(The learner has just joined. Open the scene in character with a short greeting and your first line.)";
pub const WRAP_UP_SYSTEM: &str = "About five minutes are left. Start bringing the scene to a natural close over your next replies.";
pub fn plan_prompts(character: &str, situation: &str) -> (String, String); // (system, user) asking for a 15-20 question pool, weak points, follow-ups, key facts; plain text
pub fn report_prompts(turns: &[(i64, &str, &str)]) -> (String, String);    // (idx, speaker, text) → "T{idx} [learner|character]: …"; asks for exact contiguous quotes from learner turns only
pub struct SentenceSplitter { buf: String }
impl SentenceSplitter { pub fn new() -> Self; pub fn push(&mut self, delta: &str) -> Vec<String>; pub fn finish(&mut self) -> Option<String>; }
```
**Splitting rules:**
- Split after `.`, `!`, `?` or `…` when followed by whitespace, and the next non-space char is not lowercase.
- Never split after `Mr. Mrs. Ms. Dr. Prof. St. vs. etc. e.g. i.e.`, after a single capital letter ("U."), or inside a number ("3.5").
- Trim output. Drop empty pieces.

- [ ] **Step 1: Write the failing tests:**
  - `splits_on_sentence_ends_across_deltas` ("Hmm, interesting. Why" + " did you?" → ["Hmm, interesting."], then finish → "Why did you?")
  - `keeps_abbreviations_and_numbers` ("Dr. Smith uses 3.5 GB, e.g. this. Next." → ["Dr. Smith uses 3.5 GB, e.g. this.", …])
  - `ellipsis_and_quotes` ("Well… \"Really?\" Yes." splits sensibly, no empty piece)
  - `character_system_contains_rules_and_plan` (contains the character, the situation, "never say", "one question", the plan text; no plan section when None)
  - `variety_line_depends_on_seed`
  - `report_prompt_numbers_turns_and_labels_speakers`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): prompts and sentence splitter`.

### Task 5: Report types, fluency metrics and quote validation

**Files:** Create `practice/report.rs`.

**Interfaces — Produces:**
```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub enum ItemKind { Grammar, Unnatural, Clarity }
#[derive(Serialize, Deserialize, Clone, Debug)] pub struct ReportItem { pub kind: ItemKind, pub turn: i64, pub quote: String, pub better: String, pub note: String, pub echoed: bool }
#[derive(Serialize, Deserialize, Clone, Debug, Default)] pub struct Fluency { pub fillers: std::collections::BTreeMap<String, u32>, pub filler_total: u32, pub words: u32, pub speech_secs: f64, pub wpm: f64, pub long_pauses: u32 }
#[derive(Serialize, Deserialize, Clone, Debug)] pub struct Report { pub version: u32 /* 1 */, pub fluency: Fluency, pub items: Vec<ReportItem>, pub dropped_items: u32 }
pub const FILLERS: [&str; 11] = ["um","uh","er","erm","hmm","like","you know","so basically","i mean","kind of","sort of"];
pub fn count_fillers(text: &str) -> std::collections::BTreeMap<String, u32>;
  // whole words, case-insensitive; "like" counts only when next to a comma ("so, like, maybe" / "like, I"); "kind of"/"sort of" not before "a|an|the"
pub fn word_count(text: &str) -> u32;
pub fn wpm(words: u32, speech_secs: f64) -> f64;                    // 0.0 when speech_secs < 1.0
pub fn long_pauses(segments: &[(f64, f64)], min_gap: f64) -> u32;   // gaps between consecutive speech segments inside one turn (not before the first or after the last)
pub fn looks_looped(text: &str) -> bool;                            // a 3-word sequence ≥ 4 times, or one word ≥ 6 times in a row
pub fn normalise(s: &str) -> String;                                // lowercase, keep letters/digits/apostrophes, collapse spaces
pub fn validate_items(items: Vec<ReportItem>, learner_turns: &std::collections::BTreeMap<i64, String>) -> (Vec<ReportItem>, u32);
  // keep an item only if its turn is a learner turn and normalise(quote) is non-empty and a substring of normalise(turn text); returns (kept, dropped)
pub fn schema() -> serde_json::Value; // {"type":"object","additionalProperties":false,"required":["grammar","unnatural","clarity"], each an array of
                                      //  {turn:int, quote:str, better:str, note:str[, echoed:bool for grammar]} with additionalProperties false}
pub fn items_from_json(v: &serde_json::Value) -> Result<Vec<ReportItem>, String>;
```
- [ ] **Step 1: Write the failing tests:**
  - `fillers_count_whole_words_only` ("umbrella" is not "um"; "I like it" has no "like"; "so, like, maybe" has one)
  - `kind_of_before_article_is_not_a_filler`
  - `wpm_and_pauses` (120 words / 60 s → 120.0; segments (0,2),(3.5,5),(5.2,6) with 1.0 → 1)
  - `loop_detector` ("the the the the the the" → true; normal text → false)
  - `invented_quotes_are_dropped` (quote absent → dropped; punctuation and case differences still match; a quote from a character turn → dropped)
  - `items_from_json_round_trip` (sample model JSON → 3 kinds, `echoed` defaults to false outside grammar)
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): report metrics and quote validation`.

## Phase 2 — Voice, audio and files

### Task 6: Supertonic model files

**Files:**
- Create: `practice/models.rs`
- Modify: `speakers/runtime.rs` (`fn open` → `pub(crate) fn open`; no other change), `lib.rs` setup (`practice::models::set_dir(data.join("models").join("practice"))` next to `:634`)

**Interfaces — Consumes:** `speakers::models::{ModelFile, download_one, download_client}`. **Produces:**
```rust
pub const REVISION: &str = "724fb5abbf5502583fb520898d45929e62f02c0b";
pub const FILES: [ModelFile; 16]; // flat names; url = format "https://huggingface.co/Supertone/supertonic-3/resolve/{REVISION}/{onnx|voice_styles}/{name}"
pub fn set_dir(dir: PathBuf); pub fn dir() -> Option<PathBuf>;
pub fn voices_present() -> bool;   // every file exists with the exact size (cheap; the hash is checked by download_one)
pub async fn ensure_voices(progress: impl FnMut(u64, u64)) -> Result<PathBuf, String>; // download_one per file (verified files are skipped), returns dir
```
Pinned files (bytes, sha256):

| name | bytes | sha256 |
|---|---|---|
| duration_predictor.onnx | 3700147 | c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db |
| text_encoder.onnx | 36416150 | c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff |
| vector_estimator.onnx | 256534781 | 883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c |
| vocoder.onnx | 101424195 | 085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba |
| tts.json | 8253 | 42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09 |
| unicode_indexer.json | 277676 | 9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f |
| F1.json | 292046 | bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2 |
| F2.json | 292423 | 7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6 |
| F3.json | 290794 | 12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab |
| F4.json | 291808 | c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b |
| F5.json | 291479 | 45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d |
| M1.json | 291748 | e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b |
| M2.json | 292055 | b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50 |
| M3.json | 290198 | ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b |
| M4.json | 291522 | ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303 |
| M5.json | 291469 | dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2 |

- [ ] **Step 1: Write the failing tests:**
  - `urls_are_pinned_to_the_revision` (every URL contains `REVISION`; https; `onnx/` for the 6 model files, `voice_styles/` for the 10 voices)
  - `voices_present_checks_sizes` (temp dir with one wrong-size file → false)
- [ ] **Step 2:** FAIL. **Step 3:** Implement.
- [ ] **Step 4:** PASS, then one live check: `PRACTICE_MODELS_DIR=/tmp/st cargo test -p meetily --lib practice::models::live_download -- --ignored` (an `#[ignore]` test that calls `ensure_voices` into that dir). Expected: all 16 files verified. **If any hash differs, stop and report:** the pinned revision is wrong.
- [ ] **Step 5:** Commit `feat(practice): pinned Supertonic voice files`.

### Task 7: Supertonic ONNX engine

**Files:**
- Create: `practice/tts.rs`, `frontend/src-tauri/tests/fixtures/practice/supertonic_durations.json`

**Interfaces — Consumes:** Task 2, Task 6, `speakers::runtime::{open, Backend}`. **Produces:**
```rust
pub struct Voice { ttl: ndarray::Array3<f32> /* [1,50,256] */, dp: ndarray::Array3<f32> /* [1,8,16] */ }
pub fn load_voice(path: &Path) -> Result<Voice, String>;          // JSON {style_ttl:{dims,data}, style_dp:{dims,data}}; dims checked
pub struct TtsConfig { pub sample_rate: u32, pub base_chunk_size: usize, pub chunk_compress_factor: usize, pub latent_dim: usize } // from tts.json: ae.sample_rate, ae.base_chunk_size, ttl.chunk_compress_factor, ttl.latent_dim
pub fn latent_shape(duration_secs: f32, c: &TtsConfig) -> (usize, usize); // (latent_dim*ccf, ceil(duration*sr / (base_chunk*ccf)))
pub struct Supertonic { /* 4 sessions, indexer, cfg, backend */ }
impl Supertonic {
  pub fn load(dir: &Path, prefer_directml: bool) -> Result<Self, String>;   // speakers::runtime::open for each graph; logs the backend used
  pub fn duration(&mut self, text: &str, voice: &Voice, speed: f32) -> Result<f32, String>; // dp graph only (deterministic)
  pub fn synthesize(&mut self, text: &str, voice: &Voice, steps: u32, speed: f32) -> Result<Vec<f32>, String>; // mono, TTS_RATE
}
```
**Graph I/O** (`supertonic/core.py` `Supertonic.__call__`, batch 1):
- **dp:** inputs `text_ids` i64 [1,T], `style_dp`, `text_mask` f32 [1,1,T] → `duration` [1]; divide by speed.
- **text_encoder:** inputs `text_ids`, `style_ttl`, `text_mask` → `text_emb`.
- **Noise:** `noisy_latent` is standard normal (Box–Muller over `rand`) with shape [1, `latent_shape`], multiplied by `latent_mask` [1,1,L] (1 for the first `ceil(wav_len/(base*ccf))` frames).
- **vector_estimator:** run `steps` times with `noisy_latent`, `text_emb`, `style_ttl`, `text_mask`, `latent_mask`, `current_step` f32 [1] = i, `total_step` f32 [1] = steps.
- **vocoder:** input `latent` → wav [1,N]. Trim to `duration * sample_rate` samples.

Text goes through `supported_only` → `preprocess(…, "en")` → `Indexer::ids`.

- [ ] **Step 1: Generate the duration fixture** with the reference SDK (same `uv run` pattern as Task 2). For the 7 Task 2 texts, dump `{text, voice:"M1", speed:1.05, duration}`. Take the SDK's loaded `supertonic.core.Supertonic` engine and run `text_processor([text], "en")` + `dp_ort.run(...)` exactly as in `__call__`, then divide by the speed.
- [ ] **Step 2: Write the failing tests:**
  - `latent_shape_math` (3.0 s, cfg 44100/512/6/24 → (144, 44))
  - `voice_json_dims_are_checked` (a wrong dims entry → Err)
  - `durations_match_reference` (`#[ignore]`, needs `PRACTICE_MODELS_DIR`; |rust − python| < 1e-3 for every fixture)
  - `synthesizes_finite_audio` (`#[ignore]`; "Good morning, please have a seat." → samples within ±0.02 s of duration × 44100, all finite, peak > 0.01)
- [ ] **Step 3:** FAIL. **Step 4:** Implement with `ort::inputs!` and `ndarray`, following `speakers/diarize.rs` for session runs. **Step 5:** PASS, including the ignored tests with `PRACTICE_MODELS_DIR=/tmp/st`.
- [ ] **Step 6:** Commit `feat(practice): Supertonic ONNX engine`.

### Task 8: Turn capture, `you.wav` and playback

**Files:**
- Create: `practice/audio.rs`
- Modify: `Cargo.toml` (move `hound = "3.5"` from dev-dependencies to dependencies)

**Interfaces — Consumes:** `dictate::capture::{choose_device, downmix_into, i16_to_f32, u16_to_f32}`, `audio::audio_processing::resample_audio`, `audio::recording_preferences::load_recording_preferences` + `parse_audio_device` (see `dictate/session.rs:499-503`). **Produces:**
```rust
pub struct TurnMic { /* cpal input thread → Sender<Vec<f32>> at device rate; open only while a session runs */ }
impl TurnMic { pub fn open(preferred: Option<String>) -> Result<(TurnMic, std::sync::mpsc::Receiver<Vec<f32>>, u32 /*rate*/), String>; pub fn set_recording(&self, on: bool); } // samples are sent only while recording is on
pub struct TurnRecorder { /* hound WavWriter, 16 kHz mono i16, samples_written */ }
impl TurnRecorder {
  pub fn create(path: &Path) -> Result<Self, String>;
  pub fn append(&mut self, a16: &[f32]) -> Result<(i64 /*offset*/, i64 /*len*/), String>; // writer.flush() after each append so the header stays valid
  pub fn finish(self) -> Result<(), String>;
}
pub fn read_span(path: &Path, offset: i64, len: i64) -> Result<Vec<f32>, String>;
pub struct PlayQueue { /* VecDeque<(u64 id, Vec<f32> at device rate)>, position */ }
impl PlayQueue { pub fn push(&mut self, id: u64, samples: Vec<f32>); pub fn clear(&mut self);
                 pub fn fill(&mut self, out: &mut [f32], channels: usize) -> Vec<u64> /* ids finished in this fill */; pub fn is_empty(&self) -> bool; }
pub struct Player { /* cpal default output, Arc<Mutex<PlayQueue>>, Sender<u64> finished ids */ }
impl Player { pub fn open() -> Result<(Player, std::sync::mpsc::Receiver<u64>), String>;
              pub fn play(&self, id: u64, samples_44k: Vec<f32>);  // resampled to the device rate before queueing
              pub fn stop_all(&self); }                             // clears at once; the next callback writes silence
```
- [ ] **Step 1: Write the failing tests:**
  - `recorder_offsets_accumulate_and_header_is_valid_after_each_append` (append 1.0 s and 0.5 s → offsets 0 and 16000; reopen with `hound::WavReader` after each append, spec ok, len matches)
  - `read_span_returns_the_turn`
  - `play_queue_reports_finished_ids_and_writes_silence_when_empty` (two buffers over several fills, stereo duplication)
  - `play_queue_clear_stops_immediately`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. The cpal parts follow `dictate/capture.rs`'s thread pattern (format handling for i16/u16/f32) and are checked on Windows in Task 19. **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(practice): turn capture, you.wav and playback`.

### Task 9: Session folders **[Codex]**

**Files:** Create `practice/files.rs`.

**Interfaces — Consumes:** `audio::meeting_folder::{guarded_meeting_folder, recycle, Remover, is_plain_dir}`, `audio::audio_processing::sanitize_filename`, `audio::recording_preferences::get_default_recordings_folder`. **Produces:**
```rust
pub const PRACTICE_FILES: [&str; 1] = ["you.wav"];
pub fn folder_name(started: chrono::DateTime<chrono::Local>, setup_name: &str) -> String; // "Practice 2026-10-20 18-05 Thesis defence" (sanitized, ≤ 80 chars)
pub fn create_session_folder(base: &Path, name: &str) -> Result<PathBuf, String>;       // directly inside base; " (2)", " (3)" … if taken
pub fn delete_session_folder(folder_path: &str, base: &Path, remove: Remover) -> Result<(), String>;
  // missing folder → Ok; guarded_meeting_folder None → Err("…outside the recordings folder; nothing was deleted");
  // any entry not a plain file named in PRACTICE_FILES → Err("…contains other files; nothing was deleted"); else remove(&[folder]) (one Recycle Bin call)
pub fn allow_audio<R: Runtime>(app: &AppHandle<R>, folder_path: &str) -> Result<Option<String>, String>; // guarded; allow_file(you.wav) if it exists; returns its path
```
- [ ] **Step 1: Write the failing tests** (fake `Remover` that records paths, as in `meeting_folder.rs` tests):
  - `folder_is_created_directly_inside_base_with_suffix_when_taken`
  - `delete_recycles_only_a_folder_holding_you_wav`
  - `a_foreign_file_blocks_delete_and_nothing_is_removed`
  - `a_subfolder_blocks_delete`
  - `a_folder_outside_base_or_a_symlink_is_refused` (unix symlink test like the existing ones)
  - `missing_folder_is_ok`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): session folders with guarded delete`.

## Phase 3 — Session and report

### Task 10: Session state machine

**Files:** Create `practice/session.rs`.

**Interfaces — Produces:**
```rust
#[derive(Clone, Copy, PartialEq, Debug, Serialize)] pub enum Phase { Loading, CharacterSpeaking, Thinking, UserTurn, Listening, Transcribing, Ended }
pub enum Event {
  Ready,                                   // engines loaded
  Tap, EndRequested, RecordingStarted, TurnTooLong,
  Tick { elapsed_secs: u64 },
  TurnTranscribed { text: String, speech_secs: f32 },
  ReplySentence { reply: u64, seq: u32, text: String },
  ReplyDone { reply: u64 }, ReplyFailed { reply: u64 },
  SentencePlayed { reply: u64, seq: u32 },
}
pub enum Effect {
  Phase(Phase), Notice(Notice),
  StartCapture, StopCaptureAndTranscribe,
  AskCharacter { reply: u64, wrap_up: bool },  // runner sends history + this request
  Speak { reply: u64, seq: u32, text: String }, StopSpeaking, CancelReply { reply: u64 },
  SaveUserTurn { text: String }, SaveCharacterTurn { text: String, interrupted: bool },
  End(EndReason),
}
#[derive(Clone, Copy, PartialEq, Debug, Serialize)] pub enum Notice { DidNotCatch, TenMinutesLeft, TurnCapped, ReplyFailed, MeetingStarted }
#[derive(Clone, Copy, PartialEq, Debug)] pub enum EndReason { User, TimeCap, MeetingRecording }
pub struct Core { /* phase, reply id counter, played sentences of the current reply, pending count, reply_done, warned, wrap_up */ }
impl Core { pub fn new() -> Self; pub fn on_event(&mut self, ev: Event) -> Vec<Effect>; pub fn phase(&self) -> Phase; }
```
**Rules:**
1. `Ready` → `AskCharacter{wrap_up:false}` (opening), `Phase(Thinking)`.
2. `ReplySentence` (current reply) → `Speak`, `Phase(CharacterSpeaking)`.
3. `SentencePlayed` → when `ReplyDone` arrived and all spoken sentences have played → `SaveCharacterTurn{played joined, interrupted:false}`, `Phase(UserTurn)`.
4. `ReplyDone` with zero sentences → `Phase(UserTurn)`.
5. `Tap` in `UserTurn` → `StartCapture`, `Phase(Listening)`. `Tap` in `Listening` or `TurnTooLong` → `StopCaptureAndTranscribe`, `Phase(Transcribing)` (+ `Notice(TurnCapped)` for `TurnTooLong`).
6. `Tap` in `Thinking`/`CharacterSpeaking` (barge-in) → `CancelReply`, `StopSpeaking`, `SaveCharacterTurn{played, interrupted:true}` only if some sentence played, `StartCapture`, `Phase(Listening)`.
7. `Tap` in `Loading`, `Transcribing` or `Ended` → nothing.
8. `TurnTranscribed`:
   - `speech_secs < MIN_SPEECH_SECS` or the text is blank → `Notice(DidNotCatch)`, `Phase(UserTurn)`;
   - else → `SaveUserTurn`, `AskCharacter{wrap_up}`, `Phase(Thinking)`.
9. `ReplyFailed` (current reply) → `SaveCharacterTurn` if any sentence played, `Notice(ReplyFailed)`, `Phase(UserTurn)`.
10. `Tick`:
    - ≥ `WARN_AT_SECS`, once → `Notice(TenMinutesLeft)`;
    - ≥ `WRAP_UP_AT_SECS` → `wrap_up` true for later asks;
    - ≥ `MAX_SESSION_SECS` → as `EndRequested` with `TimeCap`.
11. `EndRequested`/`RecordingStarted`:
    - if `Listening` → `StopCaptureAndTranscribe` first (that turn is saved, no reply follows);
    - `CancelReply` + `StopSpeaking` if a reply is active;
    - `SaveCharacterTurn` for played sentences;
    - `Notice(MeetingStarted)` for a recording;
    - `End(reason)`, `Phase(Ended)`.

    After `Ended`, every event → nothing, except a `TurnTranscribed` → `SaveUserTurn` when non-blank.
12. Events carrying a stale `reply` id → nothing.

- [ ] **Step 1: Write the failing tests:** one per rule.
  - `opening_asks_the_character`
  - `sentences_are_spoken_then_the_turn_passes`
  - `empty_reply_passes_the_turn`
  - `tap_starts_and_ends_listening`
  - `turn_cap_ends_listening_with_notice`
  - `barge_in_cancels_and_saves_played_part`
  - `barge_in_before_any_sentence_saves_nothing`
  - `taps_while_loading_transcribing_or_ended_do_nothing`
  - `double_tap_never_starts_two_captures`
  - `short_or_blank_turn_is_not_sent`
  - `reply_failure_keeps_played_sentences_and_returns_the_turn`
  - `warnings_and_wrap_up_at_50_and_55_minutes`
  - `time_cap_ends_and_saves_the_turn_in_progress`
  - `meeting_recording_ends_with_notice`
  - `stale_reply_events_are_ignored`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): session state machine`.

### Task 11: Session runner and commands **[Codex]**

**Files:**
- Create: `practice/runner.rs`, `practice/commands.rs`
- Modify:
  - `lib.rs`: register commands in `generate_handler!` after the speaker commands (`:963`); after the DB init at `:714`, spawn `practice::commands::recover_at_startup(app)`.
  - `dictate/mod.rs:60`: `keeps_model_loaded()` → `settings::current().enabled || crate::practice::holds_model()`.

**Interfaces — Consumes:** Tasks 1–10; `audio::transcription::engine::get_or_init_whisper`, `audio::common::acquire_engine_lifecycle_lock`, `dictate::{engine_is_whisper, unload_if_idle, whisper_engine}`, `audio::recording_commands::is_recording_now`, `audio::import::is_import_in_progress`, `audio::retranscription::is_retranscription_in_progress`, `audio::vad::ContinuousVadProcessor`, `WhisperEngine::transcribe_audio_with_confidence(a16, Some("en".into()), None, None)`.

**Produces (commands; all `Result<_, String>`; events below):**
```rust
#[tauri::command] pub async fn practice_status<R: Runtime>(app: AppHandle<R>) -> Result<PracticeStatus, String>;
  // { has_claude_key, engine_is_whisper, voices_ready, blocked: Option<String> /* recording/import/retranscription copy */, running_session: Option<String> }
pub struct SetupSummary { pub id: String, pub name: String, pub character: String, pub situation: String, pub voice: String,
  pub seeded: bool, pub has_plan: bool, pub materials: Vec<MaterialMeta>, pub session_count: i64 } // Serialize; no plan text, no material bytes
#[tauri::command] pub async fn practice_list_setups(..) -> Result<Vec<SetupSummary>, String>;
#[tauri::command] pub async fn practice_save_setup(.., input: SetupInput) -> Result<SetupSummary, String>;
#[tauri::command] pub async fn practice_delete_setup(.., id: String) -> Result<(), String>;
#[tauri::command] pub async fn practice_add_material(.., request: tauri::ipc::Request<'_>) -> Result<MaterialMeta, String>;
  // raw body bytes; headers "x-setup-id", "x-name" (URI-encoded), "x-kind" pdf|text; text must be valid UTF-8; a PDF must start with "%PDF-"
#[tauri::command] pub async fn practice_add_text(.., setup_id: String, name: String, text: String) -> Result<MaterialMeta, String>;
#[tauri::command] pub async fn practice_delete_material(.., id: String) -> Result<(), String>;
#[tauri::command] pub async fn practice_new_questions(.., setup_id: String) -> Result<(), String>;  // set_plan(None, None)
#[tauri::command] pub async fn practice_download_voices(..) -> Result<(), String>;                  // emits "practice-voices-progress" {percent}
#[tauri::command] pub async fn practice_preview_voice(.., voice: String) -> Result<(), String>;    // "Hello, I'm looking forward to our conversation." (refused while a session runs)
#[tauri::command] pub async fn practice_start(.., setup_id: String, skip_plan: bool) -> Result<String /*session id*/, String>;
#[tauri::command] pub async fn practice_cancel_start(..) -> Result<(), String>;
#[tauri::command] pub async fn practice_tap(..) -> Result<(), String>;
#[tauri::command] pub async fn practice_end(..) -> Result<(), String>;
pub async fn recover_at_startup<R: Runtime>(app: AppHandle<R>); // db::recover_interrupted
```
**Events:**
- `"practice-state"` `{ session_id, phase, character, elapsed_secs, caption: Option<String>, notice: Option<Notice> }`
- `"practice-turn"` `{ session_id, idx, speaker, text }`
- `"practice-prep"` `{ stage: "plan"|"loading"|"failed", error: Option<String> }`

**Start flow (`practice_start`):**
1. **Refuse when:**
   - another session runs;
   - `blocked` is set;
   - there is no key;
   - the engine isn't Whisper (exact copy from Global Constraints).
2. **Plan:** if the setup has materials and `plan_fingerprint != plan_fingerprint(situation, materials)` and `!skip_plan`:
   - emit `practice-prep {stage:"plan"}`;
   - `claude::stream(plan_body(…))` under a cancel token held by `practice_cancel_start`;
   - `set_plan`.
   - On error → `practice-prep {stage:"failed", error}` and `Err`. The UI offers Retry and "Start without questions" (`skip_plan = true`).
3. **Load:**
   - emit `practice-prep {stage:"loading"}`;
   - take a `hold_model()` guard **before** loading (kept by the runner until the end);
   - under `acquire_engine_lifecycle_lock`: `get_or_init_whisper`;
   - `ensure_voices`;
   - `Supertonic::load(dir, true)` and `load_voice`;
   - `TurnMic::open(preferred)`, `Player::open()`;
   - `create_session_folder`, `TurnRecorder::create(folder/you.wav)`, `db::create_session`.

   Any failure → undo (drop the hold, `unload_if_idle`, remove the empty folder with `std::fs::remove_dir` only if empty) and `Err`.
4. **Run:** spawn the runner task. Return the id.

**Runner (one tokio task, `mpsc` of `Event`s, executes `Core` effects):**
- **Timers:** a 1 s ticker sends `Tick`, and `RecordingStarted` when `is_recording_now()`. A turn timer sends `TurnTooLong` after `MAX_TURN_SECS` of listening.
- **`StartCapture`:**
  - `mic.set_recording(true)`;
  - a capture task resamples chunks to 16 kHz, keeps the whole turn, and feeds `ContinuousVadProcessor::new(16000, LIVE_VAD_REDEMPTION_MS)`;
  - each finished segment is transcribed at once on a blocking task (English forced, no prompt), and its text is kept in order.
- **`StopCaptureAndTranscribe`:**
  - `set_recording(false)`, `flush()` the VAD, transcribe the rest;
  - join the segment texts;
  - `TurnRecorder::append(whole turn)` → offset/len;
  - send `TurnTranscribed { text, speech_secs: sum of segment lengths }`.
- **`AskCharacter`:**
  - build `chat_body(character_system(persona, seed), history + optional WRAP_UP_SYSTEM)`;
  - `claude::stream` with a cancel token; feed deltas to a `SentenceSplitter`; send `ReplySentence` for each; at the end `finish()` → last sentence, then `ReplyDone`;
  - error → `ReplyFailed`;
  - a 20 s first-byte timeout counts as `ReplyFailed`;
  - `seed` = session start time in seconds.
- **`Speak`:** `tts.synthesize(text, voice, TTS_STEPS, TTS_SPEED)` on a blocking task, in sentence order → `player.play(seq_id, wav)`. A finished id → `SentencePlayed`.
- **`SaveUserTurn` / `SaveCharacterTurn`:**
  - `db::add_turn` (user turns carry `wav_offset`/`wav_len` and `at_secs` = session time at the turn start);
  - emit `practice-turn`;
  - push to the chat history. An interrupted character turn gets `" —"` appended. A user turn following a user turn is allowed.
- **`Phase` / `Notice`:** emit `practice-state`. The caption is the last played character sentence.
- **`End(reason)`:**
  - stop the mic and the player; `TurnRecorder::finish`;
  - `db::end_session(duration, reason)`;
  - drop the TTS;
  - drop the hold, then `dictate::unload_if_idle()`;
  - spawn `report_job::run(app, session_id)` (Task 12);
  - emit the final `practice-state`.
- **Latency log per turn** (no texts): `info!("practice: turn stt_ms={} first_token_ms={} first_sentence_ms={} first_audio_ms={}")`, measured from the tap that ended the user turn.

- [ ] **Step 1: Write the failing tests** (pure helpers only):
  - `start_refusals_use_exact_copy` (a `fn start_blocker(status: &PracticeStatus) -> Option<&'static str>`)
  - `plan_is_rebuilt_only_when_stale` (`fn needs_plan(setup, materials) -> bool`: no materials → false; fingerprint equal → false; changed → true)
  - `add_material_rejects_non_pdf_bytes_and_bad_utf8` (`fn check_material(kind, bytes) -> Result<(), String>`)
  - `history_marks_interrupted_turns` (`fn push_history(…)`)
- [ ] **Step 2:** FAIL. **Step 3:** Implement the runner and commands; register them.
- [ ] **Step 4:** PASS, and the full Rust check (Global Constraints) PASS.
- [ ] **Step 5:** Commit `feat(practice): session runner and commands`.

### Task 12: Report job

**Files:**
- Create: `practice/report_job.rs`
- Modify: `practice/commands.rs` (add `practice_retry_report`, `practice_list_sessions`, `practice_get_session`)

**Interfaces — Consumes:** Tasks 1, 3, 4, 5, 8; `audio::vad::get_speech_chunks`; `WhisperEngine::transcribe_audio_with_confidence(a16, Some("en".into()), None, Some(VERBATIM_PROMPT))`. **Produces:**
```rust
pub const VERBATIM_PROMPT: &str = "Um, uh, so, like, I mean, I- I was, uh, thinking that, you know, we, we had it.";
pub async fn run<R: Runtime>(app: AppHandle<R>, session_id: String);  // never panics; ends in report_status 'ready' or 'failed'
pub fn build_report(fluency: Fluency, items: Vec<ReportItem>, learner_turns: &BTreeMap<i64, String>) -> Report; // validate_items + dropped count
#[tauri::command] pub async fn practice_retry_report(.., session_id: String) -> Result<(), String>;   // only when 'failed' or 'pending' and not running
#[tauri::command] pub async fn practice_list_sessions(..) -> Result<Vec<SessionSummary>, String>;     // { id, setup_name, started_at, duration_secs, report_status, summary: Option<String> /* "4 grammar · 31 fillers" */ }
#[tauri::command] pub async fn practice_get_session(.., id: String) -> Result<SessionDetail, String>; // { summary, character, turns, report: Option<Report>, report_error, audio_path: Option<String> /* files::allow_audio */ }
```
**Flow:**
1. **Wait for meetings:** while `is_recording_now()` → status `'waiting'`, re-check every 10 s.
2. **Start:** status `'running'` (emit `"practice-report"` `{session_id, status}` on every change).
3. **Word-for-word pass:** load Whisper (the same lifecycle rules as Task 11; a `hold_model()` guard for the duration of the pass, then drop it + `unload_if_idle`). For each user turn with a WAV span:
   - `read_span`;
   - transcribe with `VERBATIM_PROMPT` → if `looks_looped` use `live_text`;
   - `set_verbatim`;
   - `get_speech_chunks(&span, 300)` → segments → speech seconds and `long_pauses(…, LONG_PAUSE_SECS)`.
4. **Metrics:** fluency over all user turns: fillers summed, words, speech seconds, WPM, pauses.
5. **Opus report:** `report_prompts` over (idx, speaker, verbatim or live text) → `claude::stream(report_body(…, schema()))` → parse JSON → `items_from_json` → `build_report` → `set_report('ready', json)`.
6. **Failure:** any error → `set_report('failed', None, user_message)`.

- [ ] **Step 1: Write the failing tests:**
  - `build_report_drops_invented_quotes_and_counts_them`
  - `summary_line_formats_counts` (`fn summary_line(&Report) -> String` → "4 grammar · 31 fillers"; zero grammar → "0 grammar · 3 fillers")
  - `malformed_model_json_fails_the_report` (`items_from_json` on `{"grammar":"x"}` → Err; the job maps it to `'failed'` via a pure `fn finish(result) -> (status, json, error)`)
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): report job`.

### Task 13: Delete session command **[Codex]**

**Files:** Modify `practice/commands.rs`.

```rust
#[tauri::command] pub async fn practice_delete_session(.., id: String) -> Result<(), String>;
// refuse while it is the running session or its report is 'running';
// files::delete_session_folder(folder, &get_default_recordings_folder(), &recycle) FIRST; on Err keep the rows and return the error;
// then db::delete_session_rows
```
- [ ] **Step 1: Failing test:** `folder_error_keeps_the_rows` (an inner `fn delete_session_inner(pool, id, base, remove)` with a remover that fails → rows still there; success → rows gone).
- [ ] **Step 2:** FAIL. **Step 3:** Implement and register. **Step 4:** PASS. **Step 5:** Commit `feat(practice): delete a session to the Recycle Bin`.

## Phase 4 — Practice page

### Task 14: Types, list view, setup editor and sidebar entry

**Files:**
- Create: `frontend/src/lib/practice.ts`, `frontend/src/app/practice/page.tsx`, `frontend/src/components/Practice/{SetupList,SessionList,SetupEditor}.tsx`
- Modify: `frontend/src/components/Sidebar/index.tsx` (collapsed entry after the Dictate tooltip `:472-482`, expanded entry after `:749-760`, icon `GraduationCap`, label "Practice", route `/practice`)
- Test: `frontend/tests/lib/practice.test.ts`, `frontend/tests/components/practice-page.test.tsx` (mock pattern of `dictate-page.test.tsx`)

**Interfaces — Produces (`lib/practice.ts`):** TS mirrors of `SetupSummary`, `MaterialMeta`, `SessionSummary`, `SessionDetail`, `Report`, `ReportItem`, `Fluency`, `PracticeStatus`, `Phase`, `Notice`, `PracticeStatePayload`, plus:
```ts
export const VOICES: { id: string; label: string }[];          // same 10 as Rust
export function formatClock(secs: number): string;             // 372 → "06:12", 3600 → "60:00"
export function materialKind(fileName: string): 'pdf' | 'text' | null; // .pdf | .txt/.md | else null
export function fitsLimit(existingBytes: number, addBytes: number): boolean; // ≤ 20 MB
export const NOTICE_TEXT: Record<Notice, string>;              // exact copy (Global Constraints)
export function phaseText(phase: Phase, name: string): string; // exact copy
```
**Page:**
- `view` state is `{kind:'list'} | {kind:'edit', id?} | {kind:'call', sessionId} | {kind:'report', sessionId}`.
- **List view:** `grid gap-4 lg:grid-cols-2` with two `bg-card` cards ("Your setups" with Edit / ▶ Start / "+ New setup"; "Past sessions" with summary line, Open report, Delete with `window.confirm`), centred `max-w-6xl`.
- **Status banner:** from `practice_status` (blocked copy, no key, not Whisper) above the grid. ▶ Start is disabled while blocked.
- **Start:** if `voices_ready` is false, first `practice_download_voices` with the progress copy. Then `practice_start`, while the "Preparing…/Loading…" overlay listens to `practice-prep`, with Cancel → `practice_cancel_start`. A plan failure shows Retry and "Start without questions".
- **Editor** (in place, `bg-card`, `max-w-3xl`): Name, Character, The situation (textarea), Material chips with ✕, "+ Add file" (`<input type=file accept=".pdf,.txt,.md" multiple>` → ArrayBuffer → `invoke('practice_add_material', bytes, {headers})`), "Paste text" (textarea dialog → `practice_add_text`), Voice `Select` with ▶ preview, "New questions" (only when the setup has materials), Save / Cancel / Delete setup.

- [ ] **Step 1: Write the failing tests:**
  - lib: `formatClock`, `materialKind`, `fitsLimit` boundaries, `phaseText` copy
  - page: `lists_setups_and_sessions_side_by_side` (two cards rendered)
  - `start_is_disabled_with_the_blocked_reason`
  - `editor_validates_name_and_saves` (`practice_save_setup` called with the trimmed input)
  - `oversized_file_is_refused_before_upload`
  - `delete_session_asks_first`
- [ ] **Step 2:** `cd frontend && bun test tests/lib/practice.test.ts tests/components/practice-page.test.tsx` → FAIL.
- [ ] **Step 3:** Implement with the theme tokens (`bg-card`, `border-border`, `text-muted-foreground`, `Button`, `Select` from `components/ui`).
- [ ] **Step 4:** PASS; `pnpm exec tsc --noEmit` PASS.
- [ ] **Step 5:** Commit `feat(practice): Practice page, setups and editor`.

### Task 15: Call view

**Files:**
- Create: `frontend/src/components/Practice/CallView.tsx`
- Test: extend `practice-page.test.tsx`

**Behaviour (spec §4.2):**
- **Header:** setup name · `formatClock(elapsed)` and "End session → report" (`practice_end`).
- **Centre:**
  - an avatar circle with the character's initial, its border in `primary`;
  - the name and `phaseText`;
  - the caption card;
  - a big primary button "● Tap to talk" with a "Space" hint, calling `practice_tap`;
  - the notice line (`NOTICE_TEXT`), cleared on the next phase;
  - "Show full transcript ▾" toggling a list of `practice-turn` events.
- **Keyboard:** `window` keydown Space (`!e.repeat`, not in an input/textarea/contenteditable) → `practice_tap`, `preventDefault`.
- **End:** on `phase === 'Ended'` → switch to the report view.

- [ ] **Step 1: Write the failing tests:**
  - `space_taps_once_per_press` (repeat ignored; Space inside a textarea ignored)
  - `phase_and_notice_copy_render` (`practice-state` payloads → texts)
  - `ten_minutes_left_notice_shows`
  - `ended_switches_to_report`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): call view`.

### Task 16: Report view

**Files:**
- Create: `frontend/src/components/Practice/ReportView.tsx`
- Test: extend `practice-page.test.tsx` with a fixture `Report`

**Behaviour (spec §4.3):**
- **Layout:** `max-w-4xl mx-auto`. Header: setup name · date · length. Tabs Report / Transcript.
- **Stats strip:** three tiles: fillers (`filler_total`), words/min (rounded), long pauses.
- **Cards, one column:**
  - **Grammar (n):** the quote with the changed part struck through (simple word diff between `quote` and `better`, `line-through text-destructive`), the better version in `text-emerald-500`, the note, "▶ mm:ss", and a "{character} echoed this" badge when `echoed`;
  - **Sounds unnatural (n);**
  - **Could be clearer (n):** quote, better, note;
  - **Fillers and fluency:** the per-filler breakdown "um 14 · so basically 9 · like 8".
- **▶ playback:** one hidden `<audio src={convertFileSrc(audio_path)}>`. ▶ seeks to `wav_offset/16000` of that turn, plays, and pauses at `(wav_offset+wav_len)/16000`. The label shows the turn's `at_secs` as mm:ss.
- **Status:** `report_status` `waiting` → "Waiting for the meeting recording to finish…"; `running` → "Writing your report…" (listen to `practice-report`); `failed` → the error + Retry (`practice_retry_report`).
- **Transcript tab:** all turns, speaker and time; user turns show `verbatim_text ?? live_text`.

- [ ] **Step 1: Write the failing tests:**
  - `stats_strip_and_cards_render_counts`
  - `echoed_badge_only_when_echoed`
  - `play_button_seeks_to_turn_offset`
  - `failed_report_offers_retry`
  - `transcript_prefers_verbatim_text`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS; the full frontend check PASS. **Step 5:** Commit `feat(practice): report view`.

## Phase 5 — Docs, Windows checkpoint, release

### Task 17: Docs and the removal checklist

**Files:** `docs/network-calls.md`, `AGENTS.md`, `practice/mod.rs` (module doc comment).

**`docs/network-calls.md`:**
- a Hosts row for `huggingface.co` → `src-tauri/src/practice/models.rs` (Supertonic 3, pinned revision, SHA-256 verified, downloaded on first use), "Allowed (model host)";
- a Runtime-built URLs row for `practice/claude.rs` → `api.anthropic.com/v1/messages` (user's own key; text and the user's materials only, never audio), "Allowed (user-configured; no new hosts)".

**`AGENTS.md`:** add a Layout row: "`frontend/src-tauri/src/practice/` | Speaking Practice: role-play sessions, Supertonic voice, report".

**Removal checklist** (top of `practice/mod.rs`):
1. delete the paths in Global Constraints;
2. `lib.rs`: the `mod practice;` line, the `practice_*` handlers, the `set_dir` and `recover_at_startup` lines;
3. `dictate/mod.rs`: the `|| crate::practice::holds_model()` clause;
4. `speakers/runtime.rs`: `open` back to private (optional);
5. Sidebar: the two Practice entries;
6. `Cargo.toml`: `base64`, `unicode-normalization` (and `hound` back to dev-dependencies) if unused elsewhere;
7. a new migration `DROP TABLE practice_turns; DROP TABLE practice_sessions; DROP TABLE practice_materials; DROP TABLE practice_setups;` (never edit the old one);
8. the two `docs/network-calls.md` rows and the AGENTS.md row;
9. the user's session folders stay in `conclave-recordings`, for them to delete.

- [ ] **Step 1:** Edit. **Step 2:** `bash scripts/check-no-phone-home.sh` → OK; full check (Global Constraints) → PASS. **Step 3:** Commit `docs(practice): network calls, layout, removal checklist`.

### Task 18: Windows CI checkpoint (manual dispatch)

- [ ] **Step 1:** Push the branch. **Step 2:** `gh workflow run ci.yml --ref practice -f full=true`. **Step 3:** Expected: all jobs green. Fix and re-run only failed jobs.

### Task 19: Release v0.8.0 and the user's first-session checks

- [ ] **Step 1:** After the whole-branch review (CLAUDE.md), merge to `main` and tag `v0.8.0`. The release run's delay-load and no-CUDA checks pass; publish.
- [ ] **Step 2:** Give the user this checklist, which covers spec §7, and record the results in memory `conclave-speaking-practice`:
  1. The first ▶ preview downloads the voices with progress. Then all 10 voices preview, and each setup keeps its own voice.
  2. **Thesis defence with a real PDF:** "Preparing…" appears once. A second Start skips it. "New questions" brings it back.
  3. **Latency:** read the `practice: turn …` log lines. Is the tap → first word time about 1–1.5 s, including after a 2-minute answer? Is the TTS backend DirectML or CPU? If too slow, try `TTS_STEPS = 5`, then the Sonnet fallback (spec §7).
  4. **Barge-in:** tap while the character speaks. It stops at once, and the transcript shows only what was said.
  5. **Ending:** 50:00 shows "10 minutes left", 55:00 starts the wrap-up, and 60:00 ends the session (shorten the constants in a debug build to check).
  6. **Meeting:** start a Zoom recording mid-session. The session ends, and the report waits until the recording stops.
  7. **Report:** mistakes made on purpose show up with your real words, and ▶ plays them. Fillers are counted. No invented quotes.
  8. **Delete:** a past session goes to the Recycle Bin; a setup delete keeps its past sessions.
  9. **With Dictate on:** practice and dictation both still work, and Whisper stays loaded after a session.
