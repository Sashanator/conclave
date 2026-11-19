import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { SETTINGS_GROUPS, isKnownTab, paneForTab, searchSettings, settingsHref, type SettingsPane } from '@/lib/settings-panes';

const SRC = join(import.meta.dir, '..', '..', 'src');
const files = (dir: string): string[] =>
  readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p) : /\.(tsx?|css)$/.test(n) ? [p] : [];
  });

describe('paneForTab', () => {
  test.each([
    [null, 'recording'], ['nonsense', 'recording'], ['general', 'app'], ['recording', 'recording'],
    ['Transcriptionmodels', 'transcription'], ['transcriptionmodels', 'transcription'], ['beta', 'transcription'],
    ['speakers', 'speakers'], ['summaryModels', 'summaries'], ['summary', 'summaries'],
    ['copilot', 'copilot'], ['knowledge', 'names'], ['costs', 'models'], ['templates', 'templates'],
  ] as [string | null, SettingsPane][])('%s', (tab, pane) => expect(paneForTab(tab)).toBe(pane));

  test('every pane id maps to itself', () => {
    for (const p of SETTINGS_GROUPS.flatMap((g) => g.panes)) expect(paneForTab(p.id)).toBe(p.id);
  });
});

test('settingsHref', () => expect(settingsHref('models')).toBe('/settings?tab=models'));

test('groups and labels', () => {
  expect(SETTINGS_GROUPS.map((g) => g.label)).toEqual(['Capture', 'AI', 'You', 'App']);
  const panes = SETTINGS_GROUPS.flatMap((g) => g.panes);
  expect(panes.map((p) => p.label)).toEqual(['Recording & devices', 'Transcription & vocabulary', 'Speakers', 'Models & keys', 'Summaries', 'Copilot', 'Templates', 'Names & calendar', 'Appearance & app']);
  expect(new Set(panes.map((p) => p.id)).size).toBe(panes.length);
});

describe('links', () => {
  const all = files(SRC).filter((f) => !f.endsWith('settings-panes.ts')).map((f) => [f, readFileSync(f, 'utf8')] as const);
  test('every /settings?tab= link is a known tab', () => {
    for (const [f, s] of all) for (const m of s.matchAll(/\/settings\?tab=([A-Za-z]+)/g)) expect([f, isKnownTab(m[1])]).toEqual([f, true]);
  });
  test('no dynamic /settings?tab=${ links', () => {
    expect(all.filter(([, s]) => s.includes('/settings?tab=${')).map(([f]) => f)).toEqual([]);
  });
});

describe('searchSettings', () => {
  test('calendar', () => expect(searchSettings('calendar')).toContainEqual({ pane: 'names', label: 'Calendar' }));
  test('case-insensitive substring', () =>
    expect(searchSettings('STARTS/STOPS')).toContainEqual({ pane: 'recording', label: 'Windows notifications when recording starts/stops' }));
  test('no match', () => expect(searchSettings('deadline words')).toEqual([]));
  test('blank', () => {
    expect(searchSettings('')).toEqual([]);
    expect(searchSettings('   ')).toEqual([]);
  });
});

// Turn on in Task 38, once Tasks 34-38 add the labels.
test.todo('every SETTINGS_INDEX label appears in src/components or src/app/settings', () => {});
