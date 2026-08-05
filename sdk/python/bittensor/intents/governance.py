"""Subnet-owner and governance intents not covered by hyperparameters."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ._money import Money, Spend, tao_amount
from .base import Intent
from .registry import register
from .staking import DEFAULT_RATE_TOLERANCE, _alpha_price_rao, _check_rate_tolerance


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
    origin = "subnet_owner"  # the chain also accepts root
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
    limit (omitted = market order), but this intent always submits one and
    executes all-or-nothing: the swap fails instead of partially filling at
    a worse rate. When ``limit_price`` is omitted, the limit is derived from
    the current pool price plus ``rate_tolerance`` (5% by default). Counts
    against a configured spend cap.
    """

    op = "stake_burn"
    signer = "coldkey"
    wraps = (("SubtensorModule", "add_stake_burn"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet whose alpha is bought and burned."})
    amount_tao: Money = field(
        metadata={"help": "Spent from the coldkey to buy alpha that is then burned."}
    )
    limit_price: Optional[int] = field(
        default=None,
        metadata={
            "help": "Worst acceptable price in rao per alpha; the call fails rather than "
            "filling beyond it. Defaults to the current pool price plus `rate_tolerance`."
        },
    )
    rate_tolerance: float = field(
        default=DEFAULT_RATE_TOLERANCE,
        metadata={
            "help": "Maximum price move accepted when `limit_price` is omitted, as a "
            "fraction (0.05 = 5%). Ignored when `limit_price` is given."
        },
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey the burn is routed through; defaults to the wallet's hotkey."},
    )

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)
        _check_rate_tolerance(self.rate_tolerance)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        if self.limit_price is not None:
            limit = self.limit_price
        else:
            price = await _alpha_price_rao(substrate, self.netuid)
            limit = int(price * (1 + self.rate_tolerance))
        return await substrate.compose(
            calls.SubtensorModule.add_stake_burn(
                hotkey=hotkey,
                netuid=self.netuid,
                amount=self.amount_tao.rao,
                limit=limit,
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
    back to an even division. Rate-limited and blocked during the
    end-of-epoch admin freeze window.
    """

    op = "set_mechanism_count"
    signer = "coldkey"
    origin = "subnet_owner"  # the chain also accepts root
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
    origin = "subnet_owner"  # the chain also accepts root
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
