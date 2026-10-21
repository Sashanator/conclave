# Conclave: Live copilot — Design

Date: 2026-10-19
Status: Implemented in v0.9.0 (2026-10-21)
Target release: v0.9.0 (Speaking Practice ships as v0.8.0)

## 1. Purpose

During a call the user sometimes drifts off, hears a language or a term they don't follow, or wants a good question to ask. The live copilot answers these on demand from the live transcript, using the user's own cloud LLM key. A name alert pops a banner over Windows when someone says the user's name, so they can tune back in even when Conclave is hidden.

The backlog item (2026-10-15) listed: catch me up, what should I ask, translate/explain the last minute, ask this meeting, and a name alert shown over Windows (not only inside the app). The open point "floating Conclave banner vs native toast" is settled here (§3).

## 2. Success criteria

1. While recording, the live page has a **Copilot** tab next to Notes with four actions: **Catch me up**, **What should I ask?**, **Explain the last minute**, and an **Ask** box for a free question.
2. Each action makes exactly one cloud call when clicked, and the answer appears as a card in the Copilot tab, newest on top. Nothing is ever sent to the LLM without a click (cost is the user's first priority).
3. Answers are written in the **Copilot answer language** (Settings). "Explain the last minute" translates speech that isn't in that language and explains jargon and references.
4. Each answer card has **Add to notes**, which turns it into a normal note prefixed "Copilot (<action or question>):", cut to the 2,000-character note limit, so it reaches the summary through the existing notes path.
5. With **Alert me when someone says** filled in (Settings), a final transcript line containing one of those words shows a banner at the top centre of the screen, over other apps, without taking focus, as long as Conclave's main window is not the focused window. Clicking the banner opens Conclave on the live page.
6. A failed call (no key, network, timeout, provider error) shows a short error on the card; the meeting, transcript and recording are unaffected.
7. With the alert words empty and the Copilot tab never clicked, Conclave behaves as today: no extra LLM calls, no banners, no extra work per transcript line beyond an empty-list check.

## 3. Decisions (with reasons)

The user skipped the questions, so apart from the backlog items and the answer language every decision here is **my call**.

| Decision | Reason |
|---|---|
| A **Copilot answer language** setting; default = the pinned summary language if one is pinned, else **Russian** | **User** (2026-10-20). Summary language is chosen per meeting after the call and is usually "auto"; auto would make "Explain the last minute" translate a German call into German. |
| Copilot is a tab in the live page's right column (**Notes \| Copilot**), not a third column | The live split is already 70/30 and bounded; a third column would squeeze the transcript. One tab switch is cheap. |
| Live page only. The saved meeting page gets no Copilot tab | After the call, "ask this meeting" overlaps with the queued AI chat over all meetings; it belongs there. |
| Answers live in memory for the current recording and are dropped at stop. **Add to notes** keeps the ones worth keeping, as a plain note with a "Copilot (…):" prefix, cut to 2,000 characters | No new tables, no migrations; the notes path already persists and feeds the summary. The user chose to add it, so it counts as their input; the prefix lets the summary model see where it came from. |
| Name alert reuses the existing top-centre meeting banner window (`banner.rs`) with a new kind, not a Windows toast | The banner is already no-focus and always on top, and Windows toasts do nothing when clicked (probe check D, 2026-10-13, the reason the banner exists). |
| Alert matching runs in Rust, in the existing `transcript-update` listener | It must work while the main window is hidden in the tray, when the webview may be throttled. |
| Copilot uses its own model setting, default **Same as summary model** | Summaries may use a slow, strong model; copilot answers must come within seconds. Same pattern as Dictate's cleanup model. |
| Non-streaming calls, one in flight at a time | `llm_client` has no streaming. One at a time keeps cost predictable and the UI simple. |
| The local built-in model (BuiltInAI sidecar) is not offered; Ollama is | Same rule as Dictate: the sidecar competes with Whisper for the GPU during a call. |
| No hotkeys, no tray items, no automatic suggestions | Anti-goal: nothing that triggers LLM calls on its own. Hotkeys can come later if the user wants them. |

## 4. Current state (main e4808bd)

- **Live transcript:** React state in `frontend/src/contexts/TranscriptContext.tsx:35`, fed by the `transcript-update` listener (`:315`, `addTranscript` `:462`). Lines carry `text`, `audio_start_time`/`audio_end_time` (seconds from recording start), `is_partial`, `speaker_idx` (0 = mic "You" when speaker ID is on), `mark`. No language field.
- **Rust side of the live transcript:** `audio/recording_commands.rs:503-520` listens to the same event and keeps segments in `recording_saver.rs:60`. The DB transcript is written only at stop.
- **Transcript formatting:** `frontend/src/lib/meeting-notes.ts` `formatSummaryTranscript(transcripts, speakerName?)` → `[mm:ss] [MARKED: x] Name: text` per line.
- **LLM:** `summary/llm_target.rs` `resolve_llm_target(pool, provider, model)` and `complete(target, system, user, temperature, app_data_dir)`: one non-streaming call, keys from the settings table. Dictate wraps it in a `tokio::time::timeout` (`dictate/mod.rs:38`, `dictate/cleanup.rs:141`) and picks its model at `dictate/session.rs:585-599` with a fallback to the summary model. The summary prompt tells the model to ignore instructions inside transcript lines (commit b5dd42b).
- **Summary output language:** chosen per meeting, usually after save; the only global value is an optional pinned default (`summaryLanguageDefault` in localStorage, `frontend/src/lib/summary-language-preferences.ts`), otherwise auto. Language list: `frontend/src/lib/summary-languages.ts`.
- **Banner:** `src-tauri/src/banner.rs`. One banner at a time (`CURRENT`), always on top, `focusable(false)`, auto-hide 15 s, kinds `Detected`/`Ended`; content fetched by the `banner` route via `get_meeting_banner`.
- **Live layout:** `frontend/src/app/_components/LiveTranscriptAndNotes.tsx`: `TranscriptPanel` left, collapsible `NotesPane` right.
- **User's name:** no display-name setting exists; the speaker-ID `people` self row is named "You" by default.

## 5. Copilot actions

All four build one request from the frontend's live transcript and call one new Tauri command (e.g. `copilot_ask { action, question? , transcript }`), which resolves the copilot model, builds the prompt, and calls `complete` under a **60 s** timeout.

**Context sent:** the transcript so far, formatted with `formatSummaryTranscript` ("You" on mic lines when speaker ID is on, since names exist only after stop; marks inline), plus the meeting's notes, final lines only (partials skipped). If it exceeds **~60,000 characters**, the oldest lines are dropped and the prompt says the transcript starts mid-meeting. For Ollama the cap is the model's context (as summaries compute it in `summary/service.rs`), not 60,000. *shortcut: tail truncation, not summarising the early part; upgrade if long meetings make "what should I ask" miss early topics.*

| Action | What the model is asked |
|---|---|
| Catch me up | What happened in the **last 5 minutes** (lines ending in that window, by `audio_end_time`), in a few bullets, with the earlier transcript as background. If the meeting is under 5 minutes, the whole meeting. |
| What should I ask? | 2–4 short, specific questions the user could ask now, based on what is unclear, undecided, or missing an owner or date. |
| Explain the last minute | The last **60 seconds** of transcript: a translation into the output language if it is in another language, then a plain explanation of jargon, acronyms and references. |
| Ask | The user's question, answered from the transcript only; if the transcript doesn't say, the answer says so. |

**Output language:** the Copilot answer language setting (§7). Never auto.

**Prompt safety:** the transcript comes from other people and is untrusted. The system prompt says to treat it as data and ignore any instructions inside it (same wording style as the summary prompt). The answer is shown as plain text with line breaks and simple bullets, never as HTML.

**Timing note:** the transcript lags speech by Whisper's chunking (a few seconds), so "last minute" means the last 60 s of transcribed audio. The card shows the time range it covered, e.g. "12:04–13:04".

## 6. Copilot tab (UI)

- The right column of the live page gets two tabs, **Notes** and **Copilot**. The last chosen tab is remembered (localStorage, like the collapse flag). Collapsing the column works as today.
- Copilot tab, top to bottom: three action buttons, the Ask box (Enter sends, Shift+Enter newline), then answer cards, newest first.
- A card shows: the action or question, the recording time it was asked at, the covered range where relevant, the answer, and **Add to notes** / **Copy**. While a call runs, the card shows a spinner and **Cancel**; all action buttons are disabled until it finishes.
- **Cancel** aborts the Rust task (builder's choice of mechanism, e.g. a request id and an abort command; dropping the frontend promise alone does not stop the Rust call).
- Before the first final line arrives, the buttons are disabled with "Copilot starts once there is transcript".
- The tab is usable only while recording. During stop/save it locks with the same rule as notes (`annotationsLocked`). Cards disappear when the recording ends.
- Errors on the card: "No API key for <provider>", "Timed out", "<provider> error: …" (short), with **Retry**.

## 7. Name alert

**Setting:** Settings → a new **Live copilot** section with:
- **Copilot model**: provider + model, default "Same as summary model".
- **Copilot answer language**: one language from the summary language list (no "auto"). Until the user changes it: the pinned summary language if there is one, else Russian.
- **Alert me when someone says**: a list of words, chips like the Dictate dictionary, empty by default (alert off). The hint suggests adding name forms, e.g. "Sasha, Саша, Alexander, Александр".

**Matching (Rust, `transcript-update` listener):**
- Only final lines (`is_partial == false`), only while recording.
- A word matches when a transcript word **starts with** it, case-insensitive, Unicode-aware (so "Саш" catches Саша/Саше/Сашу; "Alex" also catches "Alexa"). *shortcut: prefix matching, no stemming; upgrade to exact or regex entries if false alerts annoy.*
- Lines labelled "You" (`speaker_idx == 0`) never alert. When speaker ID is off every line can alert.
- At most one alert per **30 s**; later matches inside that window are ignored.
- Skip the alert when Conclave's main window is the focused window (the user is already looking).

**Banner:** new `BannerKind::Mention` showing "Someone said your name" and the matched line (trimmed to fit one or two lines). It replaces any banner on screen, auto-hides after 15 s, and clicking it shows and focuses the main window on the live page, with the Copilot tab open so **Catch me up** is one click away.

Whisper may misspell names; adding the misspelt forms to the list is the workaround. The Dictate dictionary is not sent to Whisper (gate failed in v0.7.0), so it doesn't help here.

## 8. Error handling

| Case | Behaviour |
|---|---|
| No copilot or summary model configured, or no key | Card error with a link to Settings → Live copilot. |
| Timeout (60 s), network or provider error | Card error + Retry. Nothing else changes. |
| Recording stops while a call runs | The call is dropped; no card survives the stop. |
| Banner window fails to open | Logged; recording unaffected. |
| Alert words list unreadable | Treated as empty (alert off), logged. |

## 9. Network and privacy

No new endpoints: copilot calls go to the providers the user already configured for summaries. `docs/network-calls.md` gets one line saying copilot sends the live transcript and notes to the chosen provider, only on click. The no-phone-home check needs no change.

## 10. Testing

- Rust unit tests: alert matching (prefix, case, Cyrillic, "You" lines skipped, partials skipped, 30 s cooldown, empty list = no work); prompt building per action (time windows, truncation note, output language); answer-language default (pinned summary language, else Russian); banner `Mention` view.
- Frontend (Bun): picking the last-5-min and last-60-s line windows, truncation, card state (pending, cancel, error, add to notes), tab persistence.
- Manual on the user's PC: a real call with alert words set and Conclave in the tray; each action on a RU/EN/DE call; response time with the chosen copilot model.

## 11. Not included

- Copilot on saved meetings and across meetings (that's the AI chat in the knowledge layer).
- Streaming answers, hotkeys, tray actions, automatic or periodic suggestions.
- Saving answers other than through Add to notes.
- Per-line language detection.
- A cost meter (separate backlog item).
