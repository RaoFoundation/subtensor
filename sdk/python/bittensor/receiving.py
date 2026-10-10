"""Self-contained payment addresses; never discard setup data on a write path.

``bth1_`` addresses carry the original public descriptor. The former network
field is reserved and zero in new addresses; old addresses remain accepted.
Their AccountId and authorization remain the runtime's existing ones. Only the
native codec defines the wire format and checksum.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from . import sp_core


@dataclass(frozen=True)
class Recipient:
    address: str
    account: str
    descriptor: bytes | None = None


def is_receiving_address(value: Any) -> bool:
    """Recognize the reserved family, including malformed/unsupported versions.

    This is a routing predicate, not validation. Invalid family members must
    reach the strict decoder rather than fall through to a wallet-name lookup.
    """
    return isinstance(value, str) and value.lstrip().lower().startswith("bth")


def parse_recipient(value: str) -> Recipient:
    if not isinstance(value, str):
        raise ValueError("recipient must be an address string")
    if is_receiving_address(value):
        _, descriptor = sp_core.decode_hashed_receiving_address(value)
        public = sp_core.Keypair.from_hashed_descriptor(bytes(descriptor))
        return Recipient(value, public.ss58_address, bytes(descriptor))
    # A normal address has no hidden setup information. This API validates it;
    # existing legacy coercion paths keep their own historical validation rules.
    sp_core.ss58_decode(value)
    return Recipient(value, value)


def receiving_address(keypair: Any, genesis_hash: str | bytes | None = None) -> str:
    """Derive a receiving address locally; this does not register the account.

    The optional legacy genesis_hash argument is ignored. The same receiving
    address works on every chain, with independent registration on each.

    Register each new hashed account and wait for finalization before publicly
    sharing its address.
    """
    if keypair.crypto_type not in sp_core.HASHED_CRYPTO_TYPES:
        return keypair.ss58_address
    return sp_core.encode_hashed_receiving_address(bytes(keypair.hashed_descriptor))


async def account_for_read(substrate: Any, value: Any) -> Any:
    """Resolve an explicitly read-only address without changing its account identity."""
    if isinstance(value, (list, tuple)):
        return [await account_for_read(substrate, item) for item in value]
    if not is_receiving_address(value):
        return value
    recipient = parse_recipient(value)
    return recipient.account


async def account_for_registered_write(substrate: Any, value: str) -> str:
    """Resolve a recipient for a write that cannot carry a registration guard.

    A finalized registration is permanent, so a later reorg or disabled setup
    cannot make the resolved AccountId lose its descriptor. Never resolve an
    unregistered receiving address to an unprotected bare account on this path.
    """
    from .hashed import descriptor_bytes

    recipient = parse_recipient(value)
    if recipient.descriptor is None:
        return recipient.account
    finalized = await substrate.block_hash(await substrate.finalized_block_number())
    if not finalized:
        raise ValueError("could not verify finalized hashed destination registration")
    record = await substrate.query(
        "HashedAccounts", "Accounts", [recipient.account], block_hash=finalized
    )
    if record is None:
        raise ValueError(
            "hashed destination registration is not finalized; register the recipient "
            "and wait for finalization before using this command"
        )
    if descriptor_bytes(record["descriptor"]) != recipient.descriptor:
        raise ValueError("registered hashed descriptor does not match the destination")
    return recipient.account


async def coerce_payment_address(substrate: Any, value: Any, param: str) -> Any:
    """Preserve complete typed addresses and obtain public wallet descriptors."""
    from .signing import KeyedWallet, Signer, as_ss58, public_view
    from .wallet import Wallet

    if isinstance(value, (list, tuple)):
        return [await coerce_payment_address(substrate, item, param) for item in value]
    if value is None or isinstance(value, str):
        if is_receiving_address(value):
            parse_recipient(value)
        return value
    key = value
    if isinstance(value, (Wallet, KeyedWallet)) and not isinstance(value, Signer):
        key = public_view(value, "hotkey" if "hotkey" in param else "coldkey")
    if getattr(key, "crypto_type", None) in sp_core.HASHED_CRYPTO_TYPES:
        return receiving_address(key)
    return as_ss58(value, param)
