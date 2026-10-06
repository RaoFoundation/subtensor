"""Exercise owner mode switches and exact raw weights through a development node."""

from __future__ import annotations

import asyncio
import os
import time

import pytest

import bittensor as bt
from tests.harness.samples import dev_wallet

E2E_ENDPOINT = os.getenv("E2E_ENDPOINT")

pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not E2E_ENDPOINT, reason="requires E2E_ENDPOINT with a writable localnet"),
]


async def test_null_yuma_null_emissions_via_python_sdk() -> None:
    alice = dev_wallet()
    miners = [dev_wallet("//Bob", "//Bob//hot"), dev_wallet("//Charlie", "//Charlie//hot")]
    storage = bt.storage.SubtensorModule

    async with bt.Client(E2E_ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        # This test changes global administration settings and uses funded dev keys.
        # Refuse public chains even if somebody points E2E_ENDPOINT at one.
        root = await client.query(bt.storage.Sudo.Key)
        assert root == alice.coldkey.ss58_address, "Null E2E requires the Alice development root"

        async def submit(call, wallet=alice, *, root=False):
            if root:
                call = bt.calls.Sudo.sudo(call=await client.compose(call))
            result = await client.submit_call(call, wallet)
            assert result.success, result.message

        await submit(bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0), root=True)
        await submit(bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0), root=True)
        await submit(bt.calls.SubtensorModule.register_network(hotkey=alice.hotkey.ss58_address))
        netuid = max(subnet.netuid for subnet in await client.subnets.all())
        settings = [
            bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0),
            bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
            bt.calls.AdminUtils.sudo_set_start_call_delay(delay=0),
            bt.calls.AdminUtils.sudo_set_weights_set_rate_limit(
                netuid=netuid, weights_set_rate_limit=0
            ),
            bt.calls.AdminUtils.sudo_set_min_allowed_weights(netuid=netuid, min_allowed_weights=0),
            bt.calls.AdminUtils.sudo_set_commit_reveal_weights_enabled(
                netuid=netuid, enabled=False
            ),
            bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                netuid=netuid, registration_allowed=True
            ),
        ]
        for call in settings:
            await submit(call, root=True)

        async def switch(mode):
            result = await client.execute_tool(
                "set_hyperparameter",
                {"netuid": netuid, "name": "epoch_consensus", "value": mode},
                alice,
            )
            assert result.success, result.message
            assert await client.query(storage.SubnetEpochConsensus, [netuid]) == mode

        async def epoch():
            before = await client.query(storage.LastMechansimStepBlock, [netuid])
            await submit(bt.calls.SubtensorModule.trigger_epoch(netuid=netuid))
            deadline = time.monotonic() + 30
            while await client.query(storage.LastMechansimStepBlock, [netuid]) == before:
                assert time.monotonic() < deadline, "forced epoch did not complete"
                await asyncio.sleep(0.2)
            values = await client.query(storage.Emission, [netuid])
            assert len(values) == 3 and all(value > 0 for value in values)
            return values

        yuma_validator_limit = await client.query(storage.MaxAllowedValidators, [netuid])
        await switch("Null")
        assert (
            await client.query(storage.SavedYumaMaxAllowedValidators, [netuid])
            == yuma_validator_limit
        )
        for miner in miners:
            await submit(
                bt.calls.SubtensorModule.burned_register(
                    netuid=netuid, hotkey=miner.hotkey.ss58_address
                ),
                miner,
            )
        await submit(bt.calls.SubtensorModule.start_call(netuid=netuid))
        await submit(
            bt.calls.SubtensorModule.add_stake(
                hotkey=alice.hotkey.ss58_address, netuid=netuid, amount_staked=100_000_000_000
            )
        )

        result = await client.execute(
            bt.intents.SetWeights(netuid=netuid, weights={1: 3, 2: 1}, raw_u16=True), alice
        )
        assert result.success, result.message
        assert await client.query(storage.Weights, [netuid, 0]) == [(1, 3), (2, 1)]
        first = await epoch()
        assert abs(first[1] - 3 * first[2]) <= 3

        await switch("Yuma")
        assert await client.query(storage.MaxAllowedValidators, [netuid]) == yuma_validator_limit
        assert await client.query(storage.SavedYumaMaxAllowedValidators, [netuid]) is None
        await epoch()
        await switch("Null")
        frozen = await client.query(storage.Bonds, [netuid, 0])
        final = await epoch()
        assert abs(final[1] - 3 * final[2]) <= 3
        assert await client.query(storage.Bonds, [netuid, 0]) == frozen
        assert await client.query(storage.Weights, [netuid, 0]) == [(1, 3), (2, 1)]

        result = await client.execute_tool(
            "trim_null_subnet_batch", {"netuid": netuid, "target": 64}, alice
        )
        assert result.success, result.message
        assert await client.query(storage.SubnetworkN, [netuid]) == 3
        assert await client.query(storage.MaxAllowedUids, [netuid]) == 64
        assert await client.query(storage.NullPruningTarget, [netuid]) is None
        await switch("Yuma")
        assert await client.query(storage.MaxAllowedValidators, [netuid]) == min(
            yuma_validator_limit, 64
        )
