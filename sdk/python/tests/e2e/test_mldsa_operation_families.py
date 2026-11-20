"""Successful ML-DSA business operations beyond basic transfers and authorization.

Requires an isolated activated localnet with development authorization weights.
Root changes only disposable-chain setup; every operation under test is signed
by a fresh fixed or rotating ML-DSA wallet. Parallel workers must share
E2E_WALLET_ALICE_LOCK; other suites must use a separate chain.
"""

from __future__ import annotations

import json
import os
from hashlib import blake2b
from pathlib import Path
from types import SimpleNamespace

import pytest

import bittensor as bt
from bittensor.receiving import receiving_address
from tests.e2e.test_mldsa_release import account, evidence, key
from tests.e2e.test_mldsa_release import fund as sponsor_fund
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


def event(result, module, name):
    for item in result.events:
        value = item.get("event", item)
        if (value.get("module_id"), value.get("event_id")) == (module, name):
            return value["attributes"]
    raise AssertionError((module, name, result.to_dict()))


def stored_bytes(value):
    if isinstance(value, str):
        return bytes.fromhex(value[2:]) if value.startswith("0x") else value.encode()
    return bytes(value)


async def batch_root(client, calls):
    wrapped = [bt.calls.Sudo.sudo(call=await client.compose(call)) for call in calls]
    async with alice_submission():
        result = await client.submit_call(
            bt.calls.Utility.batch_all(calls=[await client.compose(call) for call in wrapped]),
            dev_wallet(),
            wait_for_finalization=True,
        )
    assert result.success, result.to_dict()


async def fund(client, signer, amount=100):
    async with alice_submission():
        await sponsor_fund(client, signer, amount)


@pytest.mark.parametrize("code", [7, 8])
async def test_staking_locks_children_identity_serving_and_hotkey_swap(code):
    cold, hot, second, replacement, recipient = [key(code) for _ in range(5)]
    wallet = SimpleNamespace(coldkey=cold, coldkeypub=cold.public_only(), hotkey=hot)
    receipts = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        for signer in (cold, hot, second, replacement, recipient):
            await fund(client, signer, 1000)
        await batch_root(
            client,
            [
                bt.calls.Balances.force_set_balance(who=cold.ss58_address, new_free=10**16),
                bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0),
                bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0),
                bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
                bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0),
                bt.calls.AdminUtils.sudo_set_stake_threshold(min_stake=0),
                bt.calls.AdminUtils.sudo_set_nominator_min_required_stake(min_stake=1),
                bt.calls.SubtensorModule.set_pending_childkey_cooldown(cooldown=0),
            ],
        )

        async def execute(intent, signer=wallet):
            result = await client.execute(intent, signer, wait_for_finalization=True)
            assert result.success, result.to_dict()
            receipts.append({"operation": intent.op, **result.to_dict()})
            return result

        async def submit(call, signer=wallet, *, role="coldkey", success=True):
            result = await client.submit_call(call, signer, signer=role, wait_for_finalization=True)
            assert result.success == success, result.to_dict()
            receipts.append({"operation": f"{call.module}.{call.function}", **result.to_dict()})
            return result

        nets = []
        for _ in range(2):
            result = await execute(bt.intents.RegisterSubnet(hotkey_ss58=hot.ss58_address))
            netuid = result.data["netuid"]
            nets.append(netuid)
            await batch_root(
                client,
                [
                    bt.calls.AdminUtils.sudo_set_max_allowed_uids(
                        netuid=netuid, max_allowed_uids=64
                    ),
                    bt.calls.AdminUtils.sudo_set_max_registrations_per_block(
                        netuid=netuid, max_registrations_per_block=10
                    ),
                    bt.calls.AdminUtils.sudo_set_target_registrations_per_interval(
                        netuid=netuid, target_registrations_per_interval=100
                    ),
                    bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                        netuid=netuid, registration_allowed=True
                    ),
                    bt.calls.AdminUtils.sudo_set_min_burn(netuid=netuid, min_burn=10**8),
                    # Production requires max burn strictly above both the
                    # configured minimum and its 0.1 TAO lower bound.
                    bt.calls.AdminUtils.sudo_set_max_burn(netuid=netuid, max_burn=2 * 10**8),
                    bt.calls.AdminUtils.sudo_set_serving_rate_limit(
                        netuid=netuid, serving_rate_limit=0
                    ),
                    bt.calls.AdminUtils.sudo_set_collateral_lock_share(netuid=netuid, lock_share=0),
                    bt.calls.AdminUtils.sudo_set_toggle_transfer(netuid=netuid, toggle=True),
                    bt.calls.AdminUtils.sudo_set_immunity_period(
                        netuid=netuid, immunity_period=65535
                    ),
                ],
            )
            await execute(bt.intents.BurnedRegister(netuid=netuid, hotkey_ss58=second.ss58_address))
        first, other = nets
        assert (
            await client.query(bt.storage.SubtensorModule.Owner, [hot.ss58_address])
            == cold.ss58_address
        )
        assert (
            await client.query(bt.storage.SubtensorModule.Owner, [second.ss58_address])
            == cold.ss58_address
        )

        # A subnet owner's hotkey is exempt from the delegation stake floor.
        await execute(bt.intents.SetChildren(first, [[2**63, receiving_address(second)]]))
        children = await client.query(
            bt.storage.SubtensorModule.ChildKeys, [hot.ss58_address, first]
        )
        assert any(child[1] == second.ss58_address for child in children), children
        await execute(
            bt.intents.SetIdentity(name=f"MS release {code}", description="disposable local test")
        )
        identity = await client.query(bt.storage.SubtensorModule.IdentitiesV2, [cold.ss58_address])
        assert stored_bytes(identity["name"]) == f"MS release {code}".encode()
        await execute(bt.intents.SetSubnetIdentity(first, f"MS subnet {code}"))
        identity = await client.query(bt.storage.SubtensorModule.SubnetIdentitiesV3, [first])
        assert stored_bytes(identity["subnet_name"]) == f"MS subnet {code}".encode()
        await execute(bt.intents.ServeAxon(first, "8.8.8.8", 8091))
        axon = await client.query(bt.storage.SubtensorModule.Axons, [first, hot.ss58_address])
        assert int(axon["port"]) == 8091
        await execute(bt.intents.ServePrometheus(first, "8.8.4.4", 9091))
        prometheus = await client.query(
            bt.storage.SubtensorModule.Prometheus, [first, hot.ss58_address]
        )
        assert int(prometheus["port"]) == 9091
        await execute(bt.intents.ResetAxon(first))
        assert (
            int(
                (await client.query(bt.storage.SubtensorModule.Axons, [first, hot.ss58_address]))[
                    "ip"
                ]
            )
            == 0
        )

        await batch_root(
            client,
            [
                bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=netuid, subtoken_enabled=True)
                for netuid in nets
            ],
        )
        for netuid in nets:
            await execute(
                bt.intents.AddStake(hot.ss58_address, netuid, 20, slippage_protection=False)
            )

        async def stake(owner=cold, target=hot, netuid=first):
            value = await client.read(
                "stake",
                coldkey_ss58=owner.ss58_address,
                hotkey_ss58=target.ss58_address,
                netuid=netuid,
            )
            return value.rao

        original = await stake()
        portion = max(1000, original // 20)
        amount = bt.Balance.from_rao(portion, first)
        await execute(
            bt.intents.MoveStake(hot.ss58_address, first, second.ss58_address, first, amount)
        )
        assert abs((await stake()) - (original - portion)) <= 2
        assert abs((await stake(target=second)) - portion) <= 2
        recipient_before = await stake(owner=recipient)
        await execute(
            bt.intents.TransferStake(
                receiving_address(recipient), hot.ss58_address, first, first, amount
            )
        )
        assert abs((await stake(owner=recipient)) - (recipient_before + portion)) <= 2
        before_swap = await stake(netuid=other)
        await execute(
            bt.intents.SwapStake(hot.ss58_address, first, other, amount, slippage_protection=False)
        )
        assert await stake(netuid=other) > before_swap
        await execute(
            bt.intents.MoveStake(
                second.ss58_address,
                first,
                second.ss58_address,
                other,
                bt.Balance.from_rao(portion // 2, first),
                slippage_protection=False,
            )
        )
        assert await stake(target=second, netuid=other) > 0
        await execute(bt.intents.SetPerpetualLock(first, True))
        assert (
            await client.query(bt.storage.SubtensorModule.DecayingLock, [cold.ss58_address, first])
            is False
        )
        await execute(
            bt.intents.LockStake(first, bt.Balance.from_rao(portion // 2, first), hot.ss58_address)
        )
        lock = await client.query(
            bt.storage.SubtensorModule.Lock, [cold.ss58_address, first, hot.ss58_address]
        )
        assert int(lock["locked_mass"]) == portion // 2
        await execute(bt.intents.MoveLock(first, second.ss58_address))
        moved = await client.query(
            bt.storage.SubtensorModule.Lock, [cold.ss58_address, first, second.ss58_address]
        )
        assert int(moved["locked_mass"]) == portion // 2
        assert (
            await client.query(
                bt.storage.SubtensorModule.Lock, [cold.ss58_address, first, hot.ss58_address]
            )
            is None
        )
        await execute(bt.intents.SetPerpetualLock(first, False))
        assert (
            await client.query(bt.storage.SubtensorModule.DecayingLock, [cold.ss58_address, first])
            is None
        )
        await execute(bt.intents.SetRejectLockedAlpha(False), recipient)
        assert (
            int(
                await client.query(
                    bt.storage.SubtensorModule.AccountFlags, [recipient.ss58_address]
                )
            )
            & 1
            == 1
        )
        collateral_amount = max(1, portion // 100)
        await execute(
            bt.intents.SetMinCollateral(
                first, bt.Balance.from_rao(collateral_amount, first), hot.ss58_address
            )
        )
        await execute(
            bt.intents.AddCollateral(
                first, bt.Balance.from_rao(collateral_amount, first), hot.ss58_address
            )
        )
        collateral = await client.query(
            bt.storage.SubtensorModule.MinerCollateral,
            [first, hot.ss58_address, cold.ss58_address],
        )
        assert int(collateral["locked"]) == collateral_amount
        assert int(collateral["min_locked"]) == collateral_amount
        await execute(
            bt.intents.RemoveStake(
                hot.ss58_address,
                first,
                bt.Balance.from_rao(portion // 4, first),
                slippage_protection=False,
            )
        )

        uid = await client.query(bt.storage.SubtensorModule.Uids, [first, second.ss58_address])
        held = await stake(target=second)
        await execute(
            bt.intents.SwapHotkey(
                receiving_address(replacement), hotkey_ss58=second.ss58_address, netuid=first
            )
        )
        assert (
            await client.query(bt.storage.SubtensorModule.Uids, [first, replacement.ss58_address])
            == uid
        )
        assert (
            await client.query(bt.storage.SubtensorModule.Uids, [first, second.ss58_address])
            is None
        )
        assert abs((await stake(target=replacement)) - held) <= 2
        assert (
            await client.query(bt.storage.SubtensorModule.Owner, [replacement.ss58_address])
            == cold.ss58_address
        )
        await submit(bt.calls.System.remark(remark=b"replacement hotkey can sign"), replacement)
        evidence(f"business-families-{code}", {"netuids": nets, "receipts": receipts})


@pytest.mark.parametrize("code", [7, 8])
async def test_utility_branches_and_preimage_deposits(code):
    signer, destination = key(code), key(1)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, signer)
        transfer = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=destination.ss58_address, value=10**9)
        )
        fail = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=destination.ss58_address, value=10**18)
        )
        receipts = []
        # Batch keeps prior writes, force_batch continues, if_else chooses one
        # branch. Check actual value, irrespective of nested receipt labeling.
        for call, delta, event_name in (
            (bt.calls.Utility.batch(calls=[transfer, fail, transfer]), 10**9, "BatchInterrupted"),
            (
                bt.calls.Utility.force_batch(calls=[transfer, fail, transfer]),
                2 * 10**9,
                "BatchCompletedWithErrors",
            ),
            (bt.calls.Utility.if_else(main=fail, fallback=transfer), 10**9, "IfElseFallbackCalled"),
            (
                bt.calls.Utility.if_else(main=transfer, fallback=transfer),
                10**9,
                "IfElseMainSuccess",
            ),
        ):
            before = int((await account(client, destination))["data"]["free"])
            result = await client.submit_call(call, signer, wait_for_finalization=True)
            assert result.block_hash, result.to_dict()
            # Failed nested dispatches are represented by a compact error
            # receipt. Assert the actual finalized events and partial writes.
            index = int(result.extrinsic_id.rsplit("-", 1)[1])
            events = await client._substrate.raw.get_events(block_hash=result.block_hash)
            assert any(
                item.get("extrinsic_idx") == index
                and item.get("module_id") == "Utility"
                and item.get("event_id") == event_name
                for item in events
            ), (event_name, events)
            assert int((await account(client, destination))["data"]["free"]) == before + delta
            receipts.append(result.to_dict())
        data = b"ms-release-preimage" + os.urandom(32)
        reserved = int((await account(client, signer))["data"]["reserved"])
        result = await client.submit_call(
            bt.calls.Preimage.note_preimage(bytes=data), signer, wait_for_finalization=True
        )
        assert result.success, result.to_dict()
        assert int((await account(client, signer))["data"]["reserved"]) > reserved
        receipts.append(result.to_dict())
        result = await client.submit_call(
            bt.calls.Preimage.unnote_preimage(
                hash="0x" + blake2b(data, digest_size=32).hexdigest()
            ),
            signer,
            wait_for_finalization=True,
        )
        assert result.success, result.to_dict()
        assert int((await account(client, signer))["data"]["reserved"]) == reserved
        receipts.append(result.to_dict())
        evidence(f"utility-preimage-{code}", receipts)


@pytest.mark.parametrize("code", [7, 8])
async def test_crowdloan_finalize_withdraw_refund_and_dissolve(code):
    creator, contributor, target = key(code), key(code), key(code)
    receipts = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        for signer in (creator, contributor, target):
            await fund(client, signer, 1000)
        deposit = int(await client.constant(("Crowdloan", "MinimumDeposit")))
        minimum = max(
            10**9, int(await client.constant(("Crowdloan", "AbsoluteMinimumContribution")))
        )
        duration = int(await client.constant(("Crowdloan", "MinimumBlockDuration"))) + 100

        async def submit(call, signer=creator):
            result = await client.submit_call(call, signer, wait_for_finalization=True)
            assert result.success, result.to_dict()
            receipts.append(result.to_dict())
            return result

        async def create():
            result = await submit(
                bt.calls.Crowdloan.create(
                    deposit=deposit,
                    min_contribution=minimum,
                    cap=deposit + 2 * minimum,
                    end=await client.block() + duration,
                    call=None,
                    target_address=target.ss58_address,
                )
            )
            return event(result, "Crowdloan", "Created")["crowdloan_id"]

        successful = await create()
        await submit(
            bt.calls.Crowdloan.update_min_contribution(
                crowdloan_id=successful, new_min_contribution=minimum
            )
        )
        await submit(
            bt.calls.Crowdloan.set_max_contribution(
                crowdloan_id=successful, new_max_contribution=max(deposit, 3 * minimum)
            )
        )
        await submit(
            bt.calls.Crowdloan.update_end(
                crowdloan_id=successful, new_end=await client.block() + duration + 100
            )
        )
        await submit(
            bt.calls.Crowdloan.update_cap(crowdloan_id=successful, new_cap=deposit + 3 * minimum)
        )
        await submit(
            bt.calls.Crowdloan.contribute(crowdloan_id=successful, amount=3 * minimum), contributor
        )
        before = int((await account(client, target))["data"]["free"])
        await submit(bt.calls.Crowdloan.finalize(crowdloan_id=successful))
        assert (
            int((await account(client, target))["data"]["free"]) == before + deposit + 3 * minimum
        )
        abandoned = await create()
        await submit(
            bt.calls.Crowdloan.contribute(crowdloan_id=abandoned, amount=minimum), contributor
        )
        await submit(bt.calls.Crowdloan.withdraw(crowdloan_id=abandoned), contributor)
        assert (
            await client.query(
                bt.storage.Crowdloan.Contributions, [abandoned, contributor.ss58_address]
            )
            is None
        )
        await submit(
            bt.calls.Crowdloan.contribute(crowdloan_id=abandoned, amount=minimum), contributor
        )
        before = int((await account(client, contributor))["data"]["free"])
        await submit(bt.calls.Crowdloan.refund(crowdloan_id=abandoned))
        assert int((await account(client, contributor))["data"]["free"]) == before + minimum
        await submit(bt.calls.Crowdloan.dissolve(crowdloan_id=abandoned))
        assert await client.query(bt.storage.Crowdloan.Crowdloans, [abandoned]) is None
        evidence(f"crowdloan-families-{code}", receipts)


@pytest.mark.parametrize("code", [7, 8])
async def test_contracts_upload_remove_instantiate_and_call(code):
    signer = key(code)
    repo = Path(__file__).resolve().parents[4]
    wasm = (repo / "ts-tests/ink/bittensor.wasm").read_bytes()
    # A valid custom section yields test-owned code so removal can assert this
    # signer's deposit rather than depending on a previous uploader's ownership.
    custom = b"\x0ams-release" + os.urandom(16)
    wasm += b"\x00" + bytes([len(custom)]) + custom
    code_hash = "0x" + blake2b(wasm, digest_size=32).hexdigest()
    metadata = json.loads((repo / "ts-tests/ink/bittensor.json").read_text())
    constructor = metadata["spec"]["constructors"][0]["selector"]
    price = next(
        message for message in metadata["spec"]["messages"] if message["label"] == "get_alpha_price"
    )
    data = bytes.fromhex(price["selector"][2:]) + bytes(2)
    gas = {"ref_time": 10_000_000_000, "proof_size": 10_000_000}
    receipts = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, signer, 1000)

        async def submit(call):
            result = await client.submit_call(call, signer, wait_for_finalization=True)
            assert result.success, result.to_dict()
            receipts.append(result.to_dict())
            return result

        reserved = int((await account(client, signer))["data"]["reserved"])
        await submit(
            bt.calls.Contracts.upload_code(
                code=wasm, storage_deposit_limit=None, determinism="Enforced"
            )
        )
        code_info = await client.query(bt.storage.Contracts.CodeInfoOf, [code_hash])
        assert code_info["owner"] == signer.ss58_address
        assert int(code_info["deposit"]) > 0
        assert await client.query(bt.storage.Contracts.PristineCode, [code_hash]) is not None
        assert int((await account(client, signer))["data"]["reserved"]) > reserved
        await submit(bt.calls.Contracts.remove_code(code_hash=code_hash))
        assert await client.query(bt.storage.Contracts.CodeInfoOf, [code_hash]) is None
        assert await client.query(bt.storage.Contracts.PristineCode, [code_hash]) is None
        assert int((await account(client, signer))["data"]["reserved"]) == reserved
        result = await submit(
            bt.calls.Contracts.instantiate_with_code(
                value=0,
                gas_limit=gas,
                storage_deposit_limit=None,
                code=wasm,
                data=constructor,
                salt=os.urandom(8),
            )
        )
        contract = event(result, "Contracts", "Instantiated")["contract"]
        assert await client.query(bt.storage.Contracts.ContractInfoOf, [contract])
        await submit(
            bt.calls.Contracts.call(
                dest=contract, value=0, gas_limit=gas, storage_deposit_limit=None, data=data
            )
        )
        second = await submit(
            bt.calls.Contracts.instantiate(
                value=0,
                gas_limit=gas,
                storage_deposit_limit=None,
                code_hash=code_hash,
                data=constructor,
                salt=os.urandom(8),
            )
        )
        other = event(second, "Contracts", "Instantiated")["contract"]
        assert contract != other
        assert await client.query(bt.storage.Contracts.ContractInfoOf, [other])
        evidence(f"contracts-families-{code}", receipts)
