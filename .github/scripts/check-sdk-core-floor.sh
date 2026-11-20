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
# A declared floor that was never published (its stable release was skipped)
# cannot be installed by anyone, so the real minimum is the lowest version at
# or above it that exists in dist/ or on PyPI.
minimum=$(python3 - "$floor" "$dist" <<'EOF'
import json
import re
import sys
import urllib.request
from pathlib import Path

floor, dist = sys.argv[1], Path(sys.argv[2])
VERSION = re.compile(r"(\d+)\.(\d+)\.(\d+)(?:rc(\d+))?")


def key(version):
    match = VERSION.fullmatch(version)
    if match is None:
        return None
    major, minor, patch, rc = match.groups()
    return (int(major), int(minor), int(patch), int(rc) if rc else float("inf"))


available = {
    wheel.name.split("-")[1]
    for wheel in dist.glob("bittensor_core-*.whl")
}
if floor in available:
    print(floor)
    raise SystemExit(0)
# Only PyPI can say whether an older supported version exists, so fail
# closed instead of testing a newer local wheel when it cannot answer.
try:
    with urllib.request.urlopen("https://pypi.org/pypi/bittensor-core/json", timeout=30) as response:
        releases = json.load(response)["releases"]
except Exception as error:
    raise SystemExit(f"could not read PyPI releases to resolve bittensor-core>={floor}: {error}")
available |= {
    version
    for version, files in releases.items()
    if any(not file.get("yanked") for file in files)
}

floor_key = key(floor)
allow_rc = floor_key is not None and floor_key[3] != float("inf")
candidates = sorted(
    (key(version), version)
    for version in available
    if key(version) is not None
    and key(version) >= floor_key
    and (allow_rc or key(version)[3] == float("inf"))
) if floor_key is not None else []
print(candidates[0][1] if candidates and floor not in available else floor)
EOF
)
if [ "$minimum" != "$floor" ]; then
  echo "bittensor-core ${floor} was never published; checking the lowest installable version ${minimum}"
fi

uv venv --quiet --python 3.14 "$work/minimum"
uv pip install --quiet --python "$work/minimum" --only-binary bittensor-core \
  --find-links "$dist" "$wheel" "bittensor-core==${minimum}"
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
