'use client';

import React, { useEffect, useState } from 'react';
import { Circle, Mic, PanelLeftClose, PanelLeftOpen, Settings, Sparkles, Upload } from 'lucide-react';
import { useRouter, usePathname } from 'next/navigation';
import { getVersion } from '@tauri-apps/api/app';
import { useSidebar } from './SidebarProvider';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { confirmLeave } from '@/lib/unsaved-guard';
import { NAV_MAIN, WHATS_NEW_SEEN_KEY, isNavActive, recordLabel, whatsNewUnseen } from '@/lib/nav';
import { formatClock } from '@/lib/practice';
import { queueAutoStart, routeOrFallback } from '@/lib/recording-target';
import { FeedBell } from '@/components/Feed/FeedBell';

const readSeen = () => {
  try { return localStorage.getItem(WHATS_NEW_SEEN_KEY); } catch { return null; }
};

const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname() ?? '';
  // Navigation that discards nothing unless the user agrees (e.g. unsaved template edits)
  const go = async (href: string) => {
    if (await confirmLeave()) router.push(href);
  };
  const { collapsed, toggleCollapsed } = useSidebar();
  const { isRecording, liveMeetingId, activeDuration } = useRecordingState();
  const { openImportDialog } = useImportDialog();
  const [version, setVersion] = useState<string | null>(null);
  const [seen, setSeen] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(console.error);
    setSeen(readSeen());
    const reread = () => setSeen(readSeen());
    window.addEventListener('whats-new-seen', reread);
    return () => window.removeEventListener('whats-new-seen', reread);
  }, []);

  const startRecording = async () => {
    // Ask before routing: a declined leave must not leave an empty row and a queued start behind.
    if (!(await confirmLeave())) return;
    const routed = await routeOrFallback('sidebar');
    router.push(queueAutoStart(sessionStorage, routed.meetingId));
  };

  const onRecord = () => (isRecording ? go(liveMeetingId ? `/meeting-details?id=${liveMeetingId}` : '/meeting-details') : startRecording());
  const unseen = whatsNewUnseen(seen, version);
  const whatsNewLabel = `What's new in v${version ?? ''}`;

  // One row for every entry; collapsed shows the icon only, with a tooltip and the same aria-label.
  const row = (
    label: string, icon: React.ReactNode, onClick: () => void,
    opts: { active?: boolean; disabled?: boolean; accent?: boolean; rec?: boolean; dot?: boolean; hint?: string } = {},
  ) => {
    const btn = (
      <button
        onClick={onClick}
        disabled={opts.disabled}
        aria-label={label}
        aria-current={opts.active ? 'page' : undefined}
        className={`relative flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors disabled:opacity-60 ${collapsed ? 'justify-center' : 'w-full'} ${
          opts.rec ? 'bg-[hsl(var(--rec))] text-white hover:brightness-110' : opts.accent ? 'bg-primary text-primary-foreground hover:bg-primary/90' : opts.active ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
        }`}
      >
        {icon}
        {!collapsed && <span className="truncate">{label}</span>}
        {!collapsed && opts.hint && <kbd className="ml-auto text-[11px] text-muted-foreground">{opts.hint}</kbd>}
        {opts.dot && (
          <>
            <span aria-hidden="true" className="absolute right-2 top-2 h-2 w-2 rounded-full bg-[hsl(var(--rec))]" />
            <span className="sr-only"> (new)</span>
          </>
        )}
      </button>
    );
    if (!collapsed) return <React.Fragment key={label}>{btn}</React.Fragment>;
    return (
      <Tooltip key={label}>
        <TooltipTrigger asChild>{btn}</TooltipTrigger>
        <TooltipContent side="right"><p>{opts.hint ? `${label} (${opts.hint})` : label}</p></TooltipContent>
      </Tooltip>
    );
  };

  return (
    <div className="fixed top-0 left-0 h-screen z-40">
      <TooltipProvider>
        <nav
          aria-label="Main"
          className={`h-screen bg-background border-r shadow-sm flex flex-col gap-1 overflow-y-auto p-2 pt-6 transition-all duration-300 ${collapsed ? 'w-16' : 'w-64'}`}
        >
          {row(
            recordLabel(isRecording, activeDuration),
            !isRecording ? <Mic className="w-5 h-5 shrink-0" /> : collapsed ? (
              <span className="flex flex-col items-center">
                <Circle className="h-3 w-3 fill-current" />
                <span className="text-[10px] tabular-nums">{formatClock(activeDuration ?? 0)}</span>
              </span>
            ) : <Circle className="w-5 h-5 shrink-0 fill-current" />,
            onRecord,
            { accent: !isRecording, rec: isRecording },
          )}
          <div className="h-2" />
          {NAV_MAIN.map((item) =>
            row(item.label, <item.icon className="w-5 h-5 shrink-0" />, () => go(item.href), { active: isNavActive(pathname, item) }),
          )}

          <div className="mt-auto flex flex-col gap-1 border-t border-border pt-2">
            {row('Import audio', <Upload className="w-5 h-5 shrink-0" />, () => openImportDialog())}
            <div className={`flex items-center gap-1 ${collapsed ? 'flex-col' : ''}`}>
              <div className="flex-1 w-full">
                {row('Settings', <Settings className="w-5 h-5 shrink-0" />, () => go('/settings'), { active: pathname === '/settings' })}
              </div>
              <FeedBell side="right" />
            </div>
            {version && row(whatsNewLabel, <Sparkles className="w-5 h-5 shrink-0" />, () => go('/whats-new'), { active: pathname === '/whats-new', dot: unseen })}
            {row(collapsed ? 'Expand sidebar' : 'Collapse sidebar', collapsed ? <PanelLeftOpen className="w-5 h-5 shrink-0" /> : <PanelLeftClose className="w-5 h-5 shrink-0" />, toggleCollapsed, { hint: 'Ctrl B' })}
          </div>
        </nav>
      </TooltipProvider>
    </div>
  );
};

export default Sidebar;
