'use client';

import { useEffect, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ArrowDown, ArrowUp, Copy, Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { LANGUAGE_OPTIONS } from '@/lib/summary-languages';
import { setUnsavedChanges } from '@/lib/unsaved-guard';
import {
  draftFromTemplate, emptyDraft, templateFromDraft, validateDraft,
  type TemplateDraft, type TemplateJson,
} from '@/lib/template-draft';

const STANDARD_ID = 'standard_meeting';
const DISCARD_PROMPT = 'Discard unsaved changes?';

interface TemplateInfo { id: string; name: string; use_when: string | null }
interface TemplateListing { templates: TemplateInfo[]; errors: string[] }

const fail = (what: string) => (err: unknown) => toast.error(what, { description: String(err) });

function Field({ id, label, hint, children }: { id: string; label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

export function TemplateSettings() {
  const [listing, setListing] = useState<TemplateListing>({ templates: [], errors: [] });
  const [currentId, setCurrentId] = useState<string | null>(null);
  const [draft, setDraft] = useState<TemplateDraft | null>(null);
  const [savedJson, setSavedJson] = useState('');
  const [confirmDelete, setConfirmDelete] = useState(false);

  const dirty = draft !== null && JSON.stringify(draft) !== savedJson;
  const problem = draft ? validateDraft(draft) : null;

  // Lets navigation elsewhere in the app confirm before discarding this draft
  useEffect(() => {
    setUnsavedChanges(dirty);
    return () => setUnsavedChanges(false);
  }, [dirty]);

  useEffect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const refresh = async () => {
    const res = await invoke<TemplateListing>('api_list_templates');
    setListing(res);
    return res;
  };

  const open = async (id: string) => {
    const d = draftFromTemplate(await invoke<TemplateJson>('api_get_template', { templateId: id }));
    setCurrentId(id);
    setDraft(d);
    setSavedJson(JSON.stringify(d));
  };

  useEffect(() => {
    refresh()
      .then((res) => res.templates[0] && open(res.templates[0].id))
      .catch(fail('Could not load templates'));
  }, []);

  const select = (id: string) => {
    if (id === currentId) return;
    if (dirty && !window.confirm(DISCARD_PROMPT)) return;
    open(id).catch(fail('Could not open the template'));
  };

  const startNew = () => {
    if (dirty && !window.confirm(DISCARD_PROMPT)) return;
    const d = emptyDraft();
    setCurrentId(null);
    setDraft(d);
    setSavedJson(JSON.stringify(d));
  };

  const update = (patch: Partial<TemplateDraft>) => setDraft((d) => d && { ...d, ...patch });
  const setSection = (i: number, patch: Partial<TemplateDraft['sections'][number]>) =>
    setDraft((d) => d && { ...d, sections: d.sections.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const moveSection = (i: number, to: number) =>
    setDraft((d) => {
      if (!d) return d;
      const sections = [...d.sections];
      [sections[i], sections[to]] = [sections[to], sections[i]];
      return { ...d, sections };
    });
  const deleteSection = (i: number) => setDraft((d) => d && { ...d, sections: d.sections.filter((_, j) => j !== i) });
  const addSection = () => setDraft((d) => d && { ...d, sections: [...d.sections, { title: '', instruction: '' }] });

  const save = async () => {
    if (!draft) return;
    try {
      const id = await invoke<string>('api_save_template', { templateId: currentId, template: templateFromDraft(draft) });
      setCurrentId(id);
      setSavedJson(JSON.stringify(draft));
      toast.success('Template saved');
      await refresh().catch(fail('Could not reload the template list'));
    } catch (err) {
      toast.error('Could not save the template', { description: String(err) });
    }
  };

  const duplicate = async () => {
    if (!draft) return;
    try {
      const copy = { ...draft, name: `${draft.name.trim()} (copy)` };
      const id = await invoke<string>('api_save_template', { templateId: null, template: templateFromDraft(copy) });
      await refresh();
      await open(id);
    } catch (err) {
      toast.error('Could not duplicate the template', { description: String(err) });
    }
  };

  const remove = async () => {
    if (!currentId) return;
    setConfirmDelete(false);
    try {
      await invoke('api_delete_template', { templateId: currentId });
      toast.success('Template deleted');
      const res = await refresh();
      if (res.templates[0]) {
        await open(res.templates[0].id);
      } else {
        setCurrentId(null);
        setDraft(null);
        setSavedJson('');
      }
    } catch (err) {
      toast.error('Could not delete the template', { description: String(err) });
    }
  };

  return (
    <div className="grid grid-cols-[16rem_1fr] gap-6">
      <div className="bg-card rounded-lg border border-border p-4 space-y-3 self-start">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-lg font-semibold text-foreground">Templates</h3>
          <Button variant="outline" size="sm" onClick={startNew}>
            <Plus className="w-4 h-4 mr-1" />
            New template
          </Button>
        </div>
        {listing.errors.length > 0 && (
          <p className="text-destructive text-sm">
            {listing.errors.length} template file(s) could not be read: {listing.errors.join('; ')}
          </p>
        )}
        <ul className="space-y-1">
          {listing.templates.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                onClick={() => select(t.id)}
                aria-current={t.id === currentId ? 'true' : undefined}
                className={`w-full rounded-md px-3 py-2 text-left text-sm text-foreground ${t.id === currentId ? 'bg-muted font-medium' : 'hover:bg-muted/60'}`}
              >
                {t.name}
                {!t.use_when?.trim() && <span className="ml-2 text-xs text-muted-foreground">auto-pick off</span>}
              </button>
            </li>
          ))}
        </ul>
      </div>

      <div className="bg-card rounded-lg border border-border p-6">
        {!draft ? (
          <p className="text-sm text-muted-foreground">No template selected.</p>
        ) : (
          <div className="space-y-5">
            <Field id="tpl-name" label="Name">
              <Input id="tpl-name" value={draft.name} onChange={(e) => update({ name: e.target.value })} />
            </Field>
            <Field
              id="tpl-use-when"
              label="Use when"
              hint="One line Conclave's model reads to auto-pick this template"
            >
              <Input id="tpl-use-when" value={draft.use_when} onChange={(e) => update({ use_when: e.target.value })} />
            </Field>
            <Field
              id="tpl-keywords"
              label="Title keywords"
              hint="Comma-separated; a meeting title containing one picks this template"
            >
              <Input id="tpl-keywords" value={draft.title_keywords} onChange={(e) => update({ title_keywords: e.target.value })} />
            </Field>
            <Field id="tpl-language" label="Output language">
              <Select value={draft.language} onValueChange={(v) => update({ language: v })}>
                <SelectTrigger id="tpl-language">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="auto">Auto (meeting language)</SelectItem>
                  {LANGUAGE_OPTIONS.map((o) => (
                    <SelectItem key={o.code} value={o.code}>{o.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field id="tpl-context" label="Context">
              <Textarea id="tpl-context" rows={5} value={draft.context} onChange={(e) => update({ context: e.target.value })} />
            </Field>

            <div className="space-y-3">
              <Label>Sections</Label>
              {draft.sections.map((s, i) => (
                <div key={i} className="rounded-md border border-border bg-card p-4 space-y-3">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-sm font-medium text-muted-foreground">Section {i + 1}</span>
                    <div className="flex gap-1">
                      <Button variant="ghost" size="icon" aria-label="Move section up" disabled={i === 0} onClick={() => moveSection(i, i - 1)}>
                        <ArrowUp className="w-4 h-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label="Move section down"
                        disabled={i === draft.sections.length - 1}
                        onClick={() => moveSection(i, i + 1)}
                      >
                        <ArrowDown className="w-4 h-4" />
                      </Button>
                      <Button variant="ghost" size="icon" aria-label="Delete section" onClick={() => deleteSection(i)}>
                        <Trash2 className="w-4 h-4" />
                      </Button>
                    </div>
                  </div>
                  <Field id={`tpl-section-${i}-title`} label="Title">
                    <Input id={`tpl-section-${i}-title`} value={s.title} onChange={(e) => setSection(i, { title: e.target.value })} />
                  </Field>
                  <Field id={`tpl-section-${i}-instruction`} label="Instruction">
                    <Textarea
                      id={`tpl-section-${i}-instruction`}
                      rows={3}
                      value={s.instruction}
                      onChange={(e) => setSection(i, { instruction: e.target.value })}
                    />
                  </Field>
                </div>
              ))}
              <Button variant="outline" size="sm" onClick={addSection}>
                <Plus className="w-4 h-4 mr-1" />
                Add section
              </Button>
            </div>

            <Field id="tpl-style" label="Style">
              <Textarea id="tpl-style" rows={4} value={draft.style} onChange={(e) => update({ style: e.target.value })} />
            </Field>

            <div className="flex flex-wrap items-center gap-2 pt-2">
              <Button onClick={save} disabled={problem !== null}>Save</Button>
              <Button variant="outline" onClick={duplicate} disabled={problem !== null}>
                <Copy className="w-4 h-4 mr-1" />
                Duplicate
              </Button>
              <Button
                variant="outline"
                className="text-destructive"
                onClick={() => setConfirmDelete(true)}
                disabled={currentId === null || currentId === STANDARD_ID}
                title={currentId === STANDARD_ID ? 'The Standard template cannot be deleted' : undefined}
              >
                <Trash2 className="w-4 h-4 mr-1" />
                Delete
              </Button>
              {problem && <span className="text-sm text-muted-foreground">{problem}</span>}
            </div>
          </div>
        )}
      </div>

      <Dialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete this template?</DialogTitle>
            <DialogDescription>Its file is removed from your templates folder. This cannot be undone.</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmDelete(false)}>Cancel</Button>
            <Button variant="destructive" onClick={remove}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
