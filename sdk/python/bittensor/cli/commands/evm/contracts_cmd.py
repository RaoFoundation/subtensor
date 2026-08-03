"""Contract deployment for ``btcli evm``."""

from __future__ import annotations

from pathlib import Path
from typing import Optional

import typer

from ....evm import addresses as evm_addresses
from ....evm import contracts as evm_contracts
from ...context import ctx_of
from ...globals import evm_key_signed, with_tx_globals
from ._shared import EVM_KEY_HELP, PANEL_CHAIN, RPC_URL_OPTION, _submit_evm_tx, _tao_to_wei


def register(app: typer.Typer) -> None:
    app.command("deploy", rich_help_panel=PANEL_CHAIN)(deploy)


@with_tx_globals
@evm_key_signed
def deploy(
    ctx: typer.Context,
    artifact_path: str = typer.Argument(
        ...,
        help="Compiled contract: Hardhat/Foundry artifact JSON, or a raw .bin hex file.",
    ),
    args: Optional[list[str]] = typer.Argument(
        None, help="Constructor arguments, in order (bytes32 accepts ss58 addresses)."
    ),
    value_tao: Optional[str] = typer.Option(
        None, "--value-tao", help="TAO to send with a payable constructor."
    ),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Deploy a compiled contract from the stored EVM key.

    Reads the toolchain's own output — no Node.js required. The transaction
    follows the usual flow: `--dry-run` previews (including estimated gas and
    fee), then confirm, sign, submit, and wait for the receipt with the new
    contract's address.
    """
    app_ctx = ctx_of(ctx)
    try:
        artifact = evm_contracts.load_artifact(artifact_path)
        data = evm_contracts.encode_deploy(artifact, list(args or []))
    except (OSError, ValueError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)

    wei = _tao_to_wei(app_ctx, value_tao) if value_tao else 0
    result = _submit_evm_tx(
        app_ctx,
        key,
        None,  # contract creation
        value_wei=wei,
        data=data,
        summary=f"deploy {Path(artifact_path).name}",
        rpc_url=rpc_url,
    )
    if result and result.get("contract_address"):
        contract = result["contract_address"]
        app_ctx.output.message(
            f"contract mirror: {evm_addresses.h160_to_ss58(contract)} — this ss58 "
            "account is the contract's identity for native TAO and precompile calls"
        )
