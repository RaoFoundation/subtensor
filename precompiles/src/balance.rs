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
    use crate::mock::{
        AccountId, Runtime, addr_from_index, assert_static_call, fund_account, new_test_ext,
        precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;

    fn coldkey(byte: u8) -> AccountId {
        AccountId::from([byte; 32])
    }

    #[test]
    fn balance_precompile_returns_free_balance_for_coldkey() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7001);
            let target = coldkey(0x11);
            let amount = 123_456_789_u64;
            fund_account(&target, amount);

            assert_static_call(
                &precompiles::<BalancePrecompile<Runtime>>(),
                caller,
                addr_from_index(BalancePrecompile::<Runtime>::INDEX),
                encode_with_selector(
                    selector_u32("getFreeBalance(bytes32)"),
                    (H256::from_slice(target.as_ref()),),
                ),
                U256::from(amount),
            );
        });
    }

    #[test]
    fn balance_precompile_returns_zero_for_unfunded_coldkey() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x7001);
            let target = coldkey(0x22);

            assert_static_call(
                &precompiles::<BalancePrecompile<Runtime>>(),
                caller,
                addr_from_index(BalancePrecompile::<Runtime>::INDEX),
                encode_with_selector(
                    selector_u32("getFreeBalance(bytes32)"),
                    (H256::from_slice(target.as_ref()),),
                ),
                U256::zero(),
            );
        });
    }
}
