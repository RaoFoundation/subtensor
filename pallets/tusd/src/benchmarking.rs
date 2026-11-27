//! Benchmarks for `pallet_tusd`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use frame_benchmarking::v2::*;
use frame_support::traits::{EnsureOrigin, Get};
use frame_system::RawOrigin;
use sp_core::H256;
use sp_runtime::traits::StaticLookup;
use subtensor_runtime_common::{
    Token, TusdBalance,
    rails::{OriginChain, RateWindow, StableAsset},
};

use crate::{Bridge, BridgeByPortal, BridgeRoute, Bridges, Call, Config, Pallet};

const BRIDGE: u32 = 8453;

fn route(domain: u32) -> BridgeRoute {
    BridgeRoute {
        origin: OriginChain::Evm(8453),
        domain,
        portal: H256::repeat_byte(7),
        asset: StableAsset::Usdc,
    }
}

/// An enabled bridge with room for any benchmark amount.
fn open_bridge<T: Config>() {
    let route = route(BRIDGE);
    BridgeByPortal::<T>::insert(route.domain, route.portal, BRIDGE);
    Bridges::<T>::insert(
        BRIDGE,
        Bridge {
            route,
            mint_cap: TusdBalance::MAX,
            mint_window: RateWindow::new(u64::MAX, 1),
            enabled: true,
        },
    );
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn transfer() {
        let caller: T::AccountId = whitelisted_caller();
        let dest: T::AccountId = account("dest", 0, 0);
        let minimum = T::MinBalance::get().to_u64();
        open_bridge::<T>();
        // The sender is left with exactly the minimum and the receiver is created: both
        // balance entries are written.
        Pallet::<T>::mint(BRIDGE, &caller, minimum.saturating_mul(2).into()).unwrap();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller.clone()),
            T::Lookup::unlookup(dest.clone()),
            minimum.into(),
        );

        assert_eq!(Pallet::<T>::balance(&dest), minimum.into());
        assert_eq!(Pallet::<T>::balance(&caller), minimum.into());
    }

    #[benchmark]
    fn set_bridge() -> Result<(), BenchmarkError> {
        let origin =
            T::AdminOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
        // Changing an existing bridge moves its portal index: the costlier path.
        open_bridge::<T>();

        #[extrinsic_call]
        _(
            origin as T::RuntimeOrigin,
            BRIDGE,
            route(1),
            TusdBalance::MAX,
            TusdBalance::MAX,
            300,
        );

        assert_eq!(
            Pallet::<T>::bridge_of(1, H256::repeat_byte(7)),
            Some(BRIDGE)
        );
        assert_eq!(Pallet::<T>::bridge_of(BRIDGE, H256::repeat_byte(7)), None);
        Ok(())
    }

    #[benchmark]
    fn set_bridge_enabled() -> Result<(), BenchmarkError> {
        let origin =
            T::AdminOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;
        open_bridge::<T>();

        #[extrinsic_call]
        _(origin as T::RuntimeOrigin, BRIDGE, false);

        assert!(!Bridges::<T>::get(BRIDGE).unwrap().enabled);
        Ok(())
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
