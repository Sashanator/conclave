#!/usr/bin/env bash
# Default agent setup for a new container (postCreateCommand). Idempotent; a failed step never blocks container creation.
set -u

# codebase-memory-mcp: registers its MCP server, hooks, agents and skill with Claude/Codex/Antigravity
codebase-memory-mcp install -y --dir=/usr/local/bin || true

# Claude Code plugins (enabledPlugins in .claude/settings.json only enables them; this installs them)
for m in anthropics/claude-plugins-official DietrichGebert/ponytail openai/codex-plugin-cc; do
  claude plugin marketplace add "$m" || true
done
for p in \
  superpowers@claude-plugins-official \
  feature-dev@claude-plugins-official \
  ponytail@ponytail \
  codex@openai-codex; do
  claude plugin install -s user "$p" || true
done

# Claude Code MCP servers
claude mcp get context7 >/dev/null 2>&1 || claude mcp add -s user context7 -- npx -y @upstash/context7-mcp@latest || true
