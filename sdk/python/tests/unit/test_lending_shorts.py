"""Synthetic-short intents and reads against the fake chain."""

from __future__ import annotations

import pytest

from bittensor import Client
from bittensor.intents import CloseShort, IncreaseShort, OpenShort
from bittensor.intents.lending import lending_outcome_data
from bittensor.result import BittensorError
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, dev_wallet

RAO = 10**9


def _sim_sale(tao_out: int):
    return lambda _params: {"tao_amount": tao_out, "alpha_amount": 0}


@pytest.mark.asyncio
async def test_open_short_defaults_sale_floor_to_simulated_sale_less_half_percent():
    sub = FakeSubstrate()
    sub.seed_runtime("SwapRuntimeApi", "sim_swap_alpha_for_tao", _sim_sale(10 * RAO))
    call = await OpenShort(netuid=1, mint_alpha=10, collateral=5).build(sub, dev_wallet())
    assert tuple(call) == (
        "Lending",
        "open_short",
        {"netuid": 1, "mint_alpha": 10 * RAO, "collateral": 5 * RAO, "min_tao_out": 9_950_000_000},
    )


@pytest.mark.asyncio
async def test_open_short_refuses_a_sale_the_pool_prices_at_zero():
    sub = FakeSubstrate()
    sub.seed_runtime("SwapRuntimeApi", "sim_swap_alpha_for_tao", _sim_sale(0))
    with pytest.raises(BittensorError, match="pass min_tao_out"):
        await OpenShort(netuid=1, mint_alpha=10, collateral=5).build(sub, dev_wallet())


def test_open_short_rejects_zero_mint():
    with pytest.raises(BittensorError, match="mint_alpha"):
        OpenShort(netuid=1, mint_alpha=0, collateral=5)


@pytest.mark.asyncio
async def test_open_short_preflight_blocks_collateral_below_the_quote():
    preview = await OpenShort(netuid=1, mint_alpha=10, collateral=1, min_tao_out=9).preflight(
        FakeSubstrate(), ALICE, ALICE
    )
    assert any("below the required" in block for block in preview.blocks)


@pytest.mark.asyncio
async def test_open_short_preflight_passes_with_quoted_collateral():
    preview = await OpenShort(netuid=1, mint_alpha=10, collateral=5, min_tao_out=9).preflight(
        FakeSubstrate(), ALICE, ALICE
    )
    assert preview.blocks == []


@pytest.mark.asyncio
async def test_increase_with_collateral_only_sends_no_sale_floor():
    call = await IncreaseShort(netuid=1, extra_collateral=2).build(FakeSubstrate(), dev_wallet())
    assert call.params == {
        "netuid": 1,
        "extra_mint": 0,
        "extra_collateral": 2 * RAO,
        "min_tao_out": 0,
    }


def test_increase_needs_something_to_add():
    with pytest.raises(BittensorError, match="extra_mint or extra_collateral"):
        IncreaseShort(netuid=1)


@pytest.mark.asyncio
async def test_close_short_defaults_to_whole_position_and_quoted_ceiling():
    sub = FakeSubstrate()
    asked = []

    def quote(params):
        asked.append(params)
        return {"Ok": {"alpha": params[2], "tao_in": 10 * RAO, "refund": 0}}

    sub.seed_runtime("LendingRuntimeApi", "quote_close_short", quote)
    call = await CloseShort(netuid=1).build(sub, dev_wallet())
    assert call.params == {"netuid": 1, "alpha": 10 * RAO, "max_tao_in": 10_050_000_000}
    assert asked == [[ALICE, 1, 10 * RAO]]


@pytest.mark.asyncio
async def test_close_short_without_a_position_fails_before_signing():
    sub = FakeSubstrate()
    sub.seed("Lending", "Positions", [ALICE, 1], None)
    sub.seed_default("Lending", "Positions", None)
    with pytest.raises(BittensorError, match="no short position"):
        await CloseShort(netuid=1).build(sub, dev_wallet())


@pytest.mark.asyncio
async def test_position_read_reports_coverage_from_runtime_health():
    sub = FakeSubstrate()
    position = await Client("local", substrate=sub).read(
        "lending_position", coldkey_ss58=ALICE, netuid=1
    )
    assert position["minted"].rao == 10 * RAO
    assert position["funds"].rao == 20 * RAO
    assert position["coverage_cost"].rao == 15 * RAO
    assert position["covered"] is True
    assert position["deleveraging"] is False


@pytest.mark.asyncio
async def test_position_read_flags_an_uncoverable_book():
    sub = FakeSubstrate()
    health = dict(sub._runtime[("LendingRuntimeApi", "short_health")], coverage_cost=None)
    sub.seed_runtime("LendingRuntimeApi", "short_health", health)
    position = await Client("local", substrate=sub).read(
        "lending_position", coldkey_ss58=ALICE, netuid=1
    )
    assert position["coverage_cost"] is None
    assert position["covered"] is False


def _event(name: str, **attributes) -> dict:
    return {"event": {"module_id": "Lending", "event_id": name, "attributes": attributes}}


def test_outcome_data_decodes_open_and_close():
    opened = [_event("ShortOpened", netuid=3, minted=2 * RAO, proceeds=RAO, collateral=RAO // 2)]
    assert lending_outcome_data(opened, op="open_short") == {
        "minted": "2.000000000α_3",
        "minted_rao": 2 * RAO,
        "proceeds": "τ1.000000000",
        "proceeds_rao": RAO,
        "collateral": "τ0.500000000",
        "collateral_rao": RAO // 2,
    }
    closed = [_event("ShortClosed", netuid=3, burned=RAO, tao_spent=RAO, refund=0)]
    assert lending_outcome_data(closed, op="close_short")["burned"] == "1.000000000α_3"
