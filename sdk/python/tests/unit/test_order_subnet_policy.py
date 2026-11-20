"""Order authorizations and relays expose their full subnet scope to policy."""

import pytest

from bittensor.executor import Executor
from bittensor.intents.batch import Batch
from bittensor.intents.limit_orders import (
    AuthorizeOrder,
    CancelOrder,
    ExecuteBatchedOrders,
    ExecuteOrders,
)
from bittensor.intents.plan import Policy
from bittensor.result import PolicyError
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import dev_wallet
from tests.unit.test_mldsa_orders import order


def terms(netuid, version="V1"):
    value = order(dev_wallet().coldkeypub.ss58_address)["V1"]
    value["netuid"] = netuid
    if version == "V2":
        value["amount"] = {"Fixed": value["amount"]}
        value["has_linked_order"] = False
    return {version: value}


def signed(netuid, version="V1", authorized=True):
    value = terms(netuid, version)
    return {
        "order": {"Authorized": {"nonce": 1, "terms": value}} if authorized else value,
        "signature": "AccountApproved" if authorized else {"Sr25519": "00" * 64},
        "partial_fill": None,
    }


@pytest.mark.parametrize("version", ["V1", "V2"])
@pytest.mark.parametrize("factory", [AuthorizeOrder, CancelOrder])
@pytest.mark.parametrize("batched", [False, True])
async def test_owner_order_allowlist_checks_nested_terms(version, factory, batched):
    intent = factory(1, terms(2, version))
    if batched:
        intent = Batch([intent])
    await assert_rejected(intent)
    assert not Policy(allowed_netuids=[2]).check(intent, None)


@pytest.mark.parametrize("version", ["V1", "V2"])
@pytest.mark.parametrize("authorized", [False, True])
@pytest.mark.parametrize("factory", [ExecuteOrders, ExecuteBatchedOrders])
async def test_relay_allowlist_checks_every_order(version, authorized, factory):
    orders = [signed(2, version, authorized)]
    if factory is ExecuteOrders:
        orders.insert(0, signed(1, version, authorized))
        intent = factory(orders)
    else:
        intent = factory(orders, netuid=2)
    await assert_rejected(intent)
    assert not Policy(allowed_netuids=[1, 2]).check(intent, None)


async def assert_rejected(intent, *, unrestricted=False):
    substrate = FakeSubstrate()
    executor = Executor(substrate, policy=Policy(allowed_netuids=[1]))
    message = "acts on all subnets" if unrestricted else "netuid 2 not in allowed_netuids"
    plan = await executor.plan(intent, dev_wallet())
    assert any(message in violation for violation in plan.violations)
    with pytest.raises(PolicyError, match=message):
        await executor.execute(intent, dev_wallet(), shielded=False)
    assert not substrate.submissions
