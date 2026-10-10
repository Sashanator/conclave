#!/bin/bash
# Status line: model | 5h quota used + time to reset | 7d quota used + time to reset
# Context colors: green < 60%, yellow < 80%, red >= 80% (time to /compact)
# Usage colors: green < 50%, yellow < 80%, red >= 80%
input=$(cat)
now=$(date +%s)

RESET=$'\e[0m' DIM=$'\e[2m' BOLD=$'\e[1m'
GREEN=$'\e[32m' YELLOW=$'\e[33m' RED=$'\e[31m' CYAN=$'\e[36m' MAGENTA=$'\e[35m'

fmt_left() {
  local s=$(( $1 - now ))
  (( s < 0 )) && s=0
  local d=$((s/86400)) h=$((s%86400/3600)) m=$((s%3600/60))
  if (( d > 0 )); then echo "${d}d${h}h"; elif (( h > 0 )); then echo "${h}h${m}m"; else echo "${m}m"; fi
}

limit() {
  local label=$1 key=$2
  local pct=$(jq -r ".rate_limits.$key.used_percentage // empty" <<<"$input")
  local reset=$(jq -r ".rate_limits.$key.resets_at // empty" <<<"$input")
  [[ -z $pct ]] && return
  local p=$(printf '%.0f' "$pct") color=$GREEN
  (( p >= 50 )) && color=$YELLOW
  (( p >= 80 )) && color=$RED
  local out="${MAGENTA}${label}${RESET} ${color}${BOLD}${p}%${RESET}"
  [[ -n $reset ]] && out+=" ${DIM}↻ $(fmt_left "${reset%.*}")${RESET}"
  printf ' %s│%s %s' "$DIM" "$RESET" "$out"
}

ctx() {
  local pct=$(jq -r '.context_window.used_percentage // empty' <<<"$input")
  [[ -z $pct ]] && return
  local p=$(printf '%.0f' "$pct") color=$GREEN
  (( p >= 60 )) && color=$YELLOW
  (( p >= 80 )) && color=$RED
  local size=$(jq -r '.context_window.context_window_size // empty' <<<"$input")
  local out="${MAGENTA}ctx${RESET} ${color}${BOLD}${p}%${RESET} ${DIM}($((100-p))% left"
  [[ -n $size ]] && out+=" of $((size/1000))k"
  printf ' %s│%s %s%s' "$DIM" "$RESET" "$out" "${DIM})${RESET}"
}

model=$(jq -r '.model.display_name // "Claude"' <<<"$input")
quota="$(limit 5h five_hour)$(limit 7d seven_day)$(limit Spend spend_limit)"
[[ -z $quota ]] && quota=" ${DIM}│ quota: n/a until first response${RESET}"
effort=$(jq -r '.effort.level // empty' <<<"$input")
[[ -n $effort ]] && model+=" ${RESET}${DIM}(${effort})"
echo "${CYAN}${BOLD}${model}${RESET}$(ctx)${quota}"
