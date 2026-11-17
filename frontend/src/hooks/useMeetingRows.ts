import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { deleteMeeting, listMeetingRows, setReviewed, setTemplate, type MeetingRow } from '@/lib/meetings';

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function useMeetingRows() {
  const [rows, setRows] = useState<MeetingRow[]>([]);
  const [loaded, setLoaded] = useState(false);
  // shortcut: cast until `liveMeetingId` is on the context type (Task 13); drop the cast then.
  const liveId = (useRecordingState() as { liveMeetingId?: string | null }).liveMeetingId ?? null;
  const { refetchMeetings } = useSidebar();
  const alive = useRef(true);

  const reload = useCallback(async () => {
    try {
      const next = await listMeetingRows();
      if (alive.current) { setRows(next); setLoaded(true); }
    } catch (e) {
      toast.error('Could not load meetings', { description: errText(e) });
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void reload();
    const onFocus = () => void reload();
    window.addEventListener('focus', onFocus);
    const unlisten = ['recording-started', 'recording-saved', 'recording-target'].map((n) => listen(n, () => void reload()));
    return () => {
      alive.current = false;
      window.removeEventListener('focus', onFocus);
      unlisten.forEach((u) => u.then((f) => f()));
    };
  }, [reload]);

  const patch = (id: string, p: Partial<MeetingRow>) =>
    setRows((rs) => rs.map((r) => (r.id === id ? { ...r, ...p } : r)));

  const toggleReviewed = useCallback(async (id: string) => {
    const row = rows.find((r) => r.id === id);
    if (!row) return;
    patch(id, { reviewed: !row.reviewed });
    try {
      await setReviewed(id, !row.reviewed);
    } catch (e) {
      patch(id, { reviewed: row.reviewed });
      toast.error('Could not save Reviewed', { description: errText(e) });
    }
  }, [rows]);

  const changeTemplate = useCallback(async (id: string, templateId: string | null) => {
    const row = rows.find((r) => r.id === id);
    if (!row) return;
    patch(id, { templateId });
    try {
      await setTemplate(id, templateId);
    } catch (e) {
      patch(id, { templateId: row.templateId });
      toast.error('Could not save the template', { description: errText(e) });
    }
  }, [rows]);

  const removeMeeting = useCallback(async (id: string) => {
    try {
      const { folderWarning } = await deleteMeeting(id);
      setRows((rs) => rs.filter((r) => r.id !== id));
      await refetchMeetings();
      if (folderWarning) toast.warning('The meeting was deleted, but its folder was kept', { description: folderWarning });
    } catch (e) {
      toast.error('Could not delete the meeting', { description: errText(e) });
    }
  }, [refetchMeetings]);

  return { rows, loaded, liveId, reload, toggleReviewed, changeTemplate, removeMeeting };
}
