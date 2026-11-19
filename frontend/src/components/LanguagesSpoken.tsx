'use client';

import { useConfig } from '@/contexts/ConfigContext';
import { LANGUAGES } from '@/constants/languages';
import { MAX_ALLOWED_LANGUAGES, addLanguage, removeLanguage } from '@/lib/allowed-languages';

/** The languages Whisper may pick among whenever the transcription language is Auto. */
export function LanguagesSpoken() {
  const { allowedLanguages, setAllowedLanguages } = useConfig();
  const addable = LANGUAGES.filter((l) => l.code !== 'auto' && l.code !== 'auto-translate' && !allowedLanguages.includes(l.code));
  return (
    <div className="space-y-2 pt-2">
      <h3 className="text-sm font-medium text-foreground">Languages spoken</h3>
      <div className="flex flex-wrap gap-2">
        {allowedLanguages.map((code) => {
          const name = LANGUAGES.find((l) => l.code === code)?.name || code;
          return (
            <span key={code} className="inline-flex items-center gap-1 rounded-full border border-input bg-background px-2 py-1 text-xs">
              {name}
              <button type="button" aria-label={`Remove ${name}`} onClick={() => setAllowedLanguages(removeLanguage(allowedLanguages, code))} className="text-muted-foreground hover:text-foreground">
                ×
              </button>
            </span>
          );
        })}
      </div>
      {allowedLanguages.length < MAX_ALLOWED_LANGUAGES && (
        <select
          aria-label="Add language"
          value=""
          onChange={(e) => { if (e.target.value) setAllowedLanguages(addLanguage(allowedLanguages, e.target.value)); }}
          className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm focus:border-ring focus:outline-none focus:ring-1 focus:ring-ring"
        >
          <option value="">Add language…</option>
          {addable.map((l) => <option key={l.code} value={l.code}>{l.name} ({l.code})</option>)}
        </select>
      )}
      <p className="text-xs text-muted-foreground">Used whenever the language is Auto, including re-transcribe and import.</p>
    </div>
  );
}
