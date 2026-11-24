"""CLI for synthetic-mint shorts: open, increase, collateral, close, poke, list, show."""

from __future__ import annotations

from decimal import InvalidOperation
from typing import Any, NoReturn, Optional, cast

import typer

from ...balance import Balance
from ...intents import (
    AddShortCollateral,
    CloseShort,
    IncreaseShort,
    OpenShort,
    PokeShort,
    WithdrawShortCollateral,
)
from ...intents._money import alpha_amount, tao_amount
from ...intents.lending import DEFAULT_SHORT_TOLERANCE, ceiling_rao, floor_rao
from ...result import BittensorError
from ...settings import ALPHA_SYMBOL, TAO_SYMBOL, query_docs_url
from ..context import address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..prompt import confirm_wallet

app = typer.Typer(
    no_args_is_help=True,
    context_settings={"max_content_width": 200, "help_option_names": ["-h", "--help"]},
    help=(
        "Synthetic-mint shorts: open, increase, add-collateral, withdraw, close, poke, "
        "list, show, params.\n\n"
        "A short mints subnet alpha and sells it. The proceeds stay locked beside your "
        "TAO collateral, which must cover your share of buying the whole subnet book back "
        "after a stressed pump. Interest is a flat APR in TAO. Unhealthy shorts "
        "deleverage through paced buy-backs; there is no expiry.\n\n"
        "Queries: lending_position / lending_open_quote / lending_close_quote / "
        "lending_subnet / lending_params."
    ),
)

SLIPPAGE_HELP = (
    "Percent tolerance on the quote (default 0.5%). Must be > 0: the price moves one "
    "block after the quote."
)


def _fmt_amount(amount: Any) -> str:
    """τ / α_N money text (not subnet token glyphs)."""
    if not isinstance(amount, Balance):
        return str(amount)
    if amount.netuid == 0:
        return f"{TAO_SYMBOL}{amount.amount:,.9f}"
    return f"{amount.amount:,.9f}{ALPHA_SYMBOL}_{amount.netuid}"


def _exit_error(context, message: str, code: int = 2) -> NoReturn:
    context.output.error(message)
    raise typer.Exit(code)


def _amount(context, value: str, *, netuid: int = 0, label: str) -> Balance:
    """Parse a positive CLI amount: alpha on ``netuid``, TAO when ``netuid`` is 0."""
    try:
        balance = cast(Balance, alpha_amount(value, netuid) if netuid else tao_amount(value))
    except (BittensorError, InvalidOperation, ValueError, TypeError, OverflowError):
        _exit_error(context, f"{label} must be a positive number, got {value!r}")
    if balance.rao <= 0:
        _exit_error(context, f"{label} must be greater than zero")
    return balance


def _tolerance(context, max_slippage: float) -> float:
    if max_slippage <= 0:
        _exit_error(
            context, "--max-slippage must be > 0: the price moves one block after the quote"
        )
    return max_slippage / 100


def _owner(context, proxy: Optional[str]) -> str:
    """The account the short belongs to: the proxied account, else the signer."""
    if proxy is not None:
        return proxy
    owner = context.review_account()
    if owner is None:
        _exit_error(context, "select the wallet whose coldkey owns this short")
    return owner


def _position(context, owner: str, netuid: int) -> dict:
    position = context.run(
        lambda client: client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    )
    if position is None:
        _exit_error(context, f"no short on netuid {netuid} for {owner}", code=1)
    return position


def _coverage(position: dict) -> str:
    cost = position["coverage_cost"]
    return _fmt_amount(cost) if cost is not None else "unbounded"


def _status(position: dict) -> str:
    if position.get("deleveraging"):
        return "deleveraging"
    return "ok" if position.get("covered") else "uncovered"


def _json(record: dict) -> dict:
    return {k: v.rao if isinstance(v, Balance) else v for k, v in record.items()}


@app.command("open")
@with_tx_globals
def open_short(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet to short."),
    mint: str = typer.Option(..., "--mint", help=OpenShort.field_help("mint_alpha")),
    collateral: Optional[str] = typer.Option(
        None,
        "--collateral",
        help="TAO collateral. Omit to post the quoted minimum plus --max-slippage.",
    ),
    max_slippage: float = typer.Option(
        DEFAULT_SHORT_TOLERANCE * 100, "--max-slippage", min=0.0, max=99.0, help=SLIPPAGE_HELP
    ),
):
    """Mint subnet alpha, sell it, and lock the proceeds beside TAO collateral.

    Quotes the sale and the minimum collateral first. The sale floor is the
    quoted proceeds less --max-slippage. A refundable 0.1 TAO deposit is
    charged on top of the collateral. One short per coldkey and subnet; grow
    it with `increase`.
    """
    context = ctx_of(ctx)
    tolerance = _tolerance(context, max_slippage)
    mint_alpha = _amount(context, mint, netuid=netuid, label="--mint")
    confirm_wallet(context, help_text="Wallet whose coldkey owns this short.")
    proxy = context.resolve_dispatch_proxy()
    owner = _owner(context, proxy)

    async def _quote(client):
        existing = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        quote = await client.read("lending_open_quote", netuid=netuid, mint_alpha=mint)
        return existing, quote

    with context.output.activity("quoting the short…"):
        existing, quote = context.run(_quote)
    if existing is not None:
        _exit_error(context, f"{owner} already shorts netuid {netuid}; use `increase`", code=1)
    required = quote["required_collateral"]
    provided = (
        _amount(context, collateral, label="--collateral")
        if collateral is not None
        else Balance.from_rao(ceiling_rao(required.rao, tolerance))
    )
    if provided.rao < required.rao:
        _exit_error(
            context,
            f"--collateral {_fmt_amount(provided)} is below the required {_fmt_amount(required)}",
            code=1,
        )
    floor = Balance.from_rao(floor_rao(quote["proceeds"].rao, tolerance))
    rows = [
        ("owner", owner),
        ("mint", _fmt_amount(mint_alpha)),
        ("quoted sale proceeds", _fmt_amount(quote["proceeds"])),
        ("minimum sale proceeds", _fmt_amount(floor)),
        ("required collateral", _fmt_amount(required)),
        ("collateral", _fmt_amount(provided)),
        ("deposit", _fmt_amount(quote["deposit"])),
        ("APR", f"{quote['apr_bps'] / 100:g}%"),
        ("annual interest", _fmt_amount(quote["annual_interest"])),
    ]
    if quote["in_prune_zone"]:
        rows.append(("prune zone", "yes: settles at its own proceeds on deregistration"))
    context.submit(
        OpenShort(
            netuid=netuid,
            mint_alpha=mint_alpha,
            collateral=provided,
            min_tao_out=floor,
            rate_tolerance=tolerance,
        ),
        proxy_for=proxy or "self",
        card_sections=[("Short", rows)],
    )


@app.command("increase")
@with_tx_globals
def increase_short(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your short."),
    mint: Optional[str] = typer.Option(None, "--mint", help=IncreaseShort.field_help("extra_mint")),
    collateral: Optional[str] = typer.Option(
        None, "--collateral", help=IncreaseShort.field_help("extra_collateral")
    ),
    max_slippage: float = typer.Option(
        DEFAULT_SHORT_TOLERANCE * 100, "--max-slippage", min=0.0, max=99.0, help=SLIPPAGE_HELP
    ),
):
    """Mint and sell more alpha, add collateral, or both, on an open short.

    The grown position must still cover its whole-book share. The sale floor
    is the simulated sale less --max-slippage.
    """
    context = ctx_of(ctx)
    tolerance = _tolerance(context, max_slippage)
    if mint is None and collateral is None:
        _exit_error(context, "pass --mint, --collateral, or both")
    extra_mint = _amount(context, mint, netuid=netuid, label="--mint") if mint else None
    extra_collateral = _amount(context, collateral, label="--collateral") if collateral else None
    proxy = context.resolve_dispatch_proxy()
    owner = _owner(context, proxy)
    position = _position(context, owner, netuid)
    rows = [
        ("owner", owner),
        ("minted", _fmt_amount(position["minted"])),
        ("funds", _fmt_amount(position["funds"])),
        ("extra mint", _fmt_amount(extra_mint) if extra_mint else "—"),
        ("extra collateral", _fmt_amount(extra_collateral) if extra_collateral else "—"),
    ]
    context.submit(
        IncreaseShort(
            netuid=netuid,
            extra_mint=extra_mint or 0,
            extra_collateral=extra_collateral or 0,
            rate_tolerance=tolerance,
        ),
        proxy_for=proxy or "self",
        card_sections=[("Increase", rows)],
    )


def _collateral_command(ctx, netuid: int, amount: str, *, withdraw: bool) -> None:
    context = ctx_of(ctx)
    value = _amount(context, amount, label="--amount")
    proxy = context.resolve_dispatch_proxy()
    owner = _owner(context, proxy)
    position = _position(context, owner, netuid)
    rows = [
        ("owner", owner),
        ("collateral", _fmt_amount(position["collateral"])),
        ("coverage cost", _coverage(position)),
        ("withdraw" if withdraw else "add", _fmt_amount(value)),
    ]
    intent = (
        WithdrawShortCollateral(netuid=netuid, amount=value)
        if withdraw
        else AddShortCollateral(netuid=netuid, amount=value)
    )
    context.submit(intent, proxy_for=proxy or "self", card_sections=[("Collateral", rows)])


@app.command("add-collateral")
@with_tx_globals
def add_collateral(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your short."),
    amount: str = typer.Option(..., "--amount", help=AddShortCollateral.field_help("amount")),
):
    """Add TAO collateral. Works while disabled and while deleveraging."""
    _collateral_command(ctx, netuid, amount, withdraw=False)


@app.command("withdraw")
@with_tx_globals
def withdraw_collateral(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your short."),
    amount: str = typer.Option(..., "--amount", help=WithdrawShortCollateral.field_help("amount")),
):
    """Withdraw TAO collateral while coverage plus 7 days of interest still hold."""
    _collateral_command(ctx, netuid, amount, withdraw=True)


@app.command("close")
@with_tx_globals
def close_short(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your short."),
    alpha: Optional[str] = typer.Option(None, "--alpha", help=CloseShort.field_help("alpha")),
    max_slippage: float = typer.Option(
        DEFAULT_SHORT_TOLERANCE * 100, "--max-slippage", min=0.0, max=99.0, help=SLIPPAGE_HELP
    ),
):
    """Buy back and burn short alpha from the position's funds.

    Closing the whole position refunds the remaining funds and the deposit.
    The cost ceiling is the quoted buy-back plus --max-slippage.
    """
    context = ctx_of(ctx)
    tolerance = _tolerance(context, max_slippage)
    amount = _amount(context, alpha, netuid=netuid, label="--alpha") if alpha else None
    proxy = context.resolve_dispatch_proxy()
    owner = _owner(context, proxy)
    with context.output.activity("quoting the buy-back…"):
        quote = context.run(
            lambda client: client.read(
                "lending_close_quote", coldkey_ss58=owner, netuid=netuid, alpha=alpha or ""
            )
        )
    ceiling = Balance.from_rao(ceiling_rao(quote["tao_in"].rao, tolerance))
    rows = [
        ("owner", owner),
        ("buy back", _fmt_amount(quote["alpha"])),
        ("quoted cost", _fmt_amount(quote["tao_in"])),
        ("maximum cost", _fmt_amount(ceiling)),
        ("refund", _fmt_amount(quote["refund"])),
    ]
    context.submit(
        CloseShort(netuid=netuid, alpha=amount, max_tao_in=ceiling, rate_tolerance=tolerance),
        proxy_for=proxy or "self",
        card_sections=[("Close", rows)],
    )


@app.command("poke")
@with_tx_globals
def poke_short(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of the short."),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Health-check any short: charge interest, start or step deleveraging."""
    context = ctx_of(ctx)
    owner = context.resolve_address("coldkey_ss58", coldkey_ss58)
    if owner is None:
        _exit_error(context, "pass --coldkey for the owner of the short")
    context.submit(PokeShort(owner_ss58=owner, netuid=netuid))


@app.command("list")
@with_globals
def list_positions(
    ctx: typer.Context,
    netuid: Optional[int] = typer.Option(
        None,
        "--netuid",
        min=1,
        max=65535,
        help="Subnet whose shorts to list. Omit to list the wallet's shorts on every subnet.",
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    list_all: bool = typer.Option(
        False, "--all", help="List every short on the subnet. Requires --netuid."
    ),
):
    """List shorts for the wallet coldkey (default) or the whole subnet."""
    context = ctx_of(ctx)
    if list_all and coldkey_ss58 is not None:
        _exit_error(context, "--all and --coldkey are mutually exclusive")
    if list_all and netuid is None:
        _exit_error(context, "--all requires --netuid")
    owner = None if list_all else context.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _positions(client):
        if owner is None:
            return await client.read("lending_positions", netuid=netuid)
        if netuid is None:
            return await client.read("lending_positions_for_coldkey", coldkey_ss58=owner)
        position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        return [position] if position is not None else []

    positions = context.run(_positions)
    rows = [
        [
            str(p["netuid"]),
            p["coldkey"],
            _fmt_amount(p["minted"]),
            _fmt_amount(p["collateral"]),
            _fmt_amount(p["proceeds"]),
            _coverage(p),
            _status(p),
        ]
        for p in positions
    ]
    title = f"shorts on netuid {netuid}" if owner is None else f"shorts for {owner}"
    context.output.table(
        title,
        ["netuid", "owner", "minted", "collateral", "proceeds", "coverage", "status"],
        rows,
        [_json(p) for p in positions],
        truncate=False,
    )


@app.command("params")
@with_globals
def show_params(ctx: typer.Context):
    """Show whether shorts are enabled and the root-settable SynthParams."""
    context = ctx_of(ctx)
    params = context.run(lambda client: client.read("lending_params"))
    context.output.table(
        "short params (root-settable via set_synth_params / set_enabled)",
        ["parameter", "value"],
        [[key, str(value)] for key, value in params.items()],
        truncate=False,
    )


@app.command("show", help="Show one short with its health and a full-close quote.")
@app.command("quote", help="Quote closing one short (read-only).")
@with_globals
def show_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of the short."),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """Show one short and what closing it now would cost (no transaction)."""
    context = ctx_of(ctx)
    owner = context.resolve_address("coldkey_ss58", coldkey_ss58)
    if owner is None:
        _exit_error(context, "select the account that owns this short")

    async def _load(client):
        position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        if position is None:
            return None, None
        try:
            quote = await client.read("lending_close_quote", coldkey_ss58=owner, netuid=netuid)
        except BittensorError as error:
            return position, str(error)
        return position, quote

    position, quote = context.run(_load)
    if position is None:
        _exit_error(context, f"no short on netuid {netuid} for {owner}", code=1)
    fields = {
        "owner": owner,
        "netuid": netuid,
        "status": _status(position),
        "minted": _fmt_amount(position["minted"]),
        "collateral": _fmt_amount(position["collateral"]),
        "proceeds": _fmt_amount(position["proceeds"]),
        "deposit": _fmt_amount(position["deposit"]),
        "coverage cost": _coverage(position),
        "equity (TWAP)": f"{TAO_SYMBOL}{position['equity_twap'] / 1e9:,.9f}",
        "accrued interest": _fmt_amount(position["accrued_interest"]),
        "interest per day": _fmt_amount(position["day_interest"]),
        "prune-zone mint": _fmt_amount(position["zone_minted"]),
        "terms version": position["terms_version"],
    }
    json_fields = _json(position)
    if isinstance(quote, dict):
        fields["close cost"] = _fmt_amount(quote["tao_in"])
        fields["close refund"] = _fmt_amount(quote["refund"])
        json_fields["close_quote"] = _json(quote)
    else:
        fields["close quote"] = f"unavailable: {quote}"
    context.output.detail(
        f"short on netuid {netuid}",
        fields,
        json_fields=json_fields,
        docs=[query_docs_url("lending_position")],
    )
