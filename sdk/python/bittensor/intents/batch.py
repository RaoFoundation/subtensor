"""Atomic composition: several intents in one extrinsic.

``Batch`` wraps ``Utility.batch_all``: every child call succeeds or the whole
extrinsic reverts — no partially-applied multi-step operations. Children are
ordinary intents, so policy aggregates over them (total spend, union of
netuids), the plan lists every child's effects, and the CLI/tool manifest get
the command for free like any other intent.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .._generated import calls
from ..balance import Balance
from ._money import UNBOUNDED, Spend
from .base import BuiltCall, Intent
from .registry import build as build_intent
from .registry import register


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

    def spend(self) -> Spend:
        """Aggregate TAO spend across children; any unbounded child makes the
        whole batch unbounded."""
        total = Balance.from_rao(0)
        bounded = False
        for child in self._children:
            child_spend = child.spend()
            if child_spend is UNBOUNDED:
                return UNBOUNDED
            if child_spend is not None:
                total = total + child_spend
                bounded = True
        return total if bounded else None

    def touches_netuids(self) -> list[int]:
        return sorted({netuid for child in self._children for netuid in child.touches_netuids()})

    def affects_all_subnets(self) -> bool:
        return any(child.affects_all_subnets() for child in self._children)
