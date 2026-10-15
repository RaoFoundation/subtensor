"""Receiving addresses identify multisig members throughout every approval round."""

import pytest

from bittensor.executor import _compose_intent_call
from bittensor.intents.multisig import (
    MultisigApprove,
    MultisigCancel,
    MultisigExecute,
    MultisigIntentAdapter,
    MultisigThreshold1,
)
from bittensor.intents.transfer import Transfer
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair, ss58_decode, ss58_encode
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, BOB_HOT, dev_wallet

TIMEPOINT = {"height": 12, "index": 3}
CASES = [
    (MultisigThreshold1, {}, "as_multi_threshold_1"),
    (MultisigExecute, {"threshold": 2}, "as_multi"),
    (MultisigExecute, {"threshold": 2, "timepoint": TIMEPOINT}, "as_multi"),
    (MultisigApprove, {"threshold": 3}, "as_multi"),
    (MultisigApprove, {"threshold": 3, "timepoint": TIMEPOINT}, "approve_as_multi"),
    (MultisigCancel, {"threshold": 2, "timepoint": TIMEPOINT}, "cancel_as_multi"),
]


@pytest.mark.parametrize("code", [4, 6])
@pytest.mark.parametrize("intent_type,kwargs,function", CASES)
@pytest.mark.asyncio
async def test_direct_intents_normalize_receiving_signatories(code, intent_type, kwargs, function):
    substrate = FakeSubstrate()
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 100 * 10**9}})
    member = Keypair.create_from_seed(bytes([code]) * 32, code)
    spec = Transfer(BOB_HOT, 1).to_dict()
    canonical, expected_extras = await _compose_intent_call(
        substrate,
        intent_type(other_signatories=[member.ss58_address, BOB], call=spec, **kwargs),
        dev_wallet(),
    )
    received, extras = await _compose_intent_call(
        substrate,
        intent_type(other_signatories=[BOB, receiving_address(member)], call=spec, **kwargs),
        dev_wallet(),
    )
    assert received.function == function
    assert received.data == canonical.data
    assert received.call_hash == canonical.call_hash
    assert extras == expected_extras
    assert received.params["other_signatories"] == sorted(
        [BOB, member.ss58_address], key=lambda address: bytes(ss58_decode(address))
    )


@pytest.mark.parametrize("code", [4, 6])
@pytest.mark.asyncio
async def test_adapter_finds_subsequent_approval_with_receiving_signatories(code):
    substrate = FakeSubstrate()
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 100 * 10**9}})
    member = Keypair.create_from_seed(bytes([code]) * 32, code)
    semantic = Transfer(BOB_HOT, 1)
    inner = await semantic.build(substrate, dev_wallet())
    account = substrate.multisig_account([ALICE, BOB, member.ss58_address], 2)
    substrate.seed(
        "Multisig",
        "Multisigs",
        [account.ss58_address, inner.call_hash],
        {"when": TIMEPOINT, "approvals": [member.ss58_address]},
    )
    dispatch = MultisigExecute(
        threshold=2, other_signatories=[receiving_address(member), BOB], call=semantic.to_dict()
    )
    adapter = MultisigIntentAdapter(dispatch=dispatch, semantic=semantic)
    composed, _ = await _compose_intent_call(substrate, adapter, dev_wallet())
    assert composed.params["maybe_timepoint"] == TIMEPOINT
    assert composed.params["call"].data == inner.data


@pytest.mark.parametrize("code", [4, 6])
@pytest.mark.asyncio
async def test_signatory_aliases_do_not_inflate_threshold(code):
    member = Keypair.create_from_seed(bytes([code]) * 32, code)
    # These are three spellings of the same member, not three approvals.
    intent = MultisigApprove(
        threshold=3,
        other_signatories=[
            receiving_address(member),
            member.ss58_address,
            ss58_encode(bytes(member.public_key), 0),
        ],
        call=Transfer(BOB_HOT, 1).to_dict(),
    )
    with pytest.raises(ValueError, match="exceeds the signatory count"):
        await intent.build(FakeSubstrate(), dev_wallet())


@pytest.mark.parametrize("code", [4, 6])
@pytest.mark.asyncio
async def test_receiving_signatory_cannot_include_the_signer(code):
    signer = Keypair.create_from_seed(bytes([code]) * 32, code)
    intent = MultisigApprove(
        threshold=2,
        other_signatories=[receiving_address(signer), BOB],
        call=Transfer(BOB_HOT, 1).to_dict(),
    )
    with pytest.raises(ValueError, match="signer's own account"):
        await intent.build(FakeSubstrate(), signer)


@pytest.mark.parametrize("intent_type,kwargs,function", CASES)
@pytest.mark.asyncio
async def test_malformed_receiving_signatory_rejected(intent_type, kwargs, function):
    intent = intent_type(
        other_signatories=["bth1_invalid", BOB], call=Transfer(BOB_HOT, 1).to_dict(), **kwargs
    )
    with pytest.raises(ValueError):
        await intent.build(FakeSubstrate(), dev_wallet())
