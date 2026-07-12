"""Value movement proven by state delta: transfers, staking, delegation."""

from __future__ import annotations

import pytest

import bittensor as sub
from tests.harness.samples import BOB

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_transfer_moves_exactly_the_amount(client, alice):
    before = await client.balances.get(BOB)
    result = await client.execute(sub.Transfer(dest_ss58=BOB, amount_tao=3.0), alice)
    after = await client.balances.get(BOB)
    assert result.success, result.message
    assert after.rao - before.rao == 3 * 10**9


async def test_add_stake_increases_stake(client, alice, owned_subnet):
    cold = alice.coldkey.ss58_address
    hot = alice.hotkey.ss58_address
    before = await client.staking.get(cold, hot, owned_subnet)
    result = await client.execute(
        sub.AddStake(hotkey_ss58=hot, netuid=owned_subnet, amount_tao=10.0), alice
    )
    after = await client.staking.get(cold, hot, owned_subnet)
    assert result.success, result.message
    assert after.rao > before.rao


async def test_delegation_lifecycle(client, alice, owned_subnet):
    """root_register -> delegate reads -> set_take (decrease path) -> nominations."""
    cold = alice.coldkey.ss58_address
    hot = alice.hotkey.ss58_address

    # Makes the hotkey a delegate; a rerun is a no-op refusal, which is fine —
    # the reads below prove the resulting state either way.
    await client.execute(sub.RootRegister(), alice)

    assert await client.read("is_delegate", hotkey_ss58=hot) is True

    delegate = await client.read("delegate", hotkey_ss58=hot)
    assert delegate is not None
    assert delegate.hotkey == hot
    assert 0 <= delegate.take <= 1

    all_delegates = await client.read("delegates")
    assert any(d.hotkey == hot for d in all_delegates)

    take_info = await client.read("delegate_take", hotkey_ss58=hot)
    assert take_info["min"] <= take_info["take"] <= take_info["max"]

    # Move take down to an absolute target: exercises the sugar's
    # "read current, choose decrease_take" path.
    target_u16 = max(take_info["take_u16"] - 1000, take_info["take_u16"] // 2)
    result = await client.execute(sub.SetTake(hotkey_ss58=hot, take=target_u16), alice)
    after_take = await client.read("delegate_take", hotkey_ss58=hot)
    assert result.success, result.message
    assert after_take["take_u16"] == target_u16

    nominations = await client.read("delegated", coldkey_ss58=cold)
    assert any(n.netuid == owned_subnet and n.stake.rao > 0 for n in nominations)

    by_coldkey = await client.read("stake_for_coldkeys", coldkey_ss58s=[cold])
    assert cold in by_coldkey
    assert any(p.netuid == owned_subnet for p in by_coldkey[cold])
