import { describe, expect, test } from 'bun:test';

import {
  MAX_ALLOWED_LANGUAGES,
  addLanguage,
  parseAllowedLanguages,
  removeLanguage,
} from '../../src/lib/allowed-languages';

// 25 distinct valid two-letter codes: 'aa' .. 'ay'
const manyCodes = Array.from({ length: 25 }, (_, i) => 'a' + String.fromCharCode(97 + i));

describe('parseAllowedLanguages', () => {
  test('returns an empty list for a missing value', () => {
    expect(parseAllowedLanguages(null)).toEqual([]);
  });

  test('returns an empty list for bad JSON', () => {
    expect(parseAllowedLanguages('["ru",')).toEqual([]);
  });

  test('returns an empty list for a non-array JSON value', () => {
    expect(parseAllowedLanguages('{"0":"ru"}')).toEqual([]);
    expect(parseAllowedLanguages('"ru"')).toEqual([]);
  });

  test('keeps valid codes and drops invalid ones', () => {
    expect(parseAllowedLanguages('["ru","RU","auto","x",42,null,"de"]')).toEqual(['ru', 'de']);
  });

  test('dedupes keeping first order', () => {
    expect(parseAllowedLanguages('["de","ru","de"]')).toEqual(['de', 'ru']);
  });

  test('keeps at most the first 20 valid codes', () => {
    expect(MAX_ALLOWED_LANGUAGES).toBe(20);
    expect(parseAllowedLanguages(JSON.stringify(manyCodes))).toEqual(manyCodes.slice(0, 20));
  });
});

describe('addLanguage and removeLanguage', () => {
  test('adds a language at the end', () => {
    expect(addLanguage(['ru'], 'de')).toEqual(['ru', 'de']);
  });

  test('does not add a language twice', () => {
    expect(addLanguage(['ru'], 'ru')).toEqual(['ru']);
  });

  test('does not add a 21st language', () => {
    const full = manyCodes.slice(0, 20);
    expect(addLanguage(full, 'zz')).toEqual(full);
  });

  test('adds the 20th language', () => {
    const nineteen = manyCodes.slice(0, 19);
    expect(addLanguage(nineteen, 'zz')).toEqual([...nineteen, 'zz']);
  });

  test('removes a language and keeps the rest in order', () => {
    expect(removeLanguage(['ru', 'de', 'en'], 'de')).toEqual(['ru', 'en']);
  });

  test('removing a language that is not chosen changes nothing', () => {
    expect(removeLanguage(['ru'], 'en')).toEqual(['ru']);
  });
});
