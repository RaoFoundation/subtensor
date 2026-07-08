"""The Bittensor EVM precompile catalog: addresses + ABIs as data.

Precompiles are the chain's native operations exposed as EVM contracts at
fixed addresses. The ABIs here are vendored from the subtensor repository
(``precompiles/src/solidity/*.abi``) and the addresses from each precompile's
``INDEX`` constant, so Hardhat/ethers/viem users can pull canonical artifacts
from this package instead of hunting through Rust sources — and the CLI can
encode calls without a web3 dependency.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Any

try:
    from eth_abi import decode as abi_decode
    from eth_abi import encode as abi_encode
    from eth_utils import function_abi_to_4byte_selector
except ImportError:  # pragma: no cover - exercised only without the extra
    abi_decode = None  # type: ignore[assignment]
    abi_encode = None  # type: ignore[assignment]
    function_abi_to_4byte_selector = None  # type: ignore[assignment]

from .addresses import is_h160, ss58_to_pubkey
from .keys import require_eth_account

_ABI_DIR = Path(__file__).parent / "abi"


@dataclass(frozen=True)
class Precompile:
    name: str  # CLI-facing name (kebab-case)
    index: int  # the runtime's INDEX constant
    abi_file: str
    description: str
    deprecated: bool = False

    @property
    def address(self) -> str:
        """The h160 the precompile lives at (INDEX as a low-u64-be address)."""
        return "0x" + self.index.to_bytes(20, "big").hex()

    @property
    def abi(self) -> list[dict]:
        return _load_abi(self.abi_file)

    def functions(self) -> list[dict]:
        return [entry for entry in self.abi if entry.get("type") == "function"]

    def function(self, name: str) -> dict:
        for entry in self.functions():
            if entry["name"] == name:
                return entry
        available = ", ".join(sorted(entry["name"] for entry in self.functions()))
        raise ValueError(f"{self.name} has no function {name!r}. Available: {available}")


@cache
def _load_abi(filename: str) -> list[dict]:
    return json.loads((_ABI_DIR / filename).read_text())


# Addresses are each precompile's INDEX in subtensor's precompiles/src/*.rs.
PRECOMPILES: dict[str, Precompile] = {
    p.name: p
    for p in (
        Precompile(
            "balance-transfer",
            2048,
            "balanceTransfer.json",
            "Send TAO from an EVM account to any ss58 address (payable transfer(bytes32 pubkey)).",
        ),
        Precompile(
            "staking",
            2049,
            "staking.json",
            "Staking V1 — deprecated; amounts ride msg.value. Use staking-v2.",
            deprecated=True,
        ),
        Precompile(
            "metagraph",
            2050,
            "metagraph.json",
            "Read-only neuron/subnet state: stake, ranks, trust, axons, keys by (netuid, uid).",
        ),
        Precompile(
            "subnet",
            2051,
            "subnet.json",
            "Subnet registration and owner hyperparameter get/set pairs.",
        ),
        Precompile(
            "neuron",
            2052,
            "neuron.json",
            "Neuron lifecycle: burned registration, weights (direct and commit-reveal), serving.",
        ),
        Precompile(
            "staking-v2",
            2053,
            "stakingV2.json",
            "Staking V2: add/remove/move/transfer stake with explicit rao amounts.",
        ),
        Precompile(
            "uid-lookup",
            2054,
            "uidLookup.json",
            "Look up the uid(s) associated with an EVM address on a subnet.",
        ),
        Precompile(
            "alpha",
            2056,
            "alpha.json",
            "Subnet alpha token info: prices, pool reserves, and alpha amounts.",
        ),
        Precompile(
            "crowdloan",
            2057,
            "crowdloan.json",
            "Crowdloan operations from EVM.",
        ),
        Precompile(
            "leasing",
            2058,
            "leasing.json",
            "Subnet leasing operations from EVM.",
        ),
        Precompile(
            "proxy",
            2059,
            "proxy.json",
            "Add/remove proxy delegations from EVM.",
        ),
        Precompile(
            "address-mapping",
            2060,
            "addressMapping.json",
            "The chain's own h160 -> ss58-mirror mapping (addressMapping(address) -> bytes32).",
        ),
        Precompile(
            "ed25519-verify",
            1026,
            "ed25519Verify.json",
            "Verify an ed25519 signature on-chain (prove ss58 key ownership from EVM).",
        ),
        Precompile(
            "sr25519-verify",
            1027,
            "sr25519Verify.json",
            "Verify an sr25519 signature on-chain.",
        ),
    )
}

# Standard Ethereum precompiles also live on the Bittensor EVM (no ABI needed
# here; listed for discovery/doctor output).
STANDARD_PRECOMPILES: dict[str, int] = {
    "ecrecover": 1,
    "sha256": 2,
    "ripemd160": 3,
    "identity": 4,
    "modexp": 5,
    "dispatch": 6,
    "bn128-mul": 7,
    "bn128-pairing": 8,
    "bn128-add": 9,
    "sha3-fips256": 1024,
    "ecrecover-publickey": 1025,
}


def get_precompile(name: str) -> Precompile:
    try:
        return PRECOMPILES[name]
    except KeyError:
        raise ValueError(
            f"unknown precompile {name!r}. Available: {', '.join(sorted(PRECOMPILES))}"
        ) from None


def coerce_argument(abi_type: str, raw: Any) -> Any:
    """Parse one CLI-shaped argument into what the ABI type expects.

    ``bytes32`` accepts an ss58 address (converted to its public key) or
    0x-hex — precompile interfaces take hotkeys/coldkeys as ``bytes32``, and
    nobody should have to run the conversion by hand.
    """
    text = str(raw).strip()
    if abi_type.startswith(("uint", "int")):
        return int(text, 16 if text.startswith("0x") else 10)
    if abi_type == "bool":
        return text.lower() in ("1", "true", "yes", "y")
    if abi_type == "address":
        return text
    if abi_type.startswith("bytes"):
        if not text.startswith("0x") and not is_h160("0x" + text):
            # ss58 -> 32-byte public key, the shape hotkey/coldkey params take
            return bytes.fromhex(ss58_to_pubkey(text)[2:])
        return bytes.fromhex(text.removeprefix("0x"))
    if abi_type.endswith("[]"):
        inner = abi_type[:-2]
        parts = json.loads(text) if text.startswith("[") else text.split(",")
        return [coerce_argument(inner, part) for part in parts]
    return text


def encode_call(fn_abi: dict, args: "list[Any]") -> str:
    """ABI-encode a function call (selector + arguments) as 0x-hex calldata."""
    require_eth_account()
    types = [_canonical_type(i) for i in fn_abi["inputs"]]
    if len(args) != len(types):
        names = ", ".join(f"{i['type']} {i['name']}" for i in fn_abi["inputs"])
        raise ValueError(f"{fn_abi['name']} expects {len(types)} argument(s): ({names})")
    coerced = [coerce_argument(t, a) for t, a in zip(types, args)]
    selector = function_abi_to_4byte_selector(fn_abi)
    return "0x" + (selector + abi_encode(types, coerced)).hex()


def decode_result(fn_abi: dict, data: "str | bytes") -> "list[Any]":
    """Decode an eth_call result according to the function's output types."""
    require_eth_account()
    raw = bytes.fromhex(data.removeprefix("0x")) if isinstance(data, str) else bytes(data)
    types = [_canonical_type(o) for o in fn_abi.get("outputs", [])]
    if not types or not raw:
        return []
    return [_jsonable(value) for value in abi_decode(types, raw)]


def _canonical_type(param: dict) -> str:
    """The ABI type string for encoding, expanding tuple components."""
    abi_type = param["type"]
    if abi_type.startswith("tuple"):
        inner = ",".join(_canonical_type(c) for c in param["components"])
        return f"({inner})" + abi_type[len("tuple") :]
    return abi_type


def _jsonable(value: Any) -> Any:
    if isinstance(value, bytes):
        return "0x" + value.hex()
    if isinstance(value, (list, tuple)):
        return [_jsonable(item) for item in value]
    return value
