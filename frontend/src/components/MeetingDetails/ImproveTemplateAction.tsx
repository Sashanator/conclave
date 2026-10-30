'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { TemplateAssistDialog } from '@/components/TemplateAssistDialog';
import { getAssistSettings, getTemplateMeetingInfo, type TemplateMeetingInfo } from '@/lib/templateAssist';
import type { TemplateJson } from '@/lib/template-draft';
import { diffItems } from '@/lib/templateDiff';

type Pending = { merged: TemplateJson; note: string; base: TemplateJson };
type Loaded = { saved: TemplateJson; meeting: TemplateMeetingInfo; includeTranscript: boolean };

export function ImproveTemplateAction(props: {
  meetingId: string; templateId: string; templateExists: boolean; open: boolean;
  onOpenChange: (open: boolean) => void; onRegenerate: (templateId: string) => boolean | void;
}) {
  const { meetingId, templateId, templateExists, open, onOpenChange, onRegenerate } = props;
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const regeneratingRef = useRef(false);
  const versionRef = useRef(0);

  useEffect(() => {
    const version = ++versionRef.current;
    setLoaded(null);
    setPending(null);
    setError(null);
    setStale(false);
    setSaved(false);
    setSaving(false);
    savingRef.current = false;
    regeneratingRef.current = false;
    if (open && templateExists && templateId) {
      Promise.all([
        getTemplateMeetingInfo(meetingId),
        invoke<TemplateJson>('api_get_template', { templateId }),
        getAssistSettings(),
      ]).then(([meeting, template, settings]) => {
        if (version === versionRef.current) setLoaded({ meeting, saved: template, includeTranscript: settings.includeTranscript });
      }).catch(err => {
        if (version === versionRef.current) setError(String(err));
      });
    }
    return () => { versionRef.current += 1; };
  }, [open, meetingId, templateId, templateExists]);

  const changeOpen = (next: boolean) => {
    if (savingRef.current) return;
    versionRef.current += 1;
    onOpenChange(next);
  };

  const save = async () => {
    if (!pending || savingRef.current || stale || saved) return;
    const version = versionRef.current;
    savingRef.current = true;
    setSaving(true);
    setError(null);
    try {
      const current = await invoke<TemplateJson>('api_get_template', { templateId });
      if (version !== versionRef.current) return;
      if (JSON.stringify(current) !== JSON.stringify(pending.base)) {
        setStale(true);
        return;
      }
      await invoke('api_save_template', {
        templateId, template: pending.merged, origin: { source: 'ai', note: pending.note },
      });
      if (version === versionRef.current) setSaved(true);
    } catch (err) {
      if (version === versionRef.current) setError(String(err));
    } finally {
      if (version === versionRef.current) {
        savingRef.current = false;
        setSaving(false);
      }
    }
  };

  if (!templateExists || !templateId) return null;

  const confirm = saved ? <div className="space-y-4 text-sm">
    <p>Regenerate this summary with the updated template</p>
    <div className="flex justify-end gap-2">
      <Button variant="outline" onClick={() => changeOpen(false)}>Close</Button>
      <Button onClick={() => {
        if (regeneratingRef.current) return;
        regeneratingRef.current = true;
        if (onRegenerate(templateId) === false) regeneratingRef.current = false;
      }}>Regenerate</Button>
    </div>
  </div> : pending ? <div className="space-y-4 text-sm">
    <h3 className="font-medium">These changes will be saved:</h3>
    <ul className="list-disc pl-5">{diffItems(pending.base, pending.merged).map(i => <li key={i.key}>{i.label}</li>)}</ul>
    {stale && <p role="alert" className="text-destructive">The template changed since this proposal was made.</p>}
    {error && <p role="alert" className="text-destructive">{error}</p>}
    <div className="flex justify-end gap-2">
      <Button variant="outline" disabled={saving} onClick={() => { setPending(null); setStale(false); setError(null); }}>Back</Button>
      {stale && <Button variant="outline" onClick={() => changeOpen(false)}>Close</Button>}
      <Button disabled={saving || stale} onClick={save}>Save template</Button>
    </div>
  </div> : null;

  return <>
    <Button variant="outline" size="sm" onClick={() => changeOpen(true)}>Improve template…</Button>
    <Dialog open={open && !loaded} onOpenChange={changeOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Improve template</DialogTitle>
          <DialogDescription>Generate a template draft to review and save.</DialogDescription>
        </DialogHeader>
        {error ? <p role="alert" className="text-destructive">{error}</p> : <p role="status">Loading template…</p>}
        <Button variant="outline" size="sm" onClick={() => changeOpen(false)}>Close</Button>
      </DialogContent>
    </Dialog>
    {open && loaded && <TemplateAssistDialog open={open} onOpenChange={changeOpen}
      mode={{ kind: 'improve', templateId, saved: loaded.saved }}
      meeting={{ meetingId, hasEdits: loaded.meeting.hasEdits, transcriptTokens: loaded.meeting.transcriptTokens,
        includeTranscriptDefault: loaded.includeTranscript }}
      onCreated={() => {}} onAccepted={(merged, note, base) => setPending({ merged, note, base })}
      confirm={confirm} />}
  </>;
}
