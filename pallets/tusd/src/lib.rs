//! # tUSD
//!
//! tUSD is the canonical dollar on Bittensor: a native, transferable balance with 9 decimals.
//!
//! It exists only against stablecoins locked on another chain. Governance keeps a list of
//! approved [`Bridge`]s; each one mints tUSD when its portal locks a stablecoin and burns
//! tUSD when the portal releases it. There is no other way to create or destroy tUSD, and
//! there is no extrinsic that mints or burns: the bridge transport calls [`Pallet::mint`]
//! and [`Pallet::burn`] from inside the runtime.
//!
//! Each bridge is bounded on its own:
//!
//! - it can never have more than its `mint_cap` outstanding;
//! - it can mint only as fast as its `mint_window` refills;
//! - it can burn, and so release on its origin chain, only what it has outstanding.
//!
//! A bridge that is compromised can therefore mint at most its own cap, and can be drained
//! of at most its own locked stablecoins.
//!
//! ## Invariants
//!
//! 1. [`TotalIssuance`] equals the sum of [`Balances`].
//! 2. [`TotalIssuance`] equals the sum of [`BridgeOutstanding`].
//! 3. A mint never takes `BridgeOutstanding(b)` above `Bridges(b).mint_cap`.
//! 4. An account holds nothing, or at least [`Config::MinBalance`].

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{pallet_prelude::*, storage::with_storage_layer};
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{ArithmeticError, traits::StaticLookup};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{
    Token, TusdBalance,
    rails::{BridgeId, OriginChain, RateWindow, StableAsset},
};

pub use pallet::*;
pub use weights::WeightInfo;

/// Where a bridge's portal lives and what it locks.
#[freeze_struct("cbb41a94e58b4621")]
#[derive(
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct BridgeRoute {
    /// The chain the portal lives on.
    pub origin: OriginChain,
    /// The transport's identifier of that chain (its Hyperlane domain).
    pub domain: u32,
    /// The portal contract on the origin chain, as a 32-byte transport address.
    pub portal: H256,
    /// The stablecoin the portal locks.
    pub asset: StableAsset,
}

/// An approved bridge: one portal contract that locks a stablecoin on its origin chain.
#[freeze_struct("e06f6f55571d746b")]
#[derive(
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Encode,
    Eq,
    MaxEncodedLen,
    PartialEq,
    RuntimeDebug,
    TypeInfo,
)]
pub struct Bridge {
    /// Where the portal lives and what it locks.
    pub route: BridgeRoute,
    /// The most tUSD this bridge may have outstanding. A cap below what is outstanding
    /// winds the bridge down: it can burn but no longer mint.
    pub mint_cap: TusdBalance,
    /// How fast this bridge may mint.
    pub mint_window: RateWindow,
    /// A disabled bridge neither mints nor burns.
    pub enabled: bool,
}

// The FRAME macros expand to `expect` on infallible codec paths.
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Registers bridges, changes their terms and enables them.
        type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// May disable a bridge, and nothing else.
        type PauseOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// The least an account may hold, other than nothing. Keeps dust accounts out of state.
        #[pallet::constant]
        type MinBalance: Get<TusdBalance>;

        /// Weights of this pallet's calls.
        type WeightInfo: WeightInfo;
    }

    /// MAP ( account ) --> tUSD | What each account holds.
    #[pallet::storage]
    pub type Balances<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, TusdBalance, ValueQuery>;

    /// ITEM --> tUSD | All tUSD in existence.
    #[pallet::storage]
    pub type TotalIssuance<T> = StorageValue<_, TusdBalance, ValueQuery>;

    /// MAP ( bridge ) --> Bridge | The approved bridges.
    #[pallet::storage]
    pub type Bridges<T> = StorageMap<_, Twox64Concat, BridgeId, Bridge, OptionQuery>;

    /// MAP ( bridge ) --> tUSD | What each bridge has minted and not yet burned. Its portal
    /// holds at least this much of its stablecoin.
    #[pallet::storage]
    pub type BridgeOutstanding<T> = StorageMap<_, Twox64Concat, BridgeId, TusdBalance, ValueQuery>;

    /// DMAP ( domain, portal ) --> bridge | Finds the bridge a transport message came from.
    #[pallet::storage]
    pub type BridgeByPortal<T> =
        StorageDoubleMap<_, Twox64Concat, u32, Blake2_128Concat, H256, BridgeId, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// tUSD moved between two accounts.
        Transferred {
            from: T::AccountId,
            to: T::AccountId,
            amount: TusdBalance,
        },
        /// A bridge minted tUSD against a stablecoin it locked.
        Minted {
            bridge: BridgeId,
            to: T::AccountId,
            amount: TusdBalance,
        },
        /// A bridge burned tUSD to release a stablecoin.
        Burned {
            bridge: BridgeId,
            from: T::AccountId,
            amount: TusdBalance,
        },
        /// A bridge was registered or its terms changed.
        BridgeSet { id: BridgeId, bridge: Bridge },
        /// A bridge was enabled or disabled.
        BridgeEnabledSet { id: BridgeId, enabled: bool },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The account does not hold that much tUSD.
        InsufficientBalance,
        /// The transfer would leave an account holding less than the minimum balance.
        BelowMinimumBalance,
        /// No bridge has this id.
        BridgeUnknown,
        /// The bridge is disabled.
        BridgeDisabled,
        /// The mint would put the bridge over its cap.
        MintCapExceeded,
        /// The bridge has minted as much as its window allows for now.
        MintRateLimited,
        /// The bridge cannot burn more than it has outstanding.
        RedeemExceedsOutstanding,
        /// Another bridge already uses this portal.
        PortalAlreadyRegistered,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Sends `amount` of the signer's tUSD to `dest`.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::transfer())]
        pub fn transfer(
            origin: OriginFor<T>,
            dest: <T::Lookup as StaticLookup>::Source,
            amount: TusdBalance,
        ) -> DispatchResult {
            let from = ensure_signed(origin)?;
            let to = T::Lookup::lookup(dest)?;
            Self::do_transfer(&from, &to, amount)
        }

        /// Registers bridge `id`, or changes its terms. A new bridge starts disabled. A
        /// changed bridge keeps what it has outstanding, what its window has consumed and
        /// whether it is enabled.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::set_bridge())]
        pub fn set_bridge(
            origin: OriginFor<T>,
            id: BridgeId,
            route: BridgeRoute,
            mint_cap: TusdBalance,
            window_limit: TusdBalance,
            window_period: u32,
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            ensure!(
                BridgeByPortal::<T>::get(route.domain, route.portal)
                    .is_none_or(|owner| owner == id),
                Error::<T>::PortalAlreadyRegistered
            );
            let previous = Bridges::<T>::get(id);
            let bridge = Bridge {
                route,
                mint_cap,
                mint_window: previous.map_or(
                    RateWindow::new(window_limit.to_u64(), window_period),
                    |previous| {
                        previous.mint_window.retargeted(
                            Self::now(),
                            window_limit.to_u64(),
                            window_period,
                        )
                    },
                ),
                enabled: previous.is_some_and(|previous| previous.enabled),
            };
            if let Some(previous) = previous {
                BridgeByPortal::<T>::remove(previous.route.domain, previous.route.portal);
            }
            BridgeByPortal::<T>::insert(route.domain, route.portal, id);
            Bridges::<T>::insert(id, bridge);
            Self::deposit_event(Event::BridgeSet { id, bridge });
            Ok(())
        }

        /// Enables or disables bridge `id`. The pause origin may only disable.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::set_bridge_enabled())]
        pub fn set_bridge_enabled(
            origin: OriginFor<T>,
            id: BridgeId,
            enabled: bool,
        ) -> DispatchResult {
            let may_pause = !enabled && T::PauseOrigin::try_origin(origin.clone()).is_ok();
            if !may_pause {
                T::AdminOrigin::ensure_origin(origin)?;
            }
            Bridges::<T>::try_mutate(id, |bridge| {
                bridge.as_mut().ok_or(Error::<T>::BridgeUnknown)?.enabled = enabled;
                Ok::<(), Error<T>>(())
            })?;
            Self::deposit_event(Event::BridgeEnabledSet { id, enabled });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    /// What `who` holds.
    pub fn balance(who: &T::AccountId) -> TusdBalance {
        Balances::<T>::get(who)
    }

    /// The bridge whose portal is `portal` on transport domain `domain`.
    pub fn bridge_of(domain: u32, portal: H256) -> Option<BridgeId> {
        BridgeByPortal::<T>::get(domain, portal)
    }

    /// Moves `amount` from `from` to `to`.
    pub fn do_transfer(
        from: &T::AccountId,
        to: &T::AccountId,
        amount: TusdBalance,
    ) -> DispatchResult {
        with_storage_layer(|| {
            Self::debit(from, amount)?;
            Self::credit(to, amount)
        })?;
        Self::deposit_event(Event::Transferred {
            from: from.clone(),
            to: to.clone(),
            amount,
        });
        Ok(())
    }

    /// Mints `amount` to `to` against a stablecoin that `bridge` locked on its origin chain.
    ///
    /// The caller is the bridge transport: it must have verified that the portal of this
    /// very bridge reported the lock.
    pub fn mint(bridge: BridgeId, to: &T::AccountId, amount: TusdBalance) -> DispatchResult {
        with_storage_layer(|| {
            let mut terms = Bridges::<T>::get(bridge).ok_or(Error::<T>::BridgeUnknown)?;
            ensure!(terms.enabled, Error::<T>::BridgeDisabled);
            let outstanding = BridgeOutstanding::<T>::get(bridge)
                .to_u64()
                .checked_add(amount.to_u64())
                .ok_or(ArithmeticError::Overflow)?;
            ensure!(
                outstanding <= terms.mint_cap.to_u64(),
                Error::<T>::MintCapExceeded
            );
            ensure!(
                terms.mint_window.try_consume(Self::now(), amount.to_u64()),
                Error::<T>::MintRateLimited
            );
            let issuance = TotalIssuance::<T>::get()
                .to_u64()
                .checked_add(amount.to_u64())
                .ok_or(ArithmeticError::Overflow)?;
            Bridges::<T>::insert(bridge, terms);
            BridgeOutstanding::<T>::insert(bridge, TusdBalance::from(outstanding));
            TotalIssuance::<T>::put(TusdBalance::from(issuance));
            Self::credit(to, amount)
        })?;
        Self::deposit_event(Event::Minted {
            bridge,
            to: to.clone(),
            amount,
        });
        Ok(())
    }

    /// Burns `amount` from `from` so that `bridge` releases as much of its stablecoin on its
    /// origin chain. A bridge can burn only what it has outstanding.
    pub fn burn(bridge: BridgeId, from: &T::AccountId, amount: TusdBalance) -> DispatchResult {
        with_storage_layer(|| {
            let terms = Bridges::<T>::get(bridge).ok_or(Error::<T>::BridgeUnknown)?;
            ensure!(terms.enabled, Error::<T>::BridgeDisabled);
            let outstanding = BridgeOutstanding::<T>::get(bridge)
                .to_u64()
                .checked_sub(amount.to_u64())
                .ok_or(Error::<T>::RedeemExceedsOutstanding)?;
            BridgeOutstanding::<T>::insert(bridge, TusdBalance::from(outstanding));
            TotalIssuance::<T>::mutate(|issuance| *issuance = issuance.saturating_sub(amount));
            Self::debit(from, amount)
        })?;
        Self::deposit_event(Event::Burned {
            bridge,
            from: from.clone(),
            amount,
        });
        Ok(())
    }

    fn credit(who: &T::AccountId, amount: TusdBalance) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        Balances::<T>::try_mutate(who, |balance| {
            let next = balance
                .to_u64()
                .checked_add(amount.to_u64())
                .ok_or(ArithmeticError::Overflow)?;
            ensure!(
                next >= T::MinBalance::get().to_u64(),
                Error::<T>::BelowMinimumBalance
            );
            *balance = next.into();
            Ok(())
        })
    }

    fn debit(who: &T::AccountId, amount: TusdBalance) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        Balances::<T>::try_mutate_exists(who, |balance| {
            let next = balance
                .unwrap_or_default()
                .to_u64()
                .checked_sub(amount.to_u64())
                .ok_or(Error::<T>::InsufficientBalance)?;
            ensure!(
                next == 0 || next >= T::MinBalance::get().to_u64(),
                Error::<T>::BelowMinimumBalance
            );
            *balance = (next != 0).then(|| next.into());
            Ok(())
        })
    }

    fn now() -> u32 {
        frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// Checks that every tUSD in existence is outstanding at exactly one bridge.
    #[cfg(any(feature = "try-runtime", test))]
    pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
        let mut outstanding = 0u128;
        for (id, amount) in BridgeOutstanding::<T>::iter() {
            ensure!(
                Bridges::<T>::contains_key(id),
                "outstanding tUSD without a bridge"
            );
            outstanding = outstanding.saturating_add(u128::from(amount.to_u64()));
        }
        ensure!(
            outstanding == u128::from(TotalIssuance::<T>::get().to_u64()),
            "tUSD issuance differs from what the bridges have outstanding"
        );
        Ok(())
    }
}
