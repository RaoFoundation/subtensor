//! A claim sells the claimant's part of the deferred dividend credits with work that
//! grows with the number of credits, and with the results of the code of runtime 494.
//!
//! A deferred credit is a queued dividend credit that cannot go into the fund at the
//! current price. A claimant can sell their own part of it in a claim. Each sale adds
//! root stake, and each stake change moves the claimant's debt on every deferred credit of
//! the validator. In runtime 494 each sale walked all deferred credits in storage: `n`
//! credits were about `n * n` storage steps, and the claim weight counts `n`.
//!
//! The current code (`DeferredClaim`) reads the credits and the debts once. These tests
//! hold two things:
//! * Equal results. The code of runtime 494 is kept for tests
//!   (`claim_deferred_dividends_reference`). The same steps run with both, on equal
//!   states, and the complete storage is compared after each step.
//! * Linear work. The storage steps that grow with the number of credits are counted.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::staking::{DeferredBasketDebt, DeferredBasketState};
use crate::tests::claim_root::{flush_baskets, root_stake_of};
use crate::tests::claim_root_hotkey_weight::{RealFund, SHARE, TAO, priced_subnet, real_fund};
use crate::tests::mock::*;
use crate::{
    BasketRate, DeferredBasketCashRate, DeferredBasketConversions, DeferredBasketDebts,
    DeferredBasketRates, PendingBasketDeposits, RootClaimableThreshold, SubnetAlphaIn,
    SubnetAlphaOut, SubnetFastMovingPrice, SubnetLimit, SubnetTAO,
};
use frame_support::BoundedBTreeMap;
use frame_support::pallet_prelude::DispatchResult;
use sp_core::U256;
use sp_runtime::StateVersion;
use std::collections::{BTreeMap, BTreeSet};
use substrate_fixed::types::{I64F64, I96F32, U64F64};
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

/// A small generator (xorshift64*). One seed is one fixed scenario.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }
}

/// One step of a scenario. Indexes are positions in the scenario's stakers and subnets.
#[derive(Clone, Debug)]
enum Step {
    /// `claim_root_with_hotkey` by one staker.
    Claim {
        staker: usize,
    },
    /// Coldkey-wide `claim_root` by one staker.
    ClaimAll {
        staker: usize,
    },
    /// The staker adds root stake.
    AddStake {
        staker: usize,
        amount: u64,
    },
    /// The staker removes this percentage of their root stake.
    RemoveStake {
        staker: usize,
        percent: u64,
    },
    /// Move the price reference of a subnet: the spot price is then in the dividend band
    /// (credits can go into the fund) or far out of it (credits are deferred).
    Price {
        subnet: usize,
        in_band: bool,
    },
    /// A new root dividend for the validator, as an epoch makes it.
    Credit {
        subnet: usize,
        alpha: u64,
    },
    /// Close the deferred credit of a subnet into cash, as the removal of the subnet
    /// does. Later stake changes then move the cash watermark too.
    Dissolve {
        subnet: usize,
    },
    /// The per-block drain of the dividend queue.
    Flush,
    NextBlock,
    Threshold {
        rao: u64,
    },
}

struct Scenario {
    /// Alpha of each fund row.
    rows: Vec<u64>,
    /// Fund shares of each root staker, in units of `SHARE`.
    stakes: Vec<u64>,
    /// Subnets where the fund has no row.
    more_subnets: u32,
    steps: Vec<Step>,
}

fn scenario(seed: u64) -> Scenario {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let rows: Vec<u64> = (0..1 + rng.below(8))
        .map(|_| (1 + rng.below(9)) * TAO)
        .collect();
    let stakes: Vec<u64> = (0..1 + rng.below(4)).map(|_| 1 + rng.below(5)).collect();
    let more_subnets = rng.below(4) as u32;
    let subnets = rows.len() + more_subnets as usize;
    let stakers = stakes.len();

    let credit_alpha = |rng: &mut Rng| (1 + rng.below(2_000)) * (TAO / 1_000);
    let mut steps = Vec::new();
    // Most subnets have a credit, and most prices are out of the band: the first claim
    // defers these credits and sells the claimant's part.
    for subnet in 0..subnets {
        if rng.below(5) > 0 {
            steps.push(Step::Credit {
                subnet,
                alpha: credit_alpha(&mut rng),
            });
        }
        if rng.below(4) > 0 {
            steps.push(Step::Price {
                subnet,
                in_band: false,
            });
        }
    }
    for _ in 0..8 + rng.below(12) {
        let staker = rng.below(stakers as u64) as usize;
        let subnet = rng.below(subnets as u64) as usize;
        steps.push(match rng.below(20) {
            0..=7 => Step::Claim { staker },
            8 => Step::ClaimAll { staker },
            9..=10 => Step::AddStake {
                staker,
                amount: (1 + rng.below(20)) * SHARE,
            },
            11..=12 => Step::RemoveStake {
                staker,
                percent: 10 + rng.below(91),
            },
            13..=14 => Step::Price {
                subnet,
                in_band: rng.below(2) == 0,
            },
            15 => Step::Credit {
                subnet,
                alpha: credit_alpha(&mut rng),
            },
            16 => Step::Dissolve { subnet },
            17 => Step::Flush,
            18 => Step::NextBlock,
            _ => Step::Threshold {
                rao: [0, 500_000, 50_000_000][rng.below(3) as usize],
            },
        });
    }
    // Every staker claims at the end.
    for staker in 0..stakers {
        steps.push(Step::Claim { staker });
    }
    Scenario {
        rows,
        stakes,
        more_subnets,
        steps,
    }
}

struct World {
    fund: RealFund,
    /// The subnets of the fund rows, then the subnets without a row.
    subnets: Vec<NetUid>,
}

fn build(scenario: &Scenario) -> World {
    SubnetLimit::<Test>::put(2048);
    let fund = real_fund(&scenario.rows, &scenario.stakes);
    let mut subnets = fund.netuids.clone();
    for index in 0..scenario.more_subnets {
        subnets.push(priced_subnet(1_000 + index));
    }
    World { fund, subnets }
}

/// Set the price reference of `netuid` to its spot price (in the band) or to twice its
/// spot price (out of the band).
fn set_price_reference(netuid: NetUid, in_band: bool) {
    let spot = U64F64::from_num(SubnetTAO::<Test>::get(netuid).to_u64())
        / U64F64::from_num(SubnetAlphaIn::<Test>::get(netuid).to_u64().max(1));
    let reference = if in_band {
        spot
    } else {
        spot * U64F64::from_num(2)
    };
    SubnetFastMovingPrice::<Test>::insert(netuid, reference);
}

fn credit(hotkey: &U256, netuid: NetUid, alpha: u64) {
    SubnetAlphaOut::<Test>::mutate(netuid, |out| *out = out.saturating_add(alpha.into()));
    SubtensorModule::enqueue_root_dividend(hotkey, netuid, alpha.into());
}

/// Run one step. The text is the result, for the comparison of the two worlds. A stake
/// change that fails is rolled back, as its extrinsic does.
fn apply(world: &World, step: &Step) -> String {
    let hotkey = world.fund.hotkey;
    match step {
        Step::Claim { staker } => format!(
            "{:?}",
            SubtensorModule::claim_root_with_hotkey(
                RuntimeOrigin::signed(world.fund.stakers[*staker]),
                hotkey
            )
        ),
        Step::ClaimAll { staker } => format!(
            "{:?}",
            SubtensorModule::claim_root(
                RuntimeOrigin::signed(world.fund.stakers[*staker]),
                BTreeSet::from([NetUid::ROOT])
            )
        ),
        Step::AddStake { staker, amount } => {
            let staker = world.fund.stakers[*staker];
            format!(
                "{:?}",
                frame_support::storage::with_storage_layer(|| -> DispatchResult {
                    mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey,
                        &staker,
                        NetUid::ROOT,
                        (*amount).into(),
                    );
                    SubtensorModule::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                        &hotkey, &staker, *amount,
                    )
                })
            )
        }
        Step::RemoveStake { staker, percent } => {
            let staker = world.fund.stakers[*staker];
            let amount = root_stake_of(&hotkey, &staker) / 100 * percent;
            format!(
                "{:?}",
                frame_support::storage::with_storage_layer(|| -> DispatchResult {
                    let removed = SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey,
                        &staker,
                        NetUid::ROOT,
                        amount.into(),
                    );
                    SubtensorModule::remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                        &hotkey, &staker, removed,
                    )
                })
            )
        }
        Step::Price { subnet, in_band } => {
            set_price_reference(world.subnets[*subnet], *in_band);
            String::new()
        }
        Step::Credit { subnet, alpha } => {
            credit(&hotkey, world.subnets[*subnet], *alpha);
            String::new()
        }
        Step::Dissolve { subnet } => {
            let netuid = world.subnets[*subnet];
            if !DeferredBasketRates::<Test>::contains_key(hotkey, netuid) {
                return String::new();
            }
            let alpha = PendingBasketDeposits::<Test>::get(hotkey, netuid);
            format!(
                "{:?}",
                SubtensorModule::dissolve_deferred_dividend(&hotkey, netuid, alpha)
            )
        }
        Step::Flush => {
            flush_baskets();
            String::new()
        }
        Step::NextBlock => {
            System::set_block_number(System::block_number() + 1);
            String::new()
        }
        Step::Threshold { rao } => {
            RootClaimableThreshold::<Test>::insert(NetUid::ROOT, I96F32::from_num(*rao));
            String::new()
        }
    }
}

/// What the scenarios covered, summed over all seeds, so that the comparison cannot
/// pass with nothing to compare.
#[derive(Default)]
struct Coverage {
    /// Claims that sold a part of at least one deferred credit.
    claims_with_sales: u64,
    /// Claims that sold a part of at least three deferred credits.
    claims_with_three_sales: u64,
    /// Claims that sold a whole deferred credit, which converts its state.
    claims_with_conversions: u64,
    /// Claims with a sale, of an account that had debts when the claim started.
    claims_with_debts: u64,
    /// Steps that left a conversion record for a later settlement.
    steps_with_conversions: u64,
    /// Claims with a sale on a validator that owes cash of a dissolved credit.
    claims_with_cash: u64,
    /// Steps with an error result.
    errors: u64,
}

/// Run `steps` in two worlds that `setup` builds equal: one with the code of runtime 494
/// and one with the current code. After each step the result and the complete storage
/// must be equal.
fn assert_equal_worlds(
    name: &str,
    setup: impl Fn() -> World,
    steps: &[Step],
    coverage: &mut Coverage,
) {
    let mut reference = new_test_ext(1);
    let mut current = new_test_ext(1);
    let reference_world = reference.execute_with(&setup);
    let current_world = current.execute_with(&setup);
    let root = || sp_io::storage::root(StateVersion::V1);
    assert_eq!(
        reference.execute_with(root),
        current.execute_with(root),
        "{name}: the two worlds must start equal"
    );

    for (index, step) in steps.iter().enumerate() {
        let expected = reference.execute_with(|| {
            set_reference_deferred_claims(true);
            let result = apply(&reference_world, step);
            set_reference_deferred_claims(false);
            (result, root())
        });
        let actual = current.execute_with(|| {
            let hotkey = current_world.fund.hotkey;
            let debts = match step {
                Step::Claim { staker } | Step::ClaimAll { staker } => {
                    DeferredBasketDebts::<Test>::get(hotkey, current_world.fund.stakers[*staker])
                        .len()
                }
                _ => 0,
            };
            let sales_before = deferred_claim_path(DeferredClaimPath::Sale);
            let converted_before = deferred_claim_path(DeferredClaimPath::Converted);
            let result = apply(&current_world, step);
            if matches!(step, Step::Claim { .. } | Step::ClaimAll { .. })
                && result.starts_with("Ok")
            {
                let sold = deferred_claim_path(DeferredClaimPath::Sale) - sales_before;
                let converted =
                    deferred_claim_path(DeferredClaimPath::Converted) - converted_before;
                coverage.claims_with_sales += u64::from(sold > 0);
                coverage.claims_with_three_sales += u64::from(sold >= 3);
                coverage.claims_with_conversions += u64::from(converted > 0);
                coverage.claims_with_debts += u64::from(debts > 0 && sold > 0);
                coverage.claims_with_cash += u64::from(
                    sold > 0 && DeferredBasketCashRate::<Test>::get(hotkey) != I64F64::from_num(0),
                );
            }
            coverage.steps_with_conversions +=
                u64::from(DeferredBasketConversions::<Test>::iter().next().is_some());
            coverage.errors += u64::from(result.starts_with("Err"));
            (result, root())
        });
        assert_eq!(
            expected.0, actual.0,
            "{name}: step {index} ({step:?}) has another result"
        );
        assert_eq!(
            expected.1, actual.1,
            "{name}: step {index} ({step:?}) leaves another storage"
        );
    }
}

/// Random scenarios: funds with 1 to 8 rows, 1 to 4 root stakers, credits that are
/// deferred and credits that go into the fund, stake changes while credits wait, and
/// claims by every staker. The current code and the code of runtime 494 give the same
/// result and the same storage after every step.
#[test]
fn claims_of_deferred_credits_equal_the_code_of_runtime_494() {
    let mut coverage = Coverage::default();
    for seed in 0..160u64 {
        let scenario = scenario(seed);
        assert_equal_worlds(
            &format!("seed {seed}"),
            || build(&scenario),
            &scenario.steps,
            &mut coverage,
        );
    }
    // The scenarios reach the cases that matter. Without the minimum numbers, a change
    // of the scenarios could leave nothing to compare.
    for (name, count, minimum) in [
        ("claims with a sale", coverage.claims_with_sales, 350),
        (
            "claims with three sales",
            coverage.claims_with_three_sales,
            220,
        ),
        (
            "claims that convert a state",
            coverage.claims_with_conversions,
            50,
        ),
        (
            "claims with a sale of an account with debts",
            coverage.claims_with_debts,
            100,
        ),
        (
            "claims with a sale on a validator that owes cash",
            coverage.claims_with_cash,
            40,
        ),
        (
            "steps with a conversion record",
            coverage.steps_with_conversions,
            300,
        ),
        (
            "kept stake changes",
            deferred_claim_path(DeferredClaimPath::Kept),
            1_000,
        ),
    ] {
        assert!(count >= minimum, "{name}: {count}, minimum {minimum}");
    }
}

// =============================================================================
// Large numbers: the paths that the bound on the debts selects.
// =============================================================================

/// A fund whose sole root staker has `sales + 1` deferred credits of 1 TAO of alpha each.
/// The first credit in storage order is the "wide" one: nothing of it is left in the
/// queue, so the claim sells nothing of it, and its rate is `wide_rate`. The claimant has
/// a debt of `debt` on every credit. `high_debt` puts another debt on one more credit
/// that is not sold: the bound on the debts then starts from that value.
///
/// The claim sells the claimant's part of the other `sales` credits. Each sale adds about
/// 1 TAO of root stake, so the debt on the wide credit grows by about
/// `wide_rate * 10^9` for each sale.
fn wide_rate_world(sales: usize, wide_rate: u64, debt: I64F64, high_debt: Option<I64F64>) -> World {
    SubnetLimit::<Test>::put(2048);
    let spare = usize::from(high_debt.is_some());
    let fund = real_fund(&vec![5 * TAO; sales + 1 + spare], &[1]);
    let claimant = fund.stakers[0];
    for netuid in fund.netuids.iter() {
        SubnetAlphaOut::<Test>::mutate(*netuid, |out| *out = out.saturating_add(TAO.into()));
        SubtensorModule::enqueue_basket_deposit(&fund.hotkey, *netuid, TAO.into());
        // Out of the band: the claim does not put the credit into the fund.
        set_price_reference(*netuid, false);
    }
    SubtensorModule::snapshot_deferred_dividends(&fund.hotkey).expect("the credits defer");
    // No fund shares to redeem after the direct sales: the last stake change of the
    // claim would then move the wide debt once more.
    BasketRate::<Test>::insert(fund.hotkey, I96F32::from_num(0));

    let states: Vec<(NetUid, DeferredBasketState)> =
        DeferredBasketRates::<Test>::iter_prefix(fund.hotkey).collect();
    assert_eq!(states.len(), sales + 1 + spare);
    let mut debts = BTreeMap::new();
    for (position, (netuid, mut state)) in states.into_iter().enumerate() {
        let mut amount = debt;
        if position == 0 {
            state.rate = I64F64::from_num(wide_rate);
            PendingBasketDeposits::<Test>::remove(fund.hotkey, netuid);
        } else if position == 1 && spare == 1 {
            // Not sold either, and its rate stays small.
            amount = high_debt.expect("spare is set");
            PendingBasketDeposits::<Test>::remove(fund.hotkey, netuid);
        }
        state.debt_count = 1;
        debts.insert(
            netuid,
            DeferredBasketDebt {
                generation: state.generation,
                amount,
            },
        );
        DeferredBasketRates::<Test>::insert(fund.hotkey, netuid, state);
    }
    DeferredBasketDebts::<Test>::insert(
        fund.hotkey,
        claimant,
        BoundedBTreeMap::try_from(debts).expect("the debts fit the map"),
    );
    let subnets = fund.netuids.clone();
    World { fund, subnets }
}

/// `-2^62`: a debt so far below zero that the sum of the changes can be above the
/// largest value while the debt itself stays in range.
fn deep_debt() -> I64F64 {
    I64F64::from_num(-(1i64 << 62))
}

/// The kept stake changes of five sales sum to more than the largest value when they
/// are multiplied with the wide rate in one product, but the debt stays in range. The
/// claim then adds the single products. The result is the result of runtime 494.
#[test]
fn kept_changes_whose_one_product_overflows_are_added_one_at_a_time() {
    let mut coverage = Coverage::default();
    reset_deferred_claim_paths();
    assert_equal_worlds(
        "single products",
        || wide_rate_world(5, 2_500_000_000, deep_debt(), None),
        &[Step::Claim { staker: 0 }],
        &mut coverage,
    );
    assert_eq!(coverage.errors, 0, "the claim must succeed");
    assert_eq!(coverage.claims_with_three_sales, 1);
    assert_eq!(deferred_claim_path(DeferredClaimPath::Kept), 4);
    assert_eq!(deferred_claim_path(DeferredClaimPath::NotCovered), 0);
    assert!(deferred_claim_path(DeferredClaimPath::SingleProducts) >= 1);
}

/// One debt is high, so the bound cannot cover the third kept change, although no debt
/// overflows. The claim then adds each later change to each debt at once, as runtime 494
/// does. The result is the result of runtime 494.
#[test]
fn change_that_the_bound_does_not_cover_goes_to_each_debt_at_once() {
    let mut coverage = Coverage::default();
    reset_deferred_claim_paths();
    assert_equal_worlds(
        "not covered",
        || {
            wide_rate_world(
                5,
                2_500_000_000,
                deep_debt(),
                Some(I64F64::from_num(4_000_000_000_000_000_000i64)),
            )
        },
        &[Step::Claim { staker: 0 }],
        &mut coverage,
    );
    assert_eq!(coverage.errors, 0, "the claim must succeed");
    assert_eq!(coverage.claims_with_three_sales, 1);
    assert_eq!(deferred_claim_path(DeferredClaimPath::NotCovered), 1);
    assert!(deferred_claim_path(DeferredClaimPath::Kept) >= 1);
}

/// With one more sale the wide debt overflows. Both codes fail with the same error at
/// the same sale, and the claim changes nothing.
#[test]
fn debt_that_overflows_fails_like_runtime_494() {
    let mut coverage = Coverage::default();
    reset_deferred_claim_paths();
    assert_equal_worlds(
        "overflow",
        || wide_rate_world(6, 2_500_000_000, deep_debt(), None),
        &[Step::Claim { staker: 0 }],
        &mut coverage,
    );
    assert_eq!(coverage.errors, 1, "the claim must fail");
    assert_eq!(deferred_claim_path(DeferredClaimPath::NotCovered), 1);
}

// =============================================================================
// The amount of work.
// =============================================================================

/// A fund with `credits` rows and a deferred credit of 1 TAO of alpha on each row.
/// `stakes` are the root stakers. With one staker the claimant sells each credit in full,
/// and each sale converts a state. With two stakers each sale is a part, and every state
/// stays for every later sale.
fn deferred_world(credits: usize, stakes: &[u64]) -> World {
    SubnetLimit::<Test>::put(2048);
    let fund = real_fund(&vec![5 * TAO; credits], stakes);
    for netuid in fund.netuids.iter() {
        credit(&fund.hotkey, *netuid, TAO);
        set_price_reference(*netuid, false);
    }
    let subnets = fund.netuids.clone();
    World { fund, subnets }
}

/// Storage steps of one claim on [`deferred_world`] that grow with the number of
/// credits, with the current code or with the code of runtime 494.
fn claim_steps(credits: usize, stakes: &[u64], reference: bool) -> u64 {
    let mut steps = 0;
    new_test_ext(1).execute_with(|| {
        let world = deferred_world(credits, stakes);
        let claimant = world.fund.stakers[0];
        let stake_before = root_stake_of(&world.fund.hotkey, &claimant);
        set_reference_deferred_claims(reference);
        reset_deferred_dividend_ops();
        let result = SubtensorModule::claim_root_with_hotkey(
            RuntimeOrigin::signed(claimant),
            world.fund.hotkey,
        );
        steps = deferred_dividend_ops();
        set_reference_deferred_claims(false);
        result.expect("the claim succeeds");
        // The claim sold the claimant's part of every credit: about 1 TAO of alpha each
        // for a sole staker, and the staker's share of it otherwise.
        let share = stakes[0] as f64 / stakes.iter().sum::<u64>() as f64;
        let paid = root_stake_of(&world.fund.hotkey, &claimant) - stake_before;
        assert!(
            paid as f64 > credits as f64 * TAO as f64 * share * 0.9,
            "{credits} credits: paid {paid}"
        );
    });
    steps
}

/// The storage steps of a claim grow with the number of deferred credits, not with its
/// square. With the code of runtime 494 the same claim needs more than `n * n` steps.
#[test]
fn storage_steps_of_a_claim_are_linear_in_the_deferred_credits() {
    for stakes in [&[1u64][..], &[1, 1][..]] {
        let mut previous = 0;
        for credits in [16usize, 32, 64, 128] {
            let n = credits as u64;
            let steps = claim_steps(credits, stakes, false);
            assert!(
                steps <= 8 * n + 16,
                "{credits} credits, {} stakers: {steps} storage steps",
                stakes.len()
            );
            if previous > 0 {
                assert!(
                    steps <= 2 * previous + 16,
                    "{credits} credits: {steps} steps, and {previous} for half as many"
                );
            }
            previous = steps;

            // The code of runtime 494: each sale walks every credit.
            let before = claim_steps(credits, stakes, true);
            assert!(
                before > n * n,
                "{credits} credits, {} stakers: runtime 494 needs {before} storage steps",
                stakes.len()
            );
        }
    }
}

/// The two codes give the same storage for the large claims of the test above.
#[test]
fn large_claims_of_deferred_credits_equal_the_code_of_runtime_494() {
    for stakes in [&[1u64][..], &[1, 1][..], &[3, 1, 2][..]] {
        let mut coverage = Coverage::default();
        let steps: Vec<Step> = (0..stakes.len())
            .map(|staker| Step::Claim { staker })
            .collect();
        assert_equal_worlds(
            &format!("64 credits, {} stakers", stakes.len()),
            || deferred_world(64, stakes),
            &steps,
            &mut coverage,
        );
        assert_eq!(coverage.errors, 0);
        assert_eq!(coverage.claims_with_three_sales, stakes.len() as u64);
    }
}

/// A snapshot of `n` queued credits counts the rate states of the validator once. With
/// the code of runtime 494 each new state counts all states before it.
#[test]
fn snapshot_counts_the_rate_states_once() {
    for (reference, limit) in [(false, 0u64), (true, 64 * 63 / 2)] {
        new_test_ext(1).execute_with(|| {
            SubnetLimit::<Test>::put(2048);
            let fund = real_fund(&vec![5 * TAO; 64], &[1]);
            for netuid in fund.netuids.iter() {
                SubnetAlphaOut::<Test>::mutate(*netuid, |out| {
                    *out = out.saturating_add(TAO.into())
                });
                SubtensorModule::enqueue_basket_deposit(
                    &fund.hotkey,
                    *netuid,
                    AlphaBalance::from(TAO),
                );
            }
            set_reference_deferred_claims(reference);
            reset_deferred_dividend_ops();
            SubtensorModule::snapshot_deferred_dividends(&fund.hotkey).expect("the credits defer");
            let steps = deferred_dividend_ops();
            set_reference_deferred_claims(false);
            assert_eq!(
                DeferredBasketRates::<Test>::iter_prefix(fund.hotkey).count(),
                64
            );
            // The validator has no state at the start, so the one count finds none.
            assert_eq!(steps, limit, "reference: {reference}");
        });
    }
}

/// A validator has at most 256 rate states. With 250 states and 10 queued credits on
/// other subnets, the snapshot makes six states and refuses the seventh. The count that
/// the snapshot keeps is correct after each state that it makes.
#[test]
fn snapshot_stops_at_the_limit_of_rate_states_like_runtime_494() {
    let run = |reference: bool| {
        let mut result = None;
        new_test_ext(1).execute_with(|| {
            SubnetLimit::<Test>::put(2048);
            let fund = real_fund(&[5 * TAO; 10], &[1]);
            for netuid in fund.netuids.iter() {
                SubnetAlphaOut::<Test>::mutate(*netuid, |out| {
                    *out = out.saturating_add(TAO.into())
                });
                SubtensorModule::enqueue_basket_deposit(&fund.hotkey, *netuid, TAO.into());
            }
            // 250 states of earlier credits, on netuids that the queue does not have.
            for index in 0..250u16 {
                DeferredBasketRates::<Test>::insert(
                    fund.hotkey,
                    NetUid::from(10_000 + index),
                    DeferredBasketState {
                        generation: u64::from(index) + 1,
                        rate: I64F64::from_num(1),
                        debt_count: 0,
                    },
                );
            }
            set_reference_deferred_claims(reference);
            let outcome = SubtensorModule::snapshot_deferred_dividends(&fund.hotkey);
            set_reference_deferred_claims(false);
            assert_eq!(
                outcome,
                Err(crate::Error::<Test>::BasketDepositPending.into())
            );
            assert_eq!(
                DeferredBasketRates::<Test>::iter_prefix(fund.hotkey).count(),
                256
            );
            result = Some(sp_io::storage::root(StateVersion::V1));
        });
        result.expect("the snapshot ran")
    };
    assert_eq!(run(false), run(true));
}
