export type ConfirmOptions = { title: string; body?: string; confirmLabel: string; destructive?: boolean };
export type ConfirmRequest = ConfirmOptions & { resolve(ok: boolean): void };

let handler: ((req: ConfirmRequest) => void) | null = null;

/** Set by `ConfirmHost` only. */
export function setConfirmHandler(h: ((req: ConfirmRequest) => void) | null): void {
  handler = h;
}

/** Asks through the app dialog; light windows with no host fall back to `window.confirm`, so a confirm is never dropped. */
export function confirmAction(opts: ConfirmOptions): Promise<boolean> {
  if (!handler) return Promise.resolve(window.confirm(opts.body ? `${opts.title}\n\n${opts.body}` : opts.title));
  const h = handler;
  return new Promise((resolve) => h({ ...opts, resolve }));
}
