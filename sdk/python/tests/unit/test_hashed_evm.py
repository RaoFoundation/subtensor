"""Protected EVM aliases use their complete native identity for money movement."""

from __future__ import annotations

import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from rich.text import Text
from typer.testing import CliRunner

from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.evm.addresses import (
    h160_to_ss58,
    normalize_h160,
    resolve_evm_funding_recipient,
    resolve_evm_recipient,
    ss58_to_h160_truncated,
    ss58_to_pubkey,
)
from bittensor.hashed import descriptor_value, has_receiving_setup_inputs
from bittensor.intents import EvmWithdraw, FundEvmKey
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.result import BittensorError
from bittensor.sp_core import CRYPTO_HASHED, Keypair
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate


@pytest.fixture
def protected():
    key = Keypair.create_from_seed(bytes([7]) * 32, CRYPTO_HASHED)
    alias = ss58_to_h160_truncated(key.ss58_address)
    substrate = FakeSubstrate()
    substrate.seed_constant("HashedAccounts", "Enabled", True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    substrate.seed("HashedAccounts", "EvmAliases", [alias], key.ss58_address)
    substrate.seed(
        "HashedAccounts",
        "Accounts",
        [key.ss58_address],
        {
            "descriptor": descriptor_value(bytes(key.hashed_descriptor)),
            "generation": 0,
            "commitment": bytes(key.hashed_current_commitment),
        },
    )
    return substrate, key, alias


async def test_older_chain_uses_legacy_mapping_without_alias_reads(protected):
    substrate, _, alias = protected
    substrate.seed_constant("HashedAccounts", "Enabled", None)
    substrate.query = AsyncMock(side_effect=AssertionError("alias lookup on older chain"))
    recipient = await resolve_evm_funding_recipient(substrate, alias)
    assert recipient.address == recipient.account == h160_to_ss58(alias)
    assert recipient.descriptor is None
    call = await FundEvmKey(alias, 1).build(substrate, object())
    assert call.params["dest"] == h160_to_ss58(alias)


@pytest.mark.parametrize("enabled", [True, False])
@pytest.mark.parametrize("encoding", ["ss58", "hex", "bytes", "list"])
async def test_protected_mapping_validates_and_returns_full_receiving_address(
    protected, encoding, enabled
):
    substrate, key, alias = protected
    substrate.seed_constant("HashedAccounts", "Enabled", enabled)
    raw = bytes(key.public_key)
    bound = {"ss58": key.ss58_address, "hex": "0x" + raw.hex(), "bytes": raw, "list": list(raw)}[
        encoding
    ]
    substrate.seed("HashedAccounts", "EvmAliases", [alias], bound)
    substrate.query = AsyncMock(wraps=substrate.query)
    recipient = await resolve_evm_recipient(substrate, alias)
    assert recipient.account == key.ss58_address != h160_to_ss58(alias)
    assert parse_recipient(recipient.address).descriptor == bytes(key.hashed_descriptor)
    assert recipient.genesis_hash == bytes(32)
    head = await substrate.block_hash()
    assert all(call.kwargs["block_hash"] == head for call in substrate.query.call_args_list)


@pytest.mark.parametrize("enabled", [True, False])
@pytest.mark.parametrize("failure", ["short", "wrong_prefix", "missing_record", "wrong_descriptor"])
async def test_malformed_protected_mapping_fails_closed(protected, failure, enabled):
    substrate, key, alias = protected
    substrate.seed_constant("HashedAccounts", "Enabled", enabled)
    if failure == "short":
        substrate.seed("HashedAccounts", "EvmAliases", [alias], bytes(31))
    elif failure == "wrong_prefix":
        substrate.seed("HashedAccounts", "EvmAliases", [alias], bytes(32))
    elif failure == "missing_record":
        substrate.seed("HashedAccounts", "Accounts", [key.ss58_address], None)
    else:
        other = Keypair.create_from_seed(bytes([8]) * 32, CRYPTO_HASHED)
        substrate.seed(
            "HashedAccounts",
            "Accounts",
            [key.ss58_address],
            {"descriptor": descriptor_value(bytes(other.hashed_descriptor))},
        )
    with pytest.raises(ValueError, match="invalid protected EVM alias"):
        await resolve_evm_recipient(substrate, alias)


@pytest.mark.parametrize("amount,function", [(1, "transfer_keep_alive"), ("all", "transfer_all")])
async def test_protected_funding_guards_and_credits_native_account(protected, amount, function):
    substrate, key, alias = protected
    built = await FundEvmKey(alias, amount).build(substrate, object())
    assert (built.call.module, built.call.function) == ("Utility", "batch_all")
    registration, transfer = built.call.params["calls"]
    assert (registration.module, registration.function) == ("HashedAccounts", "check_registered")
    assert registration.params["descriptor"] == descriptor_value(bytes(key.hashed_descriptor))
    assert transfer.function == function
    assert transfer.params["dest"] == key.ss58_address
    assert built.extras["hashed_registration_guards"] == [key.ss58_address]


async def test_known_alias_remains_guarded_if_registration_disappears_during_planning(protected):
    substrate, key, alias = protected
    query = substrate.query
    first = True

    async def reorg_after_read(module, item, params=None, **kwargs):
        nonlocal first
        result = await query(module, item, params, **kwargs)
        if first and (module, item) == ("HashedAccounts", "Accounts"):
            first = False
            substrate.seed(module, item, params, None)
            substrate.seed("HashedAccounts", "EvmAliases", [alias], None)
        return result

    substrate.query = reorg_after_read
    built = await FundEvmKey(alias, 1).build(substrate, object())
    registration, transfer = built.call.params["calls"]
    assert registration.function == "register"
    assert transfer.params["dest"] == key.ss58_address
    assert built.extras["hashed_registration_deposit_rao"] == 200_000_000


async def test_enabled_absent_alias_native_funding_is_blocked_before_composition(protected):
    substrate, _, alias = protected
    substrate.seed("HashedAccounts", "EvmAliases", [alias], None)
    substrate.compose = AsyncMock(side_effect=AssertionError("unsafe transfer composed"))
    with pytest.raises(ValueError, match="send from an EVM wallet"):
        await FundEvmKey(alias, 1).build(substrate, object())


@pytest.mark.parametrize("amount", [1, "all"])
async def test_protected_claim_is_blocked_before_balance_query_or_composition(protected, amount):
    substrate, key, _ = protected
    substrate.query = AsyncMock(wraps=substrate.query)
    substrate.compose = AsyncMock(side_effect=AssertionError("unnecessary claim composed"))
    with pytest.raises(BittensorError, match="already credited"):
        await EvmWithdraw(amount).build(substrate, key)
    assert all(call.args[:2] != ("System", "Account") for call in substrate.query.call_args_list)


async def test_ordinary_claim_still_reads_legacy_mirror(protected):
    substrate, _, _ = protected
    substrate.seed_constant("HashedAccounts", "Enabled", False)
    key = Keypair.create_from_seed(bytes([3]) * 32)
    alias = ss58_to_h160_truncated(key.ss58_address)
    substrate.seed("System", "Account", [h160_to_ss58(alias)], {"data": {"free": 123}})
    call = await EvmWithdraw("all").build(substrate, key)
    assert (call.module, call.function) == ("EVM", "withdraw")
    assert call.params == {"address": alias, "value": 123}


def test_raw_evm_routes_reject_complete_receiving_address(protected):
    _, key, _ = protected
    full = receiving_address(key, bytes(32))
    for route in (normalize_h160, ss58_to_pubkey):
        with pytest.raises(ValueError, match="wallet transfer"):
            route(full)


@pytest.fixture
def evm_cli(tmp_path, monkeypatch, protected):
    substrate, key, alias = protected
    for variable, filename in (
        ("BTCLI_CONFIG", "config.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_SUBNET_NAMES_CACHE", "subnets.json"),
        ("BTCLI_TOKEN_SYMBOLS_CACHE", "tokens.json"),
    ):
        monkeypatch.setenv(variable, str(tmp_path / filename))
    monkeypatch.setenv("BT_WALLET", "hashed")
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path / "wallets"))
    wallet = Wallet("hashed", path=str(tmp_path / "wallets"))
    wallet.regenerate_coldkey(
        seed=bytes([7]) * 32, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
    )
    monkeypatch.setattr(
        "bittensor.cli.context.Client", lambda network, **kw: Client(network, substrate=substrate)
    )
    return substrate, key, alias


def test_cli_protected_deposit_address_says_no_claim(evm_cli):
    _, key, alias = evm_cli
    result = CliRunner().invoke(
        app, ["--yes", "--json", "--network", "local", "evm", "deposit-address"]
    )
    assert result.exit_code == 0, result.output
    fields = json.loads(result.output)
    assert fields["evm_deposit_address"] == alias
    assert fields["claim_required"] is False
    assert parse_recipient(fields["native_receiving_address"]).account == key.ss58_address
    assert h160_to_ss58(alias) not in result.output


def test_cli_protected_mirror_is_a_full_receiving_address(evm_cli):
    _, key, alias = evm_cli
    result = CliRunner().invoke(
        app, ["--yes", "--json", "--network", "local", "evm", "mirror", alias]
    )
    assert result.exit_code == 0, result.output
    assert (
        parse_recipient(json.loads(result.output)["native_receiving_address"]).account
        == key.ss58_address
    )


def test_cli_enabled_absent_alias_does_not_export_unsafe_mirror(evm_cli):
    substrate, _, alias = evm_cli
    substrate.seed("HashedAccounts", "EvmAliases", [alias], None)
    result = CliRunner().invoke(
        app, ["--yes", "--json", "--network", "local", "evm", "mirror", alias]
    )
    assert result.exit_code != 0
    assert "send from an EVM wallet" in result.output
    assert h160_to_ss58(alias) not in result.output


@pytest.mark.parametrize("custom_rpc", [False, True])
def test_cli_balance_uses_only_matching_network_receiving_information(
    evm_cli, monkeypatch, custom_rpc
):
    _, key, alias = evm_cli
    monkeypatch.setattr(
        "bittensor.cli.commands.evm.money._rpc",
        lambda *args: (None, SimpleNamespace(get_balance_wei=lambda address: 10**18)),
    )
    args = ["--yes", "--json", "--network", "local", "evm", "balance", alias]
    if custom_rpc:
        args += ["--rpc-url", "https://different-chain.invalid"]
        monkeypatch.setattr(
            "bittensor.cli.context.AppContext.run",
            lambda *args: pytest.fail("lookup against unrelated native chain"),
        )
    result = CliRunner().invoke(app, args)
    assert result.exit_code == 0, result.output
    fields = json.loads(result.output)
    assert fields["balance_wei"] == 10**18
    assert "ss58_mirror" not in fields
    if custom_rpc:
        assert "native_receiving_address" not in fields
    else:
        assert parse_recipient(fields["native_receiving_address"]).account == key.ss58_address


@pytest.mark.parametrize("enabled", [False, True])
@pytest.mark.parametrize("hashed_hotkey", [False, True])
def test_cli_stake_show_reads_the_runtime_mapped_coldkey(
    evm_cli, monkeypatch, enabled, hashed_hotkey
):
    substrate, key, alias = evm_cli
    substrate.seed_constant("HashedAccounts", "Enabled", enabled)
    info = SimpleNamespace(name="default", address=alias, ss58_mirror=h160_to_ss58(alias))
    monkeypatch.setattr("bittensor.cli.commands.evm.stake._key_info", lambda *args: info)
    requests = []

    def eth_call(params):
        requests.append(params)
        return "0x" + (123).to_bytes(32, "big").hex()

    monkeypatch.setattr(
        "bittensor.cli.commands.evm.stake._rpc",
        lambda *args: (None, SimpleNamespace(eth_call=eth_call)),
    )
    hotkey_key = (
        Keypair.create_from_seed(bytes([5]) * 32, CRYPTO_HASHED)
        if hashed_hotkey
        else Keypair.create_from_seed(bytes([5]) * 32)
    )
    hotkey = receiving_address(hotkey_key, bytes(32))
    result = CliRunner().invoke(
        app,
        [
            "--yes",
            "--json",
            "--network",
            "local",
            "evm",
            "stake",
            "show",
            "--netuid",
            "1",
            "--hotkey",
            hotkey,
        ],
    )
    assert result.exit_code == 0, result.output
    data = bytes.fromhex(requests[0]["data"][2:])
    account = key.ss58_address
    assert data[4:36] == bytes.fromhex(ss58_to_pubkey(hotkey_key.ss58_address)[2:])
    assert data[36:68] == bytes.fromhex(ss58_to_pubkey(account)[2:])
    fields = json.loads(result.output)
    assert fields["stake_rao"] == 123
    assert fields["hotkey"] == hotkey
    assert parse_recipient(fields["coldkey_address"]).account == account
    assert "coldkey_mirror" not in fields


def test_cli_stake_show_rejects_foreign_hotkey_before_evm_rpc(evm_cli, monkeypatch):
    substrate, key, _ = evm_cli
    substrate.query = AsyncMock(side_effect=AssertionError("lookup before network validation"))
    monkeypatch.setattr(
        "bittensor.cli.commands.evm.stake._rpc",
        lambda *args: pytest.fail("EVM RPC opened for a foreign-chain hotkey"),
    )
    hotkey = receiving_address(key, bytes([17]) * 32)
    result = CliRunner().invoke(
        app,
        [
            "--yes",
            "--json",
            "--network",
            "local",
            "evm",
            "stake",
            "show",
            "--netuid",
            "1",
            "--hotkey",
            hotkey,
        ],
    )
    assert result.exit_code != 0
    assert "different network" in result.output
    substrate.query.assert_not_awaited()


@pytest.mark.parametrize("styled_output", [False, True])
def test_cli_stake_show_rejects_unverified_rpc_mapping(evm_cli, monkeypatch, styled_output):
    monkeypatch.delenv("NO_COLOR", raising=False)
    monkeypatch.setenv("TERM", "xterm")
    monkeypatch.setattr("typer.rich_utils.FORCE_TERMINAL", styled_output)
    monkeypatch.setattr(
        "bittensor.cli.context.AppContext.run",
        lambda *args: pytest.fail("lookup against unrelated native chain"),
    )
    result = CliRunner().invoke(
        app,
        [
            "--yes",
            "--json",
            "--network",
            "local",
            "evm",
            "stake",
            "show",
            "--netuid",
            "1",
            "--rpc-url",
            "https://different-chain.invalid",
        ],
    )
    assert result.exit_code != 0
    if styled_output:
        assert "\x1b[" in result.output
    assert "select the chain with --network" in Text.from_ansi(result.output).plain


def test_cli_doctor_recommends_evm_receive_route_without_legacy_mirror(evm_cli, monkeypatch):
    _, _, alias = evm_cli
    info = SimpleNamespace(name="default", address=alias, ss58_mirror=h160_to_ss58(alias))
    monkeypatch.setattr("bittensor.evm.keys.list_evm_keys", lambda *args: [info])
    rpc = SimpleNamespace(
        block_number=lambda: 1,
        chain_id=lambda: 42,
        gas_price=lambda: 1,
        get_balance_wei=lambda address: 0,
        get_nonce=lambda address: 0,
    )
    network = SimpleNamespace(name="local", rpc_url="http://local.invalid", chain_id=42)
    monkeypatch.setattr("bittensor.cli.commands.evm.setup._rpc", lambda *args: (network, rpc))
    messages = []
    monkeypatch.setattr(
        "bittensor.cli.output.Output.message", lambda self, text: messages.append(text)
    )
    result = CliRunner().invoke(app, ["--yes", "--json", "--network", "local", "evm", "doctor"])
    assert result.exit_code == 0, result.output
    advice = " ".join(messages)
    assert alias in advice and "from an EVM wallet on this network" in advice
    assert h160_to_ss58(alias) not in advice


@pytest.mark.parametrize("amount", [1, "all"])
async def test_disabled_existing_alias_cannot_fund_abandoned_mirror(protected, amount):
    substrate, key, alias = protected
    substrate.seed_constant("HashedAccounts", "Enabled", False)
    recipient = await resolve_evm_funding_recipient(substrate, alias)
    assert recipient.account == key.ss58_address
    assert recipient.descriptor == bytes(key.hashed_descriptor)
    substrate.compose = AsyncMock(side_effect=AssertionError("unsafe funding composed"))
    with pytest.raises(
        ValueError, match="hashed receiving addresses are not supported on this chain"
    ):
        await FundEvmKey(alias, amount).build(substrate, object())


async def test_disabled_unbound_alias_keeps_legacy_funding(protected):
    substrate, _, alias = protected
    substrate.seed_constant("HashedAccounts", "Enabled", False)
    substrate.seed("HashedAccounts", "EvmAliases", [alias], None)
    call = await FundEvmKey(alias, 1).build(substrate, object())
    assert call.params["dest"] == h160_to_ss58(alias)
    assert not await has_receiving_setup_inputs(substrate, object(), FundEvmKey(alias, 1))
