import type { TemplateJson as Template, TemplateSectionJson as TemplateSection } from '@/lib/template-draft';

export type DiffPart = { kind: 'same' | 'add' | 'del'; text: string };

// A word keeps its trailing whitespace; only leading whitespace is its own token.
// Every character lands in exactly one token, so same+del rebuilds `a` and same+add rebuilds `b`.
function tokenize(s: string): string[] {
  return s.match(/\s+|\S+\s*/g) ?? [];
}

export function wordDiff(a: string, b: string): DiffPart[] {
  const x = tokenize(a);
  const y = tokenize(b);

  let pre = 0;
  while (pre < x.length && pre < y.length && x[pre] === y[pre]) pre++;
  let suf = 0;
  while (
    suf < x.length - pre &&
    suf < y.length - pre &&
    x[x.length - 1 - suf] === y[y.length - 1 - suf]
  ) {
    suf++;
  }

  const xm = x.slice(pre, x.length - suf);
  const ym = y.slice(pre, y.length - suf);
  const n = xm.length;
  const m = ym.length;
  const w = m + 1;
  // lcs[i*w + j] = LCS length of xm[i..] and ym[j..]
  const lcs = new Int32Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i * w + j] =
        xm[i] === ym[j]
          ? lcs[(i + 1) * w + j + 1] + 1
          : Math.max(lcs[(i + 1) * w + j], lcs[i * w + j + 1]);
    }
  }

  const out: DiffPart[] = [];
  const push = (kind: DiffPart['kind'], text: string) => {
    const last = out[out.length - 1];
    if (last && last.kind === kind) last.text += text;
    else out.push({ kind, text });
  };

  for (let k = 0; k < pre; k++) push('same', x[k]);
  let i = 0;
  let j = 0;
  // In a gap, the walk takes deletions before additions, so `del` comes before `add`.
  while (i < n || j < m) {
    if (i < n && j < m && xm[i] === ym[j]) {
      push('same', xm[i]);
      i++;
      j++;
    } else if (i < n && (j === m || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1])) {
      push('del', xm[i]);
      i++;
    } else {
      push('add', ym[j]);
      j++;
    }
  }
  for (let k = x.length - suf; k < x.length; k++) push('same', x[k]);

  return out;
}

/** Replaces the middle of a long unchanged run with "… N unchanged lines …", keeping `keep` lines at each end. */
export function collapseSame(text: string, keep = 2): string {
  const lines = text.split('\n');
  if (lines.length <= 2 * keep + 1) return text;
  const hidden = lines.length - 2 * keep;
  return [
    ...lines.slice(0, keep),
    `… ${hidden} unchanged lines …`,
    ...lines.slice(lines.length - keep),
  ].join('\n');
}

export type SectionPair = {
  old?: number;
  next?: number;
  status: 'added' | 'removed' | 'changed' | 'moved' | 'unchanged';
};

export type DiffItem = {
  key: string;
  label: string;
  kind: 'scalar' | 'text' | 'section';
  old: string;
  next: string;
  parts?: DiffPart[];
  status?: SectionPair['status'];
};

// Reconstruct one longest increasing subsequence of old indexes in next order.
function inOrder(pairs: SectionPair[]): Set<SectionPair> {
  const matched = pairs.filter(p => p.old !== undefined && p.next !== undefined)
    .sort((a, b) => a.next! - b.next!);
  const lengths = matched.map(() => 1);
  const previous = matched.map(() => -1);
  let end = -1;
  for (let i = 0; i < matched.length; i++) {
    for (let j = 0; j < i; j++) {
      if (matched[j].old! < matched[i].old! && lengths[j] + 1 > lengths[i]) {
        lengths[i] = lengths[j] + 1;
        previous[i] = j;
      }
    }
    if (end === -1 || lengths[i] > lengths[end]) end = i;
  }
  const result = new Set<SectionPair>();
  for (let i = end; i !== -1; i = previous[i]) result.add(matched[i]);
  return result;
}

export function matchSections(old: TemplateSection[], next: TemplateSection[]): SectionPair[] {
  const titleKey = (s: TemplateSection) => s.title.trim().replace(/\s+/g, ' ').toLowerCase();
  const usedOld = new Set<number>();
  const byNext = new Map<number, SectionPair>();
  for (let n = 0; n < next.length; n++) {
    const o = old.findIndex((s, i) => !usedOld.has(i) && titleKey(s) === titleKey(next[n]));
    if (o !== -1) {
      usedOld.add(o);
      byNext.set(n, { old: o, next: n, status: 'changed' });
    }
  }
  for (let i = 0; i < Math.min(old.length, next.length); i++) {
    if (!usedOld.has(i) && !byNext.has(i)) {
      usedOld.add(i);
      byNext.set(i, { old: i, next: i, status: 'changed' });
    }
  }
  const pairs: SectionPair[] = next.map((_, n) => byNext.get(n) ?? { next: n, status: 'added' });
  const ordered = inOrder(pairs);
  for (const pair of pairs) {
    if (pair.old !== undefined && pair.next !== undefined) {
      pair.status = JSON.stringify(old[pair.old]) === JSON.stringify(next[pair.next])
        ? (ordered.has(pair) ? 'unchanged' : 'moved')
        : 'changed';
    }
  }
  for (let o = 0; o < old.length; o++) {
    if (!usedOld.has(o)) pairs.push({ old: o, status: 'removed' });
  }
  return pairs;
}

function sectionText(s: TemplateSection): string {
  return [s.title, s.instruction, s.item_format,
    s.format === undefined ? undefined : `Format: ${s.format}`,
    s.example_item_format === undefined ? undefined : `Example item format: ${s.example_item_format}`,
  ].filter(Boolean).join('\n\n');
}

function sectionKey(pair: SectionPair): string {
  return `section:${pair.old ?? '-'}:${pair.next ?? '-'}`;
}

const fields = [
  { key: 'name', label: 'Name', kind: 'scalar' },
  { key: 'description', label: 'Description', kind: 'text' },
  { key: 'use_when', label: 'Use when', kind: 'scalar' },
  { key: 'title_keywords', label: 'Title keywords', kind: 'scalar' },
  { key: 'language', label: 'Output language', kind: 'scalar' },
  { key: 'context', label: 'Context', kind: 'text' },
  { key: 'prep', label: 'Prep instruction', kind: 'text' },
  { key: 'style', label: 'Style', kind: 'text' },
] as const;

export function diffItems(old: Template, next: Template): DiffItem[] {
  const items: DiffItem[] = [];
  const addField = (field: typeof fields[number]) => {
    const { key, label, kind } = field;
    if (JSON.stringify(old[key]) === JSON.stringify(next[key])) return;
    const display = (value: string | string[] | undefined) => Array.isArray(value) ? value.join(', ') : value ?? '';
    const before = display(old[key]);
    const after = display(next[key]);
    items.push({
      key, label, kind, old: before, next: after,
      ...(kind === 'text' ? { parts: wordDiff(before, after) } : {}),
    });
  };
  for (const field of fields) {
    if (field.key !== 'style') addField(field);
  }
  for (const pair of matchSections(old.sections, next.sections)) {
    if (pair.status === 'unchanged') continue;
    const before = pair.old === undefined ? undefined : old.sections[pair.old];
    const after = pair.next === undefined ? undefined : next.sections[pair.next];
    const beforeText = before ? sectionText(before) : '';
    const afterText = after ? sectionText(after) : '';
    items.push({
      key: sectionKey(pair),
      label: before && after && before.title !== after.title
        ? `Section "${before.title}" → "${after.title}"`
        : `Section "${(after ?? before)!.title}"`,
      kind: 'section', old: beforeText, next: afterText, status: pair.status,
      ...(pair.status === 'changed' ? { parts: wordDiff(beforeText, afterText) } : {}),
    });
  }
  addField(fields[fields.length - 1]);
  return items;
}

export function mergeSelected(old: Template, next: Template, selected: Set<string>): Template {
  const merged: Template = { ...old, sections: [] };
  for (const { key } of fields) {
    if (!selected.has(key)) continue;
    if (next[key] === undefined) delete merged[key];
    else Object.assign(merged, { [key]: next[key] });
  }

  const pairs = matchSections(old.sections, next.sections);
  const ordered = inOrder(pairs);
  const sequence = pairs.filter(p => p.next !== undefined && (
    ordered.has(p) || p.status === 'added' || selected.has(sectionKey(p))
  ));
  const unanchored = pairs.filter(p => p.old !== undefined && !sequence.includes(p))
    .sort((a, b) => a.old! - b.old!);
  for (const pair of unanchored) {
    const predecessor = sequence.findIndex(p => p.old === pair.old! - 1);
    sequence.splice(pair.old === 0 ? 0 : predecessor + 1, 0, pair);
  }

  for (const pair of sequence) {
    const accepted = selected.has(sectionKey(pair));
    if (pair.status === 'added') {
      if (accepted) merged.sections.push(next.sections[pair.next!]);
    } else if (pair.status === 'removed') {
      if (!accepted) merged.sections.push(old.sections[pair.old!]);
    } else if (accepted) {
      const before = old.sections[pair.old!];
      const section = { ...next.sections[pair.next!] };
      for (const key of ['format', 'example_item_format'] as const) {
        if (section[key] === undefined && before[key] !== undefined) section[key] = before[key];
      }
      merged.sections.push(section);
    } else {
      merged.sections.push(old.sections[pair.old!]);
    }
  }
  return structuredClone(merged);
}
