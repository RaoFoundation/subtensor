"""Chain error descriptions declared (first) by the `Ethereum` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "InvalidSignature": (
        "Signature verification failed: the sender of an Ethereum or EVM transaction could not "
        "be recovered from its signature, or a limit order's Sr25519 signature does not match "
        "the order payload and signer. Check the signing key, chain id, and the exact payload "
        "bytes that were signed."
    ),
    "PreLogExists": (
        "An `ethereum.transact` extrinsic was submitted in a block that already carries a "
        "pre-log digest, meaning an Ethereum transaction is being injected by other means. "
        "Check the block's digest for a pre-runtime Ethereum log; transact is not allowed "
        "alongside it."
    ),
}
