"""Multisig: pending-op lifecycle and the M-of-N account object."""

from __future__ import annotations

import pytest
from bittensor.keyfiles import Keypair

import bittensor as sub
from tests.harness.samples import BOB, dev_wallet

pytestmark = pytest.mark.asyncio(loop_scope="session")


async def test_pending_op_open_read_cancel(client, alice):
    """2-of-2 (Alice, Bob): open the op (first approval), read it back through
    the Multisigs map and the typed read, then cancel it.

    Re-runnable: a prior interrupted run may have left the op pending, so the
    open result bool is not asserted — only that a pending op exists and that
    cancel clears it.
    """
    alice_cold = alice.coldkey.ss58_address
    inner = {"op": "transfer", "dest_ss58": BOB, "amount_tao": 0.1}

    await client.execute(
        sub.MultisigExecute(threshold=2, other_signatories=[BOB], call=inner), alice
    )
    entries = await client.query_map(sub.storage.Multisig.Multisigs)
    mine = [(k, v) for k, v in entries if str((v or {}).get("depositor")) == alice_cold]
    assert len(mine) >= 1, "no pending multisig op with Alice as depositor"

    (ms_account, call_hash), value = mine[0]
    read_back = await client.read(
        "multisig", account_ss58=str(ms_account), call_hash=str(call_hash)
    )
    assert read_back is not None
    assert alice_cold in read_back["approvals"]

    timepoint = {
        "height": int(value["when"]["height"]),
        "index": int(value["when"]["index"]),
    }
    result = await client.execute(
        sub.MultisigCancel(threshold=2, other_signatories=[BOB], call=inner, timepoint=timepoint),
        alice,
    )
    entries = await client.query_map(sub.storage.Multisig.Multisigs)
    still = [k for k, v in entries if str((v or {}).get("depositor")) == alice_cold]
    assert result.success, result.message
    assert not still, f"{len(still)} pending ops left after cancel"


async def test_account_object_m_of_n_executes(client, alice, bob):
    """2-of-3 over Alice/Bob/Dave: the first approval records consent, the
    threshold-reaching approval executes the inner call."""
    dave = dev_wallet("//Dave", "//Dave//hot")
    ms = await client.multisig(
        [
            alice.coldkey.ss58_address,
            bob.coldkey.ss58_address,
            dave.coldkey.ss58_address,
        ],
        threshold=2,
    )
    assert ms.threshold == 2
    assert ms.address.startswith("5")

    await client.execute(sub.Transfer(dest_ss58=ms.address, amount_tao=20.0), alice)

    # A fresh recipient keeps the run re-runnable (no prior balance).
    recipient = Keypair.create_from_mnemonic(Keypair.generate_mnemonic()).ss58_address
    payout = sub.calls.Balances.transfer_keep_alive(dest=recipient, value=5 * 10**9)

    before = await client.balances.get(recipient)
    first = await ms.approve(payout, alice)
    mid = await client.balances.get(recipient)
    assert first.success, first.message
    assert mid.rao == before.rao, "first approval must not execute"

    second = await ms.approve(payout, bob)
    after = await client.balances.get(recipient)
    assert second.success, second.message
    assert after.rao - before.rao == 5 * 10**9
