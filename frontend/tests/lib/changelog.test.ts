import { describe, expect, test } from 'bun:test';
import { parseChangelog } from '../../src/lib/changelog';

const MD = `# Conclave changelog

Some prose.

## Unreleased
- not yet

## v1.1.0 — 2026-10-30
- Older thing.

## v2.0.0 — 2026-11-17
- **Meetings table.** All meetings in one table.
- Second.
Prose line.
`;

describe('parseChangelog', () => {
  test('newest first, skips Unreleased and prose', () => {
    const r = parseChangelog(MD);
    expect(r.map((x) => x.version)).toEqual(['2.0.0', '1.1.0']);
    expect(r[0].date).toBe('2026-11-17');
    expect(r[0].items).toEqual(['**Meetings table.** All meetings in one table.', 'Second.']);
  });
  test('1.10.0 sorts above 1.9.0', () => {
    const r = parseChangelog('## v1.9.0 — a\n- x\n## v1.10.0 — b\n- y\n');
    expect(r.map((x) => x.version)).toEqual(['1.10.0', '1.9.0']);
  });
  test('a major bump sorts above 1.10.0', () => {
    const r = parseChangelog('## v1.10.0 — a\n- x\n## v2.0.0 — b\n- y\n## v1.2.0 — c\n- z\n');
    expect(r.map((x) => x.version)).toEqual(['2.0.0', '1.10.0', '1.2.0']);
  });
});
