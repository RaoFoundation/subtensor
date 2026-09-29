"""Null-consensus setup, low-difficulty registration, and full u32 scores."""

from __future__ import annotations

import asyncio
import hashlib
from dataclasses import dataclass, field
from time import monotonic
from typing import Any

from eth_utils import keccak

from .._generated import calls
from ..result import BittensorError
from .base import Intent
from .registry import register


def pow_seal(block_hash: bytes, hotkey: bytes, nonce: int) -> bytes:
    """Match runtime create_seal_hash, including little-endian nonce/difficulty."""
    if len(block_hash) != 32 or len(hotkey) != 32:
        raise BittensorError("PoW requires a 32-byte block hash and hotkey.")
    return keccak(
        hashlib.sha256(nonce.to_bytes(8, "little") + keccak(block_hash + hotkey)).digest()
    )


def _search_work(block_hash: bytes, hotkey: bytes, difficulty: int, start: int):
    prefix = keccak(block_hash + hotkey)
    for nonce in range(start, min(start + 4096, 1 << 64)):
        seal = keccak(hashlib.sha256(nonce.to_bytes(8, "little") + prefix).digest())
        if int.from_bytes(seal, "little") * difficulty < 1 << 256:
            return nonce, seal
    return None


@register
@dataclass
class PowRegister(Intent):
    """Solve fresh PoW and register the wallet hotkey on a null subnet.

    Signed by the coldkey; no burn or collateral is charged. Normal transaction
    fees apply. Work is refreshed as the head advances, with a bounded timeout.
    Signed nonempty work binds PoW-only admission: disabling null consensus before
    inclusion rejects the request instead of converting it to paid registration.
    If signing takes longer than the chain's three-block work window, retry.
    """

    op = "pow_register"
    wraps = (("SubtensorModule", "register"),)
    netuid: int = field(metadata={"help": "Null-consensus subnet to join."})
    timeout_seconds: int = field(default=120, metadata={"help": "Maximum time to solve work."})

    async def build(self, substrate, wallet: Any):
        if self.timeout_seconds <= 0:
            raise BittensorError("PoW timeout must be positive.")
        enabled, allowed, pow_allowed = await asyncio.gather(
            substrate.query("SubtensorModule", "NullConsensus", [self.netuid]),
            substrate.query("SubtensorModule", "NetworkRegistrationAllowed", [self.netuid]),
            substrate.query("SubtensorModule", "NetworkPowRegistrationAllowed", [self.netuid]),
        )
        if not enabled:
            raise BittensorError("PoW registration requires an enabled null-consensus subnet.")
        if not allowed:
            raise BittensorError(
                "Subnet registration is paused; ask the owner to enable NetworkRegistrationAllowed."
            )
        if not pow_allowed:
            raise BittensorError(
                "PoW registration is paused; ask the owner to enable NetworkPowRegistrationAllowed."
            )
        deadline = monotonic() + self.timeout_seconds
        public_key = self.hotkey_public_key(wallet)
        while monotonic() < deadline:
            # The runtime only has hashes for completed blocks when dispatching.
            block = await substrate.block_number()
            block_hash = await substrate.block_hash(block)
            difficulty = max(
                1,
                int(
                    await substrate.query(
                        "SubtensorModule", "Difficulty", [self.netuid], block_hash=block_hash
                    )
                ),
            )
            nonce = 0
            while monotonic() < deadline:
                solution = await asyncio.to_thread(
                    _search_work,
                    bytes.fromhex(block_hash.removeprefix("0x")),
                    public_key,
                    difficulty,
                    nonce,
                )
                if await substrate.block_number() != block:
                    break
                if solution is not None:
                    nonce, work = solution
                    return await substrate.compose(
                        calls.SubtensorModule.register(
                            netuid=self.netuid,
                            block_number=block,
                            nonce=nonce,
                            work=list(work),
                            hotkey=self.hotkey_address(wallet),
                            coldkey=self.coldkey_address(wallet),
                        )
                    )
                nonce += 4096
                if nonce >= 1 << 64:
                    break
        raise BittensorError("PoW registration timed out; try again or check subnet difficulty.")

    def summary(self) -> str:
        return f"register wallet hotkey on netuid {self.netuid} with PoW (no burn or collateral)"


@register
@dataclass
class SetNullWeights(Intent):
    """Submit exact relative u32 scores for null consensus, without u16 quantization."""

    op = "set_null_weights"
    signer = "hotkey"
    wraps = (("SubtensorModule", "set_null_weights"),)
    netuid: int = field(metadata={"help": "Null-consensus subnet to score."})
    uids: list[int] = field(metadata={"help": "Distinct miner UIDs, parallel to weights."})
    weights: list[int] = field(metadata={"help": "Exact relative integers from 0 to 4294967295."})
    version_key: int = field(default=0, metadata={"help": "Required subnet weights version."})

    def __post_init__(self):
        if len(self.uids) != len(self.weights) or not 0 < len(self.uids) <= 32768:
            raise BittensorError("Provide 1–32768 parallel UIDs and u32 weights.")
        if any(type(u) is not int or not 0 <= u < 32768 for u in self.uids):
            raise BittensorError("Null-consensus UIDs must be integers from 0 to 32767.")
        if len(set(self.uids)) != len(self.uids):
            raise BittensorError("UIDs must be distinct.")
        if any(type(w) is not int or not 0 <= w <= 0xFFFFFFFF for w in self.weights):
            raise BittensorError("Weights must be exact unsigned 32-bit integers.")
        if not any(self.weights):
            raise BittensorError("At least one weight must be positive.")

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_null_weights(
                netuid=self.netuid,
                dests=self.uids,
                weights=self.weights,
                version_key=self.version_key,
            )
        )

    def summary(self) -> str:
        return f"set u32 null-consensus scores for {len(self.uids)} miners on netuid {self.netuid}"
