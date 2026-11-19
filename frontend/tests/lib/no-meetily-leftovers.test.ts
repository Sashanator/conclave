import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const SRC = join(import.meta.dir, '..', '..', 'src');
const LEFTOVER =
  /Test Mic|Auto Detect may produce|completion\(Stopping\)|never leaves your device|no cloud required|Summarization Engine|<strong>Microphone:<\/strong>|<strong>System Audio:<\/strong>/;

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return sourceFiles(p);
    return /\.tsx?$/.test(name) ? [p] : [];
  });
}

describe('Meetily leftovers', () => {
  test('no Meetily help string remains in src', () => {
    const bad = sourceFiles(SRC).filter((f) => LEFTOVER.test(readFileSync(f, 'utf8'))).map((f) => relative(SRC, f));
    expect(bad).toEqual([]);
  });
});
