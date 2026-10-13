export interface MicFollowingState {
  device: string | null;
  app: string | null;
  warning: string | null;
}

export type MicFollowingEvent =
  | { type: 'following'; device: string; app: string }
  | { type: 'switched'; device: string }
  | { type: 'warning'; message: string }
  | { type: 'reset' };

export const initialMicFollowing: MicFollowingState = { device: null, app: null, warning: null };

export function reduceMicFollowing(s: MicFollowingState, e: MicFollowingEvent): MicFollowingState {
  switch (e.type) {
    case 'following':
      return { device: e.device, app: e.app, warning: null };
    case 'switched':
      // A switch not made by following (disconnect fallback) ends the "following" label.
      return s.app && s.device === e.device ? s : { ...s, device: e.device, app: null };
    case 'warning':
      return { ...s, warning: e.message };
    case 'reset':
      return initialMicFollowing;
  }
}

export function micLine(s: MicFollowingState, selectedMic: string | null): string {
  const device = s.device ?? selectedMic ?? 'Default microphone';
  return s.app ? `🎙 ${device} (following ${s.app})` : `🎙 ${device}`;
}
