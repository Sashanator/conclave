import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'rel-'));
const file = (n, s) => { const p = join(dir, n); writeFileSync(p, s); return p; };
const run = (s, ...a) => spawnSync('node', [join(here, s), ...a], { encoding: 'utf8' });
const URL_OK = 'https://api.github.com/repos/Sashanator/conclave/releases/assets/12345';

const cl = file('cl.md', '# Conclave changelog\n\n## v1.2.01 — 2026-01-01\n- wrong\n\n## v1.2.0 — 2026-01-02\n- one\n- two\n\n## v1.1.0 — 2026-01-01\n- old\n\n## v1.0.0 — 2026-01-01\n\n');

test('section_is_extracted_between_headings', () => {
  const r = run('changelog-section.mjs', '1.2.0', cl);
  assert.equal(r.status, 0);
  assert.equal(r.stdout, '- one\n- two\n');
});

test('prefix_versions_do_not_match', () => {
  assert.equal(run('changelog-section.mjs', '1.2.0', cl).stdout.includes('wrong'), false);
  assert.equal(run('changelog-section.mjs', '1.2.', cl).status, 1);
  assert.equal(run('changelog-section.mjs', '1.2', cl).status, 1);
});

test('missing_or_empty_section_fails', () => {
  for (const v of ['9.9.9', '1.0.0', 'x']) {
    const r = run('changelog-section.mjs', v, cl);
    assert.equal(r.status, 1);
    assert.match(r.stderr, new RegExp(`changelog-section: no section for v${v}`));
  }
});

test('latest_json_has_windows_platform', () => {
  const r = run('latest-json.mjs', '1.2.0', file('n.md', 'notes\n'), file('s.sig', 'SIG\n'), URL_OK);
  assert.equal(r.status, 0);
  const j = JSON.parse(r.stdout);
  assert.equal(j.version, '1.2.0');
  assert.equal(j.notes, 'notes');
  assert.ok(!Number.isNaN(Date.parse(j.pub_date)));
  assert.deepEqual(j.platforms['windows-x86_64'], { signature: 'SIG', url: URL_OK });
});

test('latest_json_rejects_browser_url', () => {
  const r = run('latest-json.mjs', '1.2.0', file('n.md', 'n'), file('s.sig', 'SIG'),
    'https://github.com/Sashanator/conclave/releases/download/v1.2.0/Conclave_1.2.0_x64-setup.exe');
  assert.equal(r.status, 1);
});

test('latest_json_rejects_foreign_repo_and_empty_sig', () => {
  const n = file('n.md', 'n');
  assert.equal(run('latest-json.mjs', '1.2.0', n, file('s.sig', 'SIG'),
    'https://api.github.com/repos/evil/conclave/releases/assets/1').status, 1);
  assert.equal(run('latest-json.mjs', '1.2.0', n, file('e.sig', ' \n'), URL_OK).status, 1);
  assert.equal(run('latest-json.mjs', '1.2', n, file('s.sig', 'SIG'), URL_OK).status, 1);
});

test('real_changelog_has_current_release', () => {
  const r = run('changelog-section.mjs', '2.2.0', join(here, '..', 'CHANGELOG.md'));
  assert.equal(r.status, 0);
  assert.ok(r.stdout.trim().startsWith('- '));
});
