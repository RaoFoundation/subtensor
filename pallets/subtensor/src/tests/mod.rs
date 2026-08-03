trait DispatchErrorValue {
    fn dispatch_error(self) -> sp_runtime::DispatchError;
}

impl DispatchErrorValue for sp_runtime::DispatchError {
    fn dispatch_error(self) -> sp_runtime::DispatchError {
        self
    }
}

impl DispatchErrorValue for frame_support::dispatch::DispatchErrorWithPostInfo {
    fn dispatch_error(self) -> sp_runtime::DispatchError {
        self.error
    }
}

macro_rules! assert_dispatch_err {
    ($call:expr, $error:expr $(,)?) => {{
        let actual = match $call {
            Ok(_) => panic!("expected dispatch error"),
            Err(error) => crate::tests::DispatchErrorValue::dispatch_error(error),
        };
        let expected: sp_runtime::DispatchError = $error.into();
        assert_eq!(actual, expected);
    }};
}

/// Assert a post-info dispatch error without discarding the storage no-op
/// guarantee. Variable-weight dispatches legitimately attach `actual_weight`
/// to errors, so FRAME's `assert_noop!` is too strict for these calls.
macro_rules! assert_noop_ignore_postinfo {
    ($call:expr, $error:expr $(,)?) => {{
        let storage_root =
            frame_support::__private::storage_root(frame_support::__private::StateVersion::V1);
        assert_dispatch_err!($call, $error);
        assert_eq!(
            storage_root,
            frame_support::__private::storage_root(frame_support::__private::StateVersion::V1),
            "storage has been mutated"
        );
    }};
}

mod auto_stake_hotkey;
mod batch_tx;
mod children;
mod claim_root;
mod cleanup_tests;
mod coinbase;
mod coldkey_lineage;
mod consensus;
mod delegate_info;
mod destroy_alpha_tests;
mod dissolution;
mod emission;
mod ensure;
mod epoch;
mod epoch_logs;
mod evm;
mod hotkey_lineage;
mod leasing;
mod locks;
mod math;
mod mechanism;
mod migration;
pub(crate) mod mock;
pub(crate) mod mock_high_ed;
mod move_stake;
mod networks;
mod neuron_info;
mod recycle_alpha;
mod registration;
mod remove_data_tests;
mod serving;
mod staking;
mod staking2;
mod subnet;
mod subnet_emissions;
mod subnet_info;
mod swap_coldkey;
mod swap_hotkey;
mod swap_hotkey_with_subnet;
mod tao;
mod tempo_control;
mod uids;
mod voting_power;
mod weights;
