"""Missing mixed-quorum and receiving-address proxy paths on an activated dev chain.

Uses disposable keys only. The current SDK requires a direct sponsor for first
registration; a deposit cap does not authorize registration through a wrapper.
These tests preserve that policy and distinguish proxy/multisig inner failures
from successful outer extrinsics. No weights or global chain settings change.
"""

from __future__ import annotations

import json
import os
from dataclasses import replace
from pathlib import Path
from unittest.mock import Mock

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from bittensor.sp_core import MLDSA_CRYPTO_TYPES, Keypair, ss58_decode
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


class Evidence:
    def __init__(self, name):
        self.name = name
        self.items = []

    def add(self, label, **values):
        self.items.append({"step": label, **values})
        if directory := os.getenv("E2E_EVIDENCE_DIR"):
            path = Path(directory)
            path.mkdir(parents=True, exist_ok=True)
            (path / f"{self.name}.json").write_text(
                json.dumps(self.items, indent=2, default=str) + "\n"
            )


def fresh_key(code):
    return Keypair.create_from_seed(os.urandom(32), code)


async def account(client, address):
    return await client.query(bt.storage.System.Account, [address])


async def snapshot(client, signer):
    return {
        "account": await account(client, signer.ss58_address),
        "authority": await client.query(("HashedAccounts", "MlDsaAccounts"), [signer.ss58_address]),
    }


async def snapshots(client, signers):
    return {signer.ss58_address: await snapshot(client, signer) for signer in signers}


def reserved(state):
    return int(state["account"]["data"]["reserved"])


def free(state):
    return int(state["account"]["data"]["free"])


def assert_signed_once(signer, before, after):
    assert int(after["account"]["nonce"]) == int(before["account"]["nonce"]) + 1
    if signer.crypto_type in MLDSA_CRYPTO_TYPES:
        assert after["authority"]["generation"] == before["authority"]["generation"] + 1
        assert after["authority"]["descriptor"] == before["authority"]["descriptor"]
        assert (after["authority"]["commitment"] == before["authority"]["commitment"]) == (
            signer.crypto_type == 8
        )
    else:
        assert before["authority"] is after["authority"] is None


def event(result, module, name):
    for entry in result.events:
        value = entry.get("event", entry)
        if (value.get("module_id"), value.get("event_id")) == (module, name):
            return value["attributes"]
    raise AssertionError((module, name, result.to_dict()))


async def observe(client, signer, operation, evidence, label, *, success=True):
    before = await snapshot(client, signer)
    result = await operation
    after = await snapshot(client, signer)
    # Failed normalized receipts omit events. Recover only this extrinsic's
    # records from its finalized block so inner-denial assertions remain exact.
    if not result.success and result.block_hash and result.extrinsic_id:
        index = int(result.extrinsic_id.rsplit("-", 1)[1])
        events = [
            entry
            for entry in await client._substrate.events(result.block_hash)
            if entry.get("extrinsic_idx") == index
        ]
        assert events, result.to_dict()
        result = replace(result, events=events)
    evidence.add(
        label,
        signer=signer.ss58_address,
        receipt=result.to_dict(),
        events=result.events,
        signer_before=before,
        signer_after=after,
    )
    assert result.success == success and result.block_hash, result.to_dict()
    assert_signed_once(signer, before, after)
    return result


async def fund(client, signer, evidence):
    # The cap is the actual current reserve, not a guessed TAO allowance.
    deposit = int(await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit))
    async with alice_submission():
        result = await client.execute(
            bt.intents.Transfer(receiving_address(signer), 100),
            dev_wallet("//Bob"),
            max_registration_deposit=bt.Balance.from_rao(deposit),
            wait_for_finalization=True,
        )
    evidence.add("fund-disposable-signer", address=signer.ss58_address, receipt=result.to_dict())
    assert result.success and result.block_hash, result.to_dict()


@pytest.mark.parametrize("owner_code,delegate_code", [(7, 8), (8, 7)])
async def test_proxy_receiving_sponsorship_denial_and_revocation(owner_code, delegate_code):
    """A restricted or revoked delegate cannot move principal funds or reserve setup."""
    owner, delegate, recipient = (
        fresh_key(owner_code),
        fresh_key(delegate_code),
        fresh_key(delegate_code),
    )
    trace = Evidence(f"mldsa-extended-proxy-{owner_code}-{delegate_code}")
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        for signer in (owner, delegate):
            await fund(client, signer, trace)
        deposit = int(await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit))
        cap = bt.Balance.from_rao(deposit)
        initial_owner_reserved = reserved(await snapshot(client, owner))
        await observe(
            client,
            owner,
            client.submit_call(
                bt.calls.Proxy.add_proxy(
                    delegate=delegate.ss58_address, proxy_type="NonTransfer", delay=0
                ),
                owner,
                wait_for_finalization=True,
            ),
            trace,
            "grant-restricted-proxy",
        )
        payment = bt.intents.Transfer(receiving_address(recipient), 1)
        before = await snapshots(client, (owner, delegate, recipient))
        callback = Mock(return_value=True)
        with pytest.raises(ValueError, match="direct sponsor"):
            await client.execute(
                payment,
                delegate,
                proxy_for=owner.ss58_address,
                proxy_type="NonTransfer",
                max_registration_deposit=cap,
                on_registration_sponsorship=callback,
                wait_for_finalization=True,
            )
        callback.assert_not_called()
        assert await snapshots(client, (owner, delegate, recipient)) == before

        # Even a direct principal payment must explicitly consent to this exact
        # reserve. Missing/insufficient/declined consent never consumes a nonce.
        for options in (
            {},
            {"max_registration_deposit": bt.Balance.from_rao(deposit - 1)},
            {"max_registration_deposit": cap, "on_registration_sponsorship": lambda _: False},
        ):
            with pytest.raises(bt.RegistrationSponsorshipRequired) as rejected:
                await client.execute(payment, owner, wait_for_finalization=True, **options)
            assert rejected.value.quote.payer_account == owner.ss58_address
            assert rejected.value.quote.recipient_account == recipient.ss58_address
            assert rejected.value.quote.deposit == cap
            assert await snapshots(client, (owner, delegate, recipient)) == before
        trace.add("missing-wrapped-and-declined-consent-preserve-state", before=before)

        await observe(
            client,
            owner,
            client.execute(
                payment, owner, max_registration_deposit=cap, wait_for_finalization=True
            ),
            trace,
            "principal-explicitly-sponsors-recipient",
        )
        registered = await snapshots(client, (owner, delegate, recipient))
        assert (
            reserved(registered[owner.ss58_address])
            == reserved(before[owner.ss58_address]) + deposit
        )
        assert registered[delegate.ss58_address] == before[delegate.ss58_address]
        assert free(registered[recipient.ss58_address]) == 10**9
        assert registered[recipient.ss58_address]["authority"]["generation"] == 0

        # Registration now only adds a check guard. The restricted delegate's
        # outer transaction is included, but the inner transfer must fail.
        principal_before = await snapshot(client, owner)
        recipient_before = await snapshot(client, recipient)
        delegate_before = await snapshot(client, delegate)
        denied = await observe(
            client,
            delegate,
            client.execute(
                payment,
                delegate,
                proxy_for=owner.ss58_address,
                proxy_type="NonTransfer",
                max_registration_deposit=bt.Balance.from_rao(0),
                wait_for_finalization=True,
            ),
            trace,
            "restricted-proxy-inner-transfer-denied",
            success=False,
        )
        assert denied.error.name == "CallFiltered", denied.to_dict()
        assert "Err" in event(denied, "Proxy", "ProxyExecuted")["result"]
        assert await snapshot(client, owner) == principal_before
        assert await snapshot(client, recipient) == recipient_before
        delegate_after = await snapshot(client, delegate)
        assert reserved(delegate_after) == reserved(delegate_before)
        assert free(delegate_after) < free(delegate_before)

        await observe(
            client,
            owner,
            client.submit_call(
                bt.calls.Proxy.add_proxy(delegate=delegate.ss58_address, proxy_type="Any", delay=0),
                owner,
                wait_for_finalization=True,
            ),
            trace,
            "grant-full-proxy",
        )
        principal_before = await snapshot(client, owner)
        recipient_before = await snapshot(client, recipient)
        delegate_before = await snapshot(client, delegate)
        unexpected_consent = Mock(side_effect=AssertionError("already registered recipient"))
        allowed = await observe(
            client,
            delegate,
            client.execute(
                payment,
                delegate,
                proxy_for=owner.ss58_address,
                proxy_type="Any",
                max_registration_deposit=bt.Balance.from_rao(0),
                on_registration_sponsorship=unexpected_consent,
                wait_for_finalization=True,
            ),
            trace,
            "full-proxy-transfers-without-new-registration",
        )
        assert "Ok" in event(allowed, "Proxy", "ProxyExecuted")["result"]
        unexpected_consent.assert_not_called()
        principal_after = await snapshot(client, owner)
        recipient_after = await snapshot(client, recipient)
        assert free(principal_after) == free(principal_before) - 10**9
        assert reserved(principal_after) == reserved(principal_before)
        assert principal_after["account"]["nonce"] == principal_before["account"]["nonce"]
        assert principal_after["authority"] == principal_before["authority"]
        assert free(recipient_after) == free(recipient_before) + 10**9
        assert recipient_after["authority"] == recipient_before["authority"]
        assert reserved(await snapshot(client, delegate)) == reserved(delegate_before)

        # Preserve a previously composed full-permission call, then revoke that
        # permission. The still-present NonTransfer grant cannot satisfy Any.
        prepared = await client.plan(
            payment, delegate, proxy_for=owner.ss58_address, proxy_type="Any"
        )
        await observe(
            client,
            owner,
            client.submit_call(
                bt.calls.Proxy.remove_proxy(
                    delegate=delegate.ss58_address, proxy_type="Any", delay=0
                ),
                owner,
                wait_for_finalization=True,
            ),
            trace,
            "revoke-full-proxy",
        )
        principal_before = await snapshot(client, owner)
        recipient_before = await snapshot(client, recipient)
        revoked = await observe(
            client,
            delegate,
            client.submit_call(prepared.call, delegate, wait_for_finalization=True),
            trace,
            "previously-composed-call-rejected-after-revocation",
            success=False,
        )
        assert revoked.error.name == "NotProxy", revoked.to_dict()
        assert await snapshot(client, owner) == principal_before
        assert await snapshot(client, recipient) == recipient_before
        await observe(
            client,
            owner,
            client.submit_call(
                bt.calls.Proxy.remove_proxy(
                    delegate=delegate.ss58_address, proxy_type="NonTransfer", delay=0
                ),
                owner,
                wait_for_finalization=True,
            ),
            trace,
            "remove-last-proxy-and-release-proxy-deposit",
        )
        assert not (await client.query(bt.storage.Proxy.Proxies, [owner.ss58_address]))[0]
        assert reserved(await snapshot(client, owner)) == initial_owner_reserved + deposit


async def test_three_of_four_sr_ed_ms_hashed_ms_receiving_and_all_member_cancellations():
    """Both ML profiles complete a 3-of-4 quorum; all four schemes recover deposits."""
    members = [fresh_key(code) for code in (1, 0, 8, 7)]
    recipient = fresh_key(8)
    trace = Evidence("mldsa-extended-three-of-four")
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        for signer in members:
            await fund(client, signer, trace)
        group = await client.multisig([receiving_address(key) for key in members], 3)
        canonical = await client.multisig([key.ss58_address for key in members], 3)
        assert group.address == canonical.address
        status = await group.protection_status()
        assert status["eligible"] and not status["registered"]
        protection = await group.protection_call(deposit_payer=members[3].ss58_address)
        before_protection = await snapshots(client, members)
        for index in (0, 1, 3):
            await observe(
                client,
                members[index],
                group.approve(protection, members[index], wait_for_finalization=True),
                trace,
                f"protect-composite-approval-{index}",
            )
        assert (await group.protection_status())["registered"]
        protection_record = await client.query(
            ("HashedAccounts", "DerivedAuthorities"), [group.address]
        )
        derived_deposit = int(
            await client.constant(bt.constants.HashedAccounts.DerivedRegistrationDeposit)
        )
        for index, signer in enumerate(members):
            assert reserved(await snapshot(client, signer)) == reserved(
                before_protection[signer.ss58_address]
            ) + (derived_deposit if index == 3 else 0)
        async with alice_submission():
            funded = await client.submit_call(
                await group.funding_call(10 * 10**9),
                dev_wallet("//Bob"),
                wait_for_finalization=True,
            )
        trace.add("fund-protected-composite", receipt=funded.to_dict())
        assert funded.success and funded.block_hash, funded.to_dict()

        payment = bt.intents.Transfer(receiving_address(recipient), 1).to_dict()

        def intent(kind, index, timepoint=None):
            return kind(
                threshold=3,
                other_signatories=[
                    receiving_address(key) for i, key in enumerate(members) if i != index
                ],
                call=payment,
                timepoint=timepoint,
            )

        deposit = int(await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit))
        before = await snapshots(client, [*members, recipient])
        composite_before = await account(client, group.address)
        callback = Mock(return_value=True)
        for index, signer in enumerate(members):
            with pytest.raises(ValueError, match="direct sponsor"):
                await client.execute(
                    intent(bt.intents.MultisigApprove, index),
                    signer,
                    max_registration_deposit=bt.Balance.from_rao(deposit),
                    on_registration_sponsorship=callback,
                    wait_for_finalization=True,
                )
        callback.assert_not_called()
        assert await snapshots(client, [*members, recipient]) == before
        assert await account(client, group.address) == composite_before
        assert not await client.query_map(bt.storage.Multisig.Multisigs, [group.address])
        trace.add("all-members-refuse-automatic-wrapped-registration", preserved=before)

        # The fixed-MS member explicitly pays the recipient's permanent reserve.
        # Later approvals must not charge that reserve to another member/group.
        sponsor = members[2]
        await observe(
            client,
            sponsor,
            client.execute(
                bt.intents.Transfer(receiving_address(recipient), 1),
                sponsor,
                max_registration_deposit=bt.Balance.from_rao(deposit),
                wait_for_finalization=True,
            ),
            trace,
            "fixed-ms-member-explicitly-sponsors-recipient",
        )
        for signer in members:
            assert reserved(await snapshot(client, signer)) == reserved(
                before[signer.ss58_address]
            ) + (deposit if signer.ss58_address == sponsor.ss58_address else 0)
        assert await account(client, group.address) == composite_before

        # Two complementary quorums each contain both classical schemes. One
        # finishes with rotating ML-DSA, the other with fixed-key ML-DSA.
        for quorum in ((1, 0, 3), (0, 1, 2)):
            opener, approver, executor = quorum
            reserves = {key.ss58_address: reserved(await snapshot(client, key)) for key in members}
            before_group = await account(client, group.address)
            before_recipient = await snapshot(client, recipient)
            first = await observe(
                client,
                members[opener],
                client.execute(
                    intent(bt.intents.MultisigApprove, opener),
                    members[opener],
                    max_registration_deposit=bt.Balance.from_rao(0),
                    wait_for_finalization=True,
                ),
                trace,
                f"quorum-{executor}-first-approval",
            )
            storage_key = [group.address, first.data["multisig_call_hash"]]
            pending = await client.query(bt.storage.Multisig.Multisigs, storage_key)
            assert pending["approvals"] == [members[opener].ss58_address]
            assert reserved(await snapshot(client, members[opener])) == reserves[
                members[opener].ss58_address
            ] + int(pending["deposit"])
            second = await observe(
                client,
                members[approver],
                client.execute(
                    intent(bt.intents.MultisigApprove, approver, pending["when"]),
                    members[approver],
                    max_registration_deposit=bt.Balance.from_rao(0),
                    wait_for_finalization=True,
                ),
                trace,
                f"quorum-{executor}-second-approval",
            )
            assert second.data["multisig_call_data"] == first.data["multisig_call_data"]
            assert second.block_hash != first.block_hash
            pending = await client.query(bt.storage.Multisig.Multisigs, storage_key)
            assert set(pending["approvals"]) == {
                members[opener].ss58_address,
                members[approver].ss58_address,
            }
            assert await account(client, group.address) == before_group
            assert await snapshot(client, recipient) == before_recipient
            final = await observe(
                client,
                members[executor],
                client.execute(
                    intent(bt.intents.MultisigExecute, executor, pending["when"]),
                    members[executor],
                    max_registration_deposit=bt.Balance.from_rao(0),
                    wait_for_finalization=True,
                ),
                trace,
                f"quorum-{executor}-final-execution",
            )
            assert final.data["multisig_call_data"] == first.data["multisig_call_data"]
            assert final.block_hash != second.block_hash
            assert "Ok" in event(final, "Multisig", "MultisigExecuted")["result"]
            assert await client.query(bt.storage.Multisig.Multisigs, storage_key) is None
            after_group = await account(client, group.address)
            assert int(after_group["data"]["free"]) == int(before_group["data"]["free"]) - 10**9
            assert after_group["data"]["reserved"] == before_group["data"]["reserved"]
            assert after_group["nonce"] == before_group["nonce"]
            assert free(await snapshot(client, recipient)) == free(before_recipient) + 10**9
            for signer in members:
                assert reserved(await snapshot(client, signer)) == reserves[signer.ss58_address]

        # Each scheme opens and cancels a distinct operation. An approving
        # non-depositor cannot cancel or recover the opener's deposit.
        for index, opener in enumerate(members):
            call = await client.compose(bt.calls.System.remark(remark="0x" + bytes([index]).hex()))
            call_hash = "0x" + bytes(call.call_hash).hex()
            storage_key = [group.address, call_hash]
            baseline = reserved(await snapshot(client, opener))
            await observe(
                client,
                opener,
                group.approve(call, opener, wait_for_finalization=True),
                trace,
                f"open-cancellable-{opener.crypto_type}",
            )
            pending = await client.query(bt.storage.Multisig.Multisigs, storage_key)
            assert reserved(await snapshot(client, opener)) == baseline + int(pending["deposit"])

            def cancel_call(signer, when=pending["when"], approved_hash=call_hash):
                return bt.calls.Multisig.cancel_as_multi(
                    threshold=3,
                    other_signatories=sorted(
                        [
                            key.ss58_address
                            for key in members
                            if key.ss58_address != signer.ss58_address
                        ],
                        key=lambda address: bytes(ss58_decode(address)),
                    ),
                    timepoint=when,
                    call_hash=approved_hash,
                )

            if index == 0:
                other = members[1]
                await observe(
                    client,
                    other,
                    group.approve(call, other, wait_for_finalization=True),
                    trace,
                    "non-depositor-approves-cancellable-call",
                )
                pending = await client.query(bt.storage.Multisig.Multisigs, storage_key)
                opener_before = await snapshot(client, opener)
                denied = await observe(
                    client,
                    other,
                    client.submit_call(cancel_call(other), other, wait_for_finalization=True),
                    trace,
                    "non-depositor-cancellation-denied",
                    success=False,
                )
                assert denied.error.name == "NotOwner", denied.to_dict()
                assert await snapshot(client, opener) == opener_before
                assert await client.query(bt.storage.Multisig.Multisigs, storage_key) == pending
            await observe(
                client,
                opener,
                client.submit_call(cancel_call(opener), opener, wait_for_finalization=True),
                trace,
                f"cancel-and-release-deposit-{opener.crypto_type}",
            )
            assert await client.query(bt.storage.Multisig.Multisigs, storage_key) is None
            assert reserved(await snapshot(client, opener)) == baseline

        assert int((await account(client, group.address))["data"]["free"]) == 8 * 10**9
        assert free(await snapshot(client, recipient)) == 3 * 10**9
        assert (await snapshot(client, recipient))["authority"]["generation"] == 0
        # Only the member that signs an included outer transaction advances.
        # These totals also catch a peer's inner composite dispatch accidentally
        # consuming a member's authority or a second approval deposit.
        for index, (signer, included) in enumerate(zip(members, (5, 7, 4, 4))):
            initial = before_protection[signer.ss58_address]
            final_state = await snapshot(client, signer)
            assert (
                int(final_state["account"]["nonce"]) == int(initial["account"]["nonce"]) + included
            )
            assert reserved(final_state) == reserved(initial) + (
                derived_deposit if index == 3 else deposit if index == 2 else 0
            )
            if signer.crypto_type in MLDSA_CRYPTO_TYPES:
                assert (
                    final_state["authority"]["generation"]
                    == initial["authority"]["generation"] + included
                )
                assert final_state["authority"]["descriptor"] == initial["authority"]["descriptor"]
        assert (
            await client.query(("HashedAccounts", "DerivedAuthorities"), [group.address])
            == protection_record
        )
        assert (
            await client.multisig([key.ss58_address for key in members], 3)
        ).address == group.address
