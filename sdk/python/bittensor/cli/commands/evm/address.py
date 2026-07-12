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
    """The ss58 mirror of an EVM address — where its native-side balance lives.

    Transfer TAO to the mirror (from btcli, an exchange, or any substrate
    wallet) and it appears as the EVM account's balance. Computed as
    ss58(blake2_256("evm:" ++ address)).
    """
    app_ctx = ctx_of(ctx)
    h160 = _address_of(app_ctx, address, param="ADDRESS")
    app_ctx.output.detail(
        None,
        {"address": h160, "ss58 mirror": evm_addresses.h160_to_ss58(h160)},
        json_fields={"address": h160, "ss58_mirror": evm_addresses.h160_to_ss58(h160)},
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
    truncated = evm_addresses.ss58_to_h160_truncated(coldkey)
    app_ctx.output.detail(
        f"EVM deposit address for {app_ctx.wallet_name}",
        {
            "coldkey": coldkey,
            "evm deposit address": truncated,
            "its ss58 mirror": evm_addresses.h160_to_ss58(truncated),
        },
        json_fields={
            "coldkey": coldkey,
            "evm_deposit_address": truncated,
            "mirror_ss58": evm_addresses.h160_to_ss58(truncated),
        },
    )
    app_ctx.output.message(
        "send TAO from the EVM side to the deposit address, then run "
        "`btcli evm claim-deposit --amount-tao <n>` to pull it into the coldkey"
    )
