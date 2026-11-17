'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { formatMeetingDate, setTemplate, type MeetingRow } from '@/lib/meetings';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';

/** Emoji above the title, then category and date. The row menu (Delete) arrives with Task 36. */
export function MeetingHeader({ row, live = false, onChange, onTitleSaved }: { row: MeetingRow; live?: boolean; onChange: (row: MeetingRow) => void; onTitleSaved?: () => void }) {
  const { availableTemplates } = useTemplates();
  const [title, setTitle] = useState(row.title);
  useEffect(() => setTitle(row.title), [row.title]);
  const emoji = row.emoji ?? availableTemplates.find((t) => t.id === row.templateId)?.emoji ?? null;

  const saveTitle = async () => {
    const next = title.trim();
    if (!next) { setTitle(row.title); toast.error('Meeting title cannot be empty'); return; }
    if (next === row.title) return;
    try {
      await invoke('api_save_meeting_title', { meetingId: row.id, title: next });
      onChange({ ...row, title: next });
      onTitleSaved?.();
    } catch (error) {
      setTitle(row.title);
      toast.error(`Failed to save the title: ${String(error)}`);
    }
  };
  const saveCategory = async (templateId: string) => {
    try {
      await setTemplate(row.id, templateId || null);
      onChange({ ...row, templateId: templateId || null });
    } catch (error) { toast.error(`Failed to save the category: ${String(error)}`); }
  };

  return (
    <header className="space-y-1">
      {emoji && <div className="text-3xl" aria-hidden>{emoji}</div>}
      <input
        aria-label="Meeting title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onBlur={saveTitle}
        onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
        className="w-full bg-transparent text-2xl font-semibold text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
      />
      <div className="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
        <select
          aria-label="Category"
          value={row.templateId ?? ''}
          onChange={(e) => saveCategory(e.target.value)}
          className="rounded-full border border-border bg-background px-2 py-0.5 text-sm"
        >
          <option value="">No category</option>
          {availableTemplates.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
        </select>
        <span>{formatMeetingDate(row, new Date(), live)}</span>
      </div>
    </header>
  );
}
