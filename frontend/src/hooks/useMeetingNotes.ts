'use client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { newNote } from '@/lib/meeting-notes';
import type { MeetingNote } from '@/types';

// Module level: a page remount (leave and reopen) must not run a read ahead of a save still in flight.
const queues = new Map<string, Promise<unknown>>();

/** Runs `task` after the meeting's previous queued task, whether that one succeeded or failed. */
function enqueue<T>(meetingId: string, task: () => Promise<T>): Promise<T> {
  const run = (queues.get(meetingId) ?? Promise.resolve()).then(task);
  const tail = run.catch(() => {});
  queues.set(meetingId, tail);
  void tail.then(() => { if (queues.get(meetingId) === tail) queues.delete(meetingId); });
  return run;
}

export function useMeetingNotes(meetingId: string) {
  const [notes, setNotes] = useState<MeetingNote[]>([]);
  const [loaded, setLoaded] = useState(false);
  const notesRef = useRef<MeetingNote[]>([]);      // what the UI shows (optimistic)
  const confirmedRef = useRef<MeetingNote[]>([]);  // what the backend last stored
  const loadedRef = useRef(false);
  const saveSeq = useRef(0);

  useEffect(() => {
    let cancelled = false;
    loadedRef.current = false;
    setLoaded(false);
    enqueue(meetingId, () => invoke<MeetingNote[]>('get_meeting_notes', { meetingId }))
      .then((n) => {
        if (cancelled) return;
        notesRef.current = n; confirmedRef.current = n; setNotes(n);
        loadedRef.current = true; setLoaded(true);
      })
      .catch((e) => {
        console.error('Failed to load notes:', e);
        toast.error('Could not load notes');
      });
    return () => { cancelled = true; };
  }, [meetingId]);

  const save = useCallback((next: MeetingNote[]) => {
    if (!loadedRef.current) return Promise.resolve(); // never overwrite notes we have not read yet
    const seq = ++saveSeq.current;
    notesRef.current = next;
    setNotes(next);
    // Queued per meeting: the backend sees saves in order, and every success updates confirmedRef.
    return enqueue(meetingId, async () => {
      try {
        const stored = await invoke<MeetingNote[]>('save_meeting_notes', { meetingId, notes: next });
        confirmedRef.current = stored;
        if (seq === saveSeq.current) { notesRef.current = stored; setNotes(stored); }
      } catch (e) {
        if (seq === saveSeq.current) { notesRef.current = confirmedRef.current; setNotes(confirmedRef.current); }
        toast.error('Could not save notes', { description: String(e) });
      }
    });
  }, [meetingId]);

  return {
    notes,
    loaded,
    addNote: (text: string) => save([...notesRef.current, newNote(text, null)]),
    updateNote: (id: string, text: string) => save(notesRef.current.map((n) => (n.id === id ? { ...n, text } : n))),
    deleteNote: (id: string) => save(notesRef.current.filter((n) => n.id !== id)),
  };
}
