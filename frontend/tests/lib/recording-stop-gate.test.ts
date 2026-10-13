import { describe, expect, test } from 'bun:test';
import { endStop, tryBeginStop } from '../../src/lib/recording-stop-gate';

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
