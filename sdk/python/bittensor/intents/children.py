"""Child hotkeys and delegate take."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from .._generated import storage as st
from ..hyperparams import U64_MAX, proportion_to_raw
from ..result import BittensorError
from ..settings import U16_MAX
from .base import Intent
from .registry import register

HOTKEY_HELP = "Hotkey the operation applies to."
TAKE_HELP = (
    "New take: a 0..1 fraction (e.g. 0.18 for 18 percent) or the raw u16 "
    f"proportion (0..{U16_MAX}). The chain enforces its configured take bounds."
)


def take_to_u16(value: "int | float | str") -> int:
    """Normalize a take (0..1 fraction or raw u16) to the raw u16.

    A float (or a string with a decimal point) is the human 0..1 fraction; a
    plain integer is the raw on-chain u16 — the hyperparameter value rules.
    """
    return proportion_to_raw(value, U16_MAX, "take")


@register
@dataclass
class SetChildren(Intent):
    """Assign child hotkeys with stake-weight proportions on a subnet.

    Childkeys let a parent hotkey delegate a fraction of its stake weight to
    other hotkeys on one subnet — commonly used to split validation duties or
    point stake at a separate validating key without moving the stake itself.
    Each entry in ``children`` is a pair of proportion and hotkey ss58. A
    proportion with a decimal point is the human 0..1 fraction of the parent's
    stake weight (e.g. 0.5); a plain integer is the raw u64 share of u64::MAX.
    The proportions must not sum past the whole. The call replaces the full
    child set, so pass an empty list to revoke all children. Signed by the
    coldkey that owns the parent hotkey, and subject to the chain's childkey
    rate limit.

    Chain guards: not allowed on the root subnet; at most 5 children per
    hotkey per subnet; duplicate children are rejected; a hotkey that is a
    parent of this hotkey cannot be added as its child (relations stay
    bipartite); and the parent hotkey needs a minimum own stake
    (StakeThreshold) unless it is the subnet-owner hotkey. Changes take
    effect after a chain-defined cooldown, except on subnets whose subtoken
    is not yet enabled, where they apply immediately.
    """

    op = "set_children"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_children"),)

    netuid: int = field(metadata={"help": "Subnet on which the child relationships apply."})
    children: list = field(
        metadata={
            "help": "JSON list of proportion-and-hotkey pairs; each proportion is a 0..1 "
            "fraction (e.g. 0.5) or a raw u64 share of u64::MAX of the parent's stake "
            "weight delegated to that child. An empty list revokes all children."
        }
    )
    hotkey_ss58: Optional[str] = field(default=None, metadata={"help": HOTKEY_HELP})

    def __post_init__(self):
        normalized = []
        for entry in self.children:
            try:
                prop, child = entry
            except (TypeError, ValueError):
                raise ValueError(
                    f"each child entry must be a [proportion, hotkey_ss58] pair; got {entry!r}"
                ) from None
            normalized.append([proportion_to_raw(prop, U64_MAX, "child proportion"), child])
        total = sum(prop for prop, _ in normalized)
        if total > U64_MAX:
            raise ValueError(
                f"child proportions sum to {total / U64_MAX:.4g} of the parent's stake "
                "weight; together they must not exceed 1.0"
            )
        self.children = normalized

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        children = [(int(prop), child) for prop, child in self.children]
        return await substrate.compose(
            calls.SubtensorModule.set_children(hotkey=hotkey, netuid=self.netuid, children=children)
        )

    def summary(self) -> str:
        return f"set {len(self.children)} child hotkeys on netuid {self.netuid}"

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return ["delegates a share of this hotkey's stake weight to the children"]


@register
@dataclass
class SetChildkeyTake(Intent):
    """Set the childkey take for a hotkey on a subnet.

    The childkey take is the fraction of emissions a child hotkey keeps from
    the stake weight its parents delegate to it, before passing the remainder
    through. It is set per subnet, unlike the global delegate take. Signed by
    the coldkey that owns the child hotkey. The chain enforces both its
    minimum and maximum childkey take bounds, and rate-limits only increases
    — decreases apply immediately, so lowering the take is always available.
    """

    op = "set_childkey_take"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_childkey_take"),)

    netuid: int = field(metadata={"help": "Subnet the childkey take applies to."})
    take: int | float | str = field(metadata={"help": TAKE_HELP})
    hotkey_ss58: Optional[str] = field(default=None, metadata={"help": HOTKEY_HELP})

    def __post_init__(self):
        self.take = take_to_u16(self.take)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.set_childkey_take(
                hotkey=hotkey, netuid=self.netuid, take=self.take
            )
        )

    def summary(self) -> str:
        return f"set childkey take to {self.take}/{U16_MAX} on netuid {self.netuid}"


@register
@dataclass
class IncreaseTake(Intent):
    """Increase the delegate take of a hotkey.

    The delegate take is the fraction of staking emissions a delegate hotkey
    keeps for itself before distributing the rest to its nominators. This call
    only moves the take upward: the chain rejects values at or below the
    current take, values above the configured maximum, and increases made
    sooner than the take rate limit allows. Signed by the coldkey that owns
    the hotkey. Use ``set_take`` if you just want to land on an absolute value
    without tracking the direction yourself.
    """

    op = "increase_take"
    signer = "coldkey"
    wraps = (("SubtensorModule", "increase_take"),)

    take: int | float | str = field(metadata={"help": TAKE_HELP})
    hotkey_ss58: Optional[str] = field(default=None, metadata={"help": HOTKEY_HELP})

    def __post_init__(self):
        self.take = take_to_u16(self.take)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.increase_take(hotkey=hotkey, take=self.take)
        )

    def summary(self) -> str:
        return f"increase delegate take to {self.take}/{U16_MAX}"


@register
@dataclass
class DecreaseTake(Intent):
    """Decrease the delegate take of a hotkey.

    The delegate take is the fraction of staking emissions a delegate hotkey
    keeps for itself before distributing the rest to its nominators. This call
    only moves the take downward — the chain rejects values at or above the
    current take — and unlike increases it is not rate limited, so lowering
    your take is always available. Signed by the coldkey that owns the hotkey.
    Use ``set_take`` to land on an absolute value without tracking direction.
    """

    op = "decrease_take"
    signer = "coldkey"
    wraps = (("SubtensorModule", "decrease_take"),)

    take: int | float | str = field(metadata={"help": TAKE_HELP})
    hotkey_ss58: Optional[str] = field(default=None, metadata={"help": HOTKEY_HELP})

    def __post_init__(self):
        self.take = take_to_u16(self.take)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.decrease_take(hotkey=hotkey, take=self.take)
        )

    def summary(self) -> str:
        return f"decrease delegate take to {self.take}/{U16_MAX}"


@register
@dataclass
class SetTake(Intent):
    """Set the delegate take to an absolute value.

    The delegate take is the fraction of staking emissions a delegate hotkey
    keeps before distributing the rest to its nominators. This is sugar over
    the chain's directional ``increase_take`` / ``decrease_take``: it reads the
    current take and dispatches whichever call moves it to ``take``, so you do
    not need to know the current value. Signed by the coldkey that owns the
    hotkey. If the move is upward it inherits the increase path's constraints
    (take maximum and rate limit on increases).
    """

    op = "set_take"
    signer = "coldkey"
    wraps = (
        ("SubtensorModule", "increase_take"),
        ("SubtensorModule", "decrease_take"),
    )

    take: int | float | str = field(metadata={"help": TAKE_HELP})
    hotkey_ss58: Optional[str] = field(default=None, metadata={"help": HOTKEY_HELP})

    def __post_init__(self):
        self.take = take_to_u16(self.take)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        item = st.SubtensorModule.Delegates
        current = int(await substrate.query(item[0], item[1], [hotkey]) or 0)
        if self.take == current:
            raise BittensorError(f"delegate take is already {current}/{U16_MAX}; nothing to change")
        call = (
            calls.SubtensorModule.decrease_take(hotkey=hotkey, take=self.take)
            if self.take < current
            else calls.SubtensorModule.increase_take(hotkey=hotkey, take=self.take)
        )
        return await substrate.compose(call)

    def summary(self) -> str:
        return f"set delegate take to {self.take / U16_MAX:.2%} ({self.take} as u16)"
