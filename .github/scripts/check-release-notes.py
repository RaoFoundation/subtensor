#!/usr/bin/env python3
"""Fail a release that bumps spec or Python version without notes.

Spec 492, bittensor 11.3.4, and bittensor-core 0.1.11 are grandfathered.
They have no release page. A newer value needs a notes file and a real page.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import release_notes


def _root(value: str) -> Path:
    return Path(value)


def _print_result(result: release_notes.CheckResult) -> int:
    stream = sys.stdout if result.ok else sys.stderr
    print(result.detail, file=stream)
    return 0 if result.ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    runtime = subparsers.add_parser("runtime")
    runtime.add_argument("--spec", type=int, required=True)
    runtime.add_argument("--root", type=_root, default=Path("."))
    runtime.add_argument("--revision")

    python = subparsers.add_parser("python")
    python.add_argument("--sdk", required=True)
    python.add_argument("--core", required=True)
    python.add_argument("--root", type=_root, default=Path("."))
    python.add_argument("--revision")

    diff = subparsers.add_parser("diff")
    diff.add_argument("--base-spec", type=int, required=True)
    diff.add_argument("--head-spec", type=int, required=True)
    diff.add_argument("--base-sdk", required=True)
    diff.add_argument("--head-sdk", required=True)
    diff.add_argument("--base-core", required=True)
    diff.add_argument("--head-core", required=True)
    diff.add_argument("--root", type=_root, default=Path("."))

    tree = subparsers.add_parser("tree")
    tree.add_argument("--root", type=_root, default=Path("."))

    versions = subparsers.add_parser("versions")
    versions.add_argument("--root", type=_root, default=Path("."))

    arguments = parser.parse_args()
    try:
        if arguments.command == "runtime":
            result = release_notes.check_runtime(
                arguments.root, arguments.spec, arguments.revision
            )
            return _print_result(result)
        if arguments.command == "python":
            result = release_notes.check_python(
                arguments.root, arguments.sdk, arguments.core, arguments.revision
            )
            return _print_result(result)
        if arguments.command == "diff":
            result = release_notes.check_diff(
                arguments.root,
                base_spec=arguments.base_spec,
                head_spec=arguments.head_spec,
                base_sdk=arguments.base_sdk,
                head_sdk=arguments.head_sdk,
                base_core=arguments.base_core,
                head_core=arguments.head_core,
            )
            return _print_result(result)
        if arguments.command == "tree":
            return _print_result(release_notes.check_tree(arguments.root))
        if arguments.command == "versions":
            print(
                json.dumps(
                    release_notes.versions_from_root(arguments.root),
                    sort_keys=True,
                )
            )
            return 0
    except release_notes.NotesError as error:
        print(str(error), file=sys.stderr)
        return 1
    print(f"unknown command {arguments.command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
