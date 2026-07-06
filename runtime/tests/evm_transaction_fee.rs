#![allow(clippy::expect_used, clippy::unwrap_used)]

use codec::Encode;
use frame_support::traits::fungible::Inspect;
use node_subtensor_runtime::{Aura, Balances, BuildStorage, Runtime, RuntimeGenesisConfig};
use pallet_evm::{AddressMapping, EvmBalance, OnChargeEVMTransaction};
use sp_consensus_aura::{AURA_ENGINE_ID, sr25519::AuthorityId as AuraId};
use sp_core::H160;
use sp_core::sr25519;
use subtensor_runtime_common::{AuthorshipInfo, TaoBalance};

fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig {
        ..Default::default()
    }
    .build_storage()
    .unwrap()
    .into();
    ext.execute_with(|| frame_system::Pallet::<Runtime>::set_block_number(1));
    ext
}

fn add_balance_to_coldkey_account(coldkey: &sp_core::crypto::AccountId32, tao: TaoBalance) {
    let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(tao);
    let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(coldkey, credit, tao).unwrap();
}

fn initialize_block_with_aura_authority(authority: AuraId, slot: u64) {
    Aura::change_authorities(vec![authority].try_into().unwrap());
    let digest = sp_runtime::Digest {
        logs: vec![sp_runtime::DigestItem::PreRuntime(
            AURA_ENGINE_ID,
            slot.encode(),
        )],
    };
    frame_system::Pallet::<Runtime>::initialize(&1u32.into(), &Default::default(), &digest);
}

#[test]
fn evm_fee_refund_does_not_change_total_issuance() {
    new_test_ext().execute_with(|| {
        initialize_block_with_aura_authority(AuraId::from(sr25519::Public::from_raw([1u8; 32])), 0);

        let evm_addr = H160::from_low_u64_be(7);
        let account_id = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(evm_addr);
        let substrate_author = <Runtime as AuthorshipInfo<sp_runtime::AccountId32>>::author()
            .expect("aura digest should provide a substrate block author");
        let evm_author =
            <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(pallet_evm::Pallet::<
                Runtime,
            >::find_author(
            ));

        add_balance_to_coldkey_account(&account_id, 1_000_000_000u64.into());
        add_balance_to_coldkey_account(&substrate_author, 1_000_000_000u64.into());
        add_balance_to_coldkey_account(&evm_author, 1_000_000_000u64.into());

        let balances_issuance_before = Balances::total_issuance();
        let subtensor_issuance_before = pallet_subtensor::Pallet::<Runtime>::get_total_issuance();
        let balance_before = Balances::total_balance(&account_id);

        assert_eq!(balances_issuance_before, subtensor_issuance_before);

        let withdrawn =
            <<Runtime as pallet_evm::Config>::OnChargeTransaction as OnChargeEVMTransaction<
                Runtime,
            >>::withdraw_fee(&evm_addr, EvmBalance::from(10_000_000_000u128))
            .unwrap();

        let tip =
            <<Runtime as pallet_evm::Config>::OnChargeTransaction as OnChargeEVMTransaction<
                Runtime,
            >>::correct_and_deposit_fee(
                &evm_addr,
                EvmBalance::from(5_000_000_000u128),
                EvmBalance::from(3_000_000_000u128),
                withdrawn,
            );

        <<Runtime as pallet_evm::Config>::OnChargeTransaction as OnChargeEVMTransaction<
            Runtime,
        >>::pay_priority_fee(tip);

        assert_eq!(
            Balances::total_issuance(),
            pallet_subtensor::Pallet::<Runtime>::get_total_issuance()
        );
        assert_eq!(
            Balances::total_balance(&account_id),
            balance_before - TaoBalance::from(5)
        );
    });
}
