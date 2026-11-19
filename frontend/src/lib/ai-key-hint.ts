// Non-secret flag: onboarding ended without a stored Claude key, and the page reloads right after, so the app shell shows the hint.
const KEY = 'conclave.aiKeySaveFailed';

export function setAiKeyHint(): void {
  try { localStorage.setItem(KEY, '1'); } catch { /* storage blocked: no hint */ }
}

export function hasAiKeyHint(): boolean {
  try { return localStorage.getItem(KEY) === '1'; } catch { return false; }
}

export function clearAiKeyHint(): void {
  try { localStorage.removeItem(KEY); } catch { /* ignore */ }
}
