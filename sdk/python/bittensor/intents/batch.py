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
from .._policy import evaluate_policy
from ..balance import Balance
from ._money import UNBOUNDED, Spend
from .base import BuiltCall, Intent, IntentPreflight
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

    def requires_build_for_policy(self) -> bool:
        return any(child.requires_build_for_policy() for child in self._children)

    intents: list = field(
        metadata={
            "help": "The calls to execute, in order, as a JSON list of objects "
            '{"op": <intent name>, ...args}. At least one; all must share a signer; '
            "batches cannot nest."
        }
    )

    def __post_init__(self):
        children: list[Intent] = []
        for item in self.intents:
            evaluate_policy(
                "transaction.merge_intents",
                {
                    "batch": True,
                    "children": [
                        {
                            "op": item.op
                            if isinstance(item, Intent)
                            else item.get("op")
                            if isinstance(item, dict)
                            else None,
                            "signer": "coldkey",
                        }
                    ],
                },
            )
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
        try:
            merged = evaluate_policy(
                "transaction.merge_intents",
                {
                    "batch": True,
                    "children": [{"op": child.op, "signer": child.signer} for child in children],
                },
            )
        except ValueError as error:
            if "share a signer" in str(error):
                raise ValueError(
                    f"all batched intents must share one signer, got "
                    f"{sorted({child.signer for child in children})}; split into separate batches"
                ) from error
            raise
        self.signer = merged["signer"]
        self.intents = [child.to_dict() for child in children]
        self._children = children

    async def build(self, substrate, wallet: Any):
        from ..hashed import prepare_recipient_intent, with_recipient_registration

        composed = []
        extras: dict[str, Any] = {}
        maximum_reserves: dict[str, int] = {}
        for index, child in enumerate(self._children):
            prepared, recipients = await prepare_recipient_intent(substrate, child)
            built = await prepared.build(substrate, wallet)
            if isinstance(built, BuiltCall):
                if (
                    evaluate_policy(
                        "transaction.batch_guard",
                        {
                            "registration_guards": bool(
                                built.extras.get("hashed_registration_guards")
                            )
                        },
                    )
                    == "registration_guards"
                ):
                    raise ValueError(
                        "this guarded account payment must be submitted directly, "
                        "not inside a batch"
                    )
                if (
                    evaluate_policy(
                        "transaction.batch_guard",
                        {"requires_direct": bool(built.extras.get("requires_direct_submission"))},
                    )
                    == "requires_direct"
                ):
                    raise ValueError(
                        "this announced payload contains or may contain an atomic batch; "
                        "submit it directly, not inside another batch"
                    )
                child_call = built.call
                extras.update(
                    {f"{index}:{child.op}.{key}": value for key, value in built.extras.items()}
                )
                # Each pure account needs its own reserve, even when all
                # creations share a sponsor. Do not deduplicate by payer.
                for key in ("protected_pure_registration_deposit_rao", "pure_proxy_deposit_rao"):
                    if key in built.extras:
                        extras[key] = extras.get(key, 0) + built.extras[key]
                for payer, reserve in built.extras.get("pure_proxy_reserves", {}).items():
                    reserves = extras.setdefault("pure_proxy_reserves", {})
                    reserves[payer] = reserves.get(payer, 0) + reserve
                if built.extras.get("multisig_approvals"):
                    extras.setdefault("multisig_approvals", []).extend(
                        built.extras["multisig_approvals"]
                    )
            else:
                child_call = built
            child_calls, registration = await with_recipient_registration(
                substrate, wallet, prepared, child_call, recipients=recipients, as_calls=True
            )
            if (
                evaluate_policy(
                    "transaction.batch_guard",
                    {"new_registration": bool(registration.get("hashed_registration"))},
                )
                == "new_registration"
            ):
                raise ValueError(
                    "first payment to this recipient must be a direct transfer; "
                    "batched first-time account setup is not supported"
                )
            # Flatten guards: the runtime deliberately rejects nested batches.
            # Existing recipients use check-only guards and never reserve funds.
            composed.extend(child_calls)
            if registration:
                guards = registration["hashed_registration_guards"]
                per_account = registration["hashed_registration_max_deposit_rao"] // len(guards)
                for address in guards:
                    maximum_reserves[address] = max(maximum_reserves.get(address, 0), per_account)
        if maximum_reserves:
            extras["hashed_registration_guards"] = list(maximum_reserves)
            extras["hashed_registration_max_deposit_rao"] = sum(maximum_reserves.values())
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
        from ..hashed import prepare_recipient_intent

        effects = [f"all-or-nothing: {len(self._children)} calls in one extrinsic"]
        warnings: list[str] = []
        blocks: list[str] = []
        required_free: Balance | None = None
        available_free: Balance | None = None
        estimated_fee: Balance | None = None
        for index, child in enumerate(self._children):
            prepared, recipients = await prepare_recipient_intent(substrate, child)
            child_preflight = await prepared.preflight(
                substrate, dispatch_origin, fee_payer, call=call
            )
            for recipient in recipients.values():
                child_preflight.effects = [
                    effect.replace(recipient.account, recipient.address)
                    for effect in child_preflight.effects
                ]
            effects.extend(f"[{index}] {item}" for item in child_preflight.effects)
            warnings.extend(f"[{index}] {item}" for item in child_preflight.warnings)
            blocks.extend(f"[{index}] {item}" for item in child_preflight.blocks)
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
        )

    def _aggregate(self):
        children = []
        for child in self._children:
            spend = child.spend()
            children.append(
                {
                    "spend_rao": "unbounded"
                    if spend is UNBOUNDED
                    else None
                    if spend is None
                    else spend.rao,
                    "netuids": child.touches_netuids(),
                    "affects_all_subnets": child.affects_all_subnets(),
                }
            )
        return evaluate_policy("transaction.merge_intents", {"children": children})

    def spend(self) -> Spend:
        value = self._aggregate()["spend_rao"]
        return (
            UNBOUNDED
            if value == "unbounded"
            else None
            if value is None
            else Balance.from_rao(int(value))
        )

    def touches_netuids(self) -> list[int]:
        return self._aggregate()["netuids"]

    def affects_all_subnets(self) -> bool:
        return self._aggregate()["affects_all_subnets"]
