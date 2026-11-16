import { expect, test } from 'bun:test';
import { homeRoute } from '../../src/lib/home-route';

test('Home goes to the live recording while recording, and to / otherwise', () => {
  expect(homeRoute(true)).toBe('/record');
  expect(homeRoute(false)).toBe('/');
});
