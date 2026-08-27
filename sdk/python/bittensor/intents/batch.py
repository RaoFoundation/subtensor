"""Atomic composition: several intents in one extrinsic.

``Batch`` wraps ``Utility.batch_all``: every child call succeeds or the whole
extrinsic reverts — no partially-applied multi-step operations. Children are
ordinary intents, so policy aggregates over them (total spend, union of
netuids), the plan lists every child's effects, and the CLI/tool manifest get
the command for free like any other intent.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass, field
from typing import Any

from .._generated import calls
from ..balance import Balance
from ._affordability import SpendProfile
from ._money import UNBOUNDED, Spend
from .base import BuiltCall, Intent, IntentPreflight
from .registry import build as build_intent
from .registry import register


def _sum_spends(spends: Iterable[Spend]) -> Spend:
    total = Balance.from_rao(0)
    bounded = False
    for spend in spends:
        if spend is UNBOUNDED:
            return UNBOUNDED
        if spend is not None:
            total += spend
            bounded = True
    return total if bounded else None


@register
@dataclass
class Batch(Intent):
    """Execute several intents atomically in one extrinsic (all-or-nothing).

    Wraps the child calls in ``Utility.batch_all``: they run in order and if
    any one fails the whole extrinsic reverts, so there is never a
    partially-applied result. Use it for multi-step operations that must land
    together (e.g. move funds then act on them) instead of submitting the
    steps separately and risking a half-done state. All children must share
    one signer — an extrinsic has a single signature — and batches cannot
    contain other batches. Spend limits and policy checks aggregate across
    every child, and the transaction plan lists each child's effects.
    """

    op = "batch"
    wraps = (("Utility", "batch_all"),)

    intents: list = field(
        metadata={
            "help": "The calls to execute, in order, as a JSON list of objects "
            '{"op": <intent name>, ...args}. At least one; all must share a signer; '
            "batches cannot nest."
        }
    )

    def __post_init__(self):
        if not self.intents:
            raise ValueError("batch requires at least one intent")
        children: list[Intent] = []
        for item in self.intents:
            if isinstance(item, Batch) or (isinstance(item, dict) and item.get("op") == self.op):
                raise ValueError("nested batches are not supported")
            if isinstance(item, Intent):
                children.append(item)
            elif isinstance(item, dict):
                args = dict(item)
                op = args.pop("op", None)
                if not op:
                    raise ValueError(f"batched intent needs an 'op' key: {item!r}")
                children.append(build_intent(op, args))
            else:
                raise ValueError(f"batched intent must be a dict or Intent, got {type(item)!r}")
        signers = {child.signer for child in children}
        if len(signers) > 1:
            raise ValueError(
                f"all batched intents must share one signer, got {sorted(signers)}; "
                "split into separate batches"
            )
        # One extrinsic has one signer: the batch takes on its children's
        # (instance attribute shadowing the ClassVar default).
        self.signer = signers.pop()
        self.intents = [child.to_dict() for child in children]
        self._children = children

    async def build(self, substrate, wallet: Any):
        composed = []
        extras: dict[str, Any] = {}
        for index, child in enumerate(self._children):
            built = await child.build(substrate, wallet)
            if isinstance(built, BuiltCall):
                composed.append(built.call)
                extras.update(
                    {f"{index}:{child.op}.{key}": value for key, value in built.extras.items()}
                )
            else:
                composed.append(built)
        batch = await substrate.compose(calls.Utility.batch_all(calls=composed))
        return BuiltCall(batch, extras) if extras else batch

    def summary(self) -> str:
        return f"atomic batch of {len(self._children)}: " + "; ".join(
            child.summary() for child in self._children
        )

    async def effects(self, substrate, signer_address: str) -> list[str]:
        out = [f"all-or-nothing: {len(self._children)} calls in one extrinsic"]
        for index, child in enumerate(self._children):
            out.extend(f"[{index}] {e}" for e in await child.effects(substrate, signer_address))
        return out

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        out: list[str] = []
        for index, child in enumerate(self._children):
            out.extend(f"[{index}] {w}" for w in await child.warnings(substrate, signer_address))
        return out

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        out: list[str] = []
        for index, child in enumerate(self._children):
            out.extend(f"[{index}] {b}" for b in await child.blocks(substrate, signer_address))
        return out

    async def preflight(
        self, substrate, dispatch_origin: str, fee_payer: str, *, call=None
    ) -> IntentPreflight:
        effects = [f"all-or-nothing: {len(self._children)} calls in one extrinsic"]
        warnings: list[str] = []
        blocks: list[str] = []
        required_free: Balance | None = None
        available_free: Balance | None = None
        estimated_fee: Balance | None = None
        spend_profiles: list[SpendProfile] = []
        for index, child in enumerate(self._children):
            child_preflight = await child.preflight(
                substrate, dispatch_origin, fee_payer, call=call
            )
            effects.extend(f"[{index}] {item}" for item in child_preflight.effects)
            warnings.extend(f"[{index}] {item}" for item in child_preflight.warnings)
            blocks.extend(f"[{index}] {item}" for item in child_preflight.blocks)
            spend_profiles.append(child_preflight.spend_profile)
            if child_preflight.required_free is not None and (
                required_free is None or child_preflight.required_free.rao > required_free.rao
            ):
                # Every child sees the same fully composed batch call, so its
                # reserve describes that one extrinsic; do not sum duplicates.
                required_free = child_preflight.required_free
                available_free = child_preflight.available_free
                estimated_fee = child_preflight.estimated_fee
        return IntentPreflight(
            effects=effects,
            warnings=warnings,
            blocks=blocks,
            required_free=required_free,
            available_free=available_free,
            estimated_fee=estimated_fee,
            spend_profile=SpendProfile.combine(spend_profiles),
        )

    def spend(self) -> Spend:
        """Aggregate TAO spend across children; any unbounded child makes the
        whole batch unbounded."""
        return _sum_spends(child.spend() for child in self._children)

    def touches_netuids(self) -> list[int]:
        return sorted({netuid for child in self._children for netuid in child.touches_netuids()})

    def affects_all_subnets(self) -> bool:
        return any(child.affects_all_subnets() for child in self._children)
