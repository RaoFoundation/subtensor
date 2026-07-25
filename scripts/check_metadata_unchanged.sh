#!/usr/bin/env bash
# Safety oracle for the discoverability migration: docs-stripped structural
# fingerprint of Tier A–C surfaces must match the committed baseline.
#
# Usage:
#   ./scripts/check_metadata_unchanged.sh              # compare to baseline
#   ./scripts/check_metadata_unchanged.sh --write      # refresh baseline
#   ./scripts/check_metadata_unchanged.sh --print      # print fingerprint only
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASELINE="${ROOT}/refactor/metadata-baseline.txt"
EXTRACT="${ROOT}/scripts/extract_metadata_fingerprint.py"

if [[ ! -f "${EXTRACT}" ]]; then
  echo "missing ${EXTRACT}" >&2
  exit 2
fi

case "${1:-}" in
  --write)
    python3 "${EXTRACT}" --write "${BASELINE}"
    ;;
  --print)
    python3 "${EXTRACT}"
    ;;
  "")
    if [[ ! -f "${BASELINE}" ]]; then
      echo "missing baseline ${BASELINE}; run with --write first" >&2
      exit 2
    fi
    python3 "${EXTRACT}" --check "${BASELINE}"
    ;;
  *)
    echo "usage: $0 [--write|--print]" >&2
    exit 2
    ;;
esac
