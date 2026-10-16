import { describe, expect, test } from 'bun:test';

import {
  addLanguage,
  parseAllowedLanguages,
  removeLanguage,
} from '../../src/lib/allowed-languages';

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
});

describe('addLanguage and removeLanguage', () => {
  test('adds a language at the end', () => {
    expect(addLanguage(['ru'], 'de')).toEqual(['ru', 'de']);
  });

  test('does not add a language twice', () => {
    expect(addLanguage(['ru'], 'ru')).toEqual(['ru']);
  });

  test('removes a language and keeps the rest in order', () => {
    expect(removeLanguage(['ru', 'de', 'en'], 'de')).toEqual(['ru', 'en']);
  });

  test('removing a language that is not chosen changes nothing', () => {
    expect(removeLanguage(['ru'], 'en')).toEqual(['ru']);
  });
});
