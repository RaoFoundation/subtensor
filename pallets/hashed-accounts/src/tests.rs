#![allow(clippy::unwrap_used)]

use super::*;
use frame_support::{
    assert_noop, assert_ok, derive_impl, parameter_types,
    traits::{ConstU64, Currency},
};
use sp_core::Pair;
use sp_runtime::{BuildStorage, DispatchError, traits::IdentityLookup};

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        HashedAccounts: crate,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = frame_system::mocking::MockBlock<Test>;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<AccountId32>;
    type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type AccountStore = System;
    type ExistentialDeposit = ConstU64<1>;
}

parameter_types! {
    pub static Enabled: bool = true;
    pub static RejectRegistration: bool = false;
    pub static RejectSponsorship: bool = false;
}

pub struct RegistrationGuard;
impl OnRegister for RegistrationGuard {
    fn on_sponsored_deposit(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        if RejectSponsorship::get() {
            Err(DispatchError::Other("sponsor did not consent"))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_sponsored(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        Ok(())
    }
    fn on_register(_: &AccountId32, _: &AccountId32, _: &Descriptor) -> DispatchResult {
        if RejectRegistration::get() {
            Err(sp_runtime::DispatchError::Other("incompatible authority"))
        } else {
            Ok(())
        }
    }

    fn on_register_mldsa(
        _: &AccountId32,
        _: &AccountId32,
        _: &subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        if RejectRegistration::get() {
            Err(DispatchError::Other("incompatible authority"))
        } else {
            Ok(())
        }
    }

    fn on_register_derived(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        if RejectRegistration::get() {
            Err(DispatchError::Other("incompatible authority"))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_mldsa(
        _: &AccountId32,
        _: &AccountId32,
        _: &subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn setup_benchmark_derived(
        _: &AccountId32,
        _: &AccountId32,
        _: &DerivedDescriptor,
    ) -> DispatchResult {
        Ok(())
    }
}

impl Config for Test {
    type Currency = Balances;
    type RegistrationDeposit = ConstU64<10>;
    type MlDsaRegistrationDeposit = ConstU64<20>;
    type DerivedRegistrationDeposit = ConstU64<30>;
    type Enabled = Enabled;
    type OnRegister = RegistrationGuard;
    type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    Enabled::set(true);
    RejectRegistration::set(false);
    RejectSponsorship::set(false);
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(sponsor(), 1000)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.register_extension(sp_keystore::KeystoreExt::new(
        sp_keystore::testing::MemoryKeystore::new(),
    ));
    ext.execute_with(|| System::set_block_number(1));
    ext
}

pub(crate) fn sponsor() -> AccountId32 {
    AccountId32::new([1; 32])
}

fn fixture() -> (sp_core::sr25519::Pair, Descriptor, AccountId32) {
    let pair = sp_core::sr25519::Pair::from_seed(&[2; 32]);
    let descriptor = Descriptor {
        version: 1,
        scheme: Scheme::Sr25519,
        initial_commitment: subtensor_hashed::key_commitment(Scheme::Sr25519, &pair.public().0),
    };
    let account = AccountId32::new(subtensor_hashed::account_id(&descriptor));
    (pair, descriptor, account)
}

fn test_payload(
    account: &[u8; 32],
    scheme: Scheme,
    generation: u64,
    next: &[u8; 32],
    call: &[u8],
) -> Vec<u8> {
    (account, scheme, generation, next, call).encode()
}

fn proof(
    pair: &sp_core::sr25519::Pair,
    account: &AccountId32,
    generation: u64,
    next_commitment: [u8; 32],
) -> Proof {
    let payload = test_payload(
        account.as_ref(),
        Scheme::Sr25519,
        generation,
        &next_commitment,
        b"implication",
    );
    Proof {
        generation,
        public_key: pair.public().0,
        next_commitment,
        signature: pair.sign(&payload).0,
    }
}

// Signature verification belongs to the standard extrinsic checker; pallet
// tests model that prerequisite before checking the commitment and advancing.
fn check_proof(
    account: &AccountId32,
    proof: &Proof,
    implication: &[u8],
) -> Result<ValidatedProof, DispatchError> {
    let ticket = HashedAccounts::check_authorization(
        account,
        Scheme::Sr25519,
        &proof.public_key,
        proof.generation,
        proof.next_commitment,
    )?;
    frame_support::ensure!(
        sp_core::sr25519::Pair::verify(
            &sp_core::sr25519::Signature::from_raw(proof.signature),
            test_payload(
                account.as_ref(),
                Scheme::Sr25519,
                proof.generation,
                &proof.next_commitment,
                implication
            ),
            &sp_core::sr25519::Public::from_raw(proof.public_key)
        ),
        Error::<Test>::InvalidSignature
    );
    Ok(ticket)
}

#[test]
fn recipient_check_is_read_only_and_never_registers_a_missing_account() {
    new_test_ext().execute_with(|| {
        let (pair, descriptor, account) = fixture();
        assert_noop!(
            HashedAccounts::check_registered(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::NotRegistered
        );
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        let ticket = check_proof(
            &account,
            &proof(&pair, &account, 0, [9; 32]),
            b"implication",
        )
        .unwrap();
        assert_ok!(HashedAccounts::advance(&ticket));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        // Even an unfunded signer can check a rotated account's original descriptor.
        assert_ok!(HashedAccounts::check_registered(
            RuntimeOrigin::signed(AccountId32::new([8; 32])),
            descriptor.into()
        ));
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        Accounts::<Test>::remove(&account);
        assert_noop!(
            HashedAccounts::check_registered(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::NotRegistered
        );
        Enabled::set(false);
        assert_noop!(
            HashedAccounts::check_registered(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::Disabled
        );
    });
}

#[test]
fn registration_is_idempotent_and_sponsor_never_becomes_authority() {
    new_test_ext().execute_with(|| {
        let (pair, descriptor, account) = fixture();
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        assert_eq!(Balances::reserved_balance(sponsor()), 10);
        assert_eq!(System::account_nonce(&account), 1);
        assert_eq!(System::providers(&account), 1);
        assert_eq!(
            EvmAliases::<Test>::get(HashedAccounts::evm_alias(&account)),
            Some(account.clone())
        );
        let valid = proof(&pair, &account, 0, [9; 32]);
        let ticket = check_proof(&account, &valid, b"implication").unwrap();
        assert_ok!(HashedAccounts::advance(&ticket));
        // A different sponsor can replay registration without payment or reset.
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(AccountId32::new([6; 32])),
            descriptor.into()
        ));
        assert_eq!(Balances::reserved_balance(sponsor()), 10);
        assert_eq!(System::providers(&account), 1);
        assert_eq!(Accounts::<Test>::get(&account).unwrap().generation, 1);
        assert_eq!(Accounts::<Test>::get(&account).unwrap().commitment, [9; 32]);
        let foreign = proof(
            &sp_core::sr25519::Pair::from_seed(&[1; 32]),
            &account,
            1,
            [8; 32],
        );
        assert_noop!(
            check_proof(&account, &foreign, b"implication"),
            Error::<Test>::WrongCommitment
        );
    });
}

#[test]
fn validation_is_read_only_and_competing_proofs_cannot_advance_twice() {
    new_test_ext().execute_with(|| {
        let (pair, descriptor, account) = fixture();
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        let first = check_proof(
            &account,
            &proof(&pair, &account, 0, [8; 32]),
            b"implication",
        )
        .unwrap();
        let competitor = check_proof(
            &account,
            &proof(&pair, &account, 0, [9; 32]),
            b"implication",
        )
        .unwrap();
        assert_eq!(Accounts::<Test>::get(&account).unwrap().generation, 0);
        assert_ok!(HashedAccounts::advance(&first));
        assert_noop!(
            HashedAccounts::advance(&competitor),
            Error::<Test>::WrongGeneration
        );
        assert_eq!(Accounts::<Test>::get(&account).unwrap().commitment, [8; 32]);
        // Rechecking the same generation also checks the original commitment.
        Accounts::<Test>::mutate(&account, |record| record.as_mut().unwrap().generation = 0);
        assert_noop!(
            HashedAccounts::advance(&competitor),
            Error::<Test>::WrongCommitment
        );
    });
}

#[test]
fn signatures_cannot_change_the_call_or_install_invalid_commitments() {
    new_test_ext().execute_with(|| {
        let (pair, descriptor, account) = fixture();
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        let valid = proof(&pair, &account, 0, [8; 32]);
        assert_noop!(
            check_proof(&account, &valid, b"different call"),
            Error::<Test>::InvalidSignature
        );
        assert_noop!(
            check_proof(
                &account,
                &proof(&pair, &account, 0, [0; 32]),
                b"implication"
            ),
            Error::<Test>::InvalidNextCommitment
        );
        assert_noop!(
            check_proof(
                &account,
                &proof(&pair, &account, 0, descriptor.initial_commitment),
                b"implication"
            ),
            Error::<Test>::InvalidNextCommitment
        );
        Accounts::<Test>::mutate(&account, |record| {
            record.as_mut().unwrap().generation = u64::MAX
        });
        assert_noop!(
            check_proof(
                &account,
                &proof(&pair, &account, u64::MAX, [8; 32]),
                b"implication"
            ),
            Error::<Test>::GenerationExhausted
        );
    });
}

#[test]
fn failed_registration_leaves_no_policy_deposit_or_alias() {
    new_test_ext().execute_with(|| {
        let (_, descriptor, account) = fixture();
        RejectRegistration::set(true);
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            sp_runtime::DispatchError::Other("incompatible authority")
        );
        assert!(!HashedAccounts::is_registered(&account));
        assert_eq!(Balances::reserved_balance(sponsor()), 0);
        assert_eq!(System::providers(&account), 0);
        RejectRegistration::set(false);
        let poor = AccountId32::new([3; 32]);
        assert!(HashedAccounts::register(RuntimeOrigin::signed(poor), descriptor.into()).is_err());
        assert!(!HashedAccounts::is_registered(&account));
        assert!(!EvmAliases::<Test>::contains_key(
            HashedAccounts::evm_alias(&account)
        ));
    });
}

#[test]
fn disabled_unsupported_and_conflicting_registrations_fail_closed() {
    new_test_ext().execute_with(|| {
        let (_, mut descriptor, account) = fixture();
        Enabled::set(false);
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::Disabled
        );
        Enabled::set(true);
        descriptor.version = 2;
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::UnsupportedDescriptor
        );
        descriptor.version = 1;
        EvmAliases::<Test>::insert(
            HashedAccounts::evm_alias(&account),
            AccountId32::new([3; 32]),
        );
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::AliasCollision
        );
        EvmAliases::<Test>::remove(HashedAccounts::evm_alias(&account));
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        Accounts::<Test>::mutate(&account, |record| {
            record.as_mut().unwrap().descriptor.initial_commitment = [9; 32]
        });
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::DescriptorMismatch
        );
    });
}

#[test]
fn zero_balance_does_not_erase_authorization_or_reset_generation() {
    new_test_ext().execute_with(|| {
        let (pair, descriptor, account) = fixture();
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        let _ = Balances::make_free_balance_be(&account, 100);
        let ticket = check_proof(
            &account,
            &proof(&pair, &account, 0, [9; 32]),
            b"implication",
        )
        .unwrap();
        assert_ok!(HashedAccounts::advance(&ticket));
        let _ = Balances::make_free_balance_be(&account, 0);
        assert!(HashedAccounts::is_registered(&account));
        assert!(System::account_exists(&account));
        assert_eq!(System::account_nonce(&account), 1);
        assert_eq!(Accounts::<Test>::get(&account).unwrap().generation, 1);
    });
}

#[test]
fn unified_registration_preserves_scheme_policy_and_account_identity() {
    new_test_ext().execute_with(|| {
        let (_, classical, classical_account) = fixture();
        let mldsa = subtensor_hashed::mldsa::Descriptor {
            version: 2,
            algorithm: 4,
            initial_commitment: [7; 64],
        };
        let mldsa_account = AccountId32::new(subtensor_hashed::mldsa::account_id(&mldsa));

        // The single switch gates both schemes through the original dispatchable.
        Enabled::set(false);
        for descriptor in [classical.into(), mldsa.into()] {
            assert_noop!(
                HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor),
                Error::<Test>::Disabled,
            );
        }
        assert_eq!(Balances::reserved_balance(sponsor()), 0);
        Enabled::set(true);
        for descriptor in [classical.into(), mldsa.into()] {
            assert_ok!(HashedAccounts::register(
                RuntimeOrigin::signed(sponsor()),
                descriptor
            ));
        }
        assert_eq!(Balances::reserved_balance(sponsor()), 30);
        assert_eq!(
            Accounts::<Test>::get(&classical_account)
                .unwrap()
                .descriptor,
            classical
        );
        assert_eq!(
            MlDsaAccounts::<Test>::get(&mldsa_account)
                .unwrap()
                .descriptor,
            mldsa
        );
        assert!(!Accounts::<Test>::contains_key(&mldsa_account));
        assert!(!MlDsaAccounts::<Test>::contains_key(&classical_account));
        assert_eq!(System::account_nonce(&classical_account), 1);
        assert_eq!(System::account_nonce(&mldsa_account), 1);

        Enabled::set(true);
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        for descriptor in [classical.into(), mldsa.into()] {
            assert_ok!(HashedAccounts::check_registered(
                RuntimeOrigin::signed(sponsor()),
                descriptor,
            ));
        }
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn unified_registration_uses_original_call_indices_and_scheme_weights() {
    use frame_support::dispatch::GetDispatchInfo;

    let (_, classical, _) = fixture();
    let mldsa = subtensor_hashed::mldsa::Descriptor {
        version: 2,
        algorithm: 4,
        initial_commitment: [7; 64],
    };
    let classical_register = Call::<Test>::register {
        descriptor: classical.into(),
    };
    let mldsa_register = Call::<Test>::register {
        descriptor: mldsa.into(),
    };
    let classical_check = Call::<Test>::check_registered {
        descriptor: classical.into(),
    };
    let mldsa_check = Call::<Test>::check_registered {
        descriptor: mldsa.into(),
    };
    for call in [&classical_register, &mldsa_register] {
        assert_eq!(call.encode().first(), Some(&0));
        assert_eq!(
            Call::<Test>::decode(&mut call.encode().as_slice()).unwrap(),
            *call
        );
    }
    for call in [&classical_check, &mldsa_check] {
        assert_eq!(call.encode().first(), Some(&1));
        assert_eq!(
            Call::<Test>::decode(&mut call.encode().as_slice()).unwrap(),
            *call
        );
    }
    // Removed duplicate indices cannot silently alias another operation.
    assert!(Call::<Test>::decode(&mut &[2u8][..]).is_err());
    assert!(Call::<Test>::decode(&mut &[3u8][..]).is_err());
    assert_eq!(
        classical_register.get_dispatch_info().call_weight,
        <() as WeightInfo>::register()
    );
    assert_eq!(
        classical_check.get_dispatch_info().call_weight,
        <() as WeightInfo>::check_registered()
    );
    assert_ne!(
        classical_register.get_dispatch_info().call_weight,
        frame_support::weights::Weight::MAX
    );
    assert_ne!(
        classical_check.get_dispatch_info().call_weight,
        frame_support::weights::Weight::MAX
    );
    // The original calls select the measured scheme-specific weights.
    assert_ne!(
        <() as WeightInfo>::register_mldsa65(),
        frame_support::weights::Weight::MAX
    );
    assert_ne!(
        <() as WeightInfo>::check_registered_mldsa65(),
        frame_support::weights::Weight::MAX
    );
    assert_eq!(
        mldsa_register.get_dispatch_info().call_weight,
        <() as WeightInfo>::register_mldsa65()
    );
    assert_eq!(
        mldsa_check.get_dispatch_info().call_weight,
        <() as WeightInfo>::check_registered_mldsa65()
    );
}
