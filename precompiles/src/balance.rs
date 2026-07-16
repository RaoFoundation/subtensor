use core::marker::PhantomData;

use pallet_evm::PrecompileHandle;
use precompile_utils::EvmResult;
use sp_core::{H256, U256};

use crate::PrecompileExt;
use crate::PrecompileHandleExt;

pub struct BalancePrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for BalancePrecompile<R>
where
    R: frame_system::Config + pallet_balances::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
    <R as pallet_balances::Config>::Balance: Into<U256>,
{
    const INDEX: u64 = 2062;
}

#[precompile_utils::precompile]
impl<R> BalancePrecompile<R>
where
    R: frame_system::Config + pallet_balances::Config + pallet_evm::Config,
    R::AccountId: From<[u8; 32]>,
    <R as pallet_balances::Config>::Balance: Into<U256>,
{
    #[precompile::public("getFreeBalance(bytes32)")]
    #[precompile::view]
    fn get_free_balance(handle: &mut impl PrecompileHandle, coldkey: H256) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        let coldkey = R::AccountId::from(coldkey.0);
        Ok(pallet_balances::Pallet::<R>::free_balance(&coldkey).into())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]

    use super::*;
    use crate::Precompiles;
    use crate::mock::{
        AccountId, Runtime, TestBalanceStatus, abi_word, addr_from_index, execute_precompile,
        fund_account, new_test_ext, precompiles, selector_u32,
    };
    use frame_support::{
        dispatch::DispatchResult,
        traits::{
            ReservableCurrency,
            tokens::{
                Fortitude, Preservation,
                fungible::{Inspect, InspectFreeze, InspectHold, MutateFreeze, MutateHold},
            },
        },
    };
    use pallet_admin_utils::{PrecompileEnable, PrecompileEnum};
    use precompile_utils::prelude::RuntimeHelper;
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;

    fn coldkey(byte: u8) -> AccountId {
        AccountId::from([byte; 32])
    }

    fn balance_call_input(coldkey: &AccountId) -> Vec<u8> {
        encode_with_selector(
            selector_u32("getFreeBalance(bytes32)"),
            (H256::from_slice(coldkey.as_ref()),),
        )
    }

    #[test]
    fn balance_precompile_returns_free_balance_for_coldkey() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7001);
            let target = coldkey(0x11);
            let amount = 123_456_789_u64;
            fund_account(&target, amount);

            precompiles::<BalancePrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(BalancePrecompile::<Runtime>::INDEX),
                    balance_call_input(&target),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns_raw(abi_word(U256::from(amount)));
        });
    }

    #[test]
    fn balance_precompile_returns_zero_for_unfunded_coldkey() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7001);
            let target = coldkey(0x22);

            precompiles::<BalancePrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(BalancePrecompile::<Runtime>::INDEX),
                    balance_call_input(&target),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns_raw(abi_word(U256::zero()));
        });
    }

    #[test]
    fn balance_precompile_returns_free_not_total_or_reducible_balance() -> DispatchResult {
        new_test_ext().execute_with(|| -> DispatchResult {
            let caller = addr_from_index(0x7001);
            let target = coldkey(0x33);
            let initial = 1_000_u64;
            let reserved = 100_u64;
            let held = 200_u64;
            let frozen = 650_u64;
            let expected_free = initial - reserved - held;

            fund_account(&target, initial);
            pallet_balances::Pallet::<Runtime>::reserve(&target, reserved.into())?;
            <pallet_balances::Pallet<Runtime> as MutateHold<AccountId>>::hold(
                &TestBalanceStatus::Test,
                &target,
                held.into(),
            )?;
            <pallet_balances::Pallet<Runtime> as MutateFreeze<AccountId>>::set_freeze(
                &TestBalanceStatus::Test,
                &target,
                frozen.into(),
            )?;

            assert_eq!(
                pallet_balances::Pallet::<Runtime>::reserved_balance(&target),
                (reserved + held).into()
            );
            assert_eq!(
                <pallet_balances::Pallet<Runtime> as InspectHold<AccountId>>::balance_on_hold(
                    &TestBalanceStatus::Test,
                    &target,
                ),
                held.into()
            );
            assert_eq!(
                <pallet_balances::Pallet<Runtime> as InspectFreeze<AccountId>>::balance_frozen(
                    &TestBalanceStatus::Test,
                    &target,
                ),
                frozen.into()
            );
            assert_eq!(
                pallet_balances::Pallet::<Runtime>::total_balance(&target),
                initial.into()
            );
            assert_eq!(
                <pallet_balances::Pallet<Runtime> as Inspect<AccountId>>::reducible_balance(
                    &target,
                    Preservation::Expendable,
                    Fortitude::Polite,
                ),
                350_u64.into()
            );
            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(&target),
                expected_free.into()
            );

            precompiles::<BalancePrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(BalancePrecompile::<Runtime>::INDEX),
                    balance_call_input(&target),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns_raw(abi_word(U256::from(expected_free)));

            Ok(())
        })
    }

    #[test]
    fn balance_precompile_rejects_malformed_calldata() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7001);
            let precompile_addr = addr_from_index(BalancePrecompile::<Runtime>::INDEX);
            let mut malformed = selector_u32("getFreeBalance(bytes32)")
                .to_be_bytes()
                .to_vec();
            malformed.extend_from_slice(&[0u8; 31]);

            let result = execute_precompile(
                &precompiles::<BalancePrecompile<Runtime>>(),
                precompile_addr,
                caller,
                malformed,
                U256::zero(),
            );

            assert!(
                matches!(result, Some(Err(_))),
                "malformed calldata must not return a balance"
            );
        });
    }

    #[test]
    fn balance_precompile_respects_enable_disable_administration() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7001);
            let target = coldkey(0x44);
            let amount = 42_u64;
            let precompile_addr = addr_from_index(BalancePrecompile::<Runtime>::INDEX);
            let input = balance_call_input(&target);
            let precompiles = Precompiles::<Runtime>::new();

            fund_account(&target, amount);

            PrecompileEnable::<Runtime>::insert(PrecompileEnum::AccountBalance, false);
            let disabled = execute_precompile(
                &precompiles,
                precompile_addr,
                caller,
                input.clone(),
                U256::zero(),
            );
            assert!(
                matches!(disabled, Some(Err(_))),
                "disabled account-balance precompile must reject calls"
            );

            PrecompileEnable::<Runtime>::insert(PrecompileEnum::AccountBalance, true);
            precompiles
                .prepare_test(caller, precompile_addr, input)
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns_raw(abi_word(U256::from(amount)));
        });
    }
}
