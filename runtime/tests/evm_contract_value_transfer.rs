#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use frame_support::traits::fungible::Inspect;
use node_subtensor_runtime::{Balances, BuildStorage, Runtime, RuntimeGenesisConfig, System};
use pallet_evm::{AddressMapping, BalanceConverter, EvmBalance, Runner};
use sp_core::{H160, U256};
use subtensor_runtime_common::TaoBalance;

const WITHDRAW_CONTRACT_BYTECODE: &str =
    "6080604052348015600e575f80fd5b506101148061001c5f395ff3fe608060405260043610601e575f3560e01c80632e1a7d4d146028576024565b36602457005b5f80fd5b603e6004803603810190603a919060b8565b6040565b005b3373ffffffffffffffffffffffffffffffffffffffff166108fc8290811502906040515f60405180830381858888f193505050501580156082573d5f803e3d5ffd5b5050565b5f80fd5b5f819050919050565b609a81608a565b811460a3575f80fd5b50565b5f8135905060b2816093565b92915050565b5f6020828403121560ca5760c96086565b5b5f60d58482850160a6565b9150509291505056fea2646970667358221220f43400858bfe4fcc0bf3c1e2e06d3a9e6ced86454a00bd7e4866b3d4d64e46bb64736f6c634300081a0033";

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn add_balance_to_evm_address(address: H160, tao: TaoBalance) {
    let account_id = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address);
    let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(tao);
    let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(&account_id, credit, tao).unwrap();
}

fn evm_balance_from_substrate(amount: u64) -> U256 {
    <Runtime as pallet_evm::Config>::BalanceConverter::into_evm_balance(amount.into())
        .expect("test amount should convert to EVM balance")
        .into()
}

fn substrate_balance_from_evm(amount: U256) -> TaoBalance {
    let substrate_balance =
        <Runtime as pallet_evm::Config>::BalanceConverter::into_substrate_balance(EvmBalance::new(
            amount,
        ))
        .expect("test amount should convert to Substrate balance")
        .into_u64_saturating();
    TaoBalance::new(substrate_balance)
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "hex bytecode must have even length");
    hex.as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            let hi = (chunk[0] as char).to_digit(16).unwrap();
            let lo = (chunk[1] as char).to_digit(16).unwrap();
            ((hi << 4) | lo) as u8
        })
        .collect()
}

fn withdraw_input(value: U256) -> Vec<u8> {
    let mut input = sp_io::hashing::keccak_256(b"withdraw(uint256)")[0..4].to_vec();
    let encoded_value = value.to_big_endian();
    input.extend_from_slice(&encoded_value);
    input
}

#[test]
fn contract_withdraw_credits_caller_balance() {
    new_test_ext().execute_with(|| {
        let caller = H160::repeat_byte(0x11);
        let caller_account =
            <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(caller);
        let one_tao = evm_balance_from_substrate(1_000_000_000);
        let two_tao = evm_balance_from_substrate(2_000_000_000);

        add_balance_to_evm_address(caller, 10_000_000_000u64.into());

        let create = <Runtime as pallet_evm::Config>::Runner::create(
            caller,
            decode_hex(WITHDRAW_CONTRACT_BYTECODE),
            U256::zero(),
            1_000_000,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            true,
            Vec::new(),
            false,
            false,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .expect("contract deployment should succeed");
        assert!(create.exit_reason.is_succeed());

        let contract = create.value;
        let fund = <Runtime as pallet_evm::Config>::Runner::call(
            caller,
            contract,
            Vec::new(),
            two_tao,
            100_000,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            false,
            false,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .expect("funding call should succeed");
        assert!(fund.exit_reason.is_succeed());

        let contract_account =
            <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(contract);
        assert_eq!(
            Balances::total_balance(&contract_account),
            substrate_balance_from_evm(two_tao)
        );

        let caller_balance_before = Balances::total_balance(&caller_account);
        let withdraw = <Runtime as pallet_evm::Config>::Runner::call(
            caller,
            contract,
            withdraw_input(one_tao),
            U256::zero(),
            1_000_000,
            Some(U256::from(1_000_000_000u64)),
            None,
            None,
            Vec::new(),
            Vec::new(),
            true,
            false,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .expect("withdraw call should succeed");
        assert!(
            withdraw.exit_reason.is_succeed(),
            "withdraw failed: {:?}",
            withdraw.exit_reason
        );

        let caller_balance_after = Balances::total_balance(&caller_account);
        let contract_balance_after = Balances::total_balance(&contract_account);
        assert_eq!(contract_balance_after, substrate_balance_from_evm(one_tao));
        let one_tao_substrate = substrate_balance_from_evm(one_tao);
        assert!(
            caller_balance_after > caller_balance_before,
            "caller balance should increase after receiving withdrawn value"
        );
        assert!(
            caller_balance_after <= caller_balance_before + one_tao_substrate,
            "caller increase should be reduced only by transaction fees"
        );
        assert!(
            caller_balance_before + one_tao_substrate - caller_balance_after
                <= TaoBalance::new(1_000_000),
            "withdraw fee should stay below the test bound"
        );
    });
}
