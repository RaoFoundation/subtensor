"""Registry-driven tests over every typed read.

Every read in ``bittensor.reads.REGISTRY`` must have sample params in
``tests/harness/samples.py`` and dispatch successfully against the in-memory
``FakeSubstrate`` — both at the chain head (``client.read``) and pinned to a
block (``snapshot.read``), which proves each read only uses the view surface
(query/query_map/query_batch/runtime/constant/balance/at/timestamp).

The fake seeds just enough structured state (runtime-API payloads and maps)
for shape-sensitive reads to decode their empty/simple forms.
"""

from __future__ import annotations

import pytest

from bittensor.client import Client
from bittensor.reads import REGISTRY, list_reads
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, READ_SAMPLES

EMPTY_SWAP_SIM = {
    "tao_amount": 0,
    "alpha_amount": 0,
    "tao_fee": 0,
    "alpha_fee": 0,
    "tao_slippage": 0,
    "alpha_slippage": 0,
}


def seeded_substrate() -> FakeSubstrate:
    fake = FakeSubstrate()
    # Runtime APIs the typed reads decode structurally: an empty/None result
    # is a legitimate chain answer ("nothing there") for all of these.
    for api, method, value in [
        ("NeuronInfoRuntimeApi", "get_neurons_lite", []),
        ("NeuronInfoRuntimeApi", "get_neurons", []),
        ("SubnetInfoRuntimeApi", "get_all_dynamic_info", []),
        ("SubnetInfoRuntimeApi", "get_dynamic_info", None),
        ("SubnetInfoRuntimeApi", "get_subnet_hyperparams_v3", None),
        ("SubnetInfoRuntimeApi", "get_subnet_state", None),
        ("StakeInfoRuntimeApi", "get_stake_info_for_coldkey", []),
        ("StakeInfoRuntimeApi", "get_stake_info_for_coldkeys", []),
        ("StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", None),
        ("StakeInfoRuntimeApi", "get_stake_value_for_coldkey", 0),
        ("StakeInfoRuntimeApi", "get_stake_value_for_coldkeys", []),
        ("DelegateInfoRuntimeApi", "get_delegates", []),
        ("DelegateInfoRuntimeApi", "get_delegate", None),
        ("DelegateInfoRuntimeApi", "get_delegated", []),
        ("ColdkeySwapRuntimeApi", "get_scheduled_coldkey_swap", None),
        ("SwapRuntimeApi", "sim_swap_tao_for_alpha", EMPTY_SWAP_SIM),
        ("SwapRuntimeApi", "sim_swap_alpha_for_tao", EMPTY_SWAP_SIM),
        ("SwapRuntimeApi", "current_alpha_price", 10**9),
        ("SwapRuntimeApi", "current_alpha_price_all", []),
        ("SubnetRegistrationRuntimeApi", "get_network_registration_cost", 10**9),
        ("CrowdloanRuntimeApi", "get_crowdloans", []),
        ("CrowdloanRuntimeApi", "get_crowdloan", None),
        ("LeasingRuntimeApi", "get_leases", []),
        ("LeasingRuntimeApi", "get_lease", None),
    ]:
        fake.seed_runtime(api, method, value)
    return fake


@pytest.fixture()
def substrate() -> FakeSubstrate:
    return seeded_substrate()


@pytest.fixture()
def client(substrate: FakeSubstrate) -> Client:
    return Client("local", substrate=substrate)


def test_every_read_has_a_sample():
    missing = sorted(set(REGISTRY) - set(READ_SAMPLES))
    stale = sorted(set(READ_SAMPLES) - set(REGISTRY))
    assert not missing, f"reads without sample params: {missing}"
    assert not stale, f"samples for unregistered reads: {stale}"


def test_catalog_is_wellformed():
    catalog = list_reads()
    assert {entry["name"] for entry in catalog} == set(REGISTRY)
    for entry in catalog:
        assert entry["summary"], f"read {entry['name']} has no docstring"
        assert entry["category"]
        # Sample params carry every declared param name.
        assert set(entry["params"]) <= set(READ_SAMPLES[entry["name"]]) | set(entry["params"])


@pytest.mark.parametrize("name", sorted(REGISTRY))
def test_sample_params_match_declaration(name: str):
    declared = set(REGISTRY[name].params)
    sample = set(READ_SAMPLES[name])
    assert sample <= declared, f"{name}: sample has undeclared params {sample - declared}"


@pytest.mark.parametrize("name", sorted(REGISTRY))
@pytest.mark.asyncio
async def test_dispatches_at_head(name: str, client: Client):
    await client.read(name, **READ_SAMPLES[name])


@pytest.mark.parametrize("name", sorted(REGISTRY))
@pytest.mark.asyncio
async def test_dispatches_pinned_to_block(name: str, client: Client):
    snapshot = await client.at(90)
    await snapshot.read(name, **READ_SAMPLES[name])


class TestReadSemantics:
    """Spot checks that decoded values carry the right types/units."""

    @pytest.mark.asyncio
    async def test_balance_is_tao_denominated(self, client: Client, substrate: FakeSubstrate):
        substrate.seed("System", "Account", [ALICE], {"data": {"free": 1_500_000_000}})
        value = await client.read("balance", coldkey_ss58=ALICE)
        assert value.rao == 1_500_000_000
        assert value.netuid == 0

    @pytest.mark.asyncio
    async def test_scalar_read_casts_to_int(self, client: Client, substrate: FakeSubstrate):
        substrate.seed("SubtensorModule", "TxRateLimit", None, 1000)
        assert await client.read("tx_rate_limit") == 1000

    @pytest.mark.asyncio
    async def test_uid_read_none_when_unregistered(self, client: Client, substrate: FakeSubstrate):
        substrate.seed_default("SubtensorModule", "Uids", None)
        assert await client.read("uid", hotkey_ss58=ALICE_HOT, netuid=1) is None

    @pytest.mark.asyncio
    async def test_block_time_from_slot_duration(self, client: Client, substrate: FakeSubstrate):
        substrate.seed_constant("Aura", "SlotDuration", 250)
        assert await client.read("block_time") == 0.25
        assert await client.read("is_fast_blocks") is True
