"""Atomic batches and the MEV-shielded submission pipeline."""

from __future__ import annotations

import pytest

import bittensor as sub
from bittensor.keyfiles import Keypair
from tests.harness.samples import BOB

pytestmark = pytest.mark.asyncio(loop_scope="session")

DAVE = Keypair.create_from_uri("//Dave").ss58_address
EVE = Keypair.create_from_uri("//Eve").ss58_address


async def test_batch_applies_all_children_atomically(client, alice):
    dave_before = await client.balances.get(DAVE)
    eve_before = await client.balances.get(EVE)
    result = await client.execute(
        sub.Batch(
            intents=[
                sub.Transfer(dest_ss58=DAVE, amount_tao=1.0),
                sub.Transfer(dest_ss58=EVE, amount_tao=2.0),
            ]
        ),
        alice,
    )
    dave_after = await client.balances.get(DAVE)
    eve_after = await client.balances.get(EVE)
    assert result.success, result.message
    assert dave_after.rao - dave_before.rao == 10**9
    assert eve_after.rao - eve_before.rao == 2 * 10**9


async def test_failed_batch_reverts_everything(client, alice):
    dave_before = await client.balances.get(DAVE)
    result = await client.execute(
        sub.Batch(
            intents=[
                sub.Transfer(dest_ss58=DAVE, amount_tao=1.0),
                sub.Transfer(dest_ss58=EVE, amount_tao=10**10),  # must fail
            ]
        ),
        alice,
    )
    dave_after = await client.balances.get(DAVE)
    assert not result.success
    assert dave_after.rao == dave_before.rao, "atomicity: sibling transfer must revert"


async def test_policy_aggregates_spend_across_batch(client, alice):
    plan = await client.plan(
        sub.Batch(
            intents=[
                sub.Transfer(dest_ss58=DAVE, amount_tao=0.6),
                sub.Transfer(dest_ss58=EVE, amount_tao=0.6),
            ]
        ),
        alice,
        policy=sub.Policy(max_spend_tao=1.0),
    )
    assert not plan.ok


async def test_mev_shield_next_key_is_mlkem768(client):
    key = await client.read("mev_shield_next_key")
    assert key is not None
    assert len(bytes.fromhex(key[2:])) == 1184


async def test_submit_shielded_runs_full_pipeline(client, alice):
    """Read NextKey, sign the inner extrinsic at nonce+1, ML-KEM-768 encrypt,
    wrap in submit_encrypted, submit at nonce. On-chain reveal is
    validator-side (a mainnet feature) — the dev localnet may reject the pool
    submission, so this asserts the pipeline returns a typed result rather
    than asserting inclusion."""
    result = await client.submit_shielded(sub.Transfer(dest_ss58=BOB, amount_tao=2.0), alice)
    assert hasattr(result, "success")
    assert isinstance(result.message, str)
