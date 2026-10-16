//!
//! Portable JSON-RPC request construction and SCALE response decoding.

pub mod dispatch_error;
pub mod extrinsic_lookup;
pub mod fee;
pub mod metadata;
pub mod receipt;
pub mod runtime_api;
pub mod storage;

/// One JSON-RPC method and its positional arguments, without a transport or ID.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcRequest {
    /// JSON-RPC method to invoke.
    pub method: &'static str,
    /// Positional JSON-RPC arguments.
    pub params: serde_json::Value,
}
