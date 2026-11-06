"""Hashed creation requires deposit consent before generating any wallet keys."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest
from typer.testing import CliRunner

from bittensor import Balance, Client, signing, wallets
from bittensor.cli import context
from bittensor.cli.commands import wallet as wallet_commands
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.sp_core import Keypair
from bittensor.wallet import Wallet
from tests.harness.fake_substrate import FakeSubstrate

DEPOSIT = Balance.from_tao("0.73")
FINALIZED_HASH = "0x" + "ab" * 32
runner = CliRunner()


def keyfiles(wallet):
    return tuple(
        Path(keyfile.path)
        for keyfile in (
            wallet.coldkey_file,
            wallet.coldkeypub_file,
            wallet.hotkey_file,
            wallet.hotkeypub_file,
        )
    )


def snapshot(wallet):
    return {path: path.read_bytes() for path in keyfiles(wallet) if path.exists()}


@pytest.fixture
def creation(tmp_path, monkeypatch):
    path = tmp_path / "wallets"
    for variable, value in {
        "BT_WALLET_PATH": path,
        "BT_WALLET": "created",
        "BT_HOTKEY": "default",
        "BT_NETWORK": "local",
        "BTCLI_CONFIG": tmp_path / "config.json",
        "BTCLI_ADDRESSES_PATH": tmp_path / "addresses.json",
        "BTCLI_PROXIES_PATH": tmp_path / "proxies.json",
        "BTCLI_MULTISIGS_PATH": tmp_path / "multisigs.json",
        "BTCLI_MULTISIG_CACHE": tmp_path / "multisig-cache.json",
        "BTCLI_SUBNET_NAMES_CACHE": tmp_path / "subnets.json",
        "BTCLI_TOKEN_SYMBOLS_CACHE": tmp_path / "symbols.json",
    }.items():
        monkeypatch.setenv(variable, str(value))
    monkeypatch.setattr("typer.testing._NamedTextIOWrapper.isatty", lambda _: True)
    payer = wallets.create(
        name="payer", path=str(path), use_password=False, on_mnemonic=lambda *_: None
    )
    target = Wallet("created", path=str(path))
    chain = FakeSubstrate()
    chain.seed_default("System", "Account", {"data": {"free": 10**12, "frozen": 0}})
    monkeypatch.setattr(context, "Client", lambda network, **_: Client(network, substrate=chain))
    state = SimpleNamespace(
        path=path,
        payer=payer,
        target=target,
        chain=chain,
        events=[],
        registrations=[],
        mnemonics={},
        details=[],
        error=None,
    )

    async def quote(_client, kinds):
        state.events.append(("quote", tuple(kinds)))
        return DEPOSIT

    state.quote = AsyncMock(side_effect=quote)
    monkeypatch.setattr(wallet_commands, "registration_deposit_for_types", state.quote)

    async def register(_client, wallet, **options):
        state.events.append(("register",))
        state.registrations.append((wallet, options))
        if state.error is not None:
            raise state.error
        roles = {role: getattr(wallet, role + "pub").ss58_address for role in options["roles"]}
        return SimpleNamespace(
            roles=roles, accounts=tuple(roles.values()), finalized_block_hash=FINALIZED_HASH
        )

    monkeypatch.setattr(Client, "register_wallet", register, raising=False)
    sign = signing.signing_keypair

    def unlock(wallet, role, **options):
        state.events.append(("unlock", wallet.name))
        return sign(wallet, role, **options)

    monkeypatch.setattr(signing, "signing_keypair", unlock)
    render_mnemonic = Output.mnemonic

    def mnemonic(output, role, phrase):
        state.events.append(("mnemonic", role))
        state.mnemonics[role] = phrase
        render_mnemonic(output, role, phrase)

    monkeypatch.setattr(Output, "mnemonic", mnemonic)
    render_detail = Output.wallet_detail

    def detail(output, title, fields, *args, **kwargs):
        state.details.append(dict(fields))
        return render_detail(output, title, fields, *args, **kwargs)

    monkeypatch.setattr(Output, "wallet_detail", detail)
    return state


def invoke(command="create", *extra, answer="y\n", global_options=(), payer="payer"):
    args = [
        *global_options,
        "--network",
        "local",
        "--wallet",
        "created",
        "--wallet-hotkey",
        "default",
        "wallet",
        command,
    ]
    if command != "register":
        args += ["--type", "hashed"]
    if command in {"create", "new-coldkey"}:
        args += ["--no-password"]
    if payer is not None:
        args += ["--registration-payer", payer]
    return runner.invoke(app, [*args, *extra], input=answer)


def forbid_generation(monkeypatch):
    def fail(*_args, **_kwargs):
        pytest.fail("wallet keys must not be generated before deposit consent")

    monkeypatch.setattr(Wallet, "create_new_coldkey", fail)
    monkeypatch.setattr(Wallet, "create_new_hotkey", fail)


@pytest.mark.parametrize(
    "command,roles",
    [("create", ("coldkey", "hotkey")), ("new-coldkey", ("coldkey",)), ("new-hotkey", ("hotkey",))],
)
def test_yes_creates_then_registers_exact_roles_with_approved_live_deposit(
    creation, command, roles
):
    result = invoke(command)
    assert result.exit_code == 0, result.output
    assert "0.73" in result.output
    assert creation.payer.coldkeypub.ss58_address in "".join(result.output.split())
    assert len(creation.registrations) == 1
    wallet, options = creation.registrations[0]
    assert options["roles"] == roles
    assert options["payer"].ss58_address == creation.payer.coldkeypub.ss58_address
    assert options["max_registration_deposit"] == DEPOSIT
    assert creation.events[0] == ("quote", (wallets.CRYPTO_HASHED,) * len(roles))
    assert creation.events[1] == ("unlock", "payer")
    assert creation.events[2:-1] == [("mnemonic", role) for role in roles]
    assert creation.events[-1] == ("register",)
    for role in roles:
        public = getattr(wallet, role + "pub")
        private = getattr(wallet, role)
        recovered = Keypair.create_from_mnemonic(creation.mnemonics[role], private.crypto_type)
        assert public.ss58_address == private.ss58_address == recovered.ss58_address
        prefix = role + "_" if command == "create" else ""
        assert creation.details[-1][prefix + "registration"] == "registered"
        assert creation.details[-1][prefix + "registration_block_hash"] == FINALIZED_HASH
    assert not creation.chain.submissions


@pytest.mark.parametrize(
    "extra,roles",
    [
        (("--hotkey-type", "standard"), ("coldkey",)),
        (("--type", "standard", "--hotkey-type", "hashed"), ("hotkey",)),
    ],
)
def test_mixed_wallet_only_registers_the_rotating_key(creation, extra, roles):
    result = invoke("create", *extra)
    assert result.exit_code == 0, result.output
    wallet, options = creation.registrations[0]
    assert options["roles"] == roles
    assert creation.events[0] == ("quote", (wallets.CRYPTO_HASHED,))
    assert wallets.requires_registration(wallet.coldkeypub) is ("coldkey" in roles)
    assert wallets.requires_registration(wallet.hotkeypub) is ("hotkey" in roles)


@pytest.mark.parametrize(
    "hotkey_option,expected_hotkey",
    [
        ([], wallets.CRYPTO_HASHED),
        (["--hotkey-crypto-type", "sr25519", "--hotkey-type", "standard"], wallets.CRYPTO_SR25519),
    ],
)
def test_cli_type_creates_wallet_and_public_descriptors(creation, hotkey_option, expected_hotkey):
    result = invoke("create", *hotkey_option, global_options=("--yes",))
    assert result.exit_code == 0, result.output
    wallet = creation.target
    assert wallet.coldkeypub.crypto_type == wallets.CRYPTO_HASHED
    assert wallet.hotkeypub.crypto_type == expected_hotkey
    assert len(creation.registrations) == 1
    assert creation.details[-1]["coldkey_registration"] == "registered"
    assert "tao_" in result.output
    assert wallet.coldkeypub.ss58_address in result.output
    assert result.output.index("tao_") < result.output.index(wallet.coldkeypub.ss58_address)
    assert "Account ID (SS58)" in result.output
    assert "Use the receiving address for transactions" in result.output
    assert "explorers display the same account as SS58" in result.output


@pytest.mark.parametrize("command", ["create", "new-coldkey", "new-hotkey"])
@pytest.mark.parametrize("answer", ["n\n", "\n", ""])
def test_decline_empty_and_eof_preserve_overwrite_targets_before_unlock_or_generation(
    creation, monkeypatch, command, answer
):
    existing = wallets.create(
        name="created", path=str(creation.path), use_password=False, on_mnemonic=lambda *_: None
    )
    before = snapshot(existing)
    forbid_generation(monkeypatch)
    result = invoke(command, "--overwrite", answer=answer, global_options=("--yes",))
    assert result.exit_code != 0, result.output
    assert "Cancelled" in result.output
    assert snapshot(existing) == before
    assert [event[0] for event in creation.events] == ["quote"]
    assert not creation.registrations
    assert not creation.chain.submissions


@pytest.mark.parametrize("mode", ["json", "noninteractive", "quiet"])
def test_noninteractive_modes_cannot_bypass_specific_consent(creation, monkeypatch, mode):
    forbid_generation(monkeypatch)
    extra = ("--yes",)
    if mode == "noninteractive":
        monkeypatch.setattr("typer.testing._NamedTextIOWrapper.isatty", lambda _: False)
    else:
        extra += ("--" + mode,)
    result = invoke(global_options=extra)
    assert result.exit_code != 0, result.output
    assert "explicit y/n" in result.output
    assert not snapshot(creation.target)
    creation.quote.assert_not_awaited()
    assert not creation.registrations


def test_disabled_chain_quote_stops_before_keys_or_unlock(creation, monkeypatch):
    creation.quote.side_effect = ValueError("account registration is not enabled on this chain")
    forbid_generation(monkeypatch)
    result = invoke()
    assert result.exit_code != 0, result.output
    assert "not enabled" in result.output
    assert not snapshot(creation.target)
    assert not creation.events
    assert not creation.registrations


def test_dry_run_refuses_before_quote_unlock_or_key_generation(creation, monkeypatch):
    forbid_generation(monkeypatch)
    result = invoke(global_options=("--dry-run", "--yes"))
    assert result.exit_code != 0, result.output
    assert "--dry-run is not supported" in result.output
    creation.quote.assert_not_awaited()
    assert not creation.events
    assert not snapshot(creation.target)


def test_payer_identity_mismatch_prevents_key_generation_after_consent(creation, monkeypatch):
    previous = snapshot(creation.payer)
    wrong_private = Keypair.create_from_seed(bytes([83]) * 32, wallets.CRYPTO_SR25519)
    monkeypatch.setattr(signing, "signing_keypair", lambda *_args, **_kwargs: wrong_private)
    forbid_generation(monkeypatch)
    result = invoke()
    assert result.exit_code != 0, result.output
    assert "does not match" in result.output
    assert snapshot(creation.payer) == previous
    assert not snapshot(creation.target)
    assert not creation.registrations


def test_new_hotkey_can_use_its_existing_coldkey_as_payer(creation):
    existing = wallets.new_coldkey(
        name="created", path=str(creation.path), use_password=False, on_mnemonic=lambda *_: None
    )
    before = snapshot(existing)
    result = invoke("new-hotkey", payer=None)
    assert result.exit_code == 0, result.output
    _, options = creation.registrations[0]
    assert options["payer"].ss58_address == existing.coldkeypub.ss58_address
    assert options["roles"] == ("hotkey",)
    assert all(path.read_bytes() == content for path, content in before.items())


@pytest.mark.parametrize("scheme", ["sr", "ed", "ms", "falcon512", "falcon1024"])
def test_standard_and_fixed_pq_creation_remains_offline(creation, monkeypatch, scheme):
    def no_client(*_args, **_kwargs):
        pytest.fail("standard key creation must not contact the chain")

    monkeypatch.setattr(context, "Client", no_client)
    result = runner.invoke(
        app, ["--yes", "--json", "wallet", "create", "--crypto-type", scheme, "--no-password"]
    )
    assert result.exit_code == 0, result.output
    assert len(snapshot(creation.target)) == 4
    creation.quote.assert_not_awaited()
    assert not creation.registrations


def test_failed_registration_preserves_keys_and_phrases_and_retry_reuses_them(
    creation, monkeypatch
):
    creation.error = ValueError("MEV Shield unavailable")
    result = invoke()
    assert result.exit_code != 0, result.output
    assert "key files are saved" in result.output
    assert "wallet register" in result.output
    created = creation.registrations[0][0]
    saved = snapshot(created)
    assert len(saved) == 4
    assert set(creation.mnemonics) == {"coldkey", "hotkey"}
    output = " ".join(result.output.split())
    for role, phrase in creation.mnemonics.items():
        key = getattr(created, role)
        assert phrase in output
        assert (
            Keypair.create_from_mnemonic(phrase, key.crypto_type).ss58_address == key.ss58_address
        )

    forbid_generation(monkeypatch)
    creation.error = None
    retried = invoke("register", "--key", "both")
    assert retried.exit_code == 0, retried.output
    assert snapshot(created) == saved
    original, retried_wallet = (item[0] for item in creation.registrations)
    for role in ("coldkey", "hotkey"):
        assert (
            getattr(original, role + "pub").ss58_address
            == getattr(retried_wallet, role + "pub").ss58_address
        )
