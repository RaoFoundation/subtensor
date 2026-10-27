"""Falcon activation, atomic payments and Shield boundaries on an isolated chain.

Run serially: these tests change the activation switch and disposable account
nonces. All keys are generated for this run; no user wallet is read or modified.
"""

from __future__ import annotations

import asyncio
import os

import bittensor_core
import pytest

import bittensor as bt
from bittensor._transport.extrinsics import SHIELD_ENCRYPTION_OVERHEAD
from bittensor.hashed import registration_descriptor_value
from bittensor.sp_core import Keypair, requires_registration
from tests.e2e import test_mldsa_release as release
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
CODES = (9, 10, 11, 12)
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


def new_keys():
    return [Keypair.create_from_seed(os.urandom(32), code) for code in CODES]


def registration(key):
    return bt.calls.HashedAccounts.register(
        descriptor=registration_descriptor_value(bytes(key.hashed_descriptor))
    )


async def batch(client, calls):
    result = await client.submit_call(
        bt.calls.Utility.batch_all(calls=[await client.compose(call) for call in calls]),
        dev_wallet(),
        wait_for_finalization=True,
    )
    assert result.success, result.to_dict()
    return result


async def fund(client, keys, *, register=True):
    calls = []
    for key in keys:
        if register and requires_registration(key):
            calls.append(registration(key))
        calls.append(bt.calls.Balances.transfer_keep_alive(dest=key.ss58_address, value=10**11))
    return await batch(client, calls)


async def snapshot(client, key):
    return {
        "account": await release.account(client, key),
        "authority": await release.record(client, key),
    }


def assert_advanced(key, before, after, count=1):
    assert after["account"]["nonce"] == before["account"]["nonce"] + count
    if requires_registration(key):
        assert after["authority"]["generation"] == before["authority"]["generation"] + count
        assert after["authority"]["descriptor"] == before["authority"]["descriptor"]
        assert after["authority"]["commitment"] != before["authority"]["commitment"]
    else:
        assert before["authority"] is after["authority"] is None


async def test_activation_preserves_all_four_wallets_and_classical_signing():
    keys = new_keys()
    classical = [Keypair.create_from_seed(os.urandom(32), code) for code in (0, 1)]
    evidence = {"codes": CODES, "rejections": [], "resumed": []}
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(("AdminUtils", "HashedAccountsEnabled")) is True
        await fund(client, [*keys, *classical])
        raw = client._substrate.raw
        call = await client.compose(bt.calls.System.remark(remark=b"Falcon activation"))
        before = [await snapshot(client, key) for key in keys]
        proofs = [
            await raw.sign_without_nonce_tracking(call, key, nonce=state["account"]["nonce"])
            for key, state in zip(keys, before)
        ]
        try:
            await release.root(
                client, bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=False)
            )
            for key, proof, state in zip(keys, proofs, before):
                evidence["rejections"].append(await release.reject_transaction(raw, proof))
                assert await snapshot(client, key) == state
            for key in classical:
                result = await client.submit_call(call, key, wait_for_finalization=True)
                assert result.success, result.to_dict()
            for key in new_keys():
                if not requires_registration(key):
                    continue
                reserved = (await release.account(client, dev_wallet().coldkey))["data"]["reserved"]
                denied = await client.submit_call(
                    registration(key), dev_wallet(), wait_for_finalization=True
                )
                assert not denied.success and denied.error.name == "Disabled", denied.to_dict()
                assert await release.record(client, key) is None
                assert (await release.account(client, dev_wallet().coldkey))["data"][
                    "reserved"
                ] == reserved
        finally:
            await release.root(
                client, bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=True)
            )
        results = await asyncio.gather(
            *(client.submit_call(call, key, wait_for_finalization=True) for key in keys)
        )
        for key, old, result in zip(keys, before, results):
            assert result.success, result.to_dict()
            assert_advanced(key, old, await snapshot(client, key))
            evidence["resumed"].append(result.to_dict())
    release.evidence("falcon-activation-resume", evidence)


async def test_prefunding_atomic_failure_and_recovery_for_all_four_types():
    seeds = [os.urandom(32) for _ in CODES]
    keys = [Keypair.create_from_seed(seed, code) for seed, code in zip(seeds, CODES)]
    recipients = new_keys()
    sink = Keypair.create_from_seed(os.urandom(32), 1)
    evidence = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, keys, register=False)
        for key in keys:
            assert await release.record(client, key) is None
            assert (await release.account(client, key))["data"]["free"] == 10**11
        registrations = [registration(key) for key in keys if requires_registration(key)]
        before_reserve = (await release.account(client, dev_wallet().coldkey))["data"]["reserved"]
        await batch(client, registrations)
        sponsor = await release.account(client, dev_wallet().coldkey)
        deposit = int(await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit))
        assert deposit == 200_000_000
        assert sponsor["data"]["reserved"] == before_reserve + 2 * deposit
        await batch(client, registrations)
        assert (await release.account(client, dev_wallet().coldkey))["data"]["reserved"] == (
            sponsor["data"]["reserved"]
        )

        async def exercise(key, recipient, seed):
            before = await snapshot(client, key)
            calls = [registration(recipient)] if requires_registration(recipient) else []
            calls += [
                bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**9),
                bt.calls.Balances.transfer_keep_alive(dest=sink.ss58_address, value=10**18),
            ]
            failed = await client.submit_call(
                bt.calls.Utility.batch_all(calls=[await client.compose(call) for call in calls]),
                key,
                wait_for_finalization=True,
            )
            assert not failed.success and failed.block_hash, failed.to_dict()
            assert await release.record(client, recipient) is None
            assert (await release.account(client, recipient))["data"]["free"] == 0
            after = await snapshot(client, key)
            assert_advanced(key, before, after, 2 if failed.data.get("shielded") else 1)
            assert after["account"]["data"]["reserved"] == before["account"]["data"]["reserved"]
            assert failed.fee is not None
            assert (
                before["account"]["data"]["free"] - after["account"]["data"]["free"]
                == failed.fee.rao
            )
            restored = Keypair.create_from_seed(seed, key.crypto_type)
            assert restored.ss58_address == key.ss58_address
            recovered = await client.submit_call(
                bt.calls.Balances.transfer_keep_alive(dest=sink.ss58_address, value=10**9),
                restored,
                wait_for_finalization=True,
            )
            assert recovered.success, recovered.to_dict()
            recovered_state = await snapshot(client, key)
            assert_advanced(key, after, recovered_state)
            assert (
                after["account"]["data"]["free"] - recovered_state["account"]["data"]["free"]
                == 10**9 + recovered.fee.rao
            )
            return {
                "code": key.crypto_type,
                "failed": failed.to_dict(),
                "recovered": recovered.to_dict(),
            }

        evidence = await asyncio.gather(
            *(
                exercise(key, recipient, seed)
                for key, recipient, seed in zip(keys, recipients, seeds)
            )
        )
        assert (await release.account(client, sink))["data"]["free"] == 4 * 10**9
    release.evidence("falcon-prefunding-atomic-failure-recovery", evidence)


async def test_shield_compact_nonce_and_ciphertext_tier_boundaries_for_all_four_types():
    keys = new_keys()
    evidence = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, keys)
        raw = client._substrate.raw
        codec = await raw._runtimes.codec_at(None)
        entry = codec.storage_entry("System", "Account")
        for nonce, execute_target in ((63, 8192), (16383, 8193)):
            items = []
            for key in keys:
                value = await release.account(client, key)
                value["nonce"] = nonce
                items.append(
                    (
                        codec.storage_key(entry, [key.ss58_address]),
                        codec.encode(entry.value_type, value),
                    )
                )
            await release.root(client, bt.calls.System.set_storage(items=items))

            # Each independent client captures only its own quote, while all
            # accounts can finalize in the same ordinary blocks.
            async def exercise(key, nonce, execute_target):
                async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as own:
                    transport = own._substrate.raw
                    pinned = await transport.prepare_hashed_snapshot(key)
                    assert pinned.nonce == nonce
                    before = await snapshot(own, key)
                    sizes = []
                    compose = transport.compose_call

                    async def capture(
                        call_module, call_function, call_params=None, block_hash=None
                    ):
                        if (call_module, call_function) == ("MevShield", "submit_encrypted"):
                            sizes.append(len(call_params["ciphertext"]))
                        return await compose(call_module, call_function, call_params, block_hash)

                    transport.compose_call = capture
                    calls = {}
                    for target in (8192, 8193, 32768, 32769):
                        size = max(0, target - SHIELD_ENCRYPTION_OVERHEAD - 3500)
                        for _ in range(5):
                            call = await own.compose(bt.calls.System.remark(remark=bytes(size)))
                            inner = await transport.create_signed_extrinsic(
                                call,
                                key,
                                nonce=nonce + 1,
                                era={"period": 8},
                                hashed_generation_offset=1,
                                hashed_signing_snapshot=pinned,
                            )
                            delta = target - SHIELD_ENCRYPTION_OVERHEAD - len(inner.data)
                            if not delta:
                                break
                            size += delta
                        ciphertext = bittensor_core.encrypt_mlkem768(
                            bytes(1184), inner.data, include_key_hash=True
                        )
                        assert len(ciphertext) == target
                        if target > 32768:
                            with pytest.raises(ValueError, match="32768-byte carrier limit"):
                                await transport.get_shielded_payment_info(
                                    call, key.public_only(), period=8
                                )
                        else:
                            quote = await transport.get_shielded_payment_info(
                                call, key.public_only(), period=8
                            )
                            assert sizes[-1] == target
                            assert int(quote["partial_fee"]) > 0
                            calls[target] = call
                    result = await own.submit_call(
                        calls[execute_target], key, shielded=True, wait_for_finalization=True
                    )
                    assert result.success and result.data["shielded"], result.to_dict()
                    after = await snapshot(own, key)
                    assert_advanced(key, before, after, 2)
                    assert after["account"]["nonce"] == nonce + 2
                    assert (
                        result.fee.rao
                        == result.data["shield_carrier_fee_rao"]
                        + result.data["shield_inner_fee_rao"]
                    )
                    assert (
                        before["account"]["data"]["free"] - after["account"]["data"]["free"]
                        == result.fee.rao
                    )
                    return {
                        "code": key.crypto_type,
                        "nonce": nonce,
                        "sizes": sizes,
                        "result": result.to_dict(),
                    }

            evidence.extend(
                await asyncio.gather(*(exercise(key, nonce, execute_target) for key in keys))
            )
    release.evidence("falcon-shield-boundaries", evidence)
