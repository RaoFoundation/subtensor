"""Hotkey association for ``btcli evm``."""

from __future__ import annotations

from typing import Optional

import typer

from ....evm import transactions as evm_transactions
from ....intents import AssociateEvmKey
from ...context import ctx_of
from ...globals import with_tx_globals
from ._shared import EVM_KEY_HELP, PANEL_CHAIN, _key_info, _unlock


def register(app: typer.Typer) -> None:
    app.command("associate", rich_help_panel=PANEL_CHAIN)(associate)


@with_tx_globals
def associate(
    ctx: typer.Context,
    netuid: int = typer.Option(
        ..., "--netuid", help=AssociateEvmKey.field_help("netuid") or "Subnet."
    ),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
):
    """Associate an EVM key with the wallet hotkey on a subnet, proof included.

    The chain expects an EIP-191 personal-sign signature by the EVM key over
    ``hotkey_pubkey (32 bytes) ++ keccak_256(scale(block_number))``, where the
    block number is SCALE-encoded (u64 little-endian). This command produces
    that proof from the stored key, then submits ``associate_evm_key`` signed
    by the hotkey. Block height comes from the substrate connection, not EVM RPC.
    """
    app_ctx = ctx_of(ctx)
    info = _key_info(app_ctx, key)
    hotkey = app_ctx.resolve_address("hotkey_ss58", None)
    assert hotkey is not None
    summary = f"associate EVM key {info.address} with the hotkey on netuid {netuid}"

    if app_ctx.dry_run:
        block_number = app_ctx.run(lambda client: client.block())
        account = _unlock(app_ctx, key)
        signature, block_number = evm_transactions.association_proof(account, hotkey, block_number)
        app_ctx.submit(
            AssociateEvmKey(
                netuid=netuid,
                evm_key=info.address,
                block_number=block_number,
                signature=signature,
            )
        )
        return

    app_ctx.confirm(f"{summary}?")
    block_number = app_ctx.run(lambda client: client.block())
    account = _unlock(app_ctx, key)
    signature, block_number = evm_transactions.association_proof(account, hotkey, block_number)

    previous_assume_yes = app_ctx.assume_yes
    app_ctx.assume_yes = True
    try:
        app_ctx.submit(
            AssociateEvmKey(
                netuid=netuid,
                evm_key=info.address,
                block_number=block_number,
                signature=signature,
            )
        )
    finally:
        app_ctx.assume_yes = previous_assume_yes
