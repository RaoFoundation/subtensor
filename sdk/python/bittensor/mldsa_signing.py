"""Versioned offline/device signing requests for post-quantum accounts.

Devices must explicitly implement this protocol. It carries the full native
payload and authority snapshot; legacy Polkadot signer payloads and their
256-byte hashing convention cannot authorize this account profile.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

from ._transport.contract import HashedSigningSnapshot, SignedExtrinsic, UnsignedExtrinsic
from .mldsa import supports_fixed_profile
from .sp_core import (
    FALCON_CRYPTO_TYPES,
    PQ_CRYPTO_TYPES,
    Keypair,
    is_stateless_pq_descriptor,
    ss58_decode,
    verify_mldsa_authorization,
)

FORMAT = "bittensor/ms/v2/extrinsic/1"
PQ_FORMAT = "bittensor/pq/extrinsic/1"
MAX_NATIVE_PAYLOAD = 16 * 1024 * 1024


def _format(crypto_type):
    return PQ_FORMAT if crypto_type in FALCON_CRYPTO_TYPES else FORMAT


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
        """Complete native payload; the signer adds its canonical post-quantum header."""
        return self.unsigned.payload

    @property
    def spec_version(self):
        return self.unsigned.spec_version

    def validate(self):
        unsigned, authority = self.unsigned, self.authority
        public = Keypair.from_hashed_descriptor(authority.descriptor)
        if (
            unsigned.crypto_type not in PQ_CRYPTO_TYPES
            or authority.crypto_type != unsigned.crypto_type
            or public.crypto_type != unsigned.crypto_type
            or unsigned.nonce != authority.nonce
            or unsigned.public_key != bytes(public.public_key)
            or bytes(ss58_decode(unsigned.address)) != unsigned.public_key
            or bytes(ss58_decode(authority.address)) != unsigned.public_key
            or not authority.block_hash
            or (
                is_stateless_pq_descriptor(authority.descriptor)
                and (
                    authority.generation != 0
                    or authority.commitment != authority.descriptor[2:]
                    or not 0 <= authority.nonce < 2**32 - 1
                )
            )
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
            raise ValueError("invalid versioned post-quantum signing request")

    def to_dict(self):
        self.validate()
        authority = asdict(self.authority)
        for name in ("descriptor", "commitment"):
            authority[name] = "0x" + authority[name].hex()
        return {
            "format": _format(self.unsigned.crypto_type),
            "unsigned": self.unsigned.to_dict(),
            "authority": authority,
        }

    @classmethod
    def from_dict(cls, data):
        if (
            not isinstance(data, dict)
            or set(data) != {"format", "unsigned", "authority"}
            or data["format"] not in (FORMAT, PQ_FORMAT)
        ):
            raise ValueError("unsupported post-quantum signing request format")
        authority = dict(data["authority"])
        for name in ("descriptor", "commitment"):
            authority[name] = _bytes(authority[name])
        result = cls(
            UnsignedExtrinsic.from_dict(data["unsigned"]), HashedSigningSnapshot(**authority)
        )
        result.validate()
        if data["format"] != _format(result.unsigned.crypto_type):
            raise ValueError("signing request format does not match its cryptographic profile")
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
        payload_json={"format": _format(authority.crypto_type)},
        included_in_extrinsic=extra,
        included_in_signed_data=implicit,
    )
    prepared = PreparedMldsaExtrinsic(unsigned, authority)
    prepared.validate()
    profile = authority.descriptor[1]
    if profile in (5, 6, 7, 8, 9):
        constant = "FalconAccountProfiles" if profile in (6, 7, 8, 9) else "MldsaAccountProfiles"
        profiles = codec.constant("HashedAccounts", constant)
        if not supports_fixed_profile(profiles, profile):
            raise ValueError("runtime does not support this post-quantum account profile")
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
        raise ValueError("post-quantum request does not match the trusted signing chain")
    _validate_codec_request(codec, prepared)
    if (
        keypair.crypto_type not in PQ_CRYPTO_TYPES
        or bytes(keypair.hashed_descriptor) != prepared.authority.descriptor
    ):
        raise ValueError("post-quantum signing request belongs to another wallet")
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
        raise ValueError("runtime changed since post-quantum preparation; prepare and sign again")
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
        raise ValueError("post-quantum prepared call, extensions or implicit values changed")


def attach_mldsa_proof(codec, prepared: PreparedMldsaExtrinsic, proof: bytes | str):
    """Verify a proof and assemble signed-v4; caller rechecks current authority."""
    _validate_codec_request(codec, prepared)
    unsigned, authority = prepared.unsigned, prepared.authority
    proof = _bytes(proof)
    if not verify_mldsa_authorization(
        proof, prepared.payload, authority.descriptor, authority.generation, authority.commitment
    ):
        raise ValueError("invalid post-quantum transaction proof")
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


# Generic names for new integrations; existing MS entrypoints remain compatible.
PreparedPqExtrinsic = PreparedMldsaExtrinsic
prepare_pq_extrinsic = prepare_mldsa_extrinsic
sign_prepared_pq = sign_prepared_mldsa
attach_pq_proof = attach_mldsa_proof
