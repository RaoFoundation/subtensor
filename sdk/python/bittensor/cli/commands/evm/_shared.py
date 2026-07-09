"""Shared helpers for the ``btcli evm`` command group."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Optional

import typer

from .... import macos_password
from ....balance import Balance
from ....evm import addresses as evm_addresses
from ....evm import keys as evm_keys
from ....evm import networks as evm_networks
from ....evm import rpc as evm_rpc
from ....evm import transactions as evm_transactions
from ...context import AppContext

PANEL_KEYS = "EVM keys"
PANEL_MONEY = "Money movement"
PANEL_CHAIN = "Precompiles & chain"
PANEL_SETUP = "Setup & diagnosis"

EVM_KEY_HELP = (
    "Stored EVM key: NAME (in the configured wallet) or WALLET/NAME. "
    "Defaults to the wallet's `default` key."
)
EVM_ADDRESS_HELP = "0x-prefixed h160 address, or a stored EVM key (NAME or WALLET/NAME)."

RPC_URL_OPTION = typer.Option(
    None,
    "--rpc-url",
    help="EVM JSON-RPC endpoint (defaults to the configured network's).",
)

key_app = typer.Typer(
    no_args_is_help=True,
    help="Create and manage EVM keys (encrypted keystore V3, next to your hotkeys).",
)

stake_app = typer.Typer(
    no_args_is_help=True,
    help="Stake from an EVM key via the staking-v2 precompile (unit math handled).",
)

HOTKEY_OPTION_HELP = (
    "Validator hotkey: ss58 address, address-book name, or local hotkey name "
    "(HOTKEY or WALLET/HOTKEY). Defaults to your wallet's hotkey."
)


def _key_ref(app_ctx: AppContext, value: Optional[str]) -> tuple[str, str]:
    if not value:
        return app_ctx.wallet_name, "default"
    wallet, _, name = value.rpartition("/")
    return (wallet or app_ctx.wallet_name), name


def _key_info(app_ctx: AppContext, value: Optional[str]) -> evm_keys.EvmKeyInfo:
    wallet, name = _key_ref(app_ctx, value)
    try:
        return evm_keys.get_evm_key_info(name, wallet, app_ctx.wallet_path)
    except ValueError as error:
        app_ctx.output.error(str(error), help="create one with `btcli evm key new`")
        raise typer.Exit(1)


def _address_of(app_ctx: AppContext, value: Optional[str], *, param: str) -> str:
    if value and (value.startswith("0x") or evm_addresses.is_h160("0x" + value)):
        try:
            return evm_addresses.normalize_h160(value)
        except ValueError as error:
            app_ctx.output.error(f"invalid value for {param}: {error}")
            raise typer.Exit(2)
    return _key_info(app_ctx, value).address


def _password(app_ctx: AppContext) -> Optional[str]:
    if app_ctx.wallet_password_file:
        return Path(app_ctx.wallet_password_file).expanduser().read_text().strip()
    if app_ctx.keychain_password:
        stored = macos_password.keychain_load(app_ctx.wallet_name)
        if stored:
            return stored
    return None


def _unlock(app_ctx: AppContext, key: Optional[str]):
    wallet, name = _key_ref(app_ctx, key)
    try:
        return evm_keys.unlock_evm_key(
            name, wallet, app_ctx.wallet_path, password=_password(app_ctx)
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)


def _rpc(
    app_ctx: AppContext, rpc_url: Optional[str]
) -> tuple[evm_networks.EvmNetwork, evm_rpc.EvmRpc]:
    try:
        network = evm_networks.evm_network(rpc_url or app_ctx.network)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    return network, evm_rpc.EvmRpc(network.rpc_url)


def _tao_to_wei(app_ctx: AppContext, amount_tao: str) -> int:
    try:
        return evm_rpc.balance_to_wei(Balance.from_tao(amount_tao))
    except Exception as error:
        app_ctx.output.error(f"invalid TAO amount {amount_tao!r}: {error}")
        raise typer.Exit(2)


def _run_evm(app_ctx: AppContext, work):
    try:
        return work()
    except evm_rpc.EvmRpcError as error:
        app_ctx.output.error(
            f"EVM RPC error: {error}",
            note="subtensor reports any invalid transaction through eth_estimateGas — "
            "insufficient balance, a bad call, an unset chain ID, or an enabled "
            "deployment whitelist all surface here, not just gas problems",
        )
        raise typer.Exit(1)
    except (ConnectionError, TimeoutError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)


def _diagnose_estimate_failure(
    rpc: evm_rpc.EvmRpc,
    sender: str,
    *,
    value_wei: int,
    deploying: bool,
) -> list[str]:
    """Cheap probes that turn a catch-all estimate error into a likely cause.

    Subtensor's ``eth_estimateGas`` rejects *any* invalid transaction, so when
    it fails we check the usual suspects and report what we find. Probes that
    themselves fail are skipped — this must never mask the original error.
    """
    findings: list[str] = []
    try:
        balance = rpc.get_balance_wei(sender)
        if balance == 0:
            findings.append(
                f"sender {sender} has zero balance — fund it with `btcli evm fund` "
                "(or transfer TAO to its ss58 mirror)"
            )
        elif value_wei and balance < value_wei:
            findings.append(
                f"sender balance {evm_rpc.wei_to_balance(balance)} is less than the "
                f"attached value {evm_rpc.wei_to_balance(value_wei)}"
            )
    except Exception:
        pass
    try:
        rpc.chain_id()
    except Exception:
        findings.append(
            "the node has no EVM chain ID — on a localnet run `btcli evm setup-localnet`"
        )
    if deploying and not findings:
        findings.append(
            "if this is a localnet, contract deployment may be blocked by the "
            "whitelist — `btcli evm setup-localnet` disables it"
        )
    return findings


def _submit_evm_tx(
    app_ctx: AppContext,
    key: Optional[str],
    to: Optional[str],
    *,
    value_wei: int = 0,
    data: str = "0x",
    summary: str,
    rpc_url: Optional[str] = None,
    preview_fields: Optional[dict[str, Any]] = None,
) -> Optional[dict]:
    """Prepare, preview, confirm, sign, and submit one EVM transaction.

    ``to=None`` is contract creation. ``preview_fields`` are extra
    human-oriented rows (decoded arguments, caller role) shown with the
    dry-run preview and before the confirmation prompt.
    """
    info = _key_info(app_ctx, key)
    _network, rpc = _rpc(app_ctx, rpc_url)

    try:
        preview = evm_transactions.prepare_transaction(
            rpc, info.address, to, value_wei=value_wei, data=data
        )
    except evm_rpc.EvmRpcError as error:
        findings = _diagnose_estimate_failure(
            rpc, info.address, value_wei=value_wei, deploying=to is None
        )
        app_ctx.output.error(
            f"EVM RPC error: {error}",
            note="; ".join(findings)
            if findings
            else "subtensor reports any invalid transaction through eth_estimateGas — "
            "insufficient balance, a bad call, an unset chain ID, or an enabled "
            "deployment whitelist all surface here, not just gas problems",
        )
        raise typer.Exit(1)
    except (ConnectionError, TimeoutError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    if preview_fields and not app_ctx.output.json_mode:
        app_ctx.output.detail(None, preview_fields)
    if app_ctx.dry_run:
        app_ctx.output.detail("evm transaction preview", preview.to_dict())
        return None
    app_ctx.confirm(f"{summary} (max fee {preview.max_fee})?")
    account = _unlock(app_ctx, key)
    result = _run_evm(app_ctx, lambda: evm_transactions.send_transaction(rpc, account, preview))
    rendered = {**result, "from": info.address, "to": to or "(contract creation)"}
    if not result.get("success", True):
        app_ctx.output.error("transaction reverted", note=json.dumps(rendered))
        raise typer.Exit(1)
    app_ctx.output.detail(summary, rendered)
    return result


def _key_fields(info: evm_keys.EvmKeyInfo) -> dict[str, Any]:
    return {
        "name": info.name,
        "address": info.address,
        "ss58 mirror": info.ss58_mirror,
        "keystore": info.path,
    }


def _key_json(info: evm_keys.EvmKeyInfo) -> dict[str, Any]:
    return {
        "name": info.name,
        "address": info.address,
        "ss58_mirror": info.ss58_mirror,
        "keystore": info.path,
    }
