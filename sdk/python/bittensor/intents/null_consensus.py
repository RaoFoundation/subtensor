"""Equal-emission null mining: cheap registration and lazy reward claims."""

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
                    bytes(ss58_decode(self.coldkey_address(wallet))),
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


@dataclass
class SetNullWeights(Intent):
    """Retired scoring intent. Null mode pays every registered miner equally."""

    op = "set_null_weights"
    signer = "hotkey"
    wraps = (("SubtensorModule", "set_null_weights"),)
    netuid: int = field(metadata={"help": "Null-consensus subnet to score."})
    uids: list[int] = field(metadata={"help": "Distinct miner UIDs, parallel to weights."})
    weights: list[int] = field(metadata={"help": "Exact relative integers from 0 to 4294967295."})
    version_key: int = field(default=0, metadata={"help": "Required subnet weights version."})

    def __post_init__(self):
        raise BittensorError(
            "Null mode pays all miners equally and has no weights. "
            "Use ClaimNullRewards or btcli pow claim."
        )

    async def build(self, substrate, wallet: Any):
        raise BittensorError("Null mode has no weights.")

    def summary(self) -> str:
        return "retired: null mode has no weights"


@register
@dataclass
class ClaimNullRewards(Intent):
    """Claim a miner's equal alpha rewards into the coldkey's staking position.

    The destination hotkey must already be a staking account. The claiming coldkey
    owns the resulting alpha; using one destination consolidates many mining keys.
    Omit stake_hotkey to use the subnet owner's hotkey. Claims also work in Yuma mode.
    """

    op = "claim_null_rewards"
    wraps = (("SubtensorModule", "claim_null_rewards"),)
    netuid: int = field(metadata={"help": "Subnet whose equal-emission rewards to claim."})
    stake_hotkey: str | None = field(
        default=None, metadata={"help": "Existing staking hotkey; defaults to subnet owner hotkey."}
    )

    async def build(self, substrate, wallet: Any):
        target = self.stake_hotkey or await substrate.query(
            "SubtensorModule", "SubnetOwnerHotkey", [self.netuid]
        )
        return await substrate.compose(
            calls.SubtensorModule.claim_null_rewards(
                netuid=self.netuid, hotkey=self.hotkey_address(wallet), stake_hotkey=target
            )
        )

    def summary(self) -> str:
        return f"claim equal miner rewards on netuid {self.netuid}"
