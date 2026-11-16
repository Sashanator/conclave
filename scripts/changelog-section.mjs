import { readFileSync } from 'node:fs';

const [ver, path = 'CHANGELOG.md'] = process.argv.slice(2);
const fail = () => { console.error(`changelog-section: no section for v${ver}`); process.exit(1); };
if (!/^\d+\.\d+\.\d+$/.test(ver ?? '')) fail();
const lines = readFileSync(path, 'utf8').split(/\r?\n/);
// Exact match on the heading's version token, so 1.2.0 never matches v1.2.01.
const start = lines.findIndex((l) => l.startsWith(`## v${ver} `));
if (start < 0) fail();
let end = lines.findIndex((l, i) => i > start && l.startsWith('## '));
if (end < 0) end = lines.length;
const body = lines.slice(start + 1, end).join('\n').trim();
if (!body) fail();
console.log(body);
