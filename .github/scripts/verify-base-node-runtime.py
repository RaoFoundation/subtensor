#!/usr/bin/env python3
"""Prove a prebuilt node can stand in for a freshly built one with a new runtime.

classify-node-changes.sh already guarantees the node's own sources match the
base commit. What remains is the runtime-side code the node compiles natively.
This check boots the base node twice, once with its embedded runtime and once
with the candidate wasm through SUBTENSOR_LOCALNET_RUNTIME_WASM, and requires:

1. The candidate wasm is the genesis `:code` (the node honours the override).
2. Every runtime API version the base runtime exposes is unchanged.
3. Every runtime API method the base runtime exposes keeps the same inputs and
   output type structure, including field and variant names and indices. The
   node decodes these results with types compiled from the base commit.
4. `BabeApi_configuration` is unchanged. The node reads its staged BABE
   constants natively once the runtime reports BABE authorities.

Exit status 0 means the base node is equivalent for E2E purposes; any other
status means the caller must build the node.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

OVERRIDE_ENV = "SUBTENSOR_LOCALNET_RUNTIME_WASM"
CODE_KEY = "0x" + b":code".hex()
METADATA_V15 = "0x0f000000"
RPC_TIMEOUT_SECONDS = 180


class Scale:
    """Minimal SCALE reader for the parts of RuntimeMetadataV15 used here."""

    def __init__(self, data: bytes) -> None:
        self.data = data
        self.pos = 0

    def take(self, n: int) -> bytes:
        if self.pos + n > len(self.data):
            raise ValueError("metadata ended early")
        chunk = self.data[self.pos : self.pos + n]
        self.pos += n
        return chunk

    def u8(self) -> int:
        return self.take(1)[0]

    def u32(self) -> int:
        return int.from_bytes(self.take(4), "little")

    def compact(self) -> int:
        first = self.u8()
        mode = first & 3
        if mode == 0:
            return first >> 2
        if mode == 1:
            return (first | self.u8() << 8) >> 2
        if mode == 2:
            return int.from_bytes(bytes([first]) + self.take(3), "little") >> 2
        return int.from_bytes(self.take((first >> 2) + 4), "little")

    def text(self) -> str:
        return self.take(self.compact()).decode("utf-8")

    def blob(self) -> bytes:
        return self.take(self.compact())

    def vec(self, item):
        return [item() for _ in range(self.compact())]

    def option(self, item):
        tag = self.u8()
        if tag == 0:
            return None
        if tag == 1:
            return item()
        raise ValueError(f"invalid Option tag {tag}")


def decode_metadata_v15(
    raw: bytes,
) -> tuple[dict[int, tuple], dict[str, dict[str, tuple]]]:
    """Return (type registry, runtime APIs) from Metadata_metadata_at_version(15)."""
    outer = Scale(raw)
    if outer.u8() != 1:
        raise ValueError("runtime does not provide metadata V15")
    s = Scale(outer.blob())
    if s.take(4) != b"meta":
        raise ValueError("metadata magic mismatch")
    if s.u8() != 15:
        raise ValueError("expected RuntimeMetadata::V15")

    def field():
        return (s.option(s.text), s.compact(), s.option(s.text), s.vec(s.text))

    def type_def():
        kind = s.u8()
        if kind == 0:
            return ("composite", tuple((name, ty) for name, ty, _, _ in s.vec(field)))
        if kind == 1:

            def variant():
                name = s.text()
                fields = tuple((fname, ty) for fname, ty, _, _ in s.vec(field))
                index = s.u8()
                s.vec(s.text)
                return (index, name, fields)

            return ("variant", tuple(sorted(s.vec(variant))))
        if kind == 2:
            return ("sequence", s.compact())
        if kind == 3:
            length = s.u32()
            return ("array", length, s.compact())
        if kind == 4:
            return ("tuple", tuple(s.vec(s.compact)))
        if kind == 5:
            return ("primitive", s.u8())
        if kind == 6:
            return ("compact", s.compact())
        if kind == 7:
            return ("bitsequence", s.compact(), s.compact())
        raise ValueError(f"unknown TypeDef {kind}")

    registry: dict[int, tuple] = {}
    for _ in range(s.compact()):
        type_id = s.compact()
        s.vec(s.text)  # path
        s.vec(lambda: (s.text(), s.option(s.compact)))  # type params
        registry[type_id] = type_def()
        s.vec(s.text)  # docs

    def storage_entry():
        s.text()
        s.u8()
        kind = s.u8()
        if kind == 0:
            s.compact()
        elif kind == 1:
            s.vec(s.u8)
            s.compact()
            s.compact()
        else:
            raise ValueError(f"unknown StorageEntryType {kind}")
        s.blob()
        s.vec(s.text)

    def pallet():
        s.text()
        s.option(lambda: (s.text(), s.vec(storage_entry)))
        s.option(s.compact)  # calls
        s.option(s.compact)  # event
        s.vec(lambda: (s.text(), s.compact(), s.blob(), s.vec(s.text)))
        s.option(s.compact)  # error
        s.u8()
        s.vec(s.text)

    s.vec(pallet)
    s.u8()  # extrinsic version
    for _ in range(4):  # address, call, signature, extra
        s.compact()
    s.vec(lambda: (s.text(), s.compact(), s.compact()))
    s.compact()  # runtime type

    apis: dict[str, dict[str, tuple]] = {}
    for _ in range(s.compact()):
        api = s.text()
        methods = {}
        for _ in range(s.compact()):
            method = s.text()
            inputs = tuple(s.vec(lambda: (s.text(), s.compact())))
            output = s.compact()
            s.vec(s.text)
            methods[method] = (inputs, output)
        s.vec(s.text)
        apis[api] = methods
    return registry, apis


def type_shape(registry: dict[int, tuple], root: int) -> str:
    """Hash the type graph reachable from `root` independent of registry ids."""
    order = {root: 0}
    queue = [root]
    nodes = []

    def ref(type_id: int) -> int:
        if type_id not in order:
            order[type_id] = len(queue)
            queue.append(type_id)
        return order[type_id]

    index = 0
    while index < len(queue):
        definition = registry[queue[index]]
        index += 1
        kind = definition[0]
        if kind == "composite":
            nodes.append([kind, [[name, ref(ty)] for name, ty in definition[1]]])
        elif kind == "variant":
            nodes.append(
                [
                    kind,
                    [
                        [position, name, [[fname, ref(ty)] for fname, ty in fields]]
                        for position, name, fields in definition[1]
                    ],
                ]
            )
        elif kind in ("sequence", "compact"):
            nodes.append([kind, ref(definition[1])])
        elif kind == "array":
            nodes.append([kind, definition[1], ref(definition[2])])
        elif kind == "tuple":
            nodes.append([kind, [ref(ty) for ty in definition[1]]])
        elif kind == "primitive":
            nodes.append([kind, definition[1]])
        else:
            nodes.append([kind, ref(definition[1]), ref(definition[2])])
    return hashlib.sha256(json.dumps(nodes, separators=(",", ":")).encode()).hexdigest()


def api_signatures(raw_metadata: bytes) -> dict[str, str]:
    registry, apis = decode_metadata_v15(raw_metadata)
    signatures = {}
    for api, methods in apis.items():
        for method, (inputs, output) in methods.items():
            shape = {
                "inputs": [[name, type_shape(registry, ty)] for name, ty in inputs],
                "output": type_shape(registry, output),
            }
            signatures[f"{api}_{method}"] = json.dumps(shape, sort_keys=True)
    return signatures


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def rpc(port: int, method: str, params: list) -> dict:
    body = json.dumps(
        {"id": 1, "jsonrpc": "2.0", "method": method, "params": params}
    ).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def build_spec(node: Path, env: dict[str, str], destination: Path) -> dict:
    with destination.open("w", encoding="utf-8") as out:
        subprocess.run(
            [
                str(node),
                "build-spec",
                "--chain",
                "local",
                "--raw",
                "--disable-default-bootnode",
            ],
            env=env,
            stdout=out,
            stderr=subprocess.PIPE,
            check=True,
            timeout=600,
        )
    return json.loads(destination.read_text(encoding="utf-8"))


def query_runtime(node: Path, spec: Path, workdir: Path, label: str) -> dict:
    rpc_port = free_port()
    log_path = workdir / f"{label}.log"
    args = [
        str(node),
        "--chain",
        str(spec),
        "--base-path",
        str(workdir / f"{label}-db"),
        "--rpc-port",
        str(rpc_port),
        "--port",
        str(free_port()),
        "--no-prometheus",
        "--no-telemetry",
        "--no-mdns",
        "--unsafe-force-node-key-generation",
    ]
    env = {k: v for k, v in os.environ.items() if k != OVERRIDE_ENV}
    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT, env=env)
    try:
        deadline = time.monotonic() + RPC_TIMEOUT_SECONDS
        while True:
            if process.poll() is not None:
                raise RuntimeError(
                    f"{label} node exited early:\n{log_path.read_text()[-4000:]}"
                )
            try:
                genesis = rpc(rpc_port, "chain_getBlockHash", [0])["result"]
                break
            except (urllib.error.URLError, ConnectionError, OSError, KeyError):
                if time.monotonic() > deadline:
                    raise RuntimeError(f"{label} node RPC never became ready")
                time.sleep(1)
        version = rpc(rpc_port, "state_getRuntimeVersion", [genesis])["result"]
        metadata = rpc(
            rpc_port,
            "state_call",
            ["Metadata_metadata_at_version", METADATA_V15, genesis],
        )
        babe = rpc(rpc_port, "state_call", ["BabeApi_configuration", "0x", genesis])
        return {
            "version": version,
            "signatures": api_signatures(bytes.fromhex(metadata["result"][2:])),
            "babe": babe.get("result", babe.get("error")),
        }
    finally:
        process.terminate()
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def compare(base: dict, candidate: dict) -> list[str]:
    problems = []
    candidate_apis = {
        api_id: version for api_id, version in candidate["version"]["apis"]
    }
    for api_id, version in base["version"]["apis"]:
        if candidate_apis.get(api_id) != version:
            problems.append(
                f"runtime API {api_id} version {version} -> {candidate_apis.get(api_id, 'removed')}"
            )

    base_signatures = base["signatures"]
    candidate_signatures = candidate["signatures"]
    for method, signature in sorted(base_signatures.items()):
        if method not in candidate_signatures:
            problems.append(f"runtime API method {method} removed")
        elif candidate_signatures[method] != signature:
            problems.append(
                f"runtime API method {method} changed its input or output types"
            )

    if base["babe"] != candidate["babe"]:
        problems.append("BabeApi_configuration changed")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--node", type=Path, required=True, help="prebuilt base node binary"
    )
    parser.add_argument(
        "--wasm", type=Path, required=True, help="candidate runtime wasm"
    )
    args = parser.parse_args()

    wasm = args.wasm.resolve()
    workdir = Path(tempfile.mkdtemp(prefix="verify-base-node-"))
    try:
        base_env = {k: v for k, v in os.environ.items() if k != OVERRIDE_ENV}
        base_spec = workdir / "base.json"
        candidate_spec = workdir / "candidate.json"
        build_spec(args.node, base_env, base_spec)
        spec = build_spec(
            args.node, {**base_env, OVERRIDE_ENV: str(wasm)}, candidate_spec
        )
        code = spec["genesis"]["raw"]["top"].get(CODE_KEY, "")
        if code != "0x" + wasm.read_bytes().hex():
            print(
                f"base node ignores {OVERRIDE_ENV}; it predates runtime overrides",
                file=sys.stderr,
            )
            return 1

        base = query_runtime(args.node, base_spec, workdir, "base")
        candidate = query_runtime(args.node, candidate_spec, workdir, "candidate")
        problems = compare(base, candidate)
        base_version = base["version"]["specVersion"]
        candidate_version = candidate["version"]["specVersion"]
        if problems:
            print(
                f"base node cannot run runtime {candidate_version} (base runtime {base_version}):",
                file=sys.stderr,
            )
            for problem in problems:
                print(f"  - {problem}", file=sys.stderr)
            return 1
        print(
            f"base node is compatible with runtime {candidate_version} (base runtime {base_version}): "
            f"{len(base['version']['apis'])} runtime API versions and "
            f"{len(base['signatures'])} method signatures unchanged"
        )
        return 0
    except (
        OSError,
        ValueError,
        KeyError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        print(f"base node verification failed: {error}", file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
