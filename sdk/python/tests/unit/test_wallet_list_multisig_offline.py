"""Saved multisig addresses are derived locally from complete signer sets."""

from __future__ import annotations

import json
from types import SimpleNamespace

import pytest
from typer.testing import CliRunner

from bittensor import config
from bittensor._transport.codec import multisig_account
from bittensor.cli.context import AppContext
from bittensor.cli.main import app
from bittensor.cli.multisig_helpers import multisig_list_records
from bittensor.keyfiles import Keyfile
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.sp_core import Keypair
from bittensor.wallet import Wallet
from tests.harness.samples import ALICE, BOB


@pytest.fixture
def isolated_config(tmp_path, monkeypatch):
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
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path / "wallets"))
    monkeypatch.setenv("BT_WALLET", "local-test")
    monkeypatch.setenv("BT_NETWORK", "finney")
    return tmp_path


@pytest.mark.parametrize("code", [4, 6, 7, 8])
def test_offline_list_derives_complete_descriptor_signer_set(isolated_config, monkeypatch, code):
    public = Keypair.create_from_seed(bytes([code]) * 32, code).public_only()
    config.add_multisig(
        {"name": "complete", "threshold": 2, "signatories": [receiving_address(public), ALICE]}
    )
    config.add_multisig(
        {"name": "incomplete", "threshold": 2, "signatories": [ALICE, BOB, "missing-wallet"]}
    )
    monkeypatch.setattr(
        AppContext,
        "run",
        lambda *_args, **_kwargs: pytest.fail("offline wallet listing must not connect"),
    )
    result = CliRunner().invoke(app, ["--yes", "--json", "wallet", "list", "--offline"])
    assert result.exit_code == 0, result.output
    records = {row["name"]: row for row in json.loads(result.output)["multisigs"]}
    assert (
        records["complete"]["ss58"]
        == multisig_account([public.ss58_address, ALICE], 2).ss58_address
    )
    assert records["complete"]["signatories"] == [
        {"name": receiving_address(public), "ss58": public.ss58_address},
        {"name": ALICE, "ss58": ALICE},
    ]
    assert records["incomplete"]["ss58"] is None
    assert records["incomplete"]["signatory_count"] == 3
    assert records["incomplete"]["signatories"][-1] == {"name": "missing-wallet", "ss58": None}


def test_multisig_list_keeps_unresolved_or_invalid_sets_unavailable(isolated_config):
    context = SimpleNamespace(
        wallet_path=str(isolated_config / "wallets"),
        identity_address=lambda value: parse_recipient(value).account,
    )
    entries = [
        {"name": "threshold-too-large", "threshold": 3, "signatories": [ALICE, BOB]},
        {"name": "duplicate", "threshold": 2, "signatories": [ALICE, ALICE]},
        {"name": "empty", "threshold": 1, "signatories": []},
        {"name": "partial", "threshold": 1, "signatories": [ALICE, "missing-wallet"]},
    ]
    records = multisig_list_records(context, entries)
    assert len(records) == len(entries)
    assert all(record["ss58"] is None for record in records)


@pytest.mark.parametrize("code", [0, 1, 4, 6, 7, 8])
@pytest.mark.parametrize("encrypted", [False, True])
def test_offline_multisig_local_member_uses_only_unencrypted_public_metadata(
    isolated_config, monkeypatch, code, encrypted
):
    wallet = Wallet("member", path=str(isolated_config / "wallets"))
    wallet.regenerate_coldkey(
        seed=bytes([46]) * 32, crypto_type=code, use_password=False, suppress=True
    )
    expected = wallet.coldkeypub.ss58_address
    path = isolated_config / "wallets/member/coldkeypub.txt"
    if encrypted:
        path.write_bytes(b"$NACL\xfftest encrypted public metadata")
    else:
        data = json.loads(path.read_text())
        # Invalid secrets must not be interpreted while deriving public multisig identity.
        data.update(secretPhrase="not a mnemonic", secretSeed="invalid", privateKey="invalid")
        path.write_text(json.dumps(data))
    before = path.read_bytes()
    config.add_multisig({"name": "local-pair", "threshold": 2, "signatories": ["member", ALICE]})
    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_: pytest.fail("must not unlock keys"))
    monkeypatch.setattr(
        AppContext,
        "run",
        lambda *_args, **_kwargs: pytest.fail("offline listing must not connect"),
    )

    result = CliRunner().invoke(app, ["--yes", "--json", "wallet", "list", "--offline"])

    assert result.exit_code == 0, result.output
    record = json.loads(result.output)["multisigs"][0]
    if encrypted:
        assert record["ss58"] is None
        assert record["signatories"][0] == {"name": "member", "ss58": None}
    else:
        assert record["ss58"] == multisig_account([expected, ALICE], 2).ss58_address
        assert record["signatories"][0] == {"name": "member", "ss58": expected}
    assert "not a mnemonic" not in result.output
    assert path.read_bytes() == before
