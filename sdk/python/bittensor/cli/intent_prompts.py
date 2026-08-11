"""Declarative prompt policies for generated transaction commands.

The generated command runner should not grow an ``if intent.op == ...`` branch
for every richer prompt. Intent-specific choices live in this registry; the
runner applies the same small transformation pipeline to every operation.
"""

from __future__ import annotations

import functools
from dataclasses import dataclass
from typing import Callable, Optional

import typer

from .context import AppContext
from .prompt import PromptSpec, interactive
from .root_helpers import claim_root_source_spec
from .stake_picker import stake_source_spec, stake_target_spec, with_free_balance


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
    "transfer_stake": IntentPromptPolicy(rules=(_source("hotkey_ss58", "origin_netuid"),)),
    "move_stake": IntentPromptPolicy(rules=(_source("origin_hotkey_ss58", "origin_netuid"),)),
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
}


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


def validate_intent_prompt_policy(app_ctx: AppContext, op: str, kwargs: dict) -> None:
    """Enforce values that cannot silently fall back after policy prompting."""
    policy = _POLICIES.get(op)
    if policy is None:
        return
    missing = [field for field in policy.required_after_prompt if kwargs.get(field) is None]
    if not missing:
        return
    flags = ", ".join(f"`--{field.removesuffix('_ss58').replace('_', '-')}`" for field in missing)
    app_ctx.output.error(
        f"missing required option: {flags}",
        help="pass it explicitly, or run on a terminal to pick one",
    )
    raise typer.Exit(2)
