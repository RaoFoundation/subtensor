use super::{count, invalid, json, Result, Value};
use crate::mlkem;

pub(super) fn evaluate(operation: &str, input: &Value) -> Result<Value> {
    if operation == "constants" {
        return Ok(json!({
            "overhead": mlkem::SHIELD_ENCRYPTION_OVERHEAD,
            "maximum": mlkem::MAX_SHIELDED_CIPHERTEXT_BYTES,
        }));
    }
    let size = usize::try_from(count(&input["size"])?)
        .map_err(|_| invalid("Shield size exceeds platform range"))?;
    let result = match operation {
        "ciphertext_size" => mlkem::shielded_ciphertext_size(size),
        "validate_ciphertext" => mlkem::validate_shielded_ciphertext_size(size),
        _ => return Err(invalid("unknown Shield size policy")),
    };
    result
        .map(|size| json!(size))
        .map_err(|error| invalid(error.to_string()))
}
