import { useRef, useState, type KeyboardEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { hotkeyFromParts, hotkeyToParts, keyFromCode, type HotkeyParts } from '@/lib/dictate';

const LETTERS = Array.from({ length: 26 }, (_, i) => String.fromCharCode(65 + i));
const DIGITS = Array.from({ length: 10 }, (_, i) => String(i));
const F_KEYS = Array.from({ length: 24 }, (_, i) => `F${i + 1}`);
const KEYS = [...LETTERS, ...DIGITS, ...F_KEYS, 'Space'];
const MODIFIERS = [
  { name: 'ctrl', label: 'Ctrl' },
  { name: 'alt', label: 'Alt' },
  { name: 'shift', label: 'Shift' },
  { name: 'win', label: 'Win' },
] as const;

const MODIFIER_CODES = ['ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'ShiftLeft', 'ShiftRight', 'MetaLeft', 'MetaRight'];

/** A hotkey as key caps; `empty` shows when there is none. */
export function KeyCaps({ value, empty = 'No key' }: { value: string | null; empty?: string }) {
  if (!value) return <span className="text-sm text-muted-foreground">{empty}</span>;
  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      {value.split('+').map((k) => (
        <kbd key={k} className="rounded border border-border border-b-2 bg-muted px-1.5 py-0.5 font-mono text-xs text-foreground">{k}</kbd>
      ))}
    </span>
  );
}

interface HotkeyPickerProps {
  label: string;
  /** id of the visible label, when there is one */
  labelId?: string;
  /** The saved hotkey, or null for none. */
  value: string | null;
  /** Returns the problem with a candidate hotkey (null = none), or null when it is allowed. */
  check: (hotkey: string | null) => string | null;
  onChange: (hotkey: string | null) => void;
  /** Show the current key caps (off where a chip already shows them). */
  showValue?: boolean;
}

export function HotkeyPicker({ label, labelId, value, check, onChange, showValue = true }: HotkeyPickerProps) {
  // Only an invalid edit is kept here; a valid one is saved at once and the saved value shows again
  const [draft, setDraft] = useState<HotkeyParts | null>(null);
  const [recording, setRecording] = useState(false);
  const [captureError, setCaptureError] = useState<string | null>(null);
  // Modifiers held at the most, for a modifier-only shortcut (Ctrl+Win) that is saved when they are released
  const peak = useRef<HotkeyParts>({ ctrl: false, alt: false, shift: false, win: false, key: null });
  // A non-modifier key went down in this press: releasing the modifiers must not save them alone
  const sawKey = useRef(false);
  const shown = draft ?? hotkeyToParts(value ?? '');
  const problem = draft ? check(hotkeyFromParts(draft) || null) : null;

  const update = (next: HotkeyParts) => {
    const hotkey = hotkeyFromParts(next) || null;
    setCaptureError(null);
    if (check(hotkey)) {
      setDraft(next);
      return;
    }
    setDraft(null);
    if (hotkey !== (value || null)) onChange(hotkey);
  };

  const finish = (next: HotkeyParts) => {
    setRecording(false);
    update(next);
  };

  const start = () => {
    peak.current = { ctrl: false, alt: false, shift: false, win: false, key: null };
    sawKey.current = false;
    setCaptureError(null);
    setRecording(true);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (!recording) return;
    const held = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey;
    if (e.code === 'Tab' && !held) return; // let focus move on; the blur ends recording
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      setRecording(false);
      return;
    }
    if (MODIFIER_CODES.includes(e.code)) {
      const p = peak.current;
      peak.current = { ...p, ctrl: p.ctrl || e.ctrlKey, alt: p.alt || e.altKey, shift: p.shift || e.shiftKey, win: p.win || e.metaKey };
      return;
    }
    sawKey.current = true;
    const key = keyFromCode(e.code, e.key);
    if (!key) {
      setCaptureError(`'${e.key}' is not a supported key. Use a letter, a digit, F1-F24 or Space.`);
      return;
    }
    finish({ ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, win: e.metaKey, key });
  };

  const onKeyUp = (e: KeyboardEvent) => {
    if (!recording || !MODIFIER_CODES.includes(e.code)) return;
    if (e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return;
    e.preventDefault();
    if (sawKey.current) {
      // Everything is released: the next press starts clean
      sawKey.current = false;
      peak.current = { ctrl: false, alt: false, shift: false, win: false, key: null };
      return;
    }
    finish(peak.current);
  };

  const message = captureError ?? problem;

  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap items-center gap-3">
        {showValue && <KeyCaps value={hotkeyFromParts(shown) || null} />}
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`Record shortcut for ${label}`}
          aria-pressed={recording}
          onClick={start}
          onKeyDown={onKeyDown}
          onKeyUp={onKeyUp}
          onBlur={() => setRecording(false)}
        >
          Record shortcut
        </Button>
        {recording && <span role="status" className="text-sm text-muted-foreground">Press the shortcut… Esc cancels</span>}
      </div>
      <details className="text-sm">
        <summary className="w-fit cursor-pointer text-muted-foreground hover:text-foreground">Edit manually</summary>
        <div role="group" aria-label={label} aria-labelledby={labelId} className="mt-2 flex flex-wrap items-center gap-3">
          {MODIFIERS.map((mod) => (
            <label key={mod.name} className="flex items-center gap-1.5 text-sm">
              <input
                type="checkbox"
                className="h-4 w-4"
                checked={shown[mod.name]}
                onChange={(e) => update({ ...shown, [mod.name]: e.target.checked })}
              />
              {mod.label}
            </label>
          ))}
          <Select value={shown.key ?? 'none'} onValueChange={(v) => update({ ...shown, key: v === 'none' ? null : v })}>
            <SelectTrigger className="w-28" aria-label={`${label} key`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="max-h-64 overflow-y-auto">
              <SelectItem value="none">No key</SelectItem>
              {KEYS.map((key) => (
                <SelectItem key={key} value={key}>{key}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </details>
      {message && (
        <p role="alert" className="flex flex-wrap items-center gap-2 text-sm text-destructive">
          {message}
          {draft && (
            <Button type="button" variant="outline" size="sm" onClick={() => { setDraft(null); setCaptureError(null); }}>
              Revert
            </Button>
          )}
        </p>
      )}
    </div>
  );
}
