#!/usr/bin/env bash
# Index this repository and keep one watcher alive across agent restarts.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
export GITNEXUS_MEMORY=off

case "${1:-start}" in
  setup|start) ;;
  *) printf 'Usage: %s [setup|start]\n' "$0" >&2; exit 2 ;;
esac

command -v gitnexus >/dev/null || {
  printf '%s\n' 'GitNexus is missing. Rebuild the devcontainer to install gitnexus@1.6.12.' >&2
  exit 1
}
git rev-parse --show-toplevel >/dev/null 2>&1 || {
  printf '%s\n' 'GitNexus needs a Git repository; initialize one before running setup.' >&2
  exit 1
}
command -v flock >/dev/null
command -v setsid >/dev/null

# Separate state for this checkout even when several projects share a HOME.
repo_key="$(printf '%s' "$repo_root" | sha256sum)"
repo_key="${repo_key%% *}"
state_dir="${XDG_CACHE_HOME:-$HOME/.cache}/s-harness-gitnexus/${repo_key:0:12}"
mkdir -p "$state_dir"

# Serialize one-shot setup with the watcher to avoid competing writers. If a
# watcher is already running, it owns initial indexing and ongoing refreshes.
if [[ "${1:-start}" == setup ]]; then
  flock -n -E 0 "$state_dir/watcher.lock" \
    gitnexus analyze --index-only --workers 2
fi

startup_timeout="${GITNEXUS_STARTUP_TIMEOUT_SECONDS:-120}"
if [[ ! "$startup_timeout" =~ ^[1-9][0-9]*$ ]]; then
  printf '%s\n' 'GITNEXUS_STARTUP_TIMEOUT_SECONDS must be a positive integer.' >&2
  exit 2
fi
setsid nohup flock -n -E 75 "$state_dir/watcher.lock" \
  node "$repo_root/.devcontainer/gitnexus-watch.cjs" "$state_dir/ready" \
  >>"$state_dir/watcher.log" 2>&1 </dev/null &
watcher_pid=$!
deadline=$((SECONDS + startup_timeout))
while (( SECONDS < deadline )); do
  if [[ -s "$state_dir/ready" ]]; then
    ready_pid="$(<"$state_dir/ready")"
    if [[ "$ready_pid" =~ ^[0-9]+$ ]] && kill -0 "$ready_pid" 2>/dev/null; then
      exit 0
    fi
  fi
  if [[ -n "$watcher_pid" ]] && ! kill -0 "$watcher_pid" 2>/dev/null; then
    status=0
    wait "$watcher_pid" || status=$?
    if [[ "$status" != 75 ]]; then
      printf 'GitNexus watcher failed before readiness; see %s/watcher.log\n' "$state_dir" >&2
      if [[ "$status" == 0 ]]; then status=1; fi
      exit "$status"
    fi
    watcher_pid="" # Another startup/watcher owns the lock; await its readiness.
  fi
  sleep 0.1
done
printf 'GitNexus watcher readiness timed out after %ss; see %s/watcher.log\n' "$startup_timeout" "$state_dir" >&2
exit 1
