#!/usr/bin/env bash

set -euo pipefail
trap 'printf "sccache configuration test failed at line %s: %s\n" "$LINENO" "$BASH_COMMAND" >&2' ERR

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly CONFIGURE="$SCRIPT_DIR/sccache-configure.sh"
readonly ACCESS_KEY="reader-access-key-test"
readonly SECRET_KEY="reader-secret-key-test"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/runner"

cat > "$tmp/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
for argument in "$@"; do
  if [[ "$argument" == */token ]]; then
    token_request=true
  fi
done
if [[ "${token_request:-false}" == true ]]; then
  [[ "${MOCK_MMDS_FAIL:-false}" != true ]] || exit 22
  printf 'test-token'
  exit 0
fi
[[ "${MOCK_MMDS_FAIL:-false}" != true ]] || exit 22
output=''
while [[ $# -gt 0 ]]; do
  if [[ "$1" == --output ]]; then
    output="$2"
    shift 2
  else
    shift
  fi
done
cp "$MOCK_METADATA" "$output"
EOF
chmod +x "$tmp/bin/curl"

cat > "$tmp/bin/sccache" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  --stop-server) exit 0 ;;
  --start-server) [[ "${MOCK_START_FAIL:-false}" != true ]] ;;
  --show-stats) printf 'Compile requests                     1\nCache write errors                   1\n'; exit 0 ;;
  *) exit 0 ;;
esac
EOF
chmod +x "$tmp/bin/sccache"

export PATH="$tmp/bin:$PATH"
export RUNNER_TEMP="$tmp/runner"
export GITHUB_RUN_ID=1
export GITHUB_JOB=test
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export MMDS_TOKEN_URL=http://mmds/token
export MMDS_METADATA_URL=http://mmds/sccache
export SCCACHE_PATH="$tmp/bin/sccache"

write_metadata() {
  local endpoint="${1:-https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com}"
  cat > "$tmp/metadata.json" <<EOF
{"bucket":"subtensor-ci-sccache","endpoint":"$endpoint","region":"auto","s3_use_ssl":true,"s3_rw_mode":"READ_ONLY","key_prefix":"subtensor/v1","access_key_id":"$ACCESS_KEY","secret_access_key":"$SECRET_KEY"}
EOF
  export MOCK_METADATA="$tmp/metadata.json"
}

reset_outputs() {
  : > "$tmp/output"
  : > "$tmp/env"
  rm -f "$tmp/config.json"
  unset MOCK_MMDS_FAIL MOCK_START_FAIL SCCACHE_GHA_FALLBACK AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
  export GITHUB_OUTPUT="$tmp/output"
  export GITHUB_ENV="$tmp/env"
  export GITHUB_EVENT_PATH="$tmp/event.json"
  printf '{}\n' > "$GITHUB_EVENT_PATH"
  export GITHUB_EVENT_NAME=pull_request
  export GITHUB_REF=refs/pull/1/merge
}

write_pr_event() {
  local repository="$1"
  local fork="$2"
  local login="${3:-trusted-contributor}"
  cat > "$GITHUB_EVENT_PATH" <<EOF
{"pull_request":{"head":{"repo":{"full_name":"$repository","fork":$fork}},"user":{"login":"$login"}}}
EOF
}

write_dispatch_event() {
  local source_ref="$1"
  printf '{"inputs":{"source_ref":"%s"}}\n' "$source_ref" > "$GITHUB_EVENT_PATH"
}

assert_contains() {
  grep -Fq "$2" "$1" || { printf 'expected %s in %s\n' "$2" "$1" >&2; exit 1; }
}

assert_not_contains() {
  if grep -Fq "$2" "$1"; then
    printf 'did not expect %s in %s\n' "$2" "$1" >&2
    exit 1
  fi
}

write_metadata
reset_outputs
prepare_log="$tmp/prepare.log"
"$CONFIGURE" prepare reader "$tmp/config.json" "$tmp/output" >"$prepare_log"
assert_contains "$tmp/output" 'available=true'
config_mode="$(python3 -c 'import os, stat, sys; print(oct(stat.S_IMODE(os.stat(sys.argv[1]).st_mode))[2:])' "$tmp/config.json")"
[[ "$config_mode" == 600 ]]
grep -v '^::add-mask::' "$prepare_log" > "$tmp/prepare-public.log"
assert_not_contains "$tmp/prepare-public.log" "$ACCESS_KEY"
assert_not_contains "$tmp/prepare-public.log" "$SECRET_KEY"
SCCACHE_INSTALL_OUTCOME=success "$CONFIGURE" activate "$tmp/config.json" "$tmp/env" "$tmp/output" >"$tmp/activate.log"
assert_contains "$tmp/output" 'enabled=true'
assert_contains "$tmp/env" 'RUSTC_WRAPPER=sccache'
assert_contains "$tmp/env" 'CARGO_INCREMENTAL=0'
assert_contains "$tmp/env" 'SCCACHE_S3_KEY_PREFIX=subtensor/v1'
assert_contains "$tmp/env" 'SCCACHE_IGNORE_SERVER_IO_ERROR=1'
grep -v '^::add-mask::' "$tmp/activate.log" > "$tmp/activate-public.log"
assert_not_contains "$tmp/activate-public.log" "$ACCESS_KEY"
assert_not_contains "$tmp/activate-public.log" "$SECRET_KEY"

reset_outputs
export MOCK_MMDS_FAIL=true
"$CONFIGURE" prepare reader "$tmp/config.json" "$tmp/output" >"$tmp/unavailable.log"
assert_contains "$tmp/output" 'available=true'
SCCACHE_INSTALL_OUTCOME=success "$CONFIGURE" activate "$tmp/config.json" "$tmp/env" "$tmp/output" >"$tmp/gha.log"
assert_contains "$tmp/output" 'enabled=true'
assert_contains "$tmp/env" 'SCCACHE_BACKEND=gha'
assert_contains "$tmp/env" 'SCCACHE_GHA_ENABLED=true'
assert_contains "$tmp/env" 'RUSTC_WRAPPER=sccache'

reset_outputs
export MOCK_MMDS_FAIL=true
export SCCACHE_GHA_FALLBACK=false
"$CONFIGURE" prepare reader "$tmp/config.json" "$tmp/output" >"$tmp/unavailable-disabled.log"
assert_contains "$tmp/output" 'available=false'
[[ ! -e "$tmp/config.json" ]]

write_metadata http://invalid.example.com
reset_outputs
export SCCACHE_GHA_FALLBACK=false
"$CONFIGURE" prepare reader "$tmp/config.json" "$tmp/output" >"$tmp/invalid.log"
assert_contains "$tmp/output" 'available=false'
assert_not_contains "$tmp/invalid.log" "$ACCESS_KEY"
assert_not_contains "$tmp/invalid.log" "$SECRET_KEY"

write_metadata
reset_outputs
"$CONFIGURE" prepare reader "$tmp/config.json" "$tmp/output" >/dev/null
export MOCK_START_FAIL=true
SCCACHE_INSTALL_OUTCOME=success "$CONFIGURE" activate "$tmp/config.json" "$tmp/env" "$tmp/output" >"$tmp/start-fail.log"
assert_contains "$tmp/output" 'enabled=false'
assert_not_contains "$tmp/env" 'RUSTC_WRAPPER='
assert_not_contains "$tmp/env" 'SCCACHE_IGNORE_SERVER_IO_ERROR='

reset_outputs
export AWS_ACCESS_KEY_ID=writer-access-key-test
export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
"$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-pr.log"
assert_contains "$tmp/output" 'available=false'

reset_outputs
write_pr_event RaoFoundation/subtensor false
export AWS_ACCESS_KEY_ID=writer-access-key-test
export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
"$CONFIGURE" prepare auto "$tmp/config.json" "$tmp/output" >"$tmp/auto-pr.log"
assert_contains "$tmp/output" 'available=true'
assert_contains "$tmp/config.json" '"mode":"writer"'

for reader_case in fork dependabot malformed target missing-credentials partial-credentials malformed-credentials; do
  reset_outputs
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  case "$reader_case" in
    fork) write_pr_event external/fork true ;;
    dependabot) write_pr_event RaoFoundation/subtensor false 'dependabot[bot]' ;;
    malformed) printf '{not-json\n' > "$GITHUB_EVENT_PATH" ;;
    target)
      write_pr_event RaoFoundation/subtensor false
      export GITHUB_EVENT_NAME=pull_request_target
      export GITHUB_REF=refs/heads/main
      ;;
    missing-credentials)
      write_pr_event RaoFoundation/subtensor false
      unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
      ;;
    partial-credentials)
      write_pr_event RaoFoundation/subtensor false
      unset AWS_SECRET_ACCESS_KEY
      ;;
    malformed-credentials)
      write_pr_event RaoFoundation/subtensor false
      export AWS_SECRET_ACCESS_KEY=$'writer-secret-key-test\ninvalid'
      ;;
  esac
  "$CONFIGURE" prepare auto "$tmp/config.json" "$tmp/output" >"$tmp/auto-$reader_case.log"
  assert_contains "$tmp/output" 'available=true'
  assert_contains "$tmp/config.json" '"mode":"reader"'
done

reset_outputs
export GITHUB_EVENT_NAME=push
export GITHUB_REF=refs/heads/mono-bittensor
export AWS_ACCESS_KEY_ID=writer-access-key-test
export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
"$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-non-main.log"
assert_contains "$tmp/output" 'available=false'

reset_outputs
export AWS_ACCESS_KEY_ID=writer-access-key-test
export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
export GITHUB_EVENT_NAME=push
export GITHUB_REF=refs/heads/main
"$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-main.log"
assert_contains "$tmp/output" 'available=true'
grep -v '^::add-mask::' "$tmp/writer-main.log" > "$tmp/writer-main-public.log"
assert_not_contains "$tmp/writer-main-public.log" 'writer-access-key-test'
assert_not_contains "$tmp/writer-main-public.log" 'writer-secret-key-test'
SCCACHE_INSTALL_OUTCOME=success "$CONFIGURE" activate "$tmp/config.json" "$tmp/env" "$tmp/output" >"$tmp/writer-activate.log"
assert_contains "$tmp/output" 'enabled=true'
assert_contains "$tmp/env" 'SCCACHE_BACKEND=r2'
assert_contains "$tmp/env" 'RUSTC_WRAPPER=sccache'

for untrusted_branch in bittensor-core-exploration codex/subtensor-r2-sccache; do
  for untrusted_event in push workflow_dispatch; do
    reset_outputs
    export AWS_ACCESS_KEY_ID=writer-access-key-test
    export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
    export GITHUB_EVENT_NAME="$untrusted_event"
    export GITHUB_REF="refs/heads/$untrusted_branch"
    "$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-untrusted.log"
    assert_contains "$tmp/output" 'available=false'
  done
done

for trusted_event in schedule workflow_dispatch; do
  reset_outputs
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  export GITHUB_EVENT_NAME="$trusted_event"
  export GITHUB_REF=refs/heads/main
  if [[ "$trusted_event" == workflow_dispatch ]]; then
    write_dispatch_event main
  fi
  "$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-$trusted_event.log"
  assert_contains "$tmp/output" 'available=true'
done

for trusted_branch in main devnet testnet; do
  reset_outputs
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  export GITHUB_EVENT_NAME=push
  export GITHUB_REF="refs/heads/$trusted_branch"
  "$CONFIGURE" prepare auto "$tmp/config.json" "$tmp/output" >"$tmp/auto-$trusted_branch.log"
  assert_contains "$tmp/output" 'available=true'
  assert_contains "$tmp/config.json" '"mode":"writer"'
done

for manual_source in main devnet testnet; do
  reset_outputs
  write_dispatch_event "$manual_source"
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  export GITHUB_EVENT_NAME=workflow_dispatch
  export GITHUB_REF=refs/heads/main
  "$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-dispatch-$manual_source.log"
  assert_contains "$tmp/output" 'available=true'
done

reset_outputs
export AWS_ACCESS_KEY_ID=writer-access-key-test
export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
export GITHUB_EVENT_NAME=schedule
export GITHUB_REF=refs/heads/main
"$CONFIGURE" prepare auto "$tmp/config.json" "$tmp/output" >"$tmp/auto-schedule.log"
assert_contains "$tmp/output" 'available=true'
assert_contains "$tmp/config.json" '"mode":"writer"'

for manual_source in main devnet testnet; do
  reset_outputs
  write_dispatch_event "$manual_source"
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  export GITHUB_EVENT_NAME=workflow_dispatch
  export GITHUB_REF=refs/heads/main
  "$CONFIGURE" prepare auto "$tmp/config.json" "$tmp/output" >"$tmp/auto-dispatch-$manual_source.log"
  assert_contains "$tmp/output" 'available=true'
  assert_contains "$tmp/config.json" '"mode":"writer"'
done

for dispatch_payload in rejected malformed; do
  reset_outputs
  if [[ "$dispatch_payload" == malformed ]]; then
    printf '{not-json\n' > "$GITHUB_EVENT_PATH"
  else
    write_dispatch_event feature/foo
  fi
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  export GITHUB_EVENT_NAME=workflow_dispatch
  export GITHUB_REF=refs/heads/main
  "$CONFIGURE" prepare auto "$tmp/config.json" "$tmp/output" >"$tmp/auto-dispatch-$dispatch_payload.log"
  assert_contains "$tmp/output" 'available=true'
  assert_contains "$tmp/config.json" '"mode":"reader"'
done

for rejected_source in feature/foo mainnet ''; do
  reset_outputs
  write_dispatch_event "$rejected_source"
  export AWS_ACCESS_KEY_ID=writer-access-key-test
  export AWS_SECRET_ACCESS_KEY=writer-secret-key-test
  export GITHUB_EVENT_NAME=workflow_dispatch
  export GITHUB_REF=refs/heads/main
  "$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-dispatch-rejected.log"
  assert_contains "$tmp/output" 'available=false'
done

reset_outputs
export GITHUB_EVENT_NAME=push
export GITHUB_REF=refs/heads/main
"$CONFIGURE" prepare writer "$tmp/config.json" "$tmp/output" >"$tmp/writer-missing.log"
assert_contains "$tmp/output" 'available=false'

# Keep read-only write errors visible in the stats contract. The live turbo
# integration compile proves those errors do not fail rustc invocations.
"$tmp/bin/sccache" --show-stats >"$tmp/stats"
assert_contains "$tmp/stats" 'Cache write errors                   1'

printf 'sccache configuration tests passed\n'
