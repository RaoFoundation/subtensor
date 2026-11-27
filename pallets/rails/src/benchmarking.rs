//! Benchmarks for `pallet_rails`.
//!
//! They measure the hub's own work. The staking that `execute`, `bridge_out` and
//! `open_market` reach through [`RailsStaking`] is not measured here: those calls add
//! `RailsStaking::weight()`, built from the measured weights of the pallet behind it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]

use frame_benchmarking::v2::*;
use frame_support::traits::{EnsureOrigin, Get, fungible::Mutate};
use frame_system::RawOrigin;
use sp_core::{H160, H256};
use subtensor_runtime_common::{
    AlphaBalance, NetUid, TaoBalance, TusdBalance,
    rails::{BridgeId, OriginChain, StableAsset},
};

use crate::{
    Call, Config, CurrencyOf, Destination, Gateway, Guardian, Market, Markets, Message,
    NativeAsset, NextOrder, Order, OutboxBounds, Outcome, Pallet, Paused, RailsStaking, Receipt,
    Receipts, SpokeAsset, TaoCap, TaoSupply, Tokens,
};

const UNIT: u64 = 1_000_000_000;
/// A bridge that only the benchmarks use.
const BRIDGE: BridgeId = 4_000_000_000;
const DOMAIN: u32 = 4_000_000_000;

fn portal() -> H256 {
    H256::repeat_byte(0xB7)
}

fn wallet() -> H160 {
    H160::repeat_byte(0xC3)
}

fn admin<T: Config>() -> Result<T::RuntimeOrigin, BenchmarkError> {
    <T as Config>::AdminOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)
}

/// A live bridge, a gateway, canonical TAO on the bridge's spoke, and a pool to trade in.
/// Returns the gateway.
fn rails<T: Config>() -> Result<T::AccountId, BenchmarkError> {
    let origin = <T as pallet_tusd::Config>::AdminOrigin::try_successful_origin()
        .map_err(|_| BenchmarkError::Weightless)?;
    pallet_tusd::Pallet::<T>::set_bridge(
        origin.clone(),
        BRIDGE,
        pallet_tusd::BridgeRoute {
            origin: OriginChain::Evm(8453),
            domain: DOMAIN,
            portal: portal(),
            asset: StableAsset::Usdc,
        },
        TusdBalance::from(u64::MAX / 2),
        TusdBalance::from(u64::MAX / 2),
        1,
    )?;
    pallet_tusd::Pallet::<T>::set_bridge_enabled(origin, BRIDGE, true)?;

    let provider: T::AccountId = account("provider", 0, 0);
    pallet_canonical_pool::Pallet::<T>::benchmark_seed(&provider)?;

    let gateway: T::AccountId = account("gateway", 0, 0);
    Gateway::<T>::put(&gateway);
    Pallet::<T>::register_token(admin::<T>()?, BRIDGE, SpokeAsset::Tao, wallet())?;
    TaoCap::<T>::insert(BRIDGE, TaoBalance::from(u64::MAX / 2));

    // The next order drops the oldest receipt kept.
    let kept = T::ReceiptsKept::get();
    NextOrder::<T>::insert(BRIDGE, kept);
    Receipts::<T>::insert(
        BRIDGE,
        0,
        Receipt {
            block: 0,
            outcome: Outcome::Skipped,
        },
    );
    Ok(gateway)
}

#[benchmarks]
mod benchmarks {
    use super::*;

    /// The stablecoin for canonical TAO: a mint, a swap, a move to the reserve, a message
    /// and a receipt.
    #[benchmark]
    fn execute() -> Result<(), BenchmarkError> {
        let gateway = rails::<T>()?;
        let order = Order {
            nonce: NextOrder::<T>::get(BRIDGE),
            payer: wallet(),
            source: SpokeAsset::Stable,
            amount: 4_000 * UNIT,
            destination: Destination::Tao { to: wallet() },
            min_out: 1,
            deadline: u64::MAX,
        };

        #[extrinsic_call]
        _(RawOrigin::Signed(gateway), DOMAIN, portal(), order);

        assert!(TaoSupply::<T>::get(BRIDGE) > TaoBalance::from(UNIT));
        assert!(!Receipts::<T>::contains_key(BRIDGE, 0));
        Ok(())
    }

    #[benchmark]
    fn ack_outbound() -> Result<(), BenchmarkError> {
        let gateway = rails::<T>()?;
        Pallet::<T>::enqueue(
            BRIDGE,
            Message::Credit {
                asset: SpokeAsset::Tao,
                to: wallet(),
                amount: UNIT,
                order: Some(0),
                refund: false,
                rate: 0,
            },
        )?;
        let (head, tail) = OutboxBounds::<T>::get();

        #[extrinsic_call]
        _(RawOrigin::Signed(gateway), head);

        assert_eq!(OutboxBounds::<T>::get(), (head + 1, tail));
        Ok(())
    }

    /// TAO for the stablecoin: a swap, a burn at the bridge and a message.
    #[benchmark]
    fn bridge_out() -> Result<(), BenchmarkError> {
        rails::<T>()?;
        let caller: T::AccountId = account("caller", 0, 0);
        CurrencyOf::<T>::set_balance(&caller, TaoBalance::from(100 * UNIT));
        // The bridge releases only what it brought in.
        pallet_tusd::Pallet::<T>::mint(BRIDGE, &caller, TusdBalance::from(100_000 * UNIT))?;
        let (_, tail) = OutboxBounds::<T>::get();

        #[extrinsic_call]
        _(
            RawOrigin::Signed(caller),
            BRIDGE,
            NativeAsset::Tao,
            10 * UNIT,
            Destination::Stable { to: wallet() },
            1,
        );

        assert_eq!(OutboxBounds::<T>::get().1, tail + 1);
        Ok(())
    }

    #[benchmark]
    fn set_gateway() -> Result<(), BenchmarkError> {
        let gateway: T::AccountId = account("gateway", 0, 0);

        #[extrinsic_call]
        _(admin::<T>()? as T::RuntimeOrigin, Some(gateway.clone()));

        assert_eq!(Gateway::<T>::get(), Some(gateway));
        Ok(())
    }

    #[benchmark]
    fn set_guardian() -> Result<(), BenchmarkError> {
        let guardian: T::AccountId = account("guardian", 0, 0);

        #[extrinsic_call]
        _(admin::<T>()? as T::RuntimeOrigin, Some(guardian.clone()));

        assert_eq!(Guardian::<T>::get(), Some(guardian));
        Ok(())
    }

    #[benchmark]
    fn set_paused() -> Result<(), BenchmarkError> {
        #[extrinsic_call]
        _(admin::<T>()? as T::RuntimeOrigin, true);

        assert!(Paused::<T>::get());
        Ok(())
    }

    #[benchmark]
    fn register_token() -> Result<(), BenchmarkError> {
        rails::<T>()?;

        #[extrinsic_call]
        _(
            admin::<T>()? as T::RuntimeOrigin,
            BRIDGE,
            SpokeAsset::Stable,
            wallet(),
        );

        assert!(Tokens::<T>::contains_key(BRIDGE, SpokeAsset::Stable));
        Ok(())
    }

    #[benchmark]
    fn set_tao_cap() -> Result<(), BenchmarkError> {
        #[extrinsic_call]
        _(
            admin::<T>()? as T::RuntimeOrigin,
            BRIDGE,
            TaoBalance::from(UNIT),
        );

        assert_eq!(TaoCap::<T>::get(BRIDGE), TaoBalance::from(UNIT));
        Ok(())
    }

    #[benchmark]
    fn open_market() -> Result<(), BenchmarkError> {
        let netuid = T::Staking::benchmark_subnet();

        #[extrinsic_call]
        _(
            admin::<T>()? as T::RuntimeOrigin,
            netuid,
            AlphaBalance::from(UNIT),
        );

        assert!(Markets::<T>::contains_key(netuid));
        Ok(())
    }

    #[benchmark]
    fn list_market() -> Result<(), BenchmarkError> {
        rails::<T>()?;
        let netuid = NetUid::from(1);
        Markets::<T>::insert(
            netuid,
            Market {
                generation: 0,
                shares: 0,
                cap: AlphaBalance::from(UNIT),
            },
        );

        #[extrinsic_call]
        _(admin::<T>()? as T::RuntimeOrigin, netuid, BRIDGE, wallet());

        assert!(Tokens::<T>::contains_key(BRIDGE, SpokeAsset::Alpha(netuid)));
        Ok(())
    }

    #[benchmark]
    fn set_market_cap() -> Result<(), BenchmarkError> {
        let netuid = NetUid::from(1);
        Markets::<T>::insert(
            netuid,
            Market {
                generation: 0,
                shares: 0,
                cap: AlphaBalance::from(UNIT),
            },
        );

        #[extrinsic_call]
        _(
            admin::<T>()? as T::RuntimeOrigin,
            netuid,
            AlphaBalance::from(2 * UNIT),
        );

        assert_eq!(
            Markets::<T>::get(netuid).map(|market| market.cap),
            Some(AlphaBalance::from(2 * UNIT))
        );
        Ok(())
    }

    #[benchmark]
    fn skip_order() -> Result<(), BenchmarkError> {
        rails::<T>()?;
        let nonce = NextOrder::<T>::get(BRIDGE);

        #[extrinsic_call]
        _(admin::<T>()? as T::RuntimeOrigin, BRIDGE, nonce);

        assert_eq!(NextOrder::<T>::get(BRIDGE), nonce + 1);
        Ok(())
    }

    impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
