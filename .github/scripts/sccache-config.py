#!/usr/bin/env python3
"""Validate and materialize the typed sccache configuration boundary."""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path


TRUSTED_R2 = {
    "bucket": "subtensor-ci-sccache",
    "endpoint": "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
    "region": "auto",
    "s3_use_ssl": True,
    "key_prefix": "subtensor/v1",
}
LOCAL_ENDPOINT = "http://192.168.128.1:8092"
LOCAL_FIELDS = {"endpoint", "key_prefix", "username", "password"}
R2_STRING_FIELDS = (
    "bucket",
    "endpoint",
    "region",
    "key_prefix",
    "access_key_id",
    "secret_access_key",
)


class ConfigError(Exception):
    """A configuration error safe to report without credential material."""


def load_object(path: Path) -> dict[str, object]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle)
    except (OSError, UnicodeError, json.JSONDecodeError):
        raise ConfigError("configuration is not valid JSON") from None
    if not isinstance(value, dict):
        raise ConfigError("configuration is not an object")
    return value


def atomic_write(path: Path, value: dict[str, object]) -> None:
    temporary = path.with_name(path.name + ".normalized")
    try:
        with temporary.open("w", encoding="utf-8") as handle:
            json.dump(value, handle, separators=(",", ":"))
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def required_string(values: dict[str, object], key: str) -> str:
    value = values.get(key)
    if not isinstance(value, str) or not value or "\n" in value or "\r" in value:
        raise ConfigError(f"invalid {key}")
    return value


def string_value(values: dict[str, object], key: str) -> str:
    value = values.get(key)
    if not isinstance(value, str) or "\n" in value or "\r" in value:
        raise ConfigError(f"invalid {key}")
    return value


def validate_r2(values: dict[str, object], mode: str) -> None:
    if mode not in {"reader", "writer"}:
        raise ConfigError("invalid R2 mode")
    for key, expected in TRUSTED_R2.items():
        if values.get(key) != expected:
            raise ConfigError(f"invalid {key}")
    expected_rw_mode = "READ_ONLY" if mode == "reader" else "READ_WRITE"
    if values.get("s3_rw_mode") != expected_rw_mode:
        raise ConfigError("invalid s3_rw_mode")
    for key in ("access_key_id", "secret_access_key"):
        required_string(values, key)


def validate_local(
    value: object,
    *,
    credential_source: dict[str, object] | None = None,
) -> dict[str, str]:
    if not isinstance(value, dict) or set(value) != LOCAL_FIELDS:
        raise ConfigError("invalid local cache contract")
    local = {key: string_value(value, key) for key in LOCAL_FIELDS}
    if not local["endpoint"] or not local["username"] or not local["password"]:
        raise ConfigError("invalid local cache contract")
    if local["endpoint"] != LOCAL_ENDPOINT or local["key_prefix"] != "":
        raise ConfigError("invalid local cache endpoint")
    if credential_source is not None and (
        local["username"] != required_string(credential_source, "access_key_id")
        or local["password"] != required_string(credential_source, "secret_access_key")
    ):
        raise ConfigError("invalid local cache credential")
    return local


def normalize_reader(path: Path, local_mode: str) -> None:
    if local_mode not in {"auto", "disabled"}:
        raise ConfigError("invalid local tier mode")
    values = load_object(path)
    validate_r2(values, "reader")
    if local_mode == "disabled":
        values.pop("local", None)
    elif values.get("local") is not None:
        values["local"] = validate_local(values["local"], credential_source=values)
    values["mode"] = "reader"
    atomic_write(path, values)


def write_writer(path: Path) -> None:
    # Auto mode may stage a fully validated MMDS reader contract first. Keep
    # only its host-local sub-contract: the protected credential below remains
    # the sole authority for the R2 writer backend.
    local: dict[str, str] | None = None
    if path.is_file():
        existing = load_object(path)
        existing_mode = existing.get("mode")
        if existing_mode == "reader":
            validate_r2(existing, "reader")
            if existing.get("local") is not None:
                local = validate_local(existing["local"], credential_source=existing)
        elif existing_mode != "gha":
            raise ConfigError("invalid staged writer configuration")

    access_key = os.environ.get("AWS_ACCESS_KEY_ID", "")
    secret_key = os.environ.get("AWS_SECRET_ACCESS_KEY", "")
    values: dict[str, object] = {
        "mode": "writer",
        **TRUSTED_R2,
        "s3_rw_mode": "READ_WRITE",
        "access_key_id": access_key,
        "secret_access_key": secret_key,
    }
    if local is not None:
        values["local"] = local
    validate_r2(values, "writer")
    atomic_write(path, values)


def credential_values(path: Path) -> list[str]:
    values = load_object(path)
    mode = values.get("mode")
    if mode == "gha":
        return []
    if mode not in {"reader", "writer"}:
        raise ConfigError("invalid mode")
    validate_r2(values, mode)
    credentials = [
        required_string(values, "access_key_id"),
        required_string(values, "secret_access_key"),
    ]
    if values.get("local") is not None:
        local = validate_local(values["local"])
        credentials.extend((local["username"], local["password"]))
    return credentials


def source_is_trusted(event_path: Path | None) -> bool:
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    ref = os.environ.get("GITHUB_REF", "")
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    if (event_name, ref) in {
        ("push", "refs/heads/main"),
        ("push", "refs/heads/devnet"),
        ("push", "refs/heads/testnet"),
        ("schedule", "refs/heads/main"),
    }:
        return True
    if event_name == "workflow_dispatch" and ref == "refs/heads/main":
        if event_path is None or not event_path.is_file():
            return False
        inputs = load_object(event_path).get("inputs")
        return isinstance(inputs, dict) and inputs.get("source_ref") in {
            "main",
            "devnet",
            "testnet",
        }
    if event_name == "pull_request" and re.fullmatch(r"refs/pull/[0-9]+/merge", ref):
        if event_path is None or not event_path.is_file() or not repository:
            return False
        pull = load_object(event_path).get("pull_request")
        if not isinstance(pull, dict):
            return False
        head = pull.get("head")
        user = pull.get("user")
        if not isinstance(head, dict) or not isinstance(user, dict):
            return False
        head_repository = head.get("repo")
        return (
            isinstance(head_repository, dict)
            and head_repository.get("full_name") == repository
            and head_repository.get("fork") is False
            and user.get("login") != "dependabot[bot]"
        )
    return False


def field_pairs(path: Path) -> list[tuple[str, str]]:
    values = load_object(path)
    mode = values.get("mode")
    if mode == "gha":
        return [("mode", "gha")]
    if mode not in {"reader", "writer"}:
        raise ConfigError("invalid mode")
    validate_r2(values, mode)
    pairs = [("mode", mode)]
    pairs.extend((key, required_string(values, key)) for key in R2_STRING_FIELDS)
    local_value = values.get("local")
    pairs.append(("local_enabled", "true" if local_value is not None else "false"))
    if local_value is not None:
        local = validate_local(local_value)
        pairs.extend((f"local_{key}", local[key]) for key in sorted(LOCAL_FIELDS))
    return pairs


def emit_pairs(pairs: list[tuple[str, str]]) -> None:
    for key, value in pairs:
        sys.stdout.buffer.write(key.encode() + b"\0" + value.encode() + b"\0")


def main(argv: list[str]) -> int:
    if not argv:
        raise ConfigError("missing command")
    command, *arguments = argv
    if command == "normalize-reader" and len(arguments) == 2:
        normalize_reader(Path(arguments[0]), arguments[1])
    elif command == "write-writer" and len(arguments) == 1:
        write_writer(Path(arguments[0]))
    elif command == "credentials" and len(arguments) == 1:
        for value in credential_values(Path(arguments[0])):
            print(value)
    elif command == "source-trusted" and len(arguments) == 1:
        path = Path(arguments[0]) if arguments[0] else None
        return 0 if source_is_trusted(path) else 1
    elif command == "fields" and len(arguments) == 1:
        emit_pairs(field_pairs(Path(arguments[0])))
    else:
        raise ConfigError("invalid command arguments")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except ConfigError as error:
        print(f"sccache configuration error: {error}", file=sys.stderr)
        raise SystemExit(1) from None
