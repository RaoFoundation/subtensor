"""Receiving-address fixtures encoded independently of the native codec."""

import base64
from hashlib import blake2b


def descriptor_receiving_address(key) -> str:
    payload = bytes(key.hashed_descriptor)
    checksum = blake2b(b"bittensor/hashed/v1/receiving" + payload, digest_size=32).digest()[:8]
    return "bth1_" + base64.urlsafe_b64encode(payload + checksum).decode().rstrip("=")
