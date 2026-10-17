"""Protected multisig and proxy composition on an activated disposable chain."""

from __future__ import annotations

import os

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from tests.e2e.test_mldsa_extended import Evidence, account, fresh_key, snapshot
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an activated Alice-root localnet"),
]


async def test_protected_multisig_as_proxy_and_proxy_for_multisig():
    members = [fresh_key(code) for code in (7, 8)]
    principal, recipient = fresh_key(8), fresh_key(7)
    trace = Evidence("mldsa-nested-multisig-proxy")
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
            await accepted(
                client.execute(
                    bt.intents.Transfer(receiving_address(signer), 100),
                    dev_wallet("//Eve"),
                    max_registration_deposit=cap,
                    wait_for_finalization=True,
                ),
                "fund-and-register",
            )
        group = await client.multisig([receiving_address(key) for key in members], 2)

        async def quorum(call, label, *, success=True):
            before = [await snapshot(client, key) for key in members]
            for index, key in enumerate(members):
                result = await group.approve(call, key, wait_for_finalization=True)
                trace.add(label, member=key.ss58_address, receipt=result.to_dict())
                assert result.success == (True if index == 0 else success), result.to_dict()
            after = [await snapshot(client, key) for key in members]
            for old, new in zip(before, after):
                assert new["account"]["nonce"] == old["account"]["nonce"] + 1
                assert new["authority"]["generation"] == old["authority"]["generation"] + 1
            assert (
                after[0]["account"]["data"]["reserved"] == before[0]["account"]["data"]["reserved"]
            )
            return result

        await quorum(
            await group.protection_call(deposit_payer=members[1].ss58_address), "protect-group"
        )
        assert (await group.protection_status())["registered"]
        protected = await client.query(("HashedAccounts", "DerivedAuthorities"), [group.address])
        await accepted(
            client.execute(
                bt.intents.Transfer(group.address, 20),
                dev_wallet("//Eve"),
                wait_for_finalization=True,
            ),
            "fund-group",
        )

        # The PQ multisig is itself the proxy delegate of a fixed MS principal.
        await accepted(
            client.submit_call(
                bt.calls.Proxy.add_proxy(delegate=group.address, proxy_type="Any", delay=0),
                principal,
                wait_for_finalization=True,
            ),
            "grant-protected-group-proxy",
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

        # Conversely, an MS delegate can spend from the protected multisig.
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
            "ms-proxies-for-protected-group",
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
            protected
        )
