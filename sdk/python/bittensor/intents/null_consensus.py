"""Null mining: PoW admission, averaged validator scores, and reward claims."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from time import monotonic
from typing import Any

from eth_utils import keccak

from .._generated import calls
from ..result import BittensorError
from ..sp_core import ss58_decode
from .base import Intent
from .registry import register


def pow_seal(
    block_hash: bytes, hotkey: bytes, nonce: int, *, netuid: int, generation: int, coldkey: bytes
) -> bytes:
    """Match the runtime domain-separated SCALE seal; bind the payout recipient."""
    if any(len(key) != 32 for key in (block_hash, hotkey, coldkey)):
        raise BittensorError("PoW requires 32-byte block and account keys.")
    return keccak(
        b"subtensor:null:equal:v1"
        + netuid.to_bytes(2, "little")
        + generation.to_bytes(8, "little")
        + block_hash
        + nonce.to_bytes(8, "little")
        + hotkey
        + coldkey
    )


def _search_work(
    block_hash: bytes,
    hotkey: bytes,
    difficulty: int,
    start: int,
    netuid: int,
    generation: int,
    coldkey: bytes,
):
    for nonce in range(start, min(start + 4096, 1 << 64)):
        seal = pow_seal(
            block_hash, hotkey, nonce, netuid=netuid, generation=generation, coldkey=coldkey
        )
        if int.from_bytes(seal, "little") * difficulty < 1 << 256:
            return nonce, seal
    return None


@register
@dataclass
class PowRegister(Intent):
    """Solve fresh PoW and register a miner hotkey on a null subnet.

    Signed by the coldkey; no burn or collateral is charged. Normal transaction
    fees apply. Work is refreshed as the head advances, with a bounded timeout.
    Signed nonempty work binds PoW-only admission: disabling null consensus before
    inclusion rejects the request instead of converting it to paid registration.
    If signing takes longer than the chain's three-block work window, retry.
    Pass hotkey_ss58 when using a proxy or an account without a wallet hotkey.
    """

    op = "pow_register"
    wraps = (("SubtensorModule", "register"),)
    netuid: int = field(metadata={"help": "Null-consensus subnet to join."})
    timeout_seconds: int = field(default=120, metadata={"help": "Maximum time to solve work."})
    hotkey_ss58: str | None = field(
        default=None,
        metadata={"help": "Miner hotkey; defaults to wallet hotkey. Required for proxy calls."},
    )

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
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        public_key = bytes(ss58_decode(hotkey))
        coldkey = self.coldkey_address(wallet)
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
            generation = int(
                await substrate.query(
                    "SubtensorModule",
                    "RegisteredSubnetCounter",
                    [self.netuid],
                    block_hash=block_hash,
                )
                or 0
            )
            nonce = 0
            while monotonic() < deadline:
                solution = await asyncio.to_thread(
                    _search_work,
                    bytes.fromhex(block_hash.removeprefix("0x")),
                    public_key,
                    difficulty,
                    nonce,
                    self.netuid,
                    generation,
                    bytes(ss58_decode(coldkey)),
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
                            hotkey=hotkey,
                            coldkey=coldkey,
                        )
                    )
                nonce += 4096
                if nonce >= 1 << 64:
                    break
        raise BittensorError("PoW registration timed out; try again or check subnet difficulty.")

    def summary(self) -> str:
        hotkey = self.hotkey_ss58 or "wallet hotkey"
        return f"register {hotkey} on netuid {self.netuid} with PoW (no burn or collateral)"


@register
@dataclass
class SetNullWeights(Intent):
    """Submit exact u32 scores for u64 miners; eligible rows are averaged equally."""

    op = "set_null_weights"
    signer = "hotkey"
    wraps = (("SubtensorModule", "set_null_weights"),)
    netuid: int = field(metadata={"help": "Null-consensus subnet to score."})
    uids: list[int] = field(metadata={"help": "Distinct miner UIDs, parallel to weights."})
    weights: list[int] = field(metadata={"help": "Exact relative integers from 0 to 4294967295."})
    version_key: int = field(default=0, metadata={"help": "Required subnet weights version."})

    def __post_init__(self):
        if len(self.uids) != len(self.weights) or not 1 <= len(self.uids) <= 8192:
            raise BittensorError("Supply 1 to 8192 miner UIDs with one score per UID.")
        if len(set(self.uids)) != len(self.uids):
            raise BittensorError("Miner UIDs must be distinct.")
        if any(type(uid) is not int or not 0 <= uid < 1 << 64 for uid in self.uids):
            raise BittensorError("Miner UIDs must be u64 integers.")
        if any(type(weight) is not int or not 0 <= weight < 1 << 32 for weight in self.weights):
            raise BittensorError("Scores must be u32 integers.")
        if not any(self.weights):
            raise BittensorError("At least one score must be nonzero.")

    async def build(self, substrate, wallet: Any):
        if not await substrate.query("SubtensorModule", "NullConsensus", [self.netuid]):
            raise BittensorError("Enable null consensus before submitting null weights.")
        return await substrate.compose(
            calls.SubtensorModule.set_null_weights(
                netuid=self.netuid,
                dests=self.uids,
                weights=self.weights,
                version_key=self.version_key,
            )
        )

    def summary(self) -> str:
        return f"set null weights for {len(self.uids)} miners on netuid {self.netuid}"


@register
@dataclass
class ClaimNullRewards(Intent):
    """Claim a miner's scored alpha rewards into the coldkey's staking position.

    The destination hotkey must already be a staking account. The claiming coldkey
    owns the resulting alpha; using one destination consolidates many mining keys.
    Omit stake_hotkey to use the subnet owner's hotkey. Claims also work in Yuma mode.
    Pass hotkey_ss58 when using a proxy or an account without a wallet hotkey.
    """

    op = "claim_null_rewards"
    wraps = (("SubtensorModule", "claim_null_rewards"),)
    netuid: int = field(metadata={"help": "Subnet whose miner rewards to claim."})
    stake_hotkey: str | None = field(
        default=None, metadata={"help": "Existing staking hotkey; defaults to subnet owner hotkey."}
    )
    hotkey_ss58: str | None = field(
        default=None,
        metadata={"help": "Miner hotkey; defaults to wallet hotkey. Required for proxy calls."},
    )

    async def build(self, substrate, wallet: Any):
        target = self.stake_hotkey or await substrate.query(
            "SubtensorModule", "SubnetOwnerHotkey", [self.netuid]
        )
        return await substrate.compose(
            calls.SubtensorModule.claim_null_rewards(
                netuid=self.netuid,
                hotkey=self.hotkey_address(wallet, self.hotkey_ss58),
                stake_hotkey=target,
            )
        )

    def summary(self) -> str:
        return f"claim scored miner rewards on netuid {self.netuid}"
