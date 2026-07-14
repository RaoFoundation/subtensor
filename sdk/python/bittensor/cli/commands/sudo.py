"""`btcli sudo`: subnet-owner hyperparameters and governance."""

from __future__ import annotations

from typing import Any, Optional

import typer

from ...hyperparams import to_raw
from ...intents import (
    SetHyperparameter,
    SetMechanismCount,
    SetSubnetIdentity,
    SetTake,
    StakeBurn,
    StartCall,
    TrimSubnet,
    UpdateSymbol,
)
from ...intents.children import take_to_u16
from ...intents.hyperparameters import OWNER_HYPERPARAMETERS
from ...settings import U16_MAX
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..hyperparams_view import fetch_hyperparameters, show_hyperparameters
from ..prompt import PromptSpec, fill_missing, interactive
from ..tx import _parse_money

app = typer.Typer(no_args_is_help=True, help="Subnet-owner config and governance.")

PANEL_SUBNETS = "Subnets"
PANEL_VALIDATORS = "Validators"
PANEL_SENATE = "Senate"

_NETUID_HELP = "Numeric identifier of the target subnet."


_VALUE_HELP = (
    "Raw on-chain integer, or the human form with a decimal point "
    "(0..1 fraction for normalized params, TAO for burn params)."
)


def _parse_hyperparameter_name(_app_ctx: AppContext, raw: str) -> str:
    if raw not in OWNER_HYPERPARAMETERS:
        raise ValueError(
            f"unknown or owner-unsettable hyperparameter {raw!r} "
            f"(settable: {', '.join(sorted(OWNER_HYPERPARAMETERS))})"
        )
    return raw


def _prompt_set_args(
    app_ctx: AppContext, netuid: int, name: Optional[str], value: Optional[str]
) -> tuple[str, str]:
    """Prompt for the missing ``--name``/``--value``. When the name is missing,
    the subnet's current hyperparameter values are listed first so the user can
    see what they are changing (and from what)."""
    kwargs: dict[str, Any] = {"name": name, "value": value}
    specs: list[PromptSpec] = []
    if name is None:
        if interactive(app_ctx):
            params = app_ctx.run(lambda c: fetch_hyperparameters(c, netuid))
            show_hyperparameters(
                app_ctx,
                netuid,
                params,
                None,
                hint="these are the current values; only owner-settable "
                "parameters can be changed (a wrong name lists them)",
            )
        specs.append(
            PromptSpec(
                field="name",
                flag="--name",
                help="Hyperparameter name.",
                parse=_parse_hyperparameter_name,
            )
        )
    if value is None:

        def _parse_value(_app_ctx: AppContext, raw: str) -> str:
            if kwargs["name"] is not None:
                to_raw(kwargs["name"], raw)  # validate the form; the intent converts again
            return raw

        specs.append(
            PromptSpec(field="value", flag="--value", help=_VALUE_HELP, parse=_parse_value)
        )
    fill_missing(app_ctx, specs, kwargs)
    return kwargs["name"], kwargs["value"]


@app.command("set", rich_help_panel=PANEL_SUBNETS)
@with_tx_globals
def sudo_set(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetHyperparameter.field_help("netuid")),
    name: Optional[str] = typer.Option(
        None,
        "--name",
        help="Name of the hyperparameter to change; an unknown name lists the "
        "owner-settable ones. (required)",
    ),
    value: Optional[str] = typer.Option(None, "--value", help=f"{_VALUE_HELP} (required)"),
):
    """Set an owner-settable subnet hyperparameter.

    Only the subnet owner coldkey can sign this. The change takes effect
    immediately, and some parameters are rate-limited by the chain. Use
    `btcli sudo get` to see current values and per-parameter help.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if name is None or value is None:
        name, value = _prompt_set_args(app_ctx, netuid, name, value)
    try:
        intent = SetHyperparameter(netuid=netuid, name=name, value=value)
    except (ValueError, OverflowError) as error:
        app_ctx.output.error(
            str(error),
            help=f"`btcli sudo get --netuid {netuid} --name {name}` explains "
            "the parameter and the value forms it accepts",
        )
        raise typer.Exit(2)
    app_ctx.submit(intent)


@app.command("get", rich_help_panel=PANEL_SUBNETS)
@with_globals
def sudo_get(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    name: Optional[str] = typer.Option(
        None,
        "--name",
        help="Explain a single hyperparameter in detail, including the command that changes it.",
    ),
):
    """Show subnet hyperparameters."""
    app_ctx: AppContext = ctx_of(ctx)
    params = app_ctx.run(lambda c: fetch_hyperparameters(c, netuid))
    show_hyperparameters(app_ctx, netuid, params, name)


@app.command("get-take", rich_help_panel=PANEL_VALIDATORS)
@with_globals
def get_take(
    ctx: typer.Context,
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Show the delegate take for a hotkey."""
    app_ctx: AppContext = ctx_of(ctx)
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    record = app_ctx.run(lambda c: c.read("delegate_take", hotkey_ss58=hotkey))
    app_ctx.output.detail(
        None,
        {
            "hotkey": hotkey,
            "take": f"{record['take']:.2%}  ({record['take_u16']} as u16)",
            "allowed": f"{record['min']:.2%} – {record['max']:.2%}",
        },
        json_fields=record,
    )


_TAKE_HELP = (
    "New take: a 0..1 fraction with a decimal point (e.g. 0.18) "
    f"or the raw u16 integer (0..{U16_MAX})."
)


def _parse_take(_app_ctx: AppContext, raw: str) -> int:
    """Convert a take to its raw u16, with the hyperparameter value rules:
    a decimal point marks the human 0..1 fraction, a bare integer is raw."""
    return take_to_u16(raw)


def _prompt_take(app_ctx: AppContext, hotkey: str) -> int:
    """Prompt for ``--take``, showing the hotkey's current take first (like the
    hyperparameter set flow shows the current values), and validating answers
    against the chain's allowed range."""
    record: Optional[dict] = None
    if interactive(app_ctx):
        record = app_ctx.run(lambda c: c.read("delegate_take", hotkey_ss58=hotkey))
        app_ctx.output.detail(
            None,
            {
                "current take": f"{record['take']:.2%}  ({record['take_u16']} as u16)",
                "allowed": f"{record['min']:.2%} – {record['max']:.2%}",
            },
        )

    def _parse(app_ctx: AppContext, raw: str) -> int:
        value = _parse_take(app_ctx, raw)
        if record is not None and not (record["min"] <= value / U16_MAX <= record["max"]):
            raise ValueError(
                f"take {value / U16_MAX:.2%} is outside the allowed range "
                f"{record['min']:.2%} – {record['max']:.2%}"
            )
        return value

    kwargs: dict[str, Any] = {"take": None}
    fill_missing(
        app_ctx,
        [PromptSpec(field="take", flag="--take", help=_TAKE_HELP, parse=_parse)],
        kwargs,
    )
    return kwargs["take"]


@app.command("set-take", rich_help_panel=PANEL_VALIDATORS)
@with_tx_globals
def set_take(
    ctx: typer.Context,
    take: Optional[str] = typer.Option(None, "--take", help=f"{_TAKE_HELP} (required)"),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Set delegate take (delegates to tx set-take).

    Signed by the coldkey that owns the hotkey. The take is the fraction
    of staker rewards the delegate keeps for itself; the chain bounds the
    allowed range and rate-limits changes.
    """
    app_ctx: AppContext = ctx_of(ctx)
    # The wallet/hotkey is confirmed first: the current take shown before the
    # --take prompt depends on which hotkey the command targets.
    hotkey = app_ctx.resolve_address("hotkey_ss58", hotkey_ss58)
    if take is None:
        take_u16 = _prompt_take(app_ctx, hotkey)
    else:
        try:
            take_u16 = _parse_take(app_ctx, take)
        except ValueError as error:
            app_ctx.output.error(str(error))
            raise typer.Exit(2)
    app_ctx.submit(SetTake(take=take_u16, hotkey_ss58=hotkey))


@app.command("check-start", rich_help_panel=PANEL_SUBNETS)
@with_globals
def check_start(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
):
    """Show when a subnet can call start_call."""
    app_ctx: AppContext = ctx_of(ctx)
    schedule = app_ctx.run(lambda c: c.read("subnet_start_schedule", netuid=netuid))
    app_ctx.output.detail(f"start schedule netuid {netuid}", schedule)


@app.command("start", rich_help_panel=PANEL_SUBNETS)
@with_tx_globals
def start_subnet(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=StartCall.field_help("netuid")),
):
    """Start a registered subnet.

    Only the subnet owner can call this, and only after the
    post-registration waiting period (see `btcli sudo check-start`).
    It activates emissions for the subnet and cannot be undone.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(StartCall(netuid=netuid))


@app.command("set-symbol", rich_help_panel=PANEL_SUBNETS)
@with_tx_globals
def set_symbol(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=UpdateSymbol.field_help("netuid")),
    symbol: str = typer.Option(..., "--symbol", help=UpdateSymbol.field_help("symbol")),
):
    """Update a subnet symbol.

    Only the subnet owner can call this. The symbol is the ticker shown
    for the subnet's alpha currency across explorers and the CLI.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(UpdateSymbol(netuid=netuid, symbol=symbol))


@app.command("get-identity", rich_help_panel=PANEL_SUBNETS)
@with_globals
def get_subnet_identity(
    ctx: typer.Context,
    netuid: int = typer.Argument(..., help=_NETUID_HELP),
):
    """Show subnet identity metadata."""
    app_ctx: AppContext = ctx_of(ctx)
    identity = app_ctx.run(lambda c: c.read("subnet_identity", netuid=netuid))
    app_ctx.output.detail(f"subnet {netuid} identity", identity)


@app.command("set-identity", rich_help_panel=PANEL_SUBNETS)
@with_tx_globals
def set_subnet_identity(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=SetSubnetIdentity.field_help("netuid")),
    name: str = typer.Option(..., "--name", help=SetSubnetIdentity.field_help("subnet_name")),
    url: str = typer.Option("", "--url", help=SetSubnetIdentity.field_help("subnet_url")),
    description: str = typer.Option(
        "", "--description", help=SetSubnetIdentity.field_help("description")
    ),
):
    """Set subnet identity metadata (subnet owner).

    Only the subnet owner can call this. Publishes the display name, URL,
    and description that explorers and the CLI show for the subnet,
    replacing any identity set before.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(
        SetSubnetIdentity(
            netuid=netuid,
            subnet_name=name,
            subnet_url=url,
            description=description,
        )
    )


@app.command("trim", rich_help_panel=PANEL_SUBNETS)
@with_tx_globals
def trim_subnet(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=TrimSubnet.field_help("netuid")),
    max_n: int = typer.Option(..., "--max-n", help=TrimSubnet.field_help("max_n")),
):
    """Trim a subnet to at most max_n UIDs.

    Only the subnet owner can call this. Neurons beyond the new maximum
    are deregistered immediately and lose their UIDs; they can only get
    back in by registering again.
    """
    app_ctx: AppContext = ctx_of(ctx)
    app_ctx.submit(TrimSubnet(netuid=netuid, max_n=max_n))


@app.command("stake-burn", rich_help_panel=PANEL_SUBNETS)
@with_tx_globals
def stake_burn(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=StakeBurn.field_help("netuid")),
    amount_tao: str = typer.Option(
        ..., "--amount-tao", "--amount", help=StakeBurn.field_help("amount_tao")
    ),
    limit_price: int = typer.Option(..., "--limit-price", help=StakeBurn.field_help("limit_price")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=ss58_param_help("hotkey_ss58")
    ),
):
    """Execute a stake-burn (buyback) extrinsic.

    Spends the given amount of TAO from the wallet coldkey to buy and
    burn the subnet's alpha, subject to the limit price. The spend is
    irreversible once the extrinsic is included.
    """
    app_ctx: AppContext = ctx_of(ctx)
    try:
        amount = _parse_money(amount_tao, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount-tao`: {error}")
        raise typer.Exit(2)
    app_ctx.submit(
        StakeBurn(
            netuid=netuid,
            amount_tao=amount,
            limit_price=limit_price,
            hotkey_ss58=hotkey_ss58,
        )
    )


app.command("buyback", hidden=True)(stake_burn)


mechanisms_app = typer.Typer(no_args_is_help=True, help="Subnet mechanism configuration.")
app.add_typer(mechanisms_app, name="mechanisms", rich_help_panel=PANEL_SUBNETS)
app.add_typer(mechanisms_app, name="mech", hidden=True)


@mechanisms_app.command("count")
@with_tx_globals
def mechanism_count(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
    value: Optional[int] = typer.Option(
        None,
        "--value",
        help="New mechanism count to set (subnet owner only). Omit to just "
        "display the current count.",
    ),
):
    """Get or set mechanism count.

    Without --value this is a read-only query. With --value it submits a
    transaction that only the subnet owner can sign.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if value is None:
        count = app_ctx.run(lambda c: c.read("mechanism_count", netuid=netuid))
        app_ctx.output.detail(None, {"netuid": netuid, "mechanism_count": count})
        return
    app_ctx.submit(SetMechanismCount(netuid=netuid, mechanism_count=value))


@mechanisms_app.command("emissions")
@with_globals
def mechanism_emissions(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=_NETUID_HELP),
):
    """Show mechanism emission split."""
    app_ctx: AppContext = ctx_of(ctx)
    split = app_ctx.run(lambda c: c.read("mechanism_emission_split", netuid=netuid))
    app_ctx.output.detail(None, {"netuid": netuid, "split": split})
