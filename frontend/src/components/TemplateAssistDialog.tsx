'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { PROVIDERS } from '@/components/shared/LlmOverridePicker';
import { draftTemplate, reviseTemplate, estimateCostUsd, getAssistSettings, splitRawAnswer, type AssistResult } from '@/lib/templateAssist';
import { TemplateDiffReview } from '@/components/TemplateDiffReview';
import type { TemplateJson } from '@/lib/template-draft';

export type AssistMode = { kind: 'create' } | { kind: 'improve'; templateId: string; saved: TemplateJson };
type ModelConfig = { provider: string; model: string };
type PriceRow = ModelConfig & { input: number; output: number; cache_read: number; cache_write: number; built_in: boolean };
const SYSTEM_PROMPT_CHARS = 8000; // shortcut: rough prompt size, replace with the real length if the estimate misleads

export function TemplateAssistDialog(props: {
  open: boolean; onOpenChange: (open: boolean) => void; mode: AssistMode;
  onCreated: (r: AssistResult) => void;
  onAccepted: (merged: TemplateJson, note: string, base: TemplateJson) => void;
  checkStale?: boolean;
  meeting?: { meetingId: string; hasEdits: boolean; transcriptTokens: number; includeTranscriptDefault: boolean };
  confirm?: ReactNode | null;
}): JSX.Element {
  const { open, onOpenChange, mode, onCreated } = props;
  const [proposal, setProposal] = useState<AssistResult | null>(null);
  const [refining, setRefining] = useState(false);
  const [refinement, setRefinement] = useState('');
  const [notice, setNotice] = useState<string | null>(null);
  const warnedRef = useRef<AssistResult | null>(null);
  const [text, setText] = useState('');
  const [includeEdits, setIncludeEdits] = useState(false);
  const [includeTranscript, setIncludeTranscript] = useState(false);
  const canGenerate = !!text.trim() || !!(props.meeting?.hasEdits && includeEdits);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<ReturnType<typeof splitRawAnswer> | null>(null);
  const [model, setModel] = useState<ModelConfig | null>(null);
  const [prices, setPrices] = useState<PriceRow[]>([]);
  const runRef = useRef(0);
  const runningRef = useRef(false);

  useEffect(() => {
    const run = ++runRef.current;
    runningRef.current = false;
    setRunning(false);
    setError(null);
    setText('');
    setIncludeEdits(props.meeting?.hasEdits ?? false);
    setIncludeTranscript(props.meeting?.includeTranscriptDefault ?? false);
    setProposal(null);
    setRefining(false);
    setRefinement('');
    setNotice(null);
    warnedRef.current = null;
    setModel(null);
    setPrices([]);
    if (open) {
      // These are optional display hints. Generate reports any missing model configuration.
      Promise.all([
        getAssistSettings().catch(() => undefined),
        invoke<ModelConfig | undefined>('api_get_model_config').catch(() => undefined),
        invoke<PriceRow[] | undefined>('costs_get_prices').catch(() => undefined),
      ]).then(([settings, summary, rows]) => {
        if (run !== runRef.current) return;
        const chosen = settings?.provider && settings.model ? settings : summary;
        setModel(chosen?.provider && chosen.model ? { provider: chosen.provider, model: chosen.model } : null);
        setPrices(rows ?? []);
      });
    }
    return () => { runRef.current += 1; };
  }, [open]);

  const changeOpen = (next: boolean) => {
    runRef.current += 1;
    onOpenChange(next);
  };

  const generate = async () => {
    if (!open || runningRef.current || !canGenerate || (proposal && !refinement.trim())) return;
    const run = runRef.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    try {
      const result = mode.kind === 'create' ? await draftTemplate(text)
        : await reviseTemplate({ templateId: mode.templateId, request: text,
          ...(props.meeting ? { meetingId: props.meeting.meetingId, includeEdits, includeTranscript } : {}),
          ...(proposal ? { previous: proposal, refinement } : {}) });
      if (run !== runRef.current) return;
      if (mode.kind === 'create') {
        onCreated(result);
        changeOpen(false);
      } else {
        setProposal(result);
        setRefining(false);
        setRefinement('');
        setNotice(null);
        warnedRef.current = null;
      }
    } catch (err) {
      if (run === runRef.current) setError(splitRawAnswer(err));
    } finally {
      if (run === runRef.current) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  const accept = async (merged: TemplateJson) => {
    if (!open || mode.kind !== 'improve' || !proposal || runningRef.current) return;
    const run = runRef.current;
    const base = proposal.base ?? mode.saved;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    try {
      if (!props.meeting && props.checkStale !== false) {
        const saved = await invoke<TemplateJson>('api_get_template', { templateId: mode.templateId });
        if (run !== runRef.current) return;
        if (JSON.stringify(saved) !== JSON.stringify(base) && warnedRef.current !== proposal) {
          warnedRef.current = proposal;
          setNotice('The template changed since this proposal was made.');
          return;
        }
      }
      if (run !== runRef.current) return;
      props.onAccepted(merged, proposal.changes[0]?.note ?? '', base);
      if (!props.meeting) changeOpen(false);
    } catch (err) {
      if (run === runRef.current) setError(splitRawAnswer(err));
    } finally {
      if (run === runRef.current) {
        runningRef.current = false;
        setRunning(false);
      }
    }
  };

  const row = prices.find(p => p.provider === model?.provider && p.model === model?.model);
  const savedChars = mode.kind === 'improve' ? JSON.stringify(mode.saved).length : 0;
  const estimate = row ? estimateCostUsd(text.length + SYSTEM_PROMPT_CHARS + savedChars,
    mode.kind === 'improve' ? savedChars + 1000 : 6000, row) : null;

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent className="max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{mode.kind === 'create' ? 'Create with AI' : 'Improve template'}</DialogTitle>
          <DialogDescription>Generate a template draft to review and save.</DialogDescription>
        </DialogHeader>
        {props.confirm ?? <>
        {proposal && mode.kind === 'improve' ? <TemplateDiffReview
          old={proposal.base ?? mode.saved} next={proposal.template}
          changes={proposal.changes} warnings={proposal.warnings} selectable busy={running}
          acceptLabel="Accept selected" notice={notice} onAccept={accept} onDiscard={() => changeOpen(false)}
        >
          {refining ? <div className="space-y-2">
            <Label htmlFor="template-assist-refinement">What should be different?</Label>
            <Textarea id="template-assist-refinement" rows={3} value={refinement}
              onChange={e => setRefinement(e.target.value)} disabled={running} />
            <Button onClick={generate} disabled={running || !refinement.trim()}>
              {running && <Loader2 className="w-4 h-4 mr-1 animate-spin" />}Generate
            </Button>
          </div> : <Button variant="outline" disabled={running} onClick={() => setRefining(true)}>Ask for a different change</Button>}
        </TemplateDiffReview> : <div className="space-y-2">
          <Label htmlFor="template-assist-request">{mode.kind === 'create'
            ? 'Describe the meeting and the notes you want, or paste an existing template (Notion etc.).'
            : props.meeting ? 'What was wrong with this summary?' : 'What should change?'}</Label>
          <Textarea id="template-assist-request" rows={6} value={text} onChange={e => setText(e.target.value)} disabled={running} />
          {props.meeting && <>
            {props.meeting.hasEdits && <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeEdits} disabled={running} onChange={e => setIncludeEdits(e.target.checked)} />
              Your edits to this summary will be included
            </label>}
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={includeTranscript} disabled={running} onChange={e => setIncludeTranscript(e.target.checked)} />
              {`Include the transcript (more accurate, costs more: ~${props.meeting.transcriptTokens.toLocaleString()} tokens)`}
            </label>
          </>}
          {model && <p className="text-xs text-muted-foreground">
            Uses: {model.model} ({PROVIDERS.find(p => p.id === model.provider)?.label ?? model.provider})
            {estimate !== null && ` · about $${estimate.toFixed(3)}`}
          </p>}
        </div>}
        {error && <div className="space-y-2 text-sm">
            <p role="alert" className="text-destructive">{error.message}</p>
            {error.raw !== null && <details>
              <summary>Show raw answer</summary>
              <pre className="whitespace-pre-wrap max-h-64 overflow-auto">{error.raw}</pre>
            </details>}
          </div>}
        {!proposal && <DialogFooter>
          <Button variant="outline" onClick={() => changeOpen(false)}>Cancel</Button>
          <Button onClick={generate} disabled={running || !canGenerate}>
            {running && <Loader2 className="w-4 h-4 mr-1 animate-spin" />}
            Generate
          </Button>
        </DialogFooter>}
        </>}
      </DialogContent>
    </Dialog>
  );
}
