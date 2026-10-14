import { describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

// No restore: importing the real window module can fail when a sibling test leaves core mocked, and no other test imports it.
const outsideTauri = () => { throw new TypeError("Cannot read properties of undefined (reading 'metadata')"); };
let currentWindow: () => unknown = outsideTauri;
mock.module('@tauri-apps/api/window', () => ({ getCurrentWindow: () => currentWindow() }));
const { ThemeProvider } = await import('../../src/contexts/ThemeContext');

describe('ThemeProvider outside Tauri', () => {
  test('mounts in a plain browser without throwing', () => {
    expect('__TAURI_INTERNALS__' in window).toBe(false);
    expect(() => act(() => { create(<ThemeProvider><span /></ThemeProvider>); })).not.toThrow();
  });
});

describe('ThemeProvider inside Tauri', () => {
  test('the native title bar follows the stored choice (conclave is dark)', () => {
    const setTheme: unknown[] = [];
    (window as any).__TAURI_INTERNALS__ = {};
    currentWindow = () => ({ setTheme: (t: unknown) => { setTheme.push(t); return Promise.resolve(); } });
    localStorage.setItem('theme', 'conclave');
    try {
      act(() => { create(<ThemeProvider><span /></ThemeProvider>); });
      expect(setTheme).toEqual(['dark']);
    } finally {
      delete (window as any).__TAURI_INTERNALS__;
      currentWindow = outsideTauri;
      localStorage.removeItem('theme');
    }
  });
});
