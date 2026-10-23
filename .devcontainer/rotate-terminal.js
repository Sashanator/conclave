#!/usr/bin/env node
const fs = require('fs');
const { spawn } = require('child_process');

const PALETTES = [
  { color: 'terminal.ansiCyan', icon: 'terminal' },
  { color: 'terminal.ansiGreen', icon: 'rocket' },
  { color: 'terminal.ansiMagenta', icon: 'sparkle' },
  { color: 'terminal.ansiYellow', icon: 'zap' },
  { color: 'terminal.ansiRed', icon: 'flame' },
  { color: 'terminal.ansiBlue', icon: 'hubot' },
  { color: 'terminal.ansiCyan', icon: 'beaker' },
  { color: 'terminal.ansiGreen', icon: 'shield' },
  { color: 'terminal.ansiMagenta', icon: 'star' },
  { color: 'terminal.ansiYellow', icon: 'chip' },
  { color: 'terminal.ansiRed', icon: 'squirrel' },
  { color: 'terminal.ansiBlue', icon: 'server' }
];

const STATE_FILE = '/home/node/.terminal-color-index';
const TARGET_FILES = [
  '/workspace/.vscode/settings.json',
  '/home/node/.vscode-server/data/Machine/settings.json'
];

try {
  let index = 0;
  if (fs.existsSync(STATE_FILE)) {
    const raw = fs.readFileSync(STATE_FILE, 'utf8').trim();
    const parsed = parseInt(raw, 10);
    if (!isNaN(parsed)) {
      index = (parsed + 1) % PALETTES.length;
    }
  }
  fs.writeFileSync(STATE_FILE, String(index), 'utf8');

  const next = PALETTES[index];

  for (const file of TARGET_FILES) {
    if (!fs.existsSync(file)) continue;
    try {
      const content = fs.readFileSync(file, 'utf8');
      const settings = JSON.parse(content);
      settings['terminal.integrated.tabs.title'] = '${sequence}';
      const profiles = settings['terminal.integrated.profiles.linux'];
      const targetProfile = profiles && (profiles['Claude Code chat'] || profiles['Claude Code']);
      if (targetProfile) {
        targetProfile.color = next.color;
        targetProfile.icon = next.icon;
        targetProfile.overrideName = false;
        fs.writeFileSync(file, JSON.stringify(settings, null, 4) + '\n', 'utf8');
      }
    } catch (_) {}
  }
} catch (_) {}

// Ensure claude-title-sync daemon is running
try {
  const SYNC_SCRIPT = '/workspace/.devcontainer/claude-title-sync.js';
  const LOCK_FILE = '/tmp/claude-title-sync.pid';
  let isRunning = false;
  if (fs.existsSync(LOCK_FILE)) {
    const pid = parseInt(fs.readFileSync(LOCK_FILE, 'utf8').trim(), 10);
    if (!isNaN(pid)) {
      try {
        process.kill(pid, 0);
        isRunning = true;
      } catch (_) {}
    }
  }
  if (!isRunning && fs.existsSync(SYNC_SCRIPT)) {
    const child = spawn(process.execPath, [SYNC_SCRIPT], {
      detached: true,
      stdio: 'ignore'
    });
    child.unref();
  }
} catch (_) {}
