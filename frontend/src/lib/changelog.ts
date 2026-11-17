export type Release = { version: string; date: string; items: string[] };

const key = (v: string) => v.split('.').map(Number);

export function parseChangelog(md: string): Release[] {
  const out: Release[] = [];
  for (const line of md.split(/\r?\n/)) {
    const h = /^## v(\d+\.\d+\.\d+) — (.+)$/.exec(line);
    if (h) {
      out.push({ version: h[1], date: h[2].trim(), items: [] });
      continue;
    }
    if (line.startsWith('## ')) out.push({ version: '', date: '', items: [] }); // closes the section: Unreleased and others are dropped below
    const b = /^- (.+)$/.exec(line);
    if (b && out.length) out[out.length - 1].items.push(b[1]);
  }
  return out
    .filter((r) => r.version)
    .sort((a, b) => {
      const [x, y] = [key(a.version), key(b.version)];
      return y[0] - x[0] || y[1] - x[1] || y[2] - x[2];
    });
}
