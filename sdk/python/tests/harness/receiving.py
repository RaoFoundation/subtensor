"""Legacy receiving-address fixtures, independent of the current encoder."""

import base64
from hashlib import blake2b


def legacy_receiving_address(key, genesis: bytes = bytes([17]) * 32) -> str:
    payload = genesis + bytes(key.hashed_descriptor)
    checksum = blake2b(b"bittensor/hashed/v1/receiving" + payload, digest_size=32).digest()[:8]
    return "bth1_" + base64.urlsafe_b64encode(payload + checksum).decode().rstrip("=")
