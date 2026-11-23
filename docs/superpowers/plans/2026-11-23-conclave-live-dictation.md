# Live Dictation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While a dictation is recorded, the pill shows committed text and grey volatile text, phrase by phrase. On release only the last open phrase is left to decode. The paste flow is unchanged, and it falls back to today's full decode on any live failure.

**Architecture:** A pure, Linux-tested state machine `dictate/live.rs` (`LiveCore`) owns the session audio from the last commit point. It decides cuts (VAD segment ends, a 20 s force-split, the 3 s first-language hold), commit jobs (one at a time, in order, with a 200-char prompt) and preview jobs (cadence, a single pending preview, meeting yield). An async worker `dictate/live_worker.rs` does the I/O: it drains the mic session buffer every 150 ms, runs a second `ContinuousVadProcessor`, makes the Whisper calls and publishes text through a `watch` channel. `session.rs` starts the worker at capture begin, drops it on cancel and finishes it at stop. The pill gets text, a style and a bottom-anchored resize.

**Tech Stack:** Rust (Tauri 2, tokio, whisper-rs via `WhisperEngine`), Next.js 14 / React 18 pill page, Bun tests with happy-dom.

**Spec:** `docs/superpowers/specs/2026-11-23-conclave-live-dictation-design.md`

## Global Constraints

- No new dependency and no network call. `tokio::sync::{watch, oneshot, mpsc}` are already available through `tokio = { features = ["full"] }`.
- Transcript text never reaches a log line. Logs carry only counts, durations in ms, the language code and whether the fallback ran.
- Never lose text: any live decode error, a dead worker or a VAD error falls back at stop to today's full-buffer decode.
- Committed decodes run through the dictation's own `LanguagePolicy`. Previews use a clone, and their language results are thrown away.
- The pill stays `focusable(false)`. Colours come from tokens only (`--foreground`, `--muted-foreground`, `--background`). There is no `aria-live` on the live text.
- Values: Dictate VAD redemption 400 ms; 20 s maximum segment; force-cut at the quietest 30 ms frame within the last 2 s; 3 s of speech before the first commit; previews every 1 s (3 s while a meeting records), only after ≥ 0.5 s of growth, and the first one at ≥ 1.5 s of open audio; prompt = last 200 chars, cleared on a language change; nothing-heard gate 0.5 s (`MIN_SPEECH_SECS`).
- Setting: `live_text_style` is `"panel"` (default) or `"line"`. Panel 480 px wide with text at 15 px. Line 560 px wide with a 40 px pill and text at 14 px.
- Gates (from AGENTS.md). Run them under `flock -n -E 75 /run/dev-build-lock`; exit 75 means busy, so retry later. Cargo needs `. "$HOME/.cargo/env"; export CARGO_TARGET_DIR=/workspace/target CARGO_BUILD_JOBS=2`, `nice -n 10`, `frontend/out` and the stub sidecar `frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu`.
  - Rust: `cd /workspace && flock -n -E 75 /run/dev-build-lock nice -n 10 cargo test -p meetily --lib <filter>`
  - Frontend: `cd /workspace/frontend && flock -n -E 75 /run/dev-build-lock sh -c 'pnpm exec tsc --noEmit && bun test <file>'`
- Commits are on branch `enhance/live-dictation`. Run each `git commit` as its own Bash call (gitleaks hook). Do not push.

## Review Focus

1. **A short dictation ("Yes, send it.") with under 3 s of speech.** The held audio must still be committed at stop, not dropped. Pinned in Task 3, `finish_releases_held_audio_under_three_seconds`.
2. **Silence (a hands-free dictation left open).** It must make no Whisper call (Whisper invents "Thank you." from silence), and the worker's buffer must stay short. Pinned in Task 3, `silence_only_makes_no_jobs_and_keeps_buffer_short`.
3. **Audio that arrives after the worker's last drain** (up to 150 ms; `end()` takes the session buffer). It must be part of the last commit. Pinned in Task 3, `finish_includes_audio_after_the_last_drain`.
4. **Russian text in the prompt and pill tails.** Byte slicing would panic in the middle of a Cyrillic character. Pinned in Task 3, `tail_counts_chars_not_bytes`.
5. **A bad resize request from the page** (NaN, 0 or 10 000 px) must not leave a huge transparent window that eats clicks. Pinned in Task 4, `panel_height_is_clamped`.

---

### Task 1: `live_text_style` setting (mechanical)

**Files:**
- Modify: `frontend/src-tauri/src/dictate/settings.rs:49-64` (struct), `:76-90` (Default), `:107-117` (sanitize), `:390-400` (normalise); tests in the same file (`mod tests`, from `:503`)
- Modify: `frontend/src/lib/dictate.ts:14-27`
- Modify: `frontend/src/components/Dictate/DictateGeneral.tsx:64-85` (new row after Microphone)
- Modify fixtures (the new TS field is required): `frontend/tests/components/dictate-page.test.tsx:30-41`, `frontend/tests/lib/dictate.test.ts:5-15`, `frontend/tests/lib/models-keys.test.ts:55`, `frontend/src/lib/browser-preview.ts:160`
- Test: `frontend/tests/components/dictate-page.test.tsx`

**Interfaces:**
- Produces (Rust, `dictate::settings`): `#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)] #[serde(rename_all = "lowercase")] pub enum LiveTextStyle { #[default] Panel, Line }`, and the field `#[serde(default)] pub live_text_style: LiveTextStyle` on `DictateSettings`.
- Produces (TS, `@/lib/dictate`): `export type LiveTextStyle = 'panel' | 'line';` and `live_text_style: LiveTextStyle` on `DictateSettings`.

- [ ] **Step 1: Write the failing tests**

Rust, in `settings.rs` tests:
```rust
#[test]
fn live_text_style_defaults_to_panel_and_round_trips() {
    assert_eq!(sanitize(None).live_text_style, LiveTextStyle::Panel);
    let mut raw = serde_json::to_value(DictateSettings::default()).unwrap();
    assert_eq!(raw["live_text_style"], "panel");
    raw.as_object_mut().unwrap().remove("live_text_style");
    assert_eq!(serde_json::from_value::<DictateSettings>(raw.clone()).unwrap().live_text_style, LiveTextStyle::Panel);
    raw["live_text_style"] = json!("line");
    let saved = validate_for_save(&sanitize(Some(raw.clone()))).unwrap();
    assert_eq!(saved.live_text_style, LiveTextStyle::Line);
    raw["live_text_style"] = json!("sideways");
    assert_eq!(sanitize(Some(raw)).live_text_style, LiveTextStyle::Panel);
}
```
Bun, in `dictate-page.test.tsx` (same lookup pattern as the microphone test at `:237-249`):
```tsx
test('Live text saves panel or one line', async () => {
  const live = () => renderer!.root
    .findAll((n) => n.props.onValueChange !== undefined && n.props.value !== undefined)
    .find((n) => n.findAll((c) => c.props.id === 'dictate-live-text').length > 0)!;
  expect(live().props.value).toBe('panel');
  await act(async () => { live().props.onValueChange('line'); });
  await flush();
  expect(settings.live_text_style).toBe('line');
});
```

- [ ] **Step 2: Run the tests and check that they fail**

Run the Rust gate with filter `dictate::settings` and the Bun gate with `tests/components/dictate-page.test.tsx`. Expected: FAIL. The Rust test does not compile (`LiveTextStyle` is undefined), and the Bun test finds no `dictate-live-text` control.

- [ ] **Step 3: Implement**

- In `sanitize`, add `live_text_style: field(&obj, "live_text_style", LiveTextStyle::Panel)`. `field` already falls back on a wrong type.
- In `normalise`, pass `s.live_text_style` through.
- Struct literals that use `..DictateSettings::default()` need no change.
- `DictateGeneral.tsx`: add a `Row` labelled `<Label htmlFor="dictate-live-text">Live text</Label>` with the hint "How the words show while you speak. Takes effect on the next dictation." Put a `Select` in it with `value={settings.live_text_style}`, `SelectTrigger id="dictate-live-text" aria-label="Live text"`, and the items `panel` → "Panel" and `line` → "One line". It saves with `commit((s) => ({ ...s, live_text_style: v as LiveTextStyle }))`.
- Add `live_text_style: 'panel'` to the four fixtures.

- [ ] **Step 4: Run the tests and check that they pass**

Run the same two gates. Expected: PASS. `pnpm exec tsc --noEmit` is clean.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/dictate/settings.rs frontend/src/lib/dictate.ts frontend/src/components/Dictate/DictateGeneral.tsx frontend/src/lib/browser-preview.ts frontend/tests/components/dictate-page.test.tsx frontend/tests/lib/dictate.test.ts frontend/tests/lib/models-keys.test.ts
git commit -m "feat(dictate): live text style setting (panel or one line)"
```

---

### Task 2: Engine and capture hooks for the live path (mechanical)

**Files:**
- Modify: `frontend/src-tauri/src/whisper_engine/language_policy.rs:9` (derive), tests at `:79`
- Modify: `frontend/src-tauri/src/whisper_engine/whisper_engine.rs` (new method near `is_model_loaded`, `:479`), tests near `inference_lock_is_fifo`, `:2299`
- Modify: `frontend/src-tauri/src/audio/vad.rs:35-88` (new getter)
- Modify: `frontend/src-tauri/src/dictate/capture.rs:107-147` (`SessionBuffer`), `:177-239` (`MicCapture`), tests at `:346`

**Interfaces:**
- Produces:
  - `#[derive(Clone)] pub struct LanguagePolicy`, with a new private field `last: Option<String>` that `choose` sets to the language it returns.
  - `impl LanguagePolicy { pub fn lock_if_unset(&mut self) }`: when `current` is None, sets it to `last`. Only Dictate's live path calls it (after the first commit), so the spec's "the first decision over ≥ 3 s sets the dictation's language" holds even when that detection was not confident. The meeting path is unchanged.
  - `impl WhisperEngine { pub fn inference_idle(&self) -> bool }`. It returns true only when no Whisper call is running or queued (`self.inference_lock.try_lock().is_ok()`; tokio's fair mutex refuses `try_lock` while waiters are queued).
  - `impl ContinuousVadProcessor { pub fn in_speech(&self) -> bool }`
  - `impl SessionBuffer { pub fn samples_since(&self, from: usize) -> Option<Vec<f32>> }`. It returns None outside a session and an empty Vec when `from >= len`.
  - `impl MicCapture { pub fn samples_since(&self, from: usize) -> Option<Vec<f32>>; pub fn sample_rate(&self) -> u32 }`

- [ ] **Step 1: Write the failing tests**

```rust
// language_policy.rs
#[test]
fn cloned_policy_is_independent() {
    let real = policy(&["ru", "en"], None);
    let mut copy = real.clone();
    copy.choose(&[0.05, 0.9], 4.0);
    assert_eq!(copy.current(), Some("en"));
    assert_eq!(real.current(), None);
}
#[test]
fn lock_if_unset_keeps_an_unconfident_first_choice() {
    let mut p = policy(&["ru", "en"], None);
    assert_eq!(p.choose(&[0.25, 0.6], 4.0), "en"); // share 0.71 < 0.8: not confident
    assert_eq!(p.current(), None);
    p.lock_if_unset();
    assert_eq!(p.current(), Some("en"));
    p.choose(&[0.9, 0.05], 4.0); // a later confident switch still works
    assert_eq!(p.current(), Some("ru"));
}
// whisper_engine.rs
#[tokio::test]
async fn inference_idle_is_false_while_a_call_holds_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    assert!(engine.inference_idle());
    let held = engine.inference_lock.lock().await;
    assert!(!engine.inference_idle());
    drop(held);
    assert!(engine.inference_idle());
}
// capture.rs
#[test]
fn samples_since_copies_the_session_tail() {
    let mut b = SessionBuffer::new(2, 100);
    assert_eq!(b.samples_since(0), None);
    b.push(&[1.0, 2.0]);
    b.begin(Vec::new());
    b.push(&[3.0]);
    assert_eq!(b.samples_since(0), Some(vec![1.0, 2.0, 3.0]));
    assert_eq!(b.samples_since(2), Some(vec![3.0]));
    assert_eq!(b.samples_since(9), Some(vec![]));
    assert_eq!(b.end(), vec![1.0, 2.0, 3.0]); // copying never consumes
}
```

- [ ] **Step 2: Run the tests and check that they fail**

Run the Rust gate with filter `cloned_policy_is_independent`, then `inference_idle`, then `samples_since`. Expected: compile errors for the missing methods and for `clone`.

- [ ] **Step 3: Implement the five members with the signatures above.** `in_speech` returns the existing private field. `samples_since` copies under the existing lock; the slice is at most a few hundred ms.

- [ ] **Step 4: Run the tests and check that they pass**

Run the three filters again, then `whisper_engine::` and `dictate::capture`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/whisper_engine/language_policy.rs frontend/src-tauri/src/whisper_engine/whisper_engine.rs frontend/src-tauri/src/audio/vad.rs frontend/src-tauri/src/dictate/capture.rs
git commit -m "feat(dictate): policy clone, idle engine check, VAD speech flag, session tail read"
```

---

### Task 3: `LiveCore`, the pure segment scheduler (judgment, **difficult**)

**Files:**
- Create: `frontend/src-tauri/src/dictate/live.rs` (logic and `#[cfg(test)] mod tests`)
- Modify: `frontend/src-tauri/src/dictate/mod.rs:4-9` (`pub mod live;`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (`crate::dictate::live`):
```rust
// pub consts: MAX_SEGMENT_SECS 20.0, SPLIT_WINDOW_SECS 2.0, SPLIT_FRAME_MS 30, HOLD_SPEECH_SECS 3.0, MIN_CUT_SPEECH_SECS 0.25,
// SILENCE_KEEP_SECS 1.0, PREVIEW_EVERY_MS 1000, PREVIEW_EVERY_MEETING_MS 3000, PREVIEW_MIN_GROWTH_SECS 0.5,
// FIRST_PREVIEW_SECS 1.5, PROMPT_CHARS 200, PILL_TAIL_CHARS 400

#[derive(Debug, PartialEq)]
pub enum Job {
    Commit { index: u64, samples: Vec<f32>, prompt: String },
    Preview { from: usize, samples: Vec<f32>, prompt: String },
}
pub struct LiveCore { /* private */ }
impl LiveCore {
    pub fn new(rate: u32) -> Self;
    pub fn push_audio(&mut self, samples: &[f32], in_speech: bool);
    pub fn on_vad_segment(&mut self, start_ms: f64, end_ms: f64);
    pub fn next_job(&mut self, now_ms: u64, meeting: bool, engine_idle: bool) -> Option<Job>;
    pub fn on_commit_done(&mut self, index: u64, text: Result<String, ()>, language: Option<String>);
    pub fn on_preview_done(&mut self, from: usize, text: String);
    pub fn finish(&mut self);
    pub fn cancel(&mut self);
    pub fn fail(&mut self);
    pub fn is_done(&self) -> bool;
    pub fn failed(&self) -> bool;
    pub fn transcript(&self) -> &str;
    pub fn volatile(&self) -> &str;
    pub fn speech_secs(&self) -> f32;
    pub fn stats(&self) -> (usize, usize, usize); // (commits, previews, previews skipped)
}
pub fn tail_chars(s: &str, n: usize) -> &str;
/// Index (relative to `samples`) of the centre of the quietest `frame`-sample frame.
pub fn quietest_frame_centre(samples: &[f32], frame: usize) -> usize;
```

**Rules the tests pin.** Positions are absolute native-rate sample indices from the session start, including the pre-roll. The VAD's ms are session-absolute at 16 kHz, so `idx = ms * rate / 1000`.
- State: `total` (samples pushed), `cut` (end of the last closed range), `committed_to` (end of the last range sent to the ready queue), and `buf` (audio from `committed_to` on). `buf` drops its prefix whenever `committed_to` moves.
- `on_vad_segment` adds `end - start` to `speech_secs` (the nothing-heard total). Its overlap after `cut` is `(end - max(start, cut))`. If the overlap is at least `MIN_CUT_SPEECH_SECS`, it closes a range: `cut = end`.
- `push_audio`, while `in_speech` is true and `total - cut ≥ 20 s`: force-close at `cut + window_start + quietest_frame_centre(last 2 s, 30 ms)`. The range counts its full length as speech.
- `push_audio`, while not `in_speech` and with nothing held (`cut == committed_to`), once `total - cut > SILENCE_KEEP_SECS + 1 s`: move `cut = committed_to = total - 1 s` (silence is never decoded).
- Before the first decision, closed ranges are held: their speech adds up, and at ≥ `HOLD_SPEECH_SECS` the whole `[committed_to, cut)` goes to the ready queue as one range. After that, every closed range goes to the ready queue directly.
- `next_job` returns a commit first when the ready queue is not empty and no commit is in flight. A meeting never blocks commits. The prompt is `tail_chars(&transcript[prompt_start..], PROMPT_CHARS)`. Only one commit is in flight at a time, because the policy is `&mut` and the prompt needs the previous text. `on_commit_done` with a different index is ignored.
- `on_commit_done(Ok)` trims the text and joins it with one space. If `language` is `Some` and differs from the previous `Some` language, `prompt_start` moves to where this text begins. If the last accepted preview began before `committed_to`, it clears the volatile text. `Err` sets `failed`, and no further commits are issued.
- A preview is due when all of these hold: not finishing, not cancelled, no preview in flight; `in_speech` or held audio exists; `total - committed_to ≥ FIRST_PREVIEW_SECS` until the first decision; growth since the last preview ≥ 0.5 s; and `now - last_preview ≥ 1000` (or `3000` when `meeting`). When everything else holds but `meeting && !engine_idle`, it counts a skip, sets `last_preview = now` and returns None.
- `on_preview_done` always clears the in-flight flag. It sets the volatile text only when `from == committed_to`.
- `finish()` drops the open tail after `cut` (no speech remains once the worker has fed the VAD flush), sends any held range to the ready queue whatever its speech, and stops previews. `is_done()` means cancelled, or finishing with no commit in flight and either an empty ready queue or `failed`.
- `cancel()`: `next_job` returns None, `on_*` calls are ignored, and `is_done()` is true.

- [ ] **Step 1: Write the failing tests** (`rate = 16_000` unless noted; helper `fn speech(core, secs)` pushes `secs` of 0.5-amplitude audio with `in_speech = true`)

```rust
#[test] fn force_split_at_20s_cuts_at_quietest_frame_in_last_2s() {
    // 20 s of 0.5 with one 30 ms zero frame at 19.0 s; held speech ≥ 3 s, so the range commits at once
    let Some(Job::Commit { samples, .. }) = core.next_job(0, false, true) else { panic!() };
    assert!((19.0 * 16_000.0) as usize <= samples.len() && samples.len() <= (19.03 * 16_000.0) as usize);
}
#[test] fn first_commit_waits_for_3s_of_speech() {
    // segments of 1.0 s and 1.5 s speech close → no Commit; a third of 0.6 s → one Commit covering all three
}
#[test] fn finish_releases_held_audio_under_three_seconds() {
    // 1.2 s speech segment closed, then finish() → Commit whose samples end at that segment's end
}
#[test] fn commits_run_one_at_a_time_in_order() {
    // decided core, three ranges close → next_job gives Commit{index:0}, then only Previews/None until on_commit_done(0, Ok("one"), Some("en"));
    // then Commit{index:1, prompt:"one"}; after done(1,"two") transcript() == "one two"
}
#[test] fn stale_commit_result_is_ignored() {
    // on_commit_done(5, Ok("x"), None) while index 0 in flight → transcript unchanged, index 0 still in flight
}
#[test] fn prompt_clears_on_language_change() {
    // done(0, Ok("привет"), Some("ru")); done(1, Ok("hello"), Some("en")) → Commit{index:2}.prompt == "hello"
}
#[test] fn tail_counts_chars_not_bytes() {
    let s = "я".repeat(250);
    assert_eq!(tail_chars(&s, 200).chars().count(), 200);
    assert_eq!(tail_chars("abc", 200), "abc");
}
#[test] fn one_preview_pending_and_cadence() {
    // decided core, in speech: Preview at now=0; at 1000 with growth → None (in flight);
    // on_preview_done; at 1500 → None (<1 s since last); at 2000 with 0.3 s growth → None; with 0.6 s → Preview
}
#[test] fn first_preview_needs_one_and_a_half_seconds() {
    // undecided: 1.4 s open → None; 1.5 s → Preview{from:0}
}
#[test] fn meeting_previews_every_3s_and_skip_when_engine_busy() {
    // meeting=true: Preview at 0; done; at 1000 None; at 3000 with engine_idle=false → None and stats().2 == 1;
    // at 6000 engine_idle=true → Preview
}
#[test] fn preview_after_a_commit_moved_the_boundary_is_dropped() {
    // Preview{from:0} issued, then a range is released (committed_to moves), on_preview_done(0, "x") → volatile() == ""
}
#[test] fn cancel_ignores_late_results() {
    // Commit in flight, cancel(); on_commit_done(0, Ok("late")) → transcript() == ""; next_job → None; is_done()
}
#[test] fn failed_commit_marks_fallback_and_stops_commits() {
    // done(0, Err(())) → failed(); a further closed range → no Commit; finish() → is_done()
}
#[test] fn silence_only_makes_no_jobs_and_keeps_buffer_short() {
    // 30 s of zeros pushed in 100 ms steps with in_speech=false; next_job at every step → always None;
    // finish() → is_done(), speech_secs() == 0.0, and the internal buffer ≤ 2.1 s (assert via a #[cfg(test)] fn buffered())
}
#[test] fn finish_includes_audio_after_the_last_drain() {
    // decided, in speech, 2 s pushed; then push 0.15 s more (the tail from the full buffer), the VAD flush segment ending at 2.15 s, finish()
    // → the final Commit's samples.len() == 2.15 s worth
}
```

- [ ] **Step 2: Run the tests and check that they fail**

Run the Rust gate with filter `dictate::live`. Expected: FAIL to compile (the module is missing).

- [ ] **Step 3: Implement `live.rs` to the rules above.** `quietest_frame_centre` uses RMS per non-overlapping frame and takes the first minimum.

- [ ] **Step 4: Run the tests and check that they pass**

Run the Rust gate with filter `dictate::live`. Expected: PASS (15 tests).

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/dictate/live.rs frontend/src-tauri/src/dictate/mod.rs
git commit -m "feat(dictate): pure live segment scheduler (cuts, held first 3 s, previews, prompt)"
```

---

### Task 4: Pill backend: text, style, bottom-anchored resize (judgment)

**Files:**
- Modify: `frontend/src-tauri/src/dictate/pill.rs:12-14` (constants), `:30-36` (`PillView`), `:40-50` (`State`), `:58-62` (`bottom_centre`), `:65-78` (`position`), `:98-132` (`show`), `:139-148` (`update`), new command after `:171`; tests from `:173` (update the `bottom_centre` test at the end of the module to the new arity, keeping its values with `360.0, 56.0`)
- Modify: `frontend/src-tauri/src/dictate/session.rs:378-385` (`Exec` gets `style: LiveTextStyle`), `:390-394` (set `self.style = settings::current().live_text_style` in `BeginCapture`), `:419` (`pill::show(app, foreground_monitor_point(), self.style)`), `:446-451` and `:474-481` (build with `PillView::new`)
- Modify: `frontend/src-tauri/src/lib.rs:1090` (register `dictate::pill::dictate_pill_resize`)

**Interfaces:**
- Consumes: `LiveTextStyle` (Task 1).
- Produces:
```rust
#[derive(Serialize, Clone, Debug)]
pub struct PillView {
    pub state: PillState, pub elapsed_ms: u64, pub level: f32, pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub committed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub volatile: Option<String>,
    pub style: LiveTextStyle, // set by pill.rs from the style given to show(); callers' value is overwritten
}
impl PillView { pub fn new(state: PillState, mode: &str) -> Self } // elapsed 0, level 0, no text, Panel
pub fn show<R: Runtime>(app: &AppHandle<R>, near: Option<(i32, i32)>, style: LiveTextStyle);
#[tauri::command] pub fn dictate_pill_resize<R: Runtime>(app: AppHandle<R>, height: f64);
const PANEL_WIDTH: f64 = 480.0; const LINE_WIDTH: f64 = 560.0; const HEIGHT: f64 = 56.0; const PANEL_MAX_HEIGHT: f64 = 140.0;
fn window_size(style: LiveTextStyle) -> (f64, f64);              // Panel (480, 56), Line (560, 56)
fn panel_height(requested: f64) -> f64;                           // clamp to [56, 140]; non-finite → 56
fn bottom_centre(work_x: i32, work_y: i32, work_w: u32, work_h: u32, scale: f64, width: f64, height: f64) -> (f64, f64);
```
- `State` gains `style: LiveTextStyle` and `anchor: Option<(f64, f64)>`, the logical left x and the bottom edge (`top + HEIGHT`), both recorded in `show`. `update` merges: a view with `committed`/`volatile` set to `None` keeps the stored values in `st.view`, so `dictate_pill_view` and the panel after stop still have the text. The emitted payload is the incoming view (with `style` filled in), and the page merges it (Task 5).
- `dictate_pill_resize` does nothing for `Line`, without an anchor or without a window. Otherwise it calls `set_size(LogicalSize::new(PANEL_WIDTH, h))` then `set_position(LogicalPosition::new(x, bottom - h))` with `h = panel_height(height)`. The window stays `focusable(false)`; this task touches nothing in the builder except `inner_size(window_size(style))`.

- [ ] **Step 1: Write the failing tests** (in `pill.rs` tests)

- `view_carries_text_only_when_set_and_the_style`: `PillView::new(Listening, "cleaned-up")` serialises with no `committed` or `volatile` key and `style == "panel"`. With `committed: Some("Hi")`, `volatile: Some("the")` and `style: Line`, it serialises `"Hi"`, `"the"` and `"line"`.
```rust
#[test] fn panel_height_is_clamped() {
    assert_eq!(panel_height(f64::NAN), 56.0);
    assert_eq!(panel_height(0.0), 56.0);
    assert_eq!(panel_height(10_000.0), 140.0);
    assert_eq!(panel_height(100.0), 100.0);
}
#[test] fn sizes_per_style() {
    assert_eq!(window_size(LiveTextStyle::Panel), (480.0, 56.0));
    assert_eq!(window_size(LiveTextStyle::Line), (560.0, 56.0));
    // 1080p, 40 px taskbar: a 480-wide panel is centred, and its bottom stays 12 px above the work area
    assert_eq!(bottom_centre(0, 0, 1920, 1040, 1.0, 480.0, 56.0), (720.0, 972.0));
}
```

- [ ] **Step 2: Run the tests and check that they fail**

Run the Rust gate with filter `dictate::pill`. Expected: compile errors.

- [ ] **Step 3: Implement the signatures above and update the three `session.rs` call sites** (`tick` builds `PillView { elapsed_ms, level, ..PillView::new(state, &self.mode) }`).

- [ ] **Step 4: Run the tests and check that they pass**

Run the Rust gate with filters `dictate::pill` and `dictate::session`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/dictate/pill.rs frontend/src-tauri/src/dictate/session.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(dictate): pill view carries live text and style; panel resizes from the bottom"
```

---

### Task 5: Pill page: panel and one-line styles (judgment)

**Files:**
- Modify: `frontend/src/lib/dictatePill.ts:14-19` (type) and new helpers
- Modify: `frontend/src/app/dictate-pill/page.tsx` (whole render, `:56-97`; listener `:24`)
- Test: `frontend/tests/lib/dictate-pill.test.ts`, `frontend/tests/components/dictate-pill-page.test.tsx` (its `view()` helper at `:61` gains `style: 'panel'`)

**Interfaces:**
- Consumes: the JSON from Task 4: `committed?`, `volatile?` and `style: 'panel' | 'line'`; the `dictate_pill_resize` command with argument `{ height: number }`.
- Produces (`@/lib/dictatePill`):
```ts
export interface PillView { state: PillState; elapsed_ms: number; level: number; mode: string; committed?: string; volatile?: string; style: LiveTextStyle }
export function mergeView(prev: PillView | null, next: PillView): PillView; // missing text keeps prev's
export function hasText(v: PillView): boolean;                              // committed or volatile non-blank
export function liveLabel(v: PillView): string; // listening/hands-free with text → m:ss timer, else pillLabel(v)
```

**Look (decided).**
- Palette: `const conclave = document.documentElement.classList.contains('theme-conclave')`, read once on mount. The card gets `dark theme-conclave` under the Conclave theme and `dark` otherwise. `.theme-conclave` comes after `.dark` in `globals.css`, so it wins. The card keeps `bg-background text-foreground`.
- No text: the markup is exactly today's pill, for both styles.
- Panel with text: the outer wrapper becomes `items-end pb-2`. The card is `w-[464px] flex-col rounded-2xl`. Above today's status row (dot, bars, `liveLabel`, controls) sits the text block: `px-4 pt-3 text-[15px] leading-[22px] max-h-[66px] overflow-hidden flex flex-col justify-end`. It adds a top fade (`[mask-image:linear-gradient(to_bottom,transparent,black_22px)]`) only when its `scrollHeight > clientHeight`. A `useLayoutEffect` on the text sends `invoke('dictate_pill_resize', { height: card.offsetHeight + 16 })` when the value changes.
- Line with text: the card is `h-10 w-[544px] rounded-full` with dot, bars, then the text span `min-w-0 flex-1 overflow-hidden whitespace-nowrap flex justify-end [mask-image:linear-gradient(to_right,transparent,black_32px)]` (text-sm = 14 px), then `liveLabel`, then the hands-free controls.
- Committed text is a `<span className="text-foreground">`. Volatile text is a `<span className="text-muted-foreground">` after one space. There is no `aria-live`.

- [ ] **Step 1: Write the failing tests**

```ts
// dictate-pill.test.ts
test('merge keeps text the new view leaves out', () => {
  const a = { ...view('listening'), committed: 'Hello', volatile: 'wor' };
  expect(mergeView(a, view('transcribing'))).toMatchObject({ state: 'transcribing', committed: 'Hello', volatile: 'wor' });
  expect(mergeView(a, { ...view('listening'), volatile: '' }).volatile).toBe('');
});
test('live label shows the timer once text exists', () => {
  expect(liveLabel({ ...view('listening', 5000), committed: 'Hi' })).toBe('0:05');
  expect(liveLabel(view('listening', 5000))).toBe('Listening');
  expect(liveLabel({ ...view('transcribing'), committed: 'Hi' })).toBe('Transcribing…');
});
```
Page tests in `dictate-pill-page.test.tsx`. `mount` passes `createNodeMock: () => ({ offsetHeight: 50, scrollHeight: 44, clientHeight: 44 })`.
- `panel shows committed and grey volatile text, capped at three lines, and resizes`: emit `committed: 'Hello there', volatile: 'how are'`. Then the class names include `text-foreground` and `text-muted-foreground`; one class has both `max-h-[66px]` and `text-[15px]`; the JSON has no `aria-live`; and `invoke` was called with `('dictate_pill_resize', { height: 66 })`.
- `one-line style keeps one nowrap text line with the timer and hands-free controls`: emit `hands-free`, 3000 ms, `style: 'line'`, `committed: 'Hello'`. Then one class has `w-[544px]` and `h-10`; one class has `whitespace-nowrap` and `justify-end`; the text has `0:03` and `"aria-label":"Stop"`; and `dictate_pill_resize` was never called.
- `Conclave theme adds the Conclave palette`: with `theme-conclave` on `document.documentElement` (removed in `finally`), the `bg-background` card's classes contain `theme-conclave`.
- `text survives a state event without text`: emit `committed: 'Kept'`, then `view('transcribing')`. The JSON contains `Kept` and `Transcribing…`.
The existing tests in both files must keep passing unchanged, apart from the `style` field in their helpers.

- [ ] **Step 2: Run the tests and check that they fail**

Run the Bun gate with `tests/lib/dictate-pill.test.ts tests/components/dictate-pill-page.test.tsx`. Expected: FAIL (`mergeView` and `liveLabel` are missing, and there is no text markup).

- [ ] **Step 3: Implement the helpers and the page.** The listener becomes `setView((prev) => mergeView(prev, e.payload))`, and the initial `dictate_pill_view` read uses the same merge.

- [ ] **Step 4: Run the tests and check that they pass**

Run the same Bun gate, which includes `tsc`. Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/dictatePill.ts frontend/src/app/dictate-pill/page.tsx frontend/tests/lib/dictate-pill.test.ts frontend/tests/components/dictate-pill-page.test.tsx
git commit -m "feat(dictate): pill shows live text as a panel or one line"
```

---

### Task 6: Live worker and session wiring (judgment, **risky**: concurrency, data loss, untrusted timing)

**Files:**
- Create: `frontend/src-tauri/src/dictate/live_worker.rs`
- Modify: `frontend/src-tauri/src/dictate/mod.rs:4-9` (`pub mod live_worker;`)
- Modify: `frontend/src-tauri/src/dictate/session.rs`: `Exec` (`:378-385`, add `live: Option<LiveHandle>`); `BeginCapture` (`:390-400`); `CancelCapture` (`:408-411`); `EndCaptureAndProcess` (`:412-416`, pass `self.live.take()` to `process`); `tick` (`:441-453`, becomes `&mut self` and includes the changed text); `process` (`:619-696`); tests (`mod tests`, `:866`)

**Interfaces:**
- Consumes: `LiveCore`, `Job`, `tail_chars` and `PILL_TAIL_CHARS` (Task 3); `LanguagePolicy: Clone`, `WhisperEngine::inference_idle`, `ContinuousVadProcessor::in_speech`, `MicCapture::{samples_since, sample_rate}` (Task 2); `PillView::new` with `committed`/`volatile` (Task 4); `crate::audio::recording_commands::is_recording_now()`; `super::whisper_engine()`; `crate::get_allowed_languages_internal()`.
- Produces (`crate::dictate::live_worker`):
```rust
#[derive(Clone, Default, PartialEq)] pub struct LiveText { pub committed: String, pub volatile: String } // committed = tail_chars(.., PILL_TAIL_CHARS)
pub struct LiveOutcome { pub transcript: String, pub speech_secs: f32, pub language: Option<String>, pub failed: bool }
pub struct LiveHandle { pub text: tokio::sync::watch::Receiver<LiveText>, /* finish: oneshot::Sender<Arc<Vec<f32>>>, task: tauri::async_runtime::JoinHandle<LiveOutcome> */ }
pub fn start(rate: u32) -> LiveHandle;                                   // spawns the worker
impl LiveHandle { pub async fn finish(self, all: Arc<Vec<f32>>) -> Option<LiveOutcome> } // None: worker gone
```
In `session.rs`: `fn usable(outcome: Option<LiveOutcome>) -> Option<LiveOutcome>` (`outcome.filter(|o| !o.failed)`), and `async fn full_decode(samples: &[f32], rate: u32) -> Option<(String, f32)>`, which is today's code at `:635-695` moved unchanged (resample, `speech_secs`, fresh policy, one decode) and returns `None` where it now returns early.

**Worker loop** (the order matters, so it is spelled out):
```
vad = ContinuousVadProcessor::new(rate, 400)  // Err → core.fail()
policy = LanguagePolicy::new(get_allowed_languages_internal()); drained = 0; t0 = Instant::now()
interval 150 ms; results: mpsc::unbounded (CommitDone{index, text, policy} | PreviewDone{from, text})
loop select! {
  tick      → if let Some(s) = with_live_mic(|m| m.samples_since(drained)).flatten(): drained += s.len(); feed(&s)
  finish rx → Ok(all): feed(&all[drained.min(all.len())..]); for seg in vad.flush(): core.on_vad_segment; core.finish()
              Err (handle dropped = cancel): core.cancel(); break
  result    → CommitDone: on Ok, policy = returned policy; policy.lock_if_unset(); core.on_commit_done(index, text, policy.current().map(str::to_string))
              PreviewDone: core.on_preview_done(from, text)
}
after each branch: if engine loaded (engine.is_model_loaded().await) { while let Some(job) = core.next_job(ms, is_recording_now(), engine.inference_idle()) { spawn decode } }
                   publish LiveText to watch only when it differs; if core.is_done() break
feed(s): core.push_audio(s, vad.in_speech()) after vad.process_audio(s) → each segment core.on_vad_segment(start_ms, end_ms); VAD Err → core.fail()
decode task: samples16 = resample_audio(&samples, rate, 16_000); engine.transcribe_audio_with_confidence(samples16, Some("auto".into()), Some(&mut p), Some(&prompt))
             Commit: p = policy.clone(), sends back p; Err → text Err(()). Preview: p = policy.clone(), dropped; Err → text "".
end: info!("dictate: live {} segments, {} previews ({} skipped), language {}, fallback {}", ..) — no text
```
Commit tasks are never cancelled. On cancel their results go to a closed channel and are dropped.

**`process` after this task:** the engine checks (`engine_is_whisper`, `LoadingModel`, `ensure_model`) move ahead of the speech gate, because the live path needs the model to finish. Then `set_pill(Transcribing)` and `let all = Arc::new(audio.samples)`. Next comes `match usable(live.finish(all.clone()).await)`:
- `Some(o)`: gate on `o.speech_secs` with `check_speech`, then `text = o.transcript`.
- `None`: `info!("dictate: live path unavailable, full decode")`, then `full_decode(&all, rate)`.

Empty text → `nothing_heard`. Otherwise `pill::update` with `committed: Some(tail_chars(&text, PILL_TAIL_CHARS).into()), volatile: Some(String::new())` in state `Transcribing`. Everything from cleanup on (`:697-752`) is unchanged.

`Exec`:
- `BeginCapture`: when `begin_capture()` succeeds, `self.live = with_live_mic(|m| m.sample_rate()).map(live_worker::start)`.
- `CancelCapture` sets `self.live = None`.
- `tick`: when `live.text.has_changed()`, it sets `committed` and `volatile` from `borrow_and_update()`; otherwise it leaves them `None`.

- [ ] **Step 1: Write the failing test** (in `session.rs` tests)

```rust
#[test]
fn live_outcome_falls_back_unless_it_succeeded() {
    let ok = LiveOutcome { transcript: "a".into(), speech_secs: 1.0, language: None, failed: false };
    assert!(usable(None).is_none());
    assert!(usable(Some(LiveOutcome { failed: true, ..ok.clone() })).is_none());
    assert_eq!(usable(Some(ok)).map(|o| o.transcript), Some("a".to_string()));
}
```
`LiveOutcome` derives `Clone, Debug`.

- [ ] **Step 2: Run the test and check that it fails**

Run the Rust gate with filter `live_outcome_falls_back`. Expected: compile error.

- [ ] **Step 3: Implement `live_worker.rs` and the `session.rs` wiring as specified.**

- [ ] **Step 4: Run the tests and check that they pass**

Run the Rust gate with filter `dictate::` (all Dictate tests). Then run, under the same lock, `cargo check -p dictate-input --target x86_64-pc-windows-msvc --tests`. Expected: PASS, with no warnings in the new files.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/dictate/live_worker.rs frontend/src-tauri/src/dictate/mod.rs frontend/src-tauri/src/dictate/session.rs
git commit -m "feat(dictate): live worker transcribes phrases while you speak, full decode as fallback"
```

Reviewer brief: run a Codex pass. Look hard for silent failures: a worker that exits without a `failed` outcome, a `select!` branch that can starve the finish message, a policy that is not put back after a commit, and text that reaches a log.

---

### Task 7: Changelog v2.5.0 (mechanical)

**Files:**
- Modify: `CHANGELOG.md:2` (new section above `## v2.4.0 — 2026-11-23`)

The version is not stamped locally. `release.yml:27` runs `node scripts/set-version.mjs "${GITHUB_REF_NAME#v}"` from the tag, and the past releases (`b6aab6e1` v2.4.0, `44ebaac9` v2.3.0) changed only `CHANGELOG.md`. No tag is created here.

- [ ] **Step 1: Add the section**

```markdown
## v2.5.0 — YYYY-MM-DD
- **See your words while you dictate.** The Dictate pill now shows what Conclave hears as you speak: final words in normal text, words still being guessed in grey. Pick Panel (three lines above the pill) or One line in Dictate → Live text.
- **Faster paste after long dictations.** Phrases are transcribed while you speak, so only the last one is left when you release the key.
```
Use the merge date for `YYYY-MM-DD`.

- [ ] **Step 2: Verify:** run `node --test scripts/*.test.mjs` (the changelog-section tests). Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): v2.5.0"
```

---

## PC checks (user, after the merge; from the spec's Testing section)

- Compare the `dictate: pasted … ms` lines for a 60 s dictation before and after.
- The preview lags speech by about 1–3 s.
- With a meeting recording, its transcript is not delayed.
- Russian and English dictations keep their language.
- There is no click-blocking area above the taskbar in either style.
- The panel grows upward smoothly (`dictate_pill_resize` cannot be tested on Linux).
