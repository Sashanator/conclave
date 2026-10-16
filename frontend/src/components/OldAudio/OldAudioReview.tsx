'use client';
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { formatMegabytes, recordingCount, toggleId, type DeleteResult, type OldAudio } from '@/lib/old-audio';
import { AUDIO_DELETED_EVENT } from '@/lib/meeting-audio';

export function OldAudioReview({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [items, setItems] = useState<OldAudio[] | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setSelected([]);
    setItems(null);
    try {
      setItems(await invoke<OldAudio[]>('old_audio_list'));
    } catch (e) {
      setItems([]);
      toast.error('Could not list old recordings', { description: String(e) });
    }
  }, []);

  useEffect(() => { if (open) void load(); }, [open, load]);

  const remove = async () => {
    setBusy(true);
    try {
      const r = await invoke<DeleteResult>('delete_old_audio', { meetingIds: selected });
      if (r.meetings > 0) toast.success(`Moved ${recordingCount(r.meetings)} (${formatMegabytes(r.bytes)}) to the Recycle Bin`);
      for (const f of r.failed) toast.warning('Not moved to the Recycle Bin', { description: f });
      window.dispatchEvent(new Event(AUDIO_DELETED_EVENT));
      await load();
    } catch (e) {
      toast.error('Could not delete the selected audio', { description: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const total = (items ?? []).filter((i) => selected.includes(i.meeting_id)).reduce((s, i) => s + i.bytes, 0);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Review old recordings</DialogTitle>
          <DialogDescription>
            Selected recordings go to the Windows Recycle Bin. Space is freed when you empty it. Transcripts, summaries and notes stay.
          </DialogDescription>
        </DialogHeader>
        {items === null ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : items.length === 0 ? (
          <p className="text-sm text-muted-foreground">No recordings are old enough to review.</p>
        ) : (
          <>
            <div className="flex gap-3 text-sm">
              <button type="button" className="text-blue-600 dark:text-blue-400 hover:underline" onClick={() => setSelected(items.map((i) => i.meeting_id))}>Select all</button>
              <button type="button" className="text-blue-600 dark:text-blue-400 hover:underline" onClick={() => setSelected([])}>Select none</button>
            </div>
            <ul className="max-h-[50vh] space-y-2 overflow-y-auto">
              {items.map((i) => (
                <li key={i.meeting_id} className="rounded border border-border p-2">
                  <label className="flex cursor-pointer items-start gap-2">
                    <input
                      type="checkbox"
                      className="mt-1"
                      checked={selected.includes(i.meeting_id)}
                      onChange={() => setSelected((s) => toggleId(s, i.meeting_id))}
                      aria-label={`Select ${i.title}`}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block text-sm font-medium">{i.title}</span>
                      <span className="block text-xs text-muted-foreground">
                        {new Date(i.created_at).toLocaleDateString()} · {formatMegabytes(i.bytes)}
                      </span>
                      {i.files.map((f) => (
                        <span key={f} className="block break-all font-mono text-[11px] text-muted-foreground">{f}</span>
                      ))}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          </>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Close</Button>
          <Button variant="destructive" disabled={busy || selected.length === 0} onClick={remove}>
            Move {selected.length} to Recycle Bin{selected.length ? ` (${formatMegabytes(total)})` : ''}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
