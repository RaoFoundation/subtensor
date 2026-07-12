"""Subnet-owner and governance intents not covered by hyperparameters."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ..settings import U16_MAX
from ._money import Money, Spend, tao_amount
from .base import Intent
from .registry import register


@register
@dataclass
class TrimSubnet(Intent):
    """Trim a subnet to at most ``max_n`` UIDs (subnet owner).

    Lowers the subnet's UID capacity and immediately deregisters the
    lowest-emission neurons above the new limit — those miners lose their
    slots and would have to re-register (paying the burn cost) to return.
    ``max_n`` must be between the chain's minimum allowed UIDs (64) and the
    subnet's current ``max_allowed_uids``. Owner-immune and temporally
    immune UIDs are skipped, and the call fails if immune UIDs would exceed
    80% of ``max_n``. Surviving UIDs are renumbered consecutively from
    zero, so UID values change. Rate-limited to once per 216,000 blocks
    (30 days) and blocked during the end-of-epoch admin freeze window.
    Owner-only and disruptive to affected participants, so announce it
    before shrinking a live subnet. To simply cap future growth without
    evicting anyone, set the ``max_allowed_uids`` hyperparameter to a value
    at or above the current UID count instead.
    """

    op = "trim_subnet"
    signer = "coldkey"
    wraps = (("AdminUtils", "sudo_trim_to_max_allowed_uids"),)

    netuid: int = field(metadata={"help": "Subnet to trim; the signer must be its owner."})
    max_n: int = field(
        metadata={
            "help": "New maximum number of UIDs; lowest-emission neurons above this are removed."
        }
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.AdminUtils.sudo_trim_to_max_allowed_uids(netuid=self.netuid, max_n=self.max_n)
        )

    def summary(self) -> str:
        return f"trim netuid {self.netuid} to at most {self.max_n} UIDs"


@register
@dataclass
class StakeBurn(Intent):
    """Buy back / burn stake via the stake-burn extrinsic.

    Spends TAO from the signing coldkey to buy the subnet's alpha and burn it,
    reducing alpha supply (a buyback-and-burn) rather than adding to the
    signer's stake. The TAO is spent permanently — nothing lands in your stake,
    so this is not an investment call; use a regular add-stake intent to
    acquire a position. Fails on the root subnet
    (``CannotBurnOrRecycleOnRootSubnet``). The chain accepts an optional
    limit (omitted = market order), but this intent always requires
    ``limit_price`` and executes all-or-nothing: the swap fails instead of
    partially filling at a worse rate. Counts against a configured spend
    cap.
    """

    op = "stake_burn"
    signer = "coldkey"
    wraps = (("SubtensorModule", "add_stake_burn"),)

    netuid: int = field(metadata={"help": "Subnet whose alpha is bought and burned."})
    amount_tao: Money = field(
        metadata={"help": "Spent from the coldkey to buy alpha that is then burned."}
    )
    limit_price: int = field(
        metadata={
            "help": "Worst acceptable price in rao per alpha; the call fails rather than "
            "filling beyond it."
        }
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey the burn is routed through; defaults to the wallet's hotkey."},
    )

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        return await substrate.compose(
            calls.SubtensorModule.add_stake_burn(
                hotkey=hotkey,
                netuid=self.netuid,
                amount=self.amount_tao.rao,
                limit=self.limit_price,
            )
        )

    def summary(self) -> str:
        return f"stake burn {self.amount_tao} on netuid {self.netuid}"

    def spend(self) -> Spend:
        return self.amount_tao


@register
@dataclass
class SetMechanismCount(Intent):
    """Set the number of mechanisms on a subnet.

    Mechanisms are independent incentive sub-markets within one subnet, each
    running its own weights and consensus; this owner-only call sets how many
    the subnet runs. The count must be greater than zero, and the chain caps
    how many mechanisms a subnet may have. Increasing the count opens new
    mechanisms; decreasing it removes the highest-numbered ones and the
    miner state in them. Any change to the count clears the emission split
    back to an even division — reapply ``set_mechanism_emission_split``
    afterwards if you want an uneven split. Rate-limited and blocked during
    the end-of-epoch admin freeze window.
    """

    op = "set_mechanism_count"
    signer = "coldkey"
    wraps = (("AdminUtils", "sudo_set_mechanism_count"),)

    netuid: int = field(metadata={"help": "Subnet to configure; the signer must be its owner."})
    mechanism_count: int = field(metadata={"help": "Number of mechanisms the subnet should run."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.AdminUtils.sudo_set_mechanism_count(
                netuid=self.netuid, mechanism_count=self.mechanism_count
            )
        )

    def summary(self) -> str:
        return f"set mechanism count to {self.mechanism_count} on netuid {self.netuid}"


def _normalize_split(split: list) -> list[int]:
    """Normalize an emission split to raw u16 weights summing to exactly 65,535.

    All-integer input is taken as the raw weights (validated to sum exactly);
    any float in the list marks the human form: entries become relative
    weights, scaled to the exact-sum split with largest-remainder rounding so
    the result always lands on 65,535 on the nose.
    """
    if not split:
        raise ValueError("the emission split needs at least one entry")
    if any(isinstance(w, bool) or not isinstance(w, (int, float)) for w in split):
        raise ValueError(f"emission split entries must be numbers; got {split!r}")
    if any(w < 0 for w in split):
        raise ValueError(f"emission split entries must be non-negative; got {split!r}")
    if all(isinstance(w, int) for w in split):
        total = sum(split)
        if total != U16_MAX:
            raise ValueError(
                f"raw u16 emission weights must sum to exactly {U16_MAX}; got {total}. "
                "Pass weights with a decimal point (e.g. [0.5, 0.5]) to have the "
                "split normalized for you."
            )
        return list(split)
    total = float(sum(split))
    if total <= 0:
        raise ValueError("relative emission weights must sum to more than zero")
    scaled = [w / total * U16_MAX for w in split]
    floors = [int(s) for s in scaled]
    remainder = U16_MAX - sum(floors)
    # Distribute the leftover units to the largest fractional parts.
    by_fraction = sorted(range(len(split)), key=lambda i: scaled[i] - floors[i], reverse=True)
    for i in by_fraction[:remainder]:
        floors[i] += 1
    return floors


@register
@dataclass
class SetMechanismEmissionSplit(Intent):
    """Set emission split between mechanisms on a subnet.

    Owner-only: divides the subnet's emission between its mechanisms, one
    entry per mechanism in order. Entries with a decimal point are relative
    weights (e.g. ``[0.5, 0.5]`` or ``[3.0, 1.0]``), normalized to the exact
    u16 split the chain requires; plain integers are raw u16 weights and must
    sum to exactly 65,535 themselves. The list may be at most as long as the
    subnet's current mechanism count (see ``set_mechanism_count``) — a
    shorter list leaves the trailing mechanisms with zero. Changing the
    split reallocates future emission only — nothing already emitted moves.
    """

    op = "set_mechanism_emission_split"
    signer = "coldkey"
    wraps = (("AdminUtils", "sudo_set_mechanism_emission_split"),)

    netuid: int = field(metadata={"help": "Subnet to configure; the signer must be its owner."})
    split: list = field(
        metadata={
            "help": "Emission weights in mechanism order, at most one entry per "
            "mechanism: relative weights with a decimal point (normalized for you, "
            "e.g. [0.5, 0.5]), or raw u16 integers summing to exactly 65,535."
        }
    )

    def __post_init__(self):
        self.split = _normalize_split(self.split)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.AdminUtils.sudo_set_mechanism_emission_split(
                netuid=self.netuid, maybe_split=[int(x) for x in self.split]
            )
        )

    def summary(self) -> str:
        return f"set mechanism emission split on netuid {self.netuid}: {self.split}"


@register
@dataclass
class UpdateSymbol(Intent):
    """Update a subnet's symbol (from the chain's fixed catalog).

    Cosmetic call for the subnet owner (or root): changes the short ticker
    shown for the subnet's alpha token in wallets, explorers, and CLIs.
    Symbols are not arbitrary strings — the chain keeps a fixed catalog of
    roughly 439 predefined symbols, and anything outside it is rejected
    with ``SymbolDoesNotExist``. A symbol already taken by another subnet
    is rejected with ``SymbolAlreadyInUse``. No economic effect — balances,
    stake, and emissions are untouched.
    """

    op = "update_symbol"
    signer = "coldkey"
    wraps = (("SubtensorModule", "update_symbol"),)

    netuid: int = field(
        metadata={"help": "Subnet whose symbol to change; the signer must be its owner."}
    )
    symbol: str = field(
        metadata={
            "help": "New token symbol; must be one of the chain's predefined symbols and "
            "not in use by another subnet."
        }
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.update_symbol(
                netuid=self.netuid, symbol=self.symbol.encode("utf-8")
            )
        )

    def summary(self) -> str:
        return f"set symbol for netuid {self.netuid} to {self.symbol!r}"
