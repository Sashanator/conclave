import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { LlmOverridePicker } from '@/components/shared/LlmOverridePicker';
import { BUILT_IN_MODES, hotkeyProblem, withHotkey, type Commit, type DictateSettings, type DictateStatus } from '@/lib/dictate';
import { HotkeyPicker } from './HotkeyPicker';

export function DictateGeneral({ settings, status, commit }: { settings: DictateSettings; status: DictateStatus | null; commit: Commit }) {
  const blocked = !status || !status.engine_is_whisper || !status.model_downloaded;

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-5">
      <h3 className="text-lg font-semibold text-foreground">General</h3>

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="dictate-enabled">Dictate</Label>
          <p className="text-sm text-muted-foreground">Dictate hotkeys work in any app while Conclave is running</p>
        </div>
        <Switch
          id="dictate-enabled"
          aria-label="Dictate"
          checked={settings.enabled}
          // Turning it off stays possible when the engine or model is missing
          disabled={blocked && !settings.enabled}
          onCheckedChange={(enabled) => commit((s) => ({ ...s, enabled }))}
        />
      </div>
      {status?.reason && <p className="text-sm text-muted-foreground">{status.reason}</p>}
      <p className="text-sm text-muted-foreground">
        Hold the hotkey to talk, or tap it to start and tap again to stop. Esc cancels. Doesn&apos;t work while an app running as administrator, or a VMware virtual machine that has captured the keyboard, is in front. The microphone stays on while Dictate is on.
      </p>

      <div className="space-y-1.5">
        <Label id="dictate-main-hotkey">Main hotkey</Label>
        <HotkeyPicker
          label="Main hotkey"
          labelId="dictate-main-hotkey"
          value={settings.main_hotkey}
          check={(h) => hotkeyProblem(withHotkey(settings, 'main', h))}
          onChange={(h) => commit((s) => withHotkey(s, 'main', h))}
        />
      </div>

      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="dictate-default-mode">Default mode</Label>
        <Select value={settings.default_mode} onValueChange={(mode) => commit((s) => ({ ...s, default_mode: mode }))}>
          <SelectTrigger id="dictate-default-mode" className="w-56" aria-label="Default mode">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {BUILT_IN_MODES.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
            ))}
            {settings.modes.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <p className="text-sm text-muted-foreground">Used by every mode except &quot;As spoken&quot;</p>
      <LlmOverridePicker
        idPrefix="dictate-cleanup"
        providerLabel="Cleanup provider"
        modelLabel="Cleanup model"
        provider={settings.cleanup_provider}
        model={settings.cleanup_model}
        onChange={(cleanup_provider, cleanup_model) => commit((s) => ({ ...s, cleanup_provider, cleanup_model }))}
      />
    </section>
  );
}
