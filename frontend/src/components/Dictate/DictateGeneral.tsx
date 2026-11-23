import { useEffect, useState } from 'react';
import Link from 'next/link';
import { invoke } from '@tauri-apps/api/core';
import type { AudioDevice } from '@/components/DeviceSelection';
import { cn } from '@/lib/utils';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { hotkeyProblem, withHotkey, type Commit, type DictateSettings, type DictateStatus, type LiveTextStyle } from '@/lib/dictate';
import { HotkeyPicker, KeyCaps } from './HotkeyPicker';

export function DictateGeneral({ settings, status, commit }: { settings: DictateSettings; status: DictateStatus | null; commit: Commit }) {
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [loading, setLoading] = useState(true);
  const [deviceError, setDeviceError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    invoke<AudioDevice[]>('get_audio_devices')
      .then((listed) => { if (active) setDevices(listed.filter((d) => d.device_type === 'Input')); })
      .catch((err) => {
        console.error('Failed to fetch Dictate microphones:', err);
        if (active) setDeviceError('Failed to load microphones. Please check your system audio settings.');
      })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  const missingMic = settings.preferred_mic_device && !devices.some((d) => `${d.name} (input)` === settings.preferred_mic_device);
  const blocked = !status || !status.engine_is_whisper || !status.model_downloaded;
  const ready = Boolean(status) && !status!.reason && settings.enabled;
  const cleanupModel = settings.cleanup_model ? `${settings.cleanup_provider ?? ''} ${settings.cleanup_model}`.trim() : 'Same as summaries';

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
      <h2 className="text-lg font-semibold text-foreground">General</h2>

      <p role="status" className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-card px-3 py-2 text-sm text-foreground">
        <span aria-hidden className={cn('h-2 w-2 shrink-0 rounded-full', ready ? 'bg-green-500' : 'bg-amber-500')} />
        {status?.reason ?? (!status ? 'Checking Dictate…' : !settings.enabled ? 'Dictate is off. Turn it on to use the hotkeys.' : <>Ready: hold <KeyCaps value={settings.main_hotkey} />, speak, release</>)}
      </p>

      <Row label={<Label htmlFor="dictate-enabled">Enable Dictate</Label>} hint="Hotkeys work in any app while Conclave is running">
        <Switch
          id="dictate-enabled"
          aria-label="Enable Dictate"
          checked={settings.enabled}
          // Turning it off stays possible when the engine or model is missing
          disabled={blocked && !settings.enabled}
          onCheckedChange={(enabled) => commit((s) => ({ ...s, enabled }))}
        />
      </Row>

      <Row label={<Label id="dictate-main-hotkey">Main hotkey</Label>} hint="Hold to talk, or tap to start and tap to stop. Esc cancels.">
        <HotkeyPicker
          label="Main hotkey"
          labelId="dictate-main-hotkey"
          value={settings.main_hotkey}
          check={(h) => hotkeyProblem(withHotkey(settings, 'main', h), 'main')}
          onChange={(h) => commit((s) => withHotkey(s, 'main', h))}
        />
      </Row>

      <Row label={<Label htmlFor="dictate-microphone">Microphone</Label>}>
        <Select
          value={settings.preferred_mic_device ?? 'same-as-meetings'}
          onValueChange={(value) => commit((s) => ({ ...s, preferred_mic_device: value === 'same-as-meetings' ? null : value }))}
          disabled={loading}
        >
          <SelectTrigger id="dictate-microphone" aria-label="Microphone">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="same-as-meetings">Same as Meetings</SelectItem>
            {devices.map((device) => (
              <SelectItem key={device.name} value={`${device.name} (input)`}>{device.name}</SelectItem>
            ))}
            {missingMic && <SelectItem value={settings.preferred_mic_device!}>{settings.preferred_mic_device!.replace(/ \(input\)$/, "")} (unavailable)</SelectItem>}
          </SelectContent>
        </Select>
        {loading && <p className="mt-1 text-sm text-muted-foreground">Loading microphones…</p>}
        {deviceError && <p role="alert" className="mt-1 text-sm text-destructive">{deviceError}</p>}
        {!loading && !deviceError && devices.length === 0 && <p className="mt-1 text-sm text-muted-foreground">No microphones found</p>}
        {missingMic && !loading && !deviceError && <p className="mt-1 text-sm text-muted-foreground">The selected microphone is unavailable. Dictate uses the default microphone until it returns.</p>}
      </Row>

      <Row label={<Label htmlFor="dictate-live-text">Live text</Label>} hint="How the words show while you speak. Takes effect on the next dictation.">
        <Select
          value={settings.live_text_style}
          onValueChange={(v) => commit((s) => ({ ...s, live_text_style: v as LiveTextStyle }))}
        >
          <SelectTrigger id="dictate-live-text" aria-label="Live text">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="panel">Panel</SelectItem>
            <SelectItem value="line">One line</SelectItem>
          </SelectContent>
        </Select>
      </Row>

      <Row label={<span className="text-sm font-medium">Cleanup model</span>} hint="Used by every mode except &quot;As spoken&quot;">
        <p className="text-sm">
          {cleanupModel}{' '}
          <Link href="/settings?tab=models" className="text-muted-foreground underline hover:text-foreground">Change</Link>
        </p>
      </Row>

      <details className="text-sm">
        <summary className="w-fit cursor-pointer text-muted-foreground hover:text-foreground">When it doesn&apos;t work</summary>
        <ul className="mt-2 list-disc space-y-1 pl-5 text-muted-foreground">
          <li>Dictate can&apos;t see the keyboard while an app running as administrator, or a VMware virtual machine that has captured the keyboard, is in front.</li>
          <li>Some Win-key shortcuts are taken by Windows first. If recording one does nothing, use Edit manually.</li>
          <li>The microphone stays on while Dictate is on.</li>
        </ul>
      </details>
    </section>
  );
}

// A label column and a control column; the hint sits right under its control
function Row({ label, hint, children }: { label: React.ReactNode; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1.5 sm:grid-cols-[10rem_1fr] sm:items-start sm:gap-4">
      <div className="sm:pt-1.5">{label}</div>
      <div className="min-w-0 space-y-1">
        {children}
        {hint && <p className="text-sm text-muted-foreground">{hint}</p>}
      </div>
    </div>
  );
}
