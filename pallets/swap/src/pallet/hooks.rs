use frame_support::pallet_macros::pallet_section;

#[pallet_section]
mod hooks {
    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(block_number: BlockNumberFor<T>) -> Weight {
            Weight::from_parts(0, 0)
        }

        fn on_finalize(_block_number: BlockNumberFor<T>) {}

        fn on_runtime_upgrade() -> Weight {
            // --- Migrate storage
            let mut weight = Weight::from_parts(0, 0);

            weight = weight
                // Cleanup uniswap v3 and migrate to balancer
                .saturating_add(
                    migrations::migrate_swapv3_to_balancer::migrate_swapv3_to_balancer::<T>(),
                );
            weight
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Ok(())
        }
    }
}
