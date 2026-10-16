// Languages the user speaks, kept in localStorage as a JSON array of Whisper codes

const CODE_PATTERN = /^[a-z]{2,3}$/;
// Matches the Rust-side limit in set_allowed_languages
export const MAX_ALLOWED_LANGUAGES = 20;

export function parseAllowedLanguages(raw: string | null): string[] {
  if (raw === null) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const codes = parsed.filter(
    (code): code is string => typeof code === 'string' && CODE_PATTERN.test(code)
  );
  return Array.from(new Set(codes)).slice(0, MAX_ALLOWED_LANGUAGES);
}

export function addLanguage(list: string[], code: string): string[] {
  if (list.includes(code) || list.length >= MAX_ALLOWED_LANGUAGES) return list;
  return [...list, code];
}

export function removeLanguage(list: string[], code: string): string[] {
  return list.filter((existing) => existing !== code);
}
