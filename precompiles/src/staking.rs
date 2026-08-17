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

use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::marker::PhantomData;
use frame_support::Blake2_128Concat;
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::pallet_prelude::{StorageDoubleMap, ValueQuery};
use frame_support::traits::{ConstU32, Get, IsSubType, StorageInstance};
use frame_system::RawOrigin;
use pallet_evm::{
    AddressMapping, BalanceConverter, EvmBalance, ExitError, PrecompileFailure, PrecompileHandle,
    SubstrateBalance,
};
use pallet_subtensor_proxy as pallet_proxy;
use precompile_utils::EvmResult;
use precompile_utils::prelude::{Address, BoundedVec, revert};
use sp_core::{H160, H256, U256};
use sp_runtime::{
    PerU16,
    traits::{AsSystemOriginSigner, Dispatchable, StaticLookup, UniqueSaturatedInto},
};
use sp_std::vec;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, ProxyType, TaoBalance, Token};

use crate::{PrecompileExt, PrecompileHandleExt};

// `get_stake_for_hotkey_and_coldkey_on_subnet` reads the transitional V1/V2
// share storage. In the V2 fallback case it performs two reads for the initial
// share lookup, then five more for the value, share, and denominator.
const STAKE_INFO_READS_PER_HOTKEY: u64 = 7;
// Conservative charge for decoding and validating each 32-byte hotkey.
const STAKE_INFO_INPUT_GAS_PER_HOTKEY: u64 = 64;
const MAX_STAKE_INFO_HOTKEYS: usize = 64;
const MAX_CONVICTION_HOTKEYS: usize = 64;
// Individual state reads the lock row, mode, owner hotkey, global rates, and current block.
const COLDKEY_LOCK_READS: u64 = 6;
// Aggregate state reads the owner hotkey, global rates, current block, and up to four buckets.
const HOTKEY_LOCK_READS: u64 = 8;
// Each hotkey-wide total visits every possible subnet. Besides the
// `NetworksAdded` entry, one active subnet can read TotalHotkeyAlpha plus the
// four values used by `current_alpha_price`.
const TOTAL_HOTKEY_STAKE_READS_PER_SUBNET: u64 = 5;
// For each raw Alpha/AlphaV2 position, the coldkey totals read the position
// once while accounting and once in the released helper. A matching position
// can then perform the conservative V2 stake lookup, swap simulation, and
// current-price reads.
const TOTAL_COLDKEY_POSITION_BASE_READS: u64 = 2;
const TOTAL_COLDKEY_MATCHED_POSITION_READS: u64 = STAKE_INFO_READS_PER_HOTKEY + 9 + 4;
// BasketRate + root stake + BasketClaimed + BasketShares + escrow holding and quote state.
// This is deliberately conservative so each bounded request pays for its per-hotkey work.
const ROOT_UNCLAIMED_READS_PER_HOTKEY: u64 = 20;

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
        + pallet_admin_utils::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_admin_utils::Call<R>>
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
        + pallet_admin_utils::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_admin_utils::Call<R>>
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
        let coldkey = R::AccountId::from(coldkey.0);
        record_total_coldkey_stake_reads::<R>(handle, &coldkey, None)?;
        let stake = pallet_subtensor::Pallet::<R>::get_total_stake_for_coldkey(&coldkey);

        Ok(stake.to_u64().into())
    }

    #[precompile::public("getTotalHotkeyStake(bytes32)")]
    #[precompile::view]
    fn get_total_hotkey_stake(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<U256> {
        record_total_hotkey_stake_reads::<R>(handle)?;
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
        // Worst-case V2 fallback reads for the alpha share pool.
        handle.record_db_reads::<R>(STAKE_INFO_READS_PER_HOTKEY)?;
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

    #[precompile::public("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])")]
    #[precompile::view]
    fn get_stake_info_for_coldkey_and_netuid(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: U256,
        hotkeys: BoundedVec<H256, ConstU32<{ MAX_STAKE_INFO_HOTKEYS as u32 }>>,
    ) -> EvmResult<Vec<(H256, U256)>> {
        let hotkey_count: u64 = hotkeys.len().unique_saturated_into();
        handle.record_cost(hotkey_count.saturating_mul(STAKE_INFO_INPUT_GAS_PER_HOTKEY))?;
        let hotkeys: Vec<H256> = hotkeys.into();

        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);

        let mut seen = BTreeSet::new();
        for hotkey in &hotkeys {
            if !seen.insert(hotkey) {
                return Err(revert("duplicate stake info hotkey"));
            }
        }

        // Charge the conservative V2 fallback cost for the complete bounded
        // batch before performing any stake reads.
        handle.record_db_reads::<R>(hotkey_count.saturating_mul(STAKE_INFO_READS_PER_HOTKEY))?;

        Ok(hotkeys
            .into_iter()
            .filter_map(|hotkey| {
                let hotkey_account = R::AccountId::from(hotkey.0);
                let stake =
                    pallet_subtensor::Pallet::<R>::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &hotkey_account,
                        &coldkey,
                        netuid,
                    )
                    .to_u64();
                if stake == 0 {
                    return None;
                }

                Some((hotkey, stake.into()))
            })
            .collect())
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

    #[precompile::public("getDefaultMinStake()")]
    #[precompile::view]
    fn get_default_min_stake(handle: &mut impl PrecompileHandle) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::DefaultMinStake::<R>::get()
            .to_u64()
            .into())
    }

    /// Lock existing subnet alpha and begin building conviction for `hotkey`.
    #[precompile::public("lockStake(bytes32,uint256,uint256)")]
    #[precompile::payable]
    fn lock_stake(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        amount_alpha: U256,
        netuid: U256,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::lock_stake {
            hotkey: R::AccountId::from(hotkey.0),
            netuid: NetUid::from(try_u16_from_u256(netuid)?),
            amount: try_u64_from_u256(amount_alpha)?.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    /// Re-point the caller's lock on a subnet to another hotkey.
    #[precompile::public("moveLock(bytes32,uint256)")]
    #[precompile::payable]
    fn move_lock(
        handle: &mut impl PrecompileHandle,
        destination_hotkey: H256,
        netuid: U256,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::move_lock {
            destination_hotkey: R::AccountId::from(destination_hotkey.0),
            netuid: NetUid::from(try_u16_from_u256(netuid)?),
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    /// Select perpetual or normally decaying behavior for the caller's lock.
    #[precompile::public("setPerpetualLock(uint256,bool)")]
    #[precompile::payable]
    fn set_perpetual_lock(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        enabled: bool,
    ) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::set_perpetual_lock {
            netuid: NetUid::from(try_u16_from_u256(netuid)?),
            enabled,
        };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    /// Set whether the caller rejects incoming stake that carries a lock.
    #[precompile::public("setRejectLockedAlpha(bool)")]
    #[precompile::payable]
    fn set_reject_locked_alpha(handle: &mut impl PrecompileHandle, enabled: bool) -> EvmResult<()> {
        let call = pallet_subtensor::Call::<R>::set_reject_locked_alpha { enabled };

        handle.try_dispatch_runtime_call::<R, _>(
            call,
            RawOrigin::Signed(handle.caller_account_id::<R>()),
        )
    }

    /// Return one coldkey's lock, rolled forward to the current block.
    ///
    /// Conviction is returned as exact unsigned Q64.64 bits.
    /// `exists` is false if the rolled lock has crossed the cleanup threshold,
    /// even when its stale storage row has not yet been removed.
    #[precompile::public("getColdkeyLock(bytes32,uint256)")]
    #[precompile::view]
    fn get_coldkey_lock(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: U256,
    ) -> EvmResult<(bool, H256, U256, u128, bool)> {
        handle.record_db_reads::<R>(COLDKEY_LOCK_READS)?;
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let perpetual = pallet_subtensor::DecayingLock::<R>::get(&coldkey, netuid) == Some(false);

        let Some((hotkey, lock)) =
            pallet_subtensor::Lock::<R>::iter_prefix((&coldkey, netuid)).next()
        else {
            return Ok((false, H256::zero(), U256::zero(), 0, perpetual));
        };

        let now = pallet_subtensor::Pallet::<R>::get_current_block_as_u64();
        let owner_lock = hotkey == pallet_subtensor::SubnetOwnerHotkey::<R>::get(netuid);
        let (lock, _) = pallet_subtensor::staking::lock::ConvictionModel::roll_forward_lock(
            lock,
            now,
            pallet_subtensor::UnlockRate::<R>::get(),
            pallet_subtensor::MaturityRate::<R>::get(),
            owner_lock,
            perpetual,
        );
        let exists = !lock.is_zero();
        let hotkey: [u8; 32] = hotkey.into();

        Ok((
            exists,
            hotkey.into(),
            lock.locked_mass.to_u64().into(),
            lock.conviction.to_bits(),
            perpetual,
        ))
    }

    /// Return the rolled aggregate lock and conviction for a hotkey.
    ///
    /// This combines perpetual and decaying general buckets and, when the
    /// hotkey is the subnet owner, both owner buckets.
    #[precompile::public("getHotkeyLock(bytes32,uint256)")]
    #[precompile::view]
    fn get_hotkey_lock(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: U256,
    ) -> EvmResult<(bool, U256, u128)> {
        handle.record_db_reads::<R>(HOTKEY_LOCK_READS)?;
        let hotkey = R::AccountId::from(hotkey.0);
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);
        let now = pallet_subtensor::Pallet::<R>::get_current_block_as_u64();
        let unlock_rate = pallet_subtensor::UnlockRate::<R>::get();
        let maturity_rate = pallet_subtensor::MaturityRate::<R>::get();
        let is_owner = hotkey == pallet_subtensor::SubnetOwnerHotkey::<R>::get(netuid);

        let mut locked = AlphaBalance::ZERO;
        let mut conviction = U64F64::from_bits(0);
        let mut add_bucket = |maybe_lock: Option<pallet_subtensor::staking::lock::LockState>,
                              owner_lock: bool,
                              perpetual_lock: bool| {
            if let Some(lock) = maybe_lock {
                let (lock, _) = pallet_subtensor::staking::lock::ConvictionModel::roll_forward_lock(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    owner_lock,
                    perpetual_lock,
                );
                locked = locked.saturating_add(lock.locked_mass);
                conviction = conviction.saturating_add(lock.conviction);
            }
        };

        add_bucket(
            pallet_subtensor::HotkeyLock::<R>::get(netuid, &hotkey),
            false,
            true,
        );
        add_bucket(
            pallet_subtensor::DecayingHotkeyLock::<R>::get(netuid, &hotkey),
            false,
            false,
        );
        if is_owner {
            add_bucket(pallet_subtensor::OwnerLock::<R>::get(netuid), true, true);
            add_bucket(
                pallet_subtensor::DecayingOwnerLock::<R>::get(netuid),
                true,
                false,
            );
        }

        let exists = locked > AlphaBalance::ZERO || conviction > U64F64::from_bits(0);
        Ok((exists, locked.to_u64().into(), conviction.to_bits()))
    }

    /// Return exact rolled conviction for up to 64 distinct candidate hotkeys.
    ///
    /// This bounded form avoids the runtime's unbounded all-hotkey scan. The
    /// returned values align one-for-one with `hotkeys`.
    #[precompile::public("getHotkeyConvictions(uint256,bytes32[])")]
    #[precompile::view]
    fn get_hotkey_convictions(
        handle: &mut impl PrecompileHandle,
        netuid: U256,
        hotkeys: BoundedVec<H256, ConstU32<{ MAX_CONVICTION_HOTKEYS as u32 }>>,
    ) -> EvmResult<Vec<u128>> {
        let hotkey_count: u64 = hotkeys.len().unique_saturated_into();
        handle.record_cost(hotkey_count.saturating_mul(STAKE_INFO_INPUT_GAS_PER_HOTKEY))?;
        let hotkeys: Vec<H256> = hotkeys.into();
        let netuid = NetUid::from(try_u16_from_u256(netuid)?);

        let mut seen = BTreeSet::new();
        for hotkey in &hotkeys {
            if !seen.insert(hotkey) {
                return Err(revert("duplicate conviction hotkey"));
            }
        }

        handle.record_db_reads::<R>(hotkey_count.saturating_mul(HOTKEY_LOCK_READS))?;

        Ok(hotkeys
            .into_iter()
            .map(|hotkey| {
                let hotkey = R::AccountId::from(hotkey.0);
                pallet_subtensor::Pallet::<R>::hotkey_conviction(&hotkey, netuid).to_bits()
            })
            .collect())
    }

    /// Return the global lock decay and conviction maturity timescales.
    #[precompile::public("getLockRates()")]
    #[precompile::view]
    fn get_lock_rates(handle: &mut impl PrecompileHandle) -> EvmResult<(u64, u64)> {
        handle.record_db_reads::<R>(2)?;
        Ok((
            pallet_subtensor::UnlockRate::<R>::get(),
            pallet_subtensor::MaturityRate::<R>::get(),
        ))
    }

    /// Return whether a coldkey rejects incoming locked alpha.
    #[precompile::public("getRejectLockedAlpha(bytes32)")]
    #[precompile::view]
    fn get_reject_locked_alpha(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        let coldkey = R::AccountId::from(coldkey.0);
        Ok(pallet_subtensor::Pallet::<R>::account_rejects_locked_alpha(
            &coldkey,
        ))
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
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = try_u16_from_u256(netuid)?;
        record_total_coldkey_stake_reads::<R>(handle, &coldkey, Some(netuid.into()))?;
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

    #[precompile::public("decreaseTake(bytes32,uint16)")]
    fn decrease_take(handle: &mut impl PrecompileHandle, hotkey: H256, take: u16) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::decrease_take {
                hotkey: hotkey.0.into(),
                take: PerU16::from_parts(take),
            },
        )
    }

    #[precompile::public("increaseTake(bytes32,uint16)")]
    fn increase_take(handle: &mut impl PrecompileHandle, hotkey: H256, take: u16) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::increase_take {
                hotkey: hotkey.0.into(),
                take: PerU16::from_parts(take),
            },
        )
    }

    #[precompile::public("setChildkeyTake(bytes32,uint16,uint16)")]
    fn set_childkey_take(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
        take: u16,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::set_childkey_take {
                hotkey: hotkey.0.into(),
                netuid: netuid.into(),
                take: PerU16::from_parts(take),
            },
        )
    }

    #[precompile::public("unstakeAll(bytes32)")]
    fn unstake_all(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::unstake_all {
                hotkey: hotkey.0.into(),
            },
        )
    }

    #[precompile::public("unstakeAllAlpha(bytes32)")]
    fn unstake_all_alpha(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::unstake_all_alpha {
                hotkey: hotkey.0.into(),
            },
        )
    }

    #[precompile::public("swapStake(bytes32,uint16,uint16,uint64)")]
    fn swap_stake(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        origin_netuid: u16,
        destination_netuid: u16,
        alpha_amount: u64,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::swap_stake {
                hotkey: hotkey.0.into(),
                origin_netuid: origin_netuid.into(),
                destination_netuid: destination_netuid.into(),
                alpha_amount: AlphaBalance::from(alpha_amount),
            },
        )
    }

    #[precompile::public("swapStakeLimit(bytes32,uint16,uint16,uint64,uint64,bool)")]
    fn swap_stake_limit(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        origin_netuid: u16,
        destination_netuid: u16,
        alpha_amount: u64,
        limit_price: u64,
        allow_partial: bool,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::swap_stake_limit {
                hotkey: hotkey.0.into(),
                origin_netuid: origin_netuid.into(),
                destination_netuid: destination_netuid.into(),
                alpha_amount: AlphaBalance::from(alpha_amount),
                limit_price: TaoBalance::from(limit_price),
                allow_partial,
            },
        )
    }

    #[precompile::public("moveStakeLimit(bytes32,bytes32,uint16,uint16,uint64,uint64,bool)")]
    fn move_stake_limit(
        handle: &mut impl PrecompileHandle,
        origin_hotkey: H256,
        destination_hotkey: H256,
        origin_netuid: u16,
        destination_netuid: u16,
        alpha_amount: u64,
        limit_price: u64,
        allow_partial: bool,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::move_stake_limit {
                origin_hotkey: origin_hotkey.0.into(),
                destination_hotkey: destination_hotkey.0.into(),
                origin_netuid: origin_netuid.into(),
                destination_netuid: destination_netuid.into(),
                alpha_amount: AlphaBalance::from(alpha_amount),
                limit_price: TaoBalance::from(limit_price),
                allow_partial,
            },
        )
    }

    #[precompile::public("recycleAlpha(bytes32,uint64,uint16)")]
    fn recycle_alpha(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        amount: u64,
        netuid: u16,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::recycle_alpha {
                hotkey: hotkey.0.into(),
                amount: AlphaBalance::from(amount),
                netuid: netuid.into(),
            },
        )
    }

    #[precompile::public("setColdkeyAutoStakeHotkey(uint16,bytes32)")]
    fn set_coldkey_auto_stake_hotkey(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::set_coldkey_auto_stake_hotkey {
                netuid: netuid.into(),
                hotkey: hotkey.0.into(),
            },
        )
    }

    #[precompile::public("claimRoot(uint16[])")]
    fn claim_root(
        handle: &mut impl PrecompileHandle,
        subnets: BoundedVec<u16, ConstU32<5>>,
    ) -> EvmResult<()> {
        let subnets = Vec::<u16>::from(subnets)
            .into_iter()
            .map(NetUid::from)
            .collect::<BTreeSet<_>>();
        dispatch_subtensor(handle, pallet_subtensor::Call::<R>::claim_root { subnets })
    }

    #[precompile::public("claimRootWithHotkey(bytes32)")]
    fn claim_root_with_hotkey(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::claim_root_with_hotkey {
                hotkey: hotkey.0.into(),
            },
        )
    }

    /// Returns the realizable TAO currently owed to `coldkey` by one validator basket.
    #[precompile::public("getUnclaimedRootTaoByHotkey(bytes32,bytes32)")]
    #[precompile::view]
    fn get_unclaimed_root_tao_by_hotkey(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        hotkey: H256,
    ) -> EvmResult<U256> {
        // A basket can contain at most one holding per subnet. Charge against the configured
        // subnet ceiling so the prefix scan remains fully paid without first scanning it.
        let subnet_limit = u64::from(pallet_subtensor::SubnetLimit::<R>::get()).saturating_add(1);
        handle.record_db_reads::<R>(
            1_u64.saturating_add(subnet_limit.saturating_mul(ROOT_UNCLAIMED_READS_PER_HOTKEY)),
        )?;

        let coldkey = R::AccountId::from(coldkey.0);
        let hotkey = R::AccountId::from(hotkey.0);
        let tao = pallet_subtensor::Pallet::<R>::get_basket_payout_tao(&hotkey, &coldkey);
        tao_to_evm::<R>(tao)
    }

    /// Returns the realizable TAO currently owed to `coldkey` from the supplied validator
    /// baskets' holdings on `netuid`. Supply at most 64 distinct hotkeys per call.
    #[precompile::public("getUnclaimedRootTaoBySubnet(bytes32,uint16,bytes32[])")]
    #[precompile::view]
    fn get_unclaimed_root_tao_by_subnet(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: u16,
        hotkeys: BoundedVec<H256, ConstU32<64>>,
    ) -> EvmResult<U256> {
        let hotkeys = Vec::<H256>::from(hotkeys);
        handle.record_db_reads::<R>(
            u64::try_from(hotkeys.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(ROOT_UNCLAIMED_READS_PER_HOTKEY),
        )?;
        let coldkey = R::AccountId::from(coldkey.0);
        let netuid = NetUid::from(netuid);
        let mut seen = BTreeSet::new();
        let mut tao = 0_u64;
        for hotkey in hotkeys {
            if !seen.insert(hotkey) {
                return Err(revert("duplicate unclaimed root hotkey"));
            }
            let hotkey = R::AccountId::from(hotkey.0);
            tao = tao.saturating_add(pallet_subtensor::Pallet::<R>::get_basket_subnet_payout_tao(
                &hotkey, &coldkey, netuid,
            ));
        }

        tao_to_evm::<R>(tao)
    }

    #[precompile::public("setRootClaimThreshold(uint16,uint64)")]
    fn set_root_claim_threshold(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        new_value: u64,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::sudo_set_root_claim_threshold {
                netuid: netuid.into(),
                new_value,
            },
        )
    }

    #[precompile::public("addStakeBurn(bytes32,uint16,uint64,bool,uint64)")]
    fn add_stake_burn(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
        amount: u64,
        has_limit: bool,
        limit: u64,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::add_stake_burn {
                hotkey: hotkey.0.into(),
                netuid: netuid.into(),
                amount: TaoBalance::from(amount),
                limit: has_limit.then_some(TaoBalance::from(limit)),
            },
        )
    }

    #[precompile::public("setAutoParentDelegationEnabled(bytes32,bool)")]
    fn set_auto_parent_delegation_enabled(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        enabled: bool,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::set_auto_parent_delegation_enabled {
                hotkey: hotkey.0.into(),
                enabled,
            },
        )
    }

    #[precompile::public("transferStakeAndHotkey(bytes32,bytes32,bytes32,uint16,uint16,uint64)")]
    fn transfer_stake_and_hotkey(
        handle: &mut impl PrecompileHandle,
        destination_coldkey: H256,
        origin_hotkey: H256,
        destination_hotkey: H256,
        origin_netuid: u16,
        destination_netuid: u16,
        alpha_amount: u64,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::transfer_stake_and_hotkey {
                destination_coldkey: destination_coldkey.0.into(),
                origin_hotkey: origin_hotkey.0.into(),
                destination_hotkey: destination_hotkey.0.into(),
                origin_netuid: origin_netuid.into(),
                destination_netuid: destination_netuid.into(),
                alpha_amount: AlphaBalance::from(alpha_amount),
            },
        )
    }

    #[precompile::public("addCollateral(uint16,bytes32,uint64,uint64)")]
    fn add_collateral(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
        alpha: u64,
        limit_price: u64,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::add_collateral {
                netuid: netuid.into(),
                hotkey: hotkey.0.into(),
                alpha: AlphaBalance::from(alpha),
                limit_price: TaoBalance::from(limit_price),
            },
        )
    }

    #[precompile::public("setMinCollateral(uint16,bytes32,uint64)")]
    fn set_min_collateral(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
        min_locked: u64,
    ) -> EvmResult<()> {
        dispatch_subtensor(
            handle,
            pallet_subtensor::Call::<R>::set_min_collateral {
                netuid: netuid.into(),
                hotkey: hotkey.0.into(),
                min_locked: AlphaBalance::from(min_locked),
            },
        )
    }

    #[precompile::public("setMinChildkeyTakePerSubnet(uint16,uint16)")]
    fn set_min_childkey_take_per_subnet(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        take: u16,
    ) -> EvmResult<()> {
        dispatch_staking_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_min_childkey_take_per_subnet {
                netuid: netuid.into(),
                take: PerU16::from_parts(take),
            },
        )
    }

    #[precompile::public("setCollateralLockShare(uint16,uint16)")]
    fn set_collateral_lock_share(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        lock_share: u16,
    ) -> EvmResult<()> {
        dispatch_staking_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_collateral_lock_share {
                netuid: netuid.into(),
                lock_share,
            },
        )
    }

    #[precompile::public("setCollateralDrainRatio(uint16,uint128)")]
    fn set_collateral_drain_ratio(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        raw_ratio: u128,
    ) -> EvmResult<()> {
        dispatch_staking_admin(
            handle,
            pallet_admin_utils::Call::<R>::sudo_set_collateral_drain_ratio {
                netuid: netuid.into(),
                drain_ratio: U64F64::from_bits(raw_ratio),
            },
        )
    }

    #[precompile::public("getDelegate(bytes32)")]
    #[precompile::view]
    fn get_delegate(handle: &mut impl PrecompileHandle, hotkey: H256) -> EvmResult<(bool, u16)> {
        handle.record_db_reads::<R>(1)?;
        let hotkey = R::AccountId::from(hotkey.0);
        Ok(match pallet_subtensor::Delegates::<R>::try_get(hotkey) {
            Ok(take) => (true, take.deconstruct()),
            Err(()) => (false, 0),
        })
    }

    #[precompile::public("getChildkeyTake(bytes32,uint16)")]
    #[precompile::view]
    fn get_childkey_take(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
    ) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ChildkeyTake::<R>::get(
            R::AccountId::from(hotkey.0),
            NetUid::from(netuid),
        )
        .deconstruct())
    }

    #[precompile::public("getPendingChildKeys(bytes32,uint16)")]
    #[precompile::view]
    fn get_pending_child_keys(
        handle: &mut impl PrecompileHandle,
        parent: H256,
        netuid: u16,
    ) -> EvmResult<(Vec<(u64, H256)>, u64)> {
        handle.record_db_reads::<R>(1)?;
        let (children, cooldown_block) = pallet_subtensor::PendingChildKeys::<R>::get(
            NetUid::from(netuid),
            R::AccountId::from(parent.0),
        );
        Ok((
            children
                .into_iter()
                .map(|(proportion, child)| (proportion, account_to_h256(child)))
                .collect(),
            cooldown_block,
        ))
    }

    #[precompile::public("getChildKeys(bytes32,uint16)")]
    #[precompile::view]
    fn get_child_keys(
        handle: &mut impl PrecompileHandle,
        parent: H256,
        netuid: u16,
    ) -> EvmResult<Vec<(u64, H256)>> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ChildKeys::<R>::get(
            R::AccountId::from(parent.0),
            NetUid::from(netuid),
        )
        .into_iter()
        .map(|(proportion, child)| (proportion, account_to_h256(child)))
        .collect())
    }

    #[precompile::public("getParentKeys(bytes32,uint16)")]
    #[precompile::view]
    fn get_parent_keys(
        handle: &mut impl PrecompileHandle,
        child: H256,
        netuid: u16,
    ) -> EvmResult<Vec<(u64, H256)>> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::ParentKeys::<R>::get(
            R::AccountId::from(child.0),
            NetUid::from(netuid),
        )
        .into_iter()
        .map(|(proportion, parent)| (proportion, account_to_h256(parent)))
        .collect())
    }

    #[precompile::public("getPendingChildKeyCooldown()")]
    #[precompile::view]
    fn get_pending_childkey_cooldown(handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::PendingChildKeyCooldown::<R>::get())
    }

    #[precompile::public("getTakeLimits()")]
    #[precompile::view]
    fn get_take_limits(handle: &mut impl PrecompileHandle) -> EvmResult<(u16, u16, u16, u16)> {
        handle.record_db_reads::<R>(4)?;
        Ok((
            pallet_subtensor::MinDelegateTake::<R>::get().deconstruct(),
            pallet_subtensor::MaxDelegateTake::<R>::get().deconstruct(),
            pallet_subtensor::MinChildkeyTake::<R>::get().deconstruct(),
            pallet_subtensor::MaxChildkeyTake::<R>::get().deconstruct(),
        ))
    }

    #[precompile::public("getMinChildkeyTakePerSubnet(uint16)")]
    #[precompile::view]
    fn get_min_childkey_take_per_subnet(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<u16> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            pallet_subtensor::MinChildkeyTakePerSubnet::<R>::get(NetUid::from(netuid))
                .deconstruct(),
        )
    }

    #[precompile::public("getHotkeyOwner(bytes32)")]
    #[precompile::view]
    fn get_hotkey_owner(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
    ) -> EvmResult<(bool, H256)> {
        handle.record_db_reads::<R>(1)?;
        let hotkey = R::AccountId::from(hotkey.0);
        Ok(match pallet_subtensor::Owner::<R>::try_get(hotkey) {
            Ok(owner) => (true, account_to_h256(owner)),
            Err(()) => (false, H256::zero()),
        })
    }

    #[precompile::public("getOwnedHotkeys(bytes32)")]
    #[precompile::view]
    fn get_owned_hotkeys(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<Vec<H256>> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            pallet_subtensor::OwnedHotkeys::<R>::get(R::AccountId::from(coldkey.0))
                .into_iter()
                .map(account_to_h256)
                .collect(),
        )
    }

    #[precompile::public("getAutoStakeDestination(bytes32,uint16)")]
    #[precompile::view]
    fn get_auto_stake_destination(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: u16,
    ) -> EvmResult<(bool, H256)> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            match pallet_subtensor::AutoStakeDestination::<R>::get(
                R::AccountId::from(coldkey.0),
                NetUid::from(netuid),
            ) {
                Some(hotkey) => (true, account_to_h256(hotkey)),
                None => (false, H256::zero()),
            },
        )
    }

    #[precompile::public("getAutoStakeDestinationColdkeys(bytes32,uint16)")]
    #[precompile::view]
    fn get_auto_stake_destination_coldkeys(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
    ) -> EvmResult<Vec<H256>> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::AutoStakeDestinationColdkeys::<R>::get(
            R::AccountId::from(hotkey.0),
            NetUid::from(netuid),
        )
        .into_iter()
        .map(account_to_h256)
        .collect())
    }

    #[precompile::public("getHotkeySuccessor(bytes32,uint16)")]
    #[precompile::view]
    fn get_hotkey_successor(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
    ) -> EvmResult<(bool, H256)> {
        handle.record_db_reads::<R>(1)?;
        Ok(optional_account(
            pallet_subtensor::HotkeySuccessor::<R>::get(
                NetUid::from(netuid),
                R::AccountId::from(hotkey.0),
            ),
        ))
    }

    #[precompile::public("getHotkeyRoot(bytes32,uint16)")]
    #[precompile::view]
    fn get_hotkey_root(
        handle: &mut impl PrecompileHandle,
        hotkey: H256,
        netuid: u16,
    ) -> EvmResult<(bool, H256)> {
        handle.record_db_reads::<R>(1)?;
        Ok(optional_account(pallet_subtensor::HotkeyRoot::<R>::get(
            NetUid::from(netuid),
            R::AccountId::from(hotkey.0),
        )))
    }

    #[precompile::public("getColdkeySuccessor(bytes32)")]
    #[precompile::view]
    fn get_coldkey_successor(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<(bool, H256)> {
        handle.record_db_reads::<R>(1)?;
        Ok(optional_account(
            pallet_subtensor::ColdkeySuccessor::<R>::get(R::AccountId::from(coldkey.0)),
        ))
    }

    #[precompile::public("getColdkeyRoot(bytes32)")]
    #[precompile::view]
    fn get_coldkey_root(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<(bool, H256)> {
        handle.record_db_reads::<R>(1)?;
        Ok(optional_account(pallet_subtensor::ColdkeyRoot::<R>::get(
            R::AccountId::from(coldkey.0),
        )))
    }

    #[precompile::public("getColdkeySwapStatus(bytes32)")]
    #[precompile::view]
    fn get_coldkey_swap_status(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
    ) -> EvmResult<(bool, u64, H256, bool, u64)> {
        handle.record_db_reads::<R>(2)?;
        let coldkey = R::AccountId::from(coldkey.0);
        let announcement = pallet_subtensor::ColdkeySwapAnnouncements::<R>::get(&coldkey);
        let dispute = pallet_subtensor::ColdkeySwapDisputes::<R>::get(&coldkey);
        let (has_announcement, announcement_block, call_hash) = match announcement {
            Some((block, hash)) => (
                true,
                block.unique_saturated_into(),
                H256::from_slice(hash.as_ref()),
            ),
            None => (false, 0, H256::zero()),
        };
        Ok((
            has_announcement,
            announcement_block,
            call_hash,
            dispute.is_some(),
            dispute
                .map(UniqueSaturatedInto::unique_saturated_into)
                .unwrap_or(0),
        ))
    }

    #[precompile::public("getColdkeySwapDelays()")]
    #[precompile::view]
    fn get_coldkey_swap_delays(handle: &mut impl PrecompileHandle) -> EvmResult<(u64, u64)> {
        handle.record_db_reads::<R>(2)?;
        Ok((
            pallet_subtensor::ColdkeySwapAnnouncementDelay::<R>::get().unique_saturated_into(),
            pallet_subtensor::ColdkeySwapReannouncementDelay::<R>::get().unique_saturated_into(),
        ))
    }

    #[precompile::public("getLastHotkeySwapOnSubnet(bytes32,uint16)")]
    #[precompile::view]
    fn get_last_hotkey_swap_on_subnet(
        handle: &mut impl PrecompileHandle,
        coldkey: H256,
        netuid: u16,
    ) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_subtensor::LastHotkeySwapOnNetuid::<R>::get(
            NetUid::from(netuid),
            R::AccountId::from(coldkey.0),
        ))
    }

    #[precompile::public("getStakeAccounting()")]
    #[precompile::view]
    fn get_stake_accounting(handle: &mut impl PrecompileHandle) -> EvmResult<(u64, u64)> {
        handle.record_db_reads::<R>(2)?;
        Ok((
            pallet_subtensor::TotalIssuance::<R>::get().to_u64(),
            pallet_subtensor::TotalStake::<R>::get().to_u64(),
        ))
    }

    #[precompile::public("getMinerCollateral(uint16,bytes32,bytes32)")]
    #[precompile::view]
    fn get_miner_collateral(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        hotkey: H256,
        coldkey: H256,
    ) -> EvmResult<(bool, u64, u128, u64, u64)> {
        handle.record_db_reads::<R>(1)?;
        Ok(
            match pallet_subtensor::MinerCollateral::<R>::get((
                NetUid::from(netuid),
                R::AccountId::from(hotkey.0),
                R::AccountId::from(coldkey.0),
            )) {
                Some(state) => (
                    true,
                    state.locked.to_u64(),
                    state.drain_ratio.to_bits(),
                    state.min_locked.to_u64(),
                    state.earned.to_u64(),
                ),
                None => (false, 0, 0, 0, 0),
            },
        )
    }

    #[precompile::public("getColdkeyCollateral(uint16,bytes32)")]
    #[precompile::view]
    fn get_coldkey_collateral(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
        coldkey: H256,
    ) -> EvmResult<(u64, Vec<H256>)> {
        handle.record_db_reads::<R>(2)?;
        let coldkey = R::AccountId::from(coldkey.0);
        Ok((
            pallet_subtensor::ColdkeyMinerCollateral::<R>::get(NetUid::from(netuid), &coldkey)
                .to_u64(),
            pallet_subtensor::ColdkeyCollateralHotkeys::<R>::get(NetUid::from(netuid), coldkey)
                .into_iter()
                .map(account_to_h256)
                .collect(),
        ))
    }

    #[precompile::public("getCollateralConfig(uint16)")]
    #[precompile::view]
    fn get_collateral_config(
        handle: &mut impl PrecompileHandle,
        netuid: u16,
    ) -> EvmResult<(u16, u128)> {
        handle.record_db_reads::<R>(2)?;
        Ok((
            pallet_subtensor::CollateralLockShare::<R>::get(NetUid::from(netuid)),
            pallet_subtensor::CollateralDrainRatio::<R>::get(NetUid::from(netuid)).to_bits(),
        ))
    }
}

fn account_to_h256<AccountId: Into<[u8; 32]>>(account: AccountId) -> H256 {
    H256::from(account.into())
}

fn optional_account<AccountId: Into<[u8; 32]>>(account: Option<AccountId>) -> (bool, H256) {
    account
        .map(|account| (true, account_to_h256(account)))
        .unwrap_or((false, H256::zero()))
}

fn dispatch_subtensor<R>(
    handle: &mut impl PrecompileHandle,
    call: pallet_subtensor::Call<R>,
) -> EvmResult<()>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_admin_utils::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
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
    let caller = handle.caller_account_id::<R>();
    handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
}

fn dispatch_staking_admin<R>(
    handle: &mut impl PrecompileHandle,
    call: pallet_admin_utils::Call<R>,
) -> EvmResult<()>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_admin_utils::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as frame_system::Config>::RuntimeCall: From<pallet_admin_utils::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
{
    let caller = handle.caller_account_id::<R>();
    handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(caller))
}

fn record_total_hotkey_stake_reads<R>(handle: &mut impl PrecompileHandle) -> EvmResult<()>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
{
    // Charge the SubnetLimit read plus the maximum permitted work before the
    // released helper scans NetworksAdded.
    handle.record_db_reads::<R>(1)?;
    let subnet_limit: u64 = pallet_subtensor::SubnetLimit::<R>::get().unique_saturated_into();
    handle.record_db_reads::<R>(subnet_limit.saturating_mul(TOTAL_HOTKEY_STAKE_READS_PER_SUBNET))
}

fn record_total_coldkey_stake_reads<R>(
    handle: &mut impl PrecompileHandle,
    coldkey: &R::AccountId,
    selected_netuid: Option<NetUid>,
) -> EvmResult<()>
where
    R: frame_system::Config + pallet_subtensor::Config + pallet_evm::Config,
    R::AccountId: Clone,
{
    // Read the bounded-by-state list once here and once in the released
    // aggregate helper.
    handle.record_db_reads::<R>(2)?;
    let hotkeys = pallet_subtensor::StakingHotkeys::<R>::get(coldkey);

    let mut raw_positions = 0u64;
    let mut matched_positions = 0u64;
    for hotkey in hotkeys {
        for (netuid, _) in pallet_subtensor::Alpha::<R>::iter_prefix((&hotkey, coldkey)) {
            raw_positions = raw_positions.saturating_add(1);
            if selected_netuid.is_none_or(|selected| selected == netuid) {
                matched_positions = matched_positions.saturating_add(1);
            }
        }
        for (netuid, _) in pallet_subtensor::AlphaV2::<R>::iter_prefix((&hotkey, coldkey)) {
            raw_positions = raw_positions.saturating_add(1);
            if selected_netuid.is_none_or(|selected| selected == netuid) {
                matched_positions = matched_positions.saturating_add(1);
            }
        }
    }

    handle.record_db_reads::<R>(
        raw_positions
            .saturating_mul(TOTAL_COLDKEY_POSITION_BASE_READS)
            .saturating_add(matched_positions.saturating_mul(TOTAL_COLDKEY_MATCHED_POSITION_READS)),
    )
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
        let coldkey = R::AccountId::from(coldkey.0);
        record_total_coldkey_stake_reads::<R>(handle, &coldkey, None)?;

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
        record_total_hotkey_stake_reads::<R>(handle)?;
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
        // Worst-case V2 fallback reads for the alpha share pool.
        handle.record_db_reads::<R>(STAKE_INFO_READS_PER_HOTKEY)?;
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

fn try_u64_from_u256(value: U256) -> Result<u64, PrecompileFailure> {
    value.try_into().map_err(|_| PrecompileFailure::Error {
        exit_status: ExitError::Other("the value is outside of u64 bounds".into()),
    })
}

fn tao_to_evm<R: pallet_evm::Config>(value: u64) -> EvmResult<U256> {
    <R as pallet_evm::Config>::BalanceConverter::into_evm_balance(SubstrateBalance::from(value))
        .map(EvmBalance::into_u256)
        .ok_or_else(|| ExitError::InvalidRange.into())
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
    use crate::mock::{
        AccountId, Proxy, Runtime, RuntimeCall, RuntimeOrigin, addr_from_index, assert_static_call,
        execute_precompile, fund_account, mapped_account, new_test_ext, precompiles, selector_u32,
        substrate_to_evm,
    };
    use crate::{PrecompileExt, Precompiles};
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

    fn stake_read_cost(hotkey_count: usize) -> u64 {
        let hotkey_count = u64::try_from(hotkey_count).expect("hotkey count fits in u64");
        let reads = hotkey_count.saturating_mul(STAKE_INFO_READS_PER_HOTKEY);
        RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(reads)
    }

    fn stake_info_validation_cost(hotkey_count: usize) -> u64 {
        let hotkey_count = u64::try_from(hotkey_count).expect("hotkey count fits in u64");
        STAKE_INFO_INPUT_GAS_PER_HOTKEY.saturating_mul(hotkey_count)
    }

    fn stake_info_cost(hotkey_count: usize) -> u64 {
        stake_info_validation_cost(hotkey_count).saturating_add(stake_read_cost(hotkey_count))
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

    fn setup_root_basket_position(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        owed_shares: u64,
        shares_total: u64,
        holding_alpha: u64,
    ) {
        ensure_hotkey_exists(hotkey);
        pallet_subtensor::BasketShares::<Runtime>::insert(hotkey, shares_total);
        pallet_subtensor::BasketClaimed::<Runtime>::insert(
            hotkey,
            coldkey,
            -i128::from(owed_shares),
        );
        let escrow = pallet_subtensor::Pallet::<Runtime>::get_beta_escrow_account_id();
        pallet_subtensor::Pallet::<Runtime>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            &escrow,
            netuid,
            AlphaBalance::from(holding_alpha),
        );
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

    #[test]
    fn staking_precompile_v2_move_stake_limit_dispatches_to_distinct_hotkey() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x20a1);
            let coldkey = mapped_account(caller);
            let origin_hotkey = AccountId::from([0x71; 32]);
            let destination_hotkey = AccountId::from([0x72; 32]);

            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &origin_hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);
            ensure_hotkey_exists(&destination_hotkey);
            let origin_before = stake_for(&origin_hotkey, &coldkey, netuid);
            let amount = origin_before / 2;

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32(
                            "moveStakeLimit(bytes32,bytes32,uint16,uint16,uint64,uint64,bool)",
                        ),
                        (
                            H256::from_slice(origin_hotkey.as_ref()),
                            H256::from_slice(destination_hotkey.as_ref()),
                            TEST_NETUID_U16,
                            TEST_NETUID_U16,
                            amount,
                            u64::MAX,
                            false,
                        ),
                    ),
                )
                .execute_returns(());

            assert_eq!(
                stake_for(&origin_hotkey, &coldkey, netuid),
                origin_before - amount
            );
            assert_eq!(stake_for(&destination_hotkey, &coldkey, netuid), amount);
        });
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
    fn staking_precompile_v2_returns_non_zero_stake_positions_for_requested_hotkeys() {
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

            let requested_hotkeys = vec![
                H256::from_slice(hotkey_a.as_ref()),
                H256::from_slice(hotkey_b.as_ref()),
                H256::from_slice(hotkey_c.as_ref()),
            ];
            let expected: Vec<(H256, U256)> = [&hotkey_a, &hotkey_b, &hotkey_c]
                .into_iter()
                .filter_map(|hotkey| {
                    let stake = stake_for(hotkey, &coldkey, netuid);
                    (stake > 0).then(|| (H256::from_slice(hotkey.as_ref()), U256::from(stake)))
                })
                .collect();
            assert_eq!(expected.len(), 2);

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            requested_hotkeys,
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(3))
                .execute_returns_raw(encode_return_value(expected));
        });
    }

    #[test]
    fn staking_precompile_v2_returns_empty_stake_info_for_unknown_coldkey() {
        new_test_ext().execute_with(|| {
            setup_staking_subnet();
            let caller = addr_from_index(0x1102);
            let coldkey = AccountId::from([0x41; 32]);
            let hotkeys = vec![H256::repeat_byte(0x51), H256::repeat_byte(0x52)];

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            hotkeys,
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(2))
                .execute_returns_raw(encode_return_value(Vec::<(H256, U256)>::new()));
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
                    selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                    (
                        H256::from_slice(coldkey.as_ref()),
                        invalid_netuid,
                        Vec::<H256>::new(),
                    ),
                ),
                U256::zero(),
            )
            .expect("staking V2 call routes to the precompile");

            assert!(result.is_err());
        });
    }

    #[test]
    fn staking_precompile_v2_only_reads_caller_supplied_hotkeys() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1105);
            let coldkey = mapped_account(caller);
            let historical_hotkeys: Vec<AccountId> = (0..=MAX_STAKE_INFO_HOTKEYS)
                .map(|index| {
                    let mut account = [0u8; 32];
                    let index = u64::try_from(index).expect("test index fits in u64");
                    account[..8].copy_from_slice(&index.to_le_bytes());
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

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            vec![H256::from_slice(active_hotkey.as_ref())],
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(1))
                .execute_returns_raw(encode_return_value(vec![(
                    H256::from_slice(active_hotkey.as_ref()),
                    U256::from(active_stake),
                )]));
        });
    }

    #[test]
    fn staking_precompile_v2_codec_rejects_more_than_64_requested_hotkeys() {
        new_test_ext().execute_with(|| {
            setup_staking_subnet();
            let caller = addr_from_index(0x1106);
            let coldkey = AccountId::from([0x43; 32]);
            let hotkeys: Vec<H256> = (0..=MAX_STAKE_INFO_HOTKEYS)
                .map(|index| {
                    let mut hotkey = [0u8; 32];
                    hotkey[..8].copy_from_slice(&index.to_le_bytes());
                    H256::from(hotkey)
                })
                .collect();

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            hotkeys[..MAX_STAKE_INFO_HOTKEYS].to_vec(),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_cost(MAX_STAKE_INFO_HOTKEYS))
                .execute_returns_raw(encode_return_value(Vec::<(H256, U256)>::new()));

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            hotkeys,
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(0)
                .execute_reverts(|output| output == b"hotkeys: Value is too large for length");
        });
    }

    #[test]
    fn staking_precompile_v2_rejects_duplicate_requested_hotkeys() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1107);
            let coldkey = mapped_account(caller);
            let hotkey = AccountId::from([0x54; 32]);
            let requested_hotkey = H256::from_slice(hotkey.as_ref());

            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);
            assert!(stake_for(&hotkey, &coldkey, netuid) > 0);

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getStakeInfoForColdkeyAndNetuid(bytes32,uint256,bytes32[])"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                            vec![requested_hotkey, H256::repeat_byte(0x55), requested_hotkey],
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_validation_cost(3))
                .execute_reverts(|output| output == b"duplicate stake info hotkey");
        });
    }

    #[test]
    fn staking_precompile_v2_returns_default_min_stake() {
        new_test_ext().execute_with(|| {
            let threshold = pallet_subtensor::DefaultMinStake::<Runtime>::get();

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    addr_from_index(0x1104),
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(selector_u32("getDefaultMinStake()"), ()),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns(U256::from(threshold.to_u64()));
        });
    }

    #[test]
    fn staking_precompile_v2_manages_and_reads_stake_lock_lifecycle() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1120);
            let coldkey = mapped_account(caller);
            let origin_hotkey = AccountId::from([0x61; 32]);
            let destination_hotkey = AccountId::from([0x62; 32]);
            let locked = 8_000_000_000_u64;
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &origin_hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);
            pallet_subtensor::Owner::<Runtime>::insert(&origin_hotkey, &coldkey);
            pallet_subtensor::Owner::<Runtime>::insert(&destination_hotkey, &coldkey);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("lockStake(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(origin_hotkey.as_ref()),
                            U256::from(locked),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setPerpetualLock(uint256,bool)"),
                        (U256::from(TEST_NETUID_U16), true),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::DecayingLock::<Runtime>::get(&coldkey, netuid),
                Some(false)
            );

            frame_system::Pallet::<Runtime>::set_block_number(1_000);
            let expected = pallet_subtensor::Pallet::<Runtime>::get_coldkey_lock(&coldkey, netuid)
                .expect("lock exists");
            assert_eq!(expected.locked_mass.to_u64(), locked);
            assert!(expected.conviction > U64F64::from_num(0));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getColdkeyLock(bytes32,uint256)"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(COLDKEY_LOCK_READS),
                )
                .execute_returns((
                    true,
                    H256::from_slice(origin_hotkey.as_ref()),
                    U256::from(expected.locked_mass.to_u64()),
                    expected.conviction.to_bits(),
                    true,
                ));

            let expected_hotkey_conviction =
                pallet_subtensor::Pallet::<Runtime>::hotkey_conviction(&origin_hotkey, netuid);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getHotkeyLock(bytes32,uint256)"),
                        (
                            H256::from_slice(origin_hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(HOTKEY_LOCK_READS),
                )
                .execute_returns((
                    true,
                    U256::from(expected.locked_mass.to_u64()),
                    expected_hotkey_conviction.to_bits(),
                ));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("moveLock(bytes32,uint256)"),
                        (
                            H256::from_slice(destination_hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            let moved =
                pallet_subtensor::Lock::<Runtime>::get((&coldkey, netuid, &destination_hotkey))
                    .expect("lock moved to destination");
            assert_eq!(moved.locked_mass, expected.locked_mass);
            assert_eq!(moved.conviction, expected.conviction);
            assert!(!pallet_subtensor::Lock::<Runtime>::contains_key((
                &coldkey,
                netuid,
                &origin_hotkey
            )));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setPerpetualLock(uint256,bool)"),
                        (U256::from(TEST_NETUID_U16), false),
                    ),
                )
                .execute_returns(());
            frame_system::Pallet::<Runtime>::set_block_number(2_000);
            let decayed = pallet_subtensor::Pallet::<Runtime>::get_coldkey_lock(&coldkey, netuid)
                .expect("decaying lock remains above the cleanup threshold");
            assert!(decayed.locked_mass < moved.locked_mass);
            assert_eq!(
                pallet_subtensor::DecayingLock::<Runtime>::get(&coldkey, netuid),
                None
            );
        });
    }

    #[test]
    fn staking_precompile_v2_exposes_lock_rates_conviction_batches_and_recipient_flag() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1121);
            let coldkey = mapped_account(caller);
            let hotkey = AccountId::from([0x63; 32]);
            let empty_hotkey = AccountId::from([0x64; 32]);
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);
            ensure_hotkey_exists(&empty_hotkey);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("lockStake(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(5_000_000_000_u64),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());
            frame_system::Pallet::<Runtime>::set_block_number(1_000);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(selector_u32("getLockRates()"), ()),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(2))
                .execute_returns((
                    pallet_subtensor::UnlockRate::<Runtime>::get(),
                    pallet_subtensor::MaturityRate::<Runtime>::get(),
                ));

            let candidates = vec![
                H256::from_slice(hotkey.as_ref()),
                H256::from_slice(empty_hotkey.as_ref()),
            ];
            let expected = vec![
                pallet_subtensor::Pallet::<Runtime>::hotkey_conviction(&hotkey, netuid).to_bits(),
                0,
            ];
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getHotkeyConvictions(uint256,bytes32[])"),
                        (U256::from(TEST_NETUID_U16), candidates),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    stake_info_validation_cost(2).saturating_add(
                        RuntimeHelper::<Runtime>::db_read_gas_cost()
                            .saturating_mul(2 * HOTKEY_LOCK_READS),
                    ),
                )
                .execute_returns(expected);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getRejectLockedAlpha(bytes32)"),
                        (H256::from_slice(coldkey.as_ref()),),
                    ),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns(true);
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(selector_u32("setRejectLockedAlpha(bool)"), (false,)),
                )
                .execute_returns(());
            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getRejectLockedAlpha(bytes32)"),
                        (H256::from_slice(coldkey.as_ref()),),
                    ),
                )
                .with_static_call(true)
                .expect_cost(RuntimeHelper::<Runtime>::db_read_gas_cost())
                .execute_returns(false);
        });
    }

    #[test]
    fn staking_precompile_v2_reports_empty_lock_and_preconfigured_perpetual_mode() {
        new_test_ext().execute_with(|| {
            setup_staking_subnet();
            let caller = addr_from_index(0x1123);
            let coldkey = mapped_account(caller);
            let hotkey = hotkey();
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("setPerpetualLock(uint256,bool)"),
                        (U256::from(TEST_NETUID_U16), true),
                    ),
                )
                .execute_returns(());

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getColdkeyLock(bytes32,uint256)"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(COLDKEY_LOCK_READS),
                )
                .execute_returns((false, H256::zero(), U256::zero(), 0_u128, true));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getHotkeyLock(bytes32,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(HOTKEY_LOCK_READS),
                )
                .execute_returns((false, U256::zero(), 0_u128));
        });
    }

    #[test]
    fn staking_precompile_v2_reports_expired_unpersisted_locks_as_nonexistent() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x1124);
            let coldkey = mapped_account(caller);
            let hotkey = AccountId::from([0x65; 32]);
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();
            let precompile_addr = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);

            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);
            pallet_subtensor::SubnetOwnerHotkey::<Runtime>::insert(netuid, &hotkey);
            pallet_subtensor::UnlockRate::<Runtime>::set(1);
            pallet_subtensor::MaturityRate::<Runtime>::set(1);

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("lockStake(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(5_000_000_000_u64),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_returns(());

            assert!(pallet_subtensor::Lock::<Runtime>::contains_key((
                &coldkey, netuid, &hotkey
            )));
            assert!(pallet_subtensor::DecayingOwnerLock::<Runtime>::contains_key(netuid));

            frame_system::Pallet::<Runtime>::set_block_number(100);
            let raw_lock = pallet_subtensor::Lock::<Runtime>::get((&coldkey, netuid, &hotkey))
                .expect("stale individual lock remains in storage");
            let (rolled_lock, _) =
                pallet_subtensor::staking::lock::ConvictionModel::roll_forward_lock(
                    raw_lock,
                    100,
                    pallet_subtensor::UnlockRate::<Runtime>::get(),
                    pallet_subtensor::MaturityRate::<Runtime>::get(),
                    true,
                    false,
                );
            assert!(rolled_lock.is_zero());

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getColdkeyLock(bytes32,uint256)"),
                        (
                            H256::from_slice(coldkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(COLDKEY_LOCK_READS),
                )
                .execute_returns((
                    false,
                    H256::from_slice(hotkey.as_ref()),
                    U256::zero(),
                    0_u128,
                    false,
                ));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getHotkeyLock(bytes32,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(HOTKEY_LOCK_READS),
                )
                .execute_returns((false, U256::zero(), 0_u128));

            precompiles
                .prepare_test(
                    caller,
                    precompile_addr,
                    encode_with_selector(
                        selector_u32("getHotkeyConvictions(uint256,bytes32[])"),
                        (
                            U256::from(TEST_NETUID_U16),
                            vec![H256::from_slice(hotkey.as_ref())],
                        ),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_validation_cost(1).saturating_add(
                    RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(HOTKEY_LOCK_READS),
                ))
                .execute_returns(vec![0_u128]);

            // View calls roll state in memory only, so this exercises stale rows.
            assert!(pallet_subtensor::Lock::<Runtime>::contains_key((
                &coldkey, netuid, &hotkey
            )));
            assert!(pallet_subtensor::DecayingOwnerLock::<Runtime>::contains_key(netuid));
        });
    }

    #[test]
    fn staking_precompile_v2_rejects_invalid_lock_inputs() {
        new_test_ext().execute_with(|| {
            setup_staking_subnet();
            let caller = addr_from_index(0x1122);
            let coldkey = mapped_account(caller);
            let hotkey = hotkey();
            fund_account(&coldkey, COLDKEY_BALANCE);
            add_stake_v2(caller, &hotkey, TEST_NETUID_U16, INITIAL_STAKE_RAO);

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("lockStake(bytes32,uint256,uint256)"),
                        (
                            H256::from_slice(hotkey.as_ref()),
                            U256::from(u64::MAX) + U256::one(),
                            U256::from(TEST_NETUID_U16),
                        ),
                    ),
                )
                .execute_error(ExitError::Other(
                    "the value is outside of u64 bounds".into(),
                ));

            let requested = H256::from_slice(hotkey.as_ref());
            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getHotkeyConvictions(uint256,bytes32[])"),
                        (U256::from(TEST_NETUID_U16), vec![requested, requested]),
                    ),
                )
                .with_static_call(true)
                .expect_cost(stake_info_validation_cost(2))
                .execute_reverts(|output| output == b"duplicate conviction hotkey");

            let too_many_hotkeys = vec![requested; MAX_CONVICTION_HOTKEYS + 1];
            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    addr_from_index(StakingPrecompileV2::<Runtime>::INDEX),
                    encode_with_selector(
                        selector_u32("getHotkeyConvictions(uint256,bytes32[])"),
                        (U256::from(TEST_NETUID_U16), too_many_hotkeys),
                    ),
                )
                .with_static_call(true)
                .expect_cost(0)
                .execute_reverts(|output| output == b"hotkeys: Value is too large for length");
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

            precompiles::<StakingPrecompile<Runtime>>()
                .prepare_test(
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
                )
                .with_static_call(true)
                .expect_cost(stake_read_cost(1))
                .execute_returns(substrate_to_evm(stake_after));
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

            precompiles
                .prepare_test(
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
                )
                .with_static_call(true)
                .expect_cost(stake_read_cost(1))
                .execute_returns(U256::from(stake_after));
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

    #[test]
    fn aggregate_stake_views_charge_their_scans() {
        new_test_ext().execute_with(|| {
            setup_staking_subnet();
            let caller = addr_from_index(0x3005);
            let empty_account = AccountId::from([0x91; 32]);
            let account_arg = H256::from_slice(empty_account.as_ref());
            let db_read = RuntimeHelper::<Runtime>::db_read_gas_cost();
            let hotkey_reads = 1u64.saturating_add(
                u64::from(pallet_subtensor::SubnetLimit::<Runtime>::get())
                    .saturating_mul(TOTAL_HOTKEY_STAKE_READS_PER_SUBNET),
            );

            for (address, is_v2) in [
                (addr_from_index(StakingPrecompileV2::<Runtime>::INDEX), true),
                (addr_from_index(StakingPrecompile::<Runtime>::INDEX), false),
            ] {
                let precompiles = Precompiles::<Runtime>::new();
                precompiles
                    .prepare_test(
                        caller,
                        address,
                        encode_with_selector(
                            selector_u32("getTotalHotkeyStake(bytes32)"),
                            (account_arg,),
                        ),
                    )
                    .with_static_call(true)
                    .expect_cost(db_read.saturating_mul(hotkey_reads))
                    .execute_returns(U256::zero());

                precompiles
                    .prepare_test(
                        caller,
                        address,
                        encode_with_selector(
                            selector_u32("getTotalColdkeyStake(bytes32)"),
                            (account_arg,),
                        ),
                    )
                    .with_static_call(true)
                    .expect_cost(db_read.saturating_mul(2))
                    .execute_returns(U256::zero());

                if is_v2 {
                    precompiles
                        .prepare_test(
                            caller,
                            address,
                            encode_with_selector(
                                selector_u32("getTotalColdkeyStakeOnSubnet(bytes32,uint256)"),
                                (account_arg, U256::from(TEST_NETUID_U16)),
                            ),
                        )
                        .with_static_call(true)
                        .expect_cost(db_read.saturating_mul(2))
                        .execute_returns(U256::zero());
                }
            }
        });
    }

    #[test]
    fn coldkey_aggregate_views_charge_each_stake_position() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x3007);
            let coldkey = AccountId::from([0x92; 32]);
            let hotkey = AccountId::from([0x93; 32]);
            let coldkey_word = H256::from_slice(coldkey.as_ref());
            pallet_subtensor::Pallet::<Runtime>::increase_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey,
                &coldkey,
                netuid,
                AlphaBalance::from(1_000_u64),
            );

            let total =
                pallet_subtensor::Pallet::<Runtime>::get_total_stake_for_coldkey(&coldkey).to_u64();
            let subnet_total =
                pallet_subtensor::Pallet::<Runtime>::get_total_stake_for_coldkey_on_subnet(
                    &coldkey, netuid,
                )
                .to_u64();
            let reads = 2_u64
                .saturating_add(TOTAL_COLDKEY_POSITION_BASE_READS)
                .saturating_add(TOTAL_COLDKEY_MATCHED_POSITION_READS);
            let cost = RuntimeHelper::<Runtime>::db_read_gas_cost().saturating_mul(reads);
            let address = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);
            let precompiles = Precompiles::<Runtime>::new();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getTotalColdkeyStake(bytes32)"),
                        (coldkey_word,),
                    ),
                )
                .with_static_call(true)
                .expect_cost(cost)
                .execute_returns(U256::from(total));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getTotalColdkeyStakeOnSubnet(bytes32,uint256)"),
                        (coldkey_word, U256::from(TEST_NETUID_U16)),
                    ),
                )
                .with_static_call(true)
                .expect_cost(cost)
                .execute_returns(U256::from(subnet_total));
        });
    }

    #[test]
    fn staking_state_views_return_typed_values_and_missing_state() {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(TEST_NETUID_U16);
            let caller = addr_from_index(0x3006);
            let address = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);
            let hotkey = AccountId::from([0x71; 32]);
            let coldkey = AccountId::from([0x72; 32]);
            let hotkey_word = H256::from_slice(hotkey.as_ref());
            let coldkey_word = H256::from_slice(coldkey.as_ref());
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();

            macro_rules! assert_view {
                ($signature:literal, $arguments:expr, $expected:expr) => {
                    precompiles
                        .prepare_test(
                            caller,
                            address,
                            encode_with_selector(selector_u32($signature), $arguments),
                        )
                        .with_static_call(true)
                        .execute_returns($expected);
                };
            }

            pallet_subtensor::Delegates::<Runtime>::insert(&hotkey, PerU16::from_parts(123));
            pallet_subtensor::Owner::<Runtime>::insert(&hotkey, &coldkey);
            pallet_subtensor::OwnedHotkeys::<Runtime>::insert(&coldkey, vec![hotkey.clone()]);

            assert_view!("getDelegate(bytes32)", (hotkey_word,), (true, 123_u16));
            assert_view!(
                "getChildkeyTake(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                pallet_subtensor::ChildkeyTake::<Runtime>::get(&hotkey, netuid).deconstruct()
            );
            assert_view!(
                "getPendingChildKeys(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                (Vec::<(u64, H256)>::new(), 0_u64)
            );
            assert_view!(
                "getChildKeys(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                Vec::<(u64, H256)>::new()
            );
            assert_view!(
                "getParentKeys(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                Vec::<(u64, H256)>::new()
            );
            assert_view!(
                "getPendingChildKeyCooldown()",
                (),
                pallet_subtensor::PendingChildKeyCooldown::<Runtime>::get()
            );
            assert_view!(
                "getTakeLimits()",
                (),
                (
                    pallet_subtensor::MinDelegateTake::<Runtime>::get().deconstruct(),
                    pallet_subtensor::MaxDelegateTake::<Runtime>::get().deconstruct(),
                    pallet_subtensor::MinChildkeyTake::<Runtime>::get().deconstruct(),
                    pallet_subtensor::MaxChildkeyTake::<Runtime>::get().deconstruct(),
                )
            );
            assert_view!(
                "getMinChildkeyTakePerSubnet(uint16)",
                (TEST_NETUID_U16,),
                pallet_subtensor::MinChildkeyTakePerSubnet::<Runtime>::get(netuid).deconstruct()
            );
            assert_view!(
                "getHotkeyOwner(bytes32)",
                (hotkey_word,),
                (true, coldkey_word)
            );
            assert_view!(
                "getOwnedHotkeys(bytes32)",
                (coldkey_word,),
                vec![hotkey_word]
            );
            assert_view!(
                "getAutoStakeDestination(bytes32,uint16)",
                (coldkey_word, TEST_NETUID_U16),
                (false, H256::zero())
            );
            assert_view!(
                "getAutoStakeDestinationColdkeys(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                Vec::<H256>::new()
            );
            assert_view!(
                "getHotkeySuccessor(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                (false, H256::zero())
            );
            assert_view!(
                "getHotkeyRoot(bytes32,uint16)",
                (hotkey_word, TEST_NETUID_U16),
                (false, H256::zero())
            );
            assert_view!(
                "getColdkeySuccessor(bytes32)",
                (coldkey_word,),
                (false, H256::zero())
            );
            assert_view!(
                "getColdkeyRoot(bytes32)",
                (coldkey_word,),
                (false, H256::zero())
            );
            assert_view!(
                "getColdkeySwapStatus(bytes32)",
                (coldkey_word,),
                (false, 0_u64, H256::zero(), false, 0_u64)
            );
            assert_view!(
                "getColdkeySwapDelays()",
                (),
                (
                    pallet_subtensor::ColdkeySwapAnnouncementDelay::<Runtime>::get(),
                    pallet_subtensor::ColdkeySwapReannouncementDelay::<Runtime>::get(),
                )
            );
            assert_view!(
                "getLastHotkeySwapOnSubnet(bytes32,uint16)",
                (coldkey_word, TEST_NETUID_U16),
                0_u64
            );
            assert_view!(
                "getStakeAccounting()",
                (),
                (
                    pallet_subtensor::TotalIssuance::<Runtime>::get().to_u64(),
                    pallet_subtensor::TotalStake::<Runtime>::get().to_u64(),
                )
            );
            assert_view!(
                "getMinerCollateral(uint16,bytes32,bytes32)",
                (TEST_NETUID_U16, hotkey_word, coldkey_word),
                (false, 0_u64, 0_u128, 0_u64, 0_u64)
            );
            assert_view!(
                "getColdkeyCollateral(uint16,bytes32)",
                (TEST_NETUID_U16, coldkey_word),
                (0_u64, Vec::<H256>::new())
            );
            assert_view!(
                "getCollateralConfig(uint16)",
                (TEST_NETUID_U16,),
                (
                    pallet_subtensor::CollateralLockShare::<Runtime>::get(netuid),
                    pallet_subtensor::CollateralDrainRatio::<Runtime>::get(netuid).to_bits(),
                )
            );
        });
    }

    #[test]
    fn staking_precompile_v2_claims_root_for_one_hotkey_and_all_hotkeys() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let subnet_account = pallet_subtensor::Pallet::<Runtime>::get_subnet_account_id(netuid)
                .expect("test subnet has an account");
            fund_account(&subnet_account, RESERVE_TAO);
            let caller = addr_from_index(0x3060);
            let coldkey = mapped_account(caller);
            let hotkey_a = AccountId::from([0x81; 32]);
            let hotkey_b = AccountId::from([0x82; 32]);
            let address = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();

            setup_root_basket_position(&coldkey, &hotkey_a, netuid, 25, 100, 100_000_000_000);
            setup_root_basket_position(&coldkey, &hotkey_b, netuid, 40, 100, 100_000_000_000);
            pallet_subtensor::StakingHotkeys::<Runtime>::insert(
                &coldkey,
                vec![hotkey_a.clone(), hotkey_b.clone()],
            );
            pallet_subtensor::RootClaimableThreshold::<Runtime>::insert(
                NetUid::ROOT,
                substrate_fixed::types::I96F32::from_num(0),
            );

            assert!(
                pallet_subtensor::Pallet::<Runtime>::get_basket_payout_tao(&hotkey_a, &coldkey) > 0
            );
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("claimRootWithHotkey(bytes32)"),
                        (H256::from_slice(hotkey_a.as_ref()),),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::Pallet::<Runtime>::get_basket_payout_tao(&hotkey_a, &coldkey),
                0
            );
            assert!(
                pallet_subtensor::Pallet::<Runtime>::get_basket_payout_tao(&hotkey_b, &coldkey) > 0
            );

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("claimRoot(uint16[])"),
                        (vec![TEST_NETUID_U16],),
                    ),
                )
                .execute_returns(());
            assert_eq!(
                pallet_subtensor::Pallet::<Runtime>::get_basket_payout_tao(&hotkey_b, &coldkey),
                0
            );
        });
    }

    #[test]
    fn staking_precompile_v2_reads_unclaimed_root_value_by_hotkey_and_subnet() {
        new_test_ext().execute_with(|| {
            let netuid = setup_staking_subnet();
            let caller = addr_from_index(0x3061);
            let coldkey = mapped_account(caller);
            let hotkey_a = AccountId::from([0x83; 32]);
            let hotkey_b = AccountId::from([0x84; 32]);
            let coldkey_word = H256::from_slice(coldkey.as_ref());
            let address = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);
            let precompiles = precompiles::<StakingPrecompileV2<Runtime>>();

            setup_root_basket_position(&coldkey, &hotkey_a, netuid, 20, 100, 80_000_000_000);
            setup_root_basket_position(&coldkey, &hotkey_b, netuid, 30, 100, 60_000_000_000);
            pallet_subtensor::StakingHotkeys::<Runtime>::insert(
                &coldkey,
                vec![hotkey_a.clone(), hotkey_b.clone()],
            );

            let hotkey_payout =
                pallet_subtensor::Pallet::<Runtime>::get_basket_payout_tao(&hotkey_a, &coldkey);
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getUnclaimedRootTaoByHotkey(bytes32,bytes32)"),
                        (coldkey_word, H256::from_slice(hotkey_a.as_ref())),
                    ),
                )
                .with_static_call(true)
                .execute_returns(substrate_to_evm(hotkey_payout));

            let subnet_payout_a = pallet_subtensor::Pallet::<Runtime>::get_basket_subnet_payout_tao(
                &hotkey_a, &coldkey, netuid,
            );
            let subnet_payout_b = pallet_subtensor::Pallet::<Runtime>::get_basket_subnet_payout_tao(
                &hotkey_b, &coldkey, netuid,
            );

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getUnclaimedRootTaoBySubnet(bytes32,uint16,bytes32[])"),
                        (
                            coldkey_word,
                            TEST_NETUID_U16,
                            vec![
                                H256::from_slice(hotkey_a.as_ref()),
                                H256::from_slice(hotkey_b.as_ref()),
                            ],
                        ),
                    ),
                )
                .with_static_call(true)
                .execute_returns(substrate_to_evm(
                    subnet_payout_a.saturating_add(subnet_payout_b),
                ));
        });
    }

    #[test]
    fn staking_precompile_v2_bounds_and_deduplicates_unclaimed_root_hotkeys() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x3062);
            let address = addr_from_index(StakingPrecompileV2::<Runtime>::INDEX);
            let coldkey = H256::repeat_byte(0x85);
            let too_many = vec![H256::repeat_byte(0x86); 65];

            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getUnclaimedRootTaoBySubnet(bytes32,uint16,bytes32[])"),
                        (coldkey, TEST_NETUID_U16, too_many),
                    ),
                )
                .with_static_call(true)
                .execute_reverts(|output| output == b"hotkeys: Value is too large for length");

            let duplicate = H256::repeat_byte(0x87);
            precompiles::<StakingPrecompileV2<Runtime>>()
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getUnclaimedRootTaoBySubnet(bytes32,uint16,bytes32[])"),
                        (coldkey, TEST_NETUID_U16, vec![duplicate, duplicate]),
                    ),
                )
                .with_static_call(true)
                .execute_reverts(|output| output == b"duplicate unclaimed root hotkey");
        });
    }
}
