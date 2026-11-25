use core::marker::PhantomData;

use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, ExitError, PrecompileFailure, PrecompileHandle};
use precompile_utils::EvmResult;
use sp_core::{H256, U256};
use sp_runtime::DispatchError;
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable, UniqueSaturatedInto};
use subtensor_runtime_common::{AlphaBalance, GammaBalance, NetUid, TaoBalance, Token};

use crate::staking::{try_u16_from_u256, try_u64_from_u256};
use crate::{PrecompileExt, PrecompileHandleExt};

/// The reads `mint_gamma`'s weight declares.
const QUOTE_MINT_GAMMA_READS: u64 = 28;
/// Subnet, gamma and subtoken flags, gamma and spot prices, TAO/USD, the stake hotkey's owner,
/// and the deposit, its hotkey's owner and its coldkey's deposits.
const QUOTE_MINT_GAMMA_WITH_STAKE_READS: u64 = 14;

pub struct GammaPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for GammaPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_balances::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    const INDEX: u64 = 2068;
}

#[precompile_utils::precompile]
impl<R> GammaPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_balances::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("quoteMintGamma(uint256,bytes32,bytes32,uint256)")]
    #[precompile::view]
    fn quote_mint_gamma(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        deposit_hotkey: H256,
        recipient_coldkey: H256,
        amount_rao: U256,
    ) -> EvmResult<U256> {
        handle.record_db_reads::<R>(QUOTE_MINT_GAMMA_READS)?;
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let amount = TaoBalance::from(try_u64_from_u256(amount_rao)?);
        let (_, _, gamma) =
            pallet_subtensor::Pallet::<R>::sim_mint_gamma(netuid, amount).map_err(revert)?;
        Self::sim_credit(netuid, deposit_hotkey, recipient_coldkey, gamma)
    }

    #[precompile::public("quoteMintGammaWithStake(uint256,bytes32,bytes32,bytes32,uint256)")]
    #[precompile::view]
    fn quote_mint_gamma_with_stake(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        stake_hotkey: H256,
        deposit_hotkey: H256,
        recipient_coldkey: H256,
        amount_alpha: U256,
    ) -> EvmResult<U256> {
        handle.record_db_reads::<R>(QUOTE_MINT_GAMMA_WITH_STAKE_READS)?;
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let alpha = AlphaBalance::from(UniqueSaturatedInto::<u64>::unique_saturated_into(
            amount_alpha,
        ));
        let (_, _, gamma) = pallet_subtensor::Pallet::<R>::sim_mint_gamma_with_stake(netuid, alpha)
            .map_err(revert)?;
        if !pallet_subtensor::Pallet::<R>::hotkey_account_exists(&R::AccountId::from(
            stake_hotkey.0,
        )) {
            return Err(revert(
                pallet_subtensor::Error::<R>::HotKeyAccountNotExists.into(),
            ));
        }
        Self::sim_credit(netuid, deposit_hotkey, recipient_coldkey, gamma)
    }

    #[precompile::public("getGammaDeposit(uint256,bytes32)")]
    #[precompile::view]
    fn get_gamma_deposit(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        deposit_hotkey: H256,
    ) -> EvmResult<(U256, H256)> {
        handle.record_db_reads::<R>(1)?;
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        Ok(
            pallet_subtensor::GammaDeposits::<R>::get(netuid, R::AccountId::from(deposit_hotkey.0))
                .map_or((U256::zero(), H256::zero()), |deposit| {
                    (
                        U256::from(deposit.balance.to_u64()),
                        H256::from(Into::<[u8; 32]>::into(deposit.coldkey)),
                    )
                }),
        )
    }

    #[precompile::public("mintGammaOnBehalf(uint256,bytes32,bytes32,uint256,uint256)")]
    fn mint_gamma_on_behalf(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        deposit_hotkey: H256,
        recipient_coldkey: H256,
        amount_rao: U256,
        min_gamma_out: U256,
    ) -> EvmResult<U256> {
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let call = pallet_subtensor::Call::<R>::mint_gamma_on_behalf {
            coldkey: R::AccountId::from(recipient_coldkey.0),
            hotkey: R::AccountId::from(deposit_hotkey.0),
            netuid,
            amount: TaoBalance::from(try_u64_from_u256(amount_rao)?),
            min_amount_out: GammaBalance::from(try_u64_from_u256(min_gamma_out)?),
        };
        Self::dispatch_mint(handle, netuid, deposit_hotkey, call)
    }

    #[precompile::public(
        "mintGammaWithStakeOnBehalf(uint256,bytes32,bytes32,bytes32,uint256,uint256)"
    )]
    fn mint_gamma_with_stake_on_behalf(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        stake_hotkey: H256,
        deposit_hotkey: H256,
        recipient_coldkey: H256,
        alpha_amount: U256,
        min_gamma_out: U256,
    ) -> EvmResult<U256> {
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let call = pallet_subtensor::Call::<R>::mint_gamma_with_stake_on_behalf {
            coldkey: R::AccountId::from(recipient_coldkey.0),
            origin_hotkey: R::AccountId::from(stake_hotkey.0),
            destination_hotkey: R::AccountId::from(deposit_hotkey.0),
            netuid,
            alpha_amount: AlphaBalance::from(UniqueSaturatedInto::<u64>::unique_saturated_into(
                alpha_amount,
            )),
            min_amount_out: GammaBalance::from(try_u64_from_u256(min_gamma_out)?),
        };
        Self::dispatch_mint(handle, netuid, deposit_hotkey, call)
    }

    fn sim_credit(
        netuid: NetUid,
        deposit_hotkey: H256,
        recipient_coldkey: H256,
        gamma: GammaBalance,
    ) -> EvmResult<U256> {
        pallet_subtensor::Pallet::<R>::sim_credit_gamma(
            netuid,
            &R::AccountId::from(recipient_coldkey.0),
            &R::AccountId::from(deposit_hotkey.0),
            gamma,
        )
        .map_err(revert)?;
        Ok(U256::from(gamma.to_u64()))
    }

    /// Dispatches `call` signed by the caller and returns what it added to the deposit at
    /// `deposit_hotkey`.
    fn dispatch_mint(
        handle: &mut impl PrecompileHandle,
        netuid: NetUid,
        deposit_hotkey: H256,
        call: pallet_subtensor::Call<R>,
    ) -> EvmResult<U256> {
        let caller = handle.caller_account_id::<R>()?;
        let hotkey = R::AccountId::from(deposit_hotkey.0);
        let balance = || {
            pallet_subtensor::GammaDeposits::<R>::get(netuid, &hotkey)
                .map_or(0u64, |deposit| deposit.balance.to_u64())
        };
        handle.record_db_reads::<R>(2)?;
        let before = balance();
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))?;
        Ok(U256::from(balance().saturating_sub(before)))
    }
}

fn revert(error: DispatchError) -> PrecompileFailure {
    PrecompileFailure::Error {
        exit_status: ExitError::Other(Into::<&'static str>::into(error).into()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::mock::{
        AccountId, Runtime, addr_from_index, fund_account, mapped_account, new_test_ext,
        precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::H160;
    use subtensor_swap_interface::SwapHandler;

    const NETUID: u16 = 1;
    const TAO: u64 = 1_000_000_000;

    fn setup() -> (H160, AccountId, AccountId, AccountId) {
        let netuid = NetUid::from(NETUID);
        pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, 100);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(150 * TAO));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(100 * TAO));
        pallet_subtensor::GammaEnabled::<Runtime>::put(true);
        pallet_subtensor::SubnetGammaEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::SubnetGammaPrice::<Runtime>::insert(
            netuid,
            <Runtime as pallet_subtensor::Config>::SwapInterface::current_alpha_price(netuid),
        );
        pallet_subtensor::TaoUsdPrice::<Runtime>::put(pallet_subtensor::TaoUsdPriceState {
            accepted: 400 * TAO,
            feed: 400 * TAO,
            fed_at: 1,
        });
        let caller = addr_from_index(0x6a01);
        fund_account(&mapped_account(caller), 100 * TAO);
        (
            caller,
            AccountId::from([0x31; 32]),
            AccountId::from([0x32; 32]),
            AccountId::from([0x33; 32]),
        )
    }

    fn h256(account: &AccountId) -> H256 {
        H256::from_slice(account.as_ref())
    }

    fn address() -> H160 {
        addr_from_index(GammaPrecompile::<Runtime>::INDEX)
    }

    fn quote_input(hotkey: &AccountId, coldkey: &AccountId, amount: u64) -> Vec<u8> {
        encode_with_selector(
            selector_u32("quoteMintGamma(uint256,bytes32,bytes32,uint256)"),
            (
                U256::from(NETUID),
                h256(hotkey),
                h256(coldkey),
                U256::from(amount),
            ),
        )
    }

    fn mint_input(hotkey: &AccountId, coldkey: &AccountId, amount: u64, min: u64) -> Vec<u8> {
        encode_with_selector(
            selector_u32("mintGammaOnBehalf(uint256,bytes32,bytes32,uint256,uint256)"),
            (
                U256::from(NETUID),
                h256(hotkey),
                h256(coldkey),
                U256::from(amount),
                U256::from(min),
            ),
        )
    }

    fn deposit_input(hotkey: &AccountId) -> Vec<u8> {
        encode_with_selector(
            selector_u32("getGammaDeposit(uint256,bytes32)"),
            (U256::from(NETUID), h256(hotkey)),
        )
    }

    #[test]
    fn mint_on_behalf_credits_the_quote_to_the_recipient() {
        new_test_ext().execute_with(|| {
            let (caller, recipient, hotkey, _) = setup();
            let precompiles = precompiles::<GammaPrecompile<Runtime>>();
            let quote =
                pallet_subtensor::Pallet::<Runtime>::sim_mint_gamma(NETUID.into(), TAO.into())
                    .unwrap()
                    .2
                    .to_u64();
            assert!(quote > 0);

            precompiles
                .prepare_test(caller, address(), deposit_input(&hotkey))
                .with_static_call(true)
                .execute_returns((U256::zero(), H256::zero()));
            precompiles
                .prepare_test(caller, address(), quote_input(&hotkey, &recipient, TAO))
                .with_static_call(true)
                .execute_returns(U256::from(quote));
            let before =
                pallet_subtensor::Pallet::<Runtime>::get_coldkey_balance(&mapped_account(caller));
            precompiles
                .prepare_test(
                    caller,
                    address(),
                    mint_input(&hotkey, &recipient, TAO, quote),
                )
                .execute_returns(U256::from(quote));
            assert_eq!(
                pallet_subtensor::Pallet::<Runtime>::get_coldkey_balance(&mapped_account(caller)),
                before - TAO.into()
            );
            precompiles
                .prepare_test(caller, address(), deposit_input(&hotkey))
                .with_static_call(true)
                .execute_returns((U256::from(quote), h256(&recipient)));
        });
    }

    #[test]
    fn mint_with_stake_on_behalf_credits_the_quote_to_the_recipient() {
        new_test_ext().execute_with(|| {
            let (caller, recipient, hotkey, stake_hotkey) = setup();
            let netuid = NetUid::from(NETUID);
            let payer = mapped_account(caller);
            let alpha = AlphaBalance::from(10 * TAO);
            pallet_subtensor::Owner::<Runtime>::insert(&stake_hotkey, &payer);
            pallet_subtensor::SubnetAlphaOut::<Runtime>::insert(netuid, alpha);
            pallet_subtensor::Pallet::<Runtime>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &stake_hotkey,
                &payer,
                netuid,
                alpha,
            );
            let precompiles = precompiles::<GammaPrecompile<Runtime>>();
            let quote =
                pallet_subtensor::Pallet::<Runtime>::sim_mint_gamma_with_stake(netuid, alpha)
                    .unwrap()
                    .2
                    .to_u64();
            let args = (
                U256::from(NETUID),
                h256(&stake_hotkey),
                h256(&hotkey),
                h256(&recipient),
                U256::from(alpha.to_u64()),
            );

            precompiles
                .prepare_test(
                    caller,
                    address(),
                    encode_with_selector(
                        selector_u32(
                            "quoteMintGammaWithStake(uint256,bytes32,bytes32,bytes32,uint256)",
                        ),
                        args,
                    ),
                )
                .with_static_call(true)
                .execute_returns(U256::from(quote));
            precompiles
                .prepare_test(
                    caller,
                    address(),
                    encode_with_selector(
                        selector_u32(
                            "mintGammaWithStakeOnBehalf(uint256,bytes32,bytes32,bytes32,uint256,uint256)",
                        ),
                        (args.0, args.1, args.2, args.3, U256::MAX, U256::from(quote)),
                    ),
                )
                .execute_returns(U256::from(quote));
            assert_eq!(
                pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &stake_hotkey,
                    &payer,
                    netuid,
                ),
                AlphaBalance::ZERO
            );
            precompiles
                .prepare_test(caller, address(), deposit_input(&hotkey))
                .with_static_call(true)
                .execute_returns((U256::from(quote), h256(&recipient)));
        });
    }

    #[test]
    fn quote_and_mint_revert_on_a_deposit_they_cannot_credit() {
        new_test_ext().execute_with(|| {
            let (caller, recipient, hotkey, other) = setup();
            let precompiles = precompiles::<GammaPrecompile<Runtime>>();
            pallet_subtensor::Owner::<Runtime>::insert(&hotkey, &other);

            precompiles
                .prepare_test(caller, address(), quote_input(&hotkey, &recipient, TAO))
                .with_static_call(true)
                .execute_error(ExitError::Other("NonAssociatedColdKey".into()));
            pallet_subtensor::ColdkeySwapAnnouncements::<Runtime>::insert(
                &other,
                (1, H256::zero()),
            );
            precompiles
                .prepare_test(caller, address(), quote_input(&hotkey, &other, TAO))
                .with_static_call(true)
                .execute_error(ExitError::Other("ColdkeySwapAnnounced".into()));
            precompiles
                .prepare_test(caller, address(), mint_input(&hotkey, &other, TAO, 0))
                .execute_error(ExitError::Other(
                    "dispatch execution failed: ColdkeySwapAnnounced".into(),
                ));
            precompiles
                .prepare_test(caller, address(), mint_input(&hotkey, &recipient, TAO, 0))
                .execute_error(ExitError::Other(
                    "dispatch execution failed: NonAssociatedColdKey".into(),
                ));
            precompiles
                .prepare_test(
                    caller,
                    address(),
                    encode_with_selector(
                        selector_u32(
                            "quoteMintGammaWithStake(uint256,bytes32,bytes32,bytes32,uint256)",
                        ),
                        (
                            U256::from(NETUID),
                            h256(&AccountId::from([0x99; 32])),
                            h256(&recipient),
                            h256(&recipient),
                            U256::from(TAO),
                        ),
                    ),
                )
                .with_static_call(true)
                .execute_error(ExitError::Other("HotKeyAccountNotExists".into()));
            precompiles
                .prepare_test(
                    caller,
                    address(),
                    mint_input(&recipient, &recipient, TAO, u64::MAX),
                )
                .execute_error(ExitError::Other(
                    "dispatch execution failed: SlippageTooHigh".into(),
                ));
        });
    }
}
