@AGENTS.md

# Claude Code working rules

Agent routing, the per-task loop, the context budget and the personal-data rule come from the `harness@s-harness` plugin (injected at session start). This file holds only where Conclave differs; it wins over the routing.

- **Commit each task** once the implementer reports its gates passed (they are in AGENTS.md **Platform and build facts** and **Agent environment**), after checking `git diff --stat`. Don't rerun them through `harness:shipper`; use it only for work no implementer verified. Push only at checkpoints (CI minutes are scarce).
- **Codex pass**: ask `harness:reviewer` for one on every risky task (security/credentials, data persistence or deletion, concurrency/restart, untrusted input), at each phase end and on the whole branch before merge. Rechecks stay Claude-only.
- **Parallel agents:** at most 8 background agents at once (reviewers and shippers count). Only one lane runs cargo at a time (the `flock` rule in AGENTS.md **Agent environment**); keep the VM under 20 GB RAM.
- When a task's diff changes error handling, I/O, persistence, background tasks or process boundaries, tell the reviewer to look hard for silent failures.
