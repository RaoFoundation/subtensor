"""``Plan`` and ``Policy``: preview an intent and gate it before execution.

``Plan`` is the result of a dry run — predicted fee, effects, warnings, and any
policy violations — with the built call attached (not serialized) so ``execute``
can submit exactly what was previewed. ``Policy`` is caller-supplied guardrails
enforced at the execute choke point.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from ..balance import Balance
from ..settings import tx_docs_url
from ._money import UNBOUNDED, Money, tao_amount
from .base import Intent


@dataclass
class Policy:
    """Safety limits enforced when an intent is executed.

    All limits are optional; an unset limit is not enforced. Money limits
    accept a number, a decimal string, or a TAO ``Balance``, and are
    normalized to an exact ``Balance`` at construction — the comparisons that
    guard real money are integer rao, never floats.
    """

    max_fee_tao: Optional[Money] = None
    max_spend_tao: Optional[Money] = None
    allowed_netuids: Optional[list[int]] = None
    # Raw calls (client.submit_call) bypass intent preview, so a policy can't
    # bound their spend or netuids. They are therefore refused outright unless
    # the policy explicitly opts in.
    allow_raw_calls: bool = False

    def __post_init__(self):
        if self.max_fee_tao is not None:
            self.max_fee_tao = tao_amount(self.max_fee_tao)
        if self.max_spend_tao is not None:
            self.max_spend_tao = tao_amount(self.max_spend_tao)

    def check_raw_call(self) -> list[str]:
        """Violations for a raw call (``client.submit_call``), which has no intent
        to inspect — so it is refused outright unless ``allow_raw_calls`` opts in."""
        if self.allow_raw_calls:
            return []
        return ["raw call submission is disabled by policy (set allow_raw_calls=True)"]

    def check(self, intent: Intent, fee: Optional[Balance]) -> list[str]:
        violations: list[str] = []
        if self.max_fee_tao is not None:
            # A fee cap must not fail open: an unavailable estimate blocks
            # execution rather than silently skipping the cap.
            if fee is None:
                violations.append(
                    f"max_fee_tao {self.max_fee_tao} is configured but the fee "
                    "could not be estimated"
                )
            elif fee > self.max_fee_tao:
                violations.append(f"fee {fee} exceeds max_fee_tao {self.max_fee_tao}")
        if self.max_spend_tao is not None:
            spend = intent.spend()
            if spend is UNBOUNDED:
                violations.append(
                    f"{intent.op} spends an amount that cannot be bounded before "
                    f"execution; max_spend_tao {self.max_spend_tao} blocks it"
                )
            elif spend is not None and spend > self.max_spend_tao:
                violations.append(f"spend {spend} exceeds max_spend_tao {self.max_spend_tao}")
        if self.allowed_netuids is not None:
            allowed = set(self.allowed_netuids)
            if intent.affects_all_subnets():
                violations.append(
                    f"{intent.op} acts on all subnets, not just allowed_netuids "
                    f"{self.allowed_netuids}"
                )
            for netuid in intent.touches_netuids():
                if netuid not in allowed:
                    violations.append(
                        f"netuid {netuid} not in allowed_netuids {self.allowed_netuids}"
                    )
        return violations


@dataclass
class Plan:
    """A previewed, not-yet-submitted intent."""

    op: str
    summary: str
    signer: str
    signer_address: Optional[str]
    fee: Optional[Balance]
    effects: list[str]
    warnings: list[str]
    violations: list[str] = field(default_factory=list)
    call: Any = field(default=None, repr=False)  # built call; not serialized
    extras: dict[str, Any] = field(default_factory=dict)  # build-time data for the result

    @property
    def ok(self) -> bool:
        """True when there are no policy violations blocking execution."""
        return not self.violations

    def to_dict(self) -> dict[str, Any]:
        return {
            "op": self.op,
            "summary": self.summary,
            "signer": self.signer,
            "signer_address": self.signer_address,
            "fee_tao": self.fee.tao if self.fee is not None else None,
            "effects": self.effects,
            "warnings": self.warnings,
            "violations": self.violations,
            "ok": self.ok,
            "docs_url": tx_docs_url(self.op),
        }
