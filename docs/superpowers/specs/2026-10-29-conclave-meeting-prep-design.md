# Conclave: Meeting Prep (design)

Date: 2026-10-29. Status: draft for the user's review.

## 1. Intent

Before a call the user often already knows something about it. Before a technical interview, for example, they have the candidate's HR screening. Today that knowledge sits in another window, and turning it into "what do I ask, what do I check" is manual work.

Meeting Prep lets the user, days before a calendar event:
1. paste text (or a `.txt` file) about the call,
2. chat with Copilot about it, which suggests questions to ask and points to clarify, while the user steers ("drop the salary one", "add async/await"),
3. approve a final checklist.

During the recording that checklist sits in its own **Prep** tab, and the user ticks items off as they cover them. The summary of that meeting then uses the prep material and the checklist.

It is not interview-specific. The kind of call (its summary template) decides how Copilot prepares: an interview gets probing questions, a 1:1 gets follow-ups, and so on.

**Success:** the user preps a .NET interview from a pasted HR screening a week ahead, comes back the day before to refine it, records the call, sees the list in the Prep tab, ticks items, and gets a summary that notes where the candidate's answers differ from the screening and which prep items were never covered.

## 2. Goals, non-goals, anti-goals

**Goals**
- A Prep page listing upcoming calendar events (next 7 days), each with a "Prepare" action.
- For each event: material (pasted text / `.txt`), a saved chat with Copilot, and an editable checklist that the chat keeps updated.
- Prep is saved and can be resumed at any time before (or after) the event.
- Per-template prep instructions, with a general fallback.
- A Prep tab in the live side panel with tick-boxes and a remaining/total counter.
- The summary uses prep material, the checklist and the ticks.

**Non-goals (this release)**
- Prep for calls that are not in the calendar.
- PDF/Word/other attachments. Only pasted text and `.txt`.
- Copilot ticking items itself from the transcript. **The user ticks; the app never ticks automatically** (user decision).
- Sharing or exporting prep.
- Prep inside the existing hourly Briefs. Briefs stay as they are.

**Anti-goals (would make it a failure even if it works)**
- Prep tied to the 1-hour Briefs window or to the existence of a previous meeting. A first interview with a new candidate must work.
- Losing a half-built prep (chat, list, material) because the app restarted or the event moved.
- Silent extra LLM spend: every call goes through the existing usage/cost meter.

## 3. Constraints

- Invariants in `AGENTS.md`: no telemetry, no new outbound hosts. Prep calls go only to the user's configured LLM provider, the same as Copilot.
- Cost is the top priority. One LLM call per chat message, none in the background.
- Windows 10, Tauri 2 + Next.js. Follow existing patterns (Sidebar entry like Briefs, Settings sections, SQLite migrations in `frontend/src-tauri/migrations/` named `YYYYMMDDHHMMSS_snake_name.sql`).
- Deletion rule from 2026-10-22: deleting a meeting deletes everything tied to it, **except its prep** (§5.7, user decision 2026-10-29).

## 4. What exists today (verified in code)

- **Calendar:** Google iCal secret link, expanded in memory; `Occurrence { uid, title, start, end }` (`knowledge/calendar.rs:45-51`). Cache: in-memory plus `calendar-cache.json`, window 1 day back to 14 days ahead (`knowledge/mod.rs:24-25`), refreshed every 30 min by the knowledge scheduler. **No command returns the occurrence list to the frontend yet** (commands: connect, disconnect, status, title_now; `lib.rs:1018-1021`).
- **Recording ↔ event link:** `meetings.calendar_uid` + `meetings.calendar_start`, set once at recording start via `calendar_title_now` → `TitleOrigin::new` (`database/repositories/meeting.rs:18-30`). Matching is `event_for_start` (`calendar.rs:810-815`): `start − 10 min ≤ now < end`, and the closest start wins. Only stored when the title came from the calendar.
- **Templates:** `Template { name, description, use_when, title_keywords, language, context, sections, style }` (`summary/templates/types.rs:18-37`). Bundled JSON in `frontend/src-tauri/templates/`, seeded once into `<data_dir>/Conclave/templates` (`store.rs`). The event type is the first `title_keywords` match (`knowledge/briefs.rs:52` `event_type`).
- **Copilot call:** `copilot/ask.rs` `send` → shared non-streaming `complete()` (`summary/llm_target.rs:112`), provider = Copilot override else summary model (`ask.rs:194-197`), built-in AI rejected, 60 s timeout, `MAX_CONTEXT_CHARS = 60_000` with a smaller Ollama budget (`ask.rs:201-216`), usage recorded with `usage::record(..., Purpose::Copilot, ...)` (`ask.rs:240-247`).
- **Summary prompt:** `build_final_prompts` (`summary/processor.rs:253-290`) puts transcript, marks, notes and a `<user_context>` block into the final prompt; per-chunk prompts (`processor.rs:126`) do not see that block.
- **Live side panel:** `LiveSideTab = 'notes' | 'copilot'` (`contexts/CopilotContext.tsx:11`), rendered in `app/_components/LiveTranscriptAndNotes.tsx:66-83`; both bodies stay mounted.
- **Post-call page:** tabs transcript / notes / summary (`components/MeetingDetails/MeetingDetailsSplitView.tsx:14-19`).
- **Sidebar:** Briefs entry at `components/Sidebar/index.tsx:866` (collapsed icon `:501`).
- **Meeting deletion:** `delete_meeting_with_transaction` (`database/repositories/meeting.rs:274-364`) deletes dependants explicitly.

## 5. Design

### 5.1 Data

One prep per calendar occurrence, keyed by **event UID + occurrence start** (the same pair meetings already store).

Table `meeting_preps` (names are the builder's; content is fixed):
- `id`, `event_uid`, `event_start` (as stored on meetings), `event_title` (last seen), `template_id` (nullable: null = general / auto), `material` (text), `chat_json` (messages: role, text, time), `items_json` (checklist: id, text, kind, ticked), `approved_at` (nullable), `created_at`, `updated_at`.
- Item `kind`: `question` | `clarify` | `check` | `other`. It is shown as a small tag and needs no other behaviour.
- `ticked` lives on the item. The prep is shared by every recording linked to its occurrence, so a restart during the same call keeps the ticks (user decision: several recordings of one event share prep and ticks).
- Unique on (`event_uid`, `event_start`).

A meeting finds its prep by its own `calendar_uid` + `calendar_start`. No new column on `meetings`.

### 5.2 Prep page

New sidebar entry **Prep** (next to Briefs), route `app/prep/page.tsx`.

- **Left:** upcoming calendar occurrences for the next 7 days, grouped by day, each showing time, title and type (template name, or "General"). There's a marker for events that already have a prep (draft / approved). Below that is a section **Past and orphaned preps**: preps whose event has passed, or whose event is no longer in the calendar ("event no longer in calendar").
- **Middle:** the **Material** box (paste text, or "Add .txt file", which appends the file's text), the chat, and the message input. A template picker shows the matched template and lets the user override it (stored in `template_id`).
- **Right:** the **checklist** draft. Each item has its text, its kind tag, and remove and edit controls; there's also "Add item". Then come **Approve** and, once approved, "Approved · edited after approval" if it changed later.
- If the calendar is not connected, the page says so and links to Settings → Knowledge.

The page needs a new command that returns the cached upcoming occurrences (with each one's matched template and prep status). It reads the cache only and never fetches.

**Approval:** approving stamps `approved_at`. Unapproved drafts still show in the Prep tab during the call (with a "draft" label). *My call:* hiding a draft would lose work the user forgot to approve, which is worse than showing it.

### 5.3 The prep chat

Each user message is one LLM call through the Copilot target and the shared `complete()`, the same as Copilot Ask: same provider, same rejection of built-in AI, same timeout, usage recorded under a **new `Purpose::Prep`** so the cost meter shows it separately.

**Prompt inputs:** the template's prep instruction (§5.4), event title and time, material, chat history, the current checklist, and the new message.

**Output:** the reply text **plus** the full new checklist, as structured JSON (`{ reply, items: [{ text, kind }] }`). The app validates it. On invalid JSON it shows the reply and keeps the list unchanged, with a note "list not updated". It never wipes the list.

**Merge rule:** items keep their identity and tick state when their text is unchanged. Items the user edited or added by hand are sent to the model marked as the user's, and the instruction says to keep them unless the user asks to remove them.

**First message:** when the material is first added and the chat is empty, a "Suggest" button sends a default opening request ("Suggest what I should ask and clarify"). Nothing is sent automatically.

**Budget:** the whole request must fit the Copilot budget for the active provider (`MAX_CONTEXT_CHARS` for cloud, Copilot's smaller rule for Ollama). *My call:* material is capped at **two thirds of that budget** (40,000 characters on cloud), refused at input with a count shown ("41,300 / 40,000 characters"), so the instruction, checklist and message always have room. When the rest is over budget, the oldest chat turns are dropped first; material, checklist and the new message are never dropped. If the provider changes and existing material no longer fits, sending is refused with the same count and the user trims the material.

### 5.4 Per-template prep instructions

Add an optional `prep` string field to `Template` (with a serde default, so existing user templates keep loading). It's the instruction for how to prepare this kind of call.
- Fill it in the bundled templates where it makes sense. Add a bundled **Interview** template (name, `title_keywords` such as "interview", "собеседование", "Interview"; sections fit for an interview summary; and a `prep` instruction such as "suggest questions that test the claims in the material, flag vague or missing facts to clarify").
- Missing `prep` → a general built-in instruction ("given this material, suggest what to ask, clarify and check in this call").
- Seeding: user templates are copied only once (`.seeded-v1`). The builder must make the new Interview template and the `prep` additions reach existing installs without overwriting user edits. Adding missing bundled files, and leaving existing files alone, is acceptable.

### 5.5 During the call: Prep tab

- `LiveSideTab` gains `'prep'`. A third tab **Prep** shows the checklist with tick-boxes and a counter label ("Prep 2/4" = ticked/total). The tab is shown only when the current recording has a prep. Ticks save immediately.
- The recording is linked through `calendar_uid` + `calendar_start` at start, as today. The meeting row is only inserted when the recording is saved (`database/repositories/transcript.rs:85-111`), so the live tab takes the link from the title origin the frontend already holds at start. If the recording has no calendar link (no event matched, or the title came from elsewhere), there is no Prep tab. *My call:* no manual "attach a prep" in this release.
- The post-call meeting page gets a **Prep** tab too (read and tick, plus a link "Open in Prep"), so the user can correct ticks after the call.
- No automatic ticking (user decision).

### 5.6 Summary

When a meeting has a prep, `build_final_prompts` gets an extra block (alongside `<user_context>`) with the material, the checklist with ticked/unticked state, and an instruction: use the material as background facts about the call, note clear contradictions between it and what was said, and list unticked items as "Not covered". It goes only into the final prompt, not into chunk prompts.

**Template:** *my call:* when the summary template is chosen automatically, the prep's `template_id` (if set) is used after the manual and stored choices and before keyword and model matching (`choose_without_model`, `summary/service.rs:349`). So a prep set to Interview gets an Interview summary.

The material counts against the summary's context budget. If it doesn't fit, it is truncated at a character boundary with a marker, and the transcript is never dropped for it.

### 5.7 Lifecycle and deletion

- **Moved event:** a prep matches an occurrence with the same UID and start. If none matches, and the prep has **no linked meeting** and its `event_start` is **still in the future**, it moves to the single upcoming occurrence with the same UID when there is exactly one (a moved one-off event), updating `event_start`. In every other case it stays where it is and shows under "Past and orphaned" (or "event no longer in calendar" when its start is still in the future). Nothing is deleted. A moved single instance of a recurring event therefore appears orphaned; that's acceptable. The check runs against the DB's meeting links, not only the cache.
- **Meeting deleted:** the prep is **never** deleted with a recording (user decision, 2026-10-29: an exception to the 2026-10-22 "delete everything tied to it" rule, because a restarted recording would otherwise lose the prep mid-call). `delete_meeting_with_transaction` is unchanged. The prep stays on the Prep page, under "Past and orphaned" once its event has passed.
- **Prep deleted by the user** (button on the Prep page, with a confirmation): removes the prep only. Meetings and summaries are untouched.
- No automatic expiry.

### 5.8 Errors

Every failure is visible. LLM error, timeout or invalid output show in the chat with "Retry", and the user's message stays in the input. A DB write failure shows a toast and the in-memory state is kept until a retry succeeds. A missing calendar cache shows an empty state, not an error.

## 6. Out of scope / later

Prep for non-calendar calls; manual attach of a prep to a recording; PDF/DOCX material; auto-ticking; streaming chat replies; using last meeting's Brief as prep material automatically.

## 7. Testing

- **Rust unit tests:** prep JSON output validation and the merge rule (ticks and user items kept); budget trimming (oldest chat first, material never); moved-event matching (one-off moves, recurring doesn't); deleting a meeting leaves its prep in place; `prep` field default for old templates; seeding adds the Interview template without touching edited files; the summary block (unticked → listed, budget truncation).
- **Frontend (bun):** Prep tab shown only with a prep; counter; tick persistence call; draft label.
- **Manual on the user's PC:** paste a real HR screening for a calendar interview a week ahead; chat, approve, reopen the next day; record the event, tick items in the Prep tab, restart recording once and see ticks kept; summary lists a contradiction and the unticked items.

## 8. Calls made by Claude (check these)

- Builds on the existing calendar link (`calendar_uid` + `calendar_start`), not a new link.
- Unapproved drafts still show during the call, labelled "draft".
- Copilot provider and key, a separate `Purpose::Prep` in the cost meter.
- Material capped at two thirds of the provider's Copilot budget (40,000 characters on cloud).
- A prep's template also picks the summary template, after manual and stored choices.
- A moved event carries its prep along only while the prep has no recording and its event is still ahead.
- Chat output is the full checklist as JSON. Invalid output never wipes the list.
- "Suggest" button, no automatic first call.
- An `Interview` bundled template is added, plus an optional `prep` field on all templates.
- Post-call Prep tab for fixing ticks.
- No manual attach of a prep to a non-calendar recording.

## 9. Left to the builder

Table and column names; command names; exact prompt wording; UI styling (follow Briefs and Copilot pane); how the occurrence list command exposes template matching; the seeding mechanism for new bundled templates.
