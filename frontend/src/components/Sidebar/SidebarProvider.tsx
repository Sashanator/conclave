'use client';

import React, { createContext, useContext, useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { continueFailed } from '@/lib/resume';
import { readSidebarCollapsed, writeSidebarCollapsed } from '@/lib/nav';
import type { SummaryProcessResponse } from '@/types';



interface SummaryPoll {
  processId: string;
  timer: NodeJS.Timeout;
  inFlight: boolean;
}

interface SidebarContextType {
  collapsed: boolean;
  toggleCollapsed: () => void;
  serverAddress: string;
  // Summary polling management
  startSummaryPolling: (
    meetingId: string,
    processId: string,
    onUpdate: (result: SummaryProcessResponse) => void | Promise<void>
  ) => void;
  stopSummaryPolling: (meetingId: string, processId?: string) => void;
}

const SidebarContext = createContext<SidebarContextType | null>(null);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

// same rule as isTypingTarget (hooks/useBackShortcut.ts)
const isTypingTarget = (t: EventTarget | null) =>
  t instanceof Element && !!t.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])');

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [collapsed, setCollapsed] = useState(true);
  const [serverAddress, setServerAddress] = useState('');
  const summaryPollsRef = React.useRef(new Map<string, SummaryPoll>());

  useEffect(() => {
    const failed = listen<{ error: string }>('continue-target-failed', (e) => continueFailed(e.payload.error));
    return () => { failed.then((unlisten) => unlisten()); };
  }, []);

  useEffect(() => {
    const fetchSettings = async () => {
      setServerAddress('http://localhost:5167');
    };
    fetchSettings();
  }, []);

  // Read after mount: the static export prerenders the shell, so a lazy initializer would mismatch hydration.
  useEffect(() => setCollapsed(readSidebarCollapsed(window.innerWidth)), []);

  const toggleCollapsed = React.useCallback(() => {
    setCollapsed((c) => {
      writeSidebarCollapsed(!c);
      return !c;
    });
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return;
      if (e.key.toLowerCase() !== 'b' && e.code !== 'KeyB') return;
      if (isTypingTarget(e.target)) return;
      e.preventDefault();
      toggleCollapsed();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [toggleCollapsed]);

  // Summary polling management
  const stopSummaryPolling = React.useCallback((meetingId: string, processId?: string) => {
    const poll = summaryPollsRef.current.get(meetingId);
    if (!poll || (processId && poll.processId !== processId)) {
      return;
    }
    clearInterval(poll.timer);
    summaryPollsRef.current.delete(meetingId);
  }, []);

  const startSummaryPolling = React.useCallback((
    meetingId: string,
    processId: string,
    onUpdate: (result: SummaryProcessResponse) => void | Promise<void>
  ) => {
    stopSummaryPolling(meetingId);
    let pollCount = 0;
    const maxPolls = 200;
    const poll = async () => {
      const entry = summaryPollsRef.current.get(meetingId);
      if (!entry || entry.processId !== processId || entry.inFlight) {
        return;
      }
      entry.inFlight = true;
      try {
        pollCount += 1;
        if (pollCount >= maxPolls) {
          await onUpdate({
            status: 'error',
            meetingName: null,
            meeting_id: meetingId,
            start: processId,
            end: null,
            data: null,
            error: 'Summary generation timed out after 15 minutes. Please try again or check your model configuration.',
          });
          if (summaryPollsRef.current.get(meetingId) === entry) {
            stopSummaryPolling(meetingId, processId);
          }
          return;
        }

        const result = await invoke<SummaryProcessResponse>('api_get_summary', { meetingId });
        const current = summaryPollsRef.current.get(meetingId);
        if (current !== entry || result.start !== processId) {
          return;
        }
        await onUpdate(result);
        if (summaryPollsRef.current.get(meetingId) !== entry) return;
        if (
          result.status === 'completed'
          || result.status === 'error'
          || result.status === 'failed'
          || result.status === 'cancelled'
          || (result.status === 'idle' && pollCount > 1)
        ) {
          stopSummaryPolling(meetingId, processId);
        }
      } catch (error) {
        const current = summaryPollsRef.current.get(meetingId);
        if (current !== entry) {
          return;
        }
        try {
          await onUpdate({
            status: 'error',
            meetingName: null,
            meeting_id: meetingId,
            start: processId,
            end: null,
            data: null,
            error: error instanceof Error ? error.message : 'Unknown error',
          });
        } catch (callbackError) {
          console.error('Failed to handle summary polling error:', callbackError);
        } finally {
          if (summaryPollsRef.current.get(meetingId) === entry) {
            stopSummaryPolling(meetingId, processId);
          }
        }
      } finally {
        const current = summaryPollsRef.current.get(meetingId);
        if (current === entry) {
          current.inFlight = false;
        }
      }
    };

    const timer = setInterval(() => void poll(), 5000);
    summaryPollsRef.current.set(meetingId, { processId, timer, inFlight: false });
  }, [stopSummaryPolling]);

  useEffect(() => () => {
    summaryPollsRef.current.forEach(({ timer }) => clearInterval(timer));
    summaryPollsRef.current.clear();
  }, []);



  return (
    <SidebarContext.Provider value={{
      collapsed,
      toggleCollapsed,
      serverAddress,
      startSummaryPolling,
      stopSummaryPolling,
    }}>
      {children}
    </SidebarContext.Provider>
  );
}
