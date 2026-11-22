import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const css = readFileSync(join(import.meta.dir, '../../src/app/globals.css'), 'utf8');
const block = (open: string) => {
  const start = css.indexOf(open);
  return css.slice(start, css.indexOf('}', start));
};
const blocks: Record<string, string> = { root: block(':root {'), dark: block('.dark {'), 'theme-conclave': block('.theme-conclave {') };
const token = (b: string, name: string) => {
  const m = blocks[b].match(new RegExp(`--${name}:\\s*([\\d.]+)\\s+([\\d.]+)%\\s+([\\d.]+)%;`));
  if (!m) throw new Error(`--${name} missing in ${b}`);
  return [Number(m[1]), Number(m[2]) / 100, Number(m[3]) / 100] as const;
};
const luminance = ([h, s, l]: readonly [number, number, number]) => {
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => { const k = (n + h / 30) % 12; return l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)); };
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(f(0)) + 0.7152 * lin(f(8)) + 0.0722 * lin(f(4));
};
const contrast = (a: readonly [number, number, number], b: readonly [number, number, number]) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};
const raw = (b: string, name: string) => blocks[b].match(new RegExp(`--${name}:\\s*([^;]+);`))?.[1];

describe('destination colour tokens', () => {
  test('every destination colour has >= 3:1 against the background in all three themes', () => {
    const bad: [string, number, boolean][] = [];
    for (const b of Object.keys(blocks)) {
      for (let n = 1; n <= 9; n++) if (contrast(token(b, `db-${n}`), token(b, 'background')) < 3) bad.push([b, n, true]);
    }
    expect(bad).toEqual([]);
  });
  test('spec values for 1-3', () => {
    expect(raw('root', 'db-1')).toBe('340 75% 50%');
    expect(raw('dark', 'db-2')).toBe('190 85% 45%');
    expect(raw('theme-conclave', 'db-3')).toBe('85 60% 45%');
  });
});
