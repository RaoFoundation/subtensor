"""Complete receiving addresses survive creation, contacts, recovery and sending."""

from __future__ import annotations

import asyncio
import json

import pytest
import typer
from typer.testing import CliRunner

from bittensor import config
from bittensor.cli.call_names import resolve_builder_params, resolve_intent_args
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.cli.stake_picker import _dest_choices
from bittensor.client import Client
from bittensor.executor import Executor
from bittensor.hashed import descriptor_value
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.settings import FINNEY_GENESIS_HASH
from bittensor.sp_core import CRYPTO_HASHED, CRYPTO_SR25519, Keypair
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.receiving import legacy_receiving_address

MNEMONIC = "bottom drive obey lake curtain smoke basket hold race lonely fit walk"
OTHER_GENESIS = "0x" + "17" * 32
runner = CliRunner()


@pytest.fixture
def wallet_path(tmp_path, monkeypatch):
    for variable, filename in (
        ("BTCLI_CONFIG", "config.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_MULTISIG_CACHE", "multisig-cache.json"),
        ("BTCLI_SUBNET_NAMES_CACHE", "subnets.json"),
        ("BTCLI_TOKEN_SYMBOLS_CACHE", "symbols.json"),
    ):
        monkeypatch.setenv(variable, str(tmp_path / filename))
    path = tmp_path / "wallets"
    monkeypatch.setenv("BT_WALLET_PATH", str(path))
    monkeypatch.setenv("BT_WALLET", "recipient")
    monkeypatch.setenv("BT_NETWORK", "finney")
    return path


def _invoke(*args):
    return runner.invoke(app, ["--yes", "--json", *args])


def _context(path):
    return AppContext(
        "finney", "recipient", "default", str(path), True, False, Output(json_mode=True)
    )


def _public(seed=1):
    return Keypair.create_from_seed(bytes([seed]) * 32, CRYPTO_HASHED).public_only()


def test_create_contact_transfer_preserves_complete_receiving_address(wallet_path, monkeypatch):
    created = _invoke("wallet", "create", "--type", "hashed", "--no-password")
    assert created.exit_code == 0, created.output
    details = json.loads(created.output)
    address = details["coldkey_address"]
    recipient = parse_recipient(address)
    wallet = Wallet("recipient", path=str(wallet_path))
    assert recipient.account == wallet.coldkeypub.ss58_address
    assert recipient.descriptor == bytes(wallet.coldkeypub.hashed_descriptor)
    assert "coldkey_ss58" not in details and "hashed_descriptor" not in details
    assert parse_recipient(details["hotkey_address"]).account == wallet.hotkeypub.ss58_address

    saved = _invoke("addr", "add", "friend", address)
    assert saved.exit_code == 0, saved.output
    assert config.get_address("friend") == address
    assert details["coldkey_mnemonic"] not in saved.output
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda self, intent: submitted.append(intent))
    sent = _invoke("wallet", "transfer", "--dest", "friend", "--amount", "1")
    assert sent.exit_code == 0, sent.output
    assert submitted[0].dest_ss58 == address
    assert submitted[0].hashed_descriptor is None

    # Real intent/executor composition against a fake RPC boundary, without a live chain.
    sponsor = Wallet("sponsor", path=str(wallet_path))
    sponsor.regenerate_coldkey(seed=bytes([8]) * 32, use_password=False, suppress=True)
    substrate = FakeSubstrate()
    substrate.seed_constant("HashedAccounts", "Enabled", True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    substrate.block_hash = _finney_hash
    plan = asyncio.run(Executor(substrate).plan(submitted[0], sponsor))
    assert (plan.call.module, plan.call.function) == ("Utility", "batch_all")
    registration, transfer = plan.call.params["calls"]
    assert registration.params["descriptor"] == descriptor_value(recipient.descriptor)
    assert transfer.params["dest"] == recipient.account


async def _finney_hash(block=None):
    return FINNEY_GENESIS_HASH


def test_same_name_contact_wins_without_using_local_wallet_descriptor(wallet_path):
    wallet = Wallet("friend", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([3]) * 32, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
    )
    remote = receiving_address(_public(4), FINNEY_GENESIS_HASH)
    config.add_address({"name": "friend", "address": remote})
    resolved = _context(wallet_path).resolve_address_ref("dest_ss58", "friend")
    assert resolved.address == remote
    assert resolved.account != wallet.coldkeypub.ss58_address
    assert resolved.source == "address-book entry 'friend'"


def test_local_picker_and_wallet_list_share_complete_public_addresses(wallet_path):
    created = _invoke("wallet", "create", "--type", "hashed", "--no-password")
    assert created.exit_code == 0, created.output
    details = json.loads(created.output)
    context = _context(wallet_path)
    context.wallet_name = "sender"
    choice = next(row for row in _dest_choices(context) if row.name == "recipient")
    assert choice.ss58 == details["coldkey_address"]
    listed = _invoke("wallet", "list")
    assert listed.exit_code == 0, listed.output
    assert details["coldkey_address"] in listed.output
    assert details["hotkey_address"] in listed.output
    assert details["coldkey_mnemonic"] not in listed.output


def test_coldkey_swap_preserves_contact_receiving_address(wallet_path, monkeypatch):
    address = receiving_address(_public(), FINNEY_GENESIS_HASH)
    config.add_address({"name": "replacement", "address": address})
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda self, intent: submitted.append(intent))
    result = _invoke("wallet", "announce-coldkey-swap", "--new-coldkey", "replacement")
    assert result.exit_code == 0, result.output
    assert submitted[0].new_coldkey_ss58 == address
    assert submitted[0].hashed_descriptor is None


@pytest.mark.parametrize("shape", ["plain", "multiaddress", "nested"])
def test_raw_call_rejects_typed_contacts_instead_of_losing_setup(wallet_path, shape):
    config.add_address(
        {"name": "friend", "address": receiving_address(_public(), FINNEY_GENESIS_HASH)}
    )
    params = {"dest": {"Id": "friend"} if shape == "multiaddress" else "friend", "value": 1}
    target = "Balances.transfer_keep_alive"
    if shape == "nested":
        params = {
            "calls": [{"module": "Balances", "function": "transfer_keep_alive", "params": params}]
        }
        target = "Utility.batch_all"
    with pytest.raises(typer.BadParameter, match="wallet transfer"):
        resolve_builder_params(_context(wallet_path), target, params)


def test_nested_intent_resolution_keeps_full_receiving_address(wallet_path):
    address = receiving_address(_public(), FINNEY_GENESIS_HASH)
    config.add_address({"name": "friend", "address": address})
    args = {"intents": [{"op": "transfer", "dest_ss58": "friend", "amount_tao": 1}]}
    resolved = resolve_intent_args(_context(wallet_path), args)
    assert resolved["intents"][0]["dest_ss58"] == address


def test_legacy_contact_keeps_descriptor_for_registration_on_selected_chain(
    wallet_path, monkeypatch
):
    address = legacy_receiving_address(_public())
    saved = _invoke("addr", "add", "other-chain", address)
    assert saved.exit_code == 0, saved.output
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda self, intent: submitted.append(intent))
    result = _invoke("wallet", "transfer", "--dest", "other-chain", "--amount", "1")
    assert result.exit_code == 0, result.output
    assert submitted[0].dest_ss58 == address
    sponsor = Wallet("sender", path=str(wallet_path))
    sponsor.regenerate_coldkey(seed=bytes([8]) * 32, use_password=False, suppress=True)
    substrate = FakeSubstrate()
    substrate.block_hash = _finney_hash
    substrate.seed_constant("HashedAccounts", "Enabled", True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    plan = asyncio.run(Executor(substrate).plan(submitted[0], sponsor))
    guard, transfer = plan.call.params["calls"]
    assert guard.function == "register"
    assert transfer.params["dest"] == _public().ss58_address
    assert not substrate.submissions


@pytest.mark.parametrize("malformed", ["bth1_bad", "bth2_bad", "bth1_", "bth1_!"])
def test_malformed_receiving_addresses_never_become_wallet_names(
    wallet_path, monkeypatch, malformed
):
    saved = _invoke("addr", "add", "bad", malformed)
    assert saved.exit_code != 0
    assert config.get_address("bad") is None
    monkeypatch.setattr(
        "bittensor.cli.context.wallets.open_wallet", lambda **kw: pytest.fail("wallet lookup")
    )
    with pytest.raises(ValueError):
        _context(wallet_path).resolve_address_ref("dest_ss58", malformed)


def test_show_and_local_resolution_use_only_public_metadata(wallet_path, monkeypatch):
    created = _invoke("wallet", "create", "--type", "hashed", "--no-password")
    assert created.exit_code == 0, created.output
    details = json.loads(created.output)

    def fail_private(self):
        pytest.fail("private key accessed while displaying receiving information")

    monkeypatch.setattr(Wallet, "coldkey", property(fail_private))
    monkeypatch.setattr(Wallet, "hotkey", property(fail_private))
    shown = _invoke("wallet", "show")
    assert shown.exit_code == 0, shown.output
    display = json.loads(shown.output)
    assert display["coldkey_address"] == details["coldkey_address"]
    assert display["hotkey_address"] == details["hotkey_address"]
    assert details["coldkey_mnemonic"] not in shown.output
    assert details["hotkey_mnemonic"] not in shown.output
    assert "coldkey_ss58" not in display
    context = _context(wallet_path)
    assert (
        context.resolve_address_ref("dest_ss58", "recipient").address == details["coldkey_address"]
    )
    assert (
        context.resolve_address_ref("hotkey_ss58", "recipient/default").address
        == details["hotkey_address"]
    )


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_mnemonic_restore_prints_same_receiving_address_without_secret(wallet_path, role):
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_HASHED)
    args = ["wallet", f"regen-{role}", "--type", "hashed", "--mnemonic", MNEMONIC]
    if role == "coldkey":
        args.append("--no-password")
    result = _invoke(*args)
    assert result.exit_code == 0, result.output
    assert json.loads(result.output)["address"] == receiving_address(key, FINNEY_GENESIS_HASH)
    assert MNEMONIC not in result.output


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_public_restore_infers_type_and_preserves_descriptor(wallet_path, role):
    public = _public()
    address = receiving_address(public, FINNEY_GENESIS_HASH)
    result = _invoke("wallet", f"regen-{role}pub", "--address", address)
    assert result.exit_code == 0, result.output
    assert json.loads(result.output)["address"] == address
    wallet = Wallet("recipient", path=str(wallet_path))
    restored = getattr(wallet, role + "pub")
    assert restored.crypto_type == CRYPTO_HASHED
    assert bytes(restored.hashed_descriptor) == bytes(public.hashed_descriptor)
    assert not getattr(wallet, role + "_file").exists_on_device()


def test_public_restore_accepts_legacy_address_from_another_chain(wallet_path):
    address = legacy_receiving_address(_public())
    result = _invoke("wallet", "regen-coldkeypub", "--address", address)
    assert result.exit_code == 0, result.output
    wallet = Wallet("recipient", path=str(wallet_path))
    assert wallet.coldkeypub.ss58_address == _public().ss58_address
    assert not wallet.coldkey_file.exists_on_device()


def test_hashed_public_restore_rejects_incomplete_address(wallet_path):
    result = _invoke(
        "wallet", "regen-coldkeypub", "--ss58", _public().ss58_address, "--type", "hashed"
    )
    assert result.exit_code != 0
    assert "complete receiving address" in result.output
    assert not (wallet_path / "recipient").exists()


@pytest.mark.parametrize("scheme", ["sr", "ed", "hashed", "ms"])
def test_wallet_creation_and_listing_need_no_network(wallet_path, monkeypatch, scheme):
    monkeypatch.setattr(AppContext, "run", lambda *_: pytest.fail("unexpected network access"))
    result = _invoke("--network", "test", "wallet", "create", "--type", scheme, "--no-password")
    assert result.exit_code == 0, result.output
    wallet = Wallet("recipient", path=str(wallet_path))
    addresses = [receiving_address(wallet.coldkeypub), receiving_address(wallet.hotkeypub)]
    for network in ("finney", "ws://127.0.0.1:1"):
        shown = _invoke("--network", network, "wallet", "list")
        assert shown.exit_code == 0, shown.output
        for address in addresses:
            assert address in shown.output


def test_legacy_creation_and_contact_outputs_keep_ss58(wallet_path):
    result = _invoke("wallet", "create", "--no-password")
    assert result.exit_code == 0, result.output
    details = json.loads(result.output)
    wallet = Wallet("recipient", path=str(wallet_path))
    assert details["coldkey_ss58"] == wallet.coldkeypub.ss58_address
    assert "coldkey_address" not in details and "hotkey_address" not in details
    saved = _invoke("addr", "add", "legacy", details["coldkey_ss58"])
    assert saved.exit_code == 0, saved.output
    assert config.get_address("legacy") == details["coldkey_ss58"]
    assert (
        _context(wallet_path).resolve_address_ref("dest_ss58", "legacy").account
        == details["coldkey_ss58"]
    )
    assert wallet.coldkeypub.crypto_type == CRYPTO_SR25519


@pytest.mark.parametrize("scheme,code", [("hashed", 4), ("ms", 5)])
def test_named_protected_multisig_signatories_and_saved_presets(wallet_path, scheme, code):
    from bittensor._transport.codec import multisig_account
    from bittensor.cli import multisig_helpers
    from tests.harness.samples import BOB

    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([19]) * 32, crypto_type=code, use_password=False, suppress=True
    )
    address = receiving_address(wallet.coldkeypub, FINNEY_GENESIS_HASH)
    config.add_address({"name": "member", "address": address})
    config.add_multisig(
        {"name": "team", "threshold": 2, "signatories": ["recipient", "member", BOB]}
    )
    ctx = _context(wallet_path)
    expected = [wallet.coldkeypub.ss58_address, BOB]
    assert ctx.resolve_signatory_list(f"recipient,member,{BOB}") == expected
    assert multisig_helpers.resolve_multisig(ctx, multisig_name="team")[1] == expected
    derived = multisig_account(expected, 2).ss58_address
    assert ctx._saved_multisig_address("team") == derived
    assert ("team", derived) in multisig_helpers.saved_multisig_accounts(ctx)


@pytest.mark.parametrize("consumer", ["signatories", "preset"])
def test_multisig_receiving_identity_accepts_legacy_address(wallet_path, consumer):
    from bittensor._transport.codec import multisig_account
    from tests.harness.samples import BOB

    address = legacy_receiving_address(_public())
    config.add_address({"name": "member", "address": address})
    ctx = _context(wallet_path)
    expected = [_public().ss58_address, BOB]
    if consumer == "signatories":
        assert ctx.resolve_signatory_list(f"member,{BOB}") == expected
    else:
        config.add_multisig({"name": "team", "threshold": 2, "signatories": ["member", BOB]})
        assert ctx._saved_multisig_address("team") == multisig_account(expected, 2).ss58_address


@pytest.mark.parametrize("code", [4, 5])
def test_cli_multisig_add_and_show_with_named_protected_member(wallet_path, monkeypatch, code):
    from bittensor._transport.codec import multisig_account
    from tests.harness.samples import BOB

    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([21]) * 32, crypto_type=code, use_password=False, suppress=True
    )
    substrate = FakeSubstrate()
    monkeypatch.setattr(
        "bittensor.cli.context.Client", lambda network, **kw: Client(network, substrate=substrate)
    )
    result = _invoke(
        "multisig", "add", "team", "--threshold", "2", "--signatories", f"recipient,{BOB}"
    )
    assert result.exit_code == 0, result.output
    expected = multisig_account([wallet.coldkeypub.ss58_address, BOB], 2).ss58_address
    assert json.loads(result.output)["multisig_address"] == expected
    shown = _invoke("multisig", "show", "team")
    assert shown.exit_code == 0, shown.output
    assert json.loads(shown.output)["multisig_address"] == expected


@pytest.mark.parametrize("code", [4, 5])
@pytest.mark.parametrize("legacy", [False, True])
def test_cli_evm_associate_uses_protected_account_identity(wallet_path, monkeypatch, code, legacy):
    from types import SimpleNamespace

    from eth_account import Account

    from bittensor.cli.commands.evm import association
    from bittensor.evm.transactions import association_proof

    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_hotkey(seed=bytes([22]) * 32, crypto_type=code, suppress=True)
    if legacy:
        monkeypatch.setattr(
            AppContext, "resolve_address", lambda *_: legacy_receiving_address(wallet.hotkeypub)
        )
    account = Account.from_key(bytes([23]) * 32)
    monkeypatch.setattr(
        association, "_key_info", lambda *_: SimpleNamespace(address=account.address)
    )
    monkeypatch.setattr(association, "_unlock", lambda *_: account)
    substrate = FakeSubstrate()
    monkeypatch.setattr(
        "bittensor.cli.context.Client", lambda network, **kw: Client(network, substrate=substrate)
    )
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda self, intent, **kw: submitted.append(intent))
    result = _invoke("evm", "associate", "--netuid", "1")
    assert result.exit_code == 0, result.output
    intent = submitted[0]
    assert (
        intent.signature
        == association_proof(account, wallet.hotkey.ss58_address, intent.block_number)[0]
    )


def test_cli_evm_associate_rejects_malformed_address_before_unlock(wallet_path, monkeypatch):
    from types import SimpleNamespace

    from bittensor.cli.commands.evm import association

    monkeypatch.setattr(
        association, "_key_info", lambda *_: SimpleNamespace(address="0x" + "11" * 20)
    )
    monkeypatch.setattr(AppContext, "resolve_address", lambda *_: "bth1_bad")

    def unexpected(*_):
        raise AssertionError("must reject before key unlock")

    monkeypatch.setattr(association, "_unlock", unexpected)
    result = _invoke("evm", "associate", "--netuid", "1")
    assert result.exit_code != 0
    assert "104 characters" in result.output
