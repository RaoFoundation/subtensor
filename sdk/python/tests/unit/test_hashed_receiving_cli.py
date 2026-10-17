"""Complete receiving addresses survive creation, contacts, recovery and sending."""

from __future__ import annotations

import asyncio
import json

import pytest
import typer
from typer.testing import CliRunner

from bittensor import config
from bittensor.cli import wallet_addresses
from bittensor.cli.call_names import resolve_builder_params, resolve_intent_args
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.cli.stake_picker import _dest_choices
from bittensor.client import Client
from bittensor.executor import Executor
from bittensor.hashed import descriptor_value, registration_descriptor_value
from bittensor.keyfiles import Keyfile
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.settings import FINNEY_GENESIS_HASH
from bittensor.sp_core import CRYPTO_HASHED, CRYPTO_SR25519, Keypair
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.receiving import descriptor_receiving_address

MNEMONIC = "bottom drive obey lake curtain smoke basket hold race lonely fit walk"
OTHER_GENESIS = "0x" + "17" * 32
runner = CliRunner()


@pytest.fixture
def wallet_path(tmp_path, monkeypatch):
    # Transport/status behavior has its own tests; these CLI flows stay offline.
    monkeypatch.setattr(wallet_addresses, "check_registration", lambda *_args, **_kwargs: None)
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
    assert details["coldkey_ss58"] == recipient.account
    assert details["coldkey_receiving_address"] == address
    assert details["coldkey_primary_address"] == address
    assert details["coldkey_registration"] == "not_checked"
    assert "hashed_descriptor" not in details
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
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    substrate.block_hash = _finney_hash
    plan = asyncio.run(Executor(substrate).plan(submitted[0], sponsor))
    assert (plan.call.module, plan.call.function) == ("Utility", "batch_all")
    registration, transfer = plan.call.params["calls"]
    assert registration.params["descriptor"] == registration_descriptor_value(recipient.descriptor)
    assert transfer.params["dest"] == recipient.account


async def _finney_hash(block=None):
    return FINNEY_GENESIS_HASH


def test_same_name_contact_wins_without_using_local_wallet_descriptor(wallet_path):
    wallet = Wallet("friend", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([3]) * 32, crypto_type=CRYPTO_HASHED, use_password=False, suppress=True
    )
    remote = receiving_address(_public(4))
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


@pytest.mark.parametrize("code", [4, 6, 7, 8])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("failure", ["descriptor", "missing", "encrypted", "non_object"])
def test_wallet_list_isolates_unreadable_public_keys(wallet_path, monkeypatch, code, role, failure):
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([21]) * 32, crypto_type=code, use_password=False, suppress=True
    )
    wallet.regenerate_hotkey(seed=bytes([22]) * 32, crypto_type=code, suppress=True)
    other = Wallet("healthy", path=str(wallet_path))
    other.regenerate_coldkey(seed=bytes([23]) * 32, use_password=False, suppress=True)
    bad_key = getattr(wallet, role + "pub")
    public_path = (
        wallet_path
        / "recipient"
        / ("coldkeypub.txt" if role == "coldkey" else "hotkeys/defaultpub.txt")
    )
    if failure == "descriptor":
        data = json.loads(public_path.read_text())
        data["mldsaDescriptor" if code in (7, 8) else "hashedDescriptor"] = "0x00"
        public_path.write_text(json.dumps(data))
    elif failure == "missing":
        public_path.unlink()
    elif failure == "encrypted":
        public_path.write_bytes(b"$NACL\xffinvalid encrypted public keyfile")
    else:
        public_path.write_text("[]")
    before = {p: p.read_bytes() for p in wallet_path.rglob("*") if p.is_file()}
    monkeypatch.setattr(AppContext, "run", lambda *_: pytest.fail("unexpected network access"))
    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_: pytest.fail("must not unlock keys"))

    listed = _invoke("wallet", "list")
    assert listed.exit_code == 0, listed.output
    records = {row["coldkey"]: row for row in json.loads(listed.output)["coldkeys"]}
    affected = records["recipient"]
    healthy = affected["hotkeys"][0] if role == "coldkey" else affected
    affected = affected if role == "coldkey" else affected["hotkeys"][0]
    assert affected["ss58"] is None
    if role == "coldkey" and failure == "encrypted":
        assert affected["encrypted"] is True
        assert "encrypted key" in affected["note"]
        assert "error" not in affected
    else:
        assert "public key unavailable" in affected["error"]
    assert healthy["receiving_address"].startswith(("bth1_", "tao_")) and "error" not in healthy
    assert healthy["ss58"] == parse_recipient(healthy["receiving_address"]).account
    assert records["healthy"]["ss58"] == other.coldkeypub.ss58_address
    assert "error" not in records["healthy"]
    for invalid_address in (bad_key.ss58_address, receiving_address(bad_key)):
        assert invalid_address not in listed.output

    human = runner.invoke(app, ["wallet", "list"])
    assert human.exit_code == 0, human.output
    assert (affected.get("error") or affected["note"]) in human.output
    assert "healthy" in human.output
    assert bad_key.ss58_address not in human.output
    assert before == {p: p.read_bytes() for p in wallet_path.rglob("*") if p.is_file()}


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_wallet_list_retains_legacy_ms_entry_without_exposing_invalid_address(wallet_path, role):
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([24]) * 32, crypto_type=7, use_password=False, suppress=True
    )
    wallet.regenerate_hotkey(seed=bytes([25]) * 32, crypto_type=7, suppress=True)
    public_path = (
        wallet_path
        / "recipient"
        / ("coldkeypub.txt" if role == "coldkey" else "hotkeys/defaultpub.txt")
    )
    data = json.loads(public_path.read_text())
    data.pop("walletVersion")
    data.pop("mldsaDescriptor")
    data["hashedDescriptor"] = "0x0202" + "01" * 32
    public_path.write_text(json.dumps(data))
    listed = _invoke("wallet", "list")
    assert listed.exit_code == 0, listed.output
    record = json.loads(listed.output)["coldkeys"][0]
    record = record if role == "coldkey" else record["hotkeys"][0]
    assert record["ss58"] is None
    assert "public key unavailable" in record["error"]
    assert data["ss58Address"] not in listed.output
    assert json.loads(public_path.read_text()) == data


@pytest.mark.parametrize("code", [0, 1, 4, 6, 7, 8])
@pytest.mark.parametrize("has_public_file", [False, True])
def test_wallet_list_does_not_treat_encrypted_hotkeys_as_broken(
    wallet_path, monkeypatch, code, has_public_file
):
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(seed=bytes([26]) * 32, use_password=False, suppress=True)
    wallet.regenerate_hotkey(
        seed=bytes([27]) * 32,
        crypto_type=code,
        hotkey_password="test-only-password",
        suppress=True,
    )
    expected_address = receiving_address(wallet.hotkeypub)
    if not has_public_file:
        (wallet_path / "recipient" / "hotkeys/defaultpub.txt").unlink()
    before = {p: p.read_bytes() for p in wallet_path.rglob("*") if p.is_file()}
    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_: pytest.fail("must not unlock keys"))
    monkeypatch.setattr(AppContext, "run", lambda *_: pytest.fail("unexpected network access"))

    listed = _invoke("wallet", "list")
    assert listed.exit_code == 0, listed.output
    hotkey = json.loads(listed.output)["coldkeys"][0]["hotkeys"][0]
    assert "error" not in hotkey
    if has_public_file:
        assert hotkey["ss58"] == parse_recipient(expected_address).account
        if code in (4, 6, 7, 8):
            assert hotkey["receiving_address"] == expected_address
    else:
        assert hotkey["ss58"] is None
        assert hotkey["encrypted"] is True
        assert hotkey["crypto_type"] == "unknown"
        assert hotkey["signing_scheme"] is None
        assert hotkey["account_type"] is None
        assert "encrypted key" in hotkey["note"]
        human = runner.invoke(app, ["wallet", "list"])
        assert human.exit_code == 0, human.output
        assert hotkey["note"] in human.output
        assert "invalid" not in human.output
    assert before == {p: p.read_bytes() for p in wallet_path.rglob("*") if p.is_file()}


def test_coldkey_swap_preserves_contact_receiving_address(wallet_path, monkeypatch):
    address = receiving_address(_public())
    config.add_address({"name": "replacement", "address": address})
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda self, intent: submitted.append(intent))
    result = _invoke("wallet", "announce-coldkey-swap", "--new-coldkey", "replacement")
    assert result.exit_code == 0, result.output
    assert submitted[0].new_coldkey_ss58 == address
    assert submitted[0].hashed_descriptor is None


@pytest.mark.parametrize("shape", ["plain", "multiaddress", "nested"])
def test_raw_call_rejects_typed_contacts_instead_of_losing_setup(wallet_path, shape):
    config.add_address({"name": "friend", "address": receiving_address(_public())})
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
    address = receiving_address(_public())
    config.add_address({"name": "friend", "address": address})
    args = {"intents": [{"op": "transfer", "dest_ss58": "friend", "amount_tao": 1}]}
    resolved = resolve_intent_args(_context(wallet_path), args)
    assert resolved["intents"][0]["dest_ss58"] == address


def test_contact_keeps_descriptor_for_registration_on_selected_chain(wallet_path, monkeypatch):
    address = descriptor_receiving_address(_public())
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
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    substrate.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    plan = asyncio.run(Executor(substrate).plan(submitted[0], sponsor))
    guard, transfer = plan.call.params["calls"]
    assert guard.function == "register"
    assert transfer.params["dest"] == _public().ss58_address
    assert not substrate.submissions


@pytest.mark.parametrize(
    "malformed", ["bth1_bad", "bth2_bad", "tao_bad", "TAO_bad", " tao_bad", "bth1_", "bth1_!"]
)
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


@pytest.mark.parametrize("mode", ["standard", "hashed"])
@pytest.mark.parametrize("legacy_prefix", [False, True])
def test_ms_address_prefixes_support_contacts_public_recovery_and_transfers(
    wallet_path, monkeypatch, mode, legacy_prefix
):
    created = _invoke("wallet", "create", "--crypto-type", "ms", "--type", mode, "--no-password")
    assert created.exit_code == 0, created.output
    address = json.loads(created.output)["coldkey_address"]
    assert address.startswith("tao_")
    supplied = address.replace("tao_", "bth2_", 1) if legacy_prefix else address
    saved = _invoke("addr", "add", "friend", supplied)
    assert saved.exit_code == 0, saved.output
    assert config.get_address("friend") == supplied
    resolved = _context(wallet_path).resolve_address_ref("dest_ss58", "friend")
    assert resolved.account == Wallet("recipient", path=str(wallet_path)).coldkeypub.ss58_address
    restored = _invoke("wallet", "regen-coldkeypub", "--wallet", "watch", "--address", supplied)
    assert restored.exit_code == 0, restored.output
    assert receiving_address(Wallet("watch", path=str(wallet_path)).coldkeypub) == address
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda self, intent: submitted.append(intent))
    sent = _invoke("wallet", "transfer", "--dest", "friend", "--amount", "1")
    assert sent.exit_code == 0, sent.output
    assert submitted[0].dest_ss58 == supplied


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
    assert (
        display["coldkey_ss58"]
        == Wallet("recipient", path=str(wallet_path)).coldkeypub.ss58_address
    )
    assert display["coldkey_receiving_address"] == details["coldkey_address"]
    context = _context(wallet_path)
    assert (
        context.resolve_address_ref("dest_ss58", "recipient").address == details["coldkey_address"]
    )
    assert (
        context.resolve_address_ref("hotkey_ss58", "recipient/default").address
        == details["hotkey_address"]
    )


@pytest.mark.parametrize("code", [0, 1, 4, 6, 7, 8])
def test_show_legacy_hotkey_without_companion_uses_only_public_fields(
    wallet_path, monkeypatch, code
):
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_hotkey(seed=bytes([41]) * 32, crypto_type=code, suppress=True)
    expected = wallet.hotkeypub.ss58_address
    (wallet_path / "recipient/hotkeys/defaultpub.txt").unlink()
    path = wallet_path / "recipient/hotkeys/default"
    data = json.loads(path.read_text())
    # Invalid secret fields prove no signing key is recovered for display.
    data.update(secretPhrase="not a mnemonic", secretSeed="invalid", privateKey="invalid")
    path.write_text(json.dumps(data))
    before = path.read_bytes()
    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_: pytest.fail("must not unlock keys"))
    shown = _invoke("wallet", "show", "--offline")
    assert shown.exit_code == 0, shown.output
    display = json.loads(shown.output)
    assert display["hotkey_ss58"] == expected
    assert "not a mnemonic" not in shown.output
    assert before == path.read_bytes()


def test_show_encrypted_legacy_hotkey_does_not_prompt_or_unlock(wallet_path, monkeypatch):
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(seed=bytes([42]) * 32, use_password=False, suppress=True)
    hotkeys = wallet_path / "recipient/hotkeys"
    hotkeys.mkdir()
    (hotkeys / "default").write_bytes(b"$NACL\xfftest encrypted hotkey")
    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_: pytest.fail("must not unlock keys"))
    shown = _invoke("wallet", "show", "--offline")
    assert shown.exit_code == 0, shown.output
    assert "encrypted key; public address unavailable" in json.loads(shown.output)["hotkey"]


@pytest.mark.parametrize("code", [4, 6, 7, 8])
@pytest.mark.parametrize("command", ["show", "list"])
def test_cli_shows_finalized_coldkey_and_unregistered_hotkey(
    wallet_path, monkeypatch, code, command
):
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([43]) * 32, crypto_type=code, use_password=False, suppress=True
    )
    wallet.regenerate_hotkey(seed=bytes([44]) * 32, crypto_type=code, suppress=True)
    cold, hot = wallet.coldkeypub, wallet.hotkeypub
    finalized = "0x" + "ab" * 32

    class Chain:
        async def get_finalized_block_number(self):
            return 10

        async def get_block_hash(self, height):
            assert height == 10
            return finalized

        async def query(self, module, storage, params, *, block_hash):
            assert module == "HashedAccounts" and block_hash == finalized
            if params == [cold.ss58_address]:
                return {"descriptor": descriptor_value(bytes(cold.hashed_descriptor))}
            assert params == [hot.ss58_address]
            return None

    def check(context, records, *, offline=False):
        assert context.network == "test" and not offline
        asyncio.run(wallet_addresses.query_registration(Chain(), records))

    monkeypatch.setattr(wallet_addresses, "check_registration", check)
    shown = _invoke("wallet", command, "--network", "test")
    assert shown.exit_code == 0, shown.output
    data = json.loads(shown.output)
    if command == "show":
        records = [
            {k.removeprefix(role + "_"): v for k, v in data.items() if k.startswith(role + "_")}
            for role in ("coldkey", "hotkey")
        ]
    else:
        records = [data["coldkeys"][0], data["coldkeys"][0]["hotkeys"][0]]
    registered, unregistered = records
    assert registered["registration"] == "registered"
    assert registered["primary_address"] == receiving_address(cold)
    assert registered["receiving_address"] == receiving_address(cold)
    assert unregistered["registration"] == "unregistered"
    assert unregistered["primary_address"] == receiving_address(hot)
    assert unregistered["ss58"] == hot.ss58_address
    assert all(
        record["registration_network"] == "test" and record["registration_block_hash"] == finalized
        for record in records
    )


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_mnemonic_restore_prints_same_receiving_address_without_secret(wallet_path, role):
    key = Keypair.create_from_mnemonic(MNEMONIC, CRYPTO_HASHED)
    args = ["wallet", f"regen-{role}", "--type", "hashed", "--mnemonic", MNEMONIC]
    if role == "coldkey":
        args.append("--no-password")
    result = _invoke(*args)
    assert result.exit_code == 0, result.output
    assert json.loads(result.output)["address"] == receiving_address(key)
    assert MNEMONIC not in result.output


@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
def test_public_restore_infers_type_and_preserves_descriptor(wallet_path, role):
    public = _public()
    address = receiving_address(public)
    result = _invoke("wallet", f"regen-{role}pub", "--address", address)
    assert result.exit_code == 0, result.output
    assert json.loads(result.output)["address"] == address
    wallet = Wallet("recipient", path=str(wallet_path))
    restored = getattr(wallet, role + "pub")
    assert restored.crypto_type == CRYPTO_HASHED
    assert bytes(restored.hashed_descriptor) == bytes(public.hashed_descriptor)
    assert not getattr(wallet, role + "_file").exists_on_device()


def test_public_restore_accepts_descriptor_address(wallet_path):
    address = descriptor_receiving_address(_public())
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


@pytest.mark.parametrize("scheme", ["sr", "ed", "hashed", "hashed-ed25519"])
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


@pytest.mark.parametrize("scheme,code", [("hashed", 4), ("hashed-ed25519", 6)])
def test_named_protected_multisig_signatories_and_saved_presets(wallet_path, scheme, code):
    from bittensor._transport.codec import multisig_account
    from bittensor.cli import multisig_helpers
    from tests.harness.samples import BOB

    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(
        seed=bytes([19]) * 32, crypto_type=code, use_password=False, suppress=True
    )
    address = receiving_address(wallet.coldkeypub)
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
    assert multisig_helpers.resolve_member_ref(ctx, "member", expected) == (
        "member",
        wallet.coldkeypub.ss58_address,
    )
    assert (
        multisig_helpers.resolve_member_ref(ctx, address, expected)[1]
        == wallet.coldkeypub.ss58_address
    )
    for reference in ("member", address):
        ctx.signatory_wallet = reference
        multisig_helpers.infer_external_signer_from_signatory(ctx, expected)
        assert ctx.signer_backend is None
        assert multisig_helpers.pick_local_signatory(ctx, preset="team", signatories=expected) == (
            "recipient",
            wallet.coldkeypub.ss58_address,
        )
        assert multisig_helpers.plan_signatory_rounds(
            ctx,
            [f"{reference}=wallet"],
            signatories=expected,
            threshold=2,
            preset="team",
        ) == [("recipient", wallet.coldkeypub.ss58_address, "wallet")]


@pytest.mark.parametrize("consumer", ["signatories", "preset"])
def test_multisig_receiving_identity_accepts_descriptor_address(wallet_path, consumer):
    from bittensor._transport.codec import multisig_account
    from tests.harness.samples import BOB

    address = descriptor_receiving_address(_public())
    config.add_address({"name": "member", "address": address})
    ctx = _context(wallet_path)
    expected = [_public().ss58_address, BOB]
    if consumer == "signatories":
        assert ctx.resolve_signatory_list(f"member,{BOB}") == expected
    else:
        config.add_multisig({"name": "team", "threshold": 2, "signatories": ["member", BOB]})
        assert ctx._saved_multisig_address("team") == multisig_account(expected, 2).ss58_address


@pytest.mark.parametrize("code", [4, 6])
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


@pytest.mark.parametrize("code", [4, 6])
@pytest.mark.parametrize("independent", [False, True])
def test_cli_evm_associate_uses_protected_account_identity(
    wallet_path, monkeypatch, code, independent
):
    from types import SimpleNamespace

    from eth_account import Account

    from bittensor.cli.commands.evm import association
    from bittensor.evm.transactions import association_proof

    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_hotkey(seed=bytes([22]) * 32, crypto_type=code, suppress=True)
    if independent:
        monkeypatch.setattr(
            AppContext, "resolve_address", lambda *_: descriptor_receiving_address(wallet.hotkeypub)
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
    assert "61 characters" in result.output


@pytest.mark.parametrize("crypto_type", [4, 6])
@pytest.mark.parametrize("selector", ["receiving", "local", "canonical"])
def test_explicit_root_claim_matches_receiving_identity(
    wallet_path, monkeypatch, crypto_type, selector
):
    from bittensor import Balance
    from bittensor.cli.commands import root
    from bittensor.cli.root_helpers import RootPosition
    from tests.harness.samples import ALICE

    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.regenerate_coldkey(seed=bytes([10]) * 32, use_password=False, suppress=True)
    wallet.regenerate_hotkey(seed=bytes([11]) * 32, crypto_type=crypto_type, suppress=True)
    key = wallet.hotkeypub
    selected = {
        "receiving": receiving_address(key),
        "local": "recipient/default",
        "canonical": key.ss58_address,
    }[selector]

    async def positions(client, owner):
        return [RootPosition(key.ss58_address, Balance.from_tao(2), Balance.from_tao(1))]

    class ReadClient:
        async def read(self, name, **kwargs):
            assert name == "validator_basket_summary"
            assert kwargs["hotkey_ss58"] == key.ss58_address
            return {"beta_price_tao": 1.0}

    monkeypatch.setattr(root, "fetch_root_positions", positions)
    monkeypatch.setattr(AppContext, "run", lambda self, fn: asyncio.run(fn(ReadClient())))
    monkeypatch.setattr(root, "local_address_names", lambda path: {key.ss58_address: "validator"})
    submitted = []
    monkeypatch.setattr(
        AppContext, "submit", lambda self, intent, **kwargs: submitted.append(intent)
    )
    result = _invoke("root", "claim", "--hotkey", selected, "--coldkey", ALICE)
    assert result.exit_code == 0, (result.output, result.exception)
    assert len(submitted) == 1
    assert submitted[0].hotkey_ss58 == key.ss58_address


@pytest.mark.parametrize("crypto_type", [4, 6])
@pytest.mark.parametrize("selector", ["receiving", "contact", "local", "canonical"])
@pytest.mark.parametrize("pick_destination", [False, True])
def test_root_move_matches_identity_and_preserves_receiving_descriptors(
    wallet_path, monkeypatch, crypto_type, selector, pick_destination
):
    import importlib

    from bittensor import Balance
    from bittensor.cli.root_helpers import RootPosition
    from tests.harness.samples import ALICE

    move = importlib.import_module("bittensor.cli.commands.root_move")
    wallet = Wallet("recipient", path=str(wallet_path))
    if selector == "local":
        wallet.regenerate_coldkey(seed=bytes([20]) * 32, use_password=False, suppress=True)
    wallet.regenerate_hotkey(seed=bytes([21]) * 32, crypto_type=crypto_type, suppress=True)
    key = wallet.hotkeypub
    source = receiving_address(key)
    destination_key = Keypair.create_from_seed(bytes([22]) * 32, crypto_type)
    destination = receiving_address(destination_key)
    config.add_address({"name": "source-contact", "address": source})
    config.add_address({"name": "destination-contact", "address": destination})
    selected = {
        "receiving": source,
        "contact": "source-contact",
        "local": "default",
        "canonical": key.ss58_address,
    }[selector]

    async def positions(client, owner):
        return [RootPosition(key.ss58_address, Balance.from_tao(10), Balance.from_tao(1))]

    class ReadClient:
        async def read(self, name, **kwargs):
            assert name == "root_baskets"
            return []

    def pick_fund(*args, **kwargs):
        assert kwargs["exclude"] == {key.ss58_address}
        return {"hotkey": destination_key.ss58_address, "name": "destination"}

    monkeypatch.setattr(move, "fetch_root_positions", positions)
    monkeypatch.setattr(move, "interactive", lambda _: True)
    monkeypatch.setattr(move, "pick_fund", pick_fund)
    monkeypatch.setattr(AppContext, "run", lambda self, fn: asyncio.run(fn(ReadClient())))
    submitted = []
    monkeypatch.setattr(
        AppContext, "submit", lambda self, intent, **kwargs: submitted.append(intent)
    )
    args = ["root", "move", "--coldkey", ALICE, "--from", selected]
    if not pick_destination:
        args.extend(["--to", "destination-contact"])
    result = _invoke(*args)
    assert result.exit_code == 0, (result.output, result.exception)
    assert len(submitted) == 1
    intent = submitted[0]
    assert intent.origin_hotkey_ss58 == (
        key.ss58_address if selector in ("canonical", "local") else source
    )
    assert intent.dest_hotkey_ss58 == (
        destination_key.ss58_address if pick_destination else destination
    )
    assert intent.claim is True
    assert intent.origin_netuid == intent.dest_netuid == 0

    if not pick_destination:
        # Carry the actual CLI intent through recipient preparation and composition.
        # The destination descriptor must still produce an atomic guard before payment.
        from bittensor.executor import _compose_intent_call
        from tests.harness.samples import dev_wallet

        chain = FakeSubstrate()
        chain.seed("AdminUtils", "HashedAccountsEnabled", [], True)
        chain.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
        for public in (key, destination_key):
            chain.seed(
                "HashedAccounts",
                "Accounts",
                [public.ss58_address],
                {"descriptor": descriptor_value(bytes(public.hashed_descriptor))},
            )
        call, _ = asyncio.run(_compose_intent_call(chain, intent, dev_wallet()))
        assert call.function == "batch_all"
        children = call.params["calls"]
        guards = [child for child in children if child.function == "check_registered"]
        assert any(
            child.params["descriptor"]
            == registration_descriptor_value(bytes(destination_key.hashed_descriptor))
            for child in guards
        )
        assert [child.function for child in children[len(guards) :]] == [
            "claim_root_with_hotkey",
            "move_stake",
        ]
        assert children[-1].params["destination_hotkey"] == destination_key.ss58_address
        chain.seed("AdminUtils", "HashedAccountsEnabled", [], False)
        with pytest.raises(ValueError, match="hashed receiving addresses are not supported"):
            asyncio.run(_compose_intent_call(chain, intent, dev_wallet()))


@pytest.mark.parametrize("crypto_type", [4, 6])
def test_root_move_rejects_same_validator_in_different_encodings(
    wallet_path, monkeypatch, crypto_type
):
    import importlib

    from bittensor import Balance
    from bittensor.cli.root_helpers import RootPosition
    from tests.harness.samples import ALICE

    move = importlib.import_module("bittensor.cli.commands.root_move")
    key = Keypair.create_from_seed(bytes([23]) * 32, crypto_type)

    async def positions(*args):
        return [RootPosition(key.ss58_address, Balance.from_tao(10), Balance.from_tao(1))]

    monkeypatch.setattr(move, "fetch_root_positions", positions)
    monkeypatch.setattr(AppContext, "run", lambda self, fn: asyncio.run(fn(None)))
    submitted = []
    monkeypatch.setattr(AppContext, "submit", lambda *args, **kwargs: submitted.append(args))
    result = _invoke(
        "root",
        "move",
        "--coldkey",
        ALICE,
        "--from",
        receiving_address(key),
        "--to",
        key.ss58_address,
    )
    assert result.exit_code == 2, (result.output, result.exception)
    assert "source and destination are the same validator" in result.output
    assert not submitted


@pytest.mark.parametrize("crypto_type", [4, 6])
def test_root_list_receiving_selector_matches_position(wallet_path, monkeypatch, crypto_type):
    from bittensor import Balance
    from bittensor.cli.commands import root
    from bittensor.cli.root_helpers import RootPosition
    from tests.harness.samples import ALICE

    key = Keypair.create_from_seed(bytes([24]) * 32, crypto_type)
    position = RootPosition(key.ss58_address, Balance.from_tao(10), Balance.from_tao(1))

    async def positions(*args):
        return [position]

    class ReadClient:
        async def read(self, name, **kwargs):
            assert name == "validator_basket_summary"
            assert kwargs["hotkey_ss58"] == key.ss58_address
            return {"hotkey": key.ss58_address}

    monkeypatch.setattr(root, "fetch_root_positions", positions)
    monkeypatch.setattr(AppContext, "run", lambda self, fn: asyncio.run(fn(ReadClient())))
    rendered = []
    monkeypatch.setattr(
        root, "render_validator_detail", lambda ctx, summary, yours: rendered.append(yours)
    )
    result = _invoke("root", "list", receiving_address(key), "--coldkey", ALICE)
    assert result.exit_code == 0, (result.output, result.exception)
    assert rendered == [position]
