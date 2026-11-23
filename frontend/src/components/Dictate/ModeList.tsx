import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { confirmAction } from '@/lib/confirm';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { MoreHorizontal } from 'lucide-react';
import { BUILT_IN_MODES, DEFAULT_MODE, hotkeyProblem, newModeId, withHotkey, type Commit, type CustomMode, type DictateSettings } from '@/lib/dictate';
import { HotkeyPicker, KeyCaps } from './HotkeyPicker';
import { ModeEditor, type ModeDraft } from './ModeEditor';

export function ModeList({ settings, commit }: { settings: DictateSettings; commit: Commit }) {
  // 'new' opens an empty editor; a mode opens its editor
  const [editing, setEditing] = useState<CustomMode | 'new' | null>(null);

  // The mode whose hotkey picker is open
  const [openPicker, setOpenPicker] = useState<string | null>(null);

  const hotkeyCheck = (id: string) => (hotkey: string | null) => hotkeyProblem(withHotkey(settings, id, hotkey), id);

  const saveMode = (draft: ModeDraft) => {
    commit((s) => {
      if (editing && editing !== 'new') {
        return { ...s, modes: s.modes.map((m) => (m.id === editing.id ? { ...m, ...draft } : m)) };
      }
      const id = newModeId(draft.name, s.modes.map((m) => m.id));
      return { ...s, modes: [...s.modes, { ...draft, id, hotkey: null }] };
    });
    setEditing(null);
  };

  const removeMode = async (mode: CustomMode) => {
    if (!(await confirmAction({ title: `Delete the mode "${mode.name}"?`, confirmLabel: 'Delete', destructive: true }))) return;
    commit((s) => ({
      ...s,
      modes: s.modes.filter((m) => m.id !== mode.id),
      // A default that no longer exists would make the next save fail
      default_mode: s.default_mode === mode.id ? DEFAULT_MODE : s.default_mode,
    }));
  };

  const makeDefault = (id: string) => commit((s) => ({ ...s, default_mode: id }));

  // One row: name, hotkey chip (opens the picker), default badge or action, and the overflow menu of a custom mode
  const row = (id: string, name: string, hotkey: string | null, description?: string, custom?: CustomMode) => (
    <li key={id} data-mode={id} className="py-3">
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex-1 min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium truncate">{name}</span>
            {settings.default_mode === id && <span className="rounded px-1.5 py-0.5 text-xs bg-muted text-foreground/75">Default</span>}
          </div>
          {description && <p className="text-sm text-muted-foreground">{description}</p>}
        </div>
        <button
          type="button"
          aria-expanded={openPicker === id}
          className="rounded-md border border-border px-2 py-1 hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => setOpenPicker(openPicker === id ? null : id)}
        >
          <KeyCaps value={hotkey} empty="Set hotkey" />
          <span className="sr-only"> {hotkey ? `(${name} hotkey, change)` : `for ${name}`}</span>
        </button>
        <div className="w-28 shrink-0 text-right">
          {settings.default_mode !== id && (
            <Button variant="ghost" size="sm" aria-label={`Make default: ${name}`} onClick={() => makeDefault(id)}>Make default</Button>
          )}
        </div>
        {custom && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" aria-label={`More actions for ${name}`}><MoreHorizontal className="h-4 w-4" /></Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => setEditing(custom)}>Edit</DropdownMenuItem>
              <DropdownMenuItem aria-label={`Delete ${name}`} className="text-destructive focus:text-destructive" onSelect={() => removeMode(custom)}>Delete</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
      {openPicker === id && (
        <div className="mt-2">
          <HotkeyPicker
            label={`${name} hotkey`}
            value={hotkey}
            showValue={false}
            check={hotkeyCheck(id)}
            onChange={(h) => commit((s) => withHotkey(s, id, h))}
          />
        </div>
      )}
    </li>
  );

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-foreground">Modes</h2>
        <Button variant="outline" size="sm" onClick={() => setEditing('new')}>+ New mode</Button>
      </div>

      <div>
        <h3 className="text-sm font-medium text-muted-foreground">Built in</h3>
        <ul className="divide-y divide-border">
          {BUILT_IN_MODES.map((m) => row(m.id, m.name, settings.builtin_hotkeys[m.id] ?? null, m.description))}
        </ul>
      </div>

      {settings.modes.length > 0 && (
        <div>
          <h3 className="text-sm font-medium text-muted-foreground">Your modes</h3>
          <ul className="divide-y divide-border">
            {settings.modes.map((m) => row(m.id, m.name, m.hotkey, undefined, m))}
          </ul>
        </div>
      )}

      {editing !== null && (
        <ModeEditor
          mode={editing === 'new' ? null : editing}
          onSave={saveMode}
          onCancel={() => setEditing(null)}
        />
      )}
    </section>
  );
}
