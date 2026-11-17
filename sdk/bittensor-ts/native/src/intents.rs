use bittensor_core::transaction::IntentCall;
use napi_derive::napi;
use serde_json::Value as JsonValue;

use crate::errors::{CoreResultExt, NapiResult};
use crate::transaction::{NativeClient, NativeIntentCall};
use crate::values::from_wire;

#[napi(js_name = "intentOperation")]
pub fn intent_operation(operation: String, params: JsonValue) -> NapiResult<NativeIntentCall> {
    Ok(NativeIntentCall {
        inner: IntentCall::operation(&operation, from_wire(params)?).napi()?,
    })
}

#[napi(js_name = "intentDescriptor")]
pub fn intent_descriptor(
    client: &NativeClient,
    descriptor: JsonValue,
) -> NapiResult<NativeIntentCall> {
    Ok(NativeIntentCall {
        inner: IntentCall::from_descriptor(client.inner.as_ref(), &from_wire(descriptor)?)
            .napi()?,
    })
}
