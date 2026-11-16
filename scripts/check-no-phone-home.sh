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
  "[\"']updater:"
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

SEARCH_PATHS=(frontend/src frontend/src-tauri/src frontend/src-tauri/Cargo.toml frontend/src-tauri/capabilities frontend/package.json frontend/tests)
# These must exist, otherwise the guard would have nothing to scan.
REQUIRED_DIRS=(frontend/src frontend/src-tauri/src)

for d in "${REQUIRED_DIRS[@]}"; do
  if [ ! -d "$d" ]; then echo "phone-home guard: required path missing: $d" >&2; exit 1; fi
done
# Tauri merges tauri.<platform>.conf.json and Tauri.toml over tauri.conf.json, so scan every one.
CONF_FILES=()
while IFS= read -r -d '' f; do CONF_FILES+=("$f"); done < <(find frontend/src-tauri -maxdepth 1 -type f \( -iname 'tauri*.conf.json*' -o -iname 'tauri*.toml' \) -print0)
if [ "${#CONF_FILES[@]}" -eq 0 ]; then echo "phone-home guard: no Tauri config found" >&2; exit 1; fi
SEARCH_PATHS+=("${CONF_FILES[@]}")
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

# Updater checks. grep: 0 = matches, 1 = none, >=2 = error (fail closed). Callers must `|| exit 1`:
# the function runs in a $(...) subshell, so only its status can stop the script.
UPD=frontend/src-tauri/src/updates
grep_or_die() { # args: grep options and operands; prints matches
  local out rc
  out=$(grep "$@" 2>"$errf"); rc=$?
  if [ "$rc" -ge 2 ]; then echo "phone-home guard: grep failed (exit $rc):" >&2; cat "$errf" >&2; return 1; fi
  printf '%s' "$out"
}

hits=$(grep_or_die -rnIE '"pubkey"' "${paths[@]}") || exit 1
# Config files also come as TOML (pubkey = "...") and may split the value onto the next line: any pubkey key needs the Conclave value on its line.
hits+=$'\n'$(grep_or_die -niIE "[\"']?pubkey[\"']?[[:space:]]*[:=]" "${CONF_FILES[@]}") || exit 1
while IFS= read -r line; do
  [ -n "$line" ] || continue
  case "$line" in *"$CONCLAVE_UPDATER_PUBKEY"*) ;; *) echo "FOREIGN updater pubkey: $line"; fail=1 ;; esac
done <<< "$hits"

# Endpoints belong in Rust (src/updates) only, never in a Tauri config.
hits=$(grep_or_die -niIE "[\"']?endpoints[\"']?[[:space:]]*[:=]" "${CONF_FILES[@]}") || exit 1
if [ -n "$hits" ]; then echo "updater endpoints in Tauri config:"; echo "$hits"; fail=1; fi

# Workflows must not inject an updater key, endpoints or a config override at build time.
if [ -d .github/workflows ]; then
  hits=$(grep_or_die -rniIE 'pubkey|endpoints|TAURI_CONFIG' .github/workflows) || exit 1
  if [ -n "$hits" ]; then echo "updater config in workflows:"; echo "$hits"; fail=1; fi
fi

# .endpoints( is allowed only under the exact path frontend/src-tauri/src/updates/.
ep=""
while IFS= read -r line; do
  [ -n "$line" ] || continue
  case "$line" in "$UPD"/*) ;; *) ep+="$line"$'\n' ;; esac
done <<< "$(grep_or_die -rnIE '\.endpoints\(' frontend/src-tauri/src)" || exit 1
if [ -n "$ep" ]; then echo "updater endpoints outside src/updates:"; printf '%s' "$ep"; fail=1; fi

# Every URL in non-test updates code must stay inside the Conclave repo API. Scanning stops only at
# `#[cfg(test)]` directly followed by `mod name {`; any other cfg(test) item is still scanned.
H='https://api.github.com/'
if [ -d "$UPD" ]; then
  while IFS= read -r -d '' f; do
    urls=$(awk '
      /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*mod [A-Za-z_0-9]+[[:space:]]*\{/ {exit}
      /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/ {if (held != "") print held; held = $0; next}
      held != "" && /^[[:space:]]*mod [A-Za-z_0-9]+[[:space:]]*\{/ {exit}
      {if (held != "") print held; held = ""; print}
      END {if (held != "") print held}' "$f" | grep_or_die -noiE 'https?://[^"[:space:]]+') || exit 1
    while IFS= read -r u; do
      [ -n "$u" ] || continue
      u=${u#*:}; head=${u:0:${#H}}; head=${head,,}   # scheme and host are case-insensitive, the path is not
      [ "$head" = "$H" ] && [[ ${u:${#H}} == repos/Sashanator/conclave/* ]] || { echo "FOREIGN URL in src/updates: $f:$u"; fail=1; }
    done <<< "$urls"
  done < <(find "$UPD" -type f -name '*.rs' -print0)
fi

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "phone-home guard: clean"
exit 0
