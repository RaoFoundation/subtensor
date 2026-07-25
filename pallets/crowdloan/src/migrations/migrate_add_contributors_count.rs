//! Backfill [`crate::CrowdloanInfo::contributors_count`] onto pre-existing crowdloans.
//!
//! Reads the previous layout ([`old_storage::OldCrowdloanInfo`], without the count
//! field), counts [`crate::Contributions`] keys per id, and rewrites
//! [`crate::Crowdloans`]. Idempotent via migration name `migrate_add_contributors_count`.

use alloc::string::String;
use frame_support::{BoundedVec, migration::storage_key_iter, traits::Get, weights::Weight};
use subtensor_macros::freeze_struct;

use crate::*;

/// Pre-migration SCALE layout of crowdloan info (no `contributors_count` field).
mod old_storage {
    use super::*;

    #[freeze_struct("84bcbf9b8d3f0ddf")]
    #[derive(Encode, Decode, Debug)]
    pub struct OldCrowdloanInfo<AccountId, Balance, BlockNumber, Call> {
        pub creator: AccountId,
        pub deposit: Balance,
        pub min_contribution: Balance,
        pub end: BlockNumber,
        pub cap: Balance,
        pub funds_account: AccountId,
        pub raised: Balance,
        pub target_address: Option<AccountId>,
        pub call: Option<Call>,
        pub finalized: bool,
    }
}

/// Populate `contributors_count` from contribution map cardinality; skip if already run.
pub fn migrate_add_contributors_count<T: Config>() -> Weight {
    let migration_name = BoundedVec::truncate_from(b"migrate_add_contributors_count".to_vec());
    let mut weight = T::DbWeight::get().reads(1);

    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            "Migration '{:?}' has already run. Skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }

    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    let pallet_name = b"Crowdloan";
    let item_name = b"Crowdloans";
    let crowdloans = storage_key_iter::<
        CrowdloanId,
        old_storage::OldCrowdloanInfo<
            T::AccountId,
            BalanceOf<T>,
            BlockNumberFor<T>,
            BoundedCallOf<T>,
        >,
        Twox64Concat,
    >(pallet_name, item_name)
    .collect::<Vec<_>>();
    weight = weight.saturating_add(T::DbWeight::get().reads(crowdloans.len() as u64));

    for (id, crowdloan) in crowdloans {
        let contributions = Contributions::<T>::iter_key_prefix(id)
            .collect::<Vec<_>>()
            .len();
        weight = weight.saturating_add(T::DbWeight::get().reads(contributions as u64));

        Crowdloans::<T>::insert(
            id,
            CrowdloanInfo {
                creator: crowdloan.creator,
                deposit: crowdloan.deposit,
                min_contribution: crowdloan.min_contribution,
                end: crowdloan.end,
                cap: crowdloan.cap,
                funds_account: crowdloan.funds_account,
                raised: crowdloan.raised,
                target_address: crowdloan.target_address,
                call: crowdloan.call,
                finalized: crowdloan.finalized,
                contributors_count: contributions as u32,
            },
        );
        weight = weight.saturating_add(T::DbWeight::get().writes(1));
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    weight
}

#[cfg(test)]
mod tests {
    use frame_support::{Hashable, storage::unhashed::put_raw};
    use sp_core::U256;
    use sp_io::hashing::twox_128;

    use super::*;
    use crate::mock::{Test, TestState};

    #[test]
    fn test_migrate_add_contributors_count_works() {
        TestState::default().build_and_execute(|| {
            let pallet_name = twox_128(b"Crowdloan");
            let storage_name = twox_128(b"Crowdloans");
            let prefix = [pallet_name, storage_name].concat();

            let items = vec![
                (
                    old_storage::OldCrowdloanInfo {
                        creator: U256::from(1),
                        deposit: 100u64,
                        min_contribution: 10u64,
                        end: 100u64,
                        cap: 1000u64,
                        funds_account: U256::from(2),
                        raised: 0u64,
                        target_address: None,
                        call: None::<BoundedCallOf<Test>>,
                        finalized: false,
                    },
                    vec![(U256::from(1), 100)],
                ),
                (
                    old_storage::OldCrowdloanInfo {
                        creator: U256::from(1),
                        deposit: 100u64,
                        min_contribution: 10u64,
                        end: 100u64,
                        cap: 1000u64,
                        funds_account: U256::from(2),
                        raised: 0u64,
                        target_address: None,
                        call: None::<BoundedCallOf<Test>>,
                        finalized: false,
                    },
                    vec![
                        (U256::from(1), 100),
                        (U256::from(2), 100),
                        (U256::from(3), 100),
                    ],
                ),
                (
                    old_storage::OldCrowdloanInfo {
                        creator: U256::from(1),
                        deposit: 100u64,
                        min_contribution: 10u64,
                        end: 100u64,
                        cap: 1000u64,
                        funds_account: U256::from(2),
                        raised: 0u64,
                        target_address: None,
                        call: None::<BoundedCallOf<Test>>,
                        finalized: false,
                    },
                    vec![
                        (U256::from(1), 100),
                        (U256::from(2), 100),
                        (U256::from(3), 100),
                        (U256::from(4), 100),
                        (U256::from(5), 100),
                    ],
                ),
            ];

            for (id, (crowdloan, contributions)) in items.into_iter().enumerate() {
                let key = [prefix.clone(), (id as u32).twox_64_concat()].concat();
                put_raw(&key, &crowdloan.encode());

                for (contributor, amount) in contributions {
                    let amount = TaoBalance::from(amount);
                    Contributions::<Test>::insert(id as u32, contributor, amount);
                }
            }

            migrate_add_contributors_count::<Test>();

            assert!(Crowdloans::<Test>::get(0).is_some_and(|c| c.contributors_count == 1));
            assert!(Crowdloans::<Test>::get(1).is_some_and(|c| c.contributors_count == 3));
            assert!(Crowdloans::<Test>::get(2).is_some_and(|c| c.contributors_count == 5));

            assert!(HasMigrationRun::<Test>::get(BoundedVec::truncate_from(
                b"migrate_add_contributors_count".to_vec()
            )));
        });
    }
}
