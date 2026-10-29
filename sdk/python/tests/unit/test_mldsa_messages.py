"""PQ wallet keys fail closed for arbitrary messages and HTTP authentication."""

import pytest

from bittensor import http_auth, wallets
from bittensor.sp_core import Keypair


@pytest.fixture(params=[7, 8, 9, 10, 11, 12])
def key(request):
    return Keypair.create_from_seed(bytes([82]) * 32, request.param)


def test_pq_message_signing_is_unsupported(key):
    with pytest.raises(ValueError, match="post-quantum arbitrary message signing is not supported"):
        wallets.sign_message_key("hello", key)


def test_pq_http_signing_is_unsupported(key):
    with pytest.raises(ValueError, match="post-quantum HTTP signing is not supported"):
        http_auth.sign(key, method="POST", path="/infer", body=b"request")


async def test_pq_http_signer_preparation_is_unsupported(key):
    with pytest.raises(ValueError, match="post-quantum HTTP signing is not supported"):
        await http_auth.prepare_signer(key, client=None)


def test_pq_http_verification_is_unsupported(key):
    with pytest.raises(ValueError, match="post-quantum HTTP authentication is not supported"):
        http_auth.verify(
            {},
            b"request",
            method="POST",
            path="/infer",
            self_hotkey_ss58=key.ss58_address,
            expected_crypto_type=key.crypto_type,
        )


def test_pq_message_verification_does_not_accept_transaction_proofs(key):
    proof = bytes(key.sign_mldsa(b"hello"))
    assert not wallets.verify_message("hello", proof.hex(), key.ss58_address, key.crypto_type)


def test_cli_pq_message_signing_fails_without_querying_chain(key, tmp_path, monkeypatch):
    from typer.testing import CliRunner

    from bittensor.cli.context import AppContext
    from bittensor.cli.main import app
    from bittensor.wallet import Wallet

    wallet = Wallet("pq", path=str(tmp_path))
    wallet.coldkey_file.set_keypair(key, encrypt=False)
    wallet.coldkeypub_file.set_keypair(key.public_only(), encrypt=False)
    monkeypatch.setattr(AppContext, "run", lambda *_: pytest.fail("must not query the chain"))
    result = CliRunner().invoke(
        app,
        [
            "--yes",
            "--json",
            "--wallet-path",
            str(tmp_path),
            "--wallet",
            "pq",
            "wallet",
            "sign",
            "--message",
            "hello",
        ],
    )
    assert result.exit_code == 1, result.output
    assert "post-quantum arbitrary message signing is not supported" in result.output
