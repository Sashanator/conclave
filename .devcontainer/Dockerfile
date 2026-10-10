FROM mcr.microsoft.com/devcontainers/javascript-node:22-bookworm

ARG TZ
ENV TZ="$TZ"

# Mark as devcontainer for tools that check this
ENV DEVCONTAINER=true

# Install tools --no-install-recommends reduces image size and attack surface
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    jq \
    python3 \
    python3-pip \
    python3-venv \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# uv (fast Python package manager) — lets agent skills and ad-hoc tooling
# install Python packages on the fly into the global venv below.
RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR="/usr/local/bin" sh

# Global virtual environment, placed on PATH so `uv pip install` has a writable
# target (Debian's system Python is externally managed).
ENV VIRTUAL_ENV=/usr/local/python-venv
RUN python3 -m venv $VIRTUAL_ENV && chown -R node:node $VIRTUAL_ENV
ENV PATH="$VIRTUAL_ENV/bin:$PATH"

# Claude Code CLI (available independently of the VS Code extension). Installed
# as `node` (npm-global is node-owned) so its built-in auto-updater and the
# postAttachCommand `claude update` can replace it without sudo.
USER node
RUN npm install -g @anthropic-ai/claude-code
USER root

# Codex CLI: install on the standard PATH for every user and container rebuild.
RUN npm install -g --prefix /usr/local @openai/codex@latest

# codebase-memory-mcp: code knowledge-graph MCP server. Binary only here; agent
# registration happens in postCreateCommand since it writes to $HOME configs.
RUN curl -fsSL https://raw.githubusercontent.com/DeusData/codebase-memory-mcp/main/install.sh \
    | bash -s -- --dir=/usr/local/bin --skip-config

# Antigravity CLI (Google's terminal coding agent, successor to Gemini CLI)
# The installer hardcodes ~/.local/bin (no install-dir override), so relocate
# the binary (named `agy`, not `antigravity`) to /usr/local/bin for all users
RUN curl -fsSL https://antigravity.google/cli/install.sh | bash \
    && mv /root/.local/bin/agy /usr/local/bin/agy

# Persist shell history across container rebuilds
RUN mkdir /commandhistory \
    && touch /commandhistory/.bash_history \
    && chown -R node /commandhistory

# Clipboard tools for CLI image paste; xauth supports authenticated X11 access.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        wl-clipboard \
        xclip \
        xauth \
    && rm -rf /var/lib/apt/lists/*

# Optional: document rendering (LibreOffice, PDF tools, fonts). Uncomment when a project needs it.
# RUN apt-get update \
#     && apt-get install -y --no-install-recommends \
#         libreoffice-writer-nogui \
#         poppler-utils \
#         fonts-liberation \
#     && rm -rf /var/lib/apt/lists/*

# Pre-create agent config dirs with correct ownership so the mount targets exist
RUN mkdir -p /home/node/.claude /home/node/.codex /home/node/.gemini /home/node/.cache/codebase-memory-mcp \
    && chown -R node:node /home/node/.claude /home/node/.codex /home/node/.gemini /home/node/.cache

WORKDIR /workspace
