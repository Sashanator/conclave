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
  const saved = useRef(new Map<string, string>()); // last configJson known to be stored, per view
  const inflight = useRef(new Map<string, Promise<void>>()); // a save on its way to the backend, per view

  useEffect(() => {
    listViews()
      .then((v) => {
        v.forEach((x) => saved.current.set(x.id, x.configJson));
        setViews([...v].sort((a, b) => a.position - b.position));
      })
      .catch((e) => toast.error('Could not load views', { description: errText(e) }));
  }, []);

  const active = views.find((v) => v.id === activeId) ?? views[0] ?? null;
  const config = useMemo(() => parseViewConfig(active?.id ?? '', active?.configJson ?? '{}'), [active]);

  const save = useCallback((view: MeetingView) => {
    const p: Promise<void> = saveView(view).then(() => { saved.current.set(view.id, view.configJson); }).catch((e) => {
      const old = saved.current.get(view.id);
      // A failed debounced save must not leave the screen showing a config that was never stored.
      if (old !== undefined && pending.current?.id !== view.id) setViews((vs) => vs.map((v) => (v.id === view.id ? { ...v, configJson: old } : v)));
      toast.error('Could not save the view', { description: errText(e) });
    }).finally(() => { if (inflight.current.get(view.id) === p) inflight.current.delete(view.id); });
    inflight.current.set(view.id, p);
    return p;
  }, []);

  const flush = useCallback((): Promise<void> => {
    clearTimeout(timer.current);
    const v = pending.current;
    pending.current = null;
    return v ? save(v) : Promise.resolve();
  }, [save]);
  useEffect(() => () => { void flush(); }, [flush]); // unmount must not drop the last edit

  const dispatch = useCallback((action: ViewAction) => {
    if (!active) return;
    // A queued edit for another view goes out before this one replaces it.
    if (pending.current && pending.current.id !== active.id) flush();
    // Two dispatches in one handler share a stale `config`; build on the queued edit instead.
    const base = pending.current?.id === active.id ? parseViewConfig(active.id, pending.current.configJson) : config;
    const next = { ...active, configJson: JSON.stringify(viewReducer(base, action)) };
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
      saved.current.set(view.id, view.configJson);
    } catch (e) {
      setViews((vs) => vs.filter((v) => v.id !== view.id));
      activate(active.id);
      toast.error('Could not save the view', { description: errText(e) });
    }
  }, [active, config, views, flush]);

  const renameView = useCallback(async (id: string, name: string) => {
    const old = views.find((v) => v.id === id);
    if (!old || old.builtin) return;
    await flush(); // a queued save carries the old name and would undo this one; it must land first
    setViews((vs) => vs.map((v) => (v.id === id ? { ...v, name } : v)));
    try {
      await saveView({ ...old, name });
    } catch (e) {
      setViews((vs) => vs.map((v) => (v.id === id ? old : v)));
      toast.error('Could not save the view', { description: errText(e) });
    }
  }, [views, flush]);

  const removeView = useCallback(async (id: string) => {
    const old = views.find((v) => v.id === id);
    if (!old || old.builtin) return;
    // A queued save would upsert the view back after the delete.
    const queued = pending.current?.id === id ? pending.current : null;
    if (queued) { clearTimeout(timer.current); pending.current = null; }
    await inflight.current.get(id); // an earlier save still on its way would upsert the view back
    try {
      await deleteView(id);
    } catch (e) {
      // The view stays, so its unsaved edit must not be dropped; a newer edit wins.
      if (queued && !pending.current) { pending.current = queued; timer.current = setTimeout(flush, 500); }
      toast.error('Could not delete the view', { description: errText(e) });
      return;
    }
    setViews((vs) => vs.filter((v) => v.id !== id));
    if (active?.id === id) activate(views.find((v) => v.builtin)?.id ?? 'all');
  }, [views, active, flush]);

  return { views, active, config, dispatch, addView, renameView, removeView, setActive: activate };
}
