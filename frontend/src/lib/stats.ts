// Stats and cost types as the Rust side sends them (frontend/src-tauri/src/usage), with the
// display helpers the Stats page, the meeting cost line and the Costs tab share.

export type Period = 'week' | 'month';

export interface ProviderCost {
  provider: string;
  input_tokens: number; // uncached input only
  output_tokens: number;
  cost_usd: number; // priced calls only
  unpriced_calls: number;
}

export interface CostPeriod {
  start: string; // YYYY-MM-DD, local
  total_usd: number;
  providers: ProviderCost[];
}

export interface PurposeCost {
  purpose: string;
  tokens: number;
  cost_usd: number | null;
}

export interface MeetingCost {
  total_usd: number | null; // null if any call is unpriced
  tokens: number;
  by_purpose: PurposeCost[];
}

export interface TypeShare {
  template_id: string | null;
  template_name: string;
  meetings: number;
  self_seconds: number;
  total_seconds: number;
}

export interface TalkTime {
  current: TypeShare[];
  previous: TypeShare[];
}

export interface MeetingShare {
  meeting_id: string;
  title: string;
  date: string;
  self_seconds: number;
  total_seconds: number;
}

const PERIOD_KEY = 'conclave.statsPeriod';

export function formatUsd(v: number | null): string {
  return v === null ? '—' : `$${v.toFixed(3)}`;
}

export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  const k = (n / 1000).toFixed(1);
  if (Number(k) < 1000) return `${k}k`;
  return `${(n / 1_000_000).toFixed(2)}M`;
}

// Plain labels for the purpose ids the usage recording writes. A Map, so ids like "constructor" stay unknown.
const PURPOSE_LABELS = new Map<string, string>([
  ['summary', 'Summary'],
  ['template_pick', 'Template choice'],
  ['extraction', 'Task extraction'],
  ['copilot', 'Copilot'],
  ['prep', 'Meeting prep'],
  ['dictate', 'Dictation'],
  ['practice', 'Speaking practice'],
  ['ask', 'Ask'],
  ['digest', 'Weekly digest'],
  ['connection_test', 'Connection test'],
]);

export function purposeLabel(purpose: string): string {
  return PURPOSE_LABELS.get(purpose) ?? purpose;
}

export function sharePct(self: number, total: number): string {
  return total === 0 ? '—' : `${Math.round((self / total) * 100)}%`;
}

// Storage can be disabled or absent (static export prerender), so a failure just means the default.
export function readStatsPeriod(): Period {
  try {
    return localStorage.getItem(PERIOD_KEY) === 'month' ? 'month' : 'week';
  } catch {
    return 'week';
  }
}

export function saveStatsPeriod(period: Period): void {
  try {
    localStorage.setItem(PERIOD_KEY, period);
  } catch {
    // The choice is kept for this session only
  }
}
