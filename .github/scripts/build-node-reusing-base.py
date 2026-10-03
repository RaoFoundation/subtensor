#!/usr/bin/env python3
"""Run a node build, or reuse the base commit's node when only the runtime changed.

usage: build-node-reusing-base.py --node PATH [--base-node PATH] -- cargo build ...

Without --base-node this runs the cargo command unchanged.

With --base-node it runs the same cargo command, so every compiler invocation
keeps its sccache key, but stops cargo as soon as node-subtensor-runtime's build
script has produced the runtime wasm. That skips the native runtime crate, the
node crate, and the final link. verify-base-node-runtime.py then proves the
base node can run the new wasm. On success the base node is installed at
--node next to the freshly built wasm; on any failure the full cargo build
runs instead, so the job never produces a weaker artifact than before.

Writes `node_source=base|built` to $GITHUB_OUTPUT when it is set.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

RUNTIME_PACKAGE = re.compile(r"(^|#)node-subtensor-runtime[@ ]")
WASM_INCLUDE = re.compile(
    r'pub const WASM_BINARY: Option<&\[u8\]> = Some\(include_bytes!\("([^"]+)"\)\);'
)
VERIFIER = Path(__file__).with_name("verify-base-node-runtime.py")


def notice(level: str, message: str) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level}::{message}", flush=True)
    else:
        print(f"{level}: {message}", flush=True)


def append(env_name: str, text: str) -> None:
    path = os.environ.get(env_name)
    if path:
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(text)


def finish(source: str, summary: str) -> None:
    append("GITHUB_OUTPUT", f"node_source={source}\n")
    append("GITHUB_STEP_SUMMARY", f"### Node binary\n- {summary}\n")


def full_build(cargo: list[str]) -> int:
    started = time.monotonic()
    code = subprocess.run(cargo, check=False).returncode
    if code == 0:
        finish(
            "built", f"Built from this revision in {time.monotonic() - started:.0f}s"
        )
    return code


def build_runtime_wasm(cargo: list[str]) -> Path | None:
    """Run cargo until the runtime build script finishes; return the wasm path."""
    wrapper = os.environ.get("RUSTC_WRAPPER", "")
    if Path(wrapper).name == "sccache":
        # Start the server outside cargo's process group so stopping cargo
        # leaves it alive for the cache report.
        subprocess.run([wrapper, "--start-server"], capture_output=True, check=False)

    process = subprocess.Popen(
        [*cargo, "--message-format=json-render-diagnostics"],
        stdout=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    out_dir = None
    assert process.stdout is not None
    for line in process.stdout:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") == "build-script-executed" and RUNTIME_PACKAGE.search(
            message.get("package_id", "")
        ):
            out_dir = Path(message["out_dir"])
            break

    if out_dir is not None and process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
    process.wait()
    if process.stdout is not None:
        process.stdout.close()

    if out_dir is None:
        if process.returncode != 0:
            raise SystemExit(process.returncode)
        notice("warning", "cargo finished without reporting the runtime build script")
        return None
    match = WASM_INCLUDE.search(
        (out_dir / "wasm_binary.rs").read_text(encoding="utf-8")
    )
    if match is None:
        notice("warning", f"no runtime wasm recorded in {out_dir / 'wasm_binary.rs'}")
        return None
    wasm = Path(match.group(1))
    return wasm if wasm.is_file() else None


def main() -> int:
    parser = argparse.ArgumentParser(usage=__doc__.splitlines()[2])
    parser.add_argument(
        "--node", type=Path, required=True, help="where the job expects node-subtensor"
    )
    parser.add_argument(
        "--base-node", type=Path, help="verified node binary from the base commit"
    )
    parser.add_argument("cargo", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    cargo = args.cargo[1:] if args.cargo[:1] == ["--"] else args.cargo
    if not cargo or cargo[0] != "cargo":
        parser.error("expected `-- cargo ...`")

    if args.base_node is None:
        return full_build(cargo)
    if not args.base_node.is_file():
        notice("warning", f"base node {args.base_node} is missing; building the node")
        return full_build(cargo)

    started = time.monotonic()
    wasm = build_runtime_wasm(cargo)
    if wasm is None:
        return full_build(cargo)
    wasm_seconds = time.monotonic() - started

    verify = subprocess.run(
        [
            sys.executable,
            str(VERIFIER),
            "--node",
            str(args.base_node),
            "--wasm",
            str(wasm),
        ],
        check=False,
    )
    if verify.returncode != 0:
        notice(
            "warning",
            "the base node cannot stand in for this revision; building the node",
        )
        return full_build(cargo)

    args.node.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(args.base_node, args.node)
    args.node.chmod(0o755)
    finish(
        "base",
        f"Reused the base commit's node; built only the runtime wasm in {wasm_seconds:.0f}s "
        f"and verified it against the base node ({wasm})",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
