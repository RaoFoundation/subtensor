"""Setup and diagnosis commands for ``btcli evm``."""

from __future__ import annotations

from typing import Any, Optional

import typer
from rich.text import Text

from .... import calls
from ....evm import keys as evm_keys
from ....evm import networks as evm_networks
from ....evm import rpc as evm_rpc
from ...context import ctx_of
from ...globals import with_globals, with_tx_globals
from ._shared import (
    EVM_KEY_HELP,
    PANEL_SETUP,
    RPC_URL_OPTION,
    _key_info,
    _rpc,
    _run_evm,
)


def register(app: typer.Typer) -> None:
    app.command("networks", rich_help_panel=PANEL_SETUP)(networks)
    app.command("config", rich_help_panel=PANEL_SETUP)(config)
    app.command("doctor", rich_help_panel=PANEL_SETUP)(doctor)
    app.command("setup-localnet", rich_help_panel=PANEL_SETUP)(setup_localnet)


@with_globals
def networks(ctx: typer.Context):
    """EVM connectivity per network: chain ID, RPC URL, currency."""
    app_ctx = ctx_of(ctx)
    rows = [
        [
            n.name,
            str(n.chain_id) if n.chain_id else "42 at genesis (setup-localnet: 945)",
            n.rpc_url,
            n.currency_symbol,
        ]
        for n in evm_networks.EVM_NETWORKS.values()
    ]
    app_ctx.output.table(
        "Bittensor EVM networks",
        ["network", "chain id", "rpc url", "currency"],
        rows,
        records=[
            {
                "network": n.name,
                "chain_id": n.chain_id,
                "rpc_url": n.rpc_url,
                "currency": n.currency_symbol,
            }
            for n in evm_networks.EVM_NETWORKS.values()
        ],
    )
    app_ctx.output.message(
        "note: EVM values use 18 decimals (1 TAO = 1e18) while native TAO uses "
        "9 (1 TAO = 1e9 rao) — MetaMask balances are the same funds, different exponent"
    )


@with_globals
def config(
    ctx: typer.Context,
    format: str = typer.Option(
        "metamask", "--format", help="Tool to emit config for: metamask, hardhat, or remix."
    ),
):
    """Ready-to-paste EVM tool configuration for the configured network."""
    app_ctx = ctx_of(ctx)
    try:
        network = evm_networks.evm_network(app_ctx.network)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    if format == "metamask":
        display_name = "Subtensor" if network.name == "finney" else f"Subtensor {network.name}"
        app_ctx.output.detail(
            "MetaMask network settings (Add network > Add manually)",
            {
                "Network name": display_name,
                "RPC URL": network.rpc_url,
                "Chain ID": network.chain_id
                or "42 at genesis; run `btcli evm doctor` for the live value "
                "(`setup-localnet` sets 945)",
                "Currency symbol": network.currency_symbol,
            },
            json_fields={
                "network_name": display_name,
                "rpc_url": network.rpc_url,
                "chain_id": network.chain_id,
                "currency_symbol": network.currency_symbol,
            },
        )
        app_ctx.output.message(
            "MetaMask assumes 18 decimals; displayed amounts are the true balance, "
            "just with 1e18 per TAO instead of the native 1e9 rao"
        )
    elif format == "hardhat":
        snippet = (
            "// hardhat.config.js — subtensor EVM\n"
            "// Deployer key, either of:\n"
            "//   export ETH_PRIVATE_KEY=$(btcli evm key export --private-key)\n"
            "//   export ETH_KEYSTORE=./key.json ETH_KEYSTORE_PASSWORD=…   "
            "(btcli evm key export --out key.json; no raw key leaves the file)\n"
            'const { Wallet } = require("ethers");\n'
            'const fs = require("fs");\n'
            "\n"
            "const deployerKey =\n"
            "  process.env.ETH_PRIVATE_KEY ??\n"
            "  (process.env.ETH_KEYSTORE\n"
            "    ? Wallet.fromEncryptedJsonSync(\n"
            '        fs.readFileSync(process.env.ETH_KEYSTORE, "utf8"),\n'
            "        process.env.ETH_KEYSTORE_PASSWORD\n"
            "      ).privateKey\n"
            "    : undefined);\n"
            "\n"
            "module.exports = {\n"
            '  solidity: { version: "0.8.24", settings: { evmVersion: "cancun" } },\n'
            "  networks: {\n"
            f'    subtensor: {{\n      url: "{network.rpc_url}",\n'
            "      accounts: deployerKey ? [deployerKey] : [],\n    },\n"
            "  },\n"
            "  mocha: { timeout: 300000 }, // ~12s blocks: default timeouts are too short\n"
            "};"
        )
        app_ctx.output.value(snippet if app_ctx.output.json_mode else Text(snippet))
    elif format == "remix":
        app_ctx.output.detail(
            "Remix IDE settings for subtensor EVM",
            {
                "Compiler": "0.8.24 (or lower)",
                "EVM version": "cancun",
                "Environment": "Injected Provider - MetaMask (connected to the network above)",
                "RPC URL": network.rpc_url,
            },
        )
    else:
        app_ctx.output.error(
            f"unknown format {format!r}", help="expected one of: metamask, hardhat, remix"
        )
        raise typer.Exit(2)


@with_globals
def doctor(
    ctx: typer.Context,
    key: Optional[str] = typer.Option(
        None, "--evm-key", help=f"{EVM_KEY_HELP} Also checks its balance and nonce."
    ),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Probe the EVM endpoint: reachability, chain ID, gas price, key balance."""
    app_ctx = ctx_of(ctx)
    network, rpc = _rpc(app_ctx, rpc_url)
    fields: dict[str, Any] = {"rpc url": network.rpc_url}
    try:
        fields["block number"] = rpc.block_number()
    except (ConnectionError, TimeoutError, evm_rpc.EvmRpcError) as error:
        app_ctx.output.error(
            f"EVM RPC unreachable at {network.rpc_url}: {error}",
            help="check the endpoint; on localnet, start the chain with ./scripts/localnet.sh",
        )
        raise typer.Exit(1)
    try:
        chain_id = rpc.chain_id()
        fields["chain id"] = chain_id
        if network.chain_id is not None and chain_id != network.chain_id:
            app_ctx.output.error(
                f"chain ID mismatch: node reports {chain_id}, expected {network.chain_id} "
                f"for {network.name}",
                help="MetaMask and signed transactions will reject the wrong chain ID",
            )
    except evm_rpc.EvmRpcError as error:
        fields["chain id"] = f"error: {error}"
        app_ctx.output.error(
            "chain ID not set on this node",
            help="on a fresh localnet, set it with the AdminUtils.sudo_set_evm_chain_id "
            "sudo extrinsic (945 to simulate testnet, 964 mainnet)",
        )
    fields["gas price (wei)"] = _run_evm(app_ctx, rpc.gas_price)
    stored = evm_keys.list_evm_keys(app_ctx.wallet_name, app_ctx.wallet_path)
    if key is not None or stored:
        info = (
            _key_info(app_ctx, key)
            if key is not None
            else next((k for k in stored if k.name == "default"), stored[0])
        )
        wei = _run_evm(app_ctx, lambda: rpc.get_balance_wei(info.address))
        fields[f"key {info.name} ({info.address})"] = str(evm_rpc.wei_to_balance(wei))
        fields["nonce"] = _run_evm(app_ctx, lambda: rpc.get_nonce(info.address))
        if wei == 0:
            app_ctx.output.message(
                f"key {info.name} has no balance — fund it with `btcli evm fund` "
                f"(or transfer TAO to its mirror {info.ss58_mirror})"
            )
    app_ctx.output.detail(f"EVM endpoint: {network.name}", fields)


@with_tx_globals
def setup_localnet(
    ctx: typer.Context,
    chain_id: int = typer.Option(
        945, "--chain-id", help="EVM chain ID to set (945 mimics testnet, 964 mainnet)."
    ),
    rpc_url: Optional[str] = RPC_URL_OPTION,
):
    """Make a fresh localnet EVM-ready: set the chain ID, disable the whitelist.

    A new localnet has no EVM chain ID (MetaMask and signed transactions
    reject it) and blocks contract deployment behind a whitelist. This
    command submits the two sudo extrinsics that fix both, signed by the
    configured wallet — on a localnet that's Alice, the dev sudo key.
    """
    app_ctx = ctx_of(ctx)
    if app_ctx.network in ("finney", "test"):
        app_ctx.output.error(
            f"refusing to run against {app_ctx.network} — this command is for localnets",
            help="pass --network local (or a ws:// endpoint) and a sudo-holding wallet",
        )
        raise typer.Exit(2)

    _network, rpc = _rpc(app_ctx, rpc_url)
    current_chain_id: Optional[int]
    try:
        current_chain_id = rpc.chain_id()
    except (ConnectionError, TimeoutError) as error:
        app_ctx.output.error(
            f"EVM RPC unreachable: {error}",
            help="start the chain first; on a source-built localnet pass "
            "--rpc-url http://127.0.0.1:9945",
        )
        raise typer.Exit(1)
    except evm_rpc.EvmRpcError:
        current_chain_id = None

    plan = []
    if current_chain_id == chain_id:
        app_ctx.output.message(f"chain ID already {chain_id} — skipping")
    else:
        plan.append(
            (
                f"set EVM chain ID to {chain_id}"
                + (f" (currently {current_chain_id})" if current_chain_id else ""),
                calls.AdminUtils.sudo_set_evm_chain_id(chain_id=chain_id),
            )
        )
    plan.append(
        ("disable the contract deployment whitelist", calls.EVM.disable_whitelist(disabled=True))
    )

    if app_ctx.dry_run:
        app_ctx.output.detail(
            "setup-localnet plan (each submitted as Sudo.sudo, signed by the wallet coldkey)",
            {f"step {i + 1}": desc for i, (desc, _) in enumerate(plan)},
        )
        return
    app_ctx.confirm(f"submit {len(plan)} sudo extrinsic(s) with wallet {app_ctx.wallet_name!r}?")

    async def _op(client):
        results = []
        for description, call in plan:
            inner = await client.compose(call)
            result = await client.submit_call(
                calls.Sudo.sudo(call=inner), app_ctx.signer("coldkey")
            )
            results.append((description, result))
        return results

    results = app_ctx.run(_op)
    for description, result in results:
        if not app_ctx.output.result(result, description):
            raise typer.Exit(1)
    app_ctx.output.message(
        "localnet is EVM-ready — `btcli evm config --format metamask` prints the "
        "wallet settings, `btcli evm doctor` verifies"
    )
