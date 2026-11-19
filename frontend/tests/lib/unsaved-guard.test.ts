import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';

const original = { ...(await import('../../src/lib/confirm')) };
afterAll(() => mock.module('../../src/lib/confirm', () => original));

let answer = true;
const confirmAction = mock(async (_o: unknown) => answer);
mock.module('../../src/lib/confirm', () => ({ ...original, confirmAction }));
const { confirmLeave, setUnsavedChanges } = await import('../../src/lib/unsaved-guard');

afterEach(() => {
  setUnsavedChanges(false);
  confirmAction.mockClear();
  answer = true;
});

describe('unsaved guard', () => {
  test('clean: leaving is allowed without asking', async () => {
    expect(await confirmLeave()).toBe(true);
    expect(confirmAction).not.toHaveBeenCalled();
  });

  test('dirty: asks, and returns the answer', async () => {
    setUnsavedChanges(true);
    answer = true;
    expect(await confirmLeave()).toBe(true);
    answer = false;
    expect(await confirmLeave()).toBe(false);
    expect(confirmAction).toHaveBeenCalledTimes(2);
    expect(confirmAction).toHaveBeenCalledWith({ title: 'Discard unsaved changes?', confirmLabel: 'Discard', destructive: true });
  });

  test('clearing the flag stops the prompt', async () => {
    setUnsavedChanges(true);
    setUnsavedChanges(false);
    expect(await confirmLeave()).toBe(true);
    expect(confirmAction).not.toHaveBeenCalled();
  });
});
