import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = join(dirname(fileURLToPath(import.meta.url)), 'set-version.mjs');

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'setver-'));
  mkdirSync(join(root, 'frontend/src-tauri'), { recursive: true });
  writeFileSync(join(root, 'frontend/src-tauri/tauri.conf.json'), JSON.stringify({ productName: 'Conclave', version: '0.4.1' }, null, 4) + '\n');
  writeFileSync(join(root, 'frontend/package.json'), JSON.stringify({ name: 'meetily', version: '0.4.1' }, null, 4) + '\n');
  writeFileSync(join(root, 'frontend/src-tauri/Cargo.toml'),
    '[package]\nname = "meetily"\nversion = "0.4.1"\n\n[dependencies]\nfoo = { version = "1.2.3" }\nbar = "0.4.1"\n');
  return root;
}

test('stamps version into all three files', () => {
  const root = fixture();
  execFileSync('node', [script, '1.2.3', root]);
  assert.equal(JSON.parse(readFileSync(join(root, 'frontend/src-tauri/tauri.conf.json'), 'utf8')).version, '1.2.3');
  assert.equal(JSON.parse(readFileSync(join(root, 'frontend/package.json'), 'utf8')).version, '1.2.3');
  const cargo = readFileSync(join(root, 'frontend/src-tauri/Cargo.toml'), 'utf8');
  assert.match(cargo, /^\[package\]\nname = "meetily"\nversion = "1\.2\.3"$/m);
  assert.match(cargo, /foo = \{ version = "1\.2\.3" \}/); // untouched dependency stays as-is
  assert.match(cargo, /bar = "0\.4\.1"/);                  // dependency with same old version untouched
});

test('rejects a non X.Y.Z version and changes nothing', () => {
  const root = fixture();
  const r = spawnSync('node', [script, 'v1.2', root]);
  assert.equal(r.status, 1);
  assert.equal(JSON.parse(readFileSync(join(root, 'frontend/package.json'), 'utf8')).version, '0.4.1');
});
