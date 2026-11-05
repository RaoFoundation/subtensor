"""Fund-safety release regressions on a disposable activated Alice-root chain.

Run serially: the activation test temporarily changes the global activation switch, and the
Shield boundary test uses sudo to set a disposable account's nonce to 63.
The chain may use production weights; these tests do not measure capacity.
"""

from __future__ import annotations

import json
import os
from dataclasses import replace
from pathlib import Path

import bittensor_core
import pytest

import bittensor as bt
from bittensor._transport.contract import SignedExtrinsic
from bittensor._transport.errors import SubstrateRequestException
from bittensor._transport.extrinsics import SHIELD_ENCRYPTION_OVERHEAD
from bittensor.hashed import registration_descriptor_value
from bittensor.mldsa_signing import prepare_mldsa_extrinsic, sign_prepared_mldsa
from bittensor.receiving import receiving_address
from bittensor.sp_core import PQ_CRYPTO_TYPES, Keypair, is_stateless_pq
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


def evidence(name, value):
    if directory := os.getenv("E2E_EVIDENCE_DIR"):
        path = Path(directory)
        path.mkdir(parents=True, exist_ok=True)
        (path / f"{name}.json").write_text(json.dumps(value, indent=2, default=str) + "\n")


def key(code):
    return Keypair.create_from_seed(os.urandom(32), code)


async def account(client, signer):
    return await client.query(bt.storage.System.Account, [signer.ss58_address])


async def record(client, signer):
    return await client.query(
        (
            "HashedAccounts",
            "MlDsaAccounts" if signer.crypto_type in PQ_CRYPTO_TYPES else "Accounts",
        ),
        [signer.ss58_address],
    )


async def fund(client, signer, amount=100):
    result = await client.execute(
        bt.intents.Transfer(receiving_address(signer), amount),
        dev_wallet(),
        wait_for_finalization=True,
        max_registration_deposit=bt.Balance.from_tao(2),
    )
    assert result.success, result.to_dict()
    if is_stateless_pq(signer):
        assert await record(client, signer) is None


async def root(client, call):
    result = await client.submit_call(
        bt.calls.Sudo.sudo(call=await client.compose(call)),
        dev_wallet(),
        wait_for_finalization=True,
    )
    assert result.success, result.to_dict()
    return result


async def reject_transaction(raw, extrinsic):
    """Require pool validation rejection, never a timeout or admission failure."""
    with pytest.raises(
        SubstrateRequestException,
        match=r"(?i)badproof|bad proof|transaction has a bad signature",
    ) as rejected:
        await raw.submit_extrinsic(extrinsic)
    return {"message": str(rejected.value), "payload": rejected.value.payload}


@pytest.mark.parametrize("code", [4, 6, 7, 8])
async def test_prefunded_bare_account_can_register_and_spend_recovered_balance(code):
    seed = os.urandom(32)
    recipient = Keypair.create_from_seed(seed, code)
    destination = key(1)
    receipts = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        # Deliberately bypass descriptor-aware recipient setup, as another
        # Substrate client sending to a bare SS58 account would do.
        sent = await client.submit_call(
            bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**10),
            dev_wallet(),
            wait_for_finalization=True,
        )
        assert sent.success, sent.to_dict()
        receipts.append(sent.to_dict())
        assert await record(client, recipient) is None
        assert (await account(client, recipient))["data"]["free"] == 10**10
        if code != 8:
            registration = bt.calls.HashedAccounts.register(
                descriptor=registration_descriptor_value(bytes(recipient.hashed_descriptor))
            )
            registered = await client.submit_call(
                registration, dev_wallet(), wait_for_finalization=True
            )
            assert registered.success, registered.to_dict()
            receipts.append(registered.to_dict())
            assert (await account(client, recipient))["data"]["free"] == 10**10
            before = await account(client, dev_wallet().coldkey)
            repeated = await client.submit_call(
                registration, dev_wallet(), wait_for_finalization=True
            )
            assert repeated.success, repeated.to_dict()
            receipts.append(repeated.to_dict())
            assert (await account(client, dev_wallet().coldkey))["data"]["reserved"] == before[
                "data"
            ]["reserved"]
        state = await record(client, recipient)
        restored = Keypair.create_from_seed(seed, code)
        assert restored.ss58_address == recipient.ss58_address
        spent = await client.submit_call(
            bt.calls.Balances.transfer_keep_alive(dest=destination.ss58_address, value=10**9),
            restored,
            wait_for_finalization=True,
        )
        assert spent.success, spent.to_dict()
        assert (await account(client, destination))["data"]["free"] == 10**9
        if code == 8:
            assert state is None and await record(client, recipient) is None
        else:
            assert (await record(client, recipient))["generation"] == state["generation"] + 1
        receipts.append(spent.to_dict())
        evidence(f"late-registration-{code}", receipts)


@pytest.mark.parametrize("code", [4, 6, 7, 8])
async def test_failed_registration_payment_batch_preserves_funds_and_next_authority(code):
    seed = os.urandom(32)
    sender, recipient, destination = Keypair.create_from_seed(seed, code), key(code), key(1)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, sender)
        before = await record(client, sender)
        sender_before = await account(client, sender)
        registration = await client.compose(
            bt.calls.HashedAccounts.register(
                descriptor=registration_descriptor_value(bytes(recipient.hashed_descriptor))
            )
        )
        transfer = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**9)
        )
        fail = await client.compose(
            bt.calls.Balances.transfer_keep_alive(dest=destination.ss58_address, value=10**18)
        )
        result = await client.submit_call(
            bt.calls.Utility.batch_all(
                calls=([registration] if code != 8 else []) + [transfer, fail]
            ),
            sender,
            wait_for_finalization=True,
        )
        assert not result.success and result.block_hash, result.to_dict()
        assert await record(client, recipient) is None
        assert (await account(client, recipient))["data"]["free"] == 0
        assert (await account(client, destination))["data"]["free"] == 0
        after = await record(client, sender)
        sender_after = await account(client, sender)
        signed_count = 2 if result.data.get("shielded") else 1
        if code == 8:
            assert before is after is None
        else:
            assert after["generation"] == before["generation"] + signed_count
        assert sender_after["nonce"] == sender_before["nonce"] + signed_count
        assert sender_after["data"]["reserved"] == sender_before["data"]["reserved"]
        # A restored key recovers the ordinary nonce and, for rotating keys, generation.
        restored = Keypair.create_from_seed(seed, code)
        recovered = await client.submit_call(
            bt.calls.Balances.transfer_keep_alive(dest=destination.ss58_address, value=10**9),
            restored,
            wait_for_finalization=True,
        )
        assert recovered.success, recovered.to_dict()
        assert (await account(client, destination))["data"]["free"] == 10**9
        evidence(f"atomic-failure-recovery-{code}", [result.to_dict(), recovered.to_dict()])


async def test_master_gate_preserves_funds_and_resumes_all_registered_authorities():
    protected = [key(code) for code in (4, 6, 7, 8)]
    classical = [key(code) for code in (0, 1)]
    receipts = []
    rejections = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        assert await client.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
        for signer in (*protected, *classical):
            await fund(client, signer)
        initial_master = await client.query(("AdminUtils", "HashedAccountsEnabled"))
        assert initial_master is True
        raw = client._substrate.raw
        call = await client.compose(bt.calls.System.remark(remark="0x1234"))
        states = [await record(client, signer) for signer in protected]
        balances = [await account(client, signer) for signer in protected]
        # Rejected probes must not reserve nonces needed after reactivation.
        signed = [
            await raw.sign_without_nonce_tracking(call, signer, nonce=balance["nonce"])
            for signer, balance in zip(protected, balances)
        ]
        try:
            receipts.append(
                (
                    await root(
                        client, bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=False)
                    )
                ).to_dict()
            )
            for signer, extrinsic, state, balance in zip(protected, signed, states, balances):
                rejections.append(await reject_transaction(raw, extrinsic))
                assert await record(client, signer) == state
                assert await account(client, signer) == balance
            for signer in classical:
                result = await client.submit_call(call, signer, wait_for_finalization=True)
                assert result.success, result.to_dict()
            for code in (4, 6, 7):
                fresh = key(code)
                before = await account(client, dev_wallet().coldkey)
                with pytest.raises(ValueError, match="not supported"):
                    await client.execute(
                        bt.intents.Transfer(receiving_address(fresh), 1),
                        dev_wallet(),
                        max_registration_deposit=bt.Balance.from_tao(2),
                    )
                assert await record(client, fresh) is None
                assert await account(client, dev_wallet().coldkey) == before
                registration = await client.submit_call(
                    bt.calls.HashedAccounts.register(
                        descriptor=registration_descriptor_value(bytes(fresh.hashed_descriptor))
                    ),
                    dev_wallet(),
                    wait_for_finalization=True,
                )
                assert not registration.success, registration.to_dict()
                assert registration.error and registration.error.name == "Disabled", (
                    registration.to_dict()
                )
                receipts.append(registration.to_dict())
                assert await record(client, fresh) is None
                assert (await account(client, dev_wallet().coldkey))["data"]["reserved"] == before[
                    "data"
                ]["reserved"]
        finally:
            await root(
                client,
                bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=initial_master),
            )
        for signer in protected:
            result = await client.submit_call(call, signer, wait_for_finalization=True)
            assert result.success, result.to_dict()
            receipts.append(result.to_dict())
        evidence("activation-resume", {"receipts": receipts, "rejections": rejections})


@pytest.mark.parametrize("code", [7, 8])
async def test_mldsa_sudo_authority_can_dispatch_and_restore_root(code):
    """A real MS signature can exercise privileged calls when assigned that role."""
    signer, destination = key(code), key(1)
    receipts = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        original = await client.query(bt.storage.Sudo.Key)
        assert original == dev_wallet().coldkey.ss58_address
        await fund(client, signer)
        assigned = await client.submit_call(
            bt.calls.Sudo.set_key(new=signer.ss58_address),
            dev_wallet(),
            wait_for_finalization=True,
        )
        assert assigned.success, assigned.to_dict()
        receipts.append(assigned.to_dict())
        try:
            assert await client.query(bt.storage.Sudo.Key) == signer.ss58_address
            result = await client.submit_call(
                bt.calls.Sudo.sudo(
                    call=await client.compose(
                        bt.calls.Balances.force_set_balance(
                            who=destination.ss58_address, new_free=10**9
                        )
                    )
                ),
                signer,
                wait_for_finalization=True,
            )
            assert result.success, result.to_dict()
            assert (await account(client, destination))["data"]["free"] == 10**9
            receipts.append(result.to_dict())
        finally:
            restored = await client.submit_call(
                bt.calls.Sudo.set_key(new=original), signer, wait_for_finalization=True
            )
            assert restored.success, restored.to_dict()
            assert await client.query(bt.storage.Sudo.Key) == original
            receipts.append(restored.to_dict())
        evidence(f"sudo-authority-{code}", receipts)


@pytest.mark.parametrize("code", [7, 8])
async def test_wrong_chain_and_payload_proofs_cannot_change_authority_or_balance(code):
    signer = key(code)
    rejections = []
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, signer)
        raw = client._substrate.raw
        call = await client.compose(bt.calls.System.remark(remark="0x010203"))
        prepared = await raw.prepare_mldsa_unsigned(
            call, descriptor=bytes(signer.hashed_descriptor)
        )
        codec = await raw._runtimes.codec_at(prepared.authority.block_hash)
        genesis = await raw.genesis_hash()
        proof = sign_prepared_mldsa(prepared, signer, codec=codec, expected_genesis_hash=genesis)
        state, funds = await record(client, signer), await account(client, signer)
        wrong_genesis = "0x" + "ab" * 32
        wrong = prepare_mldsa_extrinsic(
            codec,
            call,
            prepared.authority,
            era="00",
            genesis_hash=wrong_genesis,
            era_block_hash=wrong_genesis,
        )
        wrong_proof = sign_prepared_mldsa(
            wrong, signer, codec=codec, expected_genesis_hash=wrong_genesis
        )
        changed = await client.compose(bt.calls.System.remark(remark="0x010204"))
        for used_call, used_proof in (
            (call, wrong_proof),
            (changed, proof),
            (call, proof[:-1] + bytes([proof[-1] ^ 1])),
        ):
            encoded, extrinsic_hash = codec.encode_signed_extrinsic(
                used_call,
                public_key=bytes(signer.public_key),
                signature=used_proof,
                signature_version=code,
                era=prepared.unsigned.era,
                nonce=prepared.authority.nonce,
                tip=0,
                tip_asset_id=None,
            )
            rejections.append(
                await reject_transaction(raw, SignedExtrinsic(encoded, extrinsic_hash))
            )
            assert await record(client, signer) == state
            assert await account(client, signer) == funds
        with pytest.raises(ValueError, match="changed"):
            sign_prepared_mldsa(
                replace(prepared, unsigned=replace(prepared.unsigned, tip=1)),
                signer,
                codec=codec,
                expected_genesis_hash=genesis,
            )
        valid = await raw.attach_mldsa_proof(prepared, proof)
        result = await raw.submit_extrinsic(valid, wait_for_finalization=True)
        assert result.is_success and result.finalized
        evidence(
            f"proof-binding-{code}",
            {
                "block_hash": result.block_hash,
                "account": signer.ss58_address,
                "rejections": rejections,
            },
        )


@pytest.mark.parametrize("code", [4, 6, 7, 8])
async def test_shield_nonce_boundary_quote_matches_real_carrier_and_executes(code):
    signer = key(code)
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, signer)
        raw = client._substrate.raw
        codec = await raw._runtimes.codec_at(None)
        value = await account(client, signer)
        value["nonce"] = 63
        entry = codec.storage_entry("System", "Account")
        storage_key = codec.storage_key(entry, [signer.ss58_address])
        await root(
            client,
            bt.calls.System.set_storage(
                items=[(storage_key, codec.encode(entry.value_type, value))]
            ),
        )
        before = await record(client, signer)
        snapshot = await raw.prepare_hashed_snapshot(signer)
        assert snapshot.nonce == 63
        sizes = []
        compose = raw.compose_call

        async def capture(call_module, call_function, call_params=None, block_hash=None):
            if (call_module, call_function) == ("MevShield", "submit_encrypted"):
                sizes.append(len(call_params["ciphertext"]))
            return await compose(call_module, call_function, call_params, block_hash)

        raw.compose_call = capture
        for target in (8193, 32769, 8192):
            size = max(0, target - SHIELD_ENCRYPTION_OVERHEAD - 5500)
            for _ in range(5):
                call = await client.compose(bt.calls.System.remark(remark=bytes(size)))
                inner = await raw.create_signed_extrinsic(
                    call,
                    signer,
                    nonce=64,
                    era={"period": 8},
                    hashed_generation_offset=1,
                    hashed_signing_snapshot=snapshot,
                )
                delta = target - SHIELD_ENCRYPTION_OVERHEAD - len(inner.data)
                if not delta:
                    break
                size += delta
            encrypted = bittensor_core.encrypt_mlkem768(
                bytes(1184), inner.data, include_key_hash=True
            )
            assert len(encrypted) == target
            if target > 32768:
                with pytest.raises(ValueError, match="32768-byte carrier limit"):
                    await raw.get_shielded_payment_info(call, signer.public_only(), period=8)
            else:
                quote = await raw.get_shielded_payment_info(call, signer.public_only(), period=8)
                assert sizes[-1] == target
                assert int(quote["partial_fee"]) > 0
        result = await client.submit_call(call, signer, shielded=True, wait_for_finalization=True)
        assert result.success, result.to_dict()
        assert (await account(client, signer))["nonce"] == 65
        if code == 8:
            assert before is None and await record(client, signer) is None
        else:
            assert (await record(client, signer))["generation"] == before["generation"] + 2
        evidence(
            f"shield-nonce-boundary-{code}", {"quoted_sizes": sizes, "result": result.to_dict()}
        )
