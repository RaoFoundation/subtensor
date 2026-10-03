use frame_support::pallet_macros::pallet_section;

#[pallet_section]
mod hooks {
    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(_block_number: BlockNumberFor<T>) -> Weight {
            Weight::from_parts(0, 0)
        }

        fn on_finalize(_block_number: BlockNumberFor<T>) {}

        fn on_runtime_upgrade() -> Weight {
            // --- Migrate storage
            let mut weight = Weight::from_parts(0, 0);

            weight = weight
                // Cleanup the abandoned V3 prefixes without replaying the obsolete balancer
                // initialization against an already-live PalSwap.
                .saturating_add(
                    migrations::migrate_storage_cleanup_v2::migrate_swap_storage_cleanup_v2::<T>(),
                )
                .saturating_add(
                    migrations::migrate_balancer_to_superellipse::migrate_balancer_to_superellipse::<T>(),
                );
            weight
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            for (netuid, curve) in SwapSuperellipse::<T>::iter() {
                curve
                    .calculate_price(
                        T::AlphaReserve::reserve(netuid).into(),
                        T::TaoReserve::reserve(netuid).into(),
                    )
                    .map_err(|_| "Invalid live superellipse pool")?;
            }
            Ok(())
        }

        #[cfg(feature = "try-runtime")]
        fn pre_upgrade() -> Result<sp_std::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
            migrations::migrate_balancer_to_superellipse::pre_upgrade::<T>()
        }

        #[cfg(feature = "try-runtime")]
        fn post_upgrade(state: sp_std::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
            migrations::migrate_balancer_to_superellipse::post_upgrade::<T>(state)
        }
    }
}
