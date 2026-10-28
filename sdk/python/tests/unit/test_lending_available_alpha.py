"""MinerCollateral is keyed by (netuid, hotkey, coldkey) — three params."""

from __future__ import annotations

import pytest

from bittensor.intents import lending as lending_intent


class _RecordingSubstrate:
    def __init__(self):
        self.queries: list[tuple] = []

    async def runtime_call(self, api, method, params=None, block_hash=None):
        if method == "get_stake_info_for_hotkey_coldkey_netuid":
            return {"stake": 10**12}
        if method == "get_stake_availability_for_coldkeys":
            return {"5Owner": {1: {"available": 10**12}}}
        return None

    async def query(self, module, storage_function, params=None, block_hash=None):
        self.queries.append((module, storage_function, list(params or []), block_hash))
        if storage_function == "MinerCollateral":
            if params is not None and len(params) != 3:
                raise TypeError(
                    f"Storage function requires 3 parameters, {len(params)} given"
                )
            return {"locked": 0, "min_locked": 0, "earned": 0, "drain_ratio": {"bits": 0}}
        return None


@pytest.mark.asyncio
async def test_available_alpha_queries_miner_collateral_with_three_keys():
    substrate = _RecordingSubstrate()
    owner = "5Owner"
    hotkey = "5Hotkey"
    netuid = 1
    bal = await lending_intent._available_alpha(substrate, owner, hotkey, netuid)
    assert bal.rao == 10**12
    collateral_queries = [
        q for q in substrate.queries if q[0] == "SubtensorModule" and q[1] == "MinerCollateral"
    ]
    assert len(collateral_queries) == 1
    assert collateral_queries[0][2] == [netuid, hotkey, owner]


@pytest.mark.asyncio
async def test_available_alpha_rejects_single_tuple_key():
    """Regression: wrapping the triple as one list element must not be used."""

    class _Strict(_RecordingSubstrate):
        async def query(self, module, storage_function, params=None, block_hash=None):
            if storage_function == "MinerCollateral":
                # Simulate scale codec / substrate-interface arity check.
                if params is not None and len(params) == 1 and isinstance(params[0], tuple):
                    raise TypeError("Storage function requires 3 parameters, 1 given")
            return await super().query(module, storage_function, params, block_hash)

    substrate = _Strict()
    # Correct call path succeeds.
    await lending_intent._available_alpha(substrate, "5Owner", "5Hotkey", 1)
