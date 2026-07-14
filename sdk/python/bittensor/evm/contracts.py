"""User-contract artifacts: load compiled output, encode deploys and calls.

The precompile catalog covers the chain's own contracts; this module covers
*yours*. It reads the artifact shapes the common toolchains emit — a Hardhat
or Foundry artifact JSON, a bare ABI JSON array, or a raw ``.bin`` hex file —
so `btcli evm deploy` and `btcli evm call --abi` work with whatever the
compiler produced, without a web3 dependency.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from eth_abi import encode as abi_encode

from .precompiles import _canonical_type, coerce_argument


@dataclass(frozen=True)
class ContractArtifact:
    """A compiled contract: ABI and (for deployable artifacts) init bytecode."""

    abi: "list[dict] | None"
    bytecode: "str | None"  # 0x-hex init code, when the artifact carries it
    source: str  # the file it came from, for error messages

    def functions(self) -> list[dict]:
        return [e for e in (self.abi or []) if e.get("type") == "function"]

    def function(self, name: str) -> dict:
        for entry in self.functions():
            if entry["name"] == name:
                return entry
        available = ", ".join(sorted(e["name"] for e in self.functions())) or "(none)"
        raise ValueError(f"{self.source} has no function {name!r}. Available: {available}")

    def constructor(self) -> "dict | None":
        for entry in self.abi or []:
            if entry.get("type") == "constructor":
                return entry
        return None


def _extract_bytecode(payload: Any) -> "str | None":
    """The init code from an artifact's ``bytecode`` field.

    Hardhat emits a hex string; Foundry emits ``{"object": "0x…"}``.
    """
    if isinstance(payload, str) and payload not in ("", "0x"):
        return payload if payload.startswith("0x") else "0x" + payload
    if isinstance(payload, dict):
        return _extract_bytecode(payload.get("object"))
    return None


def load_artifact(path: "str | Path") -> ContractArtifact:
    """Read a contract artifact: Hardhat/Foundry JSON, ABI array, or .bin hex."""
    file = Path(path).expanduser()
    text = file.read_text().strip()
    source = str(file)

    if not text.lstrip().startswith(("{", "[")):
        # a bare .bin file: hex init code, no ABI
        code = text.replace("\n", "")
        return ContractArtifact(
            abi=None,
            bytecode=code if code.startswith("0x") else "0x" + code,
            source=source,
        )

    payload = json.loads(text)
    if isinstance(payload, list):
        # a bare ABI array (e.g. extracted with jq or from a verification page)
        return ContractArtifact(abi=payload, bytecode=None, source=source)

    abi = payload.get("abi")
    bytecode = _extract_bytecode(payload.get("bytecode"))
    if abi is None and bytecode is None:
        raise ValueError(
            f"{source} is JSON but has neither an 'abi' nor a 'bytecode' field — "
            "expected a Hardhat/Foundry artifact, an ABI array, or a .bin file"
        )
    return ContractArtifact(abi=abi, bytecode=bytecode, source=source)


def encode_deploy(artifact: ContractArtifact, args: "list[Any]") -> str:
    """Init code plus ABI-encoded constructor arguments, as 0x-hex calldata."""
    if not artifact.bytecode:
        raise ValueError(f"{artifact.source} carries no bytecode — pass the compiler artifact")
    ctor = artifact.constructor()
    inputs = ctor["inputs"] if ctor else []
    if len(args) != len(inputs):
        names = ", ".join(f"{i['type']} {i['name']}" for i in inputs) or "(none)"
        raise ValueError(f"constructor expects {len(inputs)} argument(s): {names}")
    if not inputs:
        return artifact.bytecode
    types = [_canonical_type(i) for i in inputs]
    coerced = [coerce_argument(t, a) for t, a in zip(types, args)]
    return artifact.bytecode + abi_encode(types, coerced).hex()
