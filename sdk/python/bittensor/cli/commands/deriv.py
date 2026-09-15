"""`btcli deriv`: long and short positions on subnet alpha.

One position per coldkey and subnet. `short` and `long` are the same call with
the side fixed: they add to the position, take from it, or flip it. `close`
settles it; only the owner can. Once a week the chain collects the interest
out of the cushion, and forfeits a position whose cushion cannot pay. There is
no expiry.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Optional

import typer

from ...balance import Balance
from ...intents import AddPosition, ClosePosition
from ...intents.derivatives import leverage_percent
from ...settings import guide_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..tx import _parse_money

# Default `--max-slippage`, in percent of the quoted payout.
DEFAULT_MAX_SLIPPAGE_PCT = 1.0

MAX_SLIPPAGE_HELP = (
    "Your floor on what the settlement pays you, in percent under the quoted payout: the call "
    "rolls back (`SettlementBelowMinimum`) if it would pay less than "
    "`quote × (1 - max_slippage/100)` after interest. Bounds what a price pushed against you in "
    "the same block can take. 100 disables the floor."
)

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "Long and short positions on subnet alpha, borrowed from the subnet's own pool. "
        "One position per subnet; `short` and `long` add to it, against it, or through zero."
        f"\n\nGuide: {guide_docs_url('derivatives')}"
    ),
)

POSITIONS_TITLE = "derivative positions (equity estimated on a constant-product curve)"
POSITIONS_COLUMNS = [
    "netuid",
    "side",
    "lev",
    "cushion",
    "proceeds",
    "debt",
    "interest",
    "runway",
    "equity",
]
POSITIONS_LEGEND = [
    ("lev", "exposure over cushion: the blend of every tranche added"),
    ("cushion", "TAO you have put up, returned as the position settles"),
    ("proceeds", "what the opening trades produced (TAO for a short, alpha for a long)"),
    ("debt", "what must be bought back or repaid to the pool at settlement"),
    ("interest", "interest accrued since the chain last collected it from the cushion"),
    ("runway", "days the cushion keeps paying interest; at zero the chain forfeits the position"),
    ("equity", "cushion + proceeds - debt - interest, the debt priced with slippage, in TAO"),
]


def _add_options():
    """The option set `short` and `long` share."""
    return dict(
        netuid=typer.Option(..., "--netuid", help=AddPosition.field_help("netuid")),
        amount=typer.Option(..., "--amount", help=AddPosition.field_help("amount")),
        leverage=typer.Option(1.0, "--leverage", help=AddPosition.field_help("leverage")),
        max_slippage=typer.Option(
            DEFAULT_MAX_SLIPPAGE_PCT,
            "--max-slippage",
            min=0.0,
            max=100.0,
            help=MAX_SLIPPAGE_HELP
            + " Only binds when the add reduces or closes your position; an open pays "
            "nothing out and sets no floor.",
        ),
    )


def _floor(quoted: Balance, max_slippage_pct: float) -> Balance:
    """The user floor: ``quoted × (1 - max_slippage / 100)``, floored to whole rao."""
    factor = Decimal(1) - Decimal(str(max_slippage_pct)) / Decimal(100)
    return Balance.from_rao(max(int(Decimal(quoted.rao) * factor), 0))


async def _quote_buyback_rao(client, netuid: int, alpha_rao: int, estimate_rao: int) -> int:
    """TAO the pool charges right now for exactly `alpha_rao` alpha.

    The swap simulation quotes alpha out for TAO in; one pass from the
    constant-product estimate and a proportional correction lands within the
    simulation's own rounding of the exact-output figure, and the user's
    slippage margin covers the rest.
    """
    if alpha_rao <= 0:
        return 0
    probe = max(estimate_rao, 1)
    quote = await client.read("quote_stake", netuid=netuid, amount_tao=Balance.from_rao(probe).tao)
    got = quote.alpha.rao
    if got <= 0:
        return estimate_rao
    return -(-probe * alpha_rao // got)


async def _quote_payout(client, coldkey: str, netuid: int, fraction: Decimal) -> Optional[Balance]:
    """What settling `fraction` of `coldkey`'s position on `netuid` would pay right now,
    by the pool's own swap simulation, after the interest the whole position owes. None
    when there is no position to settle."""
    pos = await client.read("derivative_position", coldkey_ss58=coldkey, netuid=netuid)
    if pos is None:
        return None
    fraction = min(max(fraction, Decimal(0)), Decimal(1))
    cushion = int(Decimal(pos["cushion"].rao) * fraction)
    proceeds = int(Decimal(pos["proceeds"].rao) * fraction)
    debt = int(Decimal(pos["debt"].rao) * fraction)
    interest = pos["interest_due_tao"].rao
    if pos["side"] == "Short":
        # The read's equity prices the whole debt on a constant-product curve; scale it to
        # the share as the first guess for the exact-output quote.
        estimate = pos["cushion"].rao + pos["proceeds"].rao - pos["equity_tao"].rao - interest
        estimate = int(Decimal(max(estimate, 0)) * fraction)
        cost = await _quote_buyback_rao(client, netuid, debt, estimate)
        payout = cushion + proceeds - cost - interest
    else:
        sale = await client.read(
            "quote_unstake", netuid=netuid, amount_alpha=Balance.from_rao(proceeds).tao
        )
        payout = cushion + sale.tao.rao - debt - interest
    return Balance.from_rao(max(payout, 0))


def _floor_rows(quoted: Optional[Balance], floor: Balance, max_slippage_pct: float) -> list[tuple]:
    if quoted is None:
        return [
            (
                "warning",
                "no quote available: the floor is 0, so a price pushed against the settlement "
                "in the same block can take from what you get back",
                "yellow",
            )
        ]
    rows: list[tuple] = [("expected payout", f"~{quoted} (quoted now, after interest)")]
    if floor.rao > 0:
        rows.append(
            (
                "minimum payout",
                f"{floor} (expected less {max_slippage_pct:g}% max slippage; the call rolls "
                "back below this)",
            )
        )
    else:
        rows.append(
            (
                "warning",
                "the quote says this settlement pays nothing (underwater): with no floor it "
                "forfeits the share to the pool in kind",
                "yellow",
            )
        )
    return rows


def _submit_add(
    app_ctx: AppContext,
    side: str,
    netuid: int,
    amount: str,
    leverage: float,
    max_slippage: float,
) -> None:
    try:
        money = _parse_money(amount, False)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--amount`: {error}")
        raise typer.Exit(2)
    try:
        percent = leverage_percent(leverage)
    except ValueError as error:
        app_ctx.output.error(f"invalid value for `--leverage`: {error}")
        raise typer.Exit(2)
    owner = app_ctx.review_account()

    async def _settling_share(client) -> Optional[Balance]:
        """The quoted payout of the share this add settles, or None if it settles nothing:
        no position, or one on the same side."""
        if owner is None:
            return None
        pos = await client.read("derivative_position", coldkey_ss58=owner, netuid=netuid)
        if pos is None or pos["side"] == side:
            return None
        asked = Decimal(money.rao) * Decimal(percent) / Decimal(100)
        held = Decimal(pos["exposure_tao"].rao)
        fraction = asked / held if held > 0 else Decimal(1)
        return await _quote_payout(client, owner, netuid, fraction)

    quoted: Optional[Balance] = None
    settles = False
    if max_slippage < 100.0:
        try:
            with app_ctx.output.activity("quoting the settlement…"):
                quoted = app_ctx.run(_settling_share)
            settles = quoted is not None
        except Exception:
            # The floor is a convenience; a quoting hiccup must not block the add.
            quoted = None
    floor = _floor(quoted, max_slippage) if quoted is not None else Balance.from_rao(0)
    intent = AddPosition(
        netuid=netuid, side=side, amount=money, leverage=leverage, min_amount_out=floor
    )
    if not settles:
        app_ctx.submit(intent)
        return
    app_ctx.submit(
        intent,
        card_sections=[("Settlement", _floor_rows(quoted, floor, max_slippage))],
    )


_ADD = _add_options()


@app.command("short")
@with_tx_globals
def add_short(
    ctx: typer.Context,
    netuid: int = _ADD["netuid"],
    amount: str = _ADD["amount"],
    leverage: float = _ADD["leverage"],
    max_slippage: float = _ADD["max_slippage"],
):
    """Add short exposure: borrow alpha from the pool and sell it for TAO now.

    Profit if alpha's price falls before you settle; the cushion covers the
    loss if it rises. `--amount` is the TAO the tranche is sized by and
    `--leverage` the multiple of it, up to 1x. With no position, or a short,
    `--amount` is deposited as cushion. Against a long it takes that much off
    at the current price instead, and flips to a short if there is more.

    When it takes exposure off a long, btcli quotes that share's payout first
    and sets `min_amount_out` to the quote less `--max-slippage` (default 1%),
    so a pool that moves against you between the quote and execution rolls
    the call back instead of paying less.
    """
    _submit_add(ctx_of(ctx), "Short", netuid, amount, leverage, max_slippage)


@app.command("long")
@with_tx_globals
def add_long(
    ctx: typer.Context,
    netuid: int = _ADD["netuid"],
    amount: str = _ADD["amount"],
    leverage: float = _ADD["leverage"],
    max_slippage: float = _ADD["max_slippage"],
):
    """Add long exposure: borrow TAO from the pool and buy alpha with it now.

    Profit if alpha's price rises before you settle; the cushion covers the
    loss if it falls. `--amount` is the TAO the tranche is sized by and
    `--leverage` the multiple of it, up to 1.5x. With no position, or a long,
    `--amount` is deposited as cushion. Against a short it takes that much off
    at the current price instead, and flips to a long if there is more.

    When it takes exposure off a short, btcli quotes that share's payout first
    and sets `min_amount_out` to the quote less `--max-slippage` (default 1%),
    so a pool that moves against you between the quote and execution rolls
    the call back instead of paying less.

    Not enabled at launch: while `longs_enabled` in `deriv params` is off,
    this fails with `LongsDisabled` whenever it would leave a long open.
    Reducing or closing a short with it still works.
    """
    _submit_add(ctx_of(ctx), "Long", netuid, amount, leverage, max_slippage)


@app.command("close")
@with_tx_globals
def close_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help=ClosePosition.field_help("netuid")),
    max_slippage: float = typer.Option(
        DEFAULT_MAX_SLIPPAGE_PCT,
        "--max-slippage",
        min=0.0,
        max=100.0,
        help=MAX_SLIPPAGE_HELP,
    ),
):
    """Close your position on a subnet and settle it against the pool.

    The trade is reversed at today's price, the pool is repaid with the
    interest owed, and you get what is left of your cushion.

    btcli quotes the payout first and sets `min_amount_out` to the quote less
    `--max-slippage` (default 1%), so a pool that moves against you between
    the quote and execution rolls the close back instead of paying less. If
    the quote says the position is underwater the floor is 0 and the close
    forfeits everything to the pool; `--max-slippage 100` disables the floor.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.review_account()
    quoted: Optional[Balance] = None
    if owner is not None and max_slippage < 100.0:
        try:
            with app_ctx.output.activity("quoting the close…"):
                quoted = app_ctx.run(
                    lambda client: _quote_payout(client, owner, netuid, Decimal(1))
                )
        except Exception:
            quoted = None
    floor = _floor(quoted, max_slippage) if quoted is not None else Balance.from_rao(0)
    app_ctx.submit(
        ClosePosition(netuid=netuid, min_amount_out=floor),
        card_sections=[("Settlement", _floor_rows(quoted, floor, max_slippage))],
    )


def _runway_cell(days: Optional[float]) -> str:
    if days is None:
        return "-"
    return f"{max(days, 0.0):.0f}d"


def _position_row(pos: dict) -> list:
    return [
        pos["netuid"],
        pos["side"],
        f"{pos['leverage']:g}x",
        str(pos["cushion"]),
        str(pos["proceeds"]),
        str(pos["debt"]),
        str(pos["interest_due_tao"]),
        _runway_cell(pos["runway_days"]),
        str(pos["equity_tao"]),
    ]


def _position_record(pos: dict) -> dict:
    return {
        "coldkey": pos["coldkey"],
        "netuid": pos["netuid"],
        "side": pos["side"],
        "leverage": pos["leverage"],
        "cushion": str(pos["cushion"]),
        "proceeds": str(pos["proceeds"]),
        "debt": str(pos["debt"]),
        "escrow": str(pos["escrow"]),
        "exposure_tao": pos["exposure_tao"].tao,
        "interest_per_year_tao": pos["interest_per_year_tao"].tao,
        "interest_due_tao": pos["interest_due_tao"].tao,
        "since": pos["since"],
        "due": pos["due"],
        "runway_days": pos["runway_days"],
        "equity_tao": pos["equity_tao"].tao,
    }


@app.command("list")
@with_globals
def list_positions(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    netuid: Optional[int] = typer.Option(
        None, "--netuid", help="Only show the position on this subnet."
    ),
):
    """List a coldkey's open positions, one per subnet, with interest, runway, and equity.

    Runway is how many days the cushion keeps paying interest at the current
    rate; when it reaches zero the chain forfeits the position to the pool.
    Add cushion to extend it. Equity prices the buyback or sale on a
    constant-product curve and subtracts the interest due; the chain's own
    quote decides at settlement.
    """
    app_ctx: AppContext = ctx_of(ctx)
    owner = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)
    positions = app_ctx.run(lambda client: client.read("derivative_positions", coldkey_ss58=owner))
    if netuid is not None:
        positions = [p for p in positions if p["netuid"] == netuid]
    app_ctx.output.table(
        POSITIONS_TITLE,
        POSITIONS_COLUMNS,
        [_position_row(pos) for pos in positions],
        [_position_record(pos) for pos in positions],
        legend=POSITIONS_LEGEND,
    )


@app.command("params")
@with_globals
def show_params(ctx: typer.Context):
    """Show the two switches, the three parameters, and the fixed limits.

    `enabled` is the network-wide switch: off until governance turns it on.
    While it is off, `short` and `long` fail with `DerivativesDisabled`;
    `close` still works. `longs_enabled` is the long-side switch, also off
    at launch: while it is off, `long` fails with `LongsDisabled` whenever it
    would leave a long open; `short` and `close` are unaffected. The three
    parameters are the pool share and the short and long interest rate.
    """
    app_ctx: AppContext = ctx_of(ctx)
    params = app_ctx.run(lambda client: client.read("derivatives_params"))
    app_ctx.output.detail("derivatives params", params)
