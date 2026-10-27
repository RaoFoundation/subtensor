#!/usr/bin/env python3
"""Stamp the SDK's committed development version for a stable PyPI build."""

from __future__ import annotations

import re
from pathlib import Path


def main() -> None:
    root = Path("sdk/python")
    manifest = root / "pyproject.toml"
    text = manifest.read_text()
    match = re.search(r'^version = "([0-9]+\.[0-9]+\.[0-9]+)\.dev0"$', text, flags=re.M)
    if match is None:
        raise SystemExit("SDK manifest must declare version X.Y.Z.dev0")
    version = match.group(1)
    text = text[: match.start()] + f'version = "{version}"' + text[match.end() :]

    # These are monorepo-only development inputs. They must not appear in an
    # extracted sdist, where the sibling path and development lock do not exist.
    text, count = re.subn(r"\n\[tool\.uv\.sources\]\n.*?(?=\n\[|\Z)", "\n", text, flags=re.S)
    if count != 1:
        raise SystemExit("tool.uv.sources table not found")
    manifest.write_text(text)
    (root / "uv.lock").unlink(missing_ok=True)


if __name__ == "__main__":
    main()
