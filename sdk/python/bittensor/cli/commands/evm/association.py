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
    With ``--proxy-for`` (a ``Validate`` delegate), the proof covers that real
    hotkey instead of the local one.
    """
    app_ctx = ctx_of(ctx)
    info = _key_info(app_ctx, key)
    # The proof must name the hotkey the call dispatches as.
    real = app_ctx.resolve_dispatch_proxy()
    hotkey = real or app_ctx.resolve_address("hotkey_ss58", None)
    assert hotkey is not None
    summary = f"associate EVM key {info.address} with the hotkey on netuid {netuid}"

    # Unlock and build the proof first — the intent cannot be composed
    # without the signature. submit then shows the universal review card.
    block_number = app_ctx.run(lambda client: client.block())
    account = _unlock(app_ctx, key)
    signature, block_number = evm_transactions.association_proof(account, hotkey, block_number)
    app_ctx.submit(
        AssociateEvmKey(
            netuid=netuid,
            evm_key=info.address,
            block_number=block_number,
            signature=signature,
        ),
        proxy_for=real or "self",
        summary=summary,
    )
