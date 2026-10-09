"""Loan units, full runtime quotes and the three-command CLI contract."""

from __future__ import annotations

import json

import pytest
from typer.main import get_command
from typer.testing import CliRunner

from bittensor._generated.errors import ERRORS
from bittensor.balance import Balance, UnitMismatchError
from bittensor.cli.commands.lending import _bound, _minimum_collateral
from bittensor.cli.main import app
from bittensor.client import Client
from bittensor.intents import CloseLoan, OpenLoan, Policy
from bittensor.result import BittensorError, ChainError
from tests.harness.fake_substrate import DEFAULT_STORAGE, FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, dev_wallet
from tests.unit.test_cli_commands import fake as _cli_fake
from tests.unit.test_cli_commands import isolated_cli, wallet_dir  # noqa: F401

runner = CliRunner()


def _seed_minimum_quotes(fake, threshold, ceiling=(1 << 64) - 1, target=1_000_000_000):
    fake.seed_constant("Lending", "MinimumLoanValue", target)
    calls = []

    def quote(params):
        calls.append(params)
        amount = params[3]
        if amount > ceiling:
            return {"Err": {"Module": {"index": 33, "error": "0x08000000"}}}
        value = amount * target // threshold
        if value < target:
            return {"Err": {"Module": {"index": 33, "error": "0x06000000"}}}
        return {"Ok": {"principal": value, "opening_value": value, "annual_interest": value}}

    fake.seed_runtime("LendingRuntimeApi", "quote_open_for", quote)
    return calls


@pytest.mark.parametrize("side,unit", [("short", 0), ("long", 1)])
@pytest.mark.asyncio
async def test_minimum_collateral_search_is_exact_and_owner_aware(side, unit):
    fake = FakeSubstrate()
    threshold = 95_500_000_001
    calls = _seed_minimum_quotes(fake, threshold)
    _seed_position(fake, side.title())
    async with Client("local", substrate=fake) as client:
        minimum, target = await _minimum_collateral(client, 1, side, ALICE, ALICE_HOT)
    assert minimum == Balance.from_rao(threshold, unit)
    assert target == Balance.from_rao(1_000_000_000)
    assert all(p[0] == ALICE and p[1] == 1 and p[4] == ALICE_HOT for p in calls)
    assert len(calls) <= 128


@pytest.mark.asyncio
async def test_minimum_collateral_search_does_not_skip_a_narrow_inventory_window():
    fake = FakeSubstrate()
    threshold = 95_500_000_001
    _seed_minimum_quotes(fake, threshold, ceiling=threshold + 1000)
    async with Client("local", substrate=fake) as client:
        minimum, _ = await _minimum_collateral(client, 1, "short", ALICE, ALICE_HOT)
    assert minimum.rao == threshold


@pytest.mark.asyncio
async def test_minimum_collateral_search_refuses_when_inventory_cannot_fund_target():
    fake = FakeSubstrate()
    _seed_minimum_quotes(fake, 95_500_000_001, ceiling=95_500_000_000)
    async with Client("local", substrate=fake) as client:
        with pytest.raises(BittensorError, match="inventory"):
            await _minimum_collateral(client, 1, "short", ALICE, ALICE_HOT)


@pytest.mark.asyncio
async def test_minimum_collateral_search_respects_higher_runtime_minimum():
    fake = FakeSubstrate()
    _seed_minimum_quotes(fake, 200_000_000_000, target=2_000_000_000)
    async with Client("local", substrate=fake) as client:
        minimum, target = await _minimum_collateral(client, 1, "short", ALICE, ALICE_HOT)
    assert minimum.rao == 200_000_000_000
    assert target.rao == 2_000_000_000


@pytest.mark.asyncio
async def test_minimum_collateral_search_preserves_reference_failures():
    fake = FakeSubstrate()
    fake.seed_constant("Lending", "MinimumLoanValue", 1_000_000_000)
    fake.seed_runtime(
        "LendingRuntimeApi",
        "quote_open_for",
        {"Err": {"Module": {"index": 33, "error": "0x03000000"}}},
    )
    async with Client("local", substrate=fake) as client:
        with pytest.raises(ChainError) as failure:
            await _minimum_collateral(client, 1, "short", ALICE, ALICE_HOT)
    assert failure.value.name == "ReferenceWarmingUp"


@pytest.fixture()
def cli_fake(request, monkeypatch):
    request.getfixturevalue("isolated_cli")
    return _cli_fake.__wrapped__(None, monkeypatch)


def test_cli_collateral_prompt_shows_estimate_and_requotes_selection(cli_fake, monkeypatch):
    from bittensor.cli import prompt
    from bittensor.cli.commands import lending

    monkeypatch.setattr(lending, "interactive", lambda _ctx: True)
    monkeypatch.setattr(prompt, "interactive", lambda _ctx: True)
    calls = _seed_minimum_quotes(cli_fake, 95_500_000_001)
    result = runner.invoke(
        app,
        [
            "--dry-run",
            "--yes",
            "lending",
            "open",
            "--netuid",
            "1",
            "--side",
            "short",
            "--hotkey",
            ALICE_HOT,
        ],
        input="\n",
    )
    assert result.exit_code == 0, result.output
    assert "95.500000001" in result.output
    assert "Estimated minimum additional collateral" in result.output
    assert "borrowed alpha" in result.output
    assert calls[-1][3] == 95_500_000_001
    assert cli_fake.submissions == []


@pytest.mark.parametrize("side", ["short", "long"])
@pytest.mark.parametrize(
    "error_index,name", [(14, "InsufficientEscrow"), (21, "InsufficientRedemptionBacking")]
)
def test_cli_growth_estimate_failure_allows_larger_manual_collateral(
    cli_fake, monkeypatch, side, error_index, name
):
    from bittensor.cli import prompt
    from bittensor.cli.commands import lending

    monkeypatch.setattr(lending, "interactive", lambda _ctx: True)
    monkeypatch.setattr(prompt, "interactive", lambda _ctx: True)
    _seed_position(cli_fake, side.title())
    cli_fake.seed_constant("Lending", "MinimumLoanValue", 1_000_000_000)
    calls = []

    def quote(params):
        calls.append(params[3])
        if params[3] < 10_000_000_000:
            return {"Err": {"Module": {"index": 33, "error": f"0x{error_index:02x}000000"}}}
        return {
            "Ok": {
                "principal": 1_000_000_000,
                "opening_value": 1_000_000_000,
                "annual_interest": 1_000_000_000,
            }
        }

    cli_fake.seed_runtime("LendingRuntimeApi", "quote_open_for", quote)
    result = runner.invoke(
        app,
        [
            "--dry-run",
            "--yes",
            "lending",
            "open",
            "--netuid",
            "1",
            "--side",
            side,
            "--hotkey",
            ALICE_HOT,
        ],
        input="10\n",
    )
    assert result.exit_code == 0, result.output
    assert "Minimum collateral estimate unavailable" in result.output
    assert name in result.output
    assert "Enter collateral manually" in result.output
    assert "Estimated minimum additional collateral" not in result.output
    assert calls == [1_000_000_000, 10_000_000_000]
    assert cli_fake.submissions == []


def test_manual_collateral_after_failed_estimate_still_requires_valid_quote(cli_fake, monkeypatch):
    from bittensor.cli import prompt
    from bittensor.cli.commands import lending

    monkeypatch.setattr(lending, "interactive", lambda _ctx: True)
    monkeypatch.setattr(prompt, "interactive", lambda _ctx: True)
    cli_fake.seed_constant("Lending", "MinimumLoanValue", 1_000_000_000)
    calls = []

    def quote(params):
        calls.append(params[3])
        return {"Err": {"Module": {"index": 33, "error": "0x15000000"}}}

    cli_fake.seed_runtime("LendingRuntimeApi", "quote_open_for", quote)
    result = runner.invoke(
        app,
        [
            "--yes",
            "lending",
            "open",
            "--netuid",
            "1",
            "--side",
            "short",
            "--hotkey",
            ALICE_HOT,
        ],
        input="10\n",
    )
    assert result.exit_code != 0
    assert "Enter collateral manually" in result.output
    assert "InsufficientRedemptionBacking" in result.output
    assert calls == [1_000_000_000, 10_000_000_000]
    assert cli_fake.submissions == []


def test_missing_collateral_in_scripts_does_not_estimate_or_prompt(cli_fake):
    def unexpected_quote(_params):
        pytest.fail("missing script option must fail before querying")

    cli_fake.seed_runtime("LendingRuntimeApi", "quote_open_for", unexpected_quote)
    result = runner.invoke(app, ["--json", "lending", "open", "--netuid", "1", "--side", "short"])
    assert result.exit_code == 2
    assert "--collateral" in result.output
    assert cli_fake.submissions == []


def _seed_position(substrate: FakeSubstrate, side: str = "Short", **updates):
    position = {**DEFAULT_STORAGE[("Lending", "Positions")], "side": side, **updates}
    substrate.seed("Lending", "Positions", [ALICE, 1], position)
    return position


@pytest.mark.parametrize("side,collateral_unit,debt_unit", [("Short", 0, 1), ("Long", 1, 0)])
@pytest.mark.asyncio
async def test_open_preserves_exact_units_and_atoms(side, collateral_unit, debt_unit):
    fake = FakeSubstrate()
    intent = OpenLoan(
        netuid=1,
        side=side.lower(),
        collateral=Balance.from_rao(1_000_000_001, collateral_unit),
        min_borrow=Balance.from_rao(250_000_001, debt_unit),
        min_proceeds=Balance.from_rao(250_000_003),
    )
    call = await intent.build(fake, dev_wallet())
    assert call.module == "Lending"
    assert call.function == "open"
    assert call.params == {
        "netuid": 1,
        "side": side,
        "collateral": 1_000_000_001,
        "hotkey": ALICE_HOT,
        "min_borrow": 250_000_001,
        "min_proceeds": 250_000_003,
    }


@pytest.mark.parametrize(
    "side,field,wrong_unit",
    [
        ("Short", "collateral", 1),
        ("Long", "collateral", 0),
        ("Short", "min_borrow", 0),
        ("Long", "min_borrow", 1),
        ("Short", "min_proceeds", 1),
        ("Long", "min_proceeds", 1),
    ],
)
def test_open_rejects_wrong_currency(side, field, wrong_unit):
    args = {"netuid": 1, "side": side, "collateral": "100", field: Balance.from_rao(1, wrong_unit)}
    with pytest.raises(UnitMismatchError):
        OpenLoan(**args)


@pytest.mark.parametrize("collateral", ["0", "-1", "NaN", "Infinity"])
def test_open_rejects_invalid_collateral(collateral):
    with pytest.raises(BittensorError):
        OpenLoan(netuid=1, side="Short", collateral=collateral)


@pytest.mark.parametrize("netuid", [0, -1, 65536])
def test_root_or_invalid_subnet_cannot_borrow(netuid):
    with pytest.raises(BittensorError):
        OpenLoan(netuid=netuid, side="Short", collateral="100")


@pytest.mark.parametrize(
    "side,wallet,payment_unit,refund_unit",
    [("Short", False, 0, 0), ("Short", True, 1, 0), ("Long", False, 0, 1), ("Long", True, 0, 1)],
)
@pytest.mark.asyncio
async def test_close_derives_units_from_owned_position(side, wallet, payment_unit, refund_unit):
    fake = FakeSubstrate()
    _seed_position(fake, side)
    intent = CloseLoan(
        netuid=1,
        repay_from_wallet=wallet,
        max_payment=Balance.from_rao(700_000_001, payment_unit),
        min_refund=Balance.from_rao(900_000_003, refund_unit),
    )
    call = await intent.build(fake, dev_wallet())
    assert call.params == {
        "netuid": 1,
        "repay_from_wallet": wallet,
        "max_payment": 700_000_001,
        "min_refund": 900_000_003,
    }


@pytest.mark.asyncio
async def test_short_close_defaults_to_wallet_alpha_repayment_without_sale_proceeds():
    fake = FakeSubstrate()
    _seed_position(fake, principal=700_000_001, proceeds=0)
    intent = CloseLoan(
        netuid=1,
        max_payment=Balance.from_rao(700_000_001, 1),
        min_refund=Balance.from_rao(900_000_003),
    )
    call = await intent.build(fake, dev_wallet())
    assert call.params["repay_from_wallet"] is True
    assert call.params["max_payment"] == 700_000_001
    assert intent.spend() is None


@pytest.mark.asyncio
async def test_default_short_close_rejects_tao_tagged_as_wallet_alpha_payment():
    fake = FakeSubstrate()
    _seed_position(fake)
    with pytest.raises(UnitMismatchError):
        await CloseLoan(netuid=1, max_payment=Balance.from_rao(700)).build(fake, dev_wallet())


@pytest.mark.parametrize("side,tao_spend", [("Short", None), ("Long", 700_000_000_001)])
@pytest.mark.asyncio
async def test_plain_close_amount_gets_currency_before_tao_spend_policy(side, tao_spend):
    fake = FakeSubstrate()
    _seed_position(fake, side, principal=700_000_000_001, proceeds=0)
    async with Client("local", substrate=fake) as client:
        plan = await client.plan(
            CloseLoan(netuid=1, max_payment="700.000000001"),
            dev_wallet(),
            policy=Policy(max_spend_tao=0),
        )
    assert plan.call.params["max_payment"] == 700_000_000_001
    if tao_spend is None:
        assert plan.spend is None
        assert plan.ok
    else:
        assert plan.spend == Balance.from_rao(tao_spend)
        assert any("exceeds max_spend_tao" in item for item in plan.violations)


@pytest.mark.asyncio
async def test_close_rejects_wrong_refund_currency():
    fake = FakeSubstrate()
    _seed_position(fake, "Long")
    with pytest.raises(UnitMismatchError):
        await CloseLoan(netuid=1, max_payment="50", min_refund=Balance.from_rao(1)).build(
            fake, dev_wallet()
        )


@pytest.mark.asyncio
async def test_close_rejects_missing_position():
    fake = FakeSubstrate()
    fake.seed("Lending", "Positions", [ALICE, 1], None)
    with pytest.raises(BittensorError, match="no lending position"):
        await CloseLoan(netuid=1, max_payment="50").build(fake, dev_wallet())


@pytest.mark.asyncio
async def test_position_interest_includes_carried_fraction_and_deregistration_cutoff():
    fake = FakeSubstrate()
    fake.block = 120
    fake.seed_constant("Lending", "BlocksPerYear", 10)
    _seed_position(fake, "Long", annual_interest=7, last_accrued=100, interest_remainder=4)
    fake.seed("Lending", "Dissolutions", [1], {"frozen_at": 103})
    async with Client("local", substrate=fake) as client:
        result = await client.read("lending_position", coldkey_ss58=ALICE, netuid=1)
    assert result["interest_due"].rao == 2  # (7 * 3 + 4) // 10, frozen at block 103
    assert result["interest_due"].netuid == 1
    assert result["principal"].netuid == 0
    assert result["principal"].rao == 250_000_000


@pytest.mark.asyncio
async def test_vault_cap_uses_available_plus_outstanding_and_excludes_pending():
    fake = FakeSubstrate()
    fake.seed(
        "Lending",
        "Vaults",
        [1],
        {"available_tao": 950, "outstanding_tao": 50, "pending_tao": 10_000, "lost_tao": 100},
    )
    async with Client("local", substrate=fake) as client:
        result = await client.read("lending_reserves", netuid=1)
    assert result["borrow_headroom_tao"].rao == 50
    assert result["available_tao"].rao == 950


@pytest.mark.asyncio
async def test_quote_preserves_alpha_delivery_and_reference_value_without_sale_proceeds():
    fake = FakeSubstrate()
    seen = []

    def quote(params):
        seen.append(params)
        return {"Ok": {"principal": 234, "annual_interest": 230, "opening_value": 230}}

    fake.seed_runtime("LendingRuntimeApi", "quote_open", quote)
    async with Client("local", substrate=fake) as client:
        result = await client.read(
            "lending_open_quote", netuid=1, side="short", collateral="1.000000001"
        )
    assert seen == [[1, "Short", 1_000_000_001]]
    assert result["principal"].rao == 234
    assert result["principal"].netuid == 1
    assert result["opening_value"] == Balance.from_rao(230)
    assert result["annual_interest"].rao == 230
    assert result["annual_interest"].netuid == 0
    assert "proceeds" not in result


@pytest.mark.parametrize("wallet,payment_unit", [(True, 1), (False, 0)])
@pytest.mark.asyncio
async def test_short_close_quote_selects_wallet_or_collateral_payment(wallet, payment_unit):
    fake = FakeSubstrate()
    _seed_position(fake, principal=700, proceeds=0)
    seen = []

    def quote(params):
        seen.append(params)
        return {"Ok": {"payment": 700 if params[2] else 250, "refund": 800}}

    fake.seed_runtime("LendingRuntimeApi", "quote_close", quote)
    async with Client("local", substrate=fake) as client:
        kwargs = {} if wallet else {"repay_from_wallet": False}
        result = await client.read("lending_close_quote", coldkey_ss58=ALICE, netuid=1, **kwargs)
    assert seen == [[ALICE, 1, wallet]]
    assert result["payment"] == Balance.from_rao(700 if wallet else 250, payment_unit)
    assert result["refund"] == Balance.from_rao(800)


@pytest.mark.asyncio
async def test_collateral_buyback_quote_refusal_never_becomes_wallet_repayment():
    fake = FakeSubstrate()
    _seed_position(fake, proceeds=0)
    fake.seed_runtime("LendingRuntimeApi", "quote_close", {"Err": "InsufficientEscrow"})
    async with Client("local", substrate=fake) as client:
        with pytest.raises(BittensorError, match="InsufficientEscrow"):
            await client.read(
                "lending_close_quote", coldkey_ss58=ALICE, netuid=1, repay_from_wallet=False
            )


@pytest.mark.parametrize("side,principal_unit,collateral_unit", [("short", 1, 0), ("long", 0, 1)])
@pytest.mark.asyncio
async def test_owner_quote_uses_exact_addition_and_returns_incremental_amounts(
    side, principal_unit, collateral_unit
):
    fake = FakeSubstrate()
    _seed_position(fake, side, principal=9_000_000_000, annual_interest=8_000_000_000)
    seen = []

    def quote(params):
        seen.append(params)
        return {"Ok": {"principal": 234, "annual_interest": 230, "opening_value": 231}}

    fake.seed_runtime("LendingRuntimeApi", "quote_open_for", quote)
    fake.seed_runtime("LendingRuntimeApi", "quote_open", {"Err": "must use owner quote"})
    async with Client("local", substrate=fake) as client:
        result = await client.read(
            "lending_open_quote",
            netuid=1,
            side=side,
            collateral="1.000000001",
            coldkey_ss58=ALICE,
            hotkey_ss58=ALICE_HOT,
        )
    assert seen == [[ALICE, 1, side.capitalize(), 1_000_000_001, ALICE_HOT]]
    assert result["principal"] == Balance.from_rao(234, principal_unit)
    assert result["annual_interest"] == Balance.from_rao(230, collateral_unit)
    assert result["opening_value"] == Balance.from_rao(231)


@pytest.mark.parametrize("addresses", [{"coldkey_ss58": ALICE}, {"hotkey_ss58": ALICE_HOT}])
@pytest.mark.asyncio
async def test_owner_quote_requires_both_addresses(addresses):
    fake = FakeSubstrate()
    async with Client("local", substrate=fake) as client:
        with pytest.raises(BittensorError, match="requires both coldkey and hotkey"):
            await client.read(
                "lending_open_quote", netuid=1, side="short", collateral="100", **addresses
            )


@pytest.mark.asyncio
async def test_refused_owner_quote_does_not_fall_back_to_fresh_quote():
    fake = FakeSubstrate()
    fake.seed_runtime("LendingRuntimeApi", "quote_open_for", {"Err": "PositionExists"})
    async with Client("local", substrate=fake) as client:
        with pytest.raises(BittensorError, match="PositionExists"):
            await client.read(
                "lending_open_quote",
                netuid=1,
                side="short",
                collateral="100",
                coldkey_ss58=ALICE,
                hotkey_ss58=ALICE_HOT,
            )


@pytest.mark.parametrize("raw", [None, {"Err": {"Module": "BorrowingLimit"}}])
@pytest.mark.asyncio
async def test_unavailable_quote_never_becomes_unprotected_submission(raw):
    fake = FakeSubstrate()
    fake.seed_runtime("LendingRuntimeApi", "quote_open", raw)
    async with Client("local", substrate=fake) as client:
        with pytest.raises(BittensorError):
            await client.read("lending_open_quote", netuid=1, side="short", collateral="1000")


def test_quote_bounds_round_in_protective_direction():
    amount = Balance.from_rao(101, 1)
    assert _bound(amount, 1).rao == 99
    assert _bound(amount, 1, ceiling=True).rao == 103
    assert _bound(amount, 1).netuid == 1


def test_lending_cli_has_exactly_three_commands():
    command = get_command(app).commands["lending"]
    assert set(command.commands) == {"open", "close", "list"}


@pytest.mark.parametrize("side", ["short", "long"])
def test_cli_open_quotes_full_loan_and_sets_floor(cli_fake, side):
    seen = []

    def quote(params):
        seen.append(params)
        return {
            "Ok": {
                "principal": 250_000_000,
                "annual_interest": 250_000_000,
                "opening_value": 250_000_000,
            }
        }

    cli_fake.seed_runtime("LendingRuntimeApi", "quote_open_for", quote)
    cli_fake.seed_runtime("LendingRuntimeApi", "quote_open", {"Err": "must use owner quote"})
    result = runner.invoke(
        app,
        [
            "--json",
            "--dry-run",
            "--no-mev-shield",
            "lending",
            "open",
            "--netuid",
            "1",
            "--side",
            side,
            "--collateral",
            "1000",
        ],
    )
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert plan["op"] == "open_loan"
    assert plan["args"]["min_borrow"] == "0.2475"
    assert plan["args"]["min_proceeds"] == ("0.2475" if side == "short" else "0")
    assert len(seen) == 1
    owner, netuid, runtime_side, collateral, hotkey = seen[0]
    assert owner
    assert hotkey
    assert [netuid, runtime_side, collateral] == [1, side.capitalize(), 1_000_000_000_000]
    assert cli_fake.submissions == []


def test_cli_close_sets_payment_and_refund_bounds(cli_fake):
    result = runner.invoke(
        app,
        ["--json", "--dry-run", "--no-mev-shield", "lending", "close", "--netuid", "1"],
    )
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert plan["args"]["repay_from_wallet"] is True
    assert plan["args"]["max_payment"] == "0.2525"
    assert plan["args"]["min_refund"] == "0.99"
    assert cli_fake.submissions == []


@pytest.mark.parametrize(
    "flags,wallet,payment,protected_payment",
    [([], True, 700_000_000, "0.707"), (["--no-repay-from-wallet"], False, 250_000_000, "0.2525")],
)
def test_cli_short_close_keeps_repayment_choice_in_quote_and_call(
    cli_fake, flags, wallet, payment, protected_payment
):
    _seed_position(cli_fake, principal=700_000_000, proceeds=0)
    seen = []

    def quote(params):
        seen.append(params)
        return {"Ok": {"payment": payment, "refund": 900_000_000}}

    cli_fake.seed_runtime("LendingRuntimeApi", "quote_close", quote)
    result = runner.invoke(
        app,
        ["--json", "--dry-run", "--no-mev-shield", "lending", "close", "--netuid", "1", *flags],
    )
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert len(seen) == 1
    assert seen[0][1:] == [1, wallet]
    assert plan["args"]["repay_from_wallet"] is wallet
    assert plan["args"]["max_payment"] == protected_payment
    assert plan["args"]["min_refund"] == "0.891"
    assert cli_fake.submissions == []


def test_cli_collateral_only_buyback_refusal_does_not_submit(cli_fake):
    cli_fake.seed_runtime("LendingRuntimeApi", "quote_close", {"Err": "InsufficientEscrow"})
    result = runner.invoke(
        app,
        ["--json", "--dry-run", "lending", "close", "--netuid", "1", "--no-repay-from-wallet"],
    )
    assert result.exit_code != 0
    assert cli_fake.submissions == []


@pytest.mark.parametrize(
    "reason",
    [
        "BorrowingLimit",
        "PositionExists",
        "InsufficientEscrow",
        "InsufficientRedemptionBacking",
        "RedemptionUnavailable",
    ],
)
def test_cli_refuses_open_when_quote_fails(cli_fake, reason):
    cli_fake.seed_runtime("LendingRuntimeApi", "quote_open_for", {"Err": reason})
    result = runner.invoke(
        app,
        [
            "--json",
            "--dry-run",
            "lending",
            "open",
            "--netuid",
            "1",
            "--side",
            "short",
            "--collateral",
            "1000",
        ],
    )
    assert result.exit_code != 0
    assert cli_fake.submissions == []


@pytest.mark.parametrize(
    "index,info",
    [(index, info) for index, info in ERRORS.items() if info.pallet == "Lending"],
    ids=[info.name for info in ERRORS.values() if info.pallet == "Lending"],
)
@pytest.mark.asyncio
async def test_lending_module_quote_errors_have_names_and_descriptions(index, info):
    fake = FakeSubstrate()
    module, error = index
    fake.seed_runtime(
        "LendingRuntimeApi",
        "quote_open",
        {"Err": {"Module": {"index": module, "error": bytes([error, 0, 0, 0]).hex()}}},
    )
    async with Client("local", substrate=fake) as client:
        with pytest.raises(ChainError) as failure:
            await client.read("lending_open_quote", netuid=1, side="short", collateral="10")
    assert failure.value.name == info.name
    assert failure.value.description
    assert "Module" not in str(failure.value)


@pytest.mark.parametrize("method", ["quote_open_for", "quote_close"])
@pytest.mark.asyncio
async def test_owner_and_close_quotes_decode_module_errors(method):
    fake = FakeSubstrate()
    _seed_position(fake)
    fake.seed_runtime(
        "LendingRuntimeApi", method, {"Err": {"Module": {"index": 33, "error": "0x0e000000"}}}
    )
    async with Client("local", substrate=fake) as client:
        with pytest.raises(ChainError) as failure:
            if method == "quote_close":
                await client.read("lending_close_quote", coldkey_ss58=ALICE, netuid=1)
            else:
                await client.read(
                    "lending_open_quote",
                    netuid=1,
                    side="short",
                    collateral="10",
                    coldkey_ss58=ALICE,
                    hotkey_ss58=ALICE_HOT,
                )
    assert failure.value.name == "InsufficientEscrow"


@pytest.mark.parametrize("error_index,name", [(3, "ReferenceWarmingUp"), (6, "AmountTooSmall")])
def test_cli_renders_decoded_lending_quote_error_without_submitting(cli_fake, error_index, name):
    cli_fake.seed_runtime(
        "LendingRuntimeApi",
        "quote_open_for",
        {"Err": {"Module": {"index": 33, "error": f"0x{error_index:02x}000000"}}},
    )
    result = runner.invoke(
        app,
        ["lending", "open", "--netuid", "1", "--side", "short", "--collateral", "10"],
    )
    assert result.exit_code != 0
    assert name in result.output
    assert "Module" not in result.output
    assert "0x" not in result.output
    assert cli_fake.submissions == []


def test_cli_lists_a_subnet_without_wallet_unlock(cli_fake):
    cli_fake.seed_map("Lending", "OpenByNetuid", [(ALICE, None)])
    result = runner.invoke(app, ["--json", "lending", "list", "--netuid", "1"])
    assert result.exit_code == 0, result.output
    positions = json.loads(result.output)
    assert len(positions) == 1
    assert positions[0]["coldkey"] == ALICE
    assert positions[0]["proceeds"] == str(Balance.from_rao(0))
    assert cli_fake.submissions == []


def test_cli_position_table_does_not_present_a_locked_sale_balance(cli_fake):
    cli_fake.seed_map("Lending", "OpenByNetuid", [(ALICE, None)])
    result = runner.invoke(app, ["lending", "list", "--netuid", "1"])
    assert result.exit_code == 0, result.output
    assert "proceeds" not in result.output.lower()
    assert "principal" in result.output.lower()
