"""``Plan`` and ``Policy``: preview an intent and gate it before execution.

``Plan`` is the result of a dry run — predicted fee, effects, warnings, and any
policy violations — with the built call attached (not serialized) so ``execute``
can submit exactly what was previewed. ``Policy`` is caller-supplied guardrails
enforced at the execute choke point.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._policy import evaluate_policy
from ..balance import Balance
from ..settings import tx_docs_url
from ._money import UNBOUNDED, Money, Spend, tao_amount
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
        reasons = evaluate_policy(
            "transaction.intent_policy", {"raw": True, "allow_raw_calls": self.allow_raw_calls}
        )["violations"]
        return [
            "raw call submission is disabled by policy (set allow_raw_calls=True)" for _ in reasons
        ]

    def check(
        self, intent: Intent, fee: Optional[Balance], *, additional_spend: Optional[Balance] = None
    ) -> list[str]:
        intent = intent.semantic_intent()
        spend = intent.spend() if self.max_spend_tao is not None else None
        result = evaluate_policy(
            "transaction.intent_policy",
            {
                "max_fee_rao": None if self.max_fee_tao is None else str(self.max_fee_tao.rao),
                "max_spend_rao": None
                if self.max_spend_tao is None
                else str(self.max_spend_tao.rao),
                "fee_rao": None if fee is None else str(fee.rao),
                "spend_rao": "unbounded"
                if spend is UNBOUNDED
                else None
                if spend is None
                else str(spend.rao),
                "additional_spend_rao": None
                if additional_spend is None
                else str(additional_spend.rao),
                "allowed_netuids": self.allowed_netuids,
                "netuids": list(intent.touches_netuids())
                if self.allowed_netuids is not None
                else [],
                "affects_all_subnets": intent.affects_all_subnets()
                if self.allowed_netuids is not None
                else False,
            },
        )
        violations: list[str] = []
        for reason in result["violations"]:
            code = reason["code"]
            if code == "fee_unknown":
                violations.append(
                    f"max_fee_tao {self.max_fee_tao} is configured "
                    "but the fee could not be estimated"
                )
            elif code == "fee":
                violations.append(f"fee {fee} exceeds max_fee_tao {self.max_fee_tao}")
            elif code == "unbounded":
                violations.append(
                    f"{intent.op} spends an amount that cannot be bounded before execution; "
                    f"max_spend_tao {self.max_spend_tao} blocks it"
                )
            elif code == "spend":
                violations.append(
                    f"spend {Balance.from_rao(int(reason['actual']))} "
                    f"exceeds max_spend_tao {self.max_spend_tao}"
                )
            elif code == "all_subnets":
                violations.append(
                    f"{intent.op} acts on all subnets, "
                    f"not just allowed_netuids {self.allowed_netuids}"
                )
            elif code == "netuid":
                violations.append(
                    f"netuid {reason['netuid']} not in allowed_netuids {self.allowed_netuids}"
                )
            else:
                raise RuntimeError(f"unknown transaction policy violation {code}")
        return violations


@dataclass
class Plan:
    """A previewed, not-yet-submitted intent.

    ``to_dict`` is the machine-readable pre-approval record: alongside the
    fee, effects, warnings, and policy verdict it carries the exact spend
    (``spend_tao``, or ``spend_unbounded`` when the amount cannot be bounded
    before execution — full-balance sweeps and ownership transfers) and the
    intent's exact parsed arguments, so an agent can verify precisely what
    would run before anyone signs.
    """

    op: str
    summary: str
    signer: str
    signer_address: Optional[str]
    fee: Optional[Balance]  # total estimate, including Shield's carrier when selected
    effects: list[str]
    warnings: list[str]
    violations: list[str] = field(default_factory=list)
    call: Any = field(default=None, repr=False)  # built call; not serialized
    extras: dict[str, Any] = field(default_factory=dict)  # build-time data for the result
    # TAO leaving the signer: a Balance when bounded, UNBOUNDED, or None.
    spend: Spend = None
    # The intent's own parameters, JSON-native (money as exact decimal strings).
    args: Optional[dict[str, Any]] = None
    # Structured, JSON-native counterpart of ``effects`` for intents whose
    # preview has a fixed shape (e.g. a before/after vector). Empty when the
    # intent only produces prose.
    preview: dict[str, Any] = field(default_factory=dict)
    # Route selected before quoting; execute must retain it.
    shielded: bool = False

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
            "args": self.args,
            "fee_tao": self.fee.tao if self.fee is not None else None,
            "shielded": self.shielded,
            "spend_tao": self.spend.tao if isinstance(self.spend, Balance) else None,
            "spend_unbounded": self.spend is UNBOUNDED,
            "effects": self.effects,
            "warnings": self.warnings,
            "violations": self.violations,
            "ok": self.ok,
            "preview": self.preview,
            "docs_url": tx_docs_url(self.op),
        }
