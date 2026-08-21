//! USD rails precompile (address 2068 / 0x814).
//!
//! The single EVM door into the canonical rails kernel (`pallet-usd-psm`):
//! the Gateway contract delivers bridge envelopes through `gatewayExecute`,
//! plus read-only views (quotes, pool state, share index) for clients.
//!
//! Atomicity: Frontier wraps every EVM frame in a substrate storage
//! transaction, so pallet state changes here revert together with the
//! delivering EVM frame on any failure.

use core::marker::PhantomData;

use fp_evm::{ExitError, PrecompileFailure};
use pallet_evm::PrecompileHandle;
use precompile_utils::EvmResult;
use precompile_utils::prelude::{Address, UnboundedBytes};
use sp_core::H256;
use sp_runtime::AccountId32;
use subtensor_runtime_common::NetUid;

use crate::PrecompileExt;
use crate::extensions::PrecompileHandleExt;

pub struct UsdRailsPrecompile<R>(PhantomData<R>);

impl<R> PrecompileExt<R::AccountId> for UsdRailsPrecompile<R>
where
    R: frame_system::Config<AccountId = AccountId32> + pallet_evm::Config + pallet_usd_psm::Config,
{
    const INDEX: u64 = 2068;
}

#[precompile_utils::precompile]
impl<R> UsdRailsPrecompile<R>
where
    R: frame_system::Config<AccountId = AccountId32> + pallet_evm::Config + pallet_usd_psm::Config,
{
    /// Execute an inbound gateway envelope. Callable only by the registered
    /// Gateway contract; the pallet enforces the caller check.
    #[precompile::public("gatewayExecute(uint64,bytes)")]
    fn gateway_execute(
        handle: &mut impl PrecompileHandle,
        amount: u64,
        envelope: UnboundedBytes,
    ) -> EvmResult<()> {
        handle.record_db_reads::<R>(16)?;
        let caller = handle.context().caller;
        pallet_usd_psm::Pallet::<R>::do_gateway_execute(caller, amount, envelope.as_bytes())
            .map_err(dispatch_error)
    }

    /// Quote tUSD -> TAO through the canonical pool.
    #[precompile::public("simSwapUsdForTao(uint64)")]
    #[precompile::view]
    fn sim_swap_usd_for_tao(handle: &mut impl PrecompileHandle, amount_usd: u64) -> EvmResult<u64> {
        handle.record_db_reads::<R>(3)?;
        Ok(pallet_usd_psm::Pallet::<R>::quote_tusd_for_tao(amount_usd).unwrap_or_default())
    }

    /// Quote TAO -> tUSD through the canonical pool.
    #[precompile::public("simSwapTaoForUsd(uint64)")]
    #[precompile::view]
    fn sim_swap_tao_for_usd(handle: &mut impl PrecompileHandle, amount_tao: u64) -> EvmResult<u64> {
        handle.record_db_reads::<R>(3)?;
        Ok(pallet_usd_psm::Pallet::<R>::quote_tao_for_tusd(amount_tao).unwrap_or_default())
    }

    /// tUSD ledger balance of a substrate account.
    #[precompile::public("tusdBalanceOf(bytes32)")]
    #[precompile::view]
    fn tusd_balance_of(handle: &mut impl PrecompileHandle, account: H256) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_usd_psm::Pallet::<R>::tusd_balance(&AccountId32::new(account.0)))
    }

    /// Canonical pool state: (taoReserve, tusdReserve, feeBps).
    #[precompile::public("poolState()")]
    #[precompile::view]
    fn pool_state(handle: &mut impl PrecompileHandle) -> EvmResult<(u64, u64, u16)> {
        handle.record_db_reads::<R>(3)?;
        Ok((
            pallet_usd_psm::PoolTaoReserve::<R>::get(),
            pallet_usd_psm::PoolTUsdReserve::<R>::get(),
            pallet_usd_psm::PoolFeeBps::<R>::get(),
        ))
    }

    /// The ERC-20 contract backing a PSM asset.
    #[precompile::public("assetErc20(uint32)")]
    #[precompile::view]
    fn asset_erc20(handle: &mut impl PrecompileHandle, asset_id: u32) -> EvmResult<Address> {
        handle.record_db_reads::<R>(1)?;
        Ok(Address(
            pallet_usd_psm::Pallet::<R>::psm_asset(asset_id)
                .map(|a| a.erc20)
                .unwrap_or_default(),
        ))
    }

    /// Share index (1e9 fixed point) for a subnet's canonical shares.
    #[precompile::public("shareIndexE9(uint16)")]
    #[precompile::view]
    fn share_index_e9(handle: &mut impl PrecompileHandle, netuid: u16) -> EvmResult<u64> {
        handle.record_db_reads::<R>(4)?;
        Ok(pallet_usd_psm::Pallet::<R>::share_index_e9(NetUid::from(
            netuid,
        )))
    }

    /// Next inbound envelope nonce expected by the sequential guard.
    #[precompile::public("nextNonce()")]
    #[precompile::view]
    fn next_nonce(handle: &mut impl PrecompileHandle) -> EvmResult<u64> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_usd_psm::Pallet::<R>::next_nonce())
    }
}

fn dispatch_error(err: sp_runtime::DispatchError) -> PrecompileFailure {
    log::debug!("usd rails precompile dispatch error: {err:?}");
    PrecompileFailure::Error {
        exit_status: ExitError::Other(alloc::format!("rails: {err:?}").into()),
    }
}
