"""Every registered intent composes and plans against the LIVE runtime.

The offline table tests (tests/unit/test_intents_table.py) prove the SDK is
self-consistent; this sweep proves it matches the actual chain metadata —
the behavioral complement to ``codegen.check --drift``. A call rename or
argument change on the chain fails here even if the committed generated
layer is stale.
"""

from __future__ import annotations

import pytest

import bittensor as sub
from bittensor.intents import REGISTRY, build
from tests.harness.samples import BOB, BOB_HOT, INTENT_SAMPLES

pytestmark = pytest.mark.asyncio(loop_scope="session")


# Weights intents pre-flight the signer's registration on the target subnet,
# so they plan against the session's owned subnet (alice is uid 0 there) —
# netuid 1 on a long-lived dev chain may not have her registered.
NEEDS_OWNED_SUBNET = {"set_weights", "commit_weights", "reveal_weights"}


@pytest.mark.parametrize("op", sorted(REGISTRY))
async def test_intent_plans_against_live_metadata(op: str, client, alice, owned_subnet):
    args = dict(INTENT_SAMPLES[op])
    if op in NEEDS_OWNED_SUBNET:
        args["netuid"] = owned_subnet
    try:
        plan = await client.plan(build(op, args), alice)
    except sub.ChainError as error:
        # The weights pre-flight reads real chain state and may legitimately
        # answer "rate limited" on a freshly registered subnet (LastUpdate is
        # set at registration). That decision required live metadata to
        # decode, which is what this sweep proves — accept it.
        if op in NEEDS_OWNED_SUBNET and error.code is sub.ErrorCode.RATE_LIMITED:
            return
        raise
    assert plan.op == op


async def test_plan_simulates_real_fee(client, alice):
    plan = await client.plan(sub.Transfer(dest_ss58=BOB, amount_tao=1.0), alice)
    assert plan.fee is not None
    assert plan.fee.rao > 0


async def test_policy_blocks_with_live_fee(client, alice):
    with pytest.raises(sub.PolicyError):
        await client.execute(
            sub.Transfer(dest_ss58=BOB, amount_tao=5.0),
            alice,
            policy=sub.Policy(max_spend_tao=1.0),
        )


async def test_spend_cap_blocks_value_movers(client, alice):
    cap = sub.Policy(max_spend_tao=1.0)
    value_movers = [
        sub.TransferStake(
            dest_coldkey_ss58=BOB,
            hotkey_ss58=BOB_HOT,
            origin_netuid=1,
            dest_netuid=1,
            amount_alpha=1.0,
        ),
        sub.RegisterSubnet(),
        sub.BurnedRegister(netuid=1),
    ]
    for intent in value_movers:
        plan = await client.plan(intent, alice, policy=cap)
        assert not plan.ok, f"spend cap did not block {intent.op}"


async def test_netuid_allowlist_blocks_live(client, alice):
    allow = sub.Policy(allowed_netuids=[1])
    plan = await client.plan(
        sub.MoveStake(
            origin_hotkey_ss58=BOB_HOT,
            origin_netuid=1,
            dest_hotkey_ss58=BOB_HOT,
            dest_netuid=2,
            amount_alpha=1.0,
        ),
        alice,
        policy=allow,
    )
    assert not plan.ok
    plan = await client.plan(sub.UnstakeAllAlpha(hotkey_ss58=BOB_HOT), alice, policy=allow)
    assert not plan.ok
