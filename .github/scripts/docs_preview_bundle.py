#!/usr/bin/env python3
"""Validate and extract an untrusted docs-preview bundle.

The archive is produced by pull-request code and consumed by a workflow that
later receives deployment credentials.  Treat every tar header and every path
inside Vercel's Build Output as attacker controlled.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import shutil
import stat
import tarfile
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional


class BundleError(RuntimeError):
    """The bundle is unsafe or malformed."""


@dataclass(frozen=True)
class Limits:
    max_archive_bytes: int = 2 * 1024 * 1024 * 1024
    max_members: int = 400_000
    max_total_bytes: int = 4 * 1024 * 1024 * 1024
    max_file_bytes: int = 512 * 1024 * 1024
    max_path_bytes: int = 4_096
    max_metadata_member_bytes: int = 1024 * 1024
    max_metadata_total_bytes: int = 32 * 1024 * 1024
    max_json_bytes: int = 5 * 1024 * 1024
    max_file_path_references: int = 400_000


ALLOWED_ROOTS = (".vercel/output",)
COPY_CHUNK_BYTES = 1024 * 1024
TAR_BLOCK_BYTES = 512


def _write_all(fd: int, data: bytes) -> None:
    view = memoryview(data)
    while view:
        written = os.write(fd, view)
        if written <= 0:
            raise BundleError("short write while extracting bundle")
        view = view[written:]


def _normalise_member_name(raw_name: str, max_path_bytes: int) -> str:
    if not raw_name or "\x00" in raw_name or "\\" in raw_name:
        raise BundleError(f"unsafe archive path: {raw_name!r}")
    if len(raw_name.encode("utf-8", errors="surrogateescape")) > max_path_bytes:
        raise BundleError(f"archive path is too long: {raw_name!r}")

    name = raw_name
    while name.startswith("./"):
        name = name[2:]
    if name.endswith("/"):
        name = name[:-1]
    if not name or name.startswith("/"):
        raise BundleError(f"unsafe archive path: {raw_name!r}")

    parts = name.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise BundleError(f"unsafe archive path: {raw_name!r}")
    if any(len(part.encode("utf-8", errors="surrogateescape")) > 255 for part in parts):
        raise BundleError(f"archive path component is too long: {raw_name!r}")
    if not any(name == root or name.startswith(root + "/") for root in ALLOWED_ROOTS):
        raise BundleError(f"unexpected archive path: {raw_name!r}")
    return name


def _normalise_relative_path(raw_name: object, label: str, max_path_bytes: int) -> str:
    if not isinstance(raw_name, str) or not raw_name:
        raise BundleError(f"{label} must be a non-empty string")
    if "\x00" in raw_name or "\\" in raw_name or raw_name.startswith("/"):
        raise BundleError(f"unsafe {label}: {raw_name!r}")
    if len(raw_name.encode("utf-8", errors="surrogateescape")) > max_path_bytes:
        raise BundleError(f"{label} is too long: {raw_name!r}")
    parts = raw_name.split("/")
    if any(part in ("", ".", "..") for part in parts):
        raise BundleError(f"unsafe {label}: {raw_name!r}")
    return "/".join(parts)


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def _tar_octal(field: bytes, label: str) -> int:
    # Base-256 numeric fields are unnecessary within our quotas and have caused
    # parser differentials in tar implementations. The producer emits octal.
    if field and field[0] & 0x80:
        raise BundleError(f"base-256 {label} fields are forbidden")
    value = field.rstrip(b"\0 ").lstrip(b" ")
    if not value:
        return 0
    if any(byte < ord("0") or byte > ord("7") for byte in value):
        raise BundleError(f"invalid tar {label} field")
    return int(value, 8)


def _read_exact(handle: object, size: int, label: str) -> bytes:
    chunks = []
    remaining = size
    while remaining:
        chunk = handle.read(min(COPY_CHUNK_BYTES, remaining))
        if not chunk:
            raise BundleError(f"truncated tar {label}")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def _skip_exact(handle: object, size: int, label: str) -> None:
    remaining = size
    while remaining:
        chunk = handle.read(min(COPY_CHUNK_BYTES, remaining))
        if not chunk:
            raise BundleError(f"truncated tar {label}")
        remaining -= len(chunk)


def _parse_pax(payload: bytes, limits: Limits) -> None:
    offset = 0
    keys = set()
    while offset < len(payload):
        separator = payload.find(b" ", offset)
        if separator < 0:
            raise BundleError("invalid PAX record length")
        length_bytes = payload[offset:separator]
        if not length_bytes.isdigit():
            raise BundleError("invalid PAX record length")
        length = int(length_bytes)
        end = offset + length
        if length <= separator - offset + 2 or end > len(payload):
            raise BundleError("invalid PAX record boundary")
        record = payload[separator + 1 : end]
        if not record.endswith(b"\n") or b"=" not in record:
            raise BundleError("invalid PAX record")
        key_bytes, value = record[:-1].split(b"=", 1)
        try:
            key = key_bytes.decode("utf-8")
        except UnicodeDecodeError as error:
            raise BundleError("invalid PAX key encoding") from error
        if not key or key in keys:
            raise BundleError("empty or duplicate PAX key")
        keys.add(key)
        if key == "size" or key == "linkpath" or key.startswith("GNU.sparse."):
            raise BundleError(f"unsafe PAX override: {key}")
        if key == "path":
            try:
                path = value.decode("utf-8")
            except UnicodeDecodeError as error:
                raise BundleError("invalid PAX path encoding") from error
            _normalise_member_name(path, limits.max_path_bytes)
        elif key in {"mtime", "atime", "ctime"}:
            if (
                len(value) > 64
                or re.fullmatch(rb"-?[0-9]+(?:\.[0-9]+)?", value) is None
            ):
                raise BundleError(f"invalid PAX timestamp: {key}")
        else:
            raise BundleError(f"unsupported PAX key: {key}")
        offset = end
    if offset != len(payload):
        raise BundleError("invalid PAX payload")


def _preflight_tar(archive: Path, limits: Limits) -> None:
    """Bound tar metadata before tarfile is allowed to interpret it."""

    raw_members = 0
    content_bytes = 0
    metadata_bytes = 0
    decompressed_bytes = 0
    zero_blocks = 0
    allowed_types = {b"\0", b"0", b"5", b"x", b"L"}
    try:
        compressed = archive.open("rb")
        stream = gzip.GzipFile(fileobj=compressed, mode="rb")
    except OSError as error:
        raise BundleError(f"cannot open gzip archive {archive}: {error}") from error

    with compressed, stream:
        while True:
            header = stream.read(TAR_BLOCK_BYTES)
            decompressed_bytes += len(header)
            if not header:
                break
            if len(header) != TAR_BLOCK_BYTES:
                raise BundleError("truncated tar header")
            if header == b"\0" * TAR_BLOCK_BYTES:
                zero_blocks += 1
                if zero_blocks == 2:
                    break
                continue
            if zero_blocks:
                raise BundleError("tar archive has only one end-of-archive block")

            raw_members += 1
            if raw_members > limits.max_members:
                raise BundleError(f"archive has more than {limits.max_members} headers")
            expected_checksum = _tar_octal(header[148:156], "checksum")
            checksum_header = header[:148] + (b" " * 8) + header[156:]
            if sum(checksum_header) != expected_checksum:
                raise BundleError("invalid tar header checksum")

            size = _tar_octal(header[124:136], "size")
            type_flag = header[156:157]
            if type_flag not in allowed_types:
                raise BundleError("tar links and special entry types are forbidden")
            if type_flag in (b"x", b"L"):
                if size > limits.max_metadata_member_bytes:
                    raise BundleError("tar metadata member is too large")
                metadata_bytes += size
                if metadata_bytes > limits.max_metadata_total_bytes:
                    raise BundleError("tar metadata exceeds its total limit")
                payload = _read_exact(stream, size, "metadata")
                decompressed_bytes += size
                if type_flag == b"x":
                    _parse_pax(payload, limits)
                else:
                    try:
                        long_name = payload.rstrip(b"\0\n").decode("utf-8")
                    except UnicodeDecodeError as error:
                        raise BundleError("invalid GNU long-name encoding") from error
                    _normalise_member_name(long_name, limits.max_path_bytes)
            else:
                if type_flag in (b"\0", b"0"):
                    if size > limits.max_file_bytes:
                        raise BundleError(
                            f"tar member exceeds {limits.max_file_bytes} bytes"
                        )
                    content_bytes += size
                    if content_bytes > limits.max_total_bytes:
                        raise BundleError(
                            f"tar content exceeds {limits.max_total_bytes} bytes"
                        )
                elif size:
                    raise BundleError("tar directory has a non-zero size")
                _skip_exact(stream, size, "member data")
                decompressed_bytes += size

            padding = (-size) % TAR_BLOCK_BYTES
            if padding:
                _skip_exact(stream, padding, "member padding")
                decompressed_bytes += padding
            maximum_tar_bytes = (
                limits.max_total_bytes
                + limits.max_metadata_total_bytes
                + limits.max_members * TAR_BLOCK_BYTES * 2
                + TAR_BLOCK_BYTES * 2
            )
            if decompressed_bytes > maximum_tar_bytes:
                raise BundleError("decompressed tar exceeds its structural limit")
        if zero_blocks != 2:
            raise BundleError("tar archive is missing its end-of-archive blocks")

        while True:
            trailing = stream.read(COPY_CHUNK_BYTES)
            if not trailing:
                break
            decompressed_bytes += len(trailing)
            if any(trailing):
                raise BundleError("tar archive has non-zero trailing data")
            if decompressed_bytes > (
                limits.max_total_bytes
                + limits.max_metadata_total_bytes
                + limits.max_members * TAR_BLOCK_BYTES * 2
                + TAR_BLOCK_BYTES * 20
            ):
                raise BundleError("tar archive has excessive trailing padding")


def _reject_json_constant(value: str) -> None:
    raise ValueError(f"non-standard JSON constant {value}")


def _load_json(path: Path, limits: Limits) -> object:
    size = path.stat().st_size
    if size > limits.max_json_bytes:
        raise BundleError(f"JSON metadata is too large: {path} ({size} bytes)")
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle, parse_constant=_reject_json_constant)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        raise BundleError(f"invalid JSON metadata in {path}: {error}") from error


def _file_path_map(
    config_path: Path,
    config: dict,
    root: Path,
    limits: Limits,
) -> dict[str, Path]:
    raw_map = config.get("filePathMap")
    if raw_map is None:
        return {}
    if not isinstance(raw_map, dict):
        raise BundleError(f"filePathMap is not an object: {config_path}")
    if len(raw_map) > limits.max_file_path_references:
        raise BundleError(f"filePathMap has too many entries: {config_path}")

    resolved_root = root.resolve()
    entries: dict[str, Path] = {}
    for raw_bundle_path, raw_source_path in raw_map.items():
        bundle_path = _normalise_relative_path(
            raw_bundle_path,
            f"filePathMap bundle path in {config_path}",
            limits.max_path_bytes,
        )
        source_path = _normalise_relative_path(
            raw_source_path,
            f"filePathMap source path in {config_path}",
            limits.max_path_bytes,
        )
        resolved_source = (resolved_root / source_path).resolve()
        if not _is_within(resolved_source, resolved_root):
            raise BundleError(f"filePathMap escapes the deployment root: {config_path}")
        if not resolved_source.is_file():
            raise BundleError(
                f"filePathMap references a missing regular file: {resolved_source}"
            )
        entries[bundle_path] = resolved_source
    return entries


def _validate_vercel_paths(root: Path, limits: Limits) -> None:
    output = root / ".vercel" / "output"
    if not output.is_dir():
        raise BundleError("bundle is missing .vercel/output")

    configs = sorted(output.rglob(".vc-config.json"))
    if not configs:
        raise BundleError("bundle has no Vercel function configuration")

    reference_count = 0
    for config_path in configs:
        config = _load_json(config_path, limits)
        if not isinstance(config, dict):
            raise BundleError(f"function configuration is not an object: {config_path}")
        file_path_map = _file_path_map(config_path, config, root, limits)
        reference_count += len(file_path_map)
        if reference_count > limits.max_file_path_references:
            raise BundleError("too many Vercel filePathMap references")

        handler = config.get("handler")
        if handler is None:
            continue
        handler = _normalise_relative_path(
            handler,
            f"function handler in {config_path}",
            limits.max_path_bytes,
        )
        physical_handler = config_path.parent / handler
        if handler not in file_path_map and not physical_handler.is_file():
            raise BundleError(f"function handler does not exist: {config_path}")


def seal_bundle(
    source_root: Path,
    archive: Path,
    limits: Optional[Limits] = None,
) -> None:
    """Materialize Vercel file references inside Build Output and seal only it."""

    limits = limits or Limits()
    source_root = source_root.resolve()
    archive = archive.resolve()
    output = source_root / ".vercel" / "output"
    if not output.is_dir():
        raise BundleError("build is missing .vercel/output")
    internal = output / ".docs-preview-files"
    internal.mkdir(parents=True, exist_ok=True)

    configs = sorted(output.rglob(".vc-config.json"))
    if not configs:
        raise BundleError("build has no Vercel function configuration")
    reference_count = 0
    for config_path in configs:
        config = _load_json(config_path, limits)
        if not isinstance(config, dict):
            raise BundleError(f"function configuration is not an object: {config_path}")
        entries = _file_path_map(config_path, config, source_root, limits)
        reference_count += len(entries)
        if reference_count > limits.max_file_path_references:
            raise BundleError("too many Vercel filePathMap references")
        if not entries:
            continue

        rewritten = {}
        for bundle_path, source_path in entries.items():
            source_relative = source_path.relative_to(source_root).as_posix()
            if _is_within(source_path, output):
                materialized_relative = source_relative
            else:
                digest = hashlib.sha256(source_relative.encode("utf-8")).hexdigest()
                materialized = internal / digest
                if not materialized.exists():
                    shutil.copy2(source_path, materialized, follow_symlinks=True)
                materialized_relative = materialized.relative_to(source_root).as_posix()
            rewritten[bundle_path] = materialized_relative

        config["filePathMap"] = rewritten
        temporary = config_path.with_name(config_path.name + ".tmp")
        temporary.write_text(
            json.dumps(config, sort_keys=True, separators=(",", ":")),
            encoding="utf-8",
        )
        os.replace(temporary, config_path)

    _validate_vercel_paths(source_root, limits)
    archive.parent.mkdir(parents=True, exist_ok=True)
    if archive.exists():
        raise BundleError(f"archive already exists: {archive}")
    try:
        with tarfile.open(archive, mode="w:gz", dereference=True) as bundle:
            bundle.add(output, arcname=".vercel/output", recursive=True)
    except (OSError, tarfile.TarError) as error:
        archive.unlink(missing_ok=True)
        raise BundleError(f"failed to seal bundle: {error}") from error


def _copy_member(source: object, destination_fd: int, expected_size: int) -> None:
    remaining = expected_size
    while remaining:
        chunk = source.read(min(COPY_CHUNK_BYTES, remaining))
        if not chunk:
            raise BundleError("archive member ended before its declared size")
        _write_all(destination_fd, chunk)
        remaining -= len(chunk)
    if source.read(1):
        raise BundleError("archive member exceeds its declared size")


def extract_bundle(
    archive: Path,
    destination: Path,
    limits: Optional[Limits] = None,
) -> None:
    """Stream-validate *archive* into a staging directory and atomically publish it."""

    limits = limits or Limits()
    archive = archive.resolve()
    destination = destination.resolve()
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        raise BundleError(f"destination already exists: {destination}")
    try:
        archive_size = archive.stat().st_size
    except OSError as error:
        raise BundleError(f"cannot stat archive {archive}: {error}") from error
    if archive_size > limits.max_archive_bytes:
        raise BundleError(
            f"archive is too large: {archive_size} > {limits.max_archive_bytes} bytes"
        )
    try:
        _preflight_tar(archive, limits)
    except BundleError:
        raise
    except Exception as error:
        raise BundleError(f"failed tar preflight: {error}") from error

    staging = Path(
        tempfile.mkdtemp(prefix=f".{destination.name}.", dir=str(destination.parent))
    )
    seen = set()
    member_count = 0
    total_bytes = 0
    try:
        try:
            tar_handle = tarfile.open(archive, mode="r|gz")
        except (OSError, tarfile.TarError) as error:
            raise BundleError(f"cannot open archive {archive}: {error}") from error

        with tar_handle:
            for member in tar_handle:
                member_count += 1
                if member_count > limits.max_members:
                    raise BundleError(
                        f"archive has more than {limits.max_members} members"
                    )
                name = _normalise_member_name(member.name, limits.max_path_bytes)
                if name in seen:
                    raise BundleError(f"duplicate archive member: {name}")
                seen.add(name)

                if not (member.isdir() or member.isreg()):
                    raise BundleError(f"links and special files are forbidden: {name}")
                size = member.size if member.isreg() else 0
                if size < 0 or size > limits.max_file_bytes:
                    raise BundleError(
                        f"archive member is too large: {name} ({size} bytes)"
                    )
                total_bytes += size
                if total_bytes > limits.max_total_bytes:
                    raise BundleError(
                        f"archive expands beyond {limits.max_total_bytes} bytes"
                    )

                target = staging.joinpath(*name.split("/"))
                if not _is_within(target.resolve(), staging.resolve()):
                    raise BundleError(f"archive path escapes extraction root: {name}")
                if member.isdir():
                    if target.exists() and not target.is_dir():
                        raise BundleError(
                            f"archive directory conflicts with a file: {name}"
                        )
                    target.mkdir(parents=True, exist_ok=True)
                    continue

                target.parent.mkdir(parents=True, exist_ok=True)
                extracted = tar_handle.extractfile(member)
                if extracted is None:
                    raise BundleError(f"cannot read archive member: {name}")
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
                if hasattr(os, "O_NOFOLLOW"):
                    flags |= os.O_NOFOLLOW
                mode = 0o755 if member.mode & stat.S_IXUSR else 0o644
                try:
                    fd = os.open(target, flags, mode)
                except OSError as error:
                    raise BundleError(
                        f"cannot create extracted file {name}: {error}"
                    ) from error
                try:
                    _copy_member(extracted, fd, size)
                finally:
                    os.close(fd)

        if member_count == 0:
            raise BundleError("archive is empty")
        _validate_vercel_paths(staging, limits)
        os.replace(staging, destination)
    except Exception as error:
        shutil.rmtree(staging, ignore_errors=True)
        if isinstance(error, BundleError):
            raise
        raise BundleError(f"failed to extract bundle: {error}") from error


def _parse_args(arguments: Optional[Iterable[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    extract = commands.add_parser(
        "extract", help="validate and extract a sealed bundle"
    )
    extract.add_argument("archive", type=Path)
    extract.add_argument("destination", type=Path)
    seal = commands.add_parser(
        "seal",
        help="materialize Vercel file references and create a self-contained bundle",
    )
    seal.add_argument("source_root", type=Path)
    seal.add_argument("archive", type=Path)
    return parser.parse_args(arguments)


def main(arguments: Optional[Iterable[str]] = None) -> int:
    args = _parse_args(arguments)
    try:
        if args.command == "extract":
            extract_bundle(args.archive, args.destination)
        elif args.command == "seal":
            seal_bundle(args.source_root, args.archive)
        else:
            raise AssertionError(args.command)
    except BundleError as error:
        print(f"docs preview bundle rejected: {error}", file=os.sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
