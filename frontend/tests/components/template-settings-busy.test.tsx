import { afterAll, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import type { ReactNode } from 'react';
import { confirmLeave } from '../../src/lib/unsaved-guard';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  dialog: { ...await import('../../src/components/ui/dialog') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const noop = () => null;
const listing = {
  templates: [
    { id: 'standard_meeting', name: 'Standard', use_when: null },
    { id: 'beta', name: 'Beta', use_when: 'beta meetings' },
  ],
  errors: [],
};
const template = {
  name: 'Standard', use_when: '', title_keywords: [], language: 'auto', context: '',
  sections: [{ title: 'Notes', instruction: 'Write notes' }], style: '',
};
let pendingSave: { resolve: (id: string) => void } | null = null;
let holdBeta = false;
let failBeta = false;
let releaseBeta: (() => void) | null = null;
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'api_list_templates') return listing;
  if (command === 'api_get_template') {
    const id = _args?.templateId;
    if (id === 'beta' && failBeta) throw new Error('load failed');
    const body = id === 'beta' ? { ...template, name: 'Beta' } : template;
    if (id === 'beta' && holdBeta) {
      return new Promise((resolve) => {
        releaseBeta = () => resolve(body);
      });
    }
    return body;
  }
  if (command === 'api_save_template') {
    return new Promise<string>((resolve) => {
      pendingSave = { resolve };
    });
  }
  if (command === 'api_template_revise') return {
    base: template, template: { ...template, name: 'Improved', style: 'Concise' },
    changes: [{ field: 'style', note: 'Make notes concise' }], warnings: [],
  };
  if (command === 'api_template_draft') return { template: { ...template, name: 'AI draft' }, changes: [{ field: 'name', note: 'Created a meeting template' }], warnings: [] };
  if (command === 'api_delete_template') return undefined;
  throw new Error(`Unexpected command: ${command}`);
});
const toast = { success: mock(() => {}), error: mock((_what: string, _opts?: unknown) => {}), info: mock(() => {}), warning: mock(() => {}) };

mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast }));
mock.module('../../src/components/ui/select', () => ({
  Select: passThrough, SelectContent: passThrough, SelectItem: passThrough, SelectTrigger: passThrough, SelectValue: noop,
}));
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: passThrough, DialogContent: passThrough, DialogDescription: passThrough, DialogFooter: passThrough,
  DialogHeader: passThrough, DialogTitle: passThrough,
}));

const { TemplateSettings } = await import('../../src/components/TemplateSettings');

let renderer: ReactTestRenderer | null = null;

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function text(node: ReactTestInstance): string {
  return node.children.map((child) => (typeof child === 'string' ? child : text(child))).join('');
}

function button(label: string): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n) === label)[0];
}

function field(id: string, tag: 'input' | 'textarea'): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === tag && n.props.id === id)[0];
}

async function type(id: string, tag: 'input' | 'textarea', value: string) {
  await act(async () => {
    field(id, tag).props.onChange({ target: { value } });
  });
}

describe('template editor busy state', () => {
  beforeEach(async () => {
    invoke.mockClear();
    pendingSave = null;
    holdBeta = false;
    failBeta = false;
    releaseBeta = null;
    toast.error.mockClear();
    renderer?.unmount();
    renderer = null;
    await act(async () => {
      renderer = create(<TemplateSettings />);
    });
    await flush();
  });

  test('a repeated Save click creates one file and disables the editor while saving', async () => {
    await act(async () => {
      button('New template').props.onClick();
    });
    await type('tpl-name', 'input', 'Fresh');
    await type('tpl-section-0-title', 'input', 'Notes');
    await type('tpl-section-0-instruction', 'textarea', 'Write notes');

    await act(async () => {
      button('Save').props.onClick();
    });
    await act(async () => {
      button('Save').props.onClick();
    });

    const saves = invoke.mock.calls.filter(([command]) => command === 'api_save_template');
    expect(saves.length).toBe(1);
    expect(button('Save').props.disabled).toBe(true);
    expect(button('Duplicate').props.disabled).toBe(true);
    expect(button('Delete').props.disabled).toBe(true);
    expect(button('New template').props.disabled).toBe(true);
    expect(button('Create with AI').props.disabled).toBe(true);
    expect(renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Beta')[0].props.disabled).toBe(true);

    await act(async () => {
      pendingSave?.resolve('fresh');
    });
    await flush();
    expect(button('Save').props.disabled).toBe(false);
    expect(button('New template').props.disabled).toBe(false);
    expect(renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Beta')[0].props.disabled).toBe(false);
  });

  test('a list click during a save is ignored, so the save response lands in the editor it came from', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => true);
    await act(async () => {
      button('New template').props.onClick();
    });
    await type('tpl-name', 'input', 'Fresh');
    await type('tpl-section-0-title', 'input', 'Notes');
    await type('tpl-section-0-instruction', 'textarea', 'Write notes');
    await act(async () => {
      button('Save').props.onClick();
    });
    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Beta')[0].props.onClick();
    });
    await flush();
    expect(invoke.mock.calls.some(([command, args]) => command === 'api_get_template' && args?.templateId === 'beta')).toBe(false);
    await act(async () => {
      pendingSave?.resolve('fresh');
    });
    await flush();
    expect(field('tpl-name', 'input').props.value).toBe('Fresh');
    confirm.mockRestore();
  });

  test('a failed template load clears busy, re-enables the editor and list, and shows the error', async () => {
    failBeta = true;
    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Beta')[0].props.onClick();
    });
    await flush();
    expect(toast.error.mock.calls.map(([what]) => what)).toEqual(['Could not open the template']);
    expect(button('Save').props.disabled).toBe(false);
    expect(button('New template').props.disabled).toBe(false);
    expect(renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Beta')[0].props.disabled).toBe(false);
    expect(renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-current'] === 'true').map((n) => n.children[0])).toEqual(['Standard']);
  });

  test('Save clicked while another template is loading does not write into the loaded template', async () => {
    holdBeta = true;
    await act(async () => {
      renderer!.root.findAll((n) => n.type === 'button' && text(n) === 'Beta')[0].props.onClick();
    });
    await flush();
    expect(button('Save').props.disabled).toBe(true);

    await act(async () => {
      button('Save').props.onClick();
    });
    expect(invoke.mock.calls.filter(([command]) => command === 'api_save_template').length).toBe(0);

    await act(async () => {
      releaseBeta?.();
    });
    await flush();
    const selected = renderer!.root.findAll((n) => n.type === 'button' && n.props['aria-current'] === 'true').map(text);
    expect(selected).toEqual(['Beta']);
    expect(button('Save').props.disabled).toBe(false);

    await act(async () => {
      button('Save').props.onClick();
    });
    const saves = invoke.mock.calls.filter(([command]) => command === 'api_save_template');
    expect(saves.length).toBe(1);
    expect(saves[0][1]).toEqual({ templateId: 'beta', template: expect.anything(), origin: { source: 'manual', note: '' } });
    await act(async () => {
      pendingSave?.resolve('beta');
    });
    await flush();
  });
});

async function createWithAI() {
  await act(async () => { button('Create with AI').props.onClick(); });
  await type('template-assist-request', 'textarea', 'Planning notes');
  await act(async () => { button('Generate').props.onClick(); });
}

describe('AI template draft persistence', () => {
  beforeEach(async () => {
    renderer?.unmount();
    renderer = null;
    invoke.mockClear();
    pendingSave = null;
    holdBeta = false;
    failBeta = false;
    await act(async () => { renderer = create(<TemplateSettings />); });
    await flush();
  });

  test('canceling Create with AI keeps the open template and its dirty draft', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => true);
    try {
      await act(async () => { button('Create with AI').props.onClick(); });
      await act(async () => { button('Cancel').props.onClick(); });
      expect(field('tpl-name', 'input').props.value).toBe('Standard');
      expect(confirm).not.toHaveBeenCalled();
      await type('tpl-name', 'input', 'Unsaved edit');
      await act(async () => { button('Create with AI').props.onClick(); });
      expect(confirm).toHaveBeenCalledWith('Discard unsaved changes?');
      await act(async () => { button('Cancel').props.onClick(); });
      expect(field('tpl-name', 'input').props.value).toBe('Unsaved edit');
      expect(renderer!.root.findAll(n => n.type === 'button' && n.props['aria-current'] === 'true').map(n => n.children[0])).toEqual(['Standard']);
      await act(async () => { button('Save').props.onClick(); });
      expect(invoke.mock.calls.find(([c]) => c === 'api_save_template')?.[1]?.templateId).toBe('standard_meeting');
      await act(async () => { pendingSave?.resolve('standard_meeting'); });
    } finally { confirm.mockRestore(); }
  });

  test('created draft stays unsaved, guards navigation and sends AI origin on first Save only', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => false);
    try {
      await createWithAI();
      expect(field('tpl-name', 'input').props.value).toBe('AI draft');
      expect(confirmLeave()).toBe(false);
      expect(renderer!.root.findAll(n => n.type === 'li').map(text)).toContain('name: Created a meeting template');
      expect(invoke.mock.calls.some(([c]) => c === 'api_save_template')).toBe(false);
      await act(async () => { button('Beta').props.onClick(); });
      expect(confirm).toHaveBeenCalledWith('Discard unsaved changes?');
      expect(field('tpl-name', 'input').props.value).toBe('AI draft');
      await act(async () => { button('Create with AI').props.onClick(); });
      expect(renderer!.root.findAll(n => n.type === 'textarea' && n.props.id === 'template-assist-request')).toHaveLength(0);
      await act(async () => { button('Save').props.onClick(); });
      let saves = invoke.mock.calls.filter(([c]) => c === 'api_save_template');
      expect(saves[0][1]).toEqual({ templateId: null, template: expect.anything(), origin: { source: 'ai', note: 'Created a meeting template' } });
      await act(async () => { pendingSave?.resolve('ai-draft'); });
      await flush();
      await act(async () => { button('Save').props.onClick(); });
      saves = invoke.mock.calls.filter(([c]) => c === 'api_save_template');
      expect(saves[1][1]?.origin).toEqual({ source: 'manual', note: '' });
      await act(async () => { pendingSave?.resolve('ai-draft'); });
    } finally { confirm.mockRestore(); }
  });

  test('a hand edit before first Save changes the origin to manual; loading clears notes', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => true);
    try {
      await createWithAI();
      await type('tpl-name', 'input', 'Hand edited');
      await act(async () => { button('Save').props.onClick(); });
      const saves = invoke.mock.calls.filter(([c]) => c === 'api_save_template');
      expect(saves[0][1]?.origin).toEqual({ source: 'manual', note: '' });
      await act(async () => { pendingSave?.resolve('hand-edited'); });
      await flush();
      await act(async () => { button('Beta').props.onClick(); });
      await flush();
      expect(text(renderer!.root)).not.toContain('What I did');
    } finally { confirm.mockRestore(); }
  });
});

async function improveWithAI() {
  await act(async () => { button('Improve with AI').props.onClick(); });
  await type('template-assist-request', 'textarea', 'Make it concise');
  await act(async () => { button('Generate').props.onClick(); });
  await act(async () => renderer!.root.findByProps({ 'aria-label': 'Name' }).props.onChange({ target: { checked: false } }));
  await act(async () => { button('Accept selected').props.onClick(); });
}

describe('improve draft persistence', () => {
  beforeEach(async () => {
    renderer?.unmount(); invoke.mockClear(); pendingSave = null; holdBeta = false; failBeta = false;
    await act(async () => { renderer = create(<TemplateSettings />); });
    await flush();
  });

  test('dirty disables Improve with its tooltip; accepting selected changes remains unsaved and Save records AI origin', async () => {
    expect(button('Improve with AI').props.disabled).toBe(false);
    await improveWithAI();
    expect(field('tpl-name', 'input').props.value).toBe('Standard');
    expect(field('tpl-style', 'textarea').props.value).toBe('Concise');
    expect(button('Improve with AI').props.disabled).toBe(true);
    expect(button('Improve with AI').props.title).toBe('Save or discard your changes first');
    expect(invoke.mock.calls.filter(([c]) => c === 'api_save_template')).toHaveLength(0);
    await act(async () => { button('Save').props.onClick(); });
    const save = invoke.mock.calls.find(([c]) => c === 'api_save_template');
    expect(save?.[1]?.origin).toEqual({ source: 'ai', note: 'Make notes concise' });
    expect(save?.[1]?.templateId).toBe('standard_meeting');
    await act(async () => { pendingSave?.resolve('standard_meeting'); });
  });

  test('a hand edit after accepting makes the next Save manual', async () => {
    await improveWithAI();
    await type('tpl-name', 'input', 'My notes');
    await act(async () => { button('Save').props.onClick(); });
    expect(invoke.mock.calls.find(([c]) => c === 'api_save_template')?.[1]?.origin).toEqual({ source: 'manual', note: '' });
    await act(async () => { pendingSave?.resolve('standard_meeting'); });
  });
});
