#!/usr/bin/env node
'use strict';

// Started only under flock. GitNexus 1.6.12 emits its readiness message after
// the initial analysis succeeds, not merely after the process has started.
const fs = require('node:fs');
const { spawn } = require('node:child_process');
const marker = process.argv[2];
if (!marker) {
  process.stderr.write('Missing GitNexus readiness marker path.\n');
  process.exit(2);
}
fs.rmSync(marker, { force: true });
const child = spawn('gitnexus', ['analyze', '--watch', '--workers', '2'], {
  stdio: ['ignore', 'pipe', 'pipe'],
});
let ready = false;
function relay(stream, destination) {
  let pending = '';
  stream.on('data', (chunk) => {
    destination.write(chunk);
    pending += chunk.toString();
    const lines = pending.split(/\r?\n/);
    pending = lines.pop();
    for (const line of lines) {
      const plain = line.replace(/\x1b\[[0-9;]*m/g, '');
      if (!ready && /Watching .+; (index is up to date\.|initial index ready in \d+ms\.)/.test(plain)) {
        fs.writeFileSync(marker, String(process.pid));
        ready = true;
      }
    }
  });
}
relay(child.stdout, process.stdout);
relay(child.stderr, process.stderr);
child.on('error', (error) => {
  process.stderr.write(`GitNexus watcher could not start: ${error.message}\n`);
});
child.on('close', (code) => {
  fs.rmSync(marker, { force: true });
  process.exitCode = code || 1;
});
for (const name of ['SIGTERM', 'SIGINT']) {
  process.on(name, () => child.kill(name));
}
