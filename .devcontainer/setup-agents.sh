#!/usr/bin/env bash
# Default agent setup for a new container (postCreateCommand). Idempotent; a failed step never blocks container creation.
set -u

# GitNexus: index the repo (the gitnexus@s-harness plugin brings the Claude MCP server, hooks and skills).
bash "$(dirname "${BASH_SOURCE[0]}")/gitnexus.sh" setup || {
  printf '%s\n' 'GitNexus setup failed; rerun bash .devcontainer/gitnexus.sh setup after fixing the error.' >&2
}
# Codex does not read Claude's plugins. Register the same local graph server.
if ! codex mcp get gitnexus >/dev/null 2>&1; then
  codex mcp add gitnexus --env GITNEXUS_MEMORY=off -- gitnexus mcp || {
    printf '%s\n' 'GitNexus Codex registration failed; rerun setup-agents.sh.' >&2
  }
fi

# Claude Code plugins (enabledPlugins in .claude/settings.json only enables them; this installs them).
# all@s-harness brings harness, gitnexus, the mods, superpowers and ponytail. s-harness is private: needs gh auth.
for m in anthropics/claude-plugins-official DietrichGebert/ponytail openai/codex-plugin-cc Sashanator/s-harness nextlevelbuilder/ui-ux-pro-max-skill pbakaus/impeccable; do
  claude plugin marketplace add "$m" || true
done
for p in all@s-harness codex@openai-codex ui-ux-pro-max@ui-ux-pro-max-skill impeccable@impeccable; do
  claude plugin install -s user "$p" || true
done

# Language servers, project scope: only this project's languages. typescript@5: TypeScript 7 has no tsserver.js.
(
  cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 0
  claude plugin install typescript-lsp@claude-plugins-official --scope project || true
  claude plugin install rust-analyzer-lsp@claude-plugins-official --scope project || true
)
npm install -g typescript-language-server typescript@5 || true
if command -v rustup >/dev/null 2>&1; then rustup component add rust-analyzer || true; fi

# Claude Code MCP servers
claude mcp get context7 >/dev/null 2>&1 || claude mcp add -s user context7 -- npx -y @upstash/context7-mcp@latest || true

# Design tooling: .mcp.json starts the Playwright and shadcn MCP servers (pinned there); this installs the browsers they and
# frontend/scripts/a11y-check.mjs drive. The MCP's own playwright is an alpha pinned to another chromium revision than the
# devDependency, so install both. install-deps needs sudo for the system libraries.
(
  cd "$(dirname "${BASH_SOURCE[0]}")/../frontend" || exit 0
  pnpm install --frozen-lockfile || true
  pnpm exec playwright install chromium || true
  sudo -n "$(command -v pnpm)" exec playwright install-deps chromium || true
  npx -y -p playwright@1.64.0-alpha-1790635538000 playwright install chromium || true
)
