"""Gamma intents, previews, results and reads, the service checks, and the
`btcli gamma` input checks."""

from __future__ import annotations

import asyncio
import json

import pytest

from bittensor import Client
from bittensor.cli.context import AppContext
from bittensor.intents import build
from bittensor.intents._money import UNBOUNDED, gamma_amount, gamma_units
from bittensor.result import BittensorError, ChainError, ExtrinsicResult
from bittensor.sp_core import ss58_encode
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, INTENT_SAMPLES, dev_wallet
from tests.unit.test_cli_commands import (  # noqa: F401
    _WALLET_NAME,
    argv_from_sample,
    fake,
    invoke,
    isolated_cli,
    wallet_dir,
)

RAO = 10**9
MINT = {"netuid": 1, "hotkey_ss58": BOB_HOT, "amount_tao": 1.0}
MINT_STAKE = {
    "netuid": 1,
    "hotkey_ss58": BOB_HOT,
    "origin_hotkey_ss58": ALICE_HOT,
    "amount_alpha": 1.0,
}
MOVE = {"netuid": 1, "origin_hotkey_ss58": ALICE_HOT, "destination_hotkey_ss58": BOB_HOT}
DEPOSIT = {"netuid": 1, "hotkey_ss58": BOB_HOT}
SPEND = {"netuid": 1, "service_hotkey_ss58": ALICE_HOT, "amount_gamma": "0.5"}
SIGNER = ALICE_HOT  # any account, as the dispatch origin
GAMMA_DISABLED = {"Module": {"index": 7, "error": "0xc0000000"}}


def _deposit(coldkey: str = SIGNER, balance: int = 0, daily_spend_cap=None, **fields) -> dict:
    cap = None if daily_spend_cap is None else {"cap": daily_spend_cap, "bucket": (0, 0)}
    return {"coldkey": coldkey, "balance": balance, "daily_spend_cap": cap, **fields}


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


# The fake's defaults: 1 alpha staked, and every mint quotes 400 gamma.
@pytest.mark.parametrize(
    ("op", "args", "params"),
    [
        ("mint_gamma", MINT, {"hotkey": BOB_HOT, "netuid": 1, "amount": RAO,
            "min_amount_out": 380 * RAO, "daily_spend_cap": None}),
        ("mint_gamma_with_stake", MINT_STAKE, {"origin_hotkey": ALICE_HOT,
            "destination_hotkey": BOB_HOT, "netuid": 1, "alpha_amount": RAO,
            "min_amount_out": 380 * RAO, "daily_spend_cap": None}),
        ("move_gamma", {**MOVE, "amount_gamma": "1.5"}, {"origin_hotkey": ALICE_HOT,
            "destination_hotkey": BOB_HOT, "netuid": 1, "amount": 1_500_000_000}),
        ("move_gamma", {**MOVE, "amount_gamma": "all"}, {"origin_hotkey": ALICE_HOT,
            "destination_hotkey": BOB_HOT, "netuid": 1, "amount": 2**64 - 1}),
        ("set_gamma_deposit_daily_spend_cap", {**DEPOSIT, "daily_spend_cap_gamma": "2.5"},
            {"hotkey": BOB_HOT, "netuid": 1, "daily_spend_cap": 2_500_000_000}),
        ("set_gamma_deposit_daily_spend_cap", {**DEPOSIT, "daily_spend_cap_gamma": 0},
            {"hotkey": BOB_HOT, "netuid": 1, "daily_spend_cap": 0}),
        ("set_gamma_deposit_daily_spend_cap", {**DEPOSIT, "remove_cap": True},
            {"hotkey": BOB_HOT, "netuid": 1, "daily_spend_cap": None}),
        ("spend_gamma", SPEND, {"service": ALICE_HOT, "netuid": 1, "amount": RAO // 2}),
    ],
)  # fmt: skip
@pytest.mark.asyncio
async def test_calls_carry_chain_units(wallet, op, args, params):
    call = await build(op, args).build(FakeSubstrate(), wallet)
    assert tuple(call) == ("SubtensorModule", op, params)


class TestMinOut:
    @pytest.mark.parametrize(
        ("tolerance", "gamma", "minimum"),
        [(0.1, 400 * RAO, int(400 * RAO * 0.9)), (0, 2**53 + 3, 2**53 + 3)],
    )
    @pytest.mark.asyncio
    async def test_quote_less_tolerance(self, wallet, tolerance, gamma, minimum):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", {"Ok": (RAO, RAO, gamma)})
        call = await build("mint_gamma", {**MINT, "rate_tolerance": tolerance}).build(sub, wallet)
        assert call.params["min_amount_out"] == minimum

    @pytest.mark.asyncio
    async def test_a_refused_quote_raises_the_chain_error(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", {"Err": GAMMA_DISABLED})
        with pytest.raises(ChainError) as refused:
            await build("mint_gamma", MINT).build(sub, wallet)
        assert refused.value.name == "GammaDisabled"


class TestMintWithStake:
    @pytest.mark.parametrize(("amount", "sent"), [(1.0, RAO), ("all", 2**64 - 1)])
    @pytest.mark.asyncio
    async def test_quote_clamped_to_stake_held(self, wallet, amount, sent):
        sub = FakeSubstrate()
        quoted = []
        sub.seed_runtime(
            "StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", {"stake": RAO // 4}
        )
        sub.seed_runtime(
            "GammaRuntimeApi",
            "sim_mint_gamma_with_stake",
            lambda p: quoted.append(p[1]) or {"Ok": (p[1], p[1], 100 * RAO)},
        )
        intent = build("mint_gamma_with_stake", {**MINT_STAKE, "amount_alpha": amount})
        call = await intent.build(sub, wallet)
        assert quoted == [RAO // 4]
        assert call.params["alpha_amount"] == sent
        assert call.params["min_amount_out"] == int(100 * RAO * 0.95)

    @pytest.mark.asyncio
    async def test_no_stake_refuses(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", None)
        with pytest.raises(BittensorError, match="no stake"):
            await build("mint_gamma_with_stake", MINT_STAKE).build(sub, wallet)

    @pytest.mark.parametrize(
        ("op", "args"),
        [
            ("mint_gamma_with_stake", MINT_STAKE),
            ("move_gamma", {**MOVE, "amount_gamma": 1}),
        ],
    )
    def test_spend_is_unbounded(self, op, args):
        assert build(op, args).spend() is UNBOUNDED


class TestService:
    @pytest.mark.parametrize("url", ["", "https://" + "a" * 248])
    def test_accepts_an_empty_url_and_256_bytes(self, url):
        build("set_service", {"netuid": 1, "url": url})

    @pytest.mark.parametrize(
        "url",
        [
            "not a url at all",
            "ftp://a.example",
            "https://",
            "https://" + "a" * 249,
            "https://" + "é" * 125,
        ],
    )
    def test_rejects_non_http_and_over_256_utf8_bytes(self, url):
        with pytest.raises(BittensorError):
            build("set_service", {"netuid": 1, "url": url})

    @pytest.mark.asyncio
    async def test_publishes_the_url_at_the_wallets_hotkey(self, wallet):
        intent = build("set_service", {"netuid": 1, "url": "https://a.example"})
        params = (await intent.build(FakeSubstrate(), wallet)).params
        assert params == {
            "netuid": 1,
            "hotkey": wallet.hotkey.ss58_address,
            "url": b"https://a.example",
        }


class TestServicesRead:
    def test_urls_with_control_or_format_characters_read_as_hex(self):
        sub = FakeSubstrate()
        spoof = "https://a.example\x1b[1A\x1b[2K"
        sub.seed_map(
            "SubtensorModule",
            "Services",
            [
                (ALICE_HOT, {"url": "https://a.example"}),
                (BOB_HOT, {"url": spoof}),
                (BOB_HOT + "x", {"url": "https://\u202eexample"}),
                (BOB_HOT + "y", {"url": "0x41"}),
            ],
        )
        sub.seed("SubtensorModule", "SubnetOwnerHotkey", [1], ALICE_HOT)
        sub.seed("SubtensorModule", "SubnetOwner", [1], ALICE)
        sub.seed("SubtensorModule", "Owner", [ALICE_HOT], ALICE)
        urls = asyncio.run(Client("local", substrate=sub).subnets.services(netuid=1))
        assert urls[ALICE_HOT] == {"url": "https://a.example", "owner": True}
        assert urls[BOB_HOT]["url"] == "0x" + spoof.encode().hex()
        assert not urls[BOB_HOT]["owner"]
        assert urls[BOB_HOT + "x"]["url"].startswith("0x")
        assert urls[BOB_HOT + "y"]["url"] == "0x41"

    def test_an_owner_hotkey_another_coldkey_owns_marks_no_owner(self):
        sub = FakeSubstrate()
        sub.seed_map(
            "SubtensorModule",
            "Services",
            [(ALICE_HOT, {"url": "https://a.example"})],
        )
        sub.seed("SubtensorModule", "SubnetOwnerHotkey", [1], ALICE_HOT)
        sub.seed("SubtensorModule", "SubnetOwner", [1], ALICE)
        sub.seed("SubtensorModule", "Owner", [ALICE_HOT], BOB)
        urls = asyncio.run(Client("local", substrate=sub).subnets.services(netuid=1))
        assert not urls[ALICE_HOT]["owner"]


class TestGammaReads:
    def test_amounts_are_exact_and_the_feed_has_an_age(self):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", {"Ok": (RAO, RAO, 2**53 + 1)})
        price = {"accepted": 312_450_000_000, "feed": 312_500_000_000, "fed_at": sub.block - 7}
        sub.seed("SubtensorModule", "TaoUsdPrice", None, price)
        gamma = Client("local", substrate=sub).gamma
        quote = asyncio.run(gamma.quote_mint_gamma(netuid=1, amount_tao=1))
        assert quote["gamma"] == "9007199.254740993"
        price = asyncio.run(gamma.tao_usd_price())
        assert (price["accepted_usd"], price["blocks_since_last_feed"]) == (312.5, 7)

    @pytest.mark.parametrize(
        ("accepted", "feed", "blocks", "price"),
        [(312_450_000_000, 312_500_000_000, 0, 312.45),
         (312_450_000_000, 312_500_000_000, 1, 312.4706217),
         (1, 312_500_000_000, 2, 3e-9)],
    )  # fmt: skip
    def test_the_accepted_price_rises_by_the_step_up_to_the_feed(
        self, accepted, feed, blocks, price
    ):
        sub = FakeSubstrate()
        state = {"accepted": accepted, "feed": feed, "fed_at": sub.block - blocks}
        sub.seed("SubtensorModule", "TaoUsdPrice", None, state)
        gamma = Client("local", substrate=sub).gamma
        assert asyncio.run(gamma.tao_usd_price())["accepted_usd"] == price

    def test_quotes_ask_in_chain_units(self):
        sub, asked = FakeSubstrate(), []
        for method in ("sim_mint_gamma", "sim_mint_gamma_with_stake"):
            sub.seed_runtime(
                "GammaRuntimeApi", method, lambda p: asked.append(p) or {"Ok": (1, 2, 3)}
            )
        gamma = Client("local", substrate=sub).gamma
        asyncio.run(gamma.quote_mint_gamma(netuid=3, amount_tao=1.5))
        asyncio.run(gamma.quote_mint_gamma_with_stake(netuid=3, amount_alpha=2))
        assert asked == [[3, 1_500_000_000], [3, 2 * RAO]]

    @pytest.mark.parametrize(
        ("method", "read", "amount"),
        [("sim_mint_gamma", "quote_mint_gamma", {"amount_tao": 1}),
         ("sim_mint_gamma_with_stake", "quote_mint_gamma_with_stake", {"amount_alpha": 1})],
    )  # fmt: skip
    def test_a_refused_quote_raises_the_chain_error(self, method, read, amount):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", method, {"Err": GAMMA_DISABLED})
        gamma = Client("local", substrate=sub).gamma
        with pytest.raises(ChainError) as refused:
            asyncio.run(getattr(gamma, read)(netuid=1, **amount))
        assert refused.value.name == "GammaDisabled"

    def test_deposit_reads_are_exact_and_tell_no_cap_from_a_paused_one(self):
        def info(cap):
            daily_spend_cap = None if cap is None else {"cap": cap, "bucket": (0, 0)}
            deposit = {"coldkey": ALICE, "balance": 3 * RAO + 1, "daily_spend_cap": daily_spend_cap}
            return {"netuid": 1, "hotkey": BOB_HOT, "deposit": deposit, "spendable": RAO // 2}

        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "get_gamma_deposit", info(0))
        sub.seed_runtime("GammaRuntimeApi", "get_gamma_deposits_for_coldkey", [info(None)])
        gamma = Client("local", substrate=sub).gamma
        assert asyncio.run(gamma.gamma_deposit(netuid=1, hotkey_ss58=BOB_HOT)) == {
            "netuid": 1,
            "hotkey": BOB_HOT,
            "coldkey": ALICE,
            "balance_gamma": "3.000000001",
            "daily_spend_cap_gamma": "0",
            "spendable_gamma": "0.5",
        }
        [listed] = asyncio.run(gamma.gamma_deposits(coldkey_ss58=ALICE))
        assert listed["daily_spend_cap_gamma"] is None

    def test_spent_is_exact_and_zero_before_a_spend(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaSpent", [1, ALICE_HOT, BOB_HOT], 3 * RAO + 1)
        gamma = Client("local", substrate=sub).gamma
        spent = [
            asyncio.run(
                gamma.gamma_spent(netuid=1, service_hotkey_ss58=service, hotkey_ss58=BOB_HOT)
            )
            for service in (ALICE_HOT, BOB_HOT)
        ]
        assert spent == ["3.000000001", "0"]


class TestMintPreview:
    @pytest.mark.asyncio
    async def test_quote_facts(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "Owner", [BOB_HOT], SIGNER)
        # 1 TAO buys alpha valued at 0.5 TAO, 200 gamma at $400.
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", {"Ok": (RAO, RAO // 2, 200 * RAO)})
        preview = await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)
        facts = dict(preview.facts)
        assert "200" in facts["gamma"] and "190" in facts["minimum"]
        assert preview.blocks == []
        assert preview.preview == {
            "gamma": "200",
            "minimum_gamma": "190",
            "alpha_burned": "1",
            "tao_value": "0.5",
            "deposit_balance_gamma": None,
            "daily_spend_cap_gamma": None,
        }

    @pytest.mark.asyncio
    async def test_another_coldkeys_deposit_blocks(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], _deposit(coldkey=BOB_HOT))
        preview = await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)
        assert any("belongs to another coldkey" in block for block in preview.blocks)

    @pytest.mark.parametrize(
        ("owner", "deposit", "blocked"),
        [(SIGNER, None, False), (ALICE, None, True),
         (ss58_encode(bytes(32), 42), None, True), (None, None, False),
         (ALICE, _deposit(), False)],
        ids=["owned", "foreign", "stored-zero-owner", "unassociated", "top-up"],
    )  # fmt: skip
    @pytest.mark.parametrize(
        "op,args", [("mint_gamma", MINT), ("mint_gamma_with_stake", MINT_STAKE)]
    )
    @pytest.mark.asyncio
    async def test_a_new_deposit_rejects_another_hotkeys_owner(
        self, owner, deposit, blocked, op, args
    ):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "Owner", [BOB_HOT], owner)
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], deposit)
        preview = await build(op, args).preflight(sub, SIGNER, SIGNER)
        assert bool(preview.blocks) is blocked
        if blocked:
            assert (
                "hotkey" in preview.blocks[0] and "belongs to another coldkey" in preview.blocks[0]
            )

    @pytest.mark.parametrize(
        ("amount", "staked", "warned"),
        [(1.0, RAO, False), (1.0, RAO // 4, True), ("all", RAO, True)],
    )
    @pytest.mark.asyncio
    async def test_stake_mint_warns_when_it_burns_the_whole_stake(self, amount, staked, warned):
        sub = FakeSubstrate()
        sub.seed_runtime(
            "StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", {"stake": staked}
        )
        intent = build("mint_gamma_with_stake", {**MINT_STAKE, "amount_alpha": amount})
        preview = await intent.preflight(sub, SIGNER, SIGNER)
        assert bool(preview.warnings) is warned


class TestMintTerms:
    @pytest.mark.parametrize(
        ("op", "args"), [("mint_gamma", MINT), ("mint_gamma_with_stake", MINT_STAKE)]
    )
    @pytest.mark.asyncio
    async def test_the_mint_names_the_terms(self, op, args, wallet):
        call = await build(op, {**args, "daily_spend_cap_gamma": "2.5"}).build(
            FakeSubstrate(), wallet
        )
        assert (call.module, call.function) == ("SubtensorModule", op)
        assert call.params["daily_spend_cap"] == 2_500_000_000

    @pytest.mark.parametrize(
        ("held", "named", "blocked"),
        [(None, None, False), (RAO, None, False), (RAO, 1, False), (RAO, 2, True),
         (None, 0, True)],
    )  # fmt: skip
    @pytest.mark.asyncio
    async def test_a_deposit_with_another_cap_blocks(self, held, named, blocked):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], _deposit(daily_spend_cap=held))
        intent = build("mint_gamma", {**MINT, "daily_spend_cap_gamma": named})
        preview = await intent.preflight(sub, SIGNER, SIGNER)
        assert any("daily spend cap" in block for block in preview.blocks) is blocked
        assert ("daily spend cap", "none" if held is None else "1 gamma") in preview.facts

    @pytest.mark.asyncio
    async def test_a_new_deposit_shows_the_named_cap(self):
        intent = build("mint_gamma", {**MINT, "daily_spend_cap_gamma": "2.5"})
        preview = await intent.preflight(FakeSubstrate(), SIGNER, SIGNER)
        assert ("daily spend cap", "2.5 gamma") in preview.facts

    @pytest.mark.parametrize(
        ("op", "named"),
        [("set_gamma_deposit_daily_spend_cap", {}),
         ("set_gamma_deposit_daily_spend_cap", {"daily_spend_cap_gamma": 1, "remove_cap": True})],
    )  # fmt: skip
    def test_a_setter_names_exactly_one_value(self, op, named):
        with pytest.raises(BittensorError, match="exactly one"):
            build(op, {**DEPOSIT, **named})

    @pytest.mark.parametrize(
        ("op", "fields"),
        [("mint_gamma", MINT), ("set_gamma_deposit_daily_spend_cap", DEPOSIT)],
    )
    def test_a_cap_is_zero_or_at_least_the_minimum(self, op, fields):
        with pytest.raises(BittensorError, match="0 or at least"):
            build(op, {**fields, "daily_spend_cap_gamma": "0.099999999"})
        for cap in (0, "0.1"):
            build(op, {**fields, "daily_spend_cap_gamma": cap})


class TestDepositPreviews:
    @pytest.mark.parametrize(
        ("deposit", "blocked"),
        [(None, True), (_deposit(coldkey=BOB_HOT), True), (_deposit(), False),
         (_deposit(balance=1), None)],
        ids=["missing", "foreign", "own", "funded"],
    )  # fmt: skip
    @pytest.mark.parametrize("op", ["close_gamma_deposit", "set_gamma_deposit_daily_spend_cap"])
    @pytest.mark.asyncio
    async def test_owner_calls_need_an_own_deposit(self, op, deposit, blocked):
        if blocked is None:  # only a close needs the deposit empty
            blocked = op == "close_gamma_deposit"
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], deposit)
        blocks = await build(op, INTENT_SAMPLES[op]).blocks(sub, SIGNER)
        assert len(blocks) == blocked

    @pytest.mark.parametrize(
        ("origin", "destination", "faulty"),
        [
            (None, None, ALICE_HOT),
            (_deposit(coldkey=BOB_HOT, balance=RAO), None, ALICE_HOT),
            (_deposit(), None, ALICE_HOT),
            (_deposit(balance=RAO), _deposit(coldkey=BOB_HOT), BOB_HOT),
            (_deposit(balance=RAO), None, None),
            (_deposit(balance=RAO), _deposit(), None),
        ],
        ids=["no-origin", "foreign-origin", "empty-origin", "foreign-destination", "new", "own"],
    )
    @pytest.mark.asyncio
    async def test_move_needs_an_own_funded_origin_and_destination(
        self, origin, destination, faulty
    ):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], origin)
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], destination)
        sub.seed("SubtensorModule", "Owner", [BOB_HOT], SIGNER)
        blocks = await build("move_gamma", {**MOVE, "amount_gamma": "all"}).blocks(sub, SIGNER)
        assert [faulty in block for block in blocks] == ([True] if faulty else [])

    @pytest.mark.parametrize(
        ("amount", "blocked"),
        [("all", False), (2, False), (0.9, False), (0.1, False), (0.05, True), (0.95, True)],
    )
    @pytest.mark.asyncio
    async def test_move_empties_the_origin_or_takes_and_leaves_the_minimum(self, amount, blocked):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], _deposit(balance=RAO))
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], _deposit())
        blocks = await build("move_gamma", {**MOVE, "amount_gamma": amount}).blocks(sub, SIGNER)
        assert bool(blocks) is blocked

    @pytest.mark.parametrize("owner", [SIGNER, ALICE, None, ss58_encode(bytes(32), 42)])
    @pytest.mark.asyncio
    async def test_move_opens_only_owned_or_unassociated_api_keys(self, owner):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], _deposit(balance=RAO))
        sub.seed("SubtensorModule", "Owner", [BOB_HOT], owner)
        blocks = await build("move_gamma", {**MOVE, "amount_gamma": "all"}).blocks(sub, SIGNER)
        assert bool(blocks) is (owner is not None and owner != SIGNER)
        if blocks:
            assert "hotkey" in blocks[0] and "belongs to another coldkey" in blocks[0]

    @pytest.mark.parametrize(
        "args",
        [{"destination_hotkey_ss58": ALICE_HOT}, {"amount_gamma": 0}],
        ids=["same-hotkey", "zero"],
    )
    def test_move_refuses_a_self_move_or_zero(self, args):
        with pytest.raises(BittensorError):
            build("move_gamma", {**MOVE, "amount_gamma": "all", **args})

    @pytest.mark.parametrize(
        ("origin", "amount", "closes"),
        [(_deposit(balance=RAO), "all", True), (_deposit(balance=RAO), 2, True),
         (_deposit(balance=RAO), 0.5, False), (None, "all", False)],
    )  # fmt: skip
    @pytest.mark.asyncio
    async def test_move_warns_when_it_closes_the_origin(self, origin, amount, closes):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], origin)
        intent = build("move_gamma", {**MOVE, "amount_gamma": amount})
        assert bool(await intent.warnings(sub, SIGNER)) is closes

    @pytest.mark.parametrize(
        ("destination", "warned"),
        [(None, []), (_deposit(), []),
         (_deposit(daily_spend_cap=RAO), ["daily spend cap, 1 gamma, instead of none"])],
        ids=["new", "same-terms", "other-cap"],
    )  # fmt: skip
    @pytest.mark.asyncio
    async def test_move_warns_when_the_destination_has_other_terms(self, destination, warned):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], _deposit(balance=2 * RAO))
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], destination)
        warnings = await build("move_gamma", {**MOVE, "amount_gamma": 1}).warnings(sub, SIGNER)
        assert len(warnings) == len(warned)
        assert all(text in warning for text, warning in zip(warned, warnings))

    @pytest.mark.parametrize(
        ("listed", "balance", "spendable", "amount", "blocked"),
        [(False, RAO, RAO, "1", "has no service"),
         (True, None, None, "1", "no gamma deposit"),
         (True, RAO, RAO, "0.05", "at least 0.1 gamma"),
         (True, RAO // 20, RAO // 20, "0.05", None),
         (True, 2 * RAO, RAO // 2, "1", "can spend 0.5 gamma now"),
         (True, RAO, RAO, "1", None)],
        ids=["no-service", "no-deposit", "below-minimum", "empties", "above-spendable", "spends"],
    )  # fmt: skip
    @pytest.mark.asyncio
    async def test_spend_blocks_what_the_chain_refuses(
        self, listed, balance, spendable, amount, blocked
    ):
        sub = FakeSubstrate()
        sub.seed(
            "SubtensorModule",
            "Services",
            [1, ALICE_HOT],
            {"url": "https://a.example"} if listed else None,
        )
        info = None
        if balance is not None:
            info = {"netuid": 1, "hotkey": SIGNER, "deposit": _deposit(balance=balance),
                    "spendable": spendable}  # fmt: skip
        sub.seed_runtime("GammaRuntimeApi", "get_gamma_deposit", info)
        blocks = await build("spend_gamma", {**SPEND, "amount_gamma": amount}).blocks(sub, SIGNER)
        assert [blocked in block for block in blocks] == ([] if blocked is None else [True])

    def test_spend_refuses_zero(self):
        with pytest.raises(BittensorError, match="above 0"):
            build("spend_gamma", {**SPEND, "amount_gamma": 0})


PAID = {"event": {"module_id": "TransactionPayment", "event_id": "TransactionFeePaid"}}
APPROVAL = {"event": {"module_id": "Multisig", "event_id": "MultisigApproval"}}


def _minted(cap):
    attributes = {
        "netuid": 1,
        "hotkey": BOB_HOT,
        "daily_spend_cap": cap,
        "tao": RAO,
        "alpha": RAO // 2,
        "value": RAO // 4,
        "gamma": 100 * RAO + 1,
    }
    return {"event": {"module_id": "SubtensorModule", "event_id": "GammaMinted",
                      "attributes": attributes}}  # fmt: skip


MINTED = {"hotkey_ss58": BOB_HOT, "daily_spend_cap_gamma": None, "alpha_burned": "0.5",
          "tao_value": "0.25", "gamma": "100.000000001"}  # fmt: skip


@pytest.mark.parametrize(
    ("events", "minted"),
    [
        ([PAID, _minted(None)], MINTED),
        ([_minted(RAO // 2)], {**MINTED, "daily_spend_cap_gamma": "0.5"}),
        ([], None),
        ([PAID, APPROVAL], None),
    ],
    ids=["minted", "capped", "not-waited", "multisig-pending"],
)  # fmt: skip
@pytest.mark.asyncio
async def test_execute_reports_what_a_mint_credited(wallet, events, minted):
    sub = FakeSubstrate()
    sub.seed("SubtensorModule", "Owner", [BOB_HOT], ALICE)
    sub.queue_result(ExtrinsicResult(success=True, message="", events=events))
    intent = build("mint_gamma", INTENT_SAMPLES["mint_gamma"])
    result = await Client("local", substrate=sub).execute(intent, wallet)
    assert result.data.get("minted") == minted


class TestCli:
    @pytest.fixture()
    def submitted(self, isolated_cli, monkeypatch):  # noqa: F811
        intents = []
        monkeypatch.setattr(AppContext, "submit", lambda self, intent, **kw: intents.append(intent))
        return intents

    def test_spend_takes_the_service_and_amount(self, submitted):
        result = invoke(
            "gamma", "spend", "--netuid", "2", "--service-hotkey", ALICE_HOT, "--amount", "0.5"
        )
        assert result.exit_code == 0, result.output
        assert [
            (intent.netuid, intent.service_hotkey_ss58, intent.amount_gamma) for intent in submitted
        ] == [(2, ALICE_HOT, "0.5")]

    @pytest.mark.parametrize(
        ("flags", "cap"),
        [(["--remove-cap"], None), (["--daily-spend-cap-gamma", "0"], "0"), ([], "refused"),
         (["--remove-cap", "--daily-spend-cap-gamma", "1"], "refused"),
         (["--daily-spend-cap-gamma", "1e-10"], "refused")],
    )  # fmt: skip
    def test_set_cap_takes_exactly_one_of_a_cap_or_its_removal(self, submitted, flags, cap):
        result = invoke("gamma", "set-cap", "--netuid", "1", "--hotkey", BOB_HOT, *flags)
        if cap == "refused":
            assert result.exit_code == 2 and not submitted
        else:
            assert result.exit_code == 0, result.output
            assert [intent.daily_spend_cap_gamma for intent in submitted] == [cap]

    def test_bad_rate_tolerance_is_an_error_line(self, submitted):
        result = invoke(
            "gamma", "mint", "--netuid", "1", "--hotkey", BOB_HOT, "--amount-tao", "1",
            "--rate-tolerance", "2",
        )  # fmt: skip
        assert result.exit_code == 2 and not submitted
        assert "Traceback" not in result.output


@pytest.mark.parametrize(
    "op",
    [
        "mint_gamma",
        "mint_gamma_with_stake",
        "close_gamma_deposit",
        "set_gamma_deposit_daily_spend_cap",
    ],
)
def test_deposit_hotkey_never_defaults_to_the_wallets(fake, op):  # noqa: F811
    args = {k: v for k, v in INTENT_SAMPLES[op].items() if k != "hotkey_ss58"}
    argv = argv_from_sample(args, lists="json")
    result = invoke("--json", "--dry-run", "tx", op.replace("_", "-"), *argv)
    assert result.exit_code == 2, result.output


@pytest.mark.parametrize(
    ("argv", "field", "value"),
    [
        (["move", "--origin-hotkey", ALICE_HOT, "--destination-hotkey", BOB_HOT, "--amount", "0.5"],
            "amount_gamma", "0.5"),
        (["move", "--origin-hotkey", ALICE_HOT, "--destination-hotkey", BOB_HOT, "--all"],
            "amount_gamma", "all"),
        (["mint-with-stake", "--hotkey", BOB_HOT, "--origin-hotkey", ALICE_HOT, "--all"],
            "amount_alpha", "all"),
    ],
)  # fmt: skip
def test_amount_flags(fake, argv, field, value):  # noqa: F811
    result = invoke("--json", "--dry-run", "gamma", *argv, "--netuid", "1", "-w", _WALLET_NAME)
    assert result.exit_code == 0, result.output
    assert json.loads(result.output)["args"][field] == value


def test_unmintable_mint_submits_nothing(fake, monkeypatch):  # noqa: F811
    monkeypatch.setattr(AppContext, "confirm", lambda self, prompt, **kw: True)
    fake.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", {"Err": GAMMA_DISABLED})
    result = invoke(
        "gamma", "mint", "--netuid", "1", "--hotkey", BOB_HOT, "--amount", "1", "-w", _WALLET_NAME
    )
    assert result.exit_code == 1
    assert "GammaDisabled" in result.output
    assert fake.submissions == []


@pytest.mark.parametrize(
    "op,args",
    [
        ("mint_gamma", MINT),
        ("mint_gamma_with_stake", MINT_STAKE),
        ("move_gamma", {**MOVE, "amount_gamma": "all"}),
    ],
)
@pytest.mark.asyncio
async def test_new_gamma_deposit_distinguishes_absence_from_default_owner(op, args):
    sub = FakeSubstrate()
    # Owner is ValueQuery: missing keys decode to the zero account on a real node.
    sub.seed_default("SubtensorModule", "Owner", ss58_encode(bytes(32), 42))
    sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], _deposit(balance=RAO))
    intent = build(op, args)
    if op == "move_gamma":
        assert not await intent.blocks(sub, SIGNER)
    else:
        assert not (await intent.preflight(sub, SIGNER, SIGNER)).blocks


@pytest.mark.asyncio
async def test_new_gamma_deposit_compares_owner_account_ids_across_ss58_prefixes():
    from bittensor.sp_core import ss58_decode

    sub = FakeSubstrate()
    sub.seed("SubtensorModule", "Owner", [BOB_HOT], ss58_encode(ss58_decode(SIGNER), 0))
    assert not (await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)).blocks
