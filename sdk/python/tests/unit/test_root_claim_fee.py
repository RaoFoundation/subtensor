"""Reserved/spent root-claim fees follow the live per-hotkey quote."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.intents import _root_claim_fee as fees


@pytest.mark.asyncio
async def test_reserved_fallback_is_max_root_claim_work_when_coldkey_wide():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(
        object(), "5F3sa2TJAW", _boom, fees._MAX_ROOT_CLAIM_WORK
    )
    assert reserved.rao == fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK


@pytest.mark.asyncio
async def test_reserved_fallback_uses_live_units_for_hotkey():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(object(), "5F3sa2TJAW", _boom, 32)
    assert reserved.rao == fees._APPROX_REDEEM_FEE_RAO * 32


def test_declared_units_coldkey_wide_stays_at_envelope():
    assert fees._declared_units(True, holdings=4, networks=12) == fees._MAX_ROOT_CLAIM_WORK


def test_declared_units_per_hotkey_uses_live_networks_or_holdings():
    assert fees._declared_units(False, holdings=4, networks=12) == 12
    assert fees._declared_units(False, holdings=40, networks=12) == 40


def test_spent_scales_against_declared_units_not_always_256():
    declared = 32
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * declared)
    spent = fees._spent_fee(reserved, holdings=8, scan_only=False, declared_units=declared)
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 8


def test_spent_keeps_non_weight_base_fee():
    base = 12_345
    declared = 16
    reserved = Balance.from_rao(base + fees._APPROX_REDEEM_FEE_RAO * declared)
    spent = fees._spent_fee(reserved, holdings=8, scan_only=False, declared_units=declared)
    assert spent.rao == base + fees._APPROX_REDEEM_FEE_RAO * 8


def test_scan_only_uses_scan_ref_time():
    declared = 32
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * declared)
    spent = fees._spent_fee(reserved, holdings=32, scan_only=True, declared_units=declared)
    denom = declared * fees._REDEEM_REF_TIME
    scan = reserved.rao * 32 * fees._SCAN_REF_TIME // denom
    walk = reserved.rao * 1 // declared
    assert spent.rao == walk + scan


def test_coldkey_wide_empty_baskets_floor_to_hotkey_count():
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK)
    spent = fees._spent_fee(
        reserved,
        holdings=0,
        scan_only=False,
        hotkey_count=100,
        declared_units=fees._MAX_ROOT_CLAIM_WORK,
    )
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 100
