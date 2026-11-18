import { describe, expect, test } from 'bun:test';
import { moveSelection, tableKeyAction, type TableKeyEvent } from '../../src/lib/meetings-keys';

const event = (key: string, overrides: Partial<TableKeyEvent> = {}): TableKeyEvent => ({
  key,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...overrides,
});

describe('tableKeyAction', () => {
  const normalCtx = { inEditable: false, searching: false };
  const searchCtx = { inEditable: false, searching: true };
  const editableCtx = { inEditable: true, searching: false };
  const editableSearchCtx = { inEditable: true, searching: true };

  describe('navigation and table actions (outside editable)', () => {
    test('ArrowDown maps to down', () => {
      expect(tableKeyAction(event('ArrowDown'), normalCtx)).toBe('down');
    });

    test('ArrowUp maps to up', () => {
      expect(tableKeyAction(event('ArrowUp'), normalCtx)).toBe('up');
    });

    test('Enter maps to open', () => {
      expect(tableKeyAction(event('Enter'), normalCtx)).toBe('open');
    });

    test('space maps to toggle-reviewed', () => {
      expect(tableKeyAction(event(' '), normalCtx)).toBe('toggle-reviewed');
    });

    test('/ maps to focus-search', () => {
      expect(tableKeyAction(event('/'), normalCtx)).toBe('focus-search');
    });
  });

  describe('Ctrl N (new-meeting)', () => {
    test('a non-Latin layout still matches by physical key', () => {
      expect(tableKeyAction(event('т', { ctrlKey: true, code: 'KeyN' }), normalCtx)).toBe('new-meeting');
    });
    test('Ctrl N maps to new-meeting outside editable', () => {
      expect(tableKeyAction(event('n', { ctrlKey: true }), normalCtx)).toBe('new-meeting');
      expect(tableKeyAction(event('N', { ctrlKey: true }), normalCtx)).toBe('new-meeting');
    });

    test('Ctrl N maps to new-meeting inside editable', () => {
      expect(tableKeyAction(event('n', { ctrlKey: true }), editableCtx)).toBe('new-meeting');
      expect(tableKeyAction(event('n', { ctrlKey: true }), editableSearchCtx)).toBe('new-meeting');
    });

    test('Meta N also maps to new-meeting', () => {
      expect(tableKeyAction(event('n', { metaKey: true }), normalCtx)).toBe('new-meeting');
      expect(tableKeyAction(event('n', { metaKey: true }), editableCtx)).toBe('new-meeting');
    });

    test('Ctrl+Shift+N maps to null', () => {
      expect(tableKeyAction(event('n', { ctrlKey: true, shiftKey: true }), normalCtx)).toBeNull();
      expect(tableKeyAction(event('N', { ctrlKey: true, shiftKey: true }), normalCtx)).toBeNull();
      expect(tableKeyAction(event('n', { ctrlKey: true, shiftKey: true }), editableCtx)).toBeNull();
    });

    test('Ctrl J maps to null (Task 26 owns it)', () => {
      expect(tableKeyAction(event('j', { ctrlKey: true }), normalCtx)).toBeNull();
      expect(tableKeyAction(event('J', { ctrlKey: true }), normalCtx)).toBeNull();
      expect(tableKeyAction(event('j', { ctrlKey: true }), editableCtx)).toBeNull();
    });
  });

  describe('Escape (clear-search)', () => {
    test('Escape maps to clear-search only while searching', () => {
      expect(tableKeyAction(event('Escape'), searchCtx)).toBe('clear-search');
      expect(tableKeyAction(event('Escape'), editableSearchCtx)).toBe('clear-search');
    });

    test('Escape maps to null when not searching', () => {
      expect(tableKeyAction(event('Escape'), normalCtx)).toBeNull();
      expect(tableKeyAction(event('Escape'), editableCtx)).toBeNull();
    });
  });

  describe('editable context restrictions', () => {
    test('non-Ctrl keys are null while editable except Escape', () => {
      expect(tableKeyAction(event('ArrowDown'), editableCtx)).toBeNull();
      expect(tableKeyAction(event('ArrowUp'), editableCtx)).toBeNull();
      expect(tableKeyAction(event('Enter'), editableCtx)).toBeNull();
      expect(tableKeyAction(event(' '), editableCtx)).toBeNull();
      expect(tableKeyAction(event('/'), editableCtx)).toBeNull();
      expect(tableKeyAction(event('a'), editableCtx)).toBeNull();
    });

    test('Escape is still clear-search while editable if searching', () => {
      expect(tableKeyAction(event('Escape'), editableSearchCtx)).toBe('clear-search');
    });
  });

  describe('unmapped and modified keys', () => {
    test('unmapped keys return null', () => {
      expect(tableKeyAction(event('Tab'), normalCtx)).toBeNull();
      expect(tableKeyAction(event('x'), normalCtx)).toBeNull();
    });

    test('Alt combinations return null', () => {
      expect(tableKeyAction(event('ArrowDown', { altKey: true }), normalCtx)).toBeNull();
      expect(tableKeyAction(event('n', { ctrlKey: true, altKey: true }), normalCtx)).toBeNull();
    });
  });
});

describe('moveSelection', () => {
  test('clamps selection within [0, count - 1]', () => {
    expect(moveSelection(0, -1, 5)).toBe(0);
    expect(moveSelection(4, 1, 5)).toBe(4);
    expect(moveSelection(-1, 1, 5)).toBe(0);
  });

  test('moves selection forwards and backwards within bounds', () => {
    expect(moveSelection(1, 1, 5)).toBe(2);
    expect(moveSelection(3, -1, 5)).toBe(2);
  });

  test('returns -1 for empty list', () => {
    expect(moveSelection(0, 1, 0)).toBe(-1);
    expect(moveSelection(-1, 1, 0)).toBe(-1);
  });
});
