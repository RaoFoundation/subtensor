#!/usr/bin/env bash
# Exercise the credential gate without contacting GitHub or reading credentials.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
source <(sed -n '/^push_actor_check() {$/,/^}$/p' "$ROOT/scripts/preflight.sh")
PREFLIGHT_PUSH_ACTOR=unarbos
PREFLIGHT_PUSH_REMOTE=test
CALLS=$(mktemp)
trap 'rm -f "$CALLS"' EXIT

ssh() {
  printf '%s\n' "$*" >>"$CALLS"
  if [[ " $* " == *' -G '* ]]; then
    printf 'hostname %s\n' "$MOCK_HOST"
    return 0
  fi
  [[ -z "$MOCK_LOGIN" ]] || printf "Hi %s! You've successfully authenticated, but GitHub does not provide shell access.\n" "$MOCK_LOGIN"
  return 1 # GitHub's successful authentication still exits nonzero.
}
git() {
  [[ "$*" == 'credential fill' ]] || return 1
  cat >/dev/null
  [[ -z "$MOCK_TOKEN" ]] || printf 'password=%s\n' "$MOCK_TOKEN"
}
curl() {
  # The token must arrive on stdin, never in argv.
  [[ "$*" != *'test-secret'* ]] || return 1
  local config
  config=$(cat)
  [[ "$config" == 'header = "Authorization: Bearer test-secret"' ]] || return 1
  printf '{"login":"%s"}\n' "$MOCK_LOGIN"
}
check() {
  local expected=$1 url=$2 actual=fail output
  PREFLIGHT_PUSH_URL=$url
  : >"$CALLS"
  if output=$(push_actor_check); then actual=pass; fi
  [[ "$output" != *'test-secret'* ]] || { echo 'credential leaked'; exit 1; }
  [[ "$actual" == "$expected" ]] || { printf 'Expected %s: %s\n' "$expected" "$output"; exit 1; }
}
MOCK_HOST=github.com MOCK_LOGIN=unarbos MOCK_TOKEN=test-secret
check pass git@github.com:RaoFoundation/subtensor.git
check pass git@github-unarbos:RaoFoundation/subtensor.git
grep -q -- '-T git@github-unarbos$' "$CALLS"
MOCK_HOST=ssh.github.com
check pass ssh://git@github-unarbos:443/RaoFoundation/subtensor.git
grep -q -- '-p 443 -T git@github-unarbos$' "$CALLS"
MOCK_HOST=github.com MOCK_LOGIN=UnArbos
check pass git@github-unarbos:RaoFoundation/subtensor.git
MOCK_LOGIN=UnArbosSix
check fail git@github-unarbos:RaoFoundation/subtensor.git
MOCK_LOGIN=''
check fail git@github-unarbos:RaoFoundation/subtensor.git
MOCK_HOST=example.org MOCK_LOGIN=unarbos
check fail git@not-github:RaoFoundation/subtensor.git
! grep -q -- ' -T ' "$CALLS"
MOCK_HOST=github.com
check pass https://unarbos:test-secret@github.com/RaoFoundation/subtensor.git
check pass https://github.com/RaoFoundation/subtensor.git
MOCK_LOGIN=someone-else
check fail https://github.com/RaoFoundation/subtensor.git
MOCK_TOKEN=''
check fail https://github.com/RaoFoundation/subtensor.git
check fail https://github.com.example.org/RaoFoundation/subtensor.git
check fail ssh://git@example.org/RaoFoundation/subtensor.git
printf 'push actor tests: 13 passed\n'
