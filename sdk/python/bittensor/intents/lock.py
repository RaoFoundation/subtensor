"""Stake-lock and conviction intents."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, ClassVar, Optional

from .._generated import calls
from ._money import ALL, Money, alpha_amount
from .base import Intent
from .registry import register
from .staking import _lockable_rao


@register
@dataclass
class LockStake(Intent):
    """Lock alpha stake on a subnet, building conviction toward a hotkey.

    Commits part of the signing coldkey's alpha on the subnet as locked
    stake: the locked amount builds conviction the longer it stays locked.
    The lock acts as a subnet-wide floor on unstaking, not a hold on a
    specific position — the coldkey can freely unstake anything above the
    locked mass, and the locked amount itself keeps earning normally. The
    coldkey's total alpha on the subnet, summed across all hotkeys, must
    cover the locked amount; conviction can be pointed at one hotkey while
    the stake sits on another. If a lock already exists on the subnet, the
    hotkey must match the existing lock's hotkey or the call fails with
    ``LockHotkeyMismatch`` — repeat calls only top up the lock; use
    ``move_lock`` to change hotkeys. Whether the lock decays over time or
    persists is controlled per coldkey per subnet with
    ``set_perpetual_lock``. Pass ``all`` to lock every unlocked alpha the
    coldkey holds on the subnet (a top-up of the remaining free mass).
    """

    op = "lock_stake"
    signer = "coldkey"
    wraps = (("SubtensorModule", "lock_stake"),)
    all_amount_fields: ClassVar[tuple[str, ...]] = ("amount_alpha",)

    netuid: int = field(metadata={"help": "Subnet the locked stake lives on."})
    amount_alpha: Money = field(
        metadata={"help": "How much of the existing unlocked stake to lock, or ``all``."}
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Hotkey the lock's conviction is credited to. Defaults to the wallet hotkey."
        },
    )

    def __post_init__(self):
        self.amount_alpha = alpha_amount(self.amount_alpha, self.netuid, allow_all=True)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        if self.amount_alpha == ALL:
            rao = await _lockable_rao(substrate, self.coldkey_address(wallet), self.netuid)
        else:
            rao = self.amount_alpha.rao
        return await substrate.compose(
            calls.SubtensorModule.lock_stake(hotkey=hotkey, netuid=self.netuid, amount=rao)
        )

    def summary(self) -> str:
        amount = "ALL unlocked alpha" if self.amount_alpha == ALL else str(self.amount_alpha)
        return f"lock {amount} on netuid {self.netuid}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        if self.amount_alpha == ALL:
            return ["locks every unlocked alpha this coldkey holds on the subnet"]
        return []


@register
@dataclass
class SetPerpetualLock(Intent):
    """Enable or disable perpetual lock mode for a coldkey on a subnet.

    Switches how the signing coldkey's stake lock on the subnet behaves over
    time: perpetual mode keeps the lock (and its conviction) in force
    indefinitely, while decaying mode lets it wind down over time so the
    stake eventually becomes liquid again. A per-coldkey, per-subnet setting
    that moves no funds by itself — it changes the behavior of locks created
    with ``lock_stake``. Enabling perpetual mode means the locked stake stays
    illiquid until you switch back to decaying and the lock runs off.
    """

    op = "set_perpetual_lock"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_perpetual_lock"),)

    netuid: int = field(metadata={"help": "Subnet whose lock mode is changed."})
    enabled: bool = field(
        metadata={"help": "True for perpetual mode (lock never decays), false for decaying mode."}
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_perpetual_lock(netuid=self.netuid, enabled=self.enabled)
        )

    def summary(self) -> str:
        mode = "perpetual" if self.enabled else "decaying"
        return f"set lock mode to {mode} on netuid {self.netuid}"


@register
@dataclass
class MoveLock(Intent):
    """Move an existing lock from one hotkey to another on a subnet.

    Re-points the signing coldkey's entire stake lock on the subnet at the
    destination hotkey, carrying the locked mass with it. Accrued conviction
    is preserved only when the origin and destination hotkeys are owned by
    the same coldkey (e.g. rotating your own validator hotkeys); if the
    destination hotkey belongs to a different coldkey, conviction resets to
    zero and matures again from scratch. The destination hotkey must exist
    on chain (fails with ``HotKeyAccountNotExists``). Fails if there is no
    existing lock on the subnet to move.
    """

    op = "move_lock"
    signer = "coldkey"
    wraps = (("SubtensorModule", "move_lock"),)

    netuid: int = field(metadata={"help": "Subnet the lock lives on."})
    destination_hotkey_ss58: str = field(metadata={"help": "Hotkey the lock is moved to."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.move_lock(
                destination_hotkey=self.destination_hotkey_ss58, netuid=self.netuid
            )
        )

    def summary(self) -> str:
        return f"move lock on netuid {self.netuid} to {self.destination_hotkey_ss58}"
