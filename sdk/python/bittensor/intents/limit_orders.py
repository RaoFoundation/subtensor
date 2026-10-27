"""Owner authorization, cancellation, and relay of complete limit orders."""

from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass, field

from .._generated.calls import Call
from ..limit_orders import (
    _bytes,
    _u64,
    authorized_order,
    canonical_order,
    encoded_order,
    order_for_chain,
    order_terms,
    registered_fee_recipient,
    validate_trading_scope,
)
from ._money import UNBOUNDED
from .base import Intent
from .registry import register


@dataclass
class _OwnerOrder(Intent):
    nonce: int = field(metadata={"help": "Permanent owner-local u64 order nonce."})
    order: dict = field(
        metadata={"help": "Complete V1 or V2 VersionedOrder, including every economic term."}
    )
    order_data: str = field(
        default="",
        metadata={"help": "Optional pinned canonical VersionedOrder SCALE bytes as hex."},
    )

    signer = "coldkey"

    def touches_netuids(self) -> list[int]:
        return [order_terms(self.order)["netuid"]]

    def __post_init__(self):
        _u64(self.nonce, "order nonce")
        self.order = canonical_order(self.order)
        if self.order_data:
            _bytes(self.order_data)

    async def build(self, substrate, wallet):
        if order_terms(self.order)["signer"] != self.coldkey_address(wallet):
            raise ValueError(
                "the order owner must authorize this call, directly or through "
                "its proxy/multisig origin"
            )
        if self.order_data and await encoded_order(substrate, self.nonce, self.order) != _bytes(
            self.order_data
        ):
            raise ValueError(
                "pinned order bytes changed; prepare and authorize the complete order again"
            )
        order = await order_for_chain(substrate, self.order)
        return await substrate.compose(
            Call("LimitOrders", self.op, {"order": authorized_order(self.nonce, order)})
        )


@register
@dataclass
class AuthorizeOrder(_OwnerOrder):
    """Authorize every economic term of a order through the account's origin.

    Supports direct post-quantum owners, proxies, derivatives, and multisigs. This
    grants relayers the complete specified trade; funds move only on execution.
    Cancel with the same nonce and complete order to revoke its unfilled part.
    """

    op = "authorize_order"
    wraps = (("LimitOrders", "authorize_order"),)

    def summary(self):
        return f"authorize complete order nonce {self.nonce}"

    def spend(self):
        return UNBOUNDED


@register
@dataclass
class CancelOrder(_OwnerOrder):
    """Cancel a order and any unfilled remainder using its complete terms."""

    op = "cancel_order"
    wraps = (("LimitOrders", "cancel_order"),)

    def summary(self):
        return f"cancel order nonce {self.nonce} and its unfilled remainder"


@register
@dataclass
class ExecuteOrders(Intent):
    """Relay classical orders and up to eight account-approved or post-quantum orders.

    Each signed order includes complete terms, its signature, and an optional
    partial fill. Authorized terms also bind an owner-local nonce. With
    should_fail true, any failed order reverts the whole batch.
    """

    op = "execute_orders"
    signer = "coldkey"
    wraps = (("LimitOrders", "execute_orders"),)

    orders: list = field(metadata={"help": "Complete SignedOrder objects."})
    should_fail: bool = field(
        default=True, metadata={"help": "Revert the batch if any order fails."}
    )

    def __post_init__(self):
        if not self.orders:
            raise ValueError("execution requires at least one order")
        self.orders = deepcopy(self.orders)
        authorized_count = 0
        for signed in self.orders:
            if set(signed) != {"order", "signature", "partial_fill"}:
                raise ValueError(
                    "each signed order must include order, signature, and partial_fill"
                )
            signed["order"] = canonical_order(signed["order"])
            if signed["partial_fill"] is not None:
                _u64(signed["partial_fill"], "partial fill")
            authorized = "Authorized" in signed["order"]
            authorized_count += authorized
            authorization = signed["signature"]
            if authorization == "AccountApproved" or authorization == {"AccountApproved": None}:
                if not authorized:
                    raise ValueError("account approval requires an Authorized order")
                continue
            if not isinstance(authorization, dict) or len(authorization) != 1:
                raise ValueError("unknown order authorization")
            scheme = next(iter(authorization))
            if scheme in ("Ed25519", "Sr25519", "Ecdsa"):
                if authorized:
                    raise ValueError("classical signatures require original V1 or V2 orders")
                signature = _bytes(authorization[scheme])
                if len(signature) != (65 if scheme == "Ecdsa" else 64):
                    raise ValueError("invalid classical order signature length")
                authorization[scheme] = "0x" + signature.hex()
                continue
            if scheme not in ("MlDsa", "Falcon") or not authorized:
                raise ValueError("post-quantum authorization requires an Authorized order")
            proof = authorization[scheme]
            expected_fields = {"epoch", "public_key", "signature"}
            if scheme == "Falcon":
                expected_fields.add("algorithm")
            if (
                not isinstance(proof, dict)
                or set(proof) != expected_fields
                or not _u64(proof["epoch"], "capability epoch")
            ):
                raise ValueError("invalid post-quantum order proof")
            if scheme == "Falcon":
                if proof["algorithm"] not in (6, 8):
                    raise ValueError("unsupported Falcon capability algorithm")
                public_length, signature_length = {6: (897, 666), 8: (1793, 1280)}[
                    proof["algorithm"]
                ]
            else:
                public_length, signature_length = 1952, 3309
            if (
                len(_bytes(proof["public_key"])) != public_length
                or len(_bytes(proof["signature"])) != signature_length
            ):
                raise ValueError("invalid post-quantum order proof lengths")
            for name in ("public_key", "signature"):
                proof[name] = "0x" + _bytes(proof[name]).hex()
        if authorized_count > 8:
            raise ValueError("authorized execution requires 1..8 orders")

    async def build(self, substrate, wallet):
        orders = deepcopy(self.orders)
        for signed in orders:
            signed["order"] = await order_for_chain(substrate, signed["order"])
        return await substrate.compose(
            Call(
                "LimitOrders",
                "execute_orders",
                {"orders": orders, "should_fail": self.should_fail},
            )
        )

    def summary(self):
        return f"relay {len(self.orders)} authorized orders"

    def touches_netuids(self) -> list[int]:
        return sorted({order_terms(signed["order"])["netuid"] for signed in self.orders})


@register
@dataclass
class ExecuteBatchedOrders(ExecuteOrders):
    """Relay a bounded order batch through the atomic subnet netting engine."""

    op = "execute_batched_orders"
    wraps = (("LimitOrders", "execute_batched_orders"),)
    netuid: int = field(
        default=1, metadata={"help": "Subnet shared by all orders in the atomic netted batch."}
    )

    def __post_init__(self):
        super().__post_init__()
        if not 0 < self.netuid < 2**16 or any(
            order_terms(signed["order"])["netuid"] != self.netuid for signed in self.orders
        ):
            raise ValueError("all netted orders must target the same non-root subnet")
        if self.should_fail is not True:
            raise ValueError("netted batches are atomic")

    async def build(self, substrate, wallet):
        orders = deepcopy(self.orders)
        for signed in orders:
            signed["order"] = await order_for_chain(substrate, signed["order"])
        return await substrate.compose(
            Call(
                "LimitOrders",
                "execute_batched_orders",
                {"netuid": self.netuid, "orders": orders},
            )
        )

    def summary(self):
        return f"relay {len(self.orders)} authorized orders through subnet {self.netuid} netting"


@register
@dataclass
class InstallOrderCapability(Intent):
    """Grant a revocable post-quantum order key with an explicit trading scope.

    A permanent epoch tombstone prevents revoked signatures from becoming
    valid after reinstalling. Expiry is a block number, not an order timestamp.
    UnrestrictedTrading explicitly permits every trade until expiry/revocation.
    """

    op = "install_order_capability"
    signer = "coldkey"
    wraps = (("HashedAccounts", "install_capability"),)

    expected_epoch: int = field(metadata={"help": "Finalized owner/Order epoch plus one."})
    key_commitment: str = field(
        metadata={"help": "64-byte commitment to the post-quantum order public key, in hex."}
    )
    expires_at: int = field(metadata={"help": "Inclusive chain block-number expiry."})
    scope: dict = field(metadata={"help": "Explicit Trading bounds, or UnrestrictedTrading."})

    def __post_init__(self):
        if not _u64(self.expected_epoch, "capability epoch") or not _u64(
            self.expires_at, "capability expiry"
        ):
            raise ValueError("capability epoch and expiry must be positive")
        commitment = _bytes(self.key_commitment)
        if len(commitment) != 64 or not any(commitment):
            raise ValueError("capability commitment must contain 64 bytes and be nonzero")
        self.scope = validate_trading_scope(self.scope)

    async def build(self, substrate, wallet):
        scope = deepcopy(self.scope)
        if "Trading" in scope:
            terms = scope["Trading"]
            terms["fee_recipient"] = await registered_fee_recipient(
                substrate, terms["fee_recipient"]
            )
        return await substrate.compose(
            Call(
                "HashedAccounts",
                "install_capability",
                {
                    "expected_epoch": self.expected_epoch,
                    "key_commitment": _bytes(self.key_commitment),
                    "expires_at": self.expires_at,
                    "scope": scope,
                },
            )
        )

    def summary(self):
        return (
            f"grant post-quantum order capability epoch {self.expected_epoch} "
            f"until block {self.expires_at}"
        )

    def spend(self):
        return UNBOUNDED

    def touches_netuids(self) -> list[int]:
        terms = self.scope.get("Trading")
        return [terms["netuid"]] if terms is not None else []

    def affects_all_subnets(self) -> bool:
        return "UnrestrictedTrading" in self.scope


@register
@dataclass
class RevokeOrderCapability(Intent):
    """Revoke the active order key and permanently advance its epoch tombstone."""

    op = "revoke_order_capability"
    signer = "coldkey"
    wraps = (("HashedAccounts", "revoke_capability"),)

    async def build(self, substrate, wallet):
        return await substrate.compose(
            Call("HashedAccounts", "revoke_capability", {"role": "Order"})
        )

    def summary(self):
        return "revoke the active post-quantum order capability"
