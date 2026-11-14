#!/usr/bin/env python3
"""Build the core sdist with every workspace path patch included."""

from __future__ import annotations

import json
import re
import subprocess
import tomllib
from pathlib import Path


def sdist_manifest(root: Path) -> str:
    package = Path("sdk/bittensor-core-py")
    text = (root / package / "pyproject.toml").read_text()
    project = tomllib.loads(text)
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    paths = sorted(
        {
            patch["path"].rstrip("/") + "/**/*"
            for source in workspace.get("patch", {}).values()
            for patch in source.values()
            if "path" in patch
        }
    )
    # Maturin's include globs are relative to pyproject.toml and cannot use
    # `..`. Build from the workspace root so unused path patches (which are
    # absent from the binding crate's dependency graph) can be included too.
    includes = ", ".join(
        f'{{ path = {json.dumps(path)}, format = "sdist" }}' for path in paths
    )
    replacements = [
        (r'^manifest-path = "Cargo.toml"$', f'manifest-path = "{package}/Cargo.toml"'),
        (
            r'^readme = "README.md"$',
            f'readme = "{package / project["project"]["readme"]}"',
        ),
        (r'^\[tool\.maturin\]$', f"[tool.maturin]\ninclude = [{includes}]"),
    ]
    for pattern, replacement in replacements:
        text, count = re.subn(pattern, lambda _: replacement, text, flags=re.M)
        if count != 1:
            raise SystemExit(f"Expected one match for {pattern} in core pyproject.toml")
    return text


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    manifest = root / "pyproject.toml"
    text = sdist_manifest(root)
    # Do not overwrite a future workspace-level project configuration.
    with manifest.open("x") as output:
        output.write(text)
    try:
        subprocess.run(
            ["uv", "build", "--sdist", "--out-dir", "dist"], cwd=root, check=True
        )
    finally:
        manifest.unlink()


if __name__ == "__main__":
    main()
