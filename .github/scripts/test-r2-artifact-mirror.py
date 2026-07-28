#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


SCRIPT = Path(__file__).with_name("r2-artifact-mirror.py")
PUBLISH_HELPER = Path(__file__).with_name("publish-artifact-mirror.sh")
CURRENT_RUN_HELPER = Path(__file__).with_name(
    "publish-current-run-artifact-mirror.sh"
)
SPEC = importlib.util.spec_from_file_location("r2_artifact_mirror", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

assert MODULE.ALLOWED_ARTIFACT_NAMES == {
    "mainnet-snapshot",
    "try-runtime-snap-v0.10.1-mainnet",
    "try-runtime-snap-v0.10.1-testnet",
    "try-runtime-snap-v0.10.1-devnet",
}


assert MODULE.validate_endpoint(
    "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com"
) == "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com"

for endpoint in (
    "http://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
    "https://example.com",
    "https://00000000000000000000000000000000.r2.cloudflarestorage.com",
    "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com/escape",
    "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com:444",
):
    try:
        MODULE.validate_endpoint(endpoint)
    except MODULE.MirrorError:
        pass
    else:
        raise AssertionError(f"unsafe endpoint was accepted: {endpoint}")

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

    for artifact_name in MODULE.ALLOWED_ARTIFACT_NAMES:
        parsed = MODULE.parse_args(
            [
                str(archive),
                "123",
                artifact_name,
                f"sha256:{digest}",
                "a" * 40,
                ".github/workflows/refresh-mainnet-snapshot.yml",
            ]
        )
        assert parsed.artifact_name == artifact_name

    with mock.patch.object(sys, "stderr", io.StringIO()):
        try:
            MODULE.parse_args(
                [
                    str(archive),
                    "123",
                    "untrusted-artifact",
                    f"sha256:{digest}",
                    "a" * 40,
                    ".github/workflows/refresh-mainnet-snapshot.yml",
                ]
            )
        except SystemExit as error:
            assert error.code == 2
        else:
            raise AssertionError("untrusted mirror artifact was accepted")

    publisher_environment = {
        "SCCACHE_BUCKET": "subtensor-ci-sccache",
        "SCCACHE_REGION": "auto",
        "SCCACHE_ENDPOINT": "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
        "AWS_ACCESS_KEY_ID": "test-access-key",
        "AWS_SECRET_ACCESS_KEY": "test-secret-key",
    }
    with (
        mock.patch.dict(os.environ, publisher_environment, clear=True),
        mock.patch.object(MODULE.shutil, "which", return_value="/usr/bin/aws"),
        mock.patch.object(
            MODULE.subprocess,
            "run",
            return_value=SimpleNamespace(returncode=0, stderr=""),
        ) as run,
    ):
        MODULE.R2Publisher().put("artifacts/v1/objects/123.zip", archive)
    command = run.call_args.args[0]
    assert command[:3] == ["/usr/bin/aws", "s3api", "put-object"]
    assert command[command.index("--bucket") + 1] == "subtensor-ci-sccache"
    assert command[command.index("--key") + 1] == "artifacts/v1/objects/123.zip"
    assert "test-access-key" not in command
    assert "test-secret-key" not in command

    with (
        mock.patch.dict(os.environ, publisher_environment, clear=True),
        mock.patch.object(MODULE.shutil, "which", return_value=None),
    ):
        try:
            MODULE.R2Publisher()
        except MODULE.MirrorError:
            pass
        else:
            raise AssertionError("missing AWS CLI was accepted")

    with (
        mock.patch.dict(os.environ, publisher_environment, clear=True),
        mock.patch.object(MODULE.shutil, "which", return_value="/usr/bin/aws"),
        mock.patch.object(
            MODULE.subprocess,
            "run",
            return_value=SimpleNamespace(returncode=1, stderr="secret response"),
        ),
    ):
        try:
            MODULE.R2Publisher().put("artifacts/v1/objects/123.zip", archive)
        except MODULE.MirrorError as error:
            assert "secret response" not in str(error)
        else:
            raise AssertionError("failed AWS CLI upload was accepted")

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

with tempfile.TemporaryDirectory() as directory:
    temp = Path(directory)
    bin_dir = temp / "bin"
    bin_dir.mkdir()
    record = temp / "publisher-arguments"
    metadata = temp / "metadata.json"
    metadata.write_text(
        json.dumps(
            {
                "artifacts": [
                    {
                        "id": 123,
                        "name": "try-runtime-snap-v0.10.1-mainnet",
                        "expired": False,
                        "digest": f"sha256:{'a' * 64}",
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    (bin_dir / "gh").write_text(
        """#!/usr/bin/env bash
set -euo pipefail
case "$*" in
  *actions/runs/789/artifacts*) cat "$MOCK_METADATA" ;;
  *actions/artifacts/123/zip*) printf 'immutable artifact zip' ;;
  *) echo "unexpected gh invocation: $*" >&2; exit 2 ;;
esac
""",
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
    environment = {
        **os.environ,
        "PATH": f"{bin_dir}:{os.environ['PATH']}",
        "GH_TOKEN": "token",
        "GITHUB_REPOSITORY": "example/repository",
        "GITHUB_RUN_ID": "789",
        "GITHUB_SHA": "b" * 40,
        "MOCK_METADATA": str(metadata),
        "PUBLISH_RECORD": str(record),
    }
    result = subprocess.run(
        [str(CURRENT_RUN_HELPER), "try-runtime-snap-v0.10.1-mainnet"],
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert record.read_text(encoding="utf-8").splitlines() == [
        "123",
        "try-runtime-snap-v0.10.1-mainnet",
        f"sha256:{'a' * 64}",
        "b" * 40,
        ".github/workflows/refresh-mainnet-snapshot.yml",
    ]

    duplicate = json.loads(metadata.read_text(encoding="utf-8"))
    duplicate["artifacts"].append(dict(duplicate["artifacts"][0]))
    metadata.write_text(json.dumps(duplicate), encoding="utf-8")
    result = subprocess.run(
        [str(CURRENT_RUN_HELPER), "try-runtime-snap-v0.10.1-mainnet"],
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode != 0

print("R2 artifact mirror tests passed")
