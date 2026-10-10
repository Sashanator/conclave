@AGENTS.md

# Claude Code working rules

At each completed implementation task, before asking for my review:

- Run the relevant tests and get a `reviewer` agent review of the task's diff. Give it the scope (commit range or "uncommitted") and the task's requirements.
- Ask the reviewer for a Codex pass too when the task touches security/credentials, data persistence or deletion, concurrency/restart behaviour, or untrusted input.
- Get a `reviewer` review with a Codex pass at the end of each plan phase and of the whole branch before merge, plus one recheck after fixes.
- Evaluate each finding and fix confirmed issues. After two fix/review cycles, bring unresolved issues to me.
- Present the final diff, test results, and unresolved findings.

## Agent routing (user preference)

- Orchestration: the main session (Opus 5.5, high effort).
- Implementation: the `conclave-implementer` agent (Haiku 5.5, high effort; rules in `.claude/agents/conclave-implementer.md`). Code search: `Explore` with `model: "haiku"`, `effort: "high"`.
- Code review: the `reviewer` agent with `model: "opus"` (Opus 5.5), `effort: "medium"`.
- Codex pass for hard or dangerous changes (security or credentials, data persistence or deletion, concurrency or restart, untrusted input): tell the reviewer to run Codex as
  `codex review -c model="gpt-6.1-sol" -c model_reasoning_effort="medium" -c sandbox_mode='"danger-full-access"' -`.
