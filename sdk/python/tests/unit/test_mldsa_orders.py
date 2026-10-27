"""Full-payload pinning and finalized capability boundaries for authorized orders."""

import json
from copy import deepcopy
from hashlib import blake2b
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor.client import Client
from bittensor.intents.limit_orders import AuthorizeOrder, ExecuteBatchedOrders, ExecuteOrders
from bittensor.limit_orders import (
    ORDER_ID_DOMAIN,
    order_capability,
    prepare_order,
    sign_order,
)
from bittensor.sp_core import FALCON_CRYPTO_TYPES, ROLE_ORDER, Keypair, pq_lengths, ss58_decode
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE_HOT, BOB


@pytest.fixture(params=[7, 8, 9, 10, 11, 12])
def wallet(request):
    return Keypair.create_from_seed(bytes([7]) * 32, request.param)


def order(owner):
    return {
        "V1": {
            "signer": owner,
            "hotkey": ALICE_HOT,
            "netuid": 1,
            "order_type": "LimitBuy",
            "amount": 100,
            "limit_price": 1_000_000_000,
            "expiry": 2_000_000_000_000,
            "fee_rate": 0,
            "fee_recipient": BOB,
            "relayer": [BOB] * 10,
            "max_slippage": 1000,
            "chain_id": 42,
            "partial_fills_enabled": True,
        }
    }


class OrderSubstrate(FakeSubstrate):
    # Stable injected encoding isolates Python pinning; runtime/native Rust
    # tests independently establish canonical SCALE and signing transcripts.
    async def compose(self, call):
        if (call.module, call.function) == ("LimitOrders", "authorize_order"):
            encoded = json.dumps(
                call.params["order"]["Authorized"]["terms"], sort_keys=True, separators=(",", ":")
            ).encode()
            return SimpleNamespace(
                data=bytes([32, 5, 2])
                + call.params["order"]["Authorized"]["nonce"].to_bytes(8, "little")
                + encoded
            )
        return await super().compose(call)


def chain(key):
    substrate = OrderSubstrate()
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    substrate.seed("HashedAccounts", "CapabilityEpochs", [key.ss58_address, "Order"], 3)
    record = {
        "epoch": 3,
        "key_commitment": bytes(key.mldsa_capability_commitment(ROLE_ORDER, 3)),
        "expires_at": 1_000_000,
        "scope": "UnrestrictedTrading",
        "spent_tao": 0,
        "spent_alpha": 0,
    }
    substrate.seed("HashedAccounts", "ActiveCapabilities", [key.ss58_address, "Order"], record)
    return substrate, record


class ObservedKey:
    def __init__(self, key):
        self.key = key

    def __getattr__(self, name):
        return getattr(self.key, name)

    def sign_mldsa_order(self, epoch, nonce, genesis, payload, owner=None):
        self.signed = (epoch, nonce, genesis, bytes(payload))
        self.signed_owner = owner
        return self.key.sign_mldsa_order(epoch, nonce, genesis, payload, owner=owner)


async def test_detached_signing_keeps_every_economic_byte_and_uses_finalized_epoch(wallet):
    key = wallet
    substrate, _ = chain(key)
    value = order(key.ss58_address)
    prepared = await prepare_order(substrate, 17, value)
    assert len(prepared.encoded) > 256
    observed = ObservedKey(key)
    signed = await sign_order(substrate, prepared, observed, partial_fill=30)
    assert observed.signed == (3, 17, prepared.genesis_hash, prepared.encoded)
    proof = signed.value()["signature"][
        "Falcon" if key.crypto_type in FALCON_CRYPTO_TYPES else "MlDsa"
    ]
    assert (
        len(bytes.fromhex(proof["public_key"][2:])) == pq_lengths(key.crypto_type)[0]
        and len(bytes.fromhex(proof["signature"][2:])) == pq_lengths(key.crypto_type)[1]
    )
    assert bytes.fromhex(proof["public_key"][2:]) != key.mldsa_public_key
    assert (
        signed.value()["order"] == {"Authorized": {"nonce": 17, "terms": value}}
        and signed.value()["partial_fill"] == 30
    )
    assert (
        json.loads(json.dumps(ExecuteOrders([signed.value()]).to_dict()))["orders"][0]
        == signed.value()
    )
    expected = (
        ORDER_ID_DOMAIN
        + prepared.genesis_hash
        + bytes(ss58_decode(key.ss58_address))
        + (17).to_bytes(8, "little")
        + prepared.encoded
    )
    assert prepared.order_id == "0x" + blake2b(expected, digest_size=32).hexdigest()
    value["V1"]["amount"] = 999
    assert prepared.order["V1"]["amount"] == 100


@pytest.mark.parametrize(
    "failure", ["revoked", "epoch", "commitment", "expiry", "disabled", "nonce", "payload"]
)
async def test_stale_or_modified_authority_refuses_before_order_signature(failure, wallet):
    key = wallet
    substrate, record = chain(key)
    prepared = await prepare_order(substrate, 17, order(key.ss58_address))
    if failure == "revoked":
        substrate.seed("HashedAccounts", "ActiveCapabilities", [key.ss58_address, "Order"], None)
    elif failure == "epoch":
        substrate.seed("HashedAccounts", "CapabilityEpochs", [key.ss58_address, "Order"], 4)
    elif failure == "commitment":
        record["key_commitment"] = bytes([1]) * 64
    elif failure == "expiry":
        record["expires_at"] = 0
    elif failure == "disabled":
        substrate.seed("AdminUtils", "HashedAccountsEnabled", [], False)
    elif failure == "nonce":
        substrate.seed("LimitOrders", "OrderNonces", [key.ss58_address, 17], "0x" + "ff" * 32)
    else:
        prepared.order["V1"]["limit_price"] += 1
    observed = ObservedKey(key)
    with pytest.raises(ValueError):
        await sign_order(substrate, prepared, observed)
    assert not hasattr(observed, "signed")


async def test_scope_checks_full_amount_not_only_partial_fill_and_remaining_budget(wallet):
    key = wallet
    substrate, record = chain(key)
    record["scope"] = {
        "Trading": {
            "netuid": 1,
            "hotkey": ALICE_HOT,
            "fee_recipient": BOB,
            "max_fee_perbill": 10,
            "max_tao_per_order": 90,
            "max_alpha_per_order": 0,
            "total_tao": 200,
            "total_alpha": 0,
        }
    }
    prepared = await prepare_order(substrate, 17, order(key.ss58_address))
    with pytest.raises(ValueError, match="budget"):
        await sign_order(substrate, prepared, key, partial_fill=10)
    record["scope"]["Trading"]["max_tao_per_order"] = 100
    await sign_order(substrate, prepared, key, partial_fill=10)
    record["spent_tao"] = 199
    with pytest.raises(ValueError, match="budget"):
        await sign_order(substrate, prepared, key, partial_fill=10)


async def test_owner_approval_cancel_and_netting_reuse_identical_order_and_nonce(wallet):
    key = wallet
    substrate, _ = chain(key)
    client = Client(substrate=substrate)
    client.execute = AsyncMock(return_value="submitted")
    prepared = await client.limit_orders.prepare(order(key.ss58_address), nonce=9)
    await client.limit_orders.authorize(prepared, key)
    approval = client.execute.await_args.args[0]
    assert approval.order_data == prepared.encoded.hex()
    await client.limit_orders.cancel(prepared, key)
    cancel = client.execute.await_args.args[0]
    assert cancel.order_data == approval.order_data and cancel.order == approval.order
    approved = prepared.account_approved(partial_fill=10)
    await client.limit_orders.execute([approved], key)
    execute = client.execute.await_args.args[0]
    assert execute.orders == [approved.value()]
    await client.limit_orders.execute_batched(1, [approved], key)
    netted = client.execute.await_args.args[0]
    assert isinstance(netted, ExecuteBatchedOrders) and netted.orders == execute.orders
    with pytest.raises(ValueError, match="same non-root subnet"):
        await client.limit_orders.execute_batched(2, [approved], key)
    with pytest.raises(ValueError, match=r"1\.\.8"):
        ExecuteOrders([approved.value()] * 9)


async def test_grant_reads_tombstone_and_revoke_keeps_owner_origin(wallet):
    key = wallet
    substrate, _ = chain(key)
    client = Client(substrate=substrate)
    client.execute = AsyncMock(return_value="submitted")
    await client.limit_orders.install_capability(
        key, key, expires_at=1_000_000, scope="UnrestrictedTrading"
    )
    grant = client.execute.await_args.args[0]
    assert grant.expected_epoch == 4
    assert grant.key_commitment == bytes(key.mldsa_capability_commitment(ROLE_ORDER, 4)).hex()
    assert grant.scope == {"UnrestrictedTrading": None}
    await client.limit_orders.revoke_capability(key)
    call = await client.execute.await_args.args[0].build(substrate, key)
    assert call.params == {"role": "Order"}


async def test_account_approved_order_supports_composite_origin_without_private_key(wallet):
    from bittensor.executor import _OriginView

    key = wallet
    substrate, _ = chain(key)
    prepared = await prepare_order(substrate, 17, order(BOB))
    intent = AuthorizeOrder(prepared.nonce, deepcopy(prepared.order), prepared.encoded.hex())
    result = await intent.build(substrate, _OriginView(BOB))
    assert result.data[11:] == prepared.encoded
    with pytest.raises(ValueError, match="order owner"):
        await intent.build(substrate, key)


async def test_capability_snapshot_reads_one_finalized_block(wallet):
    key = wallet
    substrate, _ = chain(key)
    original = substrate.query
    seen = []

    async def observe(*args, **kwargs):
        seen.append(kwargs.get("block_hash"))
        return await original(*args, **kwargs)

    substrate.query = observe
    capability = await order_capability(substrate, key.ss58_address)
    assert capability.epoch == 3
    assert len(set(seen)) == 1 and seen[0] is not None


async def test_delegated_order_key_binds_composite_issuer_and_requires_its_grant(wallet):
    key = wallet
    substrate, record = chain(key)
    substrate.seed("HashedAccounts", "CapabilityEpochs", [BOB, "Order"], 3)
    substrate.seed("HashedAccounts", "ActiveCapabilities", [BOB, "Order"], record)
    prepared = await prepare_order(substrate, 1, order(BOB))
    observed = ObservedKey(key)
    signed = await sign_order(substrate, prepared, observed)
    assert signed.prepared.owner == BOB
    assert observed.signed_owner == bytes(ss58_decode(BOB))
    substrate.seed("HashedAccounts", "ActiveCapabilities", [BOB, "Order"], None)
    with pytest.raises(ValueError, match="no active finalized"):
        await sign_order(substrate, prepared, key)


def test_order_id_matches_independent_runtime_wire_fixture():
    from pathlib import Path

    from bittensor.limit_orders import PreparedOrder
    from bittensor.sp_core import ss58_encode

    fixtures = Path(__file__).resolve().parents[4] / "pallets/limit-orders/tests/fixtures"
    prepared = PreparedOrder(
        42,
        order(ss58_encode(bytes([0x11]) * 32, 42)),
        (fixtures / "authorized-order.scale").read_bytes(),
        bytes([0x44]) * 32,
    )
    assert (
        bytes.fromhex(prepared.order_id[2:]) == (fixtures / "authorized-order-id.bin").read_bytes()
    )


async def test_composite_owner_can_prepare_grant_for_a_separate_purpose_key(wallet):
    key = wallet
    substrate, _ = chain(key)
    substrate.seed("HashedAccounts", "CapabilityEpochs", [BOB, "Order"], 7)
    grant = await Client(substrate=substrate).limit_orders.capability_grant(
        key, owner=BOB, expires_at=1_000_000, scope="UnrestrictedTrading"
    )
    assert grant.expected_epoch == 8
    assert grant.key_commitment == bytes(key.mldsa_capability_commitment(ROLE_ORDER, 8)).hex()


async def test_original_execution_calls_accept_mixed_authorization_schemes(wallet):
    key = wallet
    substrate, _ = chain(key)
    prepared = await prepare_order(substrate, 17, order(key.ss58_address))
    authorized = prepared.account_approved().value()
    classical = {
        "order": order(BOB),
        "signature": {"Sr25519": "0x" + "11" * 64},
        "partial_fill": None,
    }
    # The ML-DSA proof bound must not reduce the existing classical order bound.
    execution = ExecuteOrders([classical] * 9 + [authorized])
    call = await execution.build(substrate, key)
    assert call.function == "execute_orders"
    assert call.params["orders"][-1]["order"]["Authorized"]["nonce"] == 17
    assert call.params["orders"][0] == classical
    netted = await ExecuteBatchedOrders([classical, authorized], True, 1).build(substrate, key)
    assert netted.function == "execute_batched_orders"
    for mismatched in (
        {**classical, "signature": "AccountApproved"},
        {**authorized, "signature": classical["signature"]},
    ):
        with pytest.raises(ValueError):
            ExecuteOrders([mismatched])


async def test_original_owner_calls_embed_the_same_nonce_and_complete_terms(wallet):
    from bittensor.intents.limit_orders import CancelOrder

    key = wallet
    substrate, _ = chain(key)
    prepared = await prepare_order(substrate, 17, order(key.ss58_address))
    cancel = await CancelOrder(17, prepared.order).build(substrate, key)
    assert cancel.function == "cancel_order"
    assert cancel.params == {"order": {"Authorized": {"nonce": 17, "terms": prepared.order}}}
    with pytest.raises(ValueError, match="nonrecursive"):
        ExecuteOrders(
            [
                {
                    **prepared.account_approved().value(),
                    "order": {"Authorized": {"nonce": 1, "terms": cancel.params["order"]}},
                }
            ]
        )


@pytest.mark.parametrize("crypto_type", [4, 6, 7, 8])
@pytest.mark.parametrize("route", ["prepare", "authorize", "execute", "netted"])
@pytest.mark.parametrize("registration", ["missing", "unfinalized", "mismatch", "finalized"])
async def test_fee_descriptors_require_matching_finalized_registration(
    crypto_type, route, registration
):
    from bittensor.hashed import descriptor_storage, descriptor_value
    from bittensor.receiving import receiving_address
    from tests.harness.samples import dev_wallet

    key = Keypair.create_from_seed(bytes([43]) * 32, crypto_type)
    descriptor = bytes(key.hashed_descriptor)
    chain = OrderSubstrate()
    finalized = await chain.block_hash(await chain.finalized_block_number())
    value = order(dev_wallet().coldkey.ss58_address)
    value["V1"]["fee_rate"] = 10_000_000
    value["V1"]["fee_recipient"] = receiving_address(key)
    original = deepcopy(value)
    record = {"descriptor": descriptor_value(descriptor)}
    if registration == "mismatch":
        other = Keypair.create_from_seed(bytes([44]) * 32, crypto_type)
        record = {"descriptor": descriptor_value(bytes(other.hashed_descriptor))}

    async def query(module, storage, params=None, block_hash=None):
        assert (module, storage, params) == (
            "HashedAccounts",
            descriptor_storage(descriptor),
            [key.ss58_address],
        )
        assert block_hash == finalized
        # A record visible only at the best head does not protect a payment.
        if registration == "missing" or (registration == "unfinalized" and block_hash):
            return None
        return record

    chain.query = AsyncMock(side_effect=query)
    chain.compose = AsyncMock(wraps=chain.compose)
    if route in ("execute", "netted"):
        signed = {"order": value, "signature": {"Sr25519": "0x" + "00" * 64}, "partial_fill": None}
        intent = (ExecuteOrders if route == "execute" else ExecuteBatchedOrders)([signed])
        # Serialization and reconstruction must retain the payment descriptor.
        assert intent.to_dict()["orders"][0]["order"]["V1"]["fee_recipient"] == receiving_address(
            key
        )
        action = intent.build(chain, dev_wallet())
    elif route == "authorize":
        intent = AuthorizeOrder(1, value)
        assert intent.to_dict()["order"]["V1"]["fee_recipient"] == receiving_address(key)
        action = intent.build(chain, dev_wallet())
    else:
        action = prepare_order(chain, 1, value)
    if crypto_type == 8:
        assert await action is not None
        chain.query.assert_not_awaited()
        return
    if registration != "finalized":
        with pytest.raises(
            ValueError, match=r"(registration is not finalized|descriptor does not match)"
        ):
            await action
        chain.compose.assert_not_called()
    else:
        # Registration is permanent: no activation-flag lookup is necessary.
        result = await action
        assert result is not None
        for call in chain.compose.call_args_list:
            assert receiving_address(key) not in str(call)
    chain.query.assert_awaited_once()
    assert value == original


@pytest.mark.parametrize("route", [ExecuteOrders, ExecuteBatchedOrders])
@pytest.mark.parametrize("proof", ["AccountApproved", "MlDsa"])
async def test_authorized_order_relays_preserve_fee_descriptors(route, proof):
    from bittensor.hashed import descriptor_value
    from bittensor.limit_orders import authorized_order
    from bittensor.receiving import receiving_address
    from tests.harness.samples import dev_wallet

    key = Keypair.create_from_seed(bytes([45]) * 32, 7)
    value = order(BOB)
    value["V1"]["fee_recipient"] = receiving_address(key)
    signature = "AccountApproved"
    if proof == "MlDsa":
        signature = {"MlDsa": {"epoch": 1, "public_key": "00" * 1952, "signature": "00" * 3309}}
    intent = route(
        [{"order": authorized_order(3, value), "signature": signature, "partial_fill": None}]
    )
    chain = OrderSubstrate()
    chain.query = AsyncMock(return_value=None)
    with pytest.raises(ValueError, match="registration is not finalized"):
        await intent.build(chain, dev_wallet())
    chain.query.return_value = {"descriptor": descriptor_value(bytes(key.hashed_descriptor))}
    built = await intent.build(chain, dev_wallet())
    terms = built.params["orders"][0]["order"]["Authorized"]["terms"]["V1"]
    assert terms["fee_recipient"] == key.ss58_address
    assert intent.orders[0]["order"]["Authorized"]["terms"]["V1"] == value["V1"]


async def test_trading_capability_retains_fee_descriptor_until_finalized_setup():
    from bittensor.hashed import descriptor_value
    from bittensor.intents.limit_orders import InstallOrderCapability
    from bittensor.receiving import receiving_address
    from tests.harness.samples import dev_wallet

    key = Keypair.create_from_seed(bytes([46]) * 32, 7)
    scope = {
        "Trading": {
            "netuid": 1,
            "hotkey": BOB,
            "fee_recipient": receiving_address(key),
            "max_fee_perbill": 1_000_000,
            "max_tao_per_order": 100,
            "max_alpha_per_order": 0,
            "total_tao": 1000,
            "total_alpha": 0,
        }
    }
    intent = InstallOrderCapability(1, "01" * 64, 1000, scope)
    chain = OrderSubstrate()
    chain.query = AsyncMock(return_value=None)
    with pytest.raises(ValueError, match="registration is not finalized"):
        await intent.build(chain, dev_wallet())
    assert intent.to_dict()["scope"] == scope
    chain.query.return_value = {"descriptor": descriptor_value(bytes(key.hashed_descriptor))}
    built = await intent.build(chain, dev_wallet())
    assert built.params["scope"]["Trading"]["fee_recipient"] == key.ss58_address


async def test_order_fee_registration_fails_closed_without_finalized_hash():
    from bittensor.receiving import receiving_address

    chain = OrderSubstrate()
    chain.block_hash = AsyncMock(return_value=None)
    chain.query = AsyncMock()
    value = order(BOB)
    value["V1"]["fee_recipient"] = receiving_address(Keypair.create_from_seed(bytes([47]) * 32, 7))
    with pytest.raises(ValueError, match="could not verify finalized"):
        await prepare_order(chain, 1, value)
    chain.query.assert_not_called()


async def test_master_switch_blocks_order_capabilities_despite_ms_subgate(wallet):
    substrate, _ = chain(wallet)
    substrate.seed("AdminUtils", "HashedAccountsEnabled", [], False)
    with pytest.raises(ValueError, match="authorization is disabled"):
        await order_capability(substrate, wallet.ss58_address)
