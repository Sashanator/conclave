import { confirmAction } from '@/lib/confirm';

export const REGENERATE_DISCARD_PROMPT = 'Regenerating replaces your unsaved edits to these notes. Continue?';

/** True when a regenerate may run: there are no unsaved summary edits, or the user agreed to replace them. Callers must await it. */
export async function shouldRegenerate(
  hasUnsavedEdits: boolean,
  ask: (message: string) => Promise<boolean> = (m) =>
    confirmAction({ title: 'Replace your edits?', body: m, confirmLabel: 'Regenerate', destructive: true }),
): Promise<boolean> {
  return !hasUnsavedEdits || (await ask(REGENERATE_DISCARD_PROMPT));
}

/** Picking a template replaces the summary, so it runs only when the same guard allows a regenerate. */
export async function pickTemplateIfAllowed(
  hasUnsavedEdits: boolean,
  pick: () => void,
  regenerate: () => unknown,
  ask?: (message: string) => Promise<boolean>,
): Promise<void> {
  if (!(await shouldRegenerate(hasUnsavedEdits, ask))) return;
  pick();
  void regenerate();
}
