#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

const SESSIONS_DIR = path.join(process.env.HOME || '/home/node', '.claude/sessions');
const LOCK_FILE = '/tmp/claude-title-sync.pid';

// Single instance check
try {
  if (fs.existsSync(LOCK_FILE)) {
    const existingPid = parseInt(fs.readFileSync(LOCK_FILE, 'utf8').trim(), 10);
    if (!isNaN(existingPid) && existingPid !== process.pid) {
      try {
        process.kill(existingPid, 0);
        // Another instance is already running
        process.exit(0);
      } catch (_) {
        // Stale lock file, continue
      }
    }
  }
} catch (_) {}

try {
  fs.writeFileSync(LOCK_FILE, String(process.pid), 'utf8');
} catch (_) {}

process.on('exit', () => {
  try {
    if (fs.existsSync(LOCK_FILE)) {
      const p = parseInt(fs.readFileSync(LOCK_FILE, 'utf8').trim(), 10);
      if (p === process.pid) fs.unlinkSync(LOCK_FILE);
    }
  } catch (_) {}
});

const lastTitles = new Map(); // pid -> title

function updateTitles() {
  if (!fs.existsSync(SESSIONS_DIR)) return;

  let files;
  try {
    files = fs.readdirSync(SESSIONS_DIR);
  } catch (_) {
    return;
  }

  for (const file of files) {
    if (!file.endsWith('.json')) continue;
    const filePath = path.join(SESSIONS_DIR, file);
    try {
      const content = fs.readFileSync(filePath, 'utf8');
      const session = JSON.parse(content);
      const pid = session.pid;
      const name = session.name;
      if (!pid || !name) continue;

      // Check if process is still alive
      try {
        process.kill(pid, 0);
      } catch (_) {
        lastTitles.delete(pid);
        continue;
      }

      if (lastTitles.get(pid) !== name) {
        // Resolve TTY from /proc/<pid>/fd/0, /proc/<pid>/fd/1, or /proc/<pid>/fd/2
        let ttyPath = null;
        for (const fd of [0, 1, 2]) {
          try {
            const link = fs.readlinkSync(`/proc/${pid}/fd/${fd}`);
            if (link && link.startsWith('/dev/pts/')) {
              ttyPath = link;
              break;
            }
          } catch (_) {}
        }

        if (ttyPath) {
          try {
            const fd = fs.openSync(ttyPath, 'w');
            fs.writeSync(fd, `\x1b]0;${name}\x07`);
            fs.closeSync(fd);
            lastTitles.set(pid, name);
          } catch (_) {}
        }
      }
    } catch (_) {}
  }

  // Clean up dead pids
  for (const pid of lastTitles.keys()) {
    try {
      process.kill(pid, 0);
    } catch (_) {
      lastTitles.delete(pid);
    }
  }
}

// Initial sync
updateTitles();

// Poll every 1 second
setInterval(updateTitles, 1000);
