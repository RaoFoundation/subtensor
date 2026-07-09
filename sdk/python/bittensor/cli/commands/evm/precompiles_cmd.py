"""Precompile commands for ``btcli evm``."""

from __future__ import annotations

from typing import Optional

import typer

from ....evm import precompiles as evm_precompiles
from ...context import ctx_of
from ...globals import with_globals, with_tx_globals
from ._shared import (
    EVM_KEY_HELP,
    PANEL_CHAIN,
    RPC_URL_OPTION,
    _rpc,
    _run_evm,
    _submit_evm_tx,
    _tao_to_wei,
)


def register(app: typer.Typer) -> None:
    app.command("precompiles", rich_help_panel=PANEL_CHAIN)(precompiles_list)
    app.command("abi", rich_help_panel=PANEL_CHAIN)(abi)
    app.command("call", rich_help_panel=PANEL_CHAIN)(call)


@with_globals
def precompiles_list(ctx: typer.Context):
    """The Bittensor precompile catalog: name, address, and what it does."""
    app_ctx = ctx_of(ctx)
    entries = sorted(evm_precompiles.PRECOMPILES.values(), key=lambda p: p.index)
    app_ctx.output.table(
        "Bittensor EVM precompiles",
        ["name", "address", "description"],
        [
            [p.name, p.address, p.description + (" (deprecated)" if p.deprecated else "")]
            for p in entries
        ],
        records=[
            {
                "name": p.name,
                "address": p.address,
                "description": p.description,
                "deprecated": p.deprecated,
            }
            for p in entries
        ],
    )


@with_globals
def abi(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Precompile name (see `btcli evm precompiles`)."),
):
    """A precompile's address and ABI JSON, ready for Hardhat/ethers/viem."""
    app_ctx = ctx_of(ctx)
    try:
        precompile = evm_precompiles.get_precompile(name)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)
    app_ctx.output.value({"address": precompile.address, "abi": precompile.abi})


@with_tx_globals
def call(
    ctx: typer.Context,
    name: str = typer.Argument(..., help="Precompile name (see `btcli evm precompiles`)."),
    function: Optional[str] = typer.Argument(
        None, help="Function to call (omit to list the precompile's functions)."
    ),
    args: Optional[list[str]] = typer.Argument(
        None,
        help="Function arguments, in order. bytes32 key parameters accept ss58 "
        "addresses; uints accept decimal or 0x-hex.",
    ),
    value_tao: Optional[str] = typer.Option(
        None, "--value-tao", help="TAO to attach to a payable call (msg.value)."
    ),
    key: Optional[str] = typer.Option(
        None, "--evm-key", help=f"{EVM_KEY_HELP} Needed only for non-view functions."
    ),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Call a precompile function: view functions free via eth_call, writes signed.

    View calls need no key, no gas, and no setup — e.g.
    `btcli evm call metagraph getUidCount 1`. Non-view calls are real EVM
    transactions signed with the stored key.
    """
    app_ctx = ctx_of(ctx)
    try:
        precompile = evm_precompiles.get_precompile(name)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)
    if function is None:
        rows = [
            [
                fn["name"],
                ", ".join(f"{i['type']} {i['name']}" for i in fn["inputs"]),
                ", ".join(o["type"] for o in fn.get("outputs", [])),
                fn.get("stateMutability", ""),
            ]
            for fn in sorted(precompile.functions(), key=lambda f: f["name"])
        ]
        app_ctx.output.table(
            f"{precompile.name} at {precompile.address}",
            ["function", "inputs", "outputs", "mutability"],
            rows,
        )
        return
    try:
        fn_abi = precompile.function(function)
        data = evm_precompiles.encode_call(fn_abi, list(args or []))
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)

    if fn_abi.get("stateMutability") in ("view", "pure"):
        _network, rpc = _rpc(app_ctx, rpc_url)
        raw = _run_evm(app_ctx, lambda: rpc.eth_call({"to": precompile.address, "data": data}))
        results = evm_precompiles.decode_result(fn_abi, raw)
        app_ctx.output.value(results[0] if len(results) == 1 else results)
        return

    wei = _tao_to_wei(app_ctx, value_tao) if value_tao else 0
    _submit_evm_tx(
        app_ctx,
        key,
        precompile.address,
        value_wei=wei,
        data=data,
        summary=f"call {precompile.name}.{function}({', '.join(args or [])})",
        rpc_url=rpc_url,
    )
