"""Live reserve and whole-submission cost regressions on an isolated localnet.

Uses fresh accounts, production weights and normal slots. Alice only sponsors
these disposable accounts; one test sets its own delegate's free balance with
sudo to exercise the exact fee-affordability boundary. Global settings stay put.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

import pytest

import bittensor as bt
from bittensor.hashed import descriptor_storage
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from tests.e2e.test_mldsa_extended import Evidence
from tests.e2e.test_wallet_types import alice_submission
from tests.harness.samples import dev_wallet

ENDPOINT = os.getenv("E2E_WALLET_ENDPOINT")
pytestmark = [
    pytest.mark.asyncio,
    pytest.mark.skipif(not ENDPOINT, reason="requires an isolated activated Alice-root chain"),
]


def fresh_key(code):
    return Keypair.create_from_seed(os.urandom(32), code)


def money(rao):
    return bt.Balance.from_rao(rao)


async def account(client, signer):
    return await client.query(bt.storage.System.Account, [signer.ss58_address])


async def snapshot(client, signer):
    return {
        "account": await account(client, signer),
        "authority": await client.query(
            ("HashedAccounts", descriptor_storage(bytes(signer.hashed_descriptor))),
            [signer.ss58_address],
        ),
    }


def free(state):
    return int(state["account"]["data"]["free"])


def reserved(state):
    return int(state["account"]["data"]["reserved"])


def assert_signed(before, after, count):
    assert after["account"]["nonce"] == before["account"]["nonce"] + count
    if before["authority"] is None:
        assert after["authority"] is None
    else:
        assert after["authority"]["generation"] == before["authority"]["generation"] + count
        assert after["authority"]["descriptor"] == before["authority"]["descriptor"]


def attributes(events, module, name):
    return [
        entry["event"]["attributes"]
        for entry in events
        if (entry["event"]["module_id"], entry["event"]["event_id"]) == (module, name)
    ]


async def observe(client, report, label, result):
    assert result.block_hash and result.extrinsic_id, result.to_dict()
    index = int(result.extrinsic_id.rsplit("-", 1)[1])
    events = [
        entry
        for entry in await client._substrate.raw.get_events(result.block_hash)
        if entry.get("extrinsic_idx") == index
    ]
    report.add(label, result=result.to_dict(), canonical_events=events)
    assert result.success, result.to_dict()
    assert attributes(events, "System", "ExtrinsicSuccess"), events
    return events


async def setup(client):
    assert await client.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
    assert await client.query(("AdminUtils", "HashedAccountsEnabled")) is True
    assert await client.constant(("Timestamp", "MinimumPeriod")) == 6000
    assert await client._substrate.mev_next_key() is not None
    for name in ("RegistrationDeposit", "MlDsaRegistrationDeposit", "DerivedRegistrationDeposit"):
        assert int(await client.constant(("HashedAccounts", name))) == 200_000_000
    return 200_000_000


async def fund(client, report, signer, amount=100 * 10**9):
    async with alice_submission():
        result = await client.execute(
            bt.intents.Transfer(receiving_address(signer), money(amount)),
            dev_wallet(),
            shielded=False,
            max_registration_deposit=money(200_000_000),
            wait_for_finalization=True,
        )
    await observe(client, report, "Alice sponsors and funds fresh account", result)
    assert ((await snapshot(client, signer))["authority"] is not None) == (signer.crypto_type != 8)


async def quote_pair(client, intent, signer, **kwargs):
    """Compare independent quotes within one head to avoid block-boundary drift."""
    request = client._substrate.raw._session.request
    for _ in range(4):
        head = await request("chain_getBlockHash", [])
        plain = await client.plan(intent, signer, shielded=False, **kwargs)
        shielded = await client.plan(intent, signer, shielded=True, **kwargs)
        preview = await client.preflight(intent, signer, shielded=True, **kwargs)
        carrier = await client.estimate_shielded_carrier_fee(signer.public_only(), shielded.call)
        if head == await request("chain_getBlockHash", []):
            break
    else:
        pytest.fail("could not compare cost quotes at one stable chain head")
    assert plain.fee is not None and plain.fee.rao > 0
    assert carrier.rao > 0
    assert shielded.fee == plain.fee + carrier
    assert preview.estimated_fee == shielded.fee
    facts = dict(preview.facts)
    assert facts["Inner transaction fee (estimated)"] == str(plain.fee)
    assert facts["Shield carrier fee (estimated)"] == str(carrier)
    assert facts["Total transaction fee (estimated)"] == str(shielded.fee)
    return plain, shielded, preview, carrier


@pytest.mark.parametrize("code", [7, 8])
async def test_shield_actual_fees_match_cli_and_failed_transfer_balance_deltas(tmp_path, code):
    """The public result includes both paid extrinsics, even if the inner call fails."""
    wallet_path = str(tmp_path / "wallets")
    wallet = bt.Wallet("shield-fees", path=wallet_path)
    wallet.create_new_coldkey(crypto_type=code, use_password=False, suppress=True)
    wallet.create_new_hotkey(crypto_type=code, suppress=True)
    owner = wallet.coldkey
    recipient = fresh_key(1)
    report = Evidence(f"shield-actual-fees-{code}")

    async with bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client:
        await fund(client, report, owner, amount=10 * 10**9)
        before = await snapshot(client, owner)
        recipient_before = int((await account(client, recipient))["data"]["free"])
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-m",
            "bittensor.cli.main",
            "--yes",
            "--json",
            "--mev-shield",
            "--network",
            ENDPOINT,
            "--fallback-endpoints",
            "none",
            "--archive-endpoints",
            "none",
            "--wallet-path",
            wallet_path,
            "--wallet",
            wallet.name,
            "wallet",
            "transfer",
            "--dest",
            recipient.ss58_address,
            "--amount",
            "1",
            env={**os.environ, "BTCLI_CONFIG": str(tmp_path / "config.json")},
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            out, err = await asyncio.wait_for(process.communicate(), 180)
        except BaseException:
            process.kill()
            await process.wait()
            raise
        assert process.returncode == 0, out.decode() + err.decode()
        result = json.loads(out)
        after = await snapshot(client, owner)
        assert result["success"] and result["data"]["shielded"], result
        data = result["data"]
        total = data["shield_carrier_fee_rao"] + data["shield_inner_fee_rao"]
        assert data["shield_carrier_fee_rao"] > 0 and data["shield_inner_fee_rao"] > 0
        assert money(total).tao == result["fee_tao"]
        assert free(before) - free(after) == 10**9 + total
        assert int((await account(client, recipient))["data"]["free"]) - recipient_before == 10**9
        assert_signed(before, after, 2)
        report.add("CLI successful Shield transfer", result=result, before=before, after=after)

        # Raw submission deliberately bypasses transfer preflight to include an
        # unaffordable inner call. Both envelopes pay fees, but no value moves.
        before = await snapshot(client, owner)
        recipient_before = int((await account(client, recipient))["data"]["free"])
        failed = await client.submit_call(
            bt.calls.Balances.transfer_allow_death(dest=recipient.ss58_address, value=20 * 10**9),
            wallet,
            shielded=True,
            wait_for_finalization=True,
        )
        after = await snapshot(client, owner)
        assert not failed.success and failed.data["shielded"], failed.to_dict()
        assert failed.error.name == "FundsUnavailable", failed.to_dict()
        assert failed.fee.rao == free(before) - free(after)
        assert failed.fee.rao == (
            failed.data["shield_carrier_fee_rao"] + failed.data["shield_inner_fee_rao"]
        )
        assert all(
            failed.data[name] > 0 for name in ("shield_carrier_fee_rao", "shield_inner_fee_rao")
        )
        assert attributes(failed.events, "System", "ExtrinsicFailed")
        assert int((await account(client, recipient))["data"]["free"]) == recipient_before
        assert_signed(before, after, 2)
        report.add(
            "included failed Shield transfer", result=failed.to_dict(), before=before, after=after
        )


@pytest.mark.parametrize("code", [7])
@pytest.mark.parametrize("count", [1, 2])
async def test_protected_pure_reserves_policy_and_kill_lifecycle(code, count):
    owner = fresh_key(code)
    report = Evidence(f"release-pure-reserves-{code}-{count}")
    async with (
        asyncio.timeout(1200),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        deposit = await setup(client)
        refundable = sum(
            [
                int(await client.constant(("Proxy", name)))
                for name in ("ProxyDepositBase", "ProxyDepositFactor")
            ]
        )
        minimum = int(await client.constant(("Balances", "ExistentialDeposit")))
        await fund(client, report, owner)
        children = [bt.intents.CreatePureProxy(proxy_type="Any", index=i) for i in range(count)]
        intent = children[0] if count == 1 else bt.intents.Batch(children)
        before = await snapshot(client, owner)
        plain, shield, preview, _ = await quote_pair(client, intent, owner)
        automatic = await client.plan(intent, owner)
        assert automatic.shielded and automatic.fee == shield.fee
        assert plain.spend == shield.spend == money(count * deposit)
        assert shield.extras["protected_pure_registration_deposit_rao"] == count * deposit
        assert shield.extras["pure_proxy_deposit_rao"] == count * refundable
        assert shield.extras["pure_proxy_reserves"] == {
            owner.ss58_address: count * (deposit + refundable)
        }
        assert preview.required_free == money(count * (deposit + refundable) + minimum) + shield.fee
        assert any("killing the proxy does not refund" in effect for effect in shield.effects)

        rejected = bt.Policy(max_spend_tao=money(count * deposit - 1))
        blocked = await client.plan(intent, owner, policy=rejected)
        assert not blocked.ok and any("max_spend_tao" in v for v in blocked.violations)
        with pytest.raises(bt.PolicyError, match="max_spend_tao"):
            await client.execute(intent, owner, policy=rejected, wait_for_finalization=True)
        assert await snapshot(client, owner) == before
        report.add("permanent reserve enforced before signing", rejected=blocked.to_dict())

        # Both direct plaintext and batched Shield routes use the exact same
        # reserve policy. Killing a pure proxy only releases its proxy deposit.
        use_shield = count == 2
        result = await client.execute(
            intent,
            owner,
            shielded=use_shield,
            policy=bt.Policy(max_spend_tao=money(count * deposit)),
            wait_for_finalization=True,
        )
        events = await observe(client, report, "protected pure creation", result)
        created = attributes(events, "Proxy", "PureCreated")
        assert len(created) == count and len({event["pure"] for event in created}) == count
        after = await snapshot(client, owner)
        assert reserved(after) == reserved(before) + count * (deposit + refundable)
        assert_signed(before, after, 2 if use_shield else 1)
        assert bool(result.data.get("shielded")) is use_shield
        height, ext_index = map(int, result.extrinsic_id.split("-"))
        for offset, event in enumerate(created):
            pure = event["pure"]
            derived = await client.query(("HashedAccounts", "DerivedAuthorities"), [pure])
            assert derived is not None
            proxies = await client.query(("Proxy", "Proxies"), [pure])
            assert len(proxies[0]) == 1 and int(proxies[1]) == refundable
            killed = await client.execute(
                bt.intents.KillPureProxy(
                    spawner_ss58=owner.ss58_address,
                    proxy_type="Any",
                    index=int(event["disambiguation_index"]),
                    height=height,
                    ext_index=ext_index,
                ),
                owner,
                proxy_for=pure,
                proxy_type="Any",
                shielded=False,
                wait_for_finalization=True,
            )
            await observe(client, report, "kill releases only refundable proxy deposit", killed)
            current = await snapshot(client, owner)
            assert reserved(current) == reserved(after) - (offset + 1) * refundable
            assert await client.query(("HashedAccounts", "DerivedAuthorities"), [pure]) == derived
            assert (await client.query(("Proxy", "Proxies"), [pure]))[0] == []
        final = await snapshot(client, owner)
        assert reserved(final) == reserved(before) + count * deposit
        report.add("permanent registration reserve retained", before=before, after=final)


@pytest.mark.parametrize("code", [4, 6, 7, 8])
async def test_automatic_and_explicit_shield_total_fee_caps_and_receipts(code):
    sender, recipient = fresh_key(code), fresh_key(code)
    report = Evidence(f"release-shield-total-cost-{code}")
    async with (
        asyncio.timeout(1200),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        deposit = await setup(client)
        if code == 8:
            deposit = 0
        await fund(client, report, sender)
        intent = bt.intents.Transfer(receiving_address(recipient), 1)
        before, recipient_before = await snapshot(client, sender), await snapshot(client, recipient)
        plain, shield, _, carrier = await quote_pair(client, intent, sender)
        automatic = await client.plan(intent, sender)
        assert automatic.shielded == (code != 8)
        assert automatic.fee == (shield.fee if code != 8 else plain.fee)
        cap = plain.fee + money(carrier.rao // 2)
        assert plain.fee <= cap < shield.fee
        policy = bt.Policy(max_fee_tao=cap)
        allowed_plain = await client.plan(intent, sender, shielded=False, policy=policy)
        assert allowed_plain.ok and not allowed_plain.shielded
        for mode in (None, True) if code != 8 else (True,):
            blocked = await client.plan(intent, sender, shielded=mode, policy=policy)
            assert not blocked.ok and any("max_fee_tao" in v for v in blocked.violations)
            with pytest.raises(bt.PolicyError, match="max_fee_tao"):
                await client.execute(
                    intent,
                    sender,
                    shielded=mode,
                    policy=policy,
                    max_registration_deposit=money(deposit),
                    wait_for_finalization=True,
                )
            assert await snapshot(client, sender) == before
            assert await snapshot(client, recipient) == recipient_before
        report.add(
            "automatic and explicit caps include both fees before signing",
            plain=plain.to_dict(),
            shield=shield.to_dict(),
            fee_cap_rao=cap.rao,
        )
        funded = await client.execute(
            intent,
            sender,
            shielded=True if code == 8 else None,
            policy=bt.Policy(max_fee_tao=money(shield.fee.rao * 2)),
            max_registration_deposit=money(deposit),
            wait_for_finalization=True,
        )
        events = await observe(client, report, "automatic Shield registers and transfers", funded)
        assert funded.data.get("shielded") is True
        after = await snapshot(client, sender)
        assert_signed(before, after, 2)
        assert reserved(after) == reserved(before) + deposit
        received = await snapshot(client, recipient)
        assert (received["authority"] is not None) == (code != 8)
        assert free(received) == 10**9
        fee_events = attributes(events, "TransactionPayment", "TransactionFeePaid")
        assert len(fee_events) == 1
        actual_total = free(before) - free(after) - deposit - 10**9
        assert actual_total > int(fee_events[0]["actual_fee"]) > 0
        report.add("both real fees charged", aggregate_fee_rao=actual_total, account_after=after)

        # After registration, the default no longer needs Shield, while an
        # explicit Shield request still quotes and executes both transactions.
        regular = await client.plan(intent, sender)
        assert not regular.shielded
        _, explicit, _, _ = await quote_pair(client, intent, sender)
        second = await client.execute(
            intent,
            sender,
            shielded=True,
            policy=bt.Policy(max_fee_tao=money(explicit.fee.rao * 2)),
            wait_for_finalization=True,
        )
        await observe(client, report, "explicit Shield pays registered recipient", second)
        final = await snapshot(client, sender)
        assert second.data.get("shielded") is True
        assert_signed(after, final, 2)
        assert reserved(final) == reserved(after)
        assert free(await snapshot(client, recipient)) == 2 * 10**9


@pytest.mark.parametrize("code", [7])
async def test_proxy_delegate_must_afford_both_fees_independently_of_reserve_owner(code):
    owner, delegate = fresh_key(code), fresh_key(code)
    report = Evidence(f"release-pure-delegate-fees-{code}")
    async with (
        asyncio.timeout(1200),
        bt.Client(ENDPOINT, fallback_endpoints=[], archive_endpoints=[]) as client,
    ):
        deposit = await setup(client)
        await fund(client, report, owner)
        await fund(client, report, delegate)
        authorized = await client.execute(
            bt.intents.AddProxy(delegate.ss58_address, proxy_type="Any"),
            owner,
            shielded=False,
            wait_for_finalization=True,
        )
        await observe(client, report, "authorize funded PQ delegate", authorized)
        intent = bt.intents.CreatePureProxy(proxy_type="Any")
        proxy = {"proxy_for": owner.ss58_address, "proxy_type": "Any"}
        plain, shield, _, carrier = await quote_pair(client, intent, delegate, **proxy)
        minimum = int(await client.constant(("Balances", "ExistentialDeposit")))
        delegate_free = minimum + plain.fee.rao + carrier.rao // 2
        assert minimum + plain.fee.rao <= delegate_free < minimum + shield.fee.rao
        async with alice_submission():
            balance_set = await client.submit_call(
                bt.calls.Sudo.sudo(
                    call=await client.compose(
                        bt.calls.Balances.force_set_balance(
                            who=delegate.ss58_address, new_free=delegate_free
                        )
                    )
                ),
                dev_wallet(),
                wait_for_finalization=True,
            )
        await observe(
            client, report, "set disposable delegate at combined-fee boundary", balance_set
        )
        owner_before, delegate_before = (
            await snapshot(client, owner),
            await snapshot(client, delegate),
        )
        assert free(delegate_before) == delegate_free
        ordinary = await client.plan(intent, delegate, shielded=False, **proxy)
        assert ordinary.ok
        for mode in (None, True):
            blocked = await client.plan(intent, delegate, shielded=mode, **proxy)
            assert not blocked.ok, blocked.to_dict()
            with pytest.raises(bt.PolicyError, match=r"(?i)carrier|fee|balance"):
                await client.execute(
                    intent, delegate, shielded=mode, wait_for_finalization=True, **proxy
                )
            assert await snapshot(client, owner) == owner_before
            assert await snapshot(client, delegate) == delegate_before
        report.add(
            "delegate fee shortfall refuses before signing despite funded reserve owner",
            owner=owner_before,
            delegate=delegate_before,
            plain_fee_rao=plain.fee.rao,
            shield_fee_rao=shield.fee.rao,
        )
        await fund(client, report, delegate, amount=10 * 10**9)
        delegate_ready = await snapshot(client, delegate)
        result = await client.execute(intent, delegate, wait_for_finalization=True, **proxy)
        await observe(
            client, report, "funded delegate pays fees while owner supplies reserves", result
        )
        assert result.data.get("shielded") is True
        owner_after, delegate_after = (
            await snapshot(client, owner),
            await snapshot(client, delegate),
        )
        refundable = sum(
            [
                int(await client.constant(("Proxy", name)))
                for name in ("ProxyDepositBase", "ProxyDepositFactor")
            ]
        )
        assert reserved(owner_after) == reserved(owner_before) + deposit + refundable
        assert free(owner_before) - free(owner_after) == deposit + refundable
        assert owner_after["authority"] == owner_before["authority"]
        assert owner_after["account"]["nonce"] == owner_before["account"]["nonce"]
        assert reserved(delegate_after) == reserved(delegate_ready)
        assert_signed(delegate_ready, delegate_after, 2)
        assert free(delegate_after) < free(delegate_ready)
