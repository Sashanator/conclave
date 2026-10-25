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

# GitNexus: code graph for the gitnexus@s-harness plugin (MCP server, hooks, skills).
RUN npm install -g --prefix /usr/local gitnexus@1.6.12

# gitleaks: the harness plugin scans what Claude commits or pushes for secrets.
# Pinned, and checked against the release's checksums file.
ARG GITLEAKS_VERSION=8.30.1
RUN arch="$(uname -m | sed 's/x86_64/x64/;s/aarch64/arm64/')" \
    && cd /tmp \
    && curl -fsSLO "https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VERSION}/gitleaks_${GITLEAKS_VERSION}_linux_${arch}.tar.gz" \
    && curl -fsSLO "https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VERSION}/gitleaks_${GITLEAKS_VERSION}_checksums.txt" \
    && grep " gitleaks_${GITLEAKS_VERSION}_linux_${arch}.tar.gz\$" "gitleaks_${GITLEAKS_VERSION}_checksums.txt" | sha256sum -c - \
    && tar -xzf "gitleaks_${GITLEAKS_VERSION}_linux_${arch}.tar.gz" -C /usr/local/bin gitleaks \
    && rm -f gitleaks_*

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
RUN mkdir -p /home/node/.claude /home/node/.codex /home/node/.gemini /home/node/.gitnexus /home/node/.cache \
    && chown -R node:node /home/node/.claude /home/node/.codex /home/node/.gemini /home/node/.gitnexus /home/node/.cache

# Tauri 2 / Conclave Linux build dependencies (local cargo check/test; Windows is built in CI)
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake pkg-config clang libclang-dev \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
        libasound2-dev libpulse-dev \
    && rm -rf /var/lib/apt/lists/*
# Bun runs the frontend tests (bun:test); actionlint checks workflows locally (saves CI minutes)
RUN npm install -g --prefix /usr/local bun@1.2.21 \
    && curl -sSfL https://raw.githubusercontent.com/rhysd/actionlint/v1.7.7/scripts/download-actionlint.bash | bash -s -- 1.7.7 /usr/local/bin
# pnpm 9.15.9 is the version the frontend docs require; pnpm 12 rejects the committed lockfile
RUN npm install -g --prefix /usr/local pnpm@9.15.9

WORKDIR /workspace
