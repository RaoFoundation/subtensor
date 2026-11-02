"""CLI for fixed-principal subnet loans: open, close, repay, raise, list, show."""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal, InvalidOperation
from enum import Enum
from typing import Any, Optional, cast

import typer

from ...balance import Balance
from ...intents import CloseLoan, OpenLoan, RaiseLoan, RepayLoan
from ...intents._money import alpha_amount, tao_amount
from ...intents.lending import _available_alpha, _spendable_tao
from ...result import BittensorError
from ...settings import (
    ALPHA_SYMBOL,
    BLOCKTIME,
    DEFAULT_ERA_PERIOD,
    MEV_SHIELD_ERA_PERIOD,
    TAO_SYMBOL,
    query_docs_url,
)
from ..context import address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..prompt import PromptSpec, ask, confirm_wallet, fill_missing

Side = Enum("Side", [("short", "short"), ("long", "long")], type=str)

# Intent default / "no deadline" sentinel (u32::MAX).
VALID_UNTIL_NONE = 2**32 - 1

# Runtime LendingMinimumLoanValue (1_000_000 rao = 0.001 τ).
MINIMUM_LOAN_VALUE_TAO = "0.001"

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "Lift-based escrowed loans: open, close, repay, raise, list, show.\n\n"
        f"Position query: {query_docs_url('lending-position')}"
    ),
)


def _bound(amount: Balance, percent: float, *, ceiling: bool = False) -> Balance:
    """Integer quote protection with ceilings rounded up and floors down."""
    numerator, denominator = Decimal(str(percent)).as_integer_ratio()
    denominator *= 100
    margin = (amount.rao * numerator + denominator - 1) // denominator
    rao = amount.rao + margin if ceiling else amount.rao - margin
    return Balance.from_rao(max(rao, 0), amount.netuid)


def _require_slippage(max_slippage: float) -> float:
    """Reject zero slippage: quotes go stale one block later via interest/emissions."""
    if max_slippage == 0:
        raise BittensorError(
            "--max-slippage cannot be 0: interest and emissions move the price one block "
            "after the quote; leave the default 1% (or pass a positive tolerance)"
        )
    return max_slippage


def _raise_sale_needs_quoted_floor(min_tao_out: Balance) -> bool:
    """True when the raise sale still needs a quoted min_tao_out floor.

    The runtime always sells on raise: an omitted --sell defaults to the
    newly escrowed alpha. A zero min_tao_out therefore means the user has
    not supplied protection and the CLI must quote one (Skeptic fid:abef2402).
    """
    return cast(Balance, min_tao_out).rao == 0


# Default inclusion window when --valid-until is omitted (~10 minutes at
# 12s blocks). Only raise resolves a head-relative deadline in the CLI;
# open / close leave the intent default (2**32 - 1 / "none").
DEFAULT_RAISE_VALIDITY_BLOCKS = 50


async def _resolve_valid_until(client, valid_until: Optional[int]) -> int:
    """Return an explicit deadline, or head + :data:`DEFAULT_RAISE_VALIDITY_BLOCKS`.

    Uses :meth:`Client.block` — never client.substrate (the public Client
    has no such attribute).
    """
    if valid_until is not None:
        return valid_until
    return (await client.block()) + DEFAULT_RAISE_VALIDITY_BLOCKS


def _format_valid_until(block: int, *, head: Optional[int] = None) -> str:
    """Human deadline: 'none' for the u32::MAX sentinel, else block + approx UTC."""
    if int(block) >= VALID_UNTIL_NONE:
        return "none"
    text = f"block {block}"
    if head is not None:
        when = datetime.now(timezone.utc) + timedelta(seconds=(block - head) * BLOCKTIME)
        text += f" (~{when.strftime('%Y-%m-%d %H:%M')} UTC)"
    return text


def _format_expiry(block: Optional[int], *, head: int, head_time: Optional[datetime] = None) -> str:
    """Expiry as block number plus approximate UTC date."""
    if block is None:
        return "-"
    base = head_time or datetime.now(timezone.utc)
    when = base + timedelta(seconds=(int(block) - head) * BLOCKTIME)
    return f"{block} (~{when.strftime('%Y-%m-%d')} UTC)"


def _alpha_label(netuid: int) -> str:
    """Prefer α_N over subnet-specific glyphs in lending cards."""
    return f"{ALPHA_SYMBOL}_{netuid}" if netuid else TAO_SYMBOL


def _fmt_amount(amount: Any) -> str:
    """Display Balance with τ / α_N (not subnet token glyphs)."""
    if not isinstance(amount, Balance):
        return str(amount)
    if amount.netuid == 0:
        return f"{TAO_SYMBOL}{amount.amount:,.9f}"
    return f"{amount.amount:,.9f}{_alpha_label(amount.netuid)}"


def _parse_positive_amount(
    value: str, *, tao: bool, netuid: int = 0, label: str = "amount"
) -> Balance:
    """Parse a CLI money string; reject non-numeric / non-positive without traceback."""
    try:
        balance = cast(Balance, tao_amount(value) if tao else alpha_amount(value, netuid))
    except (BittensorError, InvalidOperation, ValueError, TypeError, OverflowError) as error:
        raise BittensorError(f"{label} must be a positive number, got {value!r}") from error
    if balance.rao <= 0:
        raise BittensorError(f"{label} must be greater than zero")
    return balance


def _exit_error(context, message: str, code: int = 2) -> None:
    context.output.error(message)
    raise typer.Exit(code)


def _position_json(position: dict) -> dict:
    """JSON shape: Balance fields as rao ints (not formatted strings)."""
    out: dict[str, Any] = {}
    for key, value in position.items():
        if isinstance(value, Balance):
            out[key] = value.rao
        elif isinstance(value, float) and key == "runway_days":
            out[key] = None if value is None else round(value, 6)
        else:
            out[key] = value
    return out


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


async def _ensure_open_funds(client, *, netuid: int, side: str, owner: str, hotkey, collateral: Balance):
    """Refuse before quote when free TAO / unlocked alpha cannot cover collateral."""
    substrate = client._substrate
    if side == "Short":
        available = await _spendable_tao(substrate, owner)
        if collateral.rao > available.rao:
            raise BittensorError(
                f"insufficient free TAO for collateral: need {_fmt_amount(collateral)}, "
                f"available {_fmt_amount(available)}"
            )
        return
    if hotkey is None:
        raise BittensorError("long open requires --hotkey before quoting")
    available = await _available_alpha(substrate, owner, hotkey, netuid)
    if collateral.rao > available.rao:
        raise BittensorError(
            f"insufficient unlocked alpha on {hotkey}: need {_fmt_amount(collateral)}, "
            f"available {_fmt_amount(available)}"
        )


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
            "Quote tolerance (percent, default 1%). Amount mode adds collateral headroom and "
            "the existing coupon's submission window. Collateral mode sets a principal floor. "
            "Shorts also set an opening value floor; longs set a min escrow-alpha floor. "
            "Must be > 0 (quotes go stale one block later)."
        ),
    ),
):
    """Add collateral and open or increase a short or long on a subnet.

    Pass --amount for the additional quantity to borrow (alpha for a short,
    TAO for a long) and btcli finds the minimum required collateral. Pass
    --collateral instead to borrow its quoted limit. The minimum opening
    value is about 0.001 TAO (runtime MinimumLoanValue).

    Amount mode adds collateral headroom via --max-slippage (default 1%)
    plus the existing coupon's submission window; borrowed principal stays
    exact.

    Shorts sell borrowed alpha into escrowed TAO (no --hotkey). Longs buy
    escrowed alpha with borrowed TAO and require --hotkey as the collateral
    source. No free borrowed assets are delivered. Increasing an existing
    position restarts its one-year term.
    """
    context = ctx_of(ctx)
    try:
        _require_slippage(max_slippage)
    except BittensorError as error:
        _exit_error(context, str(error))
    if amount is not None and collateral is not None:
        _exit_error(context, "--amount and --collateral are mutually exclusive; provide one")
    if amount is None and collateral is None:
        amount, collateral = _loan_input(context, netuid, side.value)
    confirm_wallet(context, help_text="Wallet whose coldkey owns this loan.")
    proxy = context.resolve_dispatch_proxy()
    owner = _loan_owner(context, proxy)
    if owner is None:
        _exit_error(context, "select the account that owns this loan before borrowing")
    # Shorts omit hotkey; longs require an explicit collateral source (do not
    # fall back to the wallet hotkey — that is often an unregistered key).
    hotkey = (
        None if side == Side.short else context.resolve_address("lending_hotkey_ss58", hotkey_ss58)
    )
    if side == Side.short and hotkey_ss58 is not None:
        _exit_error(context, "short open is TAO-only; omit --hotkey")
    if side == Side.long and hotkey is None:
        _exit_error(
            context,
            "--hotkey is required for long collateral source "
            "(pass --hotkey <validator you staked to>)",
        )
    requested = None
    minimum_collateral = None
    try:
        if amount is not None:
            requested = _parse_positive_amount(
                amount,
                tao=side == Side.long,
                netuid=netuid,
                label="amount to borrow",
            )
        elif collateral is not None:
            collateral = _parse_positive_amount(
                collateral,
                tao=side == Side.short,
                netuid=netuid,
                label="collateral",
            )
    except BittensorError as error:
        _exit_error(context, str(error))

    existing = context.run(
        lambda client: client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    )
    if existing is not None:
        context.output.message(
            "warning: increasing this position restarts the 1-year fixed term "
            "(expires_at moves to now + BlocksPerYear)"
        )

    if requested is not None:

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
            try:
                minimum_collateral, collateral = context.run(size)
            except BittensorError as error:
                _exit_error(context, str(error), code=1)
    if collateral is None:
        _exit_error(context, "provide --amount or --collateral")
    collateral_bal = (
        collateral
        if isinstance(collateral, Balance)
        else _parse_positive_amount(
            str(collateral),
            tao=side == Side.short,
            netuid=netuid,
            label="collateral",
        )
    )

    # Check free balance / unlocked stake before the open quote so a huge
    # collateral hits InsufficientFunds locally instead of PriceTooFarFromEma.
    side_name = "Short" if side == Side.short else "Long"
    try:
        with context.output.activity("checking available collateral…"):
            context.run(
                lambda client: _ensure_open_funds(
                    client,
                    netuid=netuid,
                    side=side_name,
                    owner=owner,
                    hotkey=hotkey,
                    collateral=collateral_bal,
                )
            )
    except BittensorError as error:
        _exit_error(context, str(error))

    intent = OpenLoan(
        netuid=netuid,
        side=side.value,
        collateral=collateral_bal,
        hotkey_ss58=hotkey,
        amount=requested,
    )
    with context.output.activity("quoting the loan…"):
        try:
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
        except BittensorError as error:
            _exit_error(context, str(error), code=1)
    # Amount mode never turns the desired principal into a floor that permits
    # borrowing more. The exact-principal dispatch transfers that quantity.
    intent.min_borrow = (
        requested if requested is not None else _bound(quote["principal"], max_slippage)
    )
    if side == Side.short:
        intent.min_proceeds = _bound(quote["opening_value"], max_slippage)
    else:
        escrow = quote.get("escrow_estimate")
        if isinstance(escrow, Balance) and escrow.rao > 0:
            intent.min_escrow_alpha = _bound(escrow, max_slippage)

    # Warn when capacity (not LTV) is what limits borrow vs collateral value.
    v_dyn = quote.get("v_dyn")
    principal = quote.get("principal")
    if (
        isinstance(v_dyn, Balance)
        and isinstance(principal, Balance)
        and v_dyn.rao > 0
        and principal.rao > 0
        and principal.rao * 4 < v_dyn.rao  # well below ~25% LTV of V_dyn
    ):
        context.output.message(
            f"warning: capacity-limited — borrow {_fmt_amount(principal)} is well below "
            f"~25% of collateral value {_fmt_amount(v_dyn)}; check lending_reserves headroom"
        )

    async def _term_meta(client):
        head = await client.block()
        year = await client.constant(("Lending", "BlocksPerYear"))
        return head, int(year) if isinstance(year, int) and year > 0 else None

    head, year = context.run(_term_meta)
    new_expires = (head + year) if year else None

    loan_rows = [
        ("loan owner", owner),
        *([("collateral hotkey", hotkey)] if hotkey is not None else []),
        *(
            [("calculated minimum collateral", _fmt_amount(minimum_collateral))]
            if minimum_collateral is not None
            else []
        ),
        ("provided collateral", _fmt_amount(intent.collateral)),
        (
            "exact amount to borrow" if requested is not None else "borrowing limit",
            _fmt_amount(quote["principal"]),
        ),
        ("minimum additional principal", _fmt_amount(intent.min_borrow)),
        ("additional interest per year", _fmt_amount(quote["annual_interest"])),
        ("quoted opening value", _fmt_amount(quote["opening_value"])),
    ]
    if side == Side.short:
        loan_rows.append(("minimum additional opening value", _fmt_amount(intent.min_proceeds)))
    else:
        # Longs: drop useless min_proceeds=0; show min escrow alpha instead.
        escrow_floor = intent.min_escrow_alpha
        if isinstance(escrow_floor, Balance) and escrow_floor.rao > 0:
            loan_rows.append(("min escrow alpha", _fmt_amount(escrow_floor)))
        elif isinstance(quote.get("escrow_estimate"), Balance):
            loan_rows.append(("quoted escrow alpha", _fmt_amount(quote["escrow_estimate"])))
    if new_expires is not None:
        loan_rows.append(
            (
                "expires at (after open/increase)",
                _format_expiry(new_expires, head=head),
            )
        )
    loan_rows.append(("valid until", _format_valid_until(intent.valid_until, head=head)))
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
        help=(
            "Maximum percentage above quoted payment or below quoted refund (default 1%). "
            "Must be > 0 (quotes go stale one block later)."
        ),
    ),
):
    """Repay fixed principal and return remaining collateral.

    Positions store no hotkey. By default closes take the TAO-refund path
    (escrow AMM buyback for shorts; sell escrow for longs) — matching the
    SDK CloseLoan default. Pass --repay-from-wallet to repay short alpha from
    --hotkey stake or pay long TAO from free balance. Pass --hotkey for alpha
    repay (short wallet path) or alpha refund (long close / short buyback
    surplus). The full runtime quote sets a payment ceiling and refund floor;
    a failed quote stops submission.
    """
    context = ctx_of(ctx)
    try:
        _require_slippage(max_slippage)
    except BittensorError as error:
        _exit_error(context, str(error))
    owner = context.review_account()
    if owner is None:
        _exit_error(context, "select the account that owns this position before closing")
    # Keep omitted --hotkey as None (do not default to the wallet hotkey).
    hotkey = context.resolve_address("lending_hotkey_ss58", hotkey_ss58)
    position = context.run(
        lambda client: client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    )
    if position is None:
        _exit_error(context, f"no lending position on netuid {netuid}")
    side = position["side"]
    # Longs refund alpha; short wallet repay draws alpha — both need --hotkey.
    # Short escrow buyback is TAO-only and may omit it (runtime folds dust).
    if side == "Long" and hotkey is None:
        _exit_error(
            context,
            "pass --hotkey <validator you staked to> to receive the long's alpha refund",
        )
    if side == "Short" and repay_from_wallet and hotkey is None:
        _exit_error(
            context,
            "pass --hotkey <validator you staked to> when using --repay-from-wallet "
            "(omit the flag for the default escrow buyback)",
        )
    with context.output.activity("quoting the repayment…"):
        try:
            quote = context.run(
                lambda client: client.read(
                    "lending_close_quote",
                    coldkey_ss58=owner,
                    netuid=netuid,
                    hotkey_ss58=hotkey,
                    repay_from_wallet=repay_from_wallet,
                )
            )
        except BittensorError as error:
            _exit_error(context, str(error), code=1)
    intent = CloseLoan(
        netuid=netuid,
        hotkey_ss58=hotkey,
        repay_from_wallet=repay_from_wallet,
        max_payment=_bound(quote["payment"], max_slippage, ceiling=True),
        min_refund=_bound(quote["refund"], max_slippage),
    )
    shortfall = quote.get("shortfall")
    head = context.run(lambda client: client.block())
    repay_rows = [
        ("side", side),
        ("alpha hotkey", hotkey or "—"),
        ("quoted payment", _fmt_amount(quote["payment"])),
        ("maximum payment", _fmt_amount(intent.max_payment)),
        ("quoted refund", _fmt_amount(quote["refund"])),
        ("minimum refund", _fmt_amount(intent.min_refund)),
    ]
    if isinstance(shortfall, Balance) and shortfall.rao > 0:
        repay_rows.append(("shortfall (top-up)", _fmt_amount(shortfall)))
    repay_rows.append(("valid until", _format_valid_until(intent.valid_until, head=head)))
    context.submit(
        intent,
        card_sections=[("Repayment", repay_rows)],
    )


@app.command("repay")
@with_tx_globals
def repay_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your position."),
    principal: Optional[str] = typer.Option(
        None,
        "--principal",
        "--amount",
        help=RepayLoan.field_help("principal_part"),
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help=RepayLoan.field_help("hotkey_ss58"),
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
        help=(
            "Maximum percentage above quoted payment or below quoted refund (default 1%). "
            "Must be > 0 (quotes go stale one block later)."
        ),
    ),
):
    """Partially repay principal and return a pro-rata collateral slice.

    Pass --principal / --amount (alpha for a short, TAO for a long). Full
    principal equals btcli lending close. Hotkey and wallet/escrow flags match
    close. The runtime quote sets payment ceiling and refund floor for the
    slice. Partial repay does not restart the one-year term.
    """
    context = ctx_of(ctx)
    try:
        _require_slippage(max_slippage)
    except BittensorError as error:
        _exit_error(context, str(error))
    if principal is None:
        _exit_error(context, "provide --principal / --amount of debt to repay")
    owner = context.review_account()
    if owner is None:
        _exit_error(context, "select the account that owns this position before repaying")
    hotkey = context.resolve_address("lending_hotkey_ss58", hotkey_ss58)
    position = context.run(
        lambda client: client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    )
    if position is None:
        _exit_error(context, f"no lending position on netuid {netuid}")
    side = position["side"]
    if side == "Long" and hotkey is None:
        _exit_error(
            context,
            "pass --hotkey <validator you staked to> to receive the long's alpha refund",
        )
    if side == "Short" and repay_from_wallet and hotkey is None:
        _exit_error(
            context,
            "pass --hotkey <validator you staked to> when using --repay-from-wallet "
            "(omit the flag for the default escrow buyback)",
        )
    try:
        part = _parse_positive_amount(
            principal,
            tao=side != "Short",
            netuid=netuid,
            label="--principal / --amount",
        )
    except BittensorError as error:
        _exit_error(context, str(error))
    stored = position["principal"]
    if not isinstance(stored, Balance) or part.rao > stored.rao:
        _exit_error(context, f"--principal {_fmt_amount(part)} exceeds stored principal {_fmt_amount(stored)}")
    with context.output.activity("quoting the partial repayment…"):
        try:
            quote = context.run(
                lambda client: client.read(
                    "lending_repay_quote",
                    coldkey_ss58=owner,
                    netuid=netuid,
                    principal_part=principal,
                    hotkey_ss58=hotkey,
                    repay_from_wallet=repay_from_wallet,
                )
            )
        except BittensorError as error:
            _exit_error(context, str(error), code=1)
    intent = RepayLoan(
        netuid=netuid,
        principal_part=part,
        hotkey_ss58=hotkey,
        repay_from_wallet=repay_from_wallet,
        max_payment=_bound(quote["payment"], max_slippage, ceiling=True),
        min_refund=_bound(quote["refund"], max_slippage),
    )
    shortfall = quote.get("shortfall")
    head = context.run(lambda client: client.block())
    repay_rows = [
        ("principal part", _fmt_amount(part)),
        ("alpha hotkey", hotkey or "—"),
        ("quoted payment", _fmt_amount(quote["payment"])),
        ("maximum payment", _fmt_amount(intent.max_payment)),
        ("quoted refund", _fmt_amount(quote["refund"])),
        ("minimum refund", _fmt_amount(intent.min_refund)),
    ]
    if isinstance(shortfall, Balance) and shortfall.rao > 0:
        repay_rows.append(("shortfall (top-up)", _fmt_amount(shortfall)))
    expires = position.get("expires_at")
    if expires is not None:
        repay_rows.append(("expires at (unchanged)", _format_expiry(expires, head=head)))
    repay_rows.append(("valid until", _format_valid_until(intent.valid_until, head=head)))
    context.submit(
        intent,
        card_sections=[("Repayment", repay_rows)],
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
    max_collateral: Optional[str] = typer.Option(
        None,
        "--max-collateral",
        help=(
            "Amount-mode ceiling on alpha collateral the runtime may lock "
            "(must be ≥ quoted minimum). Default: quoted collateral + --max-slippage."
        ),
    ),
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
        help=(
            "Quote tolerance (percent, default 1%) for max collateral, min borrow, "
            "min escrow alpha, and the raise sale floor when --min-tao-out is 0. "
            "Must be > 0 (quotes go stale one block later)."
        ),
    ),
):
    """Atomic raise: escrowed long plus spot sale of unlocked alpha.

    Pass --collateral (alpha) or --amount (TAO to borrow), not both. Omitting
    --sell sells the newly escrowed alpha (runtime default); pass an explicit
    --sell amount to sell a different unlocked quantity. Fails unless both
    legs clear.

    Use --ltv to see max borrowable at current LTV/capacity and to request a
    lower LTV than the subnet max (validated client-side before submit).
    """
    context = ctx_of(ctx)
    try:
        _require_slippage(max_slippage)
    except BittensorError as error:
        _exit_error(context, str(error))
    if amount is not None and collateral is not None:
        _exit_error(context, "--amount and --collateral are mutually exclusive; provide one")
    if amount is None and collateral is None:
        _exit_error(context, "provide --amount (borrow TAO) or --collateral (alpha)")
    try:
        if amount is not None:
            _parse_positive_amount(amount, tao=True, label="--amount")
        if collateral is not None:
            _parse_positive_amount(collateral, tao=False, netuid=netuid, label="--collateral")
        if sell is not None:
            _parse_positive_amount(sell, tao=False, netuid=netuid, label="--sell")
    except BittensorError as error:
        _exit_error(context, str(error))
    confirm_wallet(context, help_text="Wallet whose coldkey owns this raise.")
    proxy = context.resolve_dispatch_proxy()
    owner = _loan_owner(context, proxy)
    if owner is None:
        _exit_error(context, "select the account that owns this raise before submitting")
    # Raise always moves alpha; require an explicit --hotkey (no wallet default).
    hotkey = context.resolve_address("lending_hotkey_ss58", hotkey_ss58)
    if hotkey is None:
        _exit_error(
            context,
            "pass --hotkey <validator you staked to> for raise collateral and sale alpha",
        )

    existing = context.run(
        lambda client: client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    )
    if existing is not None:
        context.output.message(
            "warning: increasing this position restarts the 1-year fixed term"
        )

    with context.output.activity("resolving raise deadline…"):
        until = context.run(lambda client: _resolve_valid_until(client, valid_until))

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
    ltv_sized_from_collateral = False
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
            ltv_sized_from_collateral = True
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

    # After LTV sizing switches to borrow_tao, re-quote so collateral / escrow
    # floors match the sized principal rather than the full-LTV quote.
    if ltv_sized_from_collateral:

        async def sized_quote(client):
            view = await client.at()
            return await view.read(
                "lending_raise_quote",
                netuid=netuid,
                coldkey_ss58=owner,
                hotkey_ss58=hotkey,
                collateral_alpha=None,
                borrow_tao=borrow_tao,
                sell_alpha=sell,
            )

        with context.output.activity("quoting raise at sized borrow…"):
            quote = context.run(sized_quote)
        if not isinstance(quote, dict):
            context.output.error("raise quote is unavailable for sized borrow")
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

    quoted_collateral = quote.get("collateral_alpha")
    quoted_escrow = quote.get("escrow_alpha")
    quoted_principal = quote.get("principal_tao")
    if not isinstance(quoted_collateral, Balance) or not isinstance(quoted_escrow, Balance):
        context.output.error("raise quote is missing collateral or escrow alpha")
        raise typer.Exit(2)

    # Amount / borrow_tao mode: ceiling on solved collateral (≥ quoted min).
    if intent.collateral_alpha is None:
        if max_collateral is not None:
            ceiling = cast(Balance, alpha_amount(max_collateral, netuid))
            if ceiling.rao < quoted_collateral.rao:
                context.output.error(
                    f"--max-collateral {ceiling} is below the quoted minimum {quoted_collateral}"
                )
                raise typer.Exit(2)
            intent.max_collateral_alpha = ceiling
        else:
            intent.max_collateral_alpha = _bound(quoted_collateral, max_slippage, ceiling=True)
        # Exact (or LTV-sized) principal is both the borrow and its floor.
        if borrow_tao is not None:
            intent.min_borrow_tao = tao_amount(borrow_tao)
    else:
        # Collateral mode: floor on solved principal from the quote.
        if isinstance(quoted_principal, Balance):
            intent.min_borrow_tao = _bound(quoted_principal, max_slippage)
        if max_collateral is not None:
            context.output.error("--max-collateral applies only with --amount (borrow mode)")
            raise typer.Exit(2)

    intent.min_escrow_alpha = _bound(quoted_escrow, max_slippage)

    # Runtime defaults an omitted --sell to the newly escrowed alpha
    # (`sell_alpha.unwrap_or(realised_a)`), so the default raise still executes
    # a sale. Quote a floor whenever --min-tao-out is unset (Skeptic
    # fid:abef2402).
    if _raise_sale_needs_quoted_floor(cast(Balance, intent.min_tao_out)):

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

    quoted_sell = quote.get("sell_alpha")
    sell_display = (
        quoted_sell
        if isinstance(quoted_sell, Balance)
        else intent.sell_alpha
        if intent.sell_alpha is not None
        else quoted_escrow
    )
    head = context.run(lambda client: client.block())
    year = context.run(lambda client: client.constant(("Lending", "BlocksPerYear")))
    new_expires = (head + int(year)) if isinstance(year, int) and year > 0 else None
    card = [
        ("netuid", str(netuid)),
        ("owner", owner),
        ("hotkey", hotkey),
        ("collateral", _fmt_amount(quoted_collateral)),
        ("borrow", _fmt_amount(quoted_principal if isinstance(quoted_principal, Balance) else intent.borrow_tao)),
        ("max collateral", _fmt_amount(intent.max_collateral_alpha)),
        ("min borrow", _fmt_amount(intent.min_borrow_tao)),
        ("min escrow alpha", _fmt_amount(intent.min_escrow_alpha)),
        ("quoted free TAO out", _fmt_amount(quote.get("free_tao_out"))),
        ("max LTV", f"{max_ltv:.2%} ({max_ltv_bps} bps)"),
        ("max borrow", _fmt_amount(max_borrow)),
    ]
    if ltv is not None:
        card.append(("requested LTV", f"{ltv:.2%}"))
    card.extend(
        [
            ("sell", _fmt_amount(sell_display)),
            ("min tao out", _fmt_amount(intent.min_tao_out)),
            ("valid until", _format_valid_until(intent.valid_until, head=head)),
        ]
    )
    if new_expires is not None:
        card.append(("expires at (after raise)", _format_expiry(new_expires, head=head)))
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
        None,
        address_cli_name("coldkey_ss58"),
        help=ss58_param_help("coldkey_ss58"),
    ),
    list_all: bool = typer.Option(
        False,
        "--all",
        help="List every open loan on the subnet (ignore wallet coldkey default).",
    ),
):
    """List open loans for the wallet coldkey (default) or the whole subnet."""
    context = ctx_of(ctx)
    if list_all and coldkey_ss58 is not None:
        _exit_error(context, "--all and --coldkey are mutually exclusive")
    # Help already says coldkey defaults to the wallet; resolve that unless --all.
    owner = None if list_all else context.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _positions(client):
        head = await client.block()
        try:
            head_time = await client.timestamp(block=head)
        except Exception:
            head_time = datetime.now(timezone.utc)
        if owner is None:
            positions = await client.read("lending_positions", netuid=netuid)
        else:
            position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
            positions = [position] if position is not None else []
        return positions, head, head_time

    positions, head, head_time = context.run(_positions)
    columns = [
        "owner",
        "side",
        "principal",
        "collateral",
        "status",
        "escrow",
        "expires",
        "due",
        "runway",
    ]
    rows = []
    for position in positions:
        runway = position.get("runway_days")
        rows.append(
            [
                position["coldkey"],
                position["side"],
                _fmt_amount(position["principal"]),
                _fmt_amount(position["collateral"]),
                "underwater" if position.get("underwater") else "ok",
                _fmt_amount(position.get("escrow") or position.get("proceeds")),
                _format_expiry(position.get("expires_at"), head=head, head_time=head_time),
                str(position.get("due") or "-"),
                f"{runway:.0f}d" if runway is not None else "-",
            ]
        )
    context.output.table(
        f"loans on netuid {netuid}",
        columns,
        rows,
        [_position_json(position) for position in positions],
        truncate=False,
    )


@app.command("show")
@app.command("quote")
@with_globals
def show_position(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of the position."),
    coldkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("coldkey_ss58"), help=ss58_param_help("coldkey_ss58")
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None, address_cli_name("hotkey_ss58"), help="Optional hotkey for the close quote."
    ),
    repay_from_wallet: bool = typer.Option(
        False,
        "--repay-from-wallet/--no-repay-from-wallet",
        help="Quote the wallet-repay close path instead of the default escrow path.",
    ),
):
    """Show one position and its close quote (cheap read; no transaction)."""
    context = ctx_of(ctx)
    owner = context.resolve_address("coldkey_ss58", coldkey_ss58)
    if owner is None:
        _exit_error(context, "select the account that owns this position")
    hotkey = context.resolve_address("lending_hotkey_ss58", hotkey_ss58)

    async def _load(client):
        position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        if position is None:
            return None, None, await client.block(), None
        head = await client.block()
        try:
            head_time = await client.timestamp(block=head)
        except Exception:
            head_time = datetime.now(timezone.utc)
        quote = await client.read(
            "lending_close_quote",
            coldkey_ss58=owner,
            netuid=netuid,
            hotkey_ss58=hotkey,
            repay_from_wallet=repay_from_wallet,
        )
        return position, quote, head, head_time

    position, quote, head, head_time = context.run(_load)
    if position is None:
        _exit_error(context, f"no lending position on netuid {netuid}", code=1)
    fields = {
        "owner": owner,
        "netuid": netuid,
        "side": position["side"],
        "principal": _fmt_amount(position["principal"]),
        "collateral": _fmt_amount(position["collateral"]),
        "escrow": _fmt_amount(position.get("escrow") or position.get("proceeds")),
        "interest due": _fmt_amount(position["interest_due"]),
        "status": "underwater" if position.get("underwater") else "ok",
        "expires": _format_expiry(position.get("expires_at"), head=head, head_time=head_time),
        "due": position.get("due"),
        "runway": (
            f"{position['runway_days']:.0f}d" if position.get("runway_days") is not None else "-"
        ),
        "quoted payment": _fmt_amount(quote["payment"]),
        "quoted refund": _fmt_amount(quote["refund"]),
        "shortfall": _fmt_amount(quote.get("shortfall")),
    }
    json_fields = {
        **_position_json(position),
        "close_quote": {
            "payment": quote["payment"].rao if isinstance(quote["payment"], Balance) else quote["payment"],
            "refund": quote["refund"].rao if isinstance(quote["refund"], Balance) else quote["refund"],
            "shortfall": (
                quote["shortfall"].rao
                if isinstance(quote.get("shortfall"), Balance)
                else quote.get("shortfall")
            ),
            "repay_from_wallet": repay_from_wallet,
            "hotkey_ss58": hotkey,
        },
    }
    context.output.detail(
        f"lending position on netuid {netuid}",
        fields,
        json_fields=json_fields,
        docs=[query_docs_url("lending-position"), query_docs_url("lending-close-quote")],
    )
