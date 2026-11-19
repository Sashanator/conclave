import { confirmAction } from '@/lib/confirm';

// Module-level flag so any navigation (tabs, Back, sidebar links) can ask before discarding edits.
let dirty = false;

export function setUnsavedChanges(value: boolean): void {
  dirty = value;
}

/** Resolves true when leaving is safe; otherwise asks the user and resolves their answer. */
export async function confirmLeave(): Promise<boolean> {
  return !dirty || confirmAction({ title: 'Discard unsaved changes?', confirmLabel: 'Discard', destructive: true });
}
