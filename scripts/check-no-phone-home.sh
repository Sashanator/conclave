#!/usr/bin/env bash
# Fails if telemetry, the updater, Meetily-owned hosts or Meetily data paths reappear in product code.
set -uo pipefail
cd "$(dirname "$0")/.."

PATTERNS=(
  'posthog'
  'track_[a-z_]+\('
  '@/lib/analytics'
  'src/lib/analytics'
  'Analytics(Provider|ConsentSwitch|DataModal)'
)

SEARCH_PATHS=(frontend/src frontend/src-tauri/src frontend/src-tauri/Cargo.toml frontend/src-tauri/tauri.conf.json frontend/src-tauri/capabilities frontend/package.json frontend/tests)
# These must exist, otherwise the guard would have nothing to scan.
REQUIRED_DIRS=(frontend/src frontend/src-tauri/src)
# Archived, uncompiled files (not referenced by any `mod`), matched by exact path.
ARCHIVED=(
  frontend/src-tauri/src/lib_old_complex.rs
  frontend/src-tauri/src/audio/recording_saver_old.rs
  frontend/src-tauri/src/audio/core-old.rs
  frontend/src-tauri/src/audio/recording_commands.rs.backup
)

for d in "${REQUIRED_DIRS[@]}"; do
  if [ ! -d "$d" ]; then echo "phone-home guard: required path missing: $d" >&2; exit 1; fi
done
paths=()
for p in "${SEARCH_PATHS[@]}"; do [ -e "$p" ] && paths+=("$p"); done

errf=$(mktemp); trap 'rm -f "$errf"' EXIT
fail=0
for p in "${PATTERNS[@]}"; do
  # Case-insensitive, so PostHog / POSTHOG_KEY are caught too. grep: 0 = matches, 1 = none, >=2 = error (fail closed).
  raw=$(grep -rniIE -- "$p" "${paths[@]}" 2>"$errf"); rc=$?
  if [ "$rc" -ge 2 ]; then
    echo "phone-home guard: grep failed for pattern '$p' (exit $rc):" >&2
    cat "$errf" >&2
    exit 1
  fi
  [ "$rc" -eq 0 ] || continue
  hits=""
  while IFS= read -r line; do
    file=${line%%:*}
    content=${line#*:*:}
    skip=0
    for a in "${ARCHIVED[@]}"; do [ "$file" = "$a" ] && skip=1; done
    [ "$skip" -eq 1 ] && continue
    if [ "$p" = 'track_[a-z_]+\(' ]; then
      # packet.track_id() is Symphonia's decoder API (audio/decoder.rs), not telemetry: judge the line without it.
      content=${content//'packet.track_id()'/}
      printf '%s\n' "$content" | grep -qiE -- "$p" || continue
    fi
    hits+="$line"$'\n'
  done <<< "$raw"
  if [ -n "$hits" ]; then
    echo "FORBIDDEN pattern '$p':"; printf '%s' "$hits"; fail=1
  fi
done

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "phone-home guard: clean"
exit 0
