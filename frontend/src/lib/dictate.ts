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

export interface DictateSettings {
  enabled: boolean;
  /** null follows the microphone selected for Meetings. */
  preferred_mic_device: string | null;
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

// Main first, then the built-in hotkeys and the custom modes; a blank optional hotkey means none
export function hotkeyProblem(s: DictateSettings): string | null {
  const hotkeys = [s.main_hotkey, ...BUILT_IN_IDS.map((id) => s.builtin_hotkeys[id]), ...s.modes.map((m) => m.hotkey)];
  const seen = new Set<string>();
  for (const [i, text] of hotkeys.entries()) {
    if (i > 0 && !text?.trim()) continue;
    const parts = hotkeyToParts(text ?? '');
    const problem = shapeProblem(parts);
    if (problem) return problem;
    const combo = hotkeyFromParts(parts);
    if (seen.has(combo)) return `'${combo}' is already used by another hotkey`;
    seen.add(combo);
  }
  return null;
}

// Trims, then ignores empty, over-long and duplicate (case-insensitive) words. Returns the same array when ignored.
export function addWord(words: string[], word: string): string[] {
  const w = word.trim();
  if (w === '' || Array.from(w).length > MAX_WORD_CHARS || words.length >= MAX_WORDS) return words;
  if (words.some((x) => x.toLowerCase() === w.toLowerCase())) return words;
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
