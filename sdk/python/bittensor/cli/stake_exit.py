"""High-level stake exit commands built from the single-hotkey intents.

The transaction catalog stays one-call-per-intent. The human-facing ``stake``
group adds one target selector: ``--all-hotkeys`` discovers live positions and
adapts the existing intent into an atomic batch.
"""

from __future__ import annotations

from typing import Optional

import typer

from .. import config as cfg
from ..intents import Batch, UnstakeAll, UnstakeAllAlpha
from ..intents.base import Intent
from ..intents.proxy import ProxyTypeChoice
from ..settings import tx_docs_url
from .context import AppContext, address_cli_name, ctx_of, ss58_param_help
from .globals import PANEL_EXECUTION, with_tx_globals
from .prompt import fill_missing, interactive
from .stake_picker import stake_source_spec


class _StakeExitBatch(Batch):
    """Atomic bulk exit with the same submission policy as its child intents."""

    mev_shield_default = True


def _position_kind(include_root: bool) -> str:
    return "live non-zero position" if include_root else "live non-zero alpha position outside root"


def _command_help(intent_cls: type[Intent], include_root: bool) -> str:
    return (
        f"{intent_cls.describe()}\n\n"
        "Use `--hotkey` for one hotkey, or `--all-hotkeys` to sweep every "
        f"hotkey with a {_position_kind(include_root)} for the coldkey. Multiple "
        "hotkeys execute atomically in one transaction. The multi-hotkey form "
        "cannot use a Staking proxy because that proxy type cannot wrap "
        "Utility.batch_all on the current runtime; sign directly or use a broader "
        "proxy type such as NonTransfer.\n\n"
        f"Docs: {tx_docs_url(intent_cls.op)}"
    )


def _live_hotkeys(app_ctx: AppContext, owner: str, include_root: bool) -> list[str]:
    positions = app_ctx.run(lambda client: client.read("stake_for_coldkey", coldkey_ss58=owner))
    return list(
        dict.fromkeys(
            position.hotkey
            for position in positions
            if position.stake.rao > 0 and (include_root or position.netuid != 0)
        )
    )


def _stake_owner(app_ctx: AppContext, proxy_for: Optional[str]) -> str:
    """Resolve the coldkey whose live positions ``--all-hotkeys`` should sweep."""
    effective_proxy = proxy_for
    if effective_proxy is None:
        configured = cfg.get("proxy_for")
        effective_proxy = str(configured) if configured else None
    if effective_proxy not in (None, "self"):
        owner = app_ctx.resolve_address("proxy_for", effective_proxy)
        if owner is not None:
            return owner

    if app_ctx.uses_external_signer():
        signer_ref = app_ctx.signer_address or cfg.get("signer_address")
        if signer_ref:
            owner = app_ctx.resolve_address("coldkey_ss58", str(signer_ref))
        elif app_ctx.uses_ledger_signer():
            owner = app_ctx.ledger_signer().ss58_address
        elif app_ctx.uses_vault_signer():
            owner = app_ctx.vault_signer().ss58_address
        else:
            app_ctx.output.error(
                "cannot inspect stake before selecting the extension account",
                help="pass --signer-address or --proxy-for",
            )
            raise typer.Exit(2)
        if owner is not None:
            return owner

    owner = app_ctx.resolve_address("coldkey_ss58", app_ctx.wallet_name)
    if owner is None:
        app_ctx.output.error(f"wallet {app_ctx.wallet_name!r} has no usable coldkey")
        raise typer.Exit(1)
    return owner


def _proxy_options(
    app_ctx: AppContext,
    proxy_for: Optional[str],
    force_proxy_type: Optional[ProxyTypeChoice],
) -> tuple[Optional[str], Optional[str]]:
    """Normalize proxy CLI values before handing them to ``AppContext.submit``."""
    resolved_proxy = (
        proxy_for
        if proxy_for in (None, "self")
        else app_ctx.resolve_address("proxy_for", proxy_for)
    )
    resolved_type = str(force_proxy_type.value) if force_proxy_type is not None else None
    return resolved_proxy, resolved_type


def _select_hotkeys(
    app_ctx: AppContext,
    hotkey_ss58: Optional[str],
    all_hotkeys: bool,
    proxy_for: Optional[str],
    include_root: bool,
) -> list[str]:
    if all_hotkeys:
        owner = _stake_owner(app_ctx, proxy_for)
        hotkeys = _live_hotkeys(app_ctx, owner, include_root)
        if not hotkeys:
            kind = "stake" if include_root else "alpha stake outside root"
            app_ctx.output.error(
                f"coldkey {owner} has no {kind}",
                help="`btcli stake list` shows every position",
            )
            raise typer.Exit(1)
        return hotkeys

    if (
        hotkey_ss58 is None
        and not app_ctx.assume_yes
        and not app_ctx.uses_external_signer()
        and interactive(app_ctx)
    ):
        values = {"hotkey_ss58": None, "proxy_for": proxy_for}
        fill_missing(app_ctx, [stake_source_spec("hotkey_ss58", None)], values)
        hotkey_ss58 = values["hotkey_ss58"]
    resolved = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    if resolved is None:
        app_ctx.output.error(
            "missing hotkey",
            help="pass `--hotkey <HOTKEY>` or `--all-hotkeys`",
        )
        raise typer.Exit(2)
    return [resolved]


def _submit(
    app_ctx: AppContext,
    intent_cls: type[Intent],
    include_root: bool,
    hotkey_ss58: Optional[str],
    all_hotkeys: bool,
    proxy_for: Optional[str],
    force_proxy_type: Optional[ProxyTypeChoice],
) -> None:
    if all_hotkeys and hotkey_ss58 is not None:
        app_ctx.output.error("choose only one of `--hotkey` or `--all-hotkeys`")
        raise typer.Exit(2)

    resolved_proxy, resolved_proxy_type = _proxy_options(app_ctx, proxy_for, force_proxy_type)
    hotkeys = _select_hotkeys(app_ctx, hotkey_ss58, all_hotkeys, proxy_for, include_root)
    intents = [intent_cls.from_args({"hotkey_ss58": hotkey}) for hotkey in hotkeys]
    intent = intents[0] if len(intents) == 1 else _StakeExitBatch(intents=intents)
    app_ctx.submit(
        intent,
        proxy_for=resolved_proxy,
        force_proxy_type=resolved_proxy_type,
    )


def _command(intent_cls: type[Intent], include_root: bool):
    @with_tx_globals
    def command(
        ctx: typer.Context,
        hotkey_ss58: Optional[str] = typer.Option(
            None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
        ),
        all_hotkeys: bool = typer.Option(
            False,
            "--all-hotkeys",
            help=f"Sweep every hotkey with a {_position_kind(include_root)} for this coldkey.",
        ),
        proxy_for: Optional[str] = typer.Option(
            None,
            "--proxy-for",
            help=(
                "Dispatch as this account via Proxy.proxy; pass `self` to bypass a "
                "configured default."
            ),
            rich_help_panel=PANEL_EXECUTION,
        ),
        force_proxy_type: Optional[ProxyTypeChoice] = typer.Option(
            None,
            "--force-proxy-type",
            help="Require this exact proxy type to be used (with --proxy-for).",
            rich_help_panel=PANEL_EXECUTION,
        ),
    ):
        _submit(
            ctx_of(ctx),
            intent_cls,
            include_root,
            hotkey_ss58,
            all_hotkeys,
            proxy_for,
            force_proxy_type,
        )

    return command


def mount_stake_exit_commands(app: typer.Typer, help_panel: str) -> None:
    """Mount high-level single/all-hotkey variants on the ``stake`` group."""
    for name, intent_cls, include_root in (
        ("unstake-all", UnstakeAll, True),
        ("unstake-all-alpha", UnstakeAllAlpha, False),
    ):
        app.command(
            name,
            rich_help_panel=help_panel,
            help=_command_help(intent_cls, include_root),
        )(_command(intent_cls, include_root))
