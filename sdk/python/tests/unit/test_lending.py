"""Lending intents, previews and reads, and the `btcli lending` commands."""

from __future__ import annotations

import asyncio
import json

import pytest

from bittensor import Client
from bittensor.cli.context import AppContext
from bittensor.intents import REGISTRY, build
from bittensor.intents._money import UNBOUNDED
from bittensor.intents.lending import PARAM_GROUPS
from bittensor.result import BittensorError, ChainError
from tests.harness.fake_substrate import (
    LENDING_CLOSE_QUOTE,
    LENDING_GROW_QUOTE,
    LENDING_PARAMS,
    FakeSubstrate,
)
from tests.harness.samples import ALICE, BOB, dev_wallet
from tests.unit.test_cli_commands import (  # noqa: F401
    _WALLET_NAME,
    fake,
    fake_reads,
    invoke,
    isolated_cli,
    wallet_dir,
)

RAO = 10**9
U64_MAX = 2**64 - 1
OPEN = {"netuid": 1, "amount_alpha": 10.0, "collateral_tao": 2.0}
SIGNER = ALICE
LENDING_DISABLED = {"Module": {"index": 34, "error": "0x02000000"}}
SETTERS = {
    "lending": "set_lending_params",
    "short": "set_short_params",
    "long": "set_long_params",
    "dereg": "set_dereg_params",
}
# A tightening waiting out its delay, as the chain stores it: the whole queued group.
WAITING_LENDING = {
    **LENDING_PARAMS["lending"],
    "apr_bps": 3_000,
    "rate_cap_bps": 20_000,
}


@pytest.fixture(scope="module")
def wallet():
    return dev_wallet()


# The fake's defaults: selling 10 alpha pays 1 TAO, and buying it back costs 1.1 TAO.
@pytest.mark.parametrize(
    ("op", "args", "call", "params"),
    [
        ("open_short", OPEN, "open_short",
            {"netuid": 1, "q": 10 * RAO, "collateral": 2 * RAO, "min_tao_out": 950_000_000}),
        ("increase_short", {"netuid": 1, "amount_alpha": 10.0}, "increase_short",
            {"netuid": 1, "q": 10 * RAO, "collateral": 0, "min_tao_out": 950_000_000}),
        ("close_short", {"netuid": 1, "amount_alpha": 5.0}, "close_short",
            {"netuid": 1, "alpha": 5 * RAO, "max_tao_in": 1_155_000_000}),
        ("close_short", {"netuid": 1}, "close_short",
            {"netuid": 1, "alpha": U64_MAX, "max_tao_in": 1_155_000_000}),
        ("short_add_collateral", {"netuid": 1, "amount_tao": 1.5}, "add_collateral",
            {"netuid": 1, "amount": 1_500_000_000}),
        ("set_lending_enabled", {"enabled": True}, "set_enabled", {"enabled": True}),
        ("set_lending_longs_enabled", {"enabled": False}, "set_longs_enabled",
            {"enabled": False}),
    ],
)  # fmt: skip
@pytest.mark.asyncio
async def test_calls_carry_chain_units(wallet, op, args, call, params):
    built = await build(op, args).build(FakeSubstrate(), wallet)
    assert tuple(built) == ("Lending", call, params)


class TestBounds:
    @pytest.mark.parametrize(
        ("tolerance", "proceeds", "minimum"),
        [(0.1, RAO, 900_000_000), (0, 2**53 + 3, 2**53 + 3)],
    )
    @pytest.mark.asyncio
    async def test_open_takes_the_quote_less_tolerance(self, wallet, tolerance, proceeds, minimum):
        sub = FakeSubstrate()
        quote = {"Ok": {**LENDING_GROW_QUOTE, "proceeds": proceeds}}
        sub.seed_runtime("LendingRuntimeApi", "quote_open_short", quote)
        call = await build("open_short", {**OPEN, "rate_tolerance": tolerance}).build(sub, wallet)
        assert call.params["min_tao_out"] == minimum

    @pytest.mark.parametrize(
        ("tolerance", "cost", "maximum"),
        [(0.1, RAO, 1_100_000_000), (0, 7, 7), (0.05, 3, 4), (0.5, U64_MAX, U64_MAX)],
    )
    @pytest.mark.asyncio
    async def test_close_pays_at_most_the_quote_plus_tolerance(
        self, wallet, tolerance, cost, maximum
    ):
        sub = FakeSubstrate()
        sub.seed_runtime(
            "LendingRuntimeApi", "quote_close_short", {"Ok": {**LENDING_CLOSE_QUOTE, "cost": cost}}
        )
        intent = build("close_short", {"netuid": 1, "rate_tolerance": tolerance})
        assert (await intent.build(sub, wallet)).params["max_tao_in"] == maximum

    @pytest.mark.asyncio
    async def test_increase_and_close_quote_the_signers_own_short(self, wallet):
        sub = FakeSubstrate()
        asked = []
        for method, quote in (
            ("quote_increase_short", LENDING_GROW_QUOTE),
            ("quote_close_short", LENDING_CLOSE_QUOTE),
        ):
            sub.seed_runtime(
                "LendingRuntimeApi", method, lambda p, q=quote: asked.append(p) or {"Ok": q}
            )
        await build("increase_short", {"netuid": 3, "amount_alpha": 2.0}).build(sub, wallet)
        await build("close_short", {"netuid": 3}).build(sub, wallet)
        coldkey = wallet.coldkeypub.ss58_address
        assert asked == [[coldkey, 3, 2 * RAO], [coldkey, 3, U64_MAX]]

    @pytest.mark.parametrize("op", ["open_short", "increase_short", "close_short"])
    @pytest.mark.asyncio
    async def test_a_refused_quote_raises_the_chain_error(self, wallet, op):
        sub = FakeSubstrate()
        sub.seed_runtime("LendingRuntimeApi", f"quote_{op}", {"Err": LENDING_DISABLED})
        args = OPEN if op != "close_short" else {"netuid": 1}
        with pytest.raises(ChainError) as refused:
            await build(op, args).build(sub, wallet)
        assert refused.value.name == "Disabled"

    @pytest.mark.asyncio
    async def test_a_chain_without_lending_is_an_error(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("LendingRuntimeApi", "quote_open_short", None)
        with pytest.raises(BittensorError, match="no lending runtime API"):
            await build("open_short", OPEN).build(sub, wallet)

    @pytest.mark.parametrize("tolerance", [-0.1, 1, 2])
    def test_rate_tolerance_is_a_fraction(self, tolerance):
        with pytest.raises(BittensorError):
            build("open_short", {**OPEN, "rate_tolerance": tolerance})

    def test_only_a_close_takes_all(self):
        assert build("close_short", {"netuid": 1, "amount_alpha": "all"}).amount_alpha == "all"
        with pytest.raises(BittensorError):
            build("open_short", {**OPEN, "amount_alpha": "all"})


class TestSpend:
    def test_what_leaves_the_signers_free_balance(self):
        # An open also pays the chain's deposit, which only a read knows.
        assert build("open_short", OPEN).spend() is UNBOUNDED
        assert build("increase_short", OPEN).spend().rao == 2 * RAO
        assert build("short_add_collateral", {"netuid": 1, "amount_tao": 1.5}).spend().rao == (
            1_500_000_000
        )
        # A close pays from the position's custody.
        assert build("close_short", {"netuid": 1}).spend() is None


class TestPreview:
    @pytest.mark.asyncio
    async def test_open_shows_the_quote_and_its_minimum(self):
        preview = await build("open_short", OPEN).preflight(FakeSubstrate(), SIGNER, SIGNER)
        assert not preview.blocks
        assert preview.preview == {
            "proceeds_tao": "1",
            "minimum_proceeds_tao": "0.95",
            "min_collateral_tao": "2",
            "coverage_need_tao": "3",
            "deposit_tao": "0.1",
            "interest_tao": "0",
        }
        assert preview.facts_title == "Quote"
        assert "deposit" in dict(preview.facts)

    @pytest.mark.asyncio
    async def test_too_little_collateral_blocks(self):
        intent = build("open_short", {**OPEN, "collateral_tao": 1.5})
        preview = await intent.preflight(FakeSubstrate(), SIGNER, SIGNER)
        assert len(preview.blocks) == 1 and "at least" in preview.blocks[0]

    @pytest.mark.asyncio
    async def test_increase_shows_the_interest_it_settles(self):
        sub = FakeSubstrate()
        intent = build("increase_short", {"netuid": 1, "amount_alpha": 1.0, "collateral_tao": 2})
        preview = await intent.preflight(sub, SIGNER, SIGNER)
        assert preview.preview["interest_tao"] == "0.000001"
        assert "interest settled" in dict(preview.facts)

    @pytest.mark.asyncio
    async def test_close_shows_where_the_cost_comes_from(self):
        preview = await build("close_short", {"netuid": 1}).preflight(
            FakeSubstrate(), SIGNER, SIGNER
        )
        assert preview.preview == {
            "alpha": "10",
            "cost_tao": "1.1",
            "maximum_cost_tao": "1.155",
            "from_proceeds_tao": "1",
            "from_collateral_tao": "0.1",
            "interest_tao": "0.000005",
            "full": True,
        }


class TestSetters:
    @pytest.mark.parametrize("group", list(PARAM_GROUPS))
    @pytest.mark.asyncio
    async def test_changes_merge_over_the_live_group(self, wallet, group):
        op = "set_lending_params" if group == "lending" else f"set_lending_{group}_params"
        name = PARAM_GROUPS[group][0]
        call = await build(op, {"changes": {name: 12_345}}).build(FakeSubstrate(), wallet)
        expected = {**LENDING_PARAMS[group], name: 12_345}
        assert tuple(call) == ("Lending", SETTERS[group], {"params": expected})
        assert list(call.params["params"]) == list(PARAM_GROUPS[group])

    def test_the_lending_group_is_the_rate_curve_and_the_shared_limits(self):
        assert PARAM_GROUPS["lending"] == (
            "apr_bps", "rate_k", "rate_cap_bps", "rate_delay_blocks",
            "charge_interval_blocks", "min_pool_tao", "min_notional", "deposit",
            "twap_samples", "twap_interval_blocks",
        )  # fmt: skip
        assert tuple(LENDING_PARAMS["lending"]) == PARAM_GROUPS["lending"]

    @pytest.mark.asyncio
    async def test_the_rate_curve_is_set_field_by_field(self, wallet):
        intent = build("set_lending_params", {"changes": {"rate_k": 3, "rate_cap_bps": 20_000}})
        call = await intent.build(FakeSubstrate(), wallet)
        expected = {**LENDING_PARAMS["lending"], "rate_k": 3, "rate_cap_bps": 20_000}
        assert call.params["params"] == expected
        with pytest.raises(BittensorError):
            build("set_lending_params", {"changes": {"exit_hysteresis_bps": 500}})

    @pytest.mark.parametrize("removed", ["rate_floor_bps", "stress_slope_bps"])
    def test_removed_fields_are_refused(self, removed):
        with pytest.raises(BittensorError):
            build("set_lending_params", {"changes": {removed: 1}})
        with pytest.raises(BittensorError):
            build("set_lending_short_params", {"changes": {removed: 1}})

    @pytest.mark.asyncio
    async def test_the_delay_is_set_by_name(self, wallet):
        changes = {"rate_delay_blocks": 0}
        call = await build("set_lending_params", {"changes": changes}).build(
            FakeSubstrate(), wallet
        )
        assert call.params["params"] == {**LENDING_PARAMS["lending"], **changes}

    @pytest.mark.asyncio
    async def test_a_waiting_lending_change_is_carried_by_the_next_call(self, wallet):
        sub = FakeSubstrate()
        sub.seed("Lending", "PendingLendingParams", [], (WAITING_LENDING, 30_000))
        call = await build("set_lending_params", {"changes": {"deposit": 5}}).build(sub, wallet)
        expected = {**WAITING_LENDING, "deposit": 5}
        assert call.params["params"] == expected
        assert list(call.params["params"]) == list(PARAM_GROUPS["lending"])
        # A named field wins over the waiting one.
        call = await build("set_lending_params", {"changes": {"apr_bps": 1_000}}).build(sub, wallet)
        assert call.params["params"] == {**WAITING_LENDING, "apr_bps": 1_000}

    @pytest.mark.asyncio
    async def test_changes_merge_over_a_group_queued_this_block(self, wallet):
        sub = FakeSubstrate()
        queued = {**LENDING_PARAMS["short"], "stress_bps": 40_000}
        sub.seed("Lending", "PendingShortParams", [], queued)
        intent = build("set_lending_short_params", {"changes": {"open_limit_bps": 50}})
        call = await intent.build(sub, wallet)
        assert call.params["params"] == {**queued, "open_limit_bps": 50}

    @pytest.mark.parametrize(
        "changes",
        [{}, None, {"apr": 1}, {"stress_bps": 1}, {"apr_bps": -1}, {"apr_bps": "5"},
         {"apr_bps": 1.5}, {"apr_bps": True}],
    )  # fmt: skip
    def test_changes_name_the_groups_own_fields_as_integers(self, changes):
        with pytest.raises(BittensorError):
            build("set_lending_params", {"changes": changes})

    @pytest.mark.parametrize("op", sorted(op for op in REGISTRY if "lending" in op))
    def test_every_setter_is_root_with_a_verify_read(self, op):
        intent = REGISTRY[op]
        assert intent.origin == "root" and intent.verify == "lending_params"
        assert ("Sudo", "sudo") in intent.wraps and intent.wraps[0][0] == "Lending"


class TestReads:
    def test_params_are_the_switches_and_the_four_groups(self):
        params = asyncio.run(Client("local", substrate=FakeSubstrate()).lending.lending_params())
        assert params == {**LENDING_PARAMS, "pending_lending_params": None}

    def test_params_show_a_waiting_lending_change(self):
        sub = FakeSubstrate()
        sub.seed("Lending", "PendingLendingParams", [], (WAITING_LENDING, 30_000))
        params = asyncio.run(Client("local", substrate=sub).lending.lending_params())
        assert params["pending_lending_params"] == {**WAITING_LENDING, "effective_at": 30_000}
        assert params["lending"] == LENDING_PARAMS["lending"]

    def test_a_short_position_carries_its_leg(self):
        lending = Client("local", substrate=FakeSubstrate()).lending
        position = asyncio.run(lending.lending_position(coldkey_ss58=ALICE, netuid=1, side="short"))
        assert position["side"] == "short"
        assert position["minted_alpha"].rao == 10 * RAO and position["minted_alpha"].netuid == 1
        assert position["proceeds"].rao == RAO and position["collateral"].rao == 3 * RAO
        assert position["interest_due"].rao == 5_000
        assert (position["opened_at"], position["last_accrued"], position["charge_at"]) == (
            100,
            460,
            820,
        )
        assert asyncio.run(lending.lending_positions(coldkey_ss58=ALICE)) == [position]

    def test_a_long_position_carries_its_leg(self):
        sub = FakeSubstrate()
        asked = []
        long = {
            "collateral": RAO,
            "deposit": 0,
            "opened_at": 1,
            "accrual": {"last_accrued": 1, "remainder": 0, "due": 0},
            "charge_at": 361,
            "leg": {"Long": {"locked_alpha": 4 * RAO, "debt": 2 * RAO}},
        }
        sub.seed_runtime("LendingRuntimeApi", "position", lambda p: asked.append(p) or long)
        lending = Client("local", substrate=sub).lending
        position = asyncio.run(lending.lending_position(coldkey_ss58=BOB, netuid=2, side="Long"))
        assert asked == [[BOB, 2, "Long"]]
        assert position["side"] == "long" and position["locked_alpha"].rao == 4 * RAO
        assert position["debt"].rao == 2 * RAO and "minted_alpha" not in position

    def test_no_position_is_none_and_a_bad_side_is_an_error(self):
        sub = FakeSubstrate()
        sub.seed_runtime("LendingRuntimeApi", "position", None)
        sub.seed_runtime("LendingRuntimeApi", "positions_of", [])
        lending = Client("local", substrate=sub).lending
        assert asyncio.run(
            lending.lending_position(coldkey_ss58=ALICE, netuid=1, side="short")
        ) is (None)
        assert asyncio.run(lending.lending_positions(coldkey_ss58=ALICE)) == []
        with pytest.raises(BittensorError, match="side must be"):
            asyncio.run(lending.lending_position(coldkey_ss58=ALICE, netuid=1, side="both"))

    def test_market_prices_are_tao_per_alpha(self):
        sub = FakeSubstrate()
        lending = Client("local", substrate=sub).lending
        market = asyncio.run(lending.lending_market(netuid=1))
        assert market["pool_tao"].rao == 50_000 * RAO and market["pool_alpha"].netuid == 1
        assert market["quote_weight"] == 0.5
        assert market["swap_fee"] == pytest.approx(33 / 65_535)
        assert (market["spot"], market["twap"], market["sma"]) == pytest.approx((0.1, 0.1, 0.125))
        # No short has defaulted: the entry is absent and reads as zero.
        assert market["residual_alpha"].rao == 0 and market["residual_alpha"].netuid == 1
        sub.seed("Lending", "ResidualAlpha", [1], 3 * RAO)
        market = asyncio.run(lending.lending_market(netuid=1))
        assert market["residual_alpha"].rao == 3 * RAO
        sub.seed_runtime("LendingRuntimeApi", "market", None)
        assert asyncio.run(lending.lending_market(netuid=9)) is None

    def test_quotes_are_in_tao(self):
        sub = FakeSubstrate()
        asked = []
        sub.seed_runtime(
            "LendingRuntimeApi",
            "quote_close_short",
            lambda p: asked.append(p) or {"Ok": LENDING_CLOSE_QUOTE},
        )
        lending = Client("local", substrate=sub).lending
        opened = asyncio.run(lending.quote_open_short(netuid=1, amount_alpha=10))
        assert opened["proceeds"].rao == RAO and opened["min_collateral"].rao == 2 * RAO
        assert opened["deposit"].rao == 10**8 and opened["spot_after"] == pytest.approx(0.1)
        grown = asyncio.run(
            lending.quote_increase_short(coldkey_ss58=ALICE, netuid=1, amount_alpha=10)
        )
        assert grown["interest"].rao == 1_000 and grown["deposit"].rao == 0
        closed = asyncio.run(
            lending.quote_close_short(coldkey_ss58=ALICE, netuid=1, amount_alpha=2.5)
        )
        assert asked == [[ALICE, 1, 2_500_000_000]]
        assert closed["cost"].rao == 1_100_000_000 and closed["alpha"].netuid == 1
        assert closed["full"] is True

    def test_a_refused_quote_raises_the_chain_error(self):
        sub = FakeSubstrate()
        sub.seed_runtime("LendingRuntimeApi", "quote_open_short", {"Err": LENDING_DISABLED})
        with pytest.raises(ChainError) as refused:
            asyncio.run(
                Client("local", substrate=sub).lending.quote_open_short(netuid=1, amount_alpha=1)
            )
        assert refused.value.name == "Disabled"


class TestCli:
    @pytest.fixture()
    def submitted(self, isolated_cli, monkeypatch):  # noqa: F811
        intents = []
        monkeypatch.setattr(AppContext, "submit", lambda self, intent, **kw: intents.append(intent))
        return intents

    def test_params_show(self, fake_reads):  # noqa: F811
        result = invoke("--json", "lending", "params", "show")
        assert result.exit_code == 0, result.output
        assert json.loads(result.output) == {**LENDING_PARAMS, "pending_lending_params": None}

    def test_params_set_submits_the_groups_setter(self, submitted):
        result = invoke(
            "lending", "params", "set", "short", "open_limit_bps=50", "stress_bps=40_000"
        )
        assert result.exit_code == 0, result.output
        assert [(intent.op, intent.changes) for intent in submitted] == [
            ("set_lending_short_params", {"open_limit_bps": 50, "stress_bps": 40_000})
        ]

    @pytest.mark.parametrize(
        "argv",
        [("shorts", "open_limit_bps=50"), ("short", "open_limit_bps"),
         ("short", "open_limit_bps=x"), ("short", "apr_bps=5")],
    )  # fmt: skip
    def test_params_set_refuses_what_it_cannot_read(self, submitted, argv):
        result = invoke("lending", "params", "set", *argv)
        assert result.exit_code != 0 and not submitted
        assert "Traceback" not in result.output

    @pytest.mark.parametrize(
        ("argv", "op"),
        [(["open", "--amount-alpha", "10", "--collateral-tao", "2"], "open_short"),
         (["increase", "--amount-alpha", "10", "--collateral-tao", "2"], "increase_short"),
         (["close", "--amount-alpha", "5"], "close_short"),
         (["add-collateral", "--amount-tao", "1"], "short_add_collateral")],
    )  # fmt: skip
    def test_short_commands_are_the_intents(self, fake, argv, op):  # noqa: F811
        result = invoke(
            "--json", "--dry-run", "lending", "short", *argv, "--netuid", "1", "-w", _WALLET_NAME
        )
        assert result.exit_code == 0, result.output
        assert json.loads(result.output)["op"] == op

    def test_positions_list(self, fake_reads):  # noqa: F811
        result = invoke("--json", "lending", "positions", "list", "--coldkey", ALICE)
        assert result.exit_code == 0, result.output
        (position,) = json.loads(result.output)
        assert (position["netuid"], position["side"]) == (1, "short")

    @pytest.mark.parametrize(
        ("argv", "key"),
        [(["open"], "proceeds"), (["increase", "--coldkey", ALICE], "proceeds"),
         (["close", "--coldkey", ALICE], "cost")],
    )  # fmt: skip
    def test_quotes(self, fake_reads, argv, key):  # noqa: F811
        result = invoke("--json", "lending", "quote", *argv, "--netuid", "1", "--amount", "10")
        assert result.exit_code == 0, result.output
        assert key in json.loads(result.output)

    def test_market(self, fake_reads):  # noqa: F811
        result = invoke("--json", "lending", "market", "--netuid", "1")
        assert result.exit_code == 0, result.output
        assert json.loads(result.output)["netuid"] == 1
        assert "residual_alpha" in json.loads(result.output)
        assert "residual alpha" in invoke("lending", "market", "--netuid", "1").output
        fake_reads.seed_runtime("LendingRuntimeApi", "market", None)
        assert invoke("lending", "market", "--netuid", "9").exit_code == 1

    def test_longs_are_pending(self, isolated_cli):  # noqa: F811
        result = invoke("lending", "long")
        assert result.exit_code == 1 and "pending" in result.output
