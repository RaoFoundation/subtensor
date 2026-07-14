"""Precompile and contract-call commands for ``btcli evm``."""

from __future__ import annotations

from typing import Any, Optional

import typer

from ....evm import addresses as evm_addresses
from ....evm import contracts as evm_contracts
from ....evm import precompiles as evm_precompiles
from ...context import ctx_of
from ...globals import with_globals, with_tx_globals
from ._shared import (
    EVM_KEY_HELP,
    PANEL_CHAIN,
    RPC_URL_OPTION,
    _key_info,
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


def _call_target(
    app_ctx, name: str, abi_path: Optional[str]
) -> tuple[str, str, "list[dict]", "str | None"]:
    """Resolve the call target: (label, address, function ABIs, precompile name).

    ``name`` is a precompile name from the catalog, or a 0x contract address
    combined with ``--abi`` (a Hardhat/Foundry artifact or bare ABI array).
    """
    if name.startswith("0x") or evm_addresses.is_h160("0x" + name):
        try:
            address = evm_addresses.normalize_h160(name)
        except ValueError as error:
            app_ctx.output.error(str(error))
            raise typer.Exit(2)
        if not abi_path:
            app_ctx.output.error(
                f"{address} is a contract address — pass its interface with --abi",
                help="accepts a Hardhat/Foundry artifact JSON or a bare ABI array",
            )
            raise typer.Exit(2)
        try:
            artifact = evm_contracts.load_artifact(abi_path)
        except (OSError, ValueError) as error:
            app_ctx.output.error(f"cannot load ABI from {abi_path!r}: {error}")
            raise typer.Exit(2)
        if artifact.abi is None:
            app_ctx.output.error(f"{abi_path} contains bytecode but no ABI")
            raise typer.Exit(2)
        return address, address, artifact.functions(), None
    try:
        precompile = evm_precompiles.get_precompile(name)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)
    return precompile.name, precompile.address, precompile.functions(), precompile.name


def _find_function(app_ctx, functions: "list[dict]", label: str, name: str) -> dict:
    for entry in functions:
        if entry["name"] == name:
            return entry
    available = ", ".join(sorted(e["name"] for e in functions)) or "(none)"
    app_ctx.output.error(f"{label} has no function {name!r}. Available: {available}")
    raise typer.Exit(2)


@with_tx_globals
def call(
    ctx: typer.Context,
    name: str = typer.Argument(
        ...,
        help="Precompile name (see `btcli evm precompiles`) or a 0x contract "
        "address (requires --abi).",
    ),
    function: Optional[str] = typer.Argument(
        None, help="Function to call (omit to list the target's functions)."
    ),
    args: Optional[list[str]] = typer.Argument(
        None,
        help="Function arguments, in order. bytes32 key parameters accept ss58 "
        "addresses; uints accept decimal or 0x-hex.",
    ),
    abi_path: Optional[str] = typer.Option(
        None,
        "--abi",
        help="Contract interface for a 0x address target: Hardhat/Foundry "
        "artifact JSON or a bare ABI array.",
    ),
    value_tao: Optional[str] = typer.Option(
        None, "--value-tao", help="TAO to attach to a payable call (msg.value)."
    ),
    key: Optional[str] = typer.Option(
        None, "--evm-key", help=f"{EVM_KEY_HELP} Needed only for non-view functions."
    ),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Call a precompile or contract function: views free via eth_call, writes signed.

    View calls need no key, no gas, and no setup — e.g.
    `btcli evm call metagraph getUidCount 1`. Non-view calls are real EVM
    transactions signed with the stored key. Your own contracts work the same
    way: `btcli evm call 0xADDRESS fn args --abi ./artifact.json`.
    """
    app_ctx = ctx_of(ctx)
    label, address, functions, precompile_name = _call_target(app_ctx, name, abi_path)

    if function is None:
        rows = []
        for fn in sorted(functions, key=lambda f: f["name"]):
            mutability = fn.get("stateMutability", "")
            if precompile_name:
                note = evm_precompiles.function_deprecation(precompile_name, fn["name"])
                if note:
                    mutability = f"{mutability} (deprecated: {note})"
            rows.append(
                [
                    fn["name"],
                    ", ".join(f"{i['type']} {i['name']}" for i in fn["inputs"]),
                    ", ".join(o["type"] for o in fn.get("outputs", [])),
                    mutability,
                ]
            )
        app_ctx.output.table(
            f"{label} at {address}",
            ["function", "inputs", "outputs", "mutability"],
            rows,
        )
        return

    fn_abi = _find_function(app_ctx, functions, label, function)
    try:
        data = evm_precompiles.encode_call(fn_abi, list(args or []))
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2)

    if precompile_name:
        note = evm_precompiles.function_deprecation(precompile_name, function)
        if note:
            app_ctx.output.message(f"warning: {label}.{function} is deprecated — {note}")

    if fn_abi.get("stateMutability") in ("view", "pure"):
        _network, rpc = _rpc(app_ctx, rpc_url)
        raw = _run_evm(app_ctx, lambda: rpc.eth_call({"to": address, "data": data}))
        results = evm_precompiles.decode_result(fn_abi, raw)
        app_ctx.output.value(results[0] if len(results) == 1 else results)
        return

    preview_fields: dict[str, Any] = {}
    try:
        decoded = evm_precompiles.describe_arguments(fn_abi, list(args or []))
        preview_fields.update({f"arg {k}": v for k, v in decoded.items()})
    except Exception:
        pass
    if precompile_name:
        role = evm_precompiles.caller_role(precompile_name, function)
        if role:
            info = _key_info(app_ctx, key)
            preview_fields["caller mirror"] = f"{info.ss58_mirror} acts as the {role}"
        if precompile_name == "subnet" and function == "registerNetwork":
            app_ctx.output.message(
                "warning: the caller mirror becomes the subnet owner, and some owner "
                "operations (notably start-call, which activates emissions) have no "
                "precompile — they require a native coldkey signature"
            )

    wei = _tao_to_wei(app_ctx, value_tao) if value_tao else 0
    _submit_evm_tx(
        app_ctx,
        key,
        address,
        value_wei=wei,
        data=data,
        summary=f"call {label}.{function}({', '.join(args or [])})",
        rpc_url=rpc_url,
        preview_fields=preview_fields or None,
    )
