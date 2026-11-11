"""Subnet emission weights: how a subnet splits its alpha emission.

Each subnet carries a five-slot vector ``[burn, recycle, owner, validator,
miner]`` of ``u64 / u64::MAX`` shares. The subnet owner (or root) changes it with
``AdminUtils.sudo_set_subnet_emission_weights_v2``, which replaces the whole
split (the five slots and the gamma shares).
The chain enforces the rules spelled out in :mod:`bittensor.emission_weights`
and rate-limits owners to one change per week per subnet (root is exempt; the
first change on a subnet is always allowed).

Read the current vector and its bounds with the ``subnet_emission_weights``
read.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Optional

from .. import emission_weights as ew
from .._generated import calls
from .._generated import runtime_apis as api
from .._generated import storage as st
from ..reads.subnets import (
    emission_weights_rate_limit_key,
    gamma_emission_shares,
    gamma_emission_targets,
    root_preservation_inputs,
)
from .base import Intent, IntentPreflight
from .registry import register

SLOTS_HELP = (
    "Slots to set, by name or uid: 0 burn, 1 recycle, 2 owner, 3 validator, 4 miner. "
    "Omitted slots keep their current share. Empty, with empty `values`, to change only gamma "
    "shares."
)
VALUES_HELP = (
    "New share for each slot, pairwise with `slots`: a 0..1 fraction (0.41), a percentage "
    '("41%"), or raw u64 parts of 2^64-1. Rules: burn + recycle <= 33%; owner <= 33% '
    "(>= 18% on lease-created subnets); validator <= 50% and above root's baseline "
    "share of emission, which is paid off the top of the validator slot (see `btcli "
    "subnets emission show`); miner >= 33%; all five and the subnet's gamma shares sum to "
    "exactly 1."
)
RENORMALIZE_HELP = (
    "Keep the named slots as given and rescale the other slots proportionally so the "
    "vector and the subnet's gamma shares sum to 1 (the last free slot absorbs rounding). "
    "Without it, the update is applied verbatim and must already sum to 1 with them (the "
    "miner slot absorbs up to 1,000 parts of rounding)."
)
GAMMA_HELP = (
    'Gamma shares to set, as a JSON object mapping target netuid to share ({"3": "5%"}), in '
    "the same forms as `values`. Omitted targets keep their current share; a share of 0 "
    "removes one. Rules: at most 16, each <= 10%, to another subnet root whitelisted for "
    "gamma emission with gamma enabled; none on a leased subnet. With `renormalize` the five "
    "slots make room."
)


def _vector_text(weights: list[int]) -> str:
    """``burn 5.00% / owner 18.00% / ...`` (zero burn/recycle omitted for brevity)."""
    parts = [
        f"{ew.SLOT_NAMES[slot]} {ew.format_share(weights[slot])}"
        for slot in ew.ADJUSTABLE_SLOTS
        if slot not in (ew.BURN, ew.RECYCLE) or weights[slot]
    ]
    return " / ".join(parts)


def _gamma_text(shares: dict[int, int]) -> str:
    """``gamma:3 5.00% / gamma:7 2.00%``, or ``none``."""
    text = " / ".join(f"gamma:{t} {ew.format_share(v)}" for t, v in sorted(shares.items()))
    return text or "none"


def _pct(value: float, places: int = 2) -> str:
    return f"{value * 100:.{places}f}%"


def _root_line(root: dict[str, Any]) -> str:
    """One sentence on what root and alpha validators earn here after the set."""
    baseline = _pct(root["baseline_share"], 4)
    if root["share_after"] is None:
        return (
            f"root's baseline share here is {baseline} of emission, paid off the top of the "
            "validator slot; this validator share is at or below it, so validators would get "
            "nothing (the chain rejects)"
        )
    line = (
        f"root keeps its baseline {baseline} of this subnet's emission (default validator "
        f"share, global TaoWeight) before and after; alpha validators get "
        f"{_pct(root['validators_before'], 4)} -> {_pct(root['validators_after'], 4)}"
    )
    if root["capped_after"]:
        line += (
            "; the baseline exceeds the validator slot right now, so root takes the whole slot "
            "and validators get nothing until the share is raised"
        )
    return line


@register
@dataclass
class SetSubnetEmissionWeights(Intent):
    """Set a subnet's emission split: its five slots and gamma shares (btcli ``subnets emission``).

    Dispatches ``AdminUtils.sudo_set_subnet_emission_weights_v2`` with the
    whole split, keeping the gamma shares it does not name. The signer must
    be the subnet's owner coldkey (root may also call it). With
    ``renormalize`` the untouched slots are rescaled so the vector and the
    subnet's gamma shares still sum to one; without it the merged vector is
    submitted as given.
    Root stakers are paid their baseline share of emission (what they earn
    with the default validator share and the global TaoWeight) off the top of
    the validator slot every block, whatever the subnet sets — the pre-sign
    plan shows that share and what alpha validators are left with, and lists
    every rule the chain would reject (including a validator share at or below
    the baseline), so a bad vector is caught before signing. Owners may change
    the vector once per week per subnet.
    """

    op = "set_subnet_emission_weights"
    signer = "coldkey"
    origin = "subnet_owner"
    verify = "subnet_emission_weights"
    wraps = (("AdminUtils", "sudo_set_subnet_emission_weights_v2"),)

    netuid: int = field(metadata={"help": "Subnet to configure; the signer must be its owner."})
    slots: list[int | str] = field(metadata={"help": SLOTS_HELP})
    values: list[int | float | str] = field(metadata={"help": VALUES_HELP})
    renormalize: bool = field(default=False, metadata={"help": RENORMALIZE_HELP})
    gamma: dict = field(default_factory=dict, metadata={"help": GAMMA_HELP})

    def __post_init__(self):
        if len(self.slots) != len(self.values):
            raise ValueError(
                f"slots and values must pair up: got {len(self.slots)} slots and "
                f"{len(self.values)} values"
            )
        if not self.slots and not self.gamma:
            raise ValueError("name at least one slot or gamma share to set")
        self.slots = [ew.slot_index(slot) for slot in self.slots]
        if len(set(self.slots)) != len(self.slots):
            raise ValueError("each slot may appear at most once")
        self.values = [ew.parse_share(value) for value in self.values]
        gamma: dict[int, int] = {}
        for target, share in self.gamma.items():
            if not str(target).isdecimal() or int(target) > 0xFFFF:
                raise ValueError(f"gamma target {target!r} is not a netuid")
            if int(target) in gamma:
                raise ValueError("each gamma target may appear at most once")
            gamma[int(target)] = ew.parse_share(share)
        self.gamma = gamma

    def updates(self) -> dict[int, int]:
        return {int(slot): int(value) for slot, value in zip(self.slots, self.values)}

    async def _current(self, substrate) -> Optional[dict]:
        if self.netuid == 0:
            return None  # the runtime API answers for the root subnet, which has no vector
        return await substrate.runtime_call(
            *api.SubnetInfoRuntimeApi.get_subnet_emission_weights, [self.netuid]
        )

    async def _gamma_shares(self, substrate) -> tuple[dict[int, int], dict[int, int]]:
        """The subnet's gamma shares by target netuid, now and after this change."""
        shares = await gamma_emission_shares(
            lambda item, keys: substrate.query(*item, keys), self.netuid
        )
        return shares, ew.merge_gamma(shares, self.gamma)

    async def _gamma_rules(self, substrate) -> tuple[dict[int, bool], bool]:
        """The subnets a gamma share can name, each with whether gamma is enabled
        there, and whether this subnet is leased."""
        targets, lease = await asyncio.gather(
            gamma_emission_targets(lambda item, keys: substrate.query_map(*item, keys)),
            substrate.query(*st.SubtensorModule.SubnetUidToLeaseId, [self.netuid]),
        )
        return targets, lease is not None

    def _merged(self, current: Optional[dict], gamma_share: int) -> list[int]:
        base = ew.normalize_weights(current["weights"]) if current else ew.default_weights()
        if self.renormalize:
            return ew.renormalize(base, self.updates(), gamma_share)
        return ew.absorb_rounding(ew.merge(base, self.updates()), gamma_share)

    async def build(self, substrate, wallet: Any):
        current, (_, gamma) = await asyncio.gather(
            self._current(substrate), self._gamma_shares(substrate)
        )
        weights = self._merged(current, sum(gamma.values()))
        split: list[tuple[Any, int]] = [
            (name.capitalize(), w) for name, w in zip(ew.SLOT_NAMES, weights)
        ]
        split += [({"Gamma": {"netuid": t}}, share) for t, share in gamma.items()]
        return await substrate.compose(
            calls.AdminUtils.sudo_set_subnet_emission_weights_v2(self.netuid, split)
        )

    async def _inputs(self, substrate) -> Optional[dict]:
        """Everything the preview needs, read once: the current vector and
        root's baseline share, root TAO, alpha issuance, the global TaoWeight,
        the owner's last change block, the subnet's gamma shares now and after
        this change, the subnets they can name and whether the subnet is leased."""
        current = await self._current(substrate)
        if current is None:
            return None
        query = lambda item, keys: substrate.query(*item, keys)  # noqa: E731
        (
            (root_tao, alpha_issuance, global_tao_weight),
            last_change,
            block,
            owner,
            (gamma_before, gamma_after),
            (gamma_targets, leased),
        ) = await asyncio.gather(
            root_preservation_inputs(query, self.netuid),
            substrate.query(
                *st.SubtensorModule.LastRateLimitedBlock,
                [emission_weights_rate_limit_key(self.netuid)],
            ),
            substrate.block_number(),
            substrate.query(*st.SubtensorModule.SubnetOwner, [self.netuid]),
            self._gamma_shares(substrate),
            self._gamma_rules(substrate),
        )
        return {
            "current": current,
            "before": ew.normalize_weights(current["weights"]),
            "baseline_parts": int(current["root_baseline_share"]),
            "global_tao_weight": global_tao_weight,
            "root_tao": root_tao,
            "alpha_issuance": alpha_issuance,
            "last_change_block": int(last_change or 0),
            "block": int(block),
            "owner": None if owner is None else str(owner),
            "gamma_share": sum(gamma_after.values()),
            "gamma": {"before": gamma_before, "after": gamma_after},
            "gamma_targets": gamma_targets,
            "leased": leased,
        }

    def _preview(self, inputs: dict, after: list[int]) -> dict[str, Any]:
        return ew.preview(
            inputs["before"],
            after,
            baseline_parts=inputs["baseline_parts"],
            global_tao_weight=inputs["global_tao_weight"],
            root_tao=inputs["root_tao"],
            alpha_issuance=inputs["alpha_issuance"],
        )

    def _rules(self, inputs: dict) -> ew.Rules:
        return ew.Rules(
            min_validator=ew.min_validator_parts(inputs["baseline_parts"]),
            min_owner=ew.DEFAULT_OWNER,
            created_via_lease=bool(inputs["current"]["created_via_lease"]),
            gamma_share=inputs["gamma_share"],
            gamma_targets=inputs["gamma_targets"],
            leased=inputs["leased"],
        )

    async def preflight(self, substrate, dispatch_origin: str, fee_payer: str, *, call=None):
        del fee_payer, call
        inputs = await self._inputs(substrate)
        if inputs is None:
            return IntentPreflight(
                effects=[self.summary()],
                warnings=[],
                blocks=[f"subnet {self.netuid} does not exist or has no emission vector"],
            )
        try:
            after = self._merged(inputs["current"], inputs["gamma_share"])
        except ValueError as error:
            return IntentPreflight(effects=[self.summary()], warnings=[], blocks=[str(error)])
        preview = self._preview(inputs, after)
        rules = self._rules(inputs)
        blocks = [str(violation) for violation in ew.validate(after, rules)]
        blocks += [str(v) for v in ew.validate_gamma(self.netuid, inputs["gamma"]["after"], rules)]
        owner = inputs["owner"]
        if owner is not None and dispatch_origin != owner:
            blocks.append(
                f"{dispatch_origin} is not the owner of subnet {self.netuid} (the owner is "
                f"{owner}); only the owner coldkey, or root through sudo, can change its "
                "emission weights"
            )
        rate = ew.rate_limit_status(inputs["last_change_block"], inputs["block"])
        if not rate["owner_can_change_now"]:
            hours = int(rate["blocks_until_allowed"]) * ew.BLOCK_SECONDS / 3600
            blocks.append(
                f"the owner changed this vector at block {rate['last_change_block']:,}; the "
                f"chain allows the next owner change at block {rate['next_allowed_block']:,} "
                f"(in {rate['blocks_until_allowed']:,} blocks, about {hours:.1f} h); root is "
                "exempt"
            )
        preview["rate_limit"] = rate
        preview["limits"] = rules.limits()
        preview["gamma"] = inputs["gamma"]
        return IntentPreflight(
            effects=self._effect_lines(preview),
            warnings=[],
            blocks=blocks,
            preview=preview,
        )

    def _effect_lines(self, preview: dict[str, Any]) -> list[str]:
        before, after = preview["before"]["weights"], preview["after"]["weights"]
        lines = [self.summary()]
        lines.append(
            "vector: " + _vector_text(before) + " -> " + _vector_text(after)
            if preview["changed"]
            else "vector: " + _vector_text(before) + " (unchanged)"
        )
        gamma = preview["gamma"]
        if gamma["before"] != gamma["after"]:
            lines.append(
                f"gamma shares: {_gamma_text(gamma['before'])} -> {_gamma_text(gamma['after'])}"
            )
            if set(gamma["before"]) - set(gamma["after"]):
                lines.append("removed gamma shares: their unconverted alpha goes to the miners")
        lines.append(_root_line(preview["root"]))
        return lines

    async def effects(self, substrate, signer_address: str) -> list[str]:
        return list((await self.preflight(substrate, signer_address, signer_address)).effects)

    async def blocks(self, substrate, signer_address: str) -> list[str]:
        return list((await self.preflight(substrate, signer_address, signer_address)).blocks)

    def summary(self) -> str:
        parts = [
            f"{ew.SLOT_NAMES[slot]}={ew.format_share(value)}"
            for slot, value in self.updates().items()
        ]
        parts += [f"gamma:{t}={ew.format_share(v)}" for t, v in sorted(self.gamma.items())]
        rest = self.renormalize and len(self.slots) < len(ew.SLOT_NAMES)
        mode = " (renormalizing the rest)" if rest else ""
        return f"set emission weights on netuid {self.netuid}: {', '.join(parts)}{mode}"

    def touches_netuids(self) -> list[int]:
        return [self.netuid, *sorted(set(self.gamma) - {self.netuid})]
