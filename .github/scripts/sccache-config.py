#!/usr/bin/env python3
"""Prepare and activate the complete typed sccache backend contract."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
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
MMDS_TOKEN_URL_DEFAULT = "http://169.254.169.254/latest/api/token"
MMDS_METADATA_URL_DEFAULT = "http://169.254.169.254/latest/meta-data/sccache"


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


def append_values(path: Path, values: dict[str, str]) -> None:
    with path.open("a", encoding="utf-8") as output:
        for name, value in values.items():
            if "\n" in value or "\r" in value:
                raise ConfigError(f"invalid newline in {name}")
            output.write(f"{name}={value}\n")


def warning(message: str) -> None:
    print(f"::warning::{message}")


def mask(value: str) -> None:
    if value:
        print(f"::add-mask::{value}")


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
    required_string(values, "access_key_id")
    required_string(values, "secret_access_key")


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
        or local["password"]
        != required_string(credential_source, "secret_access_key")
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
        values["local"] = validate_local(
            values["local"], credential_source=values
        )
    values["mode"] = "reader"
    atomic_write(path, values)


def write_writer(path: Path) -> None:
    values: dict[str, object] = {
        "mode": "writer",
        **TRUSTED_R2,
        "s3_rw_mode": "READ_WRITE",
        "access_key_id": os.environ.get("AWS_ACCESS_KEY_ID", ""),
        "secret_access_key": os.environ.get("AWS_SECRET_ACCESS_KEY", ""),
    }
    validate_r2(values, "writer")
    atomic_write(path, values)


def attach_writer_local(writer_path: Path, reader_path: Path) -> None:
    writer = load_object(writer_path)
    if writer.get("mode") != "writer":
        raise ConfigError("invalid writer mode")
    validate_r2(writer, "writer")

    reader = load_object(reader_path)
    if reader.get("mode") != "reader":
        raise ConfigError("invalid reader mode")
    validate_r2(reader, "reader")
    if reader.get("local") is None:
        raise ConfigError("local cache contract is unavailable")
    writer["local"] = validate_local(reader["local"], credential_source=reader)
    atomic_write(writer_path, writer)


def credential_values(values: dict[str, object]) -> list[str]:
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
    # pull_request is never writer-trusted: the job checks out PR HEAD and
    # runs PR-controlled actions/build scripts. Writer keys are exported into
    # the job environment for sccache, so granting them on PRs is an
    # exfiltration / cache-poisoning path. PR jobs must use reader mode;
    # write-through warming belongs on push/schedule of protected branches.
    return False


def trusted_source(event_path: Path | None) -> bool:
    try:
        return source_is_trusted(event_path)
    except ConfigError:
        return False


def curl(arguments: list[str], *, capture: bool = False) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            ["curl", *arguments],
            check=False,
            capture_output=capture,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        raise ConfigError("MMDS request failed") from None


def fetch_reader_contract(path: Path) -> str | None:
    token_url = os.environ.get("MMDS_TOKEN_URL", MMDS_TOKEN_URL_DEFAULT)
    metadata_url = os.environ.get("MMDS_METADATA_URL", MMDS_METADATA_URL_DEFAULT)
    try:
        token_result = curl(
            [
                "--fail",
                "--silent",
                "--show-error",
                "--connect-timeout",
                "1",
                "--max-time",
                "2",
                "--request",
                "PUT",
                "--header",
                "X-Metadata-Token-TTL-Seconds: 60",
                token_url,
            ],
            capture=True,
        )
        if token_result.returncode != 0:
            return "MMDSv2 token service is unavailable"
        token = token_result.stdout.decode("utf-8").rstrip("\r\n")
        if not token or "\n" in token or "\r" in token:
            return "MMDSv2 returned an invalid token"

        metadata_result = curl(
            [
                "--fail",
                "--silent",
                "--show-error",
                "--connect-timeout",
                "1",
                "--max-time",
                "3",
                "--header",
                f"X-Metadata-Token: {token}",
                "--header",
                "Accept: application/json",
                "--output",
                str(path),
                metadata_url,
            ]
        )
        if metadata_result.returncode != 0:
            return "MMDSv2 sccache metadata is unavailable"
        os.chmod(path, 0o600)
        normalize_reader(path, os.environ.get("SCCACHE_LOCAL_TIER_MODE", "auto"))
    except (ConfigError, OSError, UnicodeError):
        path.unlink(missing_ok=True)
        return "MMDSv2 sccache metadata failed validation"
    return None


def disable_prepare(config_path: Path, output_path: Path, reason: str) -> int:
    config_path.unlink(missing_ok=True)
    append_values(output_path, {"available": "false"})
    warning(f"sccache disabled: {reason}")
    return 0


def fallback_reader(config_path: Path, output_path: Path, reason: str) -> bool:
    if os.environ.get("SCCACHE_GHA_FALLBACK", "true") == "true":
        atomic_write(config_path, {"mode": "gha"})
        warning(f"R2 reader unavailable; using GitHub Actions sccache: {reason}")
        return True
    disable_prepare(config_path, output_path, reason)
    return False


def prepare_reader(config_path: Path, output_path: Path) -> bool:
    error = fetch_reader_contract(config_path)
    return error is None or fallback_reader(config_path, output_path, error)


def credentials_are_well_formed() -> bool:
    return all(
        value and "\n" not in value and "\r" not in value
        for value in (
            os.environ.get("AWS_ACCESS_KEY_ID", ""),
            os.environ.get("AWS_SECRET_ACCESS_KEY", ""),
        )
    )


def prepare_writer(config_path: Path, output_path: Path) -> bool:
    event_path_value = os.environ.get("GITHUB_EVENT_PATH", "")
    event_path = Path(event_path_value) if event_path_value else None
    if not trusted_source(event_path):
        disable_prepare(
            config_path, output_path, "writer mode is restricted to trusted cache sources"
        )
        return False
    if not credentials_are_well_formed():
        disable_prepare(
            config_path, output_path, "protected writer credentials are unavailable or malformed"
        )
        return False
    local_mode = os.environ.get("SCCACHE_LOCAL_TIER_MODE", "auto")
    if local_mode not in {"auto", "disabled"}:
        disable_prepare(config_path, output_path, "invalid local tier mode")
        return False

    write_writer(config_path)
    if local_mode == "disabled":
        return True

    reader_path = config_path.with_name(config_path.name + ".reader")
    reader_path.unlink(missing_ok=True)
    error = fetch_reader_contract(reader_path)
    if error is not None:
        reader_path.unlink(missing_ok=True)
        warning(f"local sccache reader unavailable; using direct R2 writer: {error}")
        return True
    try:
        attach_writer_local(config_path, reader_path)
    except ConfigError:
        warning("local sccache reader failed validation; using direct R2 writer")
    finally:
        reader_path.unlink(missing_ok=True)
    return True


def prepare(mode: str, config_path: Path, output_path: Path) -> int:
    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.unlink(missing_ok=True)

    if mode == "reader":
        available = prepare_reader(config_path, output_path)
    elif mode == "writer":
        available = prepare_writer(config_path, output_path)
    elif mode == "auto":
        event_path_value = os.environ.get("GITHUB_EVENT_PATH", "")
        event_path = Path(event_path_value) if event_path_value else None
        if trusted_source(event_path) and credentials_are_well_formed():
            available = prepare_writer(config_path, output_path)
        else:
            available = prepare_reader(config_path, output_path)
    else:
        return disable_prepare(config_path, output_path, "unknown credential mode")

    if not available:
        return 0
    values = load_object(config_path)
    for credential in credential_values(values):
        mask(credential)
    append_values(
        output_path,
        {"available": "true", "config-file": str(config_path)},
    )
    print(f"sccache {mode} configuration validated")
    return 0


def stop_server(binary: str, environment: dict[str, str]) -> None:
    try:
        subprocess.run(
            [binary, "--stop-server"],
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        pass


def start_server(binary: str, environment: dict[str, str], log_path: Path) -> bool:
    try:
        with log_path.open("wb") as log:
            result = subprocess.run(
                [binary, "--start-server"],
                env=environment,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=False,
                timeout=30,
            )
        return result.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def disable_activate(config_path: Path, output_path: Path, reason: str) -> int:
    config_path.unlink(missing_ok=True)
    append_values(output_path, {"enabled": "false"})
    warning(f"sccache disabled: {reason}")
    return 0


def activate(config_path: Path, env_path: Path, output_path: Path) -> int:
    if not config_path.is_file():
        return disable_activate(
            config_path, output_path, "validated configuration is unavailable"
        )
    if os.environ.get("SCCACHE_INSTALL_OUTCOME", "success") != "success":
        return disable_activate(config_path, output_path, "sccache installation failed")
    binary = os.environ.get("SCCACHE_PATH", "") or shutil.which("sccache") or ""
    if not binary or not os.access(binary, os.X_OK):
        return disable_activate(config_path, output_path, "sccache executable is unavailable")

    try:
        values = load_object(config_path)
        mode = values.get("mode")
        environment = os.environ.copy()
        exported: dict[str, str]
        local_tier = False

        if mode == "gha":
            if set(values) != {"mode"}:
                raise ConfigError("invalid GitHub Actions cache contract")
            environment.update(
                {
                    "SCCACHE_GHA_ENABLED": "true",
                    "SCCACHE_IGNORE_SERVER_IO_ERROR": "1",
                }
            )
            exported = {
                "SCCACHE_ENABLED": "true",
                "SCCACHE_BACKEND": "gha",
                "SCCACHE_GHA_ENABLED": "true",
                "SCCACHE_IGNORE_SERVER_IO_ERROR": "1",
                "RUSTC_WRAPPER": "sccache",
                "CARGO_INCREMENTAL": "0",
            }
        elif mode in {"reader", "writer"}:
            validate_r2(values, mode)
            access_key = required_string(values, "access_key_id")
            secret_key = required_string(values, "secret_access_key")
            mask(access_key)
            mask(secret_key)
            environment.update(
                {
                    "SCCACHE_BUCKET": required_string(values, "bucket"),
                    "SCCACHE_ENDPOINT": required_string(values, "endpoint"),
                    "SCCACHE_REGION": required_string(values, "region"),
                    "SCCACHE_S3_USE_SSL": "true",
                    "SCCACHE_S3_KEY_PREFIX": required_string(values, "key_prefix"),
                    "SCCACHE_IGNORE_SERVER_IO_ERROR": "1",
                    "AWS_ACCESS_KEY_ID": access_key,
                    "AWS_SECRET_ACCESS_KEY": secret_key,
                }
            )
            if values.get("local") is not None:
                local = validate_local(values["local"])
                mask(local["username"])
                mask(local["password"])
                environment.update(
                    {
                        "SCCACHE_MULTILEVEL_CHAIN": "webdav,s3",
                        "SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY": (
                            "all" if mode == "writer" else "ignore"
                        ),
                        "SCCACHE_WEBDAV_ENDPOINT": local["endpoint"],
                        "SCCACHE_WEBDAV_KEY_PREFIX": local["key_prefix"],
                        "SCCACHE_WEBDAV_USERNAME": local["username"],
                        "SCCACHE_WEBDAV_PASSWORD": local["password"],
                    }
                )
                local_tier = True
            exported = {
                "SCCACHE_ENABLED": "true",
                "SCCACHE_BACKEND": "r2",
                "RUSTC_WRAPPER": "sccache",
                "CARGO_INCREMENTAL": "0",
                **{
                    key: environment[key]
                    for key in (
                        "SCCACHE_BUCKET",
                        "SCCACHE_ENDPOINT",
                        "SCCACHE_REGION",
                        "SCCACHE_S3_USE_SSL",
                        "SCCACHE_S3_KEY_PREFIX",
                        "SCCACHE_IGNORE_SERVER_IO_ERROR",
                        "AWS_ACCESS_KEY_ID",
                        "AWS_SECRET_ACCESS_KEY",
                    )
                },
                "SCCACHE_LOCAL_TIER": "true" if local_tier else "false",
            }
        else:
            raise ConfigError("invalid mode")
    except ConfigError:
        return disable_activate(
            config_path, output_path, "validated configuration could not be parsed"
        )

    runner_temp = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir()))
    runner_temp.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        prefix="sccache-start-", suffix=".log", dir=runner_temp, delete=False
    ) as log:
        log_path = Path(log.name)
    try:
        stop_server(binary, environment)
        if not start_server(binary, environment, log_path):
            if local_tier:
                warning("local sccache tier startup failed; retrying direct R2")
                stop_server(binary, environment)
                for key in (
                    "SCCACHE_MULTILEVEL_CHAIN",
                    "SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY",
                    "SCCACHE_WEBDAV_ENDPOINT",
                    "SCCACHE_WEBDAV_KEY_PREFIX",
                    "SCCACHE_WEBDAV_USERNAME",
                    "SCCACHE_WEBDAV_PASSWORD",
                ):
                    environment.pop(key, None)
                local_tier = False
                exported["SCCACHE_LOCAL_TIER"] = "false"
                if not start_server(binary, environment, log_path):
                    stop_server(binary, environment)
                    return disable_activate(
                        config_path, output_path, "R2 backend startup check failed"
                    )
            else:
                stop_server(binary, environment)
                backend = "GitHub Actions" if mode == "gha" else "R2"
                return disable_activate(
                    config_path, output_path, f"{backend} backend startup failed"
                )

        if local_tier:
            for key in (
                "SCCACHE_MULTILEVEL_CHAIN",
                "SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY",
                "SCCACHE_WEBDAV_ENDPOINT",
                "SCCACHE_WEBDAV_KEY_PREFIX",
                "SCCACHE_WEBDAV_USERNAME",
                "SCCACHE_WEBDAV_PASSWORD",
            ):
                exported[key] = environment[key]
        append_values(env_path, exported)
        append_values(output_path, {"enabled": "true"})
        config_path.unlink(missing_ok=True)
        backend = "GitHub Actions" if mode == "gha" else "R2"
        print(f"sccache {backend} backend enabled" + (f" in {mode} mode" if mode != "gha" else ""))
        return 0
    finally:
        log_path.unlink(missing_ok=True)


def main(argv: list[str]) -> int:
    if not argv:
        raise ConfigError("missing command")
    command, *arguments = argv
    if command == "prepare" and len(arguments) == 3:
        return prepare(arguments[0], Path(arguments[1]), Path(arguments[2]))
    if command == "activate" and len(arguments) == 3:
        return activate(Path(arguments[0]), Path(arguments[1]), Path(arguments[2]))
    raise ConfigError("invalid command arguments")


if __name__ == "__main__":
    os.umask(0o077)
    try:
        raise SystemExit(main(sys.argv[1:]))
    except ConfigError as error:
        print(f"sccache configuration error: {error}", file=sys.stderr)
        raise SystemExit(1) from None
