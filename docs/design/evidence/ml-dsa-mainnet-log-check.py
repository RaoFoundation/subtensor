#!/usr/bin/env python3
"""Verify and enrich a completed mainnet upgrade runner's local evidence.

This reads the untouched runner result, source JSON, and execution logs. It
requires ten contiguous successful blocks with state-root/signature checks
and try-state All, including a successful full-state decode in every block.
It never executes the runtime or accesses the network.
"""

import argparse
import datetime
import hashlib
import json
from pathlib import Path
import re

ROOT_PATTERN = r"initialized state externalities with storage root (0x[0-9a-f]{64})"
EXECUTION_PATTERN = re.compile(
    r"try-runtime: executing block #(\d+) / state root check: (true|false)"
    r" / signature check: (true|false) / try-state-select: (\S+)"
)
SUCCESS_PATTERN = re.compile(r"try-runtime: Block #(\d+) successfully executed")
DECODE_PATTERN = re.compile(
    r"Entire runtime state decodes without error\. (\d+) bytes total\."
)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def one_match(pattern, text):
    matches = re.findall(pattern, text)
    require(len(matches) == 1, f"Expected exactly one match: {pattern}")
    return matches[0]


def verify_blocks(text, first_block, count):
    expected = list(range(first_block, first_block + count))
    executions = list(EXECUTION_PATTERN.finditer(text))
    successes = list(SUCCESS_PATTERN.finditer(text))
    require(
        [int(match[1]) for match in executions] == expected,
        "Executed block sequence/count differs from the requested sequence",
    )
    require(
        [int(match[1]) for match in successes] == expected,
        "Successful block sequence/count differs from the requested sequence",
    )
    blocks = []
    for index, (execution, success) in enumerate(zip(executions, successes)):
        require(
            execution.groups()[1:] == ("true", "true", "All"),
            f"Incomplete checks on block {execution[1]}",
        )
        require(execution.end() < success.start(), "Success precedes execution")
        if index + 1 < len(executions):
            require(
                success.end() < executions[index + 1].start(),
                "Block executions overlap",
            )
        decoded_bytes = int(
            one_match(DECODE_PATTERN, text[execution.end() : success.start()])
        )
        blocks.append(
            {
                "number": int(execution[1]),
                "state_root_check": True,
                "signature_check": True,
                "try_state": "All",
                "entire_runtime_state_decodes": True,
                "decoded_state_bytes": decoded_bytes,
                "successfully_executed": True,
            }
        )
    return blocks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--result", type=Path, required=True)
    parser.add_argument("--runner", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--log-dir", type=Path, required=True)
    parser.add_argument("--snapshot-log", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    require(args.result.resolve() != args.output.resolve(), "Preserve the raw result")
    result = json.loads(args.result.read_text())
    source = json.loads(args.source.read_text())
    require(result["successful"] is True, "Runner did not pass")
    require(result["source"] == source, "Source record differs from runner input")
    require(sha256(args.source) == result["source_json_sha256"], "Source hash differs")
    require(sha256(args.runner) == result["script_sha256"], "Runner hash differs")
    require(
        [run["name"] for run in result["runs"]]
        == ["on-runtime-upgrade", "fast-forward"],
        "Expected upgrade and fast-forward results",
    )
    logs = {}
    for run in result["runs"]:
        log_file = args.log_dir / run["log_file"]
        require(run["exit_code"] == 0, f"Failed run: {run['name']}")
        require(sha256(log_file) == run["log_sha256"], f"Log hash differs: {log_file}")
        text = log_file.read_text()
        require(
            one_match(ROOT_PATTERN, text) == source["state_root"],
            "Loaded state differs from the pinned finalized header",
        )
        versions = re.findall(
            r'runtime\s+\[Name: "([^"]+)"\] \[Version: ([0-9]+)\]', text
        )
        require(len(versions) == 2, "Missing source/candidate runtime versions")
        require(
            all(name == source["source_spec_name"] for name, _ in versions),
            "Runtime spec names differ",
        )
        require(
            int(versions[0][1]) == source["source_spec_version"]
            and int(versions[1][1]) > source["source_spec_version"],
            "Runtime spec versions do not advance from the source",
        )
        logs[run["name"]] = text
    upgrade = logs["on-runtime-upgrade"]
    for marker in (
        "Running TryRuntime_on_runtime_upgrade with checks: All",
        "Running TryRuntime_on_runtime_upgrade again to check idempotency: All",
        "Migrations are idempotent",
    ):
        require(marker in upgrade, f"Missing upgrade check marker: {marker}")
    forward = result["runs"][1]
    require(forward["requested_blocks"] == 10, "Expected ten requested blocks")
    require(
        int(one_match(r"Fast forwarding (\d+) blocks", logs["fast-forward"])) == 10,
        "CLI block count differs",
    )
    blocks = verify_blocks(
        logs["fast-forward"], source["finalized_block_number"] + 1, 10
    )
    forward.update(
        observed_executed_blocks=len(blocks),
        observed_successful_blocks=len(blocks),
        block_sequence_matches_request=True,
        blocks=blocks,
    )
    snapshot_log = args.snapshot_log.read_text()
    snapshot_root = one_match(ROOT_PATTERN, snapshot_log)
    snapshot_bytes = int(one_match(r"writing snapshot of (\d+) bytes", snapshot_log))
    require(snapshot_root == source["state_root"], "Snapshot acquisition root differs")
    require(snapshot_bytes == result["snapshot_size_bytes"], "Snapshot size differs")
    result["snapshot_acquisition_proof"] = {
        "log_sha256": sha256(args.snapshot_log),
        "unique_top_level_keys": int(
            one_match(
                r"Parallel key fetching complete: (\d+) unique keys", snapshot_log
            )
        ),
        "child_tries": int(
            one_match(r"Completed scraping (\d+) child tries", snapshot_log)
        ),
        "storage_root": snapshot_root,
        "snapshot_size_bytes": snapshot_bytes,
        "state_root_matches_finalized_header": True,
    }
    result["post_run_verification"] = {
        "script_sha256": sha256(Path(__file__)),
        "raw_runner_result_sha256": sha256(args.result),
        "verified_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "all_log_digests_match": True,
        "exact_block_sequence_and_per_block_checks_passed": True,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["post_run_verification"], indent=2))


if __name__ == "__main__":
    main()
