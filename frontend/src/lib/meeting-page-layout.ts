export function readStoredRatio(key: string, fallback: number, min: number, max: number): number {
  try {
    const raw = localStorage.getItem(key);
    const n = raw == null ? NaN : Number(raw);
    return Number.isFinite(n) && n >= min && n <= max ? n : fallback;
  } catch {
    return fallback;
  }
}

export function writeStoredRatio(key: string, value: number): void {
  try { localStorage.setItem(key, String(value)); } catch { /* layout persistence is optional */ }
}
