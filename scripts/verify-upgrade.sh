#!/usr/bin/env bash
# One-command independent verification of a proposed runtime upgrade.
#
# For sudo-multisig (triumvirate) signers: rebuilds the runtime from the
# source you have checked out, byte-compares it against the artifact CI
# published in the proposal pre-release, and prints the exact sign command
# pinned to YOUR build. Never signs anything itself.
#
# Usage:
#   git clone <repo> && cd subtensor && git checkout v<spec>
#   ./scripts/verify-upgrade.sh                  # infers release from the tag
#   ./scripts/verify-upgrade.sh --url <release>  # explicit release URL
#
# Requires: docker, curl, jq, git. Uses btcli for the final chain check if
# it is installed; prints the command to run manually otherwise.

set -euo pipefail

RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RESET=$'\033[0m'
die()  { echo "${RED}error:${RESET} $*" >&2; exit 1; }
ok()   { echo "${GREEN}ok:${RESET} $*"; }
note() { echo "${YELLOW}note:${RESET} $*"; }

url=""
while [ $# -gt 0 ]; do
  case "$1" in
    --url) url="${2:?--url needs a value}"; shift 2 ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument: $1 (only --url <release-url> is accepted)" ;;
  esac
done

for cmd in docker curl jq git; do
  command -v "$cmd" >/dev/null || die "$cmd is required"
done
docker info >/dev/null 2>&1 || die "docker daemon is not running"

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

# ---------------------------------------------------------------- release ---
if [ -z "$url" ]; then
  tag=$(git describe --tags --exact-match 2>/dev/null) \
    || die "HEAD is not at a release tag; check out the proposal tag (git checkout v<spec>) or pass --url"
  origin=$(git remote get-url origin)
  slug=$(echo "$origin" | sed -E 's#(git@github.com:|https://github.com/)##; s#\.git$##')
  url="https://github.com/${slug}/releases/tag/${tag}"
fi
case "$url" in
  https://github.com/*/releases/tag/*) ;;
  *) die "unrecognized release URL: $url" ;;
esac
slug=$(echo "$url" | sed -E 's#https://github.com/([^/]+/[^/]+)/releases/tag/.*#\1#')
tag=${url##*/}
dl="https://github.com/${slug}/releases/download/${tag}"

work=$(mktemp -d /tmp/verify-upgrade.XXXXXX)
trap 'rm -rf "$work"' EXIT

echo "Verifying proposal ${tag} from ${url}"
curl -fsSL "${dl}/upgrade-manifest.json" -o "$work/manifest.json" \
  || die "could not download upgrade-manifest.json from the release"
curl -fsSL "${dl}/subtensor.wasm" -o "$work/release.wasm" \
  || die "could not download subtensor.wasm from the release"

commit=$(jq -r '.commit' "$work/manifest.json")
expected_sha=$(jq -r '.wasm_sha256' "$work/manifest.json" | sed 's/^0x//')
spec=$(jq -r '.spec_version' "$work/manifest.json")
rustc_line=$(jq -r '.srtool_rustc // empty' "$work/manifest.json")

# The release wasm must itself match the manifest before we compare anything
# against it.
release_sha=$(shasum -a 256 "$work/release.wasm" | cut -d' ' -f1)
[ "$release_sha" = "$expected_sha" ] \
  || die "release asset subtensor.wasm (sha256 $release_sha) does not match manifest wasm_sha256 ($expected_sha) — do not sign"
ok "release wasm matches manifest sha256"

# ----------------------------------------------------------------- source ---
head_commit=$(git rev-parse HEAD)
[ "$head_commit" = "$commit" ] \
  || die "HEAD ($head_commit) is not the proposal commit ($commit); run: git fetch origin && git checkout $tag"
git diff --quiet HEAD -- runtime pallets Cargo.toml Cargo.lock \
  || die "working tree has local modifications under runtime/, pallets/, or the cargo manifests; verify from a clean checkout"
ok "HEAD is the proposal commit ${commit}"

# ------------------------------------------------------------------ build ---
pinned_rustc=$(grep -Eo 'RUSTC_VERSION="[0-9.]+"' scripts/srtool/build-srtool-image.sh | grep -Eo '[0-9.]+')
if [ -n "$rustc_line" ]; then
  echo "manifest toolchain: ${rustc_line}; local srtool pin: ${pinned_rustc}"
fi
if ! docker image inspect srtool >/dev/null 2>&1; then
  note "srtool image not found locally; building it (one-time, ~10 min)"
  DOCKER_DEFAULT_PLATFORM=linux/amd64 ./scripts/srtool/build-srtool-image.sh
fi

note "running the deterministic srtool build (30+ min on Apple Silicon under emulation)"
( cd runtime && { [ -L node-subtensor ] || ln -s . node-subtensor; } )
docker run --rm --user root --platform=linux/amd64 \
  -e PACKAGE=node-subtensor-runtime \
  -e BUILD_OPTS="--features=metadata-hash" \
  -e PROFILE=production \
  -v "$HOME/.cargo":/cargo-home \
  -v "$repo_root":/build \
  srtool bash -c "git config --global --add safe.directory /build && \
    /srtool/build --app > /build/runtime/node-subtensor/srtool-output.log; \
    code=\$?; [ \$code -ne 0 ] && cat /build/runtime/node-subtensor/srtool-output.log && exit \$code; \
    exit 0"

local_wasm=$(find runtime -name 'node_subtensor_runtime.compact.compressed.wasm' -path '*srtool*' | head -n 1)
[ -n "$local_wasm" ] || die "srtool build produced no compact.compressed.wasm"

# ---------------------------------------------------------------- compare ---
local_sha=$(shasum -a 256 "$local_wasm" | cut -d' ' -f1)
echo "your build:  sha256 $local_sha"
echo "release:     sha256 $release_sha"
if [ "$local_sha" != "$release_sha" ]; then
  die "your srtool build does NOT match the released runtime — do not sign; check toolchain pin and report the mismatch"
fi
cmp -s "$local_wasm" "$work/release.wasm" \
  || die "sha256 matched but bytes differ (should be impossible) — do not sign"
ok "your build is byte-identical to the released runtime (spec ${spec})"

# ------------------------------------------------------------ chain check ---
if command -v btcli >/dev/null; then
  echo
  echo "Running btcli upgrade check against the chain..."
  btcli upgrade check --url "$url" --wasm "$local_wasm"
else
  note "btcli not found; to also verify call data and on-chain state, install it and run:"
  echo "  btcli upgrade check --url $url --wasm $local_wasm"
fi

echo
echo "All verifications passed. To sign, pinning the call data to your own build:"
echo
echo "  btcli upgrade sign --url $url --wasm $local_wasm -w <your-wallet>"
