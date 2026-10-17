"""Versioned order authorization with complete, pinned economic payloads.

Original execution and cancellation calls accept classical signatures, account
approvals, and registered ML-DSA order capabilities. Authorized orders carry an
explicit nonce; the transaction key is never a detached order key.
"""

from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass
from hashlib import blake2b
from typing import Any

from ._generated.calls import Call
from .receiving import parse_recipient
from .signing import resolve_signer
from .sp_core import MLDSA_CRYPTO_TYPES, ROLE_ORDER, ss58_decode

ORDER_ID_DOMAIN = b"bittensor/ms/v2/order-id"


def _bytes(value) -> bytes:
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


def _u64(value: int, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value < 2**64:
        raise ValueError(f"{label} must be a u64 integer")
    return value


def order_terms(order: dict) -> dict:
    if not isinstance(order, dict) or len(order) != 1:
        raise ValueError("order must contain one complete VersionedOrder variant")
    if "Authorized" in order:
        authorized = order["Authorized"]
        if not isinstance(authorized, dict) or set(authorized) != {"nonce", "terms"}:
            raise ValueError("authorized order must contain nonce and complete terms")
        _u64(authorized["nonce"], "order nonce")
        terms = authorized["terms"]
        if not isinstance(terms, dict) or set(terms) not in ({"V1"}, {"V2"}):
            raise ValueError("authorized order terms must be a nonrecursive V1 or V2 variant")
        return order_terms(terms)
    if next(iter(order)) not in ("V1", "V2"):
        raise ValueError("order terms must contain one complete V1 or V2 variant")
    terms = next(iter(order.values()))
    if not isinstance(terms, dict):
        raise ValueError("VersionedOrder terms must be an object")
    return terms


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
    if "Authorized" in order:
        raise ValueError("provide V1 or V2 economic terms, not an already authorized order")
    return {"Authorized": {"nonce": _u64(nonce, "order nonce"), "terms": canonical_order(order)}}


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
    if len(data) <= 11 or data[2] != 2 or data[3:11] != nonce.to_bytes(8, "little"):
        raise ValueError("runtime returned an incompatible authorized order layout")
    return data[11:]


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
        payload = (
            ORDER_ID_DOMAIN
            + self.genesis_hash
            + bytes(ss58_decode(self.owner))
            + self.nonce.to_bytes(8, "little")
            + self.encoded
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
        if isinstance(authorization, dict) and "MlDsa" in authorization:
            for name in ("public_key", "signature"):
                authorization["MlDsa"][name] = "0x" + _bytes(authorization["MlDsa"][name]).hex()
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


@dataclass(frozen=True)
class OrderCapability:
    owner: str
    genesis_hash: bytes
    epoch: int
    commitment: bytes
    expires_at: int
    finalized_block: int
    scope: Any
    spent_tao: int
    spent_alpha: int


async def order_capability(substrate, owner: str) -> OrderCapability:
    owner = parse_recipient(owner).account
    block = await substrate.finalized_block_number()
    block_hash = await substrate.block_hash(block)
    if not block_hash:
        raise ValueError("could not resolve finalized order capability")
    if (
        await substrate.query("AdminUtils", "HashedAccountsEnabled", block_hash=block_hash)
        is not True
    ):
        raise ValueError("ML-DSA authorization is disabled")
    record = await substrate.query(
        "HashedAccounts", "ActiveCapabilities", [owner, "Order"], block_hash=block_hash
    )
    epoch = int(
        await substrate.query(
            "HashedAccounts", "CapabilityEpochs", [owner, "Order"], block_hash=block_hash
        )
        or 0
    )
    if not record or not 0 < epoch < 2**64 or int(record["epoch"]) != epoch:
        raise ValueError("no active finalized ML-DSA order capability")
    expires = int(record["expires_at"])
    commitment = _bytes(record["key_commitment"])
    if not block <= expires < 2**64 or len(commitment) != 64 or not any(commitment):
        raise ValueError("invalid or expired ML-DSA order capability")
    scope = record["scope"]
    if scope not in ("UnrestrictedTrading", {"UnrestrictedTrading": None}) and not (
        isinstance(scope, dict) and set(scope) == {"Trading"}
    ):
        raise ValueError("capability does not authorize trading")
    return OrderCapability(
        owner,
        _bytes(await substrate.block_hash(0)),
        epoch,
        commitment,
        expires,
        block,
        deepcopy(scope),
        int(record["spent_tao"]),
        int(record["spent_alpha"]),
    )


def _check_order_scope(capability: OrderCapability, prepared: PreparedOrder, partial_fill) -> None:
    if not isinstance(capability.scope, dict) or "Trading" not in capability.scope:
        return
    scope, terms = capability.scope["Trading"], order_terms(prepared.order)
    if (
        int(terms["netuid"]) != int(scope["netuid"])
        or terms["hotkey"] != scope["hotkey"]
        or terms["fee_recipient"] != scope["fee_recipient"]
        or int(terms["fee_rate"]) > int(scope["max_fee_perbill"])
    ):
        raise ValueError("order terms exceed the registered trading scope")
    buy = terms["order_type"] in ("LimitBuy", {"LimitBuy": None})
    amount = terms["amount"]
    if isinstance(amount, dict):
        amount = amount.get("Fixed")
    # Linked output amounts are resolved and budgeted by the runtime at fill
    # time. The signed payload still includes the complete provider/fraction.
    if amount is None:
        return
    asset = "tao" if buy else "alpha"
    fill = int(amount) if partial_fill is None else _u64(partial_fill, "partial fill")
    if int(amount) > int(scope[f"max_{asset}_per_order"]) or fill + getattr(
        capability, f"spent_{asset}"
    ) > int(scope[f"total_{asset}"]):
        raise ValueError("order amount exceeds the registered trading budget")


async def sign_order(
    substrate, prepared: PreparedOrder, keypair, *, partial_fill=None
) -> SignedOrder:
    await check_prepared_order(substrate, prepared)
    capability = await order_capability(substrate, prepared.owner)
    if (
        keypair.crypto_type not in MLDSA_CRYPTO_TYPES
        or capability.genesis_hash != prepared.genesis_hash
    ):
        raise ValueError("detached orders require an ML-DSA purpose key and the capability chain")
    commitment = bytes(keypair.mldsa_capability_commitment(ROLE_ORDER, capability.epoch))
    if commitment != capability.commitment:
        raise ValueError("wallet order key does not match the finalized capability")
    finalized_hash = await substrate.block_hash(capability.finalized_block)
    existing = await substrate.query(
        "LimitOrders", "OrderNonces", [prepared.owner, prepared.nonce], block_hash=finalized_hash
    )
    if existing is not None and _bytes(existing) != bytes.fromhex(prepared.order_id[2:]):
        raise ValueError("order nonce is already bound to different economic terms")
    _check_order_scope(capability, prepared, partial_fill)
    proof = {
        "epoch": capability.epoch,
        "public_key": bytes(keypair.mldsa_capability_public_key(ROLE_ORDER, capability.epoch)),
        "signature": bytes(
            keypair.sign_mldsa_order(
                capability.epoch,
                prepared.nonce,
                prepared.genesis_hash,
                prepared.encoded,
                owner=bytes(ss58_decode(prepared.owner)),
            )
        ),
    }
    return SignedOrder(prepared, {"MlDsa": proof}, partial_fill)


def validate_trading_scope(scope):
    if scope == "UnrestrictedTrading" or scope == {"UnrestrictedTrading": None}:
        return {"UnrestrictedTrading": None}
    if not isinstance(scope, dict) or set(scope) != {"Trading"}:
        raise ValueError("choose an explicit Trading scope or UnrestrictedTrading")
    result = deepcopy(scope)
    terms = result["Trading"]
    expected = {
        "netuid",
        "hotkey",
        "fee_recipient",
        "max_fee_perbill",
        "max_tao_per_order",
        "max_alpha_per_order",
        "total_tao",
        "total_alpha",
    }
    if not isinstance(terms, dict) or set(terms) != expected:
        raise ValueError("trading scope must contain every bound")
    if not 0 < terms["netuid"] < 2**16 or not 0 <= terms["max_fee_perbill"] <= 1_000_000_000:
        raise ValueError("invalid trading subnet or fee bound")
    for name in ("max_tao_per_order", "max_alpha_per_order", "total_tao", "total_alpha"):
        _u64(terms[name], name)
    if not (
        (terms["max_tao_per_order"] and terms["total_tao"])
        or (terms["max_alpha_per_order"] and terms["total_alpha"])
    ):
        raise ValueError("trading capability must authorize a positive buy or sell budget")
    terms["hotkey"] = parse_recipient(terms["hotkey"]).account
    recipient = parse_recipient(terms["fee_recipient"])
    if recipient.descriptor is None:
        terms["fee_recipient"] = recipient.account
    return result


class LimitOrders:
    """Client order helpers; submissions pass through normal intent policy."""

    def __init__(self, client):
        self._client = client

    async def prepare(self, order: dict, *, nonce: int) -> PreparedOrder:
        return await prepare_order(self._client._substrate, nonce, order)

    async def sign(self, prepared: PreparedOrder, wallet, *, signer="coldkey", partial_fill=None):
        return await sign_order(
            self._client._substrate,
            prepared,
            resolve_signer(wallet, signer),
            partial_fill=partial_fill,
        )

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

        if not 1 <= len(orders) <= 8:
            raise ValueError("authorized execution requires 1..8 orders")
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

    async def capability_grant(self, keypair, *, owner: str, expires_at: int, scope):
        """Prepare an explicit grant for direct or composite-owner authorization.

        A multisig can approve this intent's composed call through its normal
        quorum. The purpose key may come from a separate ML-DSA wallet.
        """
        from .intents.limit_orders import InstallOrderCapability

        owner = parse_recipient(owner).account
        if keypair.crypto_type not in MLDSA_CRYPTO_TYPES:
            raise ValueError("detached order key must come from an ML-DSA wallet")
        substrate = self._client._substrate
        block_hash = await substrate.block_hash(await substrate.finalized_block_number())
        if not block_hash:
            raise ValueError("could not resolve finalized capability epoch")
        epoch = (
            int(
                await substrate.query(
                    "HashedAccounts", "CapabilityEpochs", [owner, "Order"], block_hash=block_hash
                )
                or 0
            )
            + 1
        )
        _u64(epoch, "capability epoch")
        commitment = bytes(keypair.mldsa_capability_commitment(ROLE_ORDER, epoch)).hex()
        return InstallOrderCapability(epoch, commitment, expires_at, validate_trading_scope(scope))

    async def install_capability(
        self, keypair, wallet, *, expires_at: int, scope, owner: str | None = None, **kwargs
    ):
        from .signing import public_view

        owner = owner or public_view(wallet, "coldkey").ss58_address
        grant = await self.capability_grant(
            keypair, owner=owner, expires_at=expires_at, scope=scope
        )
        return await self._client.execute(grant, wallet, **kwargs)

    async def revoke_capability(self, wallet, **kwargs):
        from .intents.limit_orders import RevokeOrderCapability

        return await self._client.execute(RevokeOrderCapability(), wallet, **kwargs)
