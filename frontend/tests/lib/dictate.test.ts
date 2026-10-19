import { describe, expect, test } from 'bun:test';
import {
  addWord, hotkeyFromParts, hotkeyProblem, hotkeyToParts, newModeId, withHotkey, type DictateSettings,
} from '../../src/lib/dictate';

const base: DictateSettings = {
  enabled: false,
  main_hotkey: 'Ctrl+Win',
  default_mode: 'cleaned-up',
  cleanup_provider: null,
  cleanup_model: null,
  builtin_hotkeys: {},
  modes: [],
  dictionary: [],
};

describe('hotkey strings', () => {
  test('canonical order is Ctrl, Alt, Shift, Win, key', () => {
    expect(hotkeyFromParts({ ctrl: true, alt: false, shift: false, win: true, key: null })).toBe('Ctrl+Win');
    expect(hotkeyFromParts({ ctrl: false, alt: true, shift: true, win: true, key: 'P' })).toBe('Alt+Shift+Win+P');
    expect(hotkeyFromParts({ ctrl: false, alt: false, shift: false, win: false, key: null })).toBe('');
  });

  test('round trip through hotkeyToParts', () => {
    const parts = hotkeyToParts('win+ctrl+f5');
    expect(parts).toEqual({ ctrl: true, alt: false, shift: false, win: true, key: 'F5' });
    expect(hotkeyFromParts(parts)).toBe('Ctrl+Win+F5');
    expect(hotkeyToParts('Ctrl+Space').key).toBe('Space');
  });
});

describe('hotkeyProblem', () => {
  test('a single modifier is not enough on its own', () => {
    expect(hotkeyProblem({ ...base, main_hotkey: 'Ctrl' })).toContain('two modifiers');
  });

  test('the main hotkey needs at least one modifier', () => {
    expect(hotkeyProblem({ ...base, main_hotkey: '' })).not.toBeNull();
  });

  test('Ctrl+Shift and Alt+Shift alone are the layout switch', () => {
    expect(hotkeyProblem({ ...base, main_hotkey: 'Ctrl+Shift' })).not.toBeNull();
    expect(hotkeyProblem({ ...base, main_hotkey: 'Alt+Shift' })).not.toBeNull();
  });

  test('a key needs Ctrl, Alt or Win', () => {
    expect(hotkeyProblem({ ...base, main_hotkey: 'Shift+P' })).toContain('Ctrl, Alt or Win');
    expect(hotkeyProblem({ ...base, main_hotkey: 'Alt+P' })).toBeNull();
  });

  test('a duplicate across main, built-in and custom hotkeys is reported', () => {
    const dup = { ...base, modes: [{ id: 'm', name: 'M', instructions: '', examples: [], hotkey: 'Ctrl+Win' }] };
    expect(hotkeyProblem(dup)).toContain('already used');
    const builtin = { ...base, builtin_hotkeys: { 'as-spoken': 'Ctrl+Win' } };
    expect(hotkeyProblem(builtin)).toContain('already used');
  });

  test('a valid set has no problem', () => {
    expect(hotkeyProblem({ ...base, builtin_hotkeys: { 'cleaned-up': 'Ctrl+Alt+C' }, modes: [{ id: 'm', name: 'M', instructions: '', examples: [], hotkey: null }] })).toBeNull();
  });
});

describe('withHotkey', () => {
  test('sets and clears a built-in hotkey, and sets a mode hotkey, leaving the rest alone', () => {
    const mode = { id: 'm', name: 'M', instructions: '', examples: [], hotkey: null };
    const set = withHotkey({ ...base, modes: [mode] }, 'as-spoken', 'Ctrl+Alt+A');
    expect(set.builtin_hotkeys).toEqual({ 'as-spoken': 'Ctrl+Alt+A' });
    expect(withHotkey(set, 'as-spoken', null).builtin_hotkeys).toEqual({});
    expect(withHotkey(set, 'm', 'Alt+M').modes[0].hotkey).toBe('Alt+M');
    expect(withHotkey(set, 'main', 'Alt+Win').main_hotkey).toBe('Alt+Win');
  });
});

describe('addWord', () => {
  test('dedupes case-insensitively and keeps order', () => {
    expect(addWord(['Conclave', 'Tauri'], 'conclave')).toEqual(['Conclave', 'Tauri']);
    expect(addWord(['Conclave'], '  Tauri  ')).toEqual(['Conclave', 'Tauri']);
  });

  test('ignores empty and over-long words', () => {
    expect(addWord(['A'], '   ')).toEqual(['A']);
    expect(addWord([], 'x'.repeat(65))).toEqual([]);
    expect(addWord([], 'x'.repeat(64))).toEqual(['x'.repeat(64)]);
  });

  test('stops at 200 words', () => {
    const full = Array.from({ length: 200 }, (_, i) => `w${i}`);
    expect(addWord(full, 'extra')).toHaveLength(200);
  });
});

describe('newModeId', () => {
  test('suffixes a taken slug', () => {
    expect(newModeId('Prompt for Claude', ['prompt-for-claude'])).toBe('prompt-for-claude-2');
  });

  test('never returns a built-in id', () => {
    expect(newModeId('As spoken', [])).not.toBe('as-spoken');
    expect(newModeId('Cleaned up', [])).not.toBe('cleaned-up');
  });

  test('falls back for names with no latin letters and stays within 40 characters', () => {
    expect(newModeId('Промпт', [])).toMatch(/^[a-z0-9-]{1,40}$/);
    expect(newModeId('x'.repeat(80), [])).toMatch(/^[a-z0-9-]{1,40}$/);
    const id = newModeId('x'.repeat(80), ['x'.repeat(40)]);
    expect(id.length).toBeLessThanOrEqual(40);
    expect(id).not.toBe('x'.repeat(40));
  });
});
