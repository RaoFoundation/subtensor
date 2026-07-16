#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cleanup_script="$repo_root/.github/scripts/cleanup-localnet-ci-images.sh"
fixture="$(mktemp)"
trap 'rm -f "$fixture"' EXIT

cat >"$fixture" <<'JSON'
[
  {"id":1,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":[]}}},
  {"id":2,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":["ci-1111111111111111111111111111111111111111"]}}},
  {"id":3,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":["ci-2222222222222222222222222222222222222222","ci-3333333333333333333333333333333333333333"]}}},
  {"id":4,"created_at":"2026-06-16T00:00:00Z","metadata":{"container":{"tags":["ci-4444444444444444444444444444444444444444"]}}},
  {"id":5,"created_at":"2026-07-01T00:00:00Z","metadata":{"container":{"tags":["ci-5555555555555555555555555555555555555555"]}}},
  {"id":6,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":["ci-short"]}}},
  {"id":7,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":["ci-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"]}}},
  {"id":8,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":["ci-8888888888888888888888888888888888888888","main"]}}},
  {"id":9,"created_at":"2026-06-01T00:00:00Z","metadata":{"container":{"tags":["main"]}}}
]
JSON

actual="$($cleanup_script select "$fixture" 2026-06-16T00:00:00Z | jq -r '.id' | paste -sd, -)"
expected="1,2,3"
[[ "$actual" == "$expected" ]] || {
  echo "unexpected cleanup candidates: expected=$expected actual=$actual" >&2
  exit 1
}

echo "localnet CI image cleanup policy tests passed"
