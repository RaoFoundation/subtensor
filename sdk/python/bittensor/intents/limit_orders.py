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
    """Relay classical orders and up to eight account-approved orders.

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
            raise ValueError("unknown order authorization")
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
