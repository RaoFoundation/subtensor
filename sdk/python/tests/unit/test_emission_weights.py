"""Emission weight vector: pure rules, the intent's plan, the read, and the
``btcli subnets emission`` commands against the in-memory chain."""

from __future__ import annotations

import asyncio
import json

import pytest

from bittensor import emission_weights as ew
from bittensor.cli import context as cli_context
from bittensor.client import Client
from bittensor.intents import SetSubnetEmissionWeights
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import (
    EMISSION_ALPHA_ISSUANCE,
    EMISSION_ROOT_TAO,
    EMISSION_WEIGHTS_INFO,
    ROOT_BASELINE_SHARE,
    TAO_WEIGHT,
    dev_wallet,
)
from tests.unit.test_cli_commands import invoke, isolated_cli, wallet_dir  # noqa: F401

ONE = ew.ONE
DEFAULT = ew.default_weights()


# --- pure helpers -----------------------------------------------------------


def test_constants_match_chain():
    assert ew.MAX_OWNER == (ONE * 33) // 100
    assert ew.MAX_BURN_RECYCLE == (ONE * 33) // 100
    assert ew.MAX_VALIDATOR == (ONE * 50) // 100
    assert ew.MIN_MINER == (ONE * 33) // 100
    assert ew.DEFAULT_OWNER == (11796 * ONE) // 65535
    assert sum(DEFAULT) == ONE
    assert DEFAULT[ew.BURN] == DEFAULT[ew.RECYCLE] == 0
    assert DEFAULT[ew.VALIDATOR] == (ONE - ew.DEFAULT_OWNER) // 2
    assert ew.ADJUSTABLE_SLOTS == (ew.BURN, ew.RECYCLE, ew.OWNER, ew.VALIDATOR, ew.MINER)


@pytest.mark.parametrize(
    "raw, expected",
    [
        ("owner", 2),
        ("Validator", 3),
        (" miner ", 4),
        ("3", 3),
        (0, 0),
        (4, 4),
    ],
)
def test_slot_index_accepts_names_and_uids(raw, expected):
    assert ew.slot_index(raw) == expected


@pytest.mark.parametrize("raw", ["gamma", 5, -1, "5"])
def test_slot_index_rejects_unknown(raw):
    with pytest.raises(ValueError, match="slots are"):
        ew.slot_index(raw)


def test_parse_share_forms():
    assert ew.parse_share(0.5) == ew.fraction(1, 2)
    assert ew.parse_share("41%") == ew.fraction(41, 100)
    assert ew.parse_share("0.41") == ew.fraction(41, 100)
    assert ew.parse_share(" 12.5% ") == ew.fraction(125, 1000)
    assert ew.parse_share("1e-1") == ew.fraction(1, 10)
    assert ew.parse_share(12345) == 12345
    assert ew.parse_share("12345") == 12345
    assert ew.parse_share("1.0") == ONE
    assert ew.parse_share("100%") == ONE
    assert ew.parse_share("0") == 0
    assert ew.parse_share("0%") == 0


@pytest.mark.parametrize(
    ("raw", "bound"),
    [
        ("33%", ew.MAX_OWNER),
        ("0.33", ew.MAX_OWNER),
        (0.33, ew.MAX_OWNER),
        ("50%", ew.MAX_VALIDATOR),
        ("0.5", ew.MAX_VALIDATOR),
        (0.5, ew.MAX_VALIDATOR),
        ("33%", ew.MIN_MINER),
    ],
)
def test_parse_share_lands_exactly_on_chain_bounds(raw, bound):
    # The chain's bounds are floor(ONE * n / d); every spelling of the documented
    # inclusive maxima must parse to exactly that value, never one part above.
    assert ew.parse_share(raw) == bound


@pytest.mark.parametrize(
    "pinned",
    [
        # owner at its inclusive maximum, miner absorbs the rest
        {"owner": "33%", "validator": "34%"},
        {"owner": "0.33", "validator": "0.34"},
        {"owner": 0.33, "validator": 0.34},
        # validator at its inclusive maximum
        {"owner": "17%", "validator": "50%"},
        {"owner": "0.17", "validator": "0.5"},
        # miner exactly at its inclusive minimum (validator absorbs the rounding ulps)
        {"owner": "18%", "miner": "33%"},
        {"owner": 0.18, "miner": 0.33},
    ],
)
def test_documented_bounds_pass_validation_in_every_spelling(pinned):
    rules = ew.Rules()
    weights = ew.renormalize(
        ew.default_weights(),
        {slot: ew.parse_share(value) for slot, value in pinned.items()},
    )
    assert ew.validate(weights, rules) == [], pinned


@pytest.mark.parametrize("raw", ["1.5", "150%", "-0.1", -5, ONE + 1, "abc", "", True])
def test_parse_share_rejects_out_of_range(raw):
    with pytest.raises(ValueError):
        ew.parse_share(raw)


def test_format_share():
    assert ew.format_share(ONE) == "100.00%"
    assert ew.format_share(ONE // 2) == "50.00%"
    assert ew.format_share(0) == "0.00%"


def test_merge_keeps_omitted_slots_including_burn_and_recycle():
    current = [7, 9, 10, 20, 30]
    merged = ew.merge(current, {"owner": 11})
    assert merged == [7, 9, 11, 20, 30]
    assert ew.merge(current, {3: 1, "miner": 2}) == [7, 9, 10, 1, 2]


def test_renormalize_scales_untouched_slots_proportionally():
    owner = ew.parse_share("10%")
    out = ew.renormalize(DEFAULT, {"owner": owner})
    assert sum(out) == ONE
    assert out[ew.OWNER] == owner
    assert out[ew.BURN] == out[ew.RECYCLE] == 0
    # Validator and miner were (within a part) equal and stay so; miner absorbs rounding.
    assert abs(out[ew.VALIDATOR] - out[ew.MINER]) <= 4
    assert out[ew.VALIDATOR] > DEFAULT[ew.VALIDATOR]


def test_renormalize_keeps_two_pinned_and_fills_the_third():
    out = ew.renormalize(DEFAULT, {"owner": ONE // 10, "validator": ONE // 2})
    assert out == [0, 0, ONE // 10, ONE // 2, ONE - ONE // 10 - ONE // 2]


def test_renormalize_rejects_overflow_and_incomplete_pin():
    with pytest.raises(ValueError, match="exceed 100%"):
        ew.renormalize(DEFAULT, {"owner": ONE, "validator": 1})
    with pytest.raises(ValueError, match="not 100%"):
        ew.renormalize(
            DEFAULT,
            {
                "burn": 0,
                "recycle": 0,
                "owner": 1,
                "validator": 1,
                "miner": 1,
            },
        )


def test_renormalize_accepts_nonzero_burn_and_rescales_the_rest():
    burn = ew.parse_share("10%")
    out = ew.renormalize(DEFAULT, {"burn": burn})
    assert sum(out) == ONE
    assert out[ew.BURN] == burn
    assert out[ew.RECYCLE] == 0
    assert ew.validate(out) == []


def test_renormalize_splits_evenly_when_free_slots_are_zero():
    out = ew.renormalize([0, 0, ONE, 0, 0], {"owner": ONE // 2})
    assert sum(out) == ONE
    # Burn, recycle, validator and miner were all zero: remaining is split evenly.
    free = [out[ew.BURN], out[ew.RECYCLE], out[ew.VALIDATOR], out[ew.MINER]]
    assert max(free) - min(free) <= 1


def test_validate_accepts_default_and_names_each_rule():
    rules = ew.Rules(min_validator=ew.parse_share("2.2%"))
    assert ew.validate(DEFAULT, rules) == []

    def errors(weights, rules=rules):
        return [violation.error for violation in ew.validate(weights, rules)]

    assert errors([1, 0, *DEFAULT[2:]]) == ["EmissionWeightsDoNotSumToOne"]
    over_cap = [
        ew.MAX_BURN_RECYCLE + 1,
        0,
        0,
        ew.parse_share("34%"),
        ONE - (ew.MAX_BURN_RECYCLE + 1) - ew.parse_share("34%"),
    ]
    assert "BurnRecycleEmissionAboveMaximum" in errors(over_cap)
    split_over = [
        ew.parse_share("20%"),
        ew.parse_share("14%"),
        0,
        ew.parse_share("33%"),
        ONE - ew.parse_share("20%") - ew.parse_share("14%") - ew.parse_share("33%"),
    ]
    assert "BurnRecycleEmissionAboveMaximum" in errors(split_over)
    at_cap = [
        ew.parse_share("20%"),
        ew.parse_share("13%"),
        0,
        ew.parse_share("34%"),
        ONE - ew.parse_share("20%") - ew.parse_share("13%") - ew.parse_share("34%"),
    ]
    assert ew.validate(at_cap, rules) == []
    assert "OwnerEmissionAboveMaximum" in errors(
        ew.renormalize(DEFAULT, {"owner": ew.parse_share("34%")})
    )
    assert "ValidatorEmissionAboveMaximum" in errors(
        ew.renormalize(DEFAULT, {"validator": ew.parse_share("51%")})
    )
    assert errors(ew.renormalize(DEFAULT, {"validator": ew.parse_share("1%")})) == [
        "ValidatorEmissionTooLowToPreserveRoot"
    ]
    assert errors([0, 0, 0, 0, ONE]) == ["ValidatorEmissionTooLowToPreserveRoot"]
    # With no root stake there is nothing to preserve: even V = 0 passes.
    assert errors([0, 0, 0, 0, ONE], ew.Rules()) == []
    assert "MinerEmissionBelowMinimum" in errors(
        [0, 0, ew.MAX_OWNER, ew.MAX_VALIDATOR, ONE - ew.MAX_OWNER - ew.MAX_VALIDATOR]
    )
    assert errors([0, 0, 0, ONE // 2, ONE // 2]) == ["EmissionWeightsDoNotSumToOne"]


def test_validate_lease_floor_only_on_lease_created_subnets():
    low_owner = ew.renormalize(DEFAULT, {"owner": ew.parse_share("5%")})
    assert ew.validate(low_owner, ew.Rules()) == []
    leased = ew.Rules(min_owner=ew.DEFAULT_OWNER, created_via_lease=True)
    violations = ew.validate(low_owner, leased)
    assert [v.error for v in violations] == ["LeasedSubnetOwnerCutBelowMinimum"]
    assert "lease" in violations[0].message


def test_root_preservation_helpers_match_sn4_numbers():
    # Mainnet SN4-like: 5.443M TAO on root, 6.457M alpha issued, TaoWeight 18%, V0 = 41%.
    root_tao, issuance, tao_weight = 5_443_000e9, 6_457_000e9, 0.18
    baseline = ew.baseline_root_share(tao_weight, root_tao, issuance)
    assert baseline == pytest.approx(0.0540, abs=1e-4)
    # A subnet still on the default vector earns exactly the baseline.
    validator = ew.share_to_float(DEFAULT[ew.VALIDATOR])
    assert ew.root_emission_share(validator, tao_weight, root_tao, issuance) == pytest.approx(
        baseline
    )
    # The validator share must exceed the baseline itself: the chain pays root
    # ``min(V, r)`` off the top, so V <= r leaves alpha validators nothing.
    floor = ew.validator_floor(tao_weight, root_tao, issuance)
    assert floor == pytest.approx(baseline)
    baseline_parts = int(baseline * ONE)
    assert ew.min_validator_parts(baseline_parts) == baseline_parts + 1
    assert ew.min_validator_parts(ONE) == ONE
    # Whatever the owner sets, root is paid the baseline, capped at the slot.
    paid = ew.root_share_paid
    assert paid(ew.parse_share("30%"), baseline_parts) == pytest.approx(baseline)
    assert paid(ew.parse_share("5%"), baseline_parts) == pytest.approx(0.05)
    assert paid(ew.parse_share("5.5%"), baseline_parts) == pytest.approx(baseline)
    # The baseline moves with the global knob and the A/T ratio, never with V.
    assert ew.baseline_root_share(0.36, root_tao, issuance) > baseline
    assert ew.baseline_root_share(tao_weight, root_tao, 10 * issuance) < baseline
    # Nothing to preserve without root stake; no issuance means root earns V0.
    assert ew.validator_floor(tao_weight, 0, issuance) == 0.0
    assert ew.validator_floor(tao_weight, root_tao, 0) == pytest.approx(validator)
    assert ew.root_emission_share(validator, tao_weight, 0, 0) == 0.0


def test_preview_reports_vectors_and_root_shares():
    root_tao, issuance, tao_weight = 5_443_000 * 10**9, 6_457_000 * 10**9, 0.18
    baseline_parts = int(ew.baseline_root_share(tao_weight, root_tao, issuance) * ONE)
    baseline = ew.share_to_float(baseline_parts)
    after = ew.renormalize(DEFAULT, {"validator": ew.parse_share("30%")})
    preview = ew.preview(
        DEFAULT,
        after,
        baseline_parts=baseline_parts,
        global_tao_weight=tao_weight,
        root_tao=root_tao,
        alpha_issuance=issuance,
    )
    assert preview["before"]["weights"] == DEFAULT
    assert preview["after"]["weights"] == after
    assert preview["after"]["shares"]["validator"] == pytest.approx(0.30)
    assert preview["changed"] == ["owner", "validator", "miner"] or set(preview["changed"]) <= {
        "owner",
        "validator",
        "miner",
    }
    root = preview["root"]
    assert root["baseline_share"] == baseline
    # Root is paid the baseline before and after; validators get what is left.
    assert root["share_before"] == pytest.approx(baseline)
    assert root["share_after"] == pytest.approx(baseline)
    assert root["validators_before"] == pytest.approx(0.41 - baseline, abs=1e-4)
    assert root["validators_after"] == pytest.approx(0.30 - baseline)
    assert (root["capped_before"], root["capped_after"]) == (False, False)
    assert root["validator_floor"] == baseline
    assert root["global_tao_weight"] == tao_weight
    assert (root["root_tao"], root["alpha_issuance"]) == (root_tao, issuance)

    # Too low: the preview says so instead of guessing.
    too_low = ew.preview(
        DEFAULT,
        ew.renormalize(DEFAULT, {"validator": ew.parse_share("5%")}),
        baseline_parts=baseline_parts,
        global_tao_weight=tao_weight,
        root_tao=root_tao,
        alpha_issuance=issuance,
    )
    assert too_low["root"]["share_after"] is None
    assert too_low["root"]["validators_after"] is None
    assert too_low["root"]["capped_after"] is False

    # Capped: the baseline has grown past a slot that was fine when it was set.
    capped = ew.preview(
        ew.renormalize(DEFAULT, {"validator": ew.parse_share("5%")}),
        ew.renormalize(DEFAULT, {"validator": ew.parse_share("5.5%")}),
        baseline_parts=baseline_parts,
        global_tao_weight=tao_weight,
        root_tao=root_tao,
        alpha_issuance=issuance,
    )
    assert capped["root"]["capped_before"] is True
    assert capped["root"]["share_before"] == pytest.approx(0.05)
    assert capped["root"]["validators_before"] == pytest.approx(0.0)
    assert capped["root"]["capped_after"] is False
    assert capped["root"]["share_after"] == pytest.approx(baseline)
    assert capped["root"]["validators_after"] == pytest.approx(0.055 - baseline)


def test_rate_limit_status():
    status = ew.rate_limit_status(0, 1_000)
    assert status["owner_can_change_now"] and status["next_allowed_block"] == 1_000
    status = ew.rate_limit_status(900, 1_000)
    assert status == {
        "limit_blocks": 50_400,
        "last_change_block": 900,
        "next_allowed_block": 51_300,
        "blocks_until_allowed": 50_300,
        "owner_can_change_now": False,
    }
    assert ew.rate_limit_status(900, 51_300)["owner_can_change_now"]


def test_rules_limits_and_table_cover_every_slot():
    rules = ew.Rules(
        min_validator=ew.parse_share("2.2%"), min_owner=ew.DEFAULT_OWNER, created_via_lease=True
    )
    limits = rules.limits()
    assert list(limits) == ["burn", "recycle", "owner", "validator", "miner"]
    assert limits["owner"]["min_parts"] == ew.DEFAULT_OWNER and "lease" in limits["owner"]["why"]
    assert limits["validator"]["min"] == pytest.approx(0.022)
    assert limits["validator"]["max_parts"] == ew.MAX_VALIDATOR
    assert limits["miner"]["max_parts"] == ONE
    rows = {name: rule for name, rule, _ in rules.table()}
    assert rows["burn"] == "0.00% to 33.00%" and rows["recycle"] == "0.00% to 33.00%"
    assert rows["burn+recycle"] == "<= 33.00%"
    assert rows["owner"] == "18.00% to 33.00%"
    assert rows["validator"] == "2.2000% to 50.0000%"
    assert rows["miner"] == ">= 33.00%"
    assert rows["sum"] == "= 100%"
    assert limits["burn"]["max_parts"] == ew.MAX_BURN_RECYCLE
    # No floor: validator is only capped, owner starts at 0.
    rows = {name: rule for name, rule, _ in ew.Rules().table()}
    assert rows["validator"] == "<= 50.0000%" and rows["owner"] == "0.00% to 33.00%"
    assert ew.Rules().limits()["validator"]["min"] is None


# --- intent -----------------------------------------------------------------


ROOT_TAO = EMISSION_ROOT_TAO
ALPHA_ISSUANCE = EMISSION_ALPHA_ISSUANCE
GLOBAL_TAO_WEIGHT = ew.share_to_float(TAO_WEIGHT)
#: Root's baseline share as the runtime API reports it for the fixture (~1.74%).
BASELINE = ew.share_to_float(ROOT_BASELINE_SHARE)
#: Lowest validator share the chain accepts: one part above the baseline.
MIN_VALIDATOR = ew.min_validator_parts(ROOT_BASELINE_SHARE)
DEFAULT_VALIDATOR_SHARE = ew.share_to_float(DEFAULT[ew.VALIDATOR])
RATE_LIMIT_KEY = {"OwnerHyperparamUpdate": [1, "EmissionWeights"]}


def _client() -> tuple[Client, FakeSubstrate]:
    fake = FakeSubstrate()
    fake.seed_runtime("SubnetInfoRuntimeApi", "get_subnet_emission_weights", EMISSION_WEIGHTS_INFO)
    fake.seed("SubtensorModule", "TaoWeight", [], TAO_WEIGHT)
    fake.seed("SubtensorModule", "SubnetTAO", [0], ROOT_TAO)
    fake.seed("SubtensorModule", "SubnetAlphaIn", [1], 0)
    fake.seed("SubtensorModule", "SubnetAlphaOut", [1], ALPHA_ISSUANCE)
    fake.seed("Swap", "BalancerAlphaReservoir", [1], 0)
    fake.seed("SubtensorModule", "LastRateLimitedBlock", [RATE_LIMIT_KEY], 0)
    return Client("local", substrate=fake), fake


def test_fixture_baseline_matches_the_pure_formula():
    # The runtime payload in the harness is derived by hand; keep it honest.
    pure = ew.baseline_root_share(GLOBAL_TAO_WEIGHT, ROOT_TAO, ALPHA_ISSUANCE)
    assert pytest.approx(pure, rel=1e-9) == BASELINE
    assert pytest.approx(0.0174, abs=1e-4) == BASELINE


#: The live floor as the CLI prints it: the validator share must be *above* this
#: (1.7407% with the fixture's 5.27% global weight), i.e. at least MIN_VALIDATOR.
FLOOR_TEXT = ew.format_share(MIN_VALIDATOR - 1, 4)
#: The lowest accepted share as the rule table prints it.
MIN_VALIDATOR_TEXT = ew.format_share(MIN_VALIDATOR, 4)


def test_intent_normalises_slots_and_shares():
    intent = SetSubnetEmissionWeights(netuid=1, slots=["owner", "3"], values=["10%", 0.45])
    assert intent.slots == [2, 3]
    assert intent.values == [ew.parse_share("10%"), ew.parse_share(0.45)]
    assert "owner=10.00%" in intent.summary()


@pytest.mark.parametrize(
    "kwargs, match",
    [
        ({"slots": ["owner"], "values": []}, "pair up"),
        ({"slots": [], "values": []}, "at least one"),
        ({"slots": ["owner", 2], "values": [1, 2]}, "at most once"),
        ({"slots": ["gamma"], "values": [1]}, "slots are"),
    ],
)
def test_intent_rejects_malformed_input(kwargs, match):
    with pytest.raises(ValueError, match=match):
        SetSubnetEmissionWeights(netuid=1, **kwargs)


def test_intent_accepts_nonzero_burn():
    intent = SetSubnetEmissionWeights(netuid=1, slots=["burn"], values=["10%"], renormalize=True)
    assert intent.slots == [0]
    assert intent.values == [ew.parse_share("10%")]
    assert "burn=10.00%" in intent.summary()


def test_intent_plan_renormalizes_and_reports_root_baseline():
    client, _ = _client()
    intent = SetSubnetEmissionWeights(
        netuid=1, slots=["validator"], values=["30%"], renormalize=True
    )
    plan = asyncio.run(client.plan(intent, dev_wallet()))
    assert plan.ok, plan.violations
    assert any("validator 41.00%" in line and "validator 30.00%" in line for line in plan.effects)
    validators_before = ew.share_to_float(DEFAULT[ew.VALIDATOR]) - BASELINE
    assert any(
        f"root keeps its baseline {BASELINE * 100:.4f}% of this subnet's emission" in line
        and f"alpha validators get {validators_before * 100:.4f}% -> {(0.30 - BASELINE) * 100:.4f}%"
        in line
        for line in plan.effects
    ), plan.effects
    # The structured preview rides along for --json consumers.
    assert plan.preview["after"]["weights"][ew.VALIDATOR] == ew.parse_share("30%")
    assert plan.preview["root"]["share_after"] == pytest.approx(BASELINE)
    assert plan.preview["root"]["baseline_share"] == pytest.approx(BASELINE)
    assert plan.preview["root"]["validators_after"] == pytest.approx(0.30 - BASELINE)
    assert plan.preview["root"]["capped_after"] is False
    assert plan.preview["rate_limit"]["owner_can_change_now"] is True
    assert plan.preview["limits"]["validator"]["min_parts"] == MIN_VALIDATOR
    assert plan.to_dict()["preview"] == plan.preview

    # At or below the live baseline the plan is blocked before signing.
    too_low = SetSubnetEmissionWeights(
        netuid=1, slots=["validator"], values=["1%"], renormalize=True
    )
    plan = asyncio.run(client.plan(too_low, dev_wallet()))
    assert not plan.ok
    assert plan.violations[0].startswith("ValidatorEmissionTooLowToPreserveRoot")
    assert f"must be above {FLOOR_TEXT}" in plan.violations[0]
    assert any("validators would get nothing (the chain rejects)" in e for e in plan.effects)

    # One part above the baseline is the lowest the chain takes.
    lowest = SetSubnetEmissionWeights(
        netuid=1, slots=["validator"], values=[MIN_VALIDATOR], renormalize=True
    )
    plan = asyncio.run(client.plan(lowest, dev_wallet()))
    assert plan.ok, plan.violations
    assert plan.preview["root"]["share_after"] == pytest.approx(BASELINE)
    assert plan.preview["root"]["validators_after"] == pytest.approx(0.0, abs=1e-12)


def test_intent_plan_blocks_on_chain_rules():
    client, _ = _client()
    intent = SetSubnetEmissionWeights(netuid=1, slots=["owner"], values=["40%"], renormalize=True)
    plan = asyncio.run(client.plan(intent, dev_wallet()))
    assert not plan.ok
    assert plan.violations[0].startswith("OwnerEmissionAboveMaximum")

    verbatim = SetSubnetEmissionWeights(netuid=1, slots=["owner"], values=["10%"])
    plan = asyncio.run(client.plan(verbatim, dev_wallet()))
    assert [v.split(":")[0] for v in plan.violations] == ["EmissionWeightsDoNotSumToOne"]


def test_intent_plan_blocks_inside_the_owner_rate_limit():
    client, fake = _client()
    fake.seed("SubtensorModule", "LastRateLimitedBlock", [RATE_LIMIT_KEY], fake.block - 10)
    intent = SetSubnetEmissionWeights(netuid=1, slots=["owner"], values=["10%"], renormalize=True)
    plan = asyncio.run(client.plan(intent, dev_wallet()))
    assert not plan.ok
    (block,) = plan.violations
    assert f"block {fake.block - 10}" in block
    assert f"block {fake.block - 10 + 50_400:,}" in block and "root is exempt" in block
    assert plan.preview["rate_limit"]["blocks_until_allowed"] == 50_390

    # The window has passed: allowed again.
    fake.seed("SubtensorModule", "LastRateLimitedBlock", [RATE_LIMIT_KEY], fake.block - 50_400)
    plan = asyncio.run(client.plan(intent, dev_wallet()))
    assert plan.ok, plan.violations


def test_intent_plan_blocks_a_signer_who_is_not_the_owner():
    client, fake = _client()
    alice = dev_wallet()
    bob = dev_wallet("//Bob", "//Bob//hot").coldkey.ss58_address
    intent = SetSubnetEmissionWeights(netuid=1, slots=["owner"], values=["10%"], renormalize=True)

    fake.seed("SubtensorModule", "SubnetOwner", [1], bob)
    plan = asyncio.run(client.plan(intent, alice))
    assert not plan.ok
    (block,) = plan.violations
    assert alice.coldkey.ss58_address in block and bob in block
    assert "is not the owner of subnet 1" in block and "root through sudo" in block

    fake.seed("SubtensorModule", "SubnetOwner", [1], alice.coldkey.ss58_address)
    plan = asyncio.run(client.plan(intent, alice))
    assert plan.ok, plan.violations


def test_intent_plan_names_a_missing_subnet():
    client, fake = _client()
    fake.seed_runtime("SubnetInfoRuntimeApi", "get_subnet_emission_weights", None)
    intent = SetSubnetEmissionWeights(netuid=9, slots=["owner"], values=["10%"], renormalize=True)
    plan = asyncio.run(client.plan(intent, dev_wallet()))
    assert not plan.ok and "subnet 9 does not exist" in plan.violations[0]


def test_intent_build_sends_full_vector_when_renormalizing():
    _, fake = _client()
    owner = ew.parse_share("10%")
    intent = SetSubnetEmissionWeights(netuid=1, slots=["owner"], values=["10%"], renormalize=True)
    call = asyncio.run(intent.build(fake, dev_wallet()))
    assert (call.module, call.function) == ("AdminUtils", "sudo_set_subnet_emission_weights")
    params = call.params
    assert params["uids"] == [0, 1, 2, 3, 4]
    assert params["values"][2] == owner
    assert sum(params["values"]) == ONE

    sparse = SetSubnetEmissionWeights(netuid=1, slots=["miner", "owner"], values=[ONE // 2, 1])
    params = asyncio.run(sparse.build(fake, dev_wallet())).params
    assert params["uids"] == [2, 4]
    assert params["values"] == [1, ONE // 2]


# --- read -------------------------------------------------------------------


def test_read_decodes_runtime_payload():
    client, _ = _client()
    info = asyncio.run(client.read("subnet_emission_weights", netuid=1))
    assert info.weights == EMISSION_WEIGHTS_INFO["weights"]
    assert info.shares["owner"] == pytest.approx(0.18, abs=1e-4)
    assert info.global_tao_weight == pytest.approx(0.0527, abs=1e-4)
    assert (info.root_tao, info.alpha_issuance) == (ROOT_TAO, ALPHA_ISSUANCE)
    assert info.root_share == pytest.approx(0.0174, abs=1e-4)
    assert info.baseline_root_share == pytest.approx(info.root_share)
    assert info.validator_share_after_root == pytest.approx(0.41 - BASELINE, abs=1e-4)
    assert info.root_capped is False
    assert info.min_validator == MIN_VALIDATOR
    assert info.rules().min_validator == MIN_VALIDATOR
    assert not info.rules().created_via_lease
    assert info.block == 100 and info.last_change_block == 0
    assert info.rate_limit()["owner_can_change_now"] is True


# --- CLI --------------------------------------------------------------------


SHOW_FIELDS = [
    "netuid",
    "block",
    "weights",
    "shares",
    "global_tao_weight",
    "root_share",
    "baseline_root_share",
    "validator_share_after_root",
    "root_capped",
    "root_tao",
    "alpha_issuance",
    "min_validator",
    "created_via_lease",
    "limits",
    "rate_limit",
]


@pytest.fixture()
def emission_cli(isolated_cli, monkeypatch) -> FakeSubstrate:  # noqa: F811
    client, fake = _client()
    fake.seed_default("System", "Account", {"data": {"free": 10**18, "reserved": 0, "frozen": 0}})
    monkeypatch.setattr(cli_context, "Client", lambda network, **kwargs: client)
    return fake


def _stderr_json(result) -> dict:
    """The ``{"error": ...}`` record a failed JSON-mode command prints to stderr."""
    text = result.stderr if hasattr(result, "stderr") and result.stderr else result.output
    return json.loads(text[text.index("{") :])


def test_cli_show_json_has_stable_fields(emission_cli):
    result = invoke("--json", "subnets", "emission", "show", "--netuid", "1")
    assert result.exit_code == 0, result.output
    record = json.loads(result.output)
    assert list(record) == SHOW_FIELDS
    assert record["weights"] == EMISSION_WEIGHTS_INFO["weights"]
    assert record["shares"]["validator"] == pytest.approx(0.41, abs=1e-4)
    assert record["root_share"] == pytest.approx(BASELINE)
    assert record["baseline_root_share"] == pytest.approx(BASELINE)
    assert record["validator_share_after_root"] == pytest.approx(0.41 - BASELINE, abs=1e-4)
    assert record["root_capped"] is False
    assert record["min_validator"] == MIN_VALIDATOR
    assert record["limits"]["validator"]["min_parts"] == MIN_VALIDATOR
    assert record["limits"]["validator"]["max"] == 0.5
    assert record["limits"]["owner"] == {
        "min": 0.0,
        "max": pytest.approx(0.33),
        "min_parts": 0,
        "max_parts": ew.MAX_OWNER,
        "why": "0 turns the owner cut off",
    }
    assert record["rate_limit"] == {
        "limit_blocks": 50_400,
        "last_change_block": 0,
        "next_allowed_block": 100,
        "blocks_until_allowed": 0,
        "owner_can_change_now": True,
    }


def test_cli_show_human_output(emission_cli):
    human = invoke("subnets", "emission", "show", "--netuid", "1")
    assert human.exit_code == 0, human.output
    out = human.output
    for needle in (
        "Vector",
        "burn  0.00%",
        "recycle  0.00%",
        "owner  18.00%",
        "validator  41.00%",
        "miner  41.00%",
        "Root",
        "root share of emission  1.7",
        "paid off the top of the validator slot",
        "root baseline share  1.7",
        "global TaoWeight (5.27%)",
        "recomputed every block",
        f"validators after root  {(DEFAULT_VALIDATOR_SHARE - BASELINE) * 100:.4f}%",
        f"validator floor  > {FLOOR_TEXT}",
        "root tao  5,443,000 TAO",
        "alpha issuance  6,457,000 alpha",
        "next owner change  now",
        "Allowed",
        f"validator  {MIN_VALIDATOR_TEXT} to 50.0000%",
        "miner  >= 33.00%",
        "burn+recycle  <= 33.00%",
        "sum  = 100%",
        "set-value --netuid 1",
    ):
        assert needle in out, (needle, out)


def test_cli_show_reports_a_capped_root_share_and_a_closed_window(emission_cli):
    # The baseline grew past a 5% validator slot after it was set: root takes
    # the whole slot and the owner must raise the share to pay validators again.
    low = ew.renormalize(EMISSION_WEIGHTS_INFO["weights"], {"validator": ew.parse_share("5%")})
    capped = dict(
        EMISSION_WEIGHTS_INFO,
        weights=low,
        owner=low[ew.OWNER],
        validator=low[ew.VALIDATOR],
        miner=low[ew.MINER],
        root_baseline_share=ew.parse_share("6%"),
        root_share=low[ew.VALIDATOR],
    )
    emission_cli.seed_runtime("SubnetInfoRuntimeApi", "get_subnet_emission_weights", capped)
    emission_cli.seed("SubtensorModule", "LastRateLimitedBlock", [RATE_LIMIT_KEY], 90)
    human = invoke("subnets", "emission", "show", "--netuid", "1")
    assert human.exit_code == 0, human.output
    assert "root share of emission  5.0000%  the baseline, capped at the validator slot" in (
        human.output
    )
    assert "root baseline share  6.0000%" in human.output
    assert "validators after root  0.0000%  nothing: raise the validator share" in human.output
    assert "validator floor  > 6.0000%" in human.output
    assert "validator  6.0000% to 50.0000%" in human.output
    assert "next owner change  block 50,490" in human.output
    assert "root is exempt" in human.output
    record = json.loads(invoke("--json", "subnets", "emission", "show", "--netuid", "1").output)
    assert record["root_capped"] is True
    assert record["root_share"] == pytest.approx(0.05)
    assert record["baseline_root_share"] == pytest.approx(0.06)
    assert record["validator_share_after_root"] == 0.0
    assert record["min_validator"] == ew.parse_share("6%") + 1
    assert record["rate_limit"]["next_allowed_block"] == 50_490
    assert record["rate_limit"]["owner_can_change_now"] is False


def test_cli_show_missing_subnet_exits_1(emission_cli):
    emission_cli.seed_runtime("SubnetInfoRuntimeApi", "get_subnet_emission_weights", None)
    result = invoke("--json", "subnets", "emission", "show", "--netuid", "7")
    assert result.exit_code == 1
    assert _stderr_json(result)["error"] == "subnet 7 does not exist or has no emission vector"


def test_cli_set_value_dry_run_renormalizes(emission_cli):
    result = invoke(
        "--json",
        "--dry-run",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "owner",
        "--value",
        "10%",
    )
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert plan["ok"], plan
    assert plan["args"]["renormalize"] is True
    assert plan["args"]["slots"] == [2]
    preview = plan["preview"]
    assert preview["before"]["weights"] == EMISSION_WEIGHTS_INFO["weights"]
    assert preview["after"]["weights"][2] == ew.parse_share("10%")
    assert sum(preview["after"]["weights"]) == ONE
    assert preview["changed"] == ["owner", "validator", "miner"]
    assert preview["root"]["share_before"] == pytest.approx(BASELINE)
    assert preview["root"]["share_after"] == pytest.approx(BASELINE)
    validators_after = ew.share_to_float(preview["after"]["weights"][ew.VALIDATOR]) - BASELINE
    assert preview["root"]["validators_after"] == pytest.approx(validators_after)
    assert preview["root"]["global_tao_weight"] == pytest.approx(GLOBAL_TAO_WEIGHT)
    assert preview["rate_limit"]["owner_can_change_now"] is True
    assert "validator" in preview["limits"]
    assert "command" in plan
    assert emission_cli.submissions == []

    human = invoke(
        "--dry-run",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "owner",
        "--value",
        "10%",
    )
    assert human.exit_code == 0, human.output
    assert "dry run:" in human.output
    assert "owner 18.00% / validator 41.00% / miner 41.00% -> owner 10.00%" in human.output
    assert f"root keeps its baseline {BASELINE * 100:.4f}%" in human.output
    assert "alpha validators get" in human.output
    assert "run for real" in human.output


def test_cli_increase_and_decrease_dry_run(emission_cli):
    for verb in ("increase", "decrease"):
        result = invoke(
            "--json",
            "--dry-run",
            "subnets",
            "emission",
            verb,
            "--netuid",
            "1",
            "--slot",
            "validator",
            "--by",
            "5%",
        )
        assert result.exit_code == 0, (verb, result.output)
        plan = json.loads(result.output)
        assert plan["ok"], plan
        delta = ew.parse_share("5%") if verb == "increase" else -ew.parse_share("5%")
        assert plan["args"]["values"][0] == EMISSION_WEIGHTS_INFO["validator"] + delta
        assert plan["preview"]["after"]["weights"][ew.VALIDATOR] == plan["args"]["values"][0]


def test_cli_set_full_vector_dry_run(emission_cli):
    result = invoke(
        "--json",
        "--dry-run",
        "subnets",
        "emission",
        "set",
        "--netuid",
        "1",
        "--owner",
        "0.10",
        "--validator",
        "0.45",
        "--miner",
        "0.45",
    )
    assert result.exit_code == 0, result.output
    plan = json.loads(result.output)
    assert plan["ok"], plan
    assert plan["args"]["slots"] == [0, 1, 2, 3, 4]
    assert sum(plan["args"]["values"]) == ONE

    off = invoke(
        "--json",
        "subnets",
        "emission",
        "set",
        "--netuid",
        "1",
        "--owner",
        "10%",
        "--validator",
        "45%",
        "--miner",
        "40%",
    )
    assert off.exit_code == 2, off.output
    assert "must add up to 100%" in _stderr_json(off)["error"]


def test_cli_explains_rule_violations_before_submitting(emission_cli):
    result = invoke(
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "owner",
        "--value",
        "40%",
    )
    assert result.exit_code == 2, result.output
    assert "OwnerEmissionAboveMaximum" in result.output
    assert "may be at" in result.output and "33.00%" in result.output
    assert "allowed on subnet 1 right now: owner 0.00% to 33.00%" in result.output
    assert emission_cli.submissions == []

    too_low = invoke(
        "--json",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "validator",
        "--value",
        "1%",
    )
    assert too_low.exit_code == 2, too_low.output
    error = _stderr_json(too_low)
    assert error["error"].startswith(
        "the chain would reject this change: ValidatorEmissionTooLowToPreserveRoot"
    )
    assert f"must be above {FLOOR_TEXT} or validators would get nothing" in error["error"]
    assert error["note"].startswith(
        f"allowed on subnet 1 right now: validator {MIN_VALIDATOR_TEXT} to 50.0000%"
    )
    assert (
        error["help"]
        == "`btcli subnets emission show --netuid 1` prints the vector and every allowed range"
    )

    over_burn = invoke(
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "burn",
        "--value",
        "40%",
    )
    assert over_burn.exit_code == 2, over_burn.output
    assert "BurnRecycleEmissionAboveMaximum" in over_burn.output

    burn_ok = invoke(
        "--json",
        "--dry-run",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "burn",
        "--value",
        "10%",
    )
    assert burn_ok.exit_code == 0, burn_ok.output
    burn_plan = json.loads(burn_ok.output)
    assert burn_plan["ok"], burn_plan
    assert burn_plan["preview"]["after"]["weights"][ew.BURN] == ew.parse_share("10%")

    bad_share = invoke(
        "subnets", "emission", "set-value", "--netuid", "1", "--slot", "owner", "--value", "lots"
    )
    assert (
        bad_share.exit_code == 2
        and "invalid value for --value: cannot read emission share" in bad_share.output
    )


def test_cli_decrease_rejects_more_than_the_slot_holds(emission_cli):
    result = invoke(
        "subnets", "emission", "decrease", "--netuid", "1", "--slot", "owner", "--by", "50%"
    )
    assert result.exit_code == 2, result.output
    assert "cannot take" in result.output


def test_cli_rate_limited_owner_is_blocked_with_exit_1(emission_cli):
    emission_cli.seed("SubtensorModule", "LastRateLimitedBlock", [RATE_LIMIT_KEY], 90)
    result = invoke(
        "--json",
        "--dry-run",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "owner",
        "--value",
        "10%",
    )
    assert result.exit_code == 1, result.output
    plan = json.loads(result.output)
    assert plan["ok"] is False
    assert "next owner change at block 50,490" in plan["violations"][0]
    assert plan["preview"]["rate_limit"]["owner_can_change_now"] is False

    real = invoke(
        "--yes",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "owner",
        "--value",
        "10%",
    )
    assert real.exit_code == 1, real.output
    assert "root is exempt" in real.output
    assert emission_cli.submissions == []


def test_cli_set_value_submits_the_renormalized_vector(emission_cli):
    result = invoke(
        "--yes",
        "subnets",
        "emission",
        "set-value",
        "--netuid",
        "1",
        "--slot",
        "validator",
        "--value",
        "30%",
    )
    assert result.exit_code == 0, result.output
    (call, signer, _) = emission_cli.submissions[-1]
    assert (call.module, call.function) == ("AdminUtils", "sudo_set_subnet_emission_weights")
    assert call.params["netuid"] == 1
    assert call.params["uids"] == [0, 1, 2, 3, 4]
    assert call.params["values"][3] == ew.parse_share("30%")
    assert sum(call.params["values"]) == ONE
    assert signer.startswith("5")
    # The review card names the move, root's unchanged cut and what validators keep.
    assert "41.00% -> 30.00%" in result.output
    assert "root share of emission" in result.output and "paid off the top" in result.output
    assert f"{BASELINE * 100:.4f}% -> {BASELINE * 100:.4f}%" in result.output
    assert "validators after root" in result.output
    assert f"-> {(0.30 - BASELINE) * 100:.4f}%" in result.output
    assert "next owner change after this" in result.output
