"""Cancellation resolves the saved commitment rather than rebuilding an intent."""

from unittest.mock import AsyncMock

import pytest

from bittensor.intents import multisig
from bittensor.intents.multisig import MultisigCancel
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, dev_wallet

TIMEPOINT = {"height": 42, "index": 3}
COMMITTED_HASH = bytes([17]) * 32
ORIGINAL_CALL = {"op": "add_stake", "netuid": 1, "hotkey_ss58": BOB, "amount_tao": 1}


# Changed state must not prevent cancellation and release of the original approval deposit.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.asyncio
async def test_cancel_uses_original_pending_hash_without_rebuilding(monkeypatch):
    chain = FakeSubstrate()
    rebuild = AsyncMock(side_effect=AssertionError("original intent must not be rebuilt"))
    monkeypatch.setattr(multisig, "_compose_inner", rebuild)

    async def pending_pages():
        yield bytes([18]) * 32, {"when": {"height": 41, "index": 3}}
        yield COMMITTED_HASH, {"when": TIMEPOINT, "deposit": 1000, "depositor": ALICE}

    chain.query_map = AsyncMock(return_value=pending_pages())
    intent = MultisigCancel(2, [BOB], ORIGINAL_CALL, TIMEPOINT)
    canceled = await intent.build(chain, dev_wallet())
    account = chain.multisig_account([ALICE, BOB], 2)
    chain.query_map.assert_awaited_once_with("Multisig", "Multisigs", [account.ss58_address])
    assert canceled.function == "cancel_as_multi"
    assert canceled.params["call_hash"] == COMMITTED_HASH
    assert canceled.params["timepoint"] == TIMEPOINT
    rebuild.assert_not_awaited()


@pytest.mark.parametrize("mode", ["legacy", "safe"])
@pytest.mark.asyncio
async def test_explicit_commitment_does_not_need_an_original_intent_or_state(mode, monkeypatch):
    chain = FakeSubstrate()
    chain.query_map = AsyncMock(side_effect=AssertionError("unexpected lookup"))
    rebuild = AsyncMock(side_effect=AssertionError("unexpected rebuild"))
    monkeypatch.setattr(multisig, "_compose_inner", rebuild)
    intent = MultisigCancel(
        threshold=2,
        other_signatories=[BOB],
        timepoint=TIMEPOINT,
        call_hash="0x" + COMMITTED_HASH.hex(),
        account_mode=mode,
    )
    canceled = await intent.build(chain, dev_wallet())
    assert canceled.function == "cancel_as_multi" + ("_safe" if mode == "safe" else "")
    assert canceled.params["call_hash"] == COMMITTED_HASH
    assert canceled.params["timepoint"] == TIMEPOINT
    assert intent.summary() == "cancel multisig operation"
    chain.query_map.assert_not_awaited()
    rebuild.assert_not_awaited()


@pytest.mark.parametrize(
    "entries,message",
    [
        ([], "no pending multisig operation"),
        (
            [(COMMITTED_HASH, {"when": TIMEPOINT}), (bytes([18]) * 32, {"when": TIMEPOINT})],
            "multiple pending operations",
        ),
    ],
)
@pytest.mark.asyncio
async def test_cancellation_refuses_missing_or_ambiguous_commitments(entries, message):
    chain = FakeSubstrate()
    chain.seed_map("Multisig", "Multisigs", entries)
    with pytest.raises(ValueError, match=message):
        await MultisigCancel(2, [BOB], ORIGINAL_CALL, TIMEPOINT).build(chain, dev_wallet())
