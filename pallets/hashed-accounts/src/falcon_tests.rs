#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use crate::tests::{
    Balances, Enabled, HashedAccounts, RuntimeOrigin, System, Test, new_test_ext, sponsor,
};
use frame_support::{assert_noop, assert_ok};
use subtensor_hashed::{falcon, mldsa};

fn fixture(profile: u8) -> (mldsa::Descriptor, AccountId32, Vec<u8>) {
    for attempt in 0u32..256 {
        let mut seed = [profile; 32];
        seed[..4].copy_from_slice(&attempt.to_le_bytes());
        let public = falcon::public_key_from_seed(profile, &seed).unwrap();
        let descriptor = mldsa::Descriptor {
            version: falcon::VERSION,
            algorithm: profile,
            initial_commitment: falcon::commitment(profile, &public).unwrap(),
        };
        let id = mldsa::account_id(&descriptor);
        if !falcon::is_fixed_profile(profile) || mldsa::rejects_legacy_public_keys(&id) {
            return (descriptor, AccountId32::new(id), public);
        }
    }
    panic!("safe account fixture exhausted")
}

#[test]
fn fixed_falcon_has_no_registration_or_deposit_and_preserves_nonce_liveness() {
    for profile in [falcon::FIXED_ALGORITHM_512, falcon::FIXED_ALGORITHM_1024] {
        new_test_ext().execute_with(|| {
            let (descriptor, account, public) = fixture(profile);
            assert_noop!(
                HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
                Error::<Test>::UnsupportedDescriptor
            );
            assert_eq!(Balances::reserved_balance(sponsor()), 0);
            let ticket = HashedAccounts::check_mldsa_authorization(
                &account,
                profile,
                &public,
                0,
                descriptor.initial_commitment,
            )
            .unwrap();
            assert_ok!(HashedAccounts::advance_mldsa(&ticket));
            assert!(!MlDsaAccounts::<Test>::contains_key(&account));
            assert_eq!(System::account(&account).sufficients, 1);
            assert_eq!(Balances::reserved_balance(&account), 0);
            assert_noop!(
                HashedAccounts::check_mldsa_authorization(
                    &account,
                    profile,
                    &public,
                    1,
                    descriptor.initial_commitment
                ),
                Error::<Test>::WrongCommitment
            );
            Enabled::set(false);
            assert_noop!(
                HashedAccounts::check_mldsa_authorization(
                    &account,
                    profile,
                    &public,
                    0,
                    descriptor.initial_commitment
                ),
                Error::<Test>::Disabled
            );
            assert_noop!(
                HashedAccounts::advance_mldsa(&ticket),
                Error::<Test>::Disabled
            );
        });
    }
}

#[test]
fn rotating_falcon_registers_once_and_advances_only_matching_profile_and_generation() {
    for profile in [falcon::ALGORITHM_512, falcon::ALGORITHM_1024] {
        new_test_ext().execute_with(|| {
            let (descriptor, account, public) = fixture(profile);
            assert_noop!(
                HashedAccounts::check_mldsa_authorization(&account, profile, &public, 0, [9; 64]),
                Error::<Test>::NotRegistered
            );
            assert_noop!(
                HashedAccounts::register(
                    RuntimeOrigin::signed(sponsor()),
                    RegistrationDescriptor::MlDsa65(descriptor)
                ),
                Error::<Test>::UnsupportedDescriptor
            );
            assert_ok!(HashedAccounts::register(
                RuntimeOrigin::signed(sponsor()),
                descriptor.into()
            ));
            assert_ok!(HashedAccounts::register(
                RuntimeOrigin::signed(sponsor()),
                descriptor.into()
            ));
            assert_eq!(Balances::reserved_balance(sponsor()), 20);
            assert!(HashedAccounts::authority_meets(
                &account,
                SecurityFloor::PqRequired
            ));
            assert_ok!(HashedAccounts::check_registered(
                RuntimeOrigin::signed(sponsor()),
                descriptor.into()
            ));
            let ticket =
                HashedAccounts::check_mldsa_authorization(&account, profile, &public, 0, [9; 64])
                    .unwrap();
            Enabled::set(false);
            assert_noop!(
                HashedAccounts::advance_mldsa(&ticket),
                Error::<Test>::Disabled
            );
            Enabled::set(true);
            assert_ok!(HashedAccounts::advance_mldsa(&ticket));
            assert_noop!(
                HashedAccounts::advance_mldsa(&ticket),
                Error::<Test>::WrongGeneration
            );
            let state = MlDsaAccounts::<Test>::get(&account).unwrap();
            assert_eq!(state.generation, 1);
            assert_eq!(state.commitment, [9; 64]);
            assert!(
                HashedAccounts::check_mldsa_authorization(&account, profile, &public, 1, [8; 64])
                    .is_err()
            );
        });
    }
}
