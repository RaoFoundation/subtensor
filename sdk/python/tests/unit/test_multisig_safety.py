"""Safety invariants for automatic saved-multisig intent wrapping."""

from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest

from bittensor import Policy
from bittensor.cli import multisig_helpers
from bittensor.client import Client
from bittensor.executor import Executor
from bittensor.intents._money import UNBOUNDED
from bittensor.intents.multisig import (
    MultisigThreshold1,
    MultisigThreshold1IntentAdapter,
)
from bittensor.intents.registration import BurnedRegister
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, dev_wallet


@pytest.mark.asyncio
async def test_saved_multisig_preserves_inner_policy_and_mev_contract(monkeypatch):
    output = Mock()
    app_ctx = SimpleNamespace(
        wallet_name="treasury",
        wallet_path="/unused",
        wallet_given=True,
        multisig_wallet_name=None,
        output=output,
    )
    monkeypatch.setattr(multisig_helpers.cfg, "get_multisig", lambda name: {"name": name})
    monkeypatch.setattr(
        multisig_helpers,
        "resolve_multisig_preset",
        lambda _app_ctx, _preset: (1, [ALICE, BOB], ["alice", "bob"]),
    )
    monkeypatch.setattr(
        multisig_helpers,
        "pick_local_signatory",
        lambda _app_ctx, *, preset, signatories: ("alice", ALICE),
    )
    semantic = BurnedRegister(netuid=7, hotkey_ss58=ALICE_HOT)

    wrapped = multisig_helpers.wrap_intent_for_multisig_wallet(app_ctx, semantic)

    assert isinstance(wrapped, MultisigThreshold1IntentAdapter)
    assert wrapped.op == "multisig_threshold_1"
    assert wrapped.semantic_intent() is semantic
    assert wrapped.semantic_intent().mev_shield_default is True
    assert wrapped.semantic_intent().mev_shield_required is True
    assert wrapped.spend() is UNBOUNDED
    assert wrapped.touches_netuids() == [7]
    assert wrapped.affects_all_subnets() is False

    violations = Policy(max_spend_tao=1, allowed_netuids=[1]).check(wrapped, fee=None)
    assert any("cannot be bounded" in violation for violation in violations)
    assert any("netuid 7" in violation for violation in violations)

    plan = await Client("local", substrate=FakeSubstrate()).plan(
        wrapped,
        dev_wallet(),
        policy=Policy(max_spend_tao=1, allowed_netuids=[1]),
    )
    assert plan.spend is UNBOUNDED
    assert plan.violations == violations

    assert app_ctx.multisig_wallet_name == "treasury"
    assert app_ctx.wallet_name == "alice"


@pytest.mark.asyncio
async def test_required_mev_shield_survives_multisig_dispatch():
    semantic = BurnedRegister(netuid=7, hotkey_ss58=ALICE_HOT)
    dispatch = MultisigThreshold1(
        other_signatories=[BOB],
        call=semantic.to_dict(),
    )
    wrapped = MultisigThreshold1IntentAdapter(dispatch=dispatch, semantic=semantic)
    executor = Executor(Mock())
    expected = Mock()
    executor.submit_shielded = AsyncMock(return_value=expected)
    wallet = Mock()

    result = await executor.execute(wrapped, wallet)

    assert result is expected
    executor.submit_shielded.assert_awaited_once_with(
        wrapped,
        wallet,
        policy=None,
        wait_for_inclusion=True,
        wait_for_finalization=True,
    )
