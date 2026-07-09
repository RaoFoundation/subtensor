#!/usr/bin/env bash
# Check if the "skip-validate-benchmarks" label is present on a PR.
# Usage: check-skip-label.sh <PR_NUMBER>
# Always exits 0. Writes skip=true to $GITHUB_OUTPUT when the label is
# found so the consuming job can skip its benchmark steps.

set -euo pipefail

PR_NUMBER="${1:-}"
[[ -z "$PR_NUMBER" ]] && exit 0

REPO="${GITHUB_REPOSITORY:-}"
[[ -z "$REPO" ]] && exit 0

labels=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json labels --jq '.labels[].name' 2>/dev/null || true)

if echo "$labels" | grep -q "skip-validate-benchmarks"; then
  echo "skip-validate-benchmarks label found — skipping benchmark validation."
  echo "skip=true" >> "$GITHUB_OUTPUT"
fi
