import { useState } from 'react';
import { X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { addWord, type Commit, type DictateSettings } from '@/lib/dictate';

// Stays editable while Dictate is off: the words also help meeting transcripts
export function DictionaryEditor({ settings, commit }: { settings: DictateSettings; commit: Commit }) {
  const [word, setWord] = useState('');

  const add = () => {
    if (addWord(settings.dictionary, word) === settings.dictionary) return; // rejected: keep the text so it can be fixed
    commit((s) => ({ ...s, dictionary: addWord(s.dictionary, word) }));
    setWord('');
  };

  const remove = (target: string) => commit((s) => ({ ...s, dictionary: s.dictionary.filter((w) => w !== target) }));

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
      <div>
        <h3 className="text-lg font-semibold text-foreground">Dictionary</h3>
        <p className="text-sm text-muted-foreground">Words Whisper should hear and spell right, in dictation and in meetings.</p>
      </div>

      <Input
        placeholder="Add a word and press Enter"
        value={word}
        onChange={(e) => setWord(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            add();
          }
        }}
      />

      <ul className="flex flex-wrap gap-2">
        {settings.dictionary.map((w) => (
          <li key={w} className="flex items-center gap-1.5 rounded-full border border-border px-3 py-1 text-sm">
            {w}
            <button
              type="button"
              aria-label={`Remove ${w}`}
              className="text-muted-foreground hover:text-foreground"
              onClick={() => remove(w)}
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </li>
        ))}
      </ul>

      <p className="text-xs text-muted-foreground">Also used for meeting transcripts.</p>
    </section>
  );
}
