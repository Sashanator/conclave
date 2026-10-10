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
EXCLUDES=(--exclude='*.backup' --exclude='*_old*.rs' --exclude='core-old.rs' --exclude='lib_old_complex.rs')

fail=0
for p in "${PATTERNS[@]}"; do
  # Test the output, not grep's status: grep exits 2 if any search path is missing, even when it matched.
  # packet.track_id() is Symphonia's decoder API (audio/decoder.rs), not telemetry.
  hits=$(grep -rnIE "${EXCLUDES[@]}" -- "$p" "${SEARCH_PATHS[@]}" 2>/dev/null | grep -vF 'packet.track_id()' || true)
  if [ -n "$hits" ]; then
    echo "FORBIDDEN pattern '$p':"; echo "$hits"; fail=1
  fi
done
[ "$fail" -eq 0 ] && echo "phone-home guard: clean"
exit "$fail"
