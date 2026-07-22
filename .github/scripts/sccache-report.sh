#!/usr/bin/env bash

# Emit the most detailed sccache statistics supported by the installed client,
# while keeping observability fail-open. The optional prefix controls the two
# output files: PREFIX.txt (human-readable) and PREFIX.json (machine-readable).

set -uo pipefail

label="${1:-Compiler cache}"
slug="$(printf '%s' "$label" | tr -cs '[:alnum:]._-' '-' | sed 's/^-//; s/-$//')"
[[ -n "$slug" ]] || slug=sccache
prefix="${2:-${RUNNER_TEMP:-/tmp}/sccache-${slug}}"
text_path="${prefix}.txt"
json_path="${prefix}.json"
backend="${SCCACHE_BACKEND:-disabled}"
local_tier="${SCCACHE_LOCAL_TIER:-false}"
stats_command=unavailable

mkdir -p "$(dirname "$prefix")" 2>/dev/null || true

if [[ "${SCCACHE_ENABLED:-false}" == true ]] && command -v sccache >/dev/null 2>&1; then
  if sccache --show-adv-stats >"$text_path" 2>&1; then
    stats_command=advanced
  elif sccache --show-stats >"$text_path" 2>&1; then
    stats_command=basic
  else
    printf 'sccache statistics were unavailable\n' >"$text_path" 2>/dev/null || true
  fi

  if ! sccache --show-stats --stats-format=json >"$json_path" 2>/dev/null; then
    printf '{}\n' >"$json_path" 2>/dev/null || true
  fi
else
  printf 'sccache is disabled for this job\n' >"$text_path" 2>/dev/null || true
  printf '{}\n' >"$json_path" 2>/dev/null || true
fi

echo "sccache report: backend=$backend host-local=$local_tier stats=$stats_command"
[[ ! -f "$text_path" ]] || cat "$text_path"

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  if ! {
    echo "### $label"
    echo "- Configured backend: $backend"
    echo "- Host-local tier active: $local_tier"
    echo "- Statistics detail: $stats_command"
    if [[ "$local_tier" == true ]]; then
      echo "- Per-tier note: sccache 0.15 combines host-local and R2 hits in these totals"
    fi
    echo '```text'
    if [[ -f "$text_path" ]]; then
      cat "$text_path"
    else
      echo "sccache report could not be written"
    fi
    echo '```'
  } >>"$GITHUB_STEP_SUMMARY"; then
    echo "::warning::could not append sccache statistics to the job summary"
  fi
fi

# Cache reporting must never change the result of the build it observes.
exit 0
