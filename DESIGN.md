# DESIGN.md: Conclave

Design system every redesign UI task follows. Source of truth for tokens is `frontend/src/app/globals.css` (HSL triplets used as `hsl(var(--x))`; Tailwind maps them in `frontend/tailwind.config.js`). Mockups: `docs/superpowers/specs/2026-11-16-redesign-mockups/`.

## Tokens

Existing in `globals.css` (use these; never hard-code colours):
`--background --foreground --card --card-foreground --popover --popover-foreground --primary --primary-foreground --secondary --secondary-foreground --muted --muted-foreground --accent --accent-foreground --destructive --destructive-foreground --border --input --ring --radius --chart-1..5 --blue-50..950`.

`--radius` is `0.5rem`. Borders are 1px `hsl(var(--border))`. Focus ring is `--ring`.

### New tokens (introduced by the mockups, NOT yet in `globals.css`)
The first UI task that needs one adds it to `globals.css` for all three themes. Values come from the mockups.

| Token | Light | Dark | Use |
|---|---|---|---|
| `--rec` | `0 72% 51%` | `0 74% 42%` | recording state, Live chip |
| `--violet` | `262 83% 52%` | `258 90% 72%` | Ask / AI accents |
| `--hl` | `43 95% 55%` (alpha `--hl-a: .38`) | same | search-hit and note highlight |
| `--live` | `142 70% 28%` | n/a | "live" text on light |
| `--cat-1..5` | `217 91% 60%`, `160 60% 40%`, `27 87% 55%`, `262 70% 60%`, `0 0% 55%` | same | category chip hues (1 blue, 2 green, 3 orange, 4 violet, 5 grey = "No template") |
| `--row` | `40px` | n/a | table row height (see Density) |

The mockups also use a darker `--muted-foreground` (`0 0% 40%` light, `0 0% 68%` dark) and `--card: 0 0% 6%` in dark. `globals.css` keeps `45.1%`, `63.9%` and `3.9%`; keep the existing values unless a contrast check fails.

## Themes

Three themes, switched by a class on the root element:

| Theme | Selector | Look |
|---|---|---|
| Light | `:root` | white background `0 0% 100%`, near-black text, primary `0 0% 9%` |
| Dark | `.dark` | background `0 0% 3.9%`, text `0 0% 98%`, primary `0 0% 98%` |
| Conclave | `.dark.theme-conclave` | deep indigo background `235 45% 9%`, card `235 40% 12%`, warm text `40 30% 94%`, gold primary and ring `43 90% 58%`, border `235 30% 22%`, muted `235 30% 18%`, destructive `0 80% 66%` |

The default preference is **System** (follows Windows light/dark, `lib/theme.ts` `ThemePreference`); Conclave is opt-in in Settings → Appearance. Conclave also restyles `.record-button`. Charts use `--chart-1..5` per theme. Every new component must be checked in all three themes.

## Typography

Font: **Source Sans 3** (`--font-source-sans-3`, Tailwind `font-sans`), antialiased. Scale used by the mockups:

| Role | Size | Weight |
|---|---|---|
| Page title | 20-24px | 600 |
| Section title | 16px | 600 |
| Body, table cells, inputs | 14px | 400 |
| Chips, captions, `kbd` | 12px (`kbd` 11-12px) | 400-600 |

Muted text uses `--muted-foreground`. Tables never wrap in compact density.

## Density (meetings table)

| Mode | Row height | Behaviour |
|---|---|---|
| Comfortable (default) | 40px (`--row`) | gist and long cells may wrap, clamped to 2 lines |
| Compact | 32px (`.compact` sets `--row`) | single line, ellipsis |

Cells: `0 10px` padding, 1px bottom border. Hover on a row uses `--accent`. Transitions (150 ms background) only under `prefers-reduced-motion: no-preference`.

## Chips

- **Category chip**: pill (`border-radius:999px`), 12px, padding `1px 8px`, 6px gap, a dot or label; background `hsl(var(--c)/.12)`, text `--foreground`, where `--c` is one of `--cat-1..5`.
- **State chip**: 12px pill. Live is solid `hsl(var(--rec))` with white 600-weight text. Other states (Reviewed, New) use the neutral `--muted` fill with `--muted-foreground`.
- **Filter chip**: pill, 12px, dashed `--border` when unset; solid border and `--foreground` text when set.
- **Calendar badge**: the 📞 marker on rows created from a calendar event; sits inline before the title, 12px, no fill.

## Layout

- **Left nav**: `nav aria-label="Main"`. Items are 32px high, 16px icons, 10px gap, 6px radius, `--muted-foreground`; hover and `aria-current=page` use `--accent` background and `--foreground` text, current page weight 600. A flexible separator pushes the lower group to the bottom.
- **AI sidebar (Ask)**: right-hand `aside`, **360px** wide, left border `--border`, background `--card`, toggled from the toolbar (`aria-pressed`, `aria-controls`). `hidden` removes it. (The mockups drew 340px; 360px is the decided width.)
- **Toolbar buttons**: 32-36px high, `--radius`, ghost variant for toggles, violet-tinted variant (`--violet` at .14 fill, .6 border) for Ask actions.

## Keyboard hints (`kbd`)

Inline `kbd`: 11-12px, 1px `--border`, 4px radius, `0 4px` padding, `--muted-foreground`. Shown beside actions (for example New meeting, Ctrl N) and mirrored in the button `title`.

## Accessibility

Every control is a real button or link with an accessible name; icons are `aria-hidden`. Focus uses `--ring`. Run `node frontend/scripts/a11y-check.mjs` on changed pages; no serious or critical axe violations.
