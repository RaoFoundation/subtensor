//! Read-only views of the pool for clients.

use subtensor_runtime_common::{TaoBalance, TusdBalance};

sp_api::decl_runtime_apis! {
    pub trait CanonicalPoolApi {
        /// The TAO that `amount` tUSD buys now, after the fee. `None` if the pool cannot
        /// make the swap.
        fn quote_tusd_for_tao(amount: TusdBalance) -> Option<TaoBalance>;

        /// The tUSD that `amount` TAO buys now, after the fee. `None` if the pool cannot
        /// make the swap.
        fn quote_tao_for_tusd(amount: TaoBalance) -> Option<TusdBalance>;

        /// tUSD per TAO from the reserves, times 1e9. `None` while the pool is empty.
        fn spot_price() -> Option<u64>;

        /// tUSD per TAO averaged over the last period, times 1e9. `None` until the pool
        /// has a full period of history.
        fn twap_price() -> Option<u64>;
    }
}
