"""`btcli bridge`: the canonical rails between Bittensor and other chains."""

from __future__ import annotations

import time
from decimal import Decimal, InvalidOperation
from typing import Optional

import typer

from ... import rails
from ..._generated.runtime_apis import RailsApi
from ...evm import rpc as evm_rpc
from ...intents._money import tusd_from_units
from ...result import chain_error_from_dispatch
from ...settings import guide_docs_url, query_docs_url
from ..context import AppContext, address_cli_name, ctx_of, ss58_param_help
from ..globals import evm_key_signed, with_globals, with_tx_globals
from ..tx import intent_command
from .evm._shared import (
    EVM_KEY_HELP,
    RPC_URL_OPTION,
    _key_info,
    _run_evm,
    _submit_evm_tx,
    _tao_to_wei,
)

app = typer.Typer(
    no_args_is_help=True,
    help=(
        "The canonical rails: move value between Bittensor and other chains."
        "\n\nA stablecoin deposited on another chain becomes tUSD, TAO, a subnet's alpha "
        "token or service credits. Tokens sell back for the stablecoin."
        f"\n\nGuide: {guide_docs_url('rails')}"
    ),
)

SPOKE_OPTION = typer.Option(
    None, "--spoke", help="The other chain, by name. Needed only when several are known."
)
MANIFEST_OPTION = typer.Option(
    None,
    "--manifest",
    envvar="BT_RAILS_MANIFEST",
    help="A JSON file that lists spokes, as the local rig writes one.",
)
RECEIVE_HELP = (
    "What the order buys: tao (canonical TAO), alpha (a subnet's alpha token), credits "
    "(service credits), tusd or native-tao (at an account on Bittensor), or stable (the "
    "stablecoin, when selling a token)."
)
TOLERANCE_HELP = (
    "Maximum fall of what you receive below the current quote, as a fraction (0.05 = 5%)."
)


def _spoke(app_ctx: AppContext, name: Optional[str], manifest: Optional[str]) -> rails.Spoke:
    try:
        return rails.load_spoke(name, manifest)
    except (ValueError, OSError, KeyError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)


def _units(app_ctx: AppContext, text: str, decimals: int, flag: str) -> int:
    """An amount a person typed, in units of ``decimals``."""
    try:
        units = Decimal(text) * 10**decimals
    except InvalidOperation:
        app_ctx.output.error(f"invalid value for `{flag}`: {text!r} is not a number")
        raise typer.Exit(2)
    if units <= 0 or units != units.to_integral_value():
        app_ctx.output.error(
            f"invalid value for `{flag}`: above 0, with at most {decimals} decimals"
        )
        raise typer.Exit(2)
    return int(units)


def _destination(
    app_ctx: AppContext,
    receive: str,
    *,
    own_address: str,
    netuid: Optional[int],
    to: Optional[str],
    service: Optional[str],
    beneficiary: Optional[str],
    account: Optional[str],
) -> rails.Destination:
    kind = receive.strip().lower()
    try:
        if kind == "stable":
            return rails.to_stable(to or own_address)
        if kind == "tao":
            return rails.to_tao(to or own_address)
        if kind == "alpha":
            if netuid is None:
                raise ValueError("--receive alpha needs --netuid")
            return rails.to_alpha(netuid, to or own_address)
        if kind == "credits":
            if netuid is None or service is None:
                raise ValueError("--receive credits needs --netuid and --service")
            return rails.to_credits(
                netuid, app_ctx.resolve_address("hotkey_ss58", service), beneficiary or own_address
            )
        if kind in ("tusd", "native-tao"):
            ss58 = app_ctx.resolve_address("coldkey_ss58", account)
            return rails.to_native_tusd(ss58) if kind == "tusd" else rails.to_native_tao(ss58)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)
    app_ctx.output.error(
        f"unknown --receive {receive!r}: one of tao, alpha, credits, tusd, native-tao, stable"
    )
    raise typer.Exit(2)


def _quote(app_ctx: AppContext, bridge: int, source, amount: int, destination) -> int:
    """What the hub would deliver for the order now. Exits if it would refund it."""

    async def _op(client):
        return await client.runtime(RailsApi.quote_order, [bridge, source, amount, destination.hub])

    quote = app_ctx.run(_op)
    if "Err" in quote:
        app_ctx.output.error(
            f"the hub would refund this order: {chain_error_from_dispatch(quote['Err'])}"
        )
        raise typer.Exit(1)
    return int(quote["Ok"])


def _sent(app_ctx: AppContext, spoke: rails.Spoke, rpc: evm_rpc.EvmRpc, result: Optional[dict]):
    """Reports the order a transaction sent, and how to follow it."""
    if result is None:
        return
    receipt = _run_evm(app_ctx, lambda: rpc.call("eth_getTransactionReceipt", [result["tx_hash"]]))
    nonce = rails.order_nonce(receipt, spoke.portal)
    if nonce is None:
        app_ctx.output.error("the transaction sent no order")
        raise typer.Exit(1)
    app_ctx.output.detail(
        "order sent",
        {
            "bridge": spoke.bridge,
            "order": nonce,
            "follow it": f"btcli bridge status --bridge {spoke.bridge} --order {nonce}",
        },
        {"bridge": spoke.bridge, "order": nonce, "tx_hash": result["tx_hash"]},
    )


@app.command("deposit")
@with_tx_globals
@evm_key_signed
def deposit(
    ctx: typer.Context,
    amount: str = typer.Option(..., "--amount", help="How much of the stablecoin to deposit."),
    receive: str = typer.Option(..., "--receive", help=RECEIVE_HELP),
    netuid: Optional[int] = typer.Option(None, "--netuid", help="Subnet of the token or credits."),
    to: Optional[str] = typer.Option(
        None, "--to", help="0x address that receives a token. Defaults to the EVM key's own."
    ),
    service: Optional[str] = typer.Option(
        None, "--service", help="Hotkey of the service the credits are spent at."
    ),
    beneficiary: Optional[str] = typer.Option(
        None, "--beneficiary", help="0x address the credits are for. Defaults to the EVM key's."
    ),
    account: Optional[str] = typer.Option(
        None,
        "--account",
        help="Account on Bittensor that receives tUSD or TAO. Defaults to the wallet's coldkey.",
    ),
    rate_tolerance: float = typer.Option(0.05, "--rate-tolerance", help=TOLERANCE_HELP),
    deadline_minutes: int = typer.Option(
        30, "--deadline-minutes", help="Minutes after which the hub refunds the order unsettled."
    ),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    spoke_name: Optional[str] = SPOKE_OPTION,
    manifest: Optional[str] = MANIFEST_OPTION,
):
    """Deposit a stablecoin on another chain for an asset of the rails.

    Locks the stablecoin in the spoke's vault and sends an order to the hub on
    Bittensor, which settles it: tUSD is minted one for one, and trades through
    the canonical pool and the subnet's pool as the destination needs. If the
    hub cannot settle the order within its minimum and deadline, the vault
    returns the stablecoin. The first deposit also approves the vault to take
    the stablecoin, in a transaction of its own.
    """
    app_ctx = ctx_of(ctx)
    spoke = _spoke(app_ctx, spoke_name, manifest)
    rpc = evm_rpc.EvmRpc(spoke.rpc_url)
    owner = _key_info(app_ctx, key).address
    units = _units(app_ctx, amount, spoke.stable_decimals, "--amount")
    destination = _destination(
        app_ctx,
        receive,
        own_address=owner,
        netuid=netuid,
        to=to,
        service=service,
        beneficiary=beneficiary,
        account=account,
    )
    quote = _quote(app_ctx, spoke.bridge, "Stable", units * spoke.stable_scale, destination)
    min_out = int(quote * (1 - rate_tolerance))

    allowance = _run_evm(
        app_ctx,
        lambda: rails.decode_uint(
            rpc.eth_call({"to": spoke.stable, "data": rails.encode_allowance(owner, spoke.vault)})
        ),
    )
    if allowance < units and not app_ctx.dry_run:
        _submit_evm_tx(
            app_ctx,
            key,
            spoke.stable,
            data=rails.encode_approve(spoke.vault, units),
            summary=f"approve the vault to take {amount} {spoke.stable_symbol}",
            rpc_url=spoke.rpc_url,
        )
    fee = _run_evm(
        app_ctx,
        lambda: rails.decode_uint(
            rpc.eth_call({"to": spoke.portal, "data": rails.encode_order_fee()})
        ),
    )
    result = _submit_evm_tx(
        app_ctx,
        key,
        spoke.vault,
        value_wei=fee,
        data=rails.encode_deposit(
            units, destination, min_out, int(time.time()) + deadline_minutes * 60
        ),
        summary=f"deposit {amount} {spoke.stable_symbol} for {receive}",
        rpc_url=spoke.rpc_url,
        preview_fields={
            "receive": f"~{tusd_from_units(quote)}",
            "minimum": tusd_from_units(min_out),
        },
    )
    _sent(app_ctx, spoke, rpc, result)


@app.command("sell")
@with_tx_globals
@evm_key_signed
def sell(
    ctx: typer.Context,
    token: str = typer.Option(..., "--token", help="What is sold: tao, or alpha with --netuid."),
    amount: str = typer.Option(..., "--amount", help="How many tokens to sell."),
    netuid: Optional[int] = typer.Option(None, "--netuid", help="Subnet of the alpha token sold."),
    receive: str = typer.Option("stable", "--receive", help=RECEIVE_HELP),
    receive_netuid: Optional[int] = typer.Option(
        None, "--receive-netuid", help="Subnet of the token or credits bought."
    ),
    to: Optional[str] = typer.Option(
        None, "--to", help="0x address that receives. Defaults to the EVM key's own."
    ),
    service: Optional[str] = typer.Option(
        None, "--service", help="Hotkey of the service the credits are spent at."
    ),
    beneficiary: Optional[str] = typer.Option(
        None, "--beneficiary", help="0x address the credits are for. Defaults to the EVM key's."
    ),
    account: Optional[str] = typer.Option(
        None,
        "--account",
        help="Account on Bittensor that receives tUSD or TAO. Defaults to the wallet's coldkey.",
    ),
    rate_tolerance: float = typer.Option(0.05, "--rate-tolerance", help=TOLERANCE_HELP),
    deadline_minutes: int = typer.Option(
        30, "--deadline-minutes", help="Minutes after which the hub refunds the order unsettled."
    ),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    spoke_name: Optional[str] = SPOKE_OPTION,
    manifest: Optional[str] = MANIFEST_OPTION,
):
    """Sell canonical TAO or an alpha token on another chain.

    Burns the tokens and sends an order to the hub on Bittensor, which pays
    the stablecoin from the spoke's vault by default. If the hub cannot settle
    the order within its minimum and deadline, the tokens are minted back.
    """
    app_ctx = ctx_of(ctx)
    spoke = _spoke(app_ctx, spoke_name, manifest)
    rpc = evm_rpc.EvmRpc(spoke.rpc_url)
    owner = _key_info(app_ctx, key).address
    kind = token.strip().lower()
    if kind == "tao" and netuid is None:
        source, lookup = "Tao", rails.encode_tao_token()
    elif kind == "alpha" and netuid is not None:
        source, lookup = {"Alpha": netuid}, rails.encode_alpha_token(netuid)
    else:
        app_ctx.output.error("--token is tao, or alpha with --netuid")
        raise typer.Exit(2)
    contract = _run_evm(
        app_ctx,
        lambda: rails.decode_address(rpc.eth_call({"to": spoke.portal, "data": lookup})),
    )
    if int(contract, 16) == 0:
        app_ctx.output.error(f"netuid {netuid} has no alpha token on {spoke.name}")
        raise typer.Exit(1)
    units = _units(app_ctx, amount, rails.HUB_DECIMALS, "--amount")
    destination = _destination(
        app_ctx,
        receive,
        own_address=owner,
        netuid=receive_netuid,
        to=to,
        service=service,
        beneficiary=beneficiary,
        account=account,
    )
    quote = _quote(app_ctx, spoke.bridge, source, units, destination)
    min_out = int(quote * (1 - rate_tolerance))
    fee = _run_evm(
        app_ctx,
        lambda: rails.decode_uint(
            rpc.eth_call({"to": spoke.portal, "data": rails.encode_order_fee()})
        ),
    )
    result = _submit_evm_tx(
        app_ctx,
        key,
        contract,
        value_wei=fee,
        data=rails.encode_sell(
            units, destination, min_out, int(time.time()) + deadline_minutes * 60
        ),
        summary=f"sell {amount} of {kind} for {receive}",
        rpc_url=spoke.rpc_url,
        preview_fields={
            "receive": f"~{tusd_from_units(quote)}",
            "minimum": tusd_from_units(min_out),
        },
    )
    _sent(app_ctx, spoke, rpc, result)


@app.command(
    "status",
    help="Show what became of an order that a portal sent to the hub.\n\n"
    f"Docs: {query_docs_url('bridge_order')}",
)
@with_globals
def status(
    ctx: typer.Context,
    bridge: int = typer.Option(..., "--bridge", help="Id of the bridge: 8453 is Base."),
    order: int = typer.Option(..., "--order", help="The order's number at its portal."),
    spoke_name: Optional[str] = SPOKE_OPTION,
    manifest: Optional[str] = MANIFEST_OPTION,
):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("bridge_order", bridge=bridge, nonce=order)

    record = app_ctx.run(_op)
    answers: list[dict] = []
    try:
        spoke = rails.load_spoke(spoke_name, manifest)
    except (ValueError, OSError, KeyError):
        spoke = None
    if spoke is not None and spoke.bridge == bridge:
        rpc = evm_rpc.EvmRpc(spoke.rpc_url)
        # Public endpoints answer for a bounded range of blocks.
        start = max(_run_evm(app_ctx, rpc.block_number) - 9_000, 0)
        logs = _run_evm(
            app_ctx,
            lambda: rpc.call("eth_getLogs", [rails.credited_filter(spoke.portal, order, start)]),
        )
        answers = [answer for answer in map(rails.decode_credited, logs) if answer["answers_order"]]
        for answer in answers:
            if answer["asset"] != "stable":
                continue
            # A vault pays a large amount in parts: show what is still to come.
            read = {"to": spoke.portal, "data": rails.encode_owed(answer["to"])}
            answer["recipient_owed"] = _run_evm(
                app_ctx, lambda read=read: rails.decode_uint(rpc.eth_call(read))
            )
    record["delivered"] = answers
    paid = "; ".join(
        f"{tusd_from_units(a['amount'])} of {a['asset']}"
        + (f" {a['netuid']}" if a["netuid"] is not None else "")
        + f" to {a['to']}"
        + (" (refund)" if a["refund"] else "")
        + (
            f" (the vault still owes this address {a['recipient_owed']} units: "
            "`btcli bridge claim`)"
            if a.get("recipient_owed")
            else ""
        )
        for a in answers
    )
    app_ctx.output.detail(
        f"order {order} of bridge {bridge}",
        {
            "status": record["status"],
            "delivered by the hub": record["amount_out"] or "nothing",
            "reason": record["error"] or "none",
            "hub block": record["block"] or "none",
            "paid on the spoke": paid or ("not yet" if spoke is not None else "unknown"),
        },
        record,
    )


@app.command(
    "tokens",
    help="List the canonical tokens: which contract on which chain stands for which asset.\n\n"
    f"Docs: {query_docs_url('rails_tokens')}",
)
@with_globals
def tokens(ctx: typer.Context):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("rails_tokens")

    records = app_ctx.run(_op)
    rows = [
        [
            r["bridge"],
            r["asset"] if r["netuid"] is None else f"alpha {r['netuid']}",
            r["symbol"],
            r["name"],
            "" if r["rate"] is None else f"{r['rate']:.6f}",
            "" if r["supply"] is None else r["supply"],
            r["address"],
        ]
        for r in records
    ]
    app_ctx.output.columns(
        "canonical tokens",
        ["bridge", "asset", "symbol", "name", "alpha per token", "supply", "address"],
        rows,
        records,
        right_align={0, 4, 5},
    )


@app.command(
    "list",
    help="List the bridges that may mint tUSD, with their caps and what they have "
    f"outstanding.\n\nDocs: {query_docs_url('bridges')}",
)
@with_globals
def list_bridges(ctx: typer.Context):
    app_ctx: AppContext = ctx_of(ctx)

    async def _op(client):
        return await client.read("bridges")

    records = app_ctx.run(_op)
    rows = [
        [
            r["bridge"],
            "on" if r["enabled"] else "off",
            r["stablecoin"],
            r["outstanding_tusd"],
            r["mint_cap_tusd"],
            str(r["tao_supply"]),
            r["next_order"],
            "not set"
            if r["message_reserve"] is None
            else f"{r['message_reserve']} / {r['message_ceiling']}",
            str(r["message_budget"]),
            r["messages_waiting"],
            ", ".join(str(nonce) for nonce in r["messages_deferred"]) or "-",
            r["portal"],
        ]
        for r in records
    ]
    app_ctx.output.columns(
        "bridges",
        [
            "bridge",
            "status",
            "stablecoin",
            "outstanding",
            "cap",
            "canonical TAO",
            "orders",
            "message reserve / ceiling",
            "message budget",
            "waiting",
            "deferred",
            "portal",
        ],
        rows,
        records,
        right_align={0, 3, 4, 5, 6, 7, 8, 9},
    )


@app.command(
    "balance",
    help=f"Show the tUSD an account holds.\n\nDocs: {query_docs_url('tusd_balance')}",
)
@with_globals
def balance(
    ctx: typer.Context,
    coldkey_ss58: Optional[str] = typer.Option(
        None,
        address_cli_name("coldkey_ss58"),
        help=f"Account holding the tUSD. {ss58_param_help('coldkey_ss58')}",
    ),
):
    app_ctx: AppContext = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", coldkey_ss58)

    async def _op(client):
        return await client.read("tusd_balance", coldkey_ss58=coldkey)

    held = app_ctx.run(_op)
    app_ctx.output.detail(
        None, {"account": coldkey, "tUSD": held}, {"account": coldkey, "tusd": held}
    )


@app.command("flush")
@with_tx_globals
@evm_key_signed
def flush(
    ctx: typer.Context,
    bridge: int = typer.Option(..., "--bridge", help="Id of the bridge: 8453 is Base."),
    limit: int = typer.Option(8, "--limit", min=1, help="The most messages to take."),
    message: Optional[int] = typer.Option(
        None,
        "--message",
        min=0,
        help="Send this one deferred message, by its number, instead of the messages in line.",
    ),
    pay_up_to: Optional[str] = typer.Option(
        None,
        "--pay-up-to",
        help="The most TAO to pay towards what the transport charges above what the hub "
        "pays. What is not needed comes back.",
    ),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
    manifest: Optional[str] = MANIFEST_OPTION,
):
    """Hand the hub's waiting messages for one bridge to the transport.

    The hub queues what it tells another chain to pay out, one queue for each
    bridge. The messages leave when somebody asks the gateway contract on
    Bittensor's EVM to send them. A keeper does that, and this command does it
    now. Anyone may call it, and it can only send what the hub already decided.
    The EVM key pays the gas in TAO.

    The hub pays the transport for each message, up to a limit: the bridge's
    reserve, and above it what the bridge's account can spare, up to the
    bridge's ceiling (`btcli bridge list`). If the transport charges more,
    `--pay-up-to` pays the difference from the EVM key.

    A message that is not paid for is deferred: it leaves the line, so that the
    messages behind it can go, and it stays in the hub. `--message` sends one
    deferred message (`btcli bridge list` shows their numbers). It fails if the
    message is still not paid for.
    """
    app_ctx = ctx_of(ctx)
    try:
        gateway = rails.load_gateway(manifest)
    except (ValueError, OSError, KeyError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    if message is None:
        data = rails.encode_flush(bridge, limit)
        summary = f"hand up to {limit} of the hub's messages for bridge {bridge} to the transport"
    else:
        data = rails.encode_send(bridge, message)
        summary = f"hand message {message} of the hub for bridge {bridge} to the transport"
    _submit_evm_tx(
        app_ctx,
        key,
        gateway,
        value_wei=_tao_to_wei(app_ctx, pay_up_to) if pay_up_to else 0,
        data=data,
        summary=summary,
        rpc_url=rpc_url,
    )


@app.command("claim")
@with_tx_globals
@evm_key_signed
def claim(
    ctx: typer.Context,
    to: Optional[str] = typer.Option(
        None, "--to", help="The address that is owed. Defaults to the EVM key's address."
    ),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    spoke_name: Optional[str] = SPOKE_OPTION,
    manifest: Optional[str] = MANIFEST_OPTION,
):
    """Collect the stablecoin that a vault still owes an address.

    A vault releases a limited amount of the stablecoin in a period of time. A
    payment larger than what is left of that amount arrives in parts: the
    first part with the hub's answer, and the rest when somebody asks again
    after the allowance refilled. This asks. The stablecoin goes to the address
    that is owed, whoever calls. The EVM key pays the gas on the spoke.
    """
    app_ctx = ctx_of(ctx)
    spoke = _spoke(app_ctx, spoke_name, manifest)
    rpc = evm_rpc.EvmRpc(spoke.rpc_url)
    owner = to or _key_info(app_ctx, key).address
    read = {"to": spoke.portal, "data": rails.encode_owed(owner)}
    owed = _run_evm(app_ctx, lambda: rails.decode_uint(rpc.eth_call(read)))
    if owed == 0:
        app_ctx.output.detail(
            None,
            {"address": owner, "owed": f"0 {spoke.stable_symbol}"},
            {"address": owner, "owed": 0, "tx_hash": None},
        )
        return
    result = _submit_evm_tx(
        app_ctx,
        key,
        spoke.portal,
        data=rails.encode_claim(owner),
        summary=f"collect what the vault owes {owner} on {spoke.name}",
        rpc_url=spoke.rpc_url,
    )
    if result is None:
        return
    left = _run_evm(app_ctx, lambda: rails.decode_uint(rpc.eth_call(read)))
    scale = Decimal(10) ** spoke.stable_decimals
    app_ctx.output.detail(
        None,
        {
            "address": owner,
            "paid": f"{(owed - left) / scale} {spoke.stable_symbol}",
            "still owed": f"{left / scale} {spoke.stable_symbol}",
        },
        {"address": owner, "paid": owed - left, "owed": left, "tx_hash": result["tx_hash"]},
    )


# The hub's calls are the generated intent commands (`tx bridge-out` etc.), mounted here
# so both spellings stay in sync.
for _alias, _op in (("exit", "bridge_out"), ("transfer", "transfer_tusd")):
    app.command(_alias)(intent_command(_op))
