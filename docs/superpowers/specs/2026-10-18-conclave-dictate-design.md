# Conclave: Dictate (phase 1) — Design

Date: 2026-10-18
Status: Implemented in v0.7.0 (2026-10-19)
Target release: v0.7.0

## 1. Purpose

The user wants Wispr Flow inside Conclave: speak anywhere in Windows and get the text at the cursor of the focused app. Main uses are AI prompts (VS Code, Claude Code in a terminal) and capturing ideas, in Russian or English (rarely mixed: English terms in Russian speech). Transcription stays local on the GPU (Whisper, already in Conclave); only the text goes to the user's own cloud LLM key for cleanup.

The user has not used Wispr Flow; this design follows Wispr's defaults where the user had no preference (cleanup on by default, rewrite only on request, hold or tap, bottom pill).

## 2. Success criteria

1. With Dictate on, holding the main hotkey (default **Ctrl+Win**) in any normal app, speaking, and releasing pastes the text at the cursor of that app.
2. Tapping the main hotkey starts hands-free dictation; tapping it again stops and pastes. **Esc** cancels: nothing is pasted.
3. The default mode is **Cleaned up**: fillers ("uh", "ээ") and self-corrections ("5, no, 6" → "6") removed, punctuation fixed, wording and language kept.
4. Russian and English are both recognised without a manual switch.
5. A small pill at the bottom centre of the screen shows listening (level wave; timer, stop and cancel in hands-free), transcribing, pasted, or "couldn't paste, copied". It never takes focus.
6. If cleanup fails (no key, no network, timeout, error) the as-spoken text is pasted instead; what was said is never lost.
7. Dictating during a recorded meeting works, and the dictation does not appear in that meeting's audio, transcript, summary or speaker labels.
8. Whatever was on the clipboard before a dictation is back on the clipboard afterwards (text and image; see §10), and dictations do not appear in Windows clipboard history (Win+V), cloud clipboard sync or clipboard managers.
9. A Dictate page (new sidebar icon) holds every Dictate setting: on/off, main hotkey, default mode, cleanup model, modes, dictionary.
10. With Dictate off, Conclave behaves as today: no keyboard hook, no open microphone, no extra GPU memory. The only exception is the dictionary, which applies to meetings whenever it has words (§9); an empty dictionary means meeting transcription is exactly as today.
11. Rough latency target on the user's PC (RTX 4070 Ti SUPER, Whisper large-v3-turbo on CUDA), from key release to paste for a 15 s utterance: As spoken ≤ 1.5 s, Cleaned up ≤ 3 s with a fast cloud model. A target to measure, not a gate.
12. Windows 10 x64. No new bundled models; installer size unchanged within a few MB.

## 3. Decisions (with reasons)

| Decision | Who / reason |
|---|---|
| Dictate lives inside Conclave, works while Conclave runs (window or tray) | User |
| Three output kinds: **As spoken** (raw Whisper, no cloud), **Cleaned up** (default), and **custom modes** (named rewrite prompts) | User chose 3 levels, level 2 default; then took Superwhisper-style editable modes in place of a fixed level 3 |
| Sending dictated text to the user's cloud LLM is OK | User |
| Hold-to-talk **and** hands-free (tap to start / tap to stop) on the same hotkey | User wanted both; one key, told apart by press length, as Wispr does |
| Default main hotkey Ctrl+Win (Wispr's Windows default), changeable | Proposed, user accepted |
| Each mode may have its own optional hotkey that dictates straight into that mode | User |
| Mode choice: default mode on the Dictate page, quick switch in the tray | Proposed, user accepted |
| Indicator: small dark pill, bottom centre above the taskbar, no focus | User picked from mockups (A) |
| Dictation during a meeting recording is allowed and kept out of the meeting by silencing the mic in the meeting pipeline while dictating | User wanted it out of the meeting; mechanism is my call (one switch, no after-the-fact filtering) |
| No history page. Only the **last dictation** is kept, in memory only (as spoken + final), with tray items "Copy last dictation" and "Copy last dictation (as spoken)" | User found history not useful; this keeps the safety net |
| Personal dictionary in phase 1, shared with meeting transcription, and applied to meetings even when Dictate is off | User |
| The microphone stays open the whole time Dictate is on (short in-memory pre-roll buffer, nothing stored) | User. Their mic is wireless with a USB dongle (no Bluetooth profile switching), and an always-open stream never clips the first word. Cost: the Windows "microphone in use" icon stays lit while Dictate is on |
| Dictations are excluded from Windows clipboard history, cloud clipboard and clipboard monitors | User; matches "nothing about dictations is stored". Win+V cannot bring back old dictations; the tray's last dictation covers the latest |
| All Dictate settings on a new **Dictate** page in the sidebar; Settings gets no Dictate section | User |
| Cleanup model chosen separately from the summary model | My call: cleanup must be fast and cheap; summaries may use a slow, strong model |
| Whisper stays loaded on the GPU while Dictate is on; turning Dictate off frees it at once (or at recording stop if a meeting is recording) | User |
| Dictate is **off** after the update until the user turns it on | My call: no surprise global hotkey or GPU use (same rule as speaker ID) |
| Paste = put text on the clipboard, send Ctrl+V, restore the old clipboard | My call: typing character by character is slow, breaks on newlines in terminals (Enter would submit) and does not reach VMware guests |
| Dictate works only with the Whisper engine | My call: the user uses Whisper large-v3-turbo on CUDA; Parakeet support is not needed |

## 4. Current state (main 9905cd7)

- **LLM:** `summary/llm_client.rs:259` `generate_summary(client, provider, model_name, api_key, system_prompt, user_prompt, …)` makes one non-streaming completion for OpenAI, Claude, Groq, OpenRouter, Ollama, custom OpenAI-compatible and the local sidecar. API keys live in the SQLite `settings` row (`database/repositories/setting.rs:110` `get_api_key`). The template auto-pick already makes a short call with its own system prompt and a 30 s timeout (`summary/service.rs:384`).
- **Whisper:** global `WHISPER_ENGINE` (`whisper_engine/commands.rs:8`). `transcribe_audio_with_confidence(audio_16k_mono_f32, language, Option<&mut LanguagePolicy>)` (`whisper_engine.rs:667`). Input is 16 kHz mono f32. No initial prompt is passed anywhere today. The model is loaded at recording start (`audio/transcription/engine.rs:399`) and unloaded at recording stop (`audio/recording_commands.rs:979`) and after batch jobs (`audio/common.rs:21-46`, already skipped while recording).
- **Languages:** the user's "Languages spoken in your meetings" setting (`lib.rs:490` `get_allowed_languages_internal`) and `LanguagePolicy` (`whisper_engine/language_policy.rs`) restrict detection to the ticked languages.
- **Audio pipeline:** mic and system samples enter the mixer ring buffer at `audio/pipeline.rs:848` (`ring_buffer.add_samples`, `:62-74`) and are mixed in windows at `:852-859`. The mixed audio feeds the recording, VAD → Whisper, and the speaker-ID level track. Mic device: `RecordingPreferences.preferred_mic_device` (`audio/recording_preferences.rs:20`), fallback `default_input_device`. A Silero VAD exists (`audio/vad.rs`).
- **Banner window:** `banner.rs:92-101` creates a no-focus, always-on-top, no-taskbar window (`focused(false)`, `focusable(false)`) loading the `banner` route. The pill can copy this pattern.
- **Tray:** `tray.rs` builds the menu by id (`build_menu` `:303`, `handle_menu_event` `:42`), rebuilt via `update_tray_menu`.
- **Sidebar:** icon rail and expanded list in `components/Sidebar/index.tsx` (Home `:456`, Settings `:524`; expanded `:721`, `:800`), navigation via `go(href)`.
- **Feature flag pattern:** `speakers/settings.rs` — own tauri-store file, strict `Bool(true)`, cached `AtomicBool`, get/set commands.
- **Not present:** no global hotkey plugin, no keyboard hook, no `SendInput`, no native clipboard access. The `windows` crate is a dependency (`Cargo.toml:194`) without `Win32_UI_Input_KeyboardAndMouse` or `Win32_System_DataExchange`.

## 5. How a dictation works

1. **Trigger.** A low-level keyboard hook (needed because Ctrl+Win is a modifier-only combo, which `RegisterHotKey` and the Tauri global-shortcut plugin cannot express) watches for the main hotkey and the mode hotkeys. The hook is installed only while Dictate is on.
   - Press and hold longer than a short threshold (start at ~300 ms, tune on the PC) → **hold mode**; release ends the dictation.
   - Press and release within the threshold → **hands-free**; the next tap of the same hotkey, or the pill's stop button, ends it.
   - If any other key is pressed while the hotkey is held before the threshold (the user meant a Windows shortcut such as Ctrl+Win+→), the dictation is abandoned silently and the keys pass through untouched.
   - **Esc** while dictating cancels; that Esc is swallowed so it does not reach the app.
   - Hands-free stops automatically after **10 minutes** (my call) and processes what it has.
   - Releasing Ctrl+Win must not open the Start menu. If Windows opens it, the builder suppresses it (for example with a masking key event, as AutoHotkey does) and confirms on the user's PC.
2. **Capture.** While Dictate is on, Conclave keeps its own mic stream open (the preferred mic, else the default) feeding a small in-memory ring buffer (pre-roll, start at ~500 ms, tune) that is continuously overwritten and never used for anything else. On trigger the dictation takes the pre-roll plus everything until stop. Audio is never written to disk. The stream is reopened when the preferred mic changes or the device disappears and returns; while no mic is available the hotkey shows "No microphone" in the pill. This is a separate stream from a meeting's mic capture (WASAPI shared mode allows both); for the meeting side see §7.
3. **Transcribe.** On stop: resample to 16 kHz; if Silero VAD finds no speech (or under ~0.5 s of speech), paste nothing and show "Didn't catch that" — this also prevents Whisper's silence hallucinations ("Thank you.", "Продолжение следует…"). Otherwise transcribe the whole utterance in one call with language detection restricted to the user's "Languages spoken" setting (see §6) and no initial prompt: the dictionary is not sent to Whisper (see §9).
4. **Process** with the chosen mode (§8): As spoken → done; Cleaned up or a custom mode → one cloud call with the cleanup model.
5. **Paste** (§10), show ✓ Pasted, keep it as the last dictation, fade the pill after ~1 s.

Only one dictation runs at a time; a hotkey press while one is processing is ignored.

## 6. Language

Detection is restricted to the ticked "Languages spoken in your meetings" (the same setting and `LanguagePolicy` meetings use; the user ticks Russian and English, possibly German). Whole utterances are long enough for reliable detection; for utterances under ~1.5 s the language of the previous dictation is used. The user rarely mixes languages; English terms inside Russian speech are expected to come out right with the dictionary's help. The ticked list reaches Rust only when the UI has loaded; until then, or if it is empty, dictation uses plain auto-detection. Dictation never uses the "auto-translate" option. No separate dictation-language setting in phase 1 (my call); if German in the meeting set causes misdetections in dictation, a separate setting is the fix.

## 7. Dictating during a meeting recording

- While a dictation is capturing, the meeting pipeline replaces mic samples with silence at the mix input (`pipeline.rs:848` or inside `add_samples`), from hotkey press until release plus a short tail to cover device latency (start at ~300 ms, tune). System audio keeps recording, so other people are still captured.
- Effect: the dictation is absent from the meeting's recording, live transcript, summary and the speaker-ID level track (the mic is simply quiet).
- Dictation and live meeting transcription share the one loaded Whisper model. A dictation may wait for a live chunk in progress; the builder checks how `WhisperEngine` serialises calls and gives dictation no worse than "next in line".
- Not affected: whether people on the call hear the user. That is the call app's mute button; the user mutes there.

## 8. Modes

- **As spoken** (built-in): Whisper text after Conclave's existing light cleanup, no cloud call.
- **Cleaned up** (built-in, default): one cloud call. Remove fillers and false starts, apply spoken self-corrections, fix punctuation and capitalisation, format spoken lists as lists. Keep the speaker's words, meaning and **language** (never translate). Its prompt is fixed in code, not user-editable (my call).
- **Custom modes** (user-made): name, instructions (free text), up to 3 examples (what I said → what I want), optional hotkey. One starter mode ships: **Prompt for Claude** — turns the dictation into a clear, structured prompt for a coding assistant, keeping every requirement the user said. The user can edit or delete it.
- **Anti-goal, applies to every cloud mode:** the cleanup model must treat the dictation as text to edit, never as a request to answer. Dictating "write a function that parses dates" must paste that sentence (cleaned), not a function. The system prompt says so, the dictation is wrapped as quoted data, and the builder adds a few test dictations that are questions or commands to check this with each starter prompt.
- Every cloud mode also gets the dictionary as "correct spellings".
- Cleanup call: the existing `generate_summary` path with the cleanup provider/model and the key already stored for that provider; timeout ~10 s (my call). On any failure: paste as spoken, pill says "Pasted as spoken (cleanup failed)".
- Mode selection: the default mode (Dictate page or tray) is used by the main hotkey; a mode's own hotkey uses that mode once without changing the default.

## 9. Personal dictionary

- A list of words and names (e.g. Conclave, Tauri, Claude Code, people's names). Starts empty; the user adds words as they notice mistakes.
- Used by dictation as spelling hints in cloud cleanup modes only ("As spoken" gets none), and by meetings as a spelling line in the summary prompt. Meetings use it whenever it has words, **regardless of the Dictate on/off switch** (user decision); the Dictionary section of the Dictate page stays editable while Dictate is off.
- **Risk, kept as history of the gate:** a Whisper initial prompt can bias language detection and cause hallucinations on short chunks, which is exactly the problem the 2026-10-15 quality work fixed. The gate measured it on the saved comparison recordings (`.superpowers/quality-debug/`) and it failed: Russian chunks flipped to Latin script. So neither path passes an initial prompt to Whisper.
- Empty dictionary = no spelling line = today's behaviour.
- Chunk summaries of long meetings don't get the spelling line; the final summary pass does.

**Gate result (2026-10-19):** the Whisper-prompt measurement failed (Russian chunks flipped to Latin script; details in `docs/superpowers/plans/2026-10-19-dictate-dictionary-gate.md`). Two decisions follow:
- **Meetings:** no Whisper initial prompt. A non-empty dictionary is added to the summary system prompt as `Correct spellings of names and terms: …`.
- **Dictation:** no Whisper initial prompt either. Cloud cleanup modes get the dictionary as spelling hints; "As spoken" gets no dictionary help.

## 10. Paste and clipboard

- Every dictation placed on the clipboard also carries the Windows formats that keep it out of clipboard history, cloud clipboard and clipboard monitors (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0, `CanUploadToCloudClipboard` = 0). The restore of the old content must not add a second history entry (restore with the same exclusion formats or a method that does not re-trigger history; builder checks on Windows). The "couldn't paste" case keeps the text on the clipboard with the same exclusion formats; tray "Copy last dictation" uses them too.
- Save the current clipboard (text and image formats; other formats such as files or rich text are not restored — `shortcut:` limit, revisit if the user hits it), put the dictation text on it, send Ctrl+V to the foreground window, wait, then restore. The wait before restoring must be long enough that the target app has read the clipboard (start at ~500 ms, tune on the PC, including a VS Code terminal and a browser).
- The paste goes to whichever window is in the foreground at paste time (as Wispr does).
- **Couldn't paste:** if there is no suitable foreground window, the foreground window is Conclave's own, or injection fails (`SendInput` error, or an elevated/admin app that blocks input from a normal process), the text stays on the clipboard (no restore) and the pill says "Couldn't paste — copied, press Ctrl+V".
- The paste step is one function with the target as an input, so a later VMware mode (Ctrl+Shift+V into the guest) is a small addition.
- Known limits, not fixed in phase 1: while an elevated (admin) window is in front, the low-level hook receives no keys, so the hotkey does not fire there at all ("couldn't paste" happens only if focus moves to an elevated app during a hands-free dictation); the hotkey may not fire while a VMware VM has grabbed the keyboard (VMware's own hook). Both noted on the Dictate page help text and in the manual checklist.

## 11. The pill

- New window modelled on `banner.rs` (no focus, not focusable, always on top, no taskbar entry), placed bottom centre just above the taskbar on the monitor with the foreground window (my call; fallback: primary monitor).
- States: Listening (red dot + live level wave, ~20 updates/s); Hands-free (timer, stop, cancel ✕ — clickable without stealing focus from the target app; the builder verifies this on Windows, else keyboard-only stop/cancel); Transcribing…; ✓ Pasted (fades after ~1 s); "Pasted as spoken (cleanup failed)"; "Couldn't paste — copied, press Ctrl+V" (stays ~4 s); "Didn't catch that".
- Mockups: `.superpowers/brainstorm/437744-1792333574/content/dictate-indicator-v2.html` (option A).

## 12. The Dictate page

New sidebar icon (rail and expanded list) → route `/dictate`. Wireframe: `.superpowers/brainstorm/12814-1792344645/content/dictate-page.html`.

- **General:** Dictate on/off; main hotkey (click, press the combo, saved; must include at least one modifier, conflicts with mode hotkeys rejected); default mode; cleanup model (provider + model, only providers that have a key or endpoint configured); short help text with the known limits.
- **Modes:** list with As spoken and Cleaned up (built-in tags, hotkey settable, not editable/deletable) and custom modes (hotkey, edit, delete; "+ New mode" opens an editor with name, instructions, examples).
- **Dictionary:** input + Enter adds a word; words shown as removable chips.
- If the transcription engine is not Whisper, or no Whisper model is downloaded, the on switch is disabled with a one-line reason.

## 13. Tray

- "Copy last dictation" and "Copy last dictation (as spoken)" (disabled until the first dictation in this run).
- "Dictate mode" submenu: radio list of modes, sets the default.
- Only shown while Dictate is on.

## 14. Model lifetime

- Dictate on (toggle or app start with Dictate on): install the hotkey hook, open the mic stream (§5), and load the selected Whisper model in the background; the pill shows "Loading model…" if a dictation starts before it is ready, and the dictation still completes.
- Recording stop and batch-job unloads skip the unload while Dictate is on.
- Dictate off: remove the hook, close the mic stream, and unload the model now unless a meeting is recording or a batch job runs (then their own end-of-work unload applies, which no longer skips).
- Changing the Whisper model in Transcription settings while Dictate is on loads the new one.

## 15. Storage

- One tauri-store file `dictate.json` (my call, same pattern as `speakers.json`): enabled, main hotkey, default mode id, cleanup provider + model, custom modes, mode hotkeys, dictionary. Validation on load: unknown or broken entries are dropped with a log line, never a crash.
- The dictionary is read by the meeting transcription paths from the same store.
- Nothing about dictations themselves is stored: no audio, no text on disk. The last dictation lives in memory until quit.
- Logs never contain dictated text (lengths and timings only).

## 16. Network

No new hosts: the cleanup call goes to the provider the user configured, like summaries. Add the Dictate cleanup call site to `docs/network-calls.md` (runtime-built URLs table) and keep `scripts/check-no-phone-home.sh` passing.

## 17. Testing

- **Linux (CI and container):** unit tests for the hotkey state machine (hold vs tap vs abandoned shortcut vs Esc vs 10-min cap), mode prompt building (wrapping, dictionary hints, examples), the fallback rules (cleanup failure → as spoken; paste failure → keep clipboard), short-utterance language choice, store validation, hotkey conflict validation, the mic-silencing window logic in the pipeline, and the "skip unload while Dictate on" rule. Frontend Bun tests for the Dictate page logic.
- **Windows CI:** `cargo check` of the Windows-only hook, `SendInput` and clipboard code; unit tests where they do not need a desktop session.
- **Manual on the user's PC (checklist in the plan):** Ctrl+Win hold and tap in VS Code terminal, browser, Notepad, Telegram; Start menu not opening; Esc; clipboard restored (text and image); elevated app shows "copied"; dictation during a Zoom recording missing from the meeting; latency numbers; GPU memory freed when off.
- Cleanup prompt check: the question/command dictations from §8 with each starter prompt on the user's cleanup model.

## 18. Out of scope (later phases)

- **Phase 2:** command mode (select text, say what to do); per-app tone / automatic mode by app; clipboard and app context in the cleanup prompt.
- **Phase 3:** snippets (say a phrase, get saved text); idea inbox.
- **Later:** VMware guest paste (Ctrl+Shift+V into the VM window); dictation history; Parakeet support; a separate dictation-language setting; streaming the cleanup.
- **Skipped:** screen reading for context (Superwhisper), style learning (Willow).

## 19. Left to the builder

Exact thresholds and delays (marked "tune"), hook thread design, how the pill receives levels and state, the cleanup prompts' wording (within §8), the mode editor layout, resampling choice, and the Win32 crate features vs a small clipboard crate (prefer the existing `windows` crate; no new dependency for a few calls).
