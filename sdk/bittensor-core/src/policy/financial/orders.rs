use super::*;
fn terms(order: &Value, strict: bool) -> Result<&Value> {
    let row = object(order)?;
    if row.len() != 1 {
        return Err(ty("expected one complete order variant"));
    }
    if let Some(authorized) = row.get("Authorized") {
        let auth = object(authorized)?;
        if auth.len() != 2
            || !auth.contains_key("nonce")
            || !auth.contains_key("terms")
            || authorized["terms"].get("Authorized").is_some()
        {
            return Err(ty("invalid nonrecursive Authorized order"));
        }
        u64_value(&authorized["nonce"])?;
        return terms(&authorized["terms"], strict);
    }
    let value = row
        .get("V1")
        .or_else(|| row.get("V2"))
        .ok_or_else(|| ty("expected complete V1 or V2 order terms"))?;
    let fields = object(value)?;
    if strict {
        if ["signer", "hotkey", "fee_recipient"]
            .iter()
            .any(|key| !fields.contains_key(*key))
        {
            return Err(ty("expected complete V1 or V2 order terms"));
        }
        u16::try_from(integer(&value["netuid"])?).map_err(|_| range("invalid order subnet"))?;
    }
    Ok(value)
}
fn signed_orders(input: &Value) -> Result<Value> {
    let strict = input["strict"] != false;
    let input = array(&input["orders"])?;
    if input.is_empty() {
        return Err(ty("at least one signed order is required"));
    }
    let mut rows = input.clone();
    let mut authorized = 0usize;
    for row in &mut rows {
        let fields = object(row)?;
        if fields.len() != 3
            || ["order", "partial_fill", "signature"]
                .iter()
                .any(|key| !fields.contains_key(*key))
        {
            return Err(ty(
                "signed order requires order, signature and partial_fill",
            ));
        }
        terms(&row["order"], strict)?;
        let auth = row["order"].get("Authorized").is_some();
        if auth {
            authorized += 1;
        }
        if !row["partial_fill"].is_null() {
            row["partial_fill"] = json!(u64_value(&row["partial_fill"])?.to_string());
        }
        let signature = &row["signature"];
        if signature == "AccountApproved"
            || signature.as_object().is_some_and(|value| {
                value.len() == 1 && value.get("AccountApproved").is_some_and(Value::is_null)
            })
        {
            if !auth {
                return Err(ty("account approval requires an Authorized order"));
            }
            continue;
        }
        let map = object(signature).map_err(|_| ty("invalid order authorization"))?;
        if map.len() != 1 {
            return Err(ty("invalid order authorization"));
        }
        let (scheme, proof) = map
            .iter()
            .next()
            .ok_or_else(|| ty("invalid order authorization"))?;
        if !["Ed25519", "Sr25519", "Ecdsa"].contains(&scheme.as_str()) {
            return Err(ty("unknown order authorization"));
        }
        let length = if scheme == "Ecdsa" { 65 } else { 64 };
        let proof = proof
            .as_str()
            .ok_or_else(|| ty("invalid classical order signature"))?;
        let bytes = hex::decode(proof.strip_prefix("0x").unwrap_or(proof))
            .map_err(|_| ty("invalid classical order signature"))?;
        if auth || bytes.len() != length {
            return Err(ty("invalid classical order signature"));
        }
        row["signature"] = json!({scheme:format!("0x{}",hex::encode(bytes))});
    }
    if authorized > 8 {
        return Err(range("at most eight authorized orders can be relayed"));
    }
    Ok(json!(rows))
}
fn bytes(value: &Value) -> Result<Vec<u8>> {
    let text = value.as_str().ok_or_else(|| ty("invalid order bytes"))?;
    hex::decode(text.strip_prefix("0x").unwrap_or(text)).map_err(|_| ty("invalid order bytes"))
}
fn layout(input: &Value) -> Result<Vec<u8>> {
    let data = bytes(&input["encoded"])?;
    let nonce = u64_value(&input["nonce"])?;
    if data.len() <= 11 || data[2] != 2 || data[3..11] != nonce.to_le_bytes() {
        return Err(invalid("incompatible authorized order layout"));
    }
    Ok(data[11..].to_vec())
}
pub(super) fn evaluate(op: &str, input: &Value) -> Result<Value> {
    Ok(match op {
        "layout" => json!(format!("0x{}", hex::encode(layout(input)?))),
        "transcript" => {
            let genesis = bytes(&input["genesis"])?;
            let owner = bytes(&input["owner"])?;
            if genesis.len() != 32 || owner.len() != 32 {
                return Err(invalid(
                    "order chain genesis and owner must contain 32 bytes",
                ));
            }
            let mut data = b"bittensor/ms/v2/order-id".to_vec();
            data.extend(genesis);
            data.extend(owner);
            data.extend(u64_value(&input["nonce"])?.to_le_bytes());
            data.extend(bytes(&input["encoded"])?);
            json!(format!("0x{}", hex::encode(data)))
        }
        "approved_count" => {
            let n = count(&input["count"])?;
            if !(1..=8).contains(&n) {
                return Err(range("authorized execution requires 1..8 orders"));
            }
            json!(true)
        }
        "terms" => terms(&input["order"], input["strict"] != false)?.clone(),
        "authorized" => {
            if input["order"].get("Authorized").is_some() {
                return Err(ty("expected original V1 or V2 terms"));
            }
            terms(&input["order"], input["strict"] != false)?;
            json!({"Authorized":{"nonce":u64_value(&input["nonce"])?.to_string(),"terms":input["order"]}})
        }
        "signed" => signed_orders(input)?,
        "netted" => {
            let netuid = integer(&input["netuid"])?;
            let netuids = array(&input["netuids"])?;
            if input["should_fail"] != true
                || !(1..=65535).contains(&netuid)
                || netuids.iter().any(|v| integer(v).ok() != Some(netuid))
            {
                return Err(invalid(
                    "netted orders must be atomic and target the same non-root subnet",
                ));
            }
            json!(true)
        }
        "pinned" => {
            let encoded = hex::decode(
                input["encoded"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("0x"),
            )
            .map_err(|_| ty("invalid order bytes"))?;
            let terms = hex::decode(
                input["terms"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("0x"),
            )
            .map_err(|_| ty("invalid order bytes"))?;
            let nonce = u64_value(&input["nonce"])?;
            if encoded.len() <= 11
                || encoded[2] != 2
                || encoded[3..11] != nonce.to_le_bytes()
                || encoded[11..] != terms
            {
                return Err(invalid("pinned economic order bytes changed"));
            }
            json!(true)
        }
        _ => return Err(invalid(format!("unknown order policy operation {op}"))),
    })
}
