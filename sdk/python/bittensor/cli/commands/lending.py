"""Three commands for fixed-principal subnet loans: open, close and list."""

from __future__ import annotations

from decimal import Decimal
from enum import Enum
from typing import Optional, cast

import typer

from ...balance import Balance
from ...intents import CloseLoan, OpenLoan, RaiseLoan
from ...intents._money import alpha_amount, tao_amount
from ...result import BittensorError
from ...settings import DEFAULT_ERA_PERIOD, MEV_SHIELD_ERA_PERIOD, guide_docs_url
from ..context import address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..prompt import PromptSpec, ask, confirm_wallet, fill_missing

Side = Enum("Side", [("short", "short"), ("long", "long")], type=str)

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "Lift-based escrowed loans: open, close, raise, list.\n\n"
        f"Guide: {guide_docs_url('pool-lending')}"
    ),
)


def _bound(amount: Balance, percent: float, *, ceiling: bool = False) -> Balance:
    """Integer quote protection with ceilings rounded up and floors down."""
    numerator, denominator = Decimal(str(percent)).as_integer_ratio()
    denominator *= 100
    margin = (amount.rao * numerator + denominator - 1) // denominator
    rao = amount.rao + margin if ceiling else amount.rao - margin
    return Balance.from_rao(max(rao, 0), amount.netuid)


async def _provided_collateral(client, minimum: Balance, netuid: int, owner: str, percent: float):
    """Add quote tolerance and the old coupon's conservative submission window."""
    maximum = (1 << 64) - 1
    if not 0 < minimum.rao <= maximum:
        raise BittensorError("calculated collateral does not fit the chain's deposit units")
    if percent == 0:
        return minimum
    provided = _bound(minimum, percent, ceiling=True).rao
    view = await client.at()
    existing = await view.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    if existing is not None:
        coupon = existing.get("annual_interest")
        if not isinstance(coupon, Balance) or coupon.netuid != minimum.netuid or coupon.rao < 0:
            raise BittensorError("the existing loan's collateral coupon is unavailable or changed")
        year = await view.constant(("Lending", "BlocksPerYear"))
        if not isinstance(year, int) or isinstance(year, bool) or year <= 0:
            raise BittensorError("the lending interest year is unavailable")
        # Shielding may fall back to bare submission, whose normal mortal era
        # is longer. Reserve the old fixed coupon across either submission path.
        window = max(DEFAULT_ERA_PERIOD, MEV_SHIELD_ERA_PERIOD)
        provided += (coupon.rao * window + year - 1) // year
    if provided > maximum:
        raise BittensorError("buffered collateral does not fit the chain's deposit units")
    return view.balance(provided, minimum.netuid)


def _loan_input(context, netuid: int, side: str) -> tuple[Optional[str], Optional[str]]:
    """Choose one input mode and record only its actual CLI flag for replay."""

    def choose_mode(_ctx, value):
        choice = value.lower()
        if choice in ("1", "amount", "borrow"):
            return "amount"
        if choice in ("2", "collateral"):
            return "collateral"
        raise ValueError("choose amount (1) or collateral (2)")

    def choose(console, app_ctx, answers):
        mode, _raw = ask(
            console,
            app_ctx,
            PromptSpec(
                "mode",
                "loan input",
                "1: desired amount to borrow; 2: collateral to provide.",
                choose_mode,
                default="amount",
            ),
        )
        debt = "TAO" if side == "long" else f"subnet {netuid} alpha"
        collateral_unit = "TAO" if side == "short" else f"subnet {netuid} alpha"
        unit = debt if mode == "amount" else collateral_unit

        def parse(_ctx, value):
            try:
                balance = cast(
                    Balance, tao_amount(value) if unit == "TAO" else alpha_amount(value, netuid)
                )
                if balance.rao <= 0:
                    raise BittensorError(f"{mode} must be greater than zero")
                return balance
            except BittensorError as error:
                raise ValueError(str(error)) from error

        value, raw = ask(
            console,
            app_ctx,
            PromptSpec(
                mode,
                f"--{mode}",
                (
                    f"Additional {unit} to borrow; btcli finds the minimum required collateral."
                    if mode == "amount"
                    else f"Additional {unit} to lock; btcli quotes your borrowing limit."
                ),
                parse,
            ),
        )
        answers[mode] = format(value.decimal, "f")
        return [f"--{mode}", raw]

    answers = {"amount": None, "collateral": None}
    fill_missing(
        context,
        [PromptSpec("loan_input", "--amount or --collateral", None, choose_mode, custom=choose)],
        answers,
    )
    return answers["amount"], answers["collateral"]


def _loan_owner(context, proxy: Optional[str]) -> Optional[str]:
    if proxy is not None:
        return proxy
    multisig = context._saved_multisig_address(context.multisig_wallet_name or context.wallet_name)
    if multisig is not None:
        return multisig
    if not context.uses_external_signer():
        return context.review_account()
    address = context.external_signer_address()
    if address is not None and not context.uses_extension_signer():
        return address

    async def select(_client):
        signer = await context.resolve_signing_wallet("coldkey")
        try:
            return signer.ss58_address
        finally:
            if hasattr(signer, "close"):
                await signer.close()

    return context.run(select)


@app.command("open")
@with_tx_globals
def open_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet to borrow on."),
    side: Side = typer.Option(..., "--side", help="short: TAO collateral; long: alpha collateral."),
    collateral: Optional[str] = typer.Option(
        None, "--collateral", help=OpenLoan.field_help("collateral")
    ),
    amount: Optional[str] = typer.Option(None, "--amount", help=OpenLoan.field_help("amount")),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=OpenLoan.field_help("hotkey_ss58")
    ),
    max_slippage: float = typer.Option(
        1.0,
        "--max-slippage",
        min=0.0,
        max=99.0,
        help=(
            "Quote tolerance (percent). Amount mode adds collateral headroom and the existing "
            "coupon's submission window; 0 uses the calculated minimum. Collateral mode sets "
            "a principal floor. Shorts also set an opening value floor."
        ),
    ),
):
    """Add collateral and open or increase a short or long on a subnet.

    Pass --amount for the additional quantity to borrow (alpha for a short,
    TAO for a long) and btcli finds the minimum required collateral. Pass
    --collateral instead to borrow its quoted limit. The minimum additional
    loan has a 0.1 TAO opening value.
    Amount mode provides collateral headroom using --max-slippage (default
    1%) and the old coupon's conservative submission window. Use 0 to provide
    exactly the calculated minimum. Borrowed principal stays exact.

    Shorts force-sell borrowed alpha into escrowed TAO and take no hotkey
    (TAO-only). Longs force-buy with borrowed TAO into escrowed alpha and
    require --hotkey as the collateral source. No free borrowed assets are
    delivered. An existing position must use the same side. btcli quotes
    additional debt after checking the combined position and refuses to
    submit when a quote is unavailable.
    """
    context = ctx_of(ctx)
    if amount is not None and collateral is not None:
        context.output.error("--amount and --collateral are mutually exclusive; provide one")
        raise typer.Exit(2)
    if amount is None and collateral is None:
        amount, collateral = _loan_input(context, netuid, side.value)
    confirm_wallet(context, help_text="Wallet whose coldkey owns this loan.")
    proxy = context.resolve_dispatch_proxy()
    owner = _loan_owner(context, proxy)
    if owner is None:
        context.output.error("select the account that owns this loan before borrowing")
        raise typer.Exit(2)
    # Shorts omit hotkey; longs resolve the collateral source.
    hotkey = None if side == Side.short else context.resolve_address("hotkey_ss58", hotkey_ss58)
    if side == Side.short and hotkey_ss58 is not None:
        context.output.error("short open is TAO-only; omit --hotkey")
        raise typer.Exit(2)
    requested = None
    minimum_collateral = None
    if amount is not None:
        requested = cast(
            Balance, tao_amount(amount) if side == Side.long else alpha_amount(amount, netuid)
        )
        if requested.rao <= 0:
            context.output.error("amount to borrow must be greater than zero")
            raise typer.Exit(2)

        async def size(client):
            required = await client.read(
                "lending_required_collateral",
                netuid=netuid,
                side=side.value,
                amount=requested,
                coldkey_ss58=owner,
                **({"hotkey_ss58": hotkey} if hotkey is not None else {}),
            )
            minimum = required["collateral"]
            provided = await _provided_collateral(client, minimum, netuid, owner, max_slippage)
            return minimum, provided

        with context.output.activity("finding the minimum required collateral…"):
            minimum_collateral, collateral = context.run(size)
    if collateral is None:
        context.output.error("provide --amount or --collateral")
        raise typer.Exit(2)
    intent = OpenLoan(
        netuid=netuid,
        side=side.value,
        collateral=collateral,
        hotkey_ss58=hotkey,
        amount=requested,
    )
    with context.output.activity("quoting the loan…"):
        quote = context.run(
            lambda client: client.read(
                "lending_open_quote",
                netuid=netuid,
                side=side.value,
                collateral=intent.collateral,
                coldkey_ss58=owner,
                **({"hotkey_ss58": hotkey} if hotkey is not None else {}),
                **({"amount": requested} if requested is not None else {}),
            )
        )
    # Amount mode never turns the desired principal into a floor that permits
    # borrowing more. The exact-principal dispatch transfers that quantity.
    intent.min_borrow = (
        requested if requested is not None else _bound(quote["principal"], max_slippage)
    )
    if side == Side.short:
        intent.min_proceeds = _bound(quote["opening_value"], max_slippage)
    loan_rows = [
        ("loan owner", owner),
        *([("collateral hotkey", hotkey)] if hotkey is not None else []),
        *(
            [("calculated minimum collateral", str(minimum_collateral))]
            if minimum_collateral is not None
            else []
        ),
        ("provided collateral", str(intent.collateral)),
        (
            "exact amount to borrow" if requested is not None else "borrowing limit",
            str(quote["principal"]),
        ),
        ("minimum additional principal", str(intent.min_borrow)),
        ("minimum additional opening value", str(intent.min_proceeds)),
        ("additional interest per year", str(quote["annual_interest"])),
    ]
    context.submit(
        intent,
        proxy_for=proxy or "self",
        card_sections=[("Loan", loan_rows)],
    )


@app.command("close")
@with_tx_globals
def close_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your position."),
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help=CloseLoan.field_help("hotkey_ss58"),
    ),
    repay_from_wallet: bool = typer.Option(
        False,
        "--repay-from-wallet/--no-repay-from-wallet",
        help=(
            "False (default): buy short alpha from escrow / sell long escrow (TAO-refund path). "
            "True: repay short alpha from --hotkey stake, or pay long TAO from wallet. "
            "Longs always name --hotkey for alpha refund."
        ),
    ),
    max_slippage: float = typer.Option(
        1.0,
        "--max-slippage",
        min=0.0,
        max=99.0,
        help="Maximum percentage above quoted payment or below quoted refund (default 1%).",
    ),
):
    """Repay fixed principal and return remaining collateral.

    Positions store no hotkey. By default closes take the TAO-refund path
    (escrow AMM buyback for shorts; sell escrow for longs) — matching the
    SDK ``CloseLoan`` default. Pass ``--repay-from-wallet`` to repay short
    alpha from ``--hotkey`` stake or pay long TAO from free balance. Pass
    ``--hotkey`` for alpha repay (short wallet path) or alpha refund (long
    close / short buyback surplus). The full runtime quote sets a payment
    ceiling and refund floor; a failed quote stops submission.
    """
    context = ctx_of(ctx)
    owner = context.review_account()
    if owner is None:
        context.output.error("select the account that owns this position before closing")
        raise typer.Exit(2)
    hotkey = context.resolve_address("hotkey_ss58", hotkey_ss58)
    position = context.run(
        lambda client: client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    )
    if position is None:
        context.output.error(f"no lending position on netuid {netuid}")
        raise typer.Exit(2)
    side = position["side"]
    # Longs refund alpha; short wallet repay draws alpha — both need --hotkey.
    # Short escrow buyback is TAO-only and may omit it (quote accepts None).
    if side == "Long" and hotkey is None:
        context.output.error("pass --hotkey to receive the long's alpha refund")
        raise typer.Exit(2)
    if side == "Short" and repay_from_wallet and hotkey is None:
        context.output.error(
            "pass --hotkey when using --repay-from-wallet "
            "(omit the flag for the default escrow buyback)"
        )
        raise typer.Exit(2)
    with context.output.activity("quoting the repayment…"):
        quote = context.run(
            lambda client: client.read(
                "lending_close_quote",
                coldkey_ss58=owner,
                netuid=netuid,
                hotkey_ss58=hotkey,
                repay_from_wallet=repay_from_wallet,
            )
        )
    intent = CloseLoan(
        netuid=netuid,
        hotkey_ss58=hotkey,
        repay_from_wallet=repay_from_wallet,
        max_payment=_bound(quote["payment"], max_slippage, ceiling=True),
        min_refund=_bound(quote["refund"], max_slippage),
    )
    context.submit(
        intent,
        card_sections=[
            (
                "Repayment",
                [
                    ("alpha hotkey", hotkey),
                    ("quoted payment", str(quote["payment"])),
                    ("maximum payment", str(intent.max_payment)),
                    ("quoted refund", str(quote["refund"])),
                    ("minimum refund", str(intent.min_refund)),
                ],
            )
        ],
    )


@app.command("raise")
@with_tx_globals
def raise_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet to raise on."),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help=RaiseLoan.field_help("hotkey_ss58")
    ),
    collateral: Optional[str] = typer.Option(
        None, "--collateral", help=RaiseLoan.field_help("collateral_alpha")
    ),
    amount: Optional[str] = typer.Option(None, "--amount", help=RaiseLoan.field_help("borrow_tao")),
    sell: Optional[str] = typer.Option(None, "--sell", help=RaiseLoan.field_help("sell_alpha")),
    min_tao_out: str = typer.Option("0", "--min-tao-out", help=RaiseLoan.field_help("min_tao_out")),
    valid_until: Optional[int] = typer.Option(
        None, "--valid-until", help=RaiseLoan.field_help("valid_until")
    ),
    ltv: Optional[float] = typer.Option(
        None,
        "--ltv",
        min=0.0,
        max=1.0,
        help=(
            "Target loan-to-value as a fraction of collateral value (e.g. 0.20). "
            "Must be at or below the subnet's current max LTV. With --collateral, "
            "sizes borrow to this LTV; with --amount, rejects if implied LTV exceeds it."
        ),
    ),
    max_slippage: float = typer.Option(
        1.0,
        "--max-slippage",
        min=0.0,
        max=99.0,
        help="Quote tolerance (percent) for the raise sale floor when --min-tao-out is 0.",
    ),
):
    """Atomic raise: escrowed long plus optional spot sale of unlocked alpha.

    Pass --collateral (alpha) or --amount (TAO to borrow), not both. Optionally
    sell unlocked alpha with --sell. Fails unless both legs clear.

    Use --ltv to see max borrowable at current LTV/capacity and to request a
    lower LTV than the subnet max (validated client-side before submit).
    """
    context = ctx_of(ctx)
    if amount is not None and collateral is not None:
        context.output.error("--amount and --collateral are mutually exclusive; provide one")
        raise typer.Exit(2)
    if amount is None and collateral is None:
        context.output.error("provide --amount (borrow TAO) or --collateral (alpha)")
        raise typer.Exit(2)
    confirm_wallet(context, help_text="Wallet whose coldkey owns this raise.")
    proxy = context.resolve_dispatch_proxy()
    owner = _loan_owner(context, proxy)
    if owner is None:
        context.output.error("select the account that owns this raise before submitting")
        raise typer.Exit(2)
    hotkey = context.resolve_address("hotkey_ss58", hotkey_ss58)

    async def deadline(client):
        if valid_until is not None:
            return valid_until
        header = await client.substrate.get_block_header()
        number = header.get("header", {}).get("number") if isinstance(header, dict) else None
        if not isinstance(number, int):
            raise BittensorError("current block number is unavailable")
        # Default: allow ~10 minutes of inclusion delay at 12s blocks.
        return number + 50

    with context.output.activity("resolving raise deadline…"):
        until = context.run(deadline)

    # Quote at max LTV first so --ltv can show headroom and validate locally.
    async def max_quote(client):
        view = await client.at()
        return await view.read(
            "lending_raise_quote",
            netuid=netuid,
            coldkey_ss58=owner,
            hotkey_ss58=hotkey,
            collateral_alpha=collateral,
            borrow_tao=amount,
            sell_alpha=sell,
        )

    with context.output.activity("quoting raise at current LTV / capacity…"):
        quote = context.run(max_quote)
    if not isinstance(quote, dict):
        context.output.error("raise quote is unavailable")
        raise typer.Exit(2)

    max_borrow = quote.get("principal_tao")
    max_ltv_bps = quote.get("ltv_tier_bps")
    headroom = quote.get("long_cap_headroom")
    if not isinstance(max_borrow, Balance) or not isinstance(max_ltv_bps, int) or max_ltv_bps <= 0:
        context.output.error("raise quote is missing max borrow or LTV tier")
        raise typer.Exit(2)
    max_ltv = max_ltv_bps / 10_000.0
    context.output.message(
        f"max borrowable at LTV {max_ltv:.2%} / capacity headroom "
        f"{headroom if headroom is not None else '?'}: {max_borrow}"
    )

    borrow_tao = amount
    collateral_alpha = collateral
    if ltv is not None:
        if ltv <= 0:
            context.output.error("--ltv must be greater than 0")
            raise typer.Exit(2)
        if ltv > max_ltv + 1e-12:
            context.output.error(
                f"--ltv {ltv:.4f} exceeds subnet max LTV {max_ltv:.4f} ({max_ltv_bps} bps)"
            )
            raise typer.Exit(2)
        if collateral is not None:
            # Scale max principal by requested_ltv / max_ltv.
            scaled = (max_borrow.rao * round(ltv * 10_000)) // max_ltv_bps
            if scaled <= 0:
                context.output.error("requested --ltv yields zero borrow at this collateral")
                raise typer.Exit(2)
            borrow_tao = str(Balance.from_rao(scaled, 0))
            collateral_alpha = None
            context.output.message(f"sizing borrow at LTV {ltv:.2%}: {borrow_tao}")
        else:
            # --amount path: reject when implied LTV from v_dyn exceeds --ltv.
            v_dyn = quote.get("v_dyn")
            if isinstance(v_dyn, Balance) and v_dyn.rao > 0 and max_borrow.rao > 0:
                implied = max_borrow.rao / v_dyn.rao
                if implied > ltv + 1e-12:
                    context.output.error(
                        f"--amount implies LTV ≈ {implied:.2%}, above --ltv {ltv:.2%} "
                        f"(max LTV {max_ltv:.2%}, max borrow at full LTV depends on collateral)"
                    )
                    raise typer.Exit(2)

    intent = RaiseLoan(
        netuid=netuid,
        hotkey_ss58=hotkey,
        collateral_alpha=collateral_alpha,
        borrow_tao=borrow_tao,
        max_collateral_alpha=collateral or "0",
        min_borrow_tao=borrow_tao or amount or "0",
        sell_alpha=sell,
        min_tao_out=min_tao_out,
        valid_until=until,
    )
    # When --ltv sized from collateral, allow up to the original collateral ceiling.
    if ltv is not None and collateral is not None:
        intent.max_collateral_alpha = alpha_amount(collateral, netuid)
        intent.min_borrow_tao = tao_amount(borrow_tao)

    if sell is not None and cast(Balance, intent.min_tao_out).rao == 0 and max_slippage > 0:

        async def floor(client):
            view = await client.at()
            sale_quote = await view.read(
                "lending_raise_quote",
                netuid=netuid,
                coldkey_ss58=owner,
                hotkey_ss58=hotkey,
                collateral_alpha=intent.collateral_alpha,
                borrow_tao=intent.borrow_tao,
                sell_alpha=intent.sell_alpha,
            )
            out = sale_quote.get("free_tao_out") if isinstance(sale_quote, dict) else None
            if not isinstance(out, Balance) or out.rao <= 0:
                raise BittensorError("raise sale quote is unavailable")
            return _bound(out, max_slippage, ceiling=False)

        try:
            with context.output.activity("quoting raise sale floor…"):
                intent.min_tao_out = context.run(floor)
        except Exception as error:
            raise BittensorError("could not verify requested raise slippage protection") from error

    card = [
        ("netuid", str(netuid)),
        ("owner", owner),
        ("hotkey", hotkey),
        ("collateral", str(intent.collateral_alpha)),
        ("borrow", str(intent.borrow_tao)),
        ("max LTV", f"{max_ltv:.2%} ({max_ltv_bps} bps)"),
        ("max borrow", str(max_borrow)),
    ]
    if ltv is not None:
        card.append(("requested LTV", f"{ltv:.2%}"))
    card.extend(
        [
            ("sell", str(intent.sell_alpha)),
            ("min tao out", str(intent.min_tao_out)),
            ("valid until", str(intent.valid_until)),
        ]
    )
    context.submit(
        intent,
        proxy_for=proxy or "self",
        card_sections=[("Raise", card)],
    )


@app.command("list")
@with_globals
def list_positions(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", min=1, max=65535, help="Subnet whose loans to list."
    ),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
):
    """List open loans on a subnet; optionally select one coldkey."""
    context = ctx_of(ctx)
    owner = (
        context.resolve_address("coldkey_ss58", coldkey_ss58) if coldkey_ss58 is not None else None
    )

    async def _positions(client):
        if owner is None:
            return await client.read("lending_positions", netuid=netuid)
        position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        return [position] if position is not None else []

    positions = context.run(_positions)
    context.output.table(
        f"loans on netuid {netuid}",
        ["owner", "side", "principal", "collateral", "interest due", "runway"],
        [
            [
                position["coldkey"],
                position["side"],
                str(position["principal"]),
                str(position["collateral"]),
                str(position["interest_due"]),
                f"{position['runway_days']:.0f}d" if position["runway_days"] is not None else "-",
            ]
            for position in positions
        ],
        positions,
    )
