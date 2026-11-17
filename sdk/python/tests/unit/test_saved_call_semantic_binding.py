"""Imported announcement bytes must agree with the operation shown for approval."""

from unittest.mock import AsyncMock, Mock

import pytest

from bittensor import calls
from bittensor._transport.contract import CallBytes
from bittensor.executor import Executor, _compose_intent_call
from bittensor.intents import AddStake, Batch, Transfer, TransferAll
from bittensor.intents.children import SetTake
from bittensor.intents.crowdloan import ContributeCrowdloan, CreateCrowdloan
from bittensor.intents.gamma import MintGamma, MintGammaWithStake, SetGammaDepositSpender
from bittensor.intents.multisig import MultisigApprove, MultisigExecute, MultisigThreshold1
from bittensor.intents.proxy import (
    ExecuteProxyAnnounced,
    _announced_decoded_call,
    compose_announceable_call,
)
from bittensor.sp_core import Keypair
from tests.conftest import golden_codec
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, BOB_HOT, dev_wallet


@pytest.fixture
def chain():
    substrate = FakeSubstrate()
    codec = golden_codec()
    substrate.compose = AsyncMock(side_effect=lambda call: codec.compose_call(*call))
    substrate.decode_scale = AsyncMock(side_effect=lambda _kind, data: codec.decode_call(data))
    substrate.seed_default("System", "Account", {"data": {"free": 10**12, "frozen": 0}})
    return substrate


@pytest.fixture
def gamma_chain():
    # The golden SCALE metadata predates Gamma. Keep production intent and
    # Rust binding policy while adapting only the absent codec entries.
    substrate = FakeSubstrate()
    substrate.seed_default("System", "Account", {"data": {"free": 10**12, "frozen": 0}})
    composed = {}
    original = substrate.compose

    async def compose(call):
        result = await original(call)
        composed[bytes(result.data)] = result
        return result

    async def decode(_kind, data):
        return await _announced_decoded_call(substrate, composed[bytes(data)])

    substrate.compose = AsyncMock(side_effect=compose)
    substrate.decode_scale = AsyncMock(side_effect=decode)
    return substrate


async def saved_announcement(chain, actual, declared=None):
    saved, call_hash, _ = await compose_announceable_call(chain, actual, ALICE, dev_wallet())
    semantic = actual if declared is None else declared
    return (
        ExecuteProxyAnnounced(
            delegate_ss58=BOB,
            real_ss58=ALICE,
            inner_op=semantic.op,
            inner_args=semantic.to_dict(),
            announced_call_data="0x" + bytes(saved.data).hex(),
            expected_call_hash=call_hash,
        ),
        saved,
    )


def crowdloan(inner):
    return CreateCrowdloan(1, 1, 10, 500, call=inner.to_dict())


def multisig(inner, wrapper):
    if wrapper == "threshold1":
        return MultisigThreshold1([BOB], inner.to_dict())
    if wrapper == "execute":
        return MultisigExecute(2, [BOB], inner.to_dict())
    if wrapper == "approve":
        return MultisigApprove(2, [BOB], inner.to_dict())
    return inner


def assert_saved(chain, call, saved):
    outer = chain.compose.await_args.args[0]
    assert bytes(outer.params["call"].data) == bytes(saved.data)
    decoded = golden_codec().decode_call(call.data)
    inner = next(arg["value"] for arg in decoded["call_args"] if arg["name"] == "call")
    assert inner["call_hash"] == "0x" + bytes(saved.call_hash).hex()


# Imported bytes must not spend more or send to a different account than approved.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize(
    "actual,declared",
    [
        (TransferAll(BOB), Transfer(BOB, 1)),
        (Transfer(BOB, 2), Transfer(BOB, 1)),
        (Transfer(ALICE, 1), Transfer(BOB, 1)),
        (
            Batch([Transfer(ALICE, 1), Transfer(BOB, 2)]),
            Batch([Transfer(BOB, 2), Transfer(ALICE, 1)]),
        ),
        (crowdloan(TransferAll(BOB)), crowdloan(Transfer(BOB, 1))),
    ],
)
async def test_saved_announcement_rejects_declared_mismatch_before_signing(
    chain, monkeypatch, actual, declared
):
    announced, _ = await saved_announcement(chain, actual, declared)
    signer = Mock(side_effect=AssertionError("mismatched operation reached private signing"))
    monkeypatch.setattr("bittensor.executor.resolve_signer", signer)
    with pytest.raises(ValueError, match="declared semantic intent"):
        await Executor(chain).execute(announced, dev_wallet(), shielded=False)
    signer.assert_not_called()
    assert not chain.submissions


@pytest.mark.parametrize("amount", [1, "all"])
async def test_saved_stake_retains_approved_price_and_resolved_amount(chain, amount):
    announced, saved = await saved_announcement(chain, AddStake(BOB_HOT, 1, amount))
    chain.seed_default("System", "Account", {"data": {"free": 20**12, "frozen": 0}})
    chain.runtime_call = AsyncMock(side_effect=AssertionError("saved stake was repriced"))
    call, _ = await _compose_intent_call(chain, announced, dev_wallet())
    assert_saved(chain, call, saved)
    chain.runtime_call.assert_not_awaited()


@pytest.mark.parametrize(
    "semantic",
    [
        Transfer(BOB, 1),
        TransferAll(BOB, keep_alive=False),
        Batch([Transfer(BOB, 1), Transfer(ALICE, 2)]),
        crowdloan(Transfer(BOB, 1)),
        ContributeCrowdloan(7, "all"),
        SetTake(100, hotkey_ss58=BOB_HOT),
    ],
)
async def test_saved_declared_calls_preserve_original_bytes(chain, semantic):
    announced, saved = await saved_announcement(chain, semantic)
    chain.seed_default("System", "Account", {"data": {"free": 20**12, "frozen": 0}})
    chain.seed("SubtensorModule", "Delegates", [BOB_HOT], 200)
    call, _ = await _compose_intent_call(chain, announced, dev_wallet())
    assert_saved(chain, call, saved)


async def test_saved_transfer_accepts_public_keypair_address_input(chain):
    announced, saved = await saved_announcement(chain, Transfer(BOB, 1))
    announced.inner_args = {"dest_ss58": Keypair.create_from_uri("//Bob"), "amount_tao": 1}
    call, _ = await _compose_intent_call(chain, announced, dev_wallet())
    assert_saved(chain, call, saved)


# Unknown or opaque saved calls cannot borrow a benign transfer description.
# PROTECTED_REGRESSION: changes require explicit human approval.
async def test_saved_announcement_rejects_opaque_incompatible_call(chain):
    saved = await chain.compose(calls.System.remark(remark=b"opaque"))
    announced = ExecuteProxyAnnounced(
        BOB,
        ALICE,
        "transfer",
        Transfer(BOB, 1).to_dict(),
        announced_call_data="0x" + bytes(saved.data).hex(),
        expected_call_hash="0x" + bytes(saved.call_hash).hex(),
    )
    with pytest.raises(ValueError, match="declared semantic intent"):
        await _compose_intent_call(chain, announced, dev_wallet())
    assert not chain.submissions


async def test_saved_binding_fallback_reconstructs_and_compares_independently(chain, monkeypatch):
    from bittensor import _saved_call

    announced, saved = await saved_announcement(chain, Transfer(BOB, 1))
    policy = _saved_call.evaluate_policy
    bindings = []

    def fallback(operation, value):
        if operation == "transaction.bind_saved_call":
            bindings.append(value)
            if "prepared" not in value:
                return {"verified": False}
        return policy(operation, value)

    monkeypatch.setattr(_saved_call, "evaluate_policy", fallback)
    call, _ = await _compose_intent_call(chain, announced, dev_wallet())
    assert len(bindings) == 2 and "prepared" in bindings[-1]
    assert isinstance(saved, CallBytes)
    assert_saved(chain, call, saved)


@pytest.mark.parametrize("staked", [False, True])
@pytest.mark.parametrize("wrapper", ["none", "threshold1", "execute", "approve"])
async def test_saved_gamma_service_checks_authority_without_repricing(gamma_chain, staked, wrapper):
    chain = gamma_chain
    chain.seed("SubtensorModule", "Services", [1, BOB], {"spender": ALICE})
    mint = (
        MintGammaWithStake(1, BOB_HOT, 1, ALICE, service_hotkey_ss58=BOB)
        if staked
        else MintGamma(1, BOB_HOT, 1, service_hotkey_ss58=BOB)
    )
    semantic = multisig(mint, wrapper)
    announced, saved = await saved_announcement(chain, semantic)
    chain.runtime_call = AsyncMock(side_effect=AssertionError("saved Gamma mint was repriced"))
    call, _ = await _compose_intent_call(chain, announced, dev_wallet())
    assert bytes(call.params["call"].data) == bytes(saved.data)
    assert mint.service_hotkey_ss58 == BOB and mint.spender_ss58 is None
    declared = announced.inner_args["call"] if wrapper != "none" else announced.inner_args
    assert declared["service_hotkey_ss58"] == BOB
    chain.runtime_call.assert_not_awaited()


# A service alias must not authorize the spender supplied by untrusted saved bytes.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("operation", ["mint", "stake", "set"])
@pytest.mark.parametrize("changed_service", [False, True])
@pytest.mark.parametrize("wrapper", ["none", "threshold1", "execute", "approve"])
async def test_saved_gamma_service_rejects_substituted_or_changed_spender(
    gamma_chain, operation, changed_service, wrapper
):
    chain = gamma_chain

    def intent(**spender):
        if operation == "mint":
            return MintGamma(1, BOB_HOT, 1, **spender)
        if operation == "stake":
            return MintGammaWithStake(1, BOB_HOT, 1, ALICE, **spender)
        return SetGammaDepositSpender(1, BOB_HOT, **spender)

    chain.seed("SubtensorModule", "Services", [1, BOB], {"spender": ALICE})
    declared = intent(service_hotkey_ss58=BOB)
    actual = declared if changed_service else intent(spender_ss58=BOB)
    declared = multisig(declared, wrapper)
    actual = multisig(actual, wrapper)
    announced, _ = await saved_announcement(chain, actual, declared)
    if changed_service:
        chain.seed("SubtensorModule", "Services", [1, BOB], {"spender": BOB})
    with pytest.raises(ValueError, match="declared semantic intent"):
        await _compose_intent_call(chain, announced, dev_wallet())
    assert not chain.submissions


# A nested saved Gamma mint cannot burn more TAO or stake than the declared amount.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("staked", [False, True])
@pytest.mark.parametrize("wrapper", ["threshold1", "execute", "approve"])
async def test_saved_multisig_gamma_rejects_changed_amount(gamma_chain, staked, wrapper):
    chain = gamma_chain
    chain.seed("SubtensorModule", "Services", [1, BOB], {"spender": ALICE})

    def intent(amount):
        mint = (
            MintGammaWithStake(1, BOB_HOT, amount, ALICE, service_hotkey_ss58=BOB)
            if staked
            else MintGamma(1, BOB_HOT, amount, service_hotkey_ss58=BOB)
        )
        return multisig(mint, wrapper)

    announced, _ = await saved_announcement(chain, intent(2), intent(1))
    with pytest.raises(ValueError, match="declared semantic intent"):
        await _compose_intent_call(chain, announced, dev_wallet())
    assert not chain.submissions
