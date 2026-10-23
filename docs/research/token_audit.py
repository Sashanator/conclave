#!/usr/bin/env python3
"""Read-only Claude Code token audit over ~/.claude/projects/**/*.jsonl.

Usage: python3 token_audit.py [--root DIR] [--split YYYY-MM-DD]
Tool-result volume is in characters/4 (approx tokens); images count as ~1600 tokens each.
"""
import argparse, collections, glob, json, os, statistics

ap = argparse.ArgumentParser()
ap.add_argument("--root", default=os.path.expanduser("~/.claude/projects"))
ap.add_argument("--split", help="day of the jump (YYYY-MM-DD); default: auto from daily totals")
args = ap.parse_args()

def agent_type(path):
    meta = path[:-len(".jsonl")] + ".meta.json"
    if "/subagents/" not in path:
        return "main"
    try:
        return json.load(open(meta)).get("agentType") or "unknown"
    except OSError:
        return "unknown(no meta)"

seen = set()
day = collections.defaultdict(lambda: collections.Counter())
by_agent = collections.Counter()
by_agent_req = collections.Counter()
by_agent_day = collections.defaultdict(collections.Counter)
sessions = collections.defaultdict(list)   # main session -> [(ts, ctx)]
compacts = []
versions = collections.defaultdict(collections.Counter)
tool_tokens = collections.Counter()
tool_calls = collections.Counter()
read_files = collections.Counter()
read_file_tokens = collections.Counter()

def ctx_of(u):
    return u.get("input_tokens", 0) + u.get("cache_read_input_tokens", 0) + u.get("cache_creation_input_tokens", 0)

def result_size(content):
    if isinstance(content, str):
        return len(content) // 4, 0
    toks = imgs = 0
    for c in content or []:
        if c.get("type") == "text":
            toks += len(c.get("text", "")) // 4
        elif c.get("type") == "image":
            imgs += 1
            toks += 1600
    return toks, imgs

for path in glob.glob(os.path.join(args.root, "**", "*.jsonl"), recursive=True):
    atype = agent_type(path)
    sid = os.path.basename(path)[:-6]
    tool_names = {}
    for line in open(path, errors="replace"):
        try:
            d = json.loads(line)
        except ValueError:
            continue
        ts = d.get("timestamp", "")[:10]
        if d.get("subtype") == "compact_boundary":
            m = d.get("compactMetadata") or {}
            compacts.append((d.get("timestamp", ""), d.get("sessionId"), m.get("trigger"), m.get("preTokens")))
        msg = d.get("message") or {}
        if d.get("type") == "assistant":
            for c in msg.get("content") or []:
                if isinstance(c, dict) and c.get("type") == "tool_use":
                    n = c.get("name", "?")
                    n = "MCP:" + n.split("__")[1] if n.startswith("mcp__") else n
                    tool_names[c.get("id")] = (n, c.get("input") or {})
            u = msg.get("usage")
            if not u or msg.get("model") == "<synthetic>":
                continue
            key = (msg.get("id"), d.get("requestId"))
            if key in seen:
                continue
            seen.add(key)
            ctx = ctx_of(u)
            out = u.get("output_tokens", 0)
            dd = day[ts]
            dd["req"] += 1; dd["in"] += u.get("input_tokens", 0)
            dd["cr"] += u.get("cache_read_input_tokens", 0); dd["cw"] += u.get("cache_creation_input_tokens", 0)
            dd["out"] += out; dd["ctx"] += ctx
            dd["req_" + ("main" if atype == "main" else "sub")] += 1
            by_agent[atype] += ctx + out; by_agent_req[atype] += 1
            by_agent_day[ts][atype] += ctx + out
            versions[ts][d.get("version", "?")] += 1
            if atype == "main":
                sessions[d.get("sessionId", sid)].append((d.get("timestamp", ""), ctx))
        elif d.get("type") == "user" and isinstance(msg.get("content"), list):
            for c in msg["content"]:
                if isinstance(c, dict) and c.get("type") == "tool_result":
                    n, inp = tool_names.get(c.get("tool_use_id"), ("?", {}))
                    t, imgs = result_size(c.get("content"))
                    if imgs:
                        tool_tokens["(screenshots/images)"] += imgs * 1600
                    tool_tokens[n] += t; tool_calls[n] += 1
                    if n == "Read":
                        fp = inp.get("file_path", "?")
                        read_files[fp] += 1; read_file_tokens[fp] += t

days = sorted(k for k in day if k)
def fmt(n): return f"{n/1e6:7.2f}M" if n >= 1e5 else f"{n:8d}"

print("== Per day ==")
print("day         reqs(main/sub)    input    cacheRd  cacheWr   output  avgCtx")
for k in days:
    d = day[k]
    print(f"{k} {d['req']:5d} ({d['req_main']:4d}/{d['req_sub']:4d}) {fmt(d['in'])} {fmt(d['cr'])} {fmt(d['cw'])} {fmt(d['out'])} {d['ctx']//max(d['req'],1):7d}")

split = args.split
if not split and len(days) > 2:
    totals = [day[k]["ctx"] for k in days]
    med = statistics.median(totals)
    split = next((k for k in days if day[k]["ctx"] > 2 * med), days[len(days)//2])
print(f"\n(split day = {split})")

print("\n== First-turn context per main session (fixed overhead) ==")
before, after = [], []
for s, rows in sessions.items():
    rows.sort()
    first = rows[0]
    (after if first[0][:10] >= split else before).append(first[1])
for name, v in (("before", before), ("after", after)):
    if v:
        print(f"{name}: n={len(v)} median={int(statistics.median(v))} min={min(v)} max={max(v)}")

print("\n== Share by agent type (context+output tokens) ==")
tot = sum(by_agent.values()) or 1
for a, v in by_agent.most_common():
    print(f"{a:28s} {fmt(v)} {100*v/tot:5.1f}%  reqs={by_agent_req[a]}  avgCtx={v//max(by_agent_req[a],1)}")
print("\nper day, top agent types:")
for k in days:
    t = sum(by_agent_day[k].values()) or 1
    print(k, ", ".join(f"{a} {100*v/t:.0f}%" for a, v in by_agent_day[k].most_common(4)))

print("\n== Main sessions by peak / average context ==")
rank = sorted(sessions.items(), key=lambda kv: -max(c for _, c in kv[1]))[:12]
for s, rows in rank:
    cs = [c for _, c in rows]
    print(f"{s[:8]} {rows[0][0][:10]} reqs={len(cs):5d} avg={sum(cs)//len(cs):7d} peak={max(cs):7d} total={fmt(sum(cs))}")

print("\n== Compaction events ==")
for c in sorted(compacts, key=lambda c: c[0]):
    print(c[0][:16], (c[1] or "?")[:8], c[2], c[3])

print("\n== Claude Code version per day ==")
for k in days:
    print(k, dict(versions[k]))

print("\n== Tool-result volume (approx tokens) ==")
for n, v in tool_tokens.most_common(15):
    print(f"{n:32s} {fmt(v)}  calls={tool_calls[n]}")
print("\n== Files read most (count, approx tokens) ==")
for fp, n in read_files.most_common(15):
    print(f"{n:4d} {fmt(read_file_tokens[fp])} {fp}")
