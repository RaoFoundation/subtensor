"""Bidirectional classical/new-wallet swaps on an isolated Alice-root localnet.

Run with E2E_WALLET_ENDPOINT and E2E_EVIDENCE_DIR. Setup only changes disposable
chain configuration and funding; swaps and post-swap operations use real signed
extrinsics, production runtime code and production weights. No Shield claim is
made: transactions explicitly use the unencrypted submission path.
"""

from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path

import pytest

import bittensor as bt
from bittensor.hashed import registration_descriptor_value
from bittensor.sp_core import Keypair, requires_registration
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
NEW = [4, 6, 7, 8, 9, 10, 11, 12]
PAIRS = [(old, new) for old in [0, 1] for new in NEW]
PAIRS += [(new, old) for old in [0, 1] for new in NEW]
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires a disposable Alice-root chain"),
]


def key(code):
    return Keypair.create_from_seed(os.urandom(32), code)


SPONSOR = key(1)
LAST_BLOCK = 0


class Run:
    def __init__(self, client, name):
        self.client = client
        self.name = name
        self.receipts = []

    async def submit(self, call, signer, *, success=True, error=None, included=True):
        global LAST_BLOCK
        public = getattr(signer, "coldkey", signer)
        result = await asyncio.wait_for(
            self.client.submit_call(
                call,
                signer,
                shielded=False,
                wait_for_finalization=requires_registration(public),
            ),
            timeout=180,
        )
        if result.extrinsic_id:
            LAST_BLOCK = max(LAST_BLOCK, int(result.extrinsic_id.split("-")[0]))
        self.receipts.append({**result.to_dict(), "events": result.events})
        if directory := os.getenv("E2E_EVIDENCE_DIR"):
            path = Path(directory)
            path.mkdir(parents=True, exist_ok=True)
            (path / f"{self.name}.json").write_text(
                json.dumps(self.receipts, indent=2, default=str)
            )
        assert result.success == success, result.to_dict()
        if included:
            assert result.block_hash, result.to_dict()
        if error:
            assert error in str(result.to_dict()), result.to_dict()
        return result

    async def root(self, calls):
        calls = [bt.calls.Sudo.sudo(call=await self.client.compose(call)) for call in calls]
        async with alice_submission():
            return await self.submit(
                bt.calls.Utility.batch_all(
                    calls=[await self.client.compose(call) for call in calls]
                ),
                dev_wallet(),
            )

    async def fund(self, *keys):
        calls = []
        for public in keys:
            transfer = bt.calls.Balances.transfer_keep_alive(dest=public.ss58_address, value=10**13)
            if requires_registration(public):
                registration = bt.calls.HashedAccounts.register(
                    descriptor=registration_descriptor_value(bytes(public.hashed_descriptor))
                )
                transfer = bt.calls.Utility.batch_all(
                    calls=[
                        await self.client.compose(registration),
                        await self.client.compose(transfer),
                    ]
                )
            calls.append(transfer)
        await asyncio.gather(*(self.submit(call, SPONSOR) for call in calls))
        # Rotating-key signing deliberately uses finalized authority. Inclusion
        # alone is not sufficient readiness for a newly registered recipient.
        for _ in range(120):
            if await self.client._substrate.finalized_block_number() >= LAST_BLOCK:
                break
            await asyncio.sleep(1)
        else:
            pytest.fail("wallet registration/funding did not finalize")

    async def query(self, name, *params):
        return await self.client.query(("SubtensorModule", name), list(params))

    async def stake(self, cold, hot, netuid=0):
        return (
            await self.client.read(
                "stake", coldkey_ss58=cold.ss58_address, hotkey_ss58=hot.ss58_address, netuid=netuid
            )
        ).rao

    async def balance(self, public):
        return await self.client.query(bt.storage.System.Account, [public.ss58_address])

    async def register_and_stake(self, cold, hot):
        await self.submit(bt.calls.SubtensorModule.root_register(hotkey=hot.ss58_address), cold)
        call = await bt.intents.AddStake(hot.ss58_address, 0, 5).build(self.client._substrate, cold)
        await self.submit(call, cold)
        assert await self.stake(cold, hot) > 0

    async def prove_spending(self, public):
        recipient = key(1)
        await self.submit(
            bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**6), public
        )
        assert (await self.balance(recipient))["data"]["free"] == 10**6


@pytest.fixture(scope="session", autouse=True)
def configure_swap_chain():
    if not ENDPOINT:
        yield
        return

    async def configure():
        async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
            assert await client.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
            run = Run(client, "swap-configuration")
            await run.root(
                [
                    bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
                    bt.calls.Balances.force_set_balance(who=SPONSOR.ss58_address, new_free=10**16),
                    bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=True),
                    bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0),
                    bt.calls.AdminUtils.sudo_set_coldkey_swap_announcement_delay(duration=2),
                    bt.calls.AdminUtils.sudo_set_coldkey_swap_reannouncement_delay(duration=0),
                    bt.calls.AdminUtils.sudo_set_nominator_min_required_stake(min_stake=1),
                    bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                        netuid=0, registration_allowed=True
                    ),
                    bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=0, subtoken_enabled=True),
                    bt.calls.AdminUtils.sudo_set_max_allowed_uids(netuid=0, max_allowed_uids=256),
                    bt.calls.AdminUtils.sudo_set_max_registrations_per_block(
                        netuid=0, max_registrations_per_block=1000
                    ),
                    bt.calls.AdminUtils.sudo_set_target_registrations_per_interval(
                        netuid=0, target_registrations_per_interval=1000
                    ),
                    bt.calls.AdminUtils.sudo_set_min_burn(netuid=0, min_burn=10**8),
                    bt.calls.AdminUtils.sudo_set_max_burn(netuid=0, max_burn=2 * 10**8),
                    bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                        netuid=1, registration_allowed=True
                    ),
                    bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=1, subtoken_enabled=True),
                    bt.calls.AdminUtils.sudo_set_tempo(netuid=1, tempo=60000),
                    bt.calls.AdminUtils.sudo_set_max_registrations_per_block(
                        netuid=1, max_registrations_per_block=1000
                    ),
                    bt.calls.AdminUtils.sudo_set_target_registrations_per_interval(
                        netuid=1, target_registrations_per_interval=1000
                    ),
                    bt.calls.AdminUtils.sudo_set_min_burn(netuid=1, min_burn=10**8),
                    bt.calls.AdminUtils.sudo_set_max_burn(netuid=1, max_burn=2 * 10**8),
                ]
            )

    asyncio.run(configure())
    yield

    async def finalize():
        async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
            for _ in range(120):
                height = await client._substrate.finalized_block_number()
                if height >= LAST_BLOCK:
                    return
                await asyncio.sleep(1)
            pytest.fail(f"swap evidence did not finalize through block {LAST_BLOCK}")

    asyncio.run(finalize())


@pytest.mark.parametrize("source,destination", PAIRS)
async def test_coldkey_swap_direction(source, destination):
    old, new, hot = key(source), key(destination), key(1)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        run = Run(client, f"cold-{source}-{destination}")
        await run.fund(old, new, hot)
        await run.register_and_stake(old, hot)
        await run.submit(
            bt.calls.SubtensorModule.burned_register(netuid=1, hotkey=hot.ss58_address), old
        )
        await run.submit(
            bt.calls.SubtensorModule.add_stake(
                hotkey=hot.ss58_address, netuid=1, amount_staked=5 * 10**9
            ),
            old,
        )
        held = await run.stake(old, hot)
        alpha = await run.stake(old, hot, 1)
        assert alpha > 0
        old_before, new_before = await run.balance(old), await run.balance(new)
        # Registered rotating coldkeys cannot weaken their authority floor.
        allowed = not requires_registration(old) or destination not in (0, 1)
        announced = await run.submit(
            bt.calls.SubtensorModule.announce_coldkey_swap_v2(new_coldkey=new.ss58_address),
            old,
            success=allowed,
            error=None if allowed else "HotkeyOwnerPolicyViolation",
        )
        if not allowed:
            assert await run.query("ColdkeySwapAnnouncements", old.ss58_address) is None
            assert await run.query("Owner", hot.ss58_address) == old.ss58_address
            assert await run.stake(old, hot) == held
            assert await run.stake(new, hot) == 0
            assert await run.stake(old, hot, 1) == alpha
            assert await run.stake(new, hot, 1) == 0
            assert (await run.balance(new)) == new_before
            await run.prove_spending(old)
            return
        # Wait for the actual chain delay, without bypassing the locked path.
        header = await client._substrate.raw._session.request(
            "chain_getHeader", [announced.block_hash]
        )
        height = int(header["number"], 16)
        for _ in range(120):
            header = await client._substrate.raw._session.request("chain_getHeader", [])
            if int(header["number"], 16) > height + 2:
                break
            await asyncio.sleep(1)
        else:
            pytest.fail("chain stopped during coldkey swap delay")
        executed = await run.submit(
            bt.calls.SubtensorModule.swap_coldkey_announced(new_coldkey=new.ss58_address), old
        )
        assert await run.query("Owner", hot.ss58_address) == new.ss58_address
        assert await run.stake(old, hot) == 0
        assert await run.stake(new, hot) == held
        assert await run.stake(old, hot, 1) == 0
        assert await run.stake(new, hot, 1) == alpha
        assert await run.query("ColdkeySwapAnnouncements", old.ss58_address) is None
        debited = 0
        for result in [announced, executed]:
            for event in result.events:
                if event["module_id"] != "Balances":
                    continue
                attributes = event["attributes"]
                if attributes.get("who") != old.ss58_address:
                    continue
                if event["event_id"] == "Withdraw":
                    debited += attributes["amount"]
                elif event["event_id"] == "Deposit":
                    debited -= attributes["amount"]
        assert (await run.balance(old))["data"]["free"] == 0
        assert (await run.balance(new))["data"]["free"] == (
            new_before["data"]["free"] + old_before["data"]["free"] - debited
        )
        await run.prove_spending(new)
        for netuid, amount in [(0, held), (1, alpha)]:
            await run.submit(
                bt.calls.SubtensorModule.remove_stake(
                    hotkey=hot.ss58_address, netuid=netuid, amount_unstaked=amount // 2
                ),
                new,
            )
            assert 0 < await run.stake(new, hot, netuid) < amount
        # Give the retired coldkey fees so rejection proves lost ownership,
        # rather than merely failing transaction payment on its drained account.
        await run.fund(old)
        await run.submit(
            bt.calls.SubtensorModule.swap_hotkey(
                hotkey=hot.ss58_address, new_hotkey=key(1).ss58_address, netuid=0
            ),
            old,
            success=False,
            error="NonAssociatedColdKey",
        )


@pytest.mark.parametrize("source,destination", PAIRS)
@pytest.mark.parametrize("variant", ["legacy-all", "v2-single", "v2-keep"])
async def test_hotkey_swap_direction(source, destination, variant):
    cold, old, new = key(7), key(source), key(destination)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        run = Run(client, f"hot-{source}-{destination}-{variant}")
        await run.fund(cold, old, new)
        await run.register_and_stake(cold, old)
        held = await run.stake(cold, old)
        uid = await run.query("Uids", 0, old.ss58_address)
        keep = variant == "v2-keep"
        call = (
            bt.calls.SubtensorModule.swap_hotkey(
                hotkey=old.ss58_address, new_hotkey=new.ss58_address, netuid=None
            )
            if variant == "legacy-all"
            else bt.calls.SubtensorModule.swap_hotkey_v2(
                hotkey=old.ss58_address, new_hotkey=new.ss58_address, netuid=0, keep_stake=keep
            )
        )
        await run.submit(call, cold)
        assert await run.query("Uids", 0, new.ss58_address) == uid
        assert await run.query("Uids", 0, old.ss58_address) is None
        assert await run.query("Owner", new.ss58_address) == cold.ss58_address
        assert await run.stake(cold, old) == (held if keep else 0)
        assert await run.stake(cold, new) == (0 if keep else held)
        await run.prove_spending(new)
        # Serving checks both the new hotkey's signature and its migrated UID.
        serving = bt.calls.SubtensorModule.serve_axon(
            netuid=0,
            version=1,
            ip=0x08080808,
            port=8091,
            ip_type=4,
            protocol=4,
            placeholder1=0,
            placeholder2=0,
        )
        await run.submit(serving, new)
        await run.submit(
            serving, old, success=False, error="HotKeyNotRegisteredInNetwork", included=False
        )


@pytest.mark.parametrize("source,destination", [(old, new) for old in [0, 1] for new in NEW])
@pytest.mark.parametrize("owner", [0, 1])
async def test_classical_owner_hotkey_upgrade_policy(source, destination, owner):
    cold, old, new = key(owner), key(source), key(destination)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        run = Run(client, f"owner-{owner}-hot-{source}-{destination}")
        await run.fund(cold, old, new)
        await run.register_and_stake(cold, old)
        held = await run.stake(cold, old)
        uid = await run.query("Uids", 0, old.ss58_address)
        allowed = not requires_registration(new)
        await run.submit(
            bt.calls.SubtensorModule.swap_hotkey_v2(
                hotkey=old.ss58_address, new_hotkey=new.ss58_address, netuid=0, keep_stake=False
            ),
            cold,
            success=allowed,
            error=None if allowed else "HotkeyOwnerPolicyViolation",
        )
        active = new if allowed else old
        inactive = old if allowed else new
        assert await run.query("Owner", active.ss58_address) == cold.ss58_address
        assert await run.query("Uids", 0, active.ss58_address) == uid
        assert await run.query("Uids", 0, inactive.ss58_address) is None
        assert await run.stake(cold, active) == held
        assert await run.stake(cold, inactive) == 0


@pytest.mark.parametrize("source,destination", [(0, 7), (7, 1), (1, 11), (11, 0)])
@pytest.mark.parametrize("variant", ["legacy-all", "v2-single", "v2-keep"])
async def test_multi_subnet_swap_preserves_delegation(source, destination, variant):
    cold, old, new, delegator = key(7), key(source), key(destination), key(1)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        run = Run(client, f"multi-{source}-{destination}-{variant}")
        await run.fund(cold, old, new, delegator)
        await run.register_and_stake(cold, old)
        await run.submit(
            bt.calls.SubtensorModule.burned_register(netuid=1, hotkey=old.ss58_address), cold
        )
        await run.submit(
            bt.calls.SubtensorModule.add_stake(
                hotkey=old.ss58_address, netuid=1, amount_staked=5 * 10**9
            ),
            cold,
        )
        for netuid in [0, 1]:
            await run.submit(
                bt.calls.SubtensorModule.add_stake(
                    hotkey=old.ss58_address, netuid=netuid, amount_staked=2 * 10**9
                ),
                delegator,
            )
        before = {
            (who.ss58_address, netuid): await run.stake(who, old, netuid)
            for who in [cold, delegator]
            for netuid in [0, 1]
        }
        assert all(amount > 0 for amount in before.values())
        uids = {netuid: await run.query("Uids", netuid, old.ss58_address) for netuid in [0, 1]}
        keep = variant == "v2-keep"
        call = (
            bt.calls.SubtensorModule.swap_hotkey(
                hotkey=old.ss58_address, new_hotkey=new.ss58_address, netuid=None
            )
            if variant == "legacy-all"
            else bt.calls.SubtensorModule.swap_hotkey_v2(
                hotkey=old.ss58_address, new_hotkey=new.ss58_address, netuid=1, keep_stake=keep
            )
        )
        await run.submit(call, cold)
        for netuid in [0, 1]:
            moved = netuid == 1 or variant == "legacy-all"
            member, nonmember = (new, old) if moved else (old, new)
            assert await run.query("Uids", netuid, member.ss58_address) == uids[netuid]
            assert await run.query("Uids", netuid, nonmember.ss58_address) is None
            stake_hot, empty_hot = (new, old) if moved and not keep else (old, new)
            for who in [cold, delegator]:
                assert await run.stake(who, stake_hot, netuid) == before[who.ss58_address, netuid]
                assert await run.stake(who, empty_hot, netuid) == 0
        # Both the owner and an independent delegator can spend their migrated
        # position (or their retained position when keep_stake was requested).
        stake_hot = old if keep else new
        for who in [cold, delegator]:
            held = await run.stake(who, stake_hot, 1)
            await run.submit(
                bt.calls.SubtensorModule.remove_stake(
                    hotkey=stake_hot.ss58_address, netuid=1, amount_unstaked=held // 2
                ),
                who,
            )
            remaining = await run.stake(who, stake_hot, 1)
            assert 0 < remaining < held
