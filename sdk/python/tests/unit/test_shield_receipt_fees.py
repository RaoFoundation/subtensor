"""Shield receipts report the actual total, including charged failures."""

from dataclasses import replace
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from bittensor._substrate import RpcSubstrate
from bittensor._transport.contract import InclusionReport
from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.executor import _FeePeriodView
from bittensor.intents import Transfer
from bittensor.intents.base import IntentPreflight
from bittensor.result import ChainError
from bittensor.settings import DEFAULT_ERA_PERIOD
from bittensor.sp_core import Keypair, ss58_decode, ss58_encode
from tests.harness.fake_substrate import FakeSubstrate, success_result
from tests.harness.samples import BOB, dev_wallet


async def test_intent_fee_view_preserves_signer_identity_without_replacing_another_payer():
    signer = Keypair.create_from_seed(bytes([92]) * 32, 8).public_only()
    substrate = FakeSubstrate()
    substrate.estimate_fee = AsyncMock(return_value=Balance.from_rao(100))
    view = _FeePeriodView(substrate, 8, signer)
    same_account = SimpleNamespace(
        ss58_address=ss58_encode(bytes(ss58_decode(signer.ss58_address)), 0), crypto_type=1
    )
    other_account = dev_wallet().coldkeypub
    await view.estimate_fee("call", same_account)
    await view.estimate_fee("call", other_account)
    assert substrate.estimate_fee.await_args_list[0].args == ("call", signer)
    assert substrate.estimate_fee.await_args_list[1].args == ("call", other_account)


@pytest.mark.parametrize("period", ["default", None, 8, 256])
async def test_rpc_fee_quote_uses_the_selected_signing_era(period):
    raw = SimpleNamespace(get_payment_info=AsyncMock(return_value={"partial_fee": 123}))
    substrate = RpcSubstrate("ws://127.0.0.1:9944")

    async def read(operation):
        return await operation(raw)

    substrate._read = read
    options = {} if period == "default" else {"period": period}
    result = await substrate.estimate_fee("call", "public", **options)
    expected = DEFAULT_ERA_PERIOD if period == "default" else period
    raw.get_payment_info.assert_awaited_once_with(
        "call", "public", era={"period": expected} if expected is not None else None
    )
    assert result.rao == 123


@pytest.mark.parametrize("period", [None, DEFAULT_ERA_PERIOD, 8, 256])
@pytest.mark.parametrize("shielded", [False, True])
@pytest.mark.parametrize("intent_quote", [False, True])
async def test_planning_quotes_selected_era_for_generic_and_intent_reserves(
    monkeypatch, period, shielded, intent_quote
):
    substrate = FakeSubstrate()
    client = Client("local", substrate=substrate)
    wallet = dev_wallet()
    estimates = []

    async def quote(call, keypair, *, period=DEFAULT_ERA_PERIOD):
        estimates.append(period)
        return Balance.from_rao(100 if period is None else 100 + period)

    monkeypatch.setattr(substrate, "estimate_fee", quote)
    carrier = AsyncMock(return_value=Balance.from_rao(50))
    monkeypatch.setattr(substrate, "estimate_shielded_carrier_fee", carrier)
    if intent_quote:

        async def preflight(self, backend, origin, payer, *, call=None):
            fee = await backend.estimate_fee(call, wallet.coldkeypub)
            return IntentPreflight(
                [], [], [], estimated_fee=fee, required_free=fee, available_free=Balance.from_tao(1)
            )

        monkeypatch.setattr(Transfer, "preflight", preflight)
    expected_period = 8 if shielded and period in (None, DEFAULT_ERA_PERIOD) else period
    expected_fee = 100 if expected_period is None else 100 + expected_period
    for operation in (client.plan, client.preflight):
        result = await operation(Transfer(BOB, 1), wallet, period=period, shielded=shielded)
        fee = result.fee if operation == client.plan else result.estimated_fee
        assert fee.rao == expected_fee + (50 if shielded else 0)
    assert estimates and all(value == expected_period for value in estimates)
    assert all(call.kwargs["period"] == expected_period for call in carrier.await_args_list)


@pytest.mark.parametrize("success", [False, True])
@pytest.mark.parametrize("fee", [None, 0, 1320116])
def test_included_receipt_retains_known_fee_and_events(success, fee):
    events = [{"event": {"module_id": "TransactionPayment", "event_id": "TransactionFeePaid"}}]
    report = InclusionReport(
        extrinsic_hash="0xinner",
        block_number=26,
        extrinsic_idx=7,
        block_hash="0xblock",
        is_success=success,
        total_fee_amount=fee,
        triggered_events=events,
        error_message=None
        if success
        else {"name": "FundsUnavailable", "docs": ["Insufficient funds"]},
    )
    result = RpcSubstrate("ws://127.0.0.1:9944")._result_from_report(report, waited=True)
    assert result.success is success
    assert result.extrinsic_id == "26-0007"
    assert (result.fee.rao if result.fee is not None else None) == fee
    assert result.events == events
    if not success:
        assert result.error.name == "FundsUnavailable"


@pytest.mark.parametrize("success", [False, True])
@pytest.mark.parametrize("outer_fee,inner_fee", [(350181, 1320116), (0, 0), (None, 7), (9, None)])
async def test_shield_receipt_totals_both_fees_without_changing_inner_outcome(
    monkeypatch, success, outer_fee, inner_fee
):
    substrate = FakeSubstrate()
    client = Client("local", substrate=substrate)
    outer = replace(
        success_result(26),
        extrinsic_id="26-0006",
        fee=Balance.from_rao(outer_fee) if outer_fee is not None else None,
        data={"shielded": True, "inner_extrinsic_hash": "0xinner"},
    )
    inner = replace(
        success_result(26),
        extrinsic_id="26-0007",
        success=success,
        message="Success" if success else "Insufficient funds",
        error=None if success else ChainError("Insufficient funds", "FundsUnavailable"),
        fee=Balance.from_rao(inner_fee) if inner_fee is not None else None,
        events=[{"inner": True}],
    )
    monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=inner))
    monkeypatch.setattr(substrate, "finalized_block_number", AsyncMock(return_value=26))
    result = await client._executor._resolve_shielded_inner(
        outer, "0xinner", 8, wait_for_finalization=True
    )
    expected_fee = (
        inner_fee + outer_fee if inner_fee is not None and outer_fee is not None else None
    )
    assert (result.fee.rao if result.fee is not None else None) == expected_fee
    assert result.success is success
    assert result.message == inner.message
    assert result.error is inner.error
    assert result.extrinsic_id == inner.extrinsic_id
    assert result.events == inner.events
    assert result.data == {
        **outer.data,
        "shield_carrier_fee_rao": outer_fee,
        "shield_inner_fee_rao": inner_fee,
    }
    if expected_fee is not None:
        assert result.to_dict()["fee_tao"] == expected_fee / 1_000_000_000


async def test_expired_shield_inner_reports_only_the_paid_carrier_fee(monkeypatch):
    substrate = FakeSubstrate()
    client = Client("local", substrate=substrate)
    outer = replace(success_result(26), fee=Balance.from_rao(350181), data={"shielded": True})
    monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=None))
    monkeypatch.setattr(substrate, "finalized_block_number", AsyncMock(return_value=34))
    result = await client._executor._resolve_shielded_inner(outer, "0xinner", 8)
    assert not result.success
    assert result.error.name == "ShieldInnerExpired"
    assert result.extrinsic_id == outer.extrinsic_id
    assert result.fee.rao == 350181
    assert result.data == {
        "shielded": True,
        "shield_carrier_fee_rao": 350181,
        "shield_inner_fee_rao": 0,
    }
