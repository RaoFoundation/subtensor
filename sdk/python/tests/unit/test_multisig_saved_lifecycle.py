"""Direct multisig intents preserve committed staking calls across approvals."""

import json
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor._transport.contract import CallBytes
from bittensor.intents.multisig import MultisigApprove, MultisigExecute
from bittensor.sp_core import Keypair
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, dev_wallet


def codec_call(module, function, params):
    def encode(value):
        if isinstance(value, CallBytes):
            return json.loads(value.data)
        if isinstance(value, bytes):
            return "0x" + value.hex()
        raise TypeError(type(value).__name__)

    return CallBytes(
        json.dumps(
            {
                "call_module": module,
                "call_function": function,
                "call_args": [
                    {"name": key, "value": value} for key, value in sorted(params.items())
                ],
            },
            default=encode,
            sort_keys=True,
        ).encode(),
        478,
    )


def setup_chain():
    chain = FakeSubstrate()
    chain.compose = AsyncMock(side_effect=lambda call: codec_call(*call))
    chain.decode_scale = AsyncMock(side_effect=lambda _name, data: json.loads(data))
    chain.seed_runtime("SwapRuntimeApi", "current_alpha_price", 1_000_000_000)
    for address in (ALICE, BOB):
        chain.seed("System", "Account", [address], {"data": {"free": 10**12, "frozen": 0}})
    return chain


SPEC = {"op": "add_stake", "hotkey_ss58": BOB, "netuid": 1, "amount_tao": 1}
WHEN = {"height": 42, "index": 3}


# Changed pool prices must not strand an approved operation or its deposit.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("mode", ["legacy", "safe"])
@pytest.mark.parametrize("kind", [MultisigApprove, MultisigExecute])
@pytest.mark.asyncio
async def test_saved_staking_call_survives_price_change_with_a_different_signer(mode, kind):
    chain = setup_chain()
    opening = await MultisigApprove(2, [BOB], SPEC, account_mode=mode).build(chain, dev_wallet())
    saved = opening.extras["multisig_call_data"]
    original_hash = opening.extras["multisig_call_hash"]
    account = chain.multisig_account([ALICE, BOB], 2, account_mode=mode)
    chain.seed(
        "Multisig",
        "Multisigs",
        [account.ss58_address, bytes.fromhex(original_hash[2:])],
        {"when": WHEN, "approvals": [ALICE], "deposit": 2000},
    )
    chain.seed_runtime("SwapRuntimeApi", "current_alpha_price", 2_000_000_000)
    bob = Keypair.create_from_uri("//Bob")
    wallet = SimpleNamespace(coldkey=bob, coldkeypub=bob, hotkey=bob)
    followup = await kind(
        2,
        [ALICE],
        SPEC,
        timepoint=WHEN,
        account_mode=mode,
        call_data=saved,
        call_hash=original_hash,
    ).build(chain, wallet)
    assert followup.extras["multisig_call_hash"] == original_hash
    assert followup.extras["multisig_call_data"] == saved
    decoded = json.loads(followup.call.data)
    params = {row["name"]: row["value"] for row in decoded["call_args"]}
    assert params["maybe_timepoint"] == WHEN
    if kind is MultisigApprove:
        assert params["call_hash"] == original_hash
    else:
        assert params["call"] == json.loads(bytes.fromhex(saved[2:]))


@pytest.mark.asyncio
async def test_repriced_unpinned_call_is_rejected_before_signing():
    chain = setup_chain()
    opening = await MultisigApprove(2, [BOB], SPEC).build(chain, dev_wallet())
    account = chain.multisig_account([ALICE, BOB], 2)
    original_hash = bytes.fromhex(opening.extras["multisig_call_hash"][2:])
    chain.seed("Multisig", "Multisigs", [account.ss58_address, original_hash], {"when": WHEN})
    chain.seed_runtime("SwapRuntimeApi", "current_alpha_price", 2_000_000_000)
    with pytest.raises(ValueError, match="original multisig call_data"):
        await MultisigExecute(2, [BOB], SPEC, timepoint=WHEN).build(chain, dev_wallet())
    assert not chain.submissions


@pytest.mark.asyncio
async def test_saved_call_cannot_change_an_explicit_payment_or_hash():
    chain = setup_chain()
    opening = await MultisigApprove(2, [BOB], SPEC).build(chain, dev_wallet())
    saved = opening.extras["multisig_call_data"]
    with pytest.raises(ValueError, match="verified intent and recipient guards"):
        await MultisigExecute(2, [BOB], {**SPEC, "amount_tao": 2}, call_data=saved).build(
            chain, dev_wallet()
        )
    with pytest.raises(ValueError, match="original commitment"):
        await MultisigExecute(2, [BOB], SPEC, call_data=saved, call_hash="0x" + "ff" * 32).build(
            chain, dev_wallet()
        )
