#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
publisher="$repo_root/.github/scripts/publish-localnet-manifest.sh"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

image="ghcr.io/raofoundation/subtensor-localnet"
sha="0123456789abcdef0123456789abcdef01234567"
descriptor_dir="$workdir/descriptors"
fake_docker="$workdir/docker"
args_file="$workdir/docker-args"
mkdir -p "$descriptor_dir"

printf '%s@sha256:%064d\n' "$image" 0 > "$descriptor_dir/amd64.txt"
printf '%s@sha256:%064d\n' "$image" 1 > "$descriptor_dir/arm64.txt"

cat > "$fake_docker" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$DOCKER_ARGS_FILE"
SCRIPT
chmod +x "$fake_docker"

assert_case() {
  local publish_latest="$1"
  local argument index
  local -a expected actual

  expected=(
    buildx imagetools create
    --tag "$image:pr-2928"
    --tag "$image:sha-$sha"
  )
  if [[ "$publish_latest" == true ]]; then
    expected+=(--tag "$image:latest")
  fi
  expected+=(
    --annotation "index:org.opencontainers.image.description=Subtensor local development network for CI and local testing"
    --annotation "index:org.opencontainers.image.source=https://github.com/RaoFoundation/subtensor"
    --annotation "index:org.opencontainers.image.licenses=Apache-2.0"
    "$image@sha256:0000000000000000000000000000000000000000000000000000000000000000"
    "$image@sha256:0000000000000000000000000000000000000000000000000000000000000001"
  )

  IMAGE="$image" TAG=pr-2928 SHA="$sha" PUBLISH_LATEST="$publish_latest" \
    PATH="$workdir:$PATH" DOCKER_ARGS_FILE="$args_file" \
    "$publisher" "$descriptor_dir"

  actual=()
  while IFS= read -r argument; do
    actual+=("$argument")
  done < "$args_file"
  [[ "${#actual[@]}" -eq "${#expected[@]}" ]] || {
    echo "unexpected argument count for latest=$publish_latest" >&2
    exit 1
  }
  for index in "${!expected[@]}"; do
    [[ "${actual[$index]}" == "${expected[$index]}" ]] || {
      echo "argument $index mismatch for latest=$publish_latest" >&2
      echo "expected: ${expected[$index]}" >&2
      echo "actual:   ${actual[$index]}" >&2
      exit 1
    }
  done
}

assert_case false
assert_case true

if IMAGE="$image" TAG=pr-2928 SHA="$sha" PUBLISH_LATEST=invalid \
  PATH="$workdir:$PATH" DOCKER_ARGS_FILE="$args_file" \
  "$publisher" "$descriptor_dir" >/dev/null 2>&1; then
  echo "invalid latest policy unexpectedly succeeded" >&2
  exit 1
fi

printf '%s\n' "$image:not-a-digest" > "$descriptor_dir/amd64.txt"
if IMAGE="$image" TAG=pr-2928 SHA="$sha" PUBLISH_LATEST=false \
  PATH="$workdir:$PATH" DOCKER_ARGS_FILE="$args_file" \
  "$publisher" "$descriptor_dir" >/dev/null 2>&1; then
  echo "invalid image descriptor unexpectedly succeeded" >&2
  exit 1
fi

echo "localnet manifest publication contract tests passed"
