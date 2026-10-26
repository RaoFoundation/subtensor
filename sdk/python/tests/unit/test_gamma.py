"""Gamma intents, previews, results and reads, the service-url checks, and the
`btcli deposits` input checks."""

from __future__ import annotations

import asyncio

import pytest

from bittensor import Client
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
        build("set_service_url", {"netuid": 1, "url": "https://" + "a" * 248})

    @pytest.mark.parametrize("url", ["not a url at all", "ftp://a.example", "https://"])
    def test_rejects_non_http_urls(self, url):
        with pytest.raises(BittensorError, match="not an http"):
            build("set_service_url", {"netuid": 1, "url": url})

    @pytest.mark.parametrize("url", ["a" * 257, "é" * 129])
    def test_rejects_over_256_utf8_bytes(self, url):
        with pytest.raises(BittensorError, match="at most 256"):
            build("set_service_url", {"netuid": 1, "url": url})


class TestServiceUrlsRead:
    def test_urls_with_control_or_format_characters_read_as_hex(self):
        fake = FakeSubstrate()
        spoof = "https://a.example\x1b[1A\x1b[2K"
        fake.seed_map(
            "SubtensorModule",
            "ServiceUrls",
            [
                (ALICE_HOT, {"block": 1, "url": b"https://a.example"}),
                (BOB_HOT, {"block": 1, "url": spoof.encode()}),
                (BOB_HOT + "x", {"block": 1, "url": "https://\u202eexample".encode()}),
            ],
        )
        fake.seed("SubtensorModule", "SubnetOwnerHotkey", [1], ALICE_HOT)
        urls = asyncio.run(Client("local", substrate=fake).read("service_urls", netuid=1))
        assert urls[ALICE_HOT] == {"url": "https://a.example", "block": 1, "owner": True}
        assert urls[BOB_HOT]["url"] == "0x" + spoof.encode().hex()
        assert not urls[BOB_HOT]["owner"]
        assert urls[BOB_HOT + "x"]["url"].startswith("0x")


class TestCliSpend:
    @pytest.fixture()
    def submitted(self, isolated_cli, monkeypatch):  # noqa: F811
        intents = []
        monkeypatch.setattr(AppContext, "submit", lambda self, intent, **kw: intents.append(intent))
        return intents

    def test_parses_repeated_hotkey_amount(self, submitted):
        spends = ["--spend", f"{BOB_HOT}=0.5", "--spend", f"{ALICE_HOT}=3"]
        result = invoke("deposits", "spend", "--netuid", "2", *spends)
        assert result.exit_code == 0, result.output
        [intent] = submitted
        assert intent.netuid == 2
        assert intent.spends == [
            {"hotkey_ss58": BOB_HOT, "amount_gamma": "0.5"},
            {"hotkey_ss58": ALICE_HOT, "amount_gamma": "3"},
        ]

    @pytest.mark.parametrize("entry", [BOB_HOT, "=1"])
    def test_rejects_malformed_entry(self, submitted, entry):
        result = invoke("deposits", "spend", "--netuid", "2", "--spend", entry)
        assert result.exit_code == 2
        assert not submitted


SIGNER = ALICE_HOT  # any account, as the dispatch origin


def _deposit(coldkey: str = SIGNER, balance: int = 0, **fields) -> dict:
    return {"coldkey": coldkey, "spender": coldkey, "balance": balance, "bucket": (0, 0), **fields}


class TestMintPreview:
    @pytest.mark.asyncio
    async def test_quote_facts(self):
        sub = FakeSubstrate()
        # 1 TAO buys alpha valued at 0.5 TAO, 200 gamma at $400.
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO // 2, 200 * RAO))
        preview = await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)
        facts = dict(preview.facts)
        assert preview.facts_title == "Quote"
        assert facts["gamma"] == "~200.0000 gamma"
        assert facts["minimum"] == "190.0000 gamma"
        assert facts["deposit"] == "new"
        assert "subnet owner" in facts["spender"]
        assert preview.blocks == []

    @pytest.mark.asyncio
    async def test_another_coldkeys_deposit_blocks(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], _deposit(coldkey=BOB_HOT))
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO, 400 * RAO))
        preview = await build("mint_gamma", MINT).preflight(sub, SIGNER, SIGNER)
        assert any("belongs to another coldkey" in block for block in preview.blocks)

    @pytest.mark.asyncio
    async def test_stake_preview_warns_when_less_is_staked(self):
        sub = FakeSubstrate()
        sub.seed_runtime(
            "StakeInfoRuntimeApi", "get_stake_info_for_hotkey_coldkey_netuid", {"stake": RAO // 4}
        )
        sub.seed_runtime(
            "GammaRuntimeApi", "sim_mint_gamma_with_stake", lambda p: (p[1], p[1], 100 * RAO)
        )
        intent = build("mint_gamma_with_stake", {**MINT_STAKE, "origin_hotkey_ss58": BOB_HOT})
        preview = await intent.preflight(sub, SIGNER, SIGNER)
        assert any("is staked; that is burned" in warning for warning in preview.warnings)


class TestMintSpender:
    @pytest.mark.asyncio
    async def test_names_the_spender_in_one_batch(self, wallet):
        sub = FakeSubstrate()
        sub.seed_runtime("GammaRuntimeApi", "sim_mint_gamma", (RAO, RAO, 400 * RAO))
        call = await build("mint_gamma", {**MINT, "spender_ss58": ALICE_HOT}).build(sub, wallet)
        assert (call.module, call.function) == ("Utility", "batch_all")
        assert [inner.function for inner in call.params["calls"]] == [
            "mint_gamma",
            "set_gamma_deposit_spender",
        ]

    @pytest.mark.asyncio
    async def test_all_burns_the_whole_stake(self, wallet):
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
        intent = build("mint_gamma_with_stake", {**MINT_STAKE, "amount_alpha": "all"})
        call = await intent.build(sub, wallet)
        assert quoted == [RAO // 4]
        assert call.params["alpha_amount"] == 2**64 - 1


class TestDepositPreviews:
    @pytest.mark.parametrize(
        ("deposit", "block"),
        [
            (None, "no gamma deposit"),
            (_deposit(coldkey=BOB_HOT), "belongs to another coldkey"),
            (_deposit(balance=RAO), "holds 1.0000 gamma"),
        ],
    )
    @pytest.mark.asyncio
    async def test_close_needs_an_own_empty_deposit(self, deposit, block):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], deposit)
        intent = build("close_gamma_deposit", {"netuid": 1, "hotkey_ss58": BOB_HOT})
        assert any(block in found for found in await intent.blocks(sub, SIGNER))

    @pytest.mark.asyncio
    async def test_move_into_another_coldkeys_deposit_blocks(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], _deposit())
        sub.seed("SubtensorModule", "GammaDeposits", [1, BOB_HOT], _deposit(coldkey=BOB_HOT))
        intent = build(
            "move_gamma",
            {
                "netuid": 1,
                "origin_hotkey_ss58": ALICE_HOT,
                "destination_hotkey_ss58": BOB_HOT,
                "amount_gamma": "all",
            },
        )
        assert await intent.blocks(sub, SIGNER) == [
            f"the gamma deposit at {BOB_HOT} belongs to another coldkey"
        ]

    @pytest.mark.asyncio
    async def test_spend_warns_of_skipped_entries(self):
        sub = FakeSubstrate()
        sub.seed("SubtensorModule", "GammaDeposits", [1, ALICE_HOT], _deposit())
        spends = [
            {"hotkey_ss58": ALICE_HOT, "amount_gamma": 1},
            {"hotkey_ss58": BOB_HOT, "amount_gamma": 2},
        ]
        intent = build("spend_gamma", {"netuid": 1, "spends": spends})
        assert intent.summary() == "spend up to 3 gamma from 2 deposits on netuid 1"
        (warning,) = await intent.warnings(sub, SIGNER)
        assert warning.startswith("1 of 2 entries would be skipped") and BOB_HOT in warning
        assert await intent.warnings(sub, BOB_HOT) == [
            "nothing would be spent: no entry is a deposit you are the spender of"
        ]


class TestResults:
    def test_spent_gamma_by_hotkey(self):
        from bittensor.executor import _gamma_spent_data
        from bittensor.result import ExtrinsicResult

        def spent(hotkey, amount):
            return {"event": {"module_id": "SubtensorModule", "event_id": "GammaSpent",
                              "attributes": {"hotkey": hotkey, "amount": amount}}}  # fmt: skip

        result = ExtrinsicResult(
            success=True, message="", events=[spent(ALICE_HOT, RAO), spent(BOB_HOT, RAO // 2)]
        )
        assert _gamma_spent_data(result) == {"spent": {ALICE_HOT: 1.0, BOB_HOT: 0.5}}
        nothing = ExtrinsicResult(success=True, message="", events=[])
        assert _gamma_spent_data(nothing) == {"spent": {}}


@pytest.mark.usefixtures("isolated_cli")
class TestCliRefusals:
    def test_bad_rate_tolerance_is_an_error_line(self):
        result = invoke(
            "deposits", "mint", "--netuid", "1", "--hotkey", BOB_HOT, "--amount-tao", "1",
            "--rate-tolerance", "2", "--dry-run",
        )  # fmt: skip
        assert result.exit_code == 2
        assert "Traceback" not in result.output

    def test_coldkey_params_refuse_a_hotkey_reference(self):
        result = invoke(
            "deposits", "set-spender", "--netuid", "1", "--hotkey", BOB_HOT,
            "--spender", "testwallet/default", "--dry-run",
        )  # fmt: skip
        assert result.exit_code == 1
        assert "names a" in result.output and "hotkey" in result.output
