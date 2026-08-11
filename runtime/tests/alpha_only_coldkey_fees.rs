//! A coldkey whose only holding is alpha stake (e.g. received via
//! `transfer_stake`, never funded with TAO) has no provider or sufficient
//! reference on its system account. The custom `CheckNonce` extension used to
//! reject every fee-paying extrinsic from such a signer with
//! `InvalidTransaction::Payment` ("Inability to pay some fees") before the
//! transaction-fee pallet's pay-in-alpha fallback was ever consulted — locking
//! the stake: even the `remove_stake` that would give the account TAO was
//! rejected.
//!
//! These tests reproduce that scenario end to end through the real
//! `transfer_stake` extrinsic and assert that `CheckNonce` now admits the
//! signer while fee validation (`ChargeTransactionPaymentWrapper`) accepts the
//! alpha-paid fee, and that accounts with no alpha at all are still rejected.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]

use frame_support::assert_ok;
use frame_support::dispatch::{GetDispatchInfo, Pays};
use frame_support::pallet_prelude::Zero;
use frame_support::traits::Get;
use node_subtensor_runtime::{
    BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, SubtensorModule,
    check_nonce, transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use sp_runtime::traits::{TransactionExtension, TxBaseImplication};
use sp_runtime::transaction_validity::{
    InvalidTransaction, TransactionSource, TransactionValidityError,
};
use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, TaoBalance, Token};

fn netuid() -> NetUid {
    NetUid::from(1)
}

fn origin_coldkey() -> AccountId {
    AccountId::from([1_u8; 32])
}

fn hotkey() -> AccountId {
    AccountId::from([2_u8; 32])
}

/// The coldkey under test: receives alpha, never holds TAO.
fn alpha_only_coldkey() -> AccountId {
    AccountId::from([3_u8; 32])
}

/// A coldkey with neither TAO nor alpha.
fn empty_coldkey() -> AccountId {
    AccountId::from([4_u8; 32])
}

fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| frame_system::Pallet::<Runtime>::set_block_number(1));
    ext
}

fn add_balance_to_coldkey_account(coldkey: &AccountId, tao: TaoBalance) {
    let credit = SubtensorModule::mint_tao(tao);
    let _ = SubtensorModule::spend_tao(coldkey, credit, tao);
}

/// Stand up a stable-mechanism subnet (1 TAO : 1 alpha, no AMM liquidity
/// needed) with a staked position for `origin_coldkey`, then move that whole
/// position to the TAO-less destination via the real `transfer_stake`
/// extrinsic — Tegridy's exact scenario from the Church of Rao report.
fn setup_alpha_only_coldkey() -> AlphaBalance {
    SubtensorModule::init_new_network(netuid(), 0);
    pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid(), 0u16);
    pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid(), true);

    let stake: AlphaBalance =
        (pallet_subtensor::DefaultMinStake::<Runtime>::get().to_u64() * 100).into();

    add_balance_to_coldkey_account(&origin_coldkey(), TaoBalance::from(stake.to_u64() * 10));
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid()).unwrap();
    add_balance_to_coldkey_account(&subnet_account, TaoBalance::from(stake.to_u64()));

    let _ = SubtensorModule::create_account_if_non_existent(&origin_coldkey(), &hotkey());
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey(),
        &origin_coldkey(),
        netuid(),
        stake,
    );

    assert_ok!(SubtensorModule::transfer_stake(
        RuntimeOrigin::signed(origin_coldkey()),
        alpha_only_coldkey(),
        hotkey(),
        netuid(),
        netuid(),
        stake,
    ));

    let received = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
        &hotkey(),
        &alpha_only_coldkey(),
        netuid(),
    );
    assert!(
        !received.is_zero(),
        "destination coldkey must hold the transferred alpha"
    );
    received
}

fn remove_stake_call(amount: AlphaBalance) -> RuntimeCall {
    RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
        hotkey: hotkey(),
        netuid: netuid(),
        amount_unstaked: amount,
    })
}

fn validate_check_nonce(
    who: AccountId,
    call: &RuntimeCall,
) -> Result<(), TransactionValidityError> {
    let ext = check_nonce::CheckNonce::<Runtime>::from(0);
    let info = call.get_dispatch_info();
    assert_eq!(
        info.pays_fee,
        Pays::Yes,
        "the guard under test only applies to fee-paying calls"
    );
    ext.validate(
        RuntimeOrigin::signed(who),
        call,
        &info,
        0,
        (),
        &TxBaseImplication(()),
        TransactionSource::External,
    )
    .map(|_| ())
}

#[test]
fn alpha_only_coldkey_can_submit_remove_stake() {
    new_test_ext().execute_with(|| {
        let received = setup_alpha_only_coldkey();

        // The destination coldkey never held TAO: no system account references.
        let account = frame_system::Account::<Runtime>::get(alpha_only_coldkey());
        assert_eq!(account.providers, 0);
        assert_eq!(account.sufficients, 0);

        let call = remove_stake_call(received);

        // CheckNonce must admit the alpha-holding signer...
        assert_ok!(validate_check_nonce(alpha_only_coldkey(), &call));

        // ...and fee validation accepts the transaction because the fee is
        // payable in alpha, so the extrinsic is valid end to end.
        let payment = ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0));
        let info = call.get_dispatch_info();
        assert_ok!(
            payment
                .validate(
                    RuntimeOrigin::signed(alpha_only_coldkey()),
                    &call,
                    &info,
                    0,
                    (),
                    &TxBaseImplication(()),
                    TransactionSource::External,
                )
                .map(|_| ())
        );
    });
}

#[test]
fn coldkey_without_tao_or_alpha_is_still_rejected() {
    new_test_ext().execute_with(|| {
        setup_alpha_only_coldkey();

        let call = remove_stake_call(AlphaBalance::from(1_000_000u64));
        assert_eq!(
            validate_check_nonce(empty_coldkey(), &call),
            Err(TransactionValidityError::Invalid(
                InvalidTransaction::Payment
            )),
            "the storage-bloat guard must still reject signers with no on-chain value"
        );
    });
}
