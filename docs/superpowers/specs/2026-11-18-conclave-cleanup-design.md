# Conclave cleanup release (v2.1.0): design

Date: 2026-11-18. The user approved the five mockups on 2026-11-18. This spec is waiting for the user's approval.

**Inputs:**
- **Mockups:** `.superpowers/brainstorm/cleanup-mockups/cleanup-v2.html`, with its `files/` folder. They are local only and not committed, because the Now screenshots show real meeting titles. The page has five sections, each with a Now view (v2.0.0 screenshots) and an After view. Space switches between them, and each section lists what is removed or moved.
- **Audit:** `.superpowers/brainstorm/ux-audit-v2.md`, with findings per screen and severities. Its screenshots are `/workspace/.playwright-mcp/v2-*.png`.
- **Design system:** `DESIGN.md` (tokens, the three themes, table density, chips, sidebar).
- **Previous spec:** `docs/superpowers/specs/2026-11-16-conclave-redesign-design.md`. Its §3 named this cleanup as the later project.

## 1. Intent

v2.0.0 rebuilt the meetings table and the meeting page. Using it on the user's PC turned up these problems. The PC has two 2560×1440 monitors, probably at 125% scaling (about 2048 CSS px), plus one small monitor.
- Most pages are a narrow centred column (`max-w-3xl mx-auto`) with empty space on both sides.
- The round sidebar toggle covers page content.
- The meeting page has no way back to the table.
- The table forgets where the user was.
- This week opens with every day expanded.
- Settings are hard to use.

This release fixes those problems, the remaining audit findings and two Practice bugs. The user chose one branch and one release (v2.1.0), built in phases with as many parallel lanes as possible.

**Success:**
- At 2048 px no page is a narrow column surrounded by empty space.
- Opening a meeting and going back lands exactly where the user was.
- Every setting, including every AI model choice, has one obvious place.
- Practice reports work for German sessions.

## 2. Goals

### 2.1 Shell and navigation
- **Sidebar collapse.** Remove the floating round toggle in `Sidebar/index.tsx`.
  - Add a row at the bottom of the nav, under What's new, labelled "Collapse sidebar" with the lucide `PanelLeftClose` icon. In the collapsed nav it shows `PanelLeftOpen` and a tooltip.
  - Ctrl B toggles the sidebar, and the tooltip shows the shortcut.
  - The collapsed or expanded state is remembered across restarts. With nothing stored, the sidebar starts expanded when the window is at least 1600 CSS px wide and collapsed otherwise. Today it always starts collapsed.
- **Nav position.** The nav never scrolls with the page. At 1366×768 every row is visible, including What's new and Collapse.
- **Recording state.** While a recording runs, the top nav item shows "● Recording mm:ss" in red and opens the meeting being recorded, as v2.0.0 intended. Verify this and fix it if it is missing.
- **Shortcut labels.** One style everywhere: "Ctrl N", "Ctrl B", "Ctrl J".

### 2.2 Page widths: one shared frame
Pages get one frame component with a variant per page type. No frame uses `mx-auto`: everything is left-aligned, so titles start at the same x on every page. Padding is 32 px.

| Variant | Pages | Rule |
|---|---|---|
| list | Meetings, Tasks | Fills the available width. |
| reading | Finished meeting page, Digests, What's new, Practice report | Text column up to 80ch. From a 1280 px content width, a right rail sits 40 px from the text and takes the spare width, from 360 to 560 px. Below 1280 px the rail moves under the text. |
| dashboard | Stats, Practice home | Grid capped at 1600 px. |
| form | Settings (sub-nav), Dictate, New meeting | Sub-nav plus a form column of 720–880 px. New meeting uses two columns from 1280 px. |

At 1366 px each variant is a single column, except the Settings sub-nav. Page h1s use one style (20 px, weight 600, as on Meetings).

### 2.3 Meetings table
- **Day grouping.** In any view grouped by Day, only today's group starts expanded, labelled "Today · Wednesday 18 Nov". The other days start collapsed and show their counts, and groups stay in date order. If there is no today group, the first group starts expanded. Other groupings keep today's behaviour, where every group starts expanded.
- **Remembered state.** While the app runs, each view remembers its collapsed groups, search text, selected row and scroll position, keyed by view id. They are restored when the user comes back from a meeting and when they click Meetings in the nav. An app restart brings back the defaults. Sort, filter, group and columns are already saved per view and stay as they are.
- **Keep what v2.0.0 has.** The filter chips row, sortable headers, the row "⋯" menu, the 📞 badge and the Ask (Ctrl J) button all stay (mockup §1).
- **At 1366 px.** The Date column has a fixed width, chips never wrap (they use an ellipsis), and the Summary column keeps a usable width.
- **View tab counts.** Each view tab's count, such as "Not reviewed 0", must match the rows that view shows.

### 2.4 Meeting page
- **Back.** A breadcrumb above the title reads "← Meetings / *view name*", using the view the user came from.
  - Clicking it, Alt+Left, or Esc goes back. Esc and Alt+Left work only when no text field, dialog or menu has focus.
  - It uses `router.back()` when the previous history entry is the table and `push('/')` otherwise. Like the sidebar, it goes through `confirmLeave()`.
  - The new-meeting page gets the same breadcrumb.
- **Header.** The title row keeps the meeting "⋯" menu and adds an "Ask Ctrl J" button.
- **Right rail.** The rail is one panel with two tabs, "Tasks (n)" and "Ask".
  - **Tasks** holds today's "Tasks from this meeting" panel. Each task has visible Keep and Remove buttons with K and X hints, plus Done reviewing. The LLM cost line moves here.
  - **Ask** is where the AI sidebar opens on the meeting page. Ctrl J opens the rail on the Ask tab. The meeting page never shows a second Ask column. Elsewhere the AI sidebar stays as it is.
- **Summary toolbar.** It sits left-aligned above the text. "Regenerate" is the one primary button; language, AI model, template, Save and Copy are quieter secondary buttons.
- **Summary rendering** (audit P2):
  - Headings use the app's font and type scale (no off-scale 32 px headings).
  - The permanent green "Summary completed" bar is removed.
- **New meeting (mockup §5):**
  - A labelled, empty "Meeting name" field, then category and the languages spoken.
  - A large "Start recording" button at the top, above the fold at 1366×768.
  - One line summarising audio: "Mic: … · System audio: … · Language: Auto (ru, en, de) · Change". Change opens the existing device and language pickers.
  - Prep material and chat in the second column.
  - The Meetily leftovers are deleted: the "Test Mic" tip, the "Auto Detect may produce incorrect results" warning and the bullet help.
- **Recording (mockup §5):**
  - The header fits on one line at 1366 px: timer, a live level meter for mic and system, Pause, and "Stop and summarize". The meter uses the existing `audio-levels` event and `AudioLevelMeter`.
  - The audio summary line sits below the header, and the live transcript starts right under it. The Copilot panel is on the right.
  - The duplicate "Recording • 12:34" overlay is removed.

### 2.5 Settings
- **Structure (mockup §4).** A left sub-nav with a search box that filters by setting label and helper text. The groups:
  - **Capture:** Recording & devices (including the old General "Notifications" toggle, renamed "Windows notifications when recording starts/stops"), Transcription & vocabulary, Speakers.
  - **AI:** Models & keys (including the old Costs price table), Summaries, Copilot, Templates.
  - **You:** Names & calendar (the old Knowledge tab: my names, the calendar feed, "My calls marker", alert words).
  - **App:** Appearance & app (the rest of the old General tab).
  - **Removed:** the Beta tab, because Import audio is already in the nav. Move anything Beta still holds into the closest pane.
- **Old links keep working.** Each old `?tab=` id maps to its new pane.
- **No Back button and no top tab row.**
- **Rows.** Every pane uses one row pattern: label and helper text on the left, control on the right. Settings save on change and show an inline "Saved ✓", replacing today's mix of save behaviours.
- **Models & keys:**
  - A providers list with each key's status (Add key / Replace).
  - One row per existing model picker, labelled by what it does. Today's pickers are Summary, Copilot, Knowledge, Template assistant, Dictate cleanup and the Practice Models card. Each row is either "Same as Summary" or a specific model.
  - The per-feature pickers on the other panes and the Practice page are removed; each links to its row in Models & keys.
  - **Practice row (my call, from the builder check):** Practice is Claude-only and has no "Same as Summary". Its row carries the existing card's controls unchanged: the Conversation model, the Plan & report model and Effort. Making Practice work with any provider is out of scope.
  - **Saving a key:** today the only way to save an API key is `api_save_model_config`, which also overwrites the summary provider, model and Whisper model. "Add key" and "Replace" therefore need a new command that saves only that provider's key.
  - **Fallback:** Copilot, Knowledge, Template assistant and Dictate cleanup already store an optional provider and model and fall back to the summary model when it is empty (`summary/llm_target.rs`, `dictate/session.rs:603`). "Same as Summary" is that empty value, so the merge needs no data migration as long as storage stays the same.
- **Carrying settings over.** Every model a feature uses today is kept as that feature's explicit choice. A feature shows "Same as Summary" only if it currently has no choice of its own and follows the summary model. After the upgrade, every feature must use exactly the model it used before. Keys stay where they are stored today.

### 2.6 Wide pages
- **Stats (dashboard):**
  - A cost card for this week shows amounts with 2 decimals ("$1.44").
  - Next to it, a bar chart of the last 8 weeks plus the current week, in one hue (the primary).
  - Below, a Talk time table with share bars and a column header that explains what "Share" means.
- **Digests (reading):** three panes: the list of digests, the open digest at 80ch, and "Meetings in this digest" as chips that open the meetings. Plain `[M1]` references become meeting chips.
- **Tasks (list):** fills the width and uses the date helper. The review-queue redesign is the next feature, not this release.
- **Dictate:** uses the form frame.
- **Practice:**
  - The home page uses the dashboard frame.
  - The report becomes its own route, with a deep link and the breadcrumb back to Practice, in the reading frame.
  - The Models card moves to Models & keys.
  - Delete becomes an outline destructive button with a confirm.

### 2.7 Remaining audit items
- **Onboarding:**
  - The copy tells the truth: transcription is local; summaries use a cloud LLM with the user's key.
  - It includes an AI key step that saves into Models & keys and can be skipped.
- **Meeting-detected pop-up (banner):**
  - The title is never cut to a single letter when there is a "continue" candidate.
  - The ended state gets an Open button.
- **Accessibility:**
  - The Settings Notifications switch gets an accessible name.
  - Fix contrast on the Practice Delete button, the prep kind chips and the Settings note.
  - `a11y-check.mjs` reports zero serious or critical violations on every route.
- **Dates:**
  - One date helper is used everywhere. Table format: "Wed 18 Nov · 07:56–08:32". Long format: "Wednesday 18 Nov 2026".
  - No US-style month/day dates.
- **Destructive actions:**
  - The 11 `window.confirm` calls are replaced by the app's confirm dialog.
  - Red filled Delete buttons become outline destructive buttons.
- **Meetily leftovers:** delete the strings listed in the audit's "Merge / remove candidates" section.
- **DESIGN.md:** fix the text to match the code. The default theme is System, and Conclave is opt-in. This is a documentation fix only.

### 2.8 Practice bugs
- **German transcript shown in English.** Practice assumes English everywhere, so Whisper translates German speech instead of transcribing it.
  - Whisper is forced to English in the live call (`practice/runner.rs:818`) and in the report (`practice/report_job.rs:271`).
  - The review prompt says "spoken English practice session" (`practice/prompts.rs`, `REPORT_SYSTEM`).
  - The pronunciation pass (`pron.rs`, an English phone model) only understands English.
  - TTS is told its text is English (`tts.rs:256`).

  The user chose option A on 2026-11-18: **a language per practice setup.**
  - Every setup gets a Language field, English or German. A migration adds the column with English as the default, so existing setups are unchanged. The Setup editor shows the field, and the session keeps the setup's language.
  - The live call and the report transcribe in that language.
  - The report prompt names the language and reviews grammar, word choice and phrasing in it. My call: explanations stay in English, while quotes and corrections stay in the practised language.
  - Pronunciation hints are English-only. A German report skips the pass and adds a one-line note saying why.
  - TTS gets the setup's language if the engine accepts one; otherwise it stays as it is. Builder's check.
  - Rejected: (B) fixing the transcript only, because the review would still judge the session as English; (C) auto-detecting per turn, because it is unreliable on short turns (measured on meetings).
- **"The report stopped unexpectedly".** This text means a report step panicked; the panic is logged as `panic in thread …` (`lib.rs:575-580`).
  - The root cause comes from the user's app log, which is still pending. That fix waits for the log. Everything else in this release goes ahead without it.
  - **Retry:** the view clears the old error immediately and shows progress until the job reports back. Today the stale text stays.
  - **Silent start failure:** if the job cannot start ("could not start the report", `report_job.rs:86-96`), the UI shows that, instead of keeping the previous error.

## 3. Non-goals
- The Tasks page review queue and Notion. They are the next feature, with their own spec.
- Obsidian export, vocabulary to Anki, Practice "Create with AI", and new themes.
- A master-detail meeting pane (audit 3.2 C) and a user setting for content width.
- Moving Dictate into Settings, and folding Digests into Meetings. The audit suggested both; neither was chosen.
- Changing how keys are stored, and any new outbound network call.
- Transcription Part C (speaker labels, dropped audio stretches).

## 4. Anti-goals (failure even if it "works")
- **A setting changes or resets.** Merging the model pickers, moving settings, renaming tab ids or removing Beta must leave every value as it was: models, keys, names, alert words, calendar feed, devices, notifications.
- **Content is lost.** Back, Esc and Alt+Left go through `confirmLeave()`. Esc never fires inside a text field, dialog or menu. Leaving the recording page never stops the recording.
- **The table gets slower.** Restoring the remembered state adds no backend calls per row and no transcript loading.
- **Network calls change.** `scripts/check-no-phone-home.sh` still passes, and `docs/network-calls.md` needs no change.

## 5. Constraints
- Windows 10 with WebView2, a static Next.js export, and display scaling from 100 to 150%. Check pages at 1366, 1920, 2048 and 2560 px wide.
- Use `DESIGN.md` tokens only, and check every screen in all three themes.
- `AGENTS.md` rules apply: builds run under the flock, Windows CI runs only at checkpoints, and the invariants hold.

## 6. Approaches (decided)
1. **Shipping.** The user chose A: one branch and one release in phases, with parallel lanes.
   - B (rejected): two releases, a quick v2.0.1 first. It needs an extra Windows release build, and tags have failed before.
   - C (rejected): patching each page without a shared frame. That is how the widths drifted apart in the first place.
2. **Width.** Chosen: one left-aligned frame per page type.
   - Rejected: a single global frame, because it gives 1,500 px lines and stretched forms.
   - Rejected: a user width setting, because it hands the decision to the user.
3. **Back and remembered state.** Chosen: the breadcrumb plus per-view state for the app session.
   - Rejected for now: master-detail, because it is large and competes with the AI sidebar.
4. **Sidebar toggle.** Chosen: a row at the bottom of the nav, plus Ctrl B, with the state remembered.
   - Rejected: an icon in the header, because it competes with Record.
   - Rejected: auto-collapse only, because nobody would discover it.
5. **Settings.** Chosen: a grouped sub-nav with search and one Models & keys pane.
   - Rejected: one long page, because the Templates editor does not fit.
   - Rejected: just fewer tabs, because the layout drift and the five model pickers would stay.

## 7. Phases and lanes
- **Phase 1, foundation (one lane, first).** The shared page frame, the nav changes from §2.1, the date helper, and the app confirm dialog wherever it is missing. Every later lane depends on these.
- **Phase 2, parallel lanes, one worktree each:**
  - (a) Meetings table: §2.3.
  - (b) Meeting page, new meeting and recording: §2.4.
  - (c) Settings and Models & keys, including carrying settings over: §2.5. This is risky because it touches persisted settings, so each task gets its own review with a Codex pass.
  - (d) Stats, Digests, Tasks, Dictate, Practice home and the report route: §2.6.
  - (e) Practice bugs, onboarding and banner: §2.7 onboarding and banner, and §2.8.
- **Phase 3, finish.**
  - The rest of §2.7 (accessibility, contrast, Meetily leftovers, the remaining confirms).
  - `/impeccable audit`, Playwright screenshots at four widths in three themes, `a11y-check.mjs` on every route, then `/impeccable polish`.
  - A whole-branch review, the PC checklist, and the v2.1.0 release.

## 8. Testing
- **Bun:**
  - The date helper.
  - Which day group starts open: today present, today absent, other groupings.
  - Saving and restoring per-view state.
  - Mapping old `?tab=` ids to the new panes.
  - Carrying model choices over: explicit choices are kept, and only features with no choice of their own show "Same as Summary".
  - The Esc and Alt+Left guard: no firing inside an input, dialog or menu.
  - Sidebar state, default by width and remembered.
- **Rust:**
  - The setup language migration: existing setups default to English.
  - The live call and the report pass the setup's language to Whisper.
  - A German report skips the pronunciation pass and adds the note.
  - The prompt names the language.
  - A failed retry start reaches the UI.
  - Carrying model choices over, if those settings live on the Rust side.
- **Browser preview:** every page at 1366, 1920, 2048 and 2560 px in all three themes, `a11y-check.mjs`, and `/impeccable audit` plus `polish`.
- **Windows PC checklist:**
  - Upgrade from v2.0.0 and confirm every AI feature still uses the same model and every setting is unchanged.
  - Back and Esc from a meeting.
  - Ctrl B and the sidebar state after a restart.
  - A German Practice report, a recording header at 1366 px, and the pop-up title.

## 9. Left to the builder
- Where the per-view state lives (sessionStorage or a context) and how scroll is restored. Use the row key, not pixels, if the list is virtualised.
- The exact rail breakpoints, and how the AI sidebar's content renders inside the rail's Ask tab.
- The Settings row component and how search works (client-side over labels and helper text is enough).
- Copy beyond what this spec gives. It must stay plain and true.
