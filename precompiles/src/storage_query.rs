//! Raw Substrate storage read precompile with an allow-listed pallet-prefix set.
//!
//! Calldata must begin with one of the Twox128 pallet prefixes in
//! [`AUTHORIZED_STORAGE_PREFIXES`]; other keys revert with `Invalid key`.
//! Returns the raw SCALE value bytes, or empty output when the key is absent.
//! INDEX is frozen.

use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use frame_support::dispatch::{GetDispatchInfo, PostDispatchInfo};
use pallet_evm::{ExitSucceed, Precompile, PrecompileHandle, PrecompileOutput};
use precompile_utils::prelude::PrecompileHandleExt;
use sp_runtime::traits::{Dispatchable, StaticLookup};
use sp_std::vec::Vec;

const AUTHORIZED_PREFIXES: [[u8; 16]; 10] = [
    // twox_128("SubtensorModule")
    [
        0x65, 0x8f, 0xaa, 0x38, 0x50, 0x70, 0xe0, 0x74, 0xc8, 0x5b, 0xf6, 0xb5, 0x68, 0xcf, 0x05,
        0x55,
    ],
    // twox_128("Swap")
    [
        0x74, 0x6c, 0xc6, 0xd1, 0xe9, 0xdb, 0xcf, 0x1d, 0x03, 0x86, 0x8e, 0x49, 0x2c, 0x1d, 0x83,
        0x6e,
    ],
    // twox_128("Balances")
    [
        0xc2, 0x26, 0x12, 0x76, 0xcc, 0x9d, 0x1f, 0x85, 0x98, 0xea, 0x4b, 0x6a, 0x74, 0xb1, 0x5c,
        0x2f,
    ],
    // twox_128("Proxy")
    [
        0x18, 0x09, 0xd7, 0x83, 0x46, 0x72, 0x7a, 0x0e, 0xf5, 0x8c, 0x0f, 0xa0, 0x3b, 0xaf, 0xa3,
        0x23,
    ],
    // twox_128("Scheduler")
    [
        0x3d, 0xb7, 0xa2, 0x4c, 0xfd, 0xc9, 0xde, 0x78, 0x59, 0x74, 0x74, 0x6c, 0x14, 0xa9, 0x9d,
        0xf9,
    ],
    // twox_128("Drand")
    [
        0xa2, 0x85, 0xcd, 0xb6, 0x6e, 0x8b, 0x85, 0x24, 0xea, 0x70, 0xb1, 0x69, 0x3c, 0x7b, 0x1e,
        0x05,
    ],
    // twox_128("Crowdloan")
    [
        0x3d, 0x9c, 0xad, 0x2b, 0xaf, 0x70, 0x2e, 0x20, 0xb1, 0x36, 0xf4, 0xc8, 0x90, 0x0c, 0xd8,
        0x02,
    ],
    // twox_128("Sudo")
    [
        0x5c, 0x0d, 0x11, 0x76, 0xa5, 0x68, 0xc1, 0xf9, 0x29, 0x44, 0x34, 0x0d, 0xbf, 0xed, 0x9e,
        0x9c,
    ],
    // twox_128("Multisig")
    [
        0x74, 0x74, 0x44, 0x9c, 0xca, 0x95, 0xdc, 0x5d, 0x0c, 0x00, 0xe7, 0x17, 0x35, 0xa6, 0xd1,
        0x7d,
    ],
    // twox_128("Timestamp")
    [
        0xf0, 0xc3, 0x65, 0xc3, 0xcf, 0x59, 0xd6, 0x71, 0xeb, 0x72, 0xda, 0x0e, 0x7a, 0x41, 0x13,
        0xc4,
    ],
];

use crate::PrecompileExt;

/// EVM precompile that reads allow-listed FRAME storage keys by raw key bytes (INDEX 2055).
pub struct StorageQueryPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for StorageQueryPrecompile<R>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeCall:
        GetDispatchInfo + Dispatchable<PostInfo = PostDispatchInfo>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<PostInfo = PostDispatchInfo>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    const INDEX: u64 = 2055;
}

impl<R> Precompile for StorageQueryPrecompile<R>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeCall:
        GetDispatchInfo + Dispatchable<PostInfo = PostDispatchInfo>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<PostInfo = PostDispatchInfo>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    fn execute(handle: &mut impl PrecompileHandle) -> fp_evm::PrecompileResult {
        let input = handle.input();

        if !AUTHORIZED_PREFIXES
            .iter()
            .any(|prefix| input.starts_with(prefix))
        {
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("Invalid key".into()),
            });
        }

        match sp_io::storage::get(input) {
            Some(value) => {
                handle.record_db_read::<R>(value.len())?;

                Ok(PrecompileOutput {
                    exit_status: ExitSucceed::Returned,
                    output: value.to_vec(),
                })
            }
            None => {
                // We still record a read of 1 byte to avoid spamming.
                handle.record_db_read::<R>(1)?;

                Ok(PrecompileOutput {
                    exit_status: ExitSucceed::Returned,
                    output: Vec::new(),
                })
            }
        }
    }
}
