export const REGENERATE_DISCARD_PROMPT = 'Regenerating replaces your unsaved edits to these notes. Continue?';

/** True when a regenerate may run: there are no unsaved summary edits, or the user agreed to replace them. */
export function shouldRegenerate(
  hasUnsavedEdits: boolean,
  confirm: (message: string) => boolean = (message) => window.confirm(message),
): boolean {
  return !hasUnsavedEdits || confirm(REGENERATE_DISCARD_PROMPT);
}

/** Picking a template replaces the summary, so it runs only when the same guard allows a regenerate. */
export function pickTemplateIfAllowed(
  hasUnsavedEdits: boolean,
  pick: () => void,
  regenerate: () => unknown,
  confirm?: (message: string) => boolean,
): void {
  if (!shouldRegenerate(hasUnsavedEdits, confirm)) return;
  pick();
  void regenerate();
}
