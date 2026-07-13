#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
CLONE_DIR="clones/mainnet-clone"
CHAIN_SPEC="clones/mainnet-clone-chainspec.json"

usage() {
  echo "usage: local-clone-checkpoint.sh create ARCHIVE | restore ARCHIVE | clear" >&2
  exit 2
}

stop_clone() {
  "$SCRIPT_DIR/stop-local-clone.sh"
}

clear_clone() {
  rm -rf "$CLONE_DIR" "$CHAIN_SPEC"
}

cd "$REPO_ROOT"
command="${1:-}"
shift || true

case "$command" in
  create)
    [[ $# -eq 1 ]] || usage
    archive="$1"
    [[ -d "$CLONE_DIR" ]] || { echo "missing clone directory: $CLONE_DIR" >&2; exit 1; }
    [[ -f "$CHAIN_SPEC" ]] || { echo "missing clone chainspec: $CHAIN_SPEC" >&2; exit 1; }
    stop_clone
    tar -cf "$archive" "$CHAIN_SPEC" "$CLONE_DIR"
    echo "Created clone checkpoint $archive."
    ;;
  restore)
    [[ $# -eq 1 ]] || usage
    archive="$1"
    [[ -f "$archive" ]] || { echo "missing clone checkpoint: $archive" >&2; exit 1; }
    stop_clone
    clear_clone
    # GNU tar detects gzip automatically while reading, so callers do not need
    # to carry the producer's archive format through the workflow.
    tar -xf "$archive"
    [[ -d "$CLONE_DIR" ]] || { echo "checkpoint did not contain $CLONE_DIR" >&2; exit 1; }
    [[ -f "$CHAIN_SPEC" ]] || { echo "checkpoint did not contain $CHAIN_SPEC" >&2; exit 1; }
    echo "Restored clone checkpoint $archive."
    ;;
  clear)
    [[ $# -eq 0 ]] || usage
    stop_clone
    clear_clone
    ;;
  *) usage ;;
esac
