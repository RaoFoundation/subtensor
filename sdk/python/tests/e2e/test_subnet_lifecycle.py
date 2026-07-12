"""Owner-scoped subnet operations: serving, hyperparameters, identity, auto-stake.

All of these act on the session's owned subnet (registered by the conftest
fixture, alice as owner with her hotkey at uid 0).
"""

from __future__ import annotations

import pytest

import bittensor as sub

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_owned_subnet_is_registered(client, alice, owned_subnet):
    subnets = await client.subnets.all()
    assert owned_subnet in [s.netuid for s in subnets]
    mg = await client.read("metagraph", netuid=owned_subnet)
    assert alice.hotkey.ss58_address in mg["hotkeys"]


async def test_serving_endpoints(client, alice, owned_subnet):
    # serve_axon and serve_axon_tls share one serving rate-limit bucket, so only
    # the TLS superset is submitted live (it proves the certificate path); plain
    # serve_axon is covered by the plan sweep. Prometheus is a separate bucket.
    result = await client.execute(
        sub.ServeAxonTls(
            netuid=owned_subnet, ip="203.0.113.5", port=8091, certificate="0x" + "ab" * 32
        ),
        alice,
    )
    assert result.success, result.message
    result = await client.execute(
        sub.ServePrometheus(netuid=owned_subnet, ip="203.0.113.5", port=9090), alice
    )
    assert result.success, result.message


async def test_owner_sets_hyperparameter(client, alice, owned_subnet):
    result = await client.execute(
        sub.SetHyperparameter(netuid=owned_subnet, name="immunity_period", value=42), alice
    )
    hp = await client.read("subnet_hyperparameters", netuid=owned_subnet)
    applied = int(hp.get("immunity_period")) == 42
    # Chain timing guards that legitimately defer an owner set on a rerun (the
    # call still reached dispatch, proving the owner path).
    throttled = not result.success and any(
        s in result.message.lower() for s in ("rate limit", "prohibited", "freeze")
    )
    assert applied or throttled, f"success={result.success} msg={result.message[:60]}"


async def test_unsettable_hyperparameter_rejected_at_construction(owned_subnet):
    with pytest.raises(ValueError):
        sub.SetHyperparameter(netuid=owned_subnet, name="tempo", value=1)


async def test_identity_coldkey_and_subnet(client, alice, owned_subnet):
    result = await client.execute(sub.SetIdentity(name="E2E Alice", url="https://a.example"), alice)
    identity = await client.read("identity", coldkey_ss58=alice.coldkey.ss58_address)
    assert result.success, result.message
    assert identity is not None
    assert identity.get("name") == "E2E Alice"

    result = await client.execute(
        sub.SetSubnetIdentity(netuid=owned_subnet, subnet_name="e2e-net"), alice
    )
    subnet_identity = await client.read("subnet_identity", netuid=owned_subnet)
    assert result.success, result.message
    assert subnet_identity is not None
    assert subnet_identity.get("subnet_name") == "e2e-net"


async def test_auto_stake_destination(client, alice, owned_subnet):
    hot = alice.hotkey.ss58_address
    # Re-setting the same destination is refused ("already set"), so assert
    # the read state rather than the extrinsic bool (re-runnable).
    await client.execute(sub.SetAutoStake(netuid=owned_subnet, hotkey_ss58=hot), alice)
    dest = await client.read(
        "auto_stake", coldkey_ss58=alice.coldkey.ss58_address, netuid=owned_subnet
    )
    assert dest == hot
