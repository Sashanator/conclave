import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import type { CategoryInfo } from '@/lib/meeting-views';
import { deleteMeeting, listMeetingRows, setReviewed, setTemplate, type MeetingRow } from '@/lib/meetings';

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function useMeetingRows(templates: CategoryInfo[] = []) {
  const [rows, setRows] = useState<MeetingRow[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState(false);
  const seq = useRef(0); // a slower, older reload must not overwrite a newer one
  const errorToasted = useRef(false);
  const liveId = useRecordingState().liveMeetingId;
  const { refetchMeetings } = useSidebar();
  const alive = useRef(true);
  const pending = useRef(new Map<string, Partial<MeetingRow>>()); // edits still saving; laid over every reload

  const reload = useCallback(async () => {
    const mine = ++seq.current;
    try {
      const next = await listMeetingRows();
      if (alive.current && mine === seq.current) { setRows(next.map((r) => (pending.current.has(r.id) ? { ...r, ...pending.current.get(r.id) } : r))); setLoaded(true); setError(false); errorToasted.current = false; }
    } catch (e) {
      if (!alive.current || mine !== seq.current) return;
      setError(true);
      if (!errorToasted.current) { errorToasted.current = true; toast.error('Could not load meetings', { description: errText(e) }); }
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void reload();
    const onFocus = () => void reload();
    window.addEventListener('focus', onFocus);
    const unlisten = ['recording-started', 'recording-saved', 'recording-target'].map((n) => Promise.resolve(listen(n, () => void reload())).catch((e) => { console.error(`Could not listen for ${n}`, e); return () => {}; }));
    return () => {
      alive.current = false;
      window.removeEventListener('focus', onFocus);
      unlisten.forEach((u) => u.then((f) => f()).catch(() => {}));
    };
  }, [reload]);

  const patch = (id: string, p: Partial<MeetingRow>) =>
    setRows((rs) => rs.map((r) => (r.id === id ? { ...r, ...p } : r)));
  const hold = (id: string, p: Partial<MeetingRow>) => { pending.current.set(id, { ...pending.current.get(id), ...p }); patch(id, p); };
  const release = (id: string, keys: (keyof MeetingRow)[]) => {
    const cur = { ...pending.current.get(id) };
    keys.forEach((k) => delete cur[k]);
    if (Object.keys(cur).length) pending.current.set(id, cur); else pending.current.delete(id);
  };

  const toggleReviewed = useCallback(async (id: string) => {
    const row = rows.find((r) => r.id === id);
    if (!row) return;
    seq.current++; // an older reload must not undo this edit
    hold(id, { reviewed: !row.reviewed });
    try {
      await setReviewed(id, !row.reviewed);
      seq.current++;
      release(id, ['reviewed']);
    } catch (e) {
      release(id, ['reviewed']);
      patch(id, { reviewed: row.reviewed });
      toast.error('Could not save Reviewed', { description: errText(e) });
    }
  }, [rows]);

  const changeTemplate = useCallback(async (id: string, templateId: string | null) => {
    const row = rows.find((r) => r.id === id);
    if (!row) return;
    seq.current++;
    hold(id, { templateId, emoji: templates.find((t) => t.id === templateId)?.emoji ?? null });
    try {
      await setTemplate(id, templateId);
      seq.current++;
      release(id, ['templateId', 'emoji']);
    } catch (e) {
      release(id, ['templateId', 'emoji']);
      patch(id, { templateId: row.templateId, emoji: row.emoji });
      toast.error('Could not save the template', { description: errText(e) });
    }
  }, [rows, templates]);

  const removeMeeting = useCallback(async (id: string) => {
    seq.current++;
    try {
      const { folderWarning } = await deleteMeeting(id);
      seq.current++;
      setRows((rs) => rs.filter((r) => r.id !== id));
      await refetchMeetings();
      if (folderWarning) toast.warning('The meeting was deleted, but its folder was kept', { description: folderWarning });
    } catch (e) {
      toast.error('Could not delete the meeting', { description: errText(e) });
    }
  }, [refetchMeetings]);

  return { rows, loaded, error, liveId, reload, toggleReviewed, changeTemplate, removeMeeting };
}
