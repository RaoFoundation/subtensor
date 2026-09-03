"""Reserved/spent root-claim fees follow the runtime claim envelopes."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.intents import _root_claim_fee as fees
from bittensor.intents.registration import ClaimRoot
from bittensor.result import PolicyError
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, dev_wallet


@pytest.mark.asyncio
async def test_reserved_fallback_is_conservative_when_chain_is_unknown():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(object(), "5F3sa2TJAW", _boom)
    assert reserved.rao == fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK)


@pytest.mark.asyncio
async def test_reserved_fallback_uses_mainnet_sized_default_envelope():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(FakeSubstrate(), "5F3sa2TJAW", _boom)
    assert reserved.rao == fees._approx_declared_fee_rao(fees._DEFAULT_ROOT_CLAIM_WORK)


def test_spent_scales_against_chain_declaration_not_network_count():
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(fees._DEFAULT_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=32, scan_holdings=0),
    )
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 32


def test_spent_keeps_non_weight_base_fee():
    base = 12_345
    reserved = Balance.from_rao(base + fees._approx_declared_fee_rao(fees._DEFAULT_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=16, scan_holdings=0),
    )
    assert spent.rao == base + fees._APPROX_REDEEM_FEE_RAO * 16


def test_scan_only_uses_scan_ref_time():
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(fees._DEFAULT_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=0, scan_holdings=32),
    )
    scan = fees._APPROX_SCAN_FEE_RAO * 32
    walk = fees._APPROX_REDEEM_FEE_RAO
    assert spent.rao == walk + scan


def test_coldkey_wide_empty_baskets_floor_to_hotkey_count():
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(fees._DEFAULT_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=100, redeem_holdings=0, scan_holdings=0),
    )
    assert spent.rao == fees._APPROX_REDEEM_FEE_RAO * 100


def _seed_claim_quote(
    substrate: FakeSubstrate,
    *,
    hotkeys: list[str],
    payouts: dict[str, int],
    holdings: dict[str, int],
    networks: int = 1,
) -> None:
    substrate.seed("SubtensorModule", "StakingHotkeys", [ALICE], hotkeys)
    substrate.seed("SubtensorModule", "RootClaimableThreshold", [0], {"bits": 500_000 << 32})
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 10**12}})
    substrate.seed_map(
        "SubtensorModule",
        "NetworksAdded",
        [(netuid, True) for netuid in range(networks)],
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_root_basket_owed",
        sum(payouts.values()),
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_root_basket_positions",
        [(hotkey, 1, payout) for hotkey, payout in payouts.items() if payout],
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_payout",
        lambda params: payouts[params[0]],
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_validator_basket",
        lambda params: [(i, 1) for i in range(holdings[params[0]])],
    )


@pytest.mark.asyncio
async def test_coldkey_threshold_is_applied_per_hotkey_not_to_aggregate():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT, BOB_HOT],
        payouts={ALICE_HOT: 300_000, BOB_HOT: 300_000},
        holdings={ALICE_HOT: 2, BOB_HOT: 3},
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=None,
        compose=lambda: substrate.compose(("SubtensorModule", "claim_root", {})),
    )

    assert quote is not None
    assert quote.accrued.rao == 600_000
    assert quote.redeemable.rao == 0
    assert quote.below_threshold
    assert quote.below_threshold_hotkeys == 2
    assert any("realizes nothing" in warning for warning in quote.warnings())


@pytest.mark.asyncio
async def test_coldkey_quote_separates_eligible_and_below_threshold_hotkeys():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT, BOB_HOT],
        payouts={ALICE_HOT: 900_000, BOB_HOT: 300_000},
        holdings={ALICE_HOT: 2, BOB_HOT: 3},
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=None,
        compose=lambda: substrate.compose(("SubtensorModule", "claim_root", {})),
    )

    assert quote is not None
    assert quote.accrued.rao == 1_200_000
    assert quote.redeemable.rao == 900_000
    assert quote.eligible_hotkeys == 1
    assert quote.below_threshold_hotkeys == 1
    assert any("1 of 2 validators" in warning for warning in quote.warnings())


@pytest.mark.asyncio
async def test_coldkey_quote_does_not_scan_validator_without_owed_shares():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT, BOB_HOT],
        payouts={ALICE_HOT: 300_000},
        holdings={ALICE_HOT: 2, BOB_HOT: 3},
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=None,
        compose=lambda: substrate.compose(("SubtensorModule", "claim_root", {})),
    )

    assert quote is not None
    assert quote.below_threshold_hotkeys == 1
    assert quote.spent == fees._spent_fee(
        quote.reserved,
        fees.RootClaimWork(hotkeys=2, redeem_holdings=0, scan_holdings=2),
    )


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("hotkeys", "holdings", "networks", "reason"),
    [
        ([f"hotkey-{i}" for i in range(17)], 0, 16, "17 hotkeys × 16 networks"),
        ([ALICE_HOT], 257, 1, "257 basket holdings"),
    ],
)
async def test_root_claim_too_heavy_is_a_hard_stop(hotkeys, holdings, networks, reason):
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=hotkeys,
        payouts={hotkey: 1_000_000 for hotkey in hotkeys},
        holdings={hotkey: holdings for hotkey in hotkeys},
        networks=networks,
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=None,
        compose=lambda: substrate.compose(("SubtensorModule", "claim_root", {})),
    )

    assert quote is not None
    assert quote.too_heavy
    assert any(reason in block for block in quote.blocks())


@pytest.mark.asyncio
async def test_shielded_root_claim_refuses_too_heavy_work_before_signing():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 257},
    )

    with pytest.raises(PolicyError, match="256-unit admission limit"):
        await Client("local", substrate=substrate).submit_shielded(ClaimRoot(), dev_wallet())

    assert not substrate.submissions


@pytest.mark.asyncio
async def test_shielded_claim_reserves_free_tao_for_inner_and_carrier():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 1},
    )
    # Exactly enough for the inner reserve is not enough after the carrier
    # consumes its separate fee at the preceding nonce.
    substrate.seed(
        "System",
        "Account",
        [ALICE],
        {"data": {"free": substrate.fee.rao}},
    )

    with pytest.raises(PolicyError, match="reserve plus the MEV-shield carrier fee"):
        await Client("local", substrate=substrate).submit_shielded(ClaimRoot(), dev_wallet())

    assert not substrate.submissions


@pytest.mark.asyncio
@pytest.mark.parametrize("payout_failure", [None, RuntimeError("payout unavailable")])
async def test_admission_hard_stop_survives_unavailable_payout_preview(payout_failure):
    substrate = FakeSubstrate()
    hotkeys = [f"hotkey-{i}" for i in range(17)]
    _seed_claim_quote(
        substrate,
        hotkeys=hotkeys,
        payouts={hotkey: 1_000_000 for hotkey in hotkeys},
        holdings={hotkey: 0 for hotkey in hotkeys},
        networks=16,
    )

    if payout_failure is None:
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_positions", None)
    else:

        def fail_payout(_params):
            raise payout_failure

        substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_positions", fail_payout)

    client = Client("local", substrate=substrate)
    plan = await client.plan(ClaimRoot(), dev_wallet())
    assert any("17 hotkeys × 16 networks" in block for block in plan.violations)

    with pytest.raises(PolicyError, match="256-unit admission limit"):
        await client.submit_shielded(ClaimRoot(), dev_wallet())
    assert not substrate.submissions


@pytest.mark.asyncio
@pytest.mark.parametrize("payout_failure", [None, RuntimeError("payout unavailable")])
async def test_reserve_hard_stop_survives_unavailable_payout_preview(payout_failure):
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 1},
    )
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 0}})

    if payout_failure is None:
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_positions", None)
    else:

        def fail_payout(_params):
            raise payout_failure

        substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_positions", fail_payout)

    client = Client("local", substrate=substrate)
    plan = await client.plan(ClaimRoot(), dev_wallet())
    assert any("below the reserved claim fee" in block for block in plan.violations)

    with pytest.raises(PolicyError, match="below the reserved claim fee"):
        await client.submit_shielded(ClaimRoot(), dev_wallet())
    assert not substrate.submissions


@pytest.mark.asyncio
async def test_carrier_hard_stop_survives_unavailable_payout_preview():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 1},
    )
    substrate.seed(
        "System",
        "Account",
        [ALICE],
        {"data": {"free": substrate.fee.rao}},
    )
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_positions", None)

    with pytest.raises(PolicyError, match="reserve plus the MEV-shield carrier fee"):
        await Client("local", substrate=substrate).submit_shielded(ClaimRoot(), dev_wallet())

    assert not substrate.submissions


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("hotkey_count", "holdings", "networks", "expected_ok"),
    [
        (16, 0, 16, True),
        (1, 256, 1, True),
        (1, 257, 1, False),
    ],
)
async def test_admission_limit_is_inclusive_at_256(hotkey_count, holdings, networks, expected_ok):
    substrate = FakeSubstrate()
    hotkeys = [f"hotkey-{i}" for i in range(hotkey_count)]
    _seed_claim_quote(
        substrate,
        hotkeys=hotkeys,
        payouts={hotkey: 1_000_000 for hotkey in hotkeys},
        holdings={hotkey: holdings for hotkey in hotkeys},
        networks=networks,
    )

    plan = await Client("local", substrate=substrate).plan(ClaimRoot(), dev_wallet())

    admission_blocks = [block for block in plan.violations if "256-unit admission limit" in block]
    assert (not admission_blocks) is expected_ok


@pytest.mark.asyncio
async def test_finney_testnet_admission_limit_covers_configured_subnet_limit(monkeypatch):
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 0},
        networks=fees._MAX_ROOT_CLAIM_WORK,
    )

    async def testnet_genesis(_block=None):
        return fees._FINNEY_TESTNET_GENESIS_HASH

    monkeypatch.setattr(substrate, "block_hash", testnet_genesis)
    plan = await Client("local", substrate=substrate).plan(ClaimRoot(), dev_wallet())

    assert not any("admission limit" in block for block in plan.violations)

    substrate.seed_map(
        "SubtensorModule",
        "NetworksAdded",
        [(netuid, True) for netuid in range(fees._MAX_ROOT_CLAIM_WORK + 1)],
    )
    plan = await Client("local", substrate=substrate).plan(ClaimRoot(), dev_wallet())

    assert any("1,025-unit admission limit" in block for block in plan.violations)


@pytest.mark.asyncio
async def test_inactive_network_rows_do_not_count_toward_admission():
    substrate = FakeSubstrate()
    hotkeys = [f"hotkey-{i}" for i in range(17)]
    _seed_claim_quote(
        substrate,
        hotkeys=hotkeys,
        payouts={hotkey: 1_000_000 for hotkey in hotkeys},
        holdings={hotkey: 0 for hotkey in hotkeys},
        networks=15,
    )
    substrate.seed_map(
        "SubtensorModule",
        "NetworksAdded",
        [(netuid, True) for netuid in range(15)] + [(netuid, False) for netuid in range(15, 40)],
    )

    plan = await Client("local", substrate=substrate).plan(ClaimRoot(), dev_wallet())

    assert not any("256-unit admission limit" in block for block in plan.violations)


@pytest.mark.asyncio
async def test_proxy_claim_reads_dispatch_state_checks_delegate_and_prices_wrapper(monkeypatch):
    substrate = FakeSubstrate()
    substrate.seed("SubtensorModule", "StakingHotkeys", [BOB], [BOB_HOT])
    substrate.seed("SubtensorModule", "RootClaimableThreshold", [0], {"bits": 500_000 << 32})
    substrate.seed("System", "Account", [BOB], {"data": {"free": 10**12}})
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 0}})
    substrate.seed_map("SubtensorModule", "NetworksAdded", [(0, True)])
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_owed", 1_000_000)
    substrate.seed_runtime(
        "BetaBasketRuntimeApi", "get_root_basket_positions", [(BOB_HOT, 1, 1_000_000)]
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_payout",
        lambda params: 1_000_000 if params[1] == BOB else None,
    )
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_validator_basket", [(0, 1)])

    priced_calls = []
    original_estimate = substrate.estimate_fee

    async def capture_estimate(call, keypair):
        priced_calls.append(call)
        return await original_estimate(call, keypair)

    monkeypatch.setattr(substrate, "estimate_fee", capture_estimate)
    intent = ClaimRoot()
    client = Client("local", substrate=substrate)
    await client.preflight(intent, dev_wallet(), proxy_for=BOB)
    plan = await client.plan(
        intent,
        dev_wallet(),
        proxy_for=BOB,
    )

    assert priced_calls
    assert all((call.module, call.function) == ("Proxy", "proxy") for call in priced_calls)
    assert any("below the reserved claim fee" in violation for violation in plan.violations)
