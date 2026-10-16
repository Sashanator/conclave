// Languages the user speaks, kept in localStorage as a JSON array of Whisper codes

const CODE_PATTERN = /^[a-z]{2,3}$/;

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
  return Array.from(new Set(codes));
}

export function addLanguage(list: string[], code: string): string[] {
  return list.includes(code) ? list : [...list, code];
}

export function removeLanguage(list: string[], code: string): string[] {
  return list.filter((existing) => existing !== code);
}
