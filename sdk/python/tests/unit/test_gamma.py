"""Gamma intents, previews, results and reads, the service-url checks, and the
`btcli gamma` input checks."""

from __future__ import annotations

import asyncio
import json

import pytest

from bittensor import Client
from bittensor.cli.context import AppContext
from bittensor.intents import build
from bittensor.intents._money import UNBOUNDED, gamma_amount, gamma_units
from bittensor.result import BittensorError, ExtrinsicResult
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
SIGNER = ALICE_HOT  # any account, as the dispatch origin


def _deposit(coldkey: str = SIGNER, balance: int = 0, **fields) -> dict:
    return {"coldkey": coldkey, "spender": coldkey, "balance": balance, "bucket": (0, 0), **fields}


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
            "min_amount_out": 380 * RAO}),
        ("mint_gamma_with_stake", MINT_STAKE, {"origin_hotkey": ALICE_HOT,
            "destination_hotkey": BOB_HOT, "netuid": 1, "alpha_amount": RAO,
            "min_amount_out": 380 * RAO}),
        ("move_gamma", {**MOVE, "amount_gamma": "1.5"}, {"origin_hotkey": ALICE_HOT,
            "destination_hotkey": BOB_HOT, "netuid": 1, "amount": 1_500_000_000}),
        ("move_gamma", {**MOVE, "amount_gamma": "all"}, {"origin_hotkey": ALICE_HOT,
            "destination_hotkey": BOB_HOT, "netuid": 1, "amount": 2**64 - 1}),
        ("set_gamma_deposit_daily_spend_cap", {**DEPOSIT, "daily_spend_cap_gamma": "2.5"},
            {"hotkey": BOB_HOT, "netuid": 1, "daily_spend_cap": 2_500_000_000}),
        ("set_gamma_deposit_daily_spend_cap", {**DEPOSIT, "daily_spend_cap_gamma": 0},
            {"hotkey": BOB_HOT, "netuid": 1, "daily_spend_cap": 0}),
        ("set_gamma_deposit_daily_spend_cap", DEPOSIT,
            {"hotkey": BOB_HOT, "netuid": 1, "daily_spend_cap": None}),
        ("spend_gamma", {"netuid": 1, "spends": [{"hotkey_ss58": BOB_HOT, "amount_gamma": "0.5",
            "reference": "ab" * 32}]},
            {"netuid": 1, "spends": [(BOB_HOT, RAO // 2, "0x" + "ab" * 32)]}),
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
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO, gamma))
        call = await build("mint_gamma", {**MINT, "rate_tolerance": tolerance}).build(sub, wallet)
        assert call.params["min_amount_out"] == minimum

    @pytest.mark.asyncio
    async def test_no_quote_refuses(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", None)
        with pytest.raises(BittensorError, match="cannot be minted"):
            await build("mint_gamma", MINT).build(sub, wallet)


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
            lambda p: quoted.append(p[1]) or (p[1], p[1], 100 * RAO),
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

    def test_spend_is_unbounded(self):
        assert build("mint_gamma_with_stake", MINT_STAKE).spend() is UNBOUNDED


class TestServiceUrl:
    @pytest.mark.parametrize("url", ["", "https://" + "a" * 248])
    def test_accepts_an_empty_url_and_256_bytes(self, url):
        build("set_service_url", {"netuid": 1, "url": url})

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
            build("set_service_url", {"netuid": 1, "url": url})


class TestServiceUrlsRead:
    def test_urls_with_control_or_format_characters_read_as_hex(self):
        sub = FakeSubstrate()
        spoof = "https://a.example\x1b[1A\x1b[2K"
        sub.seed_map(
            "SubtensorModule",
            "ServiceUrls",
            [
                (ALICE_HOT, {"block": 1, "url": "https://a.example"}),
                (BOB_HOT, {"block": 1, "url": spoof}),
                (BOB_HOT + "x", {"block": 1, "url": "https://\u202eexample"}),
                (BOB_HOT + "y", {"block": 1, "url": "0x41"}),
                (BOB, {"block": 2, "url": ""}),
            ],
        )
        sub.seed("SubtensorModule", "SubnetOwnerHotkey", [1], ALICE_HOT)
        urls = asyncio.run(Client("local", substrate=sub).subnets.service_urls(netuid=1))
        assert urls[ALICE_HOT] == {"url": "https://a.example", "block": 1, "owner": True}
        assert urls[BOB_HOT]["url"] == "0x" + spoof.encode().hex()
        assert not urls[BOB_HOT]["owner"]
        assert urls[BOB_HOT + "x"]["url"].startswith("0x")
        assert urls[BOB_HOT + "y"]["url"] == "0x41"
        assert BOB not in urls


class TestGammaReads:
    def test_amounts_are_exact_and_the_feed_has_an_age(self):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO, 2**53 + 1))
        price = {"accepted": 312_450_000_000, "feed": 312_500_000_000, "fed_at": sub.block - 7}
        sub.seed("SubtensorModule", "TaoUsdPrice", None, price)
        gamma = Client("local", substrate=sub).gamma
        quote = asyncio.run(gamma.quote_mint_gamma(netuid=1, amount_tao=1))
        assert quote["gamma"] == "9007199.254740993"
        price = asyncio.run(gamma.tao_usd_price())
        assert (price["accepted_usd"], price["blocks_since_last_feed"]) == (312.45, 7)

    def test_quotes_ask_in_chain_units(self):
        sub, asked = FakeSubstrate(), []
        for method in ("sim_mint_gamma", "sim_mint_gamma_with_stake"):
            sub.seed_runtime("GammaRuntimeApi", method, lambda p: asked.append(p) or (1, 2, 3))
        gamma = Client("local", substrate=sub).gamma
        asyncio.run(gamma.quote_mint_gamma(netuid=3, amount_tao=1.5))
        asyncio.run(gamma.quote_mint_gamma_with_stake(netuid=3, amount_alpha=2))
        assert asked == [[3, 1_500_000_000], [3, 2 * RAO]]

    def test_deposit_reads_are_exact_and_tell_no_cap_from_a_paused_one(self):
        def info(cap):
            deposit = {"coldkey": ALICE, "spender": BOB, "balance": 3 * RAO + 1,
                       "daily_spend_cap": cap, "bucket": (0, 0)}  # fmt: skip
            return {"netuid": 1, "hotkey": BOB_HOT, "deposit": deposit, "spendable": RAO // 2}

        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "get_gamma_deposit", info(0))
        sub.seed_runtime("GammaRuntimeApi", "get_gamma_deposits_for_coldkey", [info(None)])
        gamma = Client("local", substrate=sub).gamma
        assert asyncio.run(gamma.gamma_deposit(netuid=1, hotkey_ss58=BOB_HOT)) == {
            "netuid": 1,
            "hotkey": BOB_HOT,
            "coldkey": ALICE,
            "spender": BOB,
            "balance_gamma": "3.000000001",
            "daily_spend_cap_gamma": "0",
            "spendable_gamma": "0.5",
        }
        [listed] = asyncio.run(gamma.gamma_deposits(coldkey_ss58=ALICE))
        assert listed["daily_spend_cap_gamma"] is None


class TestMintPreview:
    @pytest.mark.asyncio
    async def test_quote_facts(self):
        sub = FakeSubstrate()
        # 1 TAO buys alpha valued at 0.5 TAO, 200 gamma at $400.
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO // 2, 200 * RAO))
        preview = await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)
        facts = dict(preview.facts)
        assert "200" in facts["gamma"] and "190" in facts["minimum"]
        assert preview.blocks == []

    @pytest.mark.asyncio
    async def test_another_coldkeys_deposit_blocks(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], _deposit(coldkey=BOB_HOT))
        preview = await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)
        assert any("belongs to another coldkey" in block for block in preview.blocks)

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


class TestMintSpender:
    @pytest.mark.asyncio
    async def test_names_the_spender_in_one_batch(self, wallet):
        call = await build("mint_gamma", {**MINT, "spender_ss58": ALICE}).build(
            FakeSubstrate(), wallet
        )
        assert (call.module, call.function) == ("Utility", "batch_all")
        assert [tuple(inner)[1:] for inner in call.params["calls"]] == [
            ("mint_gamma", {"hotkey": BOB_HOT, "netuid": 1, "amount": RAO,
                "min_amount_out": 380 * RAO}),
            ("set_gamma_deposit_spender", {"hotkey": BOB_HOT, "netuid": 1, "spender": ALICE}),
        ]  # fmt: skip


class TestDepositPreviews:
    @pytest.mark.parametrize(
        ("deposit", "blocked"),
        [(None, True), (_deposit(coldkey=BOB_HOT), True), (_deposit(), False),
         (_deposit(balance=1), None)],
        ids=["missing", "foreign", "own", "funded"],
    )  # fmt: skip
    @pytest.mark.parametrize(
        "op",
        ["close_gamma_deposit", "set_gamma_deposit_spender", "set_gamma_deposit_daily_spend_cap"],
    )
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
        blocks = await build("move_gamma", {**MOVE, "amount_gamma": "all"}).blocks(sub, SIGNER)
        assert [faulty in block for block in blocks] == ([True] if faulty else [])

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
        ("signer", "deposits", "named"),
        [(SIGNER, (ALICE_HOT, BOB_HOT), []), (SIGNER, (ALICE_HOT,), [[BOB_HOT]]),
         (BOB_HOT, (ALICE_HOT,), [[]])],
        ids=["none", "one", "all"],
    )  # fmt: skip
    @pytest.mark.asyncio
    async def test_spend_warns_of_skipped_entries(self, signer, deposits, named):
        sub = FakeSubstrate()
        for hotkey in deposits:
            sub.seed("SubtensorModule", "GammaDeposits", [1, hotkey], _deposit())
        spends = [{"hotkey_ss58": hotkey, "amount_gamma": 1} for hotkey in (ALICE_HOT, BOB_HOT)]
        warnings = await build("spend_gamma", {"netuid": 1, "spends": spends}).warnings(sub, signer)
        hotkeys = (ALICE_HOT, BOB_HOT)
        assert [[hotkey for hotkey in hotkeys if hotkey in w] for w in warnings] == named

    @pytest.mark.parametrize(
        ("reference", "sent"),
        [(None, "0x" + "00" * 32), ("AB" * 32, "0x" + "ab" * 32), ("0x1234", None),
         ("zz" * 32, None)],
    )  # fmt: skip
    def test_spend_reference_is_32_bytes_and_zero_by_default(self, reference, sent):
        spends = [{"hotkey_ss58": BOB_HOT, "amount_gamma": 1, "reference": reference}]
        if sent is None:
            with pytest.raises(BittensorError):
                build("spend_gamma", {"netuid": 1, "spends": spends})
        else:
            intent = build("spend_gamma", {"netuid": 1, "spends": spends})
            assert intent.spends[0]["reference"] == sent

    @pytest.mark.parametrize(
        "spends",
        [
            [],
            [{"hotkey_ss58": BOB_HOT, "amount_gamma": 1}] * 513,
            [{"hotkey_ss58": BOB_HOT}],
            [{"hotkey_ss58": BOB_HOT, "amount_gamma": 1, "amount": 1}],
            ["5F"],
        ],
        ids=["none", "513", "missing-key", "unknown-key", "not-an-object"],
    )
    def test_spend_refuses_bad_entries(self, spends):
        with pytest.raises(BittensorError):
            build("spend_gamma", {"netuid": 1, "spends": spends})


def _spent(hotkey, amount, reference):
    attributes = {"hotkey": hotkey, "amount": amount, "reference": reference}
    return {"event": {"module_id": "SubtensorModule", "event_id": "GammaSpent",
                      "attributes": attributes}}  # fmt: skip


PAID = {"event": {"module_id": "TransactionPayment", "event_id": "TransactionFeePaid"}}
APPROVAL = {"event": {"module_id": "Multisig", "event_id": "MultisigApproval"}}


@pytest.mark.parametrize(
    ("events", "spent"),
    [
        (
            [_spent(ALICE_HOT, RAO, "0x" + "01" * 32),
             _spent(ALICE_HOT, RAO // 4, "0x" + "02" * 32),
             _spent(BOB_HOT, RAO // 2, "0x" + "00" * 32)],
            [{"hotkey_ss58": ALICE_HOT, "amount_gamma": "1", "reference": "0x" + "01" * 32},
             {"hotkey_ss58": ALICE_HOT, "amount_gamma": "0.25", "reference": "0x" + "02" * 32},
             {"hotkey_ss58": BOB_HOT, "amount_gamma": "0.5", "reference": "0x" + "00" * 32}],
        ),
        ([PAID], []),
        ([], None),
        ([PAID, APPROVAL], None),
    ],
    ids=["spent", "all-skipped", "not-waited", "multisig-pending"],
)  # fmt: skip
@pytest.mark.asyncio
async def test_execute_reports_what_each_entry_spent(wallet, events, spent):
    sub = FakeSubstrate()
    sub.queue_result(ExtrinsicResult(success=True, message="", events=events))
    intent = build("spend_gamma", INTENT_SAMPLES["spend_gamma"])
    result = await Client("local", substrate=sub).execute(intent, wallet)
    assert result.data.get("spent") == spent


class TestCli:
    @pytest.fixture()
    def submitted(self, isolated_cli, monkeypatch):  # noqa: F811
        intents = []
        monkeypatch.setattr(AppContext, "submit", lambda self, intent, **kw: intents.append(intent))
        return intents

    def test_spend_parses_repeated_hotkey_amount(self, submitted):
        reference = "0x" + "ab" * 32
        spends = ["--spend", f"{BOB_HOT}=0.5", "--spend", f"{ALICE_HOT}=3:{reference}"]
        result = invoke("gamma", "spend", "--netuid", "2", *spends)
        assert result.exit_code == 0, result.output
        [intent] = submitted
        assert intent.netuid == 2
        assert intent.spends == [
            {"hotkey_ss58": BOB_HOT, "amount_gamma": "0.5", "reference": "0x" + "00" * 32},
            {"hotkey_ss58": ALICE_HOT, "amount_gamma": "3", "reference": reference},
        ]

    @pytest.mark.parametrize(
        "entry", [BOB_HOT, "=1", f"{BOB_HOT}=", f"{BOB_HOT}=1:0x12", "nosuch=1"]
    )
    def test_spend_rejects_malformed_entry(self, submitted, entry):
        result = invoke("gamma", "spend", "--netuid", "2", "--spend", entry)
        assert result.exit_code != 0 and isinstance(result.exception, SystemExit)
        assert not submitted

    @pytest.mark.parametrize(
        ("flags", "cap"),
        [(["--no-cap"], None), (["--daily-spend-cap-gamma", "0"], "0"), ([], "refused"),
         (["--no-cap", "--daily-spend-cap-gamma", "1"], "refused"),
         (["--daily-spend-cap-gamma", "1e-10"], "refused")],
    )  # fmt: skip
    def test_set_cap_takes_exactly_one_of_a_cap_or_no_cap(self, submitted, flags, cap):
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
        "set_gamma_deposit_spender",
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
    fake.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", None)
    result = invoke(
        "gamma", "mint", "--netuid", "1", "--hotkey", BOB_HOT, "--amount", "1", "-w", _WALLET_NAME
    )
    assert result.exit_code == 1
    assert "cannot be minted" in result.output
    assert fake.submissions == []
