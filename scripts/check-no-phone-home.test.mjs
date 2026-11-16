import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, copyFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
// Single source of truth: the key shipped in the real config.
const KEY = JSON.parse(readFileSync(join(here, '../frontend/src-tauri/tauri.conf.json'), 'utf8')).plugins.updater.pubkey;
const API = 'https://api.github.com/repos/Sashanator/conclave/releases';

// Builds a temp repo root with the script copied in; `overrides` maps a root-relative path to its content.
function fixture(overrides = {}) {
  const root = mkdtempSync(join(tmpdir(), 'guard-'));
  const files = {
    'frontend/src/a.ts': 'export const a = 1;\n',
    'frontend/src-tauri/src/lib.rs': 'fn main() {}\n',
    'frontend/src-tauri/src/updates/mod.rs': `fn f(url: String) { b.endpoints(vec![url]); }\nconst API: &str = "${API}";\n`,
    'frontend/src-tauri/Cargo.toml': '[dependencies]\ntauri-plugin-updater = "2"\n',
    'frontend/src-tauri/tauri.conf.json': JSON.stringify({ plugins: { updater: { pubkey: KEY } } }),
    ...overrides,
  };
  for (const [p, c] of Object.entries(files)) {
    mkdirSync(dirname(join(root, p)), { recursive: true });
    writeFileSync(join(root, p), c);
  }
  mkdirSync(join(root, 'scripts'));
  copyFileSync(join(here, 'check-no-phone-home.sh'), join(root, 'scripts/check-no-phone-home.sh'));
  return root;
}

function run(overrides) {
  const r = spawnSync('bash', [join(fixture(overrides), 'scripts/check-no-phone-home.sh')], { encoding: 'utf8' });
  return { code: r.status, out: r.stdout + r.stderr };
}

const conf = (o) => JSON.stringify(o);

test('passes_with_conclave_updater_config', () => {
  const r = run();
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /clean/);
});

test('fails_with_foreign_pubkey', () => {
  const r = run({ 'frontend/src-tauri/tauri.conf.json': conf({ plugins: { updater: { pubkey: 'AAAA' } } }) });
  assert.equal(r.code, 1);
  assert.match(r.out, /FOREIGN updater pubkey/);
});

test('fails_with_config_endpoints', () => {
  const r = run({ 'frontend/src-tauri/tauri.conf.json': conf({ plugins: { updater: { pubkey: KEY, endpoints: ['https://x.example/latest.json'] } } }) });
  assert.equal(r.code, 1);
  assert.match(r.out, /"endpoints"/);
});

test('fails_with_endpoints_call_outside_updates', () => {
  const r = run({ 'frontend/src-tauri/src/lib.rs': 'fn f() { b.endpoints(vec![]); }\n' });
  assert.equal(r.code, 1);
  assert.match(r.out, /updater endpoints outside src\/updates/);
});

test('fails_with_foreign_url_in_updates', () => {
  const r = run({ 'frontend/src-tauri/src/updates/mod.rs': 'const U: &str = "https://evil.example/x";\n' });
  assert.equal(r.code, 1);
  assert.match(r.out, /FOREIGN URL/);
});

test('fails_with_runtime_pubkey_override', () => {
  for (const p of ['frontend/src-tauri/src/lib.rs', 'frontend/src-tauri/src/updates/mod.rs']) {
    const r = run({ [p]: 'fn f() { b.pubkey("AAAA"); }\n' });
    assert.equal(r.code, 1, p);
    assert.match(r.out, /pubkey/);
  }
});

test('fails_with_webview_updater_permission', () => {
  const r = run({ 'frontend/src-tauri/capabilities/main.json': '{"permissions":["updater:default"]}' });
  assert.equal(r.code, 1);
  assert.match(r.out, /updater:/);
});

test('urls_in_test_module_are_ignored', () => {
  const r = run({ 'frontend/src-tauri/src/updates/mod.rs': `const API: &str = "${API}";\n#[cfg(test)]\nmod t { const U: &str = "https://evil.example/x"; }\n` });
  assert.equal(r.code, 0, r.out);
});

test('still_fails_on_meetily_latest_json', () => {
  const r = run({ 'frontend/src/a.ts': 'fetch("https://github.com/x/y/releases/latest/download/latest.json");\n' });
  assert.equal(r.code, 1);
  assert.match(r.out, /latest/);
});

const fails = (overrides, re) => {
  const r = run(overrides);
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, re);
};

test('fails_with_foreign_pubkey_in_platform_config', () =>
  fails({ 'frontend/src-tauri/tauri.windows.conf.json': conf({ plugins: { updater: { pubkey: 'AAAA' } } }) }, /FOREIGN updater pubkey/));

test('fails_with_endpoints_in_platform_config', () =>
  fails({ 'frontend/src-tauri/tauri.windows.conf.json': conf({ plugins: { updater: { endpoints: ['https://x.example/l.json'] } } }) }, /endpoints/));

test('fails_with_toml_config_pubkey_and_endpoints', () => {
  fails({ 'frontend/src-tauri/Tauri.toml': '[plugins.updater]\npubkey = "AAAA"\n' }, /FOREIGN updater pubkey/);
  fails({ 'frontend/src-tauri/Tauri.toml': '[plugins.updater]\nendpoints = ["https://x.example"]\n' }, /endpoints in Tauri config/);
});

test('fails_with_workflow_updater_config', () => {
  for (const l of ['TAURI_CONFIG: "{}"', 'args: --config {"plugins":{"updater":{"pubkey":"x"}}}', 'x: endpoints']) {
    fails({ '.github/workflows/r.yml': `run: ${l}\n` }, /updater config in workflows/);
  }
});

test('item_level_cfg_test_does_not_hide_urls', () => {
  for (const item of ['use std::fmt;', 'mod tests;']) {
    fails({ 'frontend/src-tauri/src/updates/mod.rs': `#[cfg(test)]\n${item}\nconst E: &str = "https://evil.example/x";\n` }, /FOREIGN URL/);
  }
  fails({ 'frontend/src-tauri/src/updates/mod.rs': '#[cfg(test)]\nconst E: &str = "https://evil.example/x";\n' }, /FOREIGN URL/);
});

test('test_module_on_one_line_or_two_stops_scan', () => {
  for (const head of ['#[cfg(test)] mod t {', '#[cfg(test)]\nmod t {']) {
    const r = run({ 'frontend/src-tauri/src/updates/mod.rs': `const A: &str = "${API}";\n${head}\n const U: &str = "https://evil.example/x";\n}\n` });
    assert.equal(r.code, 0, r.out);
  }
});

test('fails_with_foreign_url_in_updates_subdirectory', () =>
  fails({ 'frontend/src-tauri/src/updates/sub/c.rs': 'const C: &str = "https://evil.example/sub";\n' }, /FOREIGN URL/));

test('fails_with_uppercase_scheme_or_host_and_lookalike_hosts', () => {
  for (const u of ['HTTPS://evil.example/x', 'https://api.github.com@evil.example/repos/Sashanator/conclave/x', 'https://api.github.com.evil.example/repos/Sashanator/conclave/x', 'https://api.github.com/repos/other/conclave/x']) {
    fails({ 'frontend/src-tauri/src/updates/mod.rs': `const U: &str = "${u}";\n` }, /FOREIGN URL/);
  }
  const r = run({ 'frontend/src-tauri/src/updates/mod.rs': `const U: &str = "HTTPS://API.GITHUB.COM/repos/Sashanator/conclave/x";\n` });
  assert.equal(r.code, 0, r.out);
});

test('fails_with_endpoints_call_in_other_dir_named_updates', () =>
  fails({ 'frontend/src-tauri/src/practice/updates/x.rs': 'fn f() { b.endpoints(vec![]); }\n' }, /updater endpoints outside src\/updates/));

test('fails_with_toml_capability_updater_permission', () =>
  fails({ 'frontend/src-tauri/capabilities/main.toml': "permissions = ['updater:default']\n" }, /updater:/));
