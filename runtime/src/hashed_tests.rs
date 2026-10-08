#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use crate::{hashed_auth::AuthorizeAccount, hashed_extrinsic::LegacyUnchecked};
use frame_support::{dispatch::GetDispatchInfo, traits::Currency};
use sp_core::{Pair, sr25519};
use sp_runtime::{
    generic::Era,
    traits::{Checkable, TransactionExtension},
};
use subtensor_hashed::{Descriptor, Proof, Scheme};
fn ext() -> sp_io::TestExternalities {
    let mut ext = sp_io::TestExternalities::default();
    ext.execute_with(|| frame_system::BlockHash::<Runtime>::insert(0, H256::zero()));
    ext
}

fn extra(nonce: u32) -> TxExtension {
    (
        (
            check_nonzero_sender::CheckNonZeroSender::new(),
            frame_system::CheckSpecVersion::new(),
            frame_system::CheckTxVersion::new(),
            frame_system::CheckGenesis::new(),
            check_mortality::CheckMortality::from(Era::Immortal),
            check_nonce::CheckNonce::from(nonce),
            frame_system::CheckWeight::new(),
        ),
        (
            transaction_payment_wrapper::ChargeTransactionPaymentWrapper::new(TaoBalance::new(0)),
            sudo_wrapper::SudoTransactionExtension::new(),
            pallet_shield::CheckShieldedTxValidity::new(),
            pallet_subtensor::SubtensorTransactionExtension::new(),
            pallet_drand::drand_priority::DrandPriority::new(),
        ),
        frame_metadata_hash_extension::CheckMetadataHash::new(false),
    )
}

fn call() -> RuntimeCall {
    RuntimeCall::System(SystemCall::remark {
        remark: b"hashed wire vector".to_vec(),
    })
}

fn pair(generation: u8) -> sr25519::Pair {
    sr25519::Pair::from_seed(&[generation; 32])
}
fn descriptor() -> Descriptor {
    Descriptor {
        version: 1,
        scheme: Scheme::Sr25519,
        initial_commitment: subtensor_hashed::key_commitment(Scheme::Sr25519, &pair(10).public().0),
    }
}
fn account() -> AccountId {
    AccountId::new(subtensor_hashed::account_id(&descriptor()))
}
fn setup() {
    HashedEnabled::set(true);
    hashed_auth::TestVerificationWeight::set(Weight::zero());
    pallet_hashed_accounts::Accounts::<Runtime>::insert(
        account(),
        pallet_hashed_accounts::AccountRecord {
            descriptor: descriptor(),
            generation: 0,
            commitment: descriptor().initial_commitment,
        },
    );
    System::inc_providers(&account());
    let _ = Balances::make_free_balance_be(&account(), TaoBalance::new(1_000_000_000_000));
}
fn signed(call: RuntimeCall, generation: u64, key: &sr25519::Pair) -> UncheckedExtrinsic {
    signed_nonce(call, generation, key, System::account_nonce(account()))
}
fn signed_nonce(
    call: RuntimeCall,
    generation: u64,
    key: &sr25519::Pair,
    nonce: u32,
) -> UncheckedExtrinsic {
    let extra = extra(nonce);
    let implication = (1u8, &call, &extra, extra.implicit().unwrap()).encode();
    let next_commitment = subtensor_hashed::key_commitment(
        Scheme::Sr25519,
        &pair(u8::try_from(generation).unwrap().checked_add(11).unwrap())
            .public()
            .0,
    );
    let payload = subtensor_hashed::transaction_payload(
        account().as_ref(),
        Scheme::Sr25519,
        generation,
        &next_commitment,
        &implication,
    );
    let proof = Proof {
        generation,
        public_key: key.public().0,
        next_commitment,
        signature: key.sign(&payload).0,
    };
    UncheckedExtrinsic::new_hashed(
        call,
        AuthorizeAccount {
            account: account(),
            proof,
        },
        extra,
    )
}
fn legacy(key: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
    let who = AccountId::from(key.public());
    let extra = extra(System::account_nonce(&who));
    let signature = SignedPayload::new(call.clone(), extra.clone())
        .unwrap()
        .using_encoded(|m| key.sign(m));
    UncheckedExtrinsic::new_signed(call, who.into(), Signature::Sr25519(signature), extra)
}
#[test]
fn wire_roundtrip_preserves_legacy_and_rejects_unknown_pipeline() {
    ext().execute_with(|| {
        setup();
        let xt = signed(call(), 0, &pair(10));
        let bytes = xt.encode();
        assert_eq!(UncheckedExtrinsic::decode(&mut &bytes[..]).unwrap(), xt);
        assert_eq!(
            xt.get_dispatch_info().extension_weight,
            xt.clone()
                .check(&ChainContext::default())
                .unwrap()
                .get_dispatch_info()
                .extension_weight
        );
        let old = LegacyUnchecked::new_signed(
            call(),
            AccountId::from(pair(1).public()).into(),
            Signature::Sr25519(pair(1).sign(b"x")),
            extra(0),
        );
        assert_eq!(
            UncheckedExtrinsic::Legacy(old.clone()).encode(),
            old.encode()
        );
        let general: LegacyUnchecked = sp_runtime::generic::UncheckedExtrinsic {
            preamble: sp_runtime::generic::Preamble::General(0, extra(0)),
            function: call(),
        }
        .into();
        assert_eq!(
            UncheckedExtrinsic::decode(&mut &general.encode()[..]).unwrap(),
            UncheckedExtrinsic::Legacy(general)
        );
        let mut input = &bytes[..];
        let _ = codec::Compact::<u32>::decode(&mut input).unwrap();
        let offset = bytes.len() - input.len();
        let mut changed = bytes;
        changed[offset + 1] = 9;
        assert!(UncheckedExtrinsic::decode(&mut &changed[..]).is_err());
    });
}
#[test]
fn included_failure_advances_generation_but_invalid_proof_does_not() {
    ext().execute_with(|| {
        setup();
        let failing = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
            dest: AccountId::new([25; 32]).into(),
            value: TaoBalance::new(u64::MAX),
        });
        let xt = signed(failing.clone(), 0, &pair(10));
        let invalid = signed(failing, 0, &pair(9));
        assert!(Executive::apply_extrinsic(invalid).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert_eq!(System::account_nonce(account()), 0);
        assert!(Executive::apply_extrinsic(xt.clone()).unwrap().is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
        assert_eq!(System::account_nonce(account()), 1);
        assert!(Executive::apply_extrinsic(xt).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
    });
}
#[test]
fn rejected_fee_or_weight_does_not_rotate() {
    ext().execute_with(|| {
        setup();
        hashed_auth::TestVerificationWeight::set(Weight::MAX);
        assert!(Executive::apply_extrinsic(signed(call(), 0, &pair(10))).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert_eq!(System::account_nonce(account()), 0);
    });
}
#[test]
fn sponsor_can_register_without_revealing_signer_and_fund_atomically() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let sponsor_id = AccountId::from(sponsor.public());
        let _ = Balances::make_free_balance_be(&sponsor_id, TaoBalance::new(1_000_000_000_000));
        let call = RuntimeCall::Utility(pallet_utility::Call::batch_all {
            calls: alloc::vec![
                RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                    descriptor: descriptor()
                }),
                RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                    dest: account().into(),
                    value: TaoBalance::new(1_000_000_000)
                }),
            ],
        });
        frame_support::assert_ok!(Executive::apply_extrinsic(legacy(&sponsor, call)).unwrap());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 0);
        assert_eq!(System::account_nonce(account()), 1);
        assert_eq!(
            Balances::free_balance(account()),
            TaoBalance::new(1_000_000_000)
        );
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(super::hashed_tests::call(), 0, &pair(10))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
    });
}
#[test]
fn registered_identity_cannot_use_legacy_signature_path() {
    ext().execute_with(|| {
        setup();
        let key = pair(20);
        let who = AccountId::from(key.public());
        pallet_hashed_accounts::Accounts::<Runtime>::insert(
            &who,
            pallet_hashed_accounts::AccountRecord {
                descriptor: descriptor(),
                generation: 0,
                commitment: descriptor().initial_commitment,
            },
        );
        assert!(Executive::apply_extrinsic(legacy(&key, call())).is_err());
    });
}

#[test]
fn hashed_accounts_only_accept_hashed_proxy_delegates() {
    ext().execute_with(|| {
        setup();
        let delegate = AccountId::from(pair(30).public());
        frame_support::assert_noop!(
            Proxy::add_proxy_delegate(&account(), delegate.clone(), ProxyType::Any, 0),
            pallet_proxy::Error::<Runtime>::AccountPolicyViolation
        );
        pallet_hashed_accounts::Accounts::<Runtime>::insert(
            &delegate,
            pallet_hashed_accounts::AccountRecord {
                descriptor: descriptor(),
                generation: 0,
                commitment: descriptor().initial_commitment,
            },
        );
        frame_support::assert_ok!(Proxy::add_proxy_delegate(
            &account(),
            delegate.clone(),
            ProxyType::Any,
            0,
        ));
        assert!(Proxy::find_proxy(&account(), &delegate, None).is_ok());
        // Even a stale grant cannot authorize an unprotected delegate.
        pallet_hashed_accounts::Accounts::<Runtime>::remove(&delegate);
        assert!(Proxy::find_proxy(&account(), &delegate, None).is_err());
    });
}

#[cfg(feature = "runtime-benchmarks")]
#[test]
fn registration_benchmark_context_covers_outer_decode_and_proxy_cleanup() {
    use pallet_hashed_accounts::OnRegister;
    ext().execute_with(|| {
        HashedEnabled::set(true);
        let sponsor = AccountId::from(pair(31).public());
        let _ = Balances::make_free_balance_be(&sponsor, TaoBalance::new(1_000_000_000_000));
        frame_support::assert_ok!(hashed_auth::OnHashedRegistered::setup_benchmark(
            &account(),
            &sponsor,
            &descriptor(),
        ));
        assert_eq!(
            Proxy::proxies(account()).0.len(),
            MaxProxies::get() as usize
        );
        assert!(System::extrinsic_data(0).len() > 16_000);
        frame_support::assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor),
            descriptor()
        ));
        assert!(Proxy::proxies(account()).0.is_empty());
    });
}

#[test]
fn successive_generations_keep_the_account_and_recover_without_secret_history() {
    ext().execute_with(|| {
        setup();
        let stable = account();
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 0, &pair(10))).unwrap()
        );
        let current = HashedAccounts::accounts(&stable).unwrap();
        assert_eq!(current.generation, 1);
        assert_eq!(
            current.commitment,
            subtensor_hashed::key_commitment(Scheme::Sr25519, &pair(11).public().0)
        );
        frame_support::assert_ok!(
            Executive::apply_extrinsic(signed(call(), 1, &pair(11))).unwrap()
        );
        assert_eq!(HashedAccounts::accounts(stable).unwrap().generation, 2);
    });
}
#[test]
fn future_nonce_preparation_failure_rolls_back_rotation() {
    ext().execute_with(|| {
        setup();
        let before = HashedAccounts::accounts(account()).unwrap();
        let balance = Balances::free_balance(account());
        assert!(Executive::apply_extrinsic(signed_nonce(call(), 0, &pair(10), 5)).is_err());
        assert_eq!(HashedAccounts::accounts(account()).unwrap(), before);
        assert_eq!(System::account_nonce(account()), 0);
        assert_eq!(Balances::free_balance(account()), balance);
    });
}
#[test]
fn setup_through_derivative_wrapper_cannot_change_authorization() {
    ext().execute_with(|| {
        HashedEnabled::set(true);
        hashed_auth::TestVerificationWeight::set(Weight::zero());
        let sponsor = pair(22);
        let _ = Balances::make_free_balance_be(
            &AccountId::from(sponsor.public()),
            TaoBalance::new(1_000_000_000_000),
        );
        let registration = RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
            descriptor: descriptor(),
        });
        let call = RuntimeCall::Utility(pallet_utility::Call::as_derivative {
            index: 0,
            call: alloc::boxed::Box::new(registration),
        });
        assert!(
            Executive::apply_extrinsic(legacy(&sponsor, call))
                .unwrap()
                .is_err()
        );
        assert!(!HashedAccounts::is_registered(&account()));
    });
}
