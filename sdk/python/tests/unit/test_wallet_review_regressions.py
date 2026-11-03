"""Public identity and registration survive listing, selection, and unlocking."""

import asyncio
import json
from pathlib import Path
from types import SimpleNamespace

import pytest
from typer.testing import CliRunner

from bittensor import Balance, config
from bittensor.cli import wallet_addresses
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.cli.root_helpers import RootPosition
from bittensor.client import Client
from bittensor.hashed import registration_descriptor_value
from bittensor.keyfiles import Keyfile
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair, requires_registration
from bittensor.wallet import Wallet
from tests.harness.samples import ALICE_HOT, BOB_HOT
from tests.unit import test_hashed_receiving_cli, test_registration_sponsorship_cli

wallet_path = test_hashed_receiving_cli.wallet_path
fake = test_registration_sponsorship_cli.fake
wallet_dir = test_registration_sponsorship_cli.wallet_dir

runner = CliRunner()


@pytest.fixture(params=[4, 6, 7, 8])
def recipient(request, fake):
    return test_registration_sponsorship_cli.make_recipient(fake, request.param)


@pytest.mark.parametrize("code", [0, 1, 4, 6, 7, 8])
@pytest.mark.parametrize("role", ["coldkey", "hotkey"])
@pytest.mark.parametrize("corrupt", [False, True])
def test_listing_validates_compatibility_crypto_strings(
    wallet_path, monkeypatch, code, role, corrupt
):
    wallet = Wallet("compat", path=str(wallet_path))
    wallet.regenerate_coldkey(seed=bytes([61]) * 32, crypto_type=code, use_password=False)
    wallet.regenerate_hotkey(seed=bytes([62]) * 32, crypto_type=code)
    public = getattr(wallet, role + "pub")
    path = getattr(wallet, role + "pub_file").path
    paths = [Path(path)]
    if role == "hotkey":
        paths.append(Path(wallet.hotkey_file.path))
    for path in paths:
        value = json.loads(path.read_text())
        value["cryptoType"] = str(code)
        if corrupt:
            value["ss58Address"] = "invalid-address"
        path.write_text(json.dumps(value))
    before = {p: p.read_bytes() for p in wallet_path.rglob("*") if p.is_file()}
    checked = []

    def registration(_ctx, records, **_kwargs):
        checked.extend(record for record in records if record.get("receiving_address"))

    monkeypatch.setattr(wallet_addresses, "check_registration", registration)
    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_: pytest.fail("must not unlock keys"))
    listed = runner.invoke(app, ["--json", "wallet", "list"])
    assert listed.exit_code == 0, (listed.output, listed.exception)
    row = json.loads(listed.output)["coldkeys"][0]
    row = row if role == "coldkey" else row["hotkeys"][0]
    if corrupt:
        assert row["ss58"] is None
        assert "public key unavailable" in row["error"]
        assert not row.get("receiving_address")
    else:
        assert row["ss58"] == public.ss58_address
        if requires_registration(public):
            assert row["primary_address"] == receiving_address(public)
            assert row["registration"] == "not_checked"
            assert row in checked
        elif code == 8:
            assert row["crypto_type"] == "ms"
            assert "receiving_address" not in row
            assert "registration" not in row
            assert row not in checked
        else:
            assert row["crypto_type"] == ("ed25519" if code == 0 else "sr25519")
    assert before == {p: p.read_bytes() for p in wallet_path.rglob("*") if p.is_file()}


@pytest.mark.parametrize("shielded", [False, True])
def test_encrypted_cli_sender_keeps_local_registration_after_unlock(
    recipient, fake, tmp_path, monkeypatch, shielded
):
    directory = tmp_path / "encrypted-wallets"
    sender = Wallet("sender", path=str(directory))
    sender.regenerate_coldkey(
        seed=bytes([63]) * 32, coldkey_password="test-only-password", suppress=True
    )
    sender.regenerate_hotkey(seed=bytes([64]) * 32, suppress=True)
    destination = Wallet("destination", path=str(directory))
    destination.coldkeypub_file.set_keypair(recipient.public_only(), encrypt=False)
    password = tmp_path / "password"
    password.write_text("test-only-password")
    monkeypatch.setenv("BT_WALLET_PATH", str(directory))
    monkeypatch.setenv("BT_WALLET", "sender")
    assert sender.coldkey_file.is_encrypted()
    if shielded:
        fake.mev_key = bytes(1184)
        fake.seed("MevShield", "NextKey", [], fake.mev_key)
    before = {p: p.read_bytes() for p in directory.rglob("*") if p.is_file()}
    previews = []
    preflight = Client.preflight

    async def preview(self, *args, **kwargs):
        result = await preflight(self, *args, **kwargs)
        previews.append(result)
        return result

    monkeypatch.setattr(Client, "preflight", preview)
    signed = []
    sign = fake.sign_extrinsic

    async def capture(call, keypair, **kwargs):
        signed.append(call)
        return await sign(call, keypair, **kwargs)

    monkeypatch.setattr(fake, "sign_extrinsic", capture)
    result = runner.invoke(
        app,
        [
            "--json",
            "--yes",
            "--network",
            "local",
            "--mev-shield" if shielded else "--no-mev-shield",
            "wallet",
            "transfer",
            "--wallet-password-file",
            str(password),
            "--dest",
            recipient.ss58_address,
            "--amount",
            "1",
            "--max-registration-deposit",
            "0.2",
        ],
    )
    assert result.exit_code == 0, (result.output, result.exception)
    assert any(
        plan.registration_sponsorship is not None for plan in previews
    ) == requires_registration(recipient)
    if shielded:
        assert fake.last_call.module == "MevShield"
        call = signed[0]
    else:
        call = fake.last_call
    if requires_registration(recipient):
        assert (call.module, call.function) == ("Utility", "batch_all")
        registration, transfer = call.params["calls"]
        assert registration.function == "register"
        assert registration.params["descriptor"] == registration_descriptor_value(
            bytes(recipient.hashed_descriptor)
        )
    else:
        transfer = call
        assert transfer.module == "Balances"
    assert transfer.params["dest"] == recipient.ss58_address
    assert len(fake.submissions) == 1
    assert fake.submissions[0][1] == sender.coldkeypub.ss58_address
    assert before == {p: p.read_bytes() for p in directory.rglob("*") if p.is_file()}


@pytest.mark.parametrize("code", [0, 1])
@pytest.mark.parametrize("reference", ["explicit", "configured", "signatory", "fallback"])
@pytest.mark.parametrize("operation", ["claim", "move"])
@pytest.mark.parametrize("interactive", [False, True])
def test_root_commands_preserve_vault_scheme(
    wallet_path, monkeypatch, code, reference, operation, interactive
):
    from bittensor.cli.commands import root, root_move

    key = Keypair.create_from_seed(bytes([code + 65]) * 32, code)
    wallet = Wallet("recipient", path=str(wallet_path))
    wallet.coldkeypub_file.set_keypair(key.public_only(), encrypt=False)
    command = root if operation == "claim" else root_move
    position = RootPosition(ALICE_HOT, Balance.from_tao(10), Balance.from_tao(1))

    async def positions(_client, owner):
        assert owner == key.ss58_address
        return [position]

    async def read(*args, **kwargs):
        return {"beta_price_tao": 1.0}

    monkeypatch.setattr(command, "interactive", lambda _: interactive)
    monkeypatch.setattr(command, "fetch_root_positions", positions)
    monkeypatch.setattr(command, "pick_claim_hotkey", lambda *a, **kw: position)
    monkeypatch.setattr(
        command, "local_address_names", lambda _: {ALICE_HOT: "source", BOB_HOT: "destination"}
    )
    monkeypatch.setattr(
        AppContext, "run", lambda self, fn: asyncio.run(fn(SimpleNamespace(read=read)))
    )
    submissions = []

    def submit(ctx, intent, **_kwargs):
        signer = ctx.vault_signer()
        assert signer.crypto_type == code
        assert signer.ss58_address == key.ss58_address
        payload = b"root operation approved by the selected Vault key"
        signature = key.sign(payload)
        assert signer._validated_signature(signature.hex(), SimpleNamespace(payload=payload))
        assert signer._server is None
        submissions.append(intent)

    monkeypatch.setattr(AppContext, "submit", submit)
    args = ["--json", "--yes", "--signer", "vault"]
    if reference == "explicit":
        args += ["--signer-address", "recipient"]
    elif reference == "configured":
        config.set_value("signer_address", "recipient")
    args += ["root", operation, "--coldkey", key.ss58_address]
    if reference == "signatory":
        args += ["--signatory", "recipient"]
    if operation == "claim" and not interactive:
        args += ["--hotkey", ALICE_HOT]
    if operation == "move":
        args += ["--to", BOB_HOT]
        if not interactive:
            args += ["--from", ALICE_HOT]
    result = runner.invoke(app, args)
    assert result.exit_code == 0, (result.output, result.exception)
    assert len(submissions) == 1
