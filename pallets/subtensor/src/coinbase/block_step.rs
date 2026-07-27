//! Per-block hook body: registration prices → mint → reveal → coinbase → EMA/root updates.

use super::*;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::NetUid;

impl<T: Config + pallet_drand::Config> Pallet<T> {
    /// Ordered per-block coinbase pipeline (see module docs). Invoked from the runtime
    /// `on_initialize` / block-step hook.
    pub fn block_step() -> Result<(), &'static str> {
        let block_number: u64 = Self::get_current_block_as_u64();
        let last_block_hash: T::Hash = <frame_system::Pallet<T>>::parent_hash();

        // --- 1. Update registration burn prices.
        Self::update_registration_prices_for_networks();

        // --- 2. Get the current coinbase emission.
        let block_emission = Self::get_block_emission();
        log::debug!("Block emission: {block_emission:?}");

        // --- 3. Reveal matured weights.
        Self::reveal_crv3_commits();
        // --- 4. Run emission through network.
        Self::run_coinbase(block_emission);
        // --- 5. Update moving prices AFTER using them for emissions.
        Self::update_moving_prices();
        // --- 6. Update root prop AFTER using them for emissions.
        Self::update_root_prop();
        // --- 7. Set pending children on the epoch; but only after the coinbase has been run.
        Self::set_pending_children_after_epoch(block_number);
        // --- 8. Run auto-claim root divs.
        Self::run_auto_claim_root_divs(last_block_hash);
        // --- 9. Populate root coldkey maps.
        Self::populate_root_coldkey_staking_maps();
        Self::populate_root_coldkey_staking_maps_v2();

        Ok(())
    }

    /// After `run_coinbase` advances `LastEpochBlock`, apply pending children for any subnet
    /// whose epoch just fired (`LastEpochBlock == block_number`).
    fn set_pending_children_after_epoch(block_number: u64) {
        for netuid in Self::get_all_subnet_netuids() {
            if LastEpochBlock::<T>::get(netuid) == block_number {
                Self::do_set_pending_children(netuid);
            }
        }
    }

    /// Advance the alpha-price EMA for every subnet that received emission this block.
    pub fn update_moving_prices() {
        let subnets_to_emit_to: Vec<NetUid> =
            Self::get_subnets_to_emit_to(&Self::get_all_subnet_netuids());
        for netuid_i in subnets_to_emit_to.iter() {
            Self::update_moving_price(*netuid_i);
        }
    }

    /// Refresh [`RootProp`] for every emit-eligible subnet from [`Pallet::root_proportion`].
    pub fn update_root_prop() {
        let subnets_to_emit_to: Vec<NetUid> =
            Self::get_subnets_to_emit_to(&Self::get_all_subnet_netuids());
        for netuid_i in subnets_to_emit_to.iter() {
            let root_prop = Self::root_proportion(*netuid_i);

            RootProp::<T>::insert(netuid_i, root_prop);
        }
    }

    /// Root's share of a subnet's stake-weighted mass:
    /// `tao_weight / (tao_weight + alpha_issuance)` where `tao_weight = root_tao * TAO_WEIGHT`.
    ///
    /// Caps alpha injection into older pools (see [`Pallet::compute_subnet_emission_terms`]).
    pub fn root_proportion(netuid: NetUid) -> U96F32 {
        let alpha_issuance = U96F32::from_num(Self::get_alpha_issuance(netuid));
        let root_tao: U96F32 = U96F32::from_num(Self::get_subnet_tao(NetUid::ROOT));
        let tao_weight: U96F32 = root_tao.saturating_mul(Self::get_tao_weight());

        let root_proportion: U96F32 = tao_weight
            .checked_div(tao_weight.saturating_add(alpha_issuance))
            .unwrap_or(U96F32::from_num(0.0));

        root_proportion
    }

    /// Reveal matured CRv3 commits for every non-root subnet not deferred by the per-block
    /// epoch cap.
    pub fn reveal_crv3_commits() {
        let current_block = Self::get_current_block_as_u64();
        let subnets: Vec<NetUid> = Self::get_all_subnet_netuids()
            .into_iter()
            .filter(|netuid| *netuid != NetUid::ROOT)
            .collect();
        // Subnets whose epoch is due this block but deferred by the per-block cap.
        let deferred = Self::epochs_deferred_this_block(&subnets, current_block);

        for netuid in subnets.into_iter() {
            if deferred.contains(&netuid) {
                continue;
            }
            if let Err(e) = Self::reveal_crv3_commits_for_subnet(netuid) {
                log::warn!("Failed to reveal commits for subnet {netuid} due to error: {e:?}");
            };
        }
    }
}
