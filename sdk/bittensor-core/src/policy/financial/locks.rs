//! Read-only projections. Runtime reads supply the authoritative state; these
//! routines share the SDK's existing float estimates and exact integer formulas.
use super::*;

fn big(value: &Value) -> Result<BigInt> {
    text(value)?
        .parse()
        .map_err(|_| range("expected an exact integer"))
}
fn decay(dt: f64, tau: f64) -> f64 {
    if dt <= 0.0 {
        1.0
    } else if tau <= 0.0 {
        0.0
    } else {
        (-f64::min(dt / tau, 40.0)).exp()
    }
}
fn project(
    mass: f64,
    conviction: f64,
    dt: f64,
    unlock: f64,
    maturity: f64,
    perpetual: bool,
    owner: bool,
) -> (f64, f64) {
    let u = decay(dt, unlock);
    let m = decay(dt, maturity);
    let next = if perpetual { mass } else { mass * u };
    let from_mass = if perpetual {
        mass * (1.0 - m)
    } else if unlock == maturity {
        if maturity != 0.0 {
            mass * (dt / maturity) * m
        } else {
            0.0
        }
    } else if unlock <= 0.0 || maturity <= 0.0 {
        0.0
    } else {
        mass * (unlock * (u - m) / (unlock - maturity)).max(0.0)
    };
    (
        next,
        if owner {
            next
        } else {
            conviction * m + from_mass
        },
    )
}
type Bucket = (f64, f64, bool, bool);
fn buckets(value: &Value) -> Result<Vec<Bucket>> {
    array(value)?
        .iter()
        .map(|value| {
            let row = array(value)?;
            if row.len() != 4 {
                return Err(ty(
                    "lock bucket requires mass, conviction, perpetual and owner",
                ));
            }
            Ok((
                float(&row[0])?,
                float(&row[1])?,
                truth(&row[2]),
                truth(&row[3]),
            ))
        })
        .collect()
}
fn conviction_at(buckets: &[Bucket], dt: f64, unlock: f64, maturity: f64) -> f64 {
    buckets
        .iter()
        .map(|(mass, conviction, perpetual, owner)| {
            project(*mass, *conviction, dt, unlock, maturity, *perpetual, *owner).1
        })
        .sum()
}
fn gate(conviction: f64, eligible: f64) -> bool {
    conviction * 100.0 > eligible * 18.0
}
fn blocks_until(buckets: &[Bucket], eligible: f64, unlock: f64, maturity: f64) -> Result<Value> {
    if eligible <= 0.0 {
        return Ok(Value::Null);
    }
    let clears = |dt| gate(conviction_at(buckets, dt, unlock, maturity), eligible);
    if clears(0.0) {
        return Ok(json!(0));
    }
    let horizon = (10.0 * unlock.max(maturity).max(1.0)).trunc();
    if horizon > 9_007_199_254_740_991.0 {
        return Err(range("lock projection horizon exceeds safe integer range"));
    }
    let horizon = horizon as u64;
    let step = (horizon / 4000).max(1);
    let mut previous = 0;
    let mut dt = step;
    while dt <= horizon {
        if clears(dt as f64) {
            let (mut low, mut high) = (previous, dt);
            while high.saturating_sub(low) > 1 {
                let mid = low.saturating_add(high.saturating_sub(low) / 2);
                if clears(mid as f64) {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            return Ok(json!(high));
        }
        previous = dt;
        dt = dt.saturating_add(step);
    }
    Ok(Value::Null)
}

pub(super) fn evaluate(operation: &str, input: &Value) -> Result<Value> {
    match operation {
        "financial.reads.lock_decay" => {
            Ok(json!(decay(float(&input["dt"])?, float(&input["rate"])?)))
        }
        "financial.reads.lock_project" => {
            let (mass, conviction) = project(
                float(&input["mass"])?,
                float(&input["conviction"])?,
                float(&input["dt"])?,
                float(&input["unlock"])?,
                float(&input["maturity"])?,
                truth(&input["perpetual"]),
                truth(&input["owner"]),
            );
            if !mass.is_finite() || !conviction.is_finite() {
                return Err(range("lock projection exceeds finite range"));
            }
            Ok(json!([mass, conviction]))
        }
        "financial.reads.lock_conviction" => Ok(json!(conviction_at(
            &buckets(&input["buckets"])?,
            float(&input["dt"])?,
            float(&input["unlock"])?,
            float(&input["maturity"])?
        ))),
        "financial.reads.lock_gate" => Ok(json!(gate(
            float(&input["conviction"])?,
            float(&input["eligible"])?
        ))),
        "financial.reads.lock_blocks" => blocks_until(
            &buckets(&input["buckets"])?,
            float(&input["eligible"])?,
            float(&input["unlock"])?,
            float(&input["maturity"])?,
        ),
        "financial.reads.eligible_alpha" => {
            let eligible =
                (big(&input["out"])? - big(&input["protocol"])? - big(&input["burned"])?)
                    .max(BigInt::zero());
            Ok(
                json!({"eligible":eligible.to_string(),"threshold":(&eligible * 18u32 / 100u32).to_string()}),
            )
        }
        "financial.reads.gamma_price" => {
            let accepted = big(&input["accepted"])?;
            let denominator = BigInt::from(1_000_000_000u64);
            // Round up the step once per feed, then grow linearly, never compound.
            let numerator = &accepted * 66_000u32;
            let step = if numerator >= BigInt::zero() {
                (&numerator + &denominator - 1u32) / &denominator
            } else {
                &numerator / &denominator
            };
            Ok(json!((accepted + step * big(&input["blocks"])?)
                .min(big(&input["feed"])?)
                .to_string()))
        }
        "financial.reads.collateral" => {
            let locked = big(&input["locked"])?;
            let floor = big(&input["floor"])?;
            let drain = big(&input["drain_bits"])?;
            let headroom = (&locked - &floor).max(BigInt::zero());
            let shortfall = (&floor - &locked).max(BigInt::zero());
            let work = if drain > BigInt::zero() {
                (&headroom << 64) / &drain
            } else {
                BigInt::zero()
            };
            let drain_ratio = drain
                .to_f64()
                .ok_or_else(|| range("collateral drain exceeds finite range"))?
                / 18446744073709551616.0;
            let legacy_work = if drain_ratio > 0.0 {
                (headroom
                    .to_f64()
                    .ok_or_else(|| range("collateral headroom exceeds finite range"))?
                    / drain_ratio)
                    .trunc()
            } else {
                0.0
            };
            if !legacy_work.is_finite() {
                return Err(range("collateral projection exceeds finite range"));
            }
            Ok(
                json!({"headroom":headroom.to_string(),"shortfall":shortfall.to_string(),"work":work.to_string(),"legacy_float_work":format!("{legacy_work:.0}"),"drain_ratio":drain_ratio}),
            )
        }
        _ => Err(invalid(format!(
            "unknown financial read policy {operation}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_math_keeps_strict_gate_and_linear_price_growth() -> Result<()> {
        assert!(!gate(1800.0, 10000.0));
        assert!(gate(1801.0, 10000.0));
        let boundary = vec![(1800.0, 1800.0, true, true)];
        assert_eq!(blocks_until(&boundary, 10000.0, 100.0, 100.0)?, Value::Null);
        assert_eq!(
            evaluate(
                "financial.reads.gamma_price",
                &json!({"accepted":"1000000000","feed":"2000000000","blocks":2})
            )?,
            "1000132000"
        );
        let collateral = evaluate(
            "financial.reads.collateral",
            &json!({"locked":"9007199254740993","floor":"9007199254740992","drain_bits":"9223372036854775808"}),
        )?;
        assert_eq!(collateral["headroom"], "1");
        assert_eq!(collateral["work"], "2");
        Ok(())
    }
}
