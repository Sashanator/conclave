import { afterAll, describe, expect, mock, test } from 'bun:test';

const originals = { core: { ...(await import('@tauri-apps/api/core')) } };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originals.core);
});

const invoke = mock(async (_command: string, _args?: Record<string, unknown>): Promise<unknown> => ({
  status: 'success',
  message: 'Transcript saved successfully',
  meeting_id: 'meeting-saved',
}));
mock.module('@tauri-apps/api/core', () => ({ ...originals.core, invoke }));

const { storageService } = await import('../../src/services/storageService');

describe('saveMeeting', () => {
  test('sends the recording id so its LLM usage attaches to the meeting', async () => {
    invoke.mockClear();
    await storageService.saveMeeting('Standup', [], null, [], 'meeting-rec-1');
    expect(invoke).toHaveBeenCalledWith(
      'api_save_transcript',
      expect.objectContaining({ meetingTitle: 'Standup', recordingId: 'meeting-rec-1' }),
    );
  });

  test('sends no recording id when none is given', async () => {
    invoke.mockClear();
    await storageService.saveMeeting('Standup', [], null);
    expect(invoke).toHaveBeenCalledWith('api_save_transcript', expect.objectContaining({ recordingId: null }));
  });
});
