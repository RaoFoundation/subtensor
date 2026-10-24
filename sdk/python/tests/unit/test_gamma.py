"""Gamma intents: amount conversion, the quoted min-out, the stake clamp, and
the `btcli misc gamma spend` and service-url input checks."""

from __future__ import annotations

import pytest

from bittensor.cli.context import AppContext
from bittensor.intents import build
from bittensor.intents._money import UNBOUNDED, gamma_amount, gamma_units
from bittensor.result import BittensorError
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE_HOT, BOB_HOT, dev_wallet
from tests.unit.test_cli_commands import invoke, isolated_cli, wallet_dir  # noqa: F401

RAO = 10**9
MINT = {"netuid": 1, "hotkey_ss58": BOB_HOT, "amount_tao": 1.0}
MINT_STAKE = {"netuid": 1, "hotkey_ss58": BOB_HOT, "amount_alpha": 1.0}


@pytest.fixture(scope="module")
def wallet():
    return dev_wallet()


class TestAmounts:
    def test_exact_decimal_round_trip(self):
        assert gamma_amount(0.5) == "0.5"
        assert gamma_units(gamma_amount("1.000000001")) == 1_000_000_001

    @pytest.mark.parametrize("bad", ["0.0000000001", -1, "nan", "all"])
    def test_rejects_unrepresentable(self, bad):
        with pytest.raises(BittensorError):
            gamma_amount(bad)

    def test_all_only_where_allowed(self):
        assert gamma_amount("ALL", allow_all=True) == "all"


class TestMinOut:
    @pytest.mark.asyncio
    async def test_quote_less_tolerance(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO, 400 * RAO))
        intent = build("mint_gamma", {**MINT, "rate_tolerance": 0.1})
        call = await intent.build(sub, wallet)
        assert call.params["min_amount_out"] == int(400 * RAO * 0.9)

    @pytest.mark.asyncio
    async def test_no_quote_refuses(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", None)
        with pytest.raises(BittensorError, match="cannot be minted"):
            await build("mint_gamma", MINT).build(sub, wallet)

    def test_bad_tolerance_names_no_slippage_switch(self):
        with pytest.raises(BittensorError) as error:
            build("mint_gamma", {**MINT, "rate_tolerance": 1.0})
        assert "slippage" not in str(error.value)


class TestMintWithStake:
    @pytest.mark.asyncio
    async def test_quote_clamped_to_stake_held(self, wallet):
        sub = FakeSubstrate()
        quoted = []
        sub.seed_runtime(
            "StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", {"stake": RAO // 4}
        )
        sub.seed_runtime(
            "GammaRuntimeApi",
            "sim_mint_gamma_with_stake",
            lambda p: quoted.append(p[1]) or (p[1], p[1], 100 * RAO),
        )
        intent = build("mint_gamma_with_stake", {**MINT_STAKE, "origin_hotkey_ss58": ALICE_HOT})
        call = await intent.build(sub, wallet)
        assert quoted == [RAO // 4]
        assert call.params["alpha_amount"] == RAO
        assert call.params["min_amount_out"] == int(100 * RAO * 0.95)

    @pytest.mark.asyncio
    async def test_no_stake_refuses(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", None)
        with pytest.raises(BittensorError, match="no stake"):
            await build("mint_gamma_with_stake", MINT_STAKE).build(sub, wallet)

    def test_spend_is_unbounded(self):
        assert build("mint_gamma_with_stake", MINT_STAKE).spend() is UNBOUNDED


class TestServiceUrl:
    def test_accepts_256_bytes(self):
        build("set_service_url", {"netuid": 1, "url": "a" * 256})

    @pytest.mark.parametrize("url", ["a" * 257, "é" * 129])
    def test_rejects_over_256_utf8_bytes(self, url):
        with pytest.raises(BittensorError, match="at most 256"):
            build("set_service_url", {"netuid": 1, "url": url})


class TestCliSpend:
    @pytest.fixture()
    def submitted(self, isolated_cli, monkeypatch):  # noqa: F811
        intents = []
        monkeypatch.setattr(AppContext, "submit", lambda self, intent, **kw: intents.append(intent))
        return intents

    def test_parses_repeated_hotkey_amount(self, submitted):
        spends = ["--spend", f"{BOB_HOT}=0.5", "--spend", f"{ALICE_HOT}=3"]
        result = invoke("misc", "gamma", "spend", "--netuid", "2", *spends)
        assert result.exit_code == 0, result.output
        [intent] = submitted
        assert intent.netuid == 2
        assert intent.spends == [
            {"hotkey_ss58": BOB_HOT, "amount_gamma": "0.5"},
            {"hotkey_ss58": ALICE_HOT, "amount_gamma": "3"},
        ]

    @pytest.mark.parametrize("entry", [BOB_HOT, "=1"])
    def test_rejects_malformed_entry(self, submitted, entry):
        result = invoke("misc", "gamma", "spend", "--netuid", "2", "--spend", entry)
        assert result.exit_code == 2
        assert not submitted
