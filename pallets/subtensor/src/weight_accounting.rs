use codec::Decode;
use frame_support::{
    dispatch::{
        DispatchErrorWithPostInfo, DispatchResult, DispatchResultWithPostInfo, PostDispatchInfo,
    },
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
    let mut encoded_prefix = prefix.get(..prefix_len).unwrap_or_default();
    codec::Compact::<u32>::decode(&mut encoded_prefix)
        .map(|length| length.0)
        .unwrap_or_default()
}

/// Attach measured post-dispatch weight regardless of dispatch outcome.
pub(crate) trait WithBenchmarkWeight {
    fn with_benchmark_weight(self, actual_weight: Weight) -> DispatchResultWithPostInfo;
}

impl WithBenchmarkWeight for DispatchResult {
    fn with_benchmark_weight(self, actual_weight: Weight) -> DispatchResultWithPostInfo {
        let post_info = PostDispatchInfo {
            actual_weight: Some(actual_weight),
            ..Default::default()
        };
        match self {
            Ok(()) => Ok(post_info),
            Err(error) => Err(DispatchErrorWithPostInfo { post_info, error }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WithBenchmarkWeight;
    use frame_support::{
        dispatch::{DispatchResult, Pays},
        weights::Weight,
    };
    use sp_runtime::DispatchError;

    #[test]
    fn attaches_actual_weight_to_successful_dispatch() {
        let actual_weight = Weight::from_parts(123, 456);
        let result: DispatchResult = Ok(());

        let post_info = match result.with_benchmark_weight(actual_weight) {
            Ok(post_info) => post_info,
            Err(_) => panic!("dispatch must remain successful"),
        };

        assert_eq!(post_info.actual_weight, Some(actual_weight));
        assert_eq!(post_info.pays_fee, Pays::Yes);
    }

    #[test]
    fn attaches_actual_weight_to_failed_dispatch() {
        let actual_weight = Weight::from_parts(123, 456);
        let result: DispatchResult = Err(DispatchError::Other("expected failure"));

        let error = match result.with_benchmark_weight(actual_weight) {
            Err(error) => error,
            Ok(_) => panic!("dispatch must remain failed"),
        };

        assert_eq!(error.post_info.actual_weight, Some(actual_weight));
        assert_eq!(error.post_info.pays_fee, Pays::Yes);
        assert_eq!(error.error, DispatchError::Other("expected failure"));
    }

    #[test]
    fn attaches_zero_actual_weight() {
        let result: DispatchResult = Ok(());

        let post_info = match result.with_benchmark_weight(Weight::zero()) {
            Ok(post_info) => post_info,
            Err(_) => panic!("dispatch must remain successful"),
        };

        assert_eq!(post_info.actual_weight, Some(Weight::zero()));
    }

    #[test]
    fn attaches_max_actual_weight_without_clamping() {
        let result: DispatchResult = Ok(());

        let post_info = match result.with_benchmark_weight(Weight::MAX) {
            Ok(post_info) => post_info,
            Err(_) => panic!("dispatch must remain successful"),
        };

        assert_eq!(post_info.actual_weight, Some(Weight::MAX));
    }
}
