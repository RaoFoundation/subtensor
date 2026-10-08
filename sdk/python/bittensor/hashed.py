"""Public descriptors and automatic setup for rotating hashed accounts.

Registration publishes commitments only. It must precede funding: an unknown
32-byte address cannot tell a sender whether its owner intended a hashed wallet.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from ._generated import calls
from .keyfiles import Keyfile, KeyfileError
from .sp_core import CRYPTO_HASHED, Keypair


def raw_bytes(value: Any) -> bytes:
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


def descriptor_bytes(value: dict) -> bytes:
    """Validate the versioned public descriptor decoded from chain storage."""
    if value["version"] != 1 or value["scheme"] not in ("Sr25519", {"Sr25519": None}):
        raise ValueError("unsupported hashed descriptor")
    commitment = raw_bytes(value["initial_commitment"])
    if len(commitment) != 32:
        raise ValueError("hashed key commitments must contain 32 bytes")
    return b"\x01\x01" + commitment


def descriptor_value(descriptor: bytes) -> dict:
    """Convert the native key's SCALE descriptor to runtime call parameters."""
    if len(descriptor) != 34 or descriptor[:2] != b"\x01\x01":
        raise ValueError("unsupported hashed descriptor")
    return {
        "version": 1,
        "scheme": "Sr25519",
        "initial_commitment": "0x" + descriptor[2:].hex(),
    }


def _local_descriptor(wallet: Any, address: str) -> bytes | None:
    """Discover only public companion files; never prompt or open private keys."""
    wallet = getattr(wallet, "_wallet", wallet)
    path = getattr(wallet, "path", None)
    if not isinstance(path, (str, Path)):
        return None
    root = Path(path).expanduser()
    for pattern in ("*/coldkeypub.txt", "*/hotkeys/*pub.txt"):
        for file in sorted(root.glob(pattern)):
            public_file = Keyfile(file)
            try:
                if public_file.is_encrypted():
                    continue
                key = public_file.get_keypair()
            except (OSError, ValueError, KeyfileError):
                continue
            if key.ss58_address == address and key.crypto_type == CRYPTO_HASHED:
                return bytes(key.hashed_descriptor)
    return None


async def with_recipient_registration(substrate, wallet: Any, intent: Any, call: Any):
    """Register a known hashed recipient and its operation in one atomic batch.

    The caller adds origin wrappers after this step, keeping setup under the
    same sponsor authorization. No mutation or private-key access occurs here,
    so plan/dry-run has the same call and fee as submission.
    """
    from .signing import public_view

    parameter = {
        "transfer": "dest_ss58",
        "transfer_all": "dest_ss58",
        "announce_coldkey_swap": "new_coldkey_ss58",
        "swap_coldkey_announced": "new_coldkey_ss58",
        "swap_hotkey": "new_hotkey_ss58",
        "burned_register": "hotkey_ss58",
        "pow_register": "hotkey_ss58",
        "root_register": "hotkey_ss58",
        "register_subnet": "hotkey_ss58",
    }.get(intent.op)
    if parameter is None:
        return call, {}
    address = getattr(intent, parameter, None)
    if address is None and parameter == "hotkey_ss58":
        address = public_view(wallet, "hotkey").ss58_address
    if not isinstance(address, str):
        raise ValueError("recipient account address is missing")
    explicit = getattr(intent, "hashed_descriptor", None)
    descriptor = raw_bytes(explicit) if explicit is not None else _local_descriptor(wallet, address)
    if descriptor is None:
        return call, {}
    public = Keypair.from_hashed_descriptor(descriptor)
    if public.ss58_address != address:
        raise ValueError("hashed descriptor does not match the destination address")
    if await substrate.constant("HashedAccounts", "Enabled") is not True:
        raise ValueError("hashed accounts are not enabled on this chain")
    record = await substrate.query("HashedAccounts", "Accounts", [address])
    if record is not None:
        if descriptor_bytes(record["descriptor"]) != descriptor:
            raise ValueError("registered hashed descriptor does not match the destination")
        return call, {}
    if intent.op == "swap_coldkey_announced":
        raise ValueError(
            "hashed swap destination is not registered; register its public descriptor "
            "with another sponsor before executing the announced swap"
        )
    registration = await substrate.compose(
        calls.HashedAccounts.register(descriptor=descriptor_value(descriptor))
    )
    deposit = int(await substrate.constant("HashedAccounts", "RegistrationDeposit"))
    batch = await substrate.compose(calls.Utility.batch_all(calls=[registration, call]))
    return batch, {"hashed_registration": address, "hashed_registration_deposit_rao": deposit}
