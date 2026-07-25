//! Smoke compile-test that `#[freeze_struct("…")]` accepts a matching layout hash.

use subtensor_macros::freeze_struct;

#[freeze_struct("ecdcaac0f6da589a")]
pub struct MyStruct {
    pub ip: u32,
    pub port: u32,
}
