"""Controller funding includes refundable reserves, not just transaction fees."""

import asyncio
from types import SimpleNamespace

import pytest
import typer

from bittensor.cli import protected
from bittensor.intents import Batch, Transfer
from bittensor.protected_funding import funding_budget


def budget(**changes):
    args = dict(
        controller="controller",
        account={"data": {"free": 0, "reserved": 93_000_000}},
        announcement_count=0,
        announcement_deposit=0,
        deposit_base=36_000_000,
        deposit_factor=68_000_000,
        existential_deposit=500,
        announce_fee=1000,
        transfer_execution_fee=2000,
        staking_execution_fee=5000,
        fee_payer="controller",
    )
    return funding_budget(**(args | changes))


def test_budget_covers_twenty_transfers_and_ten_staking_operations():
    state = budget()
    assert state.target_rao >= 36_000_000 + 20 * 68_000_000 + 20 * 3000 + 500
    assert state.target_rao >= 36_000_000 + 10 * 68_000_000 + 10 * 6000 + 500
    assert state.warning_rao >= 36_000_000 + 5 * 68_000_000 + 5 * 3000 + 500
    assert state.warning_rao >= 36_000_000 + 2 * 68_000_000 + 2 * 6000 + 500
    assert state.next_announcement_deposit_rao == 104_000_000
    assert state.low and not state.to_dict()["can_announce"]


def test_reserved_and_frozen_funds_are_not_available_for_new_announcements():
    state = budget(
        account={"data": {"free": 500_000_000, "reserved": 10**12, "frozen": 450_000_000}}
    )
    assert state.available_rao == 50_000_000
    assert state.top_up_rao == state.target_rao - 50_000_000
    assert not state.to_dict()["can_announce"]


def test_existing_announcement_reserve_is_not_charged_twice():
    state = budget(announcement_count=1, announcement_deposit=104_000_000)
    assert state.next_announcement_deposit_rao == 68_000_000
    assert state.target_rao == budget().target_rao - 36_000_000


def test_external_funding_checks_live_controller_before_sending(monkeypatch):
    errors = []
    record = {"controller_ss58": "old-controller", "protected_ss58": "pure"}
    monkeypatch.setattr(protected, "_get_protected_record", lambda *_a, **_k: record)
    monkeypatch.setattr(protected, "_account_identity", lambda value: value)

    class Chain:
        async def read(self, name, **_kwargs):
            assert name == "proxies"
            return {"proxies": [{"delegate": "new-controller", "proxy_type": "Any", "delay": 4}]}

    ctx = SimpleNamespace(
        run=lambda fn: asyncio.run(fn(Chain())),
        output=SimpleNamespace(error=errors.append),
    )
    with pytest.raises(typer.Exit):
        protected.run_fund(ctx, from_wallet="reserve")
    assert errors == ["controller has no live Any grant on the protected account"]


def test_refill_joins_original_operation_in_one_flat_atomic_batch(monkeypatch):
    messages = []
    ctx = SimpleNamespace(
        assume_yes=True,
        dry_run=False,
        output=SimpleNamespace(message=messages.append),
    )
    state = budget(account={"data": {"free": 200_000_000, "reserved": 0}}).to_dict()
    monkeypatch.setattr(protected, "_controller_funding", lambda *_: state)
    original = Batch(intents=[Transfer("recipient", 1).to_dict()])
    result = protected._offer_operation_refill(
        ctx, {"controller_ss58": "controller", "controller_wallet": "owner"}, original
    )
    assert isinstance(result, Batch)
    assert len(result.intents) == 2
    assert result.intents[0]["dest_ss58"] == "controller"
    assert result.intents[1] == original.intents[0]
    assert all(child["op"] != "batch" for child in result.intents)
    assert "whole batch waits" in messages[-1]


def test_declining_refill_preserves_requested_operation(monkeypatch):
    ctx = SimpleNamespace(
        assume_yes=False,
        dry_run=False,
        output=SimpleNamespace(message=lambda *_: None, confirm=lambda *_: False),
    )
    state = budget(account={"data": {"free": 200_000_000, "reserved": 0}}).to_dict()
    monkeypatch.setattr(protected, "_controller_funding", lambda *_: state)
    monkeypatch.setattr("bittensor.cli.prompt.interactive", lambda *_: True)
    intent = Transfer("recipient", 1)
    assert (
        protected._offer_operation_refill(
            ctx, {"controller_ss58": "controller", "controller_wallet": "owner"}, intent
        )
        is intent
    )
