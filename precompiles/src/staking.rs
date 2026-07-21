// The goal of staking precompile is to allow interaction between EVM users and smart contracts and
// subtensor staking functionality, namely add_stake, and remove_stake extrinsicsk, as well as the
// staking state.
//
// Additional requirement is to preserve compatibility with Ethereum indexers, which requires
// no balance transfers from EVM accounts without a corresponding transaction that can be
// parsed by an indexer.
//
// Implementation of add_stake:
//   - User transfers balance that will be staked to the precompile address with a payable
//     method addStake. This method also takes hotkey public key (bytes32) of the hotkey
//     that the stake should be assigned to.
//   - Precompile transfers the balance back to the signing address, and then invokes
//     do_add_stake from subtensor pallet with signing origin that mmatches to HashedAddressMapping
//     of the message sender, which will effectively withdraw and stake balance from the message
//     sender.
//   - Precompile checks the result of do_add_stake and, in case of a failure, reverts the transaction,
//     and leaves the balance on the message sender account.
//
// Implementation of remove_stake:
//   - User involkes removeStake method and specifies hotkey public key (bytes32) of the hotkey
//     to remove stake from, and the amount to unstake.
//   - Precompile calls do_remove_stake method of the subtensor pallet with the signing origin of message
//     sender, which effectively unstakes the specified amount and credits it to the message sender
//   - Precompile checks the result of do_remove_stake and, in case of a failure, reverts the transaction.
//
// Without an approve/allowance system, when an EOA transfers stake to a contract it is impossible for the
// contract to know who sent funds and how much. For that reason, the precompile provides an `approve`
// function for the sender to approve a spender (the contract) to call `transferStakeFrom`.
// The allowance is specific to a pair of `(spender, netuid)`, but doesn't specify the `hotkey` which is instead
// provided only in `transferStakeFrom`.

use alloc::vec::Vec;
use codec::{Compact, Encode};
use core::marker::PhantomData;
use frame_support::Blake2_128Concat;
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::pallet_prelude::{StorageDoubleMap, ValueQuery};
use frame_support::traits::{Get, IsSubType, StorageInstance};
use frame_system::RawOrigin;
use pallet_evm::{
    AddressMapping, BalanceConverter, EvmBalance, ExitError, PrecompileFailure, PrecompileHandle,
    SubstrateBalance,
};
use pallet_subtensor_proxy as pallet_proxy;
use precompile_utils::EvmResult;
use precompile_utils::prelude::{Address, revert};
use sp_core::{H160, H256, U256};
use sp_runtime::traits::{AsSystemOriginSigner, Dispatchable, StaticLookup, UniqueSaturatedInto};
use sp_std::vec;
use subtensor_runtime_common::{NetUid, ProxyType, Token};

use crate::{PrecompileExt, PrecompileHandleExt};

// `get_stake_for_hotkey_and_coldkey_on_subnet` reads the transitional V1/V2
// share storage. In the V2 fallback case it performs two reads for the initial
// share lookup, then five more for the value, share, and denominator.
const STAKE_INFO_READS_PER_HOTKEY: u64 = 7;
const MAX_STAKE_INFO_PAGE_SIZE: u32 = 64;
const ACCOUNT_ID_ENCODED_SIZE: usize = 32;

/// Prefix for the Allowances map in Substrate storage.
pub struct AllowancesPrefix;
impl StorageInstance for AllowancesPrefix {
    const STORAGE_PREFIX: &'static str = "Allowances";

    fn pallet_prefix() -> &'static str {
        "EvmPrecompileStaking"
    }
}

pub type AllowancesStorage = StorageDoubleMap<
    AllowancesPrefix,
    // For each approver (EVM address as only EVM-natives need the precompile)
    Blake2_128Concat,
    H160,
    // For each (spender, netuid, counter) triple — the counter tag invalidates
    // entries written under a previous registration of the same netuid.
    Blake2_128Concat,
    (H160, u16, u64),
    // Allowed amount
    U256,
    ValueQuery,
>;

// Old StakingPrecompile had ETH-precision in values, which was not alligned with Substrate API. So
// it's kinda deprecated, but exists for backward compatibility. Eventually, we should remove it
// to stop supporting both precompiles.
//
// All the future extensions should happen in StakingPrecompileV2.
pub struct StakingPrecompileV2<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for StakingPrecompileV2<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    const INDEX: u64 = 2053;
}

#[precompile_utils::precompile]
impl<R> StakingPrecompileV2<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    #[precompile::public("addStake(bytes32,uint256,uint256)")]
    #[precompile::payable]
    fn add_stake(
        handle: &mut impl PrecompileHandle,
        address: H256,
        amount_rao: U256,
        netuid: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let amount_staked: u64 = amount_rao.unique_saturated_into();
        let hotkey = R::AccountId::from(address.0);
        let netuid = try_u16_from_u256(netuid)?;
        let call = pallet_subtensor::Call::<R>::add_stake {
            hotkey,
            netuid: netuid.into(),
            amount_staked: amount_staked.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeStake(bytes32,uint256,uint256)")]
    #[precompile::payable]
    fn remove_stake(
        handle: &mut impl PrecompileHandle,
        address: H256,
        amount_alpha: U256,
        netuid: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(address.0);
        let netuid = try_u16_from_u256(netuid)?;
        let amount_unstaked: u64 = amount_alpha.unique_saturated_into();
        let call = pallet_subtensor::Call::<R>::remove_stake {
            hotkey,
            netuid: netuid.into(),
            amount_unstaked: amount_unstaked.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    fn call_remove_stake_full_limit(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: U256,
        limit_price: Option<u64>,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(hotkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        let call = pallet_subtensor::Call::<R>::remove_stake_full_limit {
            hotkey,
            netuid: netuid.into(),
            limit_price: limit_price.map(Into::into),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeStakeFull(bytes32,uint256)")]
    #[precompile::payable]
    fn remove_stake_full(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: U256,
    ) -> EvmResult<()> {
        Self::call_remove_stake_full_limit(handle, hotkey, netuid, None)
    }

    #[precompile::public("removeStakeFullLimit(bytes32,uint256,uint256)")]
    #[precompile::payable]
    fn remove_stake_full_limit(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: U256,
        limit_price: U256,
    ) -> EvmResult<()> {
        let limit_price = try_u64_from_u256(limit_price)?;
        Self::call_remove_stake_full_limit(handle, hotkey, netuid, Some(limit_price))
    }

    #[precompile::public("moveStake(bytes32,bytes32,uint256,uint256,uint256)")]
    #[precompile::payable]
    fn move_stake(
        handle: &mut impl PrecompileHandle,
        origin_hotkey: H256,
        destination_hotkey: H256,
        origin_netuid: U256,
        destination_netuid: U256,
        amount_alpha: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let origin_hotkey = R::AccountId::from(origin_hotkey.0);
        let destination_hotkey = R::AccountId::from(destination_hotkey.0);
        let origin_netuid = try_u16_from_u256(origin_netuid)?;
        let destination_netuid = try_u16_from_u256(destination_netuid)?;
        let alpha_amount: u64 = amount_alpha.unique_saturated_into();
        let call = pallet_subtensor::Call::<R>::move_stake {
            origin_hotkey,
            destination_hotkey,
            origin_netuid: origin_netuid.into(),
            destination_netuid: destination_netuid.into(),
            alpha_amount: alpha_amount.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("transferStake(bytes32,bytes32,uint256,uint256,uint256)")]
    #[precompile::payable]
    fn transfer_stake(
        handle: &mut impl PrecompileHandle,
        destination_coldkey: H256,
        hotkey: H256,
        origin_netuid: U256,
        destination_netuid: U256,
        amount_alpha: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let destination_coldkey = R::AccountId::from(destination_coldkey.0);
        let hotkey = R::AccountId::from(hotkey.0);
        let origin_netuid = try_u16_from_u256(origin_netuid)?;
        let destination_netuid = try_u16_from_u256(destination_netuid)?;
        let alpha_amount: u64 = amount_alpha.unique_saturated_into();
        let call = pallet_subtensor::Call::<R>::transfer_stake {
            destination_coldkey,
            hotkey,
            origin_netuid: origin_netuid.into(),
            destination_netuid: destination_netuid.into(),
            alpha_amount: alpha_amount.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("burnAlpha(bytes32,uint256,uint256)")]
    #[precompile::payable]
    fn burn_alpha(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        amount: U256,
        netuid: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(hotkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        let amount: u64 = amount.unique_saturated_into();
        let call = pallet_subtensor::Call::<R>::burn_alpha {
            hotkey,
            amount: amount.into(),
            netuid: netuid.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("getTotalColdkeyStake(bytes32)")]
    #[precompile::view]
    fn get_total_coldkey_stake(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<U256> {
        // StakingHotkeys + per-hotkey stake reads
        handle.record_db_reads::<R>(2)?;
        let coldkey = R::AccountId::from(coldkey.0);
        let stake = pallet_subtensor::Pallet::<R>::get_total_stake_for_coldkey(&coldkey);

        Ok(stake.to_u64().into())
    }

    #[precompile::public("getTotalHotkeyStake(bytes32)")]
    #[precompile::view]
    fn get_total_hotkey_stake(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<U256> {
        // Per-subnet stake + alpha price reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = R::AccountId::from(hotkey.0);
        let stake = pallet_subtensor::Pallet::<R>::get_total_stake_for_hotkey(&hotkey);

        Ok(stake.to_u64().into())
    }

    #[precompile::public("getStake(bytes32,bytes32,uint256)")]
    #[precompile::view]
    fn get_stake(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        coldkey: H256,
        netuid: U256,
    ) -> EvmResult<U256> {
        // Alpha share pool reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = R::AccountId::from(hotkey.0);
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        let stake = pallet_subtensor::Pallet::<R>::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid.into(),
        );

        Ok(u64::from(stake).into())
    }

    #[precompile::public("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)")]
    #[precompile::view]
    fn get_stake_info_for_coldkey_and_netuid(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: U256,
        cursor: U256,
        limit: U256,
    ) -> EvmResult<(Vec<(H256, U256)>, U256)> {
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let cursor = try_u32_from_u256(cursor)?;
        let limit = try_u32_from_u256(limit)?;

        if limit == 0 {
            return Err(revert("stake info page limit must be greater than zero"));
        }
        if limit > MAX_STAKE_INFO_PAGE_SIZE {
            return Err(revert("stake info page limit exceeds 64"));
        }

        // Charge before even reading the SCALE vector length. `decode_len` reads
        // only the compact length prefix and never allocates or decodes the
        // historical, unbounded `StakingHotkeys` value.
        handle.record_db_reads::<R>(1)?;
        let hotkey_count =
            pallet_subtensor::StakingHotkeys::<R>::decode_len(&coldkey).unwrap_or_default();
        let cursor = usize::try_from(cursor).map_err(|_| revert("stake info cursor overflow"))?;

        if cursor >= hotkey_count {
            return Ok((Vec::new(), U256::zero()));
        }

        let limit = usize::try_from(limit).map_err(|_| revert("stake info limit overflow"))?;
        let page_end = cursor.saturating_add(limit).min(hotkey_count);
        let scanned_count = page_end.saturating_sub(cursor);

        // Charge the bounded raw page read and the worst-case V2 stake reads
        // before allocating or reading the page bytes.
        let scanned_count_u64: u64 = scanned_count.unique_saturated_into();
        let page_reads =
            1u64.saturating_add(scanned_count_u64.saturating_mul(STAKE_INFO_READS_PER_HOTKEY));
        handle.record_db_reads::<R>(page_reads)?;

        // AccountId32 has a fixed 32-byte SCALE encoding. Read only the requested
        // slice from the encoded Vec instead of calling `StakingHotkeys::get()`,
        // which would decode the entire unbounded historical vector.
        if coldkey.encode().len() != ACCOUNT_ID_ENCODED_SIZE {
            return Err(revert("unsupported account id encoding"));
        }
        let storage_key = pallet_subtensor::StakingHotkeys::<R>::hashed_key_for(&coldkey);
        let compact_len = Compact(
            u32::try_from(hotkey_count).map_err(|_| revert("staking hotkey count overflow"))?,
        )
        .encode()
        .len();
        let byte_offset = compact_len
            .checked_add(
                cursor
                    .checked_mul(ACCOUNT_ID_ENCODED_SIZE)
                    .ok_or_else(|| revert("stake info cursor overflow"))?,
            )
            .ok_or_else(|| revert("stake info cursor overflow"))?;
        let page_byte_len = scanned_count
            .checked_mul(ACCOUNT_ID_ENCODED_SIZE)
            .ok_or_else(|| revert("stake info page size overflow"))?;
        let storage_offset =
            u32::try_from(byte_offset).map_err(|_| revert("stake info cursor overflow"))?;
        let mut encoded_hotkeys = vec![0u8; page_byte_len];
        let remaining = sp_io::storage::read(&storage_key, &mut encoded_hotkeys, storage_offset)
            .ok_or_else(|| revert("staking hotkey storage disappeared"))?;
        if usize::try_from(remaining).unwrap_or_default() < page_byte_len {
            return Err(revert("invalid staking hotkey storage encoding"));
        }

        let hotkeys = encoded_hotkeys
            .chunks_exact(ACCOUNT_ID_ENCODED_SIZE)
            .map(|bytes| {
                let mut account = [0u8; ACCOUNT_ID_ENCODED_SIZE];
                account.copy_from_slice(bytes);
                R::AccountId::from(account)
            });

        let positions = hotkeys
            .into_iter()
            .filter_map(|hotkey| {
                let stake =
                    pallet_subtensor::Pallet::<R>::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey, &coldkey, netuid,
                    )
                    .to_u64();
                if stake == 0 {
                    return None;
                }

                let hotkey: [u8; 32] = hotkey.into();
                Some((hotkey.into(), stake.into()))
            })
            .collect();
        let next_cursor = if page_end < hotkey_count {
            U256::from(page_end)
        } else {
            U256::zero()
        };

        Ok((positions, next_cursor))
    }

    #[precompile::public("getAlphaStakedValidators(bytes32,uint256)")]
    #[precompile::view]
    fn get_alpha_staked_validators(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: U256,
    ) -> EvmResult<Vec<H256>> {
        let hotkey = R::AccountId::from(hotkey.0);
        let mut coldkeys: Vec<H256> = vec![];
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        for (coldkey, netuid_in_alpha, _) in
            pallet_subtensor::Pallet::<R>::alpha_iter_single_prefix(&hotkey)
        {
            handle.record_db_reads::<R>(1)?;
            if netuid == netuid_in_alpha {
                let key: [u8; 32] = coldkey.into();
                coldkeys.push(key.into());
            }
        }

        Ok(coldkeys)
    }

    #[precompile::public("getTotalAlphaStaked(bytes32,uint256)")]
    #[precompile::view]
    fn get_total_alpha_staked(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: U256,
    ) -> EvmResult<U256> {
        handle.record_db_reads::<R>(2)?;
        let hotkey = R::AccountId::from(hotkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        let stake =
            pallet_subtensor::Pallet::<R>::get_stake_for_hotkey_on_subnet(&hotkey, netuid.into());

        Ok(u64::from(stake).into())
    }

    #[precompile::public("getNominatorMinRequiredStake()")]
    #[precompile::view]
    fn get_nominator_min_required_stake(handle: &mut impl PrecompileHandle) -> EvmResult<U256> {
        // NominatorMinRequiredStake + DefaultMinStake reads
        handle.record_db_reads::<R>(2)?;
        let stake = pallet_subtensor::Pallet::<R>::get_nominator_min_required_stake();

        Ok(stake.into())
    }

    #[precompile::public("getStakeOperationThreshold()")]
    #[precompile::view]
    fn get_stake_operation_threshold(handle: &mut impl PrecompileHandle) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::DefaultMinStake::<R>::get()
            .to_u64()
            .into())
    }

    #[precompile::public("addProxy(bytes32)")]
    #[precompile::payable]
    fn add_proxy(handle: &mut impl PrecompileHandle, delegate: H256) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let delegate = R::AccountId::from(delegate.0);
        let delegate = <R as frame_system::Config>::Lookup::unlookup(delegate);
        let call = pallet_proxy::Call::<R>::add_proxy {
            delegate,
            proxy_type: ProxyType::Staking,
            delay: 0u32.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeProxy(bytes32)")]
    #[precompile::payable]
    fn remove_proxy(handle: &mut impl PrecompileHandle, delegate: H256) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let delegate = R::AccountId::from(delegate.0);
        let delegate = <R as frame_system::Config>::Lookup::unlookup(delegate);
        let call = pallet_proxy::Call::<R>::remove_proxy {
            delegate,
            proxy_type: ProxyType::Staking,
            delay: 0u32.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("addStakeLimit(bytes32,uint256,uint256,bool,uint256)")]
    #[precompile::payable]
    fn add_stake_limit(
        handle: &mut impl PrecompileHandle,
        address: H256,
        amount_rao: U256,
        limit_price_rao: U256,
        allow_partial: bool,
        netuid: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let amount_staked: u64 = amount_rao.unique_saturated_into();
        let limit_price: u64 = limit_price_rao.unique_saturated_into();
        let hotkey = R::AccountId::from(address.0);
        let netuid = try_u16_from_u256(netuid)?;
        let call = pallet_subtensor::Call::<R>::add_stake_limit {
            hotkey,
            netuid: netuid.into(),
            amount_staked: amount_staked.into(),
            limit_price: limit_price.into(),
            allow_partial,
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeStakeLimit(bytes32,uint256,uint256,bool,uint256)")]
    #[precompile::payable]
    fn remove_stake_limit(
        handle: &mut impl PrecompileHandle,
        address: H256,
        amount_alpha: U256,
        limit_price_rao: U256,
        allow_partial: bool,
        netuid: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(address.0);
        let netuid = try_u16_from_u256(netuid)?;
        let amount_unstaked: u64 = amount_alpha.unique_saturated_into();
        let limit_price: u64 = limit_price_rao.unique_saturated_into();
        let call = pallet_subtensor::Call::<R>::remove_stake_limit {
            hotkey,
            netuid: netuid.into(),
            amount_unstaked: amount_unstaked.into(),
            limit_price: limit_price.into(),
            allow_partial,
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("getTotalColdkeyStakeOnSubnet(bytes32,uint256)")]
    #[precompile::view]
    fn get_total_coldkey_stake_on_subnet(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: U256,
    ) -> EvmResult<U256> {
        // StakingHotkeys + per-hotkey stake reads
        handle.record_db_reads::<R>(2)?;
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        let stake = pallet_subtensor::Pallet::<R>::get_total_stake_for_coldkey_on_subnet(
            &coldkey,
            netuid.into(),
        );

        Ok(stake.to_u64().into())
    }

    /// Current registration counter for `netuid`, used as part of the
    /// `AllowancesStorage` secondary key to invalidate approvals granted
    /// for a previous registration of the same netuid.
    fn current_subnet_counter(netuid: u16) -> u64 {
        pallet_subtensor::Pallet::<R>::get_registered_subnet_counter(netuid.into())
    }

    #[precompile::public("approve(address,uint256,uint256)")]
    fn approve(
        handle: &mut impl PrecompileHandle,
        spender_address: Address,
        origin_netuid: U256,
        amount_alpha: U256,
    ) -> EvmResult<()> {
        // AllowancesStorage write + RegisteredSubnetCounter read
        handle.record_db_reads::<R>(1)?;
        handle.record_db_writes::<R>(1)?;

        let approver = handle.context().caller;
        let spender = spender_address.0;
        let netuid = try_u16_from_u256(origin_netuid)?;
        let counter = Self::current_subnet_counter(netuid);

        if amount_alpha.is_zero() {
            AllowancesStorage::remove(approver, (spender, netuid, counter));
        } else {
            AllowancesStorage::insert(approver, (spender, netuid, counter), amount_alpha);
        }

        Ok(())
    }

    #[precompile::public("allowance(address,address,uint256)")]
    #[precompile::view]
    fn allowance(
        handle: &mut impl PrecompileHandle,
        source_address: Address,
        spender_address: Address,
        origin_netuid: U256,
    ) -> EvmResult<U256> {
        // AllowancesStorage read + RegisteredSubnetCounter read
        handle.record_db_reads::<R>(2)?;

        let spender = spender_address.0;
        let netuid = try_u16_from_u256(origin_netuid)?;
        let counter = Self::current_subnet_counter(netuid);

        Ok(AllowancesStorage::get(
            source_address.0,
            (spender, netuid, counter),
        ))
    }

    #[precompile::public("increaseAllowance(address,uint256,uint256)")]
    fn increase_allowance(
        handle: &mut impl PrecompileHandle,
        spender_address: Address,
        origin_netuid: U256,
        amount_alpha_increase: U256,
    ) -> EvmResult<()> {
        if amount_alpha_increase.is_zero() {
            return Ok(());
        }

        // AllowancesStorage read + write + RegisteredSubnetCounter read
        handle.record_db_reads::<R>(2)?;
        handle.record_db_writes::<R>(1)?;

        let approver = handle.context().caller;
        let spender = spender_address.0;
        let netuid = try_u16_from_u256(origin_netuid)?;
        let counter = Self::current_subnet_counter(netuid);

        let approval_key = (spender, netuid, counter);

        let current_amount = AllowancesStorage::get(approver, approval_key);
        let new_amount = current_amount.saturating_add(amount_alpha_increase);

        AllowancesStorage::insert(approver, approval_key, new_amount);

        Ok(())
    }

    #[precompile::public("decreaseAllowance(address,uint256,uint256)")]
    fn decrease_allowance(
        handle: &mut impl PrecompileHandle,
        spender_address: Address,
        origin_netuid: U256,
        amount_alpha_decrease: U256,
    ) -> EvmResult<()> {
        if amount_alpha_decrease.is_zero() {
            return Ok(());
        }

        // AllowancesStorage read + write + RegisteredSubnetCounter read
        handle.record_db_reads::<R>(2)?;
        handle.record_db_writes::<R>(1)?;

        let approver = handle.context().caller;
        let spender = spender_address.0;
        let netuid = try_u16_from_u256(origin_netuid)?;
        let counter = Self::current_subnet_counter(netuid);

        let approval_key = (spender, netuid, counter);

        let current_amount = AllowancesStorage::get(approver, approval_key);
        let new_amount = current_amount.saturating_sub(amount_alpha_decrease);

        if new_amount.is_zero() {
            AllowancesStorage::remove(approver, approval_key);
        } else {
            AllowancesStorage::insert(approver, approval_key, new_amount);
        }

        Ok(())
    }

    fn try_consume_allowance(
        handle: &mut impl PrecompileHandle,
        approver: H160,
        spender: H160,
        netuid: u16,
        amount: U256,
    ) -> EvmResult<()> {
        if amount.is_zero() {
            return Ok(());
        }

        // AllowancesStorage read + write + RegisteredSubnetCounter read
        handle.record_db_reads::<R>(2)?;
        handle.record_db_writes::<R>(1)?;

        let counter = Self::current_subnet_counter(netuid);
        let approval_key = (spender, netuid, counter);

        let current_amount = AllowancesStorage::get(approver, approval_key);
        let Some(new_amount) = current_amount.checked_sub(amount) else {
            return Err(revert("trying to spend more than allowed"));
        };

        if new_amount.is_zero() {
            AllowancesStorage::remove(approver, approval_key);
        } else {
            AllowancesStorage::insert(approver, approval_key, new_amount);
        }

        Ok(())
    }

    #[precompile::public("transferStakeFrom(address,address,bytes32,uint256,uint256,uint256)")]
    fn transfer_stake_from(
        handle: &mut impl PrecompileHandle,
        source_address: Address,
        destination_address: Address,
        hotkey: H256,
        origin_netuid: U256,
        destination_netuid: U256,
        amount_alpha: U256,
    ) -> EvmResult<()> {
        let spender = handle.context().caller;
        let source_address = source_address.0;
        let destination_coldkey =
            <R as pallet_evm::Config>::AddressMapping::into_account_id(destination_address.0);
        let hotkey = R::AccountId::from(hotkey.0);
        let origin_netuid = try_u16_from_u256(origin_netuid)?;
        let destination_netuid = try_u16_from_u256(destination_netuid)?;
        let alpha_amount: u64 = amount_alpha.unique_saturated_into();

        Self::try_consume_allowance(handle, source_address, spender, origin_netuid, amount_alpha)?;

        let call = pallet_subtensor::Call::<R>::transfer_stake {
            destination_coldkey,
            hotkey,
            origin_netuid: origin_netuid.into(),
            destination_netuid: destination_netuid.into(),
            alpha_amount: alpha_amount.into(),
        };
        let source_id = <R as pallet_evm::Config>::AddressMapping::into_account_id(source_address);

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(source_id))
    }
}

// Deprecated, exists for backward compatibility.
pub struct StakingPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for StakingPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_balances::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + From<pallet_balances::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as pallet_balances::Config>::Balance: TryFrom<U256>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    const INDEX: u64 = 2049;
}

#[precompile_utils::precompile]
impl<R> StakingPrecompile<R>
where
    R: frame_system::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_balances::Config
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + From<pallet_balances::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as pallet_balances::Config>::Balance: TryFrom<U256>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    #[precompile::public("addStake(bytes32,uint256)")]
    #[precompile::payable]
    fn add_stake(handle: &mut impl PrecompileHandle, address: H256, netuid: U256) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let amount = handle.context().apparent_value;

        if !amount.is_zero() {
            Self::transfer_back_to_caller(&account_id, amount)?;
        }

        let amount_sub = handle.try_convert_apparent_value::<R>()?;
        let hotkey = R::AccountId::from(address.0);
        let netuid = try_u16_from_u256(netuid)?;
        let amount_staked: u64 = amount_sub.unique_saturated_into();
        let call = pallet_subtensor::Call::<R>::add_stake {
            hotkey,
            netuid: netuid.into(),
            amount_staked: amount_staked.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeStake(bytes32,uint256,uint256)")]
    #[precompile::payable]
    fn remove_stake(
        handle: &mut impl PrecompileHandle,
        address: H256,
        amount: U256,
        netuid: U256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let hotkey = R::AccountId::from(address.0);
        let netuid = try_u16_from_u256(netuid)?;
        let amount = EvmBalance::new(amount);
        let amount_unstaked =
            <R as pallet_evm::Config>::BalanceConverter::into_substrate_balance(amount)
                .map(|amount| amount.into_u64_saturating())
                .ok_or(ExitError::OutOfFund)?;
        let call = pallet_subtensor::Call::<R>::remove_stake {
            hotkey,
            netuid: netuid.into(),
            amount_unstaked: amount_unstaked.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("getTotalColdkeyStake(bytes32)")]
    #[precompile::view]
    fn get_total_coldkey_stake(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<U256> {
        // StakingHotkeys + per-hotkey stake reads
        handle.record_db_reads::<R>(2)?;
        let coldkey = R::AccountId::from(coldkey.0);

        // get total stake of coldkey
        let total_stake =
            pallet_subtensor::Pallet::<R>::get_total_stake_for_coldkey(&coldkey).to_u64();
        // Convert to EVM decimals
        let stake_u256: SubstrateBalance = total_stake.into();
        let stake_eth = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(stake_u256)
            .map(|amount| amount.into_u256())
            .ok_or(ExitError::InvalidRange)?;

        Ok(stake_eth)
    }

    #[precompile::public("getTotalHotkeyStake(bytes32)")]
    #[precompile::view]
    fn get_total_hotkey_stake(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<U256> {
        // Per-subnet stake + alpha price reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = R::AccountId::from(hotkey.0);

        // get total stake of hotkey
        let total_stake =
            pallet_subtensor::Pallet::<R>::get_total_stake_for_hotkey(&hotkey).to_u64();
        // Convert to EVM decimals
        let stake_u256: SubstrateBalance = total_stake.into();
        let stake_eth = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(stake_u256)
            .map(|amount| amount.into_u256())
            .ok_or(ExitError::InvalidRange)?;

        Ok(stake_eth)
    }

    #[precompile::public("getStake(bytes32,bytes32,uint256)")]
    #[precompile::view]
    fn get_stake(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        coldkey: H256,
        netuid: U256,
    ) -> EvmResult<U256> {
        // Alpha share pool reads
        handle.record_db_reads::<R>(2)?;
        let hotkey = R::AccountId::from(hotkey.0);
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        let stake = pallet_subtensor::Pallet::<R>::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid.into(),
        );
        let stake: SubstrateBalance = u64::from(stake).into();
        let stake = <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(stake)
            .map(|amount| amount.into_u256())
            .ok_or(ExitError::InvalidRange)?;

        Ok(stake)
    }

    #[precompile::public("addProxy(bytes32)")]
    #[precompile::payable]
    fn add_proxy(handle: &mut impl PrecompileHandle, delegate: H256) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let delegate = R::AccountId::from(delegate.0);
        let delegate = <R as frame_system::Config>::Lookup::unlookup(delegate);
        let call = pallet_proxy::Call::<R>::add_proxy {
            delegate,
            proxy_type: ProxyType::Staking,
            delay: 0u32.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeProxy(bytes32)")]
    #[precompile::payable]
    fn remove_proxy(handle: &mut impl PrecompileHandle, delegate: H256) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let delegate = R::AccountId::from(delegate.0);
        let delegate = <R as frame_system::Config>::Lookup::unlookup(delegate);
        let call = pallet_proxy::Call::<R>::remove_proxy {
            delegate,
            proxy_type: ProxyType::Staking,
            delay: 0u32.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    fn transfer_back_to_caller(
        account_id: &<R as frame_system::Config>::AccountId,
        amount: U256,
    ) -> Result<(), PrecompileFailure> {
        let amount = EvmBalance::new(amount);
        let amount_sub =
            <R as pallet_evm::Config>::BalanceConverter::into_substrate_balance(amount)
                .ok_or(ExitError::OutOfFund)?;

        // Create a transfer call from the smart contract to the caller
        let value = amount_sub
            .into_u64_saturating()
            .try_into()
            .map_err(|_| ExitError::Other("Failed to convert u64 to Balance".into()))?;
        let transfer_call = <R as frame_system::Config>::RuntimeCall::from(
            pallet_balances::Call::<R>::transfer_allow_death {
                dest: account_id.clone().into(),
                value,
            },
        );

        // Execute the transfer
        let transfer_result = transfer_call.dispatch(RawOrigin::Signed(Self::account_id()).into());

        if let Err(dispatch_error) = transfer_result {
            log::error!("Transfer back to caller failed. Error: {dispatch_error:?}");
            return Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("Transfer back to caller failed".into()),
            });
        }

        Ok(())
    }
}

fn try_u16_from_u256(value: U256) -> Result<u16, PrecompileFailure> {
    value.try_into().map_err(|_| PrecompileFailure::Error {
        exit_status: ExitError::Other("the value is outside of u16 bounds".into()),
    })
}

fn try_u32_from_u256(value: U256) -> Result<u32, PrecompileFailure> {
    value.try_into().map_err(|_| PrecompileFailure::Error {
        exit_status: ExitError::Other("the value is outside of u32 bounds".into()),
    })
}

fn try_u64_from_u256(value: U256) -> Result<u64, PrecompileFailure> {
    value.try_into().map_err(|_| PrecompileFailure::Error {
        exit_status: ExitError::Other("the value is outside of u64 bounds".into()),
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing
    )]

    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Proxy, Runtime, RuntimeCall, RuntimeOrigin, addr_from_index, assert_static_call,
        execute_precompile, fund_account, mapped_account, new_test_ext, precompiles, selector_u32,
        substrate_to_evm,
    };
    use precompile_utils::prelude::RuntimeHelper;
    use precompile_utils::solidity::{encode_return_value, encode_with_selector};
    use precompile_utils::testing::PrecompileTesterExt;
    use sp_core::{H160, H256};
    use substrate_fixed::types::U64F64;
    use subtensor_runtime_common::{AlphaBalance, TaoBalance};

    const TEST_NETUID_U16: u16 = 1;
    const SECOND_NETUID_U16: u16 = 2;
    const INVALID_NETUID_U16: u16 = 12_345;
    const TEMPO: u16 = 100;
    const RESERVE_TAO: u64 = 200_000_000_000;
    const RESERVE_ALPHA: u64 = 100_000_000_000;
    const INITIAL_STAKE_RAO: u64 = 20_000_000_000;
    const REMOVE_STAKE_RAO: u64 = 10_000_000_000;
    const PROXY_STAKE_RAO: u64 = 1_000_000_000;
    const COLDKEY_BALANCE: u64 = 100_000_000_000;
    const APPROVED_ALLOWANCE_RAO: u64 = 10_000_000_000;
    const TRANSFERRED_ALLOWANCE_RAO: u64 = 5_000_000_000;
    const ALLOWANCE_DECREASE_RAO: u64 = 2_000_000_000;

    fn setup_staking_subnet() -> NetUid {
        setup_staking_subnet_id(TEST_NETUID_U16)
    }

    fn setup_staking_subnet_id(netuid: u16) -> NetUid {
        let netuid = NetUid::from(netuid);
        pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, TEMPO);
        pallet_subtensor::Pallet::<Runtime>::set_network_registration_allowed(netuid, true);
        pallet_subtensor::Pallet::<Runtime>::set_max_allowed_uids(netuid, 4096);
        pallet_subtensor::FirstEmissionBlockNumber::<Runtime>::insert(netuid, 0);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::BurnHalfLife::<Runtime>::insert(netuid, 1);
        pallet_subtensor::BurnIncreaseMult::<Runtime>::insert(netuid, U64F64::from_num(1));
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(RESERVE_TAO));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            netuid,
            AlphaBalance::from(RESERVE_ALPHA),
        );
        netuid
    }

    fn stake_info_cost(scanned_count: usize) -> u64 {
        let scanned_count = u64::try_from(scanned_count).expect("hotkey count fits in u64");
        let reads = if scanned_count == 0 {
            1
        } else {
            2u64.saturating_add(scanned_count.saturating_mul(STAKE_INFO_READS_PER_HOTKEY))
        };
        RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(reads)
    }

    fn hotkey() -> AccountId {
        AccountId::from([0x11; 32])
    }

    fn delegate() -> AccountId {
        AccountId::from([0x22; 32])
    }

    fn ensure_hotkey_exists(hotkey: &AccountId) {
        pallet_subtensor::Owner::<Runtime>::insert(hotkey, hotkey.clone());
    }

    fn stake_for(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> u64 {
        pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey, coldkey, netuid,
        )
        .into()
    }

    fn total_coldkey_stake_on_subnet(coldkey: &AccountId, netuid: NetUid) -> u64 {
        pallet_subtensor::Pallet::<Runtime>::get_total_stake_for_coldkey_on_subnet(coldkey, netuid)
            .into()
    }

    fn add_stake_v1(caller: H160, hotkey: &AccountId, netuid: u16, amount_rao: u64) {
        ensure_hotkey_exists(hotkey);
        fund_account(&StakingPrecompile::<Runtime>::account_id(), amount_rao);

        let result = execute_precompile(
            &precompiles::<StakingPrecompile<Runtime>>(),
            addr_from_index(StakingPrecompile::<Runtime>::INDEX),
            caller,
            encode_with_selector(
                selector_u32("addStake(bytes32,uint256)"),
                (H256::from_slice(hotkey.as_ref()), U256::from(netuid)),
            ),
            substrate_to_evm(amount_rao),
        )
        .expect("staking v1 add stake should route to the precompile");

        assert!(result.is_ok());
    }

    fn add_stake_v2(caller: H160, hotkey: &AccountId, netuid: u16, amount_rao: u64) {
        ensure_hotkey_exists(hotkey);
        precompiles::<StakingPrecompileV2<Runtime>>()
            .prepare_test(
                caller,
                addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                encode_with_selector(
                    selector_u32("addStake(bytes32,uint256,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        U256::from(amount_rao),
                        U256::from(netuid),
                    ),
                ),
            )
            .execute_returns(());
    }

    fn assert_proxy_effects(caller: H160, netuid: NetUid) {
        let caller_account = mapped_account(caller);
        let hotkey = hotkey();
        let delegate = delegate();

        ensure_hotkey_exists(&hotkey);

        let proxies = pallet_subtensor_proxy::Proxies::<Runtime>::get(&caller_account).0;
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].delegate, delegate);

        let stake_before = stake_for(&hotkey, &caller_account, netuid);
        let proxied_call = RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake {
            hotkey: hotkey.clone(),
            netuid,
            amount_staked: PROXY_STAKE_RAO.into(),
        });
        let proxy_result = Proxy::proxy(
            RuntimeOrigin::signed(delegate.clone()),
            caller_account.clone().into(),
            Some(ProxyType::Staking),
            Box::new(proxied_call),
        );
        assert!(proxy_result.is_ok());

        let stake_after = stake_for(&hotkey, &caller_account, netuid);
        assert!(stake_after > stake_before);
    }

    fn setup_approval_state() -> (NetUid, H160, H160, AccountId, AccountId, AccountId) {
        let netuid = setup_staking_subnet();
        let source = addr_from_index(0x2001);
        let spender = addr_from_index(0x2002);
        let source_account = mapped_account(source);
        let spender_account = mapped_account(spender);
        let hotkey = hotkey();

        fund_account(&source_account, COLDKEY_BALANCE);
        add_stake_v2(source, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

        (
            netuid,
            source,
            spender,
            source_account,
            spender_account,
            hotkey,
        )
    }

    fn assert_allowance(source: H160, spender: H160, caller: H160, expected: U256) {
        assert_static_call(
            &precompiles::<StakingPrecompileV2<Runtime>>(),
            caller,
            addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
            encode_with_selector(
                selector_u32("allowance(address,address,uint256)"),
                (
                    precompile_utils::solidity::codec::Address(source),
                    precompile_utils::solidity::codec::Address(spender),
                    U256::from(TEST_NETUID_U16),
                ),
            ),
            expected,
        );
    }

    #[test]
    fn staking_precompile_v2_pages_non_zero_stake_positions_for_requested_subnet() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            setup_staking_subnet_id(SECOND_NETUID_U16);
            let caller = addr_from_index(0x1101);
            let coldkey = mapped_account(caller);
            let hotkey_a = AccountId::from([0x31; 32]);
            let hotkey_b = AccountId::from([0x32; 32]);
            let hotkey_c = AccountId::from([0x33; 32]);

            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey_a, TEST_NETUID_U16, INITIAL_STAKE_RAO);
            add_stake_v2(caller, &hotkey_b, SECOND_NETUID_U16, INITIAL_STAKE_RAO);
            add_stake_v2(caller, &hotkey_c, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let hotkeys = pallet_subtensor::Pallet::<Runtime>::get_all_staked_hotkeys(&coldkey);
            let expected_first: Vec<(H256, U256)> = hotkeys
                .iter()
                .take(2)
                .filter_map(|hotkey| {
                    let stake = stake_for(hotkey, &coldkey, netuid);
                    (stake > 0).then(|| (H256::from_slice(hotkey.as_ref()), U256::from(stake)))
                })
                .collect();
            let expected_second: Vec<(H256, U256)> = hotkeys
                .iter()
                .skip(2)
                .filter_map(|hotkey| {
                    let stake = stake_for(hotkey, &coldkey, netuid);
                    (stake > 0).then(|| (H256::from_slice(hotkey.as_ref()), U256::from(stake)))
                })
                .collect();
            assert_eq!(expected_first.len(), 1);
            assert_eq!(expected_second.len(), 1);

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::zero(),
                            U256::from(2),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(2))
                .execute_returns_raw(encode_return_value((expected_first, U256::from(2))));

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::from(2),
                            U256::from(2),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(1))
                .execute_returns_raw(encode_return_value((expected_second, U256::zero())));
        });
    }

    #[test]
    fn staking_precompile_v2_returns_empty_stake_info_for_unknown_coldkey() {
        new_test_ext().execute_with(|| {
            setup_staking_subnet();
            let caller = addr_from_index(0x1102);
            let coldkey = AccountId::from([0x41; 32]);

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::zero(),
                            U256::from(MAX_STAKE_INFO_PAGE_SIZE),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(0))
                .execute_returns_raw(encode_return_value((
                    Vec::<(H256, U256)>::new(),
                    U256::zero(),
                )));
        });
    }

    #[test]
    fn staking_precompile_v2_rejects_out_of_range_stake_info_netuid() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x1103);
            let coldkey = AccountId::from([0x42; 32]);
            let invalid_netuid = U256::from(u32::from(u16::MAX) + 1);

            let result = execute_precompile(
                &precompiles::<StakingPrecompileV2<Runtime>>(),
                addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                caller,
                encode_with_selector(
                    selector_u32(
                        "getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)",
                    ),
                    (
                        H256::from_slice(coldkey.as_ref()),
                        invalid_netuid,
                        U256::zero(),
                        U256::one(),
                    ),
                ),
                U256::zero(),
            )
            .expect("staking V2 call routes to the precompile");

            assert!(result.is_err());
        });
    }

    #[test]
    fn staking_precompile_v2_bounds_historical_hotkey_scan_and_precharges_page() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1105);
            let coldkey = mapped_account(caller);
            let historical_hotkeys: Vec<AccountId> = (0..=MAX_STAKE_INFO_PAGE_SIZE)
                .map(|index| {
                    let mut account = [0u8; 32];
                    account[..4].copy_from_slice(&index.to_le_bytes());
                    AccountId::from(account)
                })
                .collect();
            let active_hotkey = historical_hotkeys
                .last()
                .expect("historical hotkeys is non-empty")
                .clone();

            pallet_subtensor::StakingHotkeys::<Runtime>::insert(
                &coldkey,
                historical_hotkeys.clone(),
            );
            pallet_subtensor::Pallet::<Runtime>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &active_hotkey,
                &coldkey,
                netuid,
                AlphaBalance::from(INITIAL_STAKE_RAO),
            );
            let active_stake = stake_for(&active_hotkey, &coldkey, netuid);
            assert!(active_stake > 0);

            let first_page_call = encode_with_selector(
                selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)"),
                (
                    H256::from_slice(coldkey.as_ref()),
                    U256::from(TEST_NETUID_U16),
                    U256::zero(),
                    U256::from(MAX_STAKE_INFO_PAGE_SIZE),
                ),
            );

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    first_page_call,
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(MAX_STAKE_INFO_PAGE_SIZE as usize))
                .execute_returns_raw(encode_return_value((
                    Vec::<(H256, U256)>::new(),
                    U256::from(MAX_STAKE_INFO_PAGE_SIZE),
                )));

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::from(MAX_STAKE_INFO_PAGE_SIZE),
                            U256::from(MAX_STAKE_INFO_PAGE_SIZE),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(1))
                .execute_returns_raw(encode_return_value((
                    vec![(
                        H256::from_slice(active_hotkey.as_ref()),
                        U256::from(active_stake),
                    )],
                    U256::zero(),
                )));
        });
    }

    #[test]
    fn staking_precompile_v2_rejects_invalid_stake_info_page_limits() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x1106);
            let coldkey = AccountId::from([0x43; 32]);

            for limit in [U256::zero(), U256::from(MAX_STAKE_INFO_PAGE_SIZE + 1)] {
                let result = execute_precompile(
                    &precompiles::<StakingPrecompileV2<Runtime>>(),
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    caller,
                    encode_with_selector(
                        selector_u32(
                            "getStakeInfoForColdkeyAndNetuid(bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::zero(),
                            limit,
                        ),
                    ),
                    U256::zero(),
                )
                .expect("staking V2 call routes to the precompile");

                assert!(result.is_err());
            }
        });
    }

    #[test]
    fn staking_precompile_v2_returns_current_stake_operation_threshold() {
        new_test_ext().execute_with(|| {
            let threshold = pallet_subtensor::DefaultMinStake::<Runtime>::get();

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    addr_from_index(0x1104),
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(selector_u32("getStakeOperationThreshold()"), ()),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns(U256::from(threshold.to_u64()));
        });
    }

    #[test]
    fn staking_precompile_v1_add_stake_and_reads_match_runtime_state() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1001);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);

            let stake_before = stake_for(&hotkey, &caller_account, netuid);
            add_stake_v1(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            assert!(stake_after > stake_before);

            assert_static_call(
                &precompiles::<StakingPrecompile<Runtime>>(),
                caller,
                addr_from_index(StakingPrecompile::<Runtime>::INDEX),
                encode_with_selector(
                    selector_u32("getStake(bytes32,bytes32,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        H256::from_slice(caller_account.as_ref()),
                        U256::from(TEST_NETUID_U16),
                    ),
                ),
                substrate_to_evm(stake_after),
            );
        });
    }

    #[test]
    fn staking_precompile_v2_add_stake_and_reads_match_runtime_state() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1002);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);

            let stake_before = stake_for(&hotkey, &caller_account, netuid);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            let total_coldkey_stake = total_coldkey_stake_on_subnet(&caller_account, netuid);

            assert!(stake_after > stake_before);
            assert!(total_coldkey_stake >= stake_after);

            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getStake(bytes32,bytes32,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        H256::from_slice(caller_account.as_ref()),
                        U256::from(TEST_NETUID_U16),
                    ),
                ),
                U256::from(stake_after),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getTotalColdkeyStakeOnSubnet(bytes32,uint256)"),
                    (
                        H256::from_slice(caller_account.as_ref()),
                        U256::from(TEST_NETUID_U16),
                    ),
                ),
                U256::from(total_coldkey_stake),
            );
        });
    }

    #[test]
    fn staking_precompile_v1_rejects_missing_subnet() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x1003);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);
            fund_account(
                &StakingPrecompile::<Runtime>::account_id(),
                INITIAL_STAKE_RAO,
            );

            let rejected = execute_precompile(
                &precompiles::<StakingPrecompile<Runtime>>(),
                addr_from_index(StakingPrecompile::<Runtime>::INDEX),
                caller,
                encode_with_selector(
                    selector_u32("addStake(bytes32,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        U256::from(INVALID_NETUID_U16),
                    ),
                ),
                substrate_to_evm(INITIAL_STAKE_RAO),
            )
            .expect("staking v1 add stake should route to the precompile");

            assert!(rejected.is_err());
            assert_eq!(
                stake_for(&hotkey, &caller_account, NetUid::from(INVALID_NETUID_U16)),
                0,
            );
        });
    }

    #[test]
    fn staking_precompile_v2_rejects_missing_subnet() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x1004);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);

            let rejected = execute_precompile(
                &precompiles::<StakingPrecompileV2<Runtime>>(),
                addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                caller,
                encode_with_selector(
                    selector_u32("addStake(bytes32,uint256,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        U256::from(INITIAL_STAKE_RAO),
                        U256::from(INVALID_NETUID_U16),
                    ),
                ),
                U256::zero(),
            )
            .expect("staking v2 add stake should route to the precompile");

            assert!(rejected.is_err());
            assert_eq!(
                stake_for(&hotkey, &caller_account, NetUid::from(INVALID_NETUID_U16)),
                0,
            );
        });
    }

    #[test]
    fn staking_precompile_v1_remove_stake_reduces_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1005);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);
            add_stake_v1(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let precompiles = precompiles::<StakingPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompile::<Runtime>::INDEX);
            let stake_before = stake_for(&hotkey, &caller_account, netuid);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeStake(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            substrate_to_evm(REMOVE_STAKE_RAO),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            assert_eq!(stake_after, stake_before - REMOVE_STAKE_RAO);
        });
    }

    #[test]
    fn staking_precompile_v2_remove_stake_reduces_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1006);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);
            let stake_before = stake_for(&hotkey, &caller_account, netuid);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeStake(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(REMOVE_STAKE_RAO),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            assert_eq!(stake_after, stake_before - REMOVE_STAKE_RAO);
        });
    }

    #[test]
    fn staking_precompile_v2_add_stake_limit_increases_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x4001);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);

            let stake_before = stake_for(&hotkey, &caller_account, netuid);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("addStakeLimit(bytes32,uint256,uint256,bool,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(INITIAL_STAKE_RAO),
                            U256::from(1_000_000_000_000_u64),
                            true,
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            assert!(stake_for(&hotkey, &caller_account, netuid) > stake_before);
        });
    }

    #[test]
    fn staking_precompile_v2_remove_stake_limit_decreases_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x4002);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("addStakeLimit(bytes32,uint256,uint256,bool,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(INITIAL_STAKE_RAO),
                            U256::from(1_000_000_000_000_u64),
                            true,
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let stake_before = stake_for(&hotkey, &caller_account, netuid);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeStakeLimit(bytes32,uint256,uint256,bool,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(REMOVE_STAKE_RAO),
                            U256::from(1_000_000_000_u64),
                            true,
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            assert!(stake_for(&hotkey, &caller_account, netuid) < stake_before);
        });
    }

    #[test]
    fn staking_precompile_v2_remove_stake_full_limit_clears_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x4003);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("addStakeLimit(bytes32,uint256,uint256,bool,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(INITIAL_STAKE_RAO),
                            U256::from(1_000_000_000_000_u64),
                            true,
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            assert!(stake_for(&hotkey, &caller_account, netuid) > 0);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeStakeFullLimit(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::from(90_000_000_u64),
                        ),
                    ),
                )
                .execute_returns(());

            assert_eq!(stake_for(&hotkey, &caller_account, netuid), 0);
        });
    }

    #[test]
    fn staking_precompile_v2_remove_stake_full_clears_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x4004);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("addStakeLimit(bytes32,uint256,uint256,bool,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(INITIAL_STAKE_RAO),
                            U256::from(1_000_000_000_000_u64),
                            true,
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            assert!(stake_for(&hotkey, &caller_account, netuid) > 0);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeStakeFull(bytes32,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            assert_eq!(stake_for(&hotkey, &caller_account, netuid), 0);
        });
    }

    #[test]
    fn staking_precompile_v2_getters_match_runtime_state() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x4005);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let stake = stake_for(&hotkey, &caller_account, netuid);
            assert!(stake > 0);
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getStake(bytes32,bytes32,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        H256::from_slice(caller_account.as_ref()),
                        U256::from(TEST_NETUID_U16),
                    ),
                ),
                U256::from(stake),
            );
            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(
                    selector_u32("getTotalAlphaStaked(bytes32,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        U256::from(TEST_NETUID_U16),
                    ),
                ),
                U256::from(
                    pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_on_subnet(
                        &hotkey, netuid,
                    )
                    .to_u64(),
                ),
            );

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getAlphaStakedValidators(bytes32,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .execute_returns_raw(encode_return_value(vec![H256::from_slice(
                    caller_account.as_ref(),
                )]));

            assert_static_call(
                &precompiles,
                caller,
                precompile_addr,
                encode_with_selector(selector_u32("getNominatorMinRequiredStake()"), ()),
                U256::from(pallet_subtensor::Pallet::<Runtime>::get_nominator_min_required_stake()),
            );
        });
    }

    #[test]
    fn staking_precompile_v1_adds_and_removes_proxy() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1007);
            let caller_account = mapped_account(caller);
            let delegate = delegate();
            let precompiles = precompiles::<StakingPrecompile<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompile::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            fund_account(&delegate, COLDKEY_BALANCE);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("addProxy(bytes32)"),
                        (H256::from_slice(delegate.as_ref()),),
                    ),
                )
                .execute_returns(());
            assert_proxy_effects(caller, netuid);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeProxy(bytes32)"),
                        (H256::from_slice(delegate.as_ref()),),
                    ),
                )
                .execute_returns(());

            let proxies = pallet_subtensor_proxy::Proxies::<Runtime>::get(&caller_account).0;
            assert!(proxies.is_empty());
        });
    }

    #[test]
    fn staking_precompile_v2_adds_and_removes_proxy() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1008);
            let caller_account = mapped_account(caller);
            let delegate = delegate();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            fund_account(&delegate, COLDKEY_BALANCE);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("addProxy(bytes32)"),
                        (H256::from_slice(delegate.as_ref()),),
                    ),
                )
                .execute_returns(());
            assert_proxy_effects(caller, netuid);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("removeProxy(bytes32)"),
                        (H256::from_slice(delegate.as_ref()),),
                    ),
                )
                .execute_returns(());

            let proxies = pallet_subtensor_proxy::Proxies::<Runtime>::get(&caller_account).0;
            assert!(proxies.is_empty());
        });
    }

    #[test]
    fn staking_precompile_v2_transfer_stake_from_requires_allowance() {
        new_test_ext().execute_with(|| {
            let (_, source, spender, _, _, hotkey) = setup_approval_state();
            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    spender,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "transferStakeFrom(address,address,bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            precompile_utils::solidity::codec::Address(source),
                            precompile_utils::solidity::codec::Address(spender),
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::from(TEST_NETUID_U16),
                            U256::from(1_u64),
                        ),
                    ),
                )
                .execute_reverts(|output| output == b"trying to spend more than allowed");
        });
    }

    #[test]
    fn staking_precompile_v2_transfer_stake_from_consumes_allowance_and_moves_stake() {
        new_test_ext().execute_with(|| {
            let (netuid, source, spender, source_account, spender_account, hotkey) =
                setup_approval_state();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            precompiles
                .prepare_test(
                    source,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("approve(address,uint256,uint256)"),
                        (
                            precompile_utils::solidity::codec::Address(spender),
                            U256::from(TEST_NETUID_U16),
                            U256::from(APPROVED_ALLOWANCE_RAO),
                        ),
                    ),
                )
                .execute_returns(());

            let source_stake_before = stake_for(&hotkey, &source_account, netuid);
            let spender_stake_before = stake_for(&hotkey, &spender_account, netuid);

            precompiles
                .prepare_test(
                    spender,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32(
                            "transferStakeFrom(address,address,bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            precompile_utils::solidity::codec::Address(source),
                            precompile_utils::solidity::codec::Address(spender),
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::from(TEST_NETUID_U16),
                            U256::from(TRANSFERRED_ALLOWANCE_RAO),
                        ),
                    ),
                )
                .execute_returns(());

            assert_allowance(
                source,
                spender,
                source,
                U256::from(APPROVED_ALLOWANCE_RAO - TRANSFERRED_ALLOWANCE_RAO),
            );
            assert_eq!(
                stake_for(&hotkey, &source_account, netuid),
                source_stake_before - TRANSFERRED_ALLOWANCE_RAO,
            );
            assert_eq!(
                stake_for(&hotkey, &spender_account, netuid),
                spender_stake_before + TRANSFERRED_ALLOWANCE_RAO,
            );
        });
    }

    #[test]
    fn staking_precompile_v2_transfer_stake_from_rejects_amount_above_allowance() {
        new_test_ext().execute_with(|| {
            let (_, source, spender, _, _, hotkey) = setup_approval_state();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            precompiles
                .prepare_test(
                    source,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("approve(address,uint256,uint256)"),
                        (
                            precompile_utils::solidity::codec::Address(spender),
                            U256::from(TEST_NETUID_U16),
                            U256::from(TRANSFERRED_ALLOWANCE_RAO),
                        ),
                    ),
                )
                .execute_returns(());

            precompiles
                .prepare_test(
                    spender,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32(
                            "transferStakeFrom(address,address,bytes32,uint256,uint256,uint256)",
                        ),
                        (
                            precompile_utils::solidity::codec::Address(source),
                            precompile_utils::solidity::codec::Address(spender),
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            U256::from(TEST_NETUID_U16),
                            U256::from(TRANSFERRED_ALLOWANCE_RAO + 1),
                        ),
                    ),
                )
                .execute_reverts(|output| output == b"trying to spend more than allowed");
        });
    }

    #[test]
    fn staking_precompile_v2_approval_functions_update_allowance() {
        new_test_ext().execute_with(|| {
            let (_, source, spender, _, _, _) = setup_approval_state();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            assert_allowance(source, spender, source, U256::zero());

            precompiles
                .prepare_test(
                    source,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("approve(address,uint256,uint256)"),
                        (
                            precompile_utils::solidity::codec::Address(spender),
                            U256::from(TEST_NETUID_U16),
                            U256::from(APPROVED_ALLOWANCE_RAO),
                        ),
                    ),
                )
                .execute_returns(());
            assert_allowance(source, spender, source, U256::from(APPROVED_ALLOWANCE_RAO));

            precompiles
                .prepare_test(
                    source,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("increaseAllowance(address,uint256,uint256)"),
                        (
                            precompile_utils::solidity::codec::Address(spender),
                            U256::from(TEST_NETUID_U16),
                            U256::from(APPROVED_ALLOWANCE_RAO),
                        ),
                    ),
                )
                .execute_returns(());
            assert_allowance(
                source,
                spender,
                source,
                U256::from(APPROVED_ALLOWANCE_RAO * 2),
            );

            precompiles
                .prepare_test(
                    source,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("decreaseAllowance(address,uint256,uint256)"),
                        (
                            precompile_utils::solidity::codec::Address(spender),
                            U256::from(TEST_NETUID_U16),
                            U256::from(ALLOWANCE_DECREASE_RAO),
                        ),
                    ),
                )
                .execute_returns(());
            assert_allowance(
                source,
                spender,
                source,
                U256::from(APPROVED_ALLOWANCE_RAO * 2 - ALLOWANCE_DECREASE_RAO),
            );

            precompiles
                .prepare_test(
                    source,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("approve(address,uint256,uint256)"),
                        (
                            precompile_utils::solidity::codec::Address(spender),
                            U256::from(TEST_NETUID_U16),
                            U256::zero(),
                        ),
                    ),
                )
                .execute_returns(());
            assert_allowance(source, spender, source, U256::zero());
        });
    }

    #[test]
    fn staking_precompile_v2_burn_alpha_reduces_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x3001);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let burn_amount = 20_000_000_000_u64;
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, 50_000_000_000);

            let stake_before = stake_for(&hotkey, &caller_account, netuid);
            assert!(stake_before > 0);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("burnAlpha(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(burn_amount),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            assert_eq!(stake_after, stake_before - burn_amount);
        });
    }

    // cargo test --package subtensor-precompiles --lib -- staking::tests::staking_precompile_v2_burn_alpha_caps_to_available_stake --exact --nocapture
    #[test]
    fn staking_precompile_v2_burn_alpha_caps_to_available_stake() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x3002);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            let stake_before = stake_for(&hotkey, &caller_account, netuid);
            assert!(stake_before > 0);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("burnAlpha(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(stake_before + 10_000_000_000_u64),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            assert_eq!(stake_after, 0);
        });
    }

    #[test]
    fn staking_precompile_v2_burn_alpha_rejects_missing_subnet() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x3003);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();

            fund_account(&caller_account, COLDKEY_BALANCE);
            ensure_hotkey_exists(&hotkey);

            let rejected = execute_precompile(
                &precompiles::<StakingPrecompileV2<Runtime>>(),
                addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                caller,
                encode_with_selector(
                    selector_u32("burnAlpha(bytes32,uint256,uint256)"),
                    (
                        H256::from_slice(hotkey.as_ref()),
                        U256::from(10_000_000_000_u64),
                        U256::from(INVALID_NETUID_U16),
                    ),
                ),
                U256::zero(),
            )
            .expect("burnAlpha should route to the staking v2 precompile");

            assert!(rejected.is_err());
        });
    }

    #[test]
    fn staking_precompile_v2_burn_zero_alpha_is_noop() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x3004);
            let caller_account = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&caller_account, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, 10_000_000_000);

            let stake_before = stake_for(&hotkey, &caller_account, netuid);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("burnAlpha(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::zero(),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let stake_after = stake_for(&hotkey, &caller_account, netuid);
            assert_eq!(stake_after, stake_before);
        });
    }
}
