from dataclasses import replace

import pytest

from bittensor import http_auth, wallets
from bittensor.mldsa import (
    MessageCapability,
    capability_proof_length,
    install_message_capability,
    message_capability,
    user_message_payload,
    verify_payload,
)
from bittensor.sp_core import (
    ROLE_MESSAGE,
    Keypair,
    account_crypto_type,
    is_stateless_pq,
    pq_lengths,
)


@pytest.fixture(params=[7, 8, 9, 10, 11, 12])
def key(request):
    return Keypair.create_from_seed(bytes([82]) * 32, request.param)


@pytest.fixture
def capability(key):
    return MessageCapability(
        key.ss58_address,
        bytes([3]) * 32,
        1,
        bytes(key.mldsa_capability_commitment(ROLE_MESSAGE, 1)),
        b"test-server",
        1000,
        900,
    )


def test_message_signing_uses_owner_grant_and_never_transaction_key(key, capability):
    signed = wallets.sign_message_key("hello", key, message_capability=capability)
    proof = bytes.fromhex(signed["signature"])
    assert len(proof) == capability_proof_length(key.crypto_type)
    assert proof[75 : 75 + pq_lengths(key.crypto_type)[0]] != bytes(key.mldsa_public_key)
    assert wallets.verify_message(
        "hello",
        signed["signature"],
        key.ss58_address,
        key.crypto_type,
        message_capability=capability,
    )
    assert not wallets.verify_message(
        "changed",
        signed["signature"],
        key.ss58_address,
        key.crypto_type,
        message_capability=capability,
    )
    for changed in (
        replace(capability, epoch=2),
        replace(capability, genesis_hash=bytes(32)),
        replace(capability, audience=b"another-server"),
    ):
        assert not verify_payload(proof, user_message_payload(b"hello", changed), changed)
    with pytest.raises(ValueError, match="finalized"):
        wallets.sign_message_key("hello", key)
    assert key.hashed_generation == 0


def test_http_binary_proof_roundtrip_is_bounded_and_replay_safe(key, capability):
    nonce = 1_800_000_000_000_000_000
    request = http_auth.sign_mldsa(
        key,
        capability=capability,
        method="POST",
        path="/infer?q=1",
        receiver_ss58=key.ss58_address,
        body=b'{"input":42}',
        nonce_ns=nonce,
    )
    assert max(map(len, request.headers.values())) < 128
    store = http_auth.InMemoryNonceStore()
    args = dict(
        capability=capability,
        method="POST",
        path="/infer?q=1",
        self_hotkey_ss58=key.ss58_address,
        nonce_store=store,
        now_ns=nonce,
    )
    # A failed proof must not poison the nonce store.
    with pytest.raises(http_auth.BadSignature):
        http_auth.verify_mldsa(request.headers, request.body + b"changed", **args)
    authenticated = http_auth.verify_mldsa(request.headers, request.body, **args)
    assert authenticated.body == b'{"input":42}'
    assert authenticated.caller.hotkey_ss58 == key.ss58_address
    with pytest.raises(http_auth.ReplayedRequest):
        http_auth.verify_mldsa(request.headers, request.body, **args)
    with pytest.raises(http_auth.MalformedAuth):
        http_auth.verify_mldsa(request.headers, request.body, **{**args, "max_body_bytes": 1})


@pytest.mark.parametrize(
    "field,value", [("method", "GET"), ("path", "/other"), ("now_ns", 1_800_000_030_000_000_000)]
)
def test_http_changed_request_or_expired_nonce_rejected(key, capability, field, value):
    nonce = 1_800_000_000_000_000_000
    request = http_auth.sign_mldsa(
        key,
        capability=capability,
        method="POST",
        path="/infer",
        receiver_ss58=key.ss58_address,
        nonce_ns=nonce,
    )
    args = dict(
        capability=capability,
        method="POST",
        path="/infer",
        self_hotkey_ss58=key.ss58_address,
        now_ns=nonce,
        nonce_store=http_auth.InMemoryNonceStore(),
    )
    args[field] = value
    with pytest.raises(http_auth.AuthError):
        http_auth.verify_mldsa(request.headers, request.body, **args)


def test_grant_builder_commits_only_message_role_and_validates_bounds(key):
    call = install_message_capability(key, epoch=4, expires_at=100, audience=b"app")
    assert call.function == "install_capability"
    assert call.params["expected_epoch"] == 4
    assert call.params["scope"] == {"Message": {"audience": b"app"}}
    assert call.params["key_commitment"] == bytes(key.mldsa_capability_commitment(ROLE_MESSAGE, 4))
    with pytest.raises(ValueError):
        install_message_capability(key, epoch=0, expires_at=100, audience=b"app")
    with pytest.raises(ValueError):
        install_message_capability(key, epoch=4, expires_at=100, audience=b"")


def test_owner_approved_message_key_can_authenticate_a_multisig_identity(key, capability):
    from bittensor.sp_core import ss58_encode

    composite = ss58_encode(bytes([94]) * 32, 42)
    delegated = replace(capability, owner=composite)
    signed = wallets.sign_message_key("approved by quorum", key, message_capability=delegated)
    assert signed["ss58"] == composite
    assert wallets.verify_message(
        "approved by quorum",
        signed["signature"],
        composite,
        key.crypto_type,
        message_capability=delegated,
    )
    assert not wallets.verify_message(
        "approved by quorum",
        signed["signature"],
        key.ss58_address,
        key.crypto_type,
        message_capability=capability,
    )


@pytest.mark.asyncio
async def test_finalized_capability_rejects_revocation_expiry_and_wrong_audience(key, capability):
    class Chain:
        epoch = 1
        expiry = 1000
        enabled = True

        async def finalized_block_number(self):
            return 900

        async def block_hash(self, block):
            return "0x" + (bytes([3]) * 32 if block == 0 else bytes([4]) * 32).hex()

        async def query(self, module, name, params=None, block_hash=None):
            assert block_hash == "0x" + (bytes([4]) * 32).hex()
            if name == "HashedAccountsEnabled":
                return self.enabled
            if name == "CapabilityEpochs":
                return self.epoch
            assert params == [key.ss58_address, "Message"]
            return {
                "epoch": 1,
                "key_commitment": capability.commitment,
                "expires_at": self.expiry,
                "scope": {"Message": {"audience": b"test-server"}},
            }

    chain = Chain()
    chain.enabled = False
    with pytest.raises(ValueError, match="authorization is disabled"):
        await message_capability(chain, key.ss58_address, b"test-server")
    chain.enabled = True
    assert await message_capability(chain, key.ss58_address, b"test-server") == capability
    with pytest.raises(ValueError, match="audience"):
        await message_capability(chain, key.ss58_address, b"wrong")
    chain.epoch = 2
    with pytest.raises(ValueError, match="no active"):
        await message_capability(chain, key.ss58_address, b"test-server")
    chain.epoch = 1
    chain.expiry = 899
    with pytest.raises(ValueError, match="expired"):
        await message_capability(chain, key.ss58_address, b"test-server")


@pytest.mark.asyncio
async def test_missing_finalized_hash_cannot_fall_back_to_unfinalized_authority(key):
    from unittest.mock import AsyncMock

    chain = AsyncMock()
    chain.finalized_block_number.return_value = 900
    chain.block_hash.return_value = None
    with pytest.raises(ValueError, match="finalized block hash"):
        await message_capability(chain, key.ss58_address, b"test-server")
    chain.query.assert_not_awaited()


def test_receiving_message_verification_binds_wallet_mode(key, capability):
    from bittensor.receiving import receiving_address

    signed = wallets.sign_message_key("mode binding", key, message_capability=capability)
    address = receiving_address(key)
    assert wallets.verify_message(
        "mode binding", signed["signature"], address, key.crypto_type, message_capability=capability
    )
    # Bare SS58 carries no account mode; the finalized capability binds its owner.
    assert wallets.verify_message(
        "mode binding",
        signed["signature"],
        address,
        account_crypto_type(key.crypto_type, "hashed" if is_stateless_pq(key) else "standard"),
        message_capability=capability,
    ) is is_stateless_pq(key)


def test_cli_ms_verification_infers_descriptor_mode_and_rejects_override(
    key, capability, monkeypatch, tmp_path
):
    from typer.testing import CliRunner

    from bittensor.cli.context import AppContext
    from bittensor.cli.main import app
    from bittensor.receiving import receiving_address

    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    capability = replace(capability, audience=b"bittensor/wallet-message")
    monkeypatch.setattr(AppContext, "run", lambda *_: capability)
    signed = wallets.sign_message_key("mode binding", key, message_capability=capability)
    args = [
        "--yes",
        "--json",
        "wallet",
        "verify",
        "--message",
        "mode binding",
        "--signature",
        signed["signature"],
        "--ss58",
        receiving_address(key),
        "--crypto-type",
        "ms"
        if wallets.signing_scheme_name(key.crypto_type) == "mldsa65"
        else wallets.signing_scheme_name(key.crypto_type),
    ]
    assert CliRunner().invoke(app, args).exit_code == 0
    wrong = "hashed" if is_stateless_pq(key) else "standard"
    result = CliRunner().invoke(app, [*args, "--type", wrong])
    assert result.exit_code == (0 if is_stateless_pq(key) else 1), result.output
