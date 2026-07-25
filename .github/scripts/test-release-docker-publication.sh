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
count=$(<"$MOCK_DISPATCH_COUNT")
published=false
revision="$EXPECTED_SHA"
version=v438

case "$MOCK_SCENARIO" in
  existing)
    published=true
    ;;
  retry)
    (( count >= 2 )) && published=true
    ;;
  failure)
    ;;
  wrong-source)
    published=true
    revision=ffffffffffffffffffffffffffffffffffffffff
    ;;
  tag-only)
    [[ "$tag" != latest ]] && published=true
    ;;
  localnet)
    [[ "$url" == *"/raofoundation/subtensor-localnet/manifests/v438" ]] \
      && published=true
    ;;
  *)
    echo "unknown mock scenario: $MOCK_SCENARIO" >&2
    exit 2
    ;;
esac

[[ "$published" == true ]] || exit 22
cat <<JSON
{"schemaVersion":2,"annotations":{"org.opencontainers.image.revision":"$revision","org.opencontainers.image.version":"$version"}}
JSON
EOF
chmod +x "$tmp/bin/curl"

cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >> "$MOCK_GH_LOG"

if [[ "$1" == workflow && "$2" == run ]]; then
  count=$(<"$MOCK_DISPATCH_COUNT")
  count=$(( count + 1 ))
  echo "$count" > "$MOCK_DISPATCH_COUNT"
  exit 0
fi

if [[ "$1" != api ]]; then
  echo "unexpected gh command: $*" >&2
  exit 2
fi

endpoint=""
for arg in "$@"; do
  case "$arg" in
    repos/*) endpoint="$arg" ;;
  esac
done

case "$endpoint" in
  */actions/workflows/docker.yml/runs)
    count=$(<"$MOCK_DISPATCH_COUNT")
    case "$MOCK_SCENARIO" in
      retry)
        if (( count == 0 )); then
          echo '{"workflow_runs":[]}'
        elif (( count == 1 )); then
          cat <<JSON
{"workflow_runs":[{"id":101,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"completed","conclusion":"startup_failure"}]}
JSON
        else
          cat <<JSON
{"workflow_runs":[{"id":102,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"completed","conclusion":"success"},{"id":101,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"completed","conclusion":"startup_failure"}]}
JSON
        fi
        ;;
      failure)
        if (( count == 0 )); then
          echo '{"workflow_runs":[]}'
        else
          cat <<JSON
{"workflow_runs":[{"id":201,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"completed","conclusion":"failure"}]}
JSON
        fi
        ;;
      wrong-source)
        if (( count == 0 )); then
          echo '{"workflow_runs":[]}'
        else
          cat <<JSON
{"workflow_runs":[{"id":301,"event":"workflow_dispatch","display_title":"$RUN_TITLE","status":"completed","conclusion":"success"}]}
JSON
        fi
        ;;
      *)
        echo "unexpected workflow run query for scenario: $MOCK_SCENARIO" >&2
        exit 2
        ;;
    esac
    ;;
  */actions/runs/101)
    cat <<JSON
{"id":101,"event":"workflow_dispatch","display_title":"$RUN_TITLE","path":".github/workflows/docker.yml","status":"completed","conclusion":"startup_failure"}
JSON
    ;;
  */actions/runs/102)
    cat <<JSON
{"id":102,"event":"workflow_dispatch","display_title":"$RUN_TITLE","path":".github/workflows/docker.yml","status":"completed","conclusion":"success"}
JSON
    ;;
  */actions/runs/201)
    cat <<JSON
{"id":201,"event":"workflow_dispatch","display_title":"$RUN_TITLE","path":".github/workflows/docker.yml","status":"completed","conclusion":"failure"}
JSON
    ;;
  */actions/runs/301)
    cat <<JSON
{"id":301,"event":"workflow_dispatch","display_title":"$RUN_TITLE","path":".github/workflows/docker.yml","status":"completed","conclusion":"success"}
JSON
    ;;
  *)
    echo "unexpected endpoint: $endpoint" >&2
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
export PUBLICATION_MAX_ATTEMPTS=3
export PUBLICATION_DISCOVERY_POLLS=2
export PUBLICATION_COMPLETION_POLLS=2
export PUBLICATION_REGISTRY_POLLS=2
export PUBLICATION_POLL_INTERVAL_SECONDS=0

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
export MOCK_SCENARIO=retry
retry_log=$("$helper" ensure docker.yml v438 "$EXPECTED_SHA")
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 2 ]]
grep -q 'run 101 failed before jobs started; retrying' <<<"$retry_log"
grep -q 'subtensor:v438 is published' <<<"$retry_log"
grep -Fq -- "--ref main -f tag=v438 -f expected-sha=$EXPECTED_SHA" "$MOCK_GH_LOG"

reset_fixture
export MOCK_SCENARIO=failure
if "$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected a real child workflow failure to be terminal" >&2
  exit 1
fi
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 1 ]]

# Regression: a successful run whose effective input built another commit must
# not satisfy publication, even if its workflow ref and head_sha look correct.
reset_fixture
export MOCK_SCENARIO=wrong-source
if "$helper" ensure docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected wrong image source metadata to be rejected" >&2
  exit 1
fi
[[ "$(<"$MOCK_DISPATCH_COUNT")" == 1 ]]

reset_fixture
export MOCK_SCENARIO=tag-only
if "$helper" status docker.yml v438 "$EXPECTED_SHA" >/dev/null 2>&1; then
  echo "expected a missing node latest tag to require publication" >&2
  exit 1
fi

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
