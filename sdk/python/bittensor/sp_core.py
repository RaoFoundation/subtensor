"""Chain-matched key primitives backed by the in-repo ``bittensor_core`` binding."""

from __future__ import annotations

import bittensor_core as _backend

BACKEND = "bittensor_core"

CRYPTO_ED25519 = _backend.CRYPTO_ED25519
CRYPTO_SR25519 = _backend.CRYPTO_SR25519
Keypair = _backend.Keypair
KeyfileError = _backend.KeyfileError
WrongPasswordError = _backend.WrongPasswordError

# Native bittensor_core names
verify = _backend.verify
ss58_decode = _backend.ss58_decode
ss58_encode = _backend.ss58_encode
decrypt_keyfile_data = _backend.decrypt_keyfile_data
deserialize_keypair_from_keyfile_data = _backend.deserialize_keypair_from_keyfile_data
encrypt_keyfile_data = _backend.encrypt_keyfile_data
get_password_from_environment = _backend.get_password_from_environment
keyfile_data_encryption_method = _backend.keyfile_data_encryption_method
keyfile_data_is_encrypted = _backend.keyfile_data_is_encrypted
keyfile_data_is_encrypted_ansible = _backend.keyfile_data_is_encrypted_ansible
keyfile_data_is_encrypted_legacy = _backend.keyfile_data_is_encrypted_legacy
keyfile_data_is_encrypted_nacl = _backend.keyfile_data_is_encrypted_nacl
save_password_to_environment = _backend.save_password_to_environment
serialized_keypair_to_keyfile_data = _backend.serialized_keypair_to_keyfile_data

# Backwards-compatible aliases (pre-migration bittensor.sp_core API)
verify_signature = verify
decode_ss58 = ss58_decode
encode_ss58 = ss58_encode


def sign(message: bytes, *, mnemonic: str, crypto_type: int = CRYPTO_SR25519) -> bytes:
    """Sign raw bytes with a key derived from ``mnemonic``."""
    keypair = _backend.Keypair.create_from_mnemonic(mnemonic, crypto_type)
    return bytes(keypair.sign(message))
