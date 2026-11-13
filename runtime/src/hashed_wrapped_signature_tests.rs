//! Classical raw and Ledger-style wrapped authorizations use the same account lifecycle.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use frame_support::traits::Currency;
use sp_core::{Pair, ed25519, sr25519};
use sp_runtime::{
    generic::{Era, Preamble},
    traits::TransactionExtension,
    transaction_validity::InvalidTransaction,
};
use subtensor_hashed::{Descriptor, Scheme, SignerAddress, VERSION, WRAPPED_SIGNER_VERSION};

enum Key {
    Sr(Box<sr25519::Pair>),
    Ed(Box<ed25519::Pair>),
}

impl Key {
    fn new(scheme: Scheme, generation: u8) -> Self {
        let seed = [generation.checked_add(10).unwrap(); 32];
        match scheme {
            Scheme::Sr25519 => Self::Sr(Box::new(sr25519::Pair::from_seed(&seed))),
            Scheme::Ed25519 => Self::Ed(Box::new(ed25519::Pair::from_seed(&seed))),
        }
    }

    fn public(&self) -> [u8; 32] {
        match self {
            Self::Sr(key) => key.public().0,
            Self::Ed(key) => key.public().0,
        }
    }

    fn sign(&self, message: &[u8]) -> Signature {
        match self {
            Self::Sr(key) => Signature::Sr25519(key.sign(message)),
            Self::Ed(key) => Signature::Ed25519(key.sign(message)),
        }
    }
}

fn ext() -> sp_io::TestExternalities {
    let mut ext = sp_io::TestExternalities::default();
    ext.execute_with(|| frame_system::BlockHash::<Runtime>::insert(0, H256::zero()));
    ext
}

fn descriptor(scheme: Scheme) -> Descriptor {
    Descriptor {
        version: VERSION,
        scheme,
        initial_commitment: subtensor_hashed::key_commitment(scheme, &Key::new(scheme, 0).public()),
    }
}

fn account(scheme: Scheme) -> AccountId {
    AccountId::new(subtensor_hashed::account_id(&descriptor(scheme)))
}

fn setup(scheme: Scheme) {
    pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
    hashed_auth::TestVerificationWeight::set(Weight::zero());
    pallet_hashed_accounts::Accounts::<Runtime>::insert(
        account(scheme),
        pallet_hashed_accounts::AccountRecord {
            descriptor: descriptor(scheme).into(),
            generation: 0,
            commitment: descriptor(scheme).initial_commitment,
        },
    );
    System::inc_providers(&account(scheme));
    let _ = Balances::make_free_balance_be(&account(scheme), TaoBalance::new(1_000_000_000_000));
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
        remark: b"wrapped classical authorization".to_vec(),
    })
}

fn signed_with(
    scheme: Scheme,
    version: u8,
    call: RuntimeCall,
    generation: u8,
    message: impl FnOnce(&SignerAddress, &[u8]) -> Vec<u8>,
) -> UncheckedExtrinsic {
    let key = Key::new(scheme, generation);
    let address = SignerAddress {
        account: *account(scheme).as_ref(),
        version,
        scheme,
        generation: <u64 as From<u8>>::from(generation),
        public_key: key.public(),
        next_commitment: subtensor_hashed::key_commitment(
            scheme,
            &Key::new(scheme, generation.checked_add(1).unwrap()).public(),
        ),
    };
    let extra = extra(System::account_nonce(account(scheme)));
    let payload = (&call, &extra, extra.implicit().unwrap()).encode();
    let signature = key.sign(&message(&address, &payload));
    UncheckedExtrinsic::new_signed(call, Address::Raw(address.encode()), signature, extra)
}

fn signed(scheme: Scheme, version: u8, call: RuntimeCall, generation: u8) -> UncheckedExtrinsic {
    signed_with(scheme, version, call, generation, |address, payload| {
        address.signing_message(payload).unwrap().as_ref().to_vec()
    })
}

fn state(scheme: Scheme) -> (pallet_hashed_accounts::AccountRecord, u32, TaoBalance) {
    (
        HashedAccounts::accounts(account(scheme)).unwrap(),
        System::account_nonce(account(scheme)),
        Balances::free_balance(account(scheme)),
    )
}

#[test]
fn raw_and_wrapped_modes_rotate_one_account_and_charge_equal_fees() {
    assert_eq!(
        HashedAccounts::classical_signer_versions(),
        [VERSION, WRAPPED_SIGNER_VERSION]
    );
    for scheme in [Scheme::Sr25519, Scheme::Ed25519] {
        ext().execute_with(|| {
            setup(scheme);
            let mut fees = Vec::new();
            for (generation, version) in [VERSION, WRAPPED_SIGNER_VERSION, VERSION]
                .into_iter()
                .enumerate()
            {
                let generation = u8::try_from(generation).unwrap();
                let before = Balances::free_balance(account(scheme));
                let transaction = signed(scheme, version, call(), generation);
                let encoded = transaction.encode();
                let decoded = UncheckedExtrinsic::decode(&mut encoded.as_slice()).unwrap();
                assert_eq!(decoded, transaction);
                frame_support::assert_ok!(Executive::apply_extrinsic(decoded).unwrap());
                let record = HashedAccounts::accounts(account(scheme)).unwrap();
                assert_eq!(record.descriptor, descriptor(scheme).into());
                assert_eq!(record.generation, <u64 as From<u8>>::from(generation) + 1);
                assert_eq!(
                    System::account_nonce(account(scheme)),
                    <u32 as From<u8>>::from(generation) + 1
                );
                assert_eq!(
                    record.commitment,
                    subtensor_hashed::key_commitment(
                        scheme,
                        &Key::new(scheme, generation + 1).public()
                    )
                );
                let after = Balances::free_balance(account(scheme));
                assert!(after < before);
                fees.push(before - after);
            }
            assert!(fees.windows(2).all(|pair| pair[0] == pair[1]));
        });
    }
}

#[test]
fn wrapped_authorization_cannot_be_replayed_or_used_while_disabled() {
    for scheme in [Scheme::Sr25519, Scheme::Ed25519] {
        ext().execute_with(|| {
            setup(scheme);
            let transaction = signed(scheme, WRAPPED_SIGNER_VERSION, call(), 0);
            let before = state(scheme);
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(false);
            assert!(Executive::apply_extrinsic(transaction.clone()).is_err());
            assert_eq!(state(scheme), before);
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);
            frame_support::assert_ok!(Executive::apply_extrinsic(transaction.clone()).unwrap());
            let after = state(scheme);
            assert!(Executive::apply_extrinsic(transaction).is_err());
            assert_eq!(state(scheme), after);
        });
    }
}

#[test]
fn wrapped_signatures_bind_payload_mode_and_rotation_fields() {
    for scheme in [Scheme::Sr25519, Scheme::Ed25519] {
        ext().execute_with(|| {
            setup(scheme);
            let original = signed(scheme, WRAPPED_SIGNER_VERSION, call(), 0);
            let before = state(scheme);
            for field in [
                "call",
                "nonce",
                "tip",
                "account",
                "mode",
                "generation",
                "public_key",
                "scheme",
                "next_commitment",
            ] {
                let mut changed = original.clone();
                let UncheckedExtrinsic::Legacy(transaction) = &mut changed;
                let Preamble::Signed(Address::Raw(raw), _, extensions) =
                    &mut transaction.0.preamble
                else {
                    panic!("expected a signed raw address");
                };
                let mut address = SignerAddress::from_bytes(raw).unwrap();
                match field {
                    "call" => {
                        transaction.0.function = RuntimeCall::System(SystemCall::remark {
                            remark: b"changed".to_vec(),
                        })
                    }
                    "nonce" => extensions.0.0.5 = check_nonce::CheckNonce::from(1),
                    "tip" => {
                        extensions.0.1.0 =
                            transaction_payment_wrapper::ChargeTransactionPaymentWrapper::new(
                                TaoBalance::new(1),
                            )
                    }
                    "account" => address.account = [99; 32],
                    "mode" => address.version = VERSION,
                    "generation" => address.generation = 1,
                    "public_key" => address.public_key = Key::new(scheme, 9).public(),
                    "scheme" => {
                        address.scheme = match scheme {
                            Scheme::Sr25519 => Scheme::Ed25519,
                            Scheme::Ed25519 => Scheme::Sr25519,
                        }
                    }
                    "next_commitment" => address.next_commitment = [99; 32],
                    _ => unreachable!(),
                }
                *raw = address.encode();
                assert_eq!(
                    Executive::apply_extrinsic(changed),
                    Err(InvalidTransaction::BadProof.into()),
                    "mutated {field}"
                );
                assert_eq!(state(scheme), before);
            }
            frame_system::BlockHash::<Runtime>::insert(0, H256::repeat_byte(7));
            assert_eq!(
                Executive::apply_extrinsic(original.clone()),
                Err(InvalidTransaction::BadProof.into())
            );
            assert_eq!(state(scheme), before);
            frame_system::BlockHash::<Runtime>::insert(0, H256::zero());
            frame_support::assert_ok!(Executive::apply_extrinsic(original).unwrap());
        });
    }
}

#[test]
fn wrapped_mode_rejects_missing_wrong_and_double_wrappers() {
    for scheme in [Scheme::Sr25519, Scheme::Ed25519] {
        ext().execute_with(|| {
            setup(scheme);
            let before = state(scheme);
            for wrapper in ["missing", "lowercase", "double", "payload", "raw_mode_hash"] {
                let invalid = signed_with(
                    scheme,
                    WRAPPED_SIGNER_VERSION,
                    call(),
                    0,
                    |address, payload| {
                        let hash = address.signing_hash(payload);
                        match wrapper {
                            "missing" => hash.to_vec(),
                            "lowercase" => [b"<bytes>".as_slice(), &hash, b"</bytes>"].concat(),
                            "double" => [
                                b"<Bytes>".as_slice(),
                                address.signing_message(payload).unwrap().as_ref(),
                                b"</Bytes>",
                            ]
                            .concat(),
                            "payload" => [b"<Bytes>".as_slice(), payload, b"</Bytes>"].concat(),
                            "raw_mode_hash" => {
                                let raw = SignerAddress {
                                    version: VERSION,
                                    ..address.clone()
                                };
                                [
                                    b"<Bytes>".as_slice(),
                                    &raw.signing_hash(payload),
                                    b"</Bytes>",
                                ]
                                .concat()
                            }
                            _ => unreachable!(),
                        }
                    },
                );
                assert_eq!(
                    Executive::apply_extrinsic(invalid),
                    Err(InvalidTransaction::BadProof.into()),
                    "wrapper {wrapper}"
                );
                assert_eq!(state(scheme), before);
            }
            // Wrapped bytes must not be accepted by the original raw signer mode either.
            let invalid = signed_with(scheme, VERSION, call(), 0, |address, payload| {
                [
                    b"<Bytes>".as_slice(),
                    &address.signing_hash(payload),
                    b"</Bytes>",
                ]
                .concat()
            });
            assert_eq!(
                Executive::apply_extrinsic(invalid),
                Err(InvalidTransaction::BadProof.into())
            );
            assert_eq!(state(scheme), before);
        });
    }
}

#[test]
fn included_wrapped_call_failure_charges_fees_and_rotates_once() {
    for scheme in [Scheme::Sr25519, Scheme::Ed25519] {
        ext().execute_with(|| {
            setup(scheme);
            let failing = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
                dest: AccountId::new([25; 32]).into(),
                value: TaoBalance::new(u64::MAX),
            });
            let transaction = signed(scheme, WRAPPED_SIGNER_VERSION, failing, 0);
            let before = state(scheme);
            assert!(
                Executive::apply_extrinsic(transaction.clone())
                    .unwrap()
                    .is_err()
            );
            let after = state(scheme);
            assert_eq!(after.0.generation, 1);
            assert_eq!(after.1, 1);
            assert!(after.2 < before.2);
            assert!(Executive::apply_extrinsic(transaction).is_err());
            assert_eq!(state(scheme), after);
            frame_support::assert_ok!(
                Executive::apply_extrinsic(signed(scheme, WRAPPED_SIGNER_VERSION, call(), 1))
                    .unwrap()
            );
            assert_eq!(
                HashedAccounts::accounts(account(scheme))
                    .unwrap()
                    .generation,
                2
            );
        });
    }
}
