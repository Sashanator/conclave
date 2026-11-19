import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC = join(import.meta.dir, '..', '..', 'src');

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return sourceFiles(p);
    return /\.tsx?$/.test(name) ? [p] : [];
  });
}

const files = sourceFiles(SRC).map((f) => [relative(SRC, f).split('\\').join('/'), readFileSync(f, 'utf8')] as const);

describe('confirmation guard', () => {
  test('no source file calls the browser confirm', () => {
    const bad = files
      .filter(([rel]) => rel !== 'lib/confirm.ts') // the no-host fallback
      .filter(([, text]) => /window\.confirm\s*\(|(^|[^.\w])confirm\s*\(/m.test(text))
      .map(([rel]) => rel);
    expect(bad).toEqual([]);
  });

  test('no red filled Delete button', () => {
    const bad = files.filter(([, text]) => /<Button[^>]*variant="destructive"/.test(text)).map(([rel]) => rel);
    expect(bad).toEqual([]);
  });
});
