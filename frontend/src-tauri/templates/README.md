# Summary templates

This folder holds the summary templates shipped with Conclave: the seven meeting templates and `standard_meeting`.

On the first start of v0.4.0 the app copies these files once into `%APPDATA%\Conclave\templates` and writes a marker file, `.seeded-v1`, there. It never copies them again, so a template you delete stays deleted. To change a template, edit it in Settings → Templates; the copies in `%APPDATA%\Conclave\templates` are the ones the app uses.

`standard_meeting` is the fallback. It cannot be deleted, and if its file is missing or invalid the built-in copy is used.

## Template fields

One JSON file per template. The file name without `.json` is the template id; ids never change after creation.

| Field | Type | Meaning |
|---|---|---|
| `name` | string, required | Display name |
| `description` | string, optional (was required) | Kept for old files; the editor does not show it |
| `use_when` | string, optional | One line for auto-pick, e.g. "1-on-1 with my HoD Rasulov about work tasks" |
| `title_keywords` | string[], optional | Case-insensitive substrings matched against the meeting title |
| `language` | string, optional | BCP-47 code (`ru`, `en`, `de`, …). Absent or `"auto"` means the meeting's language |
| `context` | string, optional | Free text, sent as written |
| `sections` | array, required, ≥1 | `{title, instruction, format?, item_format?}`; `format` becomes optional (default `paragraph`) and the editor does not show it |
| `style` | string, optional | Free text, sent as written |

Validation: non-empty name, at least one section, every section has a non-empty title and instruction, `language` is `auto` or a code Conclave knows, and `format` (when present) is one of `paragraph`, `list`, `string`. A file that fails validation is skipped and reported once in Settings → Templates.
