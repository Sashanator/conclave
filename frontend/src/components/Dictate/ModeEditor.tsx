import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import type { CustomMode, ModeExample } from '@/lib/dictate';

export type ModeDraft = Omit<CustomMode, 'id' | 'hotkey'>;

const MAX_EXAMPLES = 3;
const EMPTY_EXAMPLE: ModeExample = { said: '', want: '' };

interface ModeEditorProps {
  /** null for a new mode */
  mode: CustomMode | null;
  onSave: (draft: ModeDraft) => void;
  onCancel: () => void;
}

// Pairs with both sides blank are dropped; a pair with one side filled is a problem
function examplesProblem(examples: ModeExample[]): string | null {
  return examples.some((e) => Boolean(e.said.trim()) !== Boolean(e.want.trim()))
    ? "Each example needs both 'Said' and 'Should be'"
    : null;
}

export function ModeEditor({ mode, onSave, onCancel }: ModeEditorProps) {
  const [name, setName] = useState(mode?.name ?? '');
  const [instructions, setInstructions] = useState(mode?.instructions ?? '');
  const [examples, setExamples] = useState<ModeExample[]>(mode?.examples ?? []);

  const problem = !name.trim() ? 'Give the mode a name' : examplesProblem(examples);

  const save = () => {
    if (problem) return;
    onSave({
      name: name.trim(),
      instructions: instructions.trim(),
      examples: examples
        .map((e) => ({ said: e.said.trim(), want: e.want.trim() }))
        .filter((e) => e.said !== '' || e.want !== ''),
    });
  };

  const setExample = (i: number, field: keyof ModeExample, value: string) =>
    setExamples(examples.map((e, j) => (j === i ? { ...e, [field]: value } : e)));

  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>{mode ? 'Edit mode' : 'New mode'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="mode-name">Name</Label>
            <Input id="mode-name" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="mode-instructions">Instructions</Label>
            <Textarea
              id="mode-instructions"
              rows={6}
              maxLength={4000}
              placeholder="What the model should do with the dictation"
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
            />
          </div>

          <div className="space-y-2">
            <Label>Examples (up to {MAX_EXAMPLES})</Label>
            {examples.map((e, i) => (
              <div key={i} className="grid grid-cols-[1fr_1fr_auto] gap-2 items-start">
                <Textarea
                  aria-label={`Example ${i + 1} said`}
                  placeholder="Said"
                  rows={2}
                  maxLength={2000}
                  value={e.said}
                  onChange={(ev) => setExample(i, 'said', ev.target.value)}
                />
                <Textarea
                  aria-label={`Example ${i + 1} should be`}
                  placeholder="Should be"
                  rows={2}
                  maxLength={2000}
                  value={e.want}
                  onChange={(ev) => setExample(i, 'want', ev.target.value)}
                />
                <Button variant="ghost" size="sm" aria-label={`Remove example ${i + 1}`} onClick={() => setExamples(examples.filter((_, j) => j !== i))}>
                  Remove
                </Button>
              </div>
            ))}
            {examples.length < MAX_EXAMPLES && (
              <Button variant="outline" size="sm" onClick={() => setExamples([...examples, EMPTY_EXAMPLE])}>
                + Add example
              </Button>
            )}
          </div>

          {problem && <p className="text-sm text-destructive">{problem}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>Cancel</Button>
          <Button disabled={Boolean(problem)} onClick={save}>Save</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
