#![allow(clippy::expect_used)]

use frame_support::{
    assert_ok,
    dispatch::{DispatchResultWithPostInfo, GetDispatchInfo},
    traits::fungible::Mutate,
};
use node_subtensor_runtime::{
    Balances, BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, System,
    TxExtension, check_mortality, check_nonce, check_nonzero_sender, sudo_wrapper,
    transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use pallet_subtensor::{self as st, Pallet as Subtensor};
use sp_runtime::{
    generic::Era,
    traits::{DispatchTransaction, Dispatchable, TransactionExtension},
    transaction_validity::{InvalidTransaction, TransactionValidityError},
};
use subtensor_runtime_common::{AccountId, CustomTransactionError, NetUid, TaoBalance};

fn hotkey() -> AccountId {
    AccountId::new([7; 32])
}
fn coldkey() -> AccountId {
    AccountId::new([8; 32])
}

fn new_test_ext(fund_coldkey: bool) -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("genesis storage")
        .into();
    ext.execute_with(|| {
        System::set_block_number(2);
        let netuid = NetUid::from(1);
        Subtensor::<Runtime>::init_new_network(netuid, 360);
        st::SubnetOwner::<Runtime>::insert(netuid, coldkey());
        st::SubnetOwnerHotkey::<Runtime>::insert(netuid, hotkey());
        st::Owner::<Runtime>::insert(hotkey(), coldkey());
        Subtensor::<Runtime>::append_neuron(netuid, &hotkey(), 1);
        Subtensor::<Runtime>::append_neuron(netuid, &AccountId::new([9; 32]), 1);
        assert_ok!(Subtensor::<Runtime>::do_set_null_consensus(netuid, true));
        Subtensor::<Runtime>::set_weights_set_rate_limit(netuid, 100);
        if fund_coldkey {
            assert_ok!(Balances::mint_into(
                &coldkey(),
                TaoBalance::new(1_000_000_000_000_000)
            ));
        }
        assert!(!frame_system::Account::<Runtime>::contains_key(hotkey()));
    });
    ext
}

fn extension(nonce: u32) -> TxExtension {
    (
        (
            check_nonzero_sender::CheckNonZeroSender::<Runtime>::new(),
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            check_mortality::CheckMortality::<Runtime>::from(Era::Immortal),
            check_nonce::CheckNonce::<Runtime>::from(nonce),
            frame_system::CheckWeight::<Runtime>::new(),
        ),
        (
            ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0)),
            sudo_wrapper::SudoTransactionExtension::<Runtime>::new(),
            pallet_shield::CheckShieldedTxValidity::<Runtime>::new(),
            st::SubtensorTransactionExtension::<Runtime>::new(),
            pallet_drand::drand_priority::DrandPriority::<Runtime>::new(),
        ),
        frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
    )
}

fn score_call(dests: Vec<u16>, weights: Vec<u32>) -> RuntimeCall {
    RuntimeCall::SubtensorModule(st::Call::set_null_weights {
        netuid: NetUid::from(1),
        dests,
        weights,
        version_key: 0,
    })
}

fn submit(
    nonce: u32,
    call: RuntimeCall,
) -> Result<DispatchResultWithPostInfo, TransactionValidityError> {
    let ext = extension(nonce);
    let mut info = call.get_dispatch_info();
    info.extension_weight = ext.weight(&call);
    ext.test_run(
        RuntimeOrigin::signed(hotkey()),
        &call,
        &info,
        0,
        0,
        |origin| call.clone().dispatch(origin),
    )
}

#[test]
fn retired_null_scores_cannot_charge_the_coldkey() {
    new_test_ext(true).execute_with(|| {
        let before = Balances::free_balance(coldkey());
        assert_eq!(
            submit(0, score_call(vec![1], vec![u32::MAX])),
            Err(CustomTransactionError::BadRequest.into())
        );
        assert_eq!(Balances::free_balance(coldkey()), before);
        assert!(!frame_system::Account::<Runtime>::contains_key(hotkey()));
        assert!(!st::NullWeights::<Runtime>::contains_key(
            NetUid::from(1),
            0
        ));
    });
}

#[test]
fn coldkey_payment_exemption_still_requires_a_real_funded_payer() {
    for has_owner in [false, true] {
        new_test_ext(false).execute_with(|| {
            if !has_owner {
                st::Owner::<Runtime>::remove(hotkey());
            }
            assert_eq!(
                submit(0, score_call(vec![1], vec![1])),
                Err(InvalidTransaction::Payment.into())
            );
            assert!(!frame_system::Account::<Runtime>::contains_key(hotkey()));
        });
    }
}

#[test]
fn unrelated_calls_cannot_use_the_coldkeys_balance_for_nonce_admission() {
    new_test_ext(true).execute_with(|| {
        let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
        assert_eq!(submit(0, call), Err(InvalidTransaction::Payment.into()));
        assert!(!frame_system::Account::<Runtime>::contains_key(hotkey()));
    });
}
