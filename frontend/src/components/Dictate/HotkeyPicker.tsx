import { useState } from 'react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { hotkeyFromParts, hotkeyToParts, type HotkeyParts } from '@/lib/dictate';

const LETTERS = Array.from({ length: 26 }, (_, i) => String.fromCharCode(65 + i));
const DIGITS = Array.from({ length: 10 }, (_, i) => String(i));
const F_KEYS = Array.from({ length: 12 }, (_, i) => `F${i + 1}`);
const KEYS = [...LETTERS, ...DIGITS, ...F_KEYS, 'Space'];
const MODIFIERS = [
  { name: 'ctrl', label: 'Ctrl' },
  { name: 'alt', label: 'Alt' },
  { name: 'shift', label: 'Shift' },
  { name: 'win', label: 'Win' },
] as const;

interface HotkeyPickerProps {
  label: string;
  /** id of the visible label, when there is one */
  labelId?: string;
  /** The saved hotkey, or null for none. */
  value: string | null;
  /** Returns the problem with a candidate hotkey (null = none), or null when it is allowed. */
  check: (hotkey: string | null) => string | null;
  onChange: (hotkey: string | null) => void;
}

export function HotkeyPicker({ label, labelId, value, check, onChange }: HotkeyPickerProps) {
  // Only an invalid edit is kept here; a valid one is saved at once and the saved value shows again
  const [draft, setDraft] = useState<HotkeyParts | null>(null);
  const shown = draft ?? hotkeyToParts(value ?? '');
  const problem = draft ? check(hotkeyFromParts(draft) || null) : null;

  const update = (next: HotkeyParts) => {
    const hotkey = hotkeyFromParts(next) || null;
    if (check(hotkey)) {
      setDraft(next);
      return;
    }
    setDraft(null);
    if (hotkey !== (value || null)) onChange(hotkey);
  };

  return (
    <div className="space-y-1">
      <div role="group" aria-label={label} aria-labelledby={labelId} className="flex flex-wrap items-center gap-3">
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
      {problem && <p className="text-xs text-destructive">{problem}</p>}
    </div>
  );
}
