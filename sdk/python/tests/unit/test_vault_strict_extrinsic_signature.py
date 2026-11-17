from hashlib import blake2b
from types import SimpleNamespace

import pytest

from bittensor.sp_core import Keypair, verify
from bittensor.vault.signer import VaultError, VaultSigner


# Vault transaction signatures must match the exact payload accepted by the chain.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("crypto_type", [0, 1])
@pytest.mark.parametrize("large", [False, True])
def test_vault_rejects_message_wrapped_transaction_signatures(crypto_type, large):
    key = Keypair.create_from_seed(bytes([crypto_type + 1]) * 32, crypto_type)
    payload = (
        blake2b(b"large signing context" * 100, digest_size=32).digest()
        if large
        else b"raw signing context"
    )
    unsigned = SimpleNamespace(payload=payload)
    signer = VaultSigner(key.ss58_address, crypto_type=crypto_type)
    wrapped = key.sign(b"<Bytes>" + payload + b"</Bytes>")
    assert verify(payload, wrapped, key.ss58_address, crypto_type)
    for signature in [wrapped, bytes([crypto_type]) + wrapped]:
        with pytest.raises(VaultError, match="does not verify"):
            signer._validated_signature(signature.hex(), unsigned)
    raw = key.sign(payload)
    for signature in [raw, bytes([crypto_type]) + raw]:
        assert signer._validated_signature(signature.hex(), unsigned) == signature
