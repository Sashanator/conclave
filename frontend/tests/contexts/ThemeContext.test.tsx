import { describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

// No restore: importing the real window module can fail when a sibling test leaves core mocked, and no other test imports it.
const outsideTauri = () => { throw new TypeError("Cannot read properties of undefined (reading 'metadata')"); };
mock.module('@tauri-apps/api/window', () => ({ getCurrentWindow: outsideTauri }));
const { ThemeProvider } = await import('../../src/contexts/ThemeContext');

describe('ThemeProvider outside Tauri', () => {
  test('mounts in a plain browser without throwing', () => {
    expect('__TAURI_INTERNALS__' in window).toBe(false);
    expect(() => act(() => { create(<ThemeProvider><span /></ThemeProvider>); })).not.toThrow();
  });
});
