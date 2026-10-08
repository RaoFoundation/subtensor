"""Self-contained payment addresses; never discard setup data on a write path.

``bth1_`` addresses carry the full chain genesis hash and the original public
descriptor. Their AccountId and authorization remain the runtime's existing
ones. Only the native codec defines the wire format and checksum.
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
    genesis_hash: bytes | None = None


def is_receiving_address(value: Any) -> bool:
    """Recognize the reserved family, including malformed/unsupported versions.

    This is a routing predicate, not validation. Invalid family members must
    reach the strict decoder rather than fall through to a wallet-name lookup.
    """
    return isinstance(value, str) and value.lstrip().lower().startswith("bth")


def genesis_bytes(value: str | bytes) -> bytes:
    try:
        raw = bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)
    except (TypeError, ValueError) as error:
        raise ValueError("chain genesis hash must contain 32 bytes") from error
    if len(raw) != 32:
        raise ValueError("chain genesis hash must contain 32 bytes")
    return raw


def parse_recipient(value: str) -> Recipient:
    if not isinstance(value, str):
        raise ValueError("recipient must be an address string")
    if is_receiving_address(value):
        genesis, descriptor = sp_core.decode_hashed_receiving_address(value)
        public = sp_core.Keypair.from_hashed_descriptor(bytes(descriptor))
        return Recipient(value, public.ss58_address, bytes(descriptor), bytes(genesis))
    # A normal address has no hidden setup information. This API validates it;
    # existing legacy coercion paths keep their own historical validation rules.
    sp_core.ss58_decode(value)
    return Recipient(value, value)


def receiving_address(keypair: Any, genesis_hash: str | bytes) -> str:
    if keypair.crypto_type != sp_core.CRYPTO_HASHED:
        return keypair.ss58_address
    return sp_core.encode_hashed_receiving_address(
        bytes(keypair.hashed_descriptor), genesis_bytes(genesis_hash)
    )


async def check_network(substrate: Any, recipient: Recipient) -> None:
    if recipient.genesis_hash is not None:
        actual = genesis_bytes(await substrate.block_hash(0))
        if actual != recipient.genesis_hash:
            raise ValueError(
                "recipient address belongs to a different network; payment was not built"
            )


async def account_for_read(substrate: Any, value: Any) -> Any:
    """Resolve an explicitly read-only address after checking its network."""
    if isinstance(value, (list, tuple)):
        return [await account_for_read(substrate, item) for item in value]
    if not is_receiving_address(value):
        return value
    recipient = parse_recipient(value)
    await check_network(substrate, recipient)
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
    if getattr(key, "crypto_type", None) == sp_core.CRYPTO_HASHED:
        return receiving_address(key, await substrate.block_hash(0))
    return as_ss58(value, param)
