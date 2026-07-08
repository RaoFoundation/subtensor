"""Read-side e2e: typed reads, generic accessors, snapshots, subscriptions.

Ports the read sections of the old verify.py sweep. Everything here is
read-only against the localnet (netuid 1 ships pre-registered on the image).
"""

from __future__ import annotations

import pytest

import bittensor as sub
from tests.harness.samples import ALICE

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_block_number(client):
    block = await client.block()
    assert isinstance(block, int) and block > 0


async def test_subnets_all_batched(client):
    subnets = await client.subnets.all()
    assert len(subnets) >= 2  # root + at least one subnet
    assert all(isinstance(s, sub.SubnetInfo) for s in subnets)


async def test_balance_and_existential_deposit(client, alice):
    balance = await client.balances.get(alice.coldkey.ss58_address)
    assert balance.rao > 0
    assert balance.netuid == 0
    deposit = await client.balances.existential_deposit()
    assert deposit.rao > 0


async def test_generic_accessors_over_generated_descriptors(client):
    tempo = await client.query(sub.storage.SubtensorModule.Tempo, [1])
    assert isinstance(tempo, int)
    ed = await client.constant(sub.constants.Balances.ExistentialDeposit)
    assert int(ed) > 0
    neurons = await client.runtime(sub.runtime_api.NeuronInfoRuntimeApi.get_neurons_lite, [1])
    assert isinstance(neurons, list)


async def test_reads_catalog_nonempty(client):
    assert len(client.reads()) >= 12


async def test_typed_reads(client):
    hp = await client.read("subnet_hyperparameters", netuid=1)
    assert isinstance(hp, dict) and "tempo" in hp

    assert isinstance(await client.read("weights_rate_limit", netuid=1), int)

    mg = await client.read("metagraph", netuid=1)
    assert isinstance(mg, dict) and "hotkeys" in mg

    positions = await client.read("stake_for_coldkey", coldkey_ss58=ALICE)
    assert isinstance(positions, list)

    kids = await client.read("children", hotkey_ss58=ALICE, netuid=1)
    assert isinstance(kids, list)


async def test_quote_stake_slippage_and_fee(client):
    quote = await client.read("quote_stake", netuid=1, amount_tao=1.0)
    assert quote.alpha.rao > 0
    assert quote.tao_fee.rao >= 0


async def test_metagraph_fast_path_matches_runtime(client):
    neurons = await client.neurons.all(1)
    raw = await client.runtime(sub.runtime_api.NeuronInfoRuntimeApi.get_neurons_lite, [1])
    assert len(neurons) == len(raw)
    assert all(isinstance(n, sub.Neuron) and n.hotkey for n in neurons)


async def test_snapshot_pins_reads_to_one_block(client, alice):
    snapshot = await client.at()
    assert snapshot.block > 0
    neurons_live = await client.neurons.all(1)
    neurons_snap = await snapshot.neurons.all(1)
    assert len(neurons_snap) == len(neurons_live)
    balance = await snapshot.balances.get(alice.coldkey.ss58_address)
    assert balance.rao >= 0


async def test_leases_read(client):
    leases = await client.read("leases")
    assert isinstance(leases, list)
    assert await client.read("lease", lease_id=999_999) is None


async def test_block_subscription_streams_increasing_headers(client):
    seen: list[int] = []
    async for header in client.blocks():
        seen.append(header.number)
        if len(seen) >= 2:
            break
    assert len(seen) == 2
    assert seen[1] >= seen[0]
