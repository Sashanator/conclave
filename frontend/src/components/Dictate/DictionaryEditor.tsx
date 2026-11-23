import { useState } from 'react';
import { X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { addWord, wordProblem, type Commit, type DictateSettings } from '@/lib/dictate';

// Stays editable while Dictate is off: the words also help meeting summaries
export function DictionaryEditor({ settings, commit }: { settings: DictateSettings; commit: Commit }) {
  const [word, setWord] = useState('');
  const [problem, setProblem] = useState<string | null>(null);

  const add = () => {
    const why = wordProblem(settings.dictionary, word);
    setProblem(why);
    if (why || addWord(settings.dictionary, word) === settings.dictionary) return; // rejected: keep the text so it can be fixed
    commit((s) => ({ ...s, dictionary: addWord(s.dictionary, word) }));
    setWord('');
  };

  const remove = (target: string) => commit((s) => ({ ...s, dictionary: s.dictionary.filter((w) => w !== target) }));

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-foreground">Dictionary</h2>
        <p className="text-sm text-muted-foreground">Words Whisper should hear and spell right, in dictation and in meetings. Also used as spelling hints for meeting summaries.</p>
      </div>

      <div className="space-y-1.5">
        <div className="flex gap-2">
          <Input
            aria-label="Add a word"
            aria-invalid={problem ? true : undefined}
            placeholder="Add a word and press Enter"
            value={word}
            onChange={(e) => { setWord(e.target.value); setProblem(null); }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                add();
              }
            }}
          />
          <Button type="button" variant="outline" onClick={add}>Add</Button>
        </div>
        {problem && <p role="alert" className="text-sm text-destructive">{problem}</p>}
      </div>

      <ul className="flex flex-wrap gap-2">
        {settings.dictionary.map((w) => (
          <li key={w} className="flex items-center gap-1 rounded-full border border-border py-0.5 pl-3 pr-1 text-sm">
            {w}
            <button
              type="button"
              aria-label={`Remove ${w}`}
              className="rounded-full p-1 text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() => remove(w)}
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
