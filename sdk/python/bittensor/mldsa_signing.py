"""Versioned offline/device signing requests for registered ML-DSA accounts.

Devices must explicitly implement this protocol. It carries the full native
payload and authority snapshot; legacy Polkadot signer payloads and their
256-byte hashing convention cannot authorize this account profile.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

from ._transport.contract import HashedSigningSnapshot, SignedExtrinsic, UnsignedExtrinsic
from .mldsa import supports_fixed_profile
from .sp_core import (
    MLDSA_CRYPTO_TYPES,
    Keypair,
    ss58_decode,
    verify_mldsa_authorization,
)

FORMAT = "bittensor/ms/v2/extrinsic/1"
MAX_NATIVE_PAYLOAD = 16 * 1024 * 1024


def _bytes(value):
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


@dataclass(frozen=True)
class PreparedMldsaExtrinsic:
    unsigned: UnsignedExtrinsic
    authority: HashedSigningSnapshot

    @property
    def address(self):
        return self.unsigned.address

    @property
    def payload(self):
        """Complete native payload; the signer adds its canonical ML-DSA header."""
        return self.unsigned.payload

    @property
    def spec_version(self):
        return self.unsigned.spec_version

    def validate(self):
        unsigned, authority = self.unsigned, self.authority
        public = Keypair.from_hashed_descriptor(authority.descriptor)
        if (
            unsigned.crypto_type not in MLDSA_CRYPTO_TYPES
            or authority.crypto_type != unsigned.crypto_type
            or public.crypto_type != unsigned.crypto_type
            or unsigned.nonce != authority.nonce
            or unsigned.public_key != bytes(public.public_key)
            or bytes(ss58_decode(unsigned.address)) != unsigned.public_key
            or bytes(ss58_decode(authority.address)) != unsigned.public_key
            or not authority.block_hash
            or not 0 <= authority.generation < 2**64 - 1
            or len(authority.commitment) != 64
            or not any(authority.commitment)
            or len(_bytes(unsigned.genesis_hash)) != 32
            or len(_bytes(unsigned.era_block_hash)) != 32
            or len(unsigned.payload) > MAX_NATIVE_PAYLOAD
            or unsigned.payload
            != unsigned.call_data
            + unsigned.included_in_extrinsic
            + unsigned.included_in_signed_data
        ):
            raise ValueError("invalid versioned ML-DSA signing request")

    def to_dict(self):
        self.validate()
        authority = asdict(self.authority)
        for name in ("descriptor", "commitment"):
            authority[name] = "0x" + authority[name].hex()
        return {"format": FORMAT, "unsigned": self.unsigned.to_dict(), "authority": authority}

    @classmethod
    def from_dict(cls, data):
        if (
            not isinstance(data, dict)
            or set(data) != {"format", "unsigned", "authority"}
            or data["format"] != FORMAT
        ):
            raise ValueError("unsupported ML-DSA signing request format")
        authority = dict(data["authority"])
        for name in ("descriptor", "commitment"):
            authority[name] = _bytes(authority[name])
        result = cls(
            UnsignedExtrinsic.from_dict(data["unsigned"]), HashedSigningSnapshot(**authority)
        )
        result.validate()
        return result


def prepare_mldsa_extrinsic(
    codec,
    call,
    authority,
    *,
    era,
    genesis_hash,
    era_block_hash,
    tip=0,
    tip_asset_id=None,
    metadata_hash=None,
):
    call_data, extra, implicit = codec.signature_payload_parts(
        call,
        era=era,
        nonce=authority.nonce,
        tip=tip,
        tip_asset_id=tip_asset_id,
        genesis_hash=genesis_hash,
        era_block_hash=era_block_hash,
        metadata_hash=metadata_hash,
    )
    unsigned = UnsignedExtrinsic(
        call_data=call_data,
        address=authority.address,
        public_key=bytes(ss58_decode(authority.address)),
        crypto_type=authority.crypto_type,
        era=era,
        nonce=authority.nonce,
        tip=tip,
        tip_asset_id=tip_asset_id,
        genesis_hash=genesis_hash,
        era_block_hash=era_block_hash,
        spec_version=codec.spec_version,
        transaction_version=codec.transaction_version,
        metadata_hash=metadata_hash,
        payload=call_data + extra + implicit,
        payload_json={"format": FORMAT},
        included_in_extrinsic=extra,
        included_in_signed_data=implicit,
    )
    prepared = PreparedMldsaExtrinsic(unsigned, authority)
    prepared.validate()
    if authority.descriptor[1] == 5:
        profiles = codec.constant("HashedAccounts", "MldsaAccountProfiles")
        if not supports_fixed_profile(profiles):
            raise ValueError("runtime does not support fixed-key ML-DSA accounts")
    return prepared


def sign_prepared_mldsa(
    prepared: PreparedMldsaExtrinsic, keypair, *, codec, expected_genesis_hash: str
) -> bytes:
    """Offline signing after the operator/device reviews the full request.

    The caller supplies independently trusted runtime metadata and chain ID.
    The codec reconstructs every extension byte from the displayed fields
    before private signing. A request is not a proof that its included chain
    snapshot is authentic; the connection revalidates authority at submission.
    """
    prepared.validate()
    if _bytes(expected_genesis_hash) != _bytes(prepared.unsigned.genesis_hash):
        raise ValueError("ML-DSA request does not match the trusted signing chain")
    _validate_codec_request(codec, prepared)
    if (
        keypair.crypto_type not in MLDSA_CRYPTO_TYPES
        or bytes(keypair.hashed_descriptor) != prepared.authority.descriptor
    ):
        raise ValueError("ML-DSA signing request belongs to another wallet")
    current = keypair.at_generation(prepared.authority.generation)
    if bytes(current.hashed_current_commitment) != prepared.authority.commitment:
        raise ValueError("offline key does not match the requested authority commitment")
    return bytes(current.sign_mldsa(prepared.payload))


def _validate_codec_request(codec, prepared: PreparedMldsaExtrinsic):
    """Reconstruct native bytes from reviewed fields and trusted metadata."""
    prepared.validate()
    unsigned, authority = prepared.unsigned, prepared.authority
    if (
        codec.spec_version != unsigned.spec_version
        or codec.transaction_version != unsigned.transaction_version
    ):
        raise ValueError("runtime changed since ML-DSA preparation; prepare and sign again")
    rebuilt = prepare_mldsa_extrinsic(
        codec,
        unsigned.call_data,
        authority,
        era=unsigned.era,
        genesis_hash=unsigned.genesis_hash,
        era_block_hash=unsigned.era_block_hash,
        tip=unsigned.tip,
        tip_asset_id=unsigned.tip_asset_id,
        metadata_hash=unsigned.metadata_hash,
    )
    if rebuilt.payload != prepared.payload:
        raise ValueError("ML-DSA prepared call, extensions or implicit values changed")


def attach_mldsa_proof(codec, prepared: PreparedMldsaExtrinsic, proof: bytes | str):
    """Verify a proof and assemble signed-v4; caller rechecks current authority."""
    _validate_codec_request(codec, prepared)
    unsigned, authority = prepared.unsigned, prepared.authority
    proof = _bytes(proof)
    if not verify_mldsa_authorization(
        proof, prepared.payload, authority.descriptor, authority.generation, authority.commitment
    ):
        raise ValueError("invalid ML-DSA transaction proof")
    data, extrinsic_hash = codec.encode_signed_extrinsic(
        unsigned.call_data,
        public_key=unsigned.public_key,
        signature=proof,
        signature_version=authority.crypto_type,
        era=unsigned.era,
        nonce=unsigned.nonce,
        tip=unsigned.tip,
        tip_asset_id=unsigned.tip_asset_id,
        metadata_hash_enabled=unsigned.metadata_hash is not None,
    )
    return SignedExtrinsic(data=data, extrinsic_hash=extrinsic_hash)
