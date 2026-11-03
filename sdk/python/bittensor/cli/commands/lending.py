"""CLI for fixed-principal subnet loans: open, close, repay, raise, list, show."""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from decimal import Decimal, InvalidOperation
from enum import Enum
from typing import Any, Optional, cast

import typer

from ..._generated import runtime_apis as api
from ..._generated.storage import Item as StorageItem
from ...balance import Balance
from ...executor import _find_event
from ...intents import CloseLoan, OpenLoan, RaiseLoan, RepayLoan
from ...intents._money import alpha_amount, tao_amount
from ...intents.lending import _available_alpha, _spendable_tao
from ...result import BittensorError
from ...settings import (
    ALPHA_SYMBOL,
    BLOCKTIME,
    DEFAULT_ERA_PERIOD,
    MEV_SHIELD_ERA_PERIOD,
    RAO_PER_TAO,
    TAO_SYMBOL,
    query_docs_url,
)
from ..context import address_cli_name, ctx_of, ss58_param_help
from ..globals import with_globals, with_tx_globals
from ..prompt import PromptSpec, ask, confirm_wallet, fill_missing

_REF = StorageItem("Lending", "References", "LendingReference")
_LIFT_PARAMS = StorageItem("Lending", "LiftParamsStore", "LiftParams")
_BAND_SINCE = StorageItem("Lending", "BandDeferralSince", "u32")

Side = Enum("Side", [("short", "short"), ("long", "long")], type=str)

# Intent default / "no deadline" sentinel (u32::MAX).
VALID_UNTIL_NONE = 2**32 - 1

# Runtime LendingMinimumLoanValue (1_000_000 rao = 0.001 τ).
MINIMUM_LOAN_VALUE_TAO = "0.001"

app = typer.Typer(
    no_args_is_help=True,
    # Wide terminals: don't ellipsize long option names / help (E2E F15).
    context_settings={"max_content_width": 200, "help_option_names": ["-h", "--help"]},
    help=(
        "Lift-based escrowed loans: open, close, repay, raise, list, show.\n\n"
        "Queries: lending_position / lending_close_quote / lending_stats (SDK reads).\n\n"
        "Docs `see:` links default to RaoFoundation/subtensor@main; set "
        "BITTENSOR_DOCS_REF=<git-ref> (or BITTENSOR_DOCS_URL) for a PR tip / "
        "clone before merge (N12)."
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
    has no such attribute). Refuses a past deadline before submit (E2E N7).
    """
    head = await client.block()
    if valid_until is not None:
        if int(valid_until) < int(head):
            raise BittensorError(
                f"--valid-until {valid_until} is already past (head block {head}); "
                "omit it for head+50, or pass a future block"
            )
        return int(valid_until)
    return int(head) + DEFAULT_RAISE_VALIDITY_BLOCKS


def _format_valid_until(block: int, *, head: Optional[int] = None) -> str:
    """Human deadline: 'no deadline' for the u32::MAX sentinel (N15), else block + UTC."""
    if int(block) >= VALID_UNTIL_NONE:
        return "no deadline"
    text = f"block {block}"
    if head is not None:
        when = datetime.now(timezone.utc) + timedelta(seconds=(block - head) * BLOCKTIME)
        text += f" (~{when.strftime('%Y-%m-%d %H:%M')} UTC)"
    return text


def _result_amounts(result, *, display: dict[str, str], raw: dict[str, int]) -> None:
    """Attach Result card strings plus rao ints for --json (F9 / N15 / N19).

    Human Result uses ``display`` keys only; ``*_rao`` companions stay in
    ``result.data`` for ``--json`` / SDK consumers, and text rendering skips
    them (see :meth:`Output.result`).
    """
    for key, text in display.items():
        result.data[key] = text
    for key, rao in raw.items():
        result.data[f"{key}_rao"] = int(rao)


def _format_expiry(block: Optional[int], *, head: int, head_time: Optional[datetime] = None) -> str:
    """Expiry as block number plus approximate UTC date."""
    if block is None:
        return "-"
    base = head_time or datetime.now(timezone.utc)
    when = base + timedelta(seconds=(int(block) - head) * BLOCKTIME)
    return f"{block} (~{when.strftime('%Y-%m-%d')} UTC)"


def _terms_version_label(value: Any) -> str:
    """Strict terms_version display — never hide a missing/0 behind `or 1` (N26)."""
    if value is None:
        return "-"
    try:
        return str(int(value))
    except (TypeError, ValueError):
        return str(value)


def _past_expiry(position: dict, head: int) -> bool:
    expires = position.get("expires_at")
    return expires is not None and int(expires) <= int(head)


def _format_expiry_field(position: dict, *, head: int, head_time: Optional[datetime] = None) -> str:
    """Expiry line for list/show; past-due deferred loans say awaiting settlement (U2)."""
    expires = position.get("expires_at")
    if expires is None:
        return "-"
    if _past_expiry(position, head):
        return f"past expiry (block {int(expires)}); awaiting settlement"
    return _format_expiry(expires, head=head, head_time=head_time)


def _format_runway_field(position: dict, *, head: int) -> str:
    """Runway for list/show; never print ``0d`` once the term has ended (U2)."""
    if _past_expiry(position, head):
        return "past expiry"
    runway = position.get("runway_days")
    if runway is None:
        return "-"
    return f"{runway:.0f}d"


def _explain_band_deferral(
    reason: str,
    *,
    spot_vs_ema_pct: Optional[float],
    band_bps: Optional[int],
    retry_at: Any,
    force_at: Optional[int],
) -> str:
    """Turn ``SettlementPriceOutsideEmaBand`` into a readable deferral line (U2)."""
    if "SettlementPriceOutsideEmaBand" not in reason:
        return reason
    band_pct = (int(band_bps) / 100.0) if band_bps is not None else 15.0
    if spot_vs_ema_pct is None:
        head = f"spot is outside the EMA band (±{band_pct:g}%)"
    else:
        head = f"spot is {spot_vs_ema_pct:+.1f}% from EMA (band ±{band_pct:g}%)"
    parts = [head, f"retries at block {retry_at}"]
    if force_at is not None:
        parts.append(f"forced settle at block {force_at}")
    return "; ".join(parts)


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
        if key == "opened_at":
            # Storage has no opened_at; omit the always-null field (E2E F8).
            continue
        if isinstance(value, Balance):
            out[key] = value.rao
        elif isinstance(value, float) and key == "runway_days":
            out[key] = None if value is None else round(value, 6)
        else:
            out[key] = value
    return out


def _position_status_label(position: dict) -> str:
    """Human status: deferred / underwater when interest-exhausted or AMM shortfall."""
    deferrals = position.get("deferrals")
    if isinstance(deferrals, list) and deferrals:
        kinds = [str(d.get("kind")) for d in deferrals if isinstance(d, dict) and d.get("kind")]
        if kinds:
            return "deferred:" + "+".join(kinds)
    deferred = position.get("deferred")
    if isinstance(deferred, dict) and deferred.get("kind"):
        return f"deferred:{deferred['kind']}"
    if position.get("underwater"):
        return "underwater"
    shortfall = position.get("close_shortfall")
    if isinstance(shortfall, Balance) and shortfall.rao > 0:
        return "underwater"
    if isinstance(shortfall, int) and shortfall > 0:
        return "underwater"
    return "ok"


async def _provided_collateral(client, minimum: Balance, netuid: int, owner: str, percent: float):
    """Add quote tolerance and accrued interest reserve for the submission window."""
    maximum = (1 << 64) - 1
    if not 0 < minimum.rao <= maximum:
        raise BittensorError("calculated collateral does not fit the chain's deposit units")
    if percent == 0:
        return minimum
    provided = _bound(minimum, percent, ceiling=True).rao
    view = await client.at()
    existing = await view.read("lending_position", coldkey_ss58=owner, netuid=netuid)
    if existing is not None:
        annual = existing.get("annual_interest")
        if not isinstance(annual, Balance) or annual.netuid != minimum.netuid or annual.rao < 0:
            raise BittensorError("the existing loan's annual interest is unavailable or changed")
        year = await view.constant(("Lending", "BlocksPerYear"))
        if not isinstance(year, int) or isinstance(year, bool) or year <= 0:
            raise BittensorError("the lending interest year is unavailable")
        # Shielding may fall back to bare submission, whose normal mortal era
        # is longer. Reserve the old fixed annual interest across either path.
        window = max(DEFAULT_ERA_PERIOD, MEV_SHIELD_ERA_PERIOD)
        provided += (annual.rao * window + year - 1) // year
    if provided > maximum:
        raise BittensorError("buffered collateral does not fit the chain's deposit units")
    return view.balance(provided, minimum.netuid)


async def _ensure_open_funds(
    client, *, netuid: int, side: str, owner: str, hotkey, collateral: Balance
):
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
            "reserves accrued interest for the inclusion window. Collateral mode sets a "
            "principal floor. Shorts also set an opening value floor; longs set a min "
            "escrow-alpha floor. Must be > 0 (quotes go stale one block later)."
        ),
    ),
):
    """Add collateral and open or increase a short or long on a subnet.

    Pass --amount for the additional quantity to borrow (alpha for a short,
    TAO for a long) and btcli finds the minimum required collateral. Pass
    --collateral instead to borrow its quoted limit. The minimum opening
    value is about 0.001 TAO (runtime MinimumLoanValue).

    Amount mode adds collateral headroom via --max-slippage (default 1%)
    plus accrued-interest reserve for the inclusion window; borrowed
    principal stays exact.

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
        # Side check before the term-restart warning (E2E U3).
        existing_side = str(existing.get("side") or "").lower()
        if existing_side and existing_side != side.value.lower():
            _exit_error(
                context,
                f"existing loan is {existing['side']}; cannot open/increase as {side.value}",
                code=1,
            )
        context.output.message(
            "warning: increasing this position restarts the 1-year fixed term "
            "(expires_at moves to now + BlocksPerYear)"
        )

    if requested is not None:

        async def size(client):
            # Enrich inside this coroutine: AppContext.run catches BittensorError,
            # prints it, and raises typer.Exit — so a surrounding except never
            # runs (U4 / N16). N20: only collateral shortages get need-vs-available
            # wrapping — PrincipalExceeds* / other chain errors keep their own text.
            try:
                required = await client.read(
                    "lending_required_collateral",
                    netuid=netuid,
                    side=side.value,
                    amount=requested,
                    coldkey_ss58=owner,
                    **({"hotkey_ss58": hotkey} if hotkey is not None else {}),
                )
            except BittensorError as error:
                name = getattr(error, "name", None) or ""
                detail = str(error).strip()
                # SDK already maps PrincipalExceeds* / StakeUnavailable (N20).
                if name in (
                    "PrincipalExceedsAlphaIn",
                    "PrincipalExceedsTaoIn",
                    "StakeUnavailable",
                ):
                    raise
                if name and name not in (
                    "InsufficientCollateral",
                    "",
                ):
                    # Preserve SideCapacity / EmptyPool / PriceTooFarFromEma / …
                    if name not in detail:
                        detail = f"{detail} ({name})" if detail else name
                    if "borrow" not in detail.lower():
                        detail = f"{detail} (borrow {_fmt_amount(requested)})"
                    raise BittensorError(detail) from error
                try:
                    if side == Side.long and hotkey is not None:
                        available = await _available_alpha(client._substrate, owner, hotkey, netuid)
                        detail = (
                            f"insufficient collateral for borrow {_fmt_amount(requested)}: "
                            f"needed more than unlocked alpha {_fmt_amount(available)} "
                            f"on {hotkey}" + (f" ({name})" if name else "")
                        )
                    elif side == Side.short:
                        available = await _spendable_tao(client._substrate, owner)
                        detail = (
                            f"insufficient collateral for borrow {_fmt_amount(requested)}: "
                            f"needed more than free TAO {_fmt_amount(available)}"
                            + (f" ({name})" if name else "")
                        )
                    elif name:
                        detail = f"{detail} (borrow {_fmt_amount(requested)})"
                    elif not detail:
                        detail = f"loan sizing failed (borrow {_fmt_amount(requested)})"
                except Exception:
                    if name:
                        detail = f"{detail or name} (borrow {_fmt_amount(requested)})"
                    elif not detail:
                        detail = f"loan sizing failed (borrow {_fmt_amount(requested)})"
                raise BittensorError(detail) from error
            minimum = required["collateral"]
            provided = await _provided_collateral(client, minimum, netuid, owner, max_slippage)
            return minimum, provided

        with context.output.activity("finding the minimum required collateral…"):
            minimum_collateral, collateral = context.run(size)
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
        # Floor on simulated sale proceeds (escrow_estimate), not collateral×LTV
        # opening_value — that overstates fill when the sale moves price (E2E N3).
        proceeds = quote.get("escrow_estimate")
        if not isinstance(proceeds, Balance) or proceeds.rao <= 0:
            proceeds = quote.get("opening_value")
        if isinstance(proceeds, Balance) and proceeds.rao > 0:
            intent.min_proceeds = _bound(proceeds, max_slippage)
    else:
        escrow = quote.get("escrow_estimate")
        if isinstance(escrow, Balance) and escrow.rao > 0:
            intent.min_escrow_alpha = _bound(escrow, max_slippage)

    # Warn when capacity (not LTV) likely limits borrow. Short default LTV is
    # 25%; long thin LTV is often ~10–15%, so the old principal×4 < V_dyn
    # heuristic falsely fired on every long open (E2E N6).
    v_dyn = quote.get("v_dyn")
    principal = quote.get("principal")
    if (
        side == Side.short
        and isinstance(v_dyn, Balance)
        and isinstance(principal, Balance)
        and v_dyn.rao > 0
        and principal.rao > 0
        and principal.rao * 4 < v_dyn.rao
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
        loan_rows.append(("quoted sale proceeds", _fmt_amount(quote.get("escrow_estimate"))))
        loan_rows.append(("minimum proceeds", _fmt_amount(intent.min_proceeds)))
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
    # Open defaults valid_until to u32::MAX ("no deadline"); only show when explicit.
    if int(intent.valid_until) < VALID_UNTIL_NONE:
        loan_rows.append(("valid until", _format_valid_until(intent.valid_until, head=head)))

    def _attach_opened(result):
        error = getattr(result, "error", None)
        if error is not None and error.name == "SlippageExceeded":
            # F12: quoted vs bound for open floors (not the generic pool line).
            if side == Side.short:
                error.remediation_override = (
                    f"quoted sale proceeds {_fmt_amount(quote.get('escrow_estimate'))} vs min "
                    f"{_fmt_amount(intent.min_proceeds)}. Re-quote and raise --max-slippage "
                    f"(was {max_slippage}%), or reduce size"
                )
            else:
                error.remediation_override = (
                    f"quoted escrow alpha {_fmt_amount(quote.get('escrow_estimate'))} vs min "
                    f"{_fmt_amount(intent.min_escrow_alpha)}. Re-quote and raise --max-slippage "
                    f"(was {max_slippage}%), or reduce size"
                )
        if not result.success:
            return
        attrs = _find_event(result.events, "Lending", "Opened") or _find_event(
            result.events, "Lending", "Increased"
        )
        if not isinstance(attrs, dict):
            return
        net = int(attrs.get("netuid") or netuid)
        collateral_unit = 0 if side == Side.short else net
        principal_unit = net if side == Side.short else 0
        principal_rao = int(attrs.get("principal") or 0)
        collateral_rao = int(attrs.get("collateral") or 0)
        proceeds_rao = int(attrs.get("proceeds") or 0)
        escrow_rao = int(attrs.get("escrow_alpha") or 0)
        interest_rao = int(attrs.get("annual_interest") or 0)
        display = {
            "borrowed": _fmt_amount(Balance.from_rao(principal_rao, principal_unit)),
            "collateral_posted": _fmt_amount(Balance.from_rao(collateral_rao, collateral_unit)),
            "annual_interest": _fmt_amount(Balance.from_rao(interest_rao, collateral_unit)),
        }
        raw = {
            "borrowed": principal_rao,
            "collateral_posted": collateral_rao,
            "annual_interest": interest_rao,
        }
        if side == Side.short:
            display["escrowed"] = _fmt_amount(Balance.from_rao(proceeds_rao, 0))
            raw["escrowed"] = proceeds_rao
        else:
            display["escrowed"] = _fmt_amount(Balance.from_rao(escrow_rao, net))
            raw["escrowed"] = escrow_rao
        # U3: surface the snapshotted terms version on the result card.
        if attrs.get("terms_version") is not None:
            display["terms_version"] = _terms_version_label(attrs.get("terms_version"))
        _result_amounts(result, display=display, raw=raw)

    context.submit(
        intent,
        proxy_for=proxy or "self",
        card_sections=[("Loan", loan_rows)],
        on_result=_attach_opened,
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
        # Short names in help (F15); --repay-from-wallet restored as hidden alias (N21).
        "--from-wallet/--from-escrow",
        help=("Escrow path (default) or wallet repay. Longs still need --hotkey for alpha refund."),
    ),
    repay_from_wallet_alias: Optional[bool] = typer.Option(
        None,
        "--repay-from-wallet/--no-repay-from-wallet",
        hidden=True,
        help="Deprecated alias of --from-wallet/--from-escrow.",
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
    SDK CloseLoan default. Pass --from-wallet (alias: --repay-from-wallet) to
    repay short alpha from --hotkey stake or pay long TAO from free balance.
    Pass --hotkey for alpha repay (short wallet path) or alpha refund (long
    close / short buyback surplus). The full runtime quote sets a payment
    ceiling and refund floor; a failed quote stops submission.
    """
    context = ctx_of(ctx)
    if repay_from_wallet_alias is not None:
        repay_from_wallet = repay_from_wallet_alias
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
            "pass --hotkey <validator you staked to> when using --from-wallet "
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
    close_path = (
        "wallet repay"
        if repay_from_wallet
        else (
            "escrow buyback (wallet top-up if shortfall)"
            if side == "Short"
            else "escrow sell (A then C)"
        )
    )
    repay_rows = [
        ("side", side),
        ("close path", close_path),
        ("alpha hotkey", hotkey or "—"),
        ("quoted payment", _fmt_amount(quote["payment"])),
        ("maximum payment", _fmt_amount(intent.max_payment)),
        ("quoted leftover refund", _fmt_amount(quote["refund"])),
        ("minimum leftover refund", _fmt_amount(intent.min_refund)),
    ]
    # Only shorts collect a free-TAO top-up on the escrow path; insolvent longs
    # must use --from-wallet for the full principal (Auditor fid:7d5e8753).
    if side == "Short" and isinstance(shortfall, Balance) and shortfall.rao > 0:
        repay_rows.append(("wallet top-up (shortfall)", _fmt_amount(shortfall)))
    elif (
        side == "Long"
        and not repay_from_wallet
        and isinstance(shortfall, Balance)
        and shortfall.rao > 0
    ):
        repay_rows.append(
            (
                "close now",
                f"use --from-wallet for {_fmt_amount(position['principal'])} "
                f"(escrow path cannot top up a long shortfall)",
            )
        )
    if int(intent.valid_until) < VALID_UNTIL_NONE:
        repay_rows.append(("valid until", _format_valid_until(intent.valid_until, head=head)))

    def _enrich_slippage(result):
        error = getattr(result, "error", None)
        if error is None or error.name != "SlippageExceeded":
            return
        error.remediation_override = (
            f"quoted payment {_fmt_amount(quote['payment'])} vs max "
            f"{_fmt_amount(intent.max_payment)}; quoted leftover refund "
            f"{_fmt_amount(quote['refund'])} vs min {_fmt_amount(intent.min_refund)}. "
            f"Re-quote and raise --max-slippage (was {max_slippage}%), or reduce size"
        )

    def _attach_closed(result):
        _enrich_slippage(result)
        if not result.success:
            return
        attrs = _find_event(result.events, "Lending", "Closed")
        if not isinstance(attrs, dict):
            return
        net = int(attrs.get("netuid") or netuid)
        refund_rao = int(attrs.get("refund") or 0)
        top_up = int(attrs.get("wallet_top_up") or 0)
        payment = quote["payment"]
        payment_rao = payment.rao if isinstance(payment, Balance) else int(payment or 0)
        # Short leftover refund is TAO; long leftover refund is alpha.
        # Preflight quote is quoted_payment — never executed payment (Auditor
        # fid:37953bda). Closed has no payment field; wallet_top_up is the
        # executed free-TAO debit when the pot was short.
        refund_unit = 0 if side == "Short" else net
        _result_amounts(
            result,
            display={
                "quoted_payment": _fmt_amount(payment),
                "refund": _fmt_amount(Balance.from_rao(refund_rao, refund_unit)),
                "wallet_top_up": _fmt_amount(Balance.from_rao(top_up, 0)),
            },
            raw={
                "quoted_payment": payment_rao,
                "refund": refund_rao,
                "wallet_top_up": top_up,
            },
        )

    context.submit(
        intent,
        card_sections=[("Repayment", repay_rows)],
        on_result=_attach_closed,
    )


@app.command("repay")
@with_tx_globals
def repay_loan(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", min=1, max=65535, help="Subnet of your position."),
    principal: Optional[str] = typer.Option(
        None,
        "--principal",
        help=RepayLoan.field_help("principal_part"),
    ),
    # Hidden --amount alias: listing both names truncates at 80 cols (F15).
    principal_amount_alias: Optional[str] = typer.Option(
        None,
        "--amount",
        hidden=True,
        help="Alias of --principal.",
    ),
    hotkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("hotkey_ss58"),
        help=RepayLoan.field_help("hotkey_ss58"),
    ),
    repay_from_wallet: bool = typer.Option(
        False,
        "--from-wallet/--from-escrow",
        help=("Escrow path (default) or wallet repay. Longs still need --hotkey for alpha refund."),
    ),
    repay_from_wallet_alias: Optional[bool] = typer.Option(
        None,
        "--repay-from-wallet/--no-repay-from-wallet",
        hidden=True,
        help="Deprecated alias of --from-wallet/--from-escrow.",
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
    """Repay principal (partial or full) and return a pro-rata collateral slice.

    Pass --principal (alias: --amount; alpha for a short, TAO for a long). Full
    principal equals btcli lending close. Hotkey and --from-wallet flags match
    close (--repay-from-wallet is a hidden alias). The runtime quote sets
    payment ceiling and refund floor for the slice. Partial repay does not
    restart the one-year term.
    """
    context = ctx_of(ctx)
    if repay_from_wallet_alias is not None:
        repay_from_wallet = repay_from_wallet_alias
    if principal is None and principal_amount_alias is not None:
        principal = principal_amount_alias
    try:
        _require_slippage(max_slippage)
    except BittensorError as error:
        _exit_error(context, str(error))
    if principal is None:
        _exit_error(context, "provide --principal of debt to repay")
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
            "pass --hotkey <validator you staked to> when using --from-wallet "
            "(omit the flag for the default escrow buyback)",
        )
    try:
        part = _parse_positive_amount(
            principal,
            tao=side != "Short",
            netuid=netuid,
            label="--principal",
        )
    except BittensorError as error:
        _exit_error(context, str(error))
    stored = position["principal"]
    if not isinstance(stored, Balance) or part.rao > stored.rao:
        _exit_error(
            context,
            f"--principal {_fmt_amount(part)} exceeds stored principal {_fmt_amount(stored)}",
        )
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
    intent._closes_position = part.rao == stored.rao
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
    if (
        position.get("side") == "Short"
        and isinstance(shortfall, Balance)
        and shortfall.rao > 0
        and not repay_from_wallet
    ):
        repay_rows.append(("wallet top-up (shortfall)", _fmt_amount(shortfall)))
    expires = position.get("expires_at")
    if expires is not None:
        repay_rows.append(("expires at (unchanged)", _format_expiry(expires, head=head)))
    if int(intent.valid_until) < VALID_UNTIL_NONE:
        repay_rows.append(("valid until", _format_valid_until(intent.valid_until, head=head)))

    def _attach_repaid(result):
        if not result.success:
            return
        # Partial repay emits Repaid; full-principal repay closes and emits Closed
        # (Auditor fid:fe25eda6).
        repaid = _find_event(result.events, "Lending", "Repaid")
        closed = _find_event(result.events, "Lending", "Closed")
        payment = quote["payment"]
        payment_rao = payment.rao if isinstance(payment, Balance) else int(payment or 0)
        if isinstance(repaid, dict):
            net = int(repaid.get("netuid") or netuid)
            repaid_rao = int(repaid.get("principal_repaid") or 0)
            refund_rao = int(repaid.get("refund") or 0)
            escrow_rao = int(repaid.get("escrow_returned") or 0)
            principal_unit = net if side == "Short" else 0
            refund_unit = 0 if side == "Short" else net
            # Preflight quote only (Auditor fid:37953bda); principal_repaid is
            # the executed slice from the Repaid event.
            _result_amounts(
                result,
                display={
                    "principal_repaid": _fmt_amount(Balance.from_rao(repaid_rao, principal_unit)),
                    "quoted_payment": _fmt_amount(payment),
                    "refund": _fmt_amount(Balance.from_rao(refund_rao, refund_unit)),
                    "escrow_returned": _fmt_amount(
                        Balance.from_rao(escrow_rao, 0 if side == "Short" else net)
                    ),
                },
                raw={
                    "principal_repaid": repaid_rao,
                    "quoted_payment": payment_rao,
                    "refund": refund_rao,
                    "escrow_returned": escrow_rao,
                },
            )
            return
        if isinstance(closed, dict):
            net = int(closed.get("netuid") or netuid)
            refund_rao = int(closed.get("refund") or 0)
            top_up = int(closed.get("wallet_top_up") or 0)
            refund_unit = 0 if side == "Short" else net
            _result_amounts(
                result,
                display={
                    "quoted_payment": _fmt_amount(payment),
                    "refund": _fmt_amount(Balance.from_rao(refund_rao, refund_unit)),
                    "wallet_top_up": _fmt_amount(Balance.from_rao(top_up, 0)),
                },
                raw={
                    "quoted_payment": payment_rao,
                    "refund": refund_rao,
                    "wallet_top_up": top_up,
                },
            )

    context.submit(
        intent,
        card_sections=[("Repayment", repay_rows)],
        on_result=_attach_repaid,
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
    sell: Optional[str] = typer.Option(
        None,
        "--sell",
        help="Unlocked alpha to sell for free TAO; omit to sell the newly escrowed alpha.",
    ),
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
        # Side check before the term-restart warning (E2E U3).
        if str(existing.get("side") or "").lower() != "long":
            _exit_error(
                context,
                f"raise is long-only; existing loan is {existing.get('side')}",
                code=1,
            )
        context.output.message("warning: increasing this position restarts the 1-year fixed term")

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
        f"{headroom if headroom is not None else '?'}: {_fmt_amount(max_borrow)}"
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
            # Scale max principal by requested_ltv / max_ltv, but keep the
            # supplied collateral (Auditor fid:5cda5a30 / 4e3666e8).
            scaled = (max_borrow.rao * round(ltv * 10_000)) // max_ltv_bps
            if scaled <= 0:
                context.output.error("requested --ltv yields zero borrow at this collateral")
                raise typer.Exit(2)
            borrow_tao = str(Balance.from_rao(scaled, 0))
            collateral_alpha = collateral
            ltv_sized_from_collateral = True
            context.output.message(
                f"sizing borrow at LTV {ltv:.2%} with collateral {collateral}: {borrow_tao}"
            )
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

    # Re-quote exact C+P so escrow / sale floors match the sized raise.
    if ltv_sized_from_collateral:

        async def sized_quote(client):
            view = await client.at()
            return await view.read(
                "lending_raise_quote",
                netuid=netuid,
                coldkey_ss58=owner,
                hotkey_ss58=hotkey,
                collateral_alpha=collateral_alpha,
                borrow_tao=borrow_tao,
                sell_alpha=sell,
            )

        with context.output.activity("quoting raise at sized LTV…"):
            quote = context.run(sized_quote)
        if not isinstance(quote, dict):
            context.output.error("raise quote is unavailable for sized LTV")
            raise typer.Exit(2)

    quoted_collateral = quote.get("collateral_alpha")
    quoted_escrow = quote.get("escrow_alpha")
    quoted_principal = quote.get("principal_tao")
    if not isinstance(quoted_collateral, Balance) or not isinstance(quoted_escrow, Balance):
        context.output.error("raise quote is missing collateral or escrow alpha")
        raise typer.Exit(2)

    # Resolve max_collateral before constructing RaiseLoan — amount mode must
    # never default the ceiling to 0 (E2E N2 / CollateralExceedsMax).
    borrow_for_intent = (
        borrow_tao if collateral_alpha is None or ltv_sized_from_collateral else None
    )
    if collateral_alpha is not None and borrow_for_intent is not None:
        if max_collateral is not None:
            context.output.error(
                "--max-collateral is unused when --collateral and --ltv size together"
            )
            raise typer.Exit(2)
        ceiling: Balance = cast(Balance, alpha_amount(collateral_alpha, netuid))
        min_borrow: Balance = cast(Balance, tao_amount(borrow_for_intent))
    elif collateral_alpha is None:
        if max_collateral is not None:
            ceiling = cast(Balance, alpha_amount(max_collateral, netuid))
            if ceiling.rao < quoted_collateral.rao:
                context.output.error(
                    f"--max-collateral {ceiling} is below the quoted minimum {quoted_collateral}"
                )
                raise typer.Exit(2)
        else:
            ceiling = _bound(quoted_collateral, max_slippage, ceiling=True)
        min_borrow = (
            cast(Balance, tao_amount(borrow_tao))
            if borrow_tao is not None
            else cast(Balance, tao_amount(amount or "0"))
        )
    else:
        if max_collateral is not None:
            context.output.error("--max-collateral applies only with --amount (borrow mode)")
            raise typer.Exit(2)
        ceiling = cast(Balance, alpha_amount(collateral_alpha, netuid))
        min_borrow = (
            _bound(quoted_principal, max_slippage)
            if isinstance(quoted_principal, Balance)
            else cast(Balance, tao_amount(amount or "0"))
        )

    intent = RaiseLoan(
        netuid=netuid,
        hotkey_ss58=hotkey,
        collateral_alpha=collateral_alpha,
        borrow_tao=borrow_for_intent,
        max_collateral_alpha=ceiling,
        min_borrow_tao=min_borrow,
        sell_alpha=sell,
        min_tao_out=min_tao_out,
        valid_until=until,
    )
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

    # Card sell amount: explicit --sell, else quoted default sale (= escrowed α).
    if intent.sell_alpha is not None:
        sell_display = intent.sell_alpha
        sell_label = "sell (explicit)"
    else:
        sell_display = (
            quote.get("sell_alpha")
            if isinstance(quote.get("sell_alpha"), Balance)
            else quoted_escrow
        )
        sell_label = "sell (default = escrowed α)"
    head = context.run(lambda client: client.block())
    year = context.run(lambda client: client.constant(("Lending", "BlocksPerYear")))
    new_expires = (head + int(year)) if isinstance(year, int) and year > 0 else None
    card = [
        ("netuid", str(netuid)),
        ("owner", owner),
        ("hotkey", hotkey),
        ("collateral", _fmt_amount(quoted_collateral)),
        (
            "borrow",
            _fmt_amount(
                quoted_principal if isinstance(quoted_principal, Balance) else intent.borrow_tao
            ),
        ),
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
            (sell_label, _fmt_amount(sell_display)),
            ("min tao out", _fmt_amount(intent.min_tao_out)),
            ("valid until", _format_valid_until(intent.valid_until, head=head)),
        ]
    )
    if new_expires is not None:
        card.append(("expires at (after raise)", _format_expiry(new_expires, head=head)))

    def _attach_raised(result):
        error = getattr(result, "error", None)
        if error is not None and error.name == "SlippageExceeded":
            error.remediation_override = (
                f"quoted free TAO out {_fmt_amount(quote.get('free_tao_out'))} vs min "
                f"{_fmt_amount(intent.min_tao_out)}. Re-quote and raise --max-slippage "
                f"(was {max_slippage}%), or reduce size"
            )
        if error is not None and error.name == "StakeUnavailable":
            # N22: state required vs available alpha (collateral + sale), not
            # a generic Lock blurb that hides how much the raise needs.
            sell_need = (
                sell_display if isinstance(sell_display, Balance) else quote.get("sell_alpha")
            )
            need_rao = int(quoted_collateral.rao) + (
                int(sell_need.rao) if isinstance(sell_need, Balance) else 0
            )
            need = Balance.from_rao(need_rao, netuid)

            async def _avail(client):
                return await _available_alpha(client._substrate, owner, hotkey, netuid)

            try:
                available = context.run(_avail)
            except Exception:
                available = None
            if isinstance(available, Balance):
                error.remediation_override = (
                    f"this raise needs about {_fmt_amount(need)} unlocked alpha "
                    f"(collateral and default sale) but only {_fmt_amount(available)} "
                    f"is free on {hotkey} — stake more, free conviction Lock / miner "
                    "collateral, reduce --amount, or pass a hotkey with enough free alpha"
                )
            else:
                error.remediation_override = (
                    f"this raise needs about {_fmt_amount(need)} unlocked alpha "
                    f"(collateral and default sale) on {hotkey} — stake more, free "
                    "conviction Lock / miner collateral, reduce --amount, or pass a "
                    "hotkey with enough free alpha"
                )
        if not result.success:
            return
        attrs = _find_event(result.events, "Lending", "Raised")
        if not isinstance(attrs, dict):
            return
        net = int(attrs.get("netuid") or netuid)
        collateral_rao = int(attrs.get("collateral_alpha") or 0)
        principal_rao = int(attrs.get("principal_tao") or 0)
        escrow_rao = int(attrs.get("escrow_alpha") or 0)
        sell_rao = int(attrs.get("sell_alpha") or 0)
        free_tao_rao = int(attrs.get("free_tao_out") or 0)
        display = {
            "borrowed": _fmt_amount(Balance.from_rao(principal_rao, 0)),
            "collateral_posted": _fmt_amount(Balance.from_rao(collateral_rao, net)),
            "escrowed": _fmt_amount(Balance.from_rao(escrow_rao, net)),
            "sold": _fmt_amount(Balance.from_rao(sell_rao, net)),
            # N19: sale TAO is proceeds received, not a "payment".
            "received": _fmt_amount(Balance.from_rao(free_tao_rao, 0)),
        }
        # U3: raise keeps the open-time terms_version — show it on the result.
        if attrs.get("terms_version") is not None:
            display["terms_version"] = _terms_version_label(attrs.get("terms_version"))
        _result_amounts(
            result,
            display=display,
            raw={
                "borrowed": principal_rao,
                "collateral_posted": collateral_rao,
                "escrowed": escrow_rao,
                "sold": sell_rao,
                "received": free_tao_rao,
            },
        )

    # N22: fail before fee when unlocked alpha cannot cover collateral+sale.
    sell_need = sell_display if isinstance(sell_display, Balance) else quote.get("sell_alpha")
    need_rao = int(quoted_collateral.rao) + (
        int(sell_need.rao) if isinstance(sell_need, Balance) else 0
    )

    async def _check_raise_alpha(client):
        return await _available_alpha(client._substrate, owner, hotkey, netuid)

    try:
        available = context.run(_check_raise_alpha)
    except Exception as error:
        _exit_error(context, f"could not check unlocked alpha for raise: {error}")
    if isinstance(available, Balance) and need_rao > available.rao:
        need = Balance.from_rao(need_rao, netuid)
        _exit_error(
            context,
            f"insufficient unlocked alpha for raise: need {_fmt_amount(need)} "
            f"(collateral and default sale), available {_fmt_amount(available)} on {hotkey}",
        )

    context.submit(
        intent,
        proxy_for=proxy or "self",
        card_sections=[("Raise", card)],
        on_result=_attach_raised,
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
        # SDK lending_position / lending_positions already annotate AMM shortfall
        # onto underwater / insolvent / shortfall (E2E N4 / U2).
        return positions, head, head_time

    positions, head, head_time = context.run(_positions)
    columns = [
        "owner",
        "side",
        "principal",
        "collateral",
        "status",
        "terms",
        "escrow",
        "expires",
        "due",
        "runway",
    ]
    rows = []
    for position in positions:
        status = _position_status_label(position)
        rows.append(
            [
                position["coldkey"],
                position["side"],
                _fmt_amount(position["principal"]),
                _fmt_amount(position["collateral"]),
                status,
                _terms_version_label(position.get("terms_version")),
                _fmt_amount(position.get("escrow") or position.get("proceeds")),
                _format_expiry_field(position, head=head, head_time=head_time),
                str(position.get("due") or "-"),
                _format_runway_field(position, head=head),
            ]
        )
    context.output.table(
        f"loans on netuid {netuid}",
        columns,
        rows,
        [_position_json(position) for position in positions],
        truncate=False,
    )


@app.command(
    "show",
    help="Show one open loan with status, runway, and a live escrow close quote.",
)
@app.command(
    "quote",
    help="Quote close payment / leftover refund / shortfall for one loan (read-only).",
)
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
        "--from-wallet/--from-escrow",
        help="Quote wallet-repay close instead of the default escrow path.",
    ),
    repay_from_wallet_alias: Optional[bool] = typer.Option(
        None,
        "--repay-from-wallet/--no-repay-from-wallet",
        hidden=True,
        help="Deprecated alias of --from-wallet/--from-escrow.",
    ),
):
    """Show one open loan and its close quote (read-only; no transaction)."""
    context = ctx_of(ctx)
    if repay_from_wallet_alias is not None:
        repay_from_wallet = repay_from_wallet_alias
    owner = context.resolve_address("coldkey_ss58", coldkey_ss58)
    if owner is None:
        _exit_error(context, "select the account that owns this position")
    hotkey = context.resolve_address("lending_hotkey_ss58", hotkey_ss58)

    async def _load(client):
        position = await client.read("lending_position", coldkey_ss58=owner, netuid=netuid)
        if position is None:
            return None, None, await client.block(), None, None
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
        # U2: spot / EMA / band / forced-settle anchors for deferred show lines.
        band_ctx: dict[str, Any] = {}
        try:
            view = await client.at()
            ref = await view.query(_REF, [netuid])
            params = await view.query(_LIFT_PARAMS, [])
            since = await view.query(_BAND_SINCE, [owner, netuid])
            max_defer = await client.constant(("Lending", "MaxSettlementDeferral"))
            spot_rao = await view.runtime(api.SwapRuntimeApi.current_alpha_price, [netuid])
            ema = None
            price_raw = None
            if isinstance(ref, dict):
                price_raw = ref.get("price")
            elif isinstance(ref, (list, tuple)) and ref:
                price_raw = ref[0]
            if isinstance(price_raw, dict):
                ema = int(price_raw.get("bits") or 0) / float(2**64)
            elif price_raw is not None:
                ema = int(price_raw) / float(2**64)
            spot = float(int(spot_rao or 0)) / float(RAO_PER_TAO)
            band_bps = None
            if isinstance(params, dict) and params.get("ema_band_bps") is not None:
                band_bps = int(params["ema_band_bps"])
            spot_vs_ema_pct = None
            if ema is not None and ema > 0 and spot > 0:
                spot_vs_ema_pct = (spot / ema - 1.0) * 100.0
            force_at = None
            if since is not None and max_defer is not None:
                force_at = int(since) + int(max_defer)
            band_ctx = {
                "spot_vs_ema_pct": spot_vs_ema_pct,
                "band_bps": band_bps,
                "force_at": force_at,
            }
        except Exception:
            band_ctx = {}
        return position, quote, head, head_time, band_ctx

    try:
        position, quote, head, head_time, band_ctx = context.run(_load)
    except BittensorError as error:
        # Close quote should no longer raise for insolvent longs; keep a clear
        # read-path message if a node is still on the old InsufficientEscrow path.
        _exit_error(
            context,
            f"could not quote close for this loan: {error} (read failed — not a rejected call)",
            code=1,
        )
    if position is None:
        _exit_error(context, f"no lending position on netuid {netuid}", code=1)
    band_ctx = band_ctx or {}
    shortfall = quote.get("shortfall")
    shortfall_rao = shortfall.rao if isinstance(shortfall, Balance) else int(shortfall or 0)
    underwater = (
        bool(position.get("underwater")) or bool(position.get("insolvent")) or shortfall_rao > 0
    )
    # Mirror list: pending deferral beats ok even when the close quote is solvent.
    if underwater and not position.get("underwater"):
        position = {**position, "underwater": True, "close_shortfall": shortfall}
    status = _position_status_label(position)
    fields = {
        "owner": owner,
        "netuid": netuid,
        "side": position["side"],
        "principal": _fmt_amount(position["principal"]),
        "collateral": _fmt_amount(position["collateral"]),
        "escrow": _fmt_amount(position.get("escrow") or position.get("proceeds")),
        "interest due": _fmt_amount(position["interest_due"]),
        "status": status,
        "terms version": _terms_version_label(position.get("terms_version")),
        "expires": _format_expiry_field(position, head=head, head_time=head_time),
        "due": position.get("due"),
        "runway": _format_runway_field(position, head=head),
        "quoted payment": _fmt_amount(quote["payment"]),
        "quoted leftover refund": _fmt_amount(quote["refund"]),
        "shortfall": _fmt_amount(shortfall),
    }
    if shortfall_rao > 0 and not repay_from_wallet:
        fields["at expiry"] = (
            "forfeit if still insolvent (buy back through pool, burn leftover C, no refund)"
        )
        # Shorts can fund a free-TAO top-up on escrow close; longs cannot —
        # they need --from-wallet for the full principal (Auditor fid:7d5e8753).
        if position.get("side") == "Short" and isinstance(shortfall, Balance) and shortfall.rao > 0:
            fields["wallet top-up to close now"] = _fmt_amount(shortfall)
        elif position.get("side") == "Long":
            fields["wallet repayment to close now"] = (
                f"{_fmt_amount(position['principal'])} with --from-wallet"
            )
    deferrals = position.get("deferrals")
    if not isinstance(deferrals, list) or not deferrals:
        deferred = position.get("deferred")
        deferrals = [deferred] if isinstance(deferred, dict) and deferred.get("kind") else []

    def _deferral_line(entry: dict) -> str:
        reason = str(entry.get("reason") or entry.get("kind") or "deferred")
        explained = _explain_band_deferral(
            reason,
            spot_vs_ema_pct=band_ctx.get("spot_vs_ema_pct"),
            band_bps=band_ctx.get("band_bps"),
            retry_at=entry.get("retry_at"),
            force_at=band_ctx.get("force_at"),
        )
        if explained != reason:
            return explained
        return f"{reason}; retry at block {entry.get('retry_at')}"

    if len(deferrals) == 1:
        entry = deferrals[0]
        kind = entry.get("kind")
        fields["deferred"] = f"{kind} — {_deferral_line(entry)}"
    elif len(deferrals) > 1:
        # N18: show every pending kind (interest + settlement).
        for entry in deferrals:
            if not isinstance(entry, dict) or not entry.get("kind"):
                continue
            kind = entry["kind"]
            fields[f"deferred ({kind})"] = _deferral_line(entry)
    json_fields = {
        **_position_json(position),
        "underwater": underwater,
        "insolvent": underwater,
        "shortfall": shortfall_rao,
        "close_quote": {
            "payment": quote["payment"].rao
            if isinstance(quote["payment"], Balance)
            else quote["payment"],
            "refund": quote["refund"].rao
            if isinstance(quote["refund"], Balance)
            else quote["refund"],
            "shortfall": shortfall_rao,
            "repay_from_wallet": repay_from_wallet,
            "hotkey_ss58": hotkey,
        },
    }
    # Point `see:` at the repo MDX (N12) — no soft-redirect home pages.
    docs = [query_docs_url("lending_position")]
    context.output.detail(
        f"lending position on netuid {netuid}",
        fields,
        json_fields=json_fields,
        docs=docs,
    )
