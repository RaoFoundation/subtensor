"""Ordinary and protected multisig/proxy composition on a disposable chain."""

from __future__ import annotations

import os

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from bittensor.sp_core import requires_registration
from tests.e2e.test_mldsa_extended import Evidence, account, fresh_key, snapshot
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an activated Alice-root localnet"),
]


@pytest.mark.parametrize("protected", [True, False], ids=["rotating-protected", "fixed-ordinary"])
async def test_multisig_as_proxy_and_proxy_for_multisig(protected):
    members = [fresh_key(code) for code in ((7, 8) if protected else (8, 1))]
    principal, recipient = [fresh_key(7 if protected else 8) for _ in range(2)]
    trace = Evidence(f"mldsa-nested-multisig-proxy-{protected}")
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        cap = bt.Balance.from_rao(
            int(await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit))
        )

        async def accepted(operation, label):
            result = await operation
            trace.add(label, receipt=result.to_dict())
            assert result.success and result.block_hash, result.to_dict()
            return result

        for signer in (*members, principal, recipient):
            register = requires_registration(signer)
            async with alice_submission():
                await accepted(
                    client.execute(
                        bt.intents.Transfer(
                            receiving_address(signer) if register else signer.ss58_address, 100
                        ),
                        dev_wallet(),
                        max_registration_deposit=cap if register else None,
                        wait_for_finalization=True,
                    ),
                    "fund-and-register" if register else "fund-ordinary-ss58",
                )
            if not register:
                assert (await snapshot(client, signer))["authority"] is None
        group = await client.multisig([receiving_address(key) for key in members], 2)

        async def quorum(call, label, *, success=True):
            before = [await snapshot(client, key) for key in members]
            signed_counts = []
            for index, key in enumerate(members):
                result = await group.approve(call, key, wait_for_finalization=True)
                signed_counts.append(2 if result.data.get("shielded") else 1)
                trace.add(label, member=key.ss58_address, receipt=result.to_dict())
                assert result.success == (True if index == 0 else success), result.to_dict()
            after = [await snapshot(client, key) for key in members]
            for old, new, count in zip(before, after, signed_counts):
                assert new["account"]["nonce"] == old["account"]["nonce"] + count
                if old["authority"] is None:
                    assert new["authority"] is None
                else:
                    assert new["authority"]["generation"] == old["authority"]["generation"] + count
            assert (
                after[0]["account"]["data"]["reserved"] == before[0]["account"]["data"]["reserved"]
            )
            return result

        if protected:
            await quorum(
                await group.protection_call(deposit_payer=members[1].ss58_address), "protect-group"
            )
            assert (await group.protection_status())["registered"]
        else:
            status = await group.protection_status()
            assert not status["eligible"] and not status["registered"]
        authority = await client.query(("HashedAccounts", "DerivedAuthorities"), [group.address])
        assert bool(authority) is protected
        async with alice_submission():
            await accepted(
                client.execute(
                    bt.intents.Transfer(group.address, 20),
                    dev_wallet(),
                    wait_for_finalization=True,
                ),
                "fund-group",
            )

        # Both an opted-in PQ group and an ordinary mixed group can be delegates.
        await accepted(
            client.submit_call(
                bt.calls.Proxy.add_proxy(delegate=group.address, proxy_type="Any", delay=0),
                principal,
                wait_for_finalization=True,
            ),
            "grant-group-proxy",
        )
        transfer = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**9)
        )
        proxy = bt.calls.Proxy.proxy(
            real=principal.ss58_address, force_proxy_type="Any", call=transfer
        )
        before_principal = await snapshot(client, principal)
        before_recipient = await snapshot(client, recipient)
        await quorum(proxy, "multisig-executes-proxy")
        after_principal = await snapshot(client, principal)
        assert after_principal["authority"] == before_principal["authority"]
        assert after_principal["account"]["nonce"] == before_principal["account"]["nonce"]
        assert after_principal["account"]["data"]["free"] == (
            before_principal["account"]["data"]["free"] - 10**9
        )
        assert (await account(client, recipient.ss58_address))["data"]["free"] == (
            before_recipient["account"]["data"]["free"] + 10**9
        )

        # Conversely, an MS delegate can spend under the group's authority policy.
        await quorum(
            bt.calls.Proxy.add_proxy(delegate=principal.ss58_address, proxy_type="Any", delay=0),
            "group-grants-ms-delegate",
        )
        before_group = await account(client, group.address)
        await accepted(
            client.execute(
                bt.intents.Transfer(receiving_address(recipient), 1),
                principal,
                proxy_for=group.address,
                proxy_type="Any",
                wait_for_finalization=True,
            ),
            "ms-proxies-for-group",
        )
        after_group = await account(client, group.address)
        assert after_group["data"]["free"] == before_group["data"]["free"] - 10**9
        assert after_group["data"]["reserved"] == before_group["data"]["reserved"]
        assert after_group["nonce"] == before_group["nonce"]

        await accepted(
            client.submit_call(
                bt.calls.Proxy.remove_proxy(delegate=group.address, proxy_type="Any", delay=0),
                principal,
                wait_for_finalization=True,
            ),
            "revoke-group-delegation",
        )
        before_principal = await snapshot(client, principal)
        before_recipient = await snapshot(client, recipient)
        rejected = await quorum(proxy, "old-quorum-call-rejected-after-revocation", success=False)
        assert rejected.error.name == "NotProxy", rejected.to_dict()
        assert await snapshot(client, principal) == before_principal
        assert await snapshot(client, recipient) == before_recipient
        assert await client.query(("HashedAccounts", "DerivedAuthorities"), [group.address]) == (
            authority
        )
