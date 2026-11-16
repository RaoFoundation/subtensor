"""Versioned order authorization with complete, pinned economic payloads.

Original execution and cancellation calls accept classical signatures, account
approvals. Authorized orders carry an
explicit nonce; the transaction key is never a detached order key.
"""

from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass
from hashlib import blake2b
from typing import Any

from ._generated.calls import Call
from ._policy import evaluate_policy
from .receiving import parse_recipient
from .sp_core import ss58_decode

ORDER_ID_DOMAIN = b"bittensor/ms/v2/order-id"


def _bytes(value) -> bytes:
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


def _u64(value: int, label: str) -> int:
    try:
        if not isinstance(value, int) or isinstance(value, bool):
            raise ValueError("order integer must use Python int")
        return int(evaluate_policy("financial.registration.deposit", {"value": value}))
    except (TypeError, ValueError) as error:
        raise ValueError(f"{label} must be a u64 integer") from error


def order_terms(order: dict) -> dict:
    try:
        evaluate_policy("financial.orders.terms", {"order": order, "strict": False})
    except (TypeError, ValueError) as error:
        raise ValueError(str(error)) from error
    terms = order["Authorized"]["terms"] if "Authorized" in order else order
    return next(iter(terms.values()))


def canonical_order(order: dict) -> dict:
    """Normalize identities, retaining fee descriptors until a finalized check."""
    result = deepcopy(order)
    terms = order_terms(result)
    for field in ("signer", "hotkey"):
        terms[field] = parse_recipient(terms[field]).account
    recipient = parse_recipient(terms["fee_recipient"])
    if recipient.descriptor is None:
        terms["fee_recipient"] = recipient.account
    if terms.get("relayer") is not None:
        terms["relayer"] = [parse_recipient(item).account for item in terms["relayer"]]
    return result


async def registered_fee_recipient(substrate, address: str) -> str:
    """Resolve a payment identity only after its descriptor is permanently bound.

    Orders may be relayed much later and through either execution route. A
    finalized, immutable registration protects their fees without changing the
    signed payload, inserting setup calls, or requiring activation to stay on.
    """
    from .hashed import descriptor_bytes, descriptor_storage

    recipient = parse_recipient(address)
    if recipient.descriptor is not None:
        block_hash = await substrate.block_hash(await substrate.finalized_block_number())
        if not block_hash:
            raise ValueError("could not verify finalized order fee recipient registration")
        record = await substrate.query(
            "HashedAccounts",
            descriptor_storage(recipient.descriptor),
            [recipient.account],
            block_hash=block_hash,
        )
        if record is None:
            raise ValueError(
                "order fee recipient registration is not finalized; register the "
                "recipient and wait for finalization before preparing or executing orders"
            )
        if descriptor_bytes(record["descriptor"]) != recipient.descriptor:
            raise ValueError("registered order fee recipient descriptor does not match")
    return recipient.account


async def order_for_chain(substrate, order: dict) -> dict:
    result = canonical_order(order)
    terms = order_terms(result)
    terms["fee_recipient"] = await registered_fee_recipient(substrate, terms["fee_recipient"])
    return result


def authorized_order(nonce: int, order: dict) -> dict:
    """Wrap complete economic terms for the original account-aware calls."""
    _u64(nonce, "order nonce")
    canonical = canonical_order(order)
    try:
        result = evaluate_policy(
            "financial.orders.authorized", {"nonce": nonce, "order": canonical, "strict": False}
        )
    except (TypeError, ValueError) as error:
        raise ValueError(str(error)) from error
    return {"Authorized": {"nonce": int(result["Authorized"]["nonce"]), "terms": canonical}}


async def encoded_order(substrate, nonce: int, order: dict) -> bytes:
    """Ask the runtime codec for VersionedOrder's exact SCALE encoding.

    Call indexes occupy two bytes, followed by Authorized's discriminant and
    an uncompressed u64 nonce. This avoids a second economic codec and retains
    the cryptographic transcript over the original complete economic terms.
    """
    _u64(nonce, "order nonce")
    order = await order_for_chain(substrate, order)
    call = await substrate.compose(
        Call("LimitOrders", "authorize_order", {"order": authorized_order(nonce, order)})
    )
    data = _bytes(getattr(call, "data", call))
    return _bytes(evaluate_policy("financial.orders.layout", {"encoded": data, "nonce": nonce}))


@dataclass(frozen=True)
class PreparedOrder:
    nonce: int
    order: dict
    encoded: bytes
    genesis_hash: bytes

    @property
    def owner(self) -> str:
        return order_terms(self.order)["signer"]

    @property
    def order_id(self) -> str:
        payload = _bytes(
            evaluate_policy(
                "financial.orders.transcript",
                {
                    "genesis": self.genesis_hash,
                    "owner": bytes(ss58_decode(self.owner)),
                    "nonce": self.nonce,
                    "encoded": self.encoded,
                },
            )
        )
        return "0x" + blake2b(payload, digest_size=32).hexdigest()

    def account_approved(self, *, partial_fill: int | None = None) -> SignedOrder:
        return SignedOrder(self, "AccountApproved", partial_fill)


@dataclass(frozen=True)
class SignedOrder:
    prepared: PreparedOrder
    authorization: Any
    partial_fill: int | None = None

    def value(self) -> dict:
        if self.partial_fill is not None:
            _u64(self.partial_fill, "partial fill")
        authorization = deepcopy(self.authorization)
        return {
            "order": authorized_order(self.prepared.nonce, self.prepared.order),
            "signature": authorization,
            "partial_fill": self.partial_fill,
        }


async def prepare_order(substrate, nonce: int, order: dict) -> PreparedOrder:
    order = await order_for_chain(substrate, order)
    genesis = _bytes(await substrate.block_hash(0))
    if len(genesis) != 32:
        raise ValueError("chain genesis hash must contain 32 bytes")
    return PreparedOrder(
        _u64(nonce, "order nonce"), order, await encoded_order(substrate, nonce, order), genesis
    )


async def check_prepared_order(substrate, prepared: PreparedOrder) -> None:
    if (
        _bytes(await substrate.block_hash(0)) != prepared.genesis_hash
        or await encoded_order(substrate, prepared.nonce, prepared.order) != prepared.encoded
    ):
        raise ValueError(
            "prepared order bytes or chain changed; prepare and authorize the complete order again"
        )


class LimitOrders:
    """Client order helpers; submissions pass through normal intent policy."""

    def __init__(self, client):
        self._client = client

    async def prepare(self, order: dict, *, nonce: int) -> PreparedOrder:
        """Prepare terms with a nonce above the owner's ApprovedOrders.highest_nonce.

        The chain permits eight live approvals per owner. Cancellation or expiry
        frees a slot for the next approval, but does not reset the nonce watermark.
        """
        return await prepare_order(self._client._substrate, nonce, order)

    async def authorize(self, prepared: PreparedOrder, wallet, **kwargs):
        from .intents.limit_orders import AuthorizeOrder

        await check_prepared_order(self._client._substrate, prepared)
        return await self._client.execute(
            AuthorizeOrder(prepared.nonce, deepcopy(prepared.order), prepared.encoded.hex()),
            wallet,
            **kwargs,
        )

    async def cancel(self, prepared: PreparedOrder, wallet, **kwargs):
        from .intents.limit_orders import CancelOrder

        await check_prepared_order(self._client._substrate, prepared)
        return await self._client.execute(
            CancelOrder(prepared.nonce, deepcopy(prepared.order), prepared.encoded.hex()),
            wallet,
            **kwargs,
        )

    async def execute(self, orders: list[SignedOrder], wallet, *, should_fail=True, **kwargs):
        from .intents.limit_orders import ExecuteOrders

        evaluate_policy("financial.orders.approved_count", {"count": len(orders)})
        for order in orders:
            await check_prepared_order(self._client._substrate, order.prepared)
        return await self._client.execute(
            ExecuteOrders([order.value() for order in orders], should_fail), wallet, **kwargs
        )

    async def execute_batched(self, netuid: int, orders: list[SignedOrder], wallet, **kwargs):
        from .intents.limit_orders import ExecuteBatchedOrders

        for order in orders:
            await check_prepared_order(self._client._substrate, order.prepared)
        return await self._client.execute(
            ExecuteBatchedOrders([order.value() for order in orders], True, netuid),
            wallet,
            **kwargs,
        )
