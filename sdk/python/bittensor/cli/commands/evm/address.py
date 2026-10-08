"""Address math commands for ``btcli evm``."""

from __future__ import annotations

from typing import Optional

import typer

from ....evm import addresses as evm_addresses
from ...context import ctx_of
from ...globals import with_globals
from ._shared import EVM_ADDRESS_HELP, PANEL_KEYS, PANEL_MONEY, _address_of


def register(app: typer.Typer) -> None:
    app.command("mirror", rich_help_panel=PANEL_KEYS)(mirror)
    app.command("pubkey", rich_help_panel=PANEL_KEYS)(pubkey)
    app.command("deposit-address", rich_help_panel=PANEL_MONEY)(deposit_address)


@with_globals
def mirror(
    ctx: typer.Context,
    address: Optional[str] = typer.Argument(None, help=EVM_ADDRESS_HELP),
):
    """The current native receiving address of an EVM account.

    Protected aliases return their full receiving address. With hashed aliases
    enabled, unprotected aliases must be funded through an EVM-side transfer.
    """
    app_ctx = ctx_of(ctx)
    h160 = _address_of(app_ctx, address, param="ADDRESS")
    recipient = app_ctx.run(
        lambda client: evm_addresses.resolve_evm_funding_recipient(client._substrate, h160)
    )
    app_ctx.output.detail(
        None,
        {
            "address": h160,
            (
                "native receiving address" if recipient.descriptor else "ss58 mirror"
            ): recipient.address,
        },
        json_fields={
            "address": h160,
            (
                "native_receiving_address" if recipient.descriptor else "ss58_mirror"
            ): recipient.address,
        },
    )


@with_globals
def pubkey(
    ctx: typer.Context,
    ss58: str = typer.Argument(..., help="ss58 address (hotkey or coldkey)."),
):
    """An ss58 address's 32-byte public key — the bytes32 form precompiles take.

    Every precompile parameter typed `bytes32 hotkey`/`bytes32 coldkey` wants
    this, not the ss58 string. (`btcli evm call` converts automatically.)
    """
    app_ctx = ctx_of(ctx)
    try:
        key = evm_addresses.ss58_to_pubkey(ss58)
    except Exception as error:
        app_ctx.output.error(f"invalid ss58 address {ss58!r}: {error}")
        raise typer.Exit(2)
    app_ctx.output.detail(None, {"ss58": ss58, "pubkey": key})


@with_globals
def deposit_address(ctx: typer.Context):
    """Where to send TAO from an EVM wallet so the coldkey can claim it.

    Every native account controls one EVM address (the first 20 bytes of its
    public key). Send TAO from MetaMask to the EVM address below, then pull
    the funds into the coldkey with `btcli evm claim-deposit` — no EVM gas or
    extra key needed. This is not `btcli evm send-to-ss58`, which spends from a
    stored EVM key via the balance-transfer precompile.
    """
    app_ctx = ctx_of(ctx)
    coldkey = app_ctx.resolve_address("coldkey_ss58", None)
    assert coldkey is not None
    truncated, recipient = app_ctx.run(
        lambda client: evm_addresses.resolve_evm_deposit(client._substrate, coldkey)
    )
    if recipient.descriptor is not None:
        app_ctx.output.detail(
            f"EVM deposit address for {app_ctx.wallet_name}",
            {
                "coldkey": coldkey,
                "evm_deposit_address": truncated,
                "native_receiving_address": recipient.address,
                "claim_required": False,
            },
        )
        app_ctx.output.message(
            "EVM deposits are credited directly to this native account; no claim is needed"
        )
        return
    enabled = app_ctx.run(lambda client: client._substrate.constant("HashedAccounts", "Enabled"))
    if enabled is True:
        app_ctx.output.detail(
            f"EVM deposit address for {app_ctx.wallet_name}",
            {
                "coldkey": coldkey,
                "evm_deposit_address": truncated,
                "claim_required": True,
                "native_mirror_funding": "unsupported; send from an EVM wallet to the H160 address",
            },
        )
        return
    app_ctx.output.detail(
        f"EVM deposit address for {app_ctx.wallet_name}",
        {
            "coldkey": coldkey,
            "evm deposit address": truncated,
            "its ss58 mirror": recipient.account,
        },
        json_fields={
            "coldkey": coldkey,
            "evm_deposit_address": truncated,
            "mirror_ss58": recipient.account,
        },
    )
    app_ctx.output.message(
        "send TAO from the EVM side to the deposit address, then run "
        "`btcli evm claim-deposit --amount-tao <n>` to pull it into the coldkey"
    )
