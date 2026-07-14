"""Default slippage protection on the staking intents.

``add_stake``, ``remove_stake``, and ``swap_stake`` are slippage-protected by
default: at build time they read the spot price and compose the ``*_limit``
call variant with a fill-or-kill limit derived from ``rate_tolerance`` (5%).
These tests pin the call selection, the limit-price math against seeded
prices, the opt-out, the failure modes, and the ``SlippageTooHigh``
remediation that tells the user how to loosen or disable the protection.
"""

from __future__ import annotations

import pytest

from bittensor.intents import REGISTRY, build
from bittensor.result import BittensorError, ChainError, ErrorCode
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import BOB_HOT, dev_wallet

RAO = 10**9

ADD = {"hotkey_ss58": BOB_HOT, "netuid": 1, "amount_tao": 1.0}
REMOVE = {"hotkey_ss58": BOB_HOT, "netuid": 1, "amount_alpha": 1.0}
SWAP = {"hotkey_ss58": BOB_HOT, "origin_netuid": 1, "dest_netuid": 2, "amount_alpha": 1.0}


@pytest.fixture()
def substrate() -> FakeSubstrate:
    sub = FakeSubstrate()
    # Spot prices: netuid 1 at 2 TAO/alpha, netuid 2 at 1 TAO/alpha.
    sub.seed_runtime("SwapRuntimeApi", "current_alpha_price", lambda p: {1: 2 * RAO, 2: RAO}[p[0]])
    return sub


@pytest.fixture(scope="module")
def wallet():
    return dev_wallet()


class TestDefaultProtection:
    @pytest.mark.asyncio
    async def test_add_stake_composes_limit_call(self, substrate, wallet):
        call = await build("add_stake", ADD).build(substrate, wallet)
        assert call.function == "add_stake_limit"
        # Max price to pay: spot * (1 + 5%).
        assert call.params["limit_price"] == int(2 * RAO * 1.05)
        assert call.params["allow_partial"] is False
        assert call.params["amount_staked"] == RAO

    @pytest.mark.asyncio
    async def test_remove_stake_composes_limit_call(self, substrate, wallet):
        call = await build("remove_stake", REMOVE).build(substrate, wallet)
        assert call.function == "remove_stake_limit"
        # Min price to accept: spot * (1 - 5%).
        assert call.params["limit_price"] == int(2 * RAO * 0.95)
        assert call.params["allow_partial"] is False

    @pytest.mark.asyncio
    async def test_swap_stake_composes_limit_call(self, substrate, wallet):
        call = await build("swap_stake", SWAP).build(substrate, wallet)
        assert call.function == "swap_stake_limit"
        # Min origin/destination price ratio (scaled by 1e9): 2.0 * (1 - 5%).
        assert call.params["limit_price"] == int(2 * RAO * 0.95)
        assert call.params["allow_partial"] is False

    @pytest.mark.asyncio
    async def test_custom_tolerance_moves_the_limit(self, substrate, wallet):
        call = await build("add_stake", {**ADD, "rate_tolerance": 0.1}).build(substrate, wallet)
        assert call.params["limit_price"] == int(2 * RAO * 1.1)
        call = await build("remove_stake", {**REMOVE, "rate_tolerance": 0.1}).build(
            substrate, wallet
        )
        assert call.params["limit_price"] == int(2 * RAO * 0.9)

    @pytest.mark.asyncio
    async def test_remove_all_resolves_stake_and_keeps_protection(self, substrate, wallet):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 5 * RAO},
        )
        call = await build("remove_stake", {**REMOVE, "amount_alpha": "all"}).build(
            substrate, wallet
        )
        assert call.function == "remove_stake_limit"
        assert call.params["amount_unstaked"] == 5 * RAO

    def test_summary_names_the_tolerance(self):
        assert "5.00%" in build("add_stake", ADD).summary()
        off = build("add_stake", {**ADD, "slippage_protection": False}).summary()
        assert "no slippage protection" in off


class TestOptOut:
    @pytest.mark.parametrize(
        ("op", "args", "plain"),
        [
            ("add_stake", ADD, "add_stake"),
            ("remove_stake", REMOVE, "remove_stake"),
            ("swap_stake", SWAP, "swap_stake"),
        ],
    )
    @pytest.mark.asyncio
    async def test_disabled_composes_plain_call(self, substrate, wallet, op, args, plain):
        call = await build(op, {**args, "slippage_protection": False}).build(substrate, wallet)
        assert call.function == plain
        assert "limit_price" not in call.params


class TestFailureModes:
    @pytest.mark.parametrize("bad", [-0.1, 1.0, 5])
    @pytest.mark.parametrize(
        ("op", "args"), [("add_stake", ADD), ("remove_stake", REMOVE), ("swap_stake", SWAP)]
    )
    def test_out_of_range_tolerance_rejected_at_construction(self, op, args, bad):
        with pytest.raises(BittensorError, match="rate_tolerance"):
            build(op, {**args, "rate_tolerance": bad})

    @pytest.mark.asyncio
    async def test_missing_price_fails_with_disable_hint(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("SwapRuntimeApi", "current_alpha_price", None)
        with pytest.raises(BittensorError, match="disable slippage protection"):
            await build("add_stake", ADD).build(sub, wallet)

    @pytest.mark.asyncio
    async def test_swap_with_unpriced_destination_fails_loudly(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("SwapRuntimeApi", "current_alpha_price", lambda p: {1: RAO, 2: 0}[p[0]])
        with pytest.raises(BittensorError, match="no alpha price"):
            await build("swap_stake", SWAP).build(sub, wallet)


class TestErrorSurface:
    def test_slippage_too_high_remediation_names_the_off_switch(self):
        error = ChainError("Slippage is too high for the transaction.", "SlippageTooHigh")
        assert error.code is ErrorCode.INSUFFICIENT_LIQUIDITY
        assert "--rate-tolerance" in error.remediation
        assert "--no-slippage-protection" in error.remediation
        assert "slippage_protection=False" in error.remediation

    def test_schema_exposes_the_protection_fields(self):
        for op in ("add_stake", "remove_stake", "swap_stake"):
            schema = REGISTRY[op].json_schema()
            assert schema["properties"]["slippage_protection"]["type"] == "boolean"
            assert schema["properties"]["rate_tolerance"]["type"] == "number"
            # Optional with defaults: agents that omit them get the protection.
            assert "slippage_protection" not in schema["required"]
            assert "rate_tolerance" not in schema["required"]
