use bittensor_core::CoreError;
use napi::{Error, Status};

pub type NapiResult<T> = napi::Result<T>;

pub fn into_napi(error: CoreError) -> Error {
    let (code, status, message) = match error {
        CoreError::Keyfile(message) => ("KEYFILE", Status::GenericFailure, message),
        CoreError::WrongPassword(message) => ("WRONG_PASSWORD", Status::GenericFailure, message),
        CoreError::NotInRuntime(message) => ("NOT_IN_RUNTIME", Status::InvalidArg, message),
        CoreError::Codec(message) => ("CODEC", Status::InvalidArg, message),
        CoreError::Crypto(message) => ("CRYPTO", Status::InvalidArg, message),
        CoreError::Device(message) => ("DEVICE", Status::GenericFailure, message),
    };
    Error::new(status, format!("[BITTENSOR_CORE:{code}] {message}"))
}

pub fn invalid_arg(message: impl Into<String>) -> Error {
    Error::new(
        Status::InvalidArg,
        format!("[BITTENSOR_CORE:CODEC] {}", message.into()),
    )
}

pub trait CoreResultExt<T> {
    fn napi(self) -> NapiResult<T>;
}

impl<T> CoreResultExt<T> for Result<T, CoreError> {
    fn napi(self) -> NapiResult<T> {
        self.map_err(into_napi)
    }
}
