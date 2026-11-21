"""Focused coverage for live root-swap price-protection review text."""

from decimal import Decimal
from types import SimpleNamespace

import pytest

from bittensor import storage as st
from bittensor.balance import Balance
from bittensor.cli.commands.root_swap import _swap_review
from bittensor.client import Client
from bittensor.intents.staking import SwapBasket
from bittensor.reads.root import basket_trade_protection
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE_HOT


@pytest.mark.asyncio
@pytest.mark.parametrize("reference", ["StrictestEmaAndSpot", 1, {"StrictestEmaAndSpot": None}])
async def test_basket_trade_protection_reads_live_settings(reference):
    substrate = FakeSubstrate()
    substrate.seed_default("SubtensorModule", "BasketTradeMaxSlippageBps", 200)
    substrate.seed_default("SubtensorModule", "BasketTradePriceReferenceMode", reference)
    result = await basket_trade_protection(Client("local", substrate=substrate))
    assert result == {
        "band_bps": 200,
        "band_pct": Decimal("2"),
        "price_reference": "StrictestEmaAndSpot",
    }


@pytest.mark.asyncio
async def test_basket_trade_protection_tolerates_old_nodes():
    substrate = FakeSubstrate()
    substrate.missing_storage.add(("SubtensorModule", "BasketTradeMaxSlippageBps"))
    assert await basket_trade_protection(Client("local", substrate=substrate)) is None


def test_root_swap_review_uses_live_band_and_reference():
    app_ctx = SimpleNamespace(review_account=lambda: None)
    intent = SwapBasket(
        hotkey_ss58=ALICE_HOT,
        origin_netuid=1,
        dest_netuid=2,
        amount="1",
        min_amount_out=Balance.from_rao(1_000_000_000, 2),
    )
    _, rows = _swap_review(
        app_ctx,
        intent,
        holdings=[],
        status=None,
        protection={
            "band_bps": 200,
            "band_pct": Decimal("2"),
            "price_reference": "StrictestEmaAndSpot",
        },
        expected_out=Balance.from_rao(1_100_000_000, 2),
        max_slippage_pct=1,
    )
    note = next(value for key, value, *_ in rows if key == "note")
    assert "within 2% of the live StrictestEmaAndSpot price reference" in note
    assert "5%" not in note


def test_root_swap_review_does_not_invent_a_default_when_settings_are_unavailable():
    app_ctx = SimpleNamespace(review_account=lambda: None)
    _, rows = _swap_review(
        app_ctx,
        SwapBasket(hotkey_ss58=ALICE_HOT, origin_netuid=1, dest_netuid=2, amount="1"),
        holdings=[],
        status=None,
        protection=None,
        expected_out=None,
        max_slippage_pct=1,
    )
    note = next(value for key, value, *_ in rows if key == "note")
    assert "live configured band" in note
    assert "5%" not in note and "2%" not in note


def test_basket_protection_storage_descriptors_cover_the_new_read():
    assert st.SubtensorModule.BasketTradeMaxSlippageBps.name == "BasketTradeMaxSlippageBps"
    assert st.SubtensorModule.BasketTradePriceReferenceMode.name == "BasketTradePriceReferenceMode"
