"""Staking sugar for ``btcli evm stake``."""

from __future__ import annotations

from typing import Optional

import typer

from ....balance import Balance
from ....evm import precompiles as evm_precompiles
from ...context import ctx_of
from ...globals import with_globals, with_tx_globals
from ._shared import (
    EVM_KEY_HELP,
    HOTKEY_OPTION_HELP,
    RPC_URL_OPTION,
    _key_info,
    _rpc,
    _run_evm,
    _submit_evm_tx,
    stake_app,
)


def _staking_call(function: str, args: list) -> str:
    precompile = evm_precompiles.get_precompile("staking-v2")
    return evm_precompiles.encode_call(precompile.function(function), args)


@stake_app.command("add")
@with_tx_globals
def stake_add(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to stake on."),
    amount_tao: str = typer.Option(..., "--amount-tao", help="TAO to stake."),
    hotkey: Optional[str] = typer.Option(None, "--hotkey", help=HOTKEY_OPTION_HELP),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Stake TAO to a hotkey from the EVM key's balance (staking-v2 addStake).

    The precompile dispatches add_stake with the EVM key's ss58 mirror as the
    coldkey, so the stake belongs to the EVM account. The precompile's amount
    parameter is in rao — this command converts from TAO, no 1e9-vs-1e18
    arithmetic required.
    """
    app_ctx = ctx_of(ctx)
    hotkey_ss58 = app_ctx.resolve_address("hotkey_ss58", hotkey)
    assert hotkey_ss58 is not None
    amount = Balance.from_tao(amount_tao)
    data = _staking_call("addStake", [hotkey_ss58, amount.rao, netuid])
    _submit_evm_tx(
        app_ctx,
        key,
        evm_precompiles.get_precompile("staking-v2").address,
        data=data,
        summary=f"stake {amount} from the EVM key to {hotkey_ss58} on netuid {netuid}",
        rpc_url=rpc_url,
    )


@stake_app.command("remove")
@with_tx_globals
def stake_remove(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to unstake from."),
    amount_alpha: str = typer.Option(
        ..., "--amount-alpha", help="Alpha to unstake (the subnet's own currency)."
    ),
    hotkey: Optional[str] = typer.Option(None, "--hotkey", help=HOTKEY_OPTION_HELP),
    key: Optional[str] = typer.Option(None, "--evm-key", help=EVM_KEY_HELP),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Unstake from a hotkey back to the EVM key (staking-v2 removeStake).

    Note the unit: stake is held as the subnet's alpha, so the amount here is
    alpha, not TAO (`btcli evm stake show` prints the current position).
    """
    app_ctx = ctx_of(ctx)
    hotkey_ss58 = app_ctx.resolve_address("hotkey_ss58", hotkey)
    assert hotkey_ss58 is not None
    amount = Balance.from_alpha(amount_alpha, netuid)
    data = _staking_call("removeStake", [hotkey_ss58, amount.rao, netuid])
    _submit_evm_tx(
        app_ctx,
        key,
        evm_precompiles.get_precompile("staking-v2").address,
        data=data,
        summary=f"unstake {amount} from {hotkey_ss58} on netuid {netuid} to the EVM key",
        rpc_url=rpc_url,
    )


@stake_app.command("show")
@with_globals
def stake_show(
    ctx: typer.Context,
    netuid: int = typer.Option(..., "--netuid", help="Subnet to inspect."),
    hotkey: Optional[str] = typer.Option(None, "--hotkey", help=HOTKEY_OPTION_HELP),
    key: Optional[str] = typer.Option(
        None, "--evm-key", help=f"{EVM_KEY_HELP} Its ss58 mirror is the stake's coldkey."
    ),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """The EVM key's stake position with a hotkey on a subnet (view call, free)."""
    app_ctx = ctx_of(ctx)
    info = _key_info(app_ctx, key)
    hotkey_ss58 = app_ctx.resolve_address("hotkey_ss58", hotkey)
    assert hotkey_ss58 is not None
    precompile = evm_precompiles.get_precompile("staking-v2")
    fn_abi = precompile.function("getStake")
    data = evm_precompiles.encode_call(fn_abi, [hotkey_ss58, info.ss58_mirror, netuid])
    _network, rpc = _rpc(app_ctx, rpc_url)
    raw = _run_evm(app_ctx, lambda: rpc.eth_call({"to": precompile.address, "data": data}))
    (rao,) = evm_precompiles.decode_result(fn_abi, raw)
    position = Balance.from_rao(int(rao), netuid)
    app_ctx.output.detail(
        f"EVM stake on netuid {netuid}",
        {
            "evm key": f"{info.name} ({info.address})",
            "coldkey (mirror)": info.ss58_mirror,
            "hotkey": hotkey_ss58,
            "stake": str(position),
        },
        json_fields={
            "evm_key": info.name,
            "address": info.address,
            "coldkey_mirror": info.ss58_mirror,
            "hotkey": hotkey_ss58,
            "netuid": netuid,
            "stake_rao": int(rao),
        },
    )
