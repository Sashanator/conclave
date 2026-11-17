'use client';

import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';

type Props = { title: string; open: boolean; onOpenChange: (open: boolean) => void; onConfirm: () => void | Promise<void> };

/** Nothing is deleted until "Delete recording" is clicked; the meeting itself stays. */
export function DeleteRecordingDialog({ title, open, onOpenChange, onConfirm }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete the recording of “{title}”?</DialogTitle>
          <DialogDescription>
            Removes the transcript, audio, summary and tasks you did not keep. Title, category, prep and notes stay.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="destructive" onClick={() => void onConfirm()}>Delete recording</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
