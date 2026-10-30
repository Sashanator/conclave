'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ArrowDown, ArrowUp, Copy, History, Plus, Sparkles, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { LANGUAGE_OPTIONS } from '@/lib/summary-languages';
import { setUnsavedChanges } from '@/lib/unsaved-guard';
import {
  draftFromTemplate, emptyDraft, templateFromDraft, validateDraft,
  type TemplateDraft, type TemplateJson,
} from '@/lib/template-draft';
import { TemplateAssistDialog, type AssistMode } from '@/components/TemplateAssistDialog';
import { TemplateDiffReview } from '@/components/TemplateDiffReview';
import {
  deletedTemplates, historyLabel, originFor, templateHistory, templateVersion,
  type ChangeNote, type DeletedTemplate, type HistoryEntry, type PendingOrigin,
} from '@/lib/templateAssist';

const STANDARD_ID = 'standard_meeting';
const DISCARD_PROMPT = 'Discard unsaved changes?';

interface TemplateInfo { id: string; name: string; use_when: string | null; has_keywords: boolean }
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
  const [assist, setAssist] = useState<AssistMode | null>(null);
  const [notes, setNotes] = useState<ChangeNote[] | null>(null);
  const pendingOrigin = useRef<PendingOrigin | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  // null = loading; 'error' = the load failed (never shown as an empty list).
  const [history, setHistory] = useState<HistoryEntry[] | 'error' | null>(null);
  const [deleted, setDeleted] = useState<DeletedTemplate[] | 'error' | null>(null);
  const [review, setReview] = useState<{ old: TemplateJson; next: TemplateJson } | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  // Bumped whenever the editor switches to another template; a response for an older view is ignored.
  const viewRef = useRef(0);

  const dirty = draft !== null && JSON.stringify(draft) !== savedJson;
  const problem = draft ? validateDraft(draft) : null;
  // A restored deleted template opens under its old id before its file exists; History and Delete need the file.
  const onDisk = currentId !== null && listing.templates.some(t => t.id === currentId);

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

  // Save, duplicate, delete and open run one at a time. While one runs, the editor and list are disabled.
  const runExclusive = async (task: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await task();
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const load = async (id: string) => {
    const view = ++viewRef.current;
    const d = draftFromTemplate(await invoke<TemplateJson>('api_get_template', { templateId: id }));
    if (viewRef.current !== view) return;
    // A save started on the previous view must not land in the template that just loaded.
    viewRef.current += 1;
    setCurrentId(id);
    setDraft(d);
    setSavedJson(JSON.stringify(d));
    setNotes(null);
    pendingOrigin.current = null;
  };
  const open = (id: string) => runExclusive(() => load(id));

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
    if (dirty && !window.confirm(DISCARD_PROMPT)) return false;
    viewRef.current += 1;
    const d = emptyDraft();
    setCurrentId(null);
    setDraft(d);
    setSavedJson(JSON.stringify(d));
    setNotes(null);
    pendingOrigin.current = null;
    return true;
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

  // A repeated click cannot create or delete twice.
  const save = () => runExclusive(async () => {
    if (!draft) return;
    const view = viewRef.current;
    try {
      const id = await invoke<string>('api_save_template', { templateId: currentId, template: templateFromDraft(draft), origin: originFor(draft, pendingOrigin.current) });
      if (viewRef.current === view) {
        pendingOrigin.current = null;
        setCurrentId(id);
        setSavedJson(JSON.stringify(draft));
      }
      toast.success('Template saved');
      await refresh().catch(fail('Could not reload the template list'));
    } catch (err) {
      toast.error('Could not save the template', { description: String(err) });
    }
  });

  const duplicate = () => runExclusive(async () => {
    if (!draft) return;
    try {
      const copy = { ...draft, name: `${draft.name.trim()} (copy)` };
      const id = await invoke<string>('api_save_template', { templateId: null, template: templateFromDraft(copy) });
      await refresh();
      await load(id);
    } catch (err) {
      toast.error('Could not duplicate the template', { description: String(err) });
    }
  });

  const improve = () => runExclusive(async () => {
    if (dirty || currentId === null) return;
    try {
      const saved = await invoke<TemplateJson>('api_get_template', { templateId: currentId });
      if (saved) setAssist({ kind: 'improve', templateId: currentId, saved });
    } catch (err) {
      fail('Could not open the template assistant')(err);
    }
  });

  const remove = () => runExclusive(async () => {
    if (!currentId) return;
    setConfirmDelete(false);
    try {
      await invoke('api_delete_template', { templateId: currentId });
      toast.success('Template deleted');
      const res = await refresh();
      if (res.templates[0]) {
        await load(res.templates[0].id);
      } else {
        viewRef.current += 1;
        setCurrentId(null);
        setDraft(null);
        setSavedJson('');
      }
    } catch (err) {
      toast.error('Could not delete the template', { description: String(err) });
    }
  });

  const loadHistory = async () => {
    if (currentId === null) return;
    const id = currentId;
    const view = viewRef.current;
    setHistory(null);
    try {
      const entries = await templateHistory(id);
      // The editor moved to another template while this was loading: its history is not ours to show.
      if (viewRef.current === view) setHistory(entries);
    } catch (err) {
      if (viewRef.current !== view) return;
      fail('Could not load the history')(err);
      setHistory('error');
    }
  };

  const loadDeleted = async () => {
    setDeleted(null);
    try {
      setDeleted(await deletedTemplates());
    } catch (err) {
      fail('Could not load the deleted templates')(err);
      setDeleted('error');
    }
  };

  // Opens a saved version beside the saved template. Nothing is written until Restore and Save.
  const reviewVersion = (e: HistoryEntry) => runExclusive(async () => {
    if (currentId === null || (dirty && !window.confirm(DISCARD_PROMPT))) return;
    try {
      const next = await templateVersion(currentId, e.version);
      const old = await invoke<TemplateJson>('api_get_template', { templateId: currentId });
      setReview({ old, next });
    } catch (err) {
      toast.error('Could not open this version', { description: String(err) });
    }
  });

  // Restore fills the editor only; Save persists it with the restore origin.
  const restoreVersion = (version: TemplateJson) => {
    const d = draftFromTemplate(version);
    setDraft(d);
    setNotes(null);
    pendingOrigin.current = { json: JSON.stringify(d), origin: { source: 'restore', note: '' } };
    setReview(null);
  };

  const restoreDeleted = (entry: DeletedTemplate) => runExclusive(async () => {
    if (dirty && !window.confirm(DISCARD_PROMPT)) return;
    try {
      const version = await templateVersion(entry.id, entry.version);
      const { templates } = await refresh();
      const d = draftFromTemplate(version);
      viewRef.current += 1;
      // A reused id opens as a new template, so Save cannot overwrite the one that now holds it.
      setCurrentId(templates.some(t => t.id === entry.id) ? null : entry.id);
      setDraft(d);
      setSavedJson('');
      setNotes(null);
      pendingOrigin.current = { json: JSON.stringify(d), origin: { source: 'restore', note: '' } };
    } catch (err) {
      toast.error('Could not restore the template', { description: String(err) });
    }
  });

  return (
    <div className="grid grid-cols-[16rem_1fr] gap-6">
      <div className="bg-card rounded-lg border border-border p-4 space-y-3 self-start">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="text-lg font-semibold text-foreground">Templates</h3>
          <Button variant="outline" size="sm" onClick={startNew} disabled={busy}>
            <Plus className="w-4 h-4 mr-1" />
            New template
          </Button>
          <Button variant="outline" size="sm" disabled={busy} onClick={() => {
            if (dirty && !window.confirm(DISCARD_PROMPT)) return;
            setAssist({ kind: 'create' });
          }}>
            <Sparkles className="w-4 h-4 mr-1" />
            Create with AI
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
                disabled={busy}
                aria-current={t.id === currentId ? 'true' : undefined}
                className={`w-full rounded-md px-3 py-2 text-left text-sm text-foreground ${t.id === currentId ? 'bg-muted font-medium' : 'hover:bg-muted/60'} disabled:opacity-60`}
              >
                {t.name}
                {!t.use_when?.trim() && !t.has_keywords && <span className="ml-2 text-xs text-muted-foreground">auto-pick off</span>}
              </button>
            </li>
          ))}
        </ul>
        <DropdownMenu onOpenChange={(open) => { if (open) void loadDeleted(); }}>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm" disabled={busy}>Deleted templates</Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            {deleted === null && <DropdownMenuItem disabled>Loading…</DropdownMenuItem>}
            {deleted === 'error' && <DropdownMenuItem disabled>Could not load the deleted templates</DropdownMenuItem>}
            {Array.isArray(deleted) && deleted.length === 0 && <DropdownMenuItem disabled>None</DropdownMenuItem>}
            {Array.isArray(deleted) && deleted.map(d => (
              <DropdownMenuItem key={`${d.id}:${d.version}`} onSelect={() => void restoreDeleted(d)}>
                {`${d.name} · ${new Date(d.deletedAt).toLocaleString()}`}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="bg-card rounded-lg border border-border p-6">
        {!draft ? (
          <p className="text-sm text-muted-foreground">No template selected.</p>
        ) : (
          <div className="space-y-5">
            {!!notes?.length && <div className="space-y-1 text-sm">
              <p className="font-medium">What I did</p>
              <ul className="list-disc pl-5">
                {notes.map((change, i) => <li key={i}>{change.field}: {change.note}</li>)}
              </ul>
            </div>}
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
            <Field id="tpl-prep" label="Prep instruction">
              <Textarea id="tpl-prep" rows={3} value={draft.prep} onChange={(e) => update({ prep: e.target.value })} />
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
              <Button onClick={save} disabled={busy || problem !== null}>Save</Button>
              <Button variant="outline" onClick={duplicate} disabled={busy || problem !== null}>
                <Copy className="w-4 h-4 mr-1" />
                Duplicate
              </Button>
              <Button variant="outline" onClick={improve} disabled={busy || dirty || currentId === null}
                title={dirty ? 'Save or discard your changes first' : undefined}>
                <Sparkles className="w-4 h-4 mr-1" />
                Improve with AI
              </Button>
              <DropdownMenu onOpenChange={(open) => { if (open) void loadHistory(); }}>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline" disabled={busy || !onDisk}>
                    <History className="w-4 h-4 mr-1" />
                    History
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  {history === null && <DropdownMenuItem disabled>Loading…</DropdownMenuItem>}
                  {history === 'error' && <DropdownMenuItem disabled>Could not load the history</DropdownMenuItem>}
                  {Array.isArray(history) && history.length === 0 && <DropdownMenuItem disabled>No earlier versions</DropdownMenuItem>}
                  {Array.isArray(history) && history.map(e => (
                    <DropdownMenuItem key={e.version} onSelect={() => void reviewVersion(e)}>
                      {`${new Date(e.savedAt).toLocaleString()} · ${historyLabel(e)}`}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
              <Button
                variant="outline"
                className="text-destructive"
                onClick={() => setConfirmDelete(true)}
                disabled={busy || !onDisk || currentId === STANDARD_ID}
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

      {assist && <TemplateAssistDialog
        open={true}
        onOpenChange={open => { if (!open) setAssist(null); }}
        mode={assist}
        onCreated={r => {
          viewRef.current += 1;
          setCurrentId(null);
          const d = draftFromTemplate(r.template);
          setDraft(d);
          setSavedJson('');
          setNotes(r.changes);
          pendingOrigin.current = { json: JSON.stringify(d), origin: { source: 'ai', note: r.changes[0]?.note ?? '' } };
        }}
        onAccepted={(merged, note, _base) => {
          const d = draftFromTemplate(merged);
          setDraft(d);
          pendingOrigin.current = { json: JSON.stringify(d), origin: { source: 'ai', note } };
        }}
      />}
      {review && <Dialog open={true} onOpenChange={(open) => { if (!open) setReview(null); }}>
        <DialogContent className="max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Restore an earlier version?</DialogTitle>
            <DialogDescription>Restoring fills the editor. Nothing is saved until you press Save.</DialogDescription>
          </DialogHeader>
          <TemplateDiffReview
            old={review.old} next={review.next} selectable={false} acceptLabel="Restore"
            onAccept={restoreVersion} onDiscard={() => setReview(null)}
          />
        </DialogContent>
      </Dialog>}
      <Dialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete this template?</DialogTitle>
            <DialogDescription>Its file moves to the template history. You can restore it from Deleted templates.</DialogDescription>
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
