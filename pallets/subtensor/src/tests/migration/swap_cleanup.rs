#![allow(
    unused,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
//! swap v3 cleanup, coldkey-swap announcements, registration map clear, axon/cert purge.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_migrate_remove_unknown_neuron_axon_cert_prom() {
    use crate::migrations::migrate_remove_unknown_neuron_axon_cert_prom::*;
    const MIGRATION_NAME: &[u8] = b"migrate_remove_neuron_axon_cert_prom";

    new_test_ext(1).execute_with(|| {
        setup_for(NetUid::from(2), 64, 1231);
        setup_for(NetUid::from(42), 256, 15151);
        setup_for(NetUid::from(99), 1024, 32323);
        assert!(!HasMigrationRun::<Test>::get(MIGRATION_NAME));

        let w = migrate_remove_unknown_neuron_axon_cert_prom::<Test>();
        assert!(!w.is_zero(), "Weight must be non-zero");

        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));
        assert_for(NetUid::from(2), 64, 1231);
        assert_for(NetUid::from(42), 256, 15151);
        assert_for(NetUid::from(99), 1024, 32323);
    });

    fn setup_for(netuid: NetUid, uids: u32, items: u32) {
        NetworksAdded::<Test>::insert(netuid, true);

        for i in 1u32..=uids {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            Uids::<Test>::insert(netuid, hk, i as u16);
        }

        for i in 1u32..=items {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            Axons::<Test>::insert(netuid, hk, AxonInfo::default());
            NeuronCertificates::<Test>::insert(netuid, hk, NeuronCertificate::default());
            Prometheus::<Test>::insert(netuid, hk, PrometheusInfo::default());
        }
    }

    fn assert_for(netuid: NetUid, uids: u32, items: u32) {
        assert_eq!(
            Axons::<Test>::iter_key_prefix(netuid).count(),
            uids as usize
        );
        assert_eq!(
            NeuronCertificates::<Test>::iter_key_prefix(netuid).count(),
            uids as usize
        );
        assert_eq!(
            Prometheus::<Test>::iter_key_prefix(netuid).count(),
            uids as usize
        );

        for i in 1u32..=uids {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            assert!(Axons::<Test>::contains_key(netuid, hk));
            assert!(NeuronCertificates::<Test>::contains_key(netuid, hk));
            assert!(Prometheus::<Test>::contains_key(netuid, hk));
        }

        for i in uids + 1u32..=items {
            let hk = U256::from(netuid.inner() as u32 * 1000 + i);
            assert!(!Axons::<Test>::contains_key(netuid, hk));
            assert!(!NeuronCertificates::<Test>::contains_key(netuid, hk));
            assert!(!Prometheus::<Test>::contains_key(netuid, hk));
        }
    }
}

// cargo test --package pallet-subtensor --lib -- tests::migration::test_migrate_cleanup_swap_v3 --exact --nocapture
#[test]
fn test_migrate_cleanup_swap_v3() {
    use crate::migrations::migrate_cleanup_swap_v3::deprecated_swap_maps;
    use substrate_fixed::types::U64F64;

    new_test_ext(1).execute_with(|| {
        let migration = crate::migrations::migrate_cleanup_swap_v3::migrate_cleanup_swap_v3::<Test>;

        const MIGRATION_NAME: &str = "migrate_cleanup_swap_v3";

        let provided: u64 = 9876;
        let reserves: u64 = 1_000_000;

        SubnetTAO::<Test>::insert(NetUid::from(1), TaoBalance::from(reserves));
        SubnetAlphaIn::<Test>::insert(NetUid::from(1), AlphaBalance::from(reserves));

        // Insert deprecated maps values
        deprecated_swap_maps::SubnetTaoProvided::<Test>::insert(
            NetUid::from(1),
            TaoBalance::from(provided),
        );
        deprecated_swap_maps::SubnetAlphaInProvided::<Test>::insert(
            NetUid::from(1),
            AlphaBalance::from(provided),
        );

        // Run migration
        let weight = migration();

        // Test that values are removed from state
        assert!(!deprecated_swap_maps::SubnetTaoProvided::<Test>::contains_key(NetUid::from(1)),);
        assert!(
            !deprecated_swap_maps::SubnetAlphaInProvided::<Test>::contains_key(NetUid::from(1)),
        );

        // Provided got added to reserves
        assert_eq!(
            u64::from(SubnetTAO::<Test>::get(NetUid::from(1))),
            reserves + provided
        );
        assert_eq!(
            u64::from(SubnetAlphaIn::<Test>::get(NetUid::from(1))),
            reserves + provided
        );
    });
}

// Regression test for issue #2793: migrate_cleanup_swap_v3 must be wired into the pallet
// on_runtime_upgrade hook. Seeds a *Provided residual, runs the full upgrade hook, and asserts
// the residual is folded into the main reserves. Without the wiring line in hooks.rs this fails.
#[test]
fn test_migrate_cleanup_swap_v3_runs_on_runtime_upgrade() {
    use crate::migrations::migrate_cleanup_swap_v3::deprecated_swap_maps;
    use frame_support::traits::Hooks;

    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let provided: u64 = 9876;

        deprecated_swap_maps::SubnetTaoProvided::<Test>::insert(netuid, TaoBalance::from(provided));
        deprecated_swap_maps::SubnetAlphaInProvided::<Test>::insert(
            netuid,
            AlphaBalance::from(provided),
        );

        let tao_before = u64::from(SubnetTAO::<Test>::get(netuid));
        let alpha_before = u64::from(SubnetAlphaIn::<Test>::get(netuid));

        let _ = <crate::Pallet<Test> as Hooks<u64>>::on_runtime_upgrade();

        assert!(!deprecated_swap_maps::SubnetTaoProvided::<Test>::contains_key(netuid));
        assert!(!deprecated_swap_maps::SubnetAlphaInProvided::<Test>::contains_key(netuid));
        assert_eq!(
            u64::from(SubnetTAO::<Test>::get(netuid)),
            tao_before + provided
        );
        assert_eq!(
            u64::from(SubnetAlphaIn::<Test>::get(netuid)),
            alpha_before + provided
        );
    });
}

#[test]
fn test_migrate_coldkey_swap_scheduled_to_announcements() {
    new_test_ext(1000).execute_with(|| {
        use crate::migrations::migrate_coldkey_swap_scheduled_to_announcements::*;
        use coldkey_swap_deprecated as deprecated;

        const MIGRATION_NAME: &[u8] = b"migrate_coldkey_swap_scheduled_to_announcements";
        let now = frame_system::Pallet::<Test>::block_number();

        // Set the schedule duration and reschedule duration
        deprecated::ColdkeySwapScheduleDuration::<Test>::set(Some(now + 100));
        deprecated::ColdkeySwapRescheduleDuration::<Test>::set(Some(now + 200));

        let make_swap_task = |who: U256, new_coldkey: U256| -> ScheduledOf<Test> {
            let call_bytes = deprecated::RuntimeCall::<Test>::SubtensorCall(
                deprecated::SubtensorCall::SwapColdkey {
                    old_coldkey: who,
                    new_coldkey,
                    swap_cost: 1000.into(),
                },
            )
            .encode();
            pallet_scheduler::Scheduled {
                maybe_id: None,
                priority: 63,
                call: Bounded::Inline(BoundedVec::truncate_from(call_bytes)),
                maybe_periodic: None,
                origin: OriginCaller::system(frame_system::RawOrigin::Root),
                _phantom: PhantomData,
            }
        };

        let make_other_task = || -> ScheduledOf<Test> {
            let call_bytes = RuntimeCall::SubtensorModule(crate::Call::burned_register {
                netuid: 1u16.into(),
                hotkey: U256::from(999),
            })
            .encode();
            pallet_scheduler::Scheduled {
                maybe_id: None,
                priority: 63,
                call: Bounded::Inline(BoundedVec::truncate_from(call_bytes)),
                maybe_periodic: None,
                origin: OriginCaller::system(frame_system::RawOrigin::Root),
                _phantom: PhantomData,
            }
        };

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(1),
            (now + 100, U256::from(10)),
        );
        pallet_scheduler::Agenda::<Test>::insert(
            now + 100,
            BoundedVec::truncate_from(vec![
                Some(make_swap_task(U256::from(1), U256::from(10))),
                Some(make_other_task()),
            ]),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(2),
            (now - 200, U256::from(20)),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(3),
            (now + 200, U256::from(30)),
        );
        pallet_scheduler::Agenda::<Test>::insert(
            now + 200,
            BoundedVec::truncate_from(vec![Some(make_swap_task(U256::from(3), U256::from(30)))]),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(4),
            (now - 400, U256::from(40)),
        );

        deprecated::ColdkeySwapScheduled::<Test>::insert(
            U256::from(5),
            (now + 300, U256::from(50)),
        );
        pallet_scheduler::Agenda::<Test>::insert(
            now + 300,
            BoundedVec::truncate_from(vec![
                Some(make_other_task()),
                Some(make_swap_task(U256::from(5), U256::from(50))),
            ]),
        );

        let w = migrate_coldkey_swap_scheduled_to_announcements::<Test>();

        assert!(!w.is_zero(), "weight must be non-zero");
        assert!(HasMigrationRun::<Test>::get(MIGRATION_NAME));

        // Ensure the deprecated storage is cleared
        assert!(!deprecated::ColdkeySwapScheduleDuration::<Test>::exists());
        assert!(!deprecated::ColdkeySwapRescheduleDuration::<Test>::exists());
        assert_eq!(deprecated::ColdkeySwapScheduled::<Test>::iter().count(), 0);

        assert_eq!(
            pallet_scheduler::Agenda::<Test>::get(now + 100),
            vec![None, Some(make_other_task())],
            "swap task for who=1 should be cancelled"
        );

        assert_eq!(
            pallet_scheduler::Agenda::<Test>::get(now + 200),
            vec![None],
            "swap task for who=3 should be cancelled"
        );

        assert_eq!(
            pallet_scheduler::Agenda::<Test>::get(now + 300),
            vec![Some(make_other_task()), None],
            "swap task for who=5 should be cancelled"
        );

        let delay = ColdkeySwapAnnouncementDelay::<Test>::get();
        assert_eq!(ColdkeySwapAnnouncements::<Test>::iter().count(), 3);
        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(U256::from(
            2
        )));
        assert!(!ColdkeySwapAnnouncements::<Test>::contains_key(U256::from(
            4
        )));
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::get(U256::from(1)),
            Some((
                now + 100 - delay,
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(10))
            ))
        );
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::get(U256::from(3)),
            Some((
                now + 200 - delay,
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(30))
            ))
        );
        assert_eq!(
            ColdkeySwapAnnouncements::<Test>::get(U256::from(5)),
            Some((
                now + 300 - delay,
                <Test as frame_system::Config>::Hashing::hash_of(&U256::from(50))
            ))
        );
    });
}

#[test]
fn test_migrate_clear_deprecated_registration_maps() {
    new_test_ext(1).execute_with(|| {
        const MIG_NAME: &[u8] = b"migrate_clear_deprecated_registration_maps_v1";

        let netuid0: NetUid = 0u16.into();
        let netuid1: NetUid = 1u16.into();

        // --------------------------------------------------------------------
        // 0) Pre-state
        // --------------------------------------------------------------------
        assert!(
            !HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be false before run"
        );

        // New-model storage must remain untouched by this migration.
        crate::BurnHalfLife::<Test>::insert(netuid0, 777u16);
        crate::BurnIncreaseMult::<Test>::insert(netuid0, U64F64::from_num(9));

        crate::BurnHalfLife::<Test>::insert(netuid1, 888u16);
        crate::BurnIncreaseMult::<Test>::insert(netuid1, U64F64::from_num(11));

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid0), 777u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid0), 9u64);

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid1), 888u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid1), 11u64);

        // Seed deprecated storage items that the migration is expected to clear.
        crate::NetworkPowRegistrationAllowed::<Test>::insert(netuid0, true);

        crate::POWRegistrationsThisInterval::<Test>::insert(netuid0, 7u16);
        crate::BurnRegistrationsThisInterval::<Test>::insert(netuid0, 8u16);

        crate::NetworkPowRegistrationAllowed::<Test>::insert(netuid1, false);

        crate::POWRegistrationsThisInterval::<Test>::insert(netuid1, 17u16);
        crate::BurnRegistrationsThisInterval::<Test>::insert(netuid1, 18u16);

        assert!(crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid0));
        assert!(crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid0));
        assert!(crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid0));

        assert!(crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid1));
        assert!(crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid1));
        assert!(crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid1));

        // --------------------------------------------------------------------
        // 1) Run migration
        // --------------------------------------------------------------------
        let w = crate::migrations::migrate_clear_deprecated_registration_maps::migrate_clear_deprecated_registration_maps::<Test>();
        assert!(!w.is_zero(), "weight must be non-zero");

        // --------------------------------------------------------------------
        // 2) Post-state: deprecated storage cleared
        // --------------------------------------------------------------------
        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should be true after run"
        );

        assert!(!crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid0));
        assert!(!crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid0));
        assert!(!crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid0));

        assert!(!crate::NetworkPowRegistrationAllowed::<Test>::contains_key(netuid1));
        assert!(!crate::POWRegistrationsThisInterval::<Test>::contains_key(netuid1));
        assert!(!crate::BurnRegistrationsThisInterval::<Test>::contains_key(netuid1));

        // --------------------------------------------------------------------
        // 3) Post-state: new-model storage unchanged
        // --------------------------------------------------------------------
        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid0), 777u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid0), 9u64);

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid1), 888u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid1), 11u64);

        // --------------------------------------------------------------------
        // 4) Idempotency
        // --------------------------------------------------------------------
        let w2 = crate::migrations::migrate_clear_deprecated_registration_maps::migrate_clear_deprecated_registration_maps::<Test>();
        assert!(!w2.is_zero(), "second call should still return non-zero read weight");

        assert!(
            HasMigrationRun::<Test>::get(MIG_NAME.to_vec()),
            "migration flag should remain true after second run"
        );

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid0), 777u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid0), 9u64);

        assert_eq!(crate::BurnHalfLife::<Test>::get(netuid1), 888u16);
        assert_eq!(crate::BurnIncreaseMult::<Test>::get(netuid1), 11u64);
    });
}
