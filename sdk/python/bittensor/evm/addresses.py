"""Address math for the Bittensor EVM: the h160 <-> ss58 seam.

Subtensor runs an EVM whose accounts (h160, MetaMask-style) and native
accounts (ss58) are disjoint signing domains on one chain. Funds cross the
seam through two deterministic mappings, both implemented here:

- **Mirror (hashed) mapping** — how the chain credits an EVM address with
  native balance: ``ss58( blake2_256("evm:" ++ h160_bytes) )``. Transfer TAO
  to an h160's *mirror* and it shows up as that EVM account's balance.
  (``pallet_evm::HashedAddressMapping<BlakeTwo256>`` in the runtime.)
- **Truncated mapping** — how a native account acts *as* an EVM address for
  ``EVM.withdraw`` / ``EVM.call``: the h160 is the first 20 bytes of the
  ss58's 32-byte public key. (``EnsureAddressTruncated`` in the runtime.)

Neither mapping is invertible to a private key: a Bittensor wallet cannot
sign EVM transactions and an EVM wallet cannot sign extrinsics.
"""

from __future__ import annotations

from hashlib import blake2b

from .._transport.codec import ss58_decode, ss58_encode
from ..settings import SS58_FORMAT

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
    text = value.strip()
    if not text.startswith("0x"):
        text = "0x" + text
    if not is_h160(text):
        raise ValueError(f"not a valid EVM (h160) address: {value!r}")
    return text.lower()


def h160_to_ss58(evm_address: str, ss58_format: int = SS58_FORMAT) -> str:
    """The ss58 *mirror* of an EVM address — where its native balance lives.

    TAO transferred to this address (from btcli, an exchange, or any substrate
    wallet) appears as the EVM account's balance on the EVM side. Computed as
    ``ss58(blake2_256(b"evm:" ++ address_bytes))``.
    """
    address_bytes = bytes.fromhex(normalize_h160(evm_address)[2:])
    hashed = blake2b(_MIRROR_PREFIX + address_bytes, digest_size=32).digest()
    return ss58_encode(hashed, ss58_format=ss58_format)


def ss58_to_pubkey(ss58_address: str) -> str:
    """The 32-byte public key behind an ss58 address, as 0x-hex.

    Precompile interfaces take hotkeys/coldkeys as ``bytes32`` public keys,
    not ss58 strings; this is the conversion every such call needs.
    """
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
