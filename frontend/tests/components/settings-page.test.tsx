import { afterAll, afterEach, beforeEach, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  nav: { ...await import('next/navigation') },
  guard: { ...await import('../../src/lib/unsaved-guard') },
  frame: { ...await import('../../src/components/PageFrame/PageFrame') },
  config: { ...await import('../../src/contexts/ConfigContext') },
};
const stubs = ['TranscriptSettings', 'RecordingSettings', 'UpdatesSettings', 'PreferenceSettings', 'SummaryModelSettings', 'CopilotSettings', 'KnowledgeSettings', 'TemplateSettings', 'TemplateAssistSettings', 'SpeakerSettings', 'Settings/ModelsKeysPane', 'Settings/SummariesPane', 'Settings/NotionSettings'];
const stubOrig: Record<string, any> = {};
for (const n of stubs) stubOrig[n] = { ...(await import(`../../src/components/${n}`)) };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('next/navigation', () => originals.nav);
  mock.module('../../src/lib/unsaved-guard', () => originals.guard);
  mock.module('../../src/components/PageFrame/PageFrame', () => originals.frame);
  mock.module('../../src/contexts/ConfigContext', () => originals.config);
  for (const n of stubs) mock.module(`../../src/components/${n}`, () => stubOrig[n]);
});

let tab: string | null = null;
const replace = mock((_u: string) => {});
let leave = true;
const frameProps: any[] = [];
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke: async () => null }));
mock.module('next/navigation', () => ({ useSearchParams: () => ({ get: () => tab }), useRouter: () => ({ replace, push() {}, back() {} }) }));
mock.module('../../src/lib/unsaved-guard', () => ({ ...originals.guard, confirmLeave: async () => leave }));
mock.module('../../src/contexts/ConfigContext', () => ({ ...originals.config, useConfig: () => ({ transcriptModelConfig: {}, setTranscriptModelConfig() {} }) }));
mock.module('../../src/components/PageFrame/PageFrame', () => ({
  PageFrame: (p: any) => { frameProps.push(p); return <div><h1>{p.title}</h1>{p.subnav}{p.children}</div>; },
}));
for (const n of stubs) mock.module(`../../src/components/${n}`, () => { const k = n.split('/').pop()!; return { [k]: () => <div>{`stub:${k}`}</div> }; });

const Page = (await import('../../src/app/settings/page')).default;
const live: ReturnType<typeof create>[] = [];
const render = async () => { let r!: ReturnType<typeof create>; await act(async () => { r = create(<Page />); }); live.push(r); return r; };
afterEach(() => { act(() => { for (const r of live.splice(0)) r.unmount(); }); });
const inst = (n: any): string => (n.children as any[]).map((c) => (typeof c === 'string' ? c : inst(c))).join('');
const text = (r: ReturnType<typeof create>) => JSON.stringify(r.toJSON());
beforeEach(() => { replace.mockClear(); frameProps.length = 0; leave = true; tab = null; });

test('old tab id maps to its pane', async () => {
  tab = 'knowledge';
  const r = await render();
  expect(r.root.findByType('h1').children).toEqual(['Names & calendar']);
  expect(text(r)).toContain('stub:KnowledgeSettings');
  tab = 'costs';
  expect((await render()).root.findByType('h1').children).toEqual(['Models & keys']);
});

test('tab=notion renders the Notion pane', async () => {
  tab = 'notion';
  const r = await render();
  expect(r.root.findByType('h1').children).toEqual(['Notion']);
  expect(text(r)).toContain('stub:NotionSettings');
});

test('no Back, no tabs, no Beta; nav shows groups with aria-current', async () => {
  tab = 'copilot';
  const r = await render();
  const t = text(r);
  expect(t).not.toContain('Back');
  expect(t).not.toContain('Beta');
  expect(t).not.toContain('tablist');
  expect(r.root.findAllByProps({ role: 'tab' })).toHaveLength(0);
  const nav = r.root.findByProps({ 'aria-label': 'Settings' });
  expect(nav.type).toBe('nav');
  for (const g of ['Capture', 'AI', 'You', 'App']) expect(t).toContain(g);
  expect(r.root.findAllByProps({ 'aria-current': 'page' }).some((n) => n.type === 'button' && n.children.includes('Copilot'))).toBe(true);
});

test('clicking a pane replaces the URL, unless confirmLeave says no', async () => {
  tab = 'recording';
  const r = await render();
  const btn = () => r.root.findAllByType('button').find((b) => b.children.includes('Copilot'))!;
  await act(async () => { await btn().props.onClick(); });
  expect(replace).toHaveBeenCalledWith('/settings?tab=copilot');
  replace.mockClear();
  leave = false;
  await act(async () => { await btn().props.onClick(); });
  expect(replace).not.toHaveBeenCalled();
});

test('frame variant: list for templates, form otherwise; subnav present', async () => {
  tab = 'templates';
  await render();
  expect(frameProps.at(-1).variant).toBe('list');
  expect(frameProps.at(-1).subnav).toBeTruthy();
  tab = 'copilot';
  await render();
  expect(frameProps.at(-1).variant).toBe('form');
});

test('search lists a hit with its pane and navigates; no match message', async () => {
  tab = 'recording';
  const r = await render();
  const input = r.root.findByProps({ 'aria-label': 'Search settings' });
  await act(async () => { input.props.onChange({ target: { value: 'calendar' } }); });
  const hit = r.root.findAllByType('button').find((b) => inst(b).includes('Calendar') && inst(b).includes('Names & calendar'))!;
  expect(hit).toBeTruthy();
  await act(async () => { await hit.props.onClick(); });
  expect(replace).toHaveBeenCalledWith('/settings?tab=names');
  await act(async () => { r.root.findByProps({ 'aria-label': 'Search settings' }).props.onChange({ target: { value: 'zzz' } }); });
  expect(text(r)).toContain('No settings match');
});

test('a search pick scrolls once the target pane has rendered its row', async () => {
  tab = 'recording';
  const frames: (() => void)[] = [];
  const realRaf = globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame = ((cb: () => void) => frames.push(cb)) as any;
  let lookups = 0;
  const scrollIntoView = mock(() => {});
  const realGet = document.getElementById.bind(document);
  document.getElementById = ((id: string) => (id === 'setting-calendar' && ++lookups >= 3 ? ({ scrollIntoView } as any) : null)) as any;
  try {
    const r = await render();
    await act(async () => { r.root.findByProps({ 'aria-label': 'Search settings' }).props.onChange({ target: { value: 'calendar' } }); });
    const hit = r.root.findAllByType('button').find((b) => inst(b).includes('Names & calendar'))!;
    await act(async () => { await hit.props.onClick(); });
    for (let i = 0; i < 5 && frames.length; i++) frames.shift()!();
    // Earlier tests' retry loops may also look; what matters is that the row was found only on a later frame
    expect(lookups).toBeGreaterThanOrEqual(3);
    expect(scrollIntoView).toHaveBeenCalled();
  } finally { globalThis.requestAnimationFrame = realRaf; document.getElementById = realGet; }
});
