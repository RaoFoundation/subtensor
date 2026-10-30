"""Opening an MS multisig must budget its deposit plus both Shield fees."""

from __future__ import annotations

import asyncio
import os

import pytest

import bittensor as bt
from bittensor.intents.multisig import MultisigIntentAdapter
from bittensor.multisig import MultisigFundingError, multisig_deposit_rao
from tests.e2e.test_mldsa_extended import Evidence
from tests.e2e.test_mldsa_release_costs import (
    assert_signed,
    attributes,
    free,
    fresh_key,
    fund,
    observe,
    quote_pair,
    reserved,
    setup,
    snapshot,
)
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


@pytest.mark.parametrize("code", [7, 8])
async def test_ms_multisig_opening_budgets_deposit_and_both_shield_fees(code):
    opener, peer = fresh_key(code), fresh_key(code)
    report = Evidence(f"release-multisig-combined-opening-cost-{code}")
    async with (
        asyncio.timeout(1500),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        await setup(client)
        for signer in (opener, peer):
            await fund(client, report, signer)
        group = await client.multisig([opener.ss58_address, peer.ss58_address], 2)
        if code == 7:
            protection = await group.protection_call(deposit_payer=peer.ss58_address)
            for signer in (opener, peer):
                result = await group.approve(
                    protection, signer, shielded=False, wait_for_finalization=True
                )
                await observe(client, report, "quorum establishes protected multisig", result)
            assert (await group.protection_status())["registered"]
        else:
            status = await group.protection_status()
            assert not status["eligible"] and not status["registered"]
            for signer in (opener, peer):
                assert (await snapshot(client, signer))["authority"] is None
        async with alice_submission():
            result = await client.submit_call(
                await group.funding_call(5 * 10**9), dev_wallet(), wait_for_finalization=True
            )
        await observe(client, report, "fund multisig under its authority policy", result)
        destination = dev_wallet().coldkey.ss58_address
        transfer = bt.intents.Transfer(destination, 1)
        intent = bt.intents.MultisigExecute(
            threshold=2,
            other_signatories=[peer.ss58_address],
            call=transfer.to_dict(),
        )
        inner = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=destination, value=10**9)
        )
        call_hash = "0x" + bytes(inner.call_hash).hex()
        pending_key = [group.address, call_hash]
        plain, shield, _, carrier = await quote_pair(client, intent, opener)
        deposit = await multisig_deposit_rao(client._substrate, 2)
        minimum = int(await client.constant(("Balances", "ExistentialDeposit")))
        # Both old independent checks pass; only the combined obligation fails.
        available = (
            minimum + plain.fee.rao + max(deposit, carrier.rao) + min(deposit, carrier.rao) // 2
        )
        assert available >= minimum + max(deposit + plain.fee.rao, shield.fee.rao)
        assert available < minimum + deposit + shield.fee.rao
        async with alice_submission():
            result = await client.submit_call(
                bt.calls.Sudo.sudo(
                    call=await client.compose(
                        bt.calls.Balances.force_set_balance(
                            who=opener.ss58_address, new_free=available
                        )
                    )
                ),
                dev_wallet(),
                wait_for_finalization=True,
            )
        await observe(
            client, report, "set own opener between separate and combined budgets", result
        )

        async def state():
            return {
                "opener": await snapshot(client, opener),
                "peer": await snapshot(client, peer),
                "group": await client.query(bt.storage.System.Account, [group.address]),
                "protection": await client.query(
                    ("HashedAccounts", "DerivedAuthorities"), [group.address]
                ),
                "pending": await client.query(bt.storage.Multisig.Multisigs, pending_key),
            }

        before = await state()
        assert before["pending"] is None and free(before["opener"]) == available
        adapter = MultisigIntentAdapter(
            dispatch=bt.intents.MultisigExecute(
                threshold=2,
                other_signatories=[peer.ss58_address],
                call=transfer.to_dict(),
            ),
            semantic=transfer,
        )
        blocked_plans = []
        for approval in (intent, adapter):
            assert (await client.plan(approval, opener, shielded=False)).ok
            blocked = await client.plan(approval, opener, shielded=True)
            assert not blocked.ok, blocked.to_dict()
            blocked_plans.append(blocked.to_dict())
            with pytest.raises(bt.PolicyError, match=r"(?i)carrier|fee|deposit|balance"):
                await client.execute(approval, opener, shielded=True, wait_for_finalization=True)
            assert await state() == before
        with pytest.raises(MultisigFundingError, match=r"(?i)carrier|fee|deposit|balance"):
            await group.approve(inner, opener, shielded=True, wait_for_finalization=True)
        assert await state() == before
        report.add(
            "both approval APIs refuse combined shortfall without signing",
            available_rao=available,
            refundable_deposit_rao=deposit,
            inner_fee_rao=plain.fee.rao,
            carrier_fee_rao=carrier.rao,
            blocked_plans=blocked_plans,
            state=before,
        )

        await fund(client, report, opener, amount=10 * 10**9)
        ready = await state()
        opened = await group.approve(inner, opener, shielded=True, wait_for_finalization=True)
        events = await observe(
            client, report, "funded Shield approval reserves exact deposit", opened
        )
        after = await state()
        assert opened.data.get("shielded") is True
        assert_signed(ready["opener"], after["opener"], 2)
        assert reserved(after["opener"]) == reserved(ready["opener"]) + deposit
        assert int(after["pending"]["deposit"]) == deposit
        assert after["pending"]["depositor"] == opener.ss58_address
        assert after["pending"]["approvals"] == [opener.ss58_address]
        assert after["group"] == ready["group"]
        assert after["peer"] == ready["peer"]
        assert after["protection"] == ready["protection"]
        paid = attributes(events, "TransactionPayment", "TransactionFeePaid")
        assert len(paid) == 1
        assert free(ready["opener"]) - free(after["opener"]) - deposit > int(paid[0]["actual_fee"])
        cancelled = await client.submit_call(
            bt.calls.Multisig.cancel_as_multi(
                threshold=2,
                other_signatories=[peer.ss58_address],
                timepoint=after["pending"]["when"],
                call_hash=call_hash,
            ),
            opener,
            shielded=False,
            wait_for_finalization=True,
        )
        cancel_events = await observe(
            client, report, "cancel returns entire opening deposit", cancelled
        )
        final = await state()
        assert final["pending"] is None
        assert reserved(final["opener"]) == reserved(ready["opener"])
        assert_signed(after["opener"], final["opener"], 1)
        cancel_paid = attributes(cancel_events, "TransactionPayment", "TransactionFeePaid")
        assert len(cancel_paid) == 1
        assert free(final["opener"]) == (
            free(after["opener"]) + deposit - int(cancel_paid[0]["actual_fee"])
        )
        assert final["group"] == before["group"]
        assert final["peer"] == before["peer"]
        assert final["protection"] == before["protection"]
        report.add("no inner payment executed; group authority policy unchanged", state=final)
