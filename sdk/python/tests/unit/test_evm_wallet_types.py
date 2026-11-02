"""Ordinary EVM commands preserve protected identity checks before ABI encoding."""

from __future__ import annotations

import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
import typer
from typer.testing import CliRunner

from bittensor import config
from bittensor.cli.commands.evm import money, precompiles_cmd, stake
from bittensor.cli.commands.evm._shared import _native_arguments
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.client import Client
from bittensor.evm.precompiles import encode_call, get_precompile
from bittensor.hashed import descriptor_storage, descriptor_value
from bittensor.receiving import account_for_registered_write, receiving_address
from bittensor.sp_core import Keypair, requires_registration
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.receiving import descriptor_receiving_address


@pytest.fixture(
    params=[0, 1, 4, 6, 7, 8], ids=["ed", "sr", "hashed", "hashed-ed", "hashed-ms", "ms"]
)
def wallet_case(request, tmp_path, monkeypatch):
    for variable in ("BTCLI_CONFIG", "BTCLI_ADDRESSES_PATH"):
        monkeypatch.setenv(variable, str(tmp_path / variable))
    path = tmp_path / "wallets"
    wallet = Wallet("recipient", path=str(path))
    for role in ("coldkey", "hotkey"):
        getattr(wallet, f"regenerate_{role}")(
            seed=bytes([11 if role == "coldkey" else 12]) * 32,
            crypto_type=request.param,
            use_password=False,
            suppress=True,
        )
    substrate = FakeSubstrate()
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    for role in ("coldkey", "hotkey"):
        key = getattr(wallet, role)
        if requires_registration(key):
            substrate.seed(
                "HashedAccounts",
                descriptor_storage(bytes(key.hashed_descriptor)),
                [key.ss58_address],
                {
                    "descriptor": descriptor_value(bytes(key.hashed_descriptor)),
                },
            )
    substrate.query = AsyncMock(wraps=substrate.query)
    monkeypatch.setattr(
        "bittensor.cli.context.Client", lambda network, **kw: Client(network, substrate=substrate)
    )
    captured = []
    monkeypatch.setattr(
        precompiles_cmd, "_key_info", lambda *_: SimpleNamespace(address="0x" + "11" * 20)
    )
    for module in (stake, money, precompiles_cmd):
        monkeypatch.setattr(module, "_submit_evm_tx", lambda *a, **kw: captured.append(kw))
    prefix = [
        "--yes",
        "--json",
        "--network",
        "local",
        "--wallet",
        "recipient",
        "--wallet-path",
        str(path),
    ]
    return wallet, substrate, captured, prefix


COMMANDS = {
    "add": (["evm", "stake", "add", "--netuid", "1", "--amount-tao", "1"], "hotkey"),
    "remove": (["evm", "stake", "remove", "--netuid", "1", "--amount-alpha", "1"], "hotkey"),
    "send": (["evm", "send-to-ss58", "--amount-tao", "1"], "coldkey"),
}


@pytest.mark.parametrize("command", COMMANDS)
@pytest.mark.parametrize("reference", ["default", "name", "contact", "address", "independent"])
def test_evm_commands_encode_all_wallet_types(wallet_case, command, reference):
    wallet, substrate, captured, prefix = wallet_case
    args, role = COMMANDS[command]
    key = getattr(wallet, role)
    address = receiving_address(key)
    config.add_address({"name": "friend", "address": address})
    references = {
        "name": "recipient/default" if role == "hotkey" else "recipient",
        "contact": "friend",
        "address": address,
        "independent": (
            descriptor_receiving_address(key) if requires_registration(key) else address
        ),
    }
    if reference != "default":
        args = [*args, "--hotkey" if role == "hotkey" else "--to", references[reference]]
    result = CliRunner().invoke(app, [*prefix, *args])
    assert result.exit_code == 0, result.output or result.exception
    fn = get_precompile("balance-transfer" if command == "send" else "staking-v2").function(
        {"add": "addStake", "remove": "removeStake", "send": "transfer"}[command]
    )
    expected = [key.ss58_address] if command == "send" else [key.ss58_address, 10**9, 1]
    assert captured[0]["data"] == encode_call(fn, expected)
    queries = [
        c
        for c in substrate.query.call_args_list
        if c.args[:2]
        == ("HashedAccounts", "MlDsaAccounts" if key.crypto_type in (7, 8) else "Accounts")
    ]
    if requires_registration(key):
        assert len(queries) == 1
        assert queries[0].kwargs["block_hash"] == f"0x{substrate.finalized:064x}"
    else:
        assert not queries


@pytest.mark.parametrize("command", COMMANDS)
@pytest.mark.parametrize(
    "wallet_case", [4, 6, 7], indirect=True, ids=["hashed", "hashed-ed", "hashed-ms"]
)
@pytest.mark.parametrize("failure", ["unfinalized", "mismatch", "rpc_override", "no_finality"])
def test_protected_commands_fail_before_evm_submission(wallet_case, command, failure):
    wallet, substrate, captured, prefix = wallet_case
    args, role = COMMANDS[command]
    key = getattr(wallet, role)
    address = receiving_address(key)
    expected = "not finalized"
    if failure == "unfinalized":
        original = substrate.query

        async def query(*a, **kw):
            if a[:2] == (
                "HashedAccounts",
                "MlDsaAccounts" if key.crypto_type in (7, 8) else "Accounts",
            ) and kw.get("block_hash"):
                return None
            return await original(*a, **kw)

        substrate.query = AsyncMock(side_effect=query)
    elif failure == "mismatch":
        other = Keypair.create_from_seed(bytes([19]) * 32, key.crypto_type)
        substrate.seed(
            "HashedAccounts",
            descriptor_storage(bytes(key.hashed_descriptor)),
            [key.ss58_address],
            {
                "descriptor": descriptor_value(bytes(other.hashed_descriptor)),
            },
        )
        expected = "does not match"
    elif failure == "rpc_override":
        args = [*args, "--rpc-url", "http://127.0.0.1:9999"]
        expected = "RPC override"
    else:
        original_hash = substrate.block_hash

        async def block_hash(number=None):
            return None if number == substrate.finalized else await original_hash(number)

        substrate.block_hash = block_hash
        expected = "could not verify finalized"
    args = [*args, "--hotkey" if role == "hotkey" else "--to", address]
    result = CliRunner().invoke(app, [*prefix, *args])
    assert result.exit_code != 0
    assert expected in result.output, result.output or result.exception
    assert not captured


async def test_registered_write_does_not_depend_on_registration_being_enabled(wallet_case):
    wallet, substrate, _, _ = wallet_case
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], False)
    address = receiving_address(wallet.coldkey)
    assert await account_for_registered_write(substrate, address) == wallet.coldkey.ss58_address


def test_generic_precompile_call_resolves_receiving_address(wallet_case):
    wallet, _, captured, prefix = wallet_case
    address = receiving_address(wallet.coldkey)
    result = CliRunner().invoke(
        app, [*prefix, "evm", "call", "balance-transfer", "transfer", address, "--value-tao", "1"]
    )
    assert result.exit_code == 0, result.output or result.exception
    assert captured[0]["data"] == encode_call(
        get_precompile("balance-transfer").function("transfer"), [wallet.coldkey.ss58_address]
    )


def test_generic_views_resolve_descriptor_addresses_without_registration(wallet_case):
    wallet, substrate, _, _ = wallet_case
    address = receiving_address(wallet.hotkey)
    ctx = AppContext(
        "local", "recipient", "default", str(wallet.path), True, False, Output(json_mode=True)
    )
    fn = {"inputs": [{"type": "bytes32[]"}], "stateMutability": "view"}
    substrate.seed(
        "HashedAccounts",
        "MlDsaAccounts" if wallet.hotkey.crypto_type in (7, 8) else "Accounts",
        [wallet.hotkey.ss58_address],
        None,
    )
    assert _native_arguments(ctx, fn, [json.dumps([address])], None) == [
        [wallet.hotkey.ss58_address]
    ]
    if requires_registration(wallet.hotkey):
        assert _native_arguments(
            ctx,
            fn,
            [json.dumps([descriptor_receiving_address(wallet.hotkey)])],
            "http://127.0.0.1:9999",
        ) == [[wallet.hotkey.ss58_address]]
    assert not substrate.query.call_args_list


@pytest.mark.parametrize(
    "wallet_case", [4, 6, 7], indirect=True, ids=["hashed", "hashed-ed", "hashed-ms"]
)
def test_generic_writes_require_registration_for_every_array_member(wallet_case):
    wallet, substrate, _, _ = wallet_case
    ctx = AppContext(
        "local", "recipient", "default", str(wallet.path), True, False, Output(json_mode=True)
    )
    fn = {"inputs": [{"type": "bytes32[]"}], "stateMutability": "nonpayable"}
    addresses = [receiving_address(getattr(wallet, role)) for role in ("coldkey", "hotkey")]
    assert _native_arguments(ctx, fn, [json.dumps(addresses)], None) == [
        [wallet.coldkey.ss58_address, wallet.hotkey.ss58_address]
    ]
    substrate.seed(
        "HashedAccounts",
        "MlDsaAccounts" if wallet.hotkey.crypto_type in (7, 8) else "Accounts",
        [wallet.hotkey.ss58_address],
        None,
    )
    with pytest.raises(typer.Exit):
        _native_arguments(ctx, fn, [json.dumps(addresses)], None)


@pytest.mark.parametrize(
    "wallet_case", [4, 6, 7], indirect=True, ids=["hashed", "hashed-ed", "hashed-ms"]
)
def test_generic_precompile_write_rejects_unregistered_recipient(wallet_case):
    wallet, substrate, captured, prefix = wallet_case
    address = receiving_address(wallet.coldkey)
    substrate.seed(
        "HashedAccounts",
        descriptor_storage(bytes(wallet.coldkey.hashed_descriptor)),
        [wallet.coldkey.ss58_address],
        None,
    )
    result = CliRunner().invoke(
        app, [*prefix, "evm", "call", "balance-transfer", "transfer", address, "--value-tao", "1"]
    )
    assert result.exit_code != 0
    assert "not finalized" in result.output
    assert not captured


@pytest.mark.parametrize("wallet_case", [8], indirect=True)
def test_fixed_ss58_evm_write_needs_no_registration_or_runtime_query(wallet_case):
    wallet, substrate, _, _ = wallet_case
    ctx = AppContext(
        "local", "recipient", "default", str(wallet.path), True, False, Output(json_mode=True)
    )
    fn = {"inputs": [{"type": "bytes32[]"}], "stateMutability": "nonpayable"}
    assert _native_arguments(
        ctx, fn, [json.dumps([wallet.hotkey.ss58_address])], "http://127.0.0.1:9999"
    ) == [[wallet.hotkey.ss58_address]]
    substrate.query.assert_not_called()


def test_pubkey_command_accepts_complete_receiving_address(wallet_case):
    wallet, _, _, prefix = wallet_case
    address = receiving_address(wallet.coldkey)
    result = CliRunner().invoke(app, [*prefix, "evm", "pubkey", address])
    assert result.exit_code == 0, result.output or result.exception
    assert "0x" + bytes(wallet.coldkey.public_key).hex() in result.output
