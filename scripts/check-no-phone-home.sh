#!/usr/bin/env bash
# Fails if telemetry, a foreign updater, Meetily-owned hosts or Meetily data paths reappear in product code.
set -uo pipefail
cd "$(dirname "$0")/.."

# The only updater key the app may ship (tauri.conf.json plugins.updater.pubkey).
CONCLAVE_UPDATER_PUBKEY='dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDJDN0RGM0M1MDI2ODY3MjcKUldRbloyZ0N4Zk45TEUvSWdVYlo0dWFTWlNWYWZYbDVMMWhHWWVTMGN2V1dRTzhNWGRSL0NpTWEK'

PATTERNS=(
  'posthog'
  'track_[a-z_]+\('
  '@/lib/analytics'
  'src/lib/analytics'
  'Analytics(Provider|ConsentSwitch|DataModal)'
  '"updater:'
  '\.pubkey\('
  '"endpoints"[[:space:]]*:'
  'releases/latest/download/latest\.json'
  'zackriya\.com'
  'github\.com/your-org'
  'github\.com/Zackriya-Solutions'
  'join\("Meetily"\)'
  'push\("Meetily"\)'
  '"Meetily/"'
  '"identifier": "com\.meetily\.ai"'
  '"productName": "meetily"'
  'Notification::new\("Meetily'
  '"Meetily( Error)?",'
  '= "Meetily";'
  'Welcome to meetily'
)

SEARCH_PATHS=(frontend/src frontend/src-tauri/src frontend/src-tauri/Cargo.toml frontend/src-tauri/tauri.conf.json frontend/src-tauri/capabilities frontend/package.json frontend/tests)
# These must exist, otherwise the guard would have nothing to scan.
REQUIRED_DIRS=(frontend/src frontend/src-tauri/src)

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

# Updater checks. grep: 0 = matches, 1 = none, >=2 = error (fail closed).
UPD=frontend/src-tauri/src/updates
grep_or_die() { # args: grep options and operands; prints matches, exits 1 on grep error
  local out rc
  out=$(grep "$@" 2>"$errf"); rc=$?
  if [ "$rc" -ge 2 ]; then echo "phone-home guard: grep failed (exit $rc):" >&2; cat "$errf" >&2; exit 1; fi
  printf '%s' "$out"
}

while IFS= read -r line; do
  [ -n "$line" ] || continue
  case "$line" in *"$CONCLAVE_UPDATER_PUBKEY"*) ;; *) echo "FOREIGN updater pubkey: $line"; fail=1 ;; esac
done <<< "$(grep_or_die -rnIE '"pubkey"' "${paths[@]}")"

# Skip updates/ in the first pass: it is the only place allowed to call .endpoints(.
ep=$(grep_or_die -rnIE '\.endpoints\(' --exclude-dir=updates frontend/src-tauri/src)
if [ -n "$ep" ]; then echo "updater endpoints outside src/updates:"; echo "$ep"; fail=1; fi

# Every URL in non-test updates code must stay inside the Conclave repo API.
if [ -d "$UPD" ]; then
  for f in "$UPD"/*.rs; do
    [ -e "$f" ] || continue
    urls=$(awk '/#\[cfg\(test\)\]/{exit} {print}' "$f" | grep_or_die -noE 'https?://[^"[:space:]]+')
    while IFS= read -r u; do
      [ -n "$u" ] || continue
      case "${u#*:}" in https://api.github.com/repos/Sashanator/conclave/*) ;; *) echo "FOREIGN URL in src/updates: $f:$u"; fail=1 ;; esac
    done <<< "$urls"
  done
fi

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "phone-home guard: clean"
exit 0
