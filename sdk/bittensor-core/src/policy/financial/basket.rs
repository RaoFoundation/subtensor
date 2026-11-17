use super::*;
fn n(value: &Value) -> Result<f64> {
    if value.is_null() {
        Ok(0.0)
    } else if value.is_object() {
        float(&value["tao"])
    } else {
        float(value)
    }
}
fn spot(row: &Value) -> Result<f64> {
    let real = n(&row["beta_price_tao"])?;
    let nav = n(&row["nav_tao"])?;
    let marked = n(&row["spot_nav_tao"])?;
    Ok(if nav > 0.0 && marked > 0.0 {
        real * (marked / nav)
    } else {
        real
    })
}
fn level(series: &Value, block: &Value) -> Result<f64> {
    let series = array(series)?;
    if series.is_empty() {
        return Ok(1.0);
    }
    if block.is_null() {
        return float(&series.last().ok_or_else(|| ty("empty series"))?[1]);
    }
    let block = float(block)?;
    let mut value = 1.0;
    for row in series {
        if float(&row[0])? > block {
            break;
        }
        value = float(&row[1])?;
    }
    Ok(value)
}
fn pricing(data: &Value, hotkey: &str, raw: f64) -> Result<Value> {
    if let Some(value) = data["baselines"].get(hotkey) {
        return Ok(
            json!({"baseline":value[0],"first_block":value[1],"provisional":false,"baseline_rate":value[2]}),
        );
    }
    let level = level(&data["series"], &Value::Null)?;
    let baseline = if level != 0.0 { raw / level } else { raw };
    Ok(
        json!({"baseline":if baseline!=0.0{baseline}else{1.0},"first_block":null,"provisional":true,"baseline_rate":0}),
    )
}
fn yield_value(row: &Value, data: &Value) -> Result<Option<f64>> {
    if row["basket_rate"].is_null() {
        return Ok(None);
    }
    let raw = spot(row)?;
    let p = pricing(data, row["hotkey"].as_str().unwrap_or(""), raw)?;
    Ok(Some(
        (float(&row["basket_rate"])? - float(&p["baseline_rate"])?).max(0.0) * raw,
    ))
}
fn stake_value(row: &Value, data: &Value) -> Result<Option<f64>> {
    let Some(value) = yield_value(row, data)? else {
        return Ok(None);
    };
    let p = pricing(data, row["hotkey"].as_str().unwrap_or(""), spot(row)?)?;
    Ok(Some(
        (1.0 + value) * level(&data["tr_series"], &p["first_block"])?,
    ))
}
fn apply(row: &Value, data: &Value, index: f64, tr_index: f64) -> Result<Value> {
    let chain = &row["chain_pricing"];
    if !chain.is_null() {
        let display = float(&chain["display_price"])?;
        let bag = float(&chain["bag_index"])?;
        let stake_index = float(&chain["stake_index"])?;
        let mut out = json!({"display_price_tao":display,"vs_index":if bag!=0.0{display/bag-1.0}else{0.0},"index_first_block":if n(&chain["first_block"])?==0.0{Value::Null}else{chain["first_block"].clone()},"index_provisional":chain["provisional"],"basket_index":bag,"staker_yield":chain["staker_yield"],"stake_value":chain["stake_price"],"stake_vs_index":if stake_index!=0.0{Some(float(&chain["stake_price"])?/stake_index-1.0)}else{None},"stake_index":stake_index,"staker_twr":chain["staker_twr"]});
        if !row["beta"].is_null() {
            out["display_beta"] = json!(
                float(&row["beta"])?
                    * if display != 0.0 {
                        float(&chain["spot_price"])? / display
                    } else {
                        1.0
                    }
            );
        }
        return Ok(out);
    }
    let raw = spot(row)?;
    let p = pricing(data, row["hotkey"].as_str().unwrap_or(""), raw)?;
    let baseline = float(&p["baseline"])?;
    let display = if baseline != 0.0 { raw / baseline } else { raw };
    let value = stake_value(row, data)?;
    let mut out = json!({"display_price_tao":display,"vs_index":if index!=0.0{display/index-1.0}else{0.0},"index_first_block":p["first_block"],"index_provisional":p["provisional"],"basket_index":index,"staker_yield":yield_value(row,data)?,"stake_value":value,"stake_vs_index":value.filter(|_|tr_index!=0.0).map(|value|value/tr_index-1.0),"stake_index":tr_index});
    if !row["beta"].is_null() {
        out["display_beta"] = json!(float(&row["beta"])? * baseline);
    }
    Ok(out)
}
pub(super) fn evaluate(op: &str, input: &Value) -> Result<Value> {
    let data = &input["data"];
    Ok(match op {
        "spot" => json!(spot(&input["record"])?),
        "level" => json!(level(&input["series"], &input["block"])?),
        "pricing" => pricing(
            data,
            input["hotkey"].as_str().unwrap_or(""),
            float(&input["raw"])?,
        )?,
        "yield" => json!(yield_value(&input["record"], data)?),
        "stake_value" => json!(stake_value(&input["record"], data)?),
        "apply" => apply(
            &input["record"],
            data,
            float(&input["level"])?,
            float(&input["tr_level"])?,
        )?,
        "normalize" => apply(
            &input["record"],
            data,
            level(&data["series"], &Value::Null)?,
            level(&data["tr_series"], &Value::Null)?,
        )?,
        "normalize_many" => {
            let records = array(&input["records"])?;
            let (index, tr_index) = if let Some(chain) = records
                .iter()
                .find_map(|row| row.get("chain_pricing").filter(|v| !v.is_null()))
            {
                (float(&chain["bag_index"])?, float(&chain["stake_index"])?)
            } else {
                let (mut weight, mut total, mut tr_weight, mut tr_total) = (0.0, 0.0, 0.0, 0.0);
                for row in records {
                    let raw = spot(row)?;
                    let p = pricing(data, row["hotkey"].as_str().unwrap_or(""), raw)?;
                    let divisor = float(&p["baseline"])?;
                    let display = if divisor != 0.0 { raw / divisor } else { raw };
                    let mut nav = n(&row["spot_nav_tao"])?;
                    if nav == 0.0 {
                        nav = n(&row["nav_tao"])?;
                    }
                    if nav >= 0.1 && display > 0.0 {
                        weight += nav;
                        total += nav * display;
                        if let Some(value) = stake_value(row, data)? {
                            tr_weight += nav;
                            tr_total += nav * value;
                        }
                    }
                }
                (
                    if weight != 0.0 {
                        total / weight
                    } else {
                        level(&data["series"], &Value::Null)?
                    },
                    if tr_weight != 0.0 {
                        tr_total / tr_weight
                    } else {
                        level(&data["tr_series"], &Value::Null)?
                    },
                )
            };
            json!({"level":index,"records":records.iter().map(|row|apply(row,data,index,tr_index)).collect::<Result<Vec<_>>>()?})
        }
        "age" => {
            if input["first"].is_null() {
                Value::Null
            } else {
                json!((float(&input["current"])? - float(&input["first"])?) / 7200.0)
            }
        }
        _ => return Err(invalid(format!("unknown basket policy operation {op}"))),
    })
}
