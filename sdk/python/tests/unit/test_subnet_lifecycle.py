from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.cli.helpers import annotate_stake_groups_with_locks, netuid_groups
from bittensor.client import Client
from bittensor.reads.staking import StakePosition, StakeValuation
from bittensor.reads.subnets import SubnetLifecycleState, _decode_subnet_state
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT


def test_lifecycle_enum_decodes_scale_shapes() -> None:
    assert _decode_subnet_state("Started") is SubnetLifecycleState.STARTED
    assert _decode_subnet_state({"PendingDissolution": None}) is (
        SubnetLifecycleState.PENDING_DISSOLUTION
    )
    assert _decode_subnet_state("dissolving") is SubnetLifecycleState.DISSOLVING


@pytest.mark.asyncio
async def test_state_reads_new_storage_and_legacy_fallback() -> None:
    current = FakeSubstrate()
    current.seed("SubtensorModule", "SubnetState", [7], "Dissolving")
    current.seed_map(
        "SubtensorModule",
        "SubnetState",
        [(0, "Started"), (7, {"Dissolving": None})],
    )
    client = Client("local", substrate=current)

    assert await client.subnets.state(7, block=40) is SubnetLifecycleState.DISSOLVING
    assert await client.subnets.states(block=40) == {
        0: SubnetLifecycleState.STARTED,
        7: SubnetLifecycleState.DISSOLVING,
    }

    legacy = FakeSubstrate()
    legacy.seed_map("SubtensorModule", "NetworksAdded", [(0, True), (1, True)])
    legacy.seed("SubtensorModule", "NetworksAdded", [0], True)
    legacy.seed("SubtensorModule", "NetworksAdded", [1], True)
    legacy.seed("SubtensorModule", "FirstEmissionBlockNumber", [1], 12)
    legacy_client = Client("local", substrate=legacy)

    assert await legacy_client.subnets.states(block=39) == {
        0: SubnetLifecycleState.STARTED,
        1: SubnetLifecycleState.STARTED,
    }


@pytest.mark.asyncio
async def test_stake_state_and_price_join_are_pinned_and_continuous() -> None:
    fake = FakeSubstrate()
    fake.seed_map("SubtensorModule", "SubnetState", [(9, "PendingDissolution")])
    fake.seed_runtime(
        "StakeInfoRuntimeApi",
        "get_stake_info_for_coldkey",
        [
            {
                "hotkey": ALICE_HOT,
                "coldkey": ALICE,
                "netuid": 9,
                "stake": 2_000_000_000,
                "is_registered": False,
            }
        ],
    )
    fake.seed_runtime(
        "SwapRuntimeApi",
        "current_alpha_price_all",
        [{"netuid": 9, "price": 1_500_000_000}],
    )
    client = Client("local", substrate=fake)

    value = await client.staking.stake_value_for_coldkey(ALICE, block=41)

    assert value.block == 41
    assert value.positions[0].subnet_state is SubnetLifecycleState.PENDING_DISSOLUTION
    assert value.stake_value == Balance.from_rao(3_000_000_000)


def test_cli_marks_automatic_payout_stake_unavailable() -> None:
    position = StakePosition(
        hotkey=ALICE_HOT,
        coldkey=ALICE,
        netuid=9,
        stake=Balance.from_amount(2, 9),
        is_registered=False,
        subnet_state=SubnetLifecycleState.DISSOLVING,
    )
    valuation = StakeValuation(
        coldkey=ALICE,
        block=41,
        positions=[position],
        stake_value=Balance.from_tao(3),
        tao_per_alpha={9: 1.5},
    )

    groups = netuid_groups([position], valuation, {})
    annotate_stake_groups_with_locks(groups, {}, {}, {})

    assert groups[0]["subnet_state"] == "dissolving"
    assert groups[0]["note"] == "automatic payout in progress"
    assert groups[0]["availability_note"] == "unavailable for manual unstaking"
