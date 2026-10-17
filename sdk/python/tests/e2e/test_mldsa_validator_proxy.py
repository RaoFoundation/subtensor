"""Restricted MS proxy weights and revocation on fresh normal-slot subnets."""

from __future__ import annotations

import asyncio
import os
from types import SimpleNamespace

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from tests.e2e.test_mldsa_proxy_boundaries import ProxyRun, events_named
from tests.e2e.test_mldsa_release import key
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an activated Alice-root localnet"),
]


@pytest.mark.parametrize("code", [7, 8])
async def test_restricted_ms_proxy_sets_weights_and_revocation_invalidates_authority(code):
    cold, hot, delegate, miner, alternate = [key(code) for _ in range(5)]
    owner_wallet = SimpleNamespace(coldkey=cold, coldkeypub=cold.public_only(), hotkey=hot)
    async with (
        asyncio.timeout(1500),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        run = ProxyRun(
            client,
            f"ms-validator-proxy-{code}",
            code,
            owner=cold,
            validator=hot,
            delegate=delegate,
            miner=miner,
            alternate=alternate,
        )
        cap = bt.Balance.from_rao(
            int(await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit))
        )
        assert await client.query(("AdminUtils", "HashedAccountsEnabled")) is True
        assert await client.query(bt.storage.SubtensorModule.NetworkRateLimit) == 0

        async def finalized_state(signer):
            block = await client._substrate.raw.get_chain_finalised_head()
            return {
                "account": await client._substrate.query(
                    "System", "Account", [signer.ss58_address], block_hash=block
                ),
                "authority": await client._substrate.query(
                    "HashedAccounts", "MlDsaAccounts", [signer.ss58_address], block_hash=block
                ),
            }

        def advanced_once(before, after):
            assert after["account"]["nonce"] == before["account"]["nonce"] + 1
            assert after["authority"]["generation"] == before["authority"]["generation"] + 1
            assert after["authority"]["descriptor"] == before["authority"]["descriptor"]
            assert (after["authority"]["commitment"] == before["authority"]["commitment"]) == (
                code == 8
            )
            assert after["account"]["data"]["reserved"] == before["account"]["data"]["reserved"]

        for signer in (cold, hot, delegate, miner, alternate):
            async with alice_submission():
                result = await client.execute(
                    bt.intents.Transfer(
                        receiving_address(signer), 20_000 if signer is cold else 100
                    ),
                    dev_wallet(),
                    max_registration_deposit=cap,
                    wait_for_finalization=True,
                )
            await run.record("Alice funds and explicitly sponsors fresh descriptor", result)

        result = await client.execute(
            bt.intents.RegisterSubnet(hotkey_ss58=hot.ss58_address),
            owner_wallet,
            wait_for_finalization=True,
        )
        await run.record("MS owner creates subnet", result)
        netuid = int(result.data["netuid"])
        run.report["netuid"] = netuid
        settings = [
            bt.calls.AdminUtils.sudo_set_max_allowed_uids(netuid=netuid, max_allowed_uids=64),
            bt.calls.AdminUtils.sudo_set_max_registrations_per_block(
                netuid=netuid, max_registrations_per_block=10
            ),
            bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                netuid=netuid, registration_allowed=True
            ),
            bt.calls.AdminUtils.sudo_set_min_burn(netuid=netuid, min_burn=10**8),
            bt.calls.AdminUtils.sudo_set_max_burn(netuid=netuid, max_burn=2 * 10**8),
            bt.calls.AdminUtils.sudo_set_min_allowed_weights(netuid=netuid, min_allowed_weights=1),
            bt.calls.AdminUtils.sudo_set_weights_set_rate_limit(
                netuid=netuid, weights_set_rate_limit=0
            ),
            bt.calls.AdminUtils.sudo_set_commit_reveal_weights_enabled(
                netuid=netuid, enabled=False
            ),
        ]
        setup = bt.calls.Utility.batch_all(
            calls=[
                await client.compose(bt.calls.Sudo.sudo(call=await client.compose(call)))
                for call in settings
            ]
        )
        async with alice_submission():
            await run.submit("Alice configures only the fresh subnet", setup, dev_wallet())
        await run.submit(
            "MS owner registers two non-self miner destinations",
            bt.calls.Utility.batch_all(
                calls=[
                    await client.compose(
                        bt.calls.SubtensorModule.burned_register(
                            netuid=netuid, hotkey=signer.ss58_address
                        )
                    )
                    for signer in (miner, alternate)
                ]
            ),
            cold,
        )
        uids = [
            int(await client.query(bt.storage.SubtensorModule.Uids, [netuid, signer.ss58_address]))
            for signer in (hot, miner, alternate)
        ]
        validator_uid, miner_uid, alternate_uid = uids
        assert len(set(uids)) == 3
        run.report["uids"] = {
            "validator": validator_uid,
            "miner": miner_uid,
            "alternate": alternate_uid,
        }

        async def finalized_weights():
            block = await client._substrate.raw.get_chain_finalised_head()
            weights = await client._substrate.query(
                "SubtensorModule", "Weights", [netuid, validator_uid], block_hash=block
            )
            return weights

        await run.submit(
            "MS hotkey grants NonTransfer proxy",
            bt.calls.Proxy.add_proxy(
                delegate=delegate.ss58_address, proxy_type="NonTransfer", delay=0
            ),
            hot,
        )
        principal_before = await finalized_state(hot)
        delegate_before = await finalized_state(delegate)
        result = await client.execute(
            bt.intents.SetWeights(netuid=netuid, weights={miner_uid: 1.0}),
            delegate,
            proxy_for=hot.ss58_address,
            proxy_type="NonTransfer",
            wait_for_finalization=True,
        )
        _, events = await run.record("restricted MS proxy sets non-self weights", result)
        executed = events_named(events, "Proxy", "ProxyExecuted")
        assert len(executed) == 1 and "Ok" in executed[0]["result"]
        assert await finalized_weights() == [(miner_uid, 65535)]
        assert await finalized_state(hot) == principal_before
        delegate_after = await finalized_state(delegate)
        advanced_once(delegate_before, delegate_after)
        paid = events_named(events, "TransactionPayment", "TransactionFeePaid")
        assert len(paid) == 1 and paid[0]["who"] == delegate.ss58_address
        run.report["permitted_weights_fee_payer"] = paid[0]["who"]

        # NonTransfer must keep its filter after the MS wrapper authenticates.
        miner_before = await finalized_state(miner)
        delegate_before = delegate_after
        _, events = await run.submit(
            "restricted MS proxy cannot transfer validator funds",
            bt.calls.Proxy.proxy(
                real=hot.ss58_address,
                force_proxy_type="NonTransfer",
                call=await client.compose(
                    bt.calls.Balances.transfer_keep_alive(dest=miner.ss58_address, value=10**9)
                ),
            ),
            delegate,
            success=False,
            error="CallFiltered",
        )
        assert events_named(events, "System", "ExtrinsicSuccess")
        assert "Err" in events_named(events, "Proxy", "ProxyExecuted")[0]["result"]
        assert await finalized_state(hot) == principal_before
        assert await finalized_state(miner) == miner_before
        assert await finalized_weights() == [(miner_uid, 65535)]
        advanced_once(delegate_before, await finalized_state(delegate))

        await run.submit(
            "MS hotkey revokes validator proxy",
            bt.calls.Proxy.remove_proxy(
                delegate=delegate.ss58_address, proxy_type="NonTransfer", delay=0
            ),
            hot,
        )
        principal_after_revoke = await finalized_state(hot)
        assert (
            principal_after_revoke["account"]["nonce"] == principal_before["account"]["nonce"] + 1
        )
        assert (
            principal_after_revoke["authority"]["generation"]
            == principal_before["authority"]["generation"] + 1
        )
        assert not (await client.query(("Proxy", "Proxies"), [hot.ss58_address]))[0]
        delegate_before = await finalized_state(delegate)
        _, events = await run.submit(
            "revoked proxy cannot redirect weights to another non-self miner",
            bt.calls.Proxy.proxy(
                real=hot.ss58_address,
                force_proxy_type="NonTransfer",
                call=await client.compose(
                    bt.calls.SubtensorModule.set_mechanism_weights(
                        netuid=netuid,
                        mecid=0,
                        dests=[alternate_uid],
                        weights=[65535],
                        version_key=0,
                    )
                ),
            ),
            delegate,
            success=False,
            error="NotProxy",
        )
        assert events_named(events, "System", "ExtrinsicFailed")
        assert await finalized_weights() == [(miner_uid, 65535)]
        assert await finalized_state(hot) == principal_after_revoke
        advanced_once(delegate_before, await finalized_state(delegate))
        run.report.update(
            successful=True,
            finalized_weights=await finalized_weights(),
            principal_after_revoke=principal_after_revoke,
            delegate_final=await finalized_state(delegate),
        )
        run.save()
