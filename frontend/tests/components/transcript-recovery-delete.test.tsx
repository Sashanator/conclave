import { afterAll, describe, expect, mock, test } from 'bun:test';
import type { ReactNode } from 'react';
import { act, create, type ReactTestInstance } from 'react-test-renderer';

const originals = {
  dialog: { ...await import('../../src/components/ui/dialog') },
  confirm: { ...await import('../../src/lib/confirm') },
};
afterAll(() => {
  mock.module('../../src/components/ui/dialog', () => originals.dialog);
  mock.module('../../src/lib/confirm', () => originals.confirm);
});

const pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
mock.module('../../src/components/ui/dialog', () => ({
  Dialog: pass, DialogContent: pass, DialogDescription: pass, DialogFooter: pass, DialogHeader: pass, DialogTitle: pass,
}));
let answer = false;
const confirmAction = mock(async (_o: unknown) => answer);
mock.module('../../src/lib/confirm', () => ({ ...originals.confirm, confirmAction }));

const { TranscriptRecovery } = await import('../../src/components/TranscriptRecovery/TranscriptRecovery');

const text = (n: ReactTestInstance): string => n.children.map((c) => (typeof c === 'string' ? c : text(c))).join('');
const meeting = { meetingId: 'm1', title: 'Standup', startTime: 1, lastUpdated: 2, transcriptCount: 3, folderPath: '' } as never;

async function clickDelete(ok: boolean) {
  answer = ok;
  confirmAction.mockClear();
  const onDelete = mock(async (_id: string) => {});
  let r: ReturnType<typeof create>;
  await act(async () => {
    r = create(<TranscriptRecovery isOpen onClose={() => {}} recoverableMeetings={[meeting]} onRecover={async () => ({})} onDelete={onDelete} onLoadPreview={async () => []} />);
  });
  const btn = r!.root.findAll((n) => n.type === 'button' && text(n).includes('Delete'))[0];
  await act(async () => { await btn.props.onClick(); });
  act(() => { r.unmount(); });
  return onDelete;
}

describe('TranscriptRecovery delete', () => {
  test('cancel does not delete', async () => {
    const onDelete = await clickDelete(false);
    expect(confirmAction).toHaveBeenCalledTimes(1);
    expect(onDelete).not.toHaveBeenCalled();
  });

  test('confirm deletes that meeting', async () => {
    const onDelete = await clickDelete(true);
    expect(onDelete).toHaveBeenCalledWith('m1');
  });
});
