use codec::Decode;
use frame_support::{
    dispatch::{DispatchResult, DispatchResultWithPostInfo},
    weights::Weight,
};

/// Read the SCALE collection length prefix without decoding the full storage value.
pub(crate) fn encoded_collection_len(storage_key: &[u8]) -> u32 {
    let mut prefix = [0_u8; 5];
    let Some(encoded_len) = sp_io::storage::read(storage_key, &mut prefix, 0) else {
        return 0;
    };
    let prefix_len = usize::try_from(encoded_len)
        .unwrap_or(prefix.len())
        .min(prefix.len());
    codec::Compact::<u32>::decode(&mut &prefix[..prefix_len])
        .map(|length| length.0)
        .unwrap_or_default()
}

/// Attach measured post-dispatch weight after a successful dispatch.
pub(crate) trait WithBenchmarkWeight {
    fn with_benchmark_weight(self, actual_weight: Weight) -> DispatchResultWithPostInfo;
}

impl WithBenchmarkWeight for DispatchResult {
    fn with_benchmark_weight(self, actual_weight: Weight) -> DispatchResultWithPostInfo {
        self?;
        Ok(Some(actual_weight).into())
    }
}
