#!/usr/bin/env python3

from __future__ import annotations

import datetime as dt
import importlib.util
import json
import os
import subprocess
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


SCRIPT = Path(__file__).with_name("r2-artifact-mirror.py")
PUBLISH_HELPER = Path(__file__).with_name("publish-artifact-mirror.sh")
SPEC = importlib.util.spec_from_file_location("r2_artifact_mirror", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


assert MODULE.validate_endpoint(
    "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com"
) == ("3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com", 443)

for endpoint in (
    "http://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
    "https://example.com",
    "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com/escape",
    "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com:444",
):
    try:
        MODULE.validate_endpoint(endpoint)
    except MODULE.MirrorError:
        pass
    else:
        raise AssertionError(f"unsafe endpoint was accepted: {endpoint}")

uri, headers = MODULE.authorization_headers(
    host="3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
    key="artifacts/v1/objects/123-deadbeef.zip",
    payload_sha256="0" * 64,
    access_key="test-access-key",
    secret_key="test-secret-key",
    now=dt.datetime(2026, 7, 16, 12, 0, tzinfo=dt.timezone.utc),
)
assert uri == "/subtensor-ci-sccache/artifacts/v1/objects/123-deadbeef.zip"
assert headers["x-amz-date"] == "20260716T120000Z"
assert (
    "Credential=test-access-key/20260716/auto/s3/aws4_request"
    in headers["Authorization"]
)
assert "SignedHeaders=host;x-amz-content-sha256;x-amz-date" in headers["Authorization"]

with tempfile.TemporaryDirectory() as directory:
    archive = Path(directory) / "artifact.zip"
    archive.write_bytes(b"artifact archive")
    digest, size = MODULE.file_sha256(archive)
    args = SimpleNamespace(
        workflow_path=".github/workflows/refresh-mainnet-snapshot.yml",
        artifact_id=123,
        artifact_name="mainnet-snapshot",
        digest=f"sha256:{digest}",
        producer_sha="a" * 40,
    )
    environment = {
        "GITHUB_REPOSITORY": "RaoFoundation/subtensor",
        "GITHUB_REPOSITORY_ID": "608683796",
    }
    with (
        mock.patch.dict(os.environ, environment, clear=True),
        mock.patch.object(MODULE.time, "time", return_value=1_768_476_000),
    ):
        manifest = json.loads(
            MODULE.build_manifest(args, f"artifacts/v1/objects/123-{digest}.zip", size)
        )
    assert manifest == {
        "schema_version": 1,
        "repository": "RaoFoundation/subtensor",
        "repository_id": 608683796,
        "workflow_path": ".github/workflows/refresh-mainnet-snapshot.yml",
        "artifact_id": 123,
        "artifact_name": "mainnet-snapshot",
        "digest": f"sha256:{digest}",
        "size_in_bytes": size,
        "object_key": f"artifacts/v1/objects/123-{digest}.zip",
        "producer_sha": "a" * 40,
        "published_at": 1_768_476_000,
    }

with tempfile.TemporaryDirectory() as directory:
    temp = Path(directory)
    bin_dir = temp / "bin"
    bin_dir.mkdir()
    record = temp / "publisher-arguments"
    (bin_dir / "gh").write_text(
        "#!/usr/bin/env bash\nprintf 'immutable artifact zip'\n",
        encoding="utf-8",
    )
    (bin_dir / "python3").write_text(
        """#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == */r2-artifact-mirror.py ]]
[[ -s "$2" ]]
printf '%s\n' "${@:3}" > "$PUBLISH_RECORD"
""",
        encoding="utf-8",
    )
    (bin_dir / "gh").chmod(0o755)
    (bin_dir / "python3").chmod(0o755)
    result = subprocess.run(
        [
            str(PUBLISH_HELPER),
            "123",
            "mainnet-snapshot",
            f"sha256:{'a' * 64}",
            "b" * 40,
            ".github/workflows/refresh-mainnet-snapshot.yml",
        ],
        env={
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "GH_TOKEN": "token",
            "GITHUB_REPOSITORY": "example/repository",
            "PUBLISH_RECORD": str(record),
        },
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert record.read_text(encoding="utf-8").splitlines() == [
        "123",
        "mainnet-snapshot",
        f"sha256:{'a' * 64}",
        "b" * 40,
        ".github/workflows/refresh-mainnet-snapshot.yml",
    ]

print("R2 artifact mirror tests passed")
