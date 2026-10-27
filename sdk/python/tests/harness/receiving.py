"""Receiving-address fixtures encoded independently of the native codec."""

import base64
from hashlib import blake2b


def descriptor_receiving_address(key) -> str:
    payload = bytes(key.hashed_descriptor)
    version = 2 if payload[0] == 2 else 1
    domain = b"bittensor/ms/v2/receiving" if version == 2 else b"bittensor/hashed/v1/receiving"
    checksum = blake2b(domain + payload, digest_size=32).digest()[:8]
    return "tao_" + base64.urlsafe_b64encode(payload + checksum).decode().rstrip("=")
