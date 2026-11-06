"""Declarative prompt policies for generated transaction commands.

The generated command runner should not grow an ``if intent.op == ...`` branch
for every richer prompt. Intent-specific choices live in this registry; the
runner applies the same small transformation pipeline to every operation.
"""

from __future__ import annotations

import functools
from dataclasses import dataclass
from typing import Callable, Optional

from ..intents import MoveGamma
from .context import AppContext, address_cli_name, ss58_param_help
from .prompt import PromptSpec, interactive
from .root_helpers import claim_root_source_spec
from .stake_picker import (
    dest_account_spec,
    stake_source_spec,
    stake_target_spec,
    with_free_balance,
)


@dataclass(frozen=True)
class PromptRule:
    """Replace one ordinary prompt with a richer prompt at a stable position."""

    field: str
    build: Callable[[], PromptSpec]
    before: Optional[str] = None


@dataclass(frozen=True)
class PromptDecorator:
    """Wrap the ordinary prompt for ``field`` without changing its position."""

    field: str
    apply: Callable[[PromptSpec], PromptSpec]


@dataclass(frozen=True)
class IntentPromptPolicy:
    rules: tuple[PromptRule, ...] = ()
    decorators: tuple[PromptDecorator, ...] = ()
    notice: Optional[Callable[[dict], Optional[str]]] = None
    required_after_prompt: tuple[str, ...] = ()


def _source(field: str, netuid_field: Optional[str]) -> PromptRule:
    return PromptRule(field, functools.partial(stake_source_spec, field, netuid_field))


def _target(field: str, amount_field: str) -> PromptRule:
    return PromptRule(field, functools.partial(stake_target_spec, field), before=amount_field)


def _typed(field: str, help: Optional[str], before: str) -> PromptRule:
    """A typed address prompt, for a hotkey that is neither a stake position nor a validator."""
    spec = PromptSpec(
        field=field,
        flag=address_cli_name(field),
        help=f"{help} {ss58_param_help(field)}",
        parse=lambda app_ctx, raw: app_ctx.resolve_address(field, raw),
    )
    return PromptRule(field, lambda: spec, before=before)


# A gamma deposit's hotkey is an API key, never the wallet's own hotkey.
_GAMMA_DEPOSIT = IntentPromptPolicy(required_after_prompt=("hotkey_ss58",))


def _remove_stake_notice(kwargs: dict) -> Optional[str]:
    if kwargs.get("netuid") is not None:
        return None
    if str(kwargs.get("amount_alpha") or "").strip().lower() != "all":
        return None
    return (
        "note: `--amount all` unstakes everything on a single subnet (--netuid); "
        "to unstake every position across all subnets, use `btcli stake unstake-all`"
    )


_POLICIES: dict[str, IntentPromptPolicy] = {
    "remove_stake": IntentPromptPolicy(
        rules=(_source("hotkey_ss58", "netuid"),), notice=_remove_stake_notice
    ),
    "remove_stake_limit": IntentPromptPolicy(rules=(_source("hotkey_ss58", "netuid"),)),
    "unstake_all": IntentPromptPolicy(rules=(_source("hotkey_ss58", None),)),
    "unstake_all_alpha": IntentPromptPolicy(rules=(_source("hotkey_ss58", None),)),
    "swap_stake": IntentPromptPolicy(rules=(_source("hotkey_ss58", "origin_netuid"),)),
    "transfer_stake": IntentPromptPolicy(
        rules=(
            _source("hotkey_ss58", "origin_netuid"),
            PromptRule(
                "dest_coldkey_ss58",
                functools.partial(dest_account_spec, "dest_coldkey_ss58"),
            ),
        )
    ),
    "move_stake": IntentPromptPolicy(
        rules=(
            _source("origin_hotkey_ss58", "origin_netuid"),
            _target("dest_hotkey_ss58", "amount_alpha"),
        )
    ),
    "move_swap_stake": IntentPromptPolicy(
        rules=(
            _source("origin_hotkey_ss58", "origin_netuid"),
            _target("dest_hotkey_ss58", "amount_alpha"),
        )
    ),
    "claim_root_with_hotkey": IntentPromptPolicy(
        rules=(PromptRule("hotkey_ss58", claim_root_source_spec),),
        required_after_prompt=("hotkey_ss58",),
    ),
    "add_stake": IntentPromptPolicy(
        rules=(_target("hotkey_ss58", "amount_tao"),),
        decorators=(PromptDecorator("amount_tao", with_free_balance),),
    ),
    "add_stake_limit": IntentPromptPolicy(
        rules=(_target("hotkey_ss58", "amount_tao"),),
        decorators=(PromptDecorator("amount_tao", with_free_balance),),
    ),
    "stake_burn": IntentPromptPolicy(
        decorators=(PromptDecorator("amount_tao", with_free_balance),)
    ),
    "transfer": IntentPromptPolicy(
        rules=(PromptRule("dest_ss58", dest_account_spec),),
        decorators=(PromptDecorator("amount_tao", with_free_balance),),
    ),
    "transfer_all": IntentPromptPolicy(rules=(PromptRule("dest_ss58", dest_account_spec),)),
    "mint_gamma": _GAMMA_DEPOSIT,
    "mint_gamma_with_stake": IntentPromptPolicy(
        rules=(_source("origin_hotkey_ss58", "netuid"),), required_after_prompt=("hotkey_ss58",)
    ),
    "move_gamma": IntentPromptPolicy(
        rules=tuple(
            _typed(field, MoveGamma.field_help(field), before="amount_gamma")
            for field in ("origin_hotkey_ss58", "destination_hotkey_ss58")
        )
    ),
    "close_gamma_deposit": _GAMMA_DEPOSIT,
    "set_gamma_deposit_spender": _GAMMA_DEPOSIT,
    "set_gamma_deposit_daily_spend_cap": _GAMMA_DEPOSIT,
}


def falls_back_to_wallet(op: str, field: str) -> bool:
    """Whether an omitted ``field`` takes the signing wallet's own key: true for the
    ``hotkey_ss58`` / ``coldkey_ss58`` params unless ``op``'s policy requires them."""
    policy = _POLICIES.get(op)
    required = policy.required_after_prompt if policy is not None else ()
    return field in ("hotkey_ss58", "coldkey_ss58") and field not in required


def apply_intent_prompt_policy(
    app_ctx: AppContext,
    op: str,
    missing: list[PromptSpec],
    kwargs: dict,
) -> list[PromptSpec]:
    """Apply ``op``'s declarative prompt policy to the missing prompt list."""
    policy = _POLICIES.get(op)
    if policy is None:
        return missing

    if policy.notice is not None and (notice := policy.notice(kwargs)):
        app_ctx.output.message(notice)

    prompts_ok = (
        not app_ctx.assume_yes and not app_ctx.uses_extension_signer() and interactive(app_ctx)
    )
    if not prompts_ok:
        return missing

    for rule in policy.rules:
        if kwargs.get(rule.field) is not None:
            continue
        missing = [spec for spec in missing if spec.field != rule.field]
        index = (
            next(
                (i for i, spec in enumerate(missing) if spec.field == rule.before),
                len(missing),
            )
            if rule.before is not None
            else 0
        )
        missing.insert(index, rule.build())

    for decorator in policy.decorators:
        missing = [
            decorator.apply(spec) if spec.field == decorator.field else spec for spec in missing
        ]
    return missing
