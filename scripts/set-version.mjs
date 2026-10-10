// Usage: node scripts/set-version.mjs X.Y.Z [repoRoot]
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [version, root = process.cwd()] = process.argv.slice(2);
if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version ?? '')) {
  console.error(`set-version: expected X.Y.Z, got '${version}'`);
  process.exit(1);
}

// Keep each file's indentation and trailing newline so the diff shows only the version change.
const jsonWrites = ['frontend/src-tauri/tauri.conf.json', 'frontend/package.json'].map((rel) => {
  const path = join(root, rel);
  const text = readFileSync(path, 'utf8');
  const json = JSON.parse(text);
  json.version = version;
  const indent = text.match(/^([ \t]+)"/m)?.[1] ?? 2;
  return [path, JSON.stringify(json, null, indent) + (text.endsWith('\n') ? '\n' : '')];
});

// Only the version line inside [package], never dependency versions.
const cargoPath = join(root, 'frontend/src-tauri/Cargo.toml');
const cargo = readFileSync(cargoPath, 'utf8');
const cargoVersion = /(\[package\][^\[]*?^version = ")[^"]*(")/m;
if (!cargoVersion.test(cargo)) {
  console.error('set-version: [package] version line not found in Cargo.toml');
  process.exit(1);
}

for (const [path, text] of jsonWrites) writeFileSync(path, text);
writeFileSync(cargoPath, cargo.replace(cargoVersion, (_, head, tail) => head + version + tail));
console.log(`set-version: ${version}`);
