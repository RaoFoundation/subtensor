//! Regression coverage for Pashov T1 M-01 and M-02 using the routed precompiles.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::{PrecompileHandleExt, Precompiles, mock::*};
use codec::Encode;
use fp_evm::{Context, ExitError, PrecompileFailure, PrecompileResult};
use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use pallet_evm::{GasWeightMapping, PrecompileHandle, PrecompileSet, Runner};
use precompile_utils::{solidity::encode_with_selector, testing::MockHandle};
use sp_core::{H256, U256};
use subtensor_runtime_common::{ProxyType, TaoBalance};

const PROXY_CALL: &str = "proxyCall(bytes32,uint8[],uint8[])";
fn handle(address: u64, input: Vec<u8>, gas: u64) -> MockHandle {
    let address = addr_from_index(address);
    let mut h = MockHandle::new(
        address,
        Context {
            address,
            caller: addr_from_index(0x9911),
            apparent_value: U256::zero(),
        },
    );
    h.input = input;
    h.gas_limit = gas;
    h
}
fn execute(h: &mut MockHandle) -> PrecompileResult {
    Precompiles::<Runtime>::new().execute(h).unwrap()
}
fn price() -> u64 {
    <Runtime as pallet_evm::Config>::config().gas_transaction_non_zero_data
}
fn proxy_input(call: Vec<u8>) -> Vec<u8> {
    encode_with_selector(
        selector_u32(PROXY_CALL),
        (H256::repeat_byte(0x33), Vec::<u8>::new(), call),
    )
}
fn remark() -> RuntimeCall {
    RuntimeCall::System(frame_system::Call::remark { remark: vec![] })
}
fn transfer() -> RuntimeCall {
    RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
        dest: AccountId::from([0x44; 32]),
        value: TaoBalance::new(100),
    })
}
fn oog(result: PrecompileResult) {
    assert_eq!(
        result,
        Err(PrecompileFailure::Error {
            exit_status: ExitError::OutOfGas
        })
    );
}
fn short_limits() -> usize {
    BlockLength::set(frame_system::limits::BlockLength::max_with_normal_ratio(
        1024,
        sp_runtime::Perbill::from_percent(75),
    ));
    *BlockLength::get().max.get(DispatchClass::Normal) as usize
}

#[test]
fn static_dispatch_rejects_before_transfer_but_normal_dispatch_works() {
    new_test_ext().execute_with(|| {
        let mut h = handle(6, transfer().encode(), u64::MAX);
        let account = mapped_account(h.context.caller);
        fund_account(&account, 1000);
        h.is_static = true;
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("cannot dispatch in static frame".into())
            })
        );
        assert_eq!(Balances::free_balance(&account), TaoBalance::new(1000));
        h.is_static = false;
        assert!(execute(&mut h).is_ok());
        assert_eq!(Balances::free_balance(&account), TaoBalance::new(900));
        assert_eq!(
            Balances::free_balance(AccountId::from([0x44; 32])),
            TaoBalance::new(100)
        );
    });
}

#[test]
fn real_contract_staticcall_cannot_dispatch_native_transfer() {
    new_test_ext().execute_with(|| {
        let caller = addr_from_index(0x9920);
        let contract = addr_from_index(0x9921);
        fund_account(&mapped_account(caller), 1000);
        fund_account(&mapped_account(contract), 1000);
        // Executable EVM contract: copy calldata, STATICCALL address 6, return its success word.
        // CALLDATASIZE PUSH1 0 PUSH1 0 CALLDATACOPY; push output/input, target, GAS; STATICCALL.
        let code = hex::decode("3660006000376000600036600060065afa60005260206000f3").unwrap();
        pallet_evm::AccountCodes::<Runtime>::insert(contract, code);
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            caller,
            contract,
            transfer().encode(),
            U256::zero(),
            1_000_000,
            None,
            None,
            None,
            vec![],
            vec![],
            false,
            false,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap();
        assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
        assert_eq!(result.value, vec![0; 32]);
        assert_eq!(
            Balances::free_balance(mapped_account(contract)),
            TaoBalance::new(1000)
        );
        assert_eq!(
            Balances::free_balance(AccountId::from([0x44; 32])),
            TaoBalance::new(0)
        );
        // The same contract with CALL instead of STATICCALL can perform the transfer.
        pallet_evm::AccountCodes::<Runtime>::insert(
            contract,
            hex::decode("36600060003760006000366000600060065af160005260206000f3").unwrap(),
        );
        let result = <Runtime as pallet_evm::Config>::Runner::call(
            caller,
            contract,
            transfer().encode(),
            U256::zero(),
            1_000_000,
            None,
            None,
            None,
            vec![],
            vec![],
            false,
            false,
            None,
            None,
            <Runtime as pallet_evm::Config>::config(),
        )
        .unwrap();
        assert!(result.exit_reason.is_succeed());
        assert_eq!(U256::from_big_endian(&result.value), U256::one());
        assert_eq!(
            Balances::free_balance(mapped_account(contract)),
            TaoBalance::new(900)
        );
        assert_eq!(
            Balances::free_balance(AccountId::from([0x44; 32])),
            TaoBalance::new(100)
        );
    });
}

#[test]
fn malformed_scale_is_charged_linearly_in_both_entry_points() {
    new_test_ext().execute_with(|| {
        for len in [1, 32, 513] {
            let mut invalid = vec![0; len];
            invalid[0] = 255;
            let mut h = handle(6, invalid.clone(), u64::MAX);
            assert!(execute(&mut h).is_err());
            assert_eq!(h.gas_used, len as u64 * price());
            let input = proxy_input(invalid);
            let expected = (input.len().div_ceil(32) + len) as u64 * price();
            let mut h = handle(0x80b, input, u64::MAX);
            assert!(execute(&mut h).is_err());
            assert_eq!(h.gas_used, expected);
        }
    });
}

#[test]
fn malformed_proxy_abi_is_charged_before_argument_decoding() {
    new_test_ext().execute_with(|| {
        for len in [4, 33, 129, 1025] {
            let mut input = vec![255; len];
            input[..4].copy_from_slice(&selector_u32(PROXY_CALL).to_be_bytes());
            let mut h = handle(0x80b, input, u64::MAX);
            assert!(execute(&mut h).is_err());
            assert_eq!(h.gas_used, len.div_ceil(32) as u64 * price());
        }
    });
}

#[test]
fn one_gas_rejects_before_scale_or_abi_decoding() {
    new_test_ext().execute_with(|| {
        for input in [vec![255; 10000], remark().encode()] {
            oog(execute(&mut handle(6, input.clone(), 1)));
            oog(execute(&mut handle(0x80b, proxy_input(input), 1)));
        }
        let input = selector_u32(PROXY_CALL).to_be_bytes().to_vec();
        oog(execute(&mut handle(0x80b, input, 1)));
    });
}

#[test]
fn dispatch_weight_uses_remaining_gas_and_consumes_only_current_frame() {
    new_test_ext().execute_with(|| {
        let call = remark();
        let dispatch_gas = <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
            call.get_dispatch_info().total_weight(),
        )
        .saturating_add(1);
        let mut h = handle(6, call.encode(), dispatch_gas);
        oog(execute(&mut h));
        assert_eq!(h.gas_used, h.gas_limit);
        // Also exercise the common helper after an earlier precompile charge.
        let mut h = handle(0x80b, vec![], dispatch_gas);
        h.record_cost(1).unwrap();
        assert_eq!(
            h.try_dispatch_runtime_call::<Runtime, _>(
                call,
                frame_system::RawOrigin::Signed(mapped_account(h.context.caller))
            ),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::OutOfGas
            })
        );
        assert_eq!(h.gas_used, h.gas_limit);
    });
}

#[test]
fn scale_and_proxy_abi_limits_are_checked_before_decoding() {
    new_test_ext().execute_with(|| {
        let limit = short_limits();
        let mut allowed = remark().encode();
        allowed.resize(limit, 0);
        assert!(execute(&mut handle(6, allowed, u64::MAX)).is_ok());
        let mut h = handle(6, vec![255; limit + 1], u64::MAX);
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("call input exceeds normal block length".into())
            })
        );
        assert_eq!(h.gas_used, 0);
        let input = proxy_input(vec![255; limit]);
        let mut h = handle(0x80b, input, u64::MAX);
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("The raw call data not correctly encoded".into())
            })
        );
        // Legacy ABI permits both dynamic arrays to reference the same tail. With
        // this compact layout, an oversized SCALE call still fits the ABI bound.
        let mut input = selector_u32(PROXY_CALL).to_be_bytes().to_vec();
        input.extend_from_slice(H256::repeat_byte(0x33).as_bytes());
        input.extend(abi_word(U256::from(96)));
        input.extend(abi_word(U256::from(96)));
        input.extend(abi_word(U256::from(limit + 1)));
        for _ in 0..=limit {
            input.extend(abi_word(U256::from(255)));
        }
        let mut h = handle(0x80b, input, u64::MAX);
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("call input exceeds normal block length".into())
            })
        );
        let abi_limit = limit * 32 + 164;
        let mut input = proxy_input(remark().encode());
        input.resize(abi_limit, 0);
        // Accepted by the ABI bound: reaches the unchanged proxy authorization check.
        let mut h = handle(0x80b, input, u64::MAX);
        assert!(format!("{:?}", execute(&mut h)).contains("NotProxy"));
        let mut input = vec![255; abi_limit + 1];
        input[..4].copy_from_slice(&selector_u32(PROXY_CALL).to_be_bytes());
        let mut h = handle(0x80b, input, u64::MAX);
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("proxy ABI input exceeds limit".into())
            })
        );
        assert_eq!(h.gas_used, 0);
        // A valid, authorized legacy call succeeds exactly at both byte ceilings.
        let mut allowed = remark().encode();
        allowed.resize(limit, 0);
        let input = proxy_input(allowed);
        assert_eq!(input.len(), abi_limit);
        let mut h = handle(0x80b, input, u64::MAX);
        let real = AccountId::from([0x33; 32]);
        fund_account(&real, 1000);
        assert!(
            pallet_subtensor_proxy::Pallet::<Runtime>::add_proxy(
                RuntimeOrigin::signed(real),
                mapped_account(h.context.caller),
                ProxyType::Any,
                0
            )
            .is_ok()
        );
        assert!(execute(&mut h).is_ok());
    });
}

#[test]
fn legacy_proxy_authorization_and_other_selectors_are_preserved() {
    new_test_ext().execute_with(|| {
        let mut h = handle(0x80b, proxy_input(transfer().encode()), u64::MAX);
        let real = AccountId::from([0x33; 32]);
        let delegate = mapped_account(h.context.caller);
        fund_account(&real, 1000);
        assert!(format!("{:?}", execute(&mut h)).contains("NotProxy"));
        assert_eq!(Balances::free_balance(&real), TaoBalance::new(1000));
        assert!(
            pallet_subtensor_proxy::Pallet::<Runtime>::add_proxy(
                RuntimeOrigin::signed(real.clone()),
                delegate.clone(),
                ProxyType::Any,
                0
            )
            .is_ok()
        );
        let free_before = Balances::free_balance(&real);
        assert!(execute(&mut h).is_ok());
        assert_eq!(
            Balances::free_balance(&real),
            free_before - TaoBalance::new(100)
        );
        assert_eq!(
            Balances::free_balance(AccountId::from([0x44; 32])),
            TaoBalance::new(100)
        );
        let input = encode_with_selector(
            selector_u32("getProxyDeposit(bytes32)"),
            (H256::repeat_byte(0x33),),
        );
        let mut h = handle(0x80b, input, u64::MAX);
        h.is_static = true;
        assert!(execute(&mut h).is_ok());
        assert_eq!(
            h.gas_used,
            precompile_utils::prelude::RuntimeHelper::<Runtime>::db_read_gas_cost()
        );
    });
}

#[test]
fn real_contract_can_handle_one_gas_failures_at_both_entry_points() {
    new_test_ext().execute_with(|| {
        let caller = addr_from_index(0x9940);
        let contract = addr_from_index(0x9941);
        fund_account(&mapped_account(caller), 1000);
        for (target, input) in [
            (6, vec![255; 10000]),
            (0x80b, proxy_input(vec![255; 10000])),
        ] {
            // Copy input once, CALL with exactly one forwarded gas, return success flag.
            let code = hex::decode(format!(
                "36600060003760006000366000600061{target:04x}6001f160005260206000f3"
            ))
            .unwrap();
            pallet_evm::AccountCodes::<Runtime>::insert(contract, code);
            let result = <Runtime as pallet_evm::Config>::Runner::call(
                caller,
                contract,
                input,
                U256::zero(),
                5_000_000,
                None,
                None,
                None,
                vec![],
                vec![],
                false,
                false,
                None,
                None,
                <Runtime as pallet_evm::Config>::config(),
            )
            .unwrap();
            assert!(result.exit_reason.is_succeed(), "{:?}", result.exit_reason);
            assert_eq!(result.value, vec![0; 32]);
        }
    });
}

#[test]
fn proxy_dispatch_weight_rejection_spends_the_current_frames_remaining_gas() {
    new_test_ext().execute_with(|| {
        let call = remark();
        let proxy = RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
            real: AccountId::from([0x33; 32]),
            force_proxy_type: None,
            call: Box::new(call.clone()),
        });
        let gas = <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
            proxy.get_dispatch_info().total_weight(),
        )
        .saturating_add(1);
        let mut h = handle(0x80b, proxy_input(call.encode()), gas);
        oog(execute(&mut h));
        assert_eq!(h.gas_used, h.gas_limit);
    });
}

#[test]
fn depth_limits_still_reject_nested_scale_after_charging_input() {
    new_test_ext().execute_with(|| {
        let mut call = remark();
        for _ in 0..9 {
            call = RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
                real: AccountId::from([0x33; 32]),
                force_proxy_type: None,
                call: Box::new(call),
            });
        }
        let encoded = call.encode();
        let mut h = handle(6, encoded.clone(), u64::MAX);
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("decode failed".into())
            })
        );
        assert_eq!(h.gas_used, encoded.len() as u64 * price());
        let input = proxy_input(encoded.clone());
        let expected = (input.len().div_ceil(32) + encoded.len()) as u64 * price();
        let mut h = handle(0x80b, input, u64::MAX);
        assert_eq!(
            execute(&mut h),
            Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("The raw call data not correctly encoded".into())
            })
        );
        assert_eq!(h.gas_used, expected);
    });
}

#[test]
fn other_proxy_mutators_keep_their_dispatch_gas_and_mapped_origin() {
    new_test_ext().execute_with(|| {
        let delegate = AccountId::from([0x55; 32]);
        let input = encode_with_selector(
            selector_u32("addProxy(bytes32,uint8,uint32)"),
            (H256::repeat_byte(0x55), 0_u8, 0_u32),
        );
        let mut h = handle(0x80b, input, u64::MAX);
        let owner = mapped_account(h.context.caller);
        fund_account(&owner, 1000);
        assert!(execute(&mut h).is_ok());
        let proxies = pallet_subtensor_proxy::Proxies::<Runtime>::get(&owner).0;
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].delegate, delegate);
        let expected = <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
            RuntimeCall::Proxy(pallet_subtensor_proxy::Call::add_proxy {
                delegate,
                proxy_type: ProxyType::Any,
                delay: 0,
            })
            .get_dispatch_info()
            .total_weight(),
        );
        assert_eq!(h.gas_used, expected);
    });
}
