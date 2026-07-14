#!/usr/bin/env bash
# One-command independent verification of a proposed runtime upgrade.
#
# For sudo-multisig (triumvirate) signers: rebuilds the runtime from a
# pristine export of the proposal commit, byte-compares it against the
# artifact CI published in the proposal pre-release, and prints the exact
# sign command pinned to YOUR build. Never signs anything itself.
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
# Strict allowlist: a URL that fails this cannot smuggle shell metacharacters
# into anything we print or run below.
printf '%s' "$url" | grep -Eq \
  '^https://github\.com/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+/releases/tag/[A-Za-z0-9._-]+$' \
  || die "unrecognized release URL: $url"
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

commit=$(jq -r '.commit // empty' "$work/manifest.json")
expected_sha=$(jq -r '.wasm_sha256 // empty' "$work/manifest.json" | sed 's/^0x//')
spec=$(jq -r '.spec_version // empty' "$work/manifest.json")
rustc_line=$(jq -r '.srtool_rustc // empty' "$work/manifest.json")
printf '%s' "$commit" | grep -Eq '^[0-9a-f]{40}$' \
  || die "manifest has no valid commit hash — do not sign"
printf '%s' "$expected_sha" | grep -Eq '^[0-9a-f]{64}$' \
  || die "manifest has no valid wasm_sha256 — do not sign"

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
git cat-file -e "${commit}^{commit}" 2>/dev/null \
  || die "proposal commit $commit is not present locally; run: git fetch origin"
ok "HEAD is the proposal commit ${commit}"

# Build from a pristine clone of the proposal commit, never the working
# tree: local modifications, untracked files, and ignored inputs such as a
# repo-level .cargo/ directory cannot influence the build or the verdict.
# A clone (rather than an archive) keeps .git present, matching the CI
# checkout the released artifact was built from.
src="$work/src"
git clone --quiet "$repo_root" "$src"
git -C "$src" checkout --quiet "$commit" \
  || die "could not check out $commit in the verification clone"
ok "created pristine source clone at ${commit}"

# ------------------------------------------------------------------ build ---
pinned_rustc=$(grep -Eo 'RUSTC_VERSION="[0-9.]+"' "$src/scripts/srtool/build-srtool-image.sh" | grep -Eo '[0-9.]+')
if [ -n "$rustc_line" ]; then
  echo "manifest toolchain: ${rustc_line}; local srtool pin: ${pinned_rustc}"
fi

# Always (re)build the builder image from the commit-pinned srtool source
# under a dedicated tag, so a stale or hostile pre-existing `srtool` image
# cannot stand in as the verifier. With a warm docker layer cache this is
# fast; the first run takes ~10 min.
verify_image="srtool-verify"
note "building the pinned srtool builder image as ${verify_image} (cached after first run)"
DOCKER_DEFAULT_PLATFORM=linux/amd64 "$src/scripts/srtool/build-srtool-image.sh" "$verify_image"

note "running the deterministic srtool build (30+ min on Apple Silicon under emulation)"
( cd "$src/runtime" && { [ -L node-subtensor ] || ln -s . node-subtensor; } )
# Fresh cargo home: a signer's ~/.cargo/config.toml could redirect registries
# or patch sources, so the verification build must not see it.
mkdir -p "$work/cargo-home"
docker run --rm --user root --platform=linux/amd64 \
  -e PACKAGE=node-subtensor-runtime \
  -e BUILD_OPTS="--features=metadata-hash" \
  -e PROFILE=production \
  -v "$work/cargo-home":/cargo-home \
  -v "$src":/build \
  "$verify_image" bash -c "git config --global --add safe.directory /build && \
    /srtool/build --app > /build/runtime/node-subtensor/srtool-output.log; \
    code=\$?; [ \$code -ne 0 ] && cat /build/runtime/node-subtensor/srtool-output.log && exit \$code; \
    exit 0"

built_wasm=$(find "$src/runtime" -name 'node_subtensor_runtime.compact.compressed.wasm' -path '*srtool*' | head -n 1)
[ -n "$built_wasm" ] || die "srtool build produced no compact.compressed.wasm"
# $work (and the wasm inside it) is deleted on exit; keep a copy beside the
# repo so the printed sign command references a path that still exists.
local_wasm="$repo_root/verified-${tag}.wasm"
cp "$built_wasm" "$local_wasm"

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
  printf '  btcli upgrade check --url %q --wasm %q\n' "$url" "$local_wasm"
fi

echo
echo "All verifications passed. To sign, pinning the call data to your own build:"
echo
printf '  btcli upgrade sign --url %q --wasm %q -w <your-wallet>\n' "$url" "$local_wasm"
