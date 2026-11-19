'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { SettingsRow, useSavedTick } from '@/components/Settings/SettingsRow';
import { formatRowDate } from '@/lib/dates';

// The Rust `KnowledgeSettingsView` (snake_case, no rename)
type KnowledgeView = {
  provider: string | null;
  model: string | null;
  my_names: string[];
  extract_enabled: boolean;
  digest_enabled: boolean;
  calendar_marker: string;
};

// The Rust `CalendarStatus` (snake_case, no rename). The feed URL is a secret and never reaches the frontend, only its host.
type CalendarStatus = {
  connected: boolean;
  host: string | null;
  last_ok_at: string | null;
  last_error: string | null;
  last_error_at: string | null;
  upcoming: number;
};

const NO_CALENDAR: CalendarStatus = { connected: false, host: null, last_ok_at: null, last_error: null, last_error_at: null, upcoming: 0 };

// Same limits as the backend (knowledge/mod.rs)
const MAX_NAMES = 20;
const MAX_NAME_CHARS = 40;

function toInput(v: KnowledgeView) {
  return {
    provider: v.provider,
    model: v.model,
    my_names: v.my_names,
    extract_enabled: v.extract_enabled,
    digest_enabled: v.digest_enabled,
    calendar_marker: v.calendar_marker,
  };
}

export function KnowledgeSettings() {
  const [view, setView] = useState<KnowledgeView | null>(null);
  const [name, setName] = useState('');
  const [saved, markSaved] = useSavedTick();
  const [savedRow, setSavedRow] = useState('');
  // Edits show at once. Saves run one after another, each from the latest edit.
  const latest = useRef<KnowledgeView | null>(null);
  const queue = useRef<Promise<void>>(Promise.resolve());
  // Bumped after a failed save and its reload: saves queued before that are dropped
  const generation = useRef(0);
  const [calendar, setCalendar] = useState<CalendarStatus | null>(null);
  // The feed URL lives only in this input, and is cleared once a connect succeeds
  const [feedUrl, setFeedUrl] = useState('');
  const [feedBusy, setFeedBusy] = useState(false);
  const [feedMessage, setFeedMessage] = useState<{ text: string; error: boolean } | null>(null);
  // The marker being typed; null when not editing. Saved on blur or Enter, so a keystroke never saves.
  const [markerDraft, setMarkerDraft] = useState<string | null>(null);

  const show = (v: KnowledgeView) => {
    latest.current = v;
    setView(v);
  };

  const load = async () => {
    try {
      show(await invoke<KnowledgeView>('knowledge_get_settings'));
    } catch (err) {
      toast.error('Could not load Knowledge settings', { description: String(err) });
    }
  };

  const loadCalendar = async () => {
    try {
      setCalendar(await invoke<CalendarStatus>('calendar_status'));
    } catch (err) {
      toast.error('Could not load calendar status', { description: String(err) });
      // The connect form stays usable when the status cannot be read
      setCalendar(NO_CALENDAR);
    }
  };

  const connect = async () => {
    const url = feedUrl.trim();
    if (!url || feedBusy) return;
    setFeedBusy(true);
    setFeedMessage(null);
    try {
      const status = await invoke<CalendarStatus>('calendar_connect', { url });
      setFeedUrl('');
      setCalendar(status);
      setFeedMessage({ text: `Found ${status.upcoming} upcoming ${status.upcoming === 1 ? 'event' : 'events'}`, error: false });
    } catch (err) {
      setFeedMessage({ text: String(err), error: true });
    } finally {
      setFeedBusy(false);
    }
  };

  const disconnect = async () => {
    try {
      setCalendar(await invoke<CalendarStatus>('calendar_disconnect'));
      setFeedMessage(null);
    } catch (err) {
      setFeedMessage({ text: String(err), error: true });
    }
  };

  useEffect(() => {
    load();
    loadCalendar();
  }, []);

  const commit = (row: string, change: (v: KnowledgeView) => KnowledgeView) => {
    if (!latest.current) return;
    const next = change(latest.current);
    const gen = generation.current;
    show(next);
    queue.current = queue.current.then(async () => {
      if (gen !== generation.current) return;
      try {
        const result = await invoke<KnowledgeView>('knowledge_save_settings', { input: toInput(next) });
        // A newer edit has already replaced this one, so only the newest result is shown
        if (latest.current === next) show(result);
        setSavedRow(row);
        markSaved();
      } catch (err) {
        toast.error('Knowledge settings were not saved', { description: String(err) });
        await load();
        generation.current++;
      }
    });
  };

  if (!view) return null;

  const addName = () => {
    const text = name.trim();
    const taken = view.my_names.some((n) => n.toLowerCase() === text.toLowerCase());
    if (!text || taken || [...text].length > MAX_NAME_CHARS || view.my_names.length >= MAX_NAMES) return; // rejected: keep the text so it can be fixed
    commit('My names', (v) => ({ ...v, my_names: [...v.my_names, text] }));
    setName('');
  };

  const saveMarker = () => {
    if (markerDraft === null) return;
    const text = markerDraft;
    setMarkerDraft(null);
    if (text !== view.calendar_marker) commit('My calls marker', (v) => ({ ...v, calendar_marker: text }));
  };

  const tick = (row: string) => saved && savedRow === row;

  return (
    <div>
      <SettingsRow label="My names" htmlFor="knowledge-name" saved={tick('My names')} block helper="Names the summaries use for you, e.g. Sasha, Саша, Alexander">
        <div className="space-y-2">
          <Input
            id="knowledge-name"
            aria-label="Add a name"
            placeholder="Add a name and press Enter"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addName();
              }
            }}
          />
          <ul className="flex flex-wrap gap-2">
            {view.my_names.map((n) => (
              <li key={n} className="flex items-center gap-1.5 rounded-full border border-border px-3 py-1 text-sm">
                {n}
                <button
                  type="button"
                  aria-label={`Remove ${n}`}
                  className="text-muted-foreground hover:text-foreground"
                  onClick={() => commit('My names', (v) => ({ ...v, my_names: v.my_names.filter((x) => x !== n) }))}
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </li>
            ))}
          </ul>
        </div>
      </SettingsRow>

      <SettingsRow label="Extract my tasks after each summary" htmlFor="knowledge-extract" saved={tick('Extract my tasks after each summary')}>
        <Switch
          id="knowledge-extract"
          aria-label="Extract my tasks after each summary"
          checked={view.extract_enabled}
          onCheckedChange={(on) => commit('Extract my tasks after each summary', (v) => ({ ...v, extract_enabled: on }))}
        />
      </SettingsRow>

      <SettingsRow label="Weekly digest" htmlFor="knowledge-digest" saved={tick('Weekly digest')}>
        <Switch
          id="knowledge-digest"
          aria-label="Weekly digest"
          checked={view.digest_enabled}
          onCheckedChange={(on) => commit('Weekly digest', (v) => ({ ...v, digest_enabled: on }))}
        />
      </SettingsRow>

      {calendar && (
        <>
          <SettingsRow label="My calls marker" htmlFor="calendar-marker" saved={tick('My calls marker')} helper="Only events whose title contains it become meetings. Changing it never removes existing meetings.">
            <Input
              id="calendar-marker"
              aria-label="My calls marker"
              className="w-32"
              value={markerDraft ?? view.calendar_marker}
              onChange={(e) => setMarkerDraft(e.target.value)}
              onBlur={saveMarker}
              onKeyDown={(e) => {
                if (e.key === 'Enter') saveMarker();
              }}
            />
          </SettingsRow>

          <SettingsRow label="Calendar" block>
          {calendar.connected ? (
            <div className="space-y-2">
              <p className="text-sm text-muted-foreground">{calendar.host}</p>
              <p className="text-sm text-muted-foreground">
                Last sync: {calendar.last_ok_at ? formatRowDate(new Date(calendar.last_ok_at)) : 'never'}
              </p>
              {calendar.last_error && (
                <p className="text-sm text-destructive">
                  Last error{calendar.last_error_at && ` (${formatRowDate(new Date(calendar.last_error_at))})`}: {calendar.last_error}
                </p>
              )}
              <Button variant="outline" size="sm" onClick={disconnect}>
                Disconnect
              </Button>
            </div>
          ) : (
            <div className="space-y-2">
              <p className="text-sm text-muted-foreground">Not connected</p>
              <Label htmlFor="calendar-url">Secret address (iCal)</Label>
              <Input
                id="calendar-url"
                type="password"
                aria-label="Calendar secret address"
                autoComplete="off"
                value={feedUrl}
                onChange={(e) => setFeedUrl(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                In Google Calendar: Settings → your calendar → Integrate calendar → Secret address in iCal format
              </p>
              <Button size="sm" onClick={connect} disabled={!feedUrl.trim() || feedBusy}>
                Connect
              </Button>
            </div>
          )}

          {feedMessage && <p className={`text-sm ${feedMessage.error ? 'text-destructive' : 'text-muted-foreground'}`}>{feedMessage.text}</p>}
          </SettingsRow>
        </>
      )}
    </div>
  );
}
