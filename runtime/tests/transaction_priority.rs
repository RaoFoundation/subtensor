//! Transaction pool priority through the full transaction extension pipeline.
//!
//! The Operational class of a call is fixed by its weight annotation, but most
//! Operational calls (root-only `frame_system` calls, some `admin-utils` calls, and
//! wrappers that inherit the inner call's class) check their origin only at dispatch.
//! Only the sudo key may get `OPERATIONAL_DISPATCH_PRIORITY`; any other signer gets
//! `NORMAL_DISPATCH_BASE_PRIORITY`, whatever the class of its call.
#![allow(clippy::unwrap_used)]

use codec::Encode;
use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use node_subtensor_runtime::{
    Address, BuildStorage, Executive, NORMAL_DISPATCH_BASE_PRIORITY, OPERATIONAL_DISPATCH_PRIORITY,
    RuntimeCall, RuntimeGenesisConfig, SignedPayload, System, SystemCall, TxExtension,
    UncheckedExtrinsic, check_mortality, check_nonce, check_nonzero_sender, sudo_wrapper,
    transaction_payment_wrapper,
};
use sp_core::{Pair, sr25519};
use sp_runtime::generic::Era;
use sp_runtime::transaction_validity::{
    InvalidTransaction, TransactionSource, TransactionValidity, TransactionValidityError,
};
use subtensor_runtime_common::{AccountId, NetUid, Signature, TaoBalance};

const BALANCE: TaoBalance = TaoBalance::new(1_000_000_000_000_u64);

fn sudo_pair() -> sr25519::Pair {
    sr25519::Pair::from_seed(&[1; 32])
}

fn other_pair() -> sr25519::Pair {
    sr25519::Pair::from_seed(&[2; 32])
}

fn account(pair: &sr25519::Pair) -> AccountId {
    AccountId::from(pair.public())
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
        balances: pallet_balances::GenesisConfig {
            balances: vec![
                (account(&sudo_pair()), BALANCE),
                (account(&other_pair()), BALANCE),
            ],
            dev_accounts: None,
        },
        sudo: pallet_sudo::GenesisConfig {
            key: Some(account(&sudo_pair())),
        },
        ..Default::default()
    }
    .build_storage()
    .unwrap()
    .into();
    ext.execute_with(|| System::set_block_number(1));
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

/// Sign `call` with `pair` and validate it as the transaction pool does.
fn validate_signed(pair: &sr25519::Pair, call: RuntimeCall) -> TransactionValidity {
    let who = account(pair);
    let extra = extra(System::account_nonce(&who));
    let signature = SignedPayload::new(call.clone(), extra.clone())
        .unwrap()
        .using_encoded(|payload| pair.sign(payload));
    let xt = UncheckedExtrinsic::new_signed(
        call,
        Address::Id(who),
        Signature::Sr25519(signature),
        extra,
    );
    Executive::validate_transaction(TransactionSource::External, xt, System::parent_hash())
}

fn set_heap_pages() -> RuntimeCall {
    RuntimeCall::System(SystemCall::set_heap_pages { pages: 64 })
}

fn sudo(call: RuntimeCall) -> RuntimeCall {
    RuntimeCall::Sudo(pallet_sudo::Call::sudo {
        call: Box::new(call),
    })
}

fn assert_operational(call: &RuntimeCall) {
    assert_eq!(call.get_dispatch_info().class, DispatchClass::Operational);
}

#[test]
fn sudo_key_keeps_operational_priority() {
    new_test_ext().execute_with(|| {
        let call = sudo(set_heap_pages());
        assert_operational(&call);

        let valid = validate_signed(&sudo_pair(), call).unwrap();
        assert_eq!(valid.priority, OPERATIONAL_DISPATCH_PRIORITY);
    });
}

#[test]
fn sudo_key_normal_call_keeps_normal_priority() {
    new_test_ext().execute_with(|| {
        let call = sudo(RuntimeCall::System(SystemCall::remark { remark: vec![1] }));
        assert_eq!(call.get_dispatch_info().class, DispatchClass::Normal);

        let valid = validate_signed(&sudo_pair(), call).unwrap();
        assert_eq!(valid.priority, NORMAL_DISPATCH_BASE_PRIORITY);
    });
}

// Keep non-sudo callers from obtaining elevated Operational pool priority for MEV.
// PROTECTED_REGRESSION: changes require explicit human approval.
#[test]
fn root_only_operational_call_from_other_signer_gets_normal_priority() {
    new_test_ext().execute_with(|| {
        let call = set_heap_pages();
        assert_operational(&call);

        let valid = validate_signed(&other_pair(), call).unwrap();
        assert_eq!(valid.priority, NORMAL_DISPATCH_BASE_PRIORITY);
    });
}

#[test]
fn origin_free_operational_call_from_other_signer_gets_normal_priority() {
    new_test_ext().execute_with(|| {
        let call = RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_owner_cut_enabled {
            netuid: NetUid::from(1),
            enabled: true,
        });
        assert_operational(&call);

        let valid = validate_signed(&other_pair(), call).unwrap();
        assert_eq!(valid.priority, NORMAL_DISPATCH_BASE_PRIORITY);
    });
}

#[test]
fn class_inheriting_multisig_from_other_signer_gets_normal_priority() {
    new_test_ext().execute_with(|| {
        let other_signatories = vec![AccountId::new([9; 32])];
        let calls = [
            RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
                other_signatories: other_signatories.clone(),
                call: Box::new(set_heap_pages()),
            }),
            RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
                other_signatories: other_signatories.clone(),
                call: Box::new(sudo(set_heap_pages())),
            }),
            RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1_safe {
                other_signatories,
                call: Box::new(sudo(set_heap_pages())),
            }),
        ];
        for call in calls {
            assert_operational(&call);

            let valid = validate_signed(&other_pair(), call).unwrap();
            assert_eq!(valid.priority, NORMAL_DISPATCH_BASE_PRIORITY);
        }
    });
}

#[test]
fn top_level_sudo_from_other_signer_is_rejected() {
    new_test_ext().execute_with(|| {
        assert_eq!(
            validate_signed(&other_pair(), sudo(set_heap_pages())),
            Err(TransactionValidityError::Invalid(
                InvalidTransaction::BadSigner
            ))
        );
    });
}
