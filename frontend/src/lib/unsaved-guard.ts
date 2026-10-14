// Module-level flag so any navigation (tabs, Back, sidebar links) can ask before discarding edits.
let dirty = false;

export function setUnsavedChanges(value: boolean): void {
  dirty = value;
}

/** True when leaving is safe; otherwise asks the user and returns their answer. */
export function confirmLeave(): boolean {
  return !dirty || window.confirm('Discard unsaved changes?');
}
