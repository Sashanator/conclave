// TS mirrors of the Rust `notion` types (camelCase over the Tauri boundary) plus small pure helpers.
export type FixedKind = 'select' | 'status' | 'multi_select' | 'checkbox' | 'date';
export type FixedValue = { prop: string; kind: FixedKind; value: string | string[] | boolean };
/** Every field holds a Notion property NAME. */
export type Mapping = { titleProp: string; dueProp: string | null; meetingProp: string | null; fixed: FixedValue[]; emoji?: string | null; aiFill?: boolean };

export type DestState = 'connected' | 'not_shared' | 'not_found' | 'token_missing' | 'token_rejected' | 'mapping_broken' | 'offline';

export type Destination = {
  id: string;
  name: string;
  colour: number;
  position: number;
  databaseId: string;
  dataSourceId: string | null;
  mapping: Mapping;
  lastStatus: DestState | null;
  lastMessage: string | null;
};
export type PropInfo = { id: string; name: string; kind: string; options: string[] };
export type DbSchema = { databaseId: string; dataSourceId: string | null; title: string; properties: PropInfo[] };
export type DestStatus = { destId: string; state: DestState; message: string | null };
export type CategoryDefault = { templateKey: string; destId: string | null };
export type NotionConfig = { tokenLast4: string | null; destinations: Destination[]; categoryDefaults: CategoryDefault[] };
export type SendReport = { sent: number; failed: number; ambiguous: number; aiNotFilled?: string[] };

export const MAX_DESTINATIONS = 9;
/** A task for a destination in one of these states waits; `offline` does not block (the send just fails again). */
export const BLOCKING_STATES: DestState[] = ['not_shared', 'not_found', 'token_missing', 'token_rejected', 'mapping_broken'];
export const DEST_STATE_LABEL: Record<DestState, string> = {
  connected: 'Connected',
  not_shared: 'Not shared with Conclave',
  not_found: 'Not found',
  token_missing: 'Token missing',
  token_rejected: 'Token rejected',
  mapping_broken: 'Mapping broken',
  offline: 'Offline',
};

export const notionDbUrl = (databaseId: string) => `https://www.notion.so/${databaseId.replace(/-/g, '')}`;

/** First colour 1..9 no destination uses, else 1. */
export function nextColour(dests: { colour: number }[]): number {
  const used = new Set(dests.map((d) => d.colour));
  for (let n = 1; n <= MAX_DESTINATIONS; n++) if (!used.has(n)) return n;
  return 1;
}

const FIXED_KINDS = ['select', 'status', 'multi_select', 'checkbox'];
/** Which columns of a schema can play which role in a mapping; anything else is unused. */
export function mappingChoices(s: DbSchema) {
  const of = (...kinds: string[]) => s.properties.filter((p) => kinds.includes(p.kind));
  const known = ['title', 'date', 'rich_text', ...FIXED_KINDS];
  return {
    title: of('title')[0] ?? null,
    dates: of('date'),
    texts: of('rich_text'),
    fixed: of(...FIXED_KINDS),
    unused: s.properties.filter((p) => !known.includes(p.kind)),
  };
}
