import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { BUILT_IN_MODES, DEFAULT_MODE, hotkeyProblem, newModeId, withHotkey, type Commit, type CustomMode, type DictateSettings } from '@/lib/dictate';
import { HotkeyPicker } from './HotkeyPicker';
import { ModeEditor, type ModeDraft } from './ModeEditor';

export function ModeList({ settings, commit }: { settings: DictateSettings; commit: Commit }) {
  // 'new' opens an empty editor; a mode opens its editor
  const [editing, setEditing] = useState<CustomMode | 'new' | null>(null);

  const hotkeyCheck = (id: string) => (hotkey: string | null) => hotkeyProblem(withHotkey(settings, id, hotkey));

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

  const removeMode = (mode: CustomMode) => {
    if (!window.confirm(`Delete the mode "${mode.name}"?`)) return;
    commit((s) => ({
      ...s,
      modes: s.modes.filter((m) => m.id !== mode.id),
      // A default that no longer exists would make the next save fail
      default_mode: s.default_mode === mode.id ? DEFAULT_MODE : s.default_mode,
    }));
  };

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold text-foreground">Modes</h3>
        <Button size="sm" onClick={() => setEditing('new')}>+ New mode</Button>
      </div>

      <ul className="divide-y divide-border">
        {BUILT_IN_MODES.map((m) => (
          <li key={m.id} data-mode={m.id} className="flex flex-wrap items-center gap-3 py-3">
            <div className="flex-1 min-w-0">
              <div>
                <span className="font-medium">{m.name}</span>
                <span className="ml-2 rounded px-1.5 py-0.5 text-xs bg-muted text-muted-foreground">built-in</span>
              </div>
              <p className="text-sm text-muted-foreground">{m.description}</p>
            </div>
            <HotkeyPicker
              label={`${m.name} hotkey`}
              value={settings.builtin_hotkeys[m.id] ?? null}
              check={hotkeyCheck(m.id)}
              onChange={(h) => commit((s) => withHotkey(s, m.id, h))}
            />
          </li>
        ))}

        {settings.modes.map((m) => (
          <li key={m.id} data-mode={m.id} className="flex flex-wrap items-center gap-3 py-3">
            <div className="flex-1 min-w-0 font-medium truncate">{m.name}</div>
            <HotkeyPicker
              label={`${m.name} hotkey`}
              value={m.hotkey}
              check={hotkeyCheck(m.id)}
              onChange={(h) => commit((s) => withHotkey(s, m.id, h))}
            />
            <Button variant="outline" size="sm" onClick={() => setEditing(m)}>Edit</Button>
            <Button variant="destructive" size="sm" aria-label={`Delete ${m.name}`} onClick={() => removeMode(m)}>Delete</Button>
          </li>
        ))}
      </ul>

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
