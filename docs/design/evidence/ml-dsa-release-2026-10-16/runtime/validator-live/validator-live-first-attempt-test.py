"""Successful MS validator and commitment paths on an activated local chain.

Uses normal block timing and supported subnet-local controls. Every Alice
submission holds the shared sponsor lock through finalization. No global
activation switch, sudo owner, storage fixture or runtime weight is changed.
"""

from __future__ import annotations

import asyncio
import os
import struct
import time
from hashlib import blake2b
from types import SimpleNamespace

import pytest

import bittensor as bt
from bittensor.hashed import registration_descriptor_value
from tests.e2e.test_mldsa_release import evidence, key
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an activated Alice-root localnet"),
]


def chain_bytes(value):
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


def variant_values(value, name):
    if isinstance(value, dict):
        if name in value:
            yield value[name]
        for nested in value.values():
            yield from variant_values(nested, name)
    elif isinstance(value, (list, tuple)):
        for nested in value:
            yield from variant_values(nested, name)


@pytest.mark.parametrize("code", [7, 8])
async def test_ms_validator_weights_and_commitments(code):
    cold, hot, miner = [key(code) for _ in range(3)]
    wallet = SimpleNamespace(coldkey=cold, coldkeypub=cold.public_only(), hotkey=hot)
    report = {
        "crypto_type": code,
        "accounts": {
            "owner": cold.ss58_address,
            "validator": hot.ss58_address,
            "miner": miner.ss58_address,
        },
        "subnet_tempo": 6,
        "normal_block_timing": True,
        "successful": False,
        "receipts": [],
    }

    def save():
        evidence(f"validator-paths-{code}", report)

    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
        assert await client.query(bt.storage.SubtensorModule.NetworkRateLimit) == 0
        assert await client.query(("AdminUtils", "HashedAccountsEnabled")) is True

        def receipt(operation, result):
            report["receipts"].append({"operation": operation, **result.to_dict()})
            save()
            assert result.success, result.to_dict()
            return result

        async def submit(call, *, hotkey=False):
            result = await client.submit_call(
                call,
                wallet,
                signer="hotkey" if hotkey else "coldkey",
                wait_for_finalization=True,
            )
            return receipt(f"{call.module}.{call.function}", result)

        async def execute(intent):
            result = await client.execute(intent, wallet, wait_for_finalization=True)
            return receipt(intent.op, result)

        async def root_batch(calls):
            nested = [
                await client.compose(bt.calls.Sudo.sudo(call=await client.compose(call)))
                for call in calls
            ]
            async with alice_submission():
                result = await client.submit_call(
                    bt.calls.Utility.batch_all(calls=nested),
                    dev_wallet(),
                    wait_for_finalization=True,
                )
            return receipt("Alice subnet-local setup", result)

        async def finalized(item, params):
            block = await client._substrate.raw.rpc_request("chain_getFinalizedHead", [])
            value = await client._substrate.query(item[0], item[1], params, block_hash=block)
            return block, value

        # Register and fund all three fresh descriptors atomically. Sudo is used
        # only for disposable balances; the operation under test uses MS keys.
        funding = []
        for signer in (cold, hot, miner):
            funding.append(
                await client.compose(
                    bt.calls.HashedAccounts.register(
                        descriptor=registration_descriptor_value(bytes(signer.hashed_descriptor))
                    )
                )
            )
            funding.append(
                await client.compose(
                    bt.calls.Sudo.sudo(
                        call=await client.compose(
                            bt.calls.Balances.force_set_balance(
                                who=signer.ss58_address,
                                new_free=10**16 if signer is cold else 100 * 10**9,
                            )
                        )
                    )
                )
            )
        async with alice_submission():
            funded = await client.submit_call(
                bt.calls.Utility.batch_all(calls=funding),
                dev_wallet(),
                wait_for_finalization=True,
            )
        receipt("Alice atomic descriptor registration and funding", funded)
        registered = await execute(bt.intents.RegisterSubnet(hotkey_ss58=hot.ss58_address))
        netuid = int(registered.data["netuid"])
        report["netuid"] = netuid
        await root_batch(
            [
                bt.calls.AdminUtils.sudo_set_max_allowed_uids(netuid=netuid, max_allowed_uids=64),
                bt.calls.AdminUtils.sudo_set_max_registrations_per_block(
                    netuid=netuid, max_registrations_per_block=10
                ),
                bt.calls.AdminUtils.sudo_set_network_registration_allowed(
                    netuid=netuid, registration_allowed=True
                ),
                bt.calls.AdminUtils.sudo_set_min_burn(netuid=netuid, min_burn=10**8),
                bt.calls.AdminUtils.sudo_set_max_burn(netuid=netuid, max_burn=2 * 10**8),
                bt.calls.AdminUtils.sudo_set_min_allowed_weights(
                    netuid=netuid, min_allowed_weights=1
                ),
                bt.calls.AdminUtils.sudo_set_weights_set_rate_limit(
                    netuid=netuid, weights_set_rate_limit=0
                ),
                bt.calls.AdminUtils.sudo_set_commit_reveal_weights_enabled(
                    netuid=netuid, enabled=False
                ),
                bt.calls.AdminUtils.sudo_set_subtoken_enabled(netuid=netuid, subtoken_enabled=True),
                bt.calls.AdminUtils.sudo_set_tempo(netuid=netuid, tempo=6),
            ]
        )
        await execute(bt.intents.BurnedRegister(netuid=netuid, hotkey_ss58=miner.ss58_address))
        validator_uid = int(
            await client.query(bt.storage.SubtensorModule.Uids, [netuid, hot.ss58_address])
        )
        miner_uid = int(
            await client.query(bt.storage.SubtensorModule.Uids, [netuid, miner.ss58_address])
        )
        assert validator_uid != miner_uid
        report.update(validator_uid=validator_uid, miner_uid=miner_uid)

        # Owner-authorized Yuma non-self weights, then a genuine Null permit
        # selected from stake, not an injected ValidatorPermit storage value.
        await execute(bt.intents.SetWeights(netuid=netuid, weights={miner_uid: 1.0}))
        _, weights = await finalized(bt.storage.SubtensorModule.Weights, [netuid, validator_uid])
        assert weights == [(miner_uid, 65535)], weights
        await execute(
            bt.intents.AddStake(hot.ss58_address, netuid, 1000, slippage_protection=False)
        )
        await submit(bt.calls.AdminUtils.sudo_set_epoch_consensus(netuid=netuid, mode="Null"))
        permits = await client.query(bt.storage.SubtensorModule.ValidatorPermit, [netuid])
        assert permits[validator_uid] is True and sum(permits) == 1, permits
        await execute(bt.intents.SetWeights(netuid=netuid, weights={miner_uid: 9}, raw_u16=True))
        _, weights = await finalized(bt.storage.SubtensorModule.Weights, [netuid, validator_uid])
        assert weights == [(miner_uid, 9)], weights
        report["null_permit"] = permits

        digest = blake2b(f"MS commitment {code}".encode(), digest_size=32).digest()
        await submit(
            bt.calls.Commitments.set_commitment(
                netuid=netuid, info={"fields": [[{"BlakeTwo256": digest}]]}
            ),
            hotkey=True,
        )
        _, stored = await finalized(bt.storage.Commitments.CommitmentOf, [netuid, hot.ss58_address])
        assert any(chain_bytes(value) == digest for value in variant_values(stored, "BlakeTwo256"))
        report["plain_commitment"] = stored

        plaintext = f"MS timelock commitment {code}".encode()
        stored_round = int(await client.query(bt.storage.Drand.LastStoredRound) or 0)
        assert stored_round > 0, "requires a running drand bridge"
        sealed = bt.timelock.encrypt(
            plaintext, reveal_round=max(stored_round, bt.timelock.current_round()) + 10
        )
        await submit(
            bt.calls.Commitments.set_commitment(
                netuid=netuid,
                info={
                    "fields": [
                        [
                            {
                                "TimelockEncrypted": {
                                    "encrypted": sealed.encrypted,
                                    "reveal_round": sealed.reveal_round,
                                }
                            }
                        ]
                    ]
                },
            ),
            hotkey=True,
        )
        deadline = time.monotonic() + 180
        while True:
            block, revealed = await finalized(
                bt.storage.Commitments.RevealedCommitments, [netuid, hot.ss58_address]
            )
            if any(chain_bytes(data) == plaintext and int(at) > 0 for data, at in revealed or []):
                report["timelock_commitment_revealed_at_finalized_block"] = block
                break
            assert time.monotonic() < deadline, ("metadata timelock did not reveal", revealed)
            await asyncio.sleep(3)

        await root_batch(
            [
                bt.calls.AdminUtils.sudo_set_commit_reveal_weights_interval(
                    netuid=netuid, interval=1
                ),
                bt.calls.AdminUtils.sudo_set_commit_reveal_weights_enabled(
                    netuid=netuid, enabled=True
                ),
            ]
        )
        await execute(
            bt.intents.CommitWeights(netuid=netuid, weights={miner_uid: 13}, raw_u16=True)
        )
        deadline = time.monotonic() + 360
        while True:
            block, weights = await finalized(
                bt.storage.SubtensorModule.Weights, [netuid, validator_uid]
            )
            if weights == [(miner_uid, 13)]:
                report["timelocked_weights_revealed_at_finalized_block"] = block
                break
            assert time.monotonic() < deadline, ("timelocked weights did not reveal", weights)
            await asyncio.sleep(3)

        # Legacy salt commitments authenticate the stable AccountId, including
        # rotating MS. The manually encoded tuple has one UID/value and two salts.
        salt = [101, 202]
        encoded = (
            bytes(hot.public_key)
            + struct.pack("<H", netuid)
            + b"\x04"
            + struct.pack("<H", miner_uid)
            + b"\x04"
            + struct.pack("<H", 65535)
            + b"\x08"
            + struct.pack("<HHQ", *salt, 0)
        )
        commit_hash = "0x" + blake2b(encoded, digest_size=32).hexdigest()
        await submit(
            bt.calls.SubtensorModule.commit_weights(netuid=netuid, commit_hash=commit_hash),
            hotkey=True,
        )
        commits = await client.query(
            bt.storage.SubtensorModule.WeightCommits, [netuid, hot.ss58_address]
        )
        matching = [entry for entry in commits if chain_bytes(entry[0]).hex() == commit_hash[2:]]
        assert len(matching) == 1, commits
        target_epoch = int(matching[0][1]) + 1
        deadline = time.monotonic() + 180
        while (
            int(await client.query(bt.storage.SubtensorModule.SubnetEpochIndex, [netuid]))
            < target_epoch
        ):
            assert time.monotonic() < deadline, "legacy reveal epoch did not arrive"
            await asyncio.sleep(2)
        await execute(
            bt.intents.RevealWeights(netuid=netuid, uids=[miner_uid], weights=[1.0], salt=salt)
        )
        block, weights = await finalized(
            bt.storage.SubtensorModule.Weights, [netuid, validator_uid]
        )
        assert weights == [(miner_uid, 65535)], weights
        _, commits = await finalized(
            bt.storage.SubtensorModule.WeightCommits, [netuid, hot.ss58_address]
        )
        assert not commits, commits
        report.update(successful=True, legacy_reveal_finalized_block=block)
        save()
