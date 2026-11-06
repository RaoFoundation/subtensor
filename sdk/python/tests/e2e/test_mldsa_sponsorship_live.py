"""First-payment sweeps, reserve failure and recovery on a disposable chain.

Dave sponsors only these cases; E2E_WALLET_ALICE_LOCK must point to this suite's
own sponsor lock when workers run in parallel. No global settings are changed.
"""

from __future__ import annotations

import asyncio
import os

import pytest

import bittensor as bt
from bittensor.hashed import descriptor_storage, registration_descriptor_value
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from tests.e2e.test_mldsa_release import evidence
from tests.e2e.test_wallet_types import alice_submission as sponsor_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated chain"),
]


def attributes(events, module, name):
    return [
        entry["event"]["attributes"]
        for entry in events
        if (entry["event"]["module_id"], entry["event"]["event_id"]) == (module, name)
    ]


@pytest.mark.parametrize("code", [4, 6, 7])
@pytest.mark.parametrize("shielded", [False, True])
async def test_sponsored_transfer_all_reserve_boundaries_and_seed_recovery(code, shielded):
    seeds = [os.urandom(32) for _ in range(4)]
    sender, poor, recipient, failed_recipient = [
        Keypair.create_from_seed(seed, code) for seed in seeds
    ]
    sink = Keypair.create_from_seed(os.urandom(32), 1)
    sponsor = dev_wallet("//Dave")
    report = {
        "crypto_type": code,
        "shielded": shielded,
        "accounts": {
            "sender": sender.ss58_address,
            "poor_sender": poor.ss58_address,
            "recipient": recipient.ss58_address,
            "failed_recipient": failed_recipient.ss58_address,
            "sink": sink.ss58_address,
        },
        "successful": False,
        "receipts": [],
    }

    def save():
        evidence(f"sponsorship-sweep-recovery-{code}-{shielded}", report)

    async with (
        asyncio.timeout(1200),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        raw = client._substrate.raw
        deposit_name = "MlDsaRegistrationDeposit" if code in (7, 8) else "RegistrationDeposit"
        deposit = int(await client.constant(("HashedAccounts", deposit_name)))
        minimum = int(await client.constant(("Balances", "ExistentialDeposit")))
        cap = bt.Balance.from_rao(deposit)
        report.update(registration_deposit_rao=deposit, existential_deposit_rao=minimum)

        async def state(signer):
            return await client.query(bt.storage.System.Account, [signer.ss58_address])

        async def authority(signer):
            return await client.query(
                ("HashedAccounts", descriptor_storage(bytes(signer.hashed_descriptor))),
                [signer.ss58_address],
            )

        async def unregistered_snapshot(signer):
            return {
                "account": await state(signer),
                "authority": await authority(signer),
                "alias": await client.query(
                    ("HashedAccounts", "EvmAliases"),
                    ["0x" + bytes(signer.public_key)[:20].hex()],
                ),
            }

        async def receipt(label, result, *, success=True):
            assert result.block_hash and result.extrinsic_id, result.to_dict()
            index = int(result.extrinsic_id.split("-")[1])
            events = [
                entry
                for entry in await raw.get_events(result.block_hash)
                if entry.get("extrinsic_idx") == index
            ]
            report["receipts"].append(
                {"operation": label, **result.to_dict(), "canonical_events": events}
            )
            save()
            assert result.success is success, result.to_dict()
            return events

        async def fund(signer, rao):
            async with sponsor_submission():
                result = await client.execute(
                    bt.intents.Transfer(receiving_address(signer), bt.Balance.from_rao(rao)),
                    sponsor,
                    wait_for_finalization=True,
                    max_registration_deposit=cap,
                )
            await receipt("Dave explicitly sponsors and funds account", result)

        def fee_rao(events):
            paid = attributes(events, "TransactionPayment", "TransactionFeePaid")
            assert len(paid) == 1, paid
            return int(paid[0]["actual_fee"])

        async def all_fees_since(signer, start, result):
            """Account for the carrier and inner fees from canonical events."""
            end = int(result.extrinsic_id.split("-")[0])
            fees = []
            for number in range(start, end + 1):
                block_hash = await raw._session.request("chain_getBlockHash", [number])
                for event in await raw.get_events(block_hash):
                    if (event["event"]["module_id"], event["event"]["event_id"]) != (
                        "TransactionPayment",
                        "TransactionFeePaid",
                    ):
                        continue
                    item = event["event"]["attributes"]
                    if item["who"] == signer.ss58_address:
                        fees.append(
                            {
                                "block_hash": block_hash,
                                "index": event["extrinsic_idx"],
                                "actual_fee": int(item["actual_fee"]),
                            }
                        )
            assert len(fees) == 1 + int(shielded), fees
            report.setdefault("fee_evidence", []).append(fees)
            return sum(fee["actual_fee"] for fee in fees)

        def refunds(events, signer):
            return sum(
                int(item["amount"])
                for item in attributes(events, "Balances", "Deposit")
                if item["who"] == signer.ss58_address
            )

        def transferred(events, source, destination):
            movements = [
                item
                for item in attributes(events, "Balances", "Transfer")
                if item["from"] == source.ss58_address and item["to"] == destination.ss58_address
            ]
            assert len(movements) == 1, movements
            return int(movements[0]["amount"])

        await fund(sender, 100 * 10**9)
        await fund(poor, deposit + minimum)
        sweep = bt.intents.TransferAll(receiving_address(recipient), keep_alive=True)
        before_sender, before_authority = await state(sender), await authority(sender)
        before_recipient = await unregistered_snapshot(recipient)
        with pytest.raises(bt.RegistrationSponsorshipRequired, match="requires consent"):
            await client.execute(sweep, sender, wait_for_finalization=True)
        assert await state(sender) == before_sender
        assert await authority(sender) == before_authority
        assert await unregistered_snapshot(recipient) == before_recipient
        report["missing_consent_preserved_state"] = True
        save()

        # Funding covers the reserve plus ED, but not the transaction fee.
        # The SDK must refuse before signing. Bypass that preview deliberately
        # to prove the runtime also rolls back the actual atomic setup/sweep.
        poor_before, poor_authority = await state(poor), await authority(poor)
        failed_before = await unregistered_snapshot(failed_recipient)
        poor_sweep = bt.intents.TransferAll(receiving_address(failed_recipient), keep_alive=True)
        with pytest.raises(bt.PolicyError, match="insufficient spendable balance"):
            await client.execute(
                poor_sweep, poor, wait_for_finalization=True, max_registration_deposit=cap
            )
        assert await state(poor) == poor_before
        assert await authority(poor) == poor_authority
        assert await unregistered_snapshot(failed_recipient) == failed_before
        raw_batch = bt.calls.Utility.batch_all(
            calls=[
                await client.compose(
                    bt.calls.HashedAccounts.register(
                        descriptor=registration_descriptor_value(
                            bytes(failed_recipient.hashed_descriptor)
                        )
                    )
                ),
                await client.compose(
                    bt.calls.Balances.transfer_all(
                        dest=failed_recipient.ss58_address, keep_alive=True
                    )
                ),
            ]
        )
        fee_start = await client.finalized_block()
        failed = await client.submit_call(
            raw_batch, poor, wait_for_finalization=True, shielded=shielded
        )
        await receipt("underfunded reserve fails atomically on chain", failed, success=False)
        assert failed.error is not None and failed.error.name == "InsufficientBalance", (
            failed.to_dict()
        )
        assert await unregistered_snapshot(failed_recipient) == failed_before
        poor_after = await state(poor)
        assert poor_after["data"]["reserved"] == poor_before["data"]["reserved"]
        assert poor_after["data"]["free"] == poor_before["data"]["free"] - await all_fees_since(
            poor, fee_start, failed
        )
        assert (await authority(poor))["generation"] == poor_authority["generation"] + 1 + int(
            shielded
        )
        report["insufficient_reserve_preserved_recipient"] = True

        fee_start = await client.finalized_block()
        result = await client.execute(
            sweep,
            sender,
            wait_for_finalization=True,
            max_registration_deposit=cap,
            shielded=shielded,
        )
        events = await receipt("first-payment sweep with exact reserve allowance", result)
        sender_after, recipient_after = await state(sender), await state(recipient)
        amount = transferred(events, sender, recipient)
        assert sender_after["data"]["reserved"] == before_sender["data"]["reserved"] + deposit
        assert sender_after["data"]["free"] == minimum + refunds(events, sender)
        assert recipient_after["data"]["free"] == amount
        assert before_sender["data"]["free"] == (
            sender_after["data"]["free"]
            + deposit
            + amount
            + await all_fees_since(sender, fee_start, result)
        )
        assert (await authority(sender))["generation"] == before_authority["generation"] + 1 + int(
            shielded
        )
        recipient_authority = await authority(recipient)
        assert recipient_authority is not None
        report["first_sweep"] = {
            "sender_before": before_sender,
            "sender_after": sender_after,
            "recipient_after": recipient_after,
            "transferred_rao": amount,
            "fee_refund_rao": refunds(events, sender),
        }
        save()

        # An expendable drain may remove Balances' provider; the permanent
        # protected identity and its current generation must survive it.
        drained = await client.execute(
            bt.intents.TransferAll(sink.ss58_address, keep_alive=False),
            recipient,
            wait_for_finalization=True,
        )
        drain_events = await receipt("recipient drains all spendable free balance", drained)
        drained_state = await state(recipient)
        drain_amount = transferred(drain_events, recipient, sink)
        assert drained_state["data"]["reserved"] == 0
        assert drained_state["data"]["free"] == refunds(drain_events, recipient)
        assert drained_state["providers"] >= 1
        assert recipient_after["data"]["free"] == (
            drain_amount + drained_state["data"]["free"] + fee_rao(drain_events)
        )
        drained_authority = await authority(recipient)
        assert drained_authority["descriptor"] == recipient_authority["descriptor"]
        assert drained_authority["generation"] == recipient_authority["generation"] + 1
        report["drain"] = {
            "account_after": drained_state,
            "authority_after": drained_authority,
            "fee_refund_rao": refunds(drain_events, recipient),
        }

        restored = Keypair.create_from_seed(seeds[2], code)
        assert restored.ss58_address == recipient.ss58_address
        await fund(restored, 2 * 10**9)
        assert await authority(restored) == drained_authority
        recovered = await client.execute(
            bt.intents.Transfer(sink.ss58_address, 1), restored, wait_for_finalization=True
        )
        await receipt("restored seed spends after drain and re-funding", recovered)
        assert (await authority(restored))["generation"] == drained_authority["generation"] + 1
        assert (await state(sink))["data"]["free"] == drain_amount + 10**9
        report["successful"] = True
        save()
