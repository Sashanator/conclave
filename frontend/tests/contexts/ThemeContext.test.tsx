import { describe, expect, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { ThemeProvider } from '../../src/contexts/ThemeContext';

describe('ThemeProvider outside Tauri', () => {
  test('mounts in a plain browser without throwing', () => {
    expect('__TAURI_INTERNALS__' in window).toBe(false);
    expect(() => act(() => { create(<ThemeProvider><span /></ThemeProvider>); })).not.toThrow();
  });
});
