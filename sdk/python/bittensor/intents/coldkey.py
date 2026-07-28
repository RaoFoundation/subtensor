"""Coldkey swap: migrate everything a coldkey owns to a new coldkey.

The safe, non-deprecated flow is two steps:

  1. ``announce_coldkey_swap`` publishes only the BlakeTwo256 hash of the new
     coldkey — committing to it without revealing it — and records the block at
     which the swap becomes executable (now + the chain's announcement delay).
  2. after that delay, ``swap_coldkey_announced`` reveals the new coldkey and
     performs the swap.

An announcement can be cleared before execution, and a coldkey holder can
dispute a swap they did not initiate (freezing it for governance to resolve) —
the recovery path if a coldkey is compromised. The older one-shot
``schedule_swap_coldkey`` and the root-only ``swap_coldkey`` / ``reset_coldkey_swap``
stay raw-only on purpose.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from hashlib import blake2b
from typing import Any

from .._generated import calls
from ..sp_core import ss58_decode
from .base import Intent
from .registry import register


def coldkey_hash(ss58: str) -> str:
    """BlakeTwo256 (0x-hex) of an account's public key, as ``announce_coldkey_swap`` expects."""
    public_key = bytes(ss58_decode(ss58))
    return "0x" + blake2b(public_key, digest_size=32).hexdigest()


@register
@dataclass
class AnnounceColdkeySwap(Intent):
    """Announce (commit to) a coldkey swap; executable after the chain's delay.

    Step one of the two-step coldkey migration: publishes only the BlakeTwo256
    hash of ``new_coldkey_ss58`` — committing to the new key without revealing
    it — and starts the chain's announcement delay. After the delay, run the
    ``swap_coldkey_announced`` intent to move EVERYTHING this coldkey owns
    (balance, stake, subnets) to the new key; check timing with the
    ``coldkey_swap_announcement`` read. The first announcement charges the
    key-swap cost (0.1 TAO, recycled); re-announcing after the chain's
    reannouncement delay is free. While an announcement is pending, the chain
    blocks every other signed extrinsic from this coldkey — only the
    swap-related calls and shielded (encrypted) submission go through — so the
    account is operationally locked for the full delay. Before announcing, be
    certain you control the new coldkey and have its mnemonic backed up. A
    pending announcement can be cancelled with
    ``clear_coldkey_swap_announcement``, and the legitimate holder can freeze
    an unauthorized one with ``dispute_coldkey_swap``.
    """

    op = "announce_coldkey_swap"
    signer = "coldkey"
    wraps = (("SubtensorModule", "announce_coldkey_swap"),)

    new_coldkey_ss58: str = field(
        metadata={
            "help": "Coldkey that will take over everything this coldkey owns once the "
            "swap executes; only its hash is published now."
        }
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.announce_coldkey_swap(
                new_coldkey_hash=coldkey_hash(self.new_coldkey_ss58)
            )
        )

    def summary(self) -> str:
        return f"announce coldkey swap to {self.new_coldkey_ss58}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "after the announcement delay, swap_coldkey_announced will move EVERYTHING "
            "this coldkey owns (balance, stake, subnets) to the new coldkey",
            "make sure you control the new coldkey and have its mnemonic backed up",
        ]


@register
@dataclass
class SwapColdkeyAnnounced(Intent):
    """Execute a previously announced coldkey swap (after the delay has passed).

    Step two of the two-step migration: reveals the new coldkey and moves
    everything the signing coldkey owns — balance, stake, and subnet ownership
    — to it. Irreversible once included. The revealed key must hash to exactly
    what ``announce_coldkey_swap`` committed to, and the call fails if the
    announcement delay has not elapsed, no announcement exists, or the swap
    is frozen by a dispute. After it succeeds, the old coldkey is empty; all
    future operations sign with the new coldkey.
    """

    op = "swap_coldkey_announced"
    signer = "coldkey"
    wraps = (("SubtensorModule", "swap_coldkey_announced"),)

    new_coldkey_ss58: str = field(
        metadata={
            "help": "Coldkey receiving everything; must match the previously announced "
            "hash exactly."
        }
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.swap_coldkey_announced(new_coldkey=self.new_coldkey_ss58)
        )

    def summary(self) -> str:
        return f"SWAP coldkey to {self.new_coldkey_ss58} (moves all ownership)"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "irreversible: balance, stake, and subnet ownership move to the new coldkey",
            "the new coldkey must match the announced hash exactly",
        ]

    def affects_all_subnets(self) -> bool:
        return True


@register
@dataclass
class ClearColdkeySwapAnnouncement(Intent):
    """Cancel a pending coldkey swap announcement (after the reannouncement delay).

    Withdraws this coldkey's own pending swap announcement so the swap can no
    longer be executed — use it if you announced to the wrong key or changed
    your mind. Only clearable once the current block reaches the swap's
    execute block plus the reannouncement delay — i.e. roughly the
    announcement delay (~5 days) PLUS the reannouncement delay (~1 day) after
    announcing — so it cannot be used to rapidly cycle announcements. Nothing
    moves; a fresh ``announce_coldkey_swap`` can be made afterwards. If the
    announcement was made by an attacker, ``dispute_coldkey_swap`` is the
    right call instead.
    """

    op = "clear_coldkey_swap_announcement"
    signer = "coldkey"
    wraps = (("SubtensorModule", "clear_coldkey_swap_announcement"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.SubtensorModule.clear_coldkey_swap_announcement())

    def summary(self) -> str:
        return "clear the pending coldkey swap announcement"


@register
@dataclass
class DisputeColdkeySwap(Intent):
    """Freeze this coldkey entirely until root resolves the dispute.

    The recovery path for a compromised coldkey: if a swap was announced on
    your coldkey that you did not initiate, disputing freezes the account
    entirely — the chain rejects ALL signed extrinsics from it, including
    executing the swap, clearing the announcement, or disputing again — until
    root clears the state via ``reset_coldkey_swap``. Sign it from the
    affected coldkey itself. It does not cancel the announcement or move
    anything — it locks the situation so an attacker cannot complete the
    takeover while governance investigates; only root can unfreeze. Use
    ``clear_coldkey_swap_announcement`` instead to withdraw an announcement
    you made yourself.
    """

    op = "dispute_coldkey_swap"
    signer = "coldkey"
    wraps = (("SubtensorModule", "dispute_coldkey_swap"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.SubtensorModule.dispute_coldkey_swap())

    def summary(self) -> str:
        return "dispute the pending coldkey swap (freezes it for governance)"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["blocks the swap until the triumvirate resolves the dispute"]
