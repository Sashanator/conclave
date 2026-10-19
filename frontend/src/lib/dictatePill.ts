// What the Dictate pill shows. The states and fields mirror PillState and PillView in dictate/pill.rs.

export type PillState =
  | 'listening'
  | 'hands-free'
  | 'loading-model'
  | 'transcribing'
  | 'pasted'
  | 'pasted-as-spoken'
  | 'copied'
  | 'nothing-heard'
  | 'no-microphone';

export interface PillView {
  state: PillState;
  elapsed_ms: number;
  level: number;
  mode: string;
}

const COPY: Record<Exclude<PillState, 'hands-free'>, string> = {
  listening: 'Listening',
  'loading-model': 'Loading model…',
  transcribing: 'Transcribing…',
  pasted: 'Pasted',
  'pasted-as-spoken': 'Pasted as spoken (cleanup failed)',
  copied: "Couldn't paste — copied, press Ctrl+V",
  'nothing-heard': "Didn't catch that",
  'no-microphone': 'No microphone',
};

export function pillLabel(v: PillView): string {
  if (v.state !== 'hands-free') return COPY[v.state];
  const total = Math.floor(v.elapsed_ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

export function pillTone(s: PillState): 'live' | 'busy' | 'ok' | 'warn' {
  switch (s) {
    case 'listening':
    case 'hands-free':
      return 'live';
    case 'loading-model':
    case 'transcribing':
      return 'busy';
    case 'pasted':
      return 'ok';
    default:
      return 'warn';
  }
}

export function showsControls(s: PillState): boolean {
  return s === 'hands-free';
}
