"""Address math for the Bittensor EVM: the h160 <-> ss58 seam.

Subtensor runs an EVM whose accounts (h160, MetaMask-style) and native
accounts (ss58) are disjoint signing domains on one chain. Funds cross the
seam through two deterministic mappings, both implemented here:

- **Legacy mirror mapping** — how the chain credits an ordinary EVM address with
  native balance: ``ss58( blake2_256("evm:" ++ h160_bytes) )``. Transfer TAO
  to an unprotected h160's *mirror* and it shows up as that EVM account's balance.
  (``pallet_evm::HashedAddressMapping<BlakeTwo256>`` in the runtime.)
- **Truncated mapping** — how a native account acts *as* an EVM address for
  ``EVM.withdraw`` / ``EVM.call``: the h160 is the first 20 bytes of the
  ss58's 32-byte public key. (``EnsureAddressTruncated`` in the runtime.)

Registered hashed accounts instead bind their truncated alias to the complete
native account. Use ``resolve_evm_recipient`` for the active on-chain mapping;
the pure ``h160_to_ss58`` function intentionally computes only the legacy mirror.

Neither mapping is invertible to a private key: a Bittensor wallet cannot
sign EVM transactions and an EVM wallet cannot sign extrinsics.
"""

from __future__ import annotations

from hashlib import blake2b
from typing import Any

from .._transport.codec import ss58_decode, ss58_encode
from ..receiving import (
    Recipient,
    check_network,
    genesis_bytes,
    is_receiving_address,
    parse_recipient,
    receiving_address,
)
from ..settings import SS58_FORMAT
from ..sp_core import Keypair

# The runtime's HashedAddressMapping prefixes the address bytes with this
# ASCII tag before hashing (pallet_evm HashedAddressMapping convention).
_MIRROR_PREFIX = b"evm:"


def is_h160(value: str) -> bool:
    """Whether ``value`` is a 0x-prefixed 20-byte hex address."""
    if not value.startswith("0x") or len(value) != 42:
        return False
    try:
        bytes.fromhex(value[2:])
    except ValueError:
        return False
    return True


def normalize_h160(value: str) -> str:
    """Validate an h160 address and return it 0x-prefixed and lowercase."""
    if is_receiving_address(value):
        parse_recipient(value)
        raise ValueError(
            "EVM routes cannot set up a receiving address; use `wallet transfer` with "
            "the complete receiving address"
        )
    text = value.strip()
    if not text.startswith("0x"):
        text = "0x" + text
    if not is_h160(text):
        raise ValueError(f"not a valid EVM (h160) address: {value!r}")
    return text.lower()


def h160_to_ss58(evm_address: str, ss58_format: int = SS58_FORMAT) -> str:
    """The deterministic legacy ss58 mirror, without querying protected aliases.

    Computed as ``ss58(blake2_256(b"evm:" ++ address_bytes))``. For current
    balances and funding, use ``resolve_evm_recipient``: registered hashed
    aliases have their balance in the bound native account instead.
    """
    address_bytes = bytes.fromhex(normalize_h160(evm_address)[2:])
    hashed = blake2b(_MIRROR_PREFIX + address_bytes, digest_size=32).digest()
    return ss58_encode(hashed, ss58_format=ss58_format)


async def resolve_evm_recipient(substrate: Any, evm_address: str) -> Recipient:
    """Resolve an EVM balance account and retain any required registration guard.

    Chains without hashed-account support preserve the legacy mapping. Existing
    bindings remain authoritative even when new registrations are disabled;
    malformed alias bindings always fail closed. Funding callers must retain
    a protected recipient's full receiving address through normal transfer
    composition, including its idempotent registration guard.
    """
    from ..hashed import descriptor_bytes

    address = normalize_h160(evm_address)
    legacy = h160_to_ss58(address)
    if await substrate.constant("HashedAccounts", "Enabled") is None:
        return Recipient(legacy, legacy)
    head = await substrate.block_hash()
    bound = await substrate.query("HashedAccounts", "EvmAliases", [address], block_hash=head)
    if bound is None:
        return Recipient(legacy, legacy)
    try:
        if isinstance(bound, str):
            raw = bytes.fromhex(
                bound[2:] if bound.startswith("0x") else ss58_decode(bound).removeprefix("0x")
            )
        else:
            raw = bytes(bound)
        if len(raw) != 32 or raw[:20] != bytes.fromhex(address[2:]):
            raise ValueError("alias does not match the bound account")
        account = ss58_encode(raw, ss58_format=SS58_FORMAT)
        record = await substrate.query("HashedAccounts", "Accounts", [account], block_hash=head)
        if record is None:
            raise ValueError("alias has no registered account")
        descriptor = descriptor_bytes(record["descriptor"])
        public = Keypair.from_hashed_descriptor(descriptor)
        if bytes(public.public_key) != raw:
            raise ValueError("descriptor does not match the bound account")
    except (KeyError, TypeError, ValueError) as error:
        raise ValueError(f"invalid protected EVM alias binding: {error}") from error
    genesis = genesis_bytes(await substrate.block_hash(0))
    return Recipient(receiving_address(public, genesis), account, descriptor, genesis)


async def resolve_evm_deposit(substrate: Any, native_address: str) -> tuple[str, Recipient]:
    """Read the EVM deposit route for a particular native identity."""
    native = parse_recipient(native_address)
    await check_network(substrate, native)
    alias = ss58_to_h160_truncated(native.account)
    recipient = await resolve_evm_recipient(substrate, alias)
    if recipient.descriptor is not None:
        if recipient.account != native.account:
            raise ValueError("this EVM alias belongs to another registered native account")
    elif native.descriptor is not None:
        raise ValueError(
            "this hashed EVM deposit address is not active; first register and fund the wallet "
            "using `wallet transfer` with its complete receiving address on a supported chain"
        )
    return alias, recipient


async def resolve_evm_funding_recipient(substrate: Any, evm_address: str) -> Recipient:
    """Resolve native funding only when its destination can be guarded atomically."""
    recipient = await resolve_evm_recipient(substrate, evm_address)
    if (
        recipient.descriptor is None
        and await substrate.constant("HashedAccounts", "Enabled") is True
    ):
        raise ValueError(
            "native mirror funding is unavailable for an unprotected EVM address while hashed "
            "aliases are enabled; send from an EVM wallet to the H160 address instead"
        )
    return recipient


def ss58_to_pubkey(ss58_address: str) -> str:
    """The 32-byte public key behind an ss58 address, as 0x-hex.

    Precompile interfaces take hotkeys/coldkeys as ``bytes32`` public keys,
    not ss58 strings; this is the conversion every such call needs.
    """
    if is_receiving_address(ss58_address):
        parse_recipient(ss58_address)
        raise ValueError(
            "raw EVM calls cannot safely set up a receiving address; use `wallet transfer` "
            "with the complete receiving address"
        )
    return "0x" + ss58_decode(ss58_address).removeprefix("0x")


def pubkey_to_ss58(pubkey: "str | bytes", ss58_format: int = SS58_FORMAT) -> str:
    """The ss58 address for a 32-byte public key (0x-hex or raw bytes)."""
    raw = bytes.fromhex(pubkey.removeprefix("0x")) if isinstance(pubkey, str) else bytes(pubkey)
    if len(raw) != 32:
        raise ValueError(f"expected a 32-byte public key, got {len(raw)} bytes")
    return ss58_encode(raw, ss58_format=ss58_format)


def ss58_to_h160_truncated(ss58_address: str) -> str:
    """The *truncated* h160 of a native account: the first 20 bytes of its public key.

    This is the EVM address a native account controls for origin-checked EVM
    pallet calls (``EVM.withdraw``): the chain accepts the extrinsic only when
    the signer's public key starts with these 20 bytes. Funding path: send TAO
    from MetaMask to ``h160_to_ss58(truncated_h160)``, then withdraw it into
    the native account with the ``evm_withdraw`` intent.
    """
    pubkey = bytes.fromhex(ss58_decode(ss58_address).removeprefix("0x"))
    return "0x" + pubkey[:20].hex()
