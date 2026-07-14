"""Crowdloans: raise funds toward a cap that, on success, funds a target or call.

A creator opens a crowdloan with an initial deposit, cap, and end block, choosing
either a ``target_ss58`` to receive the raised funds or an inner ``call`` to
dispatch (exactly one). Contributors add funds; if the cap is reached the creator
finalizes, otherwise the creator refunds contributors (or they withdraw
themselves) and dissolves the loan. Amounts are TAO.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .._generated import calls
from ._money import Money, Spend, tao_amount
from .base import BuiltCall, Intent
from .registry import build as build_intent
from .registry import register

CROWDLOAN_ID_HELP = "Identifier of the crowdloan, assigned when it was created."


@register
@dataclass
class CreateCrowdloan(Intent):
    """Open a crowdloan raising up to ``cap_tao`` by ``end`` block.

    Locks ``deposit_tao`` from the creator (it counts toward the raise and
    comes back when the loan is dissolved) and opens the loan for
    contributions until ``end``. Provide exactly one of ``target_ss58`` (the
    address that receives the raised funds on finalize) or ``call`` (an inner
    intent, ``{"op": ..., ...args}``, dispatched with the creator's origin on
    finalize) — supplying both or neither is rejected. If the cap is reached
    the creator finalizes with ``finalize_crowdloan``. Passing ``end`` only
    stops new contributions — nothing is refunded automatically: the creator
    returns contributions with ``refund_crowdloan`` (contributors may also
    ``withdraw_crowdloan`` themselves) and then ``dissolve_crowdloan`` to
    recover the deposit.
    """

    op = "create_crowdloan"
    signer = "coldkey"
    wraps = (("Crowdloan", "create"),)

    deposit_tao: Money = field(
        metadata={
            "help": "Creator's initial deposit, locked when the crowdloan opens. It counts "
            "toward the raise and is returned when the crowdloan is dissolved."
        }
    )
    min_contribution_tao: Money = field(
        metadata={"help": "Smallest contribution the crowdloan will accept from a contributor."}
    )
    cap_tao: Money = field(
        metadata={"help": "Maximum total to raise. Reaching the cap lets the creator finalize."}
    )
    end: int = field(
        metadata={
            "help": "Block number at which the crowdloan stops accepting contributions. "
            "Passing it refunds nothing by itself; refunds happen via explicit "
            "refund or withdraw calls."
        }
    )
    target_ss58: Optional[str] = field(
        default=None,
        metadata={
            "help": "Account that receives the raised funds on finalize. Provide exactly "
            "one of this or call."
        },
    )
    call: Optional[dict] = field(
        default=None,
        metadata={
            "help": "Inner intent dispatched with the creator's origin on finalize, as a "
            'JSON object {"op": <name>, ...args}. Provide exactly one of this or '
            "target_ss58."
        },
    )

    def __post_init__(self):
        if bool(self.target_ss58) == bool(self.call):
            raise ValueError("provide exactly one of target_ss58 or call")
        self.deposit_tao = tao_amount(self.deposit_tao)
        self.min_contribution_tao = tao_amount(self.min_contribution_tao)
        self.cap_tao = tao_amount(self.cap_tao)

    async def build(self, substrate, wallet: Any):
        inner = None
        if self.call:
            args = dict(self.call)
            op = args.pop("op", None)
            if not op:
                raise ValueError("crowdloan 'call' needs an 'op' key")
            built = await build_intent(op, args).build(substrate, wallet)
            inner = built.call if isinstance(built, BuiltCall) else built
        return await substrate.compose(
            calls.Crowdloan.create(
                deposit=self.deposit_tao.rao,
                min_contribution=self.min_contribution_tao.rao,
                cap=self.cap_tao.rao,
                end=self.end,
                call=inner,
                target_address=self.target_ss58,
            )
        )

    def summary(self) -> str:
        return f"create crowdloan (cap {self.cap_tao}, deposit {self.deposit_tao}, end {self.end})"

    def spend(self) -> Spend:
        return self.deposit_tao


@register
@dataclass
class ContributeCrowdloan(Intent):
    """Contribute TAO to an active crowdloan.

    Moves the amount from the signer into the crowdloan's pot. The contribution
    must meet the loan's minimum (and per-contributor maximum, if set) and the
    loan must still be open — contributing after ``end`` or past the cap fails.
    Contributions are not refunded automatically if the loan fails: they come
    back when the creator calls ``refund_crowdloan``, or the contributor can
    ``withdraw_crowdloan`` themselves any time before finalization. On
    success, the funds go to the loan's target or fund its inner call.
    """

    op = "contribute_crowdloan"
    signer = "coldkey"
    wraps = (("Crowdloan", "contribute"),)

    crowdloan_id: int = field(metadata={"help": CROWDLOAN_ID_HELP})
    amount_tao: Money = field(
        metadata={
            "help": "Amount to contribute; moved into the crowdloan's pot. Recoverable "
            "via refund or withdraw while the loan is not finalized."
        }
    )

    def __post_init__(self):
        self.amount_tao = tao_amount(self.amount_tao)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Crowdloan.contribute(crowdloan_id=self.crowdloan_id, amount=self.amount_tao.rao)
        )

    def summary(self) -> str:
        return f"contribute {self.amount_tao} to crowdloan {self.crowdloan_id}"

    def spend(self) -> Spend:
        return self.amount_tao


@dataclass
class _CrowdloanId(Intent):
    """Base for the crowdloan operations that take only an id."""

    crowdloan_id: int = field(metadata={"help": CROWDLOAN_ID_HELP})


@register
@dataclass
class FinalizeCrowdloan(_CrowdloanId):
    """Finalize a crowdloan that reached its cap (creator only).

    Settles a successful raise: the pot is transferred to the loan's target
    address, or its inner call is dispatched with the creator's origin. Only
    the creator may finalize, and only once the cap has been reached. After
    finalization the crowdloan can no longer be updated, refunded, or
    dissolved.
    """

    op = "finalize_crowdloan"
    signer = "coldkey"
    wraps = (("Crowdloan", "finalize"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Crowdloan.finalize(crowdloan_id=self.crowdloan_id))

    def summary(self) -> str:
        return f"finalize crowdloan {self.crowdloan_id}"


@register
@dataclass
class RefundCrowdloan(_CrowdloanId):
    """Refund contributors of a non-finalized crowdloan (creator only).

    Returns contributions (excluding the creator's) to their contributors.
    Only the creator may call it. Each call refunds at most 50 contributors,
    so large loans may need several ``refund_crowdloan`` calls before
    everyone is paid back. Once all contributors are refunded, the creator
    runs ``dissolve_crowdloan``, which returns the creator's remaining
    contribution (including the deposit) and removes the loan.
    """

    op = "refund_crowdloan"
    signer = "coldkey"
    wraps = (("Crowdloan", "refund"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Crowdloan.refund(crowdloan_id=self.crowdloan_id))

    def summary(self) -> str:
        return f"refund contributors of crowdloan {self.crowdloan_id}"


@register
@dataclass
class DissolveCrowdloan(_CrowdloanId):
    """Dissolve a fully-refunded crowdloan (creator only).

    Removes a non-finalized crowdloan from chain state once every non-creator
    contribution has been returned (run ``refund_crowdloan`` until no other
    contributors remain). Dissolving automatically transfers the creator's
    remaining contribution — including the deposit — back to them. Only the
    creator may dissolve. Dissolving while other contributions are still
    outstanding fails — refund first.
    """

    op = "dissolve_crowdloan"
    signer = "coldkey"
    wraps = (("Crowdloan", "dissolve"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Crowdloan.dissolve(crowdloan_id=self.crowdloan_id))

    def summary(self) -> str:
        return f"dissolve crowdloan {self.crowdloan_id}"


@register
@dataclass
class WithdrawCrowdloan(_CrowdloanId):
    """Withdraw the signer's contribution from a non-finalized crowdloan.

    Contributors can pull their contribution back out of any crowdloan that
    has not finalized, including while it is still raising. The creator may
    only withdraw the part of their contribution above the initial deposit;
    the deposit itself stays locked until ``dissolve_crowdloan`` returns it.
    Fails once the crowdloan has finalized or if the signer has nothing left
    to withdraw.
    """

    op = "withdraw_crowdloan"
    signer = "coldkey"
    wraps = (("Crowdloan", "withdraw"),)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(calls.Crowdloan.withdraw(crowdloan_id=self.crowdloan_id))

    def summary(self) -> str:
        return f"withdraw from crowdloan {self.crowdloan_id}"


@register
@dataclass
class UpdateCrowdloanCap(Intent):
    """Update the cap of a non-finalized crowdloan (creator only).

    Raises or lowers the total the crowdloan can collect. Only the creator may
    change it, and only while the loan has not been finalized. Reaching the
    (new) cap is what allows the creator to finalize.
    """

    op = "update_crowdloan_cap"
    signer = "coldkey"
    wraps = (("Crowdloan", "update_cap"),)

    crowdloan_id: int = field(metadata={"help": CROWDLOAN_ID_HELP})
    new_cap_tao: Money = field(metadata={"help": "New maximum total the crowdloan can raise."})

    def __post_init__(self):
        self.new_cap_tao = tao_amount(self.new_cap_tao)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Crowdloan.update_cap(crowdloan_id=self.crowdloan_id, new_cap=self.new_cap_tao.rao)
        )

    def summary(self) -> str:
        return f"update crowdloan {self.crowdloan_id} cap to {self.new_cap_tao}"


@register
@dataclass
class UpdateCrowdloanEnd(Intent):
    """Update the end block of a non-finalized crowdloan (creator only).

    Extends or shortens the contribution window. The new end is re-validated
    against the current block: it must be in the future and fall within the
    chain's minimum/maximum crowdloan duration (roughly 7 to 60 days)
    measured from now. Only the creator may change it, and only while the
    loan has not been finalized.
    """

    op = "update_crowdloan_end"
    signer = "coldkey"
    wraps = (("Crowdloan", "update_end"),)

    crowdloan_id: int = field(metadata={"help": CROWDLOAN_ID_HELP})
    new_end: int = field(
        metadata={
            "help": "New block number at which the crowdloan stops accepting contributions. "
            "Must be in the future and roughly 7-60 days from the current block."
        }
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Crowdloan.update_end(crowdloan_id=self.crowdloan_id, new_end=self.new_end)
        )

    def summary(self) -> str:
        return f"update crowdloan {self.crowdloan_id} end to block {self.new_end}"


@register
@dataclass
class UpdateCrowdloanMinContribution(Intent):
    """Update the minimum contribution of a non-finalized crowdloan (creator only).

    Changes the smallest contribution the crowdloan will accept from that
    point on; contributions already made are unaffected. Only the creator may
    change it, and only while the loan has not been finalized.
    """

    op = "update_crowdloan_min_contribution"
    signer = "coldkey"
    wraps = (("Crowdloan", "update_min_contribution"),)

    crowdloan_id: int = field(metadata={"help": CROWDLOAN_ID_HELP})
    new_min_contribution_tao: Money = field(
        metadata={"help": "New smallest contribution the crowdloan will accept."}
    )

    def __post_init__(self):
        self.new_min_contribution_tao = tao_amount(self.new_min_contribution_tao)

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.Crowdloan.update_min_contribution(
                crowdloan_id=self.crowdloan_id,
                new_min_contribution=self.new_min_contribution_tao.rao,
            )
        )

    def summary(self) -> str:
        return (
            f"update crowdloan {self.crowdloan_id} min contribution "
            f"to {self.new_min_contribution_tao}"
        )


@register
@dataclass
class SetCrowdloanMaxContribution(Intent):
    """Set or clear the per-contributor max for a non-finalized crowdloan (creator only).

    Caps how much any single contributor can put in, useful to keep a raise
    broadly distributed. Omitting the amount clears the limit. Only the
    creator may change it, and only while the loan has not been finalized.
    """

    op = "set_crowdloan_max_contribution"
    signer = "coldkey"
    wraps = (("Crowdloan", "set_max_contribution"),)

    crowdloan_id: int = field(metadata={"help": CROWDLOAN_ID_HELP})
    new_max_contribution_tao: Optional[Money] = field(
        default=None,
        metadata={
            "help": "Largest total any single contributor may put in. Omit to clear the limit."
        },
    )

    def __post_init__(self):
        if self.new_max_contribution_tao is not None:
            self.new_max_contribution_tao = tao_amount(self.new_max_contribution_tao)

    async def build(self, substrate, wallet: Any):
        new_max = (
            None if self.new_max_contribution_tao is None else self.new_max_contribution_tao.rao
        )
        return await substrate.compose(
            calls.Crowdloan.set_max_contribution(
                crowdloan_id=self.crowdloan_id, new_max_contribution=new_max
            )
        )

    def summary(self) -> str:
        if self.new_max_contribution_tao is None:
            return f"clear crowdloan {self.crowdloan_id} max contribution"
        return (
            f"set crowdloan {self.crowdloan_id} max contribution to {self.new_max_contribution_tao}"
        )
