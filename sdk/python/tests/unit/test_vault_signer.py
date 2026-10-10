"""Vault scheme boundaries are checked before any browser/device interaction."""

from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from bittensor.cli import context as context_mod
from bittensor.cli.context import AppContext
from bittensor.cli.output import Output
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from bittensor.vault.signer import VaultError, VaultSigner


@pytest.fixture
def ctx(tmp_path, monkeypatch):
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    monkeypatch.setenv("BTCLI_ADDRESSES_PATH", str(tmp_path / "addresses.json"))
    return AppContext(
        "local",
        "member",
        "default",
        str(tmp_path),
        True,
        False,
        Output(json_mode=True),
        signer_backend="vault",
    )


@pytest.mark.parametrize("code", [4, 5, 6, 7])
@pytest.mark.parametrize("reference", ["wallet", "fallback", "descriptor", "contact"])
def test_cli_rejects_unsupported_vault_member(ctx, monkeypatch, code, reference):
    key = Keypair.create_from_seed(bytes([code]) * 32, code)
    monkeypatch.setattr(
        context_mod.wallets,
        "open_wallet",
        lambda *a, **kw: SimpleNamespace(coldkeypub=key.public_only()),
    )
    if reference == "wallet":
        ctx.signatory_wallet = "member"
    elif reference == "descriptor":
        ctx.signer_address = receiving_address(key)
    elif reference == "contact":
        ctx.signer_address = "contact"
        monkeypatch.setattr(context_mod.cfg, "get_address", lambda ref: receiving_address(key))
    with pytest.raises(ValueError, match=r"standard sr25519/ed25519.*NAME=wallet"):
        ctx.vault_signer()
    assert ctx._vault_signer is None


@pytest.mark.parametrize("code", [0, 1])
def test_cli_preserves_standard_member_scheme(ctx, monkeypatch, code):
    key = Keypair.create_from_seed(bytes([code + 1]) * 32, code)
    monkeypatch.setattr(
        context_mod.wallets,
        "open_wallet",
        lambda *a, **kw: SimpleNamespace(coldkeypub=key.public_only()),
    )
    ctx.signatory_wallet = "member"
    signer = ctx.vault_signer()
    assert signer.crypto_type == code
    assert signer.ss58_address == key.ss58_address
    assert signer._server is None


@pytest.mark.parametrize("code", [2, 4, 5, 6, 7])
def test_sdk_rejects_unsupported_scheme_before_address_or_device(code):
    with pytest.raises(VaultError, match="standard sr25519/ed25519"):
        VaultSigner("unused", crypto_type=code)


@pytest.mark.parametrize("code", [0, 1])
def test_scanned_signatures_are_bound_to_member_scheme_and_payload(code):
    key = Keypair.create_from_seed(bytes([code + 1]) * 32, code)
    signer = VaultSigner(key.ss58_address, crypto_type=code)
    unsigned = SimpleNamespace(payload=b"the exact multisig approval")
    signature = key.sign(unsigned.payload)
    for result in (signature, bytes([code]) + signature):
        assert signer._validated_signature(result.hex(), unsigned) == result
    with pytest.raises(VaultError, match="different crypto type"):
        signer._validated_signature((bytes([1 - code]) + signature).hex(), unsigned)
    with pytest.raises(VaultError, match="does not verify"):
        signer._validated_signature(signature.hex(), SimpleNamespace(payload=b"another approval"))


@pytest.mark.parametrize("code", [0, 1, 5, 7])
def test_chained_round_keeps_vault_member_metadata(ctx, monkeypatch, code):
    key = Keypair.create_from_seed(bytes([code + 1]) * 32, code)
    monkeypatch.setattr(
        context_mod.wallets,
        "open_wallet",
        lambda *a, **kw: SimpleNamespace(coldkeypub=key.public_only()),
    )
    ctx.output = Mock()
    ctx.run = Mock()
    selected = []

    def submit(*args, **kwargs):
        if ctx.signer_backend == "vault":
            selected.append(ctx.vault_signer().crypto_type)
        return SimpleNamespace(events=[], data={"multisig_call_data": "0x0000"})

    ctx.submit = submit
    plan = ("team", 2, [], [("local", "unused", "wallet"), ("member", key.ss58_address, "vault")])
    if code in (0, 1):
        ctx._submit_signatory_rounds(None, plan)
        assert selected == [code]
    else:
        with pytest.raises(ValueError, match="standard sr25519/ed25519"):
            ctx._submit_signatory_rounds(None, plan)
        assert not selected
