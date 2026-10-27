"""Four Falcon wallet modes across the reusable release-operation matrix.

Use an isolated activated localnet with development Falcon weights, normal
block timing, and a running drand bridge. These are functional checks, not
reference measurements. All sponsors share E2E_WALLET_ALICE_LOCK and all
accounts are newly generated disposable keys.
"""

from __future__ import annotations

import os
from dataclasses import replace
from itertools import combinations

import pytest

import bittensor as bt
from bittensor.hashed import compose_derivative
from bittensor.mldsa import install_message_capability, message_capability
from bittensor.receiving import receiving_address
from bittensor.sp_core import requires_registration
from tests.e2e import test_mldsa_localnet as order_paths
from tests.e2e import test_mldsa_operation_families as operation_paths
from tests.e2e import test_mldsa_proxy_boundaries as proxy_paths
from tests.e2e import test_mldsa_validator_paths as validator_paths
from tests.e2e.test_mldsa_extended import Evidence, account, fresh_key, snapshot
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
FALCON_MODES = [
    pytest.param(9, id="falcon512"),
    pytest.param(10, id="hashed-falcon512"),
    pytest.param(11, id="falcon1024"),
    pytest.param(12, id="hashed-falcon1024"),
]
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
    pytest.mark.parametrize("code", FALCON_MODES),
]


async def test_staking_children_serving_identity_locks_and_hotkey_swap(code):
    await operation_paths.test_staking_locks_children_identity_serving_and_hotkey_swap(code)


async def test_utility_failures_and_preimage_deposit_refunds(code):
    await operation_paths.test_utility_branches_and_preimage_deposits(code)


async def test_crowdloan_finalize_withdraw_refund_and_dissolve(code):
    await operation_paths.test_crowdloan_finalize_withdraw_refund_and_dissolve(code)


async def test_contracts_upload_remove_instantiate_and_call(code):
    await operation_paths.test_contracts_upload_remove_instantiate_and_call(code)


async def test_order_execution_cancellation_revocation_scope_and_budget(code):
    await order_paths.test_original_limit_order_execution_cancellation_and_capability_revocation(
        code
    )


async def test_validator_plain_null_and_commit_reveal_weights(code):
    await validator_paths.test_ms_validator_weights_and_commitments(code)


async def test_restricted_proxy_cannot_escalate_through_evm_dispatch(code):
    await proxy_paths.test_restricted_ms_proxy_cannot_escalate_through_evm_dispatch(code)


async def test_announced_proxy_delay_regrant_and_revocation(code):
    await proxy_paths.test_ms_delayed_proxy_regrant_preserves_the_accounts_policy(code)


async def test_classical_proxy_grants_follow_fixed_and_rotating_policy(code):
    await proxy_paths.test_ms_classical_delegation_matches_the_account_policy(code)


async def test_message_http_capability_audience_owner_replacement_and_revocation(code):
    owner, other = fresh_key(code), fresh_key(code)
    receiver = dev_wallet().hotkey.ss58_address
    trace = Evidence(f"falcon-message-http-boundaries-{code}")
    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await operation_paths.fund(client, owner)

        async def submit(call, label):
            result = await client.submit_call(call, owner, wait_for_finalization=True)
            trace.add(label, receipt=result.to_dict())
            assert result.success and result.block_hash, result.to_dict()

        await submit(
            install_message_capability(
                owner, epoch=1, expires_at=await client.block() + 10_000, audience=b"0xff"
            ),
            "install-first-message-capability",
        )
        first = await message_capability(client._substrate, owner.ss58_address, b"0xff")
        with pytest.raises(ValueError, match="audience"):
            await message_capability(client._substrate, owner.ss58_address, b"\xff")
        with pytest.raises(ValueError, match="no active"):
            await message_capability(client._substrate, other.ss58_address, b"0xff")

        message = "Falcon capability identity boundary"
        signed = bt.wallets.sign_message_key(message, owner, message_capability=first)["signature"]
        assert bt.wallets.verify_message(
            message, signed, owner.ss58_address, code, message_capability=first
        )
        for altered in (
            replace(first, owner=other.ss58_address),
            replace(first, audience=b"\xff"),
            replace(first, genesis_hash=bytes(32)),
        ):
            assert not bt.wallets.verify_message(
                message, signed, altered.owner, code, message_capability=altered
            )
        with pytest.raises(ValueError, match="does not match"):
            bt.wallets.sign_message_key(message, other, message_capability=first)

        request = bt.http_auth.sign_pq(
            owner,
            capability=first,
            method="POST",
            path="/falcon-capability",
            body=b"authorized request",
            receiver_ss58=receiver,
        )
        store = bt.http_auth.InMemoryNonceStore(retention=660)

        def verify(capability=first, **overrides):
            kwargs = dict(
                capability=capability,
                method="POST",
                path="/falcon-capability",
                self_hotkey_ss58=receiver,
                max_age=600,
                nonce_store=store,
            )
            kwargs.update(overrides)
            return bt.http_auth.verify_pq(request.headers, request.body, **kwargs)

        for altered in (
            replace(first, owner=other.ss58_address),
            replace(first, audience=b"\xff"),
        ):
            with pytest.raises(bt.http_auth.BadSignature):
                verify(altered)
        with pytest.raises(bt.http_auth.BadSignature):
            verify(path="/different-endpoint")
        with pytest.raises(bt.http_auth.WrongReceiver):
            verify(self_hotkey_ss58=other.ss58_address)
        accepted = verify()
        assert accepted.body == b"authorized request"
        assert accepted.caller.hotkey_ss58 == owner.ss58_address
        with pytest.raises(bt.http_auth.ReplayedRequest):
            verify()

        await submit(
            install_message_capability(
                owner, epoch=2, expires_at=await client.block() + 10_000, audience=b"\xff"
            ),
            "replace-message-capability",
        )
        second = await message_capability(client._substrate, owner.ss58_address, b"\xff")
        assert second.epoch == 2 and second.commitment != first.commitment
        assert second.finalized_block > first.finalized_block
        with pytest.raises(ValueError, match="audience"):
            await message_capability(client._substrate, owner.ss58_address, b"0xff")
        assert not bt.wallets.verify_message(
            message, signed, owner.ss58_address, code, message_capability=second
        )
        with pytest.raises(bt.http_auth.BadSignature):
            verify(second)
        replacement = bt.wallets.sign_message_key(message, owner, message_capability=second)
        assert bt.wallets.verify_message(
            message,
            replacement["signature"],
            owner.ss58_address,
            code,
            message_capability=second,
        )
        await submit(bt.calls.HashedAccounts.revoke_capability(role="Message"), "revoke-message")
        with pytest.raises(ValueError, match="no active"):
            await message_capability(client._substrate, owner.ss58_address, b"\xff")
        block = await client._substrate.raw.rpc_request("chain_getFinalizedHead", [])
        assert (
            await client._substrate.query(
                "HashedAccounts",
                "ActiveCapabilities",
                [owner.ss58_address, "Message"],
                block_hash=block,
            )
            is None
        )
        epoch = await client._substrate.query(
            "HashedAccounts", "CapabilityEpochs", [owner.ss58_address, "Message"], block_hash=block
        )
        assert epoch == 3
        trace.add(
            "fresh-finalized-resolution-rejects-revoked-capability",
            block=block,
            epoch=epoch,
            owner=owner.ss58_address,
        )


async def test_safe_mixed_multisig_quorums_cancellation_nested_proxy_and_pure(code):
    """Every mixed quorum works; cancellation refunds and revoked grants cannot spend."""
    members = [fresh_key(kind) for kind in (code, 1, 0)]
    signer, sr, _ed = members
    recipient = fresh_key(code)
    # A fixed principal permits ordinary mixed authority. A rotating principal
    # deliberately rejects grants to an unregistered mixed classical group.
    principal = fresh_key({10: 9, 12: 11}.get(code, code))
    trace = Evidence(f"falcon-safe-composites-{code}")

    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:

        async def accepted(operation, label, *, success=True):
            result = await operation
            trace.add(label, receipt=result.to_dict())
            assert result.success == success and result.block_hash, result.to_dict()
            return result

        async def fund(public, amount=100):
            address = receiving_address(public) if not isinstance(public, str) else public
            async with alice_submission():
                await accepted(
                    client.execute(
                        bt.intents.Transfer(address, amount),
                        dev_wallet(),
                        max_registration_deposit=bt.Balance.from_tao(0.2),
                        wait_for_finalization=True,
                    ),
                    "sponsor-disposable-account",
                )

        for public in (*members, recipient, principal):
            await fund(public)
        group = await client.multisig(
            [receiving_address(member) for member in members], 2, account_mode="safe"
        )
        again = await client.multisig(
            [member.ss58_address for member in reversed(members)], 2, account_mode="safe"
        )
        assert group.address == again.address
        assert (await group.protection_status()) == {
            "eligible": False,
            "registered": False,
            "safe_derivation": True,
        }
        await fund(group.address, 20)
        initial = await account(client, recipient.ss58_address)

        async def quorum(call, first=signer, second=sr, *, success=True):
            before = [await snapshot(client, member) for member in (first, second)]
            for index, member in enumerate((first, second)):
                await accepted(
                    group.approve(call, member, wait_for_finalization=True),
                    "safe-multisig-approval",
                    success=True if index == 0 else success,
                )
            after = [await snapshot(client, member) for member in (first, second)]
            for member, old, new in zip((first, second), before, after):
                assert new["account"]["nonce"] == old["account"]["nonce"] + 1
                if requires_registration(member):
                    assert new["authority"]["generation"] == old["authority"]["generation"] + 1
                    assert new["authority"]["descriptor"] == old["authority"]["descriptor"]
                else:
                    assert old["authority"] is new["authority"] is None
            assert (
                after[0]["account"]["data"]["reserved"] == before[0]["account"]["data"]["reserved"]
            )

        payment = bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**9)
        for first, second in combinations(members, 2):
            await quorum(payment, first, second)
        assert (await account(client, recipient.ss58_address))["data"]["free"] == (
            initial["data"]["free"] + 3 * 10**9
        )
        await quorum(
            bt.calls.Balances.transfer_keep_alive(dest=recipient.ss58_address, value=10**18),
            success=False,
        )

        # An opener can cancel a safe operation using the matching safe address
        # derivation; its ordinary multisig reserve is completely refunded.
        remark = bt.calls.System.remark(remark=b"Falcon safe cancellation")
        composed = await client.compose(remark)
        call_hash = "0x" + bytes(composed.call_hash).hex()
        reserved = (await account(client, signer.ss58_address))["data"]["reserved"]
        await accepted(
            group.approve(remark, signer, wait_for_finalization=True), "open-safe-operation"
        )
        pending = await client.query(bt.storage.Multisig.Multisigs, [group.address, call_hash])
        assert pending
        await accepted(
            client.submit_call(
                bt.calls.Multisig.cancel_as_multi_safe(
                    threshold=2,
                    other_signatories=[
                        address for address in group.signatories if address != signer.ss58_address
                    ],
                    timepoint=pending["when"],
                    call_hash=call_hash,
                ),
                signer,
                wait_for_finalization=True,
            ),
            "cancel-safe-operation",
        )
        assert await client.query(bt.storage.Multisig.Multisigs, [group.address, call_hash]) is None
        assert (await account(client, signer.ss58_address))["data"]["reserved"] == reserved

        # Multisig as a proxy, then a Falcon signer as a multisig's proxy.
        await accepted(
            client.submit_call(
                bt.calls.Proxy.add_proxy(delegate=group.address, proxy_type="Any", delay=0),
                principal,
                wait_for_finalization=True,
            ),
            "grant-safe-group-delegation",
        )
        proxy_call = bt.calls.Proxy.proxy(
            real=principal.ss58_address,
            force_proxy_type="Any",
            call=await client.compose(payment),
        )
        principal_before = await snapshot(client, principal)
        await quorum(proxy_call)
        principal_after = await snapshot(client, principal)
        assert principal_after["account"]["nonce"] == principal_before["account"]["nonce"]
        assert principal_after["account"]["data"]["free"] == (
            principal_before["account"]["data"]["free"] - 10**9
        )
        await quorum(
            bt.calls.Proxy.add_proxy(delegate=signer.ss58_address, proxy_type="Any", delay=0)
        )
        await accepted(
            client.execute(
                bt.intents.Transfer(receiving_address(recipient), 1),
                signer,
                proxy_for=group.address,
                proxy_type="Any",
                wait_for_finalization=True,
            ),
            "Falcon-proxies-for-safe-group",
        )
        await accepted(
            client.submit_call(
                bt.calls.Proxy.remove_proxy(delegate=group.address, proxy_type="Any", delay=0),
                principal,
                wait_for_finalization=True,
            ),
            "revoke-group-proxy",
        )
        principal_before = await snapshot(client, principal)
        await quorum(proxy_call, success=False)
        assert await snapshot(client, principal) == principal_before

        # The normal wrapper selects safe derivation for fixed Falcon and
        # protected derivation for rotating Falcon. Execute from that actual
        # origin, fund it, and prove it can spend under the same parent key.
        child_remark = await client.compose(
            bt.calls.System.remark_with_event(remark=b"Falcon derivative origin")
        )
        derived_call = await compose_derivative(client._substrate, signer, 21, child_remark)
        expected_derivative = (
            bt.calls.Utility.as_protected_derivative
            if requires_registration(signer)
            else bt.calls.Utility.as_safe_derivative
        )
        expected_call = await client.compose(expected_derivative(index=21, call=child_remark))
        assert derived_call.data == expected_call.data
        derived = await accepted(
            client.submit_call(derived_call, signer, wait_for_finalization=True),
            "default-Falcon-derivative",
        )
        child = order_paths.event(derived, "System", "Remarked")["sender"]
        assert child != signer.ss58_address
        authority = await client.query(("HashedAccounts", "DerivedAuthorities"), [child])
        assert bool(authority) is requires_registration(signer)
        await fund(child, 2)
        spend = await compose_derivative(
            client._substrate, signer, 21, await client.compose(payment)
        )
        child_before = await account(client, child)
        await accepted(
            client.submit_call(spend, signer, wait_for_finalization=True),
            "default-Falcon-derivative-spends",
        )
        assert (await account(client, child))["data"]["free"] == (
            child_before["data"]["free"] - 10**9
        )

        # Explicit safe mode works for all four Falcon modes without a derived
        # registration reserve. Pure creation's normal deposit is refundable.
        reserve = (await account(client, signer.ss58_address))["data"]["reserved"]
        pure_receipt = await accepted(
            client.execute(
                bt.intents.CreatePureProxy(proxy_type="Any", index=5, account_mode="safe"),
                signer,
                wait_for_finalization=True,
            ),
            "create-safe-pure-proxy",
        )
        pure = order_paths.event(pure_receipt, "Proxy", "PureCreated")["pure"]
        assert await client.query(("HashedAccounts", "DerivedAuthorities"), [pure]) is None
        height, ext_index = map(int, pure_receipt.extrinsic_id.split("-"))
        await accepted(
            client.execute(
                bt.intents.KillPureProxy(
                    spawner_ss58=signer.ss58_address,
                    proxy_type="Any",
                    index=5,
                    height=height,
                    ext_index=ext_index,
                    account_mode="safe",
                ),
                signer,
                proxy_for=pure,
                proxy_type="Any",
                wait_for_finalization=True,
            ),
            "kill-safe-pure-proxy",
        )
        assert not (await client.query(bt.storage.Proxy.Proxies, [pure]))[0]
        assert (await account(client, signer.ss58_address))["data"]["reserved"] == reserve
