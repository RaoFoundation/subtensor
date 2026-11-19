#!/usr/bin/env python3
"""Render a Discord announcement from release facts.

The posted summary is the human text. A model summary is ignored. Versions,
hashes, install commands, and URLs come from this script.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import release_notes


def _optional_int(value: str) -> int:
    return int(value)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--kind",
        required=True,
        choices=("runtime-proposed", "runtime-live", "python-stable"),
    )
    parser.add_argument("--spec", type=_optional_int)
    parser.add_argument("--sdk-version", required=True)
    parser.add_argument("--core-version", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--sha256")
    parser.add_argument("--utc", required=True)
    parser.add_argument("--repo", default=release_notes.DEFAULT_REPO)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--notes")
    parser.add_argument("--revision")
    parser.add_argument("--public-path")
    parser.add_argument("--sample", action="store_true")
    parser.add_argument("--allow-missing-notes", action="store_true")
    parser.add_argument("--model-summary-file", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--summary-out", type=Path)
    parser.add_argument("--facts-out", type=Path)
    arguments = parser.parse_args()

    model_summary = None
    if arguments.model_summary_file is not None:
        model_summary = arguments.model_summary_file.read_text(encoding="utf-8")

    try:
        announcement = release_notes.render_announcement(
            kind=arguments.kind,
            spec=arguments.spec,
            sdk=arguments.sdk_version,
            core=arguments.core_version,
            sha=arguments.sha.lower(),
            sha256=None if arguments.sha256 is None else arguments.sha256.lower(),
            utc=arguments.utc,
            repo=arguments.repo,
            root=arguments.root,
            notes_file=arguments.notes,
            revision=arguments.revision,
            public_path=arguments.public_path,
            sample=arguments.sample,
            allow_missing_notes=arguments.allow_missing_notes or arguments.sample,
            model_summary=model_summary,
        )
    except release_notes.NotesError as error:
        print(str(error), file=sys.stderr)
        return 1

    print(f"model summary: {announcement.model_status}", file=sys.stderr)
    sys.stdout.write(announcement.message)
    if arguments.out is not None:
        arguments.out.write_text(announcement.message, encoding="utf-8")
    if arguments.summary_out is not None:
        arguments.summary_out.write_text(announcement.summary + "\n", encoding="utf-8")
    if arguments.facts_out is not None:
        arguments.facts_out.write_text(announcement.facts + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
