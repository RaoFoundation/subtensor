"""Fixed-point money type.

A ``Balance`` is an integer amount of rao (1 TAO = 1e9 rao) tagged with the
``netuid`` whose currency it denominates. netuid 0 is TAO; any other netuid is
that subnet's alpha. Arithmetic and comparison between two balances of different
netuids raises, so mixing TAO and alpha (or two different subnets' alpha) is a
loud error rather than a silent miscalculation.

Units are explicit at every edge:

- construct with :meth:`Balance.from_tao` (TAO only), :meth:`Balance.from_alpha`
  (a subnet's alpha only), or :meth:`Balance.from_amount` (unit picked by the
  ``netuid`` argument);
- read back with ``.tao`` (raises on an alpha balance), ``.alpha`` (raises on a
  TAO balance), or ``.amount`` (the number in this balance's own currency);
- display uses the subnet's chain-registered token symbol when known (``τ1.5``
  for TAO, ``1.5β`` for subnet 2), falling back to ``α`` + a subscript netuid
  (``1.5α₄₂``). The symbol is carried *on the balance*: values decoded from
  chain state are tagged with their chain's symbol at decode time
  (``Client.balance`` / ``Substrate.balance``), so a balance renders correctly
  no matter how many clients or networks the process talks to.

All arithmetic is integer arithmetic on rao. Floats only appear at the edges
(``amount`` for display, ``from_*`` for convenience input).
"""

from __future__ import annotations

from decimal import Decimal

from .settings import ALPHA_SYMBOL, RAO_PER_TAO, TAO_SYMBOL

_SUBSCRIPT_DIGITS = "₀₁₂₃₄₅₆₇₈₉"


def _subscript(n: int) -> str:
    return "".join(_SUBSCRIPT_DIGITS[int(d)] for d in str(n))


class UnitMismatchError(TypeError):
    """Raised when combining balances denominated in different currencies."""


class Balance:
    __slots__ = ("netuid", "rao", "symbol")

    def __init__(self, rao: int, netuid: int = 0, symbol: "str | None" = None):
        self.rao = int(rao)
        self.netuid = netuid
        # Display-only: the chain-registered token symbol this balance renders
        # with, tagged at decode time. Never part of identity (eq/hash) or
        # arithmetic semantics — only the netuid is.
        self.symbol = symbol

    @classmethod
    def from_rao(cls, rao: int, netuid: int = 0, symbol: "str | None" = None) -> "Balance":
        return cls(int(rao), netuid, symbol)

    @classmethod
    def from_tao(cls, tao: "float | str | Decimal") -> "Balance":
        """Create a TAO (netuid 0) Balance from a TAO amount.

        Pass a ``str`` or ``Decimal`` for an exact amount (recommended for large
        values): a Python ``float`` literal loses precision above ~9M TAO before
        this is even called, so exactness requires a non-float input.

        For a subnet's alpha use :meth:`from_alpha`.
        """
        return cls.from_amount(tao, 0)

    @classmethod
    def from_alpha(cls, alpha: "float | str | Decimal", netuid: int) -> "Balance":
        """Create a subnet-alpha Balance from an alpha amount.

        ``netuid`` must be a real subnet (non-zero); for TAO use :meth:`from_tao`.
        """
        if netuid == 0:
            raise UnitMismatchError(
                "from_alpha requires a non-zero netuid; netuid 0 is TAO — use from_tao."
            )
        return cls.from_amount(alpha, netuid)

    @classmethod
    def from_amount(cls, amount: "float | str | Decimal", netuid: int) -> "Balance":
        """Create a Balance in the currency selected by ``netuid`` (0 = TAO, else alpha).

        Use this only where the netuid is genuinely variable; otherwise prefer
        the unit-named :meth:`from_tao` / :meth:`from_alpha`.
        """
        return cls(int((Decimal(str(amount)) * RAO_PER_TAO).to_integral_value()), netuid)

    @property
    def tao(self) -> float:
        """Numeric TAO amount. Raises on an alpha balance.

        Alpha is not TAO: to express an alpha position in TAO, value it through
        a price or swap quote first (e.g. the ``stake_value_for_coldkey`` read).
        """
        if self.netuid != 0:
            raise UnitMismatchError(
                f"This balance is subnet-{self.netuid} alpha, not TAO. Use .alpha for "
                "the alpha amount, or value it in TAO via a price/swap quote."
            )
        return self.rao / RAO_PER_TAO

    @property
    def alpha(self) -> float:
        """Numeric alpha amount. Raises on a TAO (netuid 0) balance."""
        if self.netuid == 0:
            raise UnitMismatchError("This balance is TAO (netuid 0), not alpha. Use .tao.")
        return self.rao / RAO_PER_TAO

    @property
    def amount(self) -> float:
        """Numeric amount in this balance's own currency (TAO or this subnet's alpha)."""
        return self.rao / RAO_PER_TAO

    @property
    def decimal(self) -> Decimal:
        """Exact amount in this balance's own currency, as a ``Decimal``.

        Unlike ``amount`` this never loses precision (rao is an integer and the
        divisor is a power of ten), so it is the form to use when serializing.
        """
        return Decimal(self.rao) / RAO_PER_TAO

    @property
    def unit(self) -> str:
        """Currency symbol: ``τ`` for TAO, else the chain-registered token
        symbol this balance was tagged with at decode time (e.g. ``β`` for
        subnet 2), or ``α`` + subscript netuid (``α₄₂``) for an untagged
        balance."""
        if self.netuid == 0:
            return TAO_SYMBOL
        if self.symbol:
            return self.symbol
        return ALPHA_SYMBOL + _subscript(self.netuid)

    def _same_unit(self, other: "Balance") -> None:
        if self.netuid != other.netuid:
            raise UnitMismatchError(
                f"Cannot combine balances of different currencies: "
                f"netuid {self.netuid} and netuid {other.netuid}. "
                "Set both to the same netuid first."
            )

    def __repr__(self) -> str:
        if self.netuid == 0:
            return f"{TAO_SYMBOL}{self.amount:,.9f}"
        return f"{self.amount:,.9f}{self.unit}"

    __str__ = __repr__

    def __eq__(self, other: object) -> bool:
        # Equality never raises: a different-currency balance is simply not equal,
        # so containment (`x in [...]`) and dict/set membership behave sanely.
        # Raw ints are never equal: int equality would be currency-blind and
        # inconsistent with hash((rao, netuid)). Compare via ordering or .rao.
        if isinstance(other, Balance):
            return self.netuid == other.netuid and self.rao == other.rao
        return NotImplemented

    def __hash__(self) -> int:
        return hash((self.rao, self.netuid))

    def __lt__(self, other: "Balance | int | float") -> bool:
        return self.rao < self._rao_of(other)

    def __le__(self, other: "Balance | int | float") -> bool:
        return self.rao <= self._rao_of(other)

    def __gt__(self, other: "Balance | int | float") -> bool:
        return self.rao > self._rao_of(other)

    def __ge__(self, other: "Balance | int | float") -> bool:
        return self.rao >= self._rao_of(other)

    def _rao_of(self, other: "Balance | int") -> int:
        if isinstance(other, Balance):
            self._same_unit(other)
            return other.rao
        if isinstance(other, bool):  # bool is an int subclass; reject explicitly
            raise TypeError("Cannot compare Balance with bool")
        if isinstance(other, int):
            return other
        if isinstance(other, float):
            raise TypeError(
                "Compare a Balance against another Balance or an int (rao). "
                "For a TAO amount use tao(x): `balance > tao(0.5)`."
            )
        raise TypeError(f"Cannot compare Balance with {type(other).__name__}")

    def _symbol_with(self, other: "Balance | int") -> "str | None":
        """Symbol for an arithmetic result: either operand's tag (same netuid,
        so there is no ambiguity)."""
        return self.symbol or (other.symbol if isinstance(other, Balance) else None)

    def __add__(self, other: "Balance | int") -> "Balance":
        return Balance(self.rao + self._rao_of(other), self.netuid, self._symbol_with(other))

    def __sub__(self, other: "Balance | int") -> "Balance":
        return Balance(self.rao - self._rao_of(other), self.netuid, self._symbol_with(other))

    def __neg__(self) -> "Balance":
        return Balance(-self.rao, self.netuid, self.symbol)

    def __bool__(self) -> bool:
        return bool(self.rao)


def tao(amount: float) -> Balance:
    """A TAO (netuid 0) Balance."""
    return Balance.from_tao(amount)


def alpha(amount: float, netuid: int) -> Balance:
    """A subnet-alpha Balance (netuid must be non-zero)."""
    return Balance.from_alpha(amount, netuid)


def rao(amount: int, netuid: int = 0) -> Balance:
    return Balance.from_rao(amount, netuid)
