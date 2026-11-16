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
  '\.pubkey[[:space:]]*\('
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
NONCONF_PATHS=("${SEARCH_PATHS[@]}")
SEARCH_PATHS+=("${CONF_FILES[@]}")
paths=()
for p in "${SEARCH_PATHS[@]}"; do [ -e "$p" ] && paths+=("$p"); done
nonconf=()
for p in "${NONCONF_PATHS[@]}"; do [ -e "$p" ] && nonconf+=("$p"); done

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

# Outside the Tauri configs a "pubkey" line must carry the Conclave key.
hits=$(grep_or_die -rnIE '"pubkey"' "${nonconf[@]}") || exit 1
while IFS= read -r line; do
  [ -n "$line" ] || continue
  case "$line" in *"$CONCLAVE_UPDATER_PUBKEY"*) ;; *) echo "FOREIGN updater pubkey: $line"; fail=1 ;; esac
done <<< "$hits"

# Tauri configs: JSON is parsed (text checks are fooled by escapes and extra keys on one line); plugins.updater.pubkey
# must equal the pinned key and plugins.updater.endpoints must not exist. Other formats (json5, toml) get a strict text check.
for f in "${CONF_FILES[@]}"; do
  case "$f" in
    *.json)
      msg=$(CONCLAVE_UPDATER_PUBKEY="$CONCLAVE_UPDATER_PUBKEY" node -e '
        const u = (() => { try { return JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); } catch (e) { console.log("unparseable JSON"); process.exit(0); } })()?.plugins?.updater;
        if (u === undefined) process.exit(0);
        if (u === null || typeof u !== "object" || Array.isArray(u)) console.log("plugins.updater is not an object");
        else if ("endpoints" in u) console.log("updater endpoints in Tauri config");
        else if ("pubkey" in u && u.pubkey !== process.env.CONCLAVE_UPDATER_PUBKEY) console.log("FOREIGN updater pubkey");
      ' "$f" 2>"$errf") || { echo "phone-home guard: node failed on $f:" >&2; cat "$errf" >&2; exit 1; }
      if [ -n "$msg" ]; then echo "$msg: $f"; fail=1; fi ;;
    *)
      bad=$(grep -niIE 'pubkey|endpoints|\\[ux][0-9a-f{]' "$f" | grep -vxE "[0-9]+:pubkey[[:space:]]*=[[:space:]]*\"$CONCLAVE_UPDATER_PUBKEY\"[[:space:]]*") || true
      if [ -n "$bad" ]; then echo "FOREIGN updater pubkey or endpoints in $f:"; echo "$bad"; fail=1; fi ;;
  esac
done

# Workflows must not inject an updater key, endpoints or a config override at build time.
if [ -d .github/workflows ]; then
  hits=$(grep_or_die -rniIE 'pubkey|endpoints|TAURI_CONFIG' .github/workflows) || exit 1
  if [ -n "$hits" ]; then echo "updater config in workflows:"; echo "$hits"; fail=1; fi
fi

# .endpoints( is allowed only under the exact path frontend/src-tauri/src/updates/.
ep=""
out=$(grep_or_die -rnIE '\.endpoints[[:space:]]*\(' frontend/src-tauri/src) || exit 1
while IFS= read -r line; do
  [ -n "$line" ] || continue
  case "$line" in "$UPD"/*) ;; *) ep+="$line"$'\n' ;; esac
done <<< "$out"
if [ -n "$ep" ]; then echo "updater endpoints outside src/updates:"; printf '%s' "$ep"; fail=1; fi

# Every URL in non-test updates code must stay inside the Conclave repo API. Scanning skips only
# `#[cfg(test)] mod name {` (up to the next column-0 `}`), then resumes; any other cfg(test) item is still scanned.
H='https://api.github.com/'
if [ -d "$UPD" ]; then
  while IFS= read -r -d '' f; do
    urls=$(awk '
      skip {if ($0 == "}") skip = 0; next}
      /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*mod [A-Za-z_0-9]+[[:space:]]*\{[[:space:]]*$/ {if (held != "") print held; held = ""; skip = 1; next}
      /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/ {if (held != "") print held; held = $0; next}
      held != "" && /^[[:space:]]*(pub[[:space:]]+)?mod [A-Za-z_0-9]+[[:space:]]*\{[[:space:]]*$/ {held = ""; skip = 1; next}
      {if (held != "") print held; held = ""; print}
      END {if (held != "") print held}' "$f" | grep_or_die -noiE 'https?://[^"[:space:]]+') || exit 1
    while IFS= read -r u; do
      [ -n "$u" ] || continue
      u=${u#*:}; head=${u:0:${#H}}; head=${head,,}   # scheme and host are case-insensitive, the path is not
      [ "$head" = "$H" ] && [[ ${u:${#H}} == repos/Sashanator/conclave/* && ${u:${#H}} != *..* && ${u:${#H}} != *%2[eE]* ]] || { echo "FOREIGN URL in src/updates: $f:$u"; fail=1; }
    done <<< "$urls"
  done < <(find "$UPD" -type f -name '*.rs' -print0)
fi

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "phone-home guard: clean"
exit 0
