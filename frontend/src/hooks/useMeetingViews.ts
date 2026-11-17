import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';
import { listViews, saveView, deleteView, type MeetingView } from '@/lib/meetings';
import { parseViewConfig, viewReducer, type ViewAction } from '@/lib/meeting-views';

const KEY = 'conclave.meetingsView';
const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));
const readActive = () => { try { return localStorage.getItem(KEY); } catch { return null; } };
const writeActive = (id: string) => { try { localStorage.setItem(KEY, id); } catch { /* per-viewer convenience only */ } };

export function useMeetingViews() {
  const [views, setViews] = useState<MeetingView[]>([]);
  const [activeId, setActiveId] = useState<string | null>(readActive);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const pending = useRef<MeetingView | null>(null);

  useEffect(() => {
    listViews()
      .then((v) => setViews([...v].sort((a, b) => a.position - b.position)))
      .catch((e) => toast.error('Could not load views', { description: errText(e) }));
  }, []);

  const active = views.find((v) => v.id === activeId) ?? views[0] ?? null;
  const config = useMemo(() => parseViewConfig(active?.id ?? '', active?.configJson ?? '{}'), [active]);

  const save = useCallback((view: MeetingView) =>
    saveView(view).catch((e) => toast.error('Could not save the view', { description: errText(e) })), []);

  const flush = useCallback(() => {
    clearTimeout(timer.current);
    const v = pending.current;
    pending.current = null;
    if (v) void save(v);
  }, [save]);
  useEffect(() => flush, [flush]); // unmount must not drop the last edit

  const dispatch = useCallback((action: ViewAction) => {
    if (!active) return;
    // A queued edit for another view goes out before this one replaces it.
    if (pending.current && pending.current.id !== active.id) flush();
    const next = { ...active, configJson: JSON.stringify(viewReducer(config, action)) };
    setViews((vs) => vs.map((v) => (v.id === next.id ? next : v)));
    pending.current = next;
    clearTimeout(timer.current);
    timer.current = setTimeout(flush, 500);
  }, [active, config, flush]);

  const activate = (id: string) => { setActiveId(id); writeActive(id); };

  const addView = useCallback(async () => {
    if (!active) return;
    flush();
    const view: MeetingView = {
      id: crypto.randomUUID(), name: 'New view', builtin: false,
      position: Math.max(0, ...views.map((v) => v.position)) + 1,
      configJson: JSON.stringify(config),
    };
    setViews((vs) => [...vs, view]);
    activate(view.id);
    try {
      await saveView(view);
    } catch (e) {
      setViews((vs) => vs.filter((v) => v.id !== view.id));
      activate(active.id);
      toast.error('Could not save the view', { description: errText(e) });
    }
  }, [active, config, views, flush]);

  const renameView = useCallback(async (id: string, name: string) => {
    const old = views.find((v) => v.id === id);
    if (!old || old.builtin) return;
    setViews((vs) => vs.map((v) => (v.id === id ? { ...v, name } : v)));
    try {
      await saveView({ ...old, name });
    } catch (e) {
      setViews((vs) => vs.map((v) => (v.id === id ? old : v)));
      toast.error('Could not save the view', { description: errText(e) });
    }
  }, [views]);

  const removeView = useCallback(async (id: string) => {
    const old = views.find((v) => v.id === id);
    if (!old || old.builtin) return;
    try {
      await deleteView(id);
    } catch (e) {
      toast.error('Could not delete the view', { description: errText(e) });
      return;
    }
    setViews((vs) => vs.filter((v) => v.id !== id));
    if (active?.id === id) activate(views.find((v) => v.builtin)?.id ?? 'all');
  }, [views, active]);

  return { views, active, config, dispatch, addView, renameView, removeView, setActive: activate };
}
