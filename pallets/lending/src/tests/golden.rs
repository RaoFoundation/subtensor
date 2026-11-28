//! Differential tests: replays the reference models' vectors through the pure core and requires
//! exact equality on every row. `golden/v21.json` (`model/gen_golden_v21.py`, from `model/v21`)
//! holds what v2.1 defines: the final rate (floor at apr, SPEC 2.1), the short coverage need on
//! `max(spot, SMA30_rank)` at the flat stress, and accrual at the final rate on the slow price.
//! `golden/math.json` (`model/gen_golden.py`, from `model/v201`) holds what v2.1 keeps: the pool
//! curve, the long coverage need and the accrual formula. Do not edit the JSON by hand;
//! regenerate it from the models.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use crate::math::{
    PriceRefs,
    coverage::{long_need, short_need},
    curve::{ConstantProduct as Cp, Curve, FEE_DEN, FEE_NUM, PoolState},
    fixed::{Price, Q64},
    interest::{ACCRUAL_DEN, BLOCKS_PER_DAY, BLOCKS_PER_YEAR, accrue},
    rate::{RateCurve, rate_bps},
};
use crate::params::{LongParams, ShortParams};
use crate::types::{Accrual, Leg, Position};
use serde_json::Value;

/// Row counts written by `gen_golden.py`; a silently shorter file must not pass.
const CURVE_QUOTE_ROWS: usize = 320;
const CURVE_PRICE_ROWS: usize = 200;
const LONG_COVERAGE_ROWS: usize = 180;
const INTEREST_ROWS: usize = 200;
/// Row counts written by `gen_golden_v21.py`.
const RATE_ROWS: usize = 3_264;
const RATE_EDGE_ROWS: usize = 1_857;
const SHORT_COVERAGE_ROWS: usize = 540;
const ACCRUAL_ROWS: usize = 400;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/math.json")).expect("golden JSON parses")
}

fn golden_v21() -> Value {
    serde_json::from_str(include_str!("golden/v21.json")).expect("golden JSON parses")
}

fn rows<'a>(doc: &'a Value, section: &str) -> &'a Vec<Value> {
    doc[section].as_array().expect("section is an array")
}

fn num(v: &Value) -> u128 {
    v.as_str()
        .expect("decimal string")
        .parse()
        .expect("unsigned integer")
}

fn amount(v: &Value) -> u64 {
    u64::try_from(num(v)).expect("fits u64")
}

fn small(v: &Value) -> u32 {
    u32::try_from(v.as_u64().expect("JSON number")).expect("fits u32")
}

fn opt_num(v: &Value) -> Option<u128> {
    (!v.is_null()).then(|| num(v))
}

fn price(v: &Value) -> Price {
    Price::from_bits(num(v))
}

fn pool(row: &Value) -> PoolState {
    PoolState::model(amount(&row["tao"]), amount(&row["alpha"]))
}

fn pool_pair(v: &Value) -> Option<PoolState> {
    Some(PoolState::model(amount(&v[0]), amount(&v[1])))
}

#[test]
fn header_matches_the_core_constants() {
    let doc = golden();
    assert_eq!(doc["source"], "model/v201");
    assert_eq!(num(&doc["q64"]), Q64);
    assert_eq!(u128::from(doc["fee"][0].as_u64().unwrap()), FEE_NUM);
    assert_eq!(u128::from(doc["fee"][1].as_u64().unwrap()), FEE_DEN);
}

#[test]
fn curve_rows_match_the_model() {
    let doc = golden();
    let (mut quotes, mut prices) = (0, 0);
    for (i, row) in rows(&doc, "curve").iter().enumerate() {
        let p = pool(row);
        if row.get("price").is_some() {
            let target = price(&row["price"]);
            assert_eq!(Cp::spot(&p), price(&row["spot"]), "spot, row {i}");
            assert_eq!(
                Cp::pumped(&p, target),
                pool_pair(&row["pumped"]),
                "pumped, row {i}"
            );
            assert_eq!(
                Cp::dumped(&p, target),
                pool_pair(&row["dumped"]),
                "dumped, row {i}"
            );
            prices += 1;
        } else {
            let (a, tin) = (amount(&row["a"]), amount(&row["tin"]));
            assert_eq!(
                Cp::out_alpha_for_tao(&p, tin),
                amount(&row["out_alpha_for_tao"]),
                "out_alpha_for_tao, row {i}"
            );
            assert_eq!(
                Cp::out_tao_for_alpha(&p, a),
                amount(&row["out_tao_for_alpha"]),
                "out_tao_for_alpha, row {i}"
            );
            assert_eq!(
                Cp::tao_in_for_alpha(&p, a),
                opt_num(&row["tao_in_for_alpha"]),
                "tao_in_for_alpha, row {i}"
            );
            assert_eq!(
                Cp::buy_cost_nofee(&p, a),
                opt_num(&row["buy_cost_nofee"]),
                "buy_cost_nofee, row {i}"
            );
            assert_eq!(
                Cp::sell_proceeds_nofee(&p, a),
                amount(&row["sell_proceeds_nofee"]),
                "sell_proceeds_nofee, row {i}"
            );
            quotes += 1;
        }
    }
    assert_eq!((quotes, prices), (CURVE_QUOTE_ROWS, CURVE_PRICE_ROWS));
}

#[test]
fn long_coverage_rows_match_the_model() {
    let doc = golden();
    let lp = LongParams::default();
    let all = rows(&doc, "long_coverage");
    for (i, row) in all.iter().enumerate() {
        let p = pool(row);
        let refs = PriceRefs {
            spot: price(&row["spot"]),
            twap: price(&row["twap"]),
            sma: price(&row["sma"]),
        };
        assert_eq!(Cp::spot(&p), refs.spot, "spot, row {i}");
        let long = &row["long"];
        let (a, book, debt) = (
            amount(&long["a"]),
            amount(&long["book"]),
            amount(&long["debt"]),
        );
        assert_eq!(
            long_need::<Cp>(&p, &refs, debt, a, book, lp.stress_bps),
            amount(&long["need"]),
            "long need, row {i}"
        );
    }
    assert_eq!(all.len(), LONG_COVERAGE_ROWS);
}

#[test]
fn interest_rows_match_the_model() {
    let doc = golden();
    let all = rows(&doc, "interest");
    for (i, row) in all.iter().enumerate() {
        let (notional, apr) = (amount(&row["notional"]), small(&row["apr_bps"]));
        let accrued = accrue(
            notional,
            apr,
            row["blocks"].as_u64().unwrap(),
            num(&row["rem_in"]),
        );
        assert_eq!(accrued.owed, amount(&row["owed"]), "owed, row {i}");
        assert_eq!(
            accrued.remainder,
            num(&row["rem_out"]),
            "remainder, row {i}"
        );
        assert_eq!(
            accrue(notional, apr, u64::from(BLOCKS_PER_DAY), 0).owed,
            amount(&row["day"]),
            "day interest, row {i}"
        );
    }
    assert_eq!(all.len(), INTEREST_ROWS);
}

// v2.1 vectors.

fn curve_of(row: &Value) -> RateCurve {
    RateCurve {
        apr_bps: small(&row["apr"]),
        k: u8::try_from(small(&row["k"])).expect("fits u8"),
        cap_bps: small(&row["cap"]),
    }
}

/// The final rate of a row (SPEC 2.1, floor at apr). A floor-on row is the final rate. A
/// floor-off row is the pure curve, which the final rate reads as `max(curve, apr)`.
fn final_rate(row: &Value) -> u32 {
    let model = small(&row["rate"]);
    if row["floor"].as_u64() == Some(1) {
        model
    } else {
        model.max(small(&row["apr"]))
    }
}

#[test]
fn v21_header_matches_the_core_constants() {
    let doc = golden_v21();
    let sp = ShortParams::default();
    assert_eq!(doc["source"], "model/v21");
    assert_eq!(num(&doc["q64"]), Q64);
    assert_eq!(
        u128::from(doc["blocks_per_year"].as_u64().unwrap()),
        BLOCKS_PER_YEAR
    );
    assert_eq!(small(&doc["stress_bps"]), sp.stress_bps);
    // v2.1 has one flat stress: the slope is gone from the model and from the vectors.
    assert!(doc.get("stress_slope_bps").is_none());
}

#[test]
fn rate_rows_match_the_model() {
    let doc = golden_v21();
    let all = rows(&doc, "rate");
    let (mut capped, mut covered, mut exponents) = (0, 0, [0usize; 9]);
    for (i, row) in all.iter().enumerate() {
        let curve = curve_of(row);
        let (n, d) = (num(&row["num"]), num(&row["den"]));
        let expected = final_rate(row);
        assert_eq!(
            rate_bps(curve.apr_bps, curve.k, curve.cap_bps, n, d),
            expected,
            "rate, row {i}"
        );
        assert_eq!(curve.rate(n, d), expected, "curve rate, row {i}");
        assert!(
            (curve.apr_bps..=curve.cap_bps).contains(&expected),
            "rate outside apr..=cap, row {i}"
        );
        if n <= d {
            // A covered position, and a position with no debt, pays the base APR.
            assert_eq!(expected, curve.apr_bps, "covered, row {i}");
            covered += 1;
        }
        capped += usize::from(expected == curve.cap_bps);
        exponents[usize::from(curve.k)] += 1;
    }
    assert_eq!(all.len(), RATE_ROWS);
    // The file exercises the cap, the covered region and every exponent 1..=8 equally.
    assert!(capped > 500 && covered > 1_000);
    assert_eq!(exponents[0], 0);
    assert!(exponents[1..].iter().all(|rows| *rows == RATE_ROWS / 8));
}

#[test]
fn rate_edge_rows_match_the_model() {
    let doc = golden_v21();
    let all = rows(&doc, "rate_edge");
    let mut at_cap = 0;
    for (i, row) in all.iter().enumerate() {
        let curve = curve_of(row);
        let (n, d) = (num(&row["num"]), num(&row["den"]));
        let expected = final_rate(row);
        assert_eq!(
            rate_bps(curve.apr_bps, curve.k, curve.cap_bps, n, d),
            expected,
            "edge rate, row {i}"
        );
        assert_eq!(curve.rate(n, d), expected, "edge curve rate, row {i}");
        at_cap += usize::from(expected == curve.cap_bps);
    }
    assert_eq!(all.len(), RATE_EDGE_ROWS);
    assert!(at_cap > 900);
}

#[test]
fn short_coverage_rows_match_the_model() {
    let doc = golden_v21();
    let sp = ShortParams::default();
    let all = rows(&doc, "coverage");
    let mut infinite = 0;
    for (i, row) in all.iter().enumerate() {
        let p = pool(row);
        let refs = PriceRefs {
            spot: price(&row["spot"]),
            twap: price(&row["twap"]),
            sma: price(&row["sma"]),
        };
        assert_eq!(Cp::spot(&p), refs.spot, "spot, row {i}");
        let (q, book) = (amount(&row["q"]), amount(&row["book"]));
        let need = short_need::<Cp>(&p, &refs, q, book, sp.stress_bps);
        assert_eq!(need.map(u128::from), opt_num(&row["need"]), "need, row {i}");
        infinite += usize::from(need.is_none());
    }
    assert_eq!(all.len(), SHORT_COVERAGE_ROWS);
    assert!(infinite > 0 && infinite < all.len());
    // Each pool, book and SMA comes with three TWAP medians (above, between and below spot
    // and SMA) and one need: the TWAP median is not a short reference, and the stress is flat.
    for group in all.chunks(3) {
        let twaps: Vec<_> = group.iter().map(|row| num(&row["twap"])).collect();
        let bounds = (num(&group[0]["spot"]), num(&group[0]["sma"]));
        assert!(twaps[0] > bounds.0.max(bounds.1) && twaps[2] < bounds.0.min(bounds.1));
        assert!(group.iter().all(|row| row["need"] == group[0]["need"]));
        assert!(group.iter().all(|row| row["sma"] == group[0]["sma"]));
        assert!(group.iter().all(|row| row["spot"] == group[0]["spot"]));
    }
}

#[test]
fn accrual_rows_match_the_model() {
    let doc = golden_v21();
    let all = rows(&doc, "accrual");
    let (mut shorts, mut longs, mut capped, mut at_apr) = (0, 0, 0, 0);
    for (i, row) in all.iter().enumerate() {
        let curve = curve_of(row);
        let twap = price(&row["twap"]);
        // The rate and the notional read only the TWAP median and SMA30 (SPEC 1 item 2), so the
        // spot is not part of the row and does not enter the references.
        let refs = PriceRefs {
            spot: twap,
            twap,
            sma: price(&row["sma"]),
        };
        let collateral = amount(&row["collateral"]);
        let leg = if row["side"] == "short" {
            shorts += 1;
            Leg::Short {
                minted: amount(&row["q"]),
                proceeds: amount(&row["proceeds"]),
            }
        } else {
            longs += 1;
            Leg::Long {
                locked_alpha: amount(&row["locked_alpha"]),
                debt: amount(&row["debt"]),
            }
        };
        let blocks = row["blocks"].as_u64().unwrap();
        let pos = Position {
            collateral,
            deposit: 0,
            opened_at: 0u64,
            charge_at: 360,
            accrual: Accrual {
                last_accrued: 0,
                remainder: num(&row["rem_in"]),
                due: amount(&row["due_in"]),
            },
            leg,
        };
        let rate = small(&row["rate"]);
        assert_eq!(
            leg.debt_over_holdings(collateral, &refs),
            (num(&row["num"]), num(&row["den"])),
            "debt over holdings, row {i}"
        );
        assert_eq!(
            leg.notional(refs.twap),
            amount(&row["notional"]),
            "notional, row {i}"
        );
        assert_eq!(pos.rate_bps(&refs, &curve), rate, "rate, row {i}");

        // The pure accrual at the row's rate.
        let accrued = accrue(leg.notional(refs.twap), rate, blocks, num(&row["rem_in"]));
        let due_out = amount(&row["due_out"]);
        assert_eq!(
            amount(&row["due_in"]) + accrued.owed,
            due_out,
            "due, row {i}"
        );
        assert_eq!(
            accrued.remainder,
            num(&row["rem_out"]),
            "remainder, row {i}"
        );
        assert!(accrued.remainder < ACCRUAL_DEN);

        // The position's settlement: the same rate, and the due split into what the collateral
        // pays now and what stays due.
        let (settled, charge) = pos.settle(&refs, &curve, blocks);
        assert_eq!(charge.rate_bps, rate, "settled rate, row {i}");
        assert_eq!(charge.amount, due_out.min(collateral), "charge, row {i}");
        assert_eq!(
            settled.accrual.due,
            due_out - charge.amount,
            "due left, row {i}"
        );
        assert_eq!(settled.collateral, collateral - charge.amount);
        assert_eq!(settled.accrual.remainder, num(&row["rem_out"]));
        assert_eq!(settled.accrual.last_accrued, blocks);
        capped += usize::from(rate == curve.cap_bps);
        at_apr += usize::from(rate == curve.apr_bps);
    }
    assert_eq!(all.len(), ACCRUAL_ROWS);
    assert_eq!((shorts, longs), (ACCRUAL_ROWS / 2, ACCRUAL_ROWS / 2));
    assert!(capped > 20 && at_apr > 20);
}
