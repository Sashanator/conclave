'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { MoreHorizontal } from 'lucide-react';
import { deleteMeeting, deleteRecording, formatMeetingDate, setTemplate, type MeetingRow } from '@/lib/meetings';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { DeleteMeetingDialog } from '@/components/MeetingsTable';
import { DeleteRecordingDialog } from '@/components/MeetingPage/DeleteRecordingDialog';

type Props = {
  row: MeetingRow;
  live?: boolean;
  /** A finished meeting: offers Delete recording and the Regenerate action after a category change. */
  finished?: boolean;
  onChange: (row: MeetingRow) => void;
  onTitleSaved?: () => void;
  onMeetingDeleted?: () => void;
  onRecordingDeleted?: () => void;
  onRegenerateSummary?: (templateId: string) => void;
  /** Rendered in the title row before the menu. */
  actions?: React.ReactNode;
  variant?: 'page' | 'compact' | 'new';
};

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Emoji above the title, then category and date, with the page menu (the two deletes). */
export function MeetingHeader({ row, live = false, finished = false, onChange, onTitleSaved, onMeetingDeleted, onRecordingDeleted, onRegenerateSummary, actions, variant = 'page' }: Props) {
  const [dialog, setDialog] = useState<'meeting' | 'recording' | null>(null);
  const [deleting, setDeleting] = useState(false);
  const { availableTemplates } = useTemplates();
  const isNew = variant === 'new';
  const compact = variant === 'compact'; // the recording page: title and menu on one line
  const Wrap = isNew ? 'div' : 'h1'; // PageFrame supplies the h1 on the new-meeting page
  // A new meeting still has its default title: show it as the placeholder, so typing starts from empty.
  const shown = isNew && row.titleSource === 'default' ? '' : row.title;
  const [title, setTitle] = useState(shown);
  useEffect(() => setTitle(shown), [shown]);
  const emoji = row.emoji ?? availableTemplates.find((t) => t.id === row.templateId)?.emoji ?? null;

  const saveTitle = async () => {
    const next = title.trim();
    if (!next) { setTitle(shown); if (!isNew) toast.error('Meeting title cannot be empty'); return; }
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
      const name = availableTemplates.find((t) => t.id === templateId)?.name;
      if (finished && name && onRegenerateSummary) {
        toast.success('Category changed', { action: { label: `Regenerate summary with ${name}`, onClick: () => onRegenerateSummary(templateId) } });
      }
    } catch (error) { toast.error(`Failed to save the category: ${String(error)}`); }
  };
  // A failed delete keeps the page and the dialog's meeting; a folder warning is shown, the delete still counts.
  const runDelete = async (kind: 'meeting' | 'recording') => {
    setDialog(null);
    setDeleting(true);
    try {
      const { folderWarning } = await (kind === 'meeting' ? deleteMeeting(row.id) : deleteRecording(row.id));
      if (folderWarning) toast.warning(`The ${kind} was deleted, but its folder was kept`, { description: folderWarning });
      (kind === 'meeting' ? onMeetingDeleted : onRecordingDeleted)?.();
    } catch (e) {
      toast.error(`Could not delete the ${kind}`, { description: errText(e) });
    } finally { setDeleting(false); }
  };

  return (
    <header className="space-y-1">
      {emoji && variant === 'page' && <div className="text-3xl" aria-hidden>{emoji}</div>}
      {isNew && <label htmlFor="meeting-name" className="block text-sm font-medium text-foreground">Meeting name</label>}
      <div className={compact ? 'flex items-center gap-4' : 'flex items-start gap-2'}>
      <Wrap className="flex min-w-0 flex-1 text-xl font-semibold">
      <input
        id={isNew ? 'meeting-name' : undefined}
        aria-label={isNew ? undefined : 'Meeting title'}
        placeholder={isNew ? row.title : undefined}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onBlur={saveTitle}
        onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
        className={`min-w-0 flex-1 rounded bg-transparent text-xl font-semibold text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring${compact ? ' truncate' : ''}${isNew ? ' border border-input px-3 py-2' : ''}`}
      />
      </Wrap>
      {actions}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button type="button" aria-label="Meeting actions" className={`${compact ? '' : 'mt-1 '}rounded p-1 text-muted-foreground hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring`}>
            <MoreHorizontal className="h-5 w-5" aria-hidden />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {finished && <DropdownMenuItem disabled={live || deleting} onSelect={() => setDialog('recording')}>Delete recording…</DropdownMenuItem>}
          <DropdownMenuItem disabled={live || deleting} onSelect={() => setDialog('meeting')}>Delete meeting…</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      </div>
      {!compact && <div className="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
        <select
          aria-label="Category"
          value={row.templateId ?? ''}
          onChange={(e) => saveCategory(e.target.value)}
          className="rounded-full border border-border bg-background px-2 py-0.5 text-sm"
        >
          <option value="">No category</option>
          {availableTemplates.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
        </select>
        {!isNew && <span>{formatMeetingDate(row, new Date(), live)}</span>}
      </div>}
      <DeleteMeetingDialog row={row} open={dialog === 'meeting'} onOpenChange={(o) => !o && setDialog(null)} onConfirm={() => runDelete('meeting')} />
      <DeleteRecordingDialog title={row.title} open={dialog === 'recording'} onOpenChange={(o) => !o && setDialog(null)} onConfirm={() => runDelete('recording')} />
    </header>
  );
}
