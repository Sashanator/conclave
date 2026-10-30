import { afterAll, beforeEach, describe, expect, mock, spyOn, test } from 'bun:test';
import { cloneElement, createContext, useContext, type ReactElement, type ReactNode } from 'react';
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer';

const originals = {
  core: { ...await import('@tauri-apps/api/core') },
  sonner: { ...await import('sonner') },
  select: { ...await import('../../src/components/ui/select') },
  dialog: { ...await import('../../src/components/ui/dialog') },
  menu: { ...await import('../../src/components/ui/dropdown-menu') },
};

// Bun shares module mocks between test files; restore the application modules after this suite.
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
  mock.module('sonner', () => originals.sonner);
  mock.module('../../src/components/ui/select', () => originals.select);
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
  mock.module('../../src/components/ui/dropdown-menu', () => originals.menu);
});

const passThrough = ({ children }: { children?: ReactNode }) => <>{children}</>;
const noop = () => null;

// Radix menus need a DOM, so the mock opens the menu when its trigger is clicked and renders items inline.
const MenuOpen = createContext<(open: boolean) => void>(() => {});
const MenuRoot = ({ children, onOpenChange }: { children?: ReactNode; onOpenChange?: (open: boolean) => void }) => (
  <MenuOpen.Provider value={onOpenChange ?? (() => {})}>{children}</MenuOpen.Provider>
);
const MenuTrigger = ({ children }: { children: ReactElement<{ onClick?: () => void }> }) => {
  const openChange = useContext(MenuOpen);
  return cloneElement(children, { onClick: () => openChange(true) });
};
const MenuItem = ({ children, onSelect, disabled }: { children?: ReactNode; onSelect?: () => void; disabled?: boolean }) => (
  <button type="button" disabled={disabled} onClick={() => onSelect?.()}>{children}</button>
);

const current = {
  name: 'Standard', use_when: '', title_keywords: [], language: 'auto', context: '',
  sections: [{ title: 'Notes', instruction: 'Write notes' }], style: '',
};
const older = { ...current, name: 'Older name', style: 'Concise' };
const listing = {
  templates: [{ id: 'standard_meeting', name: 'Standard', use_when: null, has_keywords: false }],
  errors: [],
};
const history = [
  { version: '20261030T100000000Z', savedAt: '2026-10-30T10:00:00Z', source: 'manual', note: '' },
  { version: '20261029T100000000Z', savedAt: '2026-10-29T10:00:00Z', source: 'ai', note: 'Make notes concise' },
  { version: '20261028T100000000Z', savedAt: '2026-10-28T10:00:00Z', source: 'restore', note: '' },
  { version: '20261027T100000000Z', savedAt: '2026-10-27T10:00:00Z', source: 'delete', note: '' },
];
const invoke = mock(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
  if (command === 'api_list_templates') return listing;
  if (command === 'api_get_template') return current;
  if (command === 'api_template_history') return history;
  if (command === 'api_template_version') return older;
  if (command === 'api_deleted_templates') return [];
  if (command === 'api_save_template') return 'standard_meeting';
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
mock.module('../../src/components/ui/dropdown-menu', () => ({
  DropdownMenu: MenuRoot, DropdownMenuTrigger: MenuTrigger, DropdownMenuContent: passThrough, DropdownMenuItem: MenuItem,
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

function historyItem(label: string): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === 'button' && text(n).endsWith(` · ${label}`))[0];
}

function field(id: string, tag: 'input' | 'textarea'): ReactTestInstance {
  return renderer!.root.findAll((n) => n.type === tag && n.props.id === id)[0];
}

async function type(id: string, tag: 'input' | 'textarea', value: string) {
  await act(async () => {
    field(id, tag).props.onChange({ target: { value } });
  });
}

describe('template history restore', () => {
  beforeEach(async () => {
    invoke.mockClear();
    toast.error.mockClear();
    renderer?.unmount();
    renderer = null;
    await act(async () => {
      renderer = create(<TemplateSettings />);
    });
    await flush();
  });

  test('history lists the four labels; a dirty draft asks first; Restore fills the draft and only Save writes, with the restore origin', async () => {
    const confirm = spyOn(window, 'confirm').mockImplementation(() => false);
    try {
      await type('tpl-name', 'input', 'Edited');
      await act(async () => { button('History').props.onClick(); });
      await flush();
      for (const label of ['Before manual edit', 'Before AI change: Make notes concise', 'Before restore', 'Deleted']) {
        expect(historyItem(label)).toBeDefined();
      }

      // Declined: nothing is loaded and the unsaved draft stays.
      await act(async () => { historyItem('Before manual edit').props.onClick(); });
      await flush();
      expect(confirm).toHaveBeenCalledWith('Discard unsaved changes?');
      expect(invoke.mock.calls.some(([command]) => command === 'api_template_version')).toBe(false);
      expect(field('tpl-name', 'input').props.value).toBe('Edited');

      // Accepted: the version opens for review, and Restore only fills the editor.
      confirm.mockImplementation(() => true);
      await act(async () => { historyItem('Before manual edit').props.onClick(); });
      await flush();
      expect(invoke.mock.calls.find(([command]) => command === 'api_template_version')?.[1])
        .toEqual({ templateId: 'standard_meeting', version: '20261030T100000000Z' });
      await act(async () => { button('Restore').props.onClick(); });
      await flush();
      expect(field('tpl-name', 'input').props.value).toBe('Older name');
      expect(invoke.mock.calls.some(([command]) => command === 'api_save_template')).toBe(false);
      expect(button('Save').props.disabled).toBe(false);

      await act(async () => { button('Save').props.onClick(); });
      await flush();
      const save = invoke.mock.calls.find(([command]) => command === 'api_save_template');
      expect(save?.[1]).toEqual({
        templateId: 'standard_meeting',
        template: expect.objectContaining({ name: 'Older name' }),
        origin: { source: 'restore', note: '' },
      });
    } finally {
      confirm.mockRestore();
    }
  });
});
