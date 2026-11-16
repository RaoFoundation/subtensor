//! Deterministic financial policy shared by every SDK binding.
//!
//! The boundary accepts decimal strings for integers and returns them for all
//! monetary/share values. Chain reads, signing and application consent stay in
//! the language adapters; this module owns their decisions and calculations.
// Exact products use BigInt/U256; variable-length totals and unbounded i128
// intermediates are checked. Monetary Gamma values are u64, emission fixed
// vectors have five slots, and weight inputs are bounded safe counts. The
// remaining primitive arithmetic is bounded by those domains or constant divisors.
#![allow(clippy::arithmetic_side_effects)]
use super::PolicyError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

mod basket;
mod fees;
mod gamma;
mod locks;
mod orders;
mod registration;
mod shield;
mod staking;

type Result<T> = std::result::Result<T, PolicyError>;
const ONE: i128 = u64::MAX as i128;
const DEFAULT_OWNER: i128 = 11796 * ONE / 65535;
const DEFAULT_VALIDATOR: i128 = (ONE - DEFAULT_OWNER) / 2;
const SLOTS: [&str; 5] = ["burn", "recycle", "owner", "validator", "miner"];
fn invalid(message: impl Into<String>) -> PolicyError {
    PolicyError::invalid(message.into())
}
fn range(message: impl Into<String>) -> PolicyError {
    PolicyError::range_error(message.into())
}
fn ty(message: impl Into<String>) -> PolicyError {
    PolicyError::type_error(message.into())
}
fn object(value: &Value) -> Result<&Map<String, Value>> {
    value.as_object().ok_or_else(|| ty("expected an object"))
}
fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().ok_or_else(|| ty("expected an array"))
}
fn text(value: &Value) -> Result<String> {
    match value {
        Value::String(v) => Ok(v.clone()),
        Value::Number(v) => Ok(v.to_string()),
        _ => Err(ty("expected a decimal string or number")),
    }
}
fn integer(value: &Value) -> Result<i128> {
    let result = text(value)?
        .parse::<i128>()
        .map_err(|_| range("expected an exact integer"))?;
    if value.is_number() && !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&result) {
        return Err(range(
            "expected an exact integer: values above the safe integer range require decimal strings",
        ));
    }
    Ok(result)
}
fn add(total: &mut i128, value: i128) -> Result<()> {
    *total = total
        .checked_add(value)
        .ok_or_else(|| range("financial total overflow"))?;
    Ok(())
}
fn sum(values: impl IntoIterator<Item = i128>) -> Result<i128> {
    values.into_iter().try_fold(0i128, |total, value| {
        total
            .checked_add(value)
            .ok_or_else(|| range("financial total overflow"))
    })
}
fn optional_integer(value: &Value, default: i128) -> Result<i128> {
    if value.is_null() {
        Ok(default)
    } else {
        integer(value)
    }
}
fn u64_value(value: &Value) -> Result<u64> {
    u64::try_from(integer(value)?).map_err(|_| range("value exceeds u64"))
}
fn count(value: &Value) -> Result<i128> {
    let n = integer(value)?;
    if !(0..=9_007_199_254_740_991).contains(&n) {
        return Err(range("count must be a nonnegative safe integer"));
    }
    Ok(n)
}
fn encoded(n: i128) -> Value {
    Value::String(n.to_string())
}
fn encoded_values(values: impl IntoIterator<Item = i128>) -> Value {
    Value::Array(values.into_iter().map(encoded).collect())
}
fn float(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .filter(|v| v.is_finite())
        .ok_or_else(|| range("expected a finite number"))
}
fn truth(value: &Value) -> bool {
    value.as_bool().unwrap_or(false)
}

/// Parse decimal text without passing amounts through floating-point arithmetic.
fn decimal_ratio(value: &Value, exponent_limit: i32) -> Result<BigRational> {
    let value = super::numeric::decimal(&text(value)?, exponent_limit)?;
    if value < BigRational::zero() {
        return Err(range("value must be a nonnegative finite decimal"));
    }
    Ok(value)
}
fn decimal_scaled(value: &Value, scale: u64, exact: bool, exponent_limit: i32) -> Result<i128> {
    let scaled = decimal_ratio(value, exponent_limit)? * BigInt::from(scale);
    if exact && !scaled.is_integer() {
        return Err(range("amount must have at most 9 decimals"));
    }
    scaled
        .to_integer()
        .to_i128()
        .ok_or_else(|| range("decimal is out of range"))
}

fn slot(value: &str) -> Result<usize> {
    let key = value.trim().to_lowercase();
    let result = key
        .parse::<usize>()
        .ok()
        .or_else(|| SLOTS.iter().position(|name| *name == key));
    result.filter(|i| *i < 5).ok_or_else(|| {
        range(format!(
            "unknown emission slot {value}; slots are burn, recycle, owner, validator, miner"
        ))
    })
}
fn weights(value: &Value) -> Result<Vec<i128>> {
    let mut out = array(value)?
        .iter()
        .map(integer)
        .collect::<Result<Vec<_>>>()?;
    if out.len() > 5 {
        return Err(range("at most five emission slots are supported"));
    }
    out.resize(5, 0);
    Ok(out)
}
fn share(value: &Value, fraction_number: bool) -> Result<i128> {
    if value.is_number() && !fraction_number {
        integer(value)?;
    }
    let raw = text(value)?.trim().replace('_', "");
    let percent = raw.ends_with('%');
    let raw = raw.strip_suffix('%').unwrap_or(&raw);
    let raw_integer =
        !percent && !fraction_number && raw.bytes().all(|c| c.is_ascii_digit()) && !raw.is_empty();
    let result = if raw_integer {
        integer(&json!(raw))?
    } else {
        let fraction = if percent {
            format!("{raw}e-2")
        } else {
            raw.to_owned()
        };
        // Percent values with an existing exponent must adjust it, not append one.
        let fraction = if percent && raw.contains(['e', 'E']) {
            let parts = raw.split(['e', 'E']).collect::<Vec<_>>();
            if parts.len() != 2 {
                return Err(range("invalid emission share"));
            }
            format!(
                "{}e{}",
                parts[0],
                parts[1]
                    .parse::<i32>()
                    .ok()
                    .and_then(|value| value.checked_sub(2))
                    .ok_or_else(|| range("invalid emission share exponent"))?
            )
        } else {
            fraction
        };
        // Compare before scaling so a fraction infinitesimally above one cannot
        // floor back into the valid u64 range.
        let value = decimal_scaled(&json!(fraction), u64::MAX, false, 1002)?;
        let above_one =
            decimal_ratio(&json!(fraction), 1002)? > BigRational::from_integer(BigInt::from(1u8));
        if above_one || value > ONE {
            return Err(range("emission fraction must be between zero and one"));
        }
        value
    };
    if !(0..=ONE).contains(&result) {
        return Err(range("emission share outside u64 range"));
    }
    Ok(result)
}
fn gamma_shares(value: &Value) -> Result<BTreeMap<u16, i128>> {
    let mut out = BTreeMap::new();
    if value.is_null() {
        return Ok(out);
    }
    for row in array(value)? {
        let pair = array(row)?;
        if pair.len() != 2 {
            return Err(ty("invalid Gamma emission share entry"));
        }
        let id =
            u16::try_from(integer(&pair[0])?).map_err(|_| range("Gamma target is not a netuid"))?;
        if out.insert(id, i128::from(u64_value(&pair[1])?)).is_some() {
            return Err(range("duplicate Gamma emission target"));
        }
    }
    Ok(out)
}
fn encoded_map(values: BTreeMap<u16, i128>) -> Value {
    Value::Object(
        values
            .into_iter()
            .map(|(key, value)| (key.to_string(), encoded(value)))
            .collect(),
    )
}
fn rounding(mut weights: Vec<i128>, gamma: i128) -> Result<Vec<i128>> {
    let gap = ONE
        .checked_sub(gamma)
        .and_then(|value| value.checked_sub(sum(weights.iter().copied()).ok()?))
        .ok_or_else(|| range("emission share total overflow"))?;
    if (-1000..=1000).contains(&gap) && weights[4].checked_add(gap).is_some_and(|value| value >= 0)
    {
        weights[4] = weights[4]
            .checked_add(gap)
            .ok_or_else(|| range("emission share total overflow"))?;
    }
    Ok(weights)
}

fn violation(out: &mut Vec<Value>, fails: bool, error: &str, message: &str, target: Option<u16>) {
    if fails {
        let mut item = json!({"error":error,"message":message});
        if let Some(target) = target {
            item["target"] = json!(target);
        }
        out.push(item);
    }
}
fn emission(op: &str, input: &Value) -> Result<Value> {
    Ok(match op {
        "constants" => {
            json!({"ONE":ONE.to_string(),"DEFAULT_OWNER":DEFAULT_OWNER.to_string(),"DEFAULT_VALIDATOR":DEFAULT_VALIDATOR.to_string(),"MAX_OWNER":(ONE*33/100).to_string(),"MAX_BURN_RECYCLE":(ONE*33/100).to_string(),"MAX_VALIDATOR":(ONE/2).to_string(),"MIN_MINER":(ONE*33/100).to_string(),"MAX_GAMMA":(ONE/10).to_string(),"MAX_GAMMA_SHARES":16,"MAX_ROUNDING_PARTS":"1000","OWNER_RATE_LIMIT_BLOCKS":50400,"SLOT_NAMES":SLOTS})
        }
        "limits" => {
            let rules = &input["rules"];
            let owner = if truth(&rules["created_via_lease"]) {
                optional_integer(&rules["min_owner"], 0)?
            } else {
                0
            };
            let validator = if rules["min_validator"].is_null() {
                None
            } else {
                Some(integer(&rules["min_validator"])?)
            };
            let pairs = [
                ("burn", Some(0), ONE * 33 / 100),
                ("recycle", Some(0), ONE * 33 / 100),
                ("owner", Some(owner), ONE * 33 / 100),
                ("validator", validator, ONE / 2),
                ("miner", Some(ONE * 33 / 100), ONE),
                ("gamma", Some(0), ONE / 10),
            ];
            Value::Object(pairs.into_iter().map(|(name,min,max)|(name.to_string(),json!({"min_parts":min.map(|v|v.to_string()),"max_parts":max.to_string()}))).collect())
        }
        "slot" => json!(slot(&text(&input["slot"])?)?),
        "parse_share" => encoded(
            share(&input["value"], truth(&input["fraction_number"]))
                .map_err(|error| range(error.message))?,
        ),
        "fraction" => {
            let denominator = integer(&input["denominator"])?;
            if denominator == 0 {
                return Err(range("division by zero"));
            }
            let result = BigInt::from(ONE) * BigInt::from(integer(&input["numerator"])?)
                / BigInt::from(denominator);
            encoded(result.to_i128().ok_or_else(|| range("share overflow"))?)
        }
        "float" => json!(integer(&input["parts"])? as f64 / ONE as f64),
        "normalize" => encoded_values(weights(&input["weights"])?),
        "default" => {
            let owner = optional_integer(&input["owner"], DEFAULT_OWNER)?;
            if !(0..=ONE).contains(&owner) {
                return Err(range("owner share outside u64 range"));
            }
            let validator = (ONE - owner) / 2;
            encoded_values([0, 0, owner, validator, ONE - owner - validator])
        }
        "merge" => {
            let mut out = weights(&input["weights"])?;
            for (name, value) in object(&input["updates"])? {
                out[slot(name)?] = integer(value)?;
            }
            encoded_values(out)
        }
        "round" => encoded_values(rounding(
            weights(&input["weights"])?,
            optional_integer(&input["gamma"], 0)?,
        )?),
        "gamma_shares" => encoded_map(gamma_shares(&input["rows"])?),
        "gamma_target" => json!(u16::try_from(integer(&input["value"])?)
            .map_err(|_| range("Gamma target is not a netuid"))?),
        "gamma_updates" => {
            let mut out = BTreeMap::new();
            for (target, value) in object(&input["updates"])? {
                let id = target
                    .parse::<u16>()
                    .map_err(|_| range("Gamma target is not a netuid"))?;
                if out.insert(id, share(value, false)?).is_some() {
                    return Err(range("each Gamma target may appear only once"));
                }
            }
            encoded_map(out)
        }
        "merge_gamma" => {
            let mut out = BTreeMap::new();
            for value in [&input["current"], &input["updates"]] {
                for (key, value) in object(value)? {
                    out.insert(
                        key.parse::<u16>()
                            .map_err(|_| range("Gamma target is not a netuid"))?,
                        integer(value)?,
                    );
                }
            }
            out.retain(|_, value| *value != 0);
            encoded_map(out)
        }
        "gamma_total" => {
            let sum = sum(gamma_shares(&input["rows"])?.values().copied())?;
            if sum > ONE {
                return Err(range("Gamma emission shares exceed 100%"));
            }
            encoded(sum)
        }
        "renormalize" => {
            let mut out = weights(&input["weights"])?;
            if out.iter().any(|value| *value < 0) {
                return Err(range("emission weights must be nonnegative"));
            }
            let gamma = optional_integer(&input["gamma"], 0)?;
            if !(0..=ONE).contains(&gamma) {
                return Err(range("Gamma emission share outside u64 range"));
            }
            let mut fixed = BTreeMap::new();
            for (key, value) in object(&input["fixed"])? {
                let value = integer(value)?;
                if !(0..=ONE).contains(&value) {
                    return Err(range("invalid pinned share"));
                }
                fixed.insert(slot(key)?, value);
            }
            let remaining = ONE - gamma - fixed.values().sum::<i128>();
            if remaining < 0 {
                return Err(range("pinned shares exceed 100%"));
            }
            let free = (0..5)
                .filter(|i| !fixed.contains_key(i))
                .collect::<Vec<_>>();
            for (index, value) in fixed {
                out[index] = value;
            }
            if free.is_empty() {
                out = rounding(out, gamma)?;
                if sum(out.iter().copied().chain([gamma]))? != ONE {
                    return Err(range("pinned shares sum is not 100%"));
                }
            } else {
                let total = sum(free.iter().map(|i| out[*i]))?;
                let mut assigned = 0;
                for (offset, index) in free.iter().enumerate() {
                    let value = if offset + 1 == free.len() {
                        remaining - assigned
                    } else if total == 0 {
                        remaining / free.len() as i128
                    } else {
                        let product = sp_core::U256::from(remaining as u128)
                            * sp_core::U256::from(out[*index] as u128);
                        (product / sp_core::U256::from(total as u128)).low_u128() as i128
                    };
                    out[*index] = value;
                    assigned += value;
                }
            }
            encoded_values(out)
        }
        "validate" => {
            let w = weights(&input["weights"])?;
            let r = &input["rules"];
            let gamma = optional_integer(&r["gamma_share"], 0)?;
            let mut out = Vec::new();
            violation(
                &mut out,
                w.iter().any(|v| !(0..=ONE).contains(v)),
                "InvalidEmissionWeight",
                "weights must be u64 parts",
                None,
            );
            violation(
                &mut out,
                sum([w[0], w[1]])? > ONE * 33 / 100,
                "BurnRecycleEmissionAboveMaximum",
                "burn and recycle together exceed 33%",
                None,
            );
            violation(
                &mut out,
                w[2] > ONE * 33 / 100,
                "OwnerEmissionAboveMaximum",
                "owner share exceeds 33%",
                None,
            );
            violation(
                &mut out,
                truth(&r["created_via_lease"]) && w[2] < optional_integer(&r["min_owner"], 0)?,
                "LeasedSubnetOwnerCutBelowMinimum",
                "owner share is below the lease floor",
                None,
            );
            violation(
                &mut out,
                w[3] > ONE / 2,
                "ValidatorEmissionAboveMaximum",
                "validator share exceeds 50%",
                None,
            );
            violation(
                &mut out,
                !r["min_validator"].is_null() && w[3] < optional_integer(&r["min_validator"], 0)?,
                "ValidatorEmissionTooLowToPreserveRoot",
                "validator share must exceed the root baseline",
                None,
            );
            violation(
                &mut out,
                w[4] < ONE * 33 / 100,
                "MinerEmissionBelowMinimum",
                "miner share is below 33%",
                None,
            );
            violation(
                &mut out,
                !(0..=ONE).contains(&gamma),
                "InvalidEmissionWeight",
                "Gamma shares must be u64 parts",
                None,
            );
            violation(
                &mut out,
                sum(w.iter().copied().chain([gamma]))? != ONE,
                "EmissionWeightsDoNotSumToOne",
                "fixed slots and Gamma shares must sum exactly to u64::MAX",
                None,
            );
            json!(out)
        }
        "validate_gamma" => {
            let shares = object(&input["shares"])?;
            let r = &input["rules"];
            let mut out = Vec::new();
            violation(
                &mut out,
                shares.len() > 16,
                "TooManyGammaShares",
                "at most 16 Gamma shares are allowed",
                None,
            );
            violation(
                &mut out,
                !shares.is_empty() && truth(&r["leased"]),
                "GammaEmissionNotSettableOnLeasedSubnet",
                "Gamma emission is not settable on a leased subnet",
                None,
            );
            for (key, value) in shares {
                let id = key
                    .parse::<u16>()
                    .map_err(|_| range("Gamma target is not a netuid"))?;
                let share = integer(value)?;
                violation(
                    &mut out,
                    i128::from(id) == integer(&input["netuid"])?,
                    "InvalidValue",
                    "Gamma emission cannot target its own subnet",
                    Some(id),
                );
                violation(
                    &mut out,
                    !(0..=ONE).contains(&share),
                    "InvalidEmissionWeight",
                    "Gamma shares must be u64 parts",
                    Some(id),
                );
                violation(
                    &mut out,
                    share > ONE / 10,
                    "GammaEmissionAboveMaximum",
                    &format!("Gamma share for subnet {id} exceeds 10%"),
                    Some(id),
                );
                if let Some(targets) = r["gamma_targets"].as_object() {
                    if !targets.contains_key(key) {
                        violation(
                            &mut out,
                            true,
                            "GammaEmissionTargetNotWhitelisted",
                            &format!("subnet {id} is not whitelisted for Gamma emission"),
                            Some(id),
                        );
                    } else {
                        violation(
                            &mut out,
                            !truth(&targets[key]),
                            "GammaDisabled",
                            &format!("Gamma is disabled on subnet {id}"),
                            Some(id),
                        );
                    }
                }
            }
            json!(out)
        }
        "min_validator" => encoded(integer(&input["baseline"])?.saturating_add(1).min(ONE)),
        "root_paid" => json!(
            integer(&input["validator"])?.min(integer(&input["baseline"])?) as f64 / ONE as f64
        ),
        "root_share" => {
            let weighted = float(&input["root_tao"])? * float(&input["tao_weight"])?;
            let denominator = weighted + float(&input["alpha_issuance"])?;
            json!(if denominator > 0.0 {
                float(&input["validator"])? * weighted / denominator
            } else {
                0.0
            })
        }
        "rate_limit" => {
            let last = integer(&input["last"])?;
            if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&last) {
                return Err(range("last block must be a safe integer"));
            }
            let current = count(&input["current"])?;
            let limit = if input["limit"].is_null() {
                50400
            } else {
                count(&input["limit"])?
            };
            let next = if last <= 0 { current } else { last + limit };
            json!({"limit_blocks":limit as i64,"last_change_block":last as i64,"next_allowed_block":next as i64,"blocks_until_allowed":(next-current).max(0) as i64,"owner_can_change_now":next<=current})
        }
        "preview" => {
            let before = weights(&input["before"])?;
            let after = weights(&input["after"])?;
            let baseline = integer(&input["baseline_parts"])?;
            let rejected = after[3] <= baseline;
            let shares = |weights: &Vec<i128>| -> Value {
                Value::Object(
                    SLOTS
                        .iter()
                        .zip(weights)
                        .map(|(name, value)| (name.to_string(), json!(*value as f64 / ONE as f64)))
                        .collect(),
                )
            };
            json!({"before":{"weights":encoded_values(before.clone()),"shares":shares(&before)},"after":{"weights":encoded_values(after.clone()),"shares":shares(&after)},"changed":SLOTS.iter().enumerate().filter(|(i,_)|before[*i]!=after[*i]).map(|(_,name)|*name).collect::<Vec<_>>(),"root":{"share_before":before[3].min(baseline) as f64/ONE as f64,"share_after":if rejected{None}else{Some(after[3].min(baseline) as f64/ONE as f64)},"baseline_share":baseline as f64/ONE as f64,"validators_before":(BigInt::from(before[3])-BigInt::from(before[3].min(baseline))).to_f64().ok_or_else(|| range("emission preview overflow"))?/ONE as f64,"validators_after":if rejected{None}else{Some((BigInt::from(after[3])-BigInt::from(baseline)).to_f64().ok_or_else(|| range("emission preview overflow"))?/ONE as f64)},"capped_before":baseline>before[3],"capped_after":false,"validator_floor":baseline as f64/ONE as f64,"global_tao_weight":input["global_tao_weight"],"root_tao":integer(&input["root_tao"])?.to_string(),"alpha_issuance":integer(&input["alpha_issuance"])?.to_string()}})
        }
        _ => {
            return Err(invalid(format!(
                "unknown financial emission operation {op}"
            )))
        }
    })
}

fn claim(units: i128) -> i128 {
    567_441_000
        + 437_318_379 * units
        + (23 + 22 * units) * 25_000_000
        + (18 + 16 * units) * 100_000_000
}
fn scan(units: i128) -> i128 {
    309_208_000 + 263_604_241 * units + (6 + 21 * units) * 25_000_000
}
fn fee(weight: i128) -> i128 {
    weight / 4000 + i128::from(weight % 4000 * 2 > 4000)
}
fn claim_weight(units: i128, quotes: i128, rows: i128) -> i128 {
    claim(units + rows)
        + scan(units)
        + if quotes > 0 {
            110_000_000 * quotes + 175_000_000 * quotes.min(256)
        } else {
            0
        }
}
fn charged(limit: i128) -> i128 {
    let n = limit.min(4);
    (claim(2 * n) + scan(n) + 110_000_000 * (6 * n).max(1)).min(249_856_000_000)
}
fn spent(
    reserved: i128,
    hotkeys: i128,
    redeem: i128,
    scanned: i128,
    selection: i128,
    limit: i128,
) -> Result<i128> {
    if [hotkeys, redeem, scanned, selection, limit]
        .iter()
        .any(|value| !(0..=9_007_199_254_740_991).contains(value))
    {
        return Err(range("claim work must be a nonnegative safe count"));
    }
    if reserved < 0 {
        return Err(range("reserved fee must be nonnegative"));
    }
    let base = (reserved - fee(charged(limit))).max(0);
    let work = fee(claim(hotkeys.max(redeem).max(selection).max(1)) + scan(scanned));
    Ok(if work >= reserved - base {
        reserved
    } else {
        base + work
    })
}
fn root_claim(op: &str, input: &Value) -> Result<Value> {
    let limit = if input["limit"].is_null() {
        256
    } else {
        count(&input["limit"])?
    };
    Ok(match op {
        "declared_work" => json!(if truth(&input["single"]) { 130 } else { 256 }),
        "fee_units" => encoded(limit.min(4)),
        "discount_weight" => encoded((claim_weight(limit, 2560, 512) - charged(limit)).max(0)),
        "flush_weight" => {
            let quotes = count(&input["quotes"])?;
            let rows = count(&input["rows"])?;
            encoded(
                if quotes > 0 {
                    quotes * 110_000_000 + 175_000_000 * (quotes.min(256) + rows)
                } else {
                    0
                } + if rows > 0 { claim(rows) } else { 0 },
            )
        }
        "admission" => {
            let work = count(&input["hotkeys"])? + count(&input["holdings"])?;
            let scans = count(&input["selection"])?;
            let queued = if input["queued"].is_null() {
                0
            } else {
                count(&input["queued"])?
            };
            json!({"too_heavy":work>limit||scans>limit,"work":work,"work_exceeded":work>limit,"selection_exceeded":scans>limit,"queued_exceeded":queued>512})
        }
        "claim_weight" => encoded(claim(count(&input["units"])?)),
        "scan_weight" => encoded(scan(count(&input["units"])?)),
        "weight_fee" => encoded(fee(count(&input["weight"])?)),
        "combined_weight" => encoded(claim_weight(
            count(&input["units"])?,
            count(&input["quotes"])?,
            count(&input["rows"])?,
        )),
        "charged_weight" => encoded(charged(limit)),
        "declared_weight" => encoded(claim_weight(limit, 2560, 512)),
        "charged_fee" => encoded(fee(charged(limit))),
        "spent_fee" => encoded(spent(
            integer(&input["reserved"])?,
            count(&input["hotkeys"])?,
            count(&input["redeem"])?,
            count(&input["scan"])?,
            count(&input["selection"])?,
            limit,
        )?),
        "work" => {
            let payouts = array(&input["payouts"])?;
            let sells = array(&input["sells"])?;
            let scans = array(&input["scans"])?;
            let sweeps = array(&input["sweeps"])?;
            if [sells.len(), scans.len(), sweeps.len()]
                .iter()
                .any(|len| *len != payouts.len())
            {
                return Err(ty("claim work arrays must have equal length"));
            }
            let threshold = u64_value(&input["threshold"])? as i128;
            let (mut eligible, mut below, mut redeem, mut scan_work, mut redeemable) =
                (0i128, 0i128, 0i128, 0i128, 0i128);
            for i in 0..payouts.len() {
                add(&mut redeem, count(&sweeps[i])?)?;
                if payouts[i].is_null() {
                    continue;
                }
                let payout = integer(&payouts[i])?;
                let scanned = count(&scans[i])?;
                if payout > 0 && payout >= threshold {
                    let sold = count(&sells[i])?;
                    if sold > scanned {
                        return Err(range("claim preview sells more rows than it scans"));
                    }
                    eligible += 1;
                    add(&mut redeemable, payout)?;
                    add(&mut redeem, sold)?;
                    add(&mut scan_work, scanned - sold)?;
                } else {
                    below += 1;
                    add(&mut scan_work, scanned)?;
                }
            }
            json!({"eligible":eligible,"below":below,"redeem":redeem,"scan":scan_work,"redeemable":redeemable.to_string()})
        }
        "preview" => {
            let previews = array(&input["previews"])?;
            let reserved = integer(&input["reserved"])?;
            let threshold = integer(&input["threshold"])?;
            if threshold < 0 {
                return Err(range("claim threshold must be nonnegative"));
            }
            let missing = count(&input["unquoted"])?;
            if missing > previews.len() as i128 {
                return Err(range("unquoted hotkeys exceed selected claim hotkeys"));
            }
            let (
                mut accrued,
                mut redeemable,
                mut forfeited,
                mut eligible,
                mut below,
                mut dust,
                mut redeemed,
                mut scanned,
            ) = (0i128, 0i128, 0i128, 0i128, 0i128, 0i128, 0i128, 0i128);
            for row in previews.iter().filter(|v| !v.is_null()) {
                add(&mut accrued, integer(&row["accrued_tao"])?)?;
                add(&mut forfeited, integer(&row["forfeited_tao_est"])?)?;
                add(&mut dust, count(&row["dust_rows"])?)?;
                let rows = count(&row["rows"])?;
                let sold = count(&row["rows_to_sell"])?;
                if sold > rows {
                    return Err(range("claim preview sells more rows than it scans"));
                }
                add(&mut redeemed, count(&row["swept"])?)?;
                let value = integer(&row["redeemable_tao"])?;
                if value > 0 && value >= threshold {
                    eligible += 1;
                    add(&mut redeemable, value)?;
                    add(&mut redeemed, sold)?;
                    add(&mut scanned, rows - sold)?;
                } else {
                    below += 1;
                    add(&mut scanned, rows)?;
                }
            }
            let spent = spent(
                reserved,
                previews.len() as i128,
                redeemed,
                scanned,
                count(&input["selection"])?,
                limit,
            )?;
            let complete = missing == 0;
            json!({"reservedFeeRao":reserved.to_string(),"spentFeeFloorRao":spent.to_string(),"possibleRefundRao":(reserved-spent).to_string(),"accruedRao":if complete{Some(accrued.to_string())}else{None},"redeemableRao":if complete{Some(redeemable.to_string())}else{None},"basketAccruedRao":accrued.to_string(),"basketRedeemableRao":redeemable.to_string(),"previewComplete":complete,"unquotedHotkeys":missing as u64,"forfeitedRao":forfeited.to_string(),"thresholdRao":threshold.to_string(),"eligibleHotkeys":eligible,"belowThresholdHotkeys":below,"dustRows":dust,"losesMoney":if complete{Some(spent>redeemable)}else{None}})
        }
        "relationship" => {
            let root = truth(&input["root"]);
            let debt = truth(&input["debt"]);
            let claimed = integer(&input["claimed"])?;
            json!({"selected":root||integer(&input["basket"])?<0||debt||claimed<0,"unquoted":debt||claimed<0||root&&(count(&input["rates"])? >0||integer(&input["cash_rate"])?!=0)})
        }
        _ => {
            return Err(invalid(format!(
                "unknown financial root claim operation {op}"
            )))
        }
    })
}

fn intent(op: &str, input: &Value) -> Result<Value> {
    let rate = || -> Result<BigRational> {
        let result = decimal_ratio(&input["rate"], 1000)?;
        if result >= BigRational::from_integer(BigInt::from(1)) {
            return Err(range("rate_tolerance must be in [0,1)"));
        }
        Ok(result)
    };
    let floor = |value: BigRational| Value::String(value.to_integer().to_string());
    Ok(match op {
        "rate_valid" => json!(decimal_ratio(&input["rate"], 1000)
            .is_ok_and(|value| value < BigRational::from_integer(BigInt::from(1)))),
        "proportion" => {
            let maximum = u64_value(&input["max"])?;
            let raw = text(&input["value"])?;
            let raw = raw.trim().strip_prefix('+').unwrap_or(raw.trim());
            if !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit()) {
                let value = u64_value(&Value::String(raw.to_owned()))?;
                if value > maximum {
                    return Err(range("proportion out of range"));
                }
                Value::String(value.to_string())
            } else {
                let (value, percent) = raw.strip_suffix('%').map_or((raw, false), |v| (v, true));
                let mut ratio = decimal_ratio(&Value::String(value.to_owned()), 1000)?;
                if percent {
                    ratio /= BigInt::from(100);
                }
                if ratio > BigRational::from_integer(BigInt::from(1)) {
                    return Err(range("proportion exceeds 1"));
                }
                Value::String(
                    super::numeric::round_even(&(ratio * BigInt::from(maximum))).to_string(),
                )
            }
        }
        "price_limit" => {
            let price = BigInt::from(u64_value(&input["price"])?);
            let rate = rate()?;
            let factor = BigRational::from_integer(BigInt::from(1))
                + if truth(&input["buy"]) { rate } else { -rate };
            floor(factor * price)
        }
        "ratio_limit" => {
            let left = BigInt::from(u64_value(&input["left"])?);
            let right = u64_value(&input["right"])?;
            if right == 0 {
                return Err(range("destination alpha price is zero"));
            }
            let ratio = left * BigInt::from(1_000_000_000u64) / BigInt::from(right);
            floor((BigRational::from_integer(BigInt::from(1)) - rate()?) * ratio)
        }
        "free_amount" => {
            let available = i128::from(u64_value(&input["free"])?)
                - i128::from(u64_value(&input["existential_deposit"])?)
                - if input["headroom"].is_null() {
                    500_000
                } else {
                    i128::from(u64_value(&input["headroom"])?)
                };
            if available <= 0 {
                return Err(range(
                    "free balance does not cover existential deposit and fee headroom",
                ));
            }
            encoded(available)
        }
        _ => return Err(invalid(format!("unknown financial intent operation {op}"))),
    })
}

fn positive_big(value: &Value) -> Result<BigInt> {
    if value.is_number() {
        integer(value)?;
    }
    let result = text(value)?
        .parse::<BigInt>()
        .map_err(|_| ty("amount must be a nonnegative integer"))?;
    if result < BigInt::zero() {
        return Err(range("amount must be a nonnegative integer"));
    }
    Ok(result)
}
fn affordability(input: &Value) -> Result<Value> {
    let mut spends: BTreeMap<String, BigInt> = BTreeMap::new();
    let mut required: BTreeMap<String, BigInt> = BTreeMap::new();
    let mut reserve_charges: BTreeMap<String, BigInt> = BTreeMap::new();
    let account = |row: &Value| -> Result<String> {
        row["account"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ty("canonical account identity is required"))
    };
    for name in ["spends", "charges", "reserve_charges"] {
        if input[name].is_null() {
            continue;
        }
        for row in array(&input[name])? {
            let key = account(row)?;
            let value = positive_big(&row["amount"])?;
            if name == "spends" {
                *spends.entry(key.clone()).or_default() += &value;
            }
            if name == "reserve_charges" {
                *reserve_charges.entry(key).or_default() += value;
            } else {
                *required.entry(key).or_default() += value;
            }
        }
    }
    if !input["floors"].is_null() {
        for row in array(&input["floors"])? {
            *required.entry(account(row)?).or_default() +=
                positive_big(&row["minimum"])?.max(positive_big(&row["frozen"])?);
        }
    }
    let required=required.into_iter().map(|(account,amount)| -> Result<Value> {
        let available=if input["free"][&account].is_null(){None}else{Some(positive_big(&input["free"][&account])?)};
        Ok(json!({"account":account,"amount":amount.to_string(),"affordable":available.as_ref().map(|free|*free>=amount),"available":available.map(|n|n.to_string())}))
    }).collect::<Result<Vec<_>>>()?;
    let mut reserves = vec![];
    if !input["reserves"].is_null() {
        for row in array(&input["reserves"])? {
            let account = account(row)?;
            let required = positive_big(&row["required"])?
                + spends.get(&account).cloned().unwrap_or_default()
                + reserve_charges.get(&account).cloned().unwrap_or_default();
            let available = positive_big(&row["available"])?;
            reserves.push(json!({"account":account,"required":required.to_string(),"available":available.to_string(),"affordable":available>=required}));
        }
    }
    Ok(json!({"required":required,"reserves":reserves}))
}

pub(crate) fn evaluate(operation: &str, input: Value) -> Result<Value> {
    if operation.starts_with("financial.reads.") {
        return locks::evaluate(operation, &input);
    }
    let op = operation.strip_prefix("financial.").unwrap_or(operation);
    if op == "constants" {
        let mut values = emission("constants", &Value::Null)?;
        let values = values
            .as_object_mut()
            .ok_or_else(|| ty("invalid financial constants"))?;
        values.extend(
            object(&json!({
                "GAMMA_UNITS": "1000000000", "MAX_GAMMA_SPENDS": 512,
                "MAX_GAMMA_DEPOSITS": 64, "MAX_SERVICE_URL_BYTES": 256,
                "ROOT_CLAIM_MAX_WORK": 256, "ROOT_CLAIM_HOTKEY_WORK": 130,
                "ROOT_CLAIM_FEE_ALLOWANCE": 4, "MAX_AUTHORIZED_ORDERS": 8,
                "MIN_INDEX_NAV_TAO": 0.1, "BLOCKS_PER_DAY": 7200
            }))?
            .clone(),
        );
        values.insert(
            "COLDKEY_PAYS_FEE_CALLS".into(),
            fees::evaluate("constants", &Value::Null)?,
        );
        return Ok(Value::Object(values.clone()));
    }
    if let Some(op) = op.strip_prefix("intent.") {
        return intent(op, &input);
    }
    if let Some(op) = op.strip_prefix("emission.") {
        return emission(op, &input);
    }
    if let Some(op) = op.strip_prefix("root_claim.") {
        return root_claim(op, &input);
    }
    if let Some(op) = op.strip_prefix("gamma.") {
        return gamma::evaluate(op, &input);
    }
    if let Some(op) = op.strip_prefix("basket.") {
        return basket::evaluate(op, &input);
    }
    if let Some(op) = op.strip_prefix("orders.") {
        return orders::evaluate(op, &input);
    }
    if let Some(op) = op.strip_prefix("fee.") {
        return fees::evaluate(op, &input);
    }
    if let Some(op) = op.strip_prefix("shield.") {
        return shield::evaluate(op, &input);
    }
    match op {
        "staking_decision" | "staking_amount" | "staking_claim" | "staking_call"
        | "basket_legs" => staking::evaluate(op, &input),
        "affordability" => affordability(&input),
        "registration.deposit" => Ok(encoded(i128::from(u64_value(&input["value"])?))),
        "registration.committed_calls" | "registration.committed_quote" => {
            registration::evaluate(op, &input)
        }
        "registration.sponsorship" => {
            let mut total = 0i128;
            for value in array(&input["deposits"])? {
                add(&mut total, i128::from(u64_value(value)?))?;
            }
            let maximum = if input["maximum"].is_null() {
                None
            } else {
                Some(i128::from(u64_value(&input["maximum"])?))
            };
            Ok(
                json!({"total":total.to_string(),"required":total>0,"reason":if total==0{None}else if maximum.is_some_and(|max|total>max){Some("maximum")}else if maximum.is_none()&&!truth(&input["callback"]){Some("consent")}else{None}}),
            )
        }
        _ => Err(invalid(format!(
            "unknown financial policy operation {operation}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(name: &str, input: Value) -> Value {
        evaluate(name, input).unwrap_or_else(|error| panic!("{}: {}", error.kind, error.message))
    }

    #[test]
    fn malformed_extreme_values_return_errors_without_overflow_panics() {
        for (op, input) in [
            (
                "emission.round",
                json!({"weights":[i128::MAX.to_string(),i128::MAX.to_string()],"gamma":"0"}),
            ),
            (
                "emission.round",
                json!({"weights":[],"gamma":i128::MIN.to_string()}),
            ),
            ("emission.parse_share", json!({"value":"1e-2147483648%"})),
            (
                "emission.renormalize",
                json!({"weights":[i128::MAX.to_string(),i128::MAX.to_string()],"fixed":{}}),
            ),
            ("gamma.move_flags", json!({"balance":"-1","amount":"1"})),
        ] {
            let result = std::panic::catch_unwind(|| evaluate(op, input));
            assert!(
                matches!(result, Ok(Err(_))),
                "{op} must fail without panicking"
            );
        }
        assert!(evaluate("orders.signed",json!({"strict":false,"orders":[{"order":{"Authorized":{"nonce":"1","terms":{"V1":{}}}},"signature":{"AccountApproved":true},"partial_fill":null}]})).is_err());
        assert_eq!(
            policy(
                "fee.charges",
                json!({"call":["Commitments","set_commitment"]})
            ),
            true
        );
        let result = policy(
            "affordability",
            json!({"spends":[{"account":"a","amount":u128::MAX.to_string()}],"charges":[{"account":"a","amount":"1"}],"free":{"a":u128::MAX.to_string()}}),
        );
        assert_eq!(
            result["required"][0]["amount"],
            "340282366920938463463374607431768211456"
        );
        assert_eq!(result["required"][0]["affordable"], false);
        assert_eq!(
            policy(
                "fee.payer",
                json!({"call":{"pallet":"Balances","call":"transfer_keep_alive","params":{"dest":"b","value":"1000"}},"signer":"a","state":{}})
            )["address"],
            "a"
        );
    }

    #[test]
    fn exact_decimal_boundaries_and_tie_even_proportions() {
        assert_eq!(
            policy("gamma.units", json!({"value":"18446744073.709551615"})),
            json!(u64::MAX.to_string())
        );
        assert!(evaluate("gamma.units", json!({"value":"18446744073.709551616"})).is_err());
        assert!(evaluate("gamma.units", json!({"value":"0.0000000001"})).is_err());
        assert_eq!(
            policy("intent.proportion", json!({"value":"50%","max":"5"})),
            "2"
        );
        assert_eq!(
            policy("intent.proportion", json!({"value":"50%","max":"7"})),
            "4"
        );
        assert_eq!(
            policy("intent.proportion", json!({"value":1,"max":"65535"})),
            "1"
        );
        assert_eq!(
            policy(
                "emission.fraction",
                json!({"numerator":u64::MAX.to_string(),"denominator":u64::MAX.to_string()})
            ),
            u64::MAX.to_string()
        );
        assert_eq!(
            policy(
                "intent.price_limit",
                json!({"price":"18446744073709551615","rate":"0.000000001","buy":false})
            ),
            "18446744055262807541"
        );
        assert!(evaluate(
            "emission.parse_share",
            json!({"value":"1.000000000000000000000000000000001"})
        )
        .is_err());
    }

    #[test]
    fn runtime_claim_allowance_is_independent_of_execution_stability_reads() {
        assert_eq!(
            policy("root_claim.charged_weight", json!({"limit":130})),
            "29894612996"
        );
        assert_eq!(
            policy("root_claim.charged_fee", json!({"limit":256})),
            "7473653"
        );
        assert_eq!(
            policy("root_claim.charged_fee", json!({"limit":0})),
            "877912"
        );
        assert_eq!(
            policy("root_claim.declared_weight", json!({"limit":130})),
            "2093378599648"
        );
        assert_eq!(
            policy("root_claim.declared_weight", json!({"limit":256})),
            "2518744849768"
        );
        assert_eq!(
            policy(
                "root_claim.spent_fee",
                json!({"reserved":"7473653","hotkeys":1,"redeem":0,"scan":32,"selection":0})
            ),
            "7473653"
        );
        assert_eq!(
            policy(
                "root_claim.work",
                json!({"payouts":["2500000","400000",null],"sells":[2,10,20],"scans":[4,10,20],"sweeps":[1,0,0],"threshold":"500000"})
            ),
            json!({"eligible":1,"below":1,"redeem":3,"scan":12,"redeemable":"2500000"})
        );
    }

    #[test]
    fn gamma_repeated_entries_consume_one_canonical_balance_and_cap() {
        let result = policy(
            "gamma.spends",
            json!({"deposits":{"id":{"balance":"100","spendable":"80","owned":true}},"spends":[{"identity":"id","hotkey":"address-a","amount":"60","reference":"0x00"},{"identity":"id","hotkey":"address-b","amount":"30","reference":"0x01"},{"identity":"id","hotkey":"address-a","amount":"20","reference":"0x02"}]}),
        );
        assert_eq!(result["total"], "80");
        assert_eq!(result["skipped"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            result["skipped"][0]["reason"],
            "GammaDepositDailySpendCapExceeded"
        );
    }

    #[test]
    fn consent_queries_require_physical_presence_and_bound_proxy_depth() {
        let transfer = json!({"pallet":"Balances","call":"transfer_keep_alive","params":{}});
        let proxy = |real: &str, call: Value| json!({"pallet":"Proxy","call":"proxy","params":{"real":real,"call":call}});
        let call = proxy("one", proxy("two", proxy("three", proxy("four", transfer))));
        let mut state = json!({});
        for pair in [["one", "signer"], ["two", "one"], ["three", "two"]] {
            let result = policy(
                "fee.payer",
                json!({"call":call,"signer":"signer","state":state}),
            );
            assert_eq!(result["kind"], "exists");
            assert_eq!(result["params"], json!(pair));
            state[result["key"].as_str().unwrap_or("")] = json!(1);
        }
        assert_eq!(
            policy(
                "fee.payer",
                json!({"call":call,"signer":"signer","state":state})
            ),
            json!({"kind":"result","address":"three","dropsTip":true})
        );
        assert_eq!(
            policy(
                "fee.charges",
                json!({"call":["Proxy","proxy",{"call":{"module":"SubtensorModule","function":"set_weights","params":{}}}]})
            ),
            true
        );
    }

    #[test]
    fn order_transcript_commits_full_economic_bytes_and_nonce() {
        let input = json!({"genesis":format!("0x{}","11".repeat(32)),"owner":format!("0x{}","22".repeat(32)),"nonce":"18446744073709551615","encoded":"0x010203"});
        let result = policy("orders.transcript", input.clone());
        assert_eq!(
            result,
            json!(format!(
                "0x{}{}{}ffffffffffffffff010203",
                hex::encode(b"bittensor/ms/v2/order-id"),
                "11".repeat(32),
                "22".repeat(32)
            ))
        );
        let mut altered = input;
        altered["encoded"] = json!("0x010204");
        assert_ne!(result, policy("orders.transcript", altered));
        assert!(evaluate(
            "orders.layout",
            json!({"encoded":"0x000002010000000000000001","nonce":"2"})
        )
        .is_err());
    }
}
