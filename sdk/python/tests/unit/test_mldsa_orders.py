"""Full-payload pinning for account-approved and classical signed orders."""

import json
from copy import deepcopy
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor.client import Client
from bittensor.intents.limit_orders import AuthorizeOrder, ExecuteBatchedOrders, ExecuteOrders
from bittensor.limit_orders import (
    prepare_order,
)
from bittensor.sp_core import Keypair
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
    return OrderSubstrate(), None


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
    # The account-approval bound must not reduce the existing classical order bound.
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
async def test_authorized_order_relays_preserve_fee_descriptors(route):
    from bittensor.hashed import descriptor_value
    from bittensor.limit_orders import authorized_order
    from bittensor.receiving import receiving_address
    from tests.harness.samples import dev_wallet

    key = Keypair.create_from_seed(bytes([45]) * 32, 7)
    value = order(BOB)
    value["V1"]["fee_recipient"] = receiving_address(key)
    signature = "AccountApproved"
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
