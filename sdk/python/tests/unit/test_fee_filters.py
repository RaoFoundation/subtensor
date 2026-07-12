"""Tests for coldkey fee routing mirror and plan warnings."""

from __future__ import annotations

import pytest

from bittensor.client import Client
from bittensor.fee_filters import (
    COLDKEY_FEE_WARNING,
    COLDKEY_PAYS_FEE_CALLS,
    charges_coldkey_fee,
    iter_call_leaves,
)
from bittensor.intents import build
from tests.harness.fake_substrate import ComposedCall, FakeSubstrate
from tests.harness.samples import dev_wallet


def test_coldkey_pays_fee_calls_matches_runtime_subset():
    assert ("SubtensorModule", "serve_axon") in COLDKEY_PAYS_FEE_CALLS
    assert ("Commitments", "set_commitment") in COLDKEY_PAYS_FEE_CALLS
    assert ("Balances", "transfer_keep_alive") not in COLDKEY_PAYS_FEE_CALLS


def test_iter_call_leaves_unwraps_proxy():
    inner = ComposedCall("SubtensorModule", "serve_axon", {"netuid": 1})
    outer = ComposedCall("Proxy", "proxy", {"real": "5Grw...", "call": inner})
    assert list(iter_call_leaves(outer)) == [("SubtensorModule", "serve_axon")]


def test_charges_coldkey_fee_on_leaf():
    call = ComposedCall("SubtensorModule", "associate_evm_key", {"netuid": 1})
    assert charges_coldkey_fee(call)


@pytest.mark.asyncio
async def test_plan_warns_hotkey_coldkey_fee_routing():
    substrate = FakeSubstrate()
    client = Client("local", substrate=substrate)
    wallet = dev_wallet()
    plan = await client.plan(
        build("serve_axon", {"netuid": 1, "ip": "203.0.113.5", "port": 8091}), wallet
    )
    assert COLDKEY_FEE_WARNING in plan.warnings


@pytest.mark.asyncio
async def test_plan_skips_warning_for_coldkey_signer():
    substrate = FakeSubstrate()
    client = Client("local", substrate=substrate)
    wallet = dev_wallet()
    plan = await client.plan(
        build("transfer", {"dest_ss58": wallet.coldkey.ss58_address, "amount_tao": 1.0}), wallet
    )
    assert COLDKEY_FEE_WARNING not in plan.warnings


@pytest.mark.asyncio
async def test_plan_skips_warning_when_proxy_for_set():
    substrate = FakeSubstrate()
    client = Client("local", substrate=substrate)
    wallet = dev_wallet()
    plan = await client.plan(
        build("serve_axon", {"netuid": 1, "ip": "203.0.113.5", "port": 8091}),
        wallet,
        proxy_for=wallet.coldkey.ss58_address,
    )
    assert COLDKEY_FEE_WARNING not in plan.warnings
