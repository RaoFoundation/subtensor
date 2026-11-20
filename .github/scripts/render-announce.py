#!/usr/bin/env python3
"""Render a Discord announcement from release facts.

The model may supply only the prose section. Versions, hashes, install
commands, and URLs come from this script. Rejected model text is not posted.
"""

from __future__ import annotations

import argparse
import json
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
    parser.add_argument("--human-out", type=Path)
    parser.add_argument("--facts-out", type=Path)
    parser.add_argument("--model-call-json", type=Path)
    parser.add_argument("--dry-run-summary-out", type=Path)
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
    if announcement.model_reason:
        print(f"model summary reason: {announcement.model_reason}", file=sys.stderr)
    sys.stdout.write(announcement.message)
    if arguments.out is not None:
        arguments.out.write_text(announcement.message, encoding="utf-8")
    if arguments.summary_out is not None:
        arguments.summary_out.write_text(announcement.summary + "\n", encoding="utf-8")
    if arguments.human_out is not None:
        arguments.human_out.write_text(announcement.human_summary + "\n", encoding="utf-8")
    if arguments.facts_out is not None:
        arguments.facts_out.write_text(announcement.facts + "\n", encoding="utf-8")
    if arguments.dry_run_summary_out is not None:
        model_call = {"status": "skipped", "text": "", "reason": "model rewrite was not called"}
        if arguments.model_call_json is not None:
            loaded = json.loads(arguments.model_call_json.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                model_call = {
                    "status": str(loaded.get("status") or "error"),
                    "text": str(loaded.get("text") or ""),
                    "reason": str(loaded.get("reason") or ""),
                }
        report = release_notes.format_dry_run_summary(
            message=announcement.message,
            human_summary=announcement.human_summary,
            model_status=model_call["status"],
            model_text=model_call["text"],
            model_reason=model_call["reason"],
        )
        arguments.dry_run_summary_out.write_text(report, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
