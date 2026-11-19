import { afterAll, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create } from 'react-test-renderer';

const originals = { dialog: { ...await import('../../src/components/ui/dialog') } };
afterAll(() => mock.module('../../src/components/ui/dialog', () => originals.dialog));
const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children?: ReactNode }) => (open ? <>{children}</> : null),
  DialogContent: pass, DialogDescription: pass, DialogFooter: pass, DialogHeader: pass, DialogTitle: pass,
}));
const { DeleteRecordingDialog } = await import('../../src/components/MeetingPage/DeleteRecordingDialog');

test('Delete recording is an outline button with the destructive text colour', async () => {
  let r!: ReturnType<typeof create>;
  await act(async () => { r = create(<DeleteRecordingDialog title="Weekly" open onOpenChange={() => {}} onConfirm={() => {}} />); });
  const del = r.root.findAll((n) => typeof n.type !== 'string' && n.props.children === 'Delete recording' && n.props.variant !== undefined)[0];
  expect(del.props.variant).toBe('outline');
  expect(del.props.className).toContain('text-destructive');
});
