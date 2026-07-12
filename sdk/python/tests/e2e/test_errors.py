"""Typed error mapping from real chain dispatch failures."""

from __future__ import annotations

import pytest

import bittensor as sub

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_nonexistent_subnet_maps_to_semantic_code(client, alice):
    result = await client.execute(sub.BurnedRegister(netuid=999), alice)
    assert not result.success
    assert result.error is not None
    assert result.error.code.value == "subnet_not_exists"
