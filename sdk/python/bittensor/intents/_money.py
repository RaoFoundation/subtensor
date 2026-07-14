"""Unit-safe money for intents: exact amounts in, exact amounts held.

A money field is denominated in a fixed unit set by the operation: a ``*_tao``
field is TAO (netuid 0), an ``*_alpha`` field is that subnet's alpha. Callers
pass a number, a decimal string (exact — the recommended form for large
amounts), or a :class:`Balance` (validated against the expected netuid and
rejected on mismatch). Whatever comes in, the field normalizes to a
:class:`Balance` at construction and never holds a float, so rao amounts are
exact end to end and the wrong unit cannot enter the one write path.

Intents that support draining a position also accept the sentinel string
``"all"`` (opt-in via ``allow_all``); the concrete amount is resolved from
chain state at build time.

``UNBOUNDED`` is the spend sentinel for :meth:`Intent.spend`: the intent moves
or burns an amount the SDK can't cheaply bound, so a spend cap must block it
until raised.
"""

from __future__ import annotations

from decimal import Decimal, InvalidOperation
from typing import Optional, Union

from .._generated import calls as generated_calls
from ..balance import Balance, UnitMismatchError
from ..result import BittensorError

# What a money field accepts at the edge; it is normalized to a Balance at
# construction. The JSON schema for these fields is number-or-string.
Money = Union[int, float, str, Decimal, Balance]

# Sentinel accepted by opt-in money fields: "use everything available".
ALL = "all"


class _Unbounded:
    """Sentinel spend: an amount the SDK can't cheaply bound before execution."""

    def __repr__(self) -> str:
        return "UNBOUNDED"


UNBOUNDED = _Unbounded()

# The result of Intent.spend(): a bounded TAO amount, UNBOUNDED, or None (no
# value leaves the signer).
Spend = Union[Balance, _Unbounded, None]


def _amount(value: Money, netuid: int, label: str, *, allow_all: bool = False) -> "Balance | str":
    if isinstance(value, str) and value.strip().lower() == ALL:
        if not allow_all:
            raise BittensorError(f"{label} does not accept 'all'.")
        return ALL
    if isinstance(value, Balance):
        if value.netuid != netuid:
            raise UnitMismatchError(
                f"{label} expects a Balance tagged netuid {netuid} "
                f"({'TAO' if netuid == 0 else 'alpha'}), got netuid {value.netuid}."
            )
        balance = value
    else:
        try:
            balance = Balance.from_amount(value, netuid)
        except (InvalidOperation, OverflowError, ValueError, TypeError):
            raise BittensorError(f"{label} must be a finite number, got {value!r}.") from None
    if balance.rao < 0:
        raise BittensorError(f"{label} must be non-negative, got {balance}.")
    return balance


def tao_amount(value: Money, *, allow_all: bool = False) -> "Balance | str":
    """Normalize a TAO amount (number, decimal string, or netuid-0 Balance) to an
    exact TAO Balance."""
    return _amount(value, 0, "TAO amount", allow_all=allow_all)


def alpha_amount(value: Money, netuid: int, *, allow_all: bool = False) -> "Balance | str":
    """Normalize an alpha amount (number, decimal string, or netuid-matched
    Balance) to an exact Balance in the subnet's currency."""
    return _amount(value, netuid, f"alpha amount for netuid {netuid}", allow_all=allow_all)


# Unit per (pallet, call, param) for pre-newtype metadata, derived from the
# runtime source (pallets/subtensor/src/macros/dispatches.rs). Once the chain
# metadata carries the TaoBalance/AlphaBalance newtypes, the regenerated call
# builders' type idents take precedence over this table.
_FALLBACK_AMOUNT_UNITS: dict[tuple[str, str, str], str] = {
    ("SubtensorModule", "add_stake", "amount_staked"): "TAO",
    ("SubtensorModule", "add_stake_limit", "amount_staked"): "TAO",
    ("SubtensorModule", "add_stake_limit", "limit_price"): "TAO",
    ("SubtensorModule", "remove_stake", "amount_unstaked"): "ALPHA",
    ("SubtensorModule", "remove_stake_limit", "amount_unstaked"): "ALPHA",
    ("SubtensorModule", "remove_stake_limit", "limit_price"): "TAO",
    ("SubtensorModule", "move_stake", "alpha_amount"): "ALPHA",
    ("SubtensorModule", "transfer_stake", "alpha_amount"): "ALPHA",
    ("SubtensorModule", "swap_stake", "alpha_amount"): "ALPHA",
}

# Call-arg type identities that name a money unit.
_IDENT_UNITS = {"TaoBalance": "TAO", "AlphaBalance": "ALPHA"}


def call_amount_unit(pallet: str, call: str, param: str) -> Optional[str]:
    """``"TAO"`` or ``"ALPHA"`` for one extrinsic parameter, or None.

    The generated call builders carry each parameter's runtime type identity
    in their (string) annotations; when the metadata declares the param
    TaoBalance/AlphaBalance that decides. Pre-newtype metadata has bare u64
    amounts, so the hand table above is the fallback truth.
    """
    builder = getattr(getattr(generated_calls, pallet, None), call, None)
    ident = getattr(builder, "__annotations__", {}).get(param)
    unit = _IDENT_UNITS.get(ident) if isinstance(ident, str) else None
    return unit or _FALLBACK_AMOUNT_UNITS.get((pallet, call, param))


def call_amount(
    value: Money,
    wraps: tuple[str, str],
    param: str,
    *,
    netuid: int,
    allow_all: bool = False,
) -> "Balance | str":
    """Normalize an extrinsic-bound amount, enforcing the parameter's unit.

    ``wraps`` is the (pallet, call) the amount feeds and ``netuid`` the subnet
    whose alpha an ALPHA parameter denominates (TAO parameters are netuid 0).
    A :class:`Balance` tagged with the other unit raises a ValueError naming
    the extrinsic, the parameter, and both units; plain numbers and decimal
    strings pass through unchanged normalization.
    """
    pallet, call = wraps
    unit = call_amount_unit(pallet, call, param)
    if unit is None:
        raise BittensorError(f"no unit declared for {pallet}.{call} parameter {param!r}")
    expected_netuid = 0 if unit == "TAO" else netuid
    if isinstance(value, Balance) and value.netuid != expected_netuid:
        want = "TAO" if expected_netuid == 0 else f"subnet-{expected_netuid} ALPHA"
        got = "TAO" if value.netuid == 0 else f"subnet-{value.netuid} ALPHA"
        hint = (
            "Balance.from_tao(...)"
            if expected_netuid == 0
            else f"Balance.from_alpha(..., netuid={expected_netuid})"
        )
        raise ValueError(
            f"{call}: parameter {param!r} takes {want}, got a Balance tagged {got}. "
            f"Construct the amount with {hint}."
        )
    label = "TAO amount" if unit == "TAO" else f"alpha amount for netuid {expected_netuid}"
    return _amount(value, expected_netuid, label, allow_all=allow_all)
