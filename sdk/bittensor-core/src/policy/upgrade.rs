//! Runtime upgrade verification and approval decisions. Fetching, address
//! decoding, hashing and chain reads remain in the language adapters.
use super::PolicyError;
use num_bigint::BigUint;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, PolicyError>;
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn lower(value: &Value) -> String {
    text(value).to_ascii_lowercase()
}
fn digest(value: &Value) -> Option<String> {
    let value = text(value).strip_prefix("0x").unwrap_or(text(value));
    (value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}
fn integer(value: &Value, name: &str) -> Result<BigUint> {
    let value = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    value
        .parse()
        .map_err(|_| PolicyError::type_error(format!("invalid {name}")))
}
fn safe_integer(value: &Value) -> Option<u64> {
    value.as_u64().filter(|v| *v <= 9_007_199_254_740_991)
}
fn manifest_checks(input: &Value) -> Value {
    let manifest = &input["manifest"];
    let mut checks = Map::new();
    checks.insert(
        "manifest call hash".into(),
        json!(lower(&manifest["call_hash"]) == lower(&input["call_hash"])),
    );
    if input["python_optional"] == true {
        if input["pending"] == true && !text(&manifest["ci_address"]).is_empty() {
            checks.insert(
                "manifest CI key".into(),
                json!(manifest["ci_address"] == input["ci_address"]),
            );
        }
        return Value::Object(checks);
    }
    if !input["code_sha256"].is_null() {
        checks.insert(
            "manifest runtime hash".into(),
            json!(digest(&manifest["wasm_sha256"])
                .is_some_and(|hash| Some(hash) == digest(&input["code_sha256"]))),
        );
    }
    let version = safe_integer(&input["manifest_version"]);
    checks.insert(
        "manifest spec version".into(),
        json!(
            version.is_some()
                && (input["spec_version"].is_null()
                    || version == safe_integer(&input["spec_version"]))
                && (input["expected_spec_version"].is_null()
                    || version == safe_integer(&input["expected_spec_version"]))
        ),
    );
    let commit = text(&manifest["commit"]);
    checks.insert(
        "manifest commit".into(),
        json!(
            commit.len() == 40
                && commit.bytes().all(|v| v.is_ascii_hexdigit())
                && (input["expected_commit"].is_null()
                    || manifest["commit"] == input["expected_commit"])
        ),
    );
    if !input["release"].is_null() {
        checks.insert(
            "release identity".into(),
            json!(
                manifest["repo"] == input["release"]["repo"]
                    && manifest["tag"] == input["release"]["tag"]
            ),
        );
        for name in ["wasm", "digest", "call_data", "manifest"] {
            checks.insert(
                format!("release {name} asset"),
                json!(manifest["assets"][name] == input["release"]["assets"][name]),
            );
        }
    }
    Value::Object(checks)
}
fn signers(input: &Value) -> Result<Value> {
    let members = input["members"]
        .as_array()
        .ok_or_else(|| PolicyError::type_error("signatories must be an address array"))?;
    if input["python_others"] == true {
        // Python historically deduplicates textual addresses, then sorts by
        // account bytes. Distinct SS58 prefixes remain separate entries.
        let mut seen = BTreeSet::new();
        let mut indices: Vec<usize> = members
            .iter()
            .enumerate()
            .filter_map(|(i, member)| {
                (member["address"] != input["self_address"]
                    && seen.insert(text(&member["address"]).to_owned()))
                .then_some(i)
            })
            .collect();
        indices.sort_by_key(|i| text(&members[*i]["identity"]));
        return Ok(json!(indices));
    }
    let threshold = safe_integer(&input["threshold"])
        .ok_or_else(|| PolicyError::type_error("invalid threshold"))?;
    let mut sorted = BTreeMap::new();
    for (index, member) in members.iter().enumerate() {
        if sorted.insert(text(&member["identity"]), index).is_some() {
            return Err(PolicyError::type_error("duplicate signatory account"));
        }
    }
    if threshold < 2 || u128::from(threshold) > sorted.len() as u128 {
        return Err(PolicyError::range_error(
            "sudo multisig threshold must be 2..signatory count",
        ));
    }
    Ok(json!(sorted.values().collect::<Vec<_>>()))
}
pub(crate) fn evaluate(operation: &str, input: Value) -> Result<Value> {
    match operation {
        "upgrade.manifest_checks" => Ok(manifest_checks(&input)),
        "upgrade.digest_matches" => Ok(json!(
            digest(&input["expected"]).is_some_and(|hash| Some(hash) == digest(&input["actual"]))
        )),
        "upgrade.hash_matches" => Ok(json!(lower(&input["expected"]) == lower(&input["actual"]))),
        "upgrade.identity_matches" => Ok(json!(input["expected"] == input["actual"])),
        "upgrade.proxy_eligible" => Ok(json!(
            input["proxy_type"] == "SudoUncheckedSetCode" && input["delay"].as_f64() == Some(0.0)
        )),
        "upgrade.signers" => signers(&input),
        "upgrade.weight_pad" => {
            // Arbitrary precision integers preserve both languages' prior
            // arithmetic without overflow, including the 10% padded result.
            #[allow(clippy::arithmetic_side_effects)]
            fn padded(value: &Value, name: &str) -> Result<String> {
                Ok((integer(value, name)? * BigUint::from(11u8) / BigUint::from(10u8)).to_string())
            }
            Ok(
                json!({"ref_time":padded(&input["ref_time"],"ref_time")?,"proof_size":padded(&input["proof_size"],"proof_size")?}),
            )
        }
        "upgrade.approval" => {
            let approvals = rows(&input["approvals"]);
            let members = rows(&input["members"]);
            let threshold = integer(&input["threshold"], "threshold")?;
            let position = if input["present"] != true {
                "first"
            } else if approvals.contains(&input["signer"]) {
                "signed"
            } else if BigUint::from(approvals.len().saturating_add(1)) >= threshold {
                "final"
            } else {
                "interior"
            };
            Ok(json!({"position":position,
                "remaining_indices":members.iter().enumerate().filter_map(|(i,member)| (!approvals.contains(member)).then_some(i)).collect::<Vec<_>>(),
                "other_indices":members.iter().enumerate().filter_map(|(i,member)| (member != &input["signer"]).then_some(i)).collect::<Vec<_>>(),
                "signer_member":members.contains(&input["signer"])}))
        }
        _ => Err(PolicyError::invalid(format!(
            "unknown upgrade policy operation {operation}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_profiles_preserve_optional_checks_and_fail_closed_hashes() -> Result<()> {
        let input = json!({"manifest":{"call_hash":"0xAB","ci_address":"ci"},"call_hash":"0xab","pending":true,"ci_address":"other","python_optional":true});
        assert_eq!(
            evaluate("upgrade.manifest_checks", input)?,
            json!({"manifest call hash":true,"manifest CI key":false})
        );
        assert_eq!(
            evaluate(
                "upgrade.digest_matches",
                json!({"expected":"bad","actual":"bad"})
            )?,
            false
        );
        assert_eq!(
            evaluate(
                "upgrade.approval",
                json!({"present":true,"threshold":3,"members":["a","b","c"],"approvals":["a","b"],"signer":"c"})
            )?["position"],
            "final"
        );
        assert_eq!(
            evaluate(
                "upgrade.weight_pad",
                json!({"ref_time":"18446744073709551615","proof_size":"10"})
            )?["ref_time"],
            "20291418481080506776"
        );
        Ok(())
    }
}
