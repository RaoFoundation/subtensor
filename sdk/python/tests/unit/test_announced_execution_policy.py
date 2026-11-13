"""Delayed execution keeps the inner operation's MEV protection contract."""

from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock

import pytest
import typer

from bittensor.cli import context
from bittensor.cli.output import Output
from bittensor.client import Client
from bittensor.executor import Executor
from bittensor.fee_filters import _call_dict
from bittensor.intents import AddCollateral, AddStake, Batch, BurnedRegister, Transfer
from bittensor.intents.multisig import (
    MultisigExecute,
    MultisigIntentAdapter,
    MultisigThreshold1,
)
from bittensor.intents.proxy import ExecuteProxyAnnounced, compose_announceable_call
from tests.harness.fake_substrate import FakeSubstrate, success_result
from tests.harness.samples import ALICE, BOB, BOB_HOT, dev_wallet


@pytest.fixture
def policy_context(tmp_path, monkeypatch):
    for variable in (
        "BTCLI_CONFIG",
        "BTCLI_PROXIES_PATH",
        "BTCLI_ADDRESSES_PATH",
        "BTCLI_MULTISIGS_PATH",
        "BTCLI_MULTISIG_CACHE",
        "BTCLI_SUBNET_NAMES_CACHE",
        "BTCLI_TOKEN_SYMBOLS_CACHE",
        "BTCLI_PROTECTED_PATH",
        "BTCLI_PROXY_ANNOUNCEMENTS_PATH",
    ):
        monkeypatch.setenv(variable, str(tmp_path / f"{variable}.json"))
    chain = FakeSubstrate()
    chain.seed_default("System", "Account", {"data": {"free": 10**12, "frozen": 0}})
    chain.mev_key = bytes(1184)
    chain.seed("MevShield", "NextKey", [], chain.mev_key)
    monkeypatch.setattr(context, "Client", lambda network, **kw: Client(network, substrate=chain))
    monkeypatch.setattr(context.AppContext, "wallet", lambda self: dev_wallet())
    encrypted = AsyncMock(return_value=success_result())
    monkeypatch.setattr(Executor, "_submit_encrypted_call", encrypted)
    app_ctx = context.AppContext(
        "local",
        "alice",
        "default",
        str(tmp_path),
        True,
        False,
        Output(json_mode=True),
        wallet_given=True,
    )
    return app_ctx, chain, encrypted


def _required_inner(operation, nesting):
    inner = (
        AddCollateral(netuid=1, hotkey_ss58=BOB_HOT, amount_alpha=5)
        if operation == "collateral"
        else BurnedRegister(netuid=1, hotkey_ss58=BOB_HOT)
    )
    if nesting == "batch":
        return Batch(intents=[Transfer(BOB, 1), inner])
    if nesting == "multisig":
        return MultisigThreshold1(other_signatories=[ALICE], call=inner.to_dict())
    return inner


async def _announced(chain, inner, saved):
    spec = inner.to_dict()
    operation = spec.pop("op")
    payload = None
    if saved:
        call, _call_hash, _extras = await compose_announceable_call(chain, inner, BOB, dev_wallet())
        payload = "0x" + bytes(call.data).hex()

        async def decode(type_name, data):
            assert type_name == "Call"
            assert bytes(data) == bytes(call.data)
            return _call_dict(call)

        # FakeSubstrate intentionally has no codec; retain the exact composed
        # payload and seed only its decode for registration-policy traversal.
        chain.decode_scale = AsyncMock(side_effect=decode)
    return ExecuteProxyAnnounced(
        delegate_ss58=ALICE,
        real_ss58=BOB,
        inner_op=operation,
        inner_args=spec,
        force_proxy_type="Any",
        announced_call_data=payload,
    )


# Collateral and burned-registration AMM buys must never enter the public mempool.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("operation", ["collateral", "registration"])
@pytest.mark.parametrize("nesting", ["direct", "batch", "multisig"])
@pytest.mark.parametrize("saved", [False, True])
def test_cli_refuses_disabling_required_shield_for_announced_execution(
    policy_context, operation, nesting, saved
):
    app_ctx, chain, encrypted = policy_context
    intent = asyncio.run(_announced(chain, _required_inner(operation, nesting), saved))
    app_ctx.mev_shield = False

    with pytest.raises(typer.Exit) as error:
        app_ctx.submit(intent, proxy_for="self")

    assert error.value.exit_code == 2
    assert chain.submissions == []
    encrypted.assert_not_awaited()


# Required Shield survives delayed replay and an explicit SDK public-submission request.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("operation", ["collateral", "registration"])
@pytest.mark.parametrize("nesting", ["direct", "batch", "multisig", "saved-multisig"])
@pytest.mark.parametrize("saved", [False, True])
async def test_sdk_keeps_required_shield_and_announced_envelope(
    policy_context, operation, nesting, saved
):
    _app_ctx, chain, encrypted = policy_context
    announced = await _announced(chain, _required_inner(operation, nesting), saved)
    intent = announced
    if nesting == "saved-multisig":
        intent = MultisigIntentAdapter(
            dispatch=MultisigExecute(
                threshold=2, other_signatories=[BOB], call=announced.to_dict()
            ),
            semantic=announced,
        )

    result = await Executor(chain).execute(intent, dev_wallet(), shielded=False)

    assert result.success
    assert chain.submissions == []
    encrypted.assert_awaited_once()
    call = encrypted.await_args.args[0]
    if nesting == "saved-multisig":
        assert (call.module, call.function) == ("Multisig", "as_multi")
        call = call.params["call"]
    assert (call.module, call.function) == ("Proxy", "proxy_announced")
    assert call.params["delegate"] == ALICE
    assert call.params["real"] == BOB
    if saved:
        assert bytes(call.params["call"].data) == bytes.fromhex(
            announced.announced_call_data.removeprefix("0x")
        )
    else:
        expected = {"direct": "SubtensorModule", "batch": "Utility", "multisig": "Multisig"}
        assert call.params["call"].module == expected.get(nesting, "SubtensorModule")


@pytest.mark.parametrize("saved", [False, True])
@pytest.mark.parametrize(
    "operation,override,expected_shield",
    [
        ("stake", None, True),
        ("stake", False, False),
        ("stake", True, True),
        ("transfer", True, True),
    ],
)
def test_cli_announced_staking_default_and_explicit_overrides(
    policy_context, monkeypatch, saved, operation, override, expected_shield
):
    app_ctx, chain, encrypted = policy_context
    inner = (
        AddStake(hotkey_ss58=BOB_HOT, netuid=1, amount_tao=5)
        if operation == "stake"
        else Transfer(BOB_HOT, 5)
    )
    intent = asyncio.run(_announced(chain, inner, saved))
    app_ctx.mev_shield = override
    reviews = []
    monkeypatch.setattr(app_ctx, "_transaction_card", lambda *args, **kw: reviews.append(kw))

    result = app_ctx.submit(intent, proxy_for="self")

    assert result is not None and result.success
    assert len(reviews) == 1 and reviews[0]["shield"] is expected_shield
    assert encrypted.await_count == int(expected_shield)
    assert len(chain.submissions) == int(not expected_shield)
    call = encrypted.await_args.args[0] if expected_shield else chain.submissions[0][0]
    assert (call.module, call.function) == ("Proxy", "proxy_announced")
    if saved:
        assert bytes(call.params["call"].data) == bytes.fromhex(
            intent.announced_call_data.removeprefix("0x")
        )


def test_saved_payload_requires_known_inner_policy():
    with pytest.raises(ValueError, match="Unknown op"):
        ExecuteProxyAnnounced(
            delegate_ss58=ALICE,
            real_ss58=BOB,
            inner_op="unknown_policy",
            inner_args={},
            announced_call_data="0x01020304",
        )
