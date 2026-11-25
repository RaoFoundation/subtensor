"""Reserved/spent root-claim fees follow each call's runtime envelope."""

from __future__ import annotations

import pytest

from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.intents import Policy
from bittensor.intents import _root_claim_fee as fees
from bittensor.intents.registration import ClaimRoot, ClaimRootWithHotkey
from bittensor.result import PolicyError
from bittensor.sp_core import Keypair
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, dev_wallet


@pytest.mark.asyncio
async def test_reserved_fallback_is_max_root_claim_work():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(object(), "5F3sa2TJAW", _boom)
    assert reserved.rao == fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK)


@pytest.mark.asyncio
async def test_reserved_fallback_uses_fixed_envelope():
    async def _boom():
        raise RuntimeError("no payment_info")

    reserved = await fees._reserved_fee(FakeSubstrate(), "5F3sa2TJAW", _boom)
    assert reserved.rao == fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK)


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
    assert reserved.rao == fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_HOTKEY_WORK)


def test_reserved_fallback_charges_the_fee_allowance_not_the_envelope():
    assert fees._fee_units(fees._MAX_ROOT_CLAIM_WORK) == fees._ROOT_CLAIM_FEE_ALLOWANCE
    assert fees._fee_units(fees._MAX_ROOT_CLAIM_HOTKEY_WORK) == fees._ROOT_CLAIM_FEE_ALLOWANCE
    assert fees._fee_units(2) == 2
    assert fees._approx_declared_fee_rao(
        fees._MAX_ROOT_CLAIM_WORK
    ) == fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_HOTKEY_WORK)


# Regression invariant: single hotkey admission covers full subnet limit plus root basket.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_single_hotkey_admission_covers_full_subnet_limit_plus_root_basket():
    admission = fees.RootClaimAdmission(
        hotkeys=(ALICE_HOT,),
        holding_counts=(129,),
        networks=129,
        limit=fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
        selection_scans=0,
    )
    assert admission.limit == 258
    assert not admission.too_heavy

    overloaded = fees.RootClaimAdmission(
        hotkeys=(ALICE_HOT,),
        holding_counts=(258,),
        networks=258,
        limit=fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
        selection_scans=0,
    )
    assert overloaded.too_heavy


# Based on the spec 467 CI release-node quotes (PR #3183, commit 546d26bd9), adjusted in
# spec 484 for the single-hotkey envelope increase from 129 to 130 units, and in spec 495
# for the fund-sized single-hotkey declaration: the single-hotkey rows are for a validator
# without a fund, which declares the smallest envelope (a spec 495 node returns this call
# weight plus 75_000_000 for its transaction extensions). The smallest envelope includes
# the debt settlement bound of 32_375_000_000 and two counts of the fund. The charged fee
# stays fixed at the four-unit allowance. The 60_000_000 ref_time gap between the node
# weight and the SDK's declared model is the folded dispatch-extension weight, which the
# fee wrapper does not discount.
_EXTENSION_REF_TIME = 60_000_000
#: Marks a single-hotkey claim in ``_NODE_QUOTES``: it has no fixed limit to declare.
_HOTKEY_CLAIM = "claim_root_with_hotkey"
_NODE_QUOTES = [
    ("claim_root_with_hotkey", 67_379_612_996, 8_224_263, [_HOTKEY_CLAIM]),
    ("claim_root", 2_476_947_290_768, 8_224_263, [fees._MAX_ROOT_CLAIM_WORK]),
    (
        "batch_all[claim_root_with_hotkey, claim_root]",
        2_544_459_505_134,
        16_481_677,
        [_HOTKEY_CLAIM, fees._MAX_ROOT_CLAIM_WORK],
    ),
]


def _quoted_declared_ref_time(limit) -> int:
    if limit == _HOTKEY_CLAIM:
        return fees.root_claim_hotkey_declared_ref_time()
    return fees.root_claim_declared_ref_time(limit)


def _quoted_discount_ref_time(limit) -> int:
    if limit == _HOTKEY_CLAIM:
        return fees.root_claim_hotkey_fee_discount_ref_time()
    return fees.root_claim_fee_discount_ref_time(limit)


def _quoted_charged_ref_time(limit) -> int:
    if limit == _HOTKEY_CLAIM:
        return fees.root_claim_charged_ref_time(fees._MAX_ROOT_CLAIM_HOTKEY_WORK)
    return fees.root_claim_charged_ref_time(limit)


@pytest.mark.parametrize(("name", "weight", "weight_fee", "limits"), _NODE_QUOTES)
def test_discount_model_matches_the_release_node_quotes(name, weight, weight_fee, limits):
    discount = sum(_quoted_discount_ref_time(limit) for limit in limits)
    assert fees._weight_fee_rao(weight - discount) == weight_fee, name
    if len(limits) == 1:
        assert weight == _quoted_declared_ref_time(limits[0]) + _EXTENSION_REF_TIME
        assert weight - discount == _quoted_charged_ref_time(limits[0]) + _EXTENSION_REF_TIME


def test_charged_allowance_is_the_same_for_both_claim_paths():
    charged = fees.root_claim_charged_ref_time(fees._MAX_ROOT_CLAIM_WORK)
    assert charged == fees.root_claim_charged_ref_time(fees._MAX_ROOT_CLAIM_HOTKEY_WORK)
    assert charged == fees._claim_ref_time(4, 24, 4)
    assert fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK) == 8_209_263
    # A single-hotkey claim reserves less than a coldkey-wide claim, also for a fund with a
    # row and a queued credit on each of 128 subnets.
    assert fees.root_claim_hotkey_fee_discount_ref_time(rows=129, queued=128) < (
        fees.root_claim_fee_discount_ref_time(fees._MAX_ROOT_CLAIM_WORK)
    )


def test_single_hotkey_declared_weight_follows_the_fund_size():
    """Runtime 495 sizes the single-hotkey declaration from the validator's fund."""
    floor = fees.root_claim_hotkey_declared_ref_time()
    ceiling = fees.root_claim_hotkey_declared_ref_time(rows=257, queued=257)
    assert floor == 67_319_612_996
    assert ceiling == 2_477_436_331_766
    # Each declaration includes the debt settlement bound: 259 reads and 259 writes.
    assert fees._DEBT_SETTLEMENT_BOUND_REF_TIME == 259 * (25_000_000 + 100_000_000)
    # Each declaration includes two counts of the fund, ten reads of the kept count and of
    # the guard, and the write that keeps the count. One count reads each row, each queued
    # credit, and the end of both maps.
    assert fees._hotkey_claim_sizing_ref_time(4, 4) == 30 * 25_000_000 + 100_000_000
    assert fees._hotkey_claim_sizing_ref_time(257, 257) == 1_042 * 25_000_000 + 100_000_000
    # A hotkey without a fund has no kept count: ten times one read and the two ends.
    assert fees._hotkey_claim_sizing_ref_time(0, 0) == 30 * 25_000_000
    # Without the two parts above, the floor and the ceiling are the values of the claim
    # work alone.
    assert floor - fees._DEBT_SETTLEMENT_BOUND_REF_TIME - 850_000_000 == 34_094_612_996
    assert ceiling - fees._DEBT_SETTLEMENT_BOUND_REF_TIME - 26_150_000_000 == 2_418_911_331_766

    full_fund = fees.root_claim_hotkey_declared_ref_time(rows=129)
    full_fund_and_queue = fees.root_claim_hotkey_declared_ref_time(rows=129, queued=128)
    assert floor < full_fund < full_fund_and_queue < ceiling
    # A small fund declares the floor, and a fund above the ceiling declares the ceiling.
    assert fees.root_claim_hotkey_declared_ref_time(rows=1) == floor
    assert fees.root_claim_hotkey_declared_ref_time(rows=258) == ceiling
    assert fees.root_claim_hotkey_declared_ref_time(rows=200, queued=58, new_rows=58) == ceiling

    # The fee does not follow the fund: every declaration is discounted to the allowance.
    charged = fees.root_claim_charged_ref_time(fees._MAX_ROOT_CLAIM_HOTKEY_WORK)
    for fund in [(0, 0, 0), (129, 0, 0), (129, 128, 0), (200, 57, 57), (257, 257, 0)]:
        declared = fees.root_claim_hotkey_declared_ref_time(*fund)
        assert declared - fees.root_claim_hotkey_fee_discount_ref_time(*fund) == charged


@pytest.mark.asyncio
async def test_single_hotkey_admission_counts_queued_credits_that_open_rows():
    substrate = FakeSubstrate()
    substrate.runtime_spec_version = 495
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 200},
    )
    # The fund holds netuids 0..199. Ten of these credits are on held subnets; the other
    # 57 can each open a row: 1 validator + 200 rows + 57 = 258 units, the limit.
    substrate.seed_map(
        "SubtensorModule",
        "PendingBasketDeposits",
        [(netuid, 1) for netuid in range(190, 257)],
    )

    admission = await fees.root_claim_admission(substrate, ALICE, hotkeys=[ALICE_HOT])

    assert admission.limit == 258
    assert admission.holdings == 200
    assert admission.queued_new == 57
    assert not admission.too_heavy

    substrate.seed_map(
        "SubtensorModule",
        "PendingBasketDeposits",
        [(netuid, 1) for netuid in range(190, 258)],
    )

    admission = await fees.root_claim_admission(substrate, ALICE, hotkeys=[ALICE_HOT])

    assert admission.too_heavy
    assert any(
        "1 root hotkey + 200 basket holdings + 58 queued dividend credits on subnets the "
        "fund does not hold = 259" in block
        for block in admission.blocks()
    )


class _RecordingSubstrate(FakeSubstrate):
    """Records the block hash of each read that the admission check makes."""

    def __init__(self):
        super().__init__()
        self.reads: list[tuple[str, object]] = []

    async def spec_version(self, block_hash=None):
        self.reads.append(("spec_version", block_hash))
        return await super().spec_version(block_hash)

    async def runtime_call(self, api, method, params, block_hash=None):
        self.reads.append((method, block_hash))
        return await super().runtime_call(api, method, params, block_hash)

    async def query(self, module, storage_function, params=None, block_hash=None):
        self.reads.append((storage_function, block_hash))
        return await super().query(module, storage_function, params, block_hash)

    async def query_map(self, module, storage_function, params=None, block_hash=None):
        self.reads.append((storage_function, block_hash))
        return await super().query_map(module, storage_function, params, block_hash)


def _seed_single_fund(substrate: FakeSubstrate, *, holdings: int, queued=()) -> None:
    """One validator fund that holds netuids ``0..holdings-1``, with one queued dividend
    credit on each netuid in ``queued``."""
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: holdings},
    )
    substrate.seed_map(
        "SubtensorModule",
        "PendingBasketDeposits",
        [(netuid, 1) for netuid in queued],
    )


def test_single_hotkey_limit_follows_the_runtime_version():
    assert fees.root_claim_declared_work([ALICE_HOT], 494) == 130
    assert fees.root_claim_declared_work([ALICE_HOT], 495) == 258
    assert fees.root_claim_declared_work([ALICE_HOT], 496) == 258
    # No version given: the newest known rules.
    assert fees.root_claim_declared_work([ALICE_HOT]) == 258
    # The coldkey-wide limit is the same on every runtime.
    assert fees.root_claim_declared_work(None, 494) == 256
    assert fees.root_claim_declared_work(None, 495) == 256


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("spec_version", "holdings", "limit", "too_heavy"),
    [
        # Runtime 494 admits 130 claim units: the validator and 129 basket rows.
        (494, 129, 130, False),
        (494, 130, 130, True),
        # A fund between the two limits: refused by runtime 494, admitted by runtime 495.
        (494, 200, 130, True),
        (495, 200, 258, False),
        # Runtime 495 admits 258 claim units: the validator and 257 basket rows.
        (495, 257, 258, False),
        (495, 258, 258, True),
    ],
)
async def test_single_hotkey_admission_uses_the_rules_of_the_runtime_version(
    spec_version, holdings, limit, too_heavy
):
    substrate = FakeSubstrate()
    substrate.runtime_spec_version = spec_version
    _seed_single_fund(substrate, holdings=holdings)

    admission = await fees.root_claim_admission(substrate, ALICE, hotkeys=[ALICE_HOT])

    assert admission.spec_version == spec_version
    assert admission.limit == limit
    assert admission.holdings == holdings
    assert admission.too_heavy is too_heavy
    blocks = admission.blocks()
    if too_heavy:
        expected = (
            f"root claim exceeds the {limit}-unit admission limit "
            f"(1 root hotkey + {holdings} basket holdings = {holdings + 1})"
        )
        assert any(expected in block for block in blocks)
    else:
        assert blocks == []


@pytest.mark.asyncio
async def test_queued_credits_count_from_runtime_495_only():
    """Runtime 494 counts the validator and its stored rows. Runtime 495 also counts each
    queued credit on a subnet that the fund does not hold."""
    queued = range(129, 329)  # 200 credits; the fund holds netuids 0..128

    before = _RecordingSubstrate()
    before.runtime_spec_version = 494
    _seed_single_fund(before, holdings=129, queued=queued)
    admission = await fees.root_claim_admission(before, ALICE, hotkeys=[ALICE_HOT])
    assert admission.limit == 130
    assert admission.queued_new == 0
    assert not admission.too_heavy  # 1 + 129 = 130 units
    # The rule of runtime 494 has no queue term, so the check does not read the queue.
    assert "PendingBasketDeposits" not in [name for name, _block in before.reads]

    after = _RecordingSubstrate()
    after.runtime_spec_version = 495
    _seed_single_fund(after, holdings=129, queued=queued)
    admission = await fees.root_claim_admission(after, ALICE, hotkeys=[ALICE_HOT])
    assert admission.limit == 258
    assert admission.queued_new == 200
    assert admission.too_heavy  # 1 + 129 + 200 = 330 units
    assert "PendingBasketDeposits" in [name for name, _block in after.reads]


@pytest.mark.asyncio
@pytest.mark.parametrize("hotkeys", [[ALICE_HOT], None])
async def test_admission_reads_the_fund_and_the_runtime_version_at_one_block(hotkeys):
    substrate = _RecordingSubstrate()
    substrate.block = 9_572_146
    substrate.runtime_spec_version = 495
    _seed_single_fund(substrate, holdings=3, queued=[7])
    head = await substrate.block_hash()

    await fees.root_claim_admission(substrate, ALICE, hotkeys=hotkeys)

    names = [name for name, _block in substrate.reads]
    assert names[0] == "spec_version"
    assert "get_validator_basket" in names
    assert ("PendingBasketDeposits" in names) is (hotkeys is not None)
    # The version that selects the rules and every fund read use the same block.
    assert {block for _name, block in substrate.reads} == {head}


@pytest.mark.asyncio
@pytest.mark.parametrize(("spec_version", "blocked"), [(494, True), (495, False)])
async def test_plan_blocks_a_fund_that_only_the_old_runtime_refuses(spec_version, blocked):
    """The audit case: on runtime 494 a fund of 131 to 258 claim units must stop in the
    plan, because the chain refuses the claim and still charges a fee."""
    substrate = FakeSubstrate()
    substrate.runtime_spec_version = spec_version
    _seed_single_fund(substrate, holdings=200)

    plan = await Client("local", substrate=substrate).plan(
        ClaimRootWithHotkey(hotkey_ss58=ALICE_HOT), dev_wallet()
    )

    stopped = [block for block in plan.violations if "admission limit" in block]
    if blocked:
        assert len(stopped) == 1
        assert "130-unit admission limit" in stopped[0]
        assert "1 root hotkey + 200 basket holdings = 201" in stopped[0]
    else:
        assert stopped == []


def test_spent_caps_at_the_fee_allowance():
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=32, scan_holdings=0),
    )
    assert spent == reserved


def test_spent_keeps_non_weight_base_fee():
    base = 12_345
    reserved = Balance.from_rao(base + fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=1, scan_holdings=0),
    )
    assert spent.rao == base + fees._weight_fee_rao(
        fees._claim_root_ref_time(1) + fees._claim_root_scan_ref_time(0)
    )


def test_scan_only_uses_scan_ref_time():
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=0, scan_holdings=32),
    )
    expected = fees._weight_fee_rao(
        fees._claim_root_ref_time(1) + fees._claim_root_scan_ref_time(32)
    )
    assert expected < reserved.rao
    assert spent.rao == expected


def test_coldkey_wide_empty_baskets_floor_to_hotkey_count():
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(fees._MAX_ROOT_CLAIM_WORK))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=3, redeem_holdings=0, scan_holdings=0),
    )
    assert spent.rao == fees._weight_fee_rao(
        fees._claim_root_ref_time(3) + fees._claim_root_scan_ref_time(0)
    )


def test_single_hotkey_spent_caps_at_the_same_allowance():
    declared_work = fees._MAX_ROOT_CLAIM_HOTKEY_WORK
    reserved = Balance.from_rao(fees._approx_declared_fee_rao(declared_work))
    spent = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=32, scan_holdings=0),
        declared_work=declared_work,
    )
    assert spent == reserved


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
        "StakeInfoRuntimeApi",
        "get_stake_info_for_coldkey",
        [{"hotkey": hotkey, "coldkey": ALICE, "netuid": 0, "stake": 1} for hotkey in hotkeys],
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


@pytest.mark.parametrize("crypto", [7, 8])
@pytest.mark.parametrize("shielded", [False, True])
async def test_ms_root_claim_reserve_and_policy_quote_the_actual_signer(
    monkeypatch, crypto, shielded
):
    from unittest.mock import AsyncMock

    substrate = FakeSubstrate()
    signer = Keypair.create_from_seed(bytes([91]) * 32, crypto)
    _seed_claim_quote(
        substrate, hotkeys=[ALICE_HOT], payouts={ALICE_HOT: 10**6}, holdings={ALICE_HOT: 1}
    )
    substrate.seed("SubtensorModule", "StakingHotkeys", [signer.ss58_address], [ALICE_HOT])
    substrate.seed("System", "Account", [signer.ss58_address], {"data": {"free": 10**9}})
    seen = []

    async def estimate(call, keypair, **kwargs):
        seen.append(keypair)
        return Balance.from_rao(100 if keypair.crypto_type == crypto else 20)

    monkeypatch.setattr(substrate, "estimate_fee", estimate)
    monkeypatch.setattr(
        substrate, "estimate_shielded_carrier_fee", AsyncMock(return_value=Balance.from_rao(10))
    )
    client = Client("local", substrate=substrate)
    expected = 110 if shielded else 100
    preview = await client.preflight(ClaimRoot(), signer, shielded=shielded)
    assert preview.estimated_fee.rao == preview.required_free.rao == expected
    assert seen and all(key.crypto_type == crypto for key in seen)
    assert all(key.ss58_address == signer.ss58_address for key in seen)
    assert all(bytes(key.hashed_descriptor) == bytes(signer.hashed_descriptor) for key in seen)

    policy = Policy(max_fee_tao=Balance.from_rao(50))
    plan = await client.plan(ClaimRoot(), signer, shielded=shielded, policy=policy)
    assert not plan.ok and any("max_fee_tao" in value for value in plan.violations)
    with pytest.raises(PolicyError, match="max_fee_tao"):
        await client.execute(ClaimRoot(), signer, shielded=shielded, policy=policy)
    assert substrate.submissions == []

    substrate.seed("System", "Account", [signer.ss58_address], {"data": {"free": 99}})
    preview = await client.preflight(ClaimRoot(), signer, shielded=shielded)
    assert preview.required_free.rao == expected
    assert any("reserved claim fee" in value for value in preview.blocks)


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
        fees.RootClaimWork(
            hotkeys=2,
            redeem_holdings=0,
            scan_holdings=2,
            selection_scans=2,
        ),
    )


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("hotkeys", "holdings", "networks", "reason"),
    [
        ([f"hotkey-{i}" for i in range(257)], 0, 16, "257 root hotkeys + 0 basket holdings"),
        ([ALICE_HOT], 256, 1, "1 root hotkey + 256 basket holdings"),
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
async def test_affordable_small_shielded_claim_is_not_rejected_by_large_tier_quote(monkeypatch):
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate, hotkeys=[ALICE_HOT], payouts={ALICE_HOT: 1_000_000}, holdings={ALICE_HOT: 1}
    )
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 2 * substrate.fee.rao}})
    substrate.mev_key = bytes(1184)
    sizes = []

    async def estimate(call, keypair, **kwargs):
        if call.module == "MevShield":
            size = len(call.params["ciphertext"])
            sizes.append(size)
            if size > 8192:
                return Balance.from_rao(10**18)
        return substrate.fee

    monkeypatch.setattr(substrate, "estimate_fee", estimate)
    result = await Client("local", substrate=substrate).submit_shielded(ClaimRoot(), dev_wallet())
    assert result.success
    assert sizes and all(size <= 8192 for size in sizes)
    assert substrate.last_call.module == "MevShield"
    assert len(substrate.last_call.params["ciphertext"]) == sizes[-1]


@pytest.mark.asyncio
@pytest.mark.parametrize("payout_failure", [None, RuntimeError("payout unavailable")])
async def test_admission_hard_stop_survives_unavailable_payout_preview(payout_failure):
    substrate = FakeSubstrate()
    hotkeys = [f"hotkey-{i}" for i in range(257)]
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
    assert any("257 root hotkeys + 0 basket holdings" in block for block in plan.violations)

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
        (1, 255, 1, True),
        (1, 256, 1, False),
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
async def test_coldkey_wide_admission_ignores_non_root_hotkeys():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT, BOB_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 1, BOB_HOT: 256},
        networks=500,
    )
    substrate.seed_runtime(
        "StakeInfoRuntimeApi",
        "get_stake_info_for_coldkey",
        [
            {"hotkey": ALICE_HOT, "coldkey": ALICE, "netuid": 0, "stake": 1},
            {"hotkey": BOB_HOT, "coldkey": ALICE, "netuid": 7, "stake": 1},
        ],
    )

    plan = await Client("local", substrate=substrate).plan(ClaimRoot(), dev_wallet())

    assert not any("admission limit" in block for block in plan.violations)


# Regression invariant: coldkey wide admission keeps unstaked basket claimant.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.asyncio
async def test_coldkey_wide_admission_keeps_unstaked_basket_claimant():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 1_000_000},
        holdings={ALICE_HOT: 1},
    )
    substrate.seed_runtime("StakeInfoRuntimeApi", "get_stake_info_for_coldkey", [])
    substrate.seed("SubtensorModule", "BasketClaimed", [ALICE_HOT, ALICE], -1)

    admission = await fees.root_claim_admission(substrate, ALICE, hotkeys=None)

    assert admission.hotkeys == (ALICE_HOT,)


@pytest.mark.asyncio
async def test_network_count_does_not_affect_admission():
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
        "StakeInfoRuntimeApi",
        "get_stake_info_for_coldkey",
        [{"hotkey": BOB_HOT, "coldkey": BOB, "netuid": 0, "stake": 1}],
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_payout",
        lambda params: 1_000_000 if params[1] == BOB else None,
    )
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_validator_basket", [(0, 1)])

    priced_calls = []
    original_estimate = substrate.estimate_fee

    async def capture_estimate(call, keypair, **kwargs):
        priced_calls.append(call)
        return await original_estimate(call, keypair, **kwargs)

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


def _preview(
    hotkey: str,
    *,
    accrued: int,
    redeemable: int,
    rows: int,
    sold: int,
    swept: int = 0,
    flushed: int = 0,
) -> dict:
    return {
        "hotkey": hotkey,
        "owed_shares": 1,
        "accrued_tao": accrued,
        "redeemable_tao": redeemable,
        "forfeited_tao_est": accrued - redeemable,
        "rows": rows,
        "rows_to_sell": sold,
        "dust_rows": rows - sold,
        "swept": swept,
        "flushed_credits": flushed,
    }


@pytest.mark.asyncio
async def test_single_hotkey_quote_uses_the_dust_aware_preview_on_mixed_dust():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 2_250_000},
        holdings={ALICE_HOT: 3},
    )
    # Spec 468 runtime: one of three rows is dust; the claim pays the other two.
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_claim_preview",
        _preview(ALICE_HOT, accrued=2_250_000, redeemable=2_200_000, rows=3, sold=2),
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=[ALICE_HOT],
        compose=lambda: substrate.compose(
            ("SubtensorModule", "claim_root_with_hotkey", {"hotkey": ALICE_HOT})
        ),
    )

    assert quote is not None
    assert quote.accrued.rao == 2_250_000, "full entitlement is kept as accrued"
    assert quote.redeemable.rao == 2_200_000, "payout excludes the skipped slice"
    assert quote.dust_rows == 1
    assert quote.forfeited.rao == 50_000
    assert quote.eligible_hotkeys == 1
    assert any("1 dust row skipped" in line for line in quote.effects())
    # The work estimate prices two redeemed rows and one scanned row, not three redeemed.
    reserved = fees.Balance.from_rao(10**9)
    spent_all_sold = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=3, scan_holdings=0),
        fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
    )
    spent_dust_aware = fees._spent_fee(
        reserved,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=2, scan_holdings=1),
        fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
    )
    assert spent_dust_aware.rao < spent_all_sold.rao
    assert quote.spent.rao <= quote.reserved.rao


@pytest.mark.asyncio
async def test_single_hotkey_quote_reports_an_all_dust_claim_as_a_noop():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 8_000},
        holdings={ALICE_HOT: 2},
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_claim_preview",
        _preview(ALICE_HOT, accrued=8_000, redeemable=0, rows=2, sold=0),
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=[ALICE_HOT],
        compose=lambda: substrate.compose(
            ("SubtensorModule", "claim_root_with_hotkey", {"hotkey": ALICE_HOT})
        ),
    )

    assert quote is not None
    assert quote.accrued.rao == 8_000
    assert quote.redeemable.rao == 0
    assert quote.eligible_hotkeys == 0
    assert quote.below_threshold, "an all-dust claim is a no-op that realizes nothing"
    assert any("realizes nothing" in warning for warning in quote.warnings())


@pytest.mark.asyncio
async def test_coldkey_quote_applies_the_threshold_to_the_dust_aware_payout():
    substrate = FakeSubstrate()
    # Threshold seeded by the helper: 0.0005 TAO (500_000 rao).
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT, BOB_HOT],
        payouts={ALICE_HOT: 700_000, BOB_HOT: 900_000},
        holdings={ALICE_HOT: 4, BOB_HOT: 3},
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_root_basket_claim_previews",
        [
            # Full entitlement above the threshold, payout after dust below it: no-op.
            _preview(ALICE_HOT, accrued=700_000, redeemable=400_000, rows=4, sold=1),
            # Above the threshold either way.
            _preview(BOB_HOT, accrued=900_000, redeemable=880_000, rows=3, sold=2),
        ],
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=None,
        compose=lambda: substrate.compose(("SubtensorModule", "claim_root", {})),
    )

    assert quote is not None
    assert quote.accrued.rao == 1_600_000
    assert quote.redeemable.rao == 880_000, "only Bob's fund pays; Alice's is below threshold"
    assert quote.eligible_hotkeys == 1
    assert quote.below_threshold_hotkeys == 1
    assert quote.dust_rows == 4
    assert quote.forfeited.rao == 320_000


@pytest.mark.asyncio
async def test_quote_falls_back_to_full_entitlement_without_the_preview_api():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 900_000},
        holdings={ALICE_HOT: 3},
    )

    async def unsupported(api, method, params, block_hash=None):
        if method in ("get_basket_claim_preview", "get_root_basket_claim_previews"):
            raise RuntimeError("Method not found")
        return await FakeSubstrate.runtime_call(substrate, api, method, params, block_hash)

    substrate.runtime_call = unsupported  # type: ignore[method-assign]

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=[ALICE_HOT],
        compose=lambda: substrate.compose(
            ("SubtensorModule", "claim_root_with_hotkey", {"hotkey": ALICE_HOT})
        ),
    )

    assert quote is not None
    assert quote.accrued.rao == 900_000
    assert quote.redeemable.rao == 900_000
    assert quote.dust_rows == 0


@pytest.mark.asyncio
async def test_quote_prices_consolidated_rows_as_full_claim_units():
    substrate = FakeSubstrate()
    # Admission sees the 8 rows the fund holds now; the claim consolidates 6 of them into
    # root cash (6 swaps), then sells the 3 rows that remain (2 real + the cash).
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 2_700_000_000},
        holdings={ALICE_HOT: 8},
    )
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_claim_preview",
        _preview(
            ALICE_HOT, accrued=2_700_000_000, redeemable=2_700_000_000, rows=3, sold=3, swept=6
        ),
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=[ALICE_HOT],
        compose=lambda: substrate.compose(
            ("SubtensorModule", "claim_root_with_hotkey", {"hotkey": ALICE_HOT})
        ),
    )

    assert quote is not None
    assert quote.holdings == 8, "admission keeps the pre-preparation row count"
    # Runtime formula: claim_root(realized + swept = 9) + claim_root_scan(rows − realized = 0).
    expected = fees._spent_fee(
        quote.reserved,
        fees.RootClaimWork(
            hotkeys=1, redeem_holdings=9, scan_holdings=0, selection_scans=quote.selection_scans
        ),
        quote.admission_limit,
    )
    assert quote.spent.rao == expected.rao
    # With a reserve large enough not to cap the estimate, 9 full units cost more than
    # 3 full units plus 5 scans: consolidation is swap work, not scans.
    uncapped = Balance.from_rao(10**9)
    full_units = fees._spent_fee(
        uncapped,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=9, scan_holdings=0),
        fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
    )
    scans_only = fees._spent_fee(
        uncapped,
        fees.RootClaimWork(hotkeys=1, redeem_holdings=3, scan_holdings=5),
        fees._MAX_ROOT_CLAIM_HOTKEY_WORK,
    )
    assert full_units.rao > scans_only.rao


@pytest.mark.asyncio
async def test_quote_counts_consolidation_even_when_the_claim_is_below_threshold():
    substrate = FakeSubstrate()
    _seed_claim_quote(
        substrate,
        hotkeys=[ALICE_HOT],
        payouts={ALICE_HOT: 300_000},
        holdings={ALICE_HOT: 8},
    )
    # Redeemable 0.0003 TAO is under the 0.0005 TAO threshold: the redemption is a no-op,
    # but the 6 consolidations still ran.
    substrate.seed_runtime(
        "BetaBasketRuntimeApi",
        "get_basket_claim_preview",
        _preview(ALICE_HOT, accrued=300_000, redeemable=300_000, rows=3, sold=3, swept=6),
    )

    quote = await fees.quote_root_claim_fee(
        substrate,
        ALICE,
        hotkeys=[ALICE_HOT],
        compose=lambda: substrate.compose(
            ("SubtensorModule", "claim_root_with_hotkey", {"hotkey": ALICE_HOT})
        ),
    )

    assert quote is not None
    assert quote.below_threshold
    expected = fees._spent_fee(
        quote.reserved,
        fees.RootClaimWork(
            hotkeys=1, redeem_holdings=6, scan_holdings=3, selection_scans=quote.selection_scans
        ),
        quote.admission_limit,
    )
    assert quote.spent.rao == expected.rao
