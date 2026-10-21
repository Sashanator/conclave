# Conclave Speaking Practice, phase 2 (ships with phase 1 as v0.8.0) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local pronunciation hints, "Keeps coming back" (recurring mistakes and filler trend) and a Models settings card to the Practice feature built in phase 1.

**Architecture:**
- **New pure modules in `practice/`:**
  - `pron_dict.rs`: CMUdict parsing and word tokenising.
  - `pron_align.rs`: CTC forced alignment and GOP scoring.
  - `recurring.rs`: pattern counts and the filler trend.
  - `settings.rs`: model settings (tauri-store `practice.json`).
- **New engine wrapper:** `pron.rs` runs the HuBERT phoneme ONNX model on CPU.
- **Changed:** the report job gains a pronunciation pass and pattern keys (report JSON v2). The Claude request bodies take the model and effort from the settings.
- **UI:** a Models card, plus two report cards (Pronunciation hints, Keeps coming back).

**Tech Stack:** as in phase 1 (Rust, `ort` 2.0.0-rc.10 CPU EP, ndarray 0.16, tauri-plugin-store, Next.js/React, Bun tests). No new crates.

**Spec:** `docs/superpowers/specs/2026-10-20-conclave-speaking-practice-design.md` §6 (phase 2 details, 2026-10-21). Research notes: `.superpowers/sdd/2026-10-20-conclave-speaking-practice/phase2-pronunciation-research.md`. The IPC contract (phase 1) is `.superpowers/sdd/2026-10-20-conclave-speaking-practice/wire-types.md`; this plan's additions are in Global Constraints.

## Global Constraints

- **Unchanged from phase 1:**
  - The phase 1 plan's Global Constraints still apply: builds, self-contained code, touch points, no telemetry, privacy, and Codex on risky tasks.
  - Branch `practice`, worktrees `/workspace/.worktrees/practice-p2-tN`.
  - Build: `CARGO_TARGET_DIR=/workspace/target`, `nice -n 15 cargo test -j 4`.
- **Pronunciation constants (`practice/mod.rs`):**
  - `PRON_THRESHOLD: f32 = -6.5`, `PRON_MAX_HINTS: usize = 6`, `PRON_MIN_PHONES: usize = 3` (skip words of 2 phones or fewer)
  - `PRON_MAX_CHUNK_SECS: f64 = 20.0`, `PRON_FRAME_SECS: f64 = 0.02`, `PRON_PAD_SECS: f64 = 0.08` (▶ you padding)
  - `PRON_VARIANT_MARGIN_FRAMES: usize = 4`
  - `RECURRING_SESSIONS: usize = 5`, `RECENT_PATTERNS_MAX: usize = 30`
- **Pinned files:**
  - `hubert_phoneme_int8.onnx`: `https://huggingface.co/ron-jo/hubert-base-phoneme-en-onnx/resolve/bbc279477648b3b7de4ec1375707d041617dbd3e/hubert_phoneme_int8.onnx`, 121951866 bytes, sha256 `af7a194be47549c8cfe0055220e52498b270773ad3d77483e0e0c9dd101a0dbb`.
  - `cmudict.dict`: `https://raw.githubusercontent.com/cmusphinx/cmudict/74790861f652b15e4ac49015a90074ad62a27690/cmudict.dict`, 3618488 bytes, sha256 `81917843c7f44ce2b094ac63873c2c7a4cf802040792c455ba3ca406891c3d22`.
- **Model I/O:**
  - Input `input_values` f32 [1, samples], 16 kHz, zero-mean/unit-variance normalised per chunk.
  - Output `logits` [1, frames, 43] with 20 ms frames.
  - Ids 0–38 are `AA AE AH AO AW AY B CH D DH EH ER EY F G HH IH IY JH K L M N NG OW OY P R S SH T TH UH UW V W Y Z ZH`; 39 `[UNK]`; 40 blank (`[PAD]`); 41/42 `<s>`/`</s>`.
  - Log-softmax over all 43. GOP's max is taken over ids 0–38 only.
- **Wire additions** (snake_case, like phase 1; update `wire-types.md` in Task 1):
  ```ts
  type ReportItem = { kind; turn; quote; better; note; echoed; pattern: string | null };   // pattern: grammar/unnatural
  type PronHint = { turn: number; word: string; start_secs: number; end_secs: number; score: number; phone: string };
  type Report = { version: number /* 2 */; fluency; items; dropped_items; pronunciation: PronHint[];
                  pronunciation_note: string | null /* e.g. "Pronunciation hints unavailable" */ };
  type Recurring = { patterns: { key: string; sessions: number; same_setup: boolean }[]; filler_trend: number[] };
  type SessionDetail = { …phase 1 fields…; recurring: Recurring | null };
  type PracticeSettings = { chat_model: 'claude-haiku-5-5' | 'claude-sonnet-5-5' | 'claude-opus-5-5';
                            plan_model: 'claude-opus-5-5' | 'claude-sonnet-5-5'; plan_effort: 'medium' | 'high' | 'xhigh' };
  // commands
  practice_get_settings → PracticeSettings
  practice_save_settings {settings} → PracticeSettings
  practice_speak_word {sessionId, word} → ()     // reference pronunciation with the session's voice
  ```
  - `start_secs` and `end_secs` are relative to the start of the turn's audio in you.wav, already padded by `PRON_PAD_SECS` and clamped to the turn.
  - Version-1 reports deserialise with `pattern: null`, `pronunciation: []` and `pronunciation_note: null` (`#[serde(default)]`).
- **Exact copy:**
  - "Pronunciation hints (may be off)", "Pronunciation hints unavailable", "No pronunciation hints this time"
  - "Keeps coming back", "Fillers per minute"
  - "Models", "Conversation", "Plan and report", "Effort", "Slower replies", "≈ ${cost} per 30-min session"
- **Codex pass** on tasks marked **[Codex]**.

## Review Focus

1. **Words not in CMUdict, digits, names and contractions** ("don't", "I'm", "COVID", "2026", "Weber") are skipped or split around, never aligned to garbage or flagged. Tests in Task 2 and Task 4.
2. **Very short or long segments.** A segment with fewer frames than 2N+1 is skipped. A segment over 20 s is skipped for pronunciation (logged), never run at O(T²) memory. Tests in Task 3 and Task 4.
3. **Old reports (version 1) and reports without hints** still open, and every new card degrades gracefully. Tests in Tasks 5, 6 and 9.
4. **The model download fails or is offline.** The report is still written, with `pronunciation_note`. Test in Task 5.
5. **A settings file holding an unknown model or effort** (hand-edited, or an older or newer app) falls back to the defaults and never sends an invalid request. Test in Task 7.

## File Structure

| Path | Responsibility |
|---|---|
| `practice/models.rs` (modify) | `PRON_FILES` + `ensure_pron_models` |
| `practice/pron_dict.rs` | CMUdict parse, phone ids, word tokeniser |
| `practice/pron_align.rs` | log-softmax, CTC Viterbi, variant pick, GOP, word scores |
| `practice/pron.rs` | HuBERT ONNX session (CPU), chunk normalise, run, hints for a segment |
| `practice/report.rs`, `prompts.rs`, `report_job.rs`, `db.rs` (modify) | Report v2, pattern keys, pronunciation pass, recent patterns query |
| `practice/recurring.rs` | Pure recurring patterns + filler trend |
| `practice/settings.rs` | PracticeSettings store + validation; `claude.rs` bodies take settings |
| `practice/commands.rs` (modify) | settings, speak_word commands; `recurring` in SessionDetail |
| `frontend/src/lib/practice.ts`, `components/Practice/{ModelsCard,ReportView}.tsx`, `app/practice/page.tsx` | UI |

---

### Task 1: Pronunciation model files and network docs

**Files:**
- Modify: `practice/models.rs`, `practice/mod.rs` (constants)
- Modify: `docs/network-calls.md` (a huggingface.co row for the HuBERT file and a raw.githubusercontent.com row for CMUdict, both "Allowed (model host)", downloaded on first report)
- Modify: `.superpowers/sdd/2026-10-20-conclave-speaking-practice/wire-types.md` (add the Wire additions above)

**Produces:**
```rust
pub const PRON_FILES: [ModelFile; 2];                     // exact values from Global Constraints
pub fn pron_present() -> bool;                            // sizes only, like voices_present
pub async fn ensure_pron_models(progress: impl FnMut(u64, u64)) -> Result<PathBuf, String>; // download_one each
```
- [ ] **Step 1: Write the failing tests:**
  - `pron_urls_are_pinned` (exact URLs, https, revisions in the URL)
  - `pron_present_checks_sizes`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, plus one live `#[ignore]` download into `/tmp/practice-pron` (both hashes must verify; stop and report BLOCKED on a mismatch).
- [ ] **Step 5:** Commit `feat(practice): pinned pronunciation model files`.

### Task 2: CMUdict parser and word tokeniser

**Files:** Create `practice/pron_dict.rs`.

**Produces:**
```rust
pub const PHONES: [&str; 39];                    // AA … ZH in id order 0..=38
pub fn phone_id(arpabet: &str) -> Option<u8>;    // stress digits stripped
pub struct PronDict { /* HashMap<String, Vec<Vec<u8>>> */ }
impl PronDict {
  pub fn parse(text: &str) -> PronDict;           // lines "word phones…", "word(2) …", "# comment" stripped; lowercased keys; unknown phones → line skipped
  pub fn variants(&self, word: &str) -> Option<&[Vec<u8>]>;
}
pub fn words(text: &str) -> Vec<String>;          // lowercase; strip surrounding punctuation; keep internal apostrophes; split on whitespace and dashes
```
- [ ] **Step 1: Write the failing tests:**
  - `parses_variants_and_strips_stress_and_comments`, with a 6-line sample including `read(2)` and a `#` comment
  - `phone_ids_follow_the_model_order` (`AA`→0, `ZH`→38, `AH0`→2)
  - `words_keep_contractions_and_strip_punctuation` (`"Don't, I'm (sure) — 2026!"` → `["don't","i'm","sure","2026"]`)
  - `unknown_word_returns_none` (`"weber"` absent from the sample)
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): CMUdict parser and tokeniser`.

### Task 3: Forced alignment and GOP (pure)

**Files:** Create `practice/pron_align.rs`.

**Produces:**
```rust
pub const BLANK: usize = 40; pub const VOCAB: usize = 43;
pub fn log_softmax(logits: &[f32], frames: usize) -> Vec<f32>;    // row-wise over VOCAB
pub struct Aligned { pub phone_spans: Vec<(usize, usize)>, pub score: f32 } // inclusive frame spans per expected phone; total path log-prob
pub fn viterbi(logp: &[f32], frames: usize, phones: &[u8]) -> Option<Aligned>; // None if frames < 2N+1 or N == 0
pub struct WordScore { pub index: usize, pub start: usize, pub end: usize, pub score: f32, pub worst_phone: u8 }
pub fn score_words(logp: &[f32], frames: usize, words: &[Vec<Vec<u8>>]) -> Option<Vec<WordScore>>;
  // words[i] = that word's variants; first alignment with variant 0 of each word; for words with >1 variant re-run viterbi on
  // the word's span ± PRON_VARIANT_MARGIN_FRAMES per variant and keep the best path score; re-align with the chosen variants;
  // GOP(p) = mean over its frames of (logp[p] − max_{q in 0..39} logp[q]); word score = min GOP, worst_phone = its argmin
```
**States:** `blank, p1, blank, p2, …, pN, blank`. Transitions: stay; +1; +2 when the skipped state is a blank and p(s) ≠ p(s−2). The path ends in the last phone or the final blank.

- [ ] **Step 1: Write the failing tests** (synthetic log-prob matrices built in the test):
  - `viterbi_recovers_known_spans` (3 phones with clear peaks at known frames → exact spans)
  - `viterbi_handles_repeated_phones` (the same phone twice needs a blank between)
  - `too_few_frames_returns_none`
  - `gop_is_zero_when_expected_phone_wins_and_drops_when_swapped`
  - `variant_pick_prefers_the_matching_pronunciation` (a word with 2 variants, the matrix favours variant 2 → its GOP is about 0)
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): CTC forced alignment and GOP scoring`.

### Task 4: Pronunciation engine

**Files:**
- Create: `practice/pron.rs`
- Test: ignored model tests use `PRACTICE_PRON_DIR` (set by Task 1's live run)

**Consumes:** Tasks 1–3, `ort` (CPU EP, with intra-op threads as in `speakers::runtime` CPU sessions). **Produces:**
```rust
pub struct PronEngine { /* Session (CPU), PronDict */ }
impl PronEngine {
  pub fn load(dir: &Path) -> Result<Self, String>;  // hubert_phoneme_int8.onnx + cmudict.dict
  pub fn segment_hints(&mut self, samples16k: &[f32], text: &str) -> Result<Vec<SegmentHint>, String>;
}
pub struct SegmentHint { pub word: String, pub start_secs: f64, pub end_secs: f64, pub score: f32, pub phone: String } // relative to the segment start
```
**`segment_hints` steps:**
1. If the segment is longer than `PRON_MAX_CHUNK_SECS` or `text` is blank, return `Ok(vec![])` and log the length only.
2. Normalise to zero mean and unit variance, run the model, then `log_softmax`.
3. Run `words(text)` → split into runs at out-of-vocabulary words → drop runs shorter than 2 words.
4. Run `score_words` on each run, using that run's frame window. Default: the whole segment for a single run. With several runs, align them in order inside the segment; the implementer may simply align each run against the whole segment's frames with the other runs' phones omitted, and document the choice.
5. Keep words where `score < PRON_THRESHOLD` and the phone count ≥ `PRON_MIN_PHONES`.
6. Convert frames to seconds (`× PRON_FRAME_SECS`) and pad by `PRON_PAD_SECS`, clamped to [0, segment length].

- [ ] **Step 1: Write the failing tests:**
  - `oov_runs_are_split_and_short_runs_dropped`, using a pure helper `fn runs(words, dict) -> Vec<Vec<(usize, &[Vec<u8>])>>`
  - `long_segment_is_skipped`
  - `#[ignore] real_model_outputs_43_logits_per_20ms` (2 s of silence → frames ≈ 100 ± 2, 43 columns)
  - `#[ignore] native_tts_sentence_has_no_hints`: synthesise "Good morning, please have a seat." with Supertonic M1 (Task 7 of phase 1, `PRACTICE_MODELS_DIR`), resample to 16 kHz → zero hints
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, including the ignored tests with the env vars. **Step 5:** Commit `feat(practice): pronunciation engine`.

### Task 5: Report v2 — pronunciation pass and pattern keys **[Codex]**

**Files:** Modify `practice/report.rs`, `practice/prompts.rs`, `practice/report_job.rs`, `practice/db.rs`.

**Produces:**
```rust
// report.rs
pub struct ReportItem { …, #[serde(default)] pub pattern: Option<String> }
pub struct PronHint { pub turn: i64, pub word: String, pub start_secs: f64, pub end_secs: f64, pub score: f32, pub phone: String }
pub struct Report { pub version: u32 /* 2 */, …, #[serde(default)] pub pronunciation: Vec<PronHint>, #[serde(default)] pub pronunciation_note: Option<String> }
pub fn select_hints(all: Vec<PronHint>) -> Vec<PronHint>; // dedupe by word (keep the lowest score), sort ascending by score, take PRON_MAX_HINTS
// schema(): grammar and unnatural items gain a required "pattern": string (2–5 words, lowercase)
// prompts.rs
pub fn report_prompts(turns: &[(i64, &str, &str)], recent_patterns: &[String]) -> (String, String);
  // adds: "Known pattern keys from earlier sessions (reuse one when it is the same mistake): …"
// db.rs
pub async fn recent_patterns(pool, limit: usize) -> Result<Vec<String>, String>; // distinct `pattern` values from the newest ready reports, newest first
```
**`report_job` changes:**
- `read_speech` returns each segment's start offset in the turn as well as its samples.
- After the word-for-word pass, if any learner turn has audio, `ensure_pron_models`. On error, `pronunciation_note = Some("Pronunciation hints unavailable")` and continue.
- `PronEngine::load` runs in `spawn_blocking`.
- For each learner turn and each segment: `segment_hints(samples, segment_text)`, using that segment's own word-for-word text (the per-segment texts from the raw pass). If the turn fell back to the live text, skip pronunciation for that turn.
- Shift hint times by the segment's start offset, then `select_hints`.
- Run all of it on a blocking thread, between the Whisper pass and the Claude call. Check `is_recording_now()` between turns, as the Whisper pass does.
- `report_prompts` gets `recent_patterns(pool, RECENT_PATTERNS_MAX)`.

- [ ] **Step 1: Write the failing tests:**
  - `v1_report_json_still_parses` (no pattern or pronunciation fields)
  - `select_hints_dedupes_sorts_and_caps`
  - `schema_requires_pattern_on_grammar_and_unnatural`
  - `report_prompt_lists_recent_patterns`
  - `recent_patterns_are_distinct_newest_first` (DB)
  - `pron_failure_keeps_the_report` (a pure helper that merges the pron result into the report → note set, report kept)
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, and the full workspace passes. **Step 5:** Commit `feat(practice): report v2 with pronunciation hints and pattern keys`.

### Task 6: Recurring patterns and filler trend

**Files:**
- Create: `practice/recurring.rs`
- Modify: `practice/commands.rs` (`SessionDetail.recurring`), `practice/db.rs` (query)

**Produces:**
```rust
pub struct Recurring { pub patterns: Vec<RecurringPattern>, pub filler_trend: Vec<f64> }
pub struct RecurringPattern { pub key: String, pub sessions: u32, pub same_setup: bool }
pub struct PastReport { pub session_id: String, pub setup_id: Option<String>, pub started_at: String, pub report: Report }
pub fn compute(current: &PastReport, recent: &[PastReport]) -> Recurring;
  // window = current + the RECURRING_SESSIONS-1 newest earlier READY sessions (all setups);
  // count, per pattern key, the sessions in the window containing it; keep keys with ≥ 2;
  // same_setup = at least one window session containing the key has the current session's setup_id;
  // order: same_setup first, then by sessions desc, then key;
  // filler_trend = filler_total / (speech_secs/60) for the last RECURRING_SESSIONS same-setup ready sessions incl. current, oldest → newest,
  // rounded to 1 decimal; sessions with speech_secs < 30 are skipped
pub async fn load_recent(pool, before_started_at: &str, limit: usize) -> Result<Vec<PastReport>, String>; // in db.rs
```
`practice_get_session` fills `recurring` only for a ready report, otherwise `None`. v1 reports contribute no patterns but do count in the filler trend.

- [ ] **Step 1: Write the failing tests:**
  - `patterns_seen_in_two_sessions_are_listed`
  - `same_setup_patterns_come_first`
  - `window_is_five_sessions`
  - `filler_trend_is_per_minute_oldest_first_and_skips_short_sessions`
  - `v1_reports_count_only_for_fillers`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): recurring patterns and filler trend`.

### Task 7: Model settings backend **[Codex]**

**Files:**
- Create: `practice/settings.rs`
- Modify: `practice/claude.rs` (bodies take settings), `runner.rs`, `report_job.rs`, `commands.rs` (plan call)
- Modify: `lib.rs` (register 2 commands; `practice::settings::load_at_startup(app)` next to Dictate's at `:641`)

**Produces:**
```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PracticeSettings { pub chat_model: String, pub plan_model: String, pub plan_effort: String }
impl Default for PracticeSettings { /* claude-haiku-5-5, claude-opus-5-5, high */ }
pub fn sanitize(s: PracticeSettings) -> PracticeSettings;  // unknown values → defaults, field by field
pub fn current() -> PracticeSettings;                      // in-memory cache, like dictate::settings
pub fn load_at_startup<R: Runtime>(app: &AppHandle<R>);    // practice.json, key "settings"
#[tauri::command] pub async fn practice_get_settings() -> PracticeSettings;
#[tauri::command] pub async fn practice_save_settings<R: Runtime>(app: AppHandle<R>, settings: PracticeSettings) -> Result<PracticeSettings, String>; // reject unknown values with Err, save, update cache
// claude.rs
pub fn chat_body(model: &str, system: &str, msgs: &[ChatMsg]) -> Value;
  // haiku: thinking {"type":"disabled"}, effort low; sonnet-5-5: thinking {"type":"between_tools"}, effort low; opus-5-5: no thinking field, effort low
pub fn plan_body(model: &str, effort: &str, …) -> Value; pub fn report_body(model: &str, effort: &str, …) -> Value;
```
- [ ] **Step 1: Write the failing tests:**
  - `chat_body_per_model` (3 models → the exact thinking and effort fields)
  - `plan_and_report_use_settings_model_and_effort`
  - `sanitize_replaces_unknown_values_field_by_field`
  - `save_rejects_unknown_values`
  - the existing claude.rs body tests updated
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS, and the full workspace passes. **Step 5:** Commit `feat(practice): model settings`.

### Task 8: Reference pronunciation command

**Files:** Modify `practice/commands.rs` (+ `lib.rs` registration).

```rust
#[tauri::command] pub async fn practice_speak_word<R: Runtime>(app: AppHandle<R>, session_id: String, word: String) -> Result<(), String>;
// refuse while a session runs (player in use); word trimmed, 1–40 chars; voice = the session row's voice; ensure_voices; Supertonic::load (DirectML→CPU)
// on a blocking thread; synthesize(word, voice, TTS_STEPS, TTS_SPEED); Player::open + play; return when queued (drop the player after playback finishes on a background thread)
```
- [ ] **Step 1: Failing test:** `speak_word_validates_input` (a pure validator: empty → Err, 41 chars → Err, `"hypothesis"` → Ok).
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `feat(practice): reference pronunciation playback`.

### Task 9: Report view — pronunciation and recurring cards

**Files:** Modify `frontend/src/lib/practice.ts`, `frontend/src/components/Practice/ReportView.tsx`, tests.

**Behaviour (spec §6.1–6.2):**
- **Card 5, "Pronunciation hints (may be off)":**
  - one row per hint: the word, ▶ you and ▶ reference;
  - ▶ you plays `wav_offset/16000 + start_secs` to `wav_offset/16000 + end_secs` of the hint's turn, through the existing hidden audio element;
  - ▶ reference calls `invoke('practice_speak_word', {sessionId, word})`;
  - no hints: show `pronunciation_note` if set, else "No pronunciation hints this time".
- **"Keeps coming back" card**, last, only when `recurring` exists:
  - pattern chips "if + will ×4 sessions" (same-setup first);
  - a "Fillers per minute" line `3.1 → 2.4 → 1.8` when it has 2 or more points;
  - hidden when both are empty.
- **v1 reports:** neither card shows.

- [ ] **Step 1: Write the failing tests:**
  - `pron_card_plays_word_span_and_reference`
  - `pron_card_shows_note_or_empty_text`
  - `recurring_card_lists_patterns_and_trend`
  - `v1_report_hides_new_cards`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS; tsc, bun and build pass. **Step 5:** Commit `feat(practice): pronunciation and recurring cards`.

### Task 10: Models card

**Files:**
- Create: `frontend/src/components/Practice/ModelsCard.tsx`
- Modify: `frontend/src/lib/practice.ts` (types, `estimateCost`), `app/practice/page.tsx` (card below the two lists), tests

**`estimateCost`:**
```ts
export const PRICES = { 'claude-haiku-5-5': [0.10, 0.50], 'claude-sonnet-5-5': [2, 10], 'claude-opus-5-5': [4, 20] }; // USD per M in/out
export function estimateCost(model: string, kind: 'chat' | 'plan_report'): number;
// chat: 360_000 in + 5_000 out tokens per 30-min session; plan_report: 60_000 in + 10_000 out (thinking at high ≈ +10_000 out) — round to 2 decimals
```
**Card layout:**
- Three `Select`s (Conversation, Plan and report, Effort), each showing "≈ $X per 30-min session".
- "Slower replies" warning when the conversation model is Opus.
- Saves on change through `practice_save_settings`, the same commit pattern as the Dictate page.

- [ ] **Step 1: Write the failing tests:**
  - `estimate_cost_values` (haiku chat ≈ 0.04; opus plan_report ≈ 0.44)
  - `models_card_loads_and_saves`
  - `opus_chat_shows_slower_replies`
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS; tsc, bun and build pass. **Step 5:** Commit `feat(practice): models card`.

### Task 11: Phase 1 leftovers and the removal checklist

**Files:** Modify `practice/report_job.rs`, `practice/runner.rs`, `practice/mod.rs` (checklist).

**Changes:**
1. A Whisper panic in a report segment (a `JoinError` from the per-segment `spawn_blocking`) maps to "The report stopped unexpectedly — Retry", not the panic text.
2. The supervisor's panic path calls `end_session` only when the row is still `status = 'running'`, so a finished session's end reason and duration are never overwritten.
3. The removal checklist adds:
   - the `practice.json` store file (user data, left for the user);
   - the new handler count;
   - `pron_*` and `settings.rs` (inside the module, so no extra steps);
   - the `load_at_startup` line in `lib.rs`;
   - the two `docs/network-calls.md` rows.

- [ ] **Step 1: Failing tests:** `segment_panic_maps_to_friendly_copy` (pure mapper) and `end_session_if_running_keeps_a_finished_row` (DB).
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS. **Step 5:** Commit `fix(practice): friendly report panic copy, keep finished end reasons; checklist`.

### Task 12: Whole-branch review, Windows CI, release

- [ ] **Step 1:** Whole-branch review of `practice` since `main` (reviewer opus + Codex + Antigravity per CLAUDE.md). One fix wave, then one re-review.
- [ ] **Step 2:** With the user's OK: push `practice`, then `gh workflow run ci.yml --ref practice -f full=true`. Must be green.
- [ ] **Step 3:** With the user's OK: merge to `main`, tag `v0.8.0`, and let the release run pass its delay-load and no-CUDA checks.
- [ ] **Step 4:** Give the user the phase 1 checklist (phase 1 plan, Task 19) plus these phase 2 checks:
  - the first report downloads the pronunciation files;
  - hints look plausible on a few known-hard words (e.g. "hypothesis", "thoroughly");
  - ▶ you and ▶ reference play the right audio;
  - a mistake repeated across 2 sessions shows in "Keeps coming back";
  - switching the conversation model to Sonnet and to Opus works (Opus is slower).
