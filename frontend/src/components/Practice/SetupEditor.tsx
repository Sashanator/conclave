import { useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { FileText, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { VOICES, fitsLimit, materialKind, type MaterialMeta, type SetupInput, type SetupSummary } from '@/lib/practice';

function Labeled({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-1">
      <span className="text-xs uppercase tracking-wide text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

// The editor for one setup, or a new one (setup null). Material needs a saved setup, so it waits for the first save.
export function SetupEditor({ setup, onDone, onChanged, voicesReady, onDownloadVoices }: {
  setup: SetupSummary | null;
  onDone: () => void;
  onChanged: () => Promise<unknown>;
  voicesReady: boolean;
  onDownloadVoices: (progress: (percent: number) => void) => Promise<void>;
}) {
  const [name, setName] = useState(setup?.name ?? '');
  const [character, setCharacter] = useState(setup?.character ?? '');
  const [situation, setSituation] = useState(setup?.situation ?? '');
  const [voice, setVoice] = useState(setup?.voice ?? VOICES[0].id);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pasting, setPasting] = useState(false);
  const [pasteName, setPasteName] = useState('');
  const [pasteText, setPasteText] = useState('');
  // Set while the voices download runs for a preview made before the first session
  const [voicePercent, setVoicePercent] = useState<number | null>(null);

  const materials = setup?.materials ?? [];
  const usedBytes = materials.reduce((sum, m) => sum + m.bytes, 0);

  const save = async () => {
    const input: SetupInput = {
      id: setup?.id ?? null,
      name: name.trim(),
      character: character.trim(),
      situation: situation.trim(),
      voice,
    };
    if (!input.name) return setError('Give the setup a name');
    if (!input.character) return setError('Say who the character is');
    setError(null);
    setBusy(true);
    try {
      await invoke('practice_save_setup', { input });
      onDone();
    } catch (err) {
      toast.error('Could not save the setup', { description: String(err) });
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!setup || !window.confirm(`Delete the setup "${setup.name}"?`)) return;
    try {
      await invoke('practice_delete_setup', { id: setup.id });
      onDone();
    } catch (err) {
      toast.error('Could not delete the setup', { description: String(err) });
    }
  };

  // Each file is checked before it is read: its size counts against the 20 MB per setup, the total so far included
  const addFiles = async (files: File[]) => {
    if (!setup) return;
    let total = usedBytes;
    try {
      for (const file of files) {
        const kind = materialKind(file.name);
        if (!kind) {
          toast.error(`${file.name} is not a PDF, TXT or MD file`);
          continue;
        }
        if (!fitsLimit(total, file.size)) {
          toast.error(`${file.name} would take the setup past 20 MB`);
          continue;
        }
        const bytes = new Uint8Array(await file.arrayBuffer());
        await invoke('practice_add_material', bytes, {
          headers: { 'x-setup-id': setup.id, 'x-name': encodeURIComponent(file.name), 'x-kind': kind },
        });
        total += file.size;
      }
    } catch (err) {
      toast.error('Could not add the file', { description: String(err) });
    } finally {
      await onChanged();
    }
  };

  const addText = async () => {
    if (!setup) return;
    const label = pasteName.trim();
    if (!label || !pasteText.trim()) {
      setError('Name the pasted text and paste some text');
      return;
    }
    if (!fitsLimit(usedBytes, new TextEncoder().encode(pasteText).byteLength)) {
      toast.error('The pasted text would take the setup past 20 MB');
      return;
    }
    try {
      await invoke('practice_add_text', { setupId: setup.id, name: label, text: pasteText });
      setPasting(false);
      setPasteName('');
      setPasteText('');
      setError(null);
      await onChanged();
    } catch (err) {
      toast.error('Could not add the text', { description: String(err) });
    }
  };

  const removeMaterial = async (m: MaterialMeta) => {
    if (!window.confirm(`Remove ${m.name} from this setup?`)) return;
    try {
      await invoke('practice_delete_material', { id: m.id });
      await onChanged();
    } catch (err) {
      toast.error('Could not remove the material', { description: String(err) });
    }
  };

  const newQuestions = async () => {
    if (!setup) return;
    try {
      await invoke('practice_new_questions', { setupId: setup.id });
      toast.success('New questions will be prepared at the next start');
      await onChanged();
    } catch (err) {
      toast.error('Could not clear the questions', { description: String(err) });
    }
  };

  const preview = async () => {
    if (!voicesReady) {
      setVoicePercent(0);
      try {
        await onDownloadVoices(setVoicePercent);
      } catch (err) {
        toast.error('Could not download the voices', { description: String(err) });
        return;
      } finally {
        setVoicePercent(null);
      }
    }
    try {
      await invoke('practice_preview_voice', { voice });
    } catch (err) {
      toast.error('Could not play the voice', { description: String(err) });
    }
  };

  return (
    <section className="bg-card rounded-lg border border-border p-6 shadow-sm space-y-4">
      <Button variant="ghost" size="sm" onClick={onDone}>← Practice</Button>
      <h2 className="text-xl font-semibold text-foreground">{setup ? setup.name : 'New setup'}</h2>

      <div className="grid gap-4 sm:grid-cols-2">
        <Labeled label="Setup name">
          <Input aria-label="Setup name" value={name} onChange={(e) => setName(e.target.value)} />
        </Labeled>
        <Labeled label="Character">
          <Input aria-label="Character" value={character} onChange={(e) => setCharacter(e.target.value)} />
        </Labeled>
      </div>

      <Labeled label="The situation (your words)">
        <Textarea aria-label="The situation (your words)" rows={3} value={situation} onChange={(e) => setSituation(e.target.value)} />
      </Labeled>

      <Labeled label="Material (optional)">
        <div className="flex flex-wrap items-center gap-2">
          {materials.map((m) => (
            <span key={m.id} className="inline-flex items-center gap-1 rounded-full border border-border px-3 py-1 text-xs">
              <FileText className="size-3" />
              {m.name}
              <button type="button" aria-label={`Remove ${m.name}`} onClick={() => removeMaterial(m)}>
                <X className="size-3" />
              </button>
            </span>
          ))}
          <label className={`inline-flex h-8 items-center rounded-md border border-input bg-background px-3 text-xs shadow-sm ${setup ? 'cursor-pointer hover:bg-accent' : 'opacity-50'}`}>
            + Add file
            <input
              type="file"
              accept=".pdf,.txt,.md"
              multiple
              className="sr-only"
              disabled={!setup}
              onChange={(e) => {
                const files = Array.from(e.target.files ?? []);
                e.target.value = '';
                void addFiles(files);
              }}
            />
          </label>
          <Button variant="outline" size="sm" disabled={!setup} onClick={() => setPasting(true)}>Paste text</Button>
        </div>
        {!setup && <p className="text-xs text-muted-foreground">Save the setup first, then add material.</p>}
      </Labeled>

      {pasting && (
        <div className="rounded-lg border border-border p-4 space-y-2">
          <Input aria-label="Pasted text name" placeholder="Name, e.g. abstract" value={pasteName} onChange={(e) => setPasteName(e.target.value)} />
          <Textarea aria-label="Pasted text" rows={6} value={pasteText} onChange={(e) => setPasteText(e.target.value)} />
          <div className="flex justify-end gap-2">
            <Button variant="outline" size="sm" onClick={() => setPasting(false)}>Close</Button>
            <Button size="sm" onClick={addText}>Add text</Button>
          </div>
        </div>
      )}

      <div className="grid gap-4 sm:grid-cols-2">
        <Labeled label="Voice">
          <div className="flex items-center gap-2">
            <Select value={voice} onValueChange={setVoice}>
              <SelectTrigger aria-label="Voice" className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {VOICES.map((v) => (
                  <SelectItem key={v.id} value={v.id}>{v.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button variant="outline" size="sm" disabled={voicePercent !== null} onClick={preview}>▶ Preview</Button>
            {voicePercent !== null && <span className="text-sm text-muted-foreground">{`Downloading voices… ${voicePercent}%`}</span>}
          </div>
        </Labeled>
      </div>

      {setup && materials.length > 0 && (
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="outline" size="sm" onClick={newQuestions}>New questions</Button>
          <p className="text-xs text-muted-foreground">The questions are made again the next time you start.</p>
        </div>
      )}

      <div className="flex items-center justify-between gap-2 pt-2">
        {setup ? <Button variant="destructive" onClick={remove}>Delete setup</Button> : <span />}
        <div className="flex items-center gap-2">
          {error && <p className="text-sm text-destructive">{error}</p>}
          <Button variant="outline" onClick={onDone}>Cancel</Button>
          <Button disabled={busy} onClick={save}>Save</Button>
        </div>
      </div>
    </section>
  );
}
