"""Falcon modes preserve native account, recipient, and Shield lifecycle boundaries."""

import json
from hashlib import blake2b
from unittest.mock import AsyncMock, Mock

import bittensor_core
import pytest

from bittensor import wallets
from bittensor._generated import calls
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.extrinsics import SHIELD_ENCRYPTION_OVERHEAD, hashed_proof_length
from bittensor.cli.wallet_addresses import address_fields, public_key_from_file
from bittensor.hashed import (
    descriptor_bytes,
    descriptor_storage,
    descriptor_value,
    prepare_recipient_intent,
    registration_descriptor_value,
    with_recipient_registration,
)
from bittensor.intents import Transfer
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.sp_core import (
    FALCON_CRYPTO_TYPES,
    Keypair,
    account_crypto_type,
    is_stateless_pq,
    pq_lengths,
    requires_registration,
    serialized_keypair_to_keyfile_data,
    verify_mldsa_authorization,
)
from bittensor.wallet import _restore_public_key
from tests.conftest import golden_codec
from tests.harness.samples import dev_wallet
from tests.unit.test_mldsa_accounts import Codec, chain, connection, record
from tests.unit.test_stateless_mldsa import connection as fixed_connection


@pytest.fixture(params=FALCON_CRYPTO_TYPES)
def wallet(request):
    return Keypair.create_from_seed(bytes([73]) * 32, request.param)


def test_identity_keyfile_restore_and_watch_only_addresses(wallet, tmp_path):
    kind = wallet.crypto_type
    rotating = requires_registration(wallet)
    assert len(wallet.public_key) == 32
    assert len(wallet.hashed_descriptor) == 66
    assert len(wallet.hashed_public_key) == pq_lengths(kind)[0]
    assert len(wallet.hashed_current_commitment) == 64
    assert wallet.account_type == wallets.account_type_name(kind)
    assert wallet.signing_scheme == wallets.signing_scheme_name(kind)
    assert account_crypto_type(kind, wallet.account_type) == kind
    assert descriptor_bytes(descriptor_value(wallet.hashed_descriptor)) == wallet.hashed_descriptor
    assert descriptor_storage(wallet.hashed_descriptor) == "MlDsaAccounts"
    assert set(registration_descriptor_value(wallet.hashed_descriptor)) == {"Falcon"}
    assert receiving_address(wallet).startswith("tao_") is rotating
    assert parse_recipient(receiving_address(wallet)).account == wallet.ss58_address
    output = address_fields(wallet, network="local")
    assert ("receiving_address" in output) is rotating
    if rotating:
        assert output["receiving_address"] == receiving_address(wallet)
    else:
        assert output == {"ss58": wallet.ss58_address}
    restored = Keypair.create_from_seed(bytes([73]) * 32, kind)
    assert restored.hashed_descriptor == wallet.hashed_descriptor
    public_file = tmp_path / "public.json"
    data = json.loads(serialized_keypair_to_keyfile_data(wallet.public_only()))
    data["cryptoType"] = str(kind)  # Supported compatibility keyfiles.
    public_file.write_text(json.dumps(data))
    listed = public_key_from_file(public_file)
    assert listed.crypto_type == kind and listed.ss58_address == wallet.ss58_address
    assert receiving_address(listed) == receiving_address(wallet)
    recovered_public = _restore_public_key(receiving_address(wallet), None, kind)
    assert recovered_public.ss58_address == wallet.ss58_address
    if rotating:
        with pytest.raises(ValueError, match="complete receiving address"):
            _restore_public_key(wallet.ss58_address, None, kind)


@pytest.mark.parametrize("generation", [0, 7])
def test_native_proof_binds_full_payload_profile_and_authority(wallet, generation):
    if is_stateless_pq(wallet) and generation:
        with pytest.raises(ValueError):
            wallet.at_generation(generation)
        return
    signer = wallet.at_generation(generation)
    payload = b"complete transaction bytes" * 100
    proof = bytes(signer.sign_mldsa(payload))
    assert len(proof) == hashed_proof_length(wallet.crypto_type)
    assert proof[:32] == bytes(wallet.public_key)
    assert proof[32:34] == bytes(wallet.hashed_descriptor[:2])
    authority = (wallet.hashed_descriptor, generation, signer.hashed_current_commitment)
    assert verify_mldsa_authorization(proof, payload, *authority)
    assert not verify_mldsa_authorization(proof, payload + b"changed", *authority)
    assert not verify_mldsa_authorization(proof + b"\0", payload, *authority)
    assert not verify_mldsa_authorization(
        proof, payload, authority[0], generation + 1, authority[2]
    )
    other_mode = "hashed" if is_stateless_pq(wallet) else "standard"
    changed = Keypair.create_from_seed(
        bytes([73]) * 32, account_crypto_type(wallet.crypto_type, other_mode)
    ).hashed_descriptor
    assert not verify_mldsa_authorization(proof, payload, changed, generation, authority[2])


async def test_recipient_registers_only_rotating_profiles_with_single_reserve(wallet):
    substrate = chain()
    substrate.query = AsyncMock(wraps=substrate.query)
    substrate.seed_constant("HashedAccounts", "MlDsaRegistrationDeposit", 200_000_000)
    payment = await substrate.compose(
        calls.Balances.transfer_keep_alive(dest=wallet.ss58_address, value=1)
    )
    intent, recipients = await prepare_recipient_intent(
        substrate, Transfer(dest_ss58=receiving_address(wallet), amount_tao=1)
    )
    guarded, extras = await with_recipient_registration(
        substrate, dev_wallet(), intent, payment, recipients=recipients, as_calls=True
    )
    if is_stateless_pq(wallet):
        assert guarded == [payment] and extras == {}
        assert not any(
            call.args[:2] == ("HashedAccounts", "MlDsaAccounts")
            for call in substrate.query.call_args_list
        )
    else:
        assert [call.function for call in guarded] == ["register", "transfer_keep_alive"]
        assert guarded[0].params["descriptor"] == registration_descriptor_value(
            wallet.hashed_descriptor
        )
        assert extras["hashed_registration_max_deposit_rao"] == 200_000_000
        substrate.seed("HashedAccounts", "MlDsaAccounts", [wallet.ss58_address], record(wallet))
        guarded, extras = await with_recipient_registration(
            substrate, dev_wallet(), intent, payment, recipients=recipients, as_calls=True
        )
        assert guarded[0].function == "check_registered"
        assert extras["hashed_registration_max_deposit_rao"] == 0


async def test_missing_profile_or_disabled_master_rejects_before_signing(wallet):
    conn, codec, _ = connection(wallet)
    original = codec.constant
    codec.constant = lambda pallet, name: (
        None if name == "FalconAccountProfiles" else original(pallet, name)
    )
    private = Mock(wraps=wallet)
    private.crypto_type = wallet.crypto_type
    private.hashed_descriptor = wallet.hashed_descriptor
    with pytest.raises(SubstrateRequestException, match="profile"):
        await conn.create_signed_extrinsic(b"call", private)
    private.at_generation.assert_not_called()
    conn._nonces.next_for.assert_not_awaited()


@pytest.mark.parametrize("crypto_type", [9, 11])
@pytest.mark.parametrize("nonce", [63, 16383])
@pytest.mark.parametrize("carrier_size", [8192, 8193, 32768, 32769])
async def test_fixed_shield_quotes_next_nonce_and_exact_carrier_tier(
    crypto_type, nonce, carrier_size
):
    class CompactCodec(Codec):
        def encode_signed_extrinsic(self, call, **kwargs):
            body, _ = super().encode_signed_extrinsic(call, **kwargs)
            body += golden_codec().encode_compact(kwargs["nonce"])
            body = golden_codec().encode_compact(len(body)) + body
            return body, "0x" + blake2b(body, digest_size=32).hexdigest()

    wallet = Keypair.create_from_seed(bytes([73]) * 32, crypto_type)
    conn, _ = fixed_connection(wallet, profiles=bytes([6, 7, 8, 9]), pending=nonce)
    codec = CompactCodec()
    conn._runtimes.codec_at.return_value = codec
    conn._normalize_era_at = AsyncMock(return_value=({"period": 8}, "0x" + "11" * 32))
    conn.compose_call = AsyncMock(return_value=b"carrier")
    conn.get_payment_info = AsyncMock(return_value={"partial_fee": 17})
    size = carrier_size - SHIELD_ENCRYPTION_OVERHEAD - hashed_proof_length(crypto_type) - 8
    for _ in range(4):
        call = bytes([42]) * size
        actual = await conn.create_signed_extrinsic(
            call, wallet, nonce=nonce + 1, era={"period": 8}
        )
        delta = carrier_size - SHIELD_ENCRYPTION_OVERHEAD - len(actual.data)
        if not delta:
            break
        size += delta
    encrypted = bittensor_core.encrypt_mlkem768(bytes(1184), actual.data, include_key_hash=True)
    assert len(encrypted) == carrier_size
    if carrier_size > 32768:
        with pytest.raises(ValueError, match="32768-byte carrier limit"):
            await conn.get_shielded_payment_info(call, wallet.public_only(), period=8)
        conn.compose_call.assert_not_awaited()
    else:
        await conn.get_shielded_payment_info(call, wallet.public_only(), period=8)
        assert codec.nonce == nonce + 1
        assert len(conn.compose_call.call_args.args[2]["ciphertext"]) == len(encrypted)
        assert conn.get_payment_info.call_args.kwargs["nonce"] == nonce
    assert all(call.args[0] != "HashedAccounts" for call in conn.query.call_args_list)


@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_fixed_address_only_watch_keys_quote_without_descriptor(crypto_type):
    wallet = Keypair.create_from_seed(bytes([73]) * 32, crypto_type)
    public = Keypair(ss58_address=wallet.ss58_address, crypto_type=crypto_type)
    conn, _ = fixed_connection(public, profiles=bytes([4, 5, 6, 7, 8, 9]))
    conn._runtimes.codec_at.return_value.constant = Mock(return_value=bytes([4, 5, 6, 7, 8, 9]))
    call = b"quote"
    quote = await conn.sign_without_nonce_tracking(call, public, nonce=63, signature=bytes(64))
    actual = await conn.sign_without_nonce_tracking(call, wallet, nonce=63)
    assert len(quote.data) == len(actual.data)
    assert quote.data != actual.data


@pytest.mark.parametrize("crypto_type", [8, 9, 10, 11, 12])
async def test_recipient_write_and_order_guards_register_only_rotating(crypto_type):
    from base64 import urlsafe_b64encode

    from bittensor.limit_orders import registered_fee_recipient
    from bittensor.receiving import account_for_registered_write
    from bittensor.sp_core import encode_hashed_receiving_address

    wallet = Keypair.create_from_seed(bytes([73]) * 32, crypto_type)
    address = encode_hashed_receiving_address(wallet.hashed_descriptor)
    substrate = chain()
    substrate.query = AsyncMock(wraps=substrate.query)
    for guard in (account_for_registered_write, registered_fee_recipient):
        if requires_registration(wallet):
            with pytest.raises(ValueError, match="registration is not finalized"):
                await guard(substrate, address)
        else:
            assert await guard(substrate, address) == wallet.ss58_address
    if requires_registration(wallet):
        substrate.seed("HashedAccounts", "MlDsaAccounts", [wallet.ss58_address], record(wallet))
        for guard in (account_for_registered_write, registered_fee_recipient):
            assert await guard(substrate, address) == wallet.ss58_address
    else:
        assert not any(
            call.args[:2] == ("HashedAccounts", "MlDsaAccounts")
            for call in substrate.query.call_args_list
        )
        assert address == wallet.ss58_address
        assert parse_recipient(address).descriptor is None
        descriptor = bytes(wallet.hashed_descriptor)
        checksum = blake2b(b"bittensor/ms/v2/receiving" + descriptor, digest_size=32).digest()[:8]
        invalid = "tao_" + urlsafe_b64encode(descriptor + checksum).decode().rstrip("=")
        with pytest.raises(ValueError, match="unsupported"):
            parse_recipient(invalid)
        substrate.seed("AdminUtils", "HashedAccountsEnabled", [], False)
        for guard in (account_for_registered_write, registered_fee_recipient):
            assert await guard(substrate, address) == wallet.ss58_address
