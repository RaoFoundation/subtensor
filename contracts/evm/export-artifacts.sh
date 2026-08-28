#!/usr/bin/env bash
# Build the rails contracts and export ABIs + creation bytecode to a single
# JSON artifact consumed by ts-tests and btcli.
set -euo pipefail

cd "$(dirname "$0")"

OUT="${1:-../../ts-tests/utils/rails-artifacts.json}"

forge build --skip test --skip script >/dev/null

CONTRACTS=(MockUSDC CanonicalShareToken Gateway RailsPortal Chutes)

{
  echo "{"
  first=1
  for c in "${CONTRACTS[@]}"; do
    artifact="out/${c}.sol/${c}.json"
    [ -f "$artifact" ] || { echo "missing artifact $artifact" >&2; exit 1; }
    [ $first -eq 1 ] || echo ","
    first=0
    printf '  "%s": ' "$c"
    jq -c '{abi: .abi, bytecode: .bytecode.object}' "$artifact"
  done
  echo ""
  echo "}"
} > "$OUT"

echo "wrote $(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"
