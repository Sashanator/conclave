// Non-secret flag: onboarding ended without a stored Claude key, and the page reloads right after, so the app shell shows the hint.
const KEY = 'conclave.aiKeySaveFailed';

export function setAiKeyHint(): void {
  try { localStorage.setItem(KEY, '1'); } catch { /* storage blocked: no hint */ }
}

/** True once per flag; clears it. */
export function takeAiKeyHint(): boolean {
  try {
    if (localStorage.getItem(KEY) !== '1') return false;
    localStorage.removeItem(KEY);
    return true;
  } catch {
    return false;
  }
}
