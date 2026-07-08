"""Governance and account-level flows: sudo nesting, root claim type,
coldkey swap announcement, commitments via the raw-call escape hatch."""

from __future__ import annotations

from types import SimpleNamespace

import pytest
from bittensor.keyfiles import Keypair

import bittensor as sub
from bittensor.intents.coldkey import coldkey_hash
from tests.harness.samples import BOB

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_compose_nested_sudo_call(client, alice):
    """A runtime upgrade is Sudo.sudo(System.set_code(...)); prove the nesting
    with a reversible admin set through the public compose API."""
    before = int(await client.query(sub.storage.SubtensorModule.TxRateLimit))
    inner = await client.compose(
        sub.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=before + 1)
    )
    result = await client.submit_call(sub.calls.Sudo.sudo(call=inner), alice)
    after = int(await client.query(sub.storage.SubtensorModule.TxRateLimit))
    assert result.success, result.message
    assert after == before + 1


async def test_raw_call_escape_hatch_and_commitment(client, alice, owned_subnet):
    hot = alice.hotkey.ss58_address
    raw_call = sub.calls.Commitments.set_commitment(
        netuid=owned_subnet, info={"fields": [[{"Raw5": "0x" + b"hello".hex()}]]}
    )
    # An active policy refuses raw calls unless allow_raw_calls is set.
    with pytest.raises(sub.PolicyError):
        await client.submit_call(
            raw_call, alice, signer="hotkey", policy=sub.Policy(max_spend_tao=100.0)
        )
    result = await client.submit_call(raw_call, alice, signer="hotkey")
    commitment = await client.read("commitment", netuid=owned_subnet, hotkey_ss58=hot)
    assert result.success, result.message
    assert commitment is not None
    assert commitment.data == "hello"
    revealed = await client.read("revealed_commitment", netuid=owned_subnet, hotkey_ss58=hot)
    assert revealed is None  # nothing revealed yet


async def test_root_claim_type_variants(client, alice, owned_subnet):
    alice_cold = alice.coldkey.ss58_address

    result = await client.execute(sub.SetRootClaimType(claim_type="Swap"), alice)
    claim = await client.read("root_claim_type", coldkey_ss58=alice_cold)
    assert result.success, result.message
    assert claim["type"] == "Swap"
    assert claim["subnets"] is None

    result = await client.execute(
        sub.SetRootClaimType(claim_type="KeepSubnets", subnets=[owned_subnet]), alice
    )
    claim = await client.read("root_claim_type", coldkey_ss58=alice_cold)
    assert result.success, result.message
    assert claim["type"] == "KeepSubnets"
    assert claim["subnets"] == [owned_subnet]

    result = await client.execute(sub.SetRootClaimType(claim_type="Keep"), alice)
    claim = await client.read("root_claim_type", coldkey_ss58=alice_cold)
    assert result.success, result.message
    assert claim["type"] == "Keep"
    assert claim["subnets"] is None


async def test_keep_subnets_requires_subnets():
    with pytest.raises(ValueError):
        sub.SetRootClaimType(claim_type="KeepSubnets")


async def test_coldkey_swap_announcement_flow(client, alice):
    assert await client.read("coldkey_swap_announcement", coldkey_ss58=BOB) is None

    # A throwaway funded coldkey keeps this re-runnable — announcements are
    # per-account state that never resets on a long-lived chain.
    swapper = Keypair.create_from_mnemonic(Keypair.generate_mnemonic())
    swapper_wallet = SimpleNamespace(
        coldkey=swapper,
        coldkeypub=Keypair(ss58_address=swapper.ss58_address),
        hotkey=swapper,
    )
    new_cold = Keypair.create_from_mnemonic(Keypair.generate_mnemonic()).ss58_address
    await client.execute(sub.Transfer(dest_ss58=swapper.ss58_address, amount_tao=2.0), alice)

    result = await client.execute(
        sub.AnnounceColdkeySwap(new_coldkey_ss58=new_cold), swapper_wallet
    )
    announcement = await client.read("coldkey_swap_announcement", coldkey_ss58=swapper.ss58_address)
    assert result.success, result.message
    assert announcement is not None
    assert announcement["execute_block"] > 0
    assert announcement["new_coldkey_hash"] == coldkey_hash(new_cold)
    assert not announcement["disputed"]

    # Executing before the announcement delay has passed must be refused.
    result = await client.execute(
        sub.SwapColdkeyAnnounced(new_coldkey_ss58=new_cold), swapper_wallet
    )
    assert not result.success


async def test_key_association(client, alice, owned_subnet):
    result = await client.execute(sub.AssociateHotkey(hotkey_ss58=alice.hotkey.ss58_address), alice)
    assert result.success, result.message
    assert await client.read("associated_evm_key", netuid=owned_subnet, uid=0) is None
