#!/usr/bin/env python3
"""Fail closed if a feature-dependent RustSec exclusion becomes reachable."""

import re
import subprocess


def violations(tree: str) -> list[str]:
    failures = set()
    for line in tree.splitlines():
        package = re.match(r"^(\S+) v(\d+)\.(\d+)\.(\d+)\b(.*)$", line)
        if package is None:
            continue
        name, major, minor, patch, features = package.groups()
        version = (int(major), int(minor), int(patch))
        if name == "ruzstd" and (
            version < (0, 9, 1) or (version == (0, 9, 1) and features.startswith("-"))
        ):
            failures.add(
                "RUSTSEC-2026-0344: affected ruzstd is in the active build graph"
            )
        if name == "wasmtime" and "component-model" in re.split(r"[,\s]+", features):
            failures.add(
                "Wasmtime component-model is enabled; reassess RustSec exclusions"
            )
    return sorted(failures)


def main() -> None:
    tree = subprocess.run(
        [
            "cargo",
            "tree",
            "--locked",
            "--workspace",
            "--all-features",
            "--target",
            "all",
            "--prefix",
            "none",
            "--format",
            "{p} {f}",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    if not tree.strip():
        raise SystemExit("Cargo returned an empty dependency graph")
    failures = violations(tree)
    if failures:
        raise SystemExit("\n".join(failures))
    print(
        "RustSec feature exclusions verified against all workspace features and targets"
    )


if __name__ == "__main__":
    main()
