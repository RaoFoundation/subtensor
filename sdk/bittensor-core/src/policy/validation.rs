// Integer arithmetic uses BigInt or bounded u16/u64 inputs widened to u128;
// lengths use checked offsets and nonce iteration explicitly wraps.
#![allow(clippy::arithmetic_side_effects)]
use super::numeric::{decimal, round_even};
use super::PolicyError;
use crate::timelock::constants::{DRAND_PERIOD, GENESIS_TIME};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde_json::{json, Value};

pub(crate) fn evaluate(operation: &str, input: Value) -> Result<Value, PolicyError> {
    match operation {
        "validation.constants" => Ok(
            json!({"RAO_PER_TAO": "1000000000", "QUICKNET_GENESIS": GENESIS_TIME, "QUICKNET_PERIOD": DRAND_PERIOD, "MAX_WORK_AGE_BLOCKS": 5, "CHALLENGE_REFRESH_SECONDS": 12}),
        ),
        "validation.amount.parse" => amount_parse(&input),
        "validation.amount.units" => {
            let left = integer(&input["left"], "netuid")?;
            Ok(json!(match input["mode"].as_str() {
                Some("tao") => left.is_zero(),
                Some("alpha") => !left.is_zero(),
                Some("same") => left == integer(&input["right"], "netuid")?,
                _ => return Err(PolicyError::type_error("unknown amount unit requirement")),
            }))
        }
        "validation.weights.normalize" => normalize_weights(&input),
        "validation.weights.clip" => clip_weights(&input),
        "validation.weights.admission" => weight_admission(&input),
        "validation.weights.finalize" => finalize_weights(&input),
        "validation.hyperparameter" => hyperparameter(&input),
        "validation.pow.difficulty" => difficulty(&input),
        "validation.pow.prefix" => pow_prefix(&input),
        "validation.pow.seal" => Ok(json!(format!(
            "0x{}",
            hex::encode(registration_seal(
                &bytes(&input["prefix"], "prefix")?,
                u64_value(&input["nonce"], "nonce")?
            )?)
        ))),
        "validation.pow.target" => {
            let work = bytes(&input["work"], "work")?;
            if work.len() != 32 {
                return Err(PolicyError::invalid(
                    "miner returned a seal with invalid length",
                ));
            }
            Ok(json!(
                sp_core::U256::from_little_endian(&work)
                    <= registration_target(u64_value(&input["difficulty"], "difficulty")?)?
            ))
        }
        "validation.pow.fresh" => {
            let current = u64_value(&input["current"], "current block")?;
            let work = u64_value(&input["work"], "work block")?;
            Ok(json!(
                current >= work && current - work < 5 && input["anchor"] == input["canonical"]
            ))
        }
        "validation.pow.admission" => pow_admission(&input),
        "validation.pow.mine" => pow_mine(&input),
        "validation.timelock.round" => timelock_round(&input),
        "validation.timelock.time" => timelock_time(&input),
        "validation.timelock.envelope" => timelock_envelope(&input),
        "validation.pruning.state" => pruning_state(&input),
        "validation.pruning.admission" => pruning_admission(&input),
        "validation.pruning.decision" => pruning_decision(&input),
        "validation.pruning.progress" => pruning_progress(&input),
        "validation.pruning.cooldown" => Ok(json!(
            integer(&input["last"], "last update")? > BigInt::zero()
                && integer(&input["block"], "block")? - integer(&input["last"], "last update")?
                    < integer(&input["tempo"], "tempo")? * integer(&input["epochs"], "epochs")?
        )),
        "validation.upgrade.blob" => upgrade_blob(&input),
        "validation.upgrade.spec_version" => upgrade_version(&input),
        _ => Err(PolicyError::invalid(format!(
            "unknown SDK policy operation {operation}"
        ))),
    }
}

fn amount_parse(input: &Value) -> Result<Value, PolicyError> {
    let text = input["text"]
        .as_str()
        .ok_or_else(|| PolicyError::type_error("balance amount requires text"))?
        .trim();
    let strict = input["precision"].as_str() == Some("strict");
    if strict {
        let unsigned = text.strip_prefix('-').unwrap_or(text);
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty()
            || !whole.bytes().all(|c| c.is_ascii_digit())
            || unsigned.contains('.') && fraction.is_empty()
            || !fraction.bytes().all(|c| c.is_ascii_digit())
        {
            return Err(PolicyError::range_error(format!(
                "invalid balance amount {text}"
            )));
        }
        if fraction.len() > 9 {
            return Err(PolicyError::range_error(
                "balance amount cannot have more than 9 decimal places",
            ));
        }
    }
    let value = decimal(text, 1000)? * BigRational::from_integer(BigInt::from(1_000_000_000u64));
    Ok(json!(round_even(&value).to_string()))
}

fn number(value: &Value, label: &str) -> Result<f64, PolicyError> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .filter(|n| n.is_finite())
        .ok_or_else(|| PolicyError::range_error(format!("{label} must be finite")))
}
fn integer(value: &Value, label: &str) -> Result<BigInt, PolicyError> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    text.parse()
        .map_err(|_| PolicyError::type_error(format!("{label} must be an integer")))
}
fn u64_value(value: &Value, label: &str) -> Result<u64, PolicyError> {
    integer(value, label)?
        .to_u64()
        .ok_or_else(|| PolicyError::range_error(format!("{label} must fit u64")))
}
fn floats(value: &Value) -> Result<Vec<f64>, PolicyError> {
    value
        .as_array()
        .ok_or_else(|| PolicyError::type_error("weights must be a list"))?
        .iter()
        .map(|value| {
            let n = number(value, "weights")?;
            if n < 0.0 {
                Err(PolicyError::range_error(
                    "weights must be finite and non-negative",
                ))
            } else {
                Ok(n)
            }
        })
        .collect()
}
fn uids(value: &Value) -> Result<Vec<u16>, PolicyError> {
    value
        .as_array()
        .ok_or_else(|| PolicyError::type_error("uids must be a list"))?
        .iter()
        .map(|value| {
            integer(value, "uid")?
                .to_u16()
                .ok_or_else(|| PolicyError::range_error("uid must be an integer in 0..65535"))
        })
        .collect()
}
fn normalize(ids: &[u16], weights: &[f64]) -> Result<(Vec<u16>, Vec<u16>), PolicyError> {
    if ids.len() != weights.len() {
        return Err(PolicyError::range_error(
            "uids and weights must be equal length",
        ));
    }
    let top = weights.iter().copied().fold(0.0, f64::max);
    if top == 0.0 {
        return Ok((vec![], vec![]));
    }
    Ok(ids
        .iter()
        .zip(weights)
        .filter_map(|(uid, weight)| {
            let value = ((weight / top) * 65535.0).round_ties_even() as u16;
            (value != 0).then_some((*uid, value))
        })
        .unzip())
}
fn normalize_weights(input: &Value) -> Result<Value, PolicyError> {
    Ok(json!(normalize(
        &uids(&input["uids"])?,
        &floats(&input["weights"])?
    )?))
}
fn clip(weights: &[f64], limit: f64) -> Result<Vec<f64>, PolicyError> {
    if !(0.0..=1.0).contains(&limit) {
        return Err(PolicyError::range_error("max weight limit must be in 0..1"));
    }
    let total: f64 = weights.iter().sum();
    let n = weights.len();
    if n == 0 || total <= 0.0 || !total.is_finite() {
        return Err(PolicyError::range_error(
            "weights require a finite positive total",
        ));
    }
    if n as f64 * limit <= 1.0 {
        return Ok(vec![1.0 / n as f64; n]);
    }
    let mut sorted: Vec<f64> = weights.iter().map(|w| w / total).collect();
    sorted.sort_by(f64::total_cmp);
    if sorted[n - 1] <= limit {
        return Ok(weights.iter().map(|w| w / total).collect());
    }
    let mut cumulative = 0.0;
    let sums: Vec<_> = sorted
        .iter()
        .map(|w| {
            cumulative += w;
            cumulative
        })
        .collect();
    let epsilon = 1e-7;
    let count = sorted
        .iter()
        .enumerate()
        .filter(|(i, w)| **w / ((n - i - 1) as f64 * **w + sums[*i] + epsilon) < limit)
        .count();
    if count == 0 {
        return Err(PolicyError::range_error(
            "weight clipping has no feasible cutoff",
        ));
    }
    let cutoff = ((limit * sums[count - 1] - epsilon) / (1.0 - limit * (n - count) as f64)) * total;
    let clipped: Vec<_> = weights.iter().map(|w| w.min(cutoff)).collect();
    let sum: f64 = clipped.iter().sum();
    Ok(clipped.into_iter().map(|w| w / sum).collect())
}
fn clip_weights(input: &Value) -> Result<Value, PolicyError> {
    Ok(json!(clip(
        &floats(&input["weights"])?,
        number(&input["limit"], "max weight limit")?
    )?))
}
fn weight_admission(input: &Value) -> Result<Value, PolicyError> {
    if input["uid"].is_null() {
        return Ok(json!({"code": "NOT_REGISTERED"}));
    }
    if input["null"] == true && input["permit"] != true {
        return Ok(json!({"code": "NOT_AUTHORIZED"}));
    }
    let previous = integer(&input["last"], "last update")?;
    let since = integer(&input["current"], "current block")? - &previous;
    let rate = integer(&input["rate"], "rate limit")?;
    if previous > BigInt::zero() && rate > BigInt::zero() && since < rate {
        return Ok(json!({"code": "RATE_LIMITED", "wait": (rate - since).to_string()}));
    }
    Ok(Value::Null)
}
fn finalize_weights(input: &Value) -> Result<Value, PolicyError> {
    let mut ids = uids(&input["uids"])?;
    let mut weights = floats(&input["weights"])?;
    if ids.len() != weights.len() {
        return Err(PolicyError::range_error(
            "uids and weights must be equal length",
        ));
    }
    let uid = u64_value(&input["uid"], "registered uid")?;
    let self_weight = ids.len() == 1 && u64::from(ids[0]) == uid;
    let maximum = u64_value(&input["maximum"], "maximum weight limit")?;
    let minimum = u64_value(&input["minimum"], "minimum weight count")?;
    let values = if input["raw"] == true {
        if input["null"] != true {
            return Err(PolicyError::type_error(
                "raw_u16 requires a subnet running Null consensus",
            ));
        }
        if weights.iter().any(|w| w.fract() != 0.0 || *w > 65535.0) {
            return Err(PolicyError::range_error(
                "raw weights must be integers in 0..65535",
            ));
        }
        let selected: Vec<_> = ids
            .into_iter()
            .zip(weights)
            .filter(|(_, w)| *w > 0.0)
            .collect();
        let (new_ids, values): (Vec<u16>, Vec<u16>) =
            selected.into_iter().map(|(id, w)| (id, w as u16)).unzip();
        ids = new_ids;
        let top = u128::from(values.iter().copied().max().unwrap_or(0));
        let sum: u128 = values.iter().map(|n| u128::from(*n)).sum();
        if !self_weight && top * 65535 > u128::from(maximum) * sum {
            return Err(PolicyError::range_error(
                "raw weights exceed max_weight_limit",
            ));
        }
        values
    } else {
        if !self_weight && maximum < 65535 {
            weights = clip(&weights, maximum as f64 / 65535.0)?;
        }
        let (new_ids, values) = normalize(&ids, &weights)?;
        ids = new_ids;
        values
    };
    if ids.is_empty() {
        return Err(PolicyError::range_error(
            "all weights are zero; nothing to set",
        ));
    }
    Ok(
        json!({"uids": ids, "values": values, "insufficient": !self_weight && (values.len() as u64) < minimum}),
    )
}

fn hyperparameter(input: &Value) -> Result<Value, PolicyError> {
    let name = input["name"].as_str().unwrap_or("hyperparameter");
    let kind = input["kind"].as_str().unwrap_or("int");
    let value = &input["value"];
    if name == "epoch_consensus" {
        return if value == "Yuma" || value == "Null" {
            Ok(value.clone())
        } else {
            Err(PolicyError::range_error(
                "epoch_consensus must be Yuma or Null",
            ))
        };
    }
    if kind == "bool" {
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        let aliases = input["aliases"] == true;
        return match text.as_str() {
            "true" | "1" => Ok(json!(true)),
            "false" | "0" => Ok(json!(false)),
            "yes" | "on" if aliases => Ok(json!(true)),
            "no" | "off" if aliases => Ok(json!(false)),
            _ => Err(PolicyError::type_error(format!(
                "{name} accepts true/false or 0/1"
            ))),
        };
    }
    if value.is_boolean() {
        return Err(PolicyError::type_error(format!(
            "{name} does not accept booleans"
        )));
    }
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    let result = if input["human"] == true {
        let ratio = decimal(&text, 1000).map_err(|error| {
            if error.kind == "range" {
                error
            } else {
                PolicyError::type_error(format!("{name} requires a numeric value"))
            }
        })?;
        let scale = match kind {
            "u16" => Some(BigInt::from(65535u32)),
            "u64" => Some(BigInt::from(u64::MAX)),
            "per_million" => Some(BigInt::from(1_000_000u32)),
            "rao" => Some(BigInt::from(1_000_000_000u32)),
            "fixed128" => Some(BigInt::one() << 64),
            _ => None,
        };
        if ["u16", "u64", "per_million"].contains(&kind)
            && (ratio.is_negative() || ratio > BigRational::one())
        {
            return Err(PolicyError::range_error(format!(
                "{name} normalized fraction must be between zero and one"
            )));
        }
        if ratio.is_negative() && ["rao", "fixed128"].contains(&kind) {
            return Err(PolicyError::range_error(format!(
                "{name} cannot be negative"
            )));
        }
        if scale.is_none() && !ratio.is_integer() {
            return Err(PolicyError::range_error(format!(
                "{name} requires an integral value"
            )));
        }
        round_even(&(ratio * BigRational::from_integer(scale.unwrap_or_else(BigInt::one))))
    } else {
        integer(value, name)?
    };
    let low = integer(&input["minimum"], "minimum")?;
    let high = integer(&input["maximum"], "maximum")?;
    if result < low || result > high {
        return Err(PolicyError::range_error(format!(
            "{name} must be within {low}..{high}"
        )));
    }
    Ok(json!(result.to_string()))
}

fn difficulty(input: &Value) -> Result<Value, PolicyError> {
    let read = |key: &str, fallback| {
        if input[key].is_null() {
            Ok(fallback)
        } else {
            u64_value(&input[key], key)
        }
    };
    let minimum = read("minimum", 1)?.max(1);
    let maximum = read("maximum", u64::MAX)?.max(minimum);
    Ok(json!(read("value", 1)?
        .max(1)
        .clamp(minimum, maximum)
        .to_string()))
}
fn bytes(value: &Value, name: &str) -> Result<Vec<u8>, PolicyError> {
    let text = value
        .as_str()
        .ok_or_else(|| PolicyError::type_error(format!("{name} requires hex bytes")))?;
    hex::decode(text.strip_prefix("0x").unwrap_or(text))
        .map_err(|_| PolicyError::type_error(format!("invalid {name} hex bytes")))
}
fn pow_prefix(input: &Value) -> Result<Value, PolicyError> {
    let netuid = integer(&input["netuid"], "netuid")?
        .to_u16()
        .filter(|n| *n != 0)
        .ok_or_else(|| PolicyError::range_error("PoW requires a non-root u16 netuid"))?;
    let mut result = b"subtensor-pow-register-v1".to_vec();
    result.extend(netuid.to_le_bytes());
    for key in ["block", "hotkey", "coldkey"] {
        let value = bytes(&input[key], key)?;
        if value.len() != 32 {
            return Err(PolicyError::range_error(
                "PoW requires a 32-byte block hash and AccountId32 addresses",
            ));
        }
        result.extend(value);
    }
    Ok(json!(format!("0x{}", hex::encode(result))))
}

/// Portable hashing and target rules shared by CPU/GPU verification and SDKs.
pub(crate) fn registration_seal(prefix: &[u8], nonce: u64) -> Result<[u8; 32], PolicyError> {
    use sha2::{Digest, Sha256};
    validate_registration_prefix(prefix)?;
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(nonce.to_le_bytes());
    Ok(sp_core::hashing::keccak_256(&hasher.finalize()))
}

pub(crate) fn validate_registration_prefix(prefix: &[u8]) -> Result<(), PolicyError> {
    if prefix.len() != 123 || !prefix.starts_with(b"subtensor-pow-register-v1") {
        return Err(PolicyError::invalid(
            "expected the 123-byte subnet registration challenge prefix",
        ));
    }
    if prefix.get(25..27) == Some(&[0, 0]) {
        return Err(PolicyError::invalid(
            "PoW registration is unavailable on root",
        ));
    }
    Ok(())
}

pub(crate) fn registration_target(difficulty: u64) -> Result<sp_core::U256, PolicyError> {
    sp_core::U256::MAX
        .checked_div(sp_core::U256::from(difficulty))
        .ok_or_else(|| PolicyError::invalid("PoW difficulty must be positive"))
}

fn pow_admission(input: &Value) -> Result<Value, PolicyError> {
    if input["allowed"] != true {
        return Ok(json!("disabled"));
    }
    if !input["uid"].is_null() {
        return Ok(json!("registered"));
    }
    let limit = u64_value(&input["limit"], "registration limit")?;
    if limit == 0 {
        return Ok(json!("limit"));
    }
    if u64_value(&input["registrations"], "registrations")? >= limit
        || (!input["last"].is_null()
            && u64_value(&input["head"], "head")?
                <= u64_value(&input["last"], "last registration")?)
    {
        return Ok(json!("wait"));
    }
    Ok(Value::Null)
}

fn pow_mine(input: &Value) -> Result<Value, PolicyError> {
    let prefix = bytes(&input["prefix"], "prefix")?;
    validate_registration_prefix(&prefix)?;
    let attempts = u64_value(&input["attempts"], "attempts")?;
    if attempts > 1_048_576 {
        return Err(PolicyError::range_error(
            "PoW batch must contain at most 1048576 attempts",
        ));
    }
    let start = u64_value(&input["start"], "start")?;
    // Python historically accepts arbitrary integer difficulty in its public
    // compatibility helper. Keep this exact path in Rust too; runtime mining
    // takes the faster u64 host entrypoint.
    let difficulty = integer(&input["difficulty"], "difficulty")?.max(BigInt::one());
    let target = ((BigInt::one() << 256) - 1u8) / difficulty;
    for offset in 0..attempts {
        let nonce = start.wrapping_add(offset);
        let seal = registration_seal(&prefix, nonce)?;
        if BigInt::from_bytes_le(num_bigint::Sign::Plus, &seal) <= target {
            return Ok(json!({"nonce": nonce.to_string(), "work": hex::encode(seal)}));
        }
    }
    Ok(Value::Null)
}
fn timelock_round(input: &Value) -> Result<Value, PolicyError> {
    let at = number(&input["at"], "timestamp")?;
    let elapsed = at - GENESIS_TIME as f64;
    let round = if input["ceil"] == true {
        if elapsed <= 0.0 {
            1.0
        } else {
            (elapsed / DRAND_PERIOD as f64).ceil() + 1.0
        }
    } else {
        ((elapsed / DRAND_PERIOD as f64).floor() + 1.0).max(1.0)
    };
    if round >= u64::MAX as f64 {
        return Err(PolicyError::range_error("round must be in 1..u64::MAX"));
    }
    Ok(json!((round as u64).to_string()))
}
fn timelock_time(input: &Value) -> Result<Value, PolicyError> {
    let round = u64_value(&input["round"], "round")?;
    if round == 0 {
        return Err(PolicyError::range_error("round must be in 1..u64::MAX"));
    }
    let seconds = u128::from(GENESIS_TIME) + u128::from(round - 1) * u128::from(DRAND_PERIOD);
    Ok(json!(seconds.to_string()))
}
fn compact_decode(data: &[u8], offset: usize) -> Result<(u64, usize), PolicyError> {
    use codec::{Compact, Decode, Encode};
    let rest = data
        .get(offset..)
        .ok_or_else(|| PolicyError::type_error("truncated compact integer"))?;
    let mut cursor = rest;
    let Compact(value) = Compact::<u64>::decode(&mut cursor)
        .map_err(|_| PolicyError::type_error("invalid or non-canonical compact integer"))?;
    let consumed = rest.len() - cursor.len();
    if Compact(value).encode() != rest[..consumed] {
        return Err(PolicyError::type_error("non-canonical compact integer"));
    }
    Ok((value, offset + consumed))
}
fn timelock_envelope(input: &Value) -> Result<Value, PolicyError> {
    let data = bytes(&input["data"], "ciphertext")?;
    if data.len() < 9 {
        return Err(PolicyError::invalid("not a timelock ciphertext"));
    }
    let (size, offset) = compact_decode(&data, 0)
        .map_err(|_| PolicyError::invalid("non-canonical ciphertext length"))?;
    let end = usize::try_from(size)
        .ok()
        .and_then(|n| offset.checked_add(n))
        .ok_or_else(|| PolicyError::invalid("ciphertext length exceeds buffer"))?;
    if end.checked_add(8) != Some(data.len()) {
        return Err(PolicyError::invalid("invalid ciphertext envelope length"));
    }
    let round = u64::from_le_bytes(
        data[end..]
            .try_into()
            .map_err(|_| PolicyError::invalid("invalid ciphertext envelope length"))?,
    );
    if round == 0 {
        return Err(PolicyError::invalid("round must be in 1..u64::MAX"));
    }
    Ok(
        json!({"inner": format!("0x{}", hex::encode(&data[offset..end])), "round": round.to_string()}),
    )
}
fn pruning_state(input: &Value) -> Result<Value, PolicyError> {
    let count = u64_value(&input["mechanisms"], "mechanism count")?;
    if !(1..=16).contains(&count) {
        return Err(PolicyError::invalid("invalid mechanism count"));
    }
    let block = u64_value(&input["block"], "block")?;
    let tempo = u64_value(&input["tempo"], "tempo")?;
    let last = u64_value(&input["last"], "last epoch")?;
    let pending = u64_value(&input["pending"], "pending epoch")?;
    let window = u64_value(&input["window"], "freeze window")?;
    Ok(
        json!({"ceiling": 256 / count, "frozen": tempo != 0 && (pending > block || last.saturating_add(tempo).saturating_sub(block) < window)}),
    )
}
fn pruning_decision(input: &Value) -> Result<Value, PolicyError> {
    let state = &input["state"];
    let reason = if state["genesis"] != input["initial"]["genesis"] {
        Some("chain")
    } else if state["owner"] != input["owner"] {
        Some("owner")
    } else if state["mechanisms"] != input["initial"]["mechanisms"] {
        Some("mechanisms")
    } else if !state["pendingTarget"].is_null() && state["pendingTarget"] != input["target"] {
        Some("conflict")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Ok(json!({"error": reason}));
    }
    let target = u64_value(&input["target"], "target")?;
    let population = u64_value(&state["population"], "population")?;
    let ceiling = u64_value(&state["yumaCeiling"], "Yuma ceiling")?;
    if state["mode"] == "Yuma" {
        return Ok(if population <= ceiling {
            json!({"action": "finished"})
        } else {
            json!({"error": "ceiling"})
        });
    }
    if target < u64_value(&state["minimum"], "minimum")?
        || target > u64_value(&state["maximum"], "maximum")?.min(ceiling)
    {
        return Ok(json!({"error": "target"}));
    }
    if state["mode"] != "Null" {
        return Ok(json!({"error": "mode"}));
    }
    Ok(
        json!({"action": if population <= target && state["pendingTarget"].is_null() { "complete" } else { "prune" }}),
    )
}
fn pruning_admission(input: &Value) -> Result<Value, PolicyError> {
    let result = pruning_decision(input)?;
    if let Some(reason) = result["error"].as_str() {
        return Err(PolicyError::invalid(match reason {
            "chain" | "owner" | "mechanisms" => {
                "chain, owner, or mechanism count changed; re-plan pruning"
            }
            "conflict" => "conflicting on-chain pruning target",
            "ceiling" => "Yuma population exceeds its ceiling",
            _ => "invalid Null pruning state or target",
        }));
    }
    Ok(result["action"].clone())
}
fn pruning_progress(input: &Value) -> Result<Value, PolicyError> {
    let after = u64_value(&input["after"], "population")?;
    let before = u64_value(&input["before"], "population")?;
    let target = u64_value(&input["target"], "target")?;
    let cleaned = after <= target && input["pending"].is_null();
    let stalled =
        if after >= before && u64_value(&input["cleared"], "cleared commitments")? == 0 && !cleaned
        {
            u64_value(&input["stalled"], "stalled batches")?.saturating_add(1)
        } else {
            0
        };
    Ok(json!({"stalled": stalled, "stop": stalled >= 3}))
}
fn upgrade_blob(input: &Value) -> Result<Value, PolicyError> {
    let data = bytes(&input["data"], "proposal")?;
    if data.get(..3) != Some(&[16, 0, 0]) {
        return Err(PolicyError::type_error(
            "call is not Proxy.proxy(MultiAddress::Id(...), ...)",
        ));
    }
    if data.len() < 35 {
        return Err(PolicyError::type_error("truncated proxied account"));
    }
    if data.get(35..40) != Some(&[0, 12, 1, 0, 2]) {
        return Err(PolicyError::type_error(
            "proposal must contain exactly Sudo.sudo_unchecked_weight(System.set_code(...))",
        ));
    }
    let (size, offset) = compact_decode(&data, 40)?;
    let end = usize::try_from(size)
        .ok()
        .and_then(|n| offset.checked_add(n))
        .filter(|end| *end <= data.len())
        .ok_or_else(|| PolicyError::type_error("truncated runtime bytes"))?;
    let (ref_time, next) = compact_decode(&data, end)?;
    let (proof_size, next) = compact_decode(&data, next)?;
    if next != data.len()
        || !input["weight"].is_null()
            && (ref_time != u64_value(&input["weight"]["ref_time"], "ref_time")?
                || proof_size != u64_value(&input["weight"]["proof_size"], "proof_size")?)
    {
        return Err(PolicyError::type_error(
            "proposal weight differs or call has trailing bytes",
        ));
    }
    Ok(
        json!({"real": format!("0x{}", hex::encode(&data[3..35])), "code": format!("0x{}", hex::encode(&data[offset..end])), "weight": {"ref_time": ref_time.to_string(), "proof_size": proof_size.to_string()}}),
    )
}
fn upgrade_version(input: &Value) -> Result<Value, PolicyError> {
    let data = bytes(&input["data"], "runtime")?;
    if data.get(..8) != Some(&[0, 97, 115, 109, 1, 0, 0, 0]) {
        return Ok(Value::Null);
    }
    fn leb(data: &[u8], cursor: &mut usize, end: usize) -> Result<usize, PolicyError> {
        let mut result = 0u32;
        for shift in (0..=28).step_by(7) {
            if *cursor >= end {
                return Err(PolicyError::type_error("invalid Wasm section size"));
            }
            let byte = data[*cursor];
            *cursor += 1;
            let part = u32::from(byte & 127);
            if shift == 28 && part > 15 {
                return Err(PolicyError::type_error("invalid Wasm section size"));
            }
            result |= part << shift;
            if byte & 128 == 0 {
                return usize::try_from(result)
                    .map_err(|_| PolicyError::type_error("invalid Wasm section size"));
            }
        }
        Err(PolicyError::type_error("invalid Wasm section size"))
    }
    let mut cursor = 8;
    while cursor < data.len() {
        let kind = data[cursor];
        cursor += 1;
        let length = leb(&data, &mut cursor, data.len())?;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| PolicyError::type_error("truncated Wasm section"))?;
        if kind == 0 {
            let size = leb(&data, &mut cursor, end)?;
            let name_end = cursor
                .checked_add(size)
                .filter(|n| *n <= end)
                .ok_or_else(|| PolicyError::type_error("truncated Wasm custom section name"))?;
            let name = &data[cursor..name_end];
            cursor = name_end;
            if name == b"runtime_version" {
                for _ in 0..2 {
                    let (size, next) = compact_decode(&data[..end], cursor)?;
                    cursor = usize::try_from(size)
                        .ok()
                        .and_then(|n| next.checked_add(n))
                        .filter(|n| *n <= end)
                        .ok_or_else(|| PolicyError::type_error("truncated runtime version"))?;
                }
                let version = data
                    .get(cursor..cursor + 8)
                    .filter(|_| cursor + 8 <= end)
                    .ok_or_else(|| PolicyError::type_error("truncated runtime version"))?;
                return Ok(json!(u32::from_le_bytes(
                    version[4..8]
                        .try_into()
                        .map_err(|_| PolicyError::type_error("truncated runtime version"))?
                )));
            }
        }
        cursor = end;
    }
    Ok(Value::Null)
}
