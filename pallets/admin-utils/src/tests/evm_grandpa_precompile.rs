//! EVM chain id, GRANDPA authority schedule, and EVM precompile enable toggles.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_root_sets_evm_chain_id() {
    new_test_ext().execute_with(|| {
        let chain_id: u64 = 945;
        assert_eq!(pallet_evm_chain_id::ChainId::<Test>::get(), 0);

        assert_ok!(AdminUtils::sudo_set_evm_chain_id(
            <<Test as Config>::RuntimeOrigin>::root(),
            chain_id
        ));

        assert_eq!(pallet_evm_chain_id::ChainId::<Test>::get(), chain_id);
    });
}

#[test]
fn test_sudo_non_root_cannot_set_evm_chain_id() {
    new_test_ext().execute_with(|| {
        let chain_id: u64 = 945;
        assert_eq!(pallet_evm_chain_id::ChainId::<Test>::get(), 0);

        assert_eq!(
            AdminUtils::sudo_set_evm_chain_id(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                chain_id
            ),
            Err(DispatchError::BadOrigin)
        );

        assert_eq!(pallet_evm_chain_id::ChainId::<Test>::get(), 0);
    });
}

#[test]
fn test_schedule_grandpa_change() {
    new_test_ext().execute_with(|| {
        assert_eq!(Grandpa::grandpa_authorities(), vec![]);

        let bob: GrandpaId = ed25519::Pair::from_legacy_string("//Bob", None)
            .public()
            .into();

        assert_ok!(AdminUtils::schedule_grandpa_change(
            RuntimeOrigin::root(),
            vec![(bob.clone(), 1)],
            41,
            None
        ));

        Grandpa::on_finalize(42);

        assert_eq!(Grandpa::grandpa_authorities(), vec![(bob, 1)]);
    });
}

#[test]
fn test_sudo_toggle_evm_precompile() {
    new_test_ext().execute_with(|| {
        let precompile_id = crate::PrecompileEnum::BalanceTransfer;
        let initial_enabled = PrecompileEnable::<Test>::get(precompile_id);
        assert!(initial_enabled); // Assuming the default is true

        run_to_block(1);

        assert_eq!(
            AdminUtils::sudo_toggle_evm_precompile(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(0)),
                precompile_id,
                false
            ),
            Err(DispatchError::BadOrigin)
        );

        assert_ok!(AdminUtils::sudo_toggle_evm_precompile(
            RuntimeOrigin::root(),
            precompile_id,
            false
        ));

        assert_eq!(
            System::events()
                .iter()
                .filter(|r| r.event
                    == RuntimeEvent::AdminUtils(crate::Event::PrecompileUpdated {
                        precompile_id,
                        enabled: false
                    }))
                .count(),
            1
        );

        let updated_enabled = PrecompileEnable::<Test>::get(precompile_id);
        assert!(!updated_enabled);

        run_to_block(2);

        assert_ok!(AdminUtils::sudo_toggle_evm_precompile(
            RuntimeOrigin::root(),
            precompile_id,
            false
        ));

        // no event without status change
        assert_eq!(
            System::events()
                .iter()
                .filter(|r| r.event
                    == RuntimeEvent::AdminUtils(crate::Event::PrecompileUpdated {
                        precompile_id,
                        enabled: false
                    }))
                .count(),
            0
        );

        assert_ok!(AdminUtils::sudo_toggle_evm_precompile(
            RuntimeOrigin::root(),
            precompile_id,
            true
        ));

        let final_enabled = PrecompileEnable::<Test>::get(precompile_id);
        assert!(final_enabled);
    });
}
