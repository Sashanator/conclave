# Live dictation: words in the pill while you speak

Status: draft, 2026-11-23. Background research: `reports/Streaming dictation for Conclave.md` (its steps 2 and 3). Earlier Dictate design: `docs/superpowers/specs/2026-10-18-conclave-dictate-design.md`.

## Why

Today Dictate shows nothing but a dot and level bars while you speak; the words appear only after the paste. The user wants to see what Conclave hears as they speak, the way Claude Code's voice mode does: final words in normal text, the words still being guessed dimmed. That is the only goal of this round. A shorter wait after releasing the key comes along for free, because the same change transcribes during recording.

## Goals

- While dictating (hold or hands-free), the pill shows the transcript live: **committed** text (final, never changes) and **volatile** text (grey, replaced on each update).
- Two looks, switchable in Dictate settings so the user can try both:
  - **Panel** (default): the pill grows upward into a 480 px wide panel, last 3 lines of text, older lines fade out at the top, status row (dot, level bars, timer; Stop and Cancel in hands-free) at the bottom where the pill sits today.
  - **One line**: a 560 px wide, 40 px tall pill; one line of text, newest words at the right, older words fade out at the left; dot at the left and timer at the right.
- On release, only the last open phrase is left to transcribe, so stop-to-paste time drops for long dictations.
- What gets pasted is unchanged in kind: the whole transcript, then the chosen mode's cleanup, then the private-clipboard paste.

Mockups (approved direction, both kept): `.superpowers/brainstorm/233486-1795447941/content/live-panel-v2.html`.

## Non-goals

- Typing into the target app while speaking (rejected: hotkey modifiers turn typed letters into shortcuts, no grey text, cleanup impossible, apps react to keystrokes).
- Cleanup hardening (research step 1), a streaming engine such as Nemotron (step 4), LocalAgreement-style stable prefixes, an "off" setting for the live text.
- Any change to modes, the dictionary, the hotkey, the mute-during-meetings rule or the paste.

## Anti-goals (a failure even if it works)

- The language flips mid-dictation because a half-second chunk was misdetected.
- A recording meeting's transcription is slowed by the preview.
- Text is lost because the live path failed; the user must always get at least what today's path would give.
- The pill steals focus, or leaves an invisible area above the taskbar that eats mouse clicks.
- Transcript text appears in logs.

## Approaches considered

1. **Whisper per phrase (chosen).** The VAD that Conclave already runs closes a segment at each pause; each closed segment is decoded once and becomes committed text. The open segment is re-decoded about every second for the grey preview. Uses the warm large-v3-turbo model and existing VAD; preview lags speech by about 1-3 s.
2. Re-decode the whole recording every second. Simplest, but cost grows with length and release still waits for a full decode. Rejected.
3. Natively streaming engine (Nemotron 3.5 via sherpa-onnx). Near-instant, but a 1.36 GB download, a new runtime with unverified Windows CUDA and DLL risks, and worse Russian WER. Rejected for now (research step 4).

The user chose 1; for the look, the user asked for both A (panel) and B (one line) behind a setting.

## How it works

### Audio to segments

- Capture stays as today (`dictate/capture.rs`): one open mic stream, pre-roll, the full session buffer is still kept until the dictation ends.
- New: a live worker per dictation drains new samples from the session buffer on a short period (about every 100-200 ms; the existing 100 ms tick in `session.rs:357` is a natural place) and feeds a **second `ContinuousVadProcessor`** (`audio/vad.rs:36`), separate from the meeting pipeline's. Redemption is the 400 ms Dictate already uses for its speech gate (`session.rs:581-589`). The processor resamples per call, as it does for meetings.
- **Max segment length** (my call): the VAD has none, so a phrase spoken without a pause would never commit. The worker force-closes an open segment at 20 s, cutting at the quietest 30 ms frame in its last 2 s.
- The open segment's samples (audio since the last closed segment) are the preview input.

### Decoding

- **Committed:** each closed segment is decoded once with `transcribe_audio_with_confidence` (`whisper_engine.rs:674`), through the dictation's own `LanguagePolicy` (already fresh per dictation, `session.rs:674`). Results are assembled in segment order even if they finish out of order.
- **Context prompt** (my call): each segment decode gets the tail of the committed text so far (about the last 200 characters) as the initial prompt, so sentences and casing continue across segments. Dictate passes no prompt today; the meeting path already does this kind of conditioning.
- **Volatile:** every ~1 s, if the open segment grew by at least 0.5 s since the last preview and no preview decode is in flight, decode it and replace the grey text with the whole hypothesis. At most one preview decode is pending at any time; a newer one never queues behind an older one.
- **Language** (my calls, from the research's per-segment rule):
  - **First decision over at least 3 s of speech.** Closed segments are held until the speech in them adds up to 3 s (or stop comes), then decoded together as one committed piece; that decode sets the dictation's language. A short "Okay, so…" never decides the language alone.
  - **After that, sticky with a confident switch.** Each later committed segment goes through the same per-dictation `LanguagePolicy`: under 1.5 s it reuses the current language (`language_policy.rs:32-41`); 3 s or more it may switch only on the policy's existing confidence rule (`language_policy.rs:51-62`), otherwise it keeps the current one. This serves German/Russian speakers who switch between sentences, and falls back to the dominant language for a foreign word inside a sentence.
  - **Prompt follows language.** The committed-text prompt is cleared whenever the language changes, so Russian text never conditions an English or German phrase.
  - **Previews never change the language.** They use a copy of the policy and their detections are thrown away. Before the first decision, the grey preview starts only once the open audio reaches 1.5 s and is detected within the allowed languages; a wrong grey guess in the first seconds is accepted because it is replaced and never pasted. Language is never re-detected per half-second for committed text.
- **Meetings:** all decodes share the engine's FIFO `inference_lock` with the meeting transcriber. While a meeting is recording, previews run at most every 3 s and are skipped when a meeting chunk is waiting for the lock. Committed segment decodes still run (they are needed for the paste) but are queued, never prioritised over meeting chunks.
- Model not loaded yet: the panel shows `Loading model…` (existing state) and segments wait; nothing is dropped.

### Stop, cancel, failure

- **Stop** (release, hands-free stop, 600 s cap): flush the VAD, decode the final open segment (plus any held-back segments) as committed, wait for pending segment decodes, discard any preview. The transcript is the committed segments joined in order. Then the existing flow runs unchanged: nothing-heard gate (0.5 s total speech, taken from the live VAD's count rather than a second VAD pass over the whole buffer), cleanup per mode, store "last dictation", paste.
- **Cancel** (Escape, pill Cancel, abandon): stop the worker, ignore any decode results that come back (epoch check, as the paste already does).
- **Fallback** (data-loss guard): if any live segment decode errors, or the live worker dies, the dictation falls back at stop to today's path: resample the full kept buffer and decode it in one call. The user still gets the full text; the log says why.
- After stop the panel keeps the committed text visible under `Transcribing…`, then `Pasted` (or the existing warning states), then fades as today. The panel shows the raw transcript; the pasted text is the cleaned version and may differ.

### The pill

- `PillView` (`dictate/pill.rs:16-36`) gains `committed` and `volatile` strings and the style. Text changes are emitted when they change, not on every 100 ms tick.
- Panel style: the window is bottom-anchored and sized to its content: height follows the number of text lines (0 to 3), so there is never a transparent area larger than today's margins. The pill is currently rebuilt at a fixed 360×56 (`pill.rs:12-13, 98-118`); it now needs a size per style and `set_size` plus re-positioning when the panel grows. It stays `focusable(false)`.
- One-line style: fixed 560 px wide, 40 px pill in a window of matching size. Left to right: dot, level bars, the text line (takes the remaining width), timer, and in hands-free mode Stop and Cancel (my call: the same controls as today, so mouse stop still works).
- Before any text exists, both styles look like today's pill (dot, level bars, timer, controls).
- Colours: the panel keeps the pill's forced dark palette in Light and Dark themes; with the Conclave theme selected it uses the Conclave tokens (indigo card, warm text). Committed text uses `--foreground`, volatile text `--muted-foreground`; both meet 4.5:1 on the panel. No hard-coded colours (DESIGN.md). Text is 15 px in the panel, 14 px in the one-line pill. No `aria-live` on the text (it would flood screen readers).
- All existing states and strings (`frontend/src/lib/dictatePill.ts:21-30`) stay.

### Setting

- New Dictate setting `live_text_style`: `panel` (default) or `line`. Added in the four places a Dictate setting lives (`dictate/settings.rs` struct, `Default`, `sanitize`, `normalise`) and the frontend type (`frontend/src/lib/dictate.ts`).
- Control on the Dictate page in `DictateGeneral.tsx`: "Live text" with two choices, Panel and One line (my call: on the Dictate page with all other dictation settings, not under Appearance; the user can override). Takes effect on the next dictation.

### Logging

Per dictation: number of segments, per-segment decode ms (the existing `whisper: chunk … transcribed in … ms` line), preview count and skips, stop-to-paste ms, chosen language, and whether the fallback ran. Never transcript text.

## Constraints

- Windows 10 x64, RTX 4070 Ti SUPER; Whisper large-v3-turbo stays warm while Dictate is on (existing).
- The Windows-only parts (hook, paste) can only be verified on the user's PC or Windows CI; the live worker, VAD, assembly and pill must be testable on Linux.
- No new dependency and no network call.

## Testing

- Rust unit tests: segment force-split at 20 s; first committed decode waits for 3 s of speech; prompt cleared on language change; in-order assembly with out-of-order completion; preview never changes the real policy's language; at most one preview pending; cancel ignores late results; fallback runs when a segment decode fails; settings round-trip and sanitise for `live_text_style`.
- Bun tests: pill renders both styles with committed and volatile text, the panel caps at 3 lines, existing states unchanged; Dictate page saves the new setting.
- PC checks (user): 60 s dictation stop-to-paste time before and after (from logs); preview lag feels like 1-3 s; with a meeting recording, its transcript is not delayed; Russian and English dictations keep their language; no click-blocking area above the taskbar.

## Left to the builder

The drain mechanism (periodic drain vs channel), the incremental resampling detail, exact window heights and line metrics, how "a meeting chunk is waiting" is detected, and how the worker is structured inside `session.rs`, as long as the rules above hold.

## Calls I made (for the user to check)

1. The setting sits on the Dictate page, not Appearance.
2. Panel is the default style.
3. Phrases are force-closed at 20 s of continuous speech.
4. Committed text is fed back as Whisper's prompt for the next phrase, cleared when the language changes.
5. The language is first decided over at least 3 s of speech, then kept unless a phrase of 3 s or more is confidently in another of your languages.
6. The one-line style keeps the level bars and, in hands-free mode, Stop and Cancel.
7. Grey text shows Whisper's whole latest guess, replaced each time (no stable-prefix logic).
8. During a meeting, previews run at most every 3 s.
9. If the live path fails, the full recording is transcribed the old way at stop.
