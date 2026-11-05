"""Bounded approvals survive key rotation and fail atomically on a live chain."""

from __future__ import annotations

import os

import pytest

import bittensor as bt
from bittensor.limit_orders import authorized_order
from bittensor.sp_core import Keypair, requires_registration
from tests.e2e.test_mldsa_extended import Evidence
from tests.e2e.test_mldsa_localnet import fund

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated chain"),
]


@pytest.mark.parametrize("code", [4, 6, 7, 8, 9, 10, 11, 12])
async def test_full_approval_record_atomicity_rotation_recovery_and_reclamation(code):
    seed = os.urandom(32)
    owner = Keypair.create_from_seed(seed, code)
    report = Evidence(f"approval-lifecycle-{code}")
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, owner)
        orders = client.limit_orders
        terms = {
            "V1": {
                "signer": owner.ss58_address,
                "hotkey": owner.ss58_address,
                "netuid": 1,
                "order_type": "LimitBuy",
                "amount": 10**9,
                "limit_price": 10**15,
                "expiry": 2_000_000_000_000,
                "fee_rate": 0,
                "fee_recipient": owner.ss58_address,
                "relayer": [owner.ss58_address],
                "max_slippage": 10**9,
                "chain_id": await client.query(("EVMChainId", "ChainId")),
                "partial_fills_enabled": True,
            }
        }
        prepared = [await orders.prepare(terms, nonce=n) for n in range(1, 11)]

        async def state():
            return await client.query(("LimitOrders", "ApprovedOrders"), [owner.ss58_address])

        async def account():
            return await client.query(bt.storage.System.Account, [owner.ss58_address])

        async def submit(call, *, success=True, error=None, signer=owner):
            receipt = await client.submit_call(
                call, signer, shielded=False, wait_for_finalization=True
            )
            report.add("submitted", receipt=receipt.to_dict())
            assert receipt.success == success and receipt.block_hash, receipt.to_dict()
            if error:
                assert receipt.error.name == error, receipt.to_dict()
            return receipt

        def approve(order):
            return bt.calls.LimitOrders.authorize_order(
                order=authorized_order(order.nonce, order.order)
            )

        before = await account()
        # One transaction fills the entire record; the ninth entry must roll
        # back the entire batch instead of leaving eight orphaned approvals.
        await submit(
            bt.calls.Utility.batch_all(
                calls=[await client.compose(approve(order)) for order in prepared[:9]]
            ),
            success=False,
            error="TooManyApprovedOrders",
        )
        assert await state() is None
        assert (await account())["data"]["reserved"] == before["data"]["reserved"]
        await submit(
            bt.calls.Utility.batch_all(
                calls=[await client.compose(approve(order)) for order in prepared[:8]]
            )
        )
        full = await state()
        assert len(full["orders"]) == 8 and full["highest_nonce"] == 8

        # Recreate the key from its seed after rotations and reuse a full record.
        recovered = Keypair.create_from_seed(seed, code)
        await submit(bt.calls.System.remark(remark=b"rotate without invalidating approvals"))
        await submit(approve(prepared[0]), signer=recovered)
        assert await state() == full
        await submit(approve(prepared[8]), success=False, error="TooManyApprovedOrders")
        assert await state() == full

        # A nonexistent cancellation does not grow storage or advance the watermark.
        result = await orders.cancel(
            prepared[9], recovered, shielded=False, wait_for_finalization=True
        )
        assert result.success, result.to_dict()
        assert await state() == full
        result = await orders.cancel(
            prepared[0], recovered, shielded=False, wait_for_finalization=True
        )
        assert result.success, result.to_dict()
        await submit(approve(prepared[8]), signer=recovered)
        reclaimed = await state()
        assert len(reclaimed["orders"]) == 8 and reclaimed["highest_nonce"] == 9
        await submit(approve(prepared[0]), success=False, error="OrderNonceUsed")
        assert await state() == reclaimed
        assert (await account())["data"]["reserved"] == before["data"]["reserved"]
        report.add("bounded-final-state", state=reclaimed, rotating=requires_registration(owner))
