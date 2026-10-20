# Conclave Speaking Practice: design

Date: 2026-10-20. Status: draft for review. Branch `practice`.

Marking: **(user)** means the user decided it in brainstorming. **(call)** means the designer decided it and the user can overrule it. Unmarked items follow from these or from Conclave's existing rules.

## 1. Intent

The user speaks English as a second language and wants to practise spoken English for **job interviews** and the **defence of their master's thesis**, plus casual conversation **(user)**. Today they use ChatGPT voice mode, and it fails them in three ways **(user)**:

1. It loses track of the conversation.
2. It interrupts them while they think.
3. It says "wait, I'm thinking" and goes silent, which kills the mood.

ChatGPT also never tells them what they did wrong. Conclave's Practice mode fixes all four:

- The user talks to a character, such as a strict professor or an interviewer, that stays in role.
- It never interrupts the user.
- It answers quickly.
- Afterwards it writes a report on the user's English.

Success looks like this: the user picks Practice over ChatGPT voice mode for interview and thesis preparation, and the report changes how they speak in the next session.

## 2. Goals, non-goals, anti-goals

**Goals (phase 1)**
- Saved, reusable **setups** **(user)**. Each setup has a character, the situation in the user's words, and optional real material (job posting, CV, thesis PDF) **(user)**.
- A **call-view** session **(user)** with **tap to talk / tap when done** **(call; the user accepted both modes, the designer picked this one)**.
- A smart **preparation** step before the call when material is attached: the examiner's plan **(user accepted)**.
- Fast character replies with the whole conversation in context.
- In-character **light corrections** during the call, also listed in the report **(user)**.
- A **report** with **(user)**:
  1. grammar mistakes;
  2. unnatural phrasing;
  3. places that could be said more clearly;
  4. filler-word counts and fluency.
- Each report item quotes the user's real words, with ▶ playback of that moment **(user approved the mockup)**.

**Goals (phase 2, same spec, later plan)**
- Pronunciation hints from a **local** model, free, voice stays on the PC **(user)**. Each hint has ▶ you / ▶ reference.
- "Keeps coming back": mistakes that repeat across the last sessions and filler trends **(user approved the mockup)**.

**Non-goals**
- Judging the *content* of answers. The user's other project, Midas, will do that. Conclave only fixes English **(user)**.
- Any Midas integration now. Keeping the data easy to export later is enough **(user)**.
- German or other languages in phase 1 (English only; German later) **(user)**.
- Cloud TTS (ElevenLabs), Azure pronunciation scoring, and any paid speech service **(user: "I don't want to pay Microsoft or anyone")**.
- Running the conversation on a local LLM in phase 1 **(user agreed: cloud LLM brain)**.
- Automatic end-of-turn detection, hands-free mode, voice cloning **(call)**.
- Several characters in one setup **(call)**.
- A cost meter **(call)**.

**Anti-goals** (a failure even if it technically works)
- The character interrupts the user, or talks over them.
- Silence while waiting, or any "please wait" / "I'm thinking". A reply that starts after about 3 s feels broken. The target is about 1–1.5 s from the tap to the first spoken word.
- The character forgets earlier answers or drops its role.
- The report shows a "mistake" the user never said. Every quote must be checked against the transcript.
- The user's voice leaves the PC. Only text goes to the cloud.
- Practice breaks or slows meetings or Dictate.
- Conclave deletes files on its own or outside the recordings folder (standing user rule).

## 3. Constraints

- **Platform:**
  - Windows 10 x64, RTX 4070 Ti SUPER (16 GB VRAM), shared with Whisper and anything else running.
  - Development happens in a Linux container with no GPU. Windows-only behaviour is verified in CI and on the user's PC (see `AGENTS.md`).
- **Self-contained code, easy to delete (user).** All new code lives in:
  - Rust module: `frontend/src-tauri/src/practice/`
  - Frontend route: `frontend/src/app/practice/`
  - Components: `frontend/src/components/Practice/`
  - Frontend library: `frontend/src/lib/practice.ts`
  - Tests: `frontend/tests/**/practice*`
  - Database: one migration that creates only `practice_*` tables.

  Practice may **call** existing functions (Whisper engine, settings repository, folder guard, recycle helper, ONNX runtime setup). It must not change how those behave for meetings or Dictate. Allowed touch points outside the module:
  - `lib.rs`: the `mod` line, commands, and the setup hook.
  - `dictate::keeps_model_loaded()` (`dictate/mod.rs:60`): one added condition, "or a Practice session is running" **(call, §5.7)**.
  - `Cargo.toml`: new dependencies.
  - The Sidebar entries.
  - `docs/network-calls.md`.
  - The migration file.

  The plan must list every touch point so that removal is a short checklist.
- **No telemetry.** New outbound hosts:
  - `api.anthropic.com`, already allowed.
  - The TTS model host, pinned URL plus SHA-256 like `speakers/models.rs`.

  Both go into `docs/network-calls.md`. The no-phone-home deny-list must stay clean.
- **Installer stays under about 100 MB.** Models download on first use.
- **Cost first.** Expected costs (designer estimates, 2026-10-06 price list):
  - About $0.04 per 30-minute conversation on Claude Haiku 5.5.
  - About $0.20 for one preparation pass on Opus 5.5, once per setup (§5.3).
  - About $0.15 for one report on Opus 5.5.
- **The user's Claude API key** is already stored in Conclave (`settings.anthropicApiKey`; read via `SettingsRepository::get_api_key("claude")`, `database/repositories/setting.rs:110`). Practice requires it. Without it, the page explains how to add one.

## 4. Experience

### 4.1 Practice page (user approved the mockup)

- **Navigation:** a new sidebar entry "Practice" below Dictate, in both the collapsed and expanded sidebar (pattern at `components/Sidebar/index.tsx:447-481` and `:749-759`).
- **Layout:**
  - Title "Practice". Two cards side by side on a wide window: **Your setups** on the left, **Past sessions** on the right. On a narrow window they stack, setups first **(user)**.
  - The look follows the Dictate page and theme tokens (`bg-card`, `border-border`, `text-muted-foreground`, primary), so Light, Dark and the Conclave theme all work.
- **Your setups:** a list of rows (name, small material badges, "character · N sessions") with **Edit** and **▶ Start**. Plus **+ New setup**.
  - First launch seeds three editable, deletable examples **(call)**: "Job interview" (an interviewer for a software role), "Thesis defence" (a sceptical examiner), "Casual chat" (a friendly car mechanic).
- **Past sessions:** newest first. Each row shows the setup name, date, length and a one-line summary (for example "4 grammar · 31 fillers"), with **Open report** and a delete action.
- **Setup editor:** opens in place, like Dictate's mode editor. Fields:
  - **Name**
  - **Character:** free text, e.g. "Prof. Weber, strict examiner".
  - **The situation:** free text, the user's own words.
  - **Material** (optional): add PDF, `.txt` or `.md` files, or paste text; each shows as a chip with ✕.
  - **Voice:** a dropdown of the TTS model's English voices, with ▶ preview.

  Buttons: Save, Cancel, Delete setup. The mockup's Language field is dropped in phase 1: English is fixed **(call)**.
- **Material limits:** total material per setup up to 20 MB and within Claude's PDF limit of 600 pages **(call)**. The editor refuses larger files with a clear message.

### 4.2 Session (call view; user)

1. **▶ Start.** If the setup has material and no current plan, the screen shows "Preparing *character*'s questions…" with a Cancel button while the preparation step runs (§5.3). With a current plan it starts straight away. It shows the same "Preparing…" while Whisper and the TTS model load.
2. **The character speaks first,** opening the scene, e.g. "Good morning, please have a seat…" **(call)**.
3. **The screen:** the sidebar stays. In the centre:
   - an avatar showing the character's initial;
   - the name and state: *speaking… / your turn / listening… / thinking…*;
   - the last line as a caption;
   - a big **Tap to talk** button, also the Space bar while the window is focused;
   - "Show full transcript ▾", which expands the turn list;
   - **End session → report** at the top.
4. **The user's turn:**
   - The user taps; the state becomes *listening…*; the mic records. They tap again when done.
   - The app never ends the user's turn by itself.
   - A safety cap of 5 minutes per turn ends it automatically, with a note **(call)**.
5. **Interrupting the character:** tapping while it speaks stops playback at once and cancels the rest of that reply. The user's turn starts. The transcript keeps only the sentences actually played.
6. **No reply:** if the recognised text is empty or under 0.5 s of speech, the screen says "I didn't catch that", stays on the user's turn, and makes no LLM call.
7. **Ending:** the session ends when the user clicks End, at a 60-minute cap **(call)**, or when a meeting recording starts (§5.7). Then the report pipeline runs (§5.4) and the report opens when ready, with progress shown.

### 4.3 Report (user approved the content; the layout is fixed per user feedback)

- **Header:** setup name, date and length. Tabs: **Report** and **Transcript**.
- **Layout:**
  - Content is width-capped and centred, so it doesn't stretch across a wide monitor **(the user found the two-column mockup awkward)**.
  - A **stats strip** at the top: fillers, words/min, long pauses.
  - Below it, one column of cards **(call)**:
    1. **Grammar (n):** what was said with the mistake struck through, the fix, a one-line rule, ▶ time, and a "*character* echoed this" tag when the character corrected it in the call.
    2. **Sounds unnatural (n):** what was said, what a native speaker would say, and a short why.
    3. **Could be clearer (n):** the user's long version, a tight version, and ▶ time.
    4. **Fillers and fluency:** a per-filler breakdown under the strip.
    5. **Pronunciation hints** (phase 2), marked "may be off": word, ▶ you, ▶ reference.
    6. **Keeps coming back** (phase 2).
- **▶ time** plays the user's own recording from the start of that turn. The time shown is the session time.
- **Transcript tab:** all turns, user and character, with times. User turns show the word-for-word text (§5.4).
- **Failure:** if the report fails, for example offline, the session and transcript stay saved, and the report card shows the error with **Retry**.

## 5. How it works

### 5.1 Session loop (Rust, `practice/`)

```
tap ─▶ mic capture ─▶ tap ─▶ resample 16 kHz ─▶ Whisper (en)
   ─▶ Claude Haiku 5.5 stream ─▶ sentence splitter ─▶ TTS per sentence ─▶ speaker playback
```

- **Mic:**
  - Use the user's preferred microphone from recording preferences, falling back to the default, as Dictate does (`dictate/capture.rs:92`, `session.rs:499-503`).
  - Practice opens its own capture only while a session runs. It records only during the user's turns. Echo is not a concern because the mic is closed while the character speaks.
  - Reusing Dictate's capture code without changing it is the builder's choice.
- **Recording:** each user turn is appended to one session file, `you.wav` (16 kHz mono). Its start offset in that file and its session time are stored with the turn. Character audio is not saved: ▶ reference and replays can be re-synthesised.
- **Whisper:**
  - Practice needs the local Whisper provider and uses the user's configured Whisper model (today large-v3-turbo on CUDA). If the provider is Parakeet, ▶ Start is disabled with an explanation, the same gate Dictate uses (`get_or_init_whisper` refuses non-Whisper providers, `audio/transcription/engine.rs:343`) **(call)**.
  - Use the global engine (`get_or_init_whisper`, `audio/transcription/engine.rs:234`), with English forced: `language = Some("en")`, no auto policy.
  - **Long answers must not cost waiting time (call).** The latency target applies to 2–3-minute answers too. Practice transcribes the user's speech in chunks at natural pauses (Silero VAD) *while they are still talking*, so after the second tap only the last chunk is left.
  - Take the engine lifecycle lock the way Dictate does (`audio/common.rs:11-16`).
  - Free Whisper after the session unless Dictate or a recording keeps it loaded (`dictate::keeps_model_loaded`, `audio/common.rs:38`).
  - While a session runs, `keeps_model_loaded()` returns true (§3 touch point). Meeting stop, import or retranscription end therefore leave the model loaded. Those paths already consult this function (`audio/common.rs:38`, `audio/recording_commands.rs:962`).
- **LLM:**
  - A new streaming Claude client inside `practice/`: SSE over `https://api.anthropic.com/v1/messages`, `x-api-key`, `anthropic-version: 2023-06-01`, as in `summary/llm_client.rs:370-381`.
  - Conclave has no Claude streaming today.
  - Model `claude-haiku-5-5`, thinking disabled at `low` effort (allowed on Haiku 5.5 at `high` or below), `max_tokens` around 400.
  - Prompt caching on the system prompt and the growing conversation prefix.
- **Character prompt** **(call; wording is the builder's)**:
  - The system prompt carries the character, the situation, the plan (§5.3) if any, and rules:
    - stay in role;
    - spoken style, usually 1–3 sentences;
    - no lists or markdown;
    - one question at a time;
    - never say "please wait" or "let me think";
    - when the user makes a clear mistake, occasionally repeat it back correctly inside a natural reply, without stopping to teach;
    - if a reply needs a moment, start with a short natural reaction ("Hmm, interesting choice…").
  - The full conversation so far is always sent.
- **Speaking:** the stream is split into sentences. Each finished sentence goes to TTS at once, and playback starts after the first sentence. Barge-in cancels the HTTP stream, the TTS queue and playback.
- **Playback:** a Rust-side output stream on the default output device. The playback component is the builder's choice. There is no existing speech playback (only a silent Bluetooth wake stream, `audio/recording_manager.rs:63`).

### 5.2 TTS (local)

- **Default:** Kokoro-82M as an ONNX model through the bundled ONNX Runtime. DirectML first with CPU fallback, using the same pattern as `speakers/runtime.rs:91-136`.
  - English voices only.
  - The model and voices download on first use from a pinned host with SHA-256 checks, using the `speakers/models.rs` downloader pattern, into `app_data/models/practice/`.
- **Phase-0 spike (§7):** Kokoro needs text-to-phoneme conversion, and its usual helper, espeak-ng, is a GPL C library that is awkward to bundle on Windows. The spike picks one of:
  - Kokoro with a Windows-friendly converter (a dictionary plus a fallback, or bundled espeak-ng; GPL is acceptable for this personal app);
  - another local TTS that needs no espeak.

  The result is recorded in this spec before the plan is written.
- **Requirements:** first audio of a sentence within about 300 ms on the user's GPU; at least 4 distinct English voices (male/female, US/UK); no cloud.

### 5.3 Preparation (material attached)

- **Model:** `claude-opus-5-5`, effort `high` **(user asked; designer recommended high over the default medium)**. PDFs go as `document` blocks, text as text. Request limits: 32 MB, 600 pages.
- **One plan per setup (user).** The plan is built once and stored with the setup. It is rebuilt automatically only when the setup's material or situation changes, or when the user clicks **New questions** in the setup editor.
- **Output:** an examiner plan as text:
  - a pool of about 15–20 questions;
  - the weak points to probe;
  - likely follow-ups;
  - facts from the material the character should know.
- **Variety:** each session, the character picks a different mix from the pool, so repeated sessions don't replay the same defence. How the mix is chosen (e.g. a random subset and order passed in the session prompt) is the builder's call.
- **Visibility:** the plan stays hidden from the user. Each session stores a copy of the plan it used **(call)**.
- **No material:** no preparation pass. The situation text alone goes into the character prompt.
- **Failure:** if preparation fails, the user can retry, or start anyway without the plan.

### 5.4 Report pipeline (after End)

1. **Word-for-word re-transcription.**
   - Each user turn is re-transcribed from `you.wav` with English forced and a disfluency prompt (e.g. "Um, uh, so, like, I mean…") so fillers and false starts stay.
   - This text is stored as the turn's *verbatim* text and is what the report and transcript use. The live text is kept only for the conversation itself.
   - The exact settings are tuned in the phase-0 spike.
   - **Known risk:** prompts like this can make Whisper hallucinate or loop. The spike measures it. A suspected loop (repeated n-grams) falls back to the live text for that turn.
2. **Fluency metrics,** computed in Rust:
   - **Fillers:** counts per filler from a fixed list (um, uh, er, like, you know, so basically, I mean, kind of, sort of). Exact rules (e.g. "like" only as a filler) are the builder's.
   - **Words per minute:** words divided by speech time in the user's turns.
   - **Long pauses:** silences over 1 s inside a turn, not at its start or end, detected with the existing Silero VAD on `you.wav`.
3. **Language feedback:** one `claude-opus-5-5` call at `high` effort. It gets the full transcript (character lines plus verbatim user turns, with turn numbers) and returns structured JSON with three lists: grammar, unnatural, clarity. Each item holds:
   - the turn number;
   - the exact quote;
   - the improved version;
   - a one-line note;
   - for grammar, whether the character echoed the correction.
4. **Validation (anti-goal "invented mistakes"):** Rust drops any item whose quote does not appear in that turn's verbatim text after normalising case, punctuation and whitespace. The report is stored as JSON with the session.

### 5.5 Data

- **One migration, `practice_*` tables only** **(call; column details are the builder's)**:
  - `practice_setups`: id, name, character, situation, voice, timestamps, seeded flag, plan text, and a fingerprint of the material and situation the plan was built from (a mismatch marks the plan stale).
  - `practice_materials`: id, setup id, kind (pdf / text), display name, content as a BLOB or text, byte size. Stored in the database, so deleting a setup deletes no files.
  - `practice_sessions`: id, setup id (set null if the setup is deleted), a snapshot of setup name and character, start time, duration, folder path, plan text, report JSON, report status and error.
  - `practice_turns`: session id, index, speaker (user / character), live text, verbatim text, session time, offset and length in `you.wav`.
- **Session audio:**
  - A folder **directly inside the recordings folder**, e.g. `Practice 2026-10-20 18-05 Thesis defence/`, so it passes `guarded_meeting_folder` (`audio/meeting_folder.rs:41`), which requires the canonical parent to be the recordings base.
  - Playback in the UI uses the asset protocol, allowing just that file as `get_meeting_audio` does.
- **Deleting a session:** the folder goes to the Recycle Bin through the existing guard and `recycle` helper (`meeting_folder.rs:27`), then the rows are removed. Nothing is ever deleted automatically.
  - Practice has its own delete function. The meeting delete refuses folders holding unknown files, so it can't be reused. Practice's delete uses the same guard and `recycle`, with an allow-list of Practice's own files (`you.wav`).
  - A folder holding anything else is refused and reported, never partly deleted **(call; strict reading of the deletion rule)**.
  - The existing old-audio review reads only the `meetings` table (`audio/retention.rs:60`), so it never touches practice folders. That is correct for phase 1 **(call)**.
- **Deleting a setup:** past sessions stay, keeping their snapshot name.

### 5.6 Models and settings

Fixed in code, no settings in phase 1 **(call; user asked about effort, see §5.3)**:

| Step | Model | Effort and thinking |
|---|---|---|
| Conversation | `claude-haiku-5-5` | thinking disabled, effort `low` |
| Preparation | `claude-opus-5-5` | effort `high` |
| Report | `claude-opus-5-5` | effort `high` |

### 5.7 Living with meetings and Dictate

- **Meeting recording, import or retranscription running:** **▶ Start** is disabled with a reason.
- **A recording starts during a session:** Practice watches `is_recording_now()` (`audio/recording_commands.rs:1098`), ends the session cleanly (saving everything), and tells the user. Practice code never blocks or changes meeting recording.
- **Dictate:** it keeps working. Its own mic stream is independent. Practice adds no special handling **(call)**.
- **Things the user starts during a session** **(call)**:
  - Imports, retranscription and Whisper model changes are not blocked; they are the user's own actions.
  - They may slow the next reply. A model change may cause a one-off reload stall.
- **Report after a meeting starts mid-session:** the word-for-word re-transcription waits until the meeting recording stops, so it never competes with the meeting's live Whisper **(call)**.
- **GPU:** Whisper, TTS and any speaker-analysis job share the GPU. No new global lock. The latency target is checked on the user's PC with Dictate on.

## 6. Phase 2 (later plan, same spec)

- **Pronunciation hints:**
  - A local phoneme-recognition model (ONNX, e.g. a wav2vec2 phoneme model) aligned against the expected words of the verbatim text. It flags words whose sounds differ, labelled "may be off".
  - ▶ reference re-synthesises the word with the TTS voice.
  - Accuracy is modest (roughly 0.7 agreement with human raters per phoneme in published results). The UI presents these as hints, never as grades.
- **Keeps coming back:**
  - Each report item gets a short pattern key (e.g. "if + will", "make research").
  - The page counts keys across the last 5 sessions of the same setup, and of all setups, and shows the filler trend.

## 7. Phase 0: spike before the plan (throwaway)

1. **TTS on Windows:** settle §5.2 (converter choice, first-audio latency, voices, VRAM).
2. **Haiku 5.5 in role:** measure time to first token from the user's PC, and check role-keeping and light recasts on a scripted 10-turn thesis defence. If it fails, try Sonnet 5.5 with thinking off (`between_tools`).
3. **Word-for-word transcription:**
   - The user reads 10 prepared sentences with deliberate mistakes and fillers.
   - Compare plain Whisper with the disfluency-prompt settings: how many mistakes and fillers survive, and any hallucination.

The spike results update this spec (§5.2, §5.4, §5.6) before planning. Items 2 and 3 need the user's PC, Claude key and voice. The spike delivers small scripts, and the user runs them (about 30 minutes).

## 8. Left to the builder

- Module layout inside `practice/`, state-machine shape, and the Tauri command and event names.
- The exact prompts.
- The SSE parser.
- The sentence splitter.
- Filler-matching rules.
- Report JSON schema details.
- The playback library or stream.
- Seed-setup wording.
- Avatar style.
- Column names.
- How the stats strip and cards look within the theme.
- Test strategy: unit tests for pure logic (sentence splitting, quote validation, fluency metrics, SSE parsing, state machine), and a fake LLM and fake TTS for the session loop, following the repo's existing Rust and Bun test patterns.

## 9. Release

Phase 1 ships as **v0.8.0** **(call)**. Phase 2 ships in a later minor release.
