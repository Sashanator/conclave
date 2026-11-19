import { confirmLeave } from '@/lib/unsaved-guard';

export type Origin = { path: string; label: string };

const KEY = 'conclave.origin';

/** Call just before the origin page pushes, so the next page knows where Back leads. */
export function rememberOrigin(o: Origin): void {
  try {
    sessionStorage.setItem(KEY, JSON.stringify({ path: o.path, label: o.label, len: history.length }));
  } catch { /* storage unavailable: Back falls back to the default page */ }
}

function read(): (Origin & { len: number }) | null {
  try {
    const v = JSON.parse(sessionStorage.getItem(KEY) ?? 'null');
    if (v && typeof v.path === 'string' && typeof v.label === 'string' && typeof v.len === 'number') return v;
  } catch { /* corrupt or blocked storage reads as no origin */ }
  return null;
}

export function readOrigin(): Origin | null {
  const v = read();
  return v && { path: v.path, label: v.label };
}

/** True only when the stored origin is the previous history entry, so a label never names a place Back will not reach. */
export function originIsPrevious(): boolean {
  const v = read();
  return !!v && history.length === v.len + 1 && v.path !== location.pathname + location.search;
}

export async function goBack(router: { back(): void; push(href: string): void }, fallback: string): Promise<void> {
  if (!(await confirmLeave())) return;
  if (originIsPrevious()) router.back();
  else router.push(fallback);
}
