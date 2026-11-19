'use client';

import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { DESTRUCTIVE_OUTLINE } from '@/lib/utils';
import { deleteMeetingParts, type MeetingRow } from '@/lib/meetings';

type Props = {
  row: MeetingRow | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void | Promise<void>;
};

/** Nothing is deleted until "Delete meeting" is clicked; the caller reports a failed delete. */
export function DeleteMeetingDialog({ row, open, onOpenChange, onConfirm }: Props) {
  if (!row) return null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete “{row.title}”?</DialogTitle>
          <DialogDescription>
            {`This removes: ${deleteMeetingParts(row).join(', ')}. Tasks you kept stay in Tasks.`}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="outline" className={DESTRUCTIVE_OUTLINE} onClick={() => void onConfirm()}>Delete meeting</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
