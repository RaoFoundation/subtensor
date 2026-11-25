use core::marker::PhantomData;

use frame_support::BoundedVec;
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::{ConstU32, IsSubType};
use frame_support::weights::Weight;
use frame_system::RawOrigin;
use pallet_evm::{
    AddressMapping, ExitError, GasWeightMapping, PrecompileFailure, PrecompileHandle,
};
use pallet_subtensor::weights::WeightInfo;
use precompile_utils::EvmResult;
use precompile_utils::prelude::BoundedString;
use sp_core::H256;
use sp_runtime::DispatchError;
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable};
use sp_std::vec::Vec;
use subtensor_runtime_common::{AlphaBalance, GammaBalance, NetUid, TaoBalance, Token};

use crate::{PrecompileExt, PrecompileHandleExt};

pub struct GammaPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for GammaPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
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
        + IsSubType<pallet_subtensor::Call<R>>,
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
        + IsSubType<pallet_subtensor::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    #[precompile::public("mintGamma(bool,bytes32,uint16,uint64,uint64,bool,uint64)")]
    #[allow(clippy::too_many_arguments)]
    fn mint_gamma(
        handle: &mut impl PrecompileHandle,
        has_hotkey: bool,
        hotkey: H256,
        netuid: u16,
        amount: u64,
        min_amount_out: u64,
        has_daily_spend_cap: bool,
        daily_spend_cap: u64,
    ) -> EvmResult<u64> {
        dispatch_credit::<R>(
            handle,
            netuid,
            has_hotkey.then(|| hotkey.0.into()),
            pallet_subtensor::Call::<R>::mint_gamma {
                hotkey: has_hotkey.then(|| hotkey.0.into()),
                netuid: netuid.into(),
                amount: TaoBalance::from(amount),
                min_amount_out: GammaBalance::from(min_amount_out),
                daily_spend_cap: has_daily_spend_cap.then_some(GammaBalance::from(daily_spend_cap)),
            },
        )
    }

    #[precompile::public(
        "mintGammaWithStake(bytes32,bool,bytes32,uint16,uint64,uint64,bool,uint64)"
    )]
    #[allow(clippy::too_many_arguments)]
    fn mint_gamma_with_stake(
        handle: &mut impl PrecompileHandle,
        origin_hotkey: H256,
        has_destination_hotkey: bool,
        destination_hotkey: H256,
        netuid: u16,
        alpha_amount: u64,
        min_amount_out: u64,
        has_daily_spend_cap: bool,
        daily_spend_cap: u64,
    ) -> EvmResult<u64> {
        dispatch_credit::<R>(
            handle,
            netuid,
            has_destination_hotkey.then(|| destination_hotkey.0.into()),
            pallet_subtensor::Call::<R>::mint_gamma_with_stake {
                origin_hotkey: origin_hotkey.0.into(),
                destination_hotkey: has_destination_hotkey.then(|| destination_hotkey.0.into()),
                netuid: netuid.into(),
                alpha_amount: AlphaBalance::from(alpha_amount),
                min_amount_out: GammaBalance::from(min_amount_out),
                daily_spend_cap: has_daily_spend_cap.then_some(GammaBalance::from(daily_spend_cap)),
            },
        )
    }

    #[precompile::public("moveGamma(bytes32,bytes32,uint16,uint64)")]
    fn move_gamma(
        handle: &mut impl PrecompileHandle,
        origin_hotkey: H256,
        destination_hotkey: H256,
        netuid: u16,
        amount: u64,
    ) -> EvmResult<u64> {
        dispatch_credit::<R>(
            handle,
            netuid,
            Some(destination_hotkey.0.into()),
            pallet_subtensor::Call::<R>::move_gamma {
                origin_hotkey: origin_hotkey.0.into(),
                destination_hotkey: destination_hotkey.0.into(),
                netuid: netuid.into(),
                amount: GammaBalance::from(amount),
            },
        )
    }

    #[precompile::public("closeGammaDeposit(bytes32,uint16)")]
    fn close_gamma_deposit(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
    ) -> EvmResult<()> {
        dispatch::<R>(
            handle,
            pallet_subtensor::Call::<R>::close_gamma_deposit {
                hotkey: hotkey.0.into(),
                netuid: netuid.into(),
            },
        )
    }

    #[precompile::public("setGammaDepositDailySpendCap(bytes32,uint16,bool,uint64)")]
    fn set_gamma_deposit_daily_spend_cap(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
        has_daily_spend_cap: bool,
        daily_spend_cap: u64,
    ) -> EvmResult<()> {
        dispatch::<R>(
            handle,
            pallet_subtensor::Call::<R>::set_gamma_deposit_daily_spend_cap {
                hotkey: hotkey.0.into(),
                netuid: netuid.into(),
                daily_spend_cap: has_daily_spend_cap.then_some(GammaBalance::from(daily_spend_cap)),
            },
        )
    }

    #[precompile::public("spendGamma(bytes32,uint16,uint64)")]
    fn spend_gamma(
        handle: &mut impl PrecompileHandle,
        service: H256,
        netuid: u16,
        amount: u64,
    ) -> EvmResult<()> {
        dispatch::<R>(
            handle,
            pallet_subtensor::Call::<R>::spend_gamma {
                service: service.0.into(),
                netuid: netuid.into(),
                amount: GammaBalance::from(amount),
            },
        )
    }

    #[precompile::public("depositGamma(bytes32,uint16,uint64)")]
    fn deposit_gamma(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
        amount: u64,
    ) -> EvmResult<u64> {
        dispatch_credit::<R>(
            handle,
            netuid,
            Some(hotkey.0.into()),
            pallet_subtensor::Call::<R>::deposit_gamma {
                hotkey: hotkey.0.into(),
                netuid: netuid.into(),
                amount: GammaBalance::from(amount),
            },
        )
    }

    #[precompile::public("withdrawGamma(bytes32,uint16,uint64)")]
    fn withdraw_gamma(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
        amount: u64,
    ) -> EvmResult<u64> {
        dispatch_credit::<R>(
            handle,
            netuid,
            None,
            pallet_subtensor::Call::<R>::withdraw_gamma {
                hotkey: hotkey.0.into(),
                netuid: netuid.into(),
                amount: GammaBalance::from(amount),
            },
        )
    }

    #[precompile::public("transferGamma(bytes32,bool,bytes32,bool,bytes32,uint16,uint64)")]
    #[allow(clippy::too_many_arguments)]
    fn transfer_gamma(
        handle: &mut impl PrecompileHandle,
        destination_coldkey: H256,
        has_origin_hotkey: bool,
        origin_hotkey: H256,
        has_destination_hotkey: bool,
        destination_hotkey: H256,
        netuid: u16,
        amount: u64,
    ) -> EvmResult<()> {
        dispatch::<R>(
            handle,
            pallet_subtensor::Call::<R>::transfer_gamma {
                destination_coldkey: destination_coldkey.0.into(),
                origin_hotkey: has_origin_hotkey.then(|| origin_hotkey.0.into()),
                destination_hotkey: has_destination_hotkey.then(|| destination_hotkey.0.into()),
                netuid: netuid.into(),
                amount: GammaBalance::from(amount),
            },
        )
    }

    /// Sets the service of the caller's hotkey on `netuid`: the url where it describes its
    /// services. An empty url removes it.
    #[precompile::public("setService(uint16,bytes32,string)")]
    fn set_service(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
        url: BoundedString<ConstU32<{ pallet_subtensor::MAX_SERVICE_URL_LEN }>>,
    ) -> EvmResult<()> {
        dispatch::<R>(
            handle,
            pallet_subtensor::Call::<R>::set_service {
                netuid: netuid.into(),
                hotkey: hotkey.0.into(),
                url: BoundedVec::truncate_from(url.into()),
            },
        )
    }

    /// The deposit at `hotkey` on `netuid` and what its hotkey can spend from it now, as
    /// `GammaRuntimeApi::get_gamma_deposit` returns it. `exists` is false when there is none.
    #[precompile::public("getGammaDeposit(uint16,bytes32)")]
    #[precompile::view]
    fn get_gamma_deposit(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
    ) -> EvmResult<(bool, H256, u64, bool, u64, u64)> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            match pallet_subtensor::Pallet::<R>::get_gamma_deposit(
                NetUid::from(netuid),
                R::AccountId::from(hotkey.0),
            ) {
                Some(info) => {
                    let cap = info.deposit.daily_spend_cap.map(|cap| cap.cap.to_u64());
                    (
                        true,
                        H256::from(info.deposit.coldkey.into()),
                        info.deposit.balance.to_u64(),
                        cap.is_some(),
                        cap.unwrap_or_default(),
                        info.spendable.to_u64(),
                    )
                }
                None => (false, H256::zero(), 0, false, 0, 0),
            },
        )
    }

    /// The hotkeys holding `coldkey`'s deposits on `netuid`, at most 64.
    #[precompile::public("getGammaDepositHotkeys(uint16,bytes32)")]
    #[precompile::view]
    fn get_gamma_deposit_hotkeys(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        coldkey: H256,
    ) -> EvmResult<Vec<H256>> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ColdkeyGammaHotkeys::<R>::get(
            NetUid::from(netuid),
            R::AccountId::from(coldkey.0),
        )
        .into_iter()
        .map(|hotkey| H256::from(hotkey.into()))
        .collect())
    }

    #[precompile::public("getGammaBalance(uint16,bytes32)")]
    #[precompile::view]
    fn get_gamma_balance(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        coldkey: H256,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::GammaBalances::<R>::get(
            NetUid::from(netuid),
            R::AccountId::from(coldkey.0),
        )
        .to_u64())
    }

    #[precompile::public("getGammaSpent(uint16,bytes32,bytes32)")]
    #[precompile::view]
    fn get_gamma_spent(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        service: H256,
        hotkey: H256,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::GammaSpent::<R>::get((
            NetUid::from(netuid),
            R::AccountId::from(service.0),
            R::AccountId::from(hotkey.0),
        ))
        .to_u64())
    }

    /// The url of `hotkey`'s service on `netuid`. `exists` is false when it has none.
    #[precompile::public("getService(uint16,bytes32)")]
    #[precompile::view]
    fn get_service(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
    ) -> EvmResult<(
        bool,
        BoundedString<ConstU32<{ pallet_subtensor::MAX_SERVICE_URL_LEN }>>,
    )> {
        handle.record_db_reads::<R>(1)?;
        let service = pallet_subtensor::Services::<R>::get(
            NetUid::from(netuid),
            R::AccountId::from(hotkey.0),
        );
        Ok((
            service.is_some(),
            service
                .map_or_else(Vec::new, |service| service.url.into_inner())
                .into(),
        ))
    }

    #[precompile::public("getGammaEnabled()")]
    #[precompile::view]
    fn get_gamma_enabled(handle: &mut impl PrecompileHandle) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::GammaEnabled::<R>::get())
    }

    /// The alpha burned, TAO value and gamma `mintGamma` would mint for `amount` now, as
    /// `GammaRuntimeApi::sim_mint_gamma` returns them; fails with the error that stops it.
    #[precompile::public("simMintGamma(uint16,uint64)")]
    #[precompile::view]
    fn sim_mint_gamma(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        amount: u64,
    ) -> EvmResult<(u64, u64, u64)> {
        record_weight::<R>(
            handle,
            <R as pallet_subtensor::Config>::WeightInfo::mint_gamma(),
        )?;
        sim_result(pallet_subtensor::Pallet::<R>::sim_mint_gamma(
            NetUid::from(netuid),
            TaoBalance::from(amount),
        ))
    }

    /// `alphaAmount` and the TAO value and gamma `mintGammaWithStake` would mint for it now, as
    /// `GammaRuntimeApi::sim_mint_gamma_with_stake` returns them; fails with the error that
    /// stops it. It reads no stake.
    #[precompile::public("simMintGammaWithStake(uint16,uint64)")]
    #[precompile::view]
    fn sim_mint_gamma_with_stake(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        alpha_amount: u64,
    ) -> EvmResult<(u64, u64, u64)> {
        record_weight::<R>(
            handle,
            <R as pallet_subtensor::Config>::WeightInfo::mint_gamma_with_stake(),
        )?;
        sim_result(pallet_subtensor::Pallet::<R>::sim_mint_gamma_with_stake(
            NetUid::from(netuid),
            AlphaBalance::from(alpha_amount),
        ))
    }
}

fn dispatch<R>(handle: &mut impl PrecompileHandle, call: pallet_subtensor::Call<R>) -> EvmResult<()>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <R as frame_system::Config>::RuntimeOrigin: From<RawOrigin<R::AccountId>> + Clone,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    let caller = handle.caller_account_id::<R>()?;
    handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
}

/// Dispatches `call` as the caller and returns the gamma it added to the deposit at `hotkey` or,
/// with none, to the caller's balance on `netuid`.
fn dispatch_credit<R>(
    handle: &mut impl PrecompileHandle,
    netuid: u16,
    hotkey: Option<R::AccountId>,
    call: pallet_subtensor::Call<R>,
) -> EvmResult<u64>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>,
    <R as frame_system::Config>::RuntimeOrigin: From<RawOrigin<R::AccountId>> + Clone,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    let netuid = NetUid::from(netuid);
    let caller = handle.caller_account_id::<R>()?;
    let held = || match &hotkey {
        Some(hotkey) => pallet_subtensor::GammaDeposits::<R>::get(netuid, hotkey)
            .map_or(0, |deposit| deposit.balance.to_u64()),
        None => pallet_subtensor::GammaBalances::<R>::get(netuid, &caller).to_u64(),
    };
    handle.record_db_reads::<R>(2)?;
    let before = held();
    handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller.clone()))?;
    Ok(held().saturating_sub(before))
}

/// Charges a simulation the weight of the mint it simulates, which does all of its work.
fn record_weight<R>(handle: &mut impl PrecompileHandle, weight: Weight) -> EvmResult<()>
where
    R: pallet_evm::Config,
{
    handle.record_external_cost(Some(weight.ref_time()), Some(weight.proof_size()), None)?;
    handle.record_cost(<R as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
        weight,
    ))?;
    Ok(())
}

fn sim_result(
    result: Result<(AlphaBalance, TaoBalance, GammaBalance), DispatchError>,
) -> EvmResult<(u64, u64, u64)> {
    let (alpha, value, gamma) = result.map_err(|error| PrecompileFailure::Error {
        exit_status: ExitError::Other(Into::<&'static str>::into(error).into()),
    })?;
    Ok((alpha.to_u64(), value.to_u64(), gamma.to_u64()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::mock::{
        AccountId, Runtime, addr_from_index, fund_account, mapped_account, new_test_ext,
        precompiles, selector_u32,
    };
    use frame_support::BoundedVec;
    use pallet_evm::GasWeightMapping;
    use pallet_evm::Runner;
    use precompile_utils::prelude::RuntimeHelper;
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::{H160, U256};
    use substrate_fixed::types::U64F64;
    use subtensor_swap_interface::SwapHandler;

    const NETUID_U16: u16 = 1;
    const TAO: u64 = 1_000_000_000;
    // 400 gamma per TAO: 400 US dollars.
    const TAO_USD_PRICE: u64 = 400 * TAO;

    fn gamma_subnet() -> NetUid {
        let netuid = NetUid::from(NETUID_U16);
        pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, 100);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 1);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(1_000 * TAO));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(1_000 * TAO));
        pallet_subtensor::GammaEnabled::<Runtime>::put(true);
        pallet_subtensor::SubnetGammaEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::TaoUsdPrice::<Runtime>::put(pallet_subtensor::TaoUsdPriceState {
            accepted: TAO_USD_PRICE,
            feed: TAO_USD_PRICE,
            fed_at: pallet_subtensor::Pallet::<Runtime>::get_current_block_as_u64(),
        });
        let spot: U64F64 =
            <Runtime as pallet_subtensor::Config>::SwapInterface::current_alpha_price(netuid);
        pallet_subtensor::SubnetGammaPrice::<Runtime>::insert(netuid, spot);
        netuid
    }

    fn key(byte: u8) -> AccountId {
        AccountId::from([byte; 32])
    }

    fn h256(account: &AccountId) -> H256 {
        H256::from_slice(account.as_ref())
    }

    fn gamma() -> H160 {
        addr_from_index(GammaPrecompile::<Runtime>::INDEX)
    }

    fn call(caller: H160, input: Vec<u8>) {
        precompiles::<GammaPrecompile<Runtime>>()
            .prepare_test(caller, gamma(), input)
            .execute_returns(());
    }

    /// `caller` makes the call, which returns `amount`.
    fn returns(caller: H160, input: Vec<u8>, amount: u64) {
        precompiles::<GammaPrecompile<Runtime>>()
            .prepare_test(caller, gamma(), input)
            .execute_returns(amount);
    }

    /// The gamma a mint of `amount` TAO gives now.
    fn sim_mint(amount: u64) -> u64 {
        pallet_subtensor::Pallet::<Runtime>::sim_mint_gamma(
            NetUid::from(NETUID_U16),
            TaoBalance::from(amount),
        )
        .unwrap()
        .2
        .to_u64()
    }

    fn refused(caller: H160, input: Vec<u8>, error: &str) {
        precompiles::<GammaPrecompile<Runtime>>()
            .prepare_test(caller, gamma(), input)
            .execute_error(ExitError::Other(
                format!("dispatch execution failed: {error}").into(),
            ));
    }

    fn mint(has_hotkey: bool, hotkey: &AccountId, amount: u64, cap: Option<u64>) -> Vec<u8> {
        encode_with_selector(
            selector_u32("mintGamma(bool,bytes32,uint16,uint64,uint64,bool,uint64)"),
            (
                has_hotkey,
                h256(hotkey),
                NETUID_U16,
                amount,
                0_u64,
                cap.is_some(),
                cap.unwrap_or_default(),
            ),
        )
    }

    fn deposit(hotkey: &AccountId) -> Option<pallet_subtensor::GammaDeposit<AccountId>> {
        pallet_subtensor::GammaDeposits::<Runtime>::get(NetUid::from(NETUID_U16), hotkey)
    }

    fn cap(hotkey: &AccountId) -> Option<u64> {
        deposit(hotkey)?.daily_spend_cap.map(|cap| cap.cap.to_u64())
    }

    fn stake(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> u64 {
        pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey, coldkey, netuid,
        )
        .to_u64()
    }

    fn balance(coldkey: &AccountId) -> u64 {
        pallet_subtensor::GammaBalances::<Runtime>::get(NetUid::from(NETUID_U16), coldkey).to_u64()
    }

    #[test]
    fn gamma_calls_dispatch_as_the_callers_mapped_account() {
        new_test_ext().execute_with(|| {
            gamma_subnet();
            let caller = addr_from_index(0x6001);
            let coldkey = mapped_account(caller);
            let (api_key, other_key, receiver) = (key(0x61), key(0x62), key(0x63));
            fund_account(&coldkey, 10 * TAO);

            // No hotkey credits the balance; the hotkey's value is ignored. A mint returns what it
            // credits, and a call taking up to an amount returns what it moved.
            let minted = sim_mint(TAO);
            assert!(minted > 0);
            returns(caller, mint(false, &api_key, TAO, None), minted);
            assert_eq!(balance(&coldkey), minted);
            assert_eq!(deposit(&api_key), None);

            returns(
                caller,
                encode_with_selector(
                    selector_u32("depositGamma(bytes32,uint16,uint64)"),
                    (h256(&api_key), NETUID_U16, u64::MAX),
                ),
                minted,
            );
            assert_eq!(balance(&coldkey), 0);
            let opened = deposit(&api_key).expect("deposit opened");
            assert_eq!(
                (opened.coldkey.clone(), opened.balance.to_u64()),
                (coldkey.clone(), minted)
            );
            assert_eq!(opened.daily_spend_cap, None);

            call(
                caller,
                encode_with_selector(
                    selector_u32("setGammaDepositDailySpendCap(bytes32,uint16,bool,uint64)"),
                    (h256(&api_key), NETUID_U16, true, TAO),
                ),
            );
            assert_eq!(cap(&api_key), Some(TAO));

            returns(
                caller,
                encode_with_selector(
                    selector_u32("moveGamma(bytes32,bytes32,uint16,uint64)"),
                    (h256(&api_key), h256(&other_key), NETUID_U16, minted),
                ),
                minted,
            );
            assert_eq!(deposit(&api_key), None);
            assert_eq!(deposit(&other_key).unwrap().balance.to_u64(), minted);

            returns(
                caller,
                encode_with_selector(
                    selector_u32("withdrawGamma(bytes32,uint16,uint64)"),
                    (h256(&other_key), NETUID_U16, minted),
                ),
                minted,
            );
            assert_eq!((deposit(&other_key), balance(&coldkey)), (None, minted));

            call(
                caller,
                encode_with_selector(
                    selector_u32("transferGamma(bytes32,bool,bytes32,bool,bytes32,uint16,uint64)"),
                    (
                        h256(&receiver),
                        false,
                        h256(&api_key),
                        false,
                        h256(&other_key),
                        NETUID_U16,
                        minted,
                    ),
                ),
            );
            assert_eq!((balance(&coldkey), balance(&receiver)), (0, minted));

            // A hotkey opens a deposit with the cap given, and a top-up returns only what it adds;
            // closing needs it empty.
            let opened = sim_mint(TAO);
            returns(caller, mint(true, &api_key, TAO, Some(TAO)), opened);
            let added = sim_mint(TAO);
            returns(caller, mint(true, &api_key, TAO, Some(TAO)), added);
            let topped = deposit(&api_key).unwrap();
            assert_eq!(topped.balance.to_u64(), opened + added);
            assert_eq!(cap(&api_key), Some(TAO));
            let close = encode_with_selector(
                selector_u32("closeGammaDeposit(bytes32,uint16)"),
                (h256(&api_key), NETUID_U16),
            );
            refused(caller, close.clone(), "GammaDepositNotEmpty");
            returns(
                caller,
                encode_with_selector(
                    selector_u32("withdrawGamma(bytes32,uint16,uint64)"),
                    (h256(&api_key), NETUID_U16, topped.balance.to_u64()),
                ),
                topped.balance.to_u64(),
            );
            assert_eq!(deposit(&api_key), None);
        });
    }

    #[test]
    fn a_spend_is_signed_by_the_caller_as_its_deposits_hotkey() {
        new_test_ext().execute_with(|| {
            let netuid = gamma_subnet();
            let caller = addr_from_index(0x6002);
            let hotkey = mapped_account(caller);
            let service = key(0x64);
            pallet_subtensor::Services::<Runtime>::insert(
                netuid,
                &service,
                pallet_subtensor::ServiceInfo {
                    url: BoundedVec::truncate_from(b"https://service.example".to_vec()),
                },
            );
            fund_account(&hotkey, 10 * TAO);
            // The caller mints to a deposit at its own mapped account, then spends as its hotkey.
            let held = sim_mint(TAO);
            returns(caller, mint(true, &hotkey, TAO, None), held);

            call(
                caller,
                encode_with_selector(
                    selector_u32("spendGamma(bytes32,uint16,uint64)"),
                    (h256(&service), NETUID_U16, TAO / 2),
                ),
            );
            assert_eq!(
                pallet_subtensor::GammaSpent::<Runtime>::get((netuid, &service, &hotkey)).to_u64(),
                TAO / 2
            );
            assert_eq!(deposit(&hotkey).unwrap().balance.to_u64(), held - TAO / 2);

            precompiles::<GammaPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    gamma(),
                    encode_with_selector(
                        selector_u32("getGammaSpent(uint16,bytes32,bytes32)"),
                        (NETUID_U16, h256(&service), h256(&hotkey)),
                    ),
                )
                .with_static_call(true)
                .execute_returns(TAO / 2);
        });
    }

    #[test]
    fn a_mint_with_stake_burns_the_callers_stake() {
        new_test_ext().execute_with(|| {
            let netuid = gamma_subnet();
            let caller = addr_from_index(0x6003);
            let coldkey = mapped_account(caller);
            let validator = key(0x65);
            let api_key = key(0x66);
            pallet_subtensor::Owner::<Runtime>::insert(&validator, &validator);
            fund_account(&coldkey, 10 * TAO);
            pallet_subtensor::Pallet::<Runtime>::add_stake(
                RawOrigin::Signed(coldkey.clone()).into(),
                validator.clone(),
                netuid,
                TaoBalance::from(5 * TAO),
            )
            .unwrap();
            let staked = stake(&validator, &coldkey, netuid);
            let credited = pallet_subtensor::Pallet::<Runtime>::sim_mint_gamma_with_stake(
                netuid,
                AlphaBalance::from(TAO),
            )
            .unwrap()
            .2
            .to_u64();

            returns(
                caller,
                encode_with_selector(
                    selector_u32(
                        "mintGammaWithStake(bytes32,bool,bytes32,uint16,uint64,uint64,bool,uint64)",
                    ),
                    (
                        h256(&validator),
                        true,
                        h256(&api_key),
                        NETUID_U16,
                        TAO,
                        0_u64,
                        false,
                        0_u64,
                    ),
                ),
                credited,
            );
            assert_eq!(stake(&validator, &coldkey, netuid), staked - TAO);
            assert_eq!(deposit(&api_key).unwrap().coldkey, coldkey);
        });
    }

    #[test]
    fn a_contract_mints_for_a_user_and_a_refused_transfer_undoes_the_mint() {
        new_test_ext().execute_with(|| {
            gamma_subnet();
            let caller = addr_from_index(0x6009);
            let contract = addr_from_index(0x600a);
            let funds = mapped_account(contract);
            let (user, user_key) = (key(0x6a), key(0x6b));
            fund_account(&mapped_account(caller), TAO);
            fund_account(&funds, 10 * TAO);
            // Copies its calldata, two 228-byte gamma calls, and calls the precompile with each in
            // turn, reverting if either fails.
            let code = hex::decode(concat!(
                "366000600037",
                "6000600060e4600060006108145af115602d57",
                "6000600060e460e460006108145af115602d5700",
                "5b60006000fd",
            ))
            .unwrap();
            pallet_evm::AccountCodes::<Runtime>::insert(contract, code);
            let mint_then_send = || {
                let gamma = sim_mint(TAO);
                let mut input = mint(false, &user_key, TAO, None);
                input.extend(encode_with_selector(
                    selector_u32("transferGamma(bytes32,bool,bytes32,bool,bytes32,uint16,uint64)"),
                    (
                        h256(&user),
                        false,
                        H256::zero(),
                        true,
                        h256(&user_key),
                        NETUID_U16,
                        gamma,
                    ),
                ));
                let result = <Runtime as pallet_evm::Config>::Runner::call(
                    caller,
                    contract,
                    input,
                    U256::zero(),
                    50_000_000,
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
                (result.exit_reason, gamma)
            };

            // The user's hotkey belongs to another coldkey, so the transfer is refused and the
            // revert takes back the mint and its TAO.
            pallet_subtensor::Owner::<Runtime>::insert(&user_key, key(0x6c));
            let (exit, _) = mint_then_send();
            assert!(exit.is_revert(), "{exit:?}");
            assert_eq!((balance(&funds), deposit(&user_key)), (0, None));
            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(&funds),
                TaoBalance::from(10 * TAO)
            );

            // Once the user owns it, the deposit opens for the user with the minted gamma.
            pallet_subtensor::Owner::<Runtime>::insert(&user_key, &user);
            let (exit, gamma) = mint_then_send();
            assert!(exit.is_succeed(), "{exit:?}");
            let opened = deposit(&user_key).unwrap();
            assert_eq!((opened.coldkey, opened.balance.to_u64()), (user, gamma));
            assert_eq!(balance(&funds), 0);
        });
    }

    #[test]
    fn a_refused_call_fails_and_changes_nothing() {
        new_test_ext().execute_with(|| {
            gamma_subnet();
            let caller = addr_from_index(0x6004);
            let coldkey = mapped_account(caller);
            let api_key = key(0x67);
            fund_account(&coldkey, 10 * TAO);

            // A balance takes no cap.
            refused(
                caller,
                mint(false, &api_key, TAO, Some(TAO)),
                "GammaDepositDailySpendCapMismatch",
            );
            refused(
                caller,
                encode_with_selector(
                    selector_u32("withdrawGamma(bytes32,uint16,uint64)"),
                    (h256(&api_key), NETUID_U16, TAO),
                ),
                "GammaDepositDoesNotExist",
            );
            assert_eq!((balance(&coldkey), deposit(&api_key)), (0, None));
            assert_eq!(
                pallet_balances::Pallet::<Runtime>::free_balance(&coldkey),
                TaoBalance::from(10 * TAO)
            );
        });
    }

    #[test]
    fn views_read_deposits_balances_and_report_missing_state() {
        new_test_ext().execute_with(|| {
            gamma_subnet();
            let caller = addr_from_index(0x6005);
            let coldkey = mapped_account(caller);
            let api_key = key(0x68);
            let gamma_precompiles = precompiles::<GammaPrecompile<Runtime>>();
            let read = |input: Vec<u8>| {
                gamma_precompiles
                    .prepare_test(caller, gamma(), input)
                    .with_static_call(true)
            };
            let deposit_view = || {
                encode_with_selector(
                    selector_u32("getGammaDeposit(uint16,bytes32)"),
                    (NETUID_U16, h256(&api_key)),
                )
            };
            let hotkeys_view = || {
                encode_with_selector(
                    selector_u32("getGammaDepositHotkeys(uint16,bytes32)"),
                    (NETUID_U16, h256(&coldkey)),
                )
            };

            read(deposit_view()).execute_returns((false, H256::zero(), 0_u64, false, 0_u64, 0_u64));
            read(hotkeys_view()).execute_returns(Vec::<H256>::new());
            read(encode_with_selector(
                selector_u32("getGammaBalance(uint16,bytes32)"),
                (NETUID_U16, h256(&coldkey)),
            ))
            .execute_returns(0_u64);
            read(encode_with_selector(selector_u32("getGammaEnabled()"), ())).execute_returns(true);

            fund_account(&coldkey, 10 * TAO);
            returns(caller, mint(true, &api_key, TAO, Some(TAO)), sim_mint(TAO));
            let held = deposit(&api_key).unwrap().balance.to_u64();
            read(deposit_view())
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns((true, h256(&coldkey), held, true, TAO, TAO));
            read(hotkeys_view()).execute_returns(vec![h256(&api_key)]);
        });
    }

    #[test]
    fn the_subnet_owner_sets_and_removes_its_service() {
        new_test_ext().execute_with(|| {
            let netuid = gamma_subnet();
            let caller = addr_from_index(0x6007);
            let owner = mapped_account(caller);
            let hotkey = key(0x69);
            fund_account(&owner, TAO);
            pallet_subtensor::SubnetOwner::<Runtime>::insert(netuid, &owner);
            pallet_subtensor::SubnetOwnerHotkey::<Runtime>::insert(netuid, &hotkey);
            pallet_subtensor::Owner::<Runtime>::insert(&hotkey, &owner);
            pallet_subtensor::Pallet::<Runtime>::append_neuron(netuid, &hotkey, 0);
            let set = |url: &str| {
                encode_with_selector(
                    selector_u32("setService(uint16,bytes32,string)"),
                    (
                        NETUID_U16,
                        h256(&hotkey),
                        BoundedString::<ConstU32<{ pallet_subtensor::MAX_SERVICE_URL_LEN }>>::from(
                            url,
                        ),
                    ),
                )
            };
            let get = |exists: bool, url: &str| {
                precompiles::<GammaPrecompile<Runtime>>()
                    .prepare_test(
                        caller,
                        gamma(),
                        encode_with_selector(
                            selector_u32("getService(uint16,bytes32)"),
                            (NETUID_U16, h256(&hotkey)),
                        ),
                    )
                    .with_static_call(true)
                    .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                    .execute_returns((
                        exists,
                        BoundedString::<ConstU32<{ pallet_subtensor::MAX_SERVICE_URL_LEN }>>::from(
                            url,
                        ),
                    ));
            };

            call(caller, set("https://service.example"));
            get(true, "https://service.example");
            refused(
                addr_from_index(0x6008),
                set("https://other.example"),
                "NonAssociatedColdKey",
            );
            call(caller, set(""));
            get(false, "");
        });
    }

    #[test]
    fn sims_match_the_runtime_api_and_fail_with_its_error() {
        new_test_ext().execute_with(|| {
            let netuid = gamma_subnet();
            let caller = addr_from_index(0x6006);
            let mint_gas = <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
                <Runtime as pallet_subtensor::Config>::WeightInfo::mint_gamma(),
            );
            let sim = |amount: u64| {
                encode_with_selector(
                    selector_u32("simMintGamma(uint16,uint64)"),
                    (NETUID_U16, amount),
                )
            };
            let (alpha, value, minted) =
                pallet_subtensor::Pallet::<Runtime>::sim_mint_gamma(netuid, TaoBalance::from(TAO))
                    .unwrap();
            precompiles::<GammaPrecompile<Runtime>>()
                .prepare_test(caller, gamma(), sim(TAO))
                .with_static_call(true)
                .expect_cost(mint_gas)
                .execute_returns((alpha.to_u64(), value.to_u64(), minted.to_u64()));

            let (alpha, value, minted) =
                pallet_subtensor::Pallet::<Runtime>::sim_mint_gamma_with_stake(
                    netuid,
                    AlphaBalance::from(TAO),
                )
                .unwrap();
            precompiles::<GammaPrecompile<Runtime>>()
                .prepare_test(
                    caller,
                    gamma(),
                    encode_with_selector(
                        selector_u32("simMintGammaWithStake(uint16,uint64)"),
                        (NETUID_U16, TAO),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    <Runtime as pallet_evm::Config>::GasWeightMapping::weight_to_gas(
                        <Runtime as pallet_subtensor::Config>::WeightInfo::mint_gamma_with_stake(),
                    ),
                )
                .execute_returns((alpha.to_u64(), value.to_u64(), minted.to_u64()));

            pallet_subtensor::SubnetGammaEnabled::<Runtime>::insert(netuid, false);
            precompiles::<GammaPrecompile<Runtime>>()
                .prepare_test(caller, gamma(), sim(TAO))
                .with_static_call(true)
                .execute_error(ExitError::Other("GammaDisabled".into()));
        });
    }
}
