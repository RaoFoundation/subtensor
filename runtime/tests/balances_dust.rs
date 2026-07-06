#![allow(clippy::unwrap_used)]

use frame_support::traits::fungible::{Inspect, Mutate};
use frame_support::traits::tokens::Preservation;
use node_subtensor_runtime::{Balances, BuildStorage, Runtime, RuntimeGenesisConfig};
use sp_core::crypto::AccountId32;
use subtensor_runtime_common::TaoBalance;

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
        ..Default::default()
    }
    .build_storage()
    .unwrap()
    .into();
    ext.execute_with(|| frame_system::Pallet::<Runtime>::set_block_number(1));
    ext
}

fn add_balance_to_account(account: &AccountId32, tao: TaoBalance) {
    let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(tao);
    let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(account, credit, tao).unwrap();
}

#[test]
fn balances_dust_removal_updates_subtensor_total_issuance() {
    new_test_ext().execute_with(|| {
        let origin = AccountId32::new([1u8; 32]);
        let destination = AccountId32::new([2u8; 32]);
        let existential_deposit = TaoBalance::from(500u64);
        let dust = TaoBalance::from(1u64);
        let transfer_amount = existential_deposit;

        add_balance_to_account(&origin, transfer_amount + dust);

        let balances_issuance_before = Balances::total_issuance();
        let subtensor_issuance_before = pallet_subtensor::Pallet::<Runtime>::get_total_issuance();
        assert_eq!(balances_issuance_before, subtensor_issuance_before);

        <Balances as Mutate<AccountId32>>::transfer(
            &origin,
            &destination,
            transfer_amount,
            Preservation::Expendable,
        )
        .unwrap();

        assert_eq!(Balances::total_balance(&origin), 0u64.into());
        assert_eq!(Balances::total_balance(&destination), transfer_amount);
        assert_eq!(balances_issuance_before - Balances::total_issuance(), dust);
        assert_eq!(
            subtensor_issuance_before - pallet_subtensor::Pallet::<Runtime>::get_total_issuance(),
            dust
        );
        assert_eq!(
            Balances::total_issuance(),
            pallet_subtensor::Pallet::<Runtime>::get_total_issuance()
        );
    });
}
