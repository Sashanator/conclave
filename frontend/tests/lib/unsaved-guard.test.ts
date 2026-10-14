import { afterEach, describe, expect, test } from 'bun:test';
import { confirmLeave, setUnsavedChanges } from '@/lib/unsaved-guard';

const realConfirm = window.confirm;
let asked: string[] = [];
let answer = true;

function stubConfirm() {
  asked = [];
  window.confirm = (message?: string) => {
    asked.push(message ?? '');
    return answer;
  };
}

afterEach(() => {
  setUnsavedChanges(false);
  window.confirm = realConfirm;
  answer = true;
});

describe('unsaved guard', () => {
  test('clean: leaving is allowed without asking', () => {
    stubConfirm();
    expect(confirmLeave()).toBe(true);
    expect(asked).toEqual([]);
  });

  test('dirty: asks, and returns the answer', () => {
    stubConfirm();
    setUnsavedChanges(true);
    answer = true;
    expect(confirmLeave()).toBe(true);
    answer = false;
    expect(confirmLeave()).toBe(false);
    expect(asked).toEqual(['Discard unsaved changes?', 'Discard unsaved changes?']);
  });

  test('clearing the flag stops the prompt', () => {
    stubConfirm();
    setUnsavedChanges(true);
    setUnsavedChanges(false);
    expect(confirmLeave()).toBe(true);
    expect(asked).toEqual([]);
  });
});
