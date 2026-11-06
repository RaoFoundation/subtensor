"""Live MS proxy restrictions and announcement lifecycle at normal block timing.

Requires an isolated activated chain with development authorization weights.
Only disposable accounts are changed; no global setting or storage is injected.
Parallel workers must share E2E_WALLET_ALICE_LOCK for Alice sponsorship.
"""

from __future__ import annotations

import asyncio
import os
from hashlib import blake2b

import pytest

import bittensor as bt
from bittensor.hashed import registration_descriptor_value
from bittensor.receiving import receiving_address
from bittensor.result import chain_error_from_dispatch
from bittensor.sp_core import requires_registration, ss58_encode
from tests.e2e.test_mldsa_release import evidence, key
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


def events_named(events, module, name):
    return [
        entry["event"]["attributes"]
        for entry in events
        if (entry["event"]["module_id"], entry["event"]["event_id"]) == (module, name)
    ]


class ProxyRun:
    def __init__(self, client, name, code, **accounts):
        self.client = client
        self.name = name
        self.report = {
            "crypto_type": code,
            "accounts": {name: signer.ss58_address for name, signer in accounts.items()},
            "normal_block_timing": True,
            "successful": False,
            "receipts": [],
        }

    def save(self):
        evidence(self.name, self.report)

    async def record(self, label, result, *, success=True, error=None):
        assert result.block_hash and result.extrinsic_id, result.to_dict()
        index = int(result.extrinsic_id.split("-")[1])
        # Compact failed SDK receipts omit events. Read canonical events even on
        # failures so outer success cannot be mistaken for inner success.
        events = [
            entry
            for entry in await self.client._substrate.raw.get_events(result.block_hash)
            if entry.get("extrinsic_idx") == index
        ]
        self.report["receipts"].append(
            {"operation": label, **result.to_dict(), "canonical_events": events}
        )
        self.save()
        assert result.success is success, result.to_dict()
        if error is not None:
            assert result.error is not None and result.error.name == error, result.to_dict()
        return result, events

    async def submit(self, label, call, signer, *, success=True, error=None):
        result = await self.client.submit_call(call, signer, wait_for_finalization=True)
        return await self.record(label, result, success=success, error=error)

    async def fund(self, signer):
        register = requires_registration(signer)
        async with alice_submission():
            result = await self.client.execute(
                bt.intents.Transfer(
                    receiving_address(signer) if register else signer.ss58_address, 100
                ),
                dev_wallet(),
                wait_for_finalization=True,
                max_registration_deposit=bt.Balance.from_tao(2) if register else None,
            )
        await self.record(
            "Alice sponsors rotating account" if register else "Alice funds ordinary SS58 account",
            result,
        )
        if not register:
            assert await self.query("HashedAccounts", "MlDsaAccounts", signer) is None

    async def query(self, pallet, storage, signer):
        return await self.client.query((pallet, storage), [signer.ss58_address])

    async def setup(self, owner, delegate, recipient, *, proxy_type, delay):
        for signer in (owner, delegate):
            await self.fund(signer)
        grant = bt.calls.Proxy.add_proxy(
            delegate=delegate.ss58_address, proxy_type=proxy_type, delay=delay
        )
        register_recipient = requires_registration(recipient)
        calls = []
        if register_recipient:
            calls.append(
                await self.client.compose(
                    bt.calls.HashedAccounts.register(
                        descriptor=registration_descriptor_value(bytes(recipient.hashed_descriptor))
                    )
                )
            )
        calls.append(await self.client.compose(grant))
        await self.submit(
            "owner registers recipient and grants proxy"
            if register_recipient
            else "owner grants ordinary proxy",
            bt.calls.Utility.batch_all(calls=calls) if register_recipient else grant,
            owner,
        )
        assert (
            bool(await self.query("HashedAccounts", "MlDsaAccounts", recipient))
            is register_recipient
        )

    async def finalized_height(self, height):
        async with asyncio.timeout(120):
            while await self.client._substrate.raw.get_finalized_block_number() < height:
                await asyncio.sleep(2)


@pytest.mark.parametrize("code", [7, 8])
async def test_restricted_ms_proxy_cannot_escalate_through_evm_dispatch(code):
    owner, delegate, recipient = [key(code) for _ in range(3)]
    async with (
        asyncio.timeout(900),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        run = ProxyRun(
            client,
            f"ms-proxy-evm-boundary-{code}",
            code,
            owner=owner,
            delegate=delegate,
            recipient=recipient,
        )
        await run.setup(owner, delegate, recipient, proxy_type="NonCritical", delay=0)
        source = "0x" + bytes(owner.public_key)[:20].hex()
        evm_account = owner.ss58_address
        if not requires_registration(owner):
            evm_account = ss58_encode(
                blake2b(b"evm:" + bytes(owner.public_key)[:20], digest_size=32).digest(), 42
            )
            await run.submit(
                "fixed MS owner funds its separate EVM backing account",
                bt.calls.Balances.transfer_keep_alive(dest=evm_account, value=10 * 10**9),
                owner,
            )
        raw = client._substrate.raw
        gas_price = int(await raw._session.request("eth_gasPrice", []), 16)
        escalation = await client.compose(
            bt.calls.Proxy.add_proxy(delegate=recipient.ss58_address, proxy_type="Any", delay=0)
        )
        evm = await client.compose(
            bt.calls.EVM.call(
                source=source,
                target="0x" + "00" * 19 + "06",
                input="0x" + bytes(escalation.data).hex(),
                value=[0, 0, 0, 0],
                gas_limit=5_000_000,
                max_fee_per_gas=[(gas_price >> (64 * i)) & (2**64 - 1) for i in range(4)],
                max_priority_fee_per_gas=None,
                nonce=None,
                access_list=[],
                authorization_list=[],
            )
        )
        grants = await run.query("Proxy", "Proxies", owner)
        owner_state = await run.query("System", "Account", owner)
        authority = await run.query("HashedAccounts", "MlDsaAccounts", owner)
        evm_nonce = await raw._session.request("eth_getTransactionCount", [source, "latest"])
        for label, call in (
            ("direct EVM escalation", evm),
            (
                "nested EVM escalation",
                await client.compose(bt.calls.Utility.batch_all(calls=[evm])),
            ),
        ):
            _, events = await run.submit(
                label,
                bt.calls.Proxy.proxy(
                    real=owner.ss58_address, force_proxy_type="NonCritical", call=call
                ),
                delegate,
                success=False,
                error="CallFiltered",
            )
            assert events_named(events, "System", "ExtrinsicSuccess")
            inner = events_named(events, "Proxy", "ProxyExecuted")
            assert len(inner) == 1 and "Err" in inner[0]["result"], inner
            assert chain_error_from_dispatch(inner[0]["result"]["Err"]).name == "CallFiltered"
            assert await run.query("Proxy", "Proxies", owner) == grants
            assert await run.query("System", "Account", owner) == owner_state
            assert await run.query("HashedAccounts", "MlDsaAccounts", owner) == authority
            assert (
                await raw._session.request("eth_getTransactionCount", [source, "latest"])
                == evm_nonce
            )

        # The restricted grant still executes an allowed value transfer.
        await run.submit(
            "NonCritical transfer succeeds",
            bt.calls.Proxy.proxy(
                real=owner.ss58_address,
                force_proxy_type="NonCritical",
                call=await client.compose(
                    bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**9)
                ),
            ),
            delegate,
        )
        assert (await run.query("System", "Account", recipient))["data"]["free"] == 10**9
        await run.submit(
            "owner explicitly grants Any",
            bt.calls.Proxy.add_proxy(delegate=delegate.ss58_address, proxy_type="Any", delay=0),
            owner,
        )
        _, events = await run.submit(
            "same EVM dispatch payload succeeds with Any",
            bt.calls.Proxy.proxy(real=owner.ss58_address, force_proxy_type="Any", call=evm),
            delegate,
        )
        assert events_named(events, "EVM", "Executed")
        assert any(
            grant["delegate"] == recipient.ss58_address and grant["proxy_type"] == "Any"
            for grant in (await client.query(("Proxy", "Proxies"), [evm_account]))[0]
        )
        if not requires_registration(owner):
            assert not any(
                grant["delegate"] == recipient.ss58_address
                for grant in (await run.query("Proxy", "Proxies", owner))[0]
            )
        run.report["successful"] = True
        run.save()


@pytest.mark.parametrize("code", [7, 8])
async def test_ms_delayed_proxy_regrant_preserves_the_accounts_policy(code):
    owner, delegate, recipient = [key(code) for _ in range(3)]
    delay = 2
    async with (
        asyncio.timeout(900),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        run = ProxyRun(
            client,
            f"ms-proxy-announcement-boundary-{code}",
            code,
            owner=owner,
            delegate=delegate,
            recipient=recipient,
        )
        await run.setup(owner, delegate, recipient, proxy_type="Any", delay=delay)
        plan = await client.plan(bt.intents.Transfer(receiving_address(recipient), 1), owner)
        assert plan.ok
        assert plan.extras.get("hashed_registration_guards", []) == (
            [recipient.ss58_address] if requires_registration(recipient) else []
        )
        call = plan.call
        call_hash = "0x" + bytes(call.call_hash).hex()
        announce = bt.calls.Proxy.announce(real=owner.ss58_address, call_hash=call_hash)
        execute = bt.calls.Proxy.proxy_announced(
            delegate=delegate.ss58_address,
            real=owner.ss58_address,
            force_proxy_type="Any",
            call=call,
        )
        grants = await run.query("Proxy", "Proxies", owner)
        reserve = (await run.query("System", "Account", delegate))["data"]["reserved"]
        # Both calls share a block, so the negative result cannot race finality.
        early, events = await run.submit(
            "announce then execute too early in the same block",
            bt.calls.Utility.batch(
                calls=[await client.compose(announce), await client.compose(execute)]
            ),
            delegate,
            success=False,
            error="Unannounced",
        )
        assert events_named(events, "System", "ExtrinsicSuccess")
        interrupted = events_named(events, "Utility", "BatchInterrupted")
        assert len(interrupted) == 1 and interrupted[0]["index"] == 1, interrupted
        assert not events_named(events, "Proxy", "ProxyExecuted")
        pending = (await run.query("Proxy", "Announcements", delegate))[0]
        assert len(pending) == 1 and pending[0]["real"] == owner.ss58_address, pending
        assert pending[0]["height"] == int(early.extrinsic_id.split("-")[0])
        assert (await run.query("System", "Account", recipient))["data"]["free"] == 0
        await run.finalized_height(pending[0]["height"] + delay)
        await run.submit("announced guarded transfer after delay", execute, delegate)
        assert not (await run.query("Proxy", "Announcements", delegate))[0]
        assert (await run.query("System", "Account", delegate))["data"]["reserved"] == reserve
        assert (await run.query("System", "Account", recipient))["data"]["free"] == 10**9

        await run.submit("announce again before revocation", announce, delegate)
        assert (await run.query("Proxy", "Announcements", delegate))[0]
        await run.submit(
            "owner revokes and recreates the identical delayed grant",
            bt.calls.Utility.batch_all(
                calls=[
                    await client.compose(
                        bt.calls.Proxy.remove_proxy(
                            delegate=delegate.ss58_address, proxy_type="Any", delay=delay
                        )
                    ),
                    await client.compose(
                        bt.calls.Proxy.add_proxy(
                            delegate=delegate.ss58_address, proxy_type="Any", delay=delay
                        )
                    ),
                ]
            ),
            owner,
        )
        assert await run.query("Proxy", "Proxies", owner) == grants
        if requires_registration(owner):
            assert not (await run.query("Proxy", "Announcements", delegate))[0]
            assert (await run.query("System", "Account", delegate))["data"]["reserved"] == reserve
            await run.submit(
                "protected regrant invalidates the old announcement",
                execute,
                delegate,
                success=False,
                error="Unannounced",
            )
        else:
            pending = (await run.query("Proxy", "Announcements", delegate))[0]
            assert len(pending) == 1
            await run.finalized_height(pending[0]["height"] + delay)
            await run.submit("ordinary regrant preserves the valid announcement", execute, delegate)
        completed = 1 if requires_registration(owner) else 2
        assert (await run.query("System", "Account", recipient))["data"][
            "free"
        ] == completed * 10**9
        assert not (await run.query("Proxy", "Announcements", delegate))[0]
        fresh, _ = await run.submit("fresh announcement after regrant", announce, delegate)
        await run.finalized_height(int(fresh.extrinsic_id.split("-")[0]) + delay)
        await run.submit("fresh guarded transfer after regrant and delay", execute, delegate)
        assert (await run.query("System", "Account", recipient))["data"]["free"] == (
            completed + 1
        ) * 10**9
        assert not (await run.query("Proxy", "Announcements", delegate))[0]
        assert (await run.query("System", "Account", delegate))["data"]["reserved"] == reserve
        run.report["successful"] = True
        run.save()


@pytest.mark.parametrize("code", [7, 8])
async def test_ms_classical_delegation_matches_the_account_policy(code):
    owner, delegate, recipient = key(code), key(1), key(1)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        run = ProxyRun(
            client, f"ms-classical-delegation-{code}", code, owner=owner, delegate=delegate
        )
        await run.fund(owner)
        await run.fund(delegate)
        await run.submit(
            "grant classical delegate under the account policy",
            bt.calls.Proxy.add_proxy(delegate=delegate.ss58_address, proxy_type="Any", delay=0),
            owner,
            success=not requires_registration(owner),
            error=None if not requires_registration(owner) else "AccountPolicyViolation",
        )
        if not requires_registration(owner):
            await run.submit(
                "classical delegate spends from an ordinary fixed MS account",
                bt.calls.Proxy.proxy(
                    real=owner.ss58_address,
                    force_proxy_type="Any",
                    call=await client.compose(
                        bt.calls.Balances.transfer_keep_alive(
                            dest=recipient.ss58_address, value=10**9
                        )
                    ),
                ),
                delegate,
            )
            assert (await run.query("System", "Account", recipient))["data"]["free"] == 10**9
            assert await run.query("HashedAccounts", "MlDsaAccounts", owner) is None
        else:
            assert not (await run.query("Proxy", "Proxies", owner))[0]
        run.report["successful"] = True
        run.save()
