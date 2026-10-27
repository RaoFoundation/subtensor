#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import os
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


SCRIPT = Path(__file__).with_name("r2-sccache-warmset.py")
SPEC = importlib.util.spec_from_file_location("r2_sccache_warmset", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


first = "a" * 64
second = "b" * 64
third = "c" * 64
assert MODULE.MAX_WARM_BYTES == 4 * 1024 * 1024 * 1024

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    log = root / "sccache.log"
    keys = root / "keys.txt"
    log.write_text(
        "ignored\n"
        f"[crate]: Hash key: {first}\n"
        f"malformed Hash key: {'d' * 63}\n"
        f"[crate]: Hash key: {second}\n"
        f"[duplicate]: Hash key: {first}\n",
        encoding="utf-8",
    )
    assert MODULE.extract_hashes(log, keys) == 2
    assert keys.read_text(encoding="ascii").splitlines() == [first, second]
    assert MODULE.load_hashes([keys]) == [first, second]

    malformed = root / "malformed.txt"
    malformed.write_text("../escape\n", encoding="ascii")
    try:
        MODULE.load_hashes([malformed])
    except MODULE.WarmsetError:
        pass
    else:
        raise AssertionError("malformed compiler cache key was accepted")

inventory = {
    f"{MODULE.COMPILER_PREFIX}/{MODULE.normalized_path(first)}": 10,
    f"{MODULE.COMPILER_PREFIX}/{MODULE.normalized_path(second)}": 20,
}
manifest = MODULE.build_manifest([first, second], inventory, "d" * 40, 100)
assert manifest == {
    "schema_version": 1,
    "bucket": MODULE.BUCKET,
    "key_prefix": MODULE.COMPILER_PREFIX,
    "generation": f"{'d' * 40}-100",
    "producer_sha": "d" * 40,
    "published_at": 100,
    "expires_at": 100 + MODULE.MANIFEST_TTL_SECONDS,
    "max_bytes": MODULE.MAX_WARM_BYTES,
    "captured_object_count": 2,
    "captured_size_bytes": 30,
    "selected_object_count": 2,
    "selected_size_bytes": 30,
    "objects": [
        {"path": MODULE.normalized_path(first), "size": 10},
        {"path": MODULE.normalized_path(second), "size": 20},
    ],
}

try:
    MODULE.build_manifest([first, third], inventory, "d" * 40, 100)
except MODULE.WarmsetError as error:
    assert "absent" in str(error)
else:
    raise AssertionError("incomplete durable cache set was accepted")

with mock.patch.object(MODULE, "MAX_WARM_BYTES", 15):
    capped = MODULE.build_manifest([first, second], inventory, "d" * 40, 100)
assert capped["selected_object_count"] == 1
assert capped["selected_size_bytes"] == 10

publisher_environment = {
    "SCCACHE_BUCKET": MODULE.BUCKET,
    "SCCACHE_REGION": MODULE.REGION,
    "SCCACHE_ENDPOINT": f"https://{MODULE.ACCOUNT_HOST}",
    "AWS_ACCESS_KEY_ID": "test-access-key",
    "AWS_SECRET_ACCESS_KEY": "test-secret-key",
}
with (
    mock.patch.dict(os.environ, publisher_environment, clear=True),
    mock.patch.object(MODULE.shutil, "which", return_value="/usr/bin/aws"),
):
    client = MODULE.R2Client()
assert client.endpoint == f"https://{MODULE.ACCOUNT_HOST}"
assert "test-access-key" not in repr(list(client.environment()))


def inventory_run(_command, **kwargs):
    kwargs["stdout"].write(
        json.dumps(
            {
                "Contents": [
                    {
                        "Key": f"{MODULE.COMPILER_PREFIX}/.sccache_check",
                        "Size": 0,
                    },
                    {
                        "Key": (
                            f"{MODULE.COMPILER_PREFIX}/"
                            f"{MODULE.normalized_path(first)}"
                        ),
                        "Size": 10,
                    },
                ]
            }
        ).encode()
    )
    return SimpleNamespace(returncode=0)


with (
    tempfile.TemporaryDirectory() as directory,
    mock.patch.dict(os.environ, publisher_environment, clear=True),
    mock.patch.object(MODULE.shutil, "which", return_value="/usr/bin/aws"),
    mock.patch.object(MODULE.subprocess, "run", side_effect=inventory_run) as run,
):
    client = MODULE.R2Client()
    listed = client.inventory()
    assert listed[f"{MODULE.COMPILER_PREFIX}/.sccache_check"] == 0
    assert listed[f"{MODULE.COMPILER_PREFIX}/{MODULE.normalized_path(first)}"] == 10
    body = Path(directory) / "manifest.json"
    body.write_text("{}\n", encoding="utf-8")
    run.reset_mock()
    run.side_effect = None
    run.return_value = SimpleNamespace(returncode=0)
    client.put(f"{MODULE.MANIFEST_PREFIX}/latest.json", body)
    command = run.call_args.args[0]
    assert "test-access-key" not in command
    assert "test-secret-key" not in command
    assert command[command.index("--key") + 1] == (
        f"{MODULE.MANIFEST_PREFIX}/latest.json"
    )

with tempfile.TemporaryDirectory() as directory:
    key_file = Path(directory) / "keys.txt"
    key_file.write_text(first + "\n", encoding="ascii")
    published = []
    stub_client = SimpleNamespace(
        inventory=lambda: {
            f"{MODULE.COMPILER_PREFIX}/{MODULE.normalized_path(first)}": 10
        },
        put=lambda key, source: published.append(
            (key, json.loads(source.read_text(encoding="utf-8")))
        ),
    )
    with (
        mock.patch.dict(
            os.environ,
            {**publisher_environment, "GITHUB_SHA": "d" * 40},
            clear=True,
        ),
        mock.patch.object(MODULE, "R2Client", return_value=stub_client),
        mock.patch.object(MODULE.time, "time", return_value=100),
    ):
        MODULE.publish([key_file])
    assert [key for key, _manifest in published] == [
        f"{MODULE.MANIFEST_PREFIX}/latest.json"
    ]
    assert published[0][1]["generation"] == f"{'d' * 40}-100"

for endpoint in (
    f"http://{MODULE.ACCOUNT_HOST}",
    "https://example.com",
    f"https://{MODULE.ACCOUNT_HOST}/escape",
):
    try:
        MODULE.validate_endpoint(endpoint)
    except MODULE.WarmsetError:
        pass
    else:
        raise AssertionError(f"unsafe endpoint was accepted: {endpoint}")
