"""Crowdloan flow: create -> read -> update cap."""

from __future__ import annotations

import pytest

import bittensor as sub
from tests.harness.samples import BOB

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_crowdloan_create_read_update(client, alice):
    alice_cold = alice.coldkey.ss58_address
    block = await client.block()
    # Derive the duration from the runtime rather than hardcoding one: the
    # localnet minimum is small, but the mainnet-clone CI job runs this suite
    # against the production runtime, whose minimum exceeds a fixed 5000.
    min_duration = int(await client.constant(sub.constants.Crowdloan.MinimumBlockDuration))

    result = await client.execute(
        sub.CreateCrowdloan(
            deposit_tao=100,
            min_contribution_tao=1,
            cap_tao=1000,
            end=block + min_duration + 100,
            target_ss58=BOB,
        ),
        alice,
    )
    assert result.success, result.message

    crowdloan_id = int(await client.query(sub.storage.Crowdloan.NextCrowdloanId)) - 1
    info = await client.read("crowdloan", crowdloan_id=crowdloan_id)
    assert info is not None
    assert info["creator"] == alice_cold
    assert info["raised"].tao >= 100  # the deposit counts toward the raise
    assert info["cap"].tao == 1000

    result = await client.execute(
        sub.UpdateCrowdloanCap(crowdloan_id=crowdloan_id, new_cap_tao=2000), alice
    )
    info = await client.read("crowdloan", crowdloan_id=crowdloan_id)
    assert result.success, result.message
    assert info is not None
    assert info["cap"].tao == 2000
