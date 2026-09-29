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
fn fresh_hotkey_submits_scores_with_coldkey_fees_and_persistent_nonce() {
    new_test_ext(true).execute_with(|| {
        let before = Balances::free_balance(coldkey());
        let call = score_call(vec![1], vec![u32::MAX]);
        assert_ok!(submit(0, call.clone()).expect("transaction admitted"));
        assert_eq!(
            st::NullWeights::<Runtime>::get(NetUid::from(1), 0),
            vec![(1, u32::MAX)]
        );
        assert!(Balances::free_balance(coldkey()) < before);
        assert_eq!(Balances::free_balance(hotkey()), TaoBalance::new(0));
        let account = frame_system::Account::<Runtime>::get(hotkey());
        assert_eq!(
            (account.nonce, account.providers, account.sufficients),
            (1, 0, 1)
        );
        let after = Balances::free_balance(coldkey());
        assert_eq!(
            submit(1, call.clone()),
            Err(CustomTransactionError::RateLimitExceeded.into())
        );
        assert_eq!(submit(0, call), Err(InvalidTransaction::Stale.into()));
        assert_eq!(Balances::free_balance(coldkey()), after);
        assert_eq!(System::account_nonce(hotkey()), 1);
    });
}

#[test]
fn invalid_scores_are_rejected_before_fees_or_nonce_writes() {
    for case in [
        "missing_subnet",
        "disabled",
        "unregistered",
        "low_stake",
        "no_permit",
        "version",
        "rate_limit",
        "reset",
        "lengths",
        "too_many",
        "duplicate",
        "destination",
        "zero",
        "minimum",
        "row_cap",
    ] {
        new_test_ext(true).execute_with(|| {
            let netuid = NetUid::from(1);
            let mut dests = vec![1];
            let mut weights = vec![u32::MAX];
            let mut expected = CustomTransactionError::BadRequest;
            match case {
                "missing_subnet" => {
                    st::NetworksAdded::<Runtime>::remove(netuid);
                    expected = CustomTransactionError::SubnetNotExists;
                }
                "disabled" => st::NullConsensus::<Runtime>::insert(netuid, false),
                "unregistered" => {
                    st::Uids::<Runtime>::remove(netuid, hotkey());
                    expected = CustomTransactionError::UidNotFound;
                }
                "low_stake" | "no_permit" => {
                    st::SubnetOwnerHotkey::<Runtime>::insert(netuid, AccountId::new([9; 32]));
                    Subtensor::<Runtime>::set_stake_threshold(if case == "low_stake" {
                        1
                    } else {
                        0
                    });
                    if case == "low_stake" {
                        expected = CustomTransactionError::StakeAmountTooLow;
                    }
                }
                "version" => st::WeightsVersionKey::<Runtime>::insert(netuid, 1),
                "rate_limit" => {
                    st::NullLastUpdate::<Runtime>::insert(netuid, 0, 2);
                    expected = CustomTransactionError::RateLimitExceeded;
                }
                "reset" => {
                    st::NullWeightsResetAt::<Runtime>::insert(netuid, 2);
                    expected = CustomTransactionError::RateLimitExceeded;
                }
                "lengths" => weights.clear(),
                "too_many" => {
                    dests = vec![0, 1, 2];
                    weights = vec![1; 3];
                }
                "duplicate" => {
                    dests = vec![1, 1];
                    weights = vec![1; 2];
                }
                "destination" => dests = vec![2],
                "zero" => weights = vec![0],
                "minimum" => Subtensor::<Runtime>::set_min_allowed_weights(netuid, 2),
                "row_cap" => {
                    for uid in 1..=65 {
                        st::NullWeights::<Runtime>::insert(netuid, uid, vec![(1, 1u32)]);
                    }
                }
                _ => unreachable!(),
            }
            let before = Balances::free_balance(coldkey());
            assert_eq!(
                submit(0, score_call(dests, weights)),
                Err(expected.into()),
                "{case}"
            );
            assert_eq!(Balances::free_balance(coldkey()), before, "{case}");
            assert!(
                !frame_system::Account::<Runtime>::contains_key(hotkey()),
                "{case}"
            );
            assert!(
                !st::NullWeights::<Runtime>::contains_key(netuid, 0),
                "{case}"
            );
        });
    }
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
