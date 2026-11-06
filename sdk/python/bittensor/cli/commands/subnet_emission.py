"""`btcli subnets emission`: view and change how a subnet splits its alpha emission.

The vector is ``[burn, recycle, owner, validator, miner]``. ``show`` prints the
vector, what root keeps, the allowed range of every slot and when the owner may
change it next. ``set`` writes owner / validator / miner (and optionally burn /
recycle) in one go; ``set-value``, ``increase`` and ``decrease`` move one slot
and rescale the others so the vector still sums to 100%. Every write is checked
against the chain's rules locally first and a rejected change names the rule and
the allowed range, so nothing is signed that the chain would refuse.

Exit codes: 0 success; 1 the subnet does not exist, the change is blocked right
now (owner rate limit) or the chain rejected it; 2 invalid input or a vector
the chain's rules would reject.
"""

from __future__ import annotations

from typing import Any, Optional

import typer

from ... import emission_weights as ew
from ...intents import SetSubnetEmissionWeights
from ...reads.subnets import SubnetEmissionWeights
from ...settings import guide_docs_url
from ..context import AppContext, ctx_of
from ..globals import with_globals, with_tx_globals

_EXIT_CODES = (
    "Exit codes: 0 done; 1 subnet missing, change blocked right now, or rejected by the "
    "chain; 2 invalid input or a vector the chain's rules would reject."
)

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "Subnet emission weights: how a subnet splits its alpha emission between burn, "
        "recycle, the owner, validators and miners.\n\n"
        "Only the subnet owner coldkey can change the vector, once every 50,400 blocks "
        "(about 7 days) per subnet. Root is paid its baseline share of emission off the top "
        "of the validator slot every block, whatever the subnet sets. Burn + recycle together "
        "may be at most 33%. Add --dry-run to any write to preview the before/after vector "
        "and what root and validators keep; --json for machine-readable output.\n\n"
        f"{_EXIT_CODES}\n\nGuide: {guide_docs_url('subnets')}"
    ),
)

_NETUID_HELP = "Numeric identifier of the subnet."
_SLOT_HELP = "Slot to change: burn, recycle, owner, validator or miner."
_SHARE_HELP = "A percentage (41%), a 0..1 fraction (0.41), or raw u64 parts."
_SHOW_HINT = (
    "`btcli subnets emission show --netuid {netuid}` prints the vector and every allowed range"
)

#: Which slot each chain error is about; None for whole-vector rules.
_ERROR_SLOT: dict[str, Optional[str]] = {
    "BurnRecycleEmissionAboveMaximum": None,
    "BurnEmissionMustBeZero": "burn",
    "RecycleEmissionMustBeZero": "recycle",
    "OwnerEmissionAboveMaximum": "owner",
    "LeasedSubnetOwnerCutBelowMinimum": "owner",
    "ValidatorEmissionAboveMaximum": "validator",
    "ValidatorEmissionTooLowToPreserveRoot": "validator",
    "MinerEmissionBelowMinimum": "miner",
    "EmissionWeightsDoNotSumToOne": None,
}


def _fetch(app_ctx: AppContext, netuid: int) -> SubnetEmissionWeights:
    info = app_ctx.run(lambda c: c.read("subnet_emission_weights", netuid=netuid))
    if info is None:
        app_ctx.output.error(
            f"subnet {netuid} does not exist or has no emission vector",
            help="the root subnet (netuid 0) has no alpha emission to divide",
        )
        raise typer.Exit(1)
    return info


def _record(info: SubnetEmissionWeights) -> dict[str, Any]:
    """The stable machine-readable shape of ``show``."""
    return {
        "netuid": info.netuid,
        "block": info.block,
        "weights": info.weights,
        "shares": info.shares,
        "global_tao_weight": info.global_tao_weight,
        "root_share": info.root_share,
        "baseline_root_share": info.baseline_root_share,
        "validator_share_after_root": info.validator_share_after_root,
        "root_capped": info.root_capped,
        "root_tao": info.root_tao,
        "alpha_issuance": info.alpha_issuance,
        "min_validator": info.min_validator,
        "created_via_lease": info.created_via_lease,
        "limits": info.rules().limits(),
        "rate_limit": info.rate_limit(),
    }


def _hours(blocks: int) -> str:
    hours = blocks * ew.BLOCK_SECONDS / 3600
    return f"{hours / 24:.1f} d" if hours >= 48 else f"{hours:.1f} h"


def _next_change(rate: dict[str, Any]) -> tuple[str, str]:
    """``(value, note)`` for the "next owner change" row."""
    if rate["owner_can_change_now"]:
        why = "first change" if not rate["last_change_block"] else "the weekly window has passed"
        return "now", f"{why}; owners may change once per {rate['limit_blocks']:,} blocks"
    return (
        f"block {rate['next_allowed_block']:,}",
        f"in {rate['blocks_until_allowed']:,} blocks (~{_hours(rate['blocks_until_allowed'])}); "
        f"last changed at block {rate['last_change_block']:,}; root is exempt",
    )


def _ranges(rules: ew.Rules) -> dict[str, tuple[str, str]]:
    """``{slot: (allowed range, why)}`` from the rule table."""
    return {name: (rule, why) for name, rule, why in rules.table()}


def _parse_share(app_ctx: AppContext, raw: str, flag: str) -> int:
    try:
        return ew.parse_share(raw)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `{flag}`: {error}")
        raise typer.Exit(2) from error


def _parse_slot(app_ctx: AppContext, raw: str) -> int:
    try:
        return ew.slot_index(raw)
    except ValueError as error:
        app_ctx.output.error(str(error), help=_SLOT_HELP)
        raise typer.Exit(2) from error


def _reject(app_ctx: AppContext, info: SubnetEmissionWeights, proposed: list[int]) -> None:
    """Exit 2 naming every rule the proposed vector breaks and the allowed range."""
    violations = ew.validate(proposed, info.rules())
    if not violations:
        return
    ranges = _ranges(info.rules())
    allowed = []
    for violation in violations:
        slot = _ERROR_SLOT.get(violation.error)
        if slot and slot in ranges and slot not in {a[0] for a in allowed}:
            allowed.append((slot, *ranges[slot]))
        elif (
            violation.error == "BurnRecycleEmissionAboveMaximum"
            and "burn+recycle" in ranges
            and "burn+recycle" not in {a[0] for a in allowed}
        ):
            allowed.append(("burn+recycle", *ranges["burn+recycle"]))
    if len(violations) == 1:
        text = f"the chain would reject this change: {violations[0]}"
    else:
        text = "the chain would reject this change:\n" + "\n".join(
            f"  - {violation}" for violation in violations
        )
    note = None
    if allowed:
        note = f"allowed on subnet {info.netuid} right now: " + "; ".join(
            f"{slot} {rule} ({why})" for slot, rule, why in allowed
        )
    app_ctx.output.error(text, note=note, help=_SHOW_HINT.format(netuid=info.netuid))
    raise typer.Exit(2)


def _card(info: SubnetEmissionWeights, preview: dict[str, Any]) -> list[tuple]:
    """Review-card rows: each moving slot, what root keeps, what validators are left."""
    before, after = preview["before"]["weights"], preview["after"]["weights"]
    rows: list[tuple] = []
    for slot in ew.ADJUSTABLE_SLOTS:
        if after[slot] == before[slot] and after[slot] == 0:
            continue
        value = ew.format_share(after[slot])
        if after[slot] != before[slot]:
            value = f"{ew.format_share(before[slot])} -> {value}"
        rows.append((ew.SLOT_NAMES[slot], value))
    root = preview["root"]
    share_after = root["share_after"]
    rows.append(
        (
            "root share of emission",
            f"{root['share_before'] * 100:.4f}% -> "
            + (f"{share_after * 100:.4f}%" if share_after is not None else "rejected")
            + f" (baseline {root['baseline_share'] * 100:.4f}%, paid off the top)",
        )
    )
    validators_after = root["validators_after"]
    rows.append(
        (
            "validators after root",
            f"{root['validators_before'] * 100:.4f}% -> "
            + (
                f"{validators_after * 100:.4f}%"
                + (" (capped: root takes the whole slot)" if root["capped_after"] else "")
                if validators_after is not None
                else "rejected"
            ),
        )
    )
    rate = info.rate_limit()
    rows.append(
        (
            "next owner change after this",
            f"about block {info.block + rate['limit_blocks']:,} "
            f"({rate['limit_blocks']:,} blocks after the one that includes this call)",
        )
    )
    return rows


def _submit(
    app_ctx: AppContext,
    info: SubnetEmissionWeights,
    proposed: list[int],
    *,
    slots: list[int],
    renormalize: bool,
) -> None:
    _reject(app_ctx, info, proposed)
    intent = SetSubnetEmissionWeights(
        netuid=info.netuid,
        slots=list(slots),
        values=[proposed[slot] for slot in slots],
        renormalize=renormalize,
    )
    preview = ew.preview(
        info.weights,
        proposed,
        baseline_parts=info.baseline_root_parts,
        global_tao_weight=info.global_tao_weight,
        root_tao=info.root_tao,
        alpha_issuance=info.alpha_issuance,
    )
    app_ctx.submit(intent, card_sections=[("Emission weights", _card(info, preview))])


def _renormalized_submit(
    app_ctx: AppContext, info: SubnetEmissionWeights, index: int, parts: int
) -> None:
    try:
        proposed = ew.renormalize(info.weights, {index: parts})
    except ValueError as error:
        app_ctx.output.error(str(error), help=_SHOW_HINT.format(netuid=info.netuid))
        raise typer.Exit(2) from error
    _submit(app_ctx, info, proposed, slots=[index], renormalize=True)


@app.command(
    "show",
    epilog=(
        "Example: btcli subnets emission show --netuid 4\n\n"
        "JSON fields: netuid, block, weights (u64 parts), shares (0..1), global_tao_weight, "
        "root_share, baseline_root_share, validator_share_after_root, root_capped, root_tao, "
        "alpha_issuance (rao), min_validator (parts), "
        "created_via_lease, limits{slot: min, max, min_parts, max_parts, why}, "
        "rate_limit{limit_blocks, last_change_block, next_allowed_block, "
        "blocks_until_allowed, owner_can_change_now}."
    ),
)
@with_globals
def show(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
):
    """Show the emission split, what root keeps, the allowed ranges and the next change window."""
    app_ctx: AppContext = ctx_of(ctx)
    info = _fetch(app_ctx, netuid)
    vector_rows: list[tuple[str, str, Optional[str]]] = [
        (ew.SLOT_NAMES[slot], ew.format_share(info.weights[slot]), f"{info.weights[slot]} parts")
        for slot in ew.ADJUSTABLE_SLOTS
    ]
    next_value, next_note = _next_change(info.rate_limit())
    root_rows: list[tuple[str, str, Optional[str]]] = [
        (
            "root share of emission",
            f"{info.root_share * 100:.4f}%",
            (
                "the baseline, capped at the validator slot: root takes the whole slot"
                if info.root_capped
                else "what root TAO stakers earn from this subnet now, paid off the top of "
                "the validator slot"
            ),
        ),
        (
            "root baseline share",
            f"{info.baseline_root_share * 100:.4f}%",
            f"with the default validator share and the global TaoWeight "
            f"({info.global_tao_weight * 100:.2f}%); recomputed every block, so it moves with "
            "root stake and alpha issuance, not with this vector",
        ),
        (
            "validators after root",
            f"{info.validator_share_after_root * 100:.4f}%",
            (
                "nothing: raise the validator share above the baseline to pay validators"
                if info.root_capped
                else "what alpha validators share once root is paid"
            ),
        ),
        (
            "validator floor",
            f"> {info.baseline_root_share * 100:.4f}%",
            "a change must leave the validator share above the baseline",
        ),
        ("root tao", f"{info.root_tao / 1e9:,.0f} TAO", None),
        ("alpha issuance", f"{info.alpha_issuance / 1e9:,.0f} alpha", None),
        ("next owner change", next_value, next_note),
        ("created via lease", "yes" if info.created_via_lease else "no", None),
    ]
    ranges = _ranges(info.rules())
    order = [*list(ew.SLOT_NAMES), "burn+recycle", "sum"]
    rule_rows: list[tuple[str, str, Optional[str]]] = [(name, *ranges[name]) for name in order]
    app_ctx.output.kv_sections(
        f"subnet {netuid} emission weights",
        [("Vector", vector_rows), ("Root", root_rows), ("Allowed", rule_rows)],
        _record(info),
        hint=(
            f"change one slot with `btcli subnets emission set-value --netuid {netuid} "
            "--slot owner --value 10%` (the other slots rescale to fill 100%); "
            "add --dry-run to preview"
        ),
    )


@app.command(
    "set",
    epilog=(
        "Example: btcli subnets emission set --netuid 4 --owner 18% --validator 20% --miner 62% "
        "--dry-run\n"
        "Example with burn: btcli subnets emission set --netuid 4 --burn 5% --owner 10% "
        "--validator 40% --miner 45%\n\n"
        f"{_EXIT_CODES}"
    ),
)
@with_tx_globals
def set_weights(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    owner: str = typer.Option(..., "--owner", help=f"Owner share. {_SHARE_HELP}"),
    validator: str = typer.Option(..., "--validator", help=f"Validator share. {_SHARE_HELP}"),
    miner: str = typer.Option(..., "--miner", help=f"Miner share. {_SHARE_HELP}"),
    burn: str = typer.Option("0", "--burn", help=f"Burn share (default 0). {_SHARE_HELP}"),
    recycle: str = typer.Option("0", "--recycle", help=f"Recycle share (default 0). {_SHARE_HELP}"),
):
    """Set the full emission split in one go.

    Burn + recycle together may be at most 33%. Root stakers always keep their
    baseline share: it is paid off the top of the validator slot every block, so
    the validator share must stay above it (a share at or below the baseline is
    rejected) and alpha validators share what is left.
    """
    app_ctx: AppContext = ctx_of(ctx)
    info = _fetch(app_ctx, netuid)
    burn_parts = _parse_share(app_ctx, burn, "--burn")
    recycle_parts = _parse_share(app_ctx, recycle, "--recycle")
    owner_parts = _parse_share(app_ctx, owner, "--owner")
    validator_parts = _parse_share(app_ctx, validator, "--validator")
    miner_parts = _parse_share(app_ctx, miner, "--miner")
    total = burn_parts + recycle_parts + owner_parts + validator_parts + miner_parts
    if 0 < abs(ew.ONE - total) <= 1_000:
        # Decimal shares rarely hit u64::MAX exactly; let the miner slot soak up the dust.
        miner_parts += ew.ONE - total
    elif total != ew.ONE:
        app_ctx.output.error(
            f"burn + recycle + owner + validator + miner must add up to 100%, not "
            f"{ew.format_share(total, 4)}",
            help="give all shares, or use set-value to move one slot and rescale the rest",
        )
        raise typer.Exit(2)
    proposed = [burn_parts, recycle_parts, owner_parts, validator_parts, miner_parts]
    _submit(app_ctx, info, proposed, slots=list(range(len(ew.SLOT_NAMES))), renormalize=False)


@app.command(
    "set-value",
    epilog=(
        "Example: btcli subnets emission set-value --netuid 4 --slot validator --value 20% "
        "--dry-run\n\n"
        f"{_EXIT_CODES}"
    ),
)
@with_tx_globals
def set_value(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    slot: str = typer.Option(..., "--slot", help=_SLOT_HELP),
    value: str = typer.Option(..., "--value", help=f"New share for the slot. {_SHARE_HELP}"),
):
    """Set one slot; the other slots are rescaled so the vector still sums to 100%.

    The untouched slots keep their ratio: lowering owner from 18% to 10%
    hands the 8% to the other non-zero slots in their current proportions.
    """
    app_ctx: AppContext = ctx_of(ctx)
    info = _fetch(app_ctx, netuid)
    index = _parse_slot(app_ctx, slot)
    parts = _parse_share(app_ctx, value, "--value")
    _renormalized_submit(app_ctx, info, index, parts)


@app.command(
    "increase",
    epilog=(
        f"Example: btcli subnets emission increase --netuid 4 --slot miner --by 5%\n\n{_EXIT_CODES}"
    ),
)
@with_tx_globals
def increase(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    slot: str = typer.Option(..., "--slot", help=_SLOT_HELP),
    by: str = typer.Option(..., "--by", help=f"Amount to add to the slot. {_SHARE_HELP}"),
):
    """Raise one slot by an amount; the other slots are rescaled to fill 100%."""
    app_ctx: AppContext = ctx_of(ctx)
    info = _fetch(app_ctx, netuid)
    index = _parse_slot(app_ctx, slot)
    delta = _parse_share(app_ctx, by, "--by")
    _renormalized_submit(app_ctx, info, index, min(ew.ONE, info.weights[index] + delta))


@app.command(
    "decrease",
    epilog=(
        "Example: btcli subnets emission decrease --netuid 4 --slot validator --by 10%\n\n"
        f"{_EXIT_CODES}"
    ),
)
@with_tx_globals
def decrease(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    slot: str = typer.Option(..., "--slot", help=_SLOT_HELP),
    by: str = typer.Option(..., "--by", help=f"Amount to take from the slot. {_SHARE_HELP}"),
):
    """Lower one slot by an amount; the other slots are rescaled to fill 100%."""
    app_ctx: AppContext = ctx_of(ctx)
    info = _fetch(app_ctx, netuid)
    index = _parse_slot(app_ctx, slot)
    delta = _parse_share(app_ctx, by, "--by")
    if delta > info.weights[index]:
        app_ctx.output.error(
            f"{ew.SLOT_NAMES[index]} is only {ew.format_share(info.weights[index])}; cannot "
            f"take {ew.format_share(delta)} from it",
            help=_SHOW_HINT.format(netuid=info.netuid),
        )
        raise typer.Exit(2)
    _renormalized_submit(app_ctx, info, index, info.weights[index] - delta)
