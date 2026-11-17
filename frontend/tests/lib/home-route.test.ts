import { expect, test } from 'bun:test';
import { homeRoute } from '../../src/lib/home-route';
import { RecordingStatus as S } from '../../src/contexts/RecordingStateContext';

test('Home goes to the live recording while recording, and to / otherwise', () => {
  expect(homeRoute(true, S.RECORDING)).toBe('/record');
  expect(homeRoute(false, S.IDLE)).toBe('/');
});

test('Home stays on /record through the stop and save flow', () => {
  for (const s of [S.STARTING, S.STOPPING, S.PROCESSING_TRANSCRIPTS, S.SAVING]) {
    expect(homeRoute(false, s)).toBe('/record');
  }
  expect(homeRoute(false, S.COMPLETED)).toBe('/');
  expect(homeRoute(false, S.ERROR)).toBe('/');
});
