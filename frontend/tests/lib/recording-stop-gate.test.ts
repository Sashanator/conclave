import { describe, expect, test } from 'bun:test';
import { beginStop, endStop, noteRecordingStarted, tryBeginStop } from '../../src/lib/recording-stop-gate';

describe('recording stop gate', () => {
  test('a second begin is refused while a stop is in flight', () => {
    expect(tryBeginStop()).toBe(true);
    expect(tryBeginStop()).toBe(false);
    endStop();
  });

  test('begin works again after end', () => {
    expect(tryBeginStop()).toBe(true);
    endStop();
    expect(tryBeginStop()).toBe(true);
    endStop();
  });
});

describe('beginStop', () => {
  test('a repeat with no start in between is the same stop heard twice', async () => {
    expect(await beginStop()).toBe(true);
    expect(await beginStop()).toBe(false);
    endStop();
  });

  test('a stop after a new start waits for the running save, then runs', async () => {
    expect(await beginStop()).toBe(true);
    noteRecordingStarted();
    let ran = false;
    const second = beginStop().then((ok) => { ran = ok; });
    await Promise.resolve();
    expect(ran).toBe(false);
    endStop();
    await second;
    expect(ran).toBe(true);
    endStop();
  });
});
