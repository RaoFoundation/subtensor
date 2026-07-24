#!/usr/bin/env python3
"""Safely unpack the GitHub artifact ZIP produced by an untrusted PR run."""

from __future__ import annotations

import argparse
import os
import shutil
import stat
import struct
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional


class ArtifactError(RuntimeError):
    """The GitHub artifact ZIP is unsafe or malformed."""


@dataclass(frozen=True)
class Limits:
    max_zip_bytes: int = 2_200_000_000
    max_entries: int = 3
    max_central_directory_bytes: int = 64 * 1024
    max_total_bytes: int = 2_000_000_128
    max_bundle_bytes: int = 2_000_000_000
    max_control_bytes: int = 32


ALLOWED_FILES = {
    "docs-preview-action.txt",
    "docs-preview-pr-number.txt",
    "docs-preview-sealed.tgz",
}
EOCD_SIGNATURE = b"PK\x05\x06"
EOCD_STRUCT = struct.Struct("<4s4H2LH")
COPY_CHUNK_BYTES = 1024 * 1024


@dataclass(frozen=True)
class ArtifactIntent:
    action: str
    pr_number: str


def _write_all(fd: int, data: bytes) -> None:
    view = memoryview(data)
    while view:
        written = os.write(fd, view)
        if written <= 0:
            raise ArtifactError("short write while extracting artifact")
        view = view[written:]


def _validate_eocd(path: Path, limits: Limits) -> int:
    size = path.stat().st_size
    if size > limits.max_zip_bytes:
        raise ArtifactError(f"artifact ZIP exceeds {limits.max_zip_bytes} bytes")
    tail_size = min(size, 65_535 + EOCD_STRUCT.size)
    with path.open("rb") as handle:
        handle.seek(size - tail_size)
        tail = handle.read()
    offset = tail.rfind(EOCD_SIGNATURE)
    if offset < 0 or len(tail) - offset < EOCD_STRUCT.size:
        raise ArtifactError("artifact ZIP has no valid end-of-central-directory record")
    fields = EOCD_STRUCT.unpack_from(tail, offset)
    (
        _,
        disk_number,
        central_disk,
        disk_entries,
        total_entries,
        central_size,
        central_offset,
        comment_size,
    ) = fields
    if offset + EOCD_STRUCT.size + comment_size != len(tail):
        raise ArtifactError("artifact ZIP has a malformed comment or trailing data")
    if disk_number or central_disk or disk_entries != total_entries:
        raise ArtifactError("multi-disk artifact ZIPs are forbidden")
    if total_entries > limits.max_entries:
        raise ArtifactError(f"artifact ZIP has more than {limits.max_entries} entries")
    if (
        total_entries == 0xFFFF
        or central_size == 0xFFFFFFFF
        or central_offset == 0xFFFFFFFF
    ):
        raise ArtifactError("ZIP64 artifact metadata is forbidden")
    if central_size > limits.max_central_directory_bytes:
        raise ArtifactError("artifact ZIP central directory is too large")
    eocd_file_offset = size - tail_size + offset
    if central_offset + central_size != eocd_file_offset:
        raise ArtifactError("artifact ZIP central-directory bounds are inconsistent")
    return total_entries


def extract_artifact(
    archive: Path,
    destination: Path,
    limits: Optional[Limits] = None,
) -> None:
    limits = limits or Limits()
    archive = archive.resolve()
    destination = destination.resolve()
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        raise ArtifactError(f"destination already exists: {destination}")
    try:
        expected_entries = _validate_eocd(archive, limits)
    except ArtifactError:
        raise
    except Exception as error:
        raise ArtifactError(f"cannot read artifact ZIP: {error}") from error

    staging = Path(
        tempfile.mkdtemp(prefix=f".{destination.name}.", dir=str(destination.parent))
    )
    try:
        try:
            artifact = zipfile.ZipFile(archive, mode="r")
        except (OSError, zipfile.BadZipFile) as error:
            raise ArtifactError(f"cannot open artifact ZIP: {error}") from error
        with artifact:
            infos = artifact.infolist()
            if len(infos) != expected_entries:
                raise ArtifactError("artifact ZIP entry counts disagree")
            names = [info.filename for info in infos]
            if len(names) != len(set(names)):
                raise ArtifactError("artifact ZIP contains duplicate names")
            if not set(names).issubset(ALLOWED_FILES):
                raise ArtifactError("artifact ZIP contains an unexpected path")
            if set(names) not in (
                {"docs-preview-action.txt", "docs-preview-pr-number.txt"},
                ALLOWED_FILES,
            ):
                raise ArtifactError("artifact ZIP has an unexpected file set")

            total_bytes = 0
            for info in infos:
                file_type = (info.external_attr >> 16) & 0o170000
                if info.is_dir() or file_type not in (0, stat.S_IFREG):
                    raise ArtifactError(
                        "artifact ZIP directories, links, and special files are forbidden"
                    )
                if info.flag_bits & 0x1:
                    raise ArtifactError("encrypted artifact ZIP entries are forbidden")
                if info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
                    raise ArtifactError("unsupported artifact ZIP compression")
                limit = (
                    limits.max_bundle_bytes
                    if info.filename == "docs-preview-sealed.tgz"
                    else limits.max_control_bytes
                )
                if info.file_size < 0 or info.file_size > limit:
                    raise ArtifactError(
                        f"artifact entry {info.filename} exceeds {limit} bytes"
                    )
                total_bytes += info.file_size
                if total_bytes > limits.max_total_bytes:
                    raise ArtifactError("artifact ZIP expands beyond its total limit")

                target = staging / info.filename
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
                if hasattr(os, "O_NOFOLLOW"):
                    flags |= os.O_NOFOLLOW
                fd = os.open(target, flags, 0o600)
                copied = 0
                try:
                    with artifact.open(info, mode="r") as source:
                        while True:
                            chunk = source.read(COPY_CHUNK_BYTES)
                            if not chunk:
                                break
                            copied += len(chunk)
                            if copied > limit:
                                raise ArtifactError(
                                    f"artifact entry {info.filename} exceeded its limit"
                                )
                            _write_all(fd, chunk)
                finally:
                    os.close(fd)
                if copied != info.file_size:
                    raise ArtifactError(
                        f"artifact entry {info.filename} size does not match metadata"
                    )
        os.replace(staging, destination)
    except Exception as error:
        shutil.rmtree(staging, ignore_errors=True)
        if isinstance(error, ArtifactError):
            raise
        raise ArtifactError(f"failed to extract artifact ZIP: {error}") from error


def read_intent(directory: Path, expected_pr_number: str) -> ArtifactIntent:
    """Read the already-extracted control files as a strict two-value contract."""

    expected_files = {
        "deploy": ALLOWED_FILES,
        "cleanup": {
            "docs-preview-action.txt",
            "docs-preview-pr-number.txt",
        },
    }
    actual_files = {
        path.relative_to(directory).as_posix()
        for path in directory.rglob("*")
        if path.is_file()
    }

    def one_line(name: str, maximum: int) -> str:
        path = directory / name
        try:
            raw = path.read_bytes()
        except OSError as error:
            raise ArtifactError(f"cannot read artifact control file {name}") from error
        if not raw or len(raw) > maximum or b"\0" in raw:
            raise ArtifactError(f"invalid artifact control file {name}")
        lines = raw.splitlines()
        if len(lines) != 1:
            raise ArtifactError(f"artifact control file {name} must contain one line")
        try:
            value = lines[0].decode("ascii")
        except UnicodeDecodeError as error:
            raise ArtifactError(f"artifact control file {name} is not ASCII") from error
        if not value:
            raise ArtifactError(f"artifact control file {name} is empty")
        return value

    action = one_line("docs-preview-action.txt", 16)
    pr_number = one_line("docs-preview-pr-number.txt", 32)
    if action not in expected_files:
        raise ArtifactError(f"invalid docs-preview action: {action}")
    if not pr_number.isdigit() or pr_number != expected_pr_number:
        raise ArtifactError(
            f"artifact PR {pr_number} does not match workflow PR {expected_pr_number}"
        )
    if actual_files != expected_files[action]:
        raise ArtifactError("artifact contains an unexpected file set")
    return ArtifactIntent(action=action, pr_number=pr_number)


def _parse_args(arguments: Optional[Iterable[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("destination", type=Path)
    return parser.parse_args(arguments)


def main(arguments: Optional[Iterable[str]] = None) -> int:
    args = _parse_args(arguments)
    try:
        extract_artifact(args.archive, args.destination)
    except ArtifactError as error:
        print(f"docs preview artifact rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
