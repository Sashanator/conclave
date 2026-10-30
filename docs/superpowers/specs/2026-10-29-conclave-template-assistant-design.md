# Conclave: Template assistant (create and improve templates with AI) — Design

Date: 2026-10-29
Status: Draft, written without a question round at the user's request; §13–§14 added 2026-10-29 after a builder check; awaiting the user's review
Builds on: `2026-10-14-conclave-templates-design.md` (template format §4.1, storage §4.2, editor §4.3)

## 1. Purpose

Writing a good summary template is slow. The user's templates are long structured prompts (Context, Sections, Style), usually adapted from Notion, and fixing them means guessing which instruction caused a bad summary. The 2026-10-29 tech-interview template took a hand conversion through a script, and a copy step then broke it.

The template assistant uses the user's own cloud LLM to:

1. **Create** a template from a description in plain words or a pasted prompt (a Notion template, for example).
2. **Improve** an existing template from a plain-words request ("add a salary section", "the hiring teams are now CRM and Billing").
3. **Learn from a summary**: after reading a summary, the user says what was wrong (and/or has already corrected the summary text), and the assistant proposes template changes that would have prevented it.

The design also carries two summary-quality fixes found on 2026-10-29 by comparing one tech interview summarised by Notion AI and by Conclave (same template, Whisper large-v3, Claude Sonnet 5.5): **Phase 0** changes the summary prompt for every template (§13), and §14 lists the template changes the assistant's first real run should apply to the interview and both 1-2-1 templates. The goal is to move fully from Notion AI to Conclave.

"Self-updating" in this design means **the app proposes, the user approves**. Nothing changes a template without the user seeing the change and accepting it.

## 2. Goals, non-goals, anti-goals

**Goals**
- Turn a pasted Notion template into a valid Conclave template in one step, inside the app, without file copying.
- Make every AI change reviewable as a readable diff, accepted whole or field by field.
- Let any template change be undone, AI-made or manual.
- One LLM call per user action, so the cost of each action is visible and small (cost is the project's first priority).

**Non-goals (v1)**
- Background or scheduled template rewriting. No LLM call happens without a button press.
- Learning from many meetings at once (batch analysis of all interview summaries). Possible later; see §11.
- Editing hardcoded prompts (Knowledge digest/ask/briefs). They are not user settings today.
- Importing directly from Notion's API. The user pastes text.

**Anti-goals (a failure even if it works)**
- A template that changes silently, or a summary that suddenly reads differently with no record of why.
- The assistant dropping parts of a long template it was asked to change slightly (for example, losing calibration examples while adding one section). §6.4 guards against this.
- A broken or invalid template reaching disk. Everything goes through `Template::validate()` and the existing save path.
- New network destinations, telemetry or phone-home.

## 3. Current state (from the code, 2026-10-29)

- **Template model**: `summary/templates/types.rs:19` `Template { name, description, use_when, title_keywords, language, context, sections[{title, instruction, format?, item_format?, example_item_format?}], style }`, with a public `validate()` (`:44`) and `render_system_prompt` (`:84`).
- **Storage**: one JSON file per template in `%APPDATA%\Conclave\templates` (`store.rs:12`). `save_template_in` (`store.rs:102`) validates, then calls `write_atomically` (`:82`): write a temp file, then rename. **No history**: a save overwrites the previous version for good.
- **Commands**: `api_list_templates`, `api_get_template`, `api_save_template(template_id?, template)`, `api_delete_template` (`template_commands.rs`, registered `lib.rs:902-905`). None calls an LLM.
- **Editor**: `frontend/src/components/TemplateSettings.tsx`. It holds `draft` and `savedJson` (`:40-42`), `dirty` compares the two as JSON (`:49`), and it already guards unsaved changes (`setUnsavedChanges` `:53`, `beforeunload` `:58-63`). `update(patch)` (`:117`) edits the draft, Save (`:131`) calls `api_save_template`, and "New template" sits at `:184`.
- **LLM helper**: `llm_target.rs:112` `complete(&LlmTarget, system, user, temperature, app_data_dir)`. It is non-streaming and has no cancellation; Knowledge Ask and Dictate cleanup already use it. `target_with_override(pool, provider?, model?)` (`llm_target.rs:23`) falls back to the summary model. The client timeout is 300 s (`llm_client.rs:9`), and errors are `String`.
- **Usage logging**: `usage/mod.rs:44` `record(pool, provider, model, Purpose, meeting_id, recording_id, Option<&TokenUsage>)`. A new `Purpose` variant goes at `:15`; the column is TEXT, so no migration is needed.
- **Summaries**: stored as JSON in `summary_processes.result` (keys from `service.rs:98`: `markdown`, `template_id`, `template_name`, `template_source`, `summary_language`, …). A user edit (`api_save_meeting_summary`, `commands.rs:94` → `summary.rs:36`) overwrites `markdown`; `summary.rs:250` carries the template keys over. **The generated original is not kept.** `result_backup` holds the pre-regeneration copy only until regeneration succeeds.
- **Network**: the user's configured provider URLs are already allowed (`docs/network-calls.md:51`). Each new purpose gets its own row there, as auto-pick (`:65`) and Ask (`:24`) do.

## 4. User-facing behaviour

### 4.1 Create with AI (Settings → Templates)

- A **"Create with AI"** button next to "New template" (`TemplateSettings.tsx:184`).
- It opens a dialog with one large text field: "Describe the meeting and the notes you want, or paste an existing template (Notion etc.)." The dialog shows which model will be used ("Uses: Claude Sonnet via your Claude key") and an approximate cost of the call, computed from the input length and the model's price if `usage/` knows it; otherwise only the model name appears.
- **Generate** sends one request. While it runs, the button shows a spinner and the dialog can be closed; a late result is then dropped.
- On success the dialog closes and the editor opens **an unsaved new template** filled with the result. A short list of **"What I did"** notes appears above the editor (for example: "Moved the scoring rules into Context because all sections depend on them"; "Turned 'output as bold, not headings' into a Style rule"). The template is not saved until the user presses Save, and the existing dirty guard applies.
- If the input has fields the format cannot express, the notes say so. A Notion "properties" block is one example.

### 4.2 Improve with AI (editor of an existing template)

- An **"Improve with AI"** button in the editor toolbar, next to Save / Duplicate / Delete (`:299-313`). It is disabled while the draft has unsaved changes ("Save or discard your changes first"), so the AI always works from the saved version and the diff is unambiguous.
- Dialog: "What should change?" (free text). Generate → **review screen** (§4.4).

### 4.3 Improve the template from this summary (meeting page)

- On the meeting summary, next to the template indicator ("Template: Техсобес .NET (auto)"), a **"Improve template…"** action.
- Dialog: "What was wrong with this summary?" (free text, optional only if the summary was edited, see below).
- If the user **edited the summary** after generation, the dialog says "Your edits to this summary will be included" with a checkbox (on by default). The edits show the assistant what the user actually wanted.
- Checkbox: "Include the transcript (more accurate, costs more: ~N tokens)". Its default comes from the setting "Include the transcript by default" in Settings → Templates, next to the assistant model (§5). The setting is **off** out of the box (cost first). The checkbox can still be changed per request.
- Generate → review screen (§4.4) for the template that produced the summary. If that template no longer exists, the action is hidden.
- After **Accept**, the review screen offers "Regenerate this summary with the updated template", using the existing `api_process_transcript(template_id, …)`.
- A **soft hint** appears after the user saves an edit to a summary: "You changed this summary. Suggest template changes?" with the same action. It makes no LLM call by itself, can be dismissed, and offers "Don't show again" (a per-app setting).

### 4.4 Review screen (shared by §4.2 and §4.3)

- Shows **"What I changed and why"**: the model's short list of changes, each tied to a field.
- Below that, a **field-by-field diff** between the saved template and the proposal:
  - Scalar fields (`name`, `use_when`, `language`, `title_keywords`): old → new.
  - Long text (`context`, `style`, each section's `instruction`): word-level diff with insertions and deletions highlighted, and unchanged runs collapsed to "… 12 unchanged lines …".
  - Sections: matched by title, then by position (§6.3). Shown as *added*, *removed*, *changed*, *moved* or *unchanged* (collapsed).
- Each changed item has a checkbox (all on by default). **Accept selected** puts the merged result into the editor draft, unsaved. **Discard** drops it. The user then presses the normal Save, so one save path and one validation remain.
- **"Ask for a different change"** keeps the dialog text and lets the user refine the request ("keep the old wording of Context"). That sends a new call with the previous proposal and the refinement.

### 4.5 History and undo (all saves, not only AI ones)

- Every successful save of an existing template keeps the previous file as a version. A delete keeps one too, so a deleted template can be restored.
- The editor gets a **"History"** menu: a list of versions (date and time, plus "AI: <first change note>" or "Manual edit"). Choosing one shows the same diff view against the current template, with **Restore**. Restore puts that version in the draft; the user saves.
- Keep the **last 20 versions** per template; older ones are pruned on save.

## 5. Model, cost and privacy

- **Model**: v1 ships a separate setting under Settings → Templates, "Model for the template assistant" (provider + model; decided by the user, 2026-10-29). It reuses the provider/model picker Ask uses if one exists and resolves through `target_with_override(pool, provider, model)`. Until it is set, the summary model is used, so the feature works with no extra setup. The key comes from the provider's stored key, as for summaries.
- **Assistant settings** (Settings → Templates): the model (above) and "Include the transcript by default" (off; §4.3). Both are stored together with the other app settings, wherever the Ask model choice is kept (builder checks), and a missing value means the default.
- **Temperature**: 0.2. Edits should be conservative and repeatable.
- **Input caps** (v1 values, constants in one place):
  - User description or paste: up to 40,000 characters, enough for the longest Notion template seen (the tech-interview template is about 11,000).
  - Summary markdown: up to 20,000 characters.
  - Summary edits: sent as a unified diff, at most 10,000 characters.
  - Transcript, only when the user ticks it: up to 60,000 characters, cut evenly from the middle with a marker if longer, the same way other long-text truncation in the app works if such a helper exists.
- **Usage**: one new `Purpose::TemplateAssist` value, recorded on every call, success or failure, with the template id in the log text and the `meeting_id` when called from a meeting.
- **Network**: no new destination. Add a row to `docs/network-calls.md` for the purpose "Template assistant", the same provider URL as summaries, triggered only by a button press. `scripts/check-no-phone-home.sh` needs no change.
- **Local providers** (Ollama, built-in AI) are allowed. If their output fails validation twice, the error says "This model could not produce a valid template; try a cloud model."

## 6. How it works

### 6.1 Rust: new module `summary/template_assist.rs`

Two Tauri commands, registered next to the template commands:

```rust
#[tauri::command]
async fn api_template_draft(description: String) -> Result<AssistResult, String>;

#[tauri::command]
async fn api_template_revise(
    template_id: String,
    request: String,               // may be empty when meeting edits are included
    meeting_id: Option<String>,    // §4.3: adds the summary and the edits
    include_edits: bool,
    include_transcript: bool,
    previous: Option<AssistResult>,// §4.4 "Ask for a different change"
) -> Result<AssistResult, String>;

#[derive(Serialize, Deserialize)]
struct AssistResult {
    template: Template,            // validated
    changes: Vec<ChangeNote>,      // what the model says it did
    warnings: Vec<String>,         // from §6.4 checks
}
struct ChangeNote { field: String, note: String } // field: "context", "style", "sections[2]", "name"…
```

**Neither command writes to disk.** Saving stays in `api_save_template`.

Flow for both:
1. Resolve the LLM target; if none is configured, return "Set up a summary model in Settings → Summary first."
2. Build the prompts (§6.2).
3. Call `complete(...)`.
4. Parse (§6.3). On a parse or `validate()` failure, make **one repair call** with the error text and the bad output, then fail with a readable error.
5. Run the preservation checks (§6.4); they add warnings and never fail the call.
6. Record usage (§5) and return.

### 6.2 Prompts

**System prompt** (an English constant; the output language follows the template content):
- What a Conclave template is: each field's meaning and limits, copied from `templates/README.md` (one source; a unit test checks that each field in the `Template` struct is mentioned).
- How Conclave uses it: a short description of `render_system_prompt`, so the model knows that `context` and `style` are sent verbatim, each section becomes a `## Title` with its instruction, and `use_when` / `title_keywords` drive auto-pick.
- Writing rules distilled from the seed templates: put rules shared across sections in `context`; one purpose per section; instructions are imperative and specific; no duplication between `context` and `style`; keep the user's own wording, names, terms and numbers verbatim; write in the language of the user's material unless asked otherwise; set `language` when the material says the meeting is in one language.
- For revisions: **minimal edits**. Change only what the request needs; never shorten, reword or drop unrelated text; keep section order unless asked; keep `name` unless asked.
- Output contract: one JSON object `{"template": {...}, "changes": [{"field": "...", "note": "..."}]}`, with no prose and no code fences.

**User prompt**: XML-tagged blocks: `<request>`, `<current_template>` (JSON, revisions only), `<summary>`, `<summary_edits>` (unified diff of generated vs current markdown), `<transcript>`, `<previous_proposal>`, and `<refinement>`. Only the blocks that apply are included.

### 6.3 Parsing and section matching

- Parsing tolerates a leading or trailing code fence and text around the outermost JSON object (strip it, then `serde_json`). The model's output never reaches disk unvalidated.
- Fields the model invents are ignored (`serde` default behaviour; check that `Template` does not `deny_unknown_fields`, or parse into a loose struct first).
- For a revision, `description` and other fields the editor hides are copied from the current template when the model leaves them out, so a revision never erases them.
- **Section matching**, for the diff and the merge, lives in the frontend (§6.5) because the user picks per item. It uses an exact title match (case and whitespace insensitive) first, then the same position for unmatched sections whose titles differ, and treats the rest as added or removed.

### 6.4 Preservation checks (anti-goal: losing content)

These run on revisions only. Each produces a warning on the review screen and never blocks:
- A section was removed that the request does not mention: "Section 'X' was removed."
- `context`, `style` or any instruction lost more than 30% of its length: "Context is 45% shorter."
- `name`, `language` or `title_keywords` changed without the request mentioning them.

The checks are plain heuristics in Rust (`template_assist::preservation_warnings(old, new, request) -> Vec<String>`). The "mentions" test is a case-insensitive substring match on the section title or field name. Unit tests cover them.

### 6.5 Frontend

- `TemplateSettings.tsx`: the two buttons, plus the dialog component `TemplateAssistDialog` (description/request text, model line, Generate, spinner, error).
- `TemplateDiffReview` component: the field diff and per-item checkboxes, producing the merged `Template`. Pure merge logic goes in `frontend/src/lib/templateDiff.ts` with Bun tests.
- Word diff: no diff library is in `package.json` today. Either add the small, MIT-licensed `diff` (jsdiff) package or write a word-level LCS (about 50 lines); builder's call. It runs on text up to about 15k characters, so a plain O(n·m) LCS over words is fine.
- Meeting page: the "Improve template…" action next to the template indicator, and the post-edit hint. The review screen opens in a dialog there. After Accept it **saves directly** (there is no editor on that page; left to Claude by the user, 2026-10-29, and chosen because history makes it undoable) after showing the merged result, then offers Regenerate. This is the one place an AI change is saved without the editor, and it still goes through `api_save_template` and history.
- Accepting into the editor reuses `setDraft`, so the dirty guard and Save stay unchanged.

### 6.6 Keeping the generated summary (needed for §4.3 edits)

- When a summary is generated, also store `generated_markdown` (identical to `markdown`) in the `summary_processes.result` JSON.
- `api_save_meeting_summary` (`summary.rs:250` path) must carry `generated_markdown` over, the same way it carries the template keys.
- Regeneration replaces `generated_markdown` with the new output.
- Summaries generated before this release have no `generated_markdown`, so "Your edits will be included" is not offered for them.
- The edits sent to the model are a unified diff of `generated_markdown` → `markdown`, computed in Rust. The `similar` crate is the obvious choice; check whether it is already in the dependency tree before adding it.

### 6.7 Template history storage

- Folder: `%APPDATA%\Conclave\templates\.history\<id>\<UTC timestamp>.json`, plus a sidecar `<timestamp>.meta.json` holding `{ "source": "manual" | "ai" | "restore" | "delete", "note": "<first change note or empty>" }`.
- `save_template_in` copies the existing file into history before `write_atomically`. A failed history copy is logged and **blocks the save** with an error, because an undo copy that silently does not exist breaks the safety promise (decided by the user, 2026-10-29).
- `delete_template_in` moves the file into history instead of deleting it.
- `list_templates_in` must skip the `.history` folder. It already reads only `*.json` in the top folder; confirm that, and add a test.
- The `source` value reaches `api_save_template` through a new optional argument, `origin: Option<SaveOrigin>`. The frontend passes `ai` when the draft came from an accepted proposal and has not been hand-edited since; otherwise `manual`.
- New commands: `api_template_history(template_id) -> Vec<HistoryEntry>`, `api_template_version(template_id, version) -> Template`. Restore needs no command: it fills the draft.
- Pruning: after each copy, keep the newest 20 per id.
- Ids are already checked by `is_valid_id` and `checked_path`. History paths are built only from a checked id and a generated timestamp, never from frontend text.

## 7. "Similar stuff": other targets (phase 3)

The assistant is written so that a target is just three things: a schema description for the prompt, a validator, and load/save functions. v1 ships the summary-template target only. Candidates for later, in order of value:

1. **Dictate custom modes**: `instructions` and `examples` in `dictate.json`, key `settings` (`dictate/settings.rs:19-20, :41`). "Create a mode from a description" fits well.
2. **Speaking Practice personas**: character, situation and plan in SQLite `practice_setups` (`practice/db.rs:15`), saved by `practice_save_setup`.
3. **One-off summary context**: the free text in `TranscriptPanel.tsx:180` is not stored, so it needs no assistant. A "Save as a template change" shortcut could route it into §4.2 later.

Knowledge prompts and Copilot have no user-editable prompt text, so they are out of scope.

Generalising now means only this: keep prompt building and parsing in functions that take the schema text and the validator, not the `Template` type directly, **only if it costs nothing extra**. Otherwise build for templates and extract when phase 3 starts. Builder's call.

## 8. Errors

Every failure reaches the user as a toast or an inline message in the dialog, never silently:
- No model configured; missing key; provider HTTP error (show the provider's message); timeout (300 s).
- Truncated output (`reject_truncated`): "The answer was cut off; try a shorter description or a model with a longer output limit."
- Invalid output after repair: show the validation error and offer "Show raw answer" (a collapsible block) so the user can copy something useful.
- Template deleted or changed on disk between Generate and Accept: before applying a revision, compare the saved template with the `current_template` sent to the model; if they differ, warn: "The template changed since this proposal was made."
- History write failure: blocks the save (§6.7).

Usage is recorded on failures too.

## 9. Testing

**Rust (Linux, `cargo test`)**
- Parsing: fenced output, prose around JSON, unknown fields, invalid template → repair path (with a mocked `complete`), double failure → error.
- Hidden fields are kept on revision.
- Preservation warnings: removed section, shrunk context, renamed template, none for a clean change.
- Prompt building: each field in the `Template` struct appears in the system prompt; blocks are included only when given; caps truncate.
- History: save copies the previous version, delete moves it, pruning keeps 20, listing skips `.history`, a failed history copy blocks the save, and ids are checked.
- `generated_markdown` survives `api_save_meeting_summary` and is replaced on regeneration.

**Frontend (Bun)**
- `templateDiff.ts`: section matching (rename, reorder, add, remove), per-item merge, accept-all equals the proposal, accept-none equals the original.
- Word diff on Cyrillic text.

**Manual on the PC** (checklist in the plan)
- Paste the Notion tech-interview template and compare the result with the hand-made `tech_interview_dotnet.json`.
- Improve it with "add a section about English level".
- Use "Improve template…" from a real interview summary with an edit.
- Restore from History.

The LLM call itself is not tested against a live provider in CI.

## 10. Decisions

1. **History write fails**: block the save (user, 2026-10-29).
2. **Meeting-page accept saves directly** (§6.5), without opening the editor: left to Claude; chosen because every save is undoable through history (2026-10-29).
3. **Transcript in "Improve from a summary"** (§4.3): off by default, configurable in Settings → Templates (user, 2026-10-29).
4. **Separate model setting** for the assistant: yes, in v1 (user, 2026-10-29); see §5.

## 11. Later (not in this design)

- **Batch learning**: "look at my last 10 interview summaries and my edits to them, and propose one template change." This adds cost and needs a summary-selection UI.
- **Rating summaries** (thumbs up/down) as a cheap signal, and a monthly "these templates get the most edits" hint with no LLM call.
- Dictate modes and practice personas through the same assistant (§7).
- Template export and import as a file (a safer route than copying JSON by hand, given the 2026-10-29 encoding break).

## 12. Phasing

0. **Phase 0**: summary prompt fixes (§13). Independent of the assistant and shipped first.
1. **Phase 1**: template history and undo (§4.5, §6.7), Create with AI (§4.1), Improve with AI (§4.2), and the review screen (§4.4). History ships first because it is the safety net for everything else.
2. **Phase 2**: improve from a summary (§4.3), `generated_markdown` (§6.6), and the post-edit hint.
3. **Phase 3**: other targets (§7).

Right after Phase 0 ships, the user applies §14 by hand once, field by field in the template editor (not by copying JSON), so the improvements arrive without waiting for the assistant (user, 2026-10-29). Phase 1's acceptance test is then a fresh request on a real template (for example §9's "add a section about English level"): it passes when the review screen shows the requested change and no warning about lost content (§6.4); a partly applied request is refined with "Ask for a different change", and prompt tuning that this exposes is follow-up work, not a blocker (Claude's call).

## 13. Phase 0: summary prompt fixes (all templates)

### 13.1 What went wrong (2026-10-29 comparison)

1. The summary said "фамилия не названа", but the meeting title held it ("📞Action C# Interview: Березинец Ярослав"). The title never reaches the model: `build_final_prompts` (`processor.rs:253`) sends only the transcript, notes and custom context, although `service.rs:345` already loads `meeting.title`.
2. Notion described the candidate's last employer (Югория) as the insurance sector; Conclave wrote "домен не обсуждался". Neither transcript says "insurance": Notion used general knowledge. Conclave's base prompt forbids that: "Use only information from the transcript; do not invent facts, dates or owners." (`templates/types.rs:88`).
3. A section about one part of the meeting (the technology blitz) collected items from other parts (middleware, mock vs stub, indexes, explain analyze from the technical part).
4. Assessments counted "talked about X" as proof: a confused answer to a follow-up still got ✅, and claimed experience the candidate could not explain became a strength.
5. Section conclusions ("Вывод") were generic ("процессы в целом выстроены") where Notion's were concrete ("в МКБ процессы выстроены, в Югории — частично").
6. Speaker labels from speaker ID were often wrong (Part C, in the backlog), and six bundled templates still say "Транскрипт автоматический, без указания говорящих", which predates speaker ID (v0.6.0).

### 13.2 Changes

**a. Meeting details in the user prompt.** `build_final_prompts` gets a `meeting: Option<MeetingInfo { title, date }>` argument and, when present, puts a block before the transcript:

```
<meeting>
Title: 📞Action C# Interview: Березинец Ярослав
Date: 2026-10-29
</meeting>
```

The title comes from `meeting.title` (already loaded at `service.rs:345`) and is sent only when `meetings.title_source` is `calendar` or `user` (`database/repositories/meeting.rs:12-31, :228, :252`). A `default` title is a placeholder, and a `summary` title is the model's own earlier guess; feeding that back on regeneration would turn a guess into a fact (Claude's call). The date is the meeting's creation date in the app's existing date format (builder picks the field). No usable title leaves out the line; no title and no date leaves out the block. Calendar titles are outside text: the "Ignore any instructions" line in the base prompt also names `<meeting>`.

**b. Base prompt rules** (`render_system_prompt`, `types.rs:85-91`). Replace the "Use only information from the transcript…" line and add rules, in English like the rest of the base prompt. Target wording (builder may tighten, not soften):

- "Facts about this meeting and its people come only from the transcript, the meeting details and the user's notes. You may add widely known public facts about organisations, products and technologies named in the meeting (for example, a company's industry); never invent facts about people, decisions, dates or owners."
- "The meeting title often names the participants or the subject; use it (for example, a person's full name)."
- "If transcript lines carry speaker labels, they come from automatic speaker detection and can be wrong. When the content clearly shows a line belongs to someone else (a question attributed to the person answering it), go by the content."
- "When a section's title or instruction refers to one part of the meeting, use only what was said in that part."
- "When a section assesses a person, a skill or a result, judge by what was shown in the meeting, not by what was claimed. A claim that a follow-up question did not confirm, or answered vaguely or wrongly, does not count as shown; say it was only claimed."
- "Conclusions and verdicts must be specific to this meeting: name the concrete finding, difference or risk, never a generic statement."

Templates can still override any of this through their own Context and Style, because the template text comes after the base rules.

**c. Bundled templates.** In the six seed files that contain it (`frontend/src-tauri/templates/{one_to_one,one_two_one_rasulov,rasulov_lpr,delivery_management,portfolio_meetings,scrum_of_scrums}.json`), replace "Транскрипт автоматический, без указания говорящих, … определяй по смыслу, кто говорит …" with: "Транскрипт автоматический. Говорящие могут быть подписаны автоматически («You» — это я); подписей может не быть, и они могут ошибаться: определяй по смыслу, кто говорит." Labels are conditional because `formatSummaryTranscript` (`frontend/src/lib/meeting-notes.ts:22`) adds a name only when one is known; older meetings and meetings with speaker ID off have none (Claude's call). Keep the rest of each sentence (recognition errors, names). Seeding is one-time, so this only reaches new installs; the user's copies are covered by §14.

### 13.3 Not changed

- Chunk prompts for local models (`processor.rs:126, :141`): they only shorten text; the meeting block and rules go into the final pass only. Evidence-based rules work less well on chunk summaries, which is accepted: the user summarises with cloud models, which are not chunked (Claude's call).
- The live-copilot and Knowledge prompts.

### 13.4 Tests (Rust)

- `build_final_prompts` with meeting info puts the title and date in `<meeting>` before `<transcript_chunks>`; without it, no block; an empty title drops the line.
- `render_system_prompt` contains each new rule (one assertion per rule, matched on a short stable phrase) and no longer contains "Use only information from the transcript".
- Each seed template still parses and validates and no longer contains "без указания говорящих".

**Manual on the PC:** (1) regenerate the 2026-10-29 interview summary with the user's unchanged templates and check item 1 (surname from the title); (2) apply §14 by hand, regenerate again and check items 2–5 of §13.1 against the user's verdicts (caching ❌, multithreading ❌, DB optimisation ❌, SQL/joins ✅, blitz without technical-part topics, Югория as insurance, concrete conclusions).

## 14. Template change requests (applied with "Improve with AI")

The user's templates live in `%APPDATA%\Conclave\templates` and differ from the repo seeds (the tech-interview template exists only there; no seed is added for it), so these are written as change requests. The user applies them by hand once after Phase 0 (§12); they are worded so they also work pasted into "Improve with AI" (§4.2). They are in Russian, like the templates. The plan's PC checklist includes them verbatim.

### 14.1 Техсобес .NET (tech interview)

Found in a copy of the template from an earlier session: the blitz section asks for "ответы блица и технологии из рассказа об опыте", and Style says "Не выдумывай: чего нет в транскрипте или заметках — «Не обсуждалось»". Request:

> 1. «Блиц по технологиям»: только технологии и инструменты из части про опыт и стек и из самого блица. Темы технической части (вопросы по C#, ASP.NET Core, SQL, ORM, тестированию, архитектуре — например middleware, mock vs stub, индексы, explain analyze) сюда не попадают.
> 2. Правила оценки, добавить:
>    - Путаный, неверный или уклончивый ответ на уточняющий вопрос по теме → ❌, даже если тема в целом обсуждалась (пример: разница in-memory и Redis кэша объяснена путано → «Кеширование» ❌).
>    - Для Middle-тем теория без практики → ❌: «знаю, но использовал редко / крайне редко / этим занимались сеньоры» (пример: race condition и lock назвал, потоки на практике почти не использовал → «Многопоточность» ❌; explain analyze почти не использовал → «Оптимизация в БД» ❌).
>    - Верный ответ на заданный вопрос засчитывается; то, о чём не спрашивали, не снимает ✅ (пример: верно объяснил COUNT(*) vs COUNT(column) → SQL-тема ✅).
>    - Реакции интервьюера («правильно», «всё так», «сойдёт», «я всё услышал» с обрывом ответа) — сигнал для оценки.
> 3. «Общее впечатление», сильные стороны: только то, что подтверждено в технической части. Опыт со слов кандидата, который он не смог объяснить на уточняющий вопрос (зачем выбрал решение, какие были альтернативы), — не сильная сторона; писать «со слов кандидата» (пример: сага).
> 4. «Доменная область»: если компания известна, укажи её отрасль из общих знаний с пометкой «(общеизвестно)». В Style уточни: «Не выдумывай» относится к фактам о кандидате и встрече, а не к общеизвестным фактам о компаниях и технологиях.
> 5. Выводы в разделах («Процессы», «Интересная задача», «Вердикт») — конкретные: по каждому месту работы или пункту, что именно выстроено / сильное / слабое. Без общих фраз вроде «в целом выстроены».
> 6. Фамилию и имя кандидата бери из названия встречи, если в разговоре их нет.
> 7. «Меток спикеров может не быть» замени на: метки спикеров расставлены автоматически и могут ошибаться («You» — это я); определяй по смыслу, кто спрашивает и кто отвечает.

Keep (the user rated Conclave better here): the structure of «Почему выходит на рынок», «Процессы», «Команда», «Интересная задача», and the Middle verdict.

### 14.2 1-2-1 Расулов (meeting with my HoD)

> 1. Строку про транскрипт без указания говорящих замени на: говорящие подписаны автоматически («You» — это я, второй участник — HoD), подписи могут ошибаться; если по смыслу реплика принадлежит другому, исправляй.
> 2. «Обратная связь от HoD»: только оценки и комментарии самого HoD, близко к его формулировкам. Мои слова о себе сюда не попадают.
> 3. Выводы и резюме — конкретные: что именно решили, поручили или оценили, без общих фраз.

### 14.3 One-to-One (meeting with a developer from my team)

> 1. Строку про транскрипт без указания говорящих замени на: говорящие подписаны автоматически («You» — это я, тимлид; второй участник — разработчик), подписи могут ошибаться; если по смыслу реплика принадлежит другому, исправляй.
> 2. Оценку состояния, производительности и развития разработчика строй на фактах из разговора (задачи, сроки, примеры). Самооценку разработчика отделяй пометкой «со слов разработчика»; если на уточняющий вопрос он не привёл пример, это не подтверждено.
> 3. Выводы — конкретные: какая проблема, какой риск, какая договорённость, без общих фраз.

The user's other templates (ЛПР, Delivery, Portfolio, Scrum of Scrums) carry the same outdated speaker line; the same request 1 applies to them.
