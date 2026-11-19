#!/usr/bin/env python3
"""Keep only the expected stable bittensor files in a dist directory.

The publish job runs this from the pinned tooling commit. It does not execute
anything from the release commit that built the files.
"""

import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit("usage: select-stable-dist.py SDK_VERSION CORE_VERSION DIST")
    sdk, core, dist_arg = sys.argv[1:]
    dist = Path(dist_arg)
    allowed = {
        f"bittensor-{sdk}-py3-none-any.whl",
        f"bittensor-{sdk}.tar.gz",
        f"bittensor_core-{core}.tar.gz",
    }
    kept: list[str] = []
    for path in sorted(dist.iterdir()):
        name = path.name
        core_wheel = name.startswith(f"bittensor_core-{core}-") and name.endswith(".whl")
        if path.is_file() and (name in allowed or core_wheel):
            kept.append(name)
            continue
        path.unlink()
        print(f"removed unexpected file {name}")
    missing = sorted(allowed - set(kept))
    core_wheels = [
        name for name in kept if name.startswith(f"bittensor_core-{core}-") and name.endswith(".whl")
    ]
    if missing or not core_wheels:
        detail = ", ".join(missing) if missing else "no core wheel"
        raise SystemExit(f"distribution is incomplete: {detail}")
    print("publishing", *kept)


if __name__ == "__main__":
    main()
