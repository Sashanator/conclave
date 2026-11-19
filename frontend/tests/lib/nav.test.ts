import { afterEach, describe, expect, spyOn, test } from 'bun:test';
import { NAV_MAIN, readSidebarCollapsed, writeSidebarCollapsed, isNavActive, recordLabel, whatsNewUnseen } from '../../src/lib/nav';

describe('nav', () => {
  test('main items', () => {
    expect(NAV_MAIN.map((i) => i.label)).toEqual(['Meetings', 'Tasks', 'Digests', 'Dictate', 'Practice', 'Stats']);
  });
  test('isNavActive', () => {
    expect(isNavActive('/meeting-details', NAV_MAIN[0])).toBe(true);
    expect(isNavActive('/', NAV_MAIN[0])).toBe(true);
    expect(isNavActive('/ask', NAV_MAIN[0])).toBe(false);
    expect(isNavActive('/tasks', NAV_MAIN[1])).toBe(true);
    expect(isNavActive('/tasks', NAV_MAIN[0])).toBe(false);
  });
  test('recordLabel', () => {
    expect(recordLabel(false, null)).toBe('Record new meeting');
    expect(recordLabel(true, 192)).toBe('Recording 03:12');
    expect(recordLabel(true, null)).toBe('Recording 00:00');
  });
  test('whatsNewUnseen', () => {
    expect(whatsNewUnseen(null, '1.3.0')).toBe(true);
    expect(whatsNewUnseen('1.3.0', '1.3.0')).toBe(false);
    expect(whatsNewUnseen(null, null)).toBe(false);
  });
});

describe('sidebar state', () => {
  afterEach(() => localStorage.clear());
  test('nothing stored: expanded from 1600 px', () => {
    localStorage.clear();
    expect(readSidebarCollapsed(1600)).toBe(false);
    expect(readSidebarCollapsed(1599)).toBe(true);
  });
  test('stored value wins', () => {
    localStorage.setItem('conclave.sidebarCollapsed', 'true');
    expect(readSidebarCollapsed(2560)).toBe(true);
    localStorage.setItem('conclave.sidebarCollapsed', 'false');
    expect(readSidebarCollapsed(1366)).toBe(false);
  });
  test('storage throwing falls back to the width default', () => {
    const spy = spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    try { expect(readSidebarCollapsed(2048)).toBe(false); } finally { spy.mockRestore(); }
  });
  test('write stores the value', () => {
    writeSidebarCollapsed(true);
    expect(localStorage.getItem('conclave.sidebarCollapsed')).toBe('true');
  });
});
