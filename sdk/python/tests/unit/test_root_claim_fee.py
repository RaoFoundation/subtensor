"""Reserved/spent root-claim fees follow each call's runtime envelope."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.intents import _root_claim_fee as fees


@pytest.mark.asyncio
async def test_reserved_fallback_is_max_root_claim_work():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(object(), "5F3sa2TJAW", _boom)
    assert reserved.rao == fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK


@pytest.mark.asyncio
async def test_single_hotkey_reserved_fallback_uses_declared_work():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(
        object(),
        "5F3sa2TJAW",
        _boom,
        declared_work=fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
    )
    assert reserved.rao == fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_HOTKEY_WORK


def test_spent_scales_against_256_not_network_count():
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK)
    spent = fees._spent_fee(reserved, holdings=32, scan_only=False)
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 32


def test_spent_keeps_non_weight_base_fee():
    base = 12_345
    reserved = Balance.from_rao(base + fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK)
    spent = fees._spent_fee(reserved, holdings=16, scan_only=False)
    assert spent.rao == base + fees._APPROX_REDEEM_FEE_RAO * 16


def test_scan_only_uses_scan_ref_time():
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK)
    spent = fees._spent_fee(reserved, holdings=32, scan_only=True)
    denom = fees._MAX_ROOT_CLAIM_WORK * fees._REDEEM_REF_TIME
    scan = reserved.rao * 32 * fees._SCAN_REF_TIME // denom
    walk = reserved.rao * 1 // fees._MAX_ROOT_CLAIM_WORK
    assert spent.rao == walk + scan


def test_coldkey_wide_empty_baskets_floor_to_hotkey_count():
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * fees._MAX_ROOT_CLAIM_WORK)
    spent = fees._spent_fee(reserved, holdings=0, scan_only=False, hotkey_count=100)
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 100


def test_single_hotkey_spent_scales_against_its_smaller_declaration():
    declared_work = fees._MAX_ROOT_CLAIM_HOTKEY_WORK
    reserved = Balance.from_rao(fees._APPROX_REDEEM_FEE_RAO * declared_work)
    spent = fees._spent_fee(
        reserved,
        holdings=32,
        scan_only=False,
        declared_work=declared_work,
    )
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 32
