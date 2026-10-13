import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC = join(import.meta.dir, '..', 'src');

// Files not yet moved to theme tokens. Tasks 7-9 of the Phase 2 plan remove their files; Task 9 leaves this empty.
const PENDING = new Set<string>([]);

// Neutral colours must be tokens (bg-background, text-muted-foreground, border-border…), never hard-coded.
const NEUTRAL =
  /(?<!dark:)\b(?:bg-white|ring-offset-white|border-[trblxyse]-(?:gray|slate|zinc|neutral|stone)-\d{2,3}|(?:bg|text|border|divide|ring|ring-offset|placeholder|from|via|to|fill|stroke|outline)-(?:gray|slate|zinc|neutral|stone)-\d{2,3})\b/;
// SVG strokes and fills hard-coded to Tailwind greys (stroke="#e5e7eb").
const HEX_GREY = /\b(?:stroke|fill)="#(?:e5e7eb|d1d5db|9ca3af|6b7280|4b5563|374151|1f2937|f3f4f6|f9fafb)"/i;
// Light tints of a hue need a dark: partner on the same line (e.g. bg-blue-50 dark:bg-blue-950/40).
const HUES = 'red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose';
const TINT = new RegExp(`(?<!dark:)\\b(?:bg-(?:${HUES})-(?:50|100|200)|text-(?:${HUES})-(?:700|800|900)|border-(?:${HUES})-(?:100|200|300))\\b`);

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return sourceFiles(p);
    return /\.(tsx?|css)$/.test(name) ? [p] : [];
  });
}

function offenders(file: string): string[] {
  return readFileSync(file, 'utf8')
    .split('\n')
    .flatMap((line, i) => (NEUTRAL.test(line) || HEX_GREY.test(line) || (TINT.test(line) && !line.includes('dark:')) ? [`${i + 1}: ${line.trim()}`] : []));
}

describe('theme guard', () => {
  const files = sourceFiles(SRC).map((f) => [relative(SRC, f).split('\\').join('/'), f] as const);

  test('converted files use theme tokens', () => {
    const bad = files.filter(([rel]) => !PENDING.has(rel)).flatMap(([rel, f]) => offenders(f).map((o) => `${rel}:${o}`));
    expect(bad).toEqual([]);
  });

  test('every pending file still needs converting (keep the list honest)', () => {
    const stale = [...PENDING].filter((rel) => {
      const hit = files.find(([r]) => r === rel);
      return !hit || offenders(hit[1]).length === 0;
    });
    expect(stale).toEqual([]);
  });
});
