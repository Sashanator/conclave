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
- Third reviewer for whole-branch reviews only (at the end of a phase): Antigravity CLI with Gemini 3.8 Flash High, run read-only:
  `agy -p "<review prompt>" --model gemini-3.8-flash-high --mode plan --dangerously-skip-permissions --print-timeout 1800s`. Verify its findings like Codex findings.

## Context budget (token use)

- Long main sessions are the biggest cost (2026-10-22 audit: one session averaged 586k context per request). At each natural phase boundary (task reviewed, phase merged, plan written), write the state to the plan's ledger or notes file, then `/clear` and resume from that file.
- Subagent briefs are short: the task text plus cited line ranges (`path:120-160`), never whole plans, specs or diffs pasted in or "read the whole doc".
- No forks for drafting work (plans, specs, briefs). Use a fresh agent with a brief, or write it in the main session.
- In the main session, read with Grep, then Read with offset/limit; trim command output (`| tail`, `--stat`, summary reporters). Don't `cat` whole diffs or files to skim them.
