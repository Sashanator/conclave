import { afterAll, describe, expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';

// bun's mock.module is process-global: a sibling test that mocks @tauri-apps/api/core breaks the real window module.
// Stub the window module with the outside-Tauri throw, and restore the original afterwards.
const outsideTauri = () => { throw new TypeError("Cannot read properties of undefined (reading 'metadata')"); };
let originalWindow: Record<string, unknown>;
try {
  originalWindow = { ...await import('@tauri-apps/api/window') };
} catch {
  originalWindow = { getCurrentWindow: outsideTauri };
}
afterAll(() => mock.module('@tauri-apps/api/window', () => originalWindow));
mock.module('@tauri-apps/api/window', () => ({ getCurrentWindow: outsideTauri }));
const { ThemeProvider } = await import('../../src/contexts/ThemeContext');

describe('ThemeProvider outside Tauri', () => {
  test('mounts in a plain browser without throwing', () => {
    expect('__TAURI_INTERNALS__' in window).toBe(false);
    expect(() => act(() => { create(<ThemeProvider><span /></ThemeProvider>); })).not.toThrow();
  });
});
