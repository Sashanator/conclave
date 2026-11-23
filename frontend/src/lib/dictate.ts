// Dictate settings as the Rust side stores them (frontend/src-tauri/src/dictate/settings.rs), and the
// pure checks the Dictate page runs before saving. The checks mirror the backend's rules.

export interface ModeExample { said: string; want: string }

export interface CustomMode {
  id: string;
  name: string;
  instructions: string;
  examples: ModeExample[];
  hotkey: string | null;
}

export type LiveTextStyle = 'panel' | 'line';

export interface DictateSettings {
  enabled: boolean;
  /** null follows the microphone selected for Meetings. */
  preferred_mic_device: string | null;
  live_text_style: LiveTextStyle;
  main_hotkey: string;
  default_mode: string;
  /** null means the same provider and model as summaries. */
  cleanup_provider: string | null;
  cleanup_model: string | null;
  /** Keys are "as-spoken" or "cleaned-up". A missing key means no hotkey. */
  builtin_hotkeys: Record<string, string>;
  modes: CustomMode[];
  dictionary: string[];
}

export interface DictateStatus {
  enabled: boolean;
  model_downloaded: boolean;
  mic_ok: boolean;
  engine_is_whisper: boolean;
  reason: string | null;
}

export interface HotkeyParts {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
  key: string | null;
}

export const BUILT_IN_MODES = [
  { id: 'as-spoken', name: 'As spoken', description: 'Whisper text, no cloud' },
  { id: 'cleaned-up', name: 'Cleaned up', description: 'Removes fillers and self-corrections, fixes punctuation' },
];
export const BUILT_IN_IDS: string[] = BUILT_IN_MODES.map((m) => m.id);
export const DEFAULT_MODE = 'cleaned-up';
const MAX_WORDS = 200;
const MAX_WORD_CHARS = 64;
const MAX_ID_CHARS = 40;

/** Applies a change to the latest settings and saves it. */
export type Commit = (change: (s: DictateSettings) => DictateSettings) => void;

/** Edits one binding: 'main', a built-in id or a custom mode id. null clears a built-in or mode hotkey. */
export function withHotkey(s: DictateSettings, target: string, hotkey: string | null): DictateSettings {
  if (target === 'main') return { ...s, main_hotkey: hotkey ?? '' };
  if (BUILT_IN_IDS.includes(target)) {
    const builtin = { ...s.builtin_hotkeys };
    if (hotkey) builtin[target] = hotkey;
    else delete builtin[target];
    return { ...s, builtin_hotkeys: builtin };
  }
  return { ...s, modes: s.modes.map((m) => (m.id === target ? { ...m, hotkey } : m)) };
}

// Canonical order (Ctrl, Alt, Shift, Win, key), the same string the backend's Hotkey::display writes
export function hotkeyFromParts(p: HotkeyParts): string {
  return [p.ctrl && 'Ctrl', p.alt && 'Alt', p.shift && 'Shift', p.win && 'Win', p.key]
    .filter(Boolean)
    .join('+');
}

export function hotkeyToParts(s: string): HotkeyParts {
  const parts: HotkeyParts = { ctrl: false, alt: false, shift: false, win: false, key: null };
  for (const raw of s.split('+')) {
    const name = raw.trim().toLowerCase();
    if (name === 'ctrl' || name === 'control') parts.ctrl = true;
    else if (name === 'alt') parts.alt = true;
    else if (name === 'shift') parts.shift = true;
    else if (name === 'win' || name === 'windows') parts.win = true;
    else if (name) parts.key = name === 'space' ? 'Space' : name.toUpperCase();
  }
  return parts;
}

// One binding on its own: at least two modifiers without a key, Ctrl or Alt or Win with a key
function shapeProblem(p: HotkeyParts): string | null {
  const combo = hotkeyFromParts(p);
  const mods = [p.ctrl, p.alt, p.shift, p.win].filter(Boolean).length;
  if (p.key === null) {
    if (mods === 0) return 'Choose at least one modifier';
    if (mods < 2) return `'${combo}' needs at least two modifiers`;
    if (combo === 'Ctrl+Shift' || combo === 'Alt+Shift') return `'${combo}' is Windows' keyboard-layout switch`;
  } else if (!(p.ctrl || p.alt || p.win)) {
    return `'${combo}' needs Ctrl, Alt or Win`;
  }
  return null;
}

// Main first, then the built-in hotkeys and the custom modes; a blank optional hotkey means none.
// `editing` (main, a built-in id or a mode id) is the binding being changed: a clash names the other one.
export function hotkeyProblem(s: DictateSettings, editing?: string): string | null {
  const bindings = [
    { id: 'main', name: 'the main hotkey', text: s.main_hotkey },
    ...BUILT_IN_MODES.map((m) => ({ id: m.id, name: m.name, text: s.builtin_hotkeys[m.id] })),
    ...s.modes.map((m) => ({ id: m.id, name: m.name, text: m.hotkey })),
  ];
  const seen = new Map<string, string>();
  for (const [i, b] of bindings.entries()) {
    if (i > 0 && !b.text?.trim()) continue;
    const parts = hotkeyToParts(b.text ?? '');
    const problem = shapeProblem(parts);
    if (problem) return problem;
    const combo = hotkeyFromParts(parts);
    const owner = seen.get(combo);
    if (owner !== undefined) {
      const other = b.id === editing ? bindings.find((x) => x.id === owner)!.name : b.name;
      return `'${combo}' is already used by ${other}`;
    }
    seen.set(combo, b.id);
  }
  return null;
}

// The key of a keyboard event the backend accepts (A-Z, 0-9, F1-F24, Space), else null. Windows virtual
// keys follow the layout, so a Latin letter comes from `key` (QWERTZ Y/Z); other layouts (Russian) fall back to `code`.
export function keyFromCode(code: string, key?: string): string | null {
  if (key && /^[a-zA-Z]$/.test(key)) return key.toUpperCase();
  const m = /^(?:Key([A-Z])|Digit(\d)|F([1-9]|1\d|2[0-4]))$/.exec(code);
  if (m) return m[1] ?? m[2] ?? `F${m[3]}`;
  return code === 'Space' ? 'Space' : null;
}

// Why a word would be ignored, or null when it can be added (an empty one is ignored without a reason)
export function wordProblem(words: string[], word: string): string | null {
  const w = word.trim();
  if (w === '') return null;
  if (Array.from(w).length > MAX_WORD_CHARS) return `Words can be at most ${MAX_WORD_CHARS} characters`;
  if (words.some((x) => x.toLowerCase() === w.toLowerCase())) return `"${w}" is already in the dictionary`;
  if (words.length >= MAX_WORDS) return `The dictionary is full (${MAX_WORDS} words)`;
  return null;
}

// Trims, then ignores empty, over-long and duplicate (case-insensitive) words. Returns the same array when ignored.
export function addWord(words: string[], word: string): string[] {
  const w = word.trim();
  if (w === '' || wordProblem(words, w)) return words;
  return [...words, w];
}

// A slug of the name ([a-z0-9-], at most 40), with -2, -3... added while taken. Built-in ids are always taken.
export function newModeId(name: string, taken: string[]): string {
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+/, '').slice(0, MAX_ID_CHARS).replace(/-+$/, '') || 'mode';
  const used = new Set([...taken, ...BUILT_IN_IDS]);
  let id = slug;
  for (let n = 2; used.has(id); n++) {
    const suffix = `-${n}`;
    id = `${slug.slice(0, MAX_ID_CHARS - suffix.length).replace(/-+$/, '')}${suffix}`;
  }
  return id;
}
