#!/usr/bin/env python3
"""Replay a local mainnet snapshot with a pinned try-runtime Wasm candidate.

The input source JSON must record the finalized block's ``state_root``,
``finalized_block_hash``, and ``source_spec_version`` from the archive RPC.
This script never submits transactions or writes remote chain state. It runs
all upgrade checks and ten local post-upgrade blocks, preserving spec-version
and migration-idempotency checks. It performs no reference weight calibration.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

CLI_VERSION = "0.10.1"
CLI_SHA256 = "d6fcd586fba4c7245668f4ae2a06ffc94e6da4135fae22cfe24453dd11aee3c3"
CLI_RELEASE = "https://github.com/paritytech/try-runtime-cli/releases/tag/v0.10.1"
MAINNET_GENESIS = "0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--snapshot", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--try-runtime", dest="cli", type=Path, required=True)
    args = parser.parse_args()
    runtime, snapshot, cli = (
        path.resolve(strict=True) for path in (args.runtime, args.snapshot, args.cli)
    )
    source = json.loads(args.source.read_text())
    assert source["genesis_hash"] == MAINNET_GENESIS, "Unexpected source chain"
    for field in ("state_root", "finalized_block_hash"):
        assert re.fullmatch(r"0x[0-9a-f]{64}", source[field]), field
    assert sha256(cli) == CLI_SHA256, "CLI differs from official pinned release"
    version = subprocess.check_output([str(cli), "--version"], text=True).strip()
    assert version == f"try-runtime-core {CLI_VERSION}", version
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    result_file = output / "ml-dsa-mainnet-upgrade-results.json"
    original_runtime_hash = sha256(runtime)
    manifest = {
        "schema_version": 1,
        "kind": "local-mainnet-try-runtime-validation",
        "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "script_sha256": sha256(Path(__file__)),
        "source": source,
        "source_json_sha256": sha256(args.source),
        "snapshot_sha256": sha256(snapshot),
        "snapshot_size_bytes": snapshot.stat().st_size,
        "runtime_wasm_sha256": original_runtime_hash,
        "runtime_wasm_size_bytes": runtime.stat().st_size,
        "try_runtime_version": CLI_VERSION,
        "try_runtime_sha256": CLI_SHA256,
        "try_runtime_release": CLI_RELEASE,
        "chain_writes": False,
        "spec_name_check_enabled": True,
        "spec_version_check_enabled": True,
        "idempotency_check_enabled": True,
        "multi_block_migrations": "disabled because runtime MultiBlockMigrator=()",
        "reference_weight_measurements": False,
        "runs": [],
    }
    commands = [
        (
            "on-runtime-upgrade",
            [
                "on-runtime-upgrade",
                "--checks=all",
                "--blocktime",
                "12000",
                "--no-weight-warnings",
                "--disable-mbm-checks",
                "snap",
                "--path",
                str(snapshot),
            ],
        ),
        (
            "fast-forward",
            [
                "fast-forward",
                "--run-migrations",
                "--n-blocks",
                "10",
                "--blocktime",
                "12000",
                "--try-state",
                "all",
                "snap",
                "--path",
                str(snapshot),
            ],
        ),
    ]
    failed = False
    # The pinned CLI lowercases --runtime paths. A lowercase temporary copy
    # avoids changing or misreading artifacts under case-sensitive directories.
    with tempfile.TemporaryDirectory(prefix="mldsa-upgrade-", dir="/tmp") as temp:
        validated_runtime = Path(temp) / "runtime.wasm"
        shutil.copyfile(runtime, validated_runtime)
        assert sha256(validated_runtime) == original_runtime_hash
        for name, options in commands:
            log = output / f"ml-dsa-mainnet-{name}.log"
            command = [str(cli), "--runtime", str(validated_runtime), *options]
            started = time.monotonic()
            with log.open("w") as handle:
                process = subprocess.run(
                    command,
                    stdout=handle,
                    stderr=subprocess.STDOUT,
                    env={
                        **os.environ,
                        "RUST_LOG": "info,remote-ext=debug,runtime=debug",
                    },
                    check=False,
                )
            text = log.read_text()
            match = re.search(
                r"initialized state externalities with storage root (0x[0-9a-f]{64})",
                text,
            )
            state_root = match.group(1) if match else None
            command_template = [
                "<try-runtime>",
                "--runtime",
                "<runtime-wasm>",
                *[
                    "<snapshot>" if option == str(snapshot) else option
                    for option in options
                ],
            ]
            result = {
                "name": name,
                "command_template": command_template,
                "exit_code": process.returncode,
                "elapsed_seconds": round(time.monotonic() - started, 3),
                "log_file": log.name,
                "log_sha256": sha256(log),
                "snapshot_loaded_state_root": state_root,
                "snapshot_state_root_matches_finalized_header": state_root
                == source["state_root"],
            }
            if name == "on-runtime-upgrade":
                result["migrations_idempotent"] = "Migrations are idempotent" in text
                result["upgrade_checks_all"] = (
                    "Running TryRuntime_on_runtime_upgrade with checks: All" in text
                )
                versions = re.findall(
                    r"runtime\s+\[Name: \"[^\"]+\"\] \[Version: ([0-9]+)\]", text
                )
                result["reported_runtime_versions"] = [int(v) for v in versions]
            else:
                result["requested_blocks"] = 10
            manifest["runs"].append(result)
            result_file.write_text(json.dumps(manifest, indent=2) + "\n")
            print(json.dumps(result, indent=2), flush=True)
            if (
                process.returncode != 0
                or state_root != source["state_root"]
                or (
                    name == "on-runtime-upgrade"
                    and (
                        not result["migrations_idempotent"]
                        or not result["upgrade_checks_all"]
                        or len(result["reported_runtime_versions"]) != 2
                        or result["reported_runtime_versions"][0]
                        != source["source_spec_version"]
                        or result["reported_runtime_versions"][1]
                        <= source["source_spec_version"]
                    )
                )
            ):
                failed = True
                break
        assert sha256(validated_runtime) == original_runtime_hash
    assert sha256(runtime) == original_runtime_hash, (
        "Candidate Wasm changed during validation"
    )
    manifest["successful"] = not failed
    result_file.write_text(json.dumps(manifest, indent=2) + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
