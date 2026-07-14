"""Money movement commands for ``btcli evm``."""

from __future__ import annotations

from typing import Optional

import typer

from ....balance import Balance
from ....evm import addresses as evm_addresses
from ....evm import precompiles as evm_precompiles
from ....evm import rpc as evm_rpc
from ....intents import FundEvmKey
from ...context import ctx_of
from ...globals import with_globals, with_tx_globals
from ._shared import (
    EVM_ADDRESS_HELP,
    EVM_KEY_HELP,
    PANEL_MONEY,
    RPC_URL_OPTION,
    _address_of,
    _rpc,
    _run_evm,
    _submit_evm_tx,
    _tao_to_wei,
)


def register(app: typer.Typer) -> None:
    app.command("balance", rich_help_panel=PANEL_MONEY)(balance)
    app.command("fund", rich_help_panel=PANEL_MONEY)(fund)
    app.command("send", rich_help_panel=PANEL_MONEY)(send)
    app.command("send-to-ss58", rich_help_panel=PANEL_MONEY)(send_to_ss58)


@with_globals
def balance(
    ctx: typer.Context,
    address: Optional[str] = typer.Argument(None, help=EVM_ADDRESS_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """An EVM account's balance, in TAO and in wei (the 18-decimal EVM view)."""
    app_ctx = ctx_of(ctx)
    h160 = _address_of(app_ctx, address, param="ADDRESS")
    _network, rpc = _rpc(app_ctx, rpc_url)
    wei = _run_evm(app_ctx, lambda: rpc.get_balance_wei(h160))
    amount = evm_rpc.wei_to_balance(wei)
    app_ctx.output.detail(
        None,
        {
            "address": h160,
            "balance": str(amount),
            "wei": wei,
            "ss58 mirror": evm_addresses.h160_to_ss58(h160),
        },
        json_fields={
            "address": h160,
            "balance_tao": format(amount.decimal, "f"),
            "balance_wei": wei,
            "ss58_mirror": evm_addresses.h160_to_ss58(h160),
        },
    )


@with_tx_globals
def fund(
    ctx: typer.Context,
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_ADDRESS_HELP),
    amount_tao: str = typer.Option(
        ..., "--amount-tao", help="How much TAO to send. Pass `all` for the entire balance."
    ),
):
    """Fund an EVM key with TAO from the coldkey (a transfer to its ss58 mirror).

    The native transfer credits the EVM account directly — MetaMask shows the
    balance (with 18 decimals) as soon as it lands.
    """
    app_ctx = ctx_of(ctx)
    h160 = _address_of(app_ctx, key, param="--evm-key")
    app_ctx.submit(FundEvmKey(evm_address=h160, amount_tao=amount_tao))


@with_tx_globals
def send(
    ctx: typer.Context,
    to: str = typer.Option(..., "--to", help=EVM_ADDRESS_HELP),
    amount_tao: str = typer.Option(..., "--amount-tao", help="How much TAO to send."),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Send TAO between EVM accounts (an ordinary EVM value transfer)."""
    app_ctx = ctx_of(ctx)
    dest = _address_of(app_ctx, to, param="--to")
    wei = _tao_to_wei(app_ctx, amount_tao)
    _submit_evm_tx(
        app_ctx,
        key,
        dest,
        value_wei=wei,
        summary=f"send {Balance.from_tao(amount_tao)} to {dest}",
        rpc_url=rpc_url,
    )


@with_tx_globals
def send_to_ss58(
    ctx: typer.Context,
    to: Optional[str] = typer.Option(
        None,
        "--to",
        help="Destination: ss58 address, address-book name, or local wallet name. "
        "Defaults to the configured wallet's coldkey.",
    ),
    amount_tao: str = typer.Option(..., "--amount-tao", help="How much TAO to send."),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Send TAO from a stored EVM key to any ss58 address (balance-transfer precompile).

    Gas is paid by the EVM key. This is not `btcli evm claim-deposit`, which pulls
    a MetaMask deposit into the coldkey from its truncated mirror via a substrate
    extrinsic (no EVM gas). See also `btcli tx evm-withdraw` for the same intent.
    """
    app_ctx = ctx_of(ctx)
    dest = app_ctx.resolve_address("coldkey_ss58", to)
    assert dest is not None
    precompile = evm_precompiles.get_precompile("balance-transfer")
    data = evm_precompiles.encode_call(
        precompile.function("transfer"), [evm_addresses.ss58_to_pubkey(dest)]
    )
    wei = _tao_to_wei(app_ctx, amount_tao)
    _submit_evm_tx(
        app_ctx,
        key,
        precompile.address,
        value_wei=wei,
        data=data,
        summary=f"send {Balance.from_tao(amount_tao)} from the EVM key to {dest}",
        rpc_url=rpc_url,
    )
