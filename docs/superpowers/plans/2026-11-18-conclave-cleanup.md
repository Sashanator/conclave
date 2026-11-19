# Conclave cleanup release (v2.1.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v2.1.0. It fixes wide-screen layouts, back navigation and remembered table state, the sidebar toggle and Settings, plus the remaining audit findings and the Practice German and report bugs.

**Architecture:**
- One shared `PageFrame`, with a variant per page type, replaces the centred `max-w-3xl mx-auto` columns.
- Small shared helpers carry the cross-page behaviour: back navigation, dates, a confirm dialog and the sidebar state.
- Feature lanes then rework their own screens in parallel worktrees, and one Rust lane adds the Practice setup language.

**Tech Stack:** Next.js 14 (static export) + React 18 + Tailwind + shadcn, Bun tests with happy-dom, Tauri 2 Rust core (sqlx SQLite), lucide-react icons.

**Spec:** `docs/superpowers/specs/2026-11-18-conclave-cleanup-design.md`. Mockups (local only): `.superpowers/brainstorm/cleanup-mockups/cleanup-v2.html`. Audit: `.superpowers/brainstorm/ux-audit-v2.md`.

**Size:** 73 tasks: Phase 1 (1–9), Lanes A (10–19), B (20–29), C (30–39), D (40–48), E (50–59), F (60–64), Phase 3 (70–79). Task 59 is BLOCKED on the user's app log.

## Global Constraints

- **Version and release:** v2.1.0, one branch `cleanup`, one release. `CHANGELOG.md` needs a `## v2.1.0 — YYYY-MM-DD` section, and `pnpm exec tauri info` must run clean before tagging. The version is stamped from the tag by `release.yml`, as for v2.0.0, not committed.
- **Frame:** frames are left-aligned (no `mx-auto` on page frames) with 32 px padding.
  - list: fills the width with a bounded height; the page body does not scroll, its content area does.
  - reading: a bounded height with independently scrolling columns. A text column up to `80ch`. From a 1280 px content width, a right rail sits 40 px from the text and takes the spare width, from 360 to 560 px. Below that, the rail goes under the text.
  - dashboard: a grid capped at 1600 px; each page lays out its own grid.
  - form: a form column of 720–880 px (the cap applies to the form column, not the sub-nav).
  - `subnav` renders as a left column in the `form`, `reading` and `list` variants.
  - At 1366 px every variant is a single column, except the Settings sub-nav.
- **Type:** every page h1 is 20 px, weight 600.
- **Shortcut labels:** "Ctrl N", "Ctrl B", "Ctrl J" (a space, no plus sign). Inside the app, Esc and Alt+Left go back.
- **Dates:**
  - Table: "Wed 18 Nov · 07:56–08:32".
  - Long: "Wednesday 18 Nov 2026".
  - Today's day group: "Today · Wednesday 18 Nov".
  - Never US month/day dates. Money shows 2 decimals ("$1.44").
- **Destructive buttons:** `<Button variant="outline" className={DESTRUCTIVE_OUTLINE}>` (Phase 1, `lib/utils.ts`); never `variant="destructive"` on a Button.
- **Sidebar:** with nothing stored, it starts expanded when the window is ≥ 1600 CSS px wide and collapsed otherwise. The state is remembered across restarts.
- **Settings:** every setting keeps its value through the upgrade (models, keys, names, alert words, calendar feed, devices, notifications). Each feature keeps exactly the model it used before.
- **Network:** no new outbound call; `bash scripts/check-no-phone-home.sh` passes.
- **Theming:** `DESIGN.md` tokens only, and every changed screen is checked in all three themes (Light, Dark, Conclave = `.dark.theme-conclave`).
- **Builds:** every cargo, pnpm, tsc or bun run goes through `flock /workspace/.superpowers/cargo.lock nice -n 10 …`, cargo with `-j 4` and `CARGO_TARGET_DIR=/workspace/target`.
- **Gates (frontend):** `cd frontend && pnpm exec tsc --noEmit && bun test && pnpm run build`.
- **Gates (Rust):** `cargo test --workspace --exclude llama-helper`, which needs `frontend/out` and the stub sidecar from AGENTS.md.

## Review Focus

1. **Upgrading from v2.0.0 with per-feature model choices.** Each feature must keep its exact provider and model, and saving a key must not touch the summary model. Owner: Lane C (Task 32 `models_keys_preserves_choices` in `tests/lib/models-keys.test.ts`; Task 31 Rust `save_provider_key_leaves_model_config` and `save_provider_key_on_fresh_db_keeps_default_provider` in `setting.rs` `provider_key_tests`).
2. **Esc pressed while typing.** This covers the notes editor, the Ask box, a template field, an open dialog (the confirm dialog included) or an open menu. Esc must do nothing there and only go back from page focus. Owner: Phase 1 (Task 4, `tests/hooks/back-shortcut.test.tsx`: the "does not fire" cases for input, textarea, select, contenteditable, `[role=dialog]`, `[role=alertdialog]`, `[role=menu]`, `[role=listbox]`).
3. **Back after a cold start or deep link,** when there is no table entry in history (auto-start route, pop-up "Open"). Back must land on `/`, never leave the app or do nothing. Owner: Phase 1 (Task 3, `tests/lib/back-nav.test.ts`: `cold start with no origin goes to the fallback`, `deep link with a stale origin goes to the fallback`, `storage that throws`).
4. **Retrying an old German report.** The session was recorded before this release, and its setup is now set to German. The retry must transcribe and review in German, because a session without a stored language uses its setup's current language. Owner: Lane E (Task 50 `old_session_follows_its_setups_current_language` in `practice/db.rs`; Task 52 makes `analyse` read `db::session_language`).
5. **Old `?tab=` links.** Any link the app still emits (`?tab=knowledge`, `costs`, `summary`, `general`, `beta`, `recording`) must open the matching new pane, never a blank one. Owner: Lane C (Task 30, `tests/lib/settings-panes.test.ts`: the `paneForTab` cases and the link grep that every `/settings?tab=` in `src/` passes `isKnownTab`).

## Decisions (settled before the lanes start)

- **`confirmLeave` is async** (`Promise<boolean>`, built on `confirmAction`) from Phase 1 (Task 2), with every caller awaiting it. Phase 3 (Task 70) handles only the remaining `window.confirm` calls (`regenerate-guard.ts`, `TranscriptRecovery.tsx`) and the guard test.
- **The Esc guard** also ignores an open `[role=alertdialog]` (Task 4).
- **`PageFrame` layout** is as in Global Constraints › Frame. Settings' Templates pane uses `list` + `subnav`, so the template editor gets the full width. The meeting page's transcript and rail scroll separately. The New meeting page uses `dashboard` with its own two-column grid.
- **The recording level meter is real.** The existing `audio-levels` monitor sends a fake sine, so Lane B builds a real emitter (Task 26, risky): it reuses the per-window RMS the pipeline already computes for the speaker level track, throttles to ≤ 10 Hz, and adds nothing heavier than an atomic store to the audio path.
- **Settings rows save on change**, except the composite provider/model/endpoint row (the Summary model row in Models & keys) and the GitHub token, which keep an explicit Save and show "Saved ✓" after it.
- **Practice:** a German setup adds "Speak German only." to the character prompt (Task 51). The setup language is English or German only.
- **Onboarding's key step offers Claude only**, because the first-run summary default is Claude; other providers are set in Settings → Models & keys (Task 63).
- **Ctrl B inside a text field** (the notes editor's Bold) is left to the field; the sidebar shortcut works from everywhere else (Task 6).
- **Lane A owns `formatMeetingDate`** in `lib/meetings.ts` (Task 17); Phase 1 only adds `lib/dates.ts`.
- **Not in this release:** the Digest regenerate/management audit item (B5) and Dictate as a Settings pane (spec §3).

## Shared interfaces (fixed names; lanes must use exactly these)

**Phase 1 produces:**
- **`PageFrame`.** `frontend/src/components/PageFrame/PageFrame.tsx`:
  `export function PageFrame(props: { variant: 'list' | 'reading' | 'dashboard' | 'form'; title?: React.ReactNode; breadcrumb?: React.ReactNode; actions?: React.ReactNode; rail?: React.ReactNode; subnav?: React.ReactNode; children: React.ReactNode }): JSX.Element`
  - `title` is optional (the meeting page's `MeetingHeader` owns its editable h1).
  - `subnav` renders as a left column in `form`, `reading` and `list`; `dashboard` ignores it.
  - `list` and `reading` have a bounded height: the page does not scroll, their columns do (`min-h-0` containers). `dashboard` and `form` scroll as a page.
  - `dashboard` only caps the width at 1600 px and left-aligns; each page lays out its own grid.
  - `form`'s 880 px cap applies to the form column only.
- **Destructive outline.** `frontend/src/lib/utils.ts`: `export const DESTRUCTIVE_OUTLINE = 'border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive'`, used as `<Button variant="outline" className={DESTRUCTIVE_OUTLINE}>`.
- **Confirm dialog.** `frontend/src/lib/confirm.ts`:
  - `export type ConfirmOptions = { title: string; body?: string; confirmLabel: string; destructive?: boolean }`
  - `confirmAction(opts: ConfirmOptions): Promise<boolean>`; with no host mounted (banner, dictate pill) it falls back to `window.confirm`.
  - `setConfirmHandler(h)`: used by `ConfirmHost` only.
  - `<ConfirmHost />` (`frontend/src/components/ConfirmHost.tsx`) is mounted once in `app/layout.tsx`.
- **Leave guard.** `frontend/src/lib/unsaved-guard.ts`: `confirmLeave(): Promise<boolean>` (resolves `true` at once when nothing is dirty; otherwise asks "Discard unsaved changes?"). Every caller awaits it.
- **Back navigation.** `frontend/src/lib/back-nav.ts`:
  - `export type Origin = { path: string; label: string }`
  - `rememberOrigin(o: Origin): void` stores `{ path, label, len: history.length }` to sessionStorage `conclave.origin`. Call it right before `router.push` or in a `Link`'s `onClick`, never after the push.
  - `readOrigin(): Origin | null`
  - `originIsPrevious(): boolean`
  - `goBack(router: { back(): void; push(href: string): void }, fallback: string): Promise<void>`. It awaits `confirmLeave()`, then uses `router.back()` when `originIsPrevious()`, and `push(fallback)` otherwise.
- **Back shortcut.** `frontend/src/hooks/useBackShortcut.ts`:
  - `useBackShortcut(onBack: () => void, enabled = true): void`. It fires on Esc and Alt+Left, and is ignored while focus is in a typing target, or while a `[role=dialog]`, `[role=alertdialog]`, `[role=menu]`, `[role=listbox]` or `[data-esc-layer]` element exists.
  - `isTypingTarget(t: EventTarget | null): boolean`: input, textarea, select or contenteditable.
  - `data-esc-layer`: set on a layer that closes itself on Esc. Phase 1 sets it on the AI sidebar `<aside>`; Lane B keeps it only for `kind === 'table'`.
- **Breadcrumb.** `frontend/src/components/BackCrumb.tsx`: `BackCrumb({ fallback, fallbackLabel }: { fallback: string; fallbackLabel: string })` renders "← {label}" with an Esc key hint, where the label is the stored origin's when `originIsPrevious()`, else `fallbackLabel`. It wires `useBackShortcut` and `goBack`.
- **Dates.** `frontend/src/lib/dates.ts`:
  - `formatRowDate(start: Date, end?: Date | null, now: Date = new Date()): string`. It adds the year when it differs from `now`'s: "Fri 14 Nov 2025 · 08:00–08:50".
  - `formatLongDate(d: Date): string`
  - `formatDayLabel(d: Date, now: Date): string`: "Today · Wednesday 18 Nov", other days "Tuesday 17 Nov", another year "Friday 14 Nov 2025".
  - `formatMoney(usd: number): string`: 2 decimals; a positive amount under half a cent is `<$0.01`.
- **Sidebar state.** The `SidebarProvider` context exposes `collapsed: boolean` and `toggleCollapsed(): void` (replacing `isCollapsed`/`toggleCollapse`) and registers Ctrl B. `frontend/src/lib/nav.ts`: `SIDEBAR_COLLAPSED_KEY = 'conclave.sidebarCollapsed'`, `readSidebarCollapsed(width: number): boolean`, `writeSidebarCollapsed(collapsed: boolean): void`.

**Lane A produces** (inside the lane): `defaultCollapsed(sections, group, now): Set<string>`, `type ViewUi`, `readViewUi`/`saveViewUi`/`clearViewUi` in `lib/meeting-views.ts`; the `onOpen: () => void` prop on `NameCell`, `SummaryCell`, `RowMenu` and `SearchResults`. The table's origin is `{ path: '/', label: 'Meetings / <view name>' }`.

**Lane B produces:**
- `MeetingRail(props: { tasks: React.ReactNode; taskCount: number })`, `AudioSummaryLine({ changeable }: { changeable: boolean })`, `LanguagesSpoken()`, `LiveLevels()`; `audioSummary(...)` in `lib/meeting-page.ts`; `MeetingHeader` props `actions?` and `variant?: 'page' | 'compact' | 'new'`.
- Rust: `RecordingState::set_levels(&self, mic: f32, system: f32)` and `levels(&self) -> (f32, f32)`; `simple_level_monitor::level_update` and `spawn_recording_levels`. The `audio-levels` event carries `AudioLevelUpdate` with `device_type` `'input'` (mic) and `'output'` (system).

**Lane C produces:**
- **Save one key.** The Rust command `api_save_provider_key(provider: String, api_key: String) -> Result<(), String>` saves only that provider's key and never creates or changes the summary config. The TS wrapper is `saveProviderKey(provider: string, apiKey: string): Promise<void>` in `frontend/src/lib/models-keys.ts`; it emits `model-config-updated` itself.
- **Feature models** (`lib/models-keys.ts`): `ModelFeature = 'copilot' | 'knowledge' | 'templateAssist' | 'dictateCleanup'`, `FeatureChoice = { provider: string; model: string } | null` (null = "Same as Summary"), `featureChoice`, `loadFeatureModels`, `saveFeatureModel`.
- **Settings panes.** `frontend/src/lib/settings-panes.ts`:
  - `export type SettingsPane = 'recording' | 'transcription' | 'speakers' | 'models' | 'summaries' | 'copilot' | 'templates' | 'names' | 'app'`
  - `paneForTab(tab: string | null): SettingsPane` maps the old ids.
  - `settingsHref(pane: SettingsPane): string` returns `/settings?tab=<pane>`.
  - `SETTINGS_GROUPS`, `paneLabel`, `isKnownTab`, `SETTINGS_INDEX`, `searchSettings`, `settingId`.
- **Rows.** `SettingsRow` and `useSavedTick` in `frontend/src/components/Settings/SettingsRow.tsx`.

**Lane D produces** (inside the lane): `useDigest`, `DigestState`, `DigestView({ digest })`, `MeetingChip({ meeting, from })` in `components/Briefs/DigestView.tsx`; `CostChart` in `components/Stats/CostChart.tsx`.

**Lane E produces:** `PracticeLanguage = 'en' | 'de'` (Rust enum `crate::practice::PracticeLanguage`, TS type and `PRACTICE_LANGUAGES` in `lib/practice.ts`). The setup column is `language TEXT NOT NULL DEFAULT 'en'`. A session's language is its own stored value, or else its setup's current language (`db::session_language`). The report route is `/practice/report?id=<sessionId>`.

**Lane F produces:** `OnboardingContextType.setPendingAiKey(key: string | null): void`, `AiKeyStep()`, and the preview URLs `/?onboarding=1` and `/banner?kind=detected|continue|ended`.

## Parallel lanes and file ownership

- **Limits:** at most 8 agents at once (reviewers count) and one cargo or pnpm build at a time (the flock). Each lane is one worktree under `/workspace/.worktrees/cl-<lane>`, branched from `cleanup` after Phase 1 merges.
- **Phase 1 (Tasks 1–9):** runs first in one lane, then merges into `cleanup`. Only Phase 1 and Phase 3 may touch:
  - `Sidebar/index.tsx`, `SidebarProvider.tsx`, `MainContent/index.tsx` and `app/layout.tsx`.
- **Lanes A–F:** start together after Phase 1 and merge back one at a time, each with its gates green after a rebase.
- **Merge order:** A, B, C and D merge in any order. **C merges before E and F**: E's Task 57 imports `settingsHref` and F's Task 63 imports `saveProviderKey`. E and F work on their other tasks first, then rebase onto `cleanup` after C merges and before those two tasks.
- **Codex pass per task** (CLAUDE.md, risky): 2, 3, 4, 6, 26, every Lane C task (30–39, spec §7), 50, 53, 63 and 70. Every other task is reviewed at its lane end; every phase end and the whole branch get a Codex pass.

| Lane | Tasks | Depends on | Owns |
|---|---|---|---|
| A, Meetings table | 10–19 | Phase 1 | `components/MeetingsTable/**`, `hooks/useMeetingViews.ts`, `lib/meeting-views.ts`, `lib/meetings.ts` (`formatMeetingDate`), `app/page.tsx` |
| B, Meeting page | 20–29 | Phase 1 | `app/meeting-details/**`, `app/_components/**`, `components/{MeetingPage,MeetingDetails,AISummary,AiSidebar}/**`, `LanguageSelection.tsx`, `LanguagesSpoken.tsx`, `DeviceSelection.tsx`, `RecordingControls.tsx`, `RecordingStatusBar.tsx`, `TranscriptView.tsx`, `VirtualizedTranscriptView.tsx`, `lib/meeting-page.ts`, `app/globals.css` (summary headings), `src-tauri/src/audio/**` |
| C, Settings | 30–39 | Phase 1 | `app/settings/**`, `components/Settings/**`, `components/*Settings.tsx` (Recording, Transcript, Speaker, SummaryModel, Copilot, Knowledge, TemplateAssist, Template, Preference, Updates, Beta), `components/shared/LlmOverridePicker.tsx`, `lib/settings-panes.ts`, `lib/models-keys.ts`, `lib/updates.ts`, `src-tauri/src/database/repositories/setting.rs`, `api/api.rs`, the command registration in `lib.rs` |
| D, Wide pages | 40–48 | Phase 1 | `app/{stats,digests,tasks,dictate,whats-new}/**`, `components/{Stats,Briefs,Tasks,Dictate,WhatsNew}/**` |
| E, Practice | 50–59 | Phase 1; C before Task 57 | `app/practice/**`, `components/Practice*/**` (except `ModelsCard.tsx`, which stays unchanged for Lane C), `lib/practice.ts`, `src-tauri/src/practice/**` and its migration |
| F, Onboarding and pop-up | 60–64 | Phase 1; C before Task 63 | `components/onboarding/**`, `contexts/OnboardingContext.tsx`, `app/banner/**`, `src-tauri/src/banner.rs` |

- **Phase 3 (Tasks 70–79):** after every lane merges. It covers the remaining `window.confirm` calls, the a11y and contrast sweep, Meetily leftovers outside lane files, the DESIGN.md fix, `/impeccable audit`, `polish`, screenshots, `a11y-check.mjs`, the whole-branch review, the changelog, the PC checklist and the release.
- **Within a lane:** each lane replaces `window.confirm` and red filled Delete buttons, and applies `formatRowDate` and the other date helpers, in the files it owns.
- **Preview mocks:** each lane adds its `browser-preview.ts` mocks in one commit at the end of the lane, so merge conflicts there stay trivial.

## Phase 1: foundation (Tasks 1–9)

Spec §2.1, §2.2, §2.7 (dates, confirms), §8 (Bun tests). One lane, worktree `/workspace/.worktrees/cl-p1` on branch `cleanup`; it merges before Lanes A–F branch. Every path below is under `frontend/`.

**Commands** (run from `/workspace/.worktrees/cl-p1/frontend`):
- **ONE** `<file>`: `flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <file>`
- **FE**: `flock /workspace/.superpowers/cargo.lock nice -n 10 sh -c 'pnpm exec tsc --noEmit && bun test && pnpm run build'`
- **GUARD**: `bash ../scripts/check-no-phone-home.sh`

**Test style:** `bun:test` + `react-test-renderer`, module mocks restored in `afterAll` (pattern: `tests/components/sidebar-nav.test.tsx:1-41`). Radix dialogs are mocked as in `tests/components/update-dialog.test.tsx:34-40`. DOM key events are dispatched on happy-dom elements with `new KeyboardEvent('keydown', { key, altKey, bubbles: true, cancelable: true })`.

**Decisions this slice makes (verified in the code):**
- **Recording nav item.** It already works. `Sidebar/index.tsx:48` opens `/meeting-details?id=<liveMeetingId>` (or the id-less page), and `recordLabel` (`lib/nav.ts:20-22`) shows `Recording mm:ss` from `activeDuration`, polled every 500 ms (`contexts/RecordingStateContext.tsx:101-135`). Tests `sidebar-nav.test.tsx:76-92` cover both. What is missing is the look: it uses the primary accent, not red, and the collapsed rail shows only a white dot (audit Shell 4). Task 7 fixes the look only.
- **Nav position.** The nav is already `fixed top-0 left-0 h-screen` (`Sidebar/index.tsx:87`), so it never scrolls with the page. Task 7 adds `overflow-y-auto` to the `<nav>` itself, only as a fallback for windows shorter than 768 px. Task 9 measures 1366×768.
- **Left padding.** `MainContent` keeps its `pl-8` (`MainContent/index.tsx:19`), and `PageFrame` pads `pr-8 py-8`, so every frame starts 32 px after the nav. Pages that are not yet on the frame keep working until their lane moves them.
- **Sidebar state is read after mount.** The shell is prerendered by the static export, so reading it in a `useState` initializer would mismatch hydration. `shortcut: a wide window animates open once at launch; upgrade by suppressing the transition until the first read if the user minds.`
- **Ctrl B and Esc never act while typing.** One guard, `isTypingTarget`, covers input, textarea, select and contenteditable, so Ctrl B stays Bold in the notes editor (BlockNote).
- **The AI sidebar and Esc.** The `aside` only renders while it is open (`AiSidebar.tsx:20-22`). Its provider closes it on Esc (`AiSidebarProvider.tsx:43`), but listener order depends on mount time. So Task 4 tags the aside with `data-esc-layer` and `useBackShortcut` ignores Esc while any `[data-esc-layer]` exists. The first Esc closes the AI sidebar; the second goes back. (Lane B Task 20 limits the attribute to `kind === 'table'`.)
- **"Origin is the previous entry".** `rememberOrigin` stores `{ path, label, len: history.length }` just before the origin page pushes. On the next page, the origin is the previous entry when `history.length === len + 1`. A cold start has no stored origin, because sessionStorage is per window session. A deep link or pop-up "Open" has a stale `len`. Both fall back to `push(fallback)`. `router.back()` is never called with only one history entry, because `len ≥ 1` makes `history.length ≥ 2`.
- **Confirm without a host.** Light windows (banner, dictate pill) have no `AppShell` (`app/layout.tsx:303-313`). There `confirmAction` falls back to `window.confirm`, so it never drops a confirm silently.
- **`components/ConfirmationModel/confirmation-modal.tsx`** has no importer (grep), so Task 2 deletes it rather than reusing it.

---

### Task 1: `lib/dates.ts` `[mechanical]`
**Files:** create `src/lib/dates.ts` and `tests/lib/dates.test.ts`. `lib/meetings.ts` (`formatMeetingDate`) is Lane A's (Task 17).

**Interfaces:**
- Produces:
  - `formatRowDate(start: Date, end?: Date | null, now: Date = new Date()): string`
  - `formatLongDate(d: Date): string`
  - `formatDayLabel(d: Date, now: Date): string`
  - `formatMoney(usd: number): string`

- [ ] **Failing tests** (`tests/lib/dates.test.ts`, all local `new Date(y, m, d, h, min)`):
  - `formatRowDate(new Date(2026,10,18,7,56), new Date(2026,10,18,8,32), new Date(2026,10,18,12))` → `'Wed 18 Nov · 07:56–08:32'`
  - with no end, or `end` null → `'Wed 18 Nov · 07:56'`
  - other year: `formatRowDate(new Date(2025,10,14,8,0), new Date(2025,10,14,8,50), new Date(2026,10,18))` → `'Fri 14 Nov 2025 · 08:00–08:50'`
  - `formatLongDate(new Date(2026,10,18))` → `'Wednesday 18 Nov 2026'`
  - `formatDayLabel`:
    - same day as `now` → `'Today · Wednesday 18 Nov'`
    - day before → `'Tuesday 17 Nov'`
    - other year → `'Friday 14 Nov 2025'`
  - `formatMoney`: `1.44` → `'$1.44'`, `0` → `'$0.00'`, `1234.5` → `'$1234.50'`, `0.004` → `'<$0.01'`
  - no output matches `/\d+\/\d+/`
- [ ] Run **ONE** `tests/lib/dates.test.ts` → FAIL (module missing).
- [ ] **Implement:**
  - Use date-fns `format`, as `lib/meetings.ts:2` does:
    - row day: `'EEE d MMM'`, plus `' yyyy'` when the year differs from `now`'s; the time is `'HH:mm'`, joined `' · '`, end `'–HH:mm'`
    - long: `'EEEE d MMM yyyy'`
    - day label: `'EEEE d MMM'` (the same as `meeting-views.ts:200`), prefixed `'Today · '` when `isSameDay(d, now)`, plus `' yyyy'` for another year
  - `formatMoney`: `usd > 0 && usd < 0.005 ? '<$0.01' : \`$${usd.toFixed(2)}\``, because a fraction of a cent must not read as free.
- [ ] Run **ONE** → PASS. Commit `feat(dates): one date and money helper`.

### Task 2: `lib/confirm.ts`, `<ConfirmHost />`, async `confirmLeave` `[judgment][risky]` (unsaved content; every leave path)
**Files:**
- create `src/lib/confirm.ts`, `src/components/ConfirmHost.tsx` and `tests/components/confirm-host.test.tsx`
- `src/lib/utils.ts`: add `DESTRUCTIVE_OUTLINE` next to `cn` (`:4`)
- `src/app/layout.tsx`: add the import near `:13`, and mount `<ConfirmHost />` right after `<ThemedToaster />` at `:290` (outside the onboarding switch, so onboarding can confirm too)
- delete `src/components/ConfirmationModel/confirmation-modal.tsx` (no importers)
- `src/lib/unsaved-guard.ts:9-11` (`confirmLeave`)
- the `confirmLeave` callers (grep verified): `src/components/Sidebar/index.tsx:25,43`, `src/components/Feed/FeedBell.tsx:54`, `src/app/settings/page.tsx:50,91`
- tests: `tests/lib/unsaved-guard.test.ts:25-43`, `tests/components/summary-unsaved-guard.test.tsx:41-45`, `tests/components/template-settings-busy.test.tsx:266`, `tests/components/sidebar-nav.test.tsx`

**Interfaces:**
- Produces:
  - `type ConfirmOptions = { title: string; body?: string; confirmLabel: string; destructive?: boolean }`
  - `confirmAction(opts: ConfirmOptions): Promise<boolean>`
  - `setConfirmHandler(h: ((req: ConfirmOptions & { resolve(ok: boolean): void }) => void) | null): void`, used by ConfirmHost only
  - `ConfirmHost(): JSX.Element | null`
  - `DESTRUCTIVE_OUTLINE` (`lib/utils.ts`)
  - `confirmLeave(): Promise<boolean>` (was `boolean`)
- Consumes `ui/dialog` (`Dialog`, `DialogContent`, `DialogHeader`, `DialogTitle`, `DialogDescription`, `DialogFooter`) and `ui/button`, in the same layout as `components/MeetingPage/DeleteRecordingDialog.tsx:11-23`.

- [ ] **Failing tests** (mock `../../src/components/ui/dialog` as in `update-dialog.test.tsx:34-40`):
  - `confirm-host.test.tsx`:
    - `confirm resolves true`: mount the host, `p = confirmAction({ title:'Delete the mode "A"?', body:'It cannot be undone.', confirmLabel:'Delete', destructive:true })`. The text shows the title, body, `'Cancel'` and `'Delete'`. Click Delete → `await p === true`, and the dialog is gone.
    - `Cancel resolves false`, and `onOpenChange(false)` (Esc or overlay) resolves false.
    - `destructive uses the outline destructive button`: the confirm `Button` has `variant 'outline'` and `className === DESTRUCTIVE_OUTLINE`; a non-destructive confirm has `variant` undefined or `'default'`.
    - `a second request settles the first as false`.
    - `no host mounted falls back to window.confirm`: unmount, `window.confirm = mock(() => true)` → resolves true, and the message contains the title.
    - `unmounting with a pending request resolves it false`.
  - `unsaved-guard.test.ts` (with `mock.module('../../src/lib/confirm')`): clean → `await confirmLeave() === true` and `confirmAction` not called; dirty → called with `{ title: 'Discard unsaved changes?', confirmLabel: 'Discard', destructive: true }` and resolves the answer (true, then false).
  - `sidebar-nav.test.tsx`, `a dirty guard and a stay answer keep the route`: `setUnsavedChanges(true)`, `confirmAction` resolving false → clicking a nav row does not call `push`; resolving true → `push` once.
  - `summary-unsaved-guard.test.tsx:45` and `template-settings-busy.test.tsx:266`: `await confirmLeave()`.
- [ ] Run **ONE** for each of those four files → FAIL.
- [ ] **Implement:**
  - `confirm.ts` keeps one module-level handler. With no handler, it returns `window.confirm(body ? \`${title}\n\n${body}\` : title)`.
  - The host holds the pending request in a ref plus state: a new request resolves any previous one `false`; `close(ok)` resolves and clears; the effect cleanup calls `setConfirmHandler(null)` and resolves `false`.
  - Footer order is Cancel, then confirm, so Radix's first focus lands on Cancel and Enter never deletes by default.
  - The confirm button is `variant={destructive ? 'outline' : 'default'}` and `className={destructive ? DESTRUCTIVE_OUTLINE : undefined}`.
  - `confirmLeave = async () => !dirty || confirmAction({ title: 'Discard unsaved changes?', confirmLabel: 'Discard', destructive: true })`.
  - Callers: each handler becomes `async` and navigates only after `if (!(await confirmLeave())) return;` (Sidebar `:25` and `:43`, FeedBell `:54`, settings `:50` and the Back button `:91`, which Lane C removes later). A bare `if (confirmLeave())` would be always true; `tsc` flags it (TS2801), so the FE gate catches any missed caller.
- [ ] Run **ONE** for the four files → PASS. Run **FE** (every other `confirmLeave` test still green).
- [ ] Commit `feat(ui): app confirm dialog; confirmLeave asks through it; drop the unused Meetily modal`.

### Task 3: `lib/back-nav.ts` `[judgment][risky]` (Review Focus 3)
**Files:** create `src/lib/back-nav.ts` and `tests/lib/back-nav.test.ts`. It consumes `confirmLeave` (Task 2).

**Interfaces:**
- Produces, as in the shared list:
  - `type Origin = { path: string; label: string }`
  - `rememberOrigin(o: Origin): void`, which stores to sessionStorage `conclave.origin` as `{ path, label, len }`
  - `readOrigin(): Origin | null`
  - `originIsPrevious(): boolean`. BackCrumb uses it so its label never names a place that Back will not reach.
  - `goBack(router: { back(): void; push(href: string): void }, fallback: string): Promise<void>`

- [ ] **Failing tests** (each clears sessionStorage and calls `setUnsavedChanges(false)` first; `router = { back: mock(), push: mock() }`; every `goBack` is awaited):
  - `cold start with no origin goes to the fallback`: `goBack(router, '/')` → `push('/')`, `back` not called.
  - `deep link with a stale origin goes to the fallback`: stored `{ path:'/', label:'Meetings', len: history.length + 3 }` → `push('/')`, no `back`.
  - `the origin as the previous entry goes back`: stored `len: history.length - 1` with `path: '/?from=test'` → `back()` once, no `push`.
  - `rememberOrigin stores path, label and history length`: the parsed JSON equals `{ path:'/tasks', label:'Tasks', len: history.length }`, and `readOrigin()` returns only `{ path, label }`.
  - `corrupt or partial storage reads as no origin`: `'{'`, `'{"path":"/"}'` and `'null'` each make `readOrigin()` null; `goBack` then pushes the fallback.
  - `declined unsaved edits stay put`: `setUnsavedChanges(true)`, no host, `window.confirm = mock(() => false)` → neither `back` nor `push`.
  - `storage that throws`: `sessionStorage.getItem` stubbed to throw → `push(fallback)`, and nothing throws.
- [ ] Run **ONE** `tests/lib/back-nav.test.ts` → FAIL.
- [ ] **Implement:** every storage access is in try/catch. `originIsPrevious` = a valid stored entry, `history.length === len + 1`, and `path !== location.pathname + location.search`. `goBack`: `if (!(await confirmLeave())) return;`, then `originIsPrevious() ? router.back() : router.push(fallback)`.
- [ ] Run **ONE** → PASS. Commit `feat(nav): remembered origin and goBack with a safe fallback`.

### Task 4: `hooks/useBackShortcut.ts` `[judgment][risky]` (Review Focus 2)
**Files:**
- create `src/hooks/useBackShortcut.ts` and `tests/hooks/back-shortcut.test.tsx`
- `src/components/AiSidebar/AiSidebar.tsx:22`: add `data-esc-layer` to the `<aside>`

**Interfaces:**
- Produces `useBackShortcut(onBack: () => void, enabled = true): void`.
- Produces `isTypingTarget(t: EventTarget | null): boolean` (Task 6 reuses it). It is true when `t` is an Element whose `closest('input,textarea,select,[contenteditable]:not([contenteditable="false"])')` matches.

- [ ] **Failing tests:** a `Probe` calls the hook with `onBack = mock()`. The DOM is built in `document.body` and the event is dispatched on the named element.
  - Fires once: `Escape` on `body`; `Alt+ArrowLeft` on `body`.
  - Does not fire, for the notes editor, Ask box, template field and select: `Escape` on an `<input>`, a `<textarea>`, a `<select>`, a `<p>` inside `<div contenteditable="true">`; `Alt+ArrowLeft` on an `<input>`.
  - Does not fire while a layer is open (an element present anywhere in `body`, with the event on `body`): `[role="dialog"]` (the confirm dialog), `[role="alertdialog"]`, `[role="menu"]`, `[role="listbox"]`, `aside[data-esc-layer]`.
  - Does not fire for `Ctrl+Escape`, `Shift+Alt+ArrowLeft`, a repeat (`repeat: true`), or an event already `defaultPrevented` (a capture listener calls `preventDefault`).
  - `enabled=false` → no call. After unmount → no call.
  - A changed `onBack` prop is used without re-registering: re-render with a new mock, press Esc, and only the new one is called.
  - It fires → `event.defaultPrevented === true`, so WebView2 does not also navigate on Alt+Left.
- [ ] Run **ONE** `tests/hooks/back-shortcut.test.tsx` → FAIL.
- [ ] **Implement:**
  - One window `keydown` listener, registered once per `enabled` value. `onBack` goes through a ref.
  - The blocker selector is `'[role="dialog"],[role="alertdialog"],[role="menu"],[role="listbox"],[data-esc-layer]'`, the same `document.querySelector` idea as `MeetingsTable.tsx:93`.
  - The handler runs `e.preventDefault()` before calling `onBack`.
- [ ] Run **ONE** and `tests/components/ai-sidebar.test.tsx` → PASS. Commit `feat(nav): Esc and Alt+Left go back, never while typing or in a dialog or menu`.

### Task 5: `components/BackCrumb.tsx` `[mechanical]`
**Files:** create `src/components/BackCrumb.tsx` and `tests/components/back-crumb.test.tsx`.

**Interfaces:**
- Produces `BackCrumb({ fallback, fallbackLabel }: { fallback: string; fallbackLabel: string })`.
- Consumes `useRouter` (`next/navigation`), `originIsPrevious`, `readOrigin`, `goBack` (Task 3) and `useBackShortcut` (Task 4).

- [ ] **Failing tests** (mock `next/navigation` with `useRouter: () => ({ back, push })`):
  - With a stored origin that is the previous entry, labelled `'Meetings / This week'`: the text contains `'Meetings / This week'` and `'Esc'`; `aria-label` is `'Back to Meetings / This week'`; click → `back()`.
  - With no origin: the label is `fallbackLabel` (`'Meetings'`); click → `push('/')`.
  - `Escape` dispatched on `document.body` → `push('/')`.
  - `Escape` inside an `<input>` → nothing.
- [ ] Run **ONE** → FAIL.
- [ ] **Implement:**
  - The label is read in `useEffect` (sessionStorage; static export): `originIsPrevious() ? readOrigin()!.label : fallbackLabel`.
  - The markup is a ghost `button`, `inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground`, holding `ArrowLeft` (`h-4 w-4`, aria-hidden), the label, and `<kbd className="ml-1 rounded border border-border px-1 text-[11px]">Esc</kbd>`.
  - `onClick` and `useBackShortcut` both call `void goBack(router, fallback)`.
- [ ] Run **ONE** → PASS. Commit `feat(nav): BackCrumb breadcrumb`.

### Task 6: Sidebar state: persisted, a default by width, and Ctrl B `[judgment][risky]` (persisted UI setting)
**Files:**
- `src/lib/nav.ts`: append
- `src/components/Sidebar/SidebarProvider.tsx:17-20, 41, 57-59, 160-164`
- `src/components/MainContent/index.tsx:11-16`
- `src/components/Sidebar/index.tsx:27`: the destructure only; the rest is Task 7
- `tests/lib/nav.test.ts`
- `tests/components/sidebar-provider.test.tsx:27`
- `tests/components/sidebar-nav.test.tsx:35`: the mock becomes `({ collapsed, toggleCollapsed })`

**Interfaces:**
- Produces:
  - Context fields `collapsed: boolean` and `toggleCollapsed(): void`, which replace `isCollapsed` and `toggleCollapse`. The only readers are `Sidebar/index.tsx`, `MainContent/index.tsx` and the two tests (grep verified).
  - `SIDEBAR_COLLAPSED_KEY = 'conclave.sidebarCollapsed'`, `readSidebarCollapsed(width: number): boolean`, `writeSidebarCollapsed(collapsed: boolean): void`
- Consumes `isTypingTarget` (Task 4).

- [ ] **Failing tests:**
  - `nav.test.ts`, `readSidebarCollapsed`: nothing stored: `1600` → `false`, `1599` → `true`; stored `'true'` at `2560` → `true`; stored `'false'` at `1366` → `false`; `localStorage.getItem` throwing at `2048` → `false`.
  - `nav.test.ts`, `writeSidebarCollapsed(true)`: stores `'true'`.
  - `sidebar-provider.test.tsx`:
    - The key list becomes `['collapsed', 'serverAddress', 'startSummaryPolling', 'stopSummaryPolling', 'toggleCollapsed']`.
    - `stored state wins after mount`: localStorage `'false'` → `collapsed === false` after `act`.
    - `Ctrl B toggles and remembers`: a `keydown` `{ key:'b', ctrlKey:true }` on `document.body` flips `collapsed`, sets localStorage to the new value, and the event is `defaultPrevented`. Also `{ key:'B', code:'KeyB', ctrlKey:true }` (Caps Lock) toggles.
    - `Ctrl B while typing is left to the editor`: on a `<p>` inside `<div contenteditable="true">` → no change, not prevented.
    - `Ctrl Shift B` → no change.
- [ ] Run **ONE** `tests/lib/nav.test.ts` and `tests/components/sidebar-provider.test.tsx` → FAIL.
- [ ] **Implement:**
  - `nav.ts`: the try/catch pattern of `lib/ai-sidebar.ts:7-22`; `stored === null ? width < 1600 : stored === 'true'`.
  - Provider: `useState(true)`, then `useEffect(() => setCollapsed(readSidebarCollapsed(window.innerWidth)), [])`; `toggleCollapsed` is a `useCallback` that flips through the functional updater and writes the new value; one window `keydown` effect: `ctrlKey && !altKey && !shiftKey && !metaKey && (key.toLowerCase()==='b' || code==='KeyB') && !isTypingTarget(e.target)` → `preventDefault()` and toggle (the same shape as `AiSidebarProvider.tsx:38-49`).
  - `MainContent` reads `collapsed`. Its margins `ml-16`/`ml-64` already match the nav widths `w-16`/`w-64` (`Sidebar/index.tsx:100`).
- [ ] Run **ONE** for both files, plus `tests/hooks/summary-generation.test.tsx` → PASS. Commit `feat(shell): remember the sidebar state, expanded by default from 1600 px, Ctrl B`.

### Task 7: Sidebar nav: the Collapse row, no floating toggle, the red Recording item `[judgment]`
**Files:** `src/components/Sidebar/index.tsx:4, 52-84, 86-125`; `tests/components/sidebar-nav.test.tsx`.

**Interfaces:**
- Consumes `collapsed` and `toggleCollapsed` (Task 6), `formatClock` (`lib/practice.ts:207`) and lucide `PanelLeftClose` and `PanelLeftOpen`.
- `recordLabel` is unchanged.

- [ ] **Failing tests** (keep every existing test; the `buttons()` filter at `:49` already drops the Collapse/Expand row, so the order test at `:59-65` stays as is):
  - `no floating toggle`: no node `className` contains `-right-6` or `rounded-full shadow-lg`.
  - `the collapse row is the last nav row`: the last `button` inside `nav[aria-label="Main"]` has `aria-label 'Collapse sidebar'`; its text contains `'Collapse sidebar'` and `'Ctrl B'`; click → the mocked `toggleCollapsed` is called.
  - `collapsed shows the expand row with the shortcut in its tooltip`: the last button has `aria-label 'Expand sidebar'`; a `TooltipContent` instance's text is `'Expand sidebar (Ctrl B)'`.
  - `the live recording item is red`: with `rec = { isRecording:true, liveMeetingId:'m1', activeDuration:192 }`, `buttons(r)[0].props.className` contains `'bg-[hsl(var(--rec))]'` and `'text-white'`, and not `'bg-primary'`; idle, it contains `'bg-primary'`.
  - `collapsed recording shows the clock in the rail`: collapsed and recording → the first button's text contains `'03:12'`.
- [ ] Run **ONE** `tests/components/sidebar-nav.test.tsx` → FAIL.
- [ ] **Implement:**
  - Delete the floating button (`:88-95`) and the `ChevronLeftCircle`/`ChevronRightCircle` imports.
  - `row()` gains `opts.hint?: string` (expanded: `<kbd className="ml-auto text-[11px] text-muted-foreground">`; collapsed: the tooltip text becomes `` `${label} (${hint})` ``) and `opts.rec?: boolean` (`bg-[hsl(var(--rec))] text-white hover:brightness-110`, the solid Live style, DESIGN.md:63, in place of the accent classes).
  - Record row: `{ accent: !isRecording, rec: isRecording }`; while collapsed and recording, the icon node is a column with a `Circle` filled `h-3 w-3` above `<span className="text-[10px] tabular-nums">{formatClock(activeDuration ?? 0)}</span>`.
  - Footer: after What's new, `row(collapsed ? 'Expand sidebar' : 'Collapse sidebar', collapsed ? <PanelLeftOpen …/> : <PanelLeftClose …/>, toggleCollapsed, { hint: 'Ctrl B' })`.
  - `<nav>` gains `overflow-y-auto`.
- [ ] Run **ONE** → PASS. Commit `feat(shell): Collapse sidebar row with Ctrl B, no floating toggle, red recording item`.

### Task 8: `PageFrame` with four variants `[judgment]`
**Files:** create `src/components/PageFrame/PageFrame.tsx` and `tests/components/page-frame.test.tsx`.

**Interfaces:**
- Produces the shared `PageFrame` signature, unchanged.
- It uses `@tailwindcss/container-queries` (already in `tailwind.config` plugins), with a named container `@container/frame` on the frame's inner box.

**Layout:**
- **Root.**
  - `list` and `reading` (bounded): `flex h-full min-h-0 min-w-0 flex-col py-8 pr-8 @container/frame`, with no scroll of their own. The Meetings table keeps its scroll area and virtualisation (`app/page.tsx:14-15`); the meeting page's transcript and rail each scroll.
  - `dashboard` and `form` (page scroll): `h-full overflow-y-auto`, holding `@container/frame min-w-0 py-8 pr-8`; `dashboard` adds `max-w-[1600px]` to that inner box.
- **Header** (`shrink-0`): `breadcrumb` in `mb-2`, then `mb-6 flex flex-wrap items-center justify-between gap-3` with `<h1 className="text-xl font-semibold">{title}</h1>` (20 px / 600) and `actions` in `flex items-center gap-2`. With no title and no actions, the row is omitted.
- **Body.**
  - `list`: `<div className="flex min-h-0 flex-1 flex-col">{children}</div>`.
  - `reading`: a grid `grid min-h-0 flex-1 gap-10 @[1280px]/frame:grid-cols-[minmax(0,80ch)_minmax(360px,560px)]`. The text cell is `flex min-h-0 min-w-0 max-w-[80ch] flex-col overflow-y-auto`. With a `rail`, the grid also has `grid-rows-[minmax(0,1fr)_minmax(0,40%)] @[1280px]/frame:grid-rows-1`, and the rail cell is `min-h-0 min-w-0 overflow-y-auto`: under the text below 1280, beside it from 1280, 40 px apart.
  - `form`: the form column `<div className="w-full min-w-0 max-w-[880px]">{children}</div>`.
  - `dashboard`: children in normal flow, with no grid imposed.
- **`subnav`** (`form`, `reading` and `list`): the body becomes a row whose first child is the sub-nav column and whose second is the variant's body (`min-w-0 flex-1`).
  - `form`: row `flex gap-8`, sub-nav `w-[220px] shrink-0`. It stays side by side at 1366; the 880 px cap is only on the form column.
  - `list`: row `flex min-h-0 flex-1 gap-8`, sub-nav `w-[220px] shrink-0 overflow-y-auto`; the body has no width cap (Settings' Templates editor).
  - `reading`: row `flex min-h-0 flex-1 flex-col gap-8 @[1280px]/frame:flex-row`, sub-nav `max-h-48 shrink-0 overflow-y-auto @[1280px]/frame:max-h-none @[1280px]/frame:w-[300px]`. Stacked above the text below 1280; list | 80ch text | rail for Digests from 1280.
  - `dashboard` ignores `subnav`.
- No class anywhere uses `mx-auto`.

- [ ] **Failing tests** (render with `react-test-renderer`; `classes(node)` = the node's `className`):
  - `no frame is centred`: for each variant, with and without `subnav` and `rail`, no node's `className` contains `mx-auto`.
  - `one h1 style`: `h1` text `'Stats'`, with a `className` containing `text-xl` and `font-semibold`. `breadcrumb` renders before the h1, and `actions` renders in the same row.
  - `list gives children a bounded flex area`: the root contains `h-full` and `flex-col`, and not `overflow-y-auto`; the children's parent contains `flex-1` and `min-h-0`.
  - `reading is bounded and its columns scroll separately`: the root has no `overflow-y-auto`; the grid contains `@[1280px]/frame:grid-cols-[minmax(0,80ch)_minmax(360px,560px)]`, `gap-10` and `min-h-0`; the text cell contains `max-w-[80ch]`, `min-h-0` and `overflow-y-auto`; the rail cell comes after it and contains `overflow-y-auto`.
  - `reading with subnav: list | text | rail`: the subnav wrapper contains `overflow-y-auto` and is the first child of a node whose className contains `@[1280px]/frame:flex-row`; the grid above is in its sibling.
  - `list with subnav keeps the full width`: the subnav wrapper contains `w-[220px]` and is the first child of a row containing `min-h-0` and `flex-1`; no node contains `max-w-[880px]`.
  - `form with subnav caps only the form column`: the subnav wrapper contains `w-[220px]` and is the first child of a `flex gap-8` row; the form column contains `max-w-[880px]`, and the subnav wrapper does not.
  - `dashboard caps at 1600 and imposes no grid`: a node contains `max-w-[1600px]`; the children's direct parent has no `grid` and no `grid-cols-` class.
  - `smoke`: `<PageFrame variant="reading" breadcrumb={<BackCrumb fallback="/" fallbackLabel="Meetings" />} title="T">x</PageFrame>` renders with `next/navigation` mocked.
- [ ] Run **ONE** `tests/components/page-frame.test.tsx` → FAIL.
- [ ] Implement as laid out above; join classes with `cn` (`lib/utils`). Run **ONE** → PASS.
- [ ] Commit `feat(ui): PageFrame with list, reading, dashboard and form variants`.

### Task 9: Phase 1 gate, browser check and review `[judgment]`
**Files:** none, unless a check fails.
- [ ] Run **FE** and **GUARD**; both green.
- [ ] Start `pnpm run dev:preview` in the background. With Playwright at http://localhost:3118:
  - After `localStorage.clear()` and a reload: at 2048×1152 the nav is expanded; at 1366×768 it is collapsed.
  - Ctrl B flips it, and the state survives a reload.
  - At 1366×768, collapsed and expanded, `document.querySelector('[aria-label="Collapse sidebar"],[aria-label="Expand sidebar"]').getBoundingClientRect().bottom <= 768`, and the nav's `scrollHeight <= clientHeight`.
  - No element overlaps the "All meetings" view tab (the audit's overlap point).
  - Screenshot the nav in Light, Dark and Conclave (`.dark.theme-conclave`).
- [ ] `node scripts/a11y-check.mjs http://localhost:3118 / /meeting-details?id=preview-1` exits 0.
- [ ] Ask `harness:reviewer` for a Codex pass over the Phase 1 range (the phase end; Tasks 2, 3, 4 and 6 are risky). Tell it to look hard for silent failures in the storage try/catch paths, the confirm fallback and any `confirmLeave()` call that is not awaited, and to check Review Focus 2 and 3. Fix the minors, then merge into `cleanup`.
- [ ] Add a PC checklist line (Task 79): Alt+Left in WebView2 is consumed by the app, not by browser history (`preventDefault`; not verifiable on Linux).

## Lane A: Meetings table (Tasks 10–19, spec §2.3)

Worktree `/workspace/.worktrees/cl-a`, branched from `cleanup` after Phase 1 merges. All paths below are under `frontend/`. Line numbers are from `cleanup` at b15df390; Phase 1 does not touch these files, so they still hold when the lane starts. The lane also owns `formatMeetingDate` in `src/lib/meetings.ts` (Task 17).

**Gates** (every task):
- FE-ONE: `cd /workspace/.worktrees/cl-a/frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 pnpm exec tsc --noEmit && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <file>`
- FE-ALL (before each commit): `cd /workspace/.worktrees/cl-a/frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test && flock /workspace/.superpowers/cargo.lock nice -n 10 pnpm run build`

Decisions this slice makes:
- **Per-view state lives in module memory**, a `Map` in `src/lib/meeting-views.ts`. It survives client navigation (the meeting page and back, a Meetings nav click) and is gone after an app restart, as §2.3 asks. It needs no storage, JSON or try/catch. Tests reset it with `clearViewUi()`.
- **Collapsed groups are stored as "flipped" keys**: the groups whose state the user changed from the default. A group is closed when `defaultCollapsed(...).has(key) !== flipped.has(key)`. A day that appears later (a new meeting) gets its default, and nothing is pre-materialised.
- **Scroll**: without virtualisation (≤ 200 items) it restores the pixel offset, which is exact because the DOM is real. With virtualisation it restores the key of the top visible item through `virtualizer.getVirtualItemForOffset` / `scrollToIndex(i, { align: 'start' })` (virtual-core 3.14.0, verified in its `index.d.ts`), as §9 asks.
- **The origin is written by explicit `onOpen` callbacks** on each link or menu item that opens a meeting, not by a capture listener. react-test-renderer has no DOM, so explicit props are what the tests can drive. The label is `Meetings / ${active.name}`, the path is `/`.

### Task 10: Day group labels through `formatDayLabel` `[mechanical]`
**Files:** modify `src/lib/meeting-views.ts:1-2` (imports) and `:197` (day label); test `tests/lib/meeting-views.test.ts:55-68`.
**Interfaces:** Consumes `formatDayLabel(d: Date, now: Date): string` from `@/lib/dates` (Phase 1).
- [ ] Failing test in `describe('arrangeRows')`, `today's day group is labelled Today`: rows on 16 and 18 Nov, `now = 2026-11-18 12:00` (the file's `now`), `BUILTIN_VIEW_DEFAULTS.week` → `labels(s)` equals `['Monday 16 Nov', 'Today · Wednesday 18 Nov']`. The existing `'Monday 16 Nov'` / `'Tuesday 17 Nov'` expectations stay.
- [ ] FE-ONE `tests/lib/meeting-views.test.ts` → FAIL (label is `Wednesday 18 Nov`).
- [ ] At `:197` replace `format(d, 'EEEE d MMM')` with `formatDayLabel(d, now)`. `now` is already in scope (destructured at `:164`). Keep `format` for the `yyyy-MM-dd` key.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Commit `feat(meetings): today's day group reads "Today · Wednesday 18 Nov"`.

### Task 11: `defaultCollapsed`, the groups that start closed `[mechanical]`
**Files:** modify `src/lib/meeting-views.ts` (new export after `arrangeRows`, `:161-217`); test `tests/lib/meeting-views.test.ts`.
**Interfaces:** Produces `export function defaultCollapsed(sections: Section[], group: ViewGroup, now: Date): Set<string>`, which returns the section keys that start closed.
- [ ] Failing tests, `describe('defaultCollapsed')`:
  - `day grouping opens only today`: sections from `arrangeRows` on days 16, 17 and 18 Nov (group `day`, `now` 18 Nov) → `new Set(['2026-11-16', '2026-11-17'])`.
  - `no today group opens the first group`: days 16 and 17 with date sort descending → `{'2026-11-16'}` (17 is first and open); ascending → `{'2026-11-17'}`.
  - `the Now section is never collapsed`: with `liveId` set, the result has no `'now'`.
  - `other groupings start all open`: group `category` and group `none` → empty set.
- [ ] FE-ONE `tests/lib/meeting-views.test.ts` → FAIL (not exported).
- [ ] Implement: when `group !== 'day'`, return an empty set. Otherwise take the day keys (sections whose key is not `'now'`). The open key is `format(now, 'yyyy-MM-dd')` when present, else the first day key. Return every other day key.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Commit `feat(meetings): only today's day group starts open`.

### Task 12: The table uses the default collapse plus the user's flips `[judgment]`
**Files:** modify `src/components/MeetingsTable/MeetingsTable.tsx:59-69` (collapse state and `items`); test `tests/components/meetings-table.test.tsx:200-211`.
**Interfaces:** Consumes `defaultCollapsed` (Task 11).
- [ ] Rewrite the test `group headers show a count and the chevron collapses the group` as `day grouping opens today only; a click flips one group`. Use `setSystemTime(new Date('2026-11-16T12:00:00Z'))` and `allCfg = JSON.stringify({ group: 'day' })`, then mount.
  - Exactly one header button has `aria-expanded === true`, and it is `Today · Monday 16 Nov`.
  - The `Sunday 15 Nov` header shows count `1` and `aria-expanded === false`, and `done2` is not in the text.
  - Clicking it shows `done2`, and two buttons are now expanded.
  - Reset with `setSystemTime()` at the end.
- [ ] Add the test `category grouping starts with every group open`: `allCfg = '{}'` (the `all` default groups by category) → no header has `aria-expanded === false`.
- [ ] FE-ONE `tests/components/meetings-table.test.tsx` → FAIL.
- [ ] Replace `:59-60` with `const [flipped, setFlipped] = useState<Set<string>>(() => new Set())` and `const defaults = useMemo(() => defaultCollapsed(sections, config.group, now), [sections, config.group, now])`. `toggleGroup` flips the key in `flipped`. At `:62` a group is closed when `!!s.label && s.key !== 'now' && defaults.has(s.key) !== flipped.has(s.key)`. Add `defaults` and `flipped` to the `items` deps at `:69`.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Commit `feat(meetings): day view opens on today, other days collapsed with counts`.

### Task 13: Per-view memory for groups, search and the selected row `[judgment]`
**Files:** modify `src/lib/meeting-views.ts` (new exports), `src/components/MeetingsTable/MeetingsTable.tsx:48-49` (query, selection) and the state from Task 12; tests `tests/lib/meeting-views.test.ts` and `tests/components/meetings-table.test.tsx` (`beforeEach` `:100-113`).
**Interfaces:** Produces in `src/lib/meeting-views.ts`:
```ts
export type ViewUi = { flipped: string[]; query: string; selectedId: string | null; scroll: { px: number; key: string | null } | null };
export function readViewUi(viewId: string): ViewUi | undefined;
export function saveViewUi(viewId: string, ui: ViewUi): void;
export function clearViewUi(): void; // tests
```
- [ ] Failing unit test `view ui is kept per view id`: save for `week`, then read `week` (equal) and `all` (`undefined`); `clearViewUi()` empties it.
- [ ] Failing component tests. Add `clearViewUi()` to `beforeEach`.
  - `coming back restores groups, search text and the selected row`, with the Task 12 setup (`setSystemTime` 16 Nov, `allCfg = JSON.stringify({ group: 'day' })`; not the week view, whose Mon–Sun range drops 15 Nov):
    1. Flip `Sunday 15 Nov` open and select `done1` through its row `onClick`.
    2. Unmount, mount again: `done2` is visible, and `tr[data-row-id=done1]` has `aria-current`.
    3. Search `plan`, unmount, mount: the search input value is `plan`.
  - `each view keeps its own state`: flip `Sunday 15 Nov` in `all` (day-grouped as above), switch to the `week` tab (its `onClick`), then back to `all`. The flip is back, and `week` showed its own defaults.
  - `a fresh session starts from the defaults`: after `clearViewUi()`, a remount shows only today open and an empty search.
- [ ] FE-ONE both files → FAIL.
- [ ] Implement in `MeetingsTable`:
  - Keep `{ flipped, query, selectedId, scroll }` in a ref updated every render (`ui.current = ...`).
  - One effect keyed on `active?.id`. It restores from `readViewUi(id)` (`setFlipped(new Set(s?.flipped))`, `setQuery(s?.query ?? '')`, `setSelectedId(s?.selectedId ?? null)`, `pendingScroll.current = s?.scroll ?? null`). Its cleanup calls `saveViewUi(id, { ...ui.current, flipped: [...ui.current.flipped] })`.
  - That one cleanup covers both unmount (opening a meeting) and switching tabs.
  - `active` is null until `listViews` resolves, so the effect returns early on null.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Commit `feat(meetings): each view remembers open groups, search and selection for the session`.

### Task 14: Restore the scroll position `[judgment]`
**Files:** modify `src/components/MeetingsTable/MeetingsTable.tsx`: the scroll `div` at `:201`, the virtualizer block `:150-167`; test `tests/components/meetings-table.test.tsx`.
**Interfaces:** Consumes `ViewUi.scroll` (Task 13).
- [ ] Failing test `scroll comes back after a remount`:
  - Mount with `create(<MeetingsTable />, { createNodeMock: (el) => (el.type === 'div' ? node : null) })`, where `node = { scrollTop: 0, querySelectorAll: () => [] }`.
  - Call the scroll div's `onScroll({ currentTarget: { scrollTop: 480 } })`, then unmount.
  - Remount with a fresh `node` and flush: `node.scrollTop === 480`.
  - Factor `mount` at `:149` so it can take the option.
- [ ] FE-ONE → FAIL.
- [ ] Implement:
  - `onScroll` on the scroll div records `scrollPos.current = { px: el.scrollTop, key: virtual ? items[virtualizer.getVirtualItemForOffset(el.scrollTop)?.index ?? -1]?.key ?? null : null }`, and the `ui` ref carries it.
  - An effect on `[loaded, items.length, virtual]` consumes `pendingScroll.current` once rows are there. Virtual with a key: `scrollToIndex(items.findIndex((i) => i.key === key), { align: 'start' })`, skipped when the index is -1. Otherwise set `scrollRef.current.scrollTop = px`.
  - Put this effect after the selection-scroll effect at `:162-167`, so the restored position wins over `scrollIntoView({ block: 'nearest' })`.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Manual check in the browser preview (`pnpm run dev:preview`): scroll, open a meeting, press the Meetings nav item; the same rows are on screen. The preview mock has fewer than 200 rows, so the virtual path is checked only by reading the code. Say so in the task report.
- [ ] Commit `feat(meetings): coming back restores the table's scroll position`.

### Task 15: Remember the origin whenever the table opens a meeting `[mechanical]`
**Files:** modify:
- `src/components/MeetingsTable/cells.tsx:13-19` (`NameCell`) and `:41` (Open prep)
- `src/components/MeetingsTable/RowMenu.tsx:9,18`
- `src/components/MeetingsTable/SearchResults.tsx:35-45,85,96` (title and "more in this meeting" links; the snippet links at `:90` too)
- `src/components/MeetingsTable/MeetingsTable.tsx`: `newMeeting` `:75-85`, the Enter action `:102`, the cell factories `:121,124`, `RowMenu` `:251`, `SearchResults` `:194`

Tests: `tests/components/meetings-table.test.tsx` and `tests/components/meetings-row-menu.test.tsx:15-24`.

**Interfaces:**
- Consumes `rememberOrigin`, `readOrigin` from `@/lib/back-nav` (Phase 1).
- Produces the prop `onOpen: () => void` on `NameCell`, `SummaryCell`, `RowMenu` and `SearchResults`, plus a local `markOrigin = () => rememberOrigin({ path: '/', label: \`Meetings / ${active?.name ?? 'All meetings'}\` })` in `MeetingsTable`.
- [ ] Failing tests. Each starts from `sessionStorage.clear()`, with the week view active.
  - Each test: `readOrigin()` equals `{ path: '/', label: 'Meetings / This week' }` after:
    - `Enter opens the selected row`: select `done1`, `key('Enter')`; `push` was called too.
    - `New meeting`: `key('n', { ctrlKey: true })`.
    - `the name link`: find the `a` whose text is `done1` and call its `onClick`.
    - `Open prep`: on the `missed` row.
    - `a search hit title`.
  - `meetings-row-menu.test.tsx`, `Open tells the caller`: clicking the `Open` item calls `onOpen` once. Pass `onOpen` in `open()` at `:19`.
- [ ] FE-ONE both files → FAIL.
- [ ] Implement:
  - Add `onClick={onOpen}` to each `Link` above. `markOrigin()` goes before `router.push` in `newMeeting` and in the `open` branch.
  - Thread `onOpen={markOrigin}` through the cell factories and `SearchResults`.
  - Add `markOrigin` (or `active`) to the keydown effect deps (`:110`), the `columns` memo deps (`:127`) and `newMeeting`'s deps.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Commit `feat(meetings): opening a meeting from the table records "Meetings / <view>" for Back`.

### Task 16: Fit the table at 1366 px `[judgment]`
**Files:** modify:
- `src/components/MeetingsTable/MeetingsTable.tsx:26-27` (widths), `:148` (`sizeTotal`), `:205-216` (header widths, resizer)
- `src/components/MeetingsTable/cells.tsx:33-37` (the "new" state chips) and `:69-72` (`CategoryChip` button)

Test: `tests/components/meetings-table.test.tsx`.

**Interfaces:** none new. `FIXED_PX: Record<string, number> = { date: 190, reviewed: 84 }` is local to `MeetingsTable.tsx`; the values come from the mockup's `.cdate` and `.crev`.
- [ ] Failing tests:
  - `Date and Reviewed have fixed widths, the rest share the remainder`: the `th` for Date has `style.width === '190px'` and Reviewed `'84px'`. Name, Category and Summary widths end in `%` and sum to 100 (±0.1). Date has no `role=separator` resizer.
  - `chips never wrap`: the category chip button's className contains `truncate` and `max-w-full`. The "new" state wrapper contains `whitespace-nowrap`.
- [ ] FE-ONE → FAIL.
- [ ] Implement:
  - Columns in `FIXED_PX` get a px width and no resizer. `sizeTotal` sums only the other visible columns, so their percentages fill what is left.
  - Chip buttons get `inline-block max-w-full truncate whitespace-nowrap align-middle`.
  - The "new" wrapper becomes `flex min-w-0 items-center gap-2 whitespace-nowrap`, and the prep text gets `truncate`.
  - Stored `widths.date` is ignored for the width (it stays in config; no migration).
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Browser preview at 1366×768 (sidebar collapsed and expanded) and 2048×1152, in Light, Dark and Conclave:
  - The date reads on one line, and no chip wraps.
  - Summary is at least about 360 px wide at 1366 with the sidebar collapsed.
  - Save the screenshots in the scratchpad; do not commit them.
- [ ] Commit `fix(meetings): fixed Date column and one-line chips keep the table usable at 1366 px`.

### Task 17: Row dates through `formatRowDate` `[mechanical]`
**Files:** modify `src/lib/meetings.ts:51-64` (`formatMeetingDate`). The callers stay unchanged: `cells.tsx:22`, `SearchResults.tsx:99`, and lane B's `MeetingHeader.tsx:108`. Test `tests/lib/meetings.test.ts:78-102`.
**Interfaces:** Consumes `formatRowDate(start: Date, end?: Date | null, now?: Date)` (Phase 1), which adds the year itself. `formatMeetingDate(row, now, live?)` keeps its signature, so nobody else changes; Lane B's `MeetingHeader.tsx:108` picks up the style with no edit.
- [ ] Add a failing test `uses the shared row date style`: `formatMeetingDate` equals `formatRowDate(start, end)` for a recorded row in the current year. The five existing cases (`'Mon 16 Nov · 10:00–10:45'`, `'…10:49–now'`, the calendar row, the no-end row and `'Fri 14 Nov 2025 · 09:00–09:30'`) must keep passing.
- [ ] FE-ONE `tests/lib/meetings.test.ts` → FAIL only if the styles differ. If it passes at once, keep the test as a guard.
- [ ] Rewrite the body to pick `start`/`end` as today, then return `formatRowDate(start, live ? null : end, now) + (live ? '–now' : '')`.
- [ ] FE-ONE → PASS; FE-ALL.
- [ ] Commit `refactor(meetings): table and meeting header dates come from formatRowDate`.

### Task 18: View tab counts match the rows, and v2.0.0 controls stay `[mechanical]`
**Files:** test `tests/components/meetings-table.test.tsx`. Modify `MeetingsTable.tsx:71-73` only if a test fails.
- [ ] Tests:
  - `each tab count equals the rows its view shows`: for `week` and an extra `unreviewed` view (add it to the mock list at `:62-65` with `BUILTIN_VIEW_DEFAULTS.unreviewed`), switch to the view. The tab's count text equals the sum of the group header counts plus the ungrouped rows. Cover it with a live row (`useRecordingState` mock returning `liveMeetingId: 'done2'`), where `Now` counts and shows in every view, and after a `dispatch` filter change on the active view: the count follows before the 500 ms save.
  - `the toolbar keeps its controls`: present are the `+ Filter` button, the sortable header buttons (Name, Date, Category, Reviewed), an `Actions: done1` trigger, the Ask button with `Ctrl J`, and the Calendar marker on the `fresh` row.
- [ ] FE-ONE. If the count test fails, compute the count from the same `sections` the view renders (`arrangeRows` with the live `config` for the active view) and rerun → PASS.
- [ ] FE-ALL.
- [ ] Commit `test(meetings): tab counts match the rows; v2.0.0 table controls stay`.

### Task 19: Meetings page on `PageFrame variant="list"` `[mechanical]`
**Files:** modify `src/app/page.tsx:1-21` and `src/components/MeetingsTable/MeetingsTable.tsx:187-188,268` (the root fragment).
**Interfaces:** Consumes `PageFrame` (Phase 1).
- [ ] Replace the outer `div` and `h1` (`page.tsx:12-18`) with `<PageFrame variant="list" title="Meetings"><StartupChecks /><MeetingsTable onAsk={toggle} onScopeChange={onScopeChange} /></PageFrame>`.
- [ ] Change the `MeetingsTable` root fragment (`:188`/`:268`) to `<div className="flex min-h-0 flex-1 flex-col">`, so the scroll area stays bounded inside the frame and long lists still virtualise.
- [ ] FE-ALL. Then the browser preview at 1366, 1920, 2048 and 2560 in the three themes:
  - The title is at the same x as on other PageFrame pages, with no `mx-auto`.
  - The table fills the width.
  - The page body does not scroll; only the table area does.
- [ ] A11Y: `pnpm run dev:preview` in the background, then `node scripts/a11y-check.mjs http://localhost:3118 /` exits 0.
- [ ] Lane end: no new Tauri commands, so no `browser-preview.ts` mocks. `grep -n "window.confirm" src/components/MeetingsTable src/app/page.tsx` is empty (the delete already uses `DeleteMeetingDialog`).
- [ ] `src/components/MeetingsTable/DeleteMeetingDialog.tsx:28`: "Delete meeting" becomes `<Button variant="outline" className={DESTRUCTIVE_OUTLINE}>` (Phase 1). Add to `tests/components/meetings-table.test.tsx`: the dialog's confirm button's `variant` is `'outline'` and its className contains `text-destructive`.
- [ ] Commit `feat(meetings): Meetings page on the shared list frame`.

## Lane B: Meeting page, new meeting and recording (Tasks 20–29)

Spec §2.4, anti-goals §4 ("Content is lost"). Worktree `/workspace/.worktrees/cl-b`, branched from `cleanup` after Phase 1 merged. Paths below are relative to `frontend/` unless they start with `src-tauri/`.

**Commands used in the steps:**
- **BUN** `<files>` = `cd frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <files>`
- **RUST** `<filter>` = `export CARGO_TARGET_DIR=/workspace/target && flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib <filter> -j 4` (needs `frontend/out` and the stub sidecar from AGENTS.md)
- **GATES** = the frontend gates from Global Constraints, run once at the end of each task, under the flock.

**Consumes (Phase 1, exact names):** `PageFrame`, `BackCrumb({ fallback, fallbackLabel })`, `confirmAction`, `formatLongDate`. `BackCrumb` wires `useBackShortcut` and `goBack`, and `goBack` runs `confirmLeave()`. This lane never calls `router.back()` directly.

**Rail decision (spec §9 leaves it to the builder):** on the meeting page the global `<AiSidebar />` (`app/layout.tsx:264`) renders nothing. `MeetingRail` reads the same `useAiSidebar()` state: `open === true` means the Ask tab is active. So Ctrl J (`AiSidebarProvider.tsx:86-88`) switches the rail to Ask with no new shortcut code, and there is never a second Ask column. On the meeting page, Esc does not close Ask; it goes back through `BackCrumb`. The provider's Esc-to-close stays only on the table screen, so one Esc never does both. For the same reason the AI sidebar `<aside>` keeps Phase 1's `data-esc-layer` only for `kind === 'table'`; on the recording page (`'live'`, Copilot) Esc does not close the panel, so the attribute would block Back for good.

---

### Task 20: `MeetingRail` and AI sidebar rail mode `[judgment]`
**Files:**
- `src/components/AiSidebar/AiSidebar.tsx:18-26`: return `null` when `kind === 'meeting'`; `data-esc-layer` only when `kind === 'table'`.
- `src/components/AiSidebar/AiSidebarProvider.tsx:89`: Escape closes only when `kind === 'table'`.
- Create `src/components/MeetingPage/MeetingRail.tsx`.
- Tests: `tests/components/ai-sidebar.test.tsx` (the "Ask chat identity" test, `:78-93`, moves to the rail), new `tests/components/meeting-rail.test.tsx`.

**Interfaces:**
- Produces: `export function MeetingRail(props: { tasks: React.ReactNode; taskCount: number }): JSX.Element`.
  - It renders `<aside aria-label="Tasks and Ask">` with a `role="tablist"` holding two `role="tab"` buttons, "Tasks (n)" and "Ask", and two `role="tabpanel"`s. Both panels stay mounted (`hidden`), so a half-typed question survives a tab switch.
  - The Ask panel is `<AskChat key={scope.meetingIds?.[0]} scope={scope} allowAll={false} />`, using `scope` from `useAiSidebar()`. Before a scope exists, it renders nothing.
  - Tab clicks: Tasks calls `close()` when `open`; Ask calls `toggle()` when `!open`.
- Consumes: `useAiSidebar()` (`{ kind, open, scope, toggle, close }`), `AskChat` (`components/Ask/AskChat.tsx:23`).

**Steps:**
- [ ] Failing tests:
  - `meeting-rail.test.tsx`: mount `<AiSidebarProvider><Screen kind="meeting" scope={{label:'This meeting', meetingIds:['a']}}/><MeetingRail tasks={<p>T</p>} taskCount={3}/><AiSidebar/></AiSidebarProvider>`. Use the `Screen`/`press` helpers copied from `ai-sidebar.test.tsx:12-23`.
    - `tasks tab first, labelled with the count`: the tab texts are `['Tasks (3)', 'Ask']`, the Tasks tab has `aria-selected` true, and the Ask panel is `hidden`.
    - `Ctrl J switches the rail to Ask; no second aside`: `press({key:'j', ctrlKey:true})` → the Ask tab is selected, and `findAllByProps({'aria-label':'AI sidebar'})` has length 0.
    - `Escape on the meeting screen keeps Ask open`: after Ctrl J, `press({key:'Escape'})` → Ask is still selected.
    - `clicking Tasks closes Ask and persists`: `readSidebarOpen('meeting')` is false afterwards.
    - `a different meeting scope starts a fresh conversation`: the test moved from `ai-sidebar.test.tsx:78-93`, now asserting the `AskChat` key inside `MeetingRail`.
  - `ai-sidebar.test.tsx`: keep `Ctrl J opens, Escape closes…` on `'table'`. Add `live: Escape does not close Copilot`: mount `'live'` → `hasAside` stays true after Escape. Mock `CopilotPane` to `() => null` if it is not mocked already.
  - `esc layer only where Esc closes it`: the `'table'` aside has `data-esc-layer`; the `'live'` aside does not; no node inside `MeetingRail` has it.
- [ ] **BUN** `tests/components/meeting-rail.test.tsx tests/components/ai-sidebar.test.tsx` → FAIL.
- [ ] Implement. Tabs use the `FinishedTabs.tsx` tab classes (`FinishedTabs.tsx:48`: `-mb-px border-b-2 px-3 py-2 text-sm`, primary underline). The rail body scrolls on its own (`min-h-0 overflow-y-auto`).
- [ ] **BUN** same → PASS; **GATES**.
- [ ] Commit `feat(meeting): one Tasks | Ask rail; Ctrl J opens Ask in it, never a second column`.

### Task 21: Tasks panel for the rail (Keep/Remove buttons, cost line) `[mechanical]`
**Files:**
- `src/components/MeetingPage/MeetingTasksPanel.tsx`: signature `:14`, row buttons `:91-95`, header `:101-104`, footer `:113-118`.
- `src/components/MeetingDetails/SummaryPanel.tsx`: cost line `:377`, `costRefresh` `:107`, `ExtractionStatusLine onSettled` `:378`.
- `src/components/MeetingDetails/MeetingCostLine.tsx:6,38,44`: `formatUsd` → `formatMoney` (Phase 1), so the cost shows 2 decimals. `tests/components/meeting-cost-line.test.tsx` expectations change to 2 decimals.
- Tests: `tests/components/meeting-page-finished.test.tsx` (`:63-71` "Restore", `:102` `aria-label 'Remove task'`), and any `summary-*.test.tsx` that mocks `./MeetingCostLine` (drop that mock when the import goes).

**Interfaces:**
- Produces: `MeetingTasksPanel({ meetingId, reviewed, onCountChange, costRefreshKey }: { meetingId: string; reviewed?: boolean; onCountChange?: (kept: number) => void; costRefreshKey?: string })`.
- `SummaryPanel` gains `onCostChanged?: () => void` (called where `setCostRefresh` is today) and loses `MeetingCostLine`.

**Behaviour:**
- Every row shows two visible buttons, `Keep <kbd>K</kbd>` and `Remove <kbd>X</kbd>`, wired to the existing `keep`/`remove`.
  - On a kept row, Keep has `aria-pressed={true}`. On a removed row, Remove has `aria-pressed={true}`.
  - "Restore" becomes Keep, which already restores (`keep`, `:49-54`).
- The hint line reads `Keyboard: K keep · X remove · ↑↓ move`, with `<kbd>`s.
- The header `h2` is 16 px/600 "Tasks from this meeting".
- `onCountChange(kept.length)` runs in an effect whenever the count changes.
- `<MeetingCostLine meetingId refreshKey={costRefreshKey} />` renders under the footer, above a top border.

**Steps:**
- [ ] Failing tests in `meeting-page-finished.test.tsx`:
  - `every task shows Keep and Remove with K and X hints`: two rows → each `li` has buttons whose text is `'Keep K'` and `'Remove X'`.
  - Update `:63-71`: "Restore" → the removed row's `Keep K` button; clicking it invokes `tasks_claim`.
  - Update `:102` to find the button with text `Remove X`.
  - `reports the kept count`: `onCountChange` is last called with `2`.
  - `shows the LLM cost line`: `meeting_cost` mocked → the text contains `LLM cost:` and a 2-decimal amount (`formatMoney` of the fixture's `total_usd`).
- [ ] **BUN** `tests/components/meeting-page-finished.test.tsx` → FAIL → implement → PASS. Then run **BUN** `tests/components/summary-*.test.tsx tests/components/meeting-cost-line.test.tsx` → PASS.
- [ ] **GATES**; commit `feat(meeting): visible Keep and Remove on tasks; LLM cost moves to the rail`.

### Task 22: Finished meeting page in the reading frame `[judgment]`
**Files:**
- `src/app/meeting-details/page-content.tsx:281-404`: the `motion.div` shell, header and tabs.
- `src/components/MeetingPage/FinishedTabs.tsx:16-25` (props) and `:54-57` (summary pane with the `md:w-80` tasks column).
- `src/components/MeetingPage/MeetingHeader.tsx:13-23` (props), `:76-96` (title row), `:83` (`text-2xl`).
- `src/app/meeting-details/page.tsx:350-364`: the error state's blue "Go Back" button.
- Tests: `tests/components/meeting-page-finished-wiring.test.tsx` (mock `MeetingRail`, `BackCrumb` and `PageFrame` as pass-throughs, the way `:8-34` mocks the others), `tests/components/meeting-page-menu.test.tsx`.

**Interfaces:**
- `MeetingHeader` gains:
  - `actions?: React.ReactNode`, rendered in the title row before the ⋯ trigger.
  - `variant?: 'page' | 'compact' | 'new'`. The default is `'page'`; Task 25 adds `'new'` and Task 27 adds `'compact'`.
- The title input becomes `text-xl font-semibold` (20 px/600, Global Constraints).
- `FinishedTabs` loses its `tasks` prop. The summary pane becomes a plain `pane('summary', …)`.

**Behaviour:**
- `page-content.tsx` returns:
  ```
  <PageFrame variant="reading"
    breadcrumb={<BackCrumb fallback="/" fallbackLabel="Meetings" />}
    rail={<MeetingRail taskCount={taskCount} tasks={<MeetingTasksPanel meetingId reviewed onCountChange={setTaskCount} costRefreshKey={`${summaryGeneration.summaryStatus}/${costKey}`} />} />}>
  ```
  - `MeetingHeader` is the first child, given `actions={<Button variant="outline" size="sm" onClick={() => { if (!open) toggle(); }}>Ask <kbd>Ctrl J</kbd></Button>}`.
  - `FinishedTabs` follows it in a `flex min-h-0 flex-1 flex-col` wrapper, then the `<audio>` element (`:402`, unchanged).
  - `costKey` is a counter bumped by `SummaryPanel`'s `onCostChanged`.
- Drop the `motion.div` slide-in and `bg-muted/50`.
- The `title` prop of `PageFrame` stays unset, because `MeetingHeader` owns the editable `h1`.
- The error state (`page.tsx:350-364`) keeps its text and uses `<BackCrumb fallback="/" fallbackLabel="Meetings" />` in place of the blue button.

**Steps:**
- [ ] Failing tests:
  - wiring: `the finished page renders the rail with tasks and a back crumb`: the mocked `MeetingRail` receives a `tasks` element of type `MeetingTasksPanel` with `meetingId 'm1'`, and the mocked `BackCrumb` receives `{fallback:'/', fallbackLabel:'Meetings'}`.
  - wiring: `Ask Ctrl J in the header opens the Ask tab`: clicking the header's `Ask` button calls the mocked `toggle` once when `open` is false.
  - menu test: `header actions render before the menu`: an `actions={<button>X</button>}` precedes the `Meeting actions` trigger.
  - `FinishedTabs` no longer renders the tasks aside inside the summary panel.
- [ ] **BUN** `tests/components/meeting-page-finished-wiring.test.tsx tests/components/meeting-page-menu.test.tsx tests/components/meeting-page-start.test.tsx` → FAIL → implement → PASS.
- [ ] Browser check: `pnpm run dev:preview`, open `/meeting-details?id=preview-1` at 1366 and 2048. At 2048 the rail sits beside the 80ch text; at 1366 it sits under the text. Both tabs work, and Esc from page focus returns to `/`.
- [ ] **GATES**; commit `feat(meeting): reading frame with back crumb, Ask Ctrl J and the Tasks | Ask rail`.

### Task 23: Summary toolbar and summary rendering `[mechanical]`
**Files:**
- `src/components/MeetingDetails/SummaryPanel.tsx:328-330` (toolbar wrapper, `justify-center`) and `:455-462` (status bar).
- `src/components/MeetingDetails/SummaryGeneratorButtonGroup.tsx:93-131` (Stop, Generate/Regenerate).
- `src/components/MeetingDetails/SummaryUpdaterButtonGroup.tsx:23-44` (Save, with the green dirty class).
- `src/app/globals.css`, next to the `.bn-container.bn-shadcn .bn-editor` rule (`:296-300`, outside `@layer`).
- Test: new `tests/components/summary-toolbar.test.tsx`.

**Behaviour:**
- The toolbar row is `flex flex-wrap items-center justify-start gap-2 px-8 py-3`, above the text.
- Generate/Regenerate is `variant="default"` with no gradient classes. Its label is `Regenerate` when `hasSummary`, else `Generate summary`.
- Stop, language, AI Model, Template, Save and Copy are `variant="outline"` with no colour classes.
- Save shows its dirty state with `border-primary` instead of `bg-green-200`.
- The status bar renders only for `summaryStatus === 'error'`, so the green "Summary completed" bar is gone.
- Headings: `.bn-container [data-content-type=heading]{font-weight:600}`, plus `--level` values of `1.125rem` (h1), `1rem` (h2) and `0.9375rem` (h3). This is the DESIGN.md scale (section titles 16 px/600).

**Steps:**
- [ ] Failing tests (mount `SummaryGeneratorButtonGroup` and `SummaryUpdaterButtonGroup` with the props used in `summary-panel-language.test.tsx`):
  - `Regenerate is the one primary button`: with `hasSummary` true, exactly one `Button` has `variant === 'default'`, and its text includes `Regenerate`.
  - `no green completed bar`: mount `SummaryPanel` with `summaryStatus 'completed'` → the text lacks `Summary completed`. With `'error'`, the error message is shown.
  - `a dirty Save has no green class`: the Save button's `className` lacks `bg-green`.
- [ ] **BUN** `tests/components/summary-toolbar.test.tsx tests/components/summary-*.test.tsx tests/hooks/summary-generation.test.tsx` → FAIL → implement → PASS.
- [ ] Browser check: summary headings in all three themes, at 1366 and 2048.
- [ ] **GATES**; commit `feat(summary): left-aligned toolbar with one primary Regenerate; app-scale headings; no completed bar`.

### Task 24: One-line audio summary with Change, and Meetily leftovers removed `[judgment]`
**Files:**
- `src/lib/meeting-page.ts` (add the helper).
- Create `src/components/MeetingPage/AudioSummaryLine.tsx`.
- `src/components/MeetingPage/AudioLine.tsx:1-23`.
- `src/components/LanguageSelection.tsx`: props `:116-129`, the "Languages spoken" block `:197-243`, the Auto Detect warning `:258-264`.
- `src/components/DeviceSelection.tsx`: commented-out Test Mic `:245-256`, the info bullets `:386-397`.
- Create `src/components/LanguagesSpoken.tsx`.
- Tests: `tests/lib/meeting-page.test.ts`, new `tests/components/audio-summary-line.test.tsx`.

**Interfaces:**
- `export function audioSummary(o: { micDevice: string | null; systemDevice: string | null; language: string; allowed: string[]; provider: string }): string`. Rules:
  - A device drops its `" (input)"`/`" (output)"` suffix (`DeviceSelection.tsx:106,113` builds the names that way). `null` becomes `Default`.
  - The language is `auto` → `Auto (ru, en, de)` (allowed codes, or just `Auto` when the list is empty or the provider is `parakeet`); `auto-translate` → `Auto, translated to English`; any other code → its `LANGUAGES` name (`constants/languages.ts:2`).
  - The result looks like `Mic: Headset · System audio: Speakers · Language: Auto (ru, en, de)`.
- `export function AudioSummaryLine({ changeable }: { changeable: boolean })`.
  - It renders the summary from `useConfig()` (`selectedDevices`, `selectedLanguage`, `allowedLanguages`, `transcriptModelConfig.provider`).
  - When `changeable` is true, it adds ` · Change`, a link-styled button that opens a `Popover` (`components/ui/popover.tsx`) holding `<AudioLine disabled={false} />`.
- `export function LanguagesSpoken()` holds the moved block, using `useConfig().allowedLanguages`/`setAllowedLanguages`, `addLanguage`/`removeLanguage`, and helper text "Used whenever the language is Auto, including re-transcribe and import."
- `LanguageSelection` gains `hideSpoken?: boolean`. It renders `<LanguagesSpoken />` unless `hideSpoken` is set, so Settings is unchanged. `AudioLine` passes `hideSpoken`.

**Deletions:**
- The "⚠️ Auto Detect may produce incorrect results" block.
- The DeviceSelection info bullets (Microphone/System Audio/Mic Levels/"Test Mic" tip) and the commented-out Test Mic button.
- The dead monitoring code in DeviceSelection. `isMonitoring` can never become true once the button is gone, so remove `isMonitoring`, its toggle, the `audio-levels` listener at `:128-158` and the meter renders at `:320-335`. This is only correct if nothing else sets `isMonitoring`; grep before removing.

**Steps:**
- [ ] Failing tests:
  - `audioSummary`: `('Headset (input)','Speakers (output)','auto',['ru','en','de'],'localWhisper')` → `'Mic: Headset · System audio: Speakers · Language: Auto (ru, en, de)'`; `(null,null,'auto',[],'parakeet')` → `'Mic: Default · System audio: Default · Language: Auto'`; `'de'` → `Language: German`.
  - `audio-summary-line.test.tsx`: the summary text is shown. `changeable` → a `Change` button whose click renders the mocked `DeviceSelection`. `changeable={false}` → no `Change`.
  - `LanguageSelection` with `selectedLanguage 'auto'` → the text lacks `Auto Detect may produce`.
- [ ] **BUN** `tests/lib/meeting-page.test.ts tests/components/audio-summary-line.test.tsx` → FAIL → implement → PASS.
- [ ] **GATES**; commit `feat(meeting): one-line audio summary with Change; remove Meetily device and language help`.

### Task 25: New meeting page `[judgment]`
**Files:**
- `src/app/meeting-details/page.tsx:339-345` (new-meeting shell).
- `src/components/MeetingPage/NewMeetingView.tsx:70-105` (render), `:86` (`AudioLine`).
- `src/components/MeetingPage/MeetingHeader.tsx` (`variant 'new'`).
- Tests: `tests/components/meeting-page-new.test.tsx` (its `ConfigContext` mock `:37-42` gains `allowedLanguages: ['ru','en']`; the first test `:68-75` changes), `tests/components/meeting-page-start.test.tsx`.

**Interfaces:**
- `MeetingHeader` `variant: 'new'`:
  - No emoji, no date and no `h1` wrapper.
  - A visible `<label htmlFor>` "Meeting name" on the input. Its `value` is `''` when `row.titleSource === 'default'`, with `placeholder={row.title}`.
  - A blur with an empty value saves nothing and shows no error toast, so the default title stays.
  - Category stays below the input, as today. The ⋯ menu (Delete meeting) stays.

**Layout:**
- `page.tsx` renders `<PageFrame variant="dashboard" title="New meeting" breadcrumb={<BackCrumb fallback="/" fallbackLabel="Meetings" />}>{<NewMeetingView … header={<MeetingHeader variant="new" …/>} />}</PageFrame>` (decided: `dashboard`, because `form` caps its column at 880 px and the page needs two columns).
- `NewMeetingView` gains `header: React.ReactNode` and drops its `mx-auto max-w-3xl` wrapper (`:70`). It renders `@container` → `grid gap-10 @[80rem]:grid-cols-2`, so there are two columns only from 1280 px of content width; each column is `min-w-0 max-w-[880px]`.
  - **Column 1:** `header`; then the big Start button, or the live line (`:74-77`); then `<AudioSummaryLine changeable={!disabled} />`; then `<LanguagesSpoken />`.
  - **Column 2:** `h2` "Prep material and chat", the textarea, `PrepTickList` and `PrepChat`, all unchanged.
- The Start button: `size="lg"`, at least 48 px high, with the `--rec` dot, and "Recording goes into this meeting." under it.

**Steps:**
- [ ] Failing tests:
  - `Start recording comes before the audio line, languages and prep`: in `JSON.stringify(r.toJSON())`, `indexOf('Start recording')` is less than `indexOf('Mic:')`, which is less than `indexOf('Languages spoken')`, which is less than `indexOf('Prep material')`.
  - `a default title shows an empty labelled Meeting name`: `row.titleSource 'default'` → the input has `value ''` and `placeholder` set to the row title, and a label `Meeting name` exists. Blur while empty → no `api_save_meeting_title` call and no toast.
  - `a calendar title is shown`: `titleSource 'calendar'` → `value` is the title.
  - Update `:68-75`: expect `Prep material and chat`, `Mic:`, `Start recording`, and no `Test Mic` or `Auto Detect may produce`.
- [ ] **BUN** `tests/components/meeting-page-new.test.tsx tests/components/meeting-page-start.test.tsx` → FAIL → implement → PASS.
- [ ] Browser check: `/meeting-details?id=preview-7` at 1366×768 has Start recording above the fold, a single column, and Change opens the device and language pickers. At 2048 it shows two columns, in all three themes.
- [ ] **GATES**; commit `feat(meeting): new meeting form with Start on top, one audio line and prep beside it`.

### Task 26: Real `audio-levels` while recording `[judgment][risky]` (concurrency, audio path; per-task review with a Codex pass)
Why (decided): the spec assumes an existing `audio-levels` level feed. Today the only emitter, `start_audio_level_monitoring` → `simple_level_monitor::start_monitoring` (`simple_level_monitor.rs:26-74`), sends a **fake sine wave** and is never started during a recording. The meter would lie, so this task feeds it real levels.

**Constraints:**
- Reuse the RMS the pipeline already computes: `speakers::levels::window_levels(&mic_window, &sys_window)` (`speakers/levels.rs:37`, twelve 50 ms `(mic_rms, sys_rms)` slices per 600 ms mix window), called today in `AudioPipeline::send_mixed_window` (`pipeline.rs:902-907`) only while speaker ID is on. No new pass in the capture callback (`AudioCapture::process_audio_data`).
- The only addition to the audio path is two relaxed atomic stores per mix window. No lock, no allocation, no channel.
- Emit at most 10 Hz, from a separate tokio task.

**Files:**
- `src-tauri/src/audio/recording_state.rs`: struct `:101-130`, `new` `:133`, `mod tests` `:420`.
- `src-tauri/src/audio/pipeline.rs`: `send_mixed_window` `:902-907`; `mod tests` `:1153`.
- `src-tauri/src/audio/simple_level_monitor.rs` (add the emitter next to `AudioLevelUpdate`, `:17`).
- `src-tauri/src/audio/recording_commands.rs`: both start paths, just before the manager moves into `RECORDING_MANAGER` (`:589-590`, `:782-783`) and after the `recording-started` emits (`:650`, `:843`).

**Interfaces:**
- `RecordingState` gains `mic_level: AtomicU32` and `system_level: AtomicU32` (f32 bits, `Ordering::Relaxed`), plus `pub fn set_levels(&self, mic: f32, system: f32)` and `pub fn levels(&self) -> (f32, f32)`.
- `fn peak_pair(slices: &[LevelPair]) -> (f32, f32)` (private, `pipeline.rs`): the max of each side.
- `pub fn level_update(mic: f32, system: f32, timestamp: u64) -> AudioLevelUpdate`: two `AudioLevelData` entries, `("microphone","input")` and `("system","output")`, with `rms_level` and `peak_level` = the value clamped to 0..1 and `is_active = v > 0.01`.
- `pub fn spawn_recording_levels<R: Runtime>(app: AppHandle<R>, state: Arc<RecordingState>)`: a tokio task that ticks every 100 ms while `state.is_recording()`, emits `audio-levels` when the pair changed since its last emit, and sends `(0,0)` once on pause. It exits when recording stops. Each recording has its own `RecordingState` (`recording_manager.rs:202`), so an old task always ends. An emit error is logged with `warn!` once per task, not per tick.

**Steps:**
- [ ] Failing tests:
  - `recording_state::tests::levels_store_and_read`: a fresh state reads `(0.0, 0.0)`; `set_levels(0.5, 0.1)` → `levels() == (0.5, 0.1)`.
  - `pipeline::tests::peak_pair_takes_each_sides_max`: `[(0.1,0.4),(0.3,0.2)]` → `(0.3, 0.4)`; `[]` → `(0.0, 0.0)`.
  - `simple_level_monitor::tests::level_update_shape`: `level_update(1.5, 0.0, 7)` → two entries, the mic one `device_type "input"` with `rms_level 1.0` and `is_active true`, the system one `"output"` with `is_active false`.
- [ ] **RUST** `audio::recording_state`, `audio::pipeline` and `audio::simple_level_monitor` → FAIL → implement → PASS.
- [ ] Implement `send_mixed_window`: compute `let slices = window_levels(&mic_window, &sys_window);` once; push it to the speaker track when `live_is_active()` (so speaker ID adds no second pass); then `let (m, s) = peak_pair(&slices); self.state.set_levels(m, s);`.
- [ ] Wiring: `let level_state = manager.get_state().clone();` (`recording_manager.rs:615`) before the move, then `simple_level_monitor::spawn_recording_levels(app.clone(), level_state)` after `recording-started`, in both start paths.
- [ ] `flock … cargo check -p meetily --target x86_64-pc-windows-msvc -j 4` only if the touched code has `cfg(windows)` branches; otherwise **RUST** suffices.
- [ ] Commit `feat(audio): emit real mic and system levels while recording`. Per-task review with a Codex pass: look hard for silent failures (dropped emits, a task that outlives its recording) and for anything heavier than the two atomic stores on the pipeline path.

### Task 27: Recording page: one-line header, levels, transcript right below `[judgment]`
**Files:**
- `src/components/MeetingPage/RecordingView.tsx:1-73` (header `:33`, bar `:34-57`, `barHeights` `:48`, `AudioLine` `:56`).
- Create `src/components/MeetingPage/LiveLevels.tsx`.
- `src/components/RecordingControls.tsx`: `barHeights` prop `:17,:37`, fake bars `:466-477`.
- `src/components/TranscriptView.tsx:253-260` and `src/components/VirtualizedTranscriptView.tsx:326-333`: the sticky `RecordingStatusBar`.
- Delete `src/components/RecordingStatusBar.tsx`.
- Tests: `tests/components/meeting-page-recording.test.tsx`; the `RecordingStatusBar` mocks in `tests/components/transcript-speaker-label.test.tsx:8-19`, `transcript-focus.test.tsx:7-14` and `transcript-streaming.test.tsx:8-19` are removed.

**Interfaces:**
- `LiveLevels()`: `listen<AudioLevelUpdate>('audio-levels', …)`. It picks the `device_type 'input'` and `'output'` entries and renders "Mic" and "System" labels, each with `CompactAudioLevelMeter` (`AudioLevelMeter.tsx:116`). The wrapper is `aria-hidden` (decorative; the timer carries the state). Before any event, the meters show level 0. It unlistens on unmount.
- `MeetingHeader variant 'compact'`: the title input and the ⋯ menu on one line, with no emoji and no category/date row. `actions` holds the recording controls.
- `RecordingControls`: the `barHeights` prop and the bars are removed. `RecordingView` is the only caller (`grep barHeights`).

**Layout:**
- `RecordingView` renders `px-8`, left-aligned:
  - `<BackCrumb fallback="/" fallbackLabel="Meetings" />`.
  - One header row (`flex flex-nowrap items-center gap-4`): the title (`min-w-0 flex-1`, truncating), then the timer `<span role="status"><span className="sr-only">Recording </span>01:13</span>` with the `--rec` dot, then `<LiveLevels />`, then `RecordingControls labeled` (Pause, Stop and summarize), then ⋯.
  - When `row` is null, the title is the plain `h1` "New recording".
  - Below the header: `<AudioSummaryLine changeable={false} />`, because devices cannot change mid-call. Then the failed-save alert (unchanged), then `LiveTranscriptAndNotes`.
  - Copilot stays the global `AiSidebar` `live` panel on the right (unchanged).
- Saving and Not saved texts stay as they are (`:37`).

**Anti-goal:**
- Nothing on this page calls `stop_recording` on unmount or on back. `BackCrumb`/Esc go through `goBack` → `confirmLeave()`.
- `useRecordingStop` is only reached from Stop (`:43`).

**Steps:**
- [ ] Failing tests in `meeting-page-recording.test.tsx` (add `next/navigation` and `BackCrumb` mocks; the mocked `BackCrumb` renders a button that calls a spy `onBack`):
  - Update `the bar shows…`: it contains `Recording 01:13`, `Pause`, `Stop and summarize`, `Live transcript` and `Mic: mic`, and lacks `Microphone`.
  - `the header is one row`: the timer, `LiveLevels` and the Stop button share one parent element.
  - `levels come from audio-levels`: the mocked `listen` captures the handler. Fire `{levels:[{device_type:'input', rms_level:0.5, peak_level:0.5, is_active:true, device_name:'microphone'}]}` → the Mic `CompactAudioLevelMeter` gets `rmsLevel 0.5`.
  - `leaving never stops the recording`: unmount the view → `calls` has no `stop_recording`, and `handleRecordingStop` was not called.
  - In `transcript-*.test.tsx`, the transcript renders with `isRecording` true and no `Recording •` text.
- [ ] **BUN** `tests/components/meeting-page-recording.test.tsx tests/components/transcript-*.test.tsx tests/components/recording-controls.test.tsx` → FAIL → implement → PASS.
- [ ] Browser check: at 1366 the header stays on one line with the sidebar both expanded and collapsed. The live transcript starts right under the audio line.
- [ ] **GATES**; commit `feat(recording): one-line header with live levels; transcript right below; drop the duplicate overlay`.

### Task 28: Lane-owned confirm and date leftovers `[mechanical]`
**Files:**
- `src/components/MeetingDetails/TranscriptButtonGroup.tsx:51-54` (`window.confirm`).
- `src/components/AISummary/index.tsx:562` (`toLocaleDateString`, a US date).
- `src/components/MeetingPage/DeleteRecordingDialog.tsx:21` (`variant="destructive"`).
- Test: `tests/components/speakers-bar.test.tsx:238` (it stubs `window.confirm` with `EXACT`).

**Behaviour:**
- `handleIdentify` becomes `async`: `if (replacesLabels && !(await confirmAction({ title: 'Replace speaker labels?', body: 'Labels, including your manual fixes, will be replaced.', confirmLabel: 'Replace', destructive: true }))) return;`.
- The markdown date line becomes `formatLongDate(new Date(meeting?.created_at ?? Date.now()))`.
- "Delete recording" becomes `<Button variant="outline" className={DESTRUCTIVE_OUTLINE}>`.

**Steps:**
- [ ] Update the failing test: mock `../../src/lib/confirm` with `confirmAction: mock(async () => answer)`. Assert it is called with `title 'Replace speaker labels?'`, and that `false` means no identify call.
- [ ] Failing test in a new `tests/components/delete-recording-dialog.test.tsx` (mock `ui/dialog` as Phase 1 Task 2 does): the "Delete recording" button's `variant` is `'outline'` and its className contains `text-destructive`.
- [ ] **BUN** `tests/components/speakers-bar.test.tsx tests/components/delete-recording-dialog.test.tsx` → FAIL → implement → PASS; **GATES**.
- [ ] Commit `fix(meeting): app confirm for replacing speaker labels; long date in summary markdown; outline Delete recording`.

### Task 29: Preview mocks, three-theme and a11y check `[judgment]`
**Files:** `src/lib/browser-preview.ts` (one commit, per the lane rule).

**Steps:**
- [ ] Run `pnpm run dev:preview`. On `/meeting-details?id=preview-1` (finished) and `?id=preview-7` (new), look for "unmocked command" warnings in the console. Add only the mocks those pages need (e.g. `tasks_list` rows for `preview-1`, if it has none, so the rail shows `Tasks (n)`).
- [ ] Playwright screenshots of both pages at 1366, 1920, 2048 and 2560, in Light, Dark and Conclave. Check the success criteria: no narrow centred column at 2048, the rail at 360–560 px, Start above the fold at 1366×768.
- [ ] `node frontend/scripts/a11y-check.mjs http://localhost:3118 /meeting-details?id=preview-1 /meeting-details?id=preview-7` → exit 0. Fix serious or critical issues in lane files, such as tab roles and kbd contrast.
- [ ] **GATES**; commit `chore(preview): meeting page mocks for the rail and new meeting`.

## Lane C: Settings and Models & keys (Tasks 30–39)

**Lane:** worktree `/workspace/.worktrees/cl-c`, branched from `cleanup` after Phase 1 merges. Spec §2.5, plus the Settings items of §2.7 (Notifications accessible name, the Settings note contrast, dates, confirms). Mockup §4.
**Risky:** this lane touches persisted settings. Every task in it gets a per-task review with a Codex pass (spec §7). Tell the reviewer to look hard for silent failures and for whole-object saves that write stale values.

**Commands** (run from `/workspace/.worktrees/cl-c/frontend`):
- **BUN** `<file>`: `flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <file>`
- **RUST** `<filter>`: `cd src-tauri && CARGO_TARGET_DIR=/workspace/target flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib <filter> -j 4`. This needs `frontend/out` and the stub sidecar from AGENTS.md.
- **GATES**: the frontend gates from the Global Constraints, and the Rust gates after Task 31.

**Facts this lane relies on (verified):**
- **Old tab ids** (`app/settings/page.tsx:26-37`): `general`, `recording`, `Transcriptionmodels`, `speakers`, `summaryModels`, `copilot`, `knowledge`, `costs`, `templates`, `beta`. Today `?tab=` is read once at mount (`:45-46`).
- **Links into Settings** in `src/`: `MeetingsTable.tsx:181` (`?tab=knowledge`), `Copilot/CopilotPane.tsx:28,148` (`?tab=copilot`), `Sidebar/index.tsx:117` (`/settings`). The tray opens `/settings` (`src-tauri/src/tray.rs:66`).
- **Key storage.**
  - `SettingsRepository::save_api_key` (`database/repositories/setting.rs:75-115`) writes the secret store, then upserts the `settings` row. On an empty table that upsert **creates a summary config** (`'openai','gpt-4o-2024-11-20','large-v3'`). The new command must not reuse it.
  - `get_api_key_in` (`:124-155`) reads the store first and the plain column second (`secrets::read_or_plain`), so a key stored with no settings row still reads back.
  - The custom server key lives in the store under `secrets::CUSTOM_OPENAI_API_KEY`. `save_custom_openai_config` (`:377-411`) updates only `customOpenAIConfig` when the row exists.
  - In tests, `secrets::store()` is a per-thread `MemoryStore` (`secrets/mod.rs:141-148`).
- **`api_save_model_config`** (`api/api.rs:547-602`) overwrites provider, model, whisperModel and ollamaEndpoint. It is registered at `lib.rs:899`.
- **Feature overrides.** Each one falls back to the summary model unless provider **and** model are both set:
  - Copilot: `copilot_get_settings`/`copilot_save_settings {settings}`. Fields: `provider, model, answer_language, alert_words`.
  - Knowledge: `knowledge_get_settings` returns a view with an extra `calendar_host`; `knowledge_save_settings {input}` takes `provider, model, my_names, extract_enabled, digest_enabled, calendar_marker`.
  - Template assistant: `getAssistSettings`/`saveAssistSettings` in `lib/templateAssist.ts:38-41`. Fields: `provider, model, includeTranscript, hideEditHint`.
  - Dictate: `dictate_get_settings`/`dictate_save_settings {settings}`, with `cleanup_provider` and `cleanup_model` (`dictate/session.rs:603-616`).
  - Rust: `summary/llm_target.rs:22-38`.
- **Stale key holders.** These keep a copy of the summary key, and a later Summary save writes that copy back with `api_save_model_config`:
  - `ConfigContext` holds `providerApiKeys` and updates it from the `model-config-updated` event (`contexts/ConfigContext.tsx:359-372`, `:472`).
  - `ModelSettingsModal` copies the key into local state (`ModelSettingsModal.tsx:128`, `:404-413`).
  - So a key saved anywhere must refresh both.
- **Practice:** the Models card is `components/Practice/ModelsCard.tsx` (Lane E's file). It is self-contained (it loads and saves `practice_get_settings` and `practice_save_settings`) and is rendered at `app/practice/page.tsx:225`.

---

### Task 30: `lib/settings-panes.ts`, the pane map and search index [mechanical]
**Files:** create `frontend/src/lib/settings-panes.ts`, `frontend/tests/lib/settings-panes.test.ts`.
**Interfaces:**
- **Produces the shared names:**
  - `export type SettingsPane = 'recording' | 'transcription' | 'speakers' | 'models' | 'summaries' | 'copilot' | 'templates' | 'names' | 'app'`
  - `paneForTab(tab: string | null): SettingsPane`
  - `settingsHref(pane: SettingsPane): string`
- **Also produces:**
  - `SETTINGS_GROUPS: { label: 'Capture' | 'AI' | 'You' | 'App'; panes: { id: SettingsPane; label: string }[] }[]`
  - `paneLabel(p: SettingsPane): string`
  - `isKnownTab(tab: string): boolean`
  - `SETTINGS_INDEX: { pane: SettingsPane; label: string; helper?: string }[]`
  - `searchSettings(q: string): { pane: SettingsPane; label: string }[]`
  - `settingId(label: string): string` (a DOM id: `setting-` plus the lowercased label, with each run of non-alphanumerics turned into `-`)
- [ ] **Failing tests:**
  - **`paneForTab`:**
    - `null` and `'nonsense'` → `'recording'`.
    - `'general'` → `'app'`, `'recording'` → `'recording'`.
    - `'Transcriptionmodels'` and `'transcriptionmodels'` → `'transcription'`.
    - `'beta'` → `'transcription'`, because the Import toggle moves there.
    - `'speakers'` → `'speakers'`.
    - `'summaryModels'` and `'summary'` → `'summaries'`.
    - `'copilot'` → `'copilot'`, `'knowledge'` → `'names'`, `'costs'` → `'models'`, `'templates'` → `'templates'`.
    - Every `SettingsPane` id maps to itself.
  - **`settingsHref('models')`** is `'/settings?tab=models'`.
  - **`SETTINGS_GROUPS`:**
    - The group labels are, in order, `Capture, AI, You, App`.
    - The pane labels are exactly:
      - `Recording & devices`, `Transcription & vocabulary`, `Speakers`
      - `Models & keys`, `Summaries`, `Copilot`, `Templates`
      - `Names & calendar`
      - `Appearance & app`
    - Each pane appears once.
  - **Link grep:**
    - Walk `src/` (as `theme-guard.test.ts:20-26` does). Every match of `/\/settings\?tab=([A-Za-z]+)/g` passes `isKnownTab`.
    - No source contains `/settings?tab=${`, so dynamic links must use `settingsHref`.
  - **`searchSettings`:**
    - `'calendar'` contains `{ pane: 'names', label: 'Calendar' }`.
    - `'STARTS/STOPS'` finds `Windows notifications when recording starts/stops` in `recording`.
    - `'deadline words'` matches nothing; it returns `[]`.
    - `''` and `'   '` → `[]`.
    - The match is a case-insensitive substring of the label or helper.
  - **Index drift:** every `SETTINGS_INDEX` label appears literally in some file under `src/components/` or `src/app/settings/`. `lib/settings-panes.ts` itself is excluded.
- [ ] **BUN** `tests/lib/settings-panes.test.ts` → FAIL (module missing).
- [ ] **Implement.**
  - **The alias table:** `TAB_ALIASES: Record<string, SettingsPane>`, keyed lowercase, holding the old ids plus every pane id. `paneForTab` lowercases its input and falls back to `'recording'`.
  - **`SETTINGS_INDEX`** holds these labels (helpers are short plain text):
    - **recording:** `Save audio recordings`, `Save location`, `Recording start notification`, `Windows notifications when recording starts/stops`, `Meeting detection`, `Remind me about audio older than`, `Default audio devices`
    - **transcription:** `Transcription model`, `Import audio & retranscribe`, `Vocabulary`
    - **speakers:** `Identify speakers`, `Known voices`
    - **models:** `Claude`, `OpenAI`, `Groq`, `OpenRouter`, `Custom server`, `Summary`, `Copilot`, `Tasks & digests`, `Template assistant`, `Dictate cleanup`, `Practice`, `Model prices`
    - **summaries:** `Auto summary`, `Summary language`
    - **copilot:** `Answer language`
    - **templates:** `Templates`, `Include the transcript by default`
    - **names:** `My names`, `Extract my tasks after each summary`, `Weekly digest`, `Alert me when someone says`, `My calls marker`, `Calendar`
    - **app:** `Theme`, `Data storage locations`, `Updates`, `Check for updates on start`, `GitHub token`, `Backups`
  - The drift test stays red until Tasks 34–38 add the labels. So in this task, mark that one test `test.todo`, and turn it on in Task 38.
- [ ] **BUN** → PASS. Commit `feat(settings): pane map, old tab ids and search index`.

### Task 31: Rust `api_save_provider_key` and `saveProviderKey` [mechanical] [risky]
**Files:**
- `frontend/src-tauri/src/database/repositories/setting.rs`: a new method after `save_api_key` (`:115`), and a new `#[cfg(test)] mod provider_key_tests` after `secret_tests`.
- `frontend/src-tauri/src/api/api.rs`: a new command after `api_get_api_key` (`:628`).
- `frontend/src-tauri/src/lib.rs`: register it after `:900`.
- Create `frontend/src/lib/models-keys.ts` and `frontend/tests/lib/models-keys.test.ts`.

**Interfaces:**
- **Rust:**
  - `pub async fn save_provider_key(pool: &SqlitePool, provider: &str, api_key: &str) -> Result<(), sqlx::Error>`
  - `#[tauri::command] pub async fn api_save_provider_key<R: Runtime>(_app: AppHandle<R>, state: tauri::State<'_, AppState>, provider: String, api_key: String) -> Result<(), String>`
- **TS:** `export async function saveProviderKey(provider: string, apiKey: string): Promise<void>`

- [ ] **Failing Rust tests** in `provider_key_tests` (use `crate::database::repositories::test_pool`):
  - **`save_provider_key_leaves_model_config`:**
    - Seed the row: `INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint) VALUES ('1','claude','claude-sonnet-5-5','large-v3-turbo','http://h:11434')`.
    - Call `save_provider_key(&pool, "openrouter", "  sk-or  ")`.
    - `get_model_config` still returns exactly those four values.
    - `get_api_key(&pool,"openrouter")` is `Some("sk-or")` (trimmed).
    - The plain `openRouterApiKey` column is NULL.
  - **`save_provider_key_on_fresh_db_keeps_default_provider`** (Lane F's finding):
    - On a fresh table, saving `"claude"`, `"k"` leaves `get_model_config` at `None`, so the app's own default still applies and no row says OpenAI.
    - `get_api_key(&pool,"claude")` is `Some("k")`.
    - A later `save_model_config(&pool,"claude","claude-sonnet-5-5","large-v3",None)`, standing in for `complete_onboarding`'s save, followed by `save_provider_key` again, keeps provider `claude`.
  - **`save_provider_key_custom_keeps_endpoint`:**
    - First call `save_custom_openai_config` with endpoint `http://lm:1234/v1`, model `m1` and key `old`, over a seeded `claude` row.
    - `save_provider_key(&pool,"custom-openai","new")` keeps the endpoint and model.
    - The key becomes `new`, and the summary provider is still `claude`.
  - **`save_provider_key_custom_without_config_errors`:** returns `Err`, and the store has no `CUSTOM_OPENAI_API_KEY`.
  - **`save_provider_key_rejects_blank_and_keyless`:**
    - `("claude","   ")`, `("ollama","k")` and `("builtin-ai","k")` each return `Err`.
    - `("nope","k")` returns `Err`.
    - Nothing is stored.
- [ ] **RUST** `provider_key_tests` → FAIL.
- [ ] **Implement `save_provider_key`.**
  - Trim the key, and return `Err(Protocol("Enter a key"))` when it is empty.
  - **`custom-openai`:**
    - Call `get_custom_openai_config(pool)`. `None` returns `Err(Protocol("Set up the custom server first"))`.
    - `Some(mut c)` sets `c.api_key = Some(key)`, then calls `save_custom_openai_config(pool, &c)`.
  - **Every other provider:**
    - Map `openai`, `claude`, `groq` and `openrouter` to their column, as at `:87-99`. Anything else, `ollama` and `builtin-ai` included, is `Err(Protocol("Invalid provider: …"))`.
    - Then `secrets::store().set(column, key).map_err(store_err)?`.
    - Then `UPDATE settings SET "<column>" = NULL WHERE id = '1'`. **No INSERT**: it never creates or changes the summary config.
  - **The command:** a thin wrapper that logs the provider only (never the key) and maps the error to a string. It does not shut down the sidecar.
- [ ] **RUST** `provider_key_tests` and `secret_tests` → PASS.
- [ ] **Failing Bun test `saveProviderKey`** in `tests/lib/models-keys.test.ts`. Mock `@tauri-apps/api/core` `invoke` and `@tauri-apps/api/event` `emit`, as `tests/lib/meetings.test.ts:7-12` does.
  - It calls `invoke('api_save_provider_key', { provider: 'claude', apiKey: 'sk' })`, then `invoke('api_get_model_config')`.
  - It emits `'model-config-updated'` with that result. When the config is `null` it does not emit.
  - When the save rejects, it rejects and does not emit.
- [ ] **Implement** `saveProviderKey` as tested. The event refreshes `ConfigContext.providerApiKeys` for the summary provider and the Summary editor (Facts: stale key holders). Lane F's onboarding uses it too.
- [ ] **BUN** `tests/lib/models-keys.test.ts` → PASS. Commit `feat(settings): api_save_provider_key saves one key and nothing else`.

### Task 32: Feature model choices in `lib/models-keys.ts` [judgment] [risky]
**Files:** `frontend/src/lib/models-keys.ts` and `frontend/tests/lib/models-keys.test.ts` (both from Task 31).
**Interfaces (produces):**
- `export type ModelFeature = 'copilot' | 'knowledge' | 'templateAssist' | 'dictateCleanup'`
- `export type FeatureChoice = { provider: string; model: string } | null` (`null` = "Same as Summary")
- `export function featureChoice(provider?: string | null, model?: string | null): FeatureChoice`
- `export async function loadFeatureModels(): Promise<Record<ModelFeature, FeatureChoice>>`
- `export async function saveFeatureModel(feature: ModelFeature, choice: FeatureChoice): Promise<void>`

- [ ] **Failing test `models_keys_preserves_choices`.** Mock `invoke` by command:
  - `copilot_get_settings` → `{provider:'claude', model:'claude-sonnet-5-5', answer_language:'de', alert_words:['Sasha']}`
  - `knowledge_get_settings` → `{provider:null, model:null, my_names:['Sasha'], extract_enabled:true, digest_enabled:false, calendar_host:'cal.example', calendar_marker:'📞'}`
  - `template_assist_get_settings` → `{provider:'openai', model:'gpt-5', includeTranscript:true, hideEditHint:true}`
  - `dictate_get_settings` → the preview dictate object (`lib/browser-preview.ts:103`) with `cleanup_provider:'groq', cleanup_model:null`

  Assert:
  - **Loading.** `loadFeatureModels()` equals `{copilot:{provider:'claude',model:'claude-sonnet-5-5'}, knowledge:null, templateAssist:{provider:'openai',model:'gpt-5'}, dictateCleanup:null}`. A half-set Dictate pair follows Summary today, so it shows "Same as Summary". No `*_save_*` command was called.
  - **Copilot.** `saveFeatureModel('copilot', {provider:'openai', model:'gpt-5'})` calls `copilot_save_settings` with `{settings:{provider:'openai', model:'gpt-5', answer_language:'de', alert_words:['Sasha']}}`.
  - **Knowledge.** `saveFeatureModel('knowledge', {provider:'groq', model:'llama'})` calls `knowledge_save_settings` with `{input:{provider:'groq', model:'llama', my_names:['Sasha'], extract_enabled:true, digest_enabled:false, calendar_marker:'📞'}}`. There is no `calendar_host`.
  - **Template assistant.** `saveFeatureModel('templateAssist', null)` calls `template_assist_save_settings` with `{settings:{provider:null, model:null, includeTranscript:true, hideEditHint:true}}`.
  - **Dictate cleanup.** `saveFeatureModel('dictateCleanup', {provider:'claude', model:'claude-haiku-5-5'})` calls `dictate_save_settings` with the full dictate object, changed only in `cleanup_provider` and `cleanup_model`.
  - **Fresh read.** Each save reads the feature's settings right before writing; the call order is get, then save.
  - **`featureChoice`:**
    - `('claude','m')` → `{provider:'claude', model:'m'}`.
    - `('groq', null)`, `(null,'m')`, `('','m')` and `('claude','  ')` → `null`.
    - Whitespace is trimmed.
- [ ] **BUN** `tests/lib/models-keys.test.ts` → FAIL.
- [ ] **Implement.** Saves do a read-modify-write of that feature's own settings object, so its other fields (alert words, names, marker, dictate modes) are never written from a stale copy. Reuse `getAssistSettings` and `saveAssistSettings`. A save error propagates.
- [ ] **BUN** → PASS. Commit `feat(settings): per-feature model choices load and save without touching other fields`.

### Task 33: `SettingsRow` and the inline "Saved ✓" [mechanical]
**Files:** create `frontend/src/components/Settings/SettingsRow.tsx`, `frontend/tests/components/settings-row.test.tsx`.
**Interfaces (produces):**
- `export function SettingsRow(props: { label: string; helper?: React.ReactNode; htmlFor?: string; saved?: boolean; block?: boolean; children?: React.ReactNode }): JSX.Element`
  - The row's `id` is `settingId(label)`.
  - Label and helper sit on the left and the control on the right. `block` puts the control full width under the label, for editors and lists.
- `export function useSavedTick(): [saved: boolean, markSaved: () => void]` shows the tick for 2 s, and each new call restarts it.
- [ ] **Failing tests:**
  - The label renders as a `<label for=htmlFor>` when `htmlFor` is given, and as text otherwise.
  - The helper text renders.
  - `saved` shows `Saved ✓` inside `role="status"` (`aria-live="polite"`); not saved, the text is absent.
  - The row `id` is `setting-windows-notifications-when-recording-starts-stops` for that label.
  - `useSavedTick`: `markSaved()` → true; after 2 s (fake timers) → false.
- [ ] **BUN** `tests/components/settings-row.test.tsx` → FAIL.
- [ ] **Implement** with DESIGN.md tokens only (`text-muted-foreground` helper, `border-border` divider).
- [ ] **BUN** → PASS. Commit `feat(settings): one row pattern with an inline Saved tick`.

### Task 34: Settings shell: form frame, grouped sub-nav, search, no Back, no tabs [judgment]
**Files:**
- Rewrite `frontend/src/app/settings/page.tsx`:
  - Drop `TABS` (`:26-37`), the underline state (`:47-48`, `:73-82`), the Back header (`:84-100`) and `Tabs` (`:106-170`).
  - Keep the transcript-config load (`:53-71`) and the `Suspense` wrapper (`:177-183`).
- Create `frontend/src/components/Settings/SettingsNav.tsx` and `frontend/tests/components/settings-page.test.tsx`.

**Interfaces:**
- **Consumes:** `PageFrame` (Phase 1); `SETTINGS_GROUPS`, `paneForTab`, `paneLabel`, `settingsHref`, `searchSettings` and `settingId` (Task 30); `confirmLeave(): Promise<boolean>` (`lib/unsaved-guard.ts`, async since Phase 1).
- **Produces** `SettingsNav({ pane, onPick }: { pane: SettingsPane; onPick: (p: SettingsPane, label?: string) => void })`.

- [ ] **Failing tests.** Mock `next/navigation` with `useSearchParams` and `useRouter().replace`, and mock every pane component module to a stub that prints its name.
  - With `?tab=knowledge`, the h1 is `Names & calendar` and the Knowledge stub renders.
  - With `?tab=costs`, the h1 is `Models & keys`.
  - No button is named `Back`, and there is no `role="tab"` or `role="tablist"`.
  - `nav[aria-label="Settings"]` shows the four group labels, with the current pane's item `aria-current="page"`.
  - Clicking `Copilot` calls `router.replace('/settings?tab=copilot')`.
  - When `confirmLeave` resolves false, clicking a pane does not navigate.
  - With `?tab=templates`, the (pass-through mocked) `PageFrame` gets `variant 'list'` and a `subnav`; with `?tab=copilot`, `variant 'form'`.
  - Typing `calendar` into `Search settings` lists `Calendar` with the pane name `Names & calendar`. Clicking it replaces to `?tab=names`.
  - `zzz` shows `No settings match`.
  - No text `Beta` anywhere.
- [ ] **BUN** `tests/components/settings-page.test.tsx` → FAIL.
- [ ] **Implement.**
  - **State lives in the URL.** `pane = paneForTab(searchParams.get('tab'))` on every render, so an in-app link to another pane works without a remount.
  - **Sub-nav clicks:** `if (await confirmLeave()) router.replace(settingsHref(p))`. After a search pick, `requestAnimationFrame` scrolls `document.getElementById(settingId(label))` into view.
  - **Frame:** `<PageFrame variant={pane === 'templates' ? 'list' : 'form'} title={paneLabel(pane)} subnav={<SettingsNav …/>}>`. The Templates pane uses `list` (decided) so its two-pane editor gets the full width instead of the 880 px form column.
  - **Pane switch.** A `switch (pane)` renders the existing components until later tasks replace them:
    - recording → `RecordingSettings`
    - transcription → `TranscriptSettings`
    - speakers → `SpeakerSettings`
    - models → `CostsSettings` (filled in by Task 35)
    - summaries → `SummaryModelSettings`
    - copilot → `CopilotSettings`
    - templates → `TemplateAssistSettings` + `TemplateSettings`
    - names → `KnowledgeSettings`
    - app → `PreferenceSettings` + `UpdatesSettings`
  - `BetaSettings` is no longer imported; Task 37 moves its toggle.
  - **Search field:** an `Input` with `aria-label="Search settings"` at the top of the sub-nav. Results are buttons showing the label plus a muted pane name.
- [ ] **BUN** the new test, then **GATES** → PASS. Commit `feat(settings): grouped sub-nav with search in the form frame; no Back or tab row`.

### Task 35: Models & keys pane [judgment] [risky]
**Files:**
- Create `frontend/src/components/Settings/ModelsKeysPane.tsx`, `frontend/src/components/Settings/SummariesPane.tsx` and `frontend/tests/components/models-keys-pane.test.tsx`.
- Edit `frontend/src/components/SummaryModelSettings.tsx`:
  - Remove the Auto Summary card `:127-135` and `<SummaryLanguageSettings />` `:137`.
  - Add the prop `onSaved?: () => void`, called after a successful save at `:118`; the success toast is dropped.
- Edit `frontend/src/app/settings/page.tsx` (the models and summaries cases).

**Interfaces:**
- **Consumes:**
  - `saveProviderKey`, `loadFeatureModels`, `saveFeatureModel` and `featureChoice` (Tasks 31–32).
  - `configuredProviders`, `PROVIDERS` and `LlmOverridePicker` (`components/shared/LlmOverridePicker.tsx`).
  - `useConfig().updateProviderApiKey`.
  - `ModelsCard` from `@/components/Practice/ModelsCard`, imported unchanged.
  - `CostsSettings`, `SettingsRow` and `useSavedTick`.
- **Produces:** `ModelsKeysPane()` and `SummariesPane()`.

- [ ] **Failing tests.** Mock `invoke`:
  - `api_get_api_key` returns `'sk-c'` for claude and `''` otherwise.
  - `api_get_custom_openai_config` returns `null`.
  - Use the Task 32 feature fixtures.
  - `practice_get_settings` returns `{chat_model:'claude-sonnet-5-5', plan_model:'claude-opus-5-5', plan_effort:'high'}`.
  - `costs_get_prices` returns `[]`.

  Assert:
  - **Providers.** The `Claude` row says `Key set` and has `Replace`; `OpenAI` says `Not set` and has `Add key`. The `Custom server` row says the custom server is set up under Summary and has no key button.
  - **Adding a key.**
    - Clicking `Add key` on OpenAI shows a `type="password"` input named `OpenAI API key`.
    - Typing `sk-o` and clicking `Save` calls `api_save_provider_key` with `{provider:'openai', apiKey:'sk-o'}`. It never calls `api_save_model_config`.
    - The row then says `Key set` and shows `Saved ✓`, and the input is cleared and hidden.
  - **A failed key save** shows an error toast and keeps the typed value.
  - **Feature rows.**
    - The `Copilot` row's provider select shows `Claude`.
    - `Tasks & digests`, `Template assistant` (shows OpenAI) and `Dictate cleanup` show `Same as Summary` where the fixture is null.
    - Picking a model for `Tasks & digests` saves through `knowledge_save_settings` with `my_names` intact, and shows `Saved ✓`.
  - **Practice row.** It shows the `Conversation`, `Plan and report` and `Effort` selects with the fixture values. No "Same as Summary" is offered there.
  - **Prices.** A `Model prices` heading is present.
  - **Mount writes nothing.** Mounting calls no `*_save_*` command.
  - **`SummariesPane`** shows `Auto summary`, with the switch named `Auto summary` and helper `Generate the summary when a recording stops`, plus `Summary language`.
- [ ] **BUN** `tests/components/models-keys-pane.test.tsx` → FAIL.
- [ ] **Implement `ModelsKeysPane`.** Three sections, each with an h2: `Providers`, `Which model each feature uses`, `Prices (per 1M tokens)`. Save behaviour (decided): every row saves on change, except the Summary row, which keeps `SummaryModelSettings`' explicit Save (it takes provider, model, endpoint and key together) and shows `Saved ✓` after it.
  - **Providers:**
    - Rows for `claude`, `openai`, `groq` and `openrouter`, labelled from `PROVIDERS`. The status comes from `configuredProviders()`.
    - `Add key`/`Replace` opens an inline password input with Save and Cancel. Save calls `saveProviderKey`, then `updateProviderApiKey(provider, key)`, marks the row saved, and bumps `keysVersion`.
    - **Custom server row:**
      - When `api_get_custom_openai_config` returns a config, it shows the endpoint host and `Replace key` (same flow, provider `custom-openai`).
      - Otherwise the helper reads "Set it up in the Summary row: choose Custom Server (OpenAI)".
  - **Feature rows:**
    - `Summary` is a `block` row holding `<SummaryModelSettings key={keysVersion} onSaved={() => setKeysVersion(v => v + 1)} />`. Remounting reloads the key, so the editor never saves a stale one back.
    - `Copilot`, `Tasks & digests`, `Template assistant` and `Dictate cleanup` each hold `<LlmOverridePicker key={keysVersion} …>`. Use the pending-provider pattern from `CopilotSettings.tsx:18-19,74-88`: a provider alone is shown but not saved. A change calls `saveFeatureModel`, then the tick.
    - `Practice` (helper "Claude only: role-play partner, plan and report") holds `<ModelsCard />`.
  - **Prices:** `<CostsSettings key={keysVersion} />`.
  - **`SummariesPane`:**
    - The `Auto summary` row moves from `SummaryModelSettings.tsx:127-135`, with the same `useConfig` toggle.
    - `<SummaryLanguageSettings />` sits in a `Summary language` block row.
    - A last row, `Summary model`, says "Set in Models & keys" and links to `settingsHref('models')`.
  - **Page:** models → `<ModelsKeysPane />`, summaries → `<SummariesPane />`.
- [ ] **BUN** the new test plus the `copilot-settings`, `costs-settings` and `practice-models-card` tests, then **GATES** → PASS. Commit `feat(settings): Models & keys pane with per-provider keys, per-feature models and prices`.

### Task 36: Remove the per-feature pickers; Copilot, Templates and Names & calendar panes [judgment] [risky]
**Files:**
- `frontend/src/components/CopilotSettings.tsx`:
  - Remove the picker `:74-88` and `pendingProvider` `:18-19`.
  - Add the prop `part: 'answer' | 'alert-words'`. `answer` renders the answer-language row (`:90-105`); `alert-words` renders the alert-words block (`:107-142`). Both keep the existing load and commit queue.
- `frontend/src/components/KnowledgeSettings.tsx`: remove the picker `:163-177` and `pendingProvider` `:53-54`.
- `frontend/src/components/TemplateAssistSettings.tsx`: remove the picker `:61-74` and `pendingProvider` `:12-13`.
- `frontend/src/components/shared/LlmOverridePicker.tsx` `:92` and `:105`: `Same as summaries` → `Same as Summary`.
- `frontend/src/app/settings/page.tsx`.
- Tests:
  - `tests/components/copilot-settings.test.tsx`: drop `:166-203`; those cases now live in Tasks 32 and 35.
  - `tests/components/template-assist-settings.test.tsx:102`.
  - `tests/components/knowledge-settings.test.tsx`.

**Interfaces:** consumes `SettingsRow`, `useSavedTick` and `settingsHref`. `toInput` (`KnowledgeSettings.tsx:39-48`) still sends the loaded `provider` and `model` unchanged, which is safe because they come from a fresh load on mount.
- [ ] **Failing tests:**
  - **Copilot:**
    - `CopilotSettings part="answer"` has no `copilot-provider` select and has a `Model` row linking to `/settings?tab=models` with the text `Set in Models & keys`.
    - `part="alert-words"` adds and removes a word, and the save keeps `provider:'claude', model:'m'` from the load.
  - **Knowledge:**
    - It has no `knowledge-provider`.
    - It shows the rows `My names`, `Extract my tasks after each summary`, `Weekly digest`, `My calls marker` and `Calendar`.
    - A toggle save sends the loaded provider and model unchanged and shows `Saved ✓`.
  - **Template assistant:**
    - It has no picker.
    - It has `Include the transcript by default` as a row.
    - It has a `Model` link row to `/settings?tab=models`.
  - The rest of the existing Knowledge and Copilot tests stay green.
- [ ] **BUN** those three files → FAIL.
- [ ] **Implement.**
  - **Rows.** Convert each card to `SettingsRow`s. Successful saves call `markSaved()`; error toasts stay.
  - **Panes:**
    - copilot → `<CopilotSettings part="answer" />`.
    - names → `<KnowledgeSettings />` then `<CopilotSettings part="alert-words" />` in an `Alert me when someone says` block row.
    - templates → `TemplateAssistSettings` then `TemplateSettings`.
  - **Knowledge dates:** last sync and last error use `formatRowDate(new Date(x))` (`KnowledgeSettings.tsx:262,266`).
  - **Contrast:** `text-red-600` → `text-destructive` (`:265,294`).
- [ ] **BUN** the three files, then **GATES** → PASS. Commit `feat(settings): model pickers move to Models & keys; Names & calendar holds alert words`.

### Task 37: Capture panes, the Notifications row and the Beta toggle [judgment] [risky]
**Files:**
- `frontend/src/components/RecordingSettings.tsx`:
  - Convert to rows.
  - The success toasts at `:125`, `:141`, `:163` and `:179-181` become `markSaved()`; the error toasts stay.
  - `Recording start notification` (`:265-275` region) keeps its store key.
- Create `frontend/src/components/Settings/NotificationsRow.tsx` and `frontend/src/components/Settings/TranscriptionExtras.tsx`.
- `frontend/src/components/PreferenceSettings.tsx`: remove the Notifications state and effects `:22-24`, `:31-81` and the card `:138-147`.
- Delete `frontend/src/components/BetaSettings.tsx`.
- Edit `frontend/src/components/SpeakerSettings.tsx` (rows) and `frontend/src/app/settings/page.tsx`.
- Create `frontend/tests/components/settings-capture.test.tsx`.

**Interfaces:**
- **`NotificationsRow()`:**
  - It reads `useConfig().notificationSettings` and calls `loadPreferences()` on mount.
  - The switch has `id="windows-notifications"` and the accessible name `Windows notifications when recording starts/stops`.
  - It is checked when `show_recording_started && show_recording_stopped`, or `true` when nothing is stored, as today at `PreferenceSettings.tsx:33-49`. It is disabled while `notificationSettings` is null.
  - `onCheckedChange(on)` calls `updateNotificationSettings({ ...s, notification_preferences: { ...s.notification_preferences, show_recording_started: on, show_recording_stopped: on } })`, then the tick. On failure it shows an error toast and the switch goes back.
- **`TranscriptionExtras()`:**
  - Row `Import audio & retranscribe`: a Switch over `useConfig().betaFeatures.importAndRetranscribe` and `toggleBetaFeature('importAndRetranscribe', on)`. The same localStorage value is kept. Helper: "Shows Import audio in the nav and Retranscribe on meetings".
  - Row `Vocabulary`: helper "Words Conclave should spell your way, in dictation and in meetings", with a link `Edit on Dictate` to `/dictate`.

- [ ] **Failing tests:**
  - **Notifications:**
    - `getByRole('switch', { name: 'Windows notifications when recording starts/stops' })` exists.
    - Turning it off calls `updateNotificationSettings` with both flags false and every other field unchanged.
    - `Saved ✓` appears.
    - A rejected update shows `toast.error` and the switch is checked again.
  - **Beta toggle:** the Import row reflects `betaFeatures.importAndRetranscribe=false` as unchecked, and toggling calls `toggleBetaFeature('importAndRetranscribe', true)`.
  - **Recording:** a meeting-detection change calls `set_recording_preferences` and shows `Saved ✓`, with no `toast.success`.
- [ ] **BUN** `tests/components/settings-capture.test.tsx` → FAIL.
- [ ] **Implement.**
  - recording → `RecordingSettings` with `NotificationsRow` placed after `Recording start notification`.
  - transcription → `TranscriptSettings` in a `Transcription model` block row, then `TranscriptionExtras`.
  - speakers → `SpeakerSettings` as an `Identify speakers` row plus a `Known voices` block row.
  - `DeviceSelection` and `LanguageSelection` are Lane B's (Task 24 deletes their Meetily help and moves "Languages spoken" into `LanguagesSpoken`). This lane renders them unchanged and adds no help text back; Phase 3's leftovers guard (Task 72) checks the result.
- [ ] **BUN** the new test, `speaker-settings` and `sidebar-nav`, then **GATES** → PASS. Commit `feat(settings): Capture panes; Windows notifications row with a name; Beta toggle moves to Transcription`.

### Task 38: Appearance & app pane [judgment]
**Files:**
- `frontend/src/components/PreferenceSettings.tsx`:
  - The two loading states (`:104`, `:109`) lose `max-w-2xl mx-auto`.
  - The Theme row's select widens from `w-40` to `w-56`, so it no longer truncates (audit Settings 4).
  - Data storage locations becomes rows. The blue Note text becomes `text-muted-foreground` (axe contrast).
- `frontend/src/components/UpdatesSettings.tsx`: rows; `Check for updates on start` shows `Saved ✓` after `toggle` succeeds.
- `frontend/src/lib/updates.ts:61-65`: `lastCheckLine` uses `formatRowDate(new Date(s.lastCheck.at))`.
- `frontend/src/components/UpdatesSettings.tsx:133`: use `formatRowDate`.
- Tests: `tests/lib/updates.test.ts:56-57`, `tests/lib/settings-panes.test.ts` (turn on the drift test from Task 30).

**Interfaces:** consumes `formatRowDate` (Phase 1) and `SettingsRow`.
- [ ] **Failing tests:**
  - `lastCheckLine` for `2026-11-18T07:56:00` (local) is `Last checked Wed 18 Nov · 07:56: Up to date`.
  - The drift test (no longer `todo`) fails until every index label exists.
- [ ] **BUN** `tests/lib/updates.test.ts tests/lib/settings-panes.test.ts` → FAIL.
- [ ] **Implement.**
  - app → `PreferenceSettings` then `UpdatesSettings`.
  - Add any missing index labels to the panes; if a label really differs, fix the index rather than invent a row.
  - The GitHub token keeps its explicit Save, Test and Remove: it is a secret entry, not a toggle.
- [ ] **BUN** those files and `updates-settings`, then **GATES** → PASS. Commit `feat(settings): Appearance & app pane; dates use the shared helper`.

### Task 39: Confirms and dates in lane files; preview mocks [mechanical]
**Files:**
- `frontend/src/components/SpeakerSettings.tsx:107`, `:190`, and the red Delete at `:208`.
- `frontend/src/components/TemplateSettings.tsx:122`, `:127`, `:239`, `:259`, `:286`, `:324`, `:453`, and the red Delete at `:513`.
- `frontend/src/lib/browser-preview.ts`.
- Tests: `tests/components/speaker-settings.test.tsx`, `template-settings-busy.test.tsx`, `template-settings-history.test.tsx` (replace the `window.confirm` stubs with a `mock.module('@/lib/confirm')` stub), and `tests/lib/browser-preview.test.ts`.

**Interfaces:** consumes `confirmAction` (Phase 1) and `formatRowDate`.
- [ ] **Failing tests:**
  - **Speakers:** deleting a voice calls `confirmAction({ title: 'Delete this voice?', body: DELETE_PROMPT, confirmLabel: 'Delete', destructive: true })`. When it resolves false, `speakers_delete` is not called.
  - **Templates:**
    - With a dirty draft, `New template` awaits `confirmAction({ title: 'Discard unsaved changes?', confirmLabel: 'Discard', destructive: true })`. When it resolves false, the draft is kept.
    - The history and deleted lists show `formatRowDate` text.
  - **Delete buttons:** the voice Delete (`speaker-settings`) and the template Delete (`template-settings-busy`) have `variant 'outline'` and a className containing `text-destructive`.
  - **Preview:** `browser-preview` answers `api_save_provider_key` with null and `dictate_get_settings` with the existing object.
- [ ] **BUN** those four files → FAIL.
- [ ] **Implement.**
  - **Speakers:** `if (!(await confirmAction(…))) return`.
  - **Templates:** `select`, `startNew` (now `async`, returning `Promise<boolean>`), `reviewVersion`, `restoreDeleted` and the Create-with-AI click await the same Discard confirm.
  - **Dates:** `toLocaleString` and `toLocaleDateString` → `formatRowDate`.
  - **Delete buttons:** `variant="destructive"` → `variant="outline" className={DESTRUCTIVE_OUTLINE}`.
  - **Preview mocks:** add `api_save_provider_key: () => null` to `browser-preview.ts`, plus any other mock still missing for the panes (the Knowledge, Copilot, Practice and price mocks already exist at `:119-217`).
  - Confirm `grep -n "window.confirm" src/components/{SpeakerSettings,TemplateSettings}.tsx` is empty.
- [ ] **BUN** those files, then **GATES**, then check the preview at 1366 and 2048 px in all three themes and run `a11y-check.mjs` on `/settings?tab=recording` and `/settings?tab=models`. Then commit `fix(settings): app confirm dialog and shared dates; preview mocks`.

## Lane D: wide pages (Tasks 40–48)

Spec §2.6 (Stats, Digests, Tasks, Dictate; Practice is Lane E) and §2.2 (What's new on the reading frame). Worktree `/workspace/.worktrees/cl-d`, branched from `cleanup` after Phase 1 merges. Every path below is under `frontend/`.

**Consumes (Phase 1, exact names):** `PageFrame` (`src/components/PageFrame/PageFrame.tsx`), `formatRowDate`, `formatLongDate`, `formatMoney` (`src/lib/dates.ts`), `confirmAction` (`src/lib/confirm.ts`), `DESTRUCTIVE_OUTLINE` (`src/lib/utils.ts`), `rememberOrigin`, `readOrigin` (`src/lib/back-nav.ts`). None of these exist on `main` yet; all are taken from the plan's Shared interfaces. The lane does not depend on Lane C: the one Settings link it adds is the literal `/settings?tab=models`, which Lane C's link-grep test (Task 30) checks once both have merged.

**Commands** (run from `/workspace/.worktrees/cl-d/frontend`):
- **ONE** `<file>`: `flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <file>`
- **FE**: `flock /workspace/.superpowers/cargo.lock nice -n 10 sh -c 'pnpm exec tsc --noEmit && bun test && pnpm run build'`

**Test style:** `bun:test` + `react-test-renderer` with `mock.module('@tauri-apps/api/core')`, as in the existing `tests/components/stats-page.test.tsx:1-80`. Each suite restores its mocks in `afterAll`.

**Shared conventions in this lane:**
- **Outline destructive button:** `<Button variant="outline" size="sm" className={DESTRUCTIVE_OUTLINE}>` (Phase 1); `components/ui/button.tsx` gets no new variant.
- **Day-only strings:** a `YYYY-MM-DD` value from Rust (`usage/stats.rs:75`) is parsed as a local day with `new Date(\`${v}T00:00\`)`, not `new Date(v)`, which would be UTC midnight. Anything that is not a valid date is shown as given.
- **Meeting links** on these pages call `rememberOrigin({ path, label })` in `onClick`, so the meeting page's breadcrumb returns here. The `path` and `label` values are given in each task.

---

### Task 40: Stats on the dashboard frame, cost card with 2-decimal money `[judgment]`
**Files:**
- `src/app/stats/page.tsx:47-77`: the return block; `:49` is today's `mx-auto max-w-3xl` wrapper and `:51` the h1.
- `src/components/Stats/CostSection.tsx:15-53`: drop the "Previous 8 weeks" list at `:38-50`, which Task 41 replaces with a chart.
- `tests/components/stats-page.test.tsx:82-102`

**Interfaces:**
- Consumes: `PageFrame`, `formatMoney`.
- Produces: `CostSection({ periods, period }: { periods: CostPeriod[]; period: Period })`, with the same signature as today; it renders only the current period card. `formatUsd` (`src/lib/stats.ts:55`) is no longer used on Stats; Lane B (Task 21) moves `MeetingCostLine` off it, and Phase 3 (Task 72) deletes it.

- [ ] **Failing tests** (edit `stats-page.test.tsx`):
  - `renders the current cost, provider rows and unpriced count`:
    - expects `'$0.01'` (was `'$0.012'`), `'Uncached input 18.4k'`, `'2 unpriced'`;
    - and `pageText()` does not contain `'$0.012'`.
  - `the period toggle …`: `'$1.27'` (was `'$1.270'`).
  - New, `the page title is one h1 "Stats" and the frame is not centred`:
    - `findAll(n => n.type === 'h1')` gives `['Stats']`;
    - no element's `className` contains `mx-auto` or `max-w-3xl`.
  - New, `the Week/Month toggle sits in the frame actions`: the `role="group"` `aria-label="Period"` element is still found and still switches periods. This guards the move into `actions`.
- [ ] Run **ONE** `tests/components/stats-page.test.tsx` → FAIL (`$0.012` still shown, `mx-auto` present).
- [ ] **Implement:**
  - `page.tsx`: `<PageFrame variant="dashboard" title="Stats" actions={<PeriodToggle …/>}>`. The existing toggle markup at `:52-64` moves into `actions` unchanged.
  - The children are, in order:
    1. the error and loading lines (`:67-68`);
    2. a top grid `div className="grid gap-6 2xl:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]"` holding `<CostSection/>` and `<CostChart/>` (Task 41; until then only `CostSection`);
    3. `<TalkTimeSection/>` below it, full width.
  - `2xl` (1536 px) keeps 1366 single-column, as Global Constraints require.
  - `CostSection`:
    - the card is `rounded-lg border border-border bg-card p-4`;
    - the heading line keeps the text "This week, from 16 Nov" (`:24`);
    - every `formatUsd(...)` at `:25` and `:32` becomes `formatMoney(...)`;
    - the `previous` list and its destructure are removed.
- [ ] Run **ONE** → PASS. Run **FE** → PASS.
- [ ] Commit `feat(stats): dashboard frame and a this-week cost card with 2-decimal amounts`.

### Task 41: `CostChart`, a plain CSS bar chart of the last 8 periods plus this one `[judgment]`
No chart library is installed (`package.json` has none) and none is added: the chart is plain divs.

**Files:**
- create `src/components/Stats/CostChart.tsx`
- `src/app/stats/page.tsx`: mount it in Task 40's top grid.
- `tests/components/stats-page.test.tsx`

**Interfaces:** `export function CostChart({ periods, period }: { periods: CostPeriod[]; period: Period }): JSX.Element | null`. `periods[0]` is the current period and the rest go back in time, as `stats_cost` returns them (`stats/page.tsx:9-10`: 9 weeks or 13 months).

- [ ] **Failing tests** (`describe('cost chart')`, using the existing week fixture `COST_WEEK` with `0.012` this week and `0.5` the week before):
  - `bars run oldest to newest with the current period last`:
    - the chart is the element with `aria-label="Weekly cost"`;
    - it has `li` items in the order `['12 Oct', '19 Oct']` (read with `labelOf`);
    - each item contains its money: `'$0.50'` and `'$0.01'`;
    - the last item has `aria-current="true"`.
  - `bar heights scale to the largest period`: the bar for `0.5` has `style.height === '100%'`, and the bar for `0.012` has `'2.4%'`.
  - `an all-zero range still draws a 1% stub`, with a cost fixture of two zero periods → both `'1%'`.
  - `month view labels bars by month`: after clicking Month, `aria-label="Monthly cost"` and the item text contains `'Oct'`.
- [ ] Run **ONE** → FAIL (no element labelled "Weekly cost").
- [ ] **Implement:**
  - Markup: `<section aria-labelledby="stats-chart">` with an h2 "Weekly cost" or "Monthly cost" and a muted sub-line "Last 8 weeks and this week" or "Last 12 months and this month". Under it, `<ol aria-label="Weekly cost" className="flex h-44 items-end gap-3">`.
  - Each `<li className="flex h-full flex-1 flex-col items-center justify-end text-xs text-muted-foreground">` holds:
    - the amount (`formatMoney`);
    - `<span className="block w-full max-w-14 rounded-t bg-primary" style={{ height: \`${pct}%\` }} />`;
    - the label.
  - Height: `pct = max > 0 ? Math.max(1, round1(v / max * 100)) : 1`, where `round1` rounds to one decimal.
  - Labels: weeks reuse `periodLabel` (`CostSection.tsx:6-12`, exported for this); months use `toLocaleDateString('en-GB', { month: 'short' })`.
  - The current item gets `aria-current="true"` and `text-foreground font-medium`.
  - One hue only: `bg-primary`, no per-bar colours.
- [ ] Run **ONE** → PASS. Run **FE** → PASS.
- [ ] Commit `feat(stats): weekly cost bar chart in the primary colour`.

### Task 42: Talk time as a table with share bars and an explained "Share" header `[judgment]`
**Files:**
- `src/components/Stats/TalkTimeSection.tsx:41-80` (the return block); the meeting link is at `:68-70` and its date at `:71`.
- `tests/components/stats-page.test.tsx:104-113`

**Interfaces:** `TalkTimeSection({ period, talk })`, with the same signature; it consumes `rememberOrigin` and `formatLongDate`.

- [ ] **Failing tests:**
  - `talk time is a table with an explained share column`:
    - a `table` exists, and its `th` texts include `'Category'`, `'Meetings'` and `'Previous week'`;
    - a `th` contains `'Your share of talk time'` and the explainer `'how much of the talking was you'`.
  - `each row shows a share bar sized to the share`: the Standup row text has `'30%'` and `'40%'` (previous), and an element with `style.width === '30%'`.
  - Existing `clicking a talk-time type lists its meetings` still passes. Its button keeps `aria-expanded` and is now the Category cell's button.
  - New assertions in that test:
    - the meeting line shows `formatLongDate(new Date('2026-10-20T00:00'))`, not `'2026-10-20'`;
    - calling the meeting link's `onClick` stores `{ path: '/stats', label: 'Stats' }` (check with `readOrigin()`).
  - `month view says Previous month`.
- [ ] Run **ONE** → FAIL (no `table`).
- [ ] **Implement:**
  - `<table className="w-full text-sm">` with columns:
    - **Category**: a `button` with `aria-expanded` and a lucide `ChevronRight` that rotates when open, so the row looks expandable (audit Stats item 15);
    - **Meetings**: "n meetings";
    - **Your share of talk time**: the explainer sits in a `span className="block text-xs font-normal text-muted-foreground"` inside the `th`. Each cell shows the `sharePct` text plus a track `div className="h-2 flex-1 rounded bg-muted"` with a `bg-primary` fill at `width: \`${total ? Math.round(self / total * 100) : 0}%\``;
    - **Previous week / Previous month**: muted text.
  - When a row is open, the next `<tr><td colSpan={4}>` holds the existing meetings list (`:62-75` logic unchanged). The date is `formatLongDate` of the local day, and the `Link` gets `onClick={() => rememberOrigin({ path: '/stats', label: 'Stats' })}`.
  - The table sits in an `overflow-x-auto` wrapper.
- [ ] Run **ONE** → PASS. Run **FE** → PASS.
- [ ] Commit `feat(stats): talk time table with share bars and a share explainer`.

### Task 43: Digest content as a hook plus a view, with `[Mn]` tags as meeting chips `[judgment]`
**Files:**
- `src/components/Briefs/DigestView.tsx:1-89`:
  - `formatDay` `:24-28`;
  - the fetch `:30-51`;
  - the states `:53-55`;
  - the tag renderer `:57-74`;
  - the article `:76-88`.
- `tests/components/digests-page.test.tsx:116-129`

`DigestView` has one caller, `src/app/digests/page.tsx:72` (checked with Grep), so its signature can change.

**Interfaces** (all names are new):
- `export type DigestState = 'loading' | 'ready' | 'missing' | 'error'`
- `export function useDigest(id: string | null): { state: DigestState; digest: DigestContent | null }`: the fetch moves here unchanged from `:34-51`, including the rule that errors are not logged. `id === null` gives `'missing'` with no invoke.
- `export function DigestView({ digest }: { digest: DigestContent }): JSX.Element`: presentational.
- `export function MeetingChip({ meeting, from }: { meeting: { id: string; title: string }; from: Origin }): JSX.Element`:
  - with an id, a `Link` to `meetingHref({ id })` whose `onClick` is `rememberOrigin(from)`;
  - an empty id (a deleted meeting) gives a non-link `span` with the same look and `text-muted-foreground`.
- `DigestContent` keeps its type (`:9-14`).

- [ ] **Failing tests** (edit `digests-page.test.tsx`):
  - In `?id= opens that digest …`, the inline `[M1]` is now an `a` with `className` containing `rounded` and `border`, and its text is the meeting title `'Ivan sync'`.
  - Unknown `[M9]` still renders as the plain text `'[M9]'`.
  - Script, image and external-link sanitising stay as they are (`:121-127`).
  - `the period shows long dates`: the text contains `formatLongDate(new Date('2026-10-16T09:00:00Z'))` and does not match `/\d+\/\d+\/\d{4}/`.
  - `a chip click remembers Digests as the origin`: after `onClick`, `readOrigin()` equals `{ path: '/digests?id=d1', label: 'Digests' }`.
- [ ] Run **ONE** `tests/components/digests-page.test.tsx` → FAIL.
- [ ] **Implement:**
  - `withTagLinks` and `TAG_HREF` stay.
  - The `a` renderer returns `<MeetingChip meeting={links[n-1]} from={…} />` when the link exists, and the plain label otherwise.
  - Chip classes: `inline-flex max-w-[28ch] truncate rounded border border-primary/35 bg-primary/10 px-2 py-0.5 align-middle text-xs text-foreground hover:bg-primary/20`.
  - `formatDay` becomes `formatLongDate` on the parsed value, using the day-only rule from the conventions; an invalid date is shown raw.
  - The article loses its card box: it is a plain `article className="space-y-4"` inside the reading column.
- [ ] Run **ONE** → PASS (the page wiring is Task 44; this task keeps `page.tsx` compiling by calling `useDigest(selected)` and rendering `DigestView` where it does today).
- [ ] Commit `feat(digests): meeting chips for [Mn] references, long dates, useDigest hook`.

### Task 44: Digests in three panes on the reading frame `[judgment]`
**Files:**
- `src/app/digests/page.tsx`: `formatDate` `:10-13`, the list `:43-70`, the return `:45-75`.
- `tests/components/digests-page.test.tsx`

**Interfaces:** it consumes `PageFrame` with `variant="reading"`, `subnav`, `rail` and `title`, plus `useDigest`, `DigestView` and `MeetingChip` (Task 43) and `formatLongDate`.

- [ ] **Failing tests:**
  - `lists only digests`: unchanged, plus the item's date is `formatLongDate(new Date('2026-10-21T09:00:00Z'))`.
  - `with no ?id= the newest digest opens`: `mount('')` gives `digestGets` of `[{ id: 'd1' }]`.
  - `the rail lists the digest's meetings as chips`:
    - an element `aria-labelledby` the h2 `'Meetings in this digest'` contains one `a` with `href '/meeting-details?id=m-old'` and the text `'Ivan sync'`;
    - the whole page now has two meeting links (the inline chip and the rail chip). Update the old `:120` equality to `['/meeting-details?id=m-old', '/meeting-details?id=m-old']`.
  - `no centred column`: no `className` contains `mx-auto` or `max-w-3xl`.
- [ ] Run **ONE** → FAIL.
- [ ] **Implement:**
  - `<PageFrame variant="reading" title="Digests" subnav={list} rail={digest && <MeetingsRail/>}>`.
  - The list (`subnav`) is today's `<ul>` (`:55-69`) without its card border. Each button shows the title on one line and `formatLongDate(createdAt)` under it, muted. `aria-pressed` stays.
  - `selected` defaults to `urlId ?? shown[0]?.id ?? null`.
  - The main column renders the `useDigest` states (the loading, error and missing texts from `DigestView.tsx:53-55`) or `<DigestView digest/>`.
  - The rail is `section aria-labelledby` an h2 "Meetings in this digest", with a column of `MeetingChip`s for `digest.meetingLinks`.
  - The origin is `{ path: \`/digests?id=${selected}\`, label: 'Digests' }`.
  - The empty state "No digests yet." stays.
- [ ] Run **ONE** → PASS. Run **FE** → PASS.
- [ ] Commit `feat(digests): three-pane reading layout with the digest's meetings in the rail`.

### Task 45: Tasks on the list frame with the date helper `[mechanical]`
No review-queue changes: that is the next feature.

**Files:**
- `src/app/tasks/page.tsx:5-14`
- `src/components/Tasks/TaskList.tsx`:
  - `formatDate` `:21-24`;
  - the group header `:199-208`;
  - the meeting `Link` `:201`;
  - the clock `Link` `:285-291`.
- `tests/components/tasks-page.test.tsx`

**Interfaces:** it consumes `PageFrame` (`variant="list"`, `title="Tasks"`), `formatRowDate` and `rememberOrigin`.

- [ ] **Failing tests** (add to `describe('Tasks page')`):
  - `the group date uses the app date format`: the page text contains `formatRowDate(new Date('2026-10-20T10:00:00Z'))` and does not match `/\d+\/\d+\/\d{4}/`.
  - `the page fills the width`: no `className` contains `mx-auto` or `max-w-3xl`, and there is one h1, `'Tasks'`.
  - `opening the meeting remembers Tasks`: after the group link's `onClick`, `readOrigin()` equals `{ path: '/tasks', label: 'Tasks' }`.
- [ ] Run **ONE** `tests/components/tasks-page.test.tsx` → FAIL.
- [ ] **Implement:**
  - `page.tsx` becomes `<PageFrame variant="list" title="Tasks"><TaskList /></PageFrame>`.
  - In `TaskList`, `formatDate` becomes `(s) => { const d = new Date(s); return Number.isNaN(d.getTime()) ? s : formatRowDate(d); }`.
  - Both meeting links get `onClick={() => rememberOrigin({ path: '/tasks', label: 'Tasks' })}`.
  - `task.due` (`:270`) is free text from the model and stays as it is.
- [ ] Run **ONE** → PASS. Run **FE** → PASS.
- [ ] Commit `feat(tasks): full-width list frame and the app date format`.

### Task 46: Dictate on the form frame; mode delete through `confirmAction` with an outline button `[judgment]`
**Files:**
- `src/app/dictate/page.tsx:82-95`
- `src/components/Dictate/ModeList.tsx:24-25` (`window.confirm`) and `:70` (`variant="destructive"`)
- The h3s that follow the h1 directly, which axe reports as heading-order (audit, Dictate P3): `src/components/Dictate/DictateGeneral.tsx:33`, `ModeList.tsx:37` and `DictionaryEditor.tsx:21`.
- `src/components/Dictate/DictateGeneral.tsx:7,106-114`: the cleanup `LlmOverridePicker` (spec §2.5: per-feature pickers move to Models & keys).
- `tests/components/dictate-page.test.tsx:163-166`

**Interfaces:** it consumes `PageFrame` (`variant="form"`, `title="Dictate"`, no `subnav`) and `confirmAction`. `removeMode` becomes `async`.

- [ ] **Failing tests:**
  - `renders the General, Modes and Dictionary sections`: this now finds `h2` (was `h3`).
  - New, `deleting a mode asks first and saves only on yes`:
    - Mock `../../src/lib/confirm` so that `confirmAction` resolves `false`, then `true`.
    - Use the fixture's custom mode, or add one if the fixture has none.
    - Click "Delete {name}". `confirmAction` was called with `{ title: 'Delete the mode "{name}"?', confirmLabel: 'Delete', destructive: true }`, and no `dictate_save_settings` happened.
    - Click again, after which `dictate_save_settings` was called without that mode.
  - New, `the mode Delete button is outline, not filled red`: the button's `className` contains `text-destructive` and its `variant` prop is `'outline'`.
  - New, `while loading the page already has its title`: before `dictate_get_settings` resolves, there is an h1 `'Dictate'` and the text `'Loading…'`.
  - New, `the cleanup model is set in Models & keys`: no `dictate-cleanup` select; a link with text `Set in Models & keys` and `href '/settings?tab=models'` after the text `Cleanup model:`; the "Used by every mode except As spoken" note stays. Update any existing assertion on the picker.
- [ ] Run **ONE** `tests/components/dictate-page.test.tsx` → FAIL.
- [ ] **Implement:**
  - `page.tsx` returns `<PageFrame variant="form" title="Dictate">`, which holds either `Loading…` or the three sections in a `space-y-6` div. This also replaces the bare loading `div` at `:83`.
  - `removeMode`: `if (!(await confirmAction({ title: \`Delete the mode "${mode.name}"?\`, confirmLabel: 'Delete', destructive: true }))) return;`.
  - The button becomes the outline destructive button from the conventions.
  - The three `h3` become `h2` with the same classes.
  - `DictateGeneral`: delete the `LlmOverridePicker` and its import; in its place `<p className="text-sm">Cleanup model: <Link href="/settings?tab=models" className="underline">Set in Models & keys</Link></p>`. The page keeps saving the whole `DictateSettings` from its own fresh load, so `cleanup_provider`/`cleanup_model` pass through unchanged.
- [ ] Run **ONE** → PASS. Run `grep -rn "window.confirm\|variant=\"destructive\"" src/app/{stats,digests,tasks,dictate} src/components/{Stats,Briefs,Tasks,Dictate}` → no output. Run **FE** → PASS.
- [ ] Commit `feat(dictate): form frame, app confirm, an outline Delete for modes; cleanup model set in Models & keys`.

### Task 47: What's new on the reading frame `[mechanical]`
**Files:**
- `src/components/WhatsNew/WhatsNew.tsx:45-46` (the `mx-auto max-w-2xl p-6` wrapper and its h1)
- `tests/components/whats-new.test.tsx`

**Interfaces:** consumes `PageFrame` (`variant="reading"`, `title="What's new"`, no rail).

- [ ] **Failing test** `the page is left-aligned with one h1`: one h1 `"What's new"`, and no `className` contains `mx-auto` or `max-w-2xl`. The existing release-list and footer tests stay.
- [ ] Run **ONE** `tests/components/whats-new.test.tsx` → FAIL.
- [ ] **Implement:** `return <PageFrame variant="reading" title="What's new">…</PageFrame>`, with the version line, releases and footer as children unchanged.
- [ ] Run **ONE** → PASS. Commit `feat(whats-new): reading frame`.

### Task 48: Lane D browser-preview mocks `[mechanical]`
**Files:**
- `src/lib/browser-preview.ts`: `briefs_list` `:145-148`, `digest_get` `:153-157`, `stats_talk_time_meetings` `:188`.
- `tests/lib/browser-preview.test.ts`, only if it asserts these values.

- [ ] **Change:**
  - `briefs_list` gets a second digest (`digest-2`, "Week of 2 November", `2026-11-09T07:00:00Z`) so the list pane shows more than one row.
  - `digest_get` gets a third `meetingLinks` entry with `id: ''` ("Deleted meeting"), so the non-link chip is visible.
  - `stats_talk_time_meetings` `date` becomes `'2026-11-10'`, the shape Rust sends (`usage/stats.rs:75`), not an ISO time.
- [ ] Run **ONE** `tests/lib/browser-preview.test.ts` and **FE** → PASS.
- [ ] Check in `pnpm run dev:preview` at 1366, 1920 and 2560 px in Light, Dark and Conclave on `/stats`, `/digests`, `/tasks`, `/dictate` and `/whats-new`:
  - 1366 is single-column;
  - the chart bars are visible in all three themes;
  - the rail sits beside the text only at ≥1280 px content width.
- [ ] Run `node scripts/a11y-check.mjs http://localhost:3118 /stats /digests /tasks /dictate /whats-new` → no serious or critical violations.
- [ ] Commit `chore(preview): lane D mocks for digests and stats`.

## Lane E: Practice (Tasks 50–59)

Spec §2.6 (Practice), §2.7 (Practice Delete contrast) and §2.8. Review Focus 4 is owned here (Task 50 test `old_session_follows_its_setups_current_language`, Task 52 wiring). Tasks 50–56 do not need Lane C; rebase onto `cleanup` after Lane C merges and before Task 57, which imports `settingsHref`.

**Pinned for every task in this lane**
- Worktree: `/workspace/.worktrees/cl-e` (branched from `cleanup` after Phase 1).
- RUST gate (one module): `. "$HOME/.cargo/env"; export CARGO_TARGET_DIR=/workspace/target; cd /workspace/.worktrees/cl-e/frontend/src-tauri && flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib -j 4 <filter>`. It needs `frontend/out` and the stub sidecar from AGENTS.md (`mkdir -p frontend/src-tauri/binaries && touch frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu`).
- BUN gate (one file): `cd /workspace/.worktrees/cl-e/frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <file>`.
- Full lane gates before merge: the Global Constraints' frontend and Rust gates.
- Language values: Rust `PracticeLanguage::{En, De}` serialised as `"en"` / `"de"`; TS `PracticeLanguage = 'en' | 'de'`. Display names "English" / "German".
- A session's language: its own `practice_sessions.language`, or, when that is NULL (sessions from v2.0.0), its setup's **current** `language`, or English when the setup is gone. Never backfilled by the migration.
- Copy (exact):
  - Pronunciation note for a German report: `Pronunciation hints are English-only, so this German session has none`
  - Retry in flight: `Starting the report…`
  - Start failure (from the command): `Could not start the report: <error>`

---

### Task 50: Setup language column, `PracticeLanguage`, session language [mechanical] [risky]

Migration plus storage. Persisted data: existing setups must read as English and existing sessions must keep working.

**Files:**
- Create: `frontend/src-tauri/migrations/20261118000050_practice_language.sql`
- Modify: `frontend/src-tauri/src/practice/mod.rs` (new enum after the `pub mod` list, `:34-52`)
- Modify: `frontend/src-tauri/src/practice/db.rs`: `SetupRow` `:15-27`, `SetupInput` `:51-58`, `SessionRow` `:60-76`, `save_setup` `:174-235`, `create_session` `:364-405`, new `session_language` after `get_session` `:595-615`, tests `:646+` (`input()` `:662-670`)
- Modify: `frontend/src-tauri/src/practice/commands.rs`: `SetupSummary` `:64-74`, `summarize` `:255-268`, test literals `session_row` `:860`, `setup` `:945`, `finished_session` `:1012-1018`

**Interfaces:**
- Produces: `PracticeLanguage` (Rust, `crate::practice`), `SetupRow.language: String`, `SetupInput.language: PracticeLanguage`, `SessionRow.language: Option<String>`, `SetupSummary.language: PracticeLanguage`, `db::session_language`.

- [ ] Write the migration (never edit `20261020000000_practice.sql`):
  ```sql
  ALTER TABLE practice_setups ADD COLUMN language TEXT NOT NULL DEFAULT 'en' CHECK (language IN ('en','de'));
  -- NULL: recorded before v2.1.0; the session then follows its setup's current language.
  ALTER TABLE practice_sessions ADD COLUMN language TEXT CHECK (language IN ('en','de'));
  ```
- [ ] Failing tests in `db.rs` tests:
  - `seeds_are_three_editable_setups` (extend): each seed has `setup.language == "en"`.
  - `setup_language_is_saved_and_updated`: `save_setup` with `language: PracticeLanguage::De` → row `language == "de"`; saving the same id with `En` → `"en"`. `sqlx::query("UPDATE practice_setups SET language = 'fr'")` → `is_err()` (CHECK).
  - `setup_input_language_defaults_and_rejects_unknown`: `serde_json::from_str::<SetupInput>` of a body without `language` → `PracticeLanguage::En`; with `"language":"de"` → `De`; with `"language":"fr"` → `is_err()`.
  - `session_stores_its_setups_language`: German setup → `create_session` → stored `language == Some("de")`; switching the setup back to English leaves `session_language` at `De`.
  - `old_session_follows_its_setups_current_language` (Review Focus 4): English setup, session, then `UPDATE practice_sessions SET language = NULL` (a v2.0.0 row); save the setup as German → `session_language(&pool, &row) == De`; delete the setup → `En`.
- [ ] Run RUST `practice::db` → FAIL (compile error: no `language` field).
- [ ] Implement:
  - `mod.rs`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "lowercase")] pub enum PracticeLanguage { #[default] En, De }` with `pub fn code(self) -> &'static str` (`"en"`/`"de"`), `pub fn name(self) -> &'static str` (`"English"`/`"German"`), `pub fn from_code(code: &str) -> Self` (`"de"` → `De`, anything else → `En`; one-line comment: the column CHECK keeps stored values to en/de).
  - `SetupInput`: `#[serde(default)] pub language: PracticeLanguage` (the IPC boundary rejects any other value through serde).
  - `save_setup`: bind `input.language.code()` in both the UPDATE (`..., voice = ?, language = ?, updated_at = ?`) and the INSERT column list.
  - `create_session`: `language: Some(setup.language.clone())` in the row and the INSERT.
  - `pub async fn session_language(pool: &SqlitePool, session: &SessionRow) -> Result<PracticeLanguage, String>`: stored value via `from_code`; else `get_setup(session.setup_id)` → its `language`; else `En`.
  - `SetupSummary.language: PracticeLanguage` = `PracticeLanguage::from_code(&row.language)` in `summarize`.
  - Test literals: `language: "en".into()` / `None` / `Default::default()`.
- [ ] Run RUST `practice::` → PASS (whole module, since `commands.rs` literals changed).
- [ ] Commit: `feat(practice): a language per setup (migration, storage, session language)`

### Task 51: Live call in the setup's language (Whisper, character, TTS) [judgment]

**Files:**
- Modify: `frontend/src-tauri/src/practice/prompts.rs`: `Persona` `:1-5`, `character_system` `:23-36`, tests `:212-255`
- Modify: `frontend/src-tauri/src/practice/runner.rs`: `Runner` fields `:240-278`, `Runner::new` `:281-330`, capture spawn `:424-427`, `synthesize` `:702-724`, `transcribe_segments` `:808-831` (`"en"` at `:818`), `capture_turn` `:836-876`
- Modify: `frontend/src-tauri/src/practice/tts.rs`: `Supertonic` struct `:35`, `load` `:154-178`, `text_ids` `:253-256`, ignored model tests `:400-440`
- Modify: `frontend/src-tauri/src/practice/commands.rs`: `load_session` TTS load `:524-530`

**Interfaces:** Consumes `PracticeLanguage`, `SetupRow.language` (Task 50).

- [ ] Failing tests:
  - `prompts.rs` `character_system_names_the_language`: a `Persona { language: PracticeLanguage::De, .. }` → system contains `"Speak German only."`; with `En` → contains `"Speak English only."`. Existing Persona literals gain `language: PracticeLanguage::En`.
  - `tts.rs` (add to the existing `#[ignore]` model tests, needs `PRACTICE_MODELS_DIR`) `synthesizes_german`: `Supertonic::load(&dir, false)`, `language = De`, `synthesize("Guten Morgen, schön Sie zu sehen.", ..)` returns finite, non-empty audio.
- [ ] Run RUST `practice::prompts` → FAIL (compile error).
- [ ] Implement:
  - `Persona.language: PracticeLanguage`; `character_system` adds `"Speak {name} only.\n\n"` after the Situation line (decided: a German character must not answer in English).
  - `Runner` gets `language: PracticeLanguage` (`from_code(&l.setup.language)`), passed to `character_system` and into the capture closure: `capture_turn(rx, rate, stop, engine, language)` → `transcribe_segments(engine, segments, texts, speech_secs, language)` → `Some(language.code().to_string())`.
  - TTS, builder's check first: confirm Supertonic 3 at the pinned revision accepts the `<de>` tag (the model card / reference SDK's language list; the 2026-10-20 spec §5.2 says "It also speaks German"). If it does: `pub language: PracticeLanguage` on `Supertonic` (`En` in `load`), `text_ids` uses `self.language.code()`, `load_session` sets it from the setup, and the CPU fallback in `runner.rs` `synthesize` copies the old engine's `language` onto the reloaded one. If it does not: leave `tts.rs` unchanged and say so in the commit body. `practice_speak_word` stays English (it only voices English pronunciation hints).
- [ ] Run RUST `practice::` → PASS. Run the ignored test only if `PRACTICE_MODELS_DIR` is available; otherwise it goes on the PC checklist.
- [ ] Commit: `feat(practice): the live call transcribes and speaks the setup's language`

### Task 52: Report in the session's language; pronunciation English-only [judgment]

**Files:**
- Modify: `frontend/src-tauri/src/practice/prompts.rs`: `REPORT_SYSTEM` `:51-62`, `report_prompts` `:64-~80`, tests `:262-289`
- Modify: `frontend/src-tauri/src/practice/report_job.rs`: `VERBATIM_PROMPT` `:41`, consts `:48-52`, `analyse` `:187-232`, `word_for_word` `:238-296` (`"en"` at `:271`), `with_pronunciation` `:387-399`, tests `:520+`

**Interfaces:** Consumes `db::session_language` (Task 50). Produces `report_prompts(turns, recent_patterns, language)`, `verbatim_prompt(language) -> &'static str`, `finish_pronunciation(report, language, pron) -> Report`.

- [ ] Failing tests:
  - `prompts.rs` `report_prompt_names_the_language`: `report_prompts(&turns, &[], PracticeLanguage::De).0` contains `"spoken German practice session"` and `"Write each note in English"` and `"quote and better stay in German"`; the `En` prompt contains `"spoken English practice session"`. Existing two report tests pass `PracticeLanguage::En` and keep their assertions.
  - `report_job.rs` `german_report_skips_pronunciation_with_a_note`: `finish_pronunciation(report, De, Ok(vec![]))` → `pronunciation.is_empty()` and `pronunciation_note == Some(PRON_ENGLISH_ONLY)`; `finish_pronunciation(report, En, Err(..))` → note `PRON_UNAVAILABLE` (same as today).
  - `report_job.rs` `verbatim_prompt_follows_the_language`: `verbatim_prompt(En) == VERBATIM_PROMPT`; `verbatim_prompt(De)` contains `"äh"`.
- [ ] Run RUST `practice::prompts` and `practice::report_job` → FAIL.
- [ ] Implement:
  - `REPORT_SYSTEM` becomes a template; `report_prompts(turns, recent_patterns, language: PracticeLanguage)` formats `"You review a spoken {name} practice session."`, reviews grammar, word choice and phrasing in that language, and adds: `"Write each note in English; quote and better stay in {name}."` The JSON shape, the `pattern` rules and the turn format are unchanged.
  - `pub fn verbatim_prompt(language) -> &'static str`: `En` → `VERBATIM_PROMPT`; `De` → a German filler line, e.g. `"Ähm, also, ich- ich hab, äh, gedacht, dass wir, na ja, das schon hatten."`.
  - `analyse`: after `get_session`, `let language = db::session_language(&pool, &session).await?;`. Pass it to `word_for_word` (Whisper `Some(language.code().to_string())`, `Some(verbatim_prompt(language))`) and to `report_prompts`. For `De`, skip `pronunciation(..)` entirely (`Ok(vec![])`, so no pron model download).
  - `pub fn finish_pronunciation(report: Report, language: PracticeLanguage, pron: Result<Vec<PronHint>, String>) -> Report`: `De` → `Report { pronunciation: vec![], pronunciation_note: Some(PRON_ENGLISH_ONLY.into()), ..report }`; `En` → `with_pronunciation(report, pron)`. Both return sites in `analyse` (`:212`, `:231`) call it.
  - `const PRON_ENGLISH_ONLY: &str = "Pronunciation hints are English-only, so this German session has none";`
- [ ] Run RUST `practice::` → PASS.
- [ ] Commit: `feat(practice): German reports transcribe and review in German; pronunciation stays English-only`

### Task 53: Retry claims before it returns, so a start failure reaches the UI [judgment] [risky]

Concurrency: the claim stays under `REPORT_GATE` (the delete takes the same gate). Today `practice_retry_report` spawns `run`, whose claim error (`report_job.rs:86-96`) is only logged, so the UI keeps the old error.

**Files:**
- Modify: `frontend/src-tauri/src/practice/report_job.rs`: `run` `:77-105`, `claim_for_report` `:108-111`
- Modify: `frontend/src-tauri/src/practice/commands.rs`: `practice_retry_report` `:784-803`, consts `:47-48`, tests (use `session_pool` `:1000` and `finished_session` `:1011`)

**Interfaces:** Produces `report_job::run_claimed(app, session_id, hold, busy)`, `commands::claim_retry(pool, session_id) -> Result<(), String>`.

- [ ] Failing tests in `commands.rs`:
  - `retry_claim_clears_the_old_error`: a finished session set to `report_status='failed', report_error='Claude did not answer'` → `claim_retry` is `Ok(())`; the row reads `report_status == "waiting"` and `report_error == None`.
  - `retry_claim_refuses_a_ready_report`: `report_status='ready'` → `Err(NOT_RETRYABLE)`.
  - `retry_claim_failure_reaches_the_caller`: `pool.close().await` then `claim_retry` → `Err(e)` with `e.starts_with("Could not start the report: ")`.
- [ ] Run RUST `practice::commands` → FAIL.
- [ ] Implement:
  - `pub(crate) async fn claim_retry(pool: &SqlitePool, session_id: &str) -> Result<(), String>`: `report_job::claim_for_report` → `Ok(true)` → `Ok(())`; `Ok(false)` → `Err(NOT_RETRYABLE)`; `Err(e)` → `Err(format!("Could not start the report: {e}"))`.
  - `practice_retry_report`: keeps its running and status checks, then `claim_retry(..).await?`, then takes `busy` and spawns `report_job::run_claimed(app, session_id, hold_model(), busy)`.
  - `report_job::run` keeps the claim (the session-end path, `runner.rs:652`) and then calls `run_claimed`; `run_claimed` is the old body from `publish(&app, &session_id, "waiting")` on. The claim-error branch in `run` keeps its `warn!` and releases `hold` as today.
- [ ] Run RUST `practice::` → PASS.
- [ ] Commit: `fix(practice): Retry claims the report before returning, so a start failure is shown`
- Reviewer: look hard for silent failures and for a hold or busy guard leaked on the early-return paths.

### Task 54: Language in the TS types, the Setup editor and the setup list [mechanical]

**Files:**
- Modify: `frontend/src/lib/practice.ts`: `SetupSummary` `:15-25`, `SetupInput` `:27-33`
- Modify: `frontend/src/components/Practice/SetupEditor.tsx`: state `:28-38`, `save` `:43-60`, Voice row `:226-243`
- Modify: `frontend/src/components/Practice/SetupList.tsx`: name line `:21-26`
- Test: `frontend/tests/components/practice-page.test.tsx` (fixtures `THESIS` etc. gain `language: 'en'`)

**Interfaces:** Produces TS `export type PracticeLanguage = 'en' | 'de'` and `export const PRACTICE_LANGUAGES: { id: PracticeLanguage; label: string }[]` (`English`, `German`) in `lib/practice.ts` (not `LANGUAGES`, which `constants/languages.ts` already exports).

- [ ] Failing tests:
  - `setup_editor_saves_the_language`: open Edit on a setup, pick "German" in the `aria-label="Language"` select, Save → `practice_save_setup` args `input.language === 'de'`. A new setup saves `'en'` by default.
  - `german_setup_shows_a_language_chip`: a setup with `language: 'de'` shows a "German" chip in the list; an English one shows none.
- [ ] Run BUN `tests/components/practice-page.test.tsx` → FAIL.
- [ ] Implement: `language` state from `setup?.language ?? 'en'`, sent in `input`; a `Labeled label="Language"` Select (same pattern as Voice) next to Voice in the same grid; the chip uses the existing `bg-muted text-muted-foreground` chip classes.
- [ ] Run BUN → PASS. Run `pnpm exec tsc --noEmit` (same flock).
- [ ] Commit: `feat(practice): Language field in the setup editor`

### Task 55: Retry clears the old error and shows the start failure [judgment]

**Files:**
- Modify: `frontend/src/components/Practice/ReportView.tsx`: listener `:72-90`, `retry` `:92-108`, status blocks `:308-333`
- Test: `frontend/tests/components/practice-page.test.tsx` (`retry_is_disabled_while_the_call_is_in_flight` `:766-782` is rewritten)

**Interfaces:** Consumes the Task 53 command behaviour (rejects with `Could not start the report: …`).

- [ ] Failing tests:
  - `retry_clears_the_old_error_at_once`: failed detail with `report_error: 'Claude did not answer'`, `retryImpl` held open; click Retry → `hasDirectText('Claude did not answer') === false`, `hasDirectText('Starting the report…') === true`, no Retry button; a second click is impossible, so `calls('practice_retry_report').length === 1`.
  - `a_start_failure_replaces_the_old_error`: `retryImpl` rejects `'Could not start the report: database is locked'` → that text shows in the failed block, the old error does not, and Retry is offered again.
  - `a_report_event_clears_the_start_failure`: after the rejection above, `emit('practice-report', { session_id: 's1', status: 'running' })` with detail `report_status: 'running'` → `Writing your report…`, no start-failure text.
- [ ] Run BUN → FAIL.
- [ ] Implement: `startError: string | null` state; `retry` sets `startError(null)` and `busy` before the invoke, sets `startError(String(err))` on rejection (replacing the toast), then `load()`. While `busy`, the status area renders `Starting the report…` instead of the failed or pending block. The failed and "not generated yet" blocks show `startError || detail.report_error || 'The report could not be written'` (failed) or `startError` above Generate (pending). The `practice-report` listener clears `startError`.
- [ ] Run BUN → PASS.
- [ ] Commit: `fix(practice): Retry clears the old error and shows a start failure`

### Task 56: The report becomes its own route [judgment]

**Files:**
- Create: `frontend/src/app/practice/report/page.tsx`
- Modify: `frontend/src/components/Practice/ReportView.tsx`: props `:55`, loading `:173-183`, header `:185-210`
- Modify: `frontend/src/app/practice/page.tsx`: `View` `:18-22`, `onEnded` `:178`, report branch `:181`, `onOpen` `:221`
- Test: `frontend/tests/components/practice-page.test.tsx` (mock `next/navigation` and `../../src/lib/back-nav`; `openReport()` `:223-226` mounts the report page with `?id=s1`)

**Interfaces:** Consumes Phase 1 `PageFrame`, `BackCrumb`, `rememberOrigin`, `formatLongDate`. Produces the route `/practice/report?id=<sessionId>`; `ReportView({ sessionId }: { sessionId: string })` (no `onBack`).

- [ ] Failing tests:
  - `open_report_navigates_to_the_report_route`: on the Practice home, click "Open report" → `rememberOrigin` called with `{ path: '/practice', label: 'Practice' }` and `router.push('/practice/report?id=s1')`.
  - `ended_call_opens_the_report_route`: the call's end → `router.push('/practice/report?id=session-1')`.
  - `report_route_reads_the_id_and_shows_the_breadcrumb`: render `app/practice/report/page.tsx` with `useSearchParams` → `id=s1` → `practice_get_session` called with `{ id: 's1' }`, a `BackCrumb` with `fallback="/practice"` and `fallbackLabel="Practice"` is rendered, and the h1 reads `Thesis defence · <formatLongDate(started_at)> · 18 min` (existing `short_session_shows_less_than_a_minute` still ends with `· <1 min`).
  - `report_route_without_an_id_says_so`: no `id` → `No session selected`, no invoke.
- [ ] Run BUN → FAIL.
- [ ] Implement:
  - `app/practice/report/page.tsx`: `'use client'`; inner component reads `useSearchParams().get('id')` and renders `<ReportView key={id} sessionId={id} />`, wrapped in `<Suspense fallback={null}>` (the `meeting-details/page.tsx` pattern for the static export).
  - `ReportView` renders `<PageFrame variant="reading" breadcrumb={<BackCrumb fallback="/practice" fallbackLabel="Practice" />} title={…}>` for both the loading and loaded states; the two `← Practice` buttons and the `max-w-4xl mx-auto` wrappers go. The date uses `formatLongDate`. No rail.
  - `page.tsx`: `View` loses `report`; `useRouter()`; a helper `openReport(id)` = `rememberOrigin({ path: '/practice', label: 'Practice' })` then `router.push(`/practice/report?id=${id}`)`, used by `SessionList.onOpen` and `CallView.onEnded`.
- [ ] Run BUN → PASS; `pnpm run build` (flock) confirms the static export builds the new route.
- [ ] Commit: `feat(practice): the report is its own route with a breadcrumb back to Practice`

### Task 57: Practice home on the dashboard frame; Models link; outline Delete with confirm [mechanical]

**Files:**
- Modify: `frontend/src/app/practice/page.tsx`: `deleteSession` `:162-170`, editor wrapper `:186-200`, home `:205-226`, `ModelsCard` import `:12`
- Modify: `frontend/src/components/Practice/SessionList.tsx` (`sessionLine` `:4-10`, Delete `:29`)
- Modify: `frontend/src/components/Practice/SetupEditor.tsx`: `remove` `:65-70`, `removeMaterial` `:126-130`, Delete setup `:253`
- Test: `frontend/tests/components/practice-page.test.tsx` (replace the `window.confirm` mock `:143`, `:269-271`, `:411-417` with a `mock.module('../../src/lib/confirm', …)` `confirmAction` mock)

**Interfaces:** Consumes Phase 1 `PageFrame`, `confirmAction`, `formatRowDate`, `DESTRUCTIVE_OUTLINE`; Lane C `settingsHref` from `@/lib/settings-panes` (rebase onto `cleanup` after Lane C merges, before this task).

- [ ] Failing tests:
  - `home_links_to_models_and_keys`: no "Models" card heading; the text `Models:` followed by a link with text `Set in Models & keys` and `href === '/settings?tab=models'`.
  - `delete_session_asks_with_confirm_action`: Delete → `confirmAction` called with `{ title: 'Delete this session?', body: 'Thesis defence and its report go to the Recycle Bin.', confirmLabel: 'Delete', destructive: true }`; resolving `false` → no `practice_delete_session`; `true` → invoked with `{ id: 's1' }`.
  - `delete_setup_and_material_use_confirm_action`: same pattern for "Delete setup" and the material ✕.
  - `delete_buttons_are_outline`: every Delete / Delete setup button has `variant === 'outline'` and a `text-destructive` class.
  - `session_line_uses_the_row_date`: the row shows `formatRowDate(start, end)` (end = start + `duration_secs`) followed by the summary; no `toLocaleDateString` text such as "Nov 15".
- [ ] Run BUN → FAIL.
- [ ] Implement:
  - Home: `<PageFrame variant="dashboard" title="Practice">`, the blocked-reason note and the two cards in the existing grid; `<ModelsCard />` and its import (`:13`) removed from the page and replaced by a muted line with a `Link` to `settingsHref('models')`. `components/Practice/ModelsCard.tsx` and `tests/components/practice-models-card.test.tsx` stay unchanged: Lane C renders the card in the Practice row of Models & keys. Update `practice-page.test.tsx` where it expected the card.
  - Editor: `<PageFrame variant="form">` instead of `max-w-3xl mx-auto p-8`.
  - Delete buttons: `variant="outline" className={DESTRUCTIVE_OUTLINE}` (Phase 1, tokens only). Check contrast with `a11y-check.mjs` on `/practice` in Light, Dark and Conclave; if a theme fails on the `--destructive` token itself, note it in the task report: Phase 3 Task 71 fixes the token, never raw colours here.
  - All three `window.confirm` calls → `await confirmAction({...})`.
- [ ] Run BUN → PASS; tsc.
- [ ] Commit: `feat(practice): dashboard frame, Models link, outline Delete with confirm`

### Task 58: Browser-preview mocks for the lane [mechanical]

**Files:**
- Modify: `frontend/src/lib/browser-preview.ts`: `practiceSession` `:102`, `practice_*` mocks `:194-199`

- [ ] Add `language: 'en'` to the setup mock, add a second, German setup (`language: 'de'`), and a `practice_get_session` mock for `ps-1` (a ready report, plus a German one whose `pronunciation_note` is the English-only note), so `/practice` and `/practice/report?id=ps-1` render in `pnpm run dev:preview`.
- [ ] Check: `pnpm exec tsc --noEmit`; open both routes at 1366, 1920, 2048 and 2560 px in the three themes; `node frontend/scripts/a11y-check.mjs http://localhost:3118 /practice /practice/report?id=ps-1` reports no serious or critical violations.
- [ ] Commit: `chore(preview): Practice language and report route mocks`

### Task 59: Fix the report panic from the user's log [judgment] BLOCKED

Blocked until the user sends the app log line `panic in thread '…' at <file>:<line>: …` (written by the hook at `frontend/src-tauri/src/lib.rs:575-580`) for a report that ended in "The report stopped unexpectedly — Retry".

- [ ] Read the log line: the file and line give the panicking code.
- [ ] Write a failing regression test next to that code that reproduces the panic with the same input shape, then fix the root cause and run it to PASS.

## Lane F: onboarding and the meeting-detected pop-up (Tasks 60–64, spec §2.7)

Worktree `/workspace/.worktrees/cl-f`, branched from `cleanup` after Phase 1 merges. The lane owns `contexts/OnboardingContext.tsx` and `frontend/src-tauri/src/banner.rs` (Task 61: the `open` branch and one test) besides the onboarding components and `app/banner/**`. Tasks 60–62 do not need Lane C; Task 63 does. Paths below are relative to `frontend/` unless they start with `frontend/src-tauri`. Line numbers were verified on `cleanup` at b15df390; re-grep after the rebase.

Gate commands used below (run from `/workspace/.worktrees/cl-f`):
- **BUN(file)**: `cd frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test <file>`
- **FE**: `cd frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 pnpm exec tsc --noEmit && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test && flock /workspace/.superpowers/cargo.lock nice -n 10 pnpm run build`
- **RUST(filter)**: `export CARGO_TARGET_DIR=/workspace/target; mkdir -p frontend/src-tauri/binaries && touch frontend/src-tauri/binaries/llama-helper-x86_64-unknown-linux-gnu; flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test -p meetily --lib <filter> -j 4` (needs `frontend/out` from the FE build).

Facts verified:
- `app/banner/page.tsx` (102 lines) renders one `flex` row. The message `<span className="flex-1 truncate …">` (line 74) shares the row with Record new, `Continue "<title>"` (`min-w-0 truncate`, lines 62–69) and a text "Dismiss" button. With a continue offer in a 460×72 window (`banner.rs:13-14`), the message shrinks to "Z.". The ended kind (text at line 56) renders only an X.
- `meeting_banner_action` (`frontend/src-tauri/src/banner.rs:143-188`) focuses the main window on `open` only for `Mention` (line 151). After an auto-stop, `useRecordingStop.ts:396/406` already pushes the main window to the saved meeting, so "Open" for the ended state only needs to focus the main window.
- Onboarding: `WelcomeStep.tsx:10-28` says "Your data never leaves your device", "Works offline, no cloud required", "All on your device". `SetupOverviewStep.tsx:95-115` promises "Download Summarization Engine", which does not exist (`complete_onboarding` saves Claude `claude-haiku-5-5` as the summary default, `onboarding.rs:172-192`; no local summary model is downloaded, `OnboardingContext.tsx:333`).
- **Ordering trap:** `SettingsRepository::save_api_key` (`frontend/src-tauri/src/database/repositories/setting.rs:101-108`) inserts the settings row as `provider='openai', model='gpt-4o-2024-11-20'` when none exists. `save_onboarding_defaults` (`onboarding.rs:182`) writes the Claude default only when no row exists. A key saved before `complete_onboarding` on a fresh install would therefore leave summaries on OpenAI with no OpenAI key. The key step must save **after** `complete_onboarding`.
- On Windows the flow is Welcome (1) → overview (2) → downloads (3, completes, `DownloadProgressStep.tsx:212-224`); macOS adds Permissions (4). Step numbers are persisted (`save_onboarding_status_cmd`) and clamped to 4. Replacing step 2 keeps the count, so no persisted-state change.

### Task 60: Banner keeps the message whole when there is a continue candidate [mechanical]

**Files:** modify `app/banner/page.tsx:54-101`; test `tests/components/banner-continue.test.tsx` (add cases).
**Interfaces:** Consumes `get_meeting_banner`, `meeting_banner_action` (unchanged).

- [ ] Failing tests in `banner-continue.test.tsx`:
  - `detected with an offer keeps the message on its own row`: view `{ kind:'detected', app:'Zoom', certain:true, line:null, continue_title:'Weekly sync with the platform team' }`. Find the element whose text is `Zoom meeting detected`. Expect its `className` not to contain `truncate`. Expect its parent not to contain the `Continue "…"` button (the message and the actions are on different rows). Expect the Continue button's `title` prop to be `Weekly sync with the platform team`.
  - `continue kind keeps the message on its own row`: same assertions for `Zoom: still the same call?`.
  - The existing cases stay green: labels `Record new` and `Continue "Standup"` are present, `Record` is absent, and Continue invokes `{ action: 'continue' }`.
- [ ] Run BUN(`tests/components/banner-continue.test.tsx`). Expected: FAIL (the message has `truncate` and shares the row).
- [ ] Implement. When `view.continue_title` is set (kinds `detected` and `continue`), render two rows inside the same card (`flex h-screen w-screen flex-col justify-center gap-1.5 px-4 py-2`):
  - Row 1: the Mic icon, the message `<span className="min-w-0 flex-1 text-sm font-medium leading-tight">` (no `truncate`; the text is at most ~40 chars), and the X dismiss (`aria-label="Dismiss"`).
  - Row 2, right-aligned `gap-2`: Record new (detected only, outline) and Continue (`max-w-[17rem] truncate`, `title={view.continue_title}`), both `h-7 px-3 py-1`.
  - Without a continue title, keep today's one-row layout. The window stays 460×72 (row heights 20 + 6 + 28 + 16 padding).
- [ ] Run BUN(`tests/components/banner-continue.test.tsx`) and BUN(`tests/components/banner-mention.test.tsx`). Expected: PASS.
- [ ] Commit `fix(banner): keep the message whole when a continue offer is shown`.

### Task 61: Ended pop-up gets an Open button [mechanical]

**Files:** modify `frontend/src-tauri/src/banner.rs:143-155` and its tests module (from line 190); modify `app/banner/page.tsx` (ended branch); test `tests/components/banner-continue.test.tsx`.
**Interfaces:** Produces `fn focuses_main(kind: Option<&BannerKind>) -> bool` (private to `banner.rs`).

- [ ] Failing Rust test in `banner.rs` tests: `open_focuses_main_for_mention_and_ended`. It asserts `focuses_main(Some(&BannerKind::Ended { call: CallApp::Zoom }))` is true, `Mention { line: "x".into() }` is true, `Detected { call: CallApp::Zoom, certain: true, offer: None }` is false, and `None` is false.
- [ ] Failing Bun test: `ended banner shows Open`. View `{ kind:'ended', app:'Zoom', certain:true, line:null, continue_title:null }`. Labels contain `Open`. Clicking it invokes `meeting_banner_action` with `{ action: 'open' }`.
- [ ] Run RUST(`banner::`). Expected: FAIL (no `focuses_main`). Run BUN(`tests/components/banner-continue.test.tsx`). Expected: FAIL.
- [ ] Implement:
  - `fn focuses_main(kind: Option<&BannerKind>) -> bool { matches!(kind, Some(BannerKind::Mention { .. } | BannerKind::Ended { .. })) }`.
  - Line 151 becomes `if action == "open" && focuses_main(kind.as_ref())`. `focus_main_window(&app)` runs for both, and the `copilot-open` emit stays only for `Mention`. Keep the focus before `window.close()` (comment at line 150).
  - In `page.tsx`, the ended kind renders a primary `Open` button (`act('open')`) before the X.
- [ ] Run RUST(`banner::`) and BUN(`tests/components/banner-continue.test.tsx`). Expected: PASS.
- [ ] Commit `feat(banner): Open button on the meeting-ended pop-up`.

### Task 62: Welcome copy tells the truth [mechanical]

**Files:** modify `components/onboarding/steps/WelcomeStep.tsx:10-28`; create `tests/components/onboarding-welcome.test.tsx`.
**Interfaces:** none new.

- [ ] Failing test `onboarding-welcome.test.tsx`. Mock `../../src/contexts/OnboardingContext` so that `useOnboarding` returns `{ goNext, goPrevious, goToStep }` (restore the original in `afterAll`, as `banner-continue.test.tsx:4-7` does), then render `WelcomeStep`.
  - `says transcription is local and summaries use the user's key`: the text contains `Transcription runs on this PC` and `Summaries use a cloud AI with your own API key`.
  - `makes no offline or no-cloud promise`: the text does not match `/never leaves|no cloud|All on your device|offline/i`.
- [ ] Run BUN(`tests/components/onboarding-welcome.test.tsx`). Expected: FAIL.
- [ ] Implement. The features become:
  - `Cpu` "Transcription runs on this PC with Whisper"
  - `Sparkles` "Summaries use a cloud AI with your own API key"
  - `Lock` "Audio stays on this PC; only the transcript is sent for a summary"

  The description becomes "Record. Transcribe on this PC. Summarize with your own AI key." Drop "Takes less than 3 minutes", because the download is about 1.5 GB.
- [ ] Run BUN(`tests/components/onboarding-welcome.test.tsx`). Expected: PASS.
- [ ] Commit `fix(onboarding): welcome copy says what runs locally and what uses the cloud`.

### Task 63: Skippable AI key step replaces the false overview [judgment] [risky: persisted settings, the key and summary config order]

**Start only after Lane C's `saveProviderKey` has merged into `cleanup`.** Rebase `cl-f` onto `cleanup` first. Tests mock `../../src/lib/models-keys`, so they do not depend on Lane C's implementation, but `tsc` needs the file.

**Files:**
- Create `components/onboarding/steps/AiKeyStep.tsx`.
- Delete `components/onboarding/steps/SetupOverviewStep.tsx`.
- Modify `components/onboarding/steps/index.ts:4`.
- Modify `components/onboarding/OnboardingFlow.tsx` (imports at lines 3–8, the comment at 203–207, and line 212).
- Modify `contexts/OnboardingContext.tsx`: the type at lines 34–57, `completeOnboarding` at 308–331, and the provider value at 386–404.
- Create `tests/components/onboarding-ai-key.test.tsx` and `tests/contexts/onboarding-complete-key.test.tsx`.

**Interfaces:**
- Consumes `saveProviderKey(provider: string, apiKey: string): Promise<void>` from `@/lib/models-keys` (Lane C). It saves only the key and never creates a settings row (Lane C Task 31 `save_provider_key_on_fresh_db_keeps_default_provider`). The key is still saved after `complete_onboarding`, so a failed completion saves nothing and the order never depends on that guarantee.
- Decided: the step offers Claude only, because the first-run summary default is Claude; other providers are chosen later in Settings → Models & keys.
- Produces `OnboardingContextType.setPendingAiKey(key: string | null): void`.
- Produces `export function AiKeyStep(): JSX.Element`.

- [ ] Failing tests in `onboarding-ai-key.test.tsx`. The context is mocked with `goNext` and `setPendingAiKey` spies.
  - `save is disabled until a key is typed`: the `Save and continue` button has `disabled === true`.
  - `saving keeps the trimmed key and goes next`: type `"  sk-ant-x  "`, then click. `setPendingAiKey` is called with `'sk-ant-x'`, then `goNext`.
  - `skip goes next without a key`: `Skip for now` calls `setPendingAiKey(null)` and `goNext`.
  - `the key field is a password field`: the input has `type === 'password'` and an accessible label `Anthropic API key`.
  - `onboarding source makes no false promise`: read every file under `src/components/onboarding/` with `fs` and expect none to match `/Summarization Engine|never leaves your device|no cloud required/`.
- [ ] Failing tests in `onboarding-complete-key.test.tsx`. Render the real `OnboardingProvider`. Mock `invoke` so it records the command names (returning `undefined`), `listen` so it resolves to a no-op, `saveProviderKey` so it records `save:<provider>:<key>`, and `sonner`'s `toast.error`. A consumer component grabs the context.
  - `key is saved after complete_onboarding`: run `setPendingAiKey('sk-x')`, then `await completeOnboarding()`. The order is `complete_onboarding` before `save:claude:sk-x`.
  - `no key, no save`: `saveProviderKey` is never called.
  - `a failed key save still completes and tells the user`: `saveProviderKey` rejects. `completeOnboarding` resolves, and `toast.error` is called with `'Could not save the API key'` and a description containing `Settings → Models & keys`.
  - `a failed completion saves no key and rethrows`: `complete_onboarding` rejects. `completeOnboarding` rejects, and `saveProviderKey` is not called.
- [ ] Run BUN for both files. Expected: FAIL (no `AiKeyStep`, no `setPendingAiKey`).
- [ ] Implement `AiKeyStep`, at step 2. It uses `OnboardingContainer` with title "Summaries: add your AI key", `step={2}` and `totalSteps={isMac ? 4 : 3}` (move the `isMac` effect over from the deleted `SetupOverviewStep.tsx:83-93`).
  - Body copy: "Conclave writes summaries with Claude, using your own Anthropic API key. Only the transcript is sent; audio stays on this PC."
  - A shadcn `Label` and `Input` (`type="password"`, `autoComplete="off"`).
  - "Save and continue": the primary button, disabled while `key.trim()` is empty.
  - "Skip for now": a ghost button.
  - A muted line: "Using OpenAI, Groq, OpenRouter or your own server? Skip this, then choose it in Settings → Models & keys."
  - No network call, so the key is not validated here.
- [ ] Implement in `OnboardingContext`:
  - `const pendingAiKey = useRef<string | null>(null)`. A ref, so the key never enters state, the auto-save or the persisted status.
  - `setPendingAiKey = useCallback((k) => { pendingAiKey.current = k; }, [])`.
  - In `completeOnboarding`, after `await invoke('complete_onboarding')` succeeds and before `setCompleted(true)`: if the ref is set, `try { await saveProviderKey('claude', key) } catch { toast.error('Could not save the API key', { description: 'Add it in Settings → Models & keys.' }) } finally { pendingAiKey.current = null }`.
  - One-line comment: save after `complete_onboarding`, so a failed setup stores no key and the Claude default is written first.
- [ ] Wire `AiKeyStep` into `OnboardingFlow` at `currentStep === 2`, update `steps/index.ts`, delete `SetupOverviewStep.tsx`, and fix the step comment (2 = AI key).
- [ ] Run BUN for both files, then FE. Expected: PASS.
- [ ] Commit `feat(onboarding): skippable Anthropic key step, saved after setup completes`.
- [ ] Per-task review with a Codex pass (risky). Tell the reviewer to look for silent failures and for any path that saves the key before `complete_onboarding`.

### Task 64: Preview mocks and visual check for onboarding and the pop-up [judgment]

**Files:** modify `src/lib/browser-preview.ts`: the `previewHandlers` entry for `get_onboarding_status` (line 213), plus a new `get_meeting_banner` entry. This is one commit, at the end of the lane, per the plan's preview-mocks rule.
**Interfaces:** Preview URLs `/?onboarding=1` and `/banner?kind=detected|continue|ended`, used by Task 74.

- [ ] Change `get_onboarding_status` so that, with `?onboarding=1` in `location.search`, it returns `{ completed: false, current_step: 1, model_status: { parakeet: 'not_downloaded', summary: 'not_used' } }`. Otherwise it returns today's value.
- [ ] Add `get_meeting_banner`:
  - It returns `{ kind, app: 'Zoom', certain: true, line: null, continue_title: kind === 'ended' ? null : 'Weekly sync with the platform team' }`.
  - `kind` comes from `?kind=` and defaults to `detected`.
  - `meeting_banner_action` returns `null`.
- [ ] Run `pnpm run dev:preview` (under the flock rule for the build only; the dev server itself is not a build). In Playwright, at 460×72, screenshot `/banner?kind=detected`, `continue` and `ended` in the light, dark and conclave themes (`localStorage.theme`). Then screenshot onboarding steps 1–3 at 1366×768 via `/?onboarding=1`, clicking through. Save them to the scratchpad. Check: no message cut to a letter, Open visible, every control inside the window.
- [ ] Run `node frontend/scripts/a11y-check.mjs http://localhost:3118 "/banner?kind=detected" "/banner?kind=ended" "/?onboarding=1"`. Expected: exit 0 (moderate `main`/`h1` findings on the pop-up are accepted, per audit §2 "Banner").
- [ ] FE. Commit `chore(preview): onboarding and pop-up mocks`.

## Phase 3: finish, review and release (Tasks 70–79, spec §2.7 rest, §5, §7 Phase 3, §8)

Runs in `/workspace` on branch `cleanup` after every lane (A–F) has merged. Only Phase 1 and Phase 3 may touch `Sidebar/index.tsx`, `SidebarProvider.tsx`, `MainContent/index.tsx` and `app/layout.tsx`. Phase 3 may touch any file. Line numbers were verified at b15df390; lanes move them, so every task starts with the grep it names.

Gates (from `/workspace`):
- **FE**: `cd frontend && flock /workspace/.superpowers/cargo.lock nice -n 10 pnpm exec tsc --noEmit && flock /workspace/.superpowers/cargo.lock nice -n 10 bun test && flock /workspace/.superpowers/cargo.lock nice -n 10 pnpm run build`
- **RUST**: `export CARGO_TARGET_DIR=/workspace/target; flock /workspace/.superpowers/cargo.lock nice -n 10 cargo test --workspace --exclude llama-helper -j 4` (needs `frontend/out` and the stub sidecar from AGENTS.md)
- **GUARD**: `bash scripts/check-no-phone-home.sh && node --test scripts/*.test.mjs`
- **A11Y(routes)**: `node frontend/scripts/a11y-check.mjs http://localhost:3118 <routes>` against `pnpm run dev:preview`

### Task 70: The remaining `window.confirm` calls and red Delete buttons; guard tests [judgment] [risky: unsaved content and meeting deletion]

`confirmLeave` became async in Phase 1 (Task 2), and each lane replaced the confirms and red Delete buttons in its own files. This task handles what no lane owns and adds the guards.

**Files:**
- `src/lib/regenerate-guard.ts:4-18` (`shouldRegenerate`, `pickTemplateIfAllowed`) and their callers: `app/meeting-details/page-content.tsx:300,387`, `components/MeetingDetails/SummaryPanel.tsx:230,357,403`.
- `src/components/TranscriptRecovery/TranscriptRecovery.tsx:98` (bare `confirm(`) and `:276` (`variant="destructive"`).
- `src/components/OldAudio/OldAudioReview.tsx:92` (`variant="destructive"`).
- Tests: `tests/lib/regenerate-guard.test.ts`, `tests/components/summary-panel-language.test.tsx`; create `tests/lib/no-window-confirm.test.ts`.

**Interfaces:**
- Consumes `confirmAction` and `DESTRUCTIVE_OUTLINE` (Phase 1).
- Changes `shouldRegenerate(hasUnsavedEdits: boolean, ask?: (message: string) => Promise<boolean>): Promise<boolean>` and `pickTemplateIfAllowed(hasUnsavedEdits, pick, regenerate, ask?): Promise<void>`.

- [ ] Run `grep -rnE 'window\.confirm\(|(^|[^.\w])confirm\(|variant="destructive"' frontend/src` and record the list in the task notes. Any hit outside the files above (an `Alert variant="destructive"` is fine) is a lane leftover, and this task fixes it the same way.
- [ ] Failing tests:
  - `no-window-confirm.test.ts`: `no source file calls the browser confirm`: walk `src/` for `.ts`/`.tsx` files (as `theme-guard.test.ts:20-26` does) and expect no match for `/window\.confirm\s*\(|(^|[^.\w])confirm\s*\(/m`, except `lib/confirm.ts` (the no-host fallback). And `no red filled Delete button`: no match for `/<Button[^>]*variant="destructive"/`.
  - `regenerate-guard.test.ts`: `await shouldRegenerate(true, async () => false)` is `false`; `pickTemplateIfAllowed` then calls neither `pick` nor `regenerate`; clean edits resolve `true` without asking. The existing cases become async.
  - A `TranscriptRecovery` delete with a "cancel" answer (`confirmAction` mocked to resolve false) does not invoke the delete command.
- [ ] Run FE. Expected: FAIL.
- [ ] Implement:
  - The `shouldRegenerate` default `ask` is `(m) => confirmAction({ title: 'Replace your edits?', body: m, confirmLabel: 'Regenerate', destructive: true })`; rename the parameter to `ask` so the guard regex stays meaningful. Every caller awaits it before regenerating (`SummaryPanel`'s synchronous `onRegenerate` at `:357` becomes async and returns the awaited answer).
  - `TranscriptRecovery` uses `confirmAction({ title: 'Delete this meeting?', body: 'This cannot be undone.', confirmLabel: 'Delete', destructive: true })`.
  - The two red buttons become `variant="outline" className={DESTRUCTIVE_OUTLINE}`.
- [ ] Run FE. Expected: PASS.
- [ ] Commit `refactor(ui): every confirmation uses the app dialog; no red filled Delete`.
- [ ] Review with a Codex pass (risky: unsaved content, deletion). Tell the reviewer to look hard for silent failures: a missing `await` turns into an always-truthy Promise and regenerates without asking.

### Task 71: Remaining accessibility and contrast fixes [mechanical]

**Files:**
- `src/components/Prep/PrepTickList.tsx:41` (kind chip).
- `src/components/PreferenceSettings.tsx:203-207` (the blue "Note"), if Lane C did not already remove or restyle it.
- The Settings Notifications switch (Lane C, Task 37, gave it a name). Check only.
- `src/components/AudioLevelMeter.tsx:34-36,67,127-136` (`bg-green-*`, `bg-yellow-*`, `bg-red-*`), now used by the recording header's `LiveLevels`.
- `src/app/globals.css`: the `--destructive` token, only if `DESTRUCTIVE_OUTLINE` text fails contrast in a theme (light `--destructive` is about 3.8:1 on white for small text).

- [ ] Run `pnpm run dev:preview`, then A11Y(`/meeting-details?id=preview-9 "/settings?tab=app" "/settings?tab=recording"`). Expected: FAIL (`color-contrast` serious on the kind chips; the Note if still present).
- [ ] Fix with `DESIGN.md` tokens only:
  - The kind chip becomes a state chip: `rounded-full bg-muted px-2 py-0.5 text-xs text-foreground`. `--muted-foreground` on `--muted` fails at 12 px in Light.
  - The Note becomes `rounded-md bg-muted p-3 text-xs text-foreground`, with no `blue-*` classes.
  - If the Notifications switch still has no name, it gets `aria-label` (or `id` with `htmlFor`) matching its visible label.
  - The level meter colours become tokens: `bg-primary` for the level and `bg-[hsl(var(--rec))]` above 0.7; no green/yellow/red utilities.
  - If a Delete button fails contrast, darken `--destructive` in that theme's block of `globals.css` (one token change; never raw colours on the button).
- [ ] Also run A11Y on `/practice` and `/settings?tab=speakers` (outline Delete buttons).
- [ ] Repeat the A11Y routes in all three themes: set `localStorage.theme` to `light`, `dark` or `conclave` via a Playwright init script, or switch it in Settings → Appearance. Expected: exit 0. Then FE.
- [ ] Commit `fix(a11y): prep kind chips, settings note and switch names pass contrast`.

### Task 72: Meetily leftover strings [mechanical]

**Files:** whatever the grep finds. At b15df390 these were:
- `components/DeviceSelection.tsx:388-395` ("Microphone:", "System Audio:" bullets, the "Test Mic" tip; also the commented `Test Mic` at line 255). It is used by `RecordingSettings.tsx` (Lane C) and `app/_components/SettingsModal.tsx`.
- `components/LanguageSelection.tsx:261` (the "Auto Detect may produce incorrect results" warning).
- `components/SummaryModelSettings.tsx:131` ("completion(Stopping)"; Lane C Task 35 moves that card).
- The onboarding copy (Lane F, Tasks 62–63).
- `formatUsd` in `src/lib/stats.ts:55` and its test, once `grep -rn formatUsd frontend/src` shows no caller (Lanes B and D moved to `formatMoney`).

- [ ] Failing test `tests/lib/no-meetily-leftovers.test.ts`: walk `src/` and expect no match for `/Test Mic|Auto Detect may produce|completion\(Stopping\)|never leaves your device|no cloud required|Summarization Engine|<strong>Microphone:<\/strong>|<strong>System Audio:<\/strong>/`. Internal names are allowed and are not user-visible: `MeetilyRecoveryDB`, the Homebrew `meetily` paths, and the MIT credit in `WhatsNew.tsx`.
- [ ] Run BUN(`tests/lib/no-meetily-leftovers.test.ts`). Expected: FAIL if any lane left a string, otherwise PASS. Either way the test stays as the guard.
- [ ] Delete the remaining strings and their now-dead branches (for example the `isMonitoring` help block, if empty). Keep any control they described. Run FE.
- [ ] Commit `chore(ui): remove the remaining Meetily help strings`.

### Task 73: DESIGN.md names System as the default theme [mechanical]

**Files:** `DESIGN.md:26-36`.
- [ ] Edit the Themes table:
  - The Conclave row loses "(default)", and its selector becomes `.dark.theme-conclave`.
  - Add a line: the default preference is **System** (follows Windows light/dark, `lib/theme.ts` `ThemePreference`), and Conclave is opt-in in Settings → Appearance.
- [ ] Check `grep -n 'default' DESIGN.md` shows no claim that Conclave is the default. Documentation only, no gates.
- [ ] Commit `docs(design): System is the default theme, Conclave is opt-in`.

### Task 74: UI review pass: audit, screenshots, a11y on every route, polish [judgment]

**Files:** fixes inside any page, using `DESIGN.md` tokens only. The screenshot script lives in the scratchpad and is not committed.

Routes:
- `/`
- `/meeting-details` (new)
- `/meeting-details?id=preview-1`
- `/meeting-details?id=preview-9`
- `/tasks`, `/digests`, `/dictate`, `/practice`
- `/practice/report?id=ps-1` (Lane E's route and preview mock)
- `/stats`, `/whats-new`
- `/settings?tab=` for each pane: `recording`, `transcription`, `speakers`, `models`, `summaries`, `copilot`, `templates`, `names`, `app`
- `/?onboarding=1`
- `/banner?kind=detected`, `/banner?kind=ended`
- `/dictate-pill`

- [ ] Run `pnpm run dev:preview`. Open every route once with the Playwright MCP. Every "unmocked command" console warning gets a realistic handler in `browser-preview.ts`. Commit `chore(preview): mocks for every cleanup command` (only if anything changed).
- [ ] Run `/impeccable audit` on the changed screens. List the findings by severity in the task notes.
- [ ] Write a scratchpad Playwright script, using `chromium` from `frontend/node_modules`, as `a11y-check.mjs` does. It covers each route × widths 1366, 1920, 2048, 2560 (height 900; 768 at 1366) × themes `light`, `dark`, `conclave`, set via `addInitScript` on `localStorage.theme`. The sidebar uses the default for that width. The pop-up routes run at 460×72 only. Check every shot against spec §2.2 and the Global Constraints:
  - no narrow centred column at 2048
  - left-aligned titles at the same x
  - 20 px h1
  - every nav row visible at 1366×768
  - the recording header on one line at 1366
  - Start recording above the fold at 1366×768
  - no horizontal scroll
- [ ] Run A11Y(all routes above). Expected: exit 0, with zero serious or critical. Repeat for `dark` and `conclave` by adding a theme init to a scratch copy of the script; do not change the committed script.
- [ ] Run `/impeccable polish` on the defects found, then FE after each batch.
- [ ] Re-shoot the changed screens, re-run A11Y, then FE and GUARD.
- [ ] Commit `style(ui): cleanup polish from the audit` (if anything changed).

### Task 75: Whole-branch review [judgment]

- [ ] Dispatch `harness:reviewer` on `main..cleanup` with a Codex pass and an Argon pass. The reviewer skips any pass that Mission Control has switched off and says so. The brief names:
  - the spec, and the plan's Review Focus 1–5
  - the anti-goals in spec §4 (no setting reset, no lost content, no slower table, no new network call)
  - "look hard for silent failures" (the branch changes persistence, key saving, async confirmations and background report jobs)
- [ ] Fix every finding, minors included (the user's rule: fix minors before merge). Commit each fix as `fix(<area>): <what>`. Rechecks are Claude-only.
- [ ] Run FE, RUST, GUARD, plus `cargo check -p meeting-detector --target x86_64-pc-windows-msvc --tests --bins` and `cargo check -p dictate-input --target x86_64-pc-windows-msvc --tests` (each under the flock).

### Task 76: CHANGELOG v2.1.0, the version stamp and `tauri info` [mechanical]

**Files:** `CHANGELOG.md` (a new section above `## v2.0.0 — 2026-11-18`, line 3); `scripts/release-scripts.test.mjs:62` (`'2.0.0'` → `'2.1.0'`).

The version is not committed (decided, as for v2.0.0). `release.yml:27` runs `node scripts/set-version.mjs "${GITHUB_REF_NAME#v}"` on the tag, as for v2.0.0 (the repo files still say 0.4.1).

- [ ] Change the test. Run `node --test scripts/*.test.mjs`. Expected: FAIL (no v2.1.0 section).
- [ ] Add the section (the date is the day of the tag):
  ```
  ## v2.1.0 — YYYY-MM-DD
  - **Wide screens.** Pages use the full width with one left-aligned layout; the sidebar collapses from the bottom row or Ctrl B and remembers its state.
  - **Back to where you were.** A breadcrumb, Esc or Alt+Left return from a meeting to the same view, groups, search and row.
  - **Settings in one place.** Grouped settings with search; every AI model choice and API key lives in Models & keys.
  - **Practice in German.** Each practice setup has a language; German sessions are transcribed and reviewed in German.
  - Onboarding says what runs on your PC and asks for an AI key; the meeting-ended pop-up has an Open button.
  ```
- [ ] Run `node --test scripts/*.test.mjs` (PASS). `node scripts/changelog-section.mjs 2.1.0` prints the five bullets.
- [ ] Run `node scripts/set-version.mjs 2.1.0 "$(mktemp -d)"` against a scratch copy of the three files (`frontend/package.json`, `frontend/src-tauri/tauri.conf.json`, `frontend/src-tauri/Cargo.toml` under the same relative paths). Expect exit 0 and `2.1.0` in all three.
- [ ] `cd frontend && pnpm exec tauri info`. Expected: no error or mismatch line. The `tauri` crate and `@tauri-apps/api` must share a minor version (the v2.0.0 pitfall).
- [ ] Commit `docs(changelog): v2.1.0`.

### Task 77: Checkpoint push and full Windows CI [mechanical]

- [ ] `git push origin cleanup`, as its own Bash call.
- [ ] `gh workflow run ci.yml -f full=true --ref cleanup`, then `gh run watch <id> --exit-status`. Follow it to the end and report the result. On a failure, fix it locally, rerun FE/RUST, push, and rerun.

### Task 78: Merge into main and tag v2.1.0 [mechanical]

- [ ] Check `git status` is clean and `git log main..cleanup --oneline` holds only this release.
- [ ] `git checkout main && git merge --no-ff cleanup -m "Merge branch 'cleanup': wide-screen frames, back navigation, Settings and Models & keys, Practice language (v2.1.0)"`. Then run FE and GUARD on `main`.
- [ ] Set the CHANGELOG date to today if it differs (commit `docs(changelog): v2.1.0 release date`).
- [ ] `git tag v2.1.0`. Then `git push origin main` and `git push origin v2.1.0`, as separate Bash calls.

### Task 79: Follow the Release run and hand over the PC checklist [mechanical]

- [ ] `gh run list --workflow release.yml --limit 1`, then `gh run watch <id> --exit-status`, followed to the end: build, delay-load check, smoke test, `latest.json` upload and publish (`release.yml:238`). Report the result to the user. If it fails (billing, a flaky `next/font` fetch: rerun), fix and retag only with the user's agreement.
- [ ] `gh release view v2.1.0` shows it is not a draft and has `Conclave_2.1.0_x64-setup.exe` and `latest.json`.
- [ ] Hand the user the PC checklist (spec §8):
  1. Upgrade from v2.0.0 with the in-app updater. Every AI feature still uses the same provider and model (Summary, Copilot, Knowledge, Template assistant, Dictate cleanup, Practice). Keys, names, alert words, calendar feed, devices and notifications are unchanged.
  2. Open a meeting from a grouped, searched, scrolled view. Back, Esc and Alt+Left return to the same view, groups, search and row. Esc inside the notes editor does nothing. Alt+Left is consumed by the app, not by WebView2's history.
  3. Ctrl B toggles the sidebar. Restart: the state is kept.
  4. A German Practice setup: the live captions stay German, the character speaks German (if Task 51's TTS check passed), and the report quotes German with English notes and the English-only pronunciation note. Retrying a v2.0.0 session whose setup is now German gives a German report (Review Focus 4).
  5. Start a recording at 1366 px: the header fits on one line, with live meters.
  6. The meeting pop-up with a continue candidate shows the whole message. When a call ends, Open brings up the saved meeting.
- [ ] Update the Ledger rows to `done`.
- [ ] Remind the user that the next feature is the Tasks page review queue (→ Notion), as they asked on 2026-11-18 (memory `conclave-later-list`).

## Spec coverage

| Spec section | Tasks |
|---|---|
| §1 Intent, success criteria | 8, 19, 22, 25, 27, 34–35, 40–47, 52, 56–57 (checked in 74, 79) |
| §2.1 Collapse row, Ctrl B, remembered state, nav fixed, red Recording item, shortcut labels | 6, 7 (checked in 9) |
| §2.2 One `PageFrame`, variants, left-aligned, 20 px h1 | 8; applied in 19 (Meetings), 45 (Tasks), 22 (meeting page), 44 (Digests), 47 (What's new), 56 (Practice report), 40 (Stats), 57 (Practice home), 34 (Settings), 46 (Dictate), 25 (New meeting, `dashboard`); checked in 74 |
| §2.3 Day grouping and "Today · …" label | 10, 11, 12 |
| §2.3 Remembered per-view state, restored on Back and on the Meetings nav click | 13, 14, 15 |
| §2.3 Keep v2.0.0 controls; view tab counts | 18 |
| §2.3 At 1366 px: fixed Date column, one-line chips | 16 |
| §2.4 Back: breadcrumb, Esc, Alt+Left, `router.back()` or `push('/')`, `confirmLeave()` | 2, 3, 4, 5 (helpers); 22, 25, 27 (pages); 15, 42–45, 56 (origins) |
| §2.4 Header "Ask Ctrl J"; rail with Tasks (n) and Ask; Keep/Remove; cost line | 20, 21, 22 |
| §2.4 Summary toolbar and rendering | 23 |
| §2.4 New meeting: labelled name, Start on top, one audio line, prep column, leftovers removed | 24, 25 |
| §2.4 Recording: one-line header, real live levels, audio line, no duplicate overlay | 26, 27 |
| §2.5 Sub-nav with search, groups, no Back, no tab row | 30, 34 |
| §2.5 Old `?tab=` links | 30 (Review Focus 5) |
| §2.5 One row pattern, save on change, "Saved ✓" (Summary row keeps Save, decided) | 33, 35–38 |
| §2.5 Models & keys: providers, key-only save, per-feature rows, Practice row, prices | 31, 32, 35 |
| §2.5 Per-feature pickers removed elsewhere | 36 (Copilot, Knowledge, Template assistant), 46 (Dictate), 57 (Practice) |
| §2.5 Beta removed; Notifications moved and renamed | 37 |
| §2.5 Carrying settings over | 31, 32 (Review Focus 1), 35 ("mount writes nothing") |
| §2.6 Stats: cost card, 8-week chart, Talk time table | 40, 41, 42 |
| §2.6 Digests: three panes, meeting chips | 43, 44 |
| §2.6 Tasks; Dictate | 45; 46 |
| §2.6 Practice: dashboard home, report route, Models card move, outline Delete with confirm | 56, 57 (Models row in 35) |
| §2.7 Onboarding copy and skippable AI key step | 62, 63 (key command from 31) |
| §2.7 Pop-up title; ended Open | 60; 61 |
| §2.7 Notifications switch name | 37 (checked in 71) |
| §2.7 Contrast: Practice Delete, prep kind chips, Settings note | 57 and 71; 71; 38 (checked in 71) |
| §2.7 `a11y-check.mjs` zero serious or critical on every route | 9, 19, 29, 39, 48, 58, 64 (per lane); 71, 74 (all routes) |
| §2.7 One date helper, no US dates | 1; applied in 10, 17, 28, 36, 38, 39, 42–45, 56, 57 |
| §2.7 `window.confirm` replaced | 2 (`confirmLeave`), 28, 39, 46, 57; 70 (the rest and the guard test) |
| §2.7 Red filled Delete → outline destructive | 2 (`DESTRUCTIVE_OUTLINE`, confirm dialog), 19, 28, 39, 46, 57; 70 (the rest and the guard test) |
| §2.7 Meetily leftovers | 24 (device and language help), 35 (Summary copy), 62–63 (onboarding), 72 (sweep and guard) |
| §2.7 DESIGN.md default theme | 73 |
| §2.8 Language per setup: migration, editor, session language | 50, 54 |
| §2.8 Whisper, character prompt and TTS in the setup's language | 51 |
| §2.8 Report prompt, pronunciation English-only with the note | 52 |
| §2.8 Retry clears the old error; a failed start reaches the UI | 53, 55 |
| §2.8 "The report stopped unexpectedly" root cause | 59 (BLOCKED on the user's log) |
| §3 Non-goals, §4 Anti-goals | Review Focus owners; 27 ("leaving never stops the recording"); 13 (no backend calls per row); 75 |
| §5 Constraints (1366–2560 px, three themes, tokens) | each lane's preview task (19, 29, 39, 48, 58, 64); 74 |
| §6 Approaches | decided; no task |
| §7 Phases and lanes | plan structure: 1–9 → 10–64 → 70–79; Lane C per-task Codex pass |
| §8 Bun tests | 1 (dates), 11–12 (day groups), 13–14 (view state), 30 (`?tab=`), 32 (carry-over), 4 (Esc guard), 6 (sidebar state) |
| §8 Rust tests | 50 (migration), 51 (Whisper language), 52 (pronunciation skip, prompt language), 53 (retry start), 31 (key save leaves the model config) |
| §8 Browser preview: four widths, three themes, a11y, audit, polish | 74 (lane checks in 9, 16, 19, 22, 25, 29, 39, 48, 58, 64) |
| §8 Windows PC checklist | 79 |
| §9 Left to the builder | decided in 13–14 (view state in module memory, scroll by row key when virtual), 20 (rail and Ask tab), 33–34 (row component, client-side search) |
| Release: CHANGELOG, `tauri info`, full CI, merge, tag, Release run | 76, 77, 78, 79 |

## Ledger

| Task | Status |
|---|---|
| 1 Dates helper | todo |
| 2 Confirm dialog, async `confirmLeave` | todo |
| 3 Back navigation helper | todo |
| 4 Esc and Alt+Left shortcut | todo |
| 5 BackCrumb | todo |
| 6 Sidebar state and Ctrl B | todo |
| 7 Sidebar Collapse row and red Recording item | todo |
| 8 PageFrame | todo |
| 9 Phase 1 gate and review | todo |
| 10 Today's day label | todo |
| 11 `defaultCollapsed` | todo |
| 12 Table default collapse plus flips | todo |
| 13 Per-view memory | todo |
| 14 Scroll restore | todo |
| 15 Origin when the table opens a meeting | todo |
| 16 Table fits at 1366 px | todo |
| 17 `formatMeetingDate` on `formatRowDate` | todo |
| 18 Tab counts and v2.0.0 controls | todo |
| 19 Meetings on the list frame | todo |
| 20 MeetingRail and AI sidebar rail mode | todo |
| 21 Tasks panel for the rail | todo |
| 22 Finished meeting page on the reading frame | todo |
| 23 Summary toolbar and rendering | todo |
| 24 Audio summary line, Meetily help removed | todo |
| 25 New meeting page | todo |
| 26 Real recording levels (Rust) | todo |
| 27 Recording page header | todo |
| 28 Lane B confirms, dates, Delete recording | todo |
| 29 Lane B preview mocks and checks | todo |
| 30 Settings pane map and search index | todo |
| 31 `api_save_provider_key` and `saveProviderKey` | todo |
| 32 Feature model choices | todo |
| 33 SettingsRow and "Saved ✓" | todo |
| 34 Settings shell | todo |
| 35 Models & keys pane | todo |
| 36 Pickers removed; Copilot, Templates, Names & calendar | todo |
| 37 Capture panes, Notifications row, Beta toggle | todo |
| 38 Appearance & app pane | todo |
| 39 Lane C confirms, dates, Delete buttons, preview mocks | todo |
| 40 Stats frame and cost card | todo |
| 41 Cost chart | todo |
| 42 Talk time table | todo |
| 43 Digest hook, view and meeting chips | todo |
| 44 Digests in three panes | todo |
| 45 Tasks on the list frame | todo |
| 46 Dictate frame, mode delete, cleanup model link | todo |
| 47 What's new on the reading frame | todo |
| 48 Lane D preview mocks and checks | todo |
| 50 Practice setup language (migration, storage) | todo |
| 51 Live call in the setup's language | todo |
| 52 Report in the session's language | todo |
| 53 Retry claims before it returns | todo |
| 54 Language in the Setup editor | todo |
| 55 Retry clears the old error | todo |
| 56 Practice report route | todo |
| 57 Practice home frame, Models link, outline Delete | todo |
| 58 Lane E preview mocks | todo |
| 59 Report panic fix | BLOCKED (the user's app log) |
| 60 Banner message on its own row | todo |
| 61 Banner ended Open | todo |
| 62 Welcome copy | todo |
| 63 AI key step | todo |
| 64 Onboarding and pop-up preview | todo |
| 70 Remaining confirms and red Delete buttons | todo |
| 71 A11y and contrast | todo |
| 72 Meetily leftovers | todo |
| 73 DESIGN.md theme | todo |
| 74 UI review pass | todo |
| 75 Whole-branch review | todo |
| 76 CHANGELOG, version stamp, `tauri info` | todo |
| 77 Push and full CI | todo |
| 78 Merge and tag | todo |
| 79 Release run and PC checklist | todo |

## Open items for the user

1. **German filler counts (my default, 2026-11-18).** The fluency numbers count English fillers only. A German report hides the filler line rather than showing a misleading "0" (Lane E, Task 52). A German filler list can come later if the user wants it.
2. **Task 59** stays BLOCKED until the user sends the `panic in thread …` line from the app log for a report that ended in "The report stopped unexpectedly".
3. **Level meter (decision d).** The plan builds a real meter (Task 26). If the user would rather not touch the recording path, drop Task 26 and the meter from the recording header.

## Resume notes

- **Branch and status:** `cleanup`, started from `main` at `a2618022`. The spec is `b15df390`. `main` is 1 commit ahead of `origin/main`; nothing is pushed.
- **Mockups:** the five sections the user approved are at `.superpowers/brainstorm/cleanup-mockups/cleanup-v2.html`. They are local only and contain real meeting titles, so never commit them.
- **Audit:** `.superpowers/brainstorm/ux-audit-v2.md`.
- **Execution:** the user asked for maximum parallelism. Run subagent-driven development: Phase 1 in one lane, then Lanes A–F in their own worktrees `/workspace/.worktrees/cl-<lane>` (at most 8 agents, one build at a time under the flock), merged in the order in "Parallel lanes", then Phase 3.
- **Stale worktrees:** every folder in `/workspace/.worktrees/` that existed before this plan (from the v2.0.0 redesign) is merged and can be removed before the lanes start.
