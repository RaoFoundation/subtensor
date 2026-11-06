#!/usr/bin/env bash
# Install the SDK wheel from dist/ the way users get it, without the
# monorepo's path override, and check that the bittensor-core it resolves
# exports every native helper bittensor.sp_core maps to a minimum core version
# (_CORE_HELPERS_SINCE). sp_core imports without those helpers and only fails
# when one is called, so `import bittensor` alone cannot catch a floor that is
# too low.
#
# Two environments:
#   minimum  bittensor-core pinned to the SDK's declared floor (the rc pin or
#            the stable >= bound), from dist/ or PyPI;
#   upgrade  the newest published core below the floor is installed first,
#            then the SDK wheel, so the resolver has to replace it.
#
# Usage: check-sdk-core-floor.sh <dist dir>
# Run from the repository root after the SDK wheel and the bittensor-core
# wheels are in <dist dir>, with sdk/python/pyproject.toml as built.
set -euo pipefail

dist=$(realpath "${1:?dist directory required}")
manifest=sdk/python/pyproject.toml
sdk_version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$manifest" | head -n 1)
: "${sdk_version:?could not parse the SDK version from $manifest}"
floor=$(sed -n 's/^ *"bittensor-core\(>=\|==\)\([^,"]*\).*/\2/p' "$manifest" | head -n 1)
: "${floor:?could not parse the bittensor-core requirement from $manifest}"
wheel="$dist/bittensor-${sdk_version}-py3-none-any.whl"
test -f "$wheel" || { echo "missing SDK wheel $wheel"; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

check() {
  "$1/bin/python" - <<'EOF'
from importlib.metadata import version

import bittensor_core
from bittensor import sp_core

core = version("bittensor-core")
missing = [
    name
    for name in sp_core._CORE_HELPERS_SINCE
    if getattr(sp_core, name) is not getattr(bittensor_core, name, None)
]
if missing:
    raise SystemExit(f"bittensor-core {core} does not export {', '.join(missing)}")
print(f"bittensor-core {core} exports {', '.join(sp_core._CORE_HELPERS_SINCE)}")
EOF
}

echo "== minimum: bittensor ${sdk_version} with bittensor-core==${floor}"
uv venv --quiet --python 3.14 "$work/minimum"
uv pip install --quiet --python "$work/minimum" --only-binary bittensor-core \
  --find-links "$dist" "$wheel" "bittensor-core==${floor}"
check "$work/minimum"

echo "== upgrade: bittensor ${sdk_version} over bittensor-core<${floor}"
uv venv --quiet --python 3.14 "$work/upgrade"
uv pip install --quiet --python "$work/upgrade" --only-binary bittensor-core \
  "bittensor-core<${floor}"
older=$(uv pip list --python "$work/upgrade" --format freeze | sed -n 's/^bittensor-core==//p')
echo "installed older bittensor-core ${older}"
uv pip install --quiet --python "$work/upgrade" --only-binary bittensor-core \
  --find-links "$dist" "$wheel"
check "$work/upgrade"
