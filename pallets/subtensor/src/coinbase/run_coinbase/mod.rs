//! Per-block coinbase: mint distribution, pool injection, pending drain, and dividend payout.
//!
//! ## Pipeline (called from [`crate::Pallet::block_step`])
//!
//! 1. [`Pallet::run_coinbase`] — orchestrates steps 2–5 for the block's minted TAO credit.
//! 2. [`Pallet::get_subnet_block_emissions`] / [`Pallet::emit_to_subnets`] — split TAO by
//!    price shares, inject pool liquidity (`tao_in`/`alpha_in`), swap excess TAO, and
//!    accumulate pending alpha (server / validator / root / owner cut).
//! 3. [`Pallet::drain_pending_subnet_emissions`] — on each subnet's epoch slot (tempo / trigger / defer),
//!    take pending alpha and advance `LastEpochBlock`.
//! 4. [`Pallet::distribute_emissions_to_subnets`] — run consensus epoch and pay incentives
//!    / dividends / root claimables.
//!
//! ## Search anchors
//!
//! | Module | Owns |
//! |--------|------|
//! | [`emission_injection`] | `inject_pool_liquidity_and_swap_excess`, `compute_subnet_emission_terms`, `emit_to_subnets` |
//! | [`drain_pending_emissions`] | epoch scheduling + pending drain |
//! | [`dividend_distribution`] | incentive/dividend split and stake payout |

use super::*;
use crate::coinbase::tao::TaoCreditOf;
use frame_support::traits::Imbalance;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::NetUid;

mod dividend_distribution;
mod drain_pending_emissions;
mod emission_injection;
mod fixed_point;

#[allow(unused_imports)]
pub(crate) use fixed_point::{as_u96f32, to_u64};

impl<T: Config> Pallet<T> {
    /// Distribute this block's minted TAO credit across eligible subnets, then drain and
    /// pay any subnet whose epoch slot fires this block.
    ///
    /// Resets [`SubnetRootSellTao`] counters at the start (prior block's root sells are
    /// consumed here). Unused credit is recycled via [`Pallet::recycle_credit`].
    pub fn run_coinbase(block_emission_credit: TaoCreditOf<T>) {
        // --- 0. Get current block.
        let current_block: u64 = Self::get_current_block_as_u64();
        let block_emission = U96F32::saturating_from_num(block_emission_credit.peek());
        log::debug!(
            "Running coinbase for block {current_block:?} with block emission: {block_emission:?}"
        );

        // Reset per-block root sell counters from the previous block.
        // Root sells happen after coinbase, so their accumulated values
        // are consumed here at the start of the next block.
        let _ = SubnetRootSellTao::<T>::clear(u32::MAX, None);

        // --- 1. Get all subnets (excluding root).
        let subnets: Vec<NetUid> = Self::get_all_subnet_netuids()
            .into_iter()
            .filter(|netuid| *netuid != NetUid::ROOT)
            .collect();
        log::debug!("All subnets: {subnets:?}");

        // --- 2. Get subnets to emit to
        let subnets_to_emit_to: Vec<NetUid> = Self::get_subnets_to_emit_to(&subnets);
        log::debug!("Subnets to emit to: {subnets_to_emit_to:?}");

        // --- 3. Get emissions for subnets to emit to
        let subnet_emissions =
            Self::get_subnet_block_emissions(&subnets_to_emit_to, block_emission);
        log::debug!("Subnet emissions: {subnet_emissions:?}");
        let root_sell_flag = Self::get_network_root_sell_flag(&subnets_to_emit_to);
        log::debug!("Root sell flag: {root_sell_flag:?}");

        // --- 4. Emit to subnets for this block.
        Self::emit_to_subnets(
            &subnets_to_emit_to,
            &subnet_emissions,
            block_emission_credit,
            root_sell_flag,
        );

        // --- 5. Drain pending emissions.
        let emissions_to_distribute = Self::drain_pending_subnet_emissions(&subnets, current_block);

        // --- 6. Distribute the emissions to the subnets.
        Self::distribute_emissions_to_subnets(&emissions_to_distribute);
    }
}
