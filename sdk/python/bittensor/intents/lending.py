"""Fixed-principal loans backed by a subnet's separate lending reserves."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Optional, cast

from .._generated.calls import Call
from .._generated.runtime_apis import StakeInfoRuntimeApi
from .._generated.storage import Item
from ..balance import Balance
from ..result import BittensorError, ChainError
from ..sp_core import ss58_decode
from ._money import Money, Spend, alpha_amount, tao_amount
from .base import Intent, IntentPreflight
from .registry import register

POSITIONS = Item("Lending", "Positions", "Position")


def _nonnegative(value: Any, label: str) -> int:
    """Reject missing or malformed balance data instead of guessing availability."""
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise BittensorError(f"invalid {label} in wallet balance response")
    return value


async def _spendable_tao(substrate, address: str, *, block_hash=None) -> Balance:
    """TAO available while retaining the account, matching Preserve + Polite."""
    account, deposit = await asyncio.gather(
        substrate.query("System", "Account", [address], block_hash=block_hash),
        substrate.constant("Balances", "ExistentialDeposit"),
    )
    data = account.get("data") if isinstance(account, dict) else None
    if not isinstance(data, dict):
        raise BittensorError("wallet TAO balance is unavailable")
    free = _nonnegative(data.get("free"), "free TAO")
    reserved = _nonnegative(data.get("reserved"), "reserved TAO")
    frozen = _nonnegative(data.get("frozen"), "frozen TAO")
    ed = _nonnegative(deposit, "existential deposit")
    # Freezes apply to total balance, including reserved funds. The ED must
    # remain in free balance even when the account also holds reserved funds.
    return Balance.from_rao(max(free - max(max(frozen - reserved, 0), ed), 0))


async def _available_alpha(
    substrate, owner: str, hotkey: str, netuid: int, *, block_hash=None
) -> Balance:
    """Free alpha on this hotkey, excluding conviction locks and miner bonds."""
    info, availability, bond = await asyncio.gather(
        substrate.runtime_call(
            *StakeInfoRuntimeApi.get_stake_info_for_hotkey_coldkey_netuid,
            [hotkey, owner, netuid],
            block_hash=block_hash,
        ),
        substrate.runtime_call(
            *StakeInfoRuntimeApi.get_stake_availability_for_coldkeys,
            [[owner], [netuid]],
            block_hash=block_hash,
        ),
        substrate.query(
            "SubtensorModule", "MinerCollateral", [netuid, hotkey, owner], block_hash=block_hash
        ),
    )
    if info is not None and not isinstance(info, dict):
        raise BittensorError("wallet alpha stake is unavailable")
    if not isinstance(availability, dict):
        raise BittensorError("wallet alpha availability is unavailable")
    per_owner = availability.get(owner, {})
    if not isinstance(per_owner, dict):
        raise BittensorError("invalid wallet alpha availability")
    entry = per_owner.get(netuid, per_owner.get(str(netuid), {}))
    if not isinstance(entry, dict):
        raise BittensorError("invalid wallet alpha availability")
    stake = _nonnegative(info.get("stake") if info is not None else 0, "alpha stake")
    available = _nonnegative(entry.get("available", 0), "available alpha")
    if bond is not None and not isinstance(bond, dict):
        raise BittensorError("wallet miner collateral is unavailable")
    locked = _nonnegative(bond.get("locked") if bond is not None else 0, "miner collateral")
    return Balance.from_rao(min(available, max(stake - locked, 0)), netuid)


class _FeeAddress:
    """Public account information for fee estimates; carries no signing key."""

    crypto_type = 1

    def __init__(self, address: str):
        self.ss58_address = address
        self.public_key = bytes(ss58_decode(address))


def check_side(side: str) -> str:
    """Accept the two runtime variants, including lower-case CLI spelling."""
    result = str(side).capitalize()
    if result not in ("Short", "Long"):
        raise BittensorError("side must be 'short' or 'long'")
    return result


def _check_netuid(netuid: int) -> None:
    if not 1 <= netuid <= 65535:
        raise BittensorError("lending requires a subnet netuid from 1 to 65535")


def _contextual_amount(value: Money) -> Money:
    """Keep exact decimals until the owned position identifies the currency.

    An explicitly tagged Balance keeps its tag for validation at build time.
    Plain amounts become decimal strings rather than floats. A close's units
    depend on stored position state, not an extra caller-supplied side field.
    """
    normalized = cast(Balance, tao_amount(value)) if not isinstance(value, Balance) else value
    if normalized.rao < 0:
        raise BittensorError("amount must be non-negative")
    return normalized if isinstance(value, Balance) else format(normalized.decimal, "f")


@register
@dataclass
class OpenLoan(Intent):
    """Open or increase a fixed-principal subnet loan at up to 25% LTV.

    A short locks TAO collateral, lifts borrowed alpha, and immediately sells
    that alpha into escrow TAO (`proceeds`) on the position's custody account.
    A long locks existing free alpha stake, lifts TAO, and buys escrow alpha
    with the borrowed TAO (no free TAO payout). Debt stays in the borrowed
    currency. Interest is 100% annually on opening loan value, charged in fixed
    collateral units and collected every ``InterestPeriod`` blocks (runtime
    constant; 50_400 ≈ 7 days at 12 s). There are no price-triggered
    liquidations; exhausted collateral forfeits the position.

    Alpha debt must also fit 25% of collateral at a conservative immediate
    funded-redemption valuation, including payout rounding. TAO debt requires
    conservative funded collateral backing. New borrowing must preserve each
    other same-side loan's individual coverage after accrued interest. These
    opening checks do not guarantee future repayment.

    An existing position must have the same side and hotkey. ``collateral``
    adds to its remaining collateral after accrued interest; borrowed principal,
    and the new fixed coupon add to the saved position.
    The old coupon is not repriced. The combined position must pass current
    opening guards, and an already exhausted position cannot be increased.

    ``amount`` selects exact additional principal: alpha for shorts and TAO
    for longs. With no amount, the collateral determines the borrowing limit.
    The runtime transfers exactly the requested amount or refuses the call.
    ``min_borrow`` is a floor on additional principal. ``min_proceeds`` is the
    short's minimum escrow TAO from the forced sale (slippage floor);
    ``min_escrow_alpha`` is the long's minimum escrow alpha from the forced buy.
    The call is atomic and refuses a smaller loan or opening value. There remains one
    position per coldkey and subnet; closing repays its total principal.

    On fork / clone chains without a live MEV decryptor, submit with
    ``--no-mev-shield`` (encrypted submission will never be decrypted).
    """

    op = "open_loan"
    signer = "coldkey"
    wraps = (("Lending", "open"), ("Lending", "open_exact"))
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet to borrow against."})
    side: str = field(metadata={"help": "short: lock TAO; long: lock subnet alpha."})
    collateral: Money = field(
        metadata={"help": "Additional collateral to lock: TAO for a short, alpha for a long."}
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Hotkey receiving short alpha or holding long collateral and repayment alpha."
        },
    )
    min_borrow: Money = field(
        default=0,
        metadata={"help": "Minimum additional principal: alpha for a short, TAO for a long."},
    )
    min_proceeds: Money = field(
        default=0,
        metadata={"help": "Minimum additional short opening value in TAO; unused for longs."},
    )
    min_escrow_alpha: Money = field(
        default=0,
        metadata={"help": "Minimum escrowed alpha after a long's forced buy; unused for shorts."},
    )
    valid_until: int = field(
        default=2**32 - 1,
        metadata={"help": "Latest block number that may include this open."},
    )
    amount: Optional[Money] = field(
        default=None,
        metadata={"help": "Exact additional amount to borrow: alpha for a short, TAO for a long."},
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.side = check_side(self.side)
        collateral_unit = (
            tao_amount if self.side == "Short" else lambda x: alpha_amount(x, self.netuid)
        )
        debt_unit = tao_amount if self.side == "Long" else lambda x: alpha_amount(x, self.netuid)
        normalized = cast(Balance, collateral_unit(self.collateral))
        self.collateral = normalized
        self.min_borrow = debt_unit(self.min_borrow)
        self.min_proceeds = tao_amount(self.min_proceeds)
        self.min_escrow_alpha = alpha_amount(self.min_escrow_alpha, self.netuid)
        if not isinstance(self.valid_until, int) or isinstance(self.valid_until, bool):
            raise BittensorError("valid_until must be a block number")
        if self.valid_until < 0:
            raise BittensorError("valid_until must be non-negative")
        if normalized.rao == 0:
            raise BittensorError("collateral must be greater than zero")
        if self.amount is not None:
            self.amount = debt_unit(self.amount)
            requested = cast(Balance, self.amount)
            if requested.rao == 0:
                raise BittensorError("amount to borrow must be greater than zero")
            if cast(Balance, self.min_borrow).rao > requested.rao:
                raise BittensorError("min_borrow cannot exceed the exact amount to borrow")

    async def build(self, substrate, wallet: Any):
        collateral = cast(Balance, self.collateral)
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        self._built_hotkey = hotkey
        params = {
            "netuid": self.netuid,
            "side": self.side,
            "collateral": collateral.rao,
            "hotkey": hotkey,
            "min_proceeds": cast(Balance, self.min_proceeds).rao,
            "min_escrow_alpha": cast(Balance, self.min_escrow_alpha).rao,
            "valid_until": self.valid_until,
        }
        if self.amount is None:
            params["min_borrow"] = cast(Balance, self.min_borrow).rao
        else:
            params["principal"] = cast(Balance, self.amount).rao
        return await substrate.compose(
            Call(
                "Lending",
                "open" if self.amount is None else "open_exact",
                params,
            )
        )

    def summary(self) -> str:
        if self.amount is not None:
            return (
                f"borrow exactly {self.amount} as {self.side.lower()} on netuid {self.netuid}, "
                f"add {self.collateral}"
            )
        return (
            f"open or increase {self.side.lower()} on netuid {self.netuid}, add {self.collateral}"
        )

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        """Verify available collateral and inclusion fees before any signature."""
        preview = IntentPreflight(effects=[self.summary()], warnings=[], blocks=[])
        try:
            from ..reads.lending import _ok

            if call is None:
                raise BittensorError("composed lending call is unavailable for fee estimation")
            block_hash = await substrate.block_hash()
            collateral = cast(Balance, self.collateral)
            hotkey = getattr(self, "_built_hotkey", None) or self.hotkey_ss58
            if hotkey is None:
                raise BittensorError("selected collateral hotkey is unavailable")
            quote_args = [dispatch_origin, self.netuid, self.side, collateral.rao, hotkey]
            method = "quote_open_for"
            if self.amount is not None:
                method = "quote_open_exact_for"
                quote_args.append(cast(Balance, self.amount).rao)
            quote = _ok(
                await substrate.runtime_call(
                    "LendingRuntimeApi", method, quote_args, block_hash=block_hash
                )
            )
            principal = _nonnegative(quote.get("principal"), "quoted principal")
            opening = _nonnegative(quote.get("opening_value"), "quoted opening value")
            annual = _nonnegative(quote.get("annual_interest"), "quoted annual interest")
            if principal == 0 or opening == 0 or annual == 0:
                raise BittensorError("lending quote must contain a positive loan and coupon")
            if self.amount is not None and principal != cast(Balance, self.amount).rao:
                raise BittensorError("lending quote does not match the exact amount to borrow")
            if principal < cast(Balance, self.min_borrow).rao:
                raise BittensorError(
                    "fresh lending quote is below the minimum additional principal"
                )
            if self.side == "Short" and opening < cast(Balance, self.min_proceeds).rao:
                raise BittensorError("fresh lending quote is below the minimum opening value")
            fee = await substrate.estimate_fee(call, _FeeAddress(fee_payer))
            if not isinstance(fee, Balance) or fee.netuid != 0 or fee.rao < 0:
                raise BittensorError("invalid lending transaction fee estimate")
            payer_available = await _spendable_tao(substrate, fee_payer, block_hash=block_hash)
            same_account = dispatch_origin == fee_payer
            required = fee.rao
            if self.side == "Short" and same_account:
                required += collateral.rao
            preview.estimated_fee = fee
            preview.required_free = Balance.from_rao(required)
            preview.available_free = payer_available
            preview.facts = [("spendable TAO for fees", str(payer_available))]
            if self.side == "Short":
                available = (
                    payer_available
                    if same_account
                    else await _spendable_tao(substrate, dispatch_origin, block_hash=block_hash)
                )
            else:
                available = await _available_alpha(
                    substrate, dispatch_origin, hotkey, self.netuid, block_hash=block_hash
                )
            preview.facts.append(("available collateral", str(available)))
            if collateral.rao > available.rao:
                preview.blocks.append(
                    f"insufficient spendable collateral: need {collateral}, available {available}"
                    + (f" on hotkey {hotkey}" if self.side == "Long" else "")
                )
            elif payer_available.rao < required:
                preview.blocks.append(
                    "insufficient spendable TAO for "
                    + (
                        "collateral plus transaction fee"
                        if required > fee.rao
                        else "transaction fee"
                    )
                    + f": need {Balance.from_rao(required)}, available {payer_available} "
                    + f"(estimated fee {fee})"
                )
        except Exception as error:
            # CLI review previews intentionally suppress thrown exceptions.
            # Mandatory wallet checks must therefore become explicit hard stops.
            reason = (
                f"{error.name}: {error}"
                if isinstance(error, ChainError) and error.name
                else str(error)
            )
            preview.blocks.append(f"could not verify lending quote, collateral and fees: {reason}")
        return preview

    def spend(self) -> Spend:
        return cast(Balance, self.collateral) if self.side == "Short" else None


@register
@dataclass
class RaiseLoan(Intent):
    """Atomic raise: escrowed long plus spot sale of unlocked alpha.

    Opens (or sizes) a long against alpha collateral and, in the same
    extrinsic, sells additional unlocked alpha for free TAO. Either both
    legs succeed or the call rolls back. ``valid_until`` is a block deadline.
    """

    op = "raise_loan"
    signer = "coldkey"
    wraps = (("Lending", "open_long_and_sell"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet to raise against."})
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey holding collateral and sale alpha."},
    )
    collateral_alpha: Optional[Money] = field(
        default=None,
        metadata={"help": "Exact alpha collateral for the long; omit to size from borrow_tao."},
    )
    borrow_tao: Optional[Money] = field(
        default=None,
        metadata={"help": "Exact TAO principal to borrow; omit to size from collateral_alpha."},
    )
    max_collateral_alpha: Money = field(
        default=0,
        metadata={"help": "Ceiling on alpha collateral when sizing from borrow_tao."},
    )
    min_borrow_tao: Money = field(
        default=0,
        metadata={"help": "Floor on borrowed TAO principal."},
    )
    sell_alpha: Optional[Money] = field(
        default=None,
        metadata={"help": "Unlocked alpha to sell for free TAO; omit to sell none."},
    )
    min_tao_out: Money = field(
        default=0,
        metadata={"help": "Minimum free TAO from the raise sale leg."},
    )
    min_escrow_alpha: Money = field(
        default=0,
        metadata={"help": "Minimum escrowed alpha after the long's forced buy."},
    )
    valid_until: int = field(
        default=0,
        metadata={"help": "Latest block number that may include this raise."},
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        if self.collateral_alpha is not None and self.borrow_tao is not None:
            raise BittensorError("provide collateral_alpha or borrow_tao, not both")
        if self.collateral_alpha is None and self.borrow_tao is None:
            raise BittensorError("provide collateral_alpha or borrow_tao")
        if self.collateral_alpha is not None:
            self.collateral_alpha = alpha_amount(self.collateral_alpha, self.netuid)
        if self.borrow_tao is not None:
            self.borrow_tao = tao_amount(self.borrow_tao)
        self.max_collateral_alpha = alpha_amount(self.max_collateral_alpha, self.netuid)
        self.min_borrow_tao = tao_amount(self.min_borrow_tao)
        if self.sell_alpha is not None:
            self.sell_alpha = alpha_amount(self.sell_alpha, self.netuid)
        self.min_tao_out = tao_amount(self.min_tao_out)
        self.min_escrow_alpha = alpha_amount(self.min_escrow_alpha, self.netuid)
        if not isinstance(self.valid_until, int) or isinstance(self.valid_until, bool):
            raise BittensorError("valid_until must be a block number")
        if self.valid_until < 0:
            raise BittensorError("valid_until must be non-negative")

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        self._built_hotkey = hotkey
        params = {
            "netuid": self.netuid,
            "hotkey": hotkey,
            "collateral_alpha": (
                None if self.collateral_alpha is None else cast(Balance, self.collateral_alpha).rao
            ),
            "borrow_tao": (None if self.borrow_tao is None else cast(Balance, self.borrow_tao).rao),
            "max_collateral_alpha": cast(Balance, self.max_collateral_alpha).rao,
            "min_borrow_tao": cast(Balance, self.min_borrow_tao).rao,
            "sell_alpha": (None if self.sell_alpha is None else cast(Balance, self.sell_alpha).rao),
            "min_tao_out": cast(Balance, self.min_tao_out).rao,
            "min_escrow_alpha": cast(Balance, self.min_escrow_alpha).rao,
            "valid_until": self.valid_until,
        }
        return await substrate.compose(Call("Lending", "open_long_and_sell", params))

    def summary(self) -> str:
        sell = self.sell_alpha if self.sell_alpha is not None else 0
        return f"raise on netuid {self.netuid}: long + sell {sell} (min out {self.min_tao_out})"

    def spend(self) -> Spend:
        return None


@register
@dataclass
class CloseLoan(Intent):
    """Repay total principal and return remaining collateral on a subnet.

    By default (``repay_from_wallet=False``) an escrowed short buys its alpha
    debt back through the AMM using escrow sale proceeds plus remaining TAO
    collateral, then refunds any leftover TAO. Set ``repay_from_wallet=True``
    to repay short alpha from the saved hotkey instead and receive the full
    escrow pot. A long with the default sells escrow (then collateral) for
    principal TAO (selling only enough collateral to cover any shortfall,
    returning leftover collateral as alpha); ``repay_from_wallet=True`` pays
    TAO from free balance and returns escrow alpha. Both charge accrued
    interest through the closing block. A failed repayment, payment ceiling
    or refund floor rolls back.

    ``max_payment`` is TAO for AMM short closes and long repayment, or alpha
    for a wallet-repaid short. ``min_refund`` is TAO for shorts and alpha for
    longs (matching the refund asset on each path). Units are checked against
    the owner's actual stored position. Closing remains available when new
    borrowing is disabled.

    On fork / clone chains without a live MEV decryptor, submit with
    ``--no-mev-shield``.
    """

    op = "close_loan"
    signer = "coldkey"
    wraps = (("Lending", "close"),)
    mev_shield_default = True

    netuid: int = field(metadata={"help": "Subnet of the position to repay."})
    max_payment: Money = field(
        metadata={"help": "Maximum repayment: TAO, or alpha for a wallet-repaid short."}
    )
    min_refund: Money = field(
        default=0,
        metadata={"help": "Minimum returned collateral: TAO for shorts, alpha for longs."},
    )
    repay_from_wallet: bool = field(
        default=False,
        metadata={
            "help": (
                "False (default): buy short alpha back from escrow / sell long escrow. "
                "True: repay short alpha from the saved hotkey, or pay long TAO from wallet."
            )
        },
    )
    valid_until: int = field(
        default=2**32 - 1,
        metadata={"help": "Latest block number that may include this close."},
    )

    def __post_init__(self):
        _check_netuid(self.netuid)
        self.max_payment = _contextual_amount(self.max_payment)
        self.min_refund = _contextual_amount(self.min_refund)
        if not isinstance(self.valid_until, int) or isinstance(self.valid_until, bool):
            raise BittensorError("valid_until must be a block number")
        if self.valid_until < 0:
            raise BittensorError("valid_until must be non-negative")

    async def build(self, substrate, wallet: Any):
        owner = self.coldkey_address(wallet)
        position = await substrate.query(*POSITIONS, [owner, self.netuid])
        if not isinstance(position, dict):
            raise BittensorError(f"no lending position on netuid {self.netuid}")
        side = position["side"]
        if isinstance(side, dict):
            side = next(iter(side))
        side = check_side(str(side))
        payment = cast(
            Balance,
            (
                alpha_amount(self.max_payment, self.netuid)
                if side == "Short" and self.repay_from_wallet
                else tao_amount(self.max_payment)
            ),
        )
        refund = cast(
            Balance,
            (
                tao_amount(self.min_refund)
                if side == "Short"
                else alpha_amount(self.min_refund, self.netuid)
            ),
        )
        # The saved side resolves plain decimal amounts before the executor's
        # TAO spend policy runs. Wallet alpha repayment is not free-TAO spend.
        self.max_payment = payment
        self.min_refund = refund
        return await substrate.compose(
            Call(
                "Lending",
                "close",
                {
                    "netuid": self.netuid,
                    "repay_from_wallet": self.repay_from_wallet,
                    "max_payment": payment.rao,
                    "min_refund": refund.rao,
                    "valid_until": self.valid_until,
                },
            )
        )

    def summary(self) -> str:
        return f"repay and close lending position on netuid {self.netuid}"

    def spend(self) -> Spend:
        # The ceiling also bounds any wallet TAO repayment. An AMM short close
        # uses escrow, so this can overstate free-TAO spend but cannot understate it.
        # Explicit alpha amounts are validated against stored side before dispatch.
        if isinstance(self.max_payment, Balance) and self.max_payment.netuid != 0:
            return None
        return cast(Balance, tao_amount(self.max_payment))
