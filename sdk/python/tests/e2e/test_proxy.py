"""Proxy signing mode: the real coldkey stays offline, a delegate signs.

One flow test — add -> proxied transfer -> filtered call -> remove -> refused
— because the steps only make sense in sequence against shared chain state.
"""

from __future__ import annotations

import pytest
from bittensor.keyfiles import Keypair

import bittensor as sub
from tests.harness.samples import BOB, BOB_HOT

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_proxy_flow(client, alice, bob, owned_subnet):
    alice_cold = alice.coldkey.ss58_address
    charlie = Keypair.create_from_uri("//Charlie").ss58_address

    # -- add: Bob becomes a Transfer-type proxy of Alice ----------------------
    result = await client.execute(sub.AddProxy(delegate_ss58=BOB, proxy_type="Transfer"), alice)
    proxy_state = await client.read("proxies", coldkey_ss58=alice_cold)
    assert result.success, result.message
    assert any(
        p["delegate"] == BOB and p["proxy_type"] == "Transfer" for p in proxy_state["proxies"]
    )

    try:
        # -- proxied transfer moves the REAL account's funds ------------------
        alice_before = await client.balances.get(alice_cold)
        charlie_before = await client.balances.get(charlie)
        result = await client.execute(
            sub.Transfer(dest_ss58=charlie, amount_tao=1.0), bob, proxy_for=alice_cold
        )
        alice_after = await client.balances.get(alice_cold)
        charlie_after = await client.balances.get(charlie)
        assert result.success, result.message
        assert charlie_after.rao - charlie_before.rao == 10**9
        assert alice_before.rao - alice_after.rao == 10**9

        # -- a call outside the proxy type fails via ProxyExecuted, not silently
        result = await client.execute(
            sub.AddStake(hotkey_ss58=BOB_HOT, netuid=owned_subnet, amount_tao=1.0),
            bob,
            proxy_for=alice_cold,
        )
        assert not result.success
        assert "filter" in result.message
    finally:
        # -- remove clears the delegation (also the rerun cleanup) -------------
        result = await client.execute(
            sub.RemoveProxy(delegate_ss58=BOB, proxy_type="Transfer"), alice
        )
    proxy_state = await client.read("proxies", coldkey_ss58=alice_cold)
    assert result.success, result.message
    assert proxy_state["proxies"] == []

    # -- without delegation the proxied call is refused (NotProxy) -------------
    result = await client.execute(
        sub.Transfer(dest_ss58=charlie, amount_tao=1.0), bob, proxy_for=alice_cold
    )
    assert not result.success
    assert "not a proxy" in result.message.lower()
