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
