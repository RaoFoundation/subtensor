#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
helper="$script_dir/release-docker-publication.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"

cat > "$tmp/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

url=""
for arg in "$@"; do
  [[ "$arg" == https://* ]] && url="$arg"
done

if [[ "$url" == https://ghcr.io/token ]]; then
  echo '{"token":"mock-registry-token"}'
  exit 0
fi

[[ "$url" == https://ghcr.io/v2/*/manifests/* ]] || {
  echo "unexpected curl URL: $url" >&2
  exit 2
}

tag="${url##*/}"
published=false
revision="$EXPECTED_SHA"

case "$MOCK_SCENARIO" in
  existing) published=true ;;
  wrong-source)
    published=true
    revision=ffffffffffffffffffffffffffffffffffffffff
    ;;
  tag-only) [[ "$tag" != latest ]] && published=true ;;
  localnet)
    [[ "$url" == *"/raofoundation/subtensor-localnet/manifests/v438" ]] \
      && published=true
    ;;
  active|missing|failed|malformed) ;;
  unavailable) exit 7 ;;
  *)
    echo "unknown mock scenario: $MOCK_SCENARIO" >&2
    exit 2
    ;;
esac

if [[ "$MOCK_SCENARIO" == malformed ]]; then
  echo '{'
  echo 200
  exit 0
fi

if [[ "$published" == true ]]; then
  cat <<JSON
{"schemaVersion":2,"annotations":{"org.opencontainers.image.revision":"$revision","org.opencontainers.image.version":"v438"}}
JSON
  echo 200
else
  echo
  echo 404
fi
EOF
chmod +x "$tmp/bin/curl"

cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >> "$MOCK_GH_LOG"

if [[ "$1" == workflow && "$2" == run ]]; then
  count=$(<"$MOCK_DISPATCH_COUNT")
  echo $(( count + 1 )) > "$MOCK_DISPATCH_COUNT"
  exit 0
fi

[[ "$1" == api ]] || {
  echo "unexpected gh command: $*" >&2
  exit 2
}

case "$MOCK_SCENARIO" in
  active)
    cat <<JSON
{"workflow_runs":[{"id":101,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"in_progress","conclusion":null}]}
JSON
    ;;
  failed)
    cat <<JSON
{"workflow_runs":[{"id":201,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"completed","conclusion":"failure"}]}
JSON
    ;;
  missing|wrong-source)
    echo '{"workflow_runs":[]}'
    ;;
  *)
    echo "unexpected Actions query for scenario: $MOCK_SCENARIO" >&2
    exit 2
    ;;
esac
EOF
chmod +x "$tmp/bin/gh"

export PATH="$tmp/bin:$PATH"
export GH_TOKEN=test-token
export GITHUB_REPOSITORY=RaoFoundation/subtensor
export EXPECTED_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
export RUN_TITLE="Release image v438 from $EXPECTED_SHA"
export MOCK_DISPATCH_COUNT="$tmp/dispatch-count"
export MOCK_GH_LOG="$tmp/gh.log"

reset_fixture() {
  echo 0 > "$MOCK_DISPATCH_COUNT"
  : > "$MOCK_GH_LOG"
}

reset_fixture
export MOCK_SCENARIO=existing
"$helper" status docker.yml v438 "$EXPECTED_SHA" >/dev/null
"$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 0 ]]

reset_fixture
export MOCK_SCENARIO=active
active_log=$("$helper" ensure docker.yml v438 "$EXPECTED_SHA")
grep -q 'run 101 is already publishing' <<<"$active_log"
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 0 ]]

reset_fixture
export MOCK_SCENARIO=missing
"$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 1 ]]
grep -Fq -- "--ref main -f tag=v438 -f expected-sha=$EXPECTED_SHA" "$MOCK_GH_LOG"

# A completed failure is not active. Scheduled reconciliation dispatches a new
# attempt while keeping the failed child visible as its own terminal run.
reset_fixture
export MOCK_SCENARIO=failed
"$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 1 ]]

# Regression: successful Actions metadata cannot satisfy publication when the
# registry identifies a different source commit.
reset_fixture
export MOCK_SCENARIO=wrong-source
if "$helper" status docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected wrong image source metadata to be rejected" >&2
  exit 1
fi
"$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 1 ]]

reset_fixture
export MOCK_SCENARIO=tag-only
if "$helper" status docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected a missing node latest tag to require publication" >&2
  exit 1
fi

reset_fixture
export MOCK_SCENARIO=unavailable
if "$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected an unavailable registry to block dispatch" >&2
  exit 1
fi
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 0 ]]

reset_fixture
export MOCK_SCENARIO=malformed
if "$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected malformed registry metadata to block dispatch" >&2
  exit 1
fi
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 0 ]]

reset_fixture
export MOCK_SCENARIO=localnet
"$helper" status docker-localnet.yml v438 "$EXPECTED_SHA" >/dev/null

if "$helper" status other.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected an unsupported workflow to be rejected" >&2
  exit 1
fi

if "$helper" status docker.yml main "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected a non-release tag to be rejected" >&2
  exit 1
fi

echo "release Docker publication tests passed"
