//! Bonds EMA (normal + liquid-alpha), liquid-alpha sigmoid, and alpha-bounds / bonds-reset helpers.

use super::*;
use crate::epoch::math::*;
use alloc::collections::BTreeSet;
use safe_math::*;
use substrate_fixed::types::{I32F32, I64F64};
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

impl<T: Config> Pallet<T> {
    /// Bonds EMA with a single subnet-wide alpha (sparse); used when liquid alpha is off.
    pub fn ema_bonds_normal_sparse(
        bonds_delta: &[Vec<(u16, I32F32)>],
        bonds: &[Vec<(u16, I32F32)>],
        netuid_index: NetUidStorageIndex,
    ) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();

        // Retrieve the bonds moving average for the given network ID and scale it down.
        let bonds_moving_average: I64F64 =
            I64F64::saturating_from_num(Self::get_bonds_moving_average(netuid))
                .safe_div(I64F64::saturating_from_num(1_000_000));

        // Calculate the alpha value for the EMA calculation.
        // Alpha is derived by subtracting the scaled bonds moving average from 1.
        let alpha: I32F32 = I32F32::saturating_from_num(1)
            .saturating_sub(I32F32::saturating_from_num(bonds_moving_average));

        // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
        let ema_bonds = mat_ema_sparse(bonds_delta, bonds, alpha);

        // Log the computed EMA bonds for debugging purposes.
        log::trace!("Exponential Moving Average Bonds Normal: {ema_bonds:?}");

        // Return the computed EMA bonds.
        ema_bonds
    }

    /// Bonds EMA with a single subnet-wide alpha (dense); test / dense-epoch path.
    pub fn ema_bonds_normal_dense(
        bonds_delta: &[Vec<I32F32>],
        bonds: &[Vec<I32F32>],
        netuid: NetUid,
    ) -> Vec<Vec<I32F32>> {
        // Retrieve the bonds moving average for the given network ID and scale it down.
        let bonds_moving_average: I64F64 =
            I64F64::saturating_from_num(Self::get_bonds_moving_average(netuid))
                .safe_div(I64F64::saturating_from_num(1_000_000));

        // Calculate the alpha value for the EMA calculation.
        // Alpha is derived by subtracting the scaled bonds moving average from 1.
        let alpha: I32F32 = I32F32::saturating_from_num(1)
            .saturating_sub(I32F32::saturating_from_num(bonds_moving_average));

        // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
        let ema_bonds = mat_ema(bonds_delta, bonds, alpha);

        // Log the computed EMA bonds for debugging purposes.
        log::trace!("Exponential Moving Average Bonds Normal: {ema_bonds:?}");

        // Return the computed EMA bonds.
        ema_bonds
    }

    pub fn compute_bonds(
        netuid: NetUid,
        weights: &[Vec<I32F32>], // weights_for_bonds
        bonds: &[Vec<I32F32>],
        consensus: &[I32F32],
    ) -> Vec<Vec<I32F32>> {
        // Check if Liquid Alpha is enabled, consensus is not empty, and contains non-zero values.
        if LiquidAlphaOn::<T>::get(netuid)
            && !consensus.is_empty()
            && consensus
                .iter()
                .any(|&c| c != I32F32::saturating_from_num(0))
        {
            // Liquid Alpha is enabled, compute the liquid alphas matrix.
            let alphas: Vec<Vec<I32F32>> =
                Self::liquid_alpha_matrix_dense(netuid, weights, bonds, consensus);
            log::trace!("alphas: {:?}", &alphas);

            // Compute the Exponential Moving Average (EMA) of bonds using the provided clamped alpha values.
            mat_ema_alpha(weights, bonds, &alphas)
        } else {
            // Liquid Alpha is disabled, compute the liquid alpha value.
            let alpha: I32F32 = Self::bonds_moving_average_alpha(netuid);

            // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
            mat_ema(weights, bonds, alpha)
        }
    }

    /// Sparse bonds EMA: liquid-alpha matrix when enabled, else [`Self::bonds_moving_average_alpha`].
    pub fn ema_bonds_liquid_or_normal_sparse(
        netuid_index: NetUidStorageIndex,
        weights: &[Vec<(u16, I32F32)>],
        bonds: &[Vec<(u16, I32F32)>],
        consensus: &[I32F32],
    ) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();

        // Check if Liquid Alpha is enabled, consensus is not empty, and contains non-zero values.
        if LiquidAlphaOn::<T>::get(netuid)
            && !consensus.is_empty()
            && consensus
                .iter()
                .any(|&c| c != I32F32::saturating_from_num(0))
        {
            // Liquid Alpha is enabled, compute the liquid alphas matrix.
            let alphas: Vec<Vec<I32F32>> =
                Self::liquid_alpha_matrix_sparse(netuid, weights, bonds, consensus);
            log::trace!("alphas: {:?}", &alphas);

            // Compute the Exponential Moving Average (EMA) of bonds using the provided clamped alpha values.
            mat_ema_alpha_sparse(weights, bonds, &alphas)
        } else {
            // Liquid Alpha is disabled, compute the liquid alpha value.
            let alpha: I32F32 = Self::bonds_moving_average_alpha(netuid);

            // Compute the Exponential Moving Average (EMA) of bonds using the calculated alpha value.
            mat_ema_sparse(weights, bonds, alpha)
        }
    }

    /// Per validator-miner liquid-alpha values (dense) from weights, prior bonds, and consensus.
    pub fn liquid_alpha_matrix_dense(
        netuid: NetUid,
        weights: &[Vec<I32F32>], // current epoch weights
        bonds: &[Vec<I32F32>],   // previous epoch bonds
        consensus: &[I32F32],    // previous epoch consensus weights
    ) -> Vec<Vec<I32F32>> {
        let mut alphas = Vec::new();

        if weights.len() != bonds.len() {
            log::error!(
                "math error: liquid_alpha_matrix_dense: weights and bonds have different lengths: {:?} != {:?}",
                weights.len(),
                bonds.len()
            );
            return alphas;
        }

        // Get the high and low alpha values for the network.
        let alpha_sigmoid_steepness: I32F32 = Self::get_alpha_sigmoid_steepness(netuid);
        let (alpha_low, alpha_high): (I32F32, I32F32) = Self::get_alpha_values_32(netuid);

        for (w_row, b_row) in weights.iter().zip(bonds.iter()) {
            let mut row_alphas = Vec::new();

            for ((weight, bond), consensus_val) in
                w_row.iter().zip(b_row.iter()).zip(consensus.iter())
            {
                let alpha = Self::liquid_alpha_sigmoid(
                    *consensus_val,
                    *weight,
                    *bond,
                    alpha_low,
                    alpha_high,
                    alpha_sigmoid_steepness,
                );
                row_alphas.push(alpha);
            }
            alphas.push(row_alphas);
        }
        alphas
    }

    /// Per validator-miner liquid-alpha values (sparse weights/bonds to dense alpha matrix).
    pub fn liquid_alpha_matrix_sparse(
        netuid: NetUid,
        weights: &[Vec<(u16, I32F32)>], // current epoch weights
        bonds: &[Vec<(u16, I32F32)>],   // previous epoch bonds
        consensus: &[I32F32],           // previous epoch consensus weights
    ) -> Vec<Vec<I32F32>> {
        let mut alphas = Vec::with_capacity(consensus.len());

        if weights.len() != bonds.len() {
            log::error!(
                "math error: liquid_alpha_matrix_dense: weights and bonds have different lengths: {:?} != {:?}",
                weights.len(),
                bonds.len()
            );
            return alphas;
        }

        let alpha_sigmoid_steepness: I32F32 = Self::get_alpha_sigmoid_steepness(netuid);
        let (alpha_low, alpha_high): (I32F32, I32F32) = Self::get_alpha_values_32(netuid);

        let zero = I32F32::from_num(0.0);

        // iterate over rows
        for (w_row, b_row) in weights.iter().zip(bonds.iter()) {
            let mut row_alphas = Vec::with_capacity(w_row.len());
            let mut w_iter = w_row.iter().peekable();
            let mut b_iter = b_row.iter().peekable();
            for (j_pos, consensus_val) in consensus.iter().enumerate() {
                let j = j_pos as u16;

                let mut weight = zero;
                while let Some(&&(i, val)) = w_iter.peek() {
                    if i < j {
                        w_iter.next();
                    } else {
                        if i == j {
                            weight = val;
                        }
                        break;
                    }
                }

                let mut bond = zero;
                while let Some(&&(i, val)) = b_iter.peek() {
                    if i < j {
                        b_iter.next();
                    } else {
                        if i == j {
                            bond = val;
                        }
                        break;
                    }
                }

                let alpha = Self::liquid_alpha_sigmoid(
                    *consensus_val,
                    weight,
                    bond,
                    alpha_low,
                    alpha_high,
                    alpha_sigmoid_steepness,
                );
                row_alphas.push(alpha);
            }
            alphas.push(row_alphas);
        }
        alphas
    }

    /// Sigmoid liquid-alpha for one edge, clamped to `[alpha_low, alpha_high]`.
    pub fn liquid_alpha_sigmoid(
        consensus: I32F32,
        weight: I32F32,
        bond: I32F32,
        alpha_low: I32F32,
        alpha_high: I32F32,
        alpha_sigmoid_steepness: I32F32,
    ) -> I32F32 {
        let zero = I32F32::from_num(0.0);
        let one = I32F32::from_num(1.0);

        let diff_buy = clamp_i32f32(weight.saturating_sub(consensus), zero, one);
        let diff_sell = clamp_i32f32(bond.saturating_sub(weight), zero, one);
        let combined_diff = if weight >= bond { diff_buy } else { diff_sell };

        // sigmoid = 1. / (1. + e^(-steepness * (combined_diff - 0.5)))
        let sigmoid = one.saturating_div(
            one.saturating_add(exp_safe(
                alpha_sigmoid_steepness
                    .saturating_div(I32F32::from_num(-100))
                    .saturating_mul(combined_diff.saturating_sub(I32F32::from_num(0.5))),
            )),
        );
        let alpha =
            alpha_low.saturating_add(sigmoid.saturating_mul(alpha_high.saturating_sub(alpha_low)));

        clamp_i32f32(alpha, alpha_low, alpha_high)
    }

    /// `1 - bonds_moving_average/1e6` — constant EMA alpha when liquid alpha is disabled.
    pub fn bonds_moving_average_alpha(netuid: NetUid) -> I32F32 {
        // Retrieve the bonds moving average for the given network ID and scale it down.
        let bonds_moving_average: I64F64 = I64F64::from_num(Self::get_bonds_moving_average(netuid))
            .saturating_div(I64F64::from_num(1_000_000));

        // Calculate the alpha value for the EMA calculation.
        // Alpha is derived by subtracting the scaled bonds moving average from 1.
        let alpha: I32F32 =
            I32F32::from_num(1).saturating_sub(I32F32::from_num(bonds_moving_average));
        alpha
    }

    /// Owner/root setter for liquid-alpha bounds (`AlphaValues`); enforces enabled + range checks.
    pub fn do_set_alpha_values(
        origin: OriginFor<T>,
        netuid: NetUid,
        alpha_low: u16,
        alpha_high: u16,
    ) -> Result<(), DispatchError> {
        Self::ensure_subnet_owner_or_root(origin, netuid)?;

        ensure!(
            Self::get_liquid_alpha_enabled(netuid),
            Error::<T>::LiquidAlphaDisabled
        );

        let max_u16: u32 = u16::MAX as u32; // 65535
        let min_alpha_low: u16 = (max_u16.safe_div(40)) as u16; // 1638
        let min_alpha_high: u16 = min_alpha_low;

        ensure!(alpha_high >= min_alpha_high, Error::<T>::AlphaHighTooLow);

        ensure!(
            alpha_low >= min_alpha_low && alpha_low <= alpha_high,
            Error::<T>::AlphaLowOutOfRange
        );

        AlphaValues::<T>::insert(netuid, (alpha_low, alpha_high));

        log::debug!(
            "AlphaValuesSet( netuid: {netuid:?}, AlphaLow: {alpha_low:?}, AlphaHigh: {alpha_high:?} ) ",
        );
        Ok(())
    }

    /// Zero a hotkey column in `Bonds` when bonds-reset is enabled for the subnet.
    pub fn do_reset_bonds(
        netuid_index: NetUidStorageIndex,
        account_id: &T::AccountId,
    ) -> Result<(), DispatchError> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();

        // check bonds reset enabled for this subnet
        let bonds_reset_enabled: bool = Self::get_bonds_reset(netuid);
        if !bonds_reset_enabled {
            return Ok(());
        }

        if let Ok(uid) = Self::get_uid_for_net_and_hotkey(netuid, account_id) {
            for (i, bonds_vec) in Bonds::<T>::iter_prefix(netuid_index) {
                Bonds::<T>::insert(
                    netuid_index,
                    i,
                    bonds_vec
                        .clone()
                        .iter()
                        .filter(|(j, _)| *j != uid)
                        .collect::<Vec<&(u16, u16)>>(),
                );
            }
            log::debug!("Reset bonds for {account_id:?}, netuid {netuid:?}");
        } else {
            log::warn!(
                "Uid not found for {account_id:?}, netuid {netuid:?} - skipping bonds reset"
            );
        }

        Ok(())
    }

    /// Preflight: `Keys` for `netuid` must not contain duplicate hotkeys.
    pub fn is_epoch_input_state_consistent(netuid: NetUid) -> bool {
        // Check if Keys map has duplicate hotkeys or uids
        let mut hotkey_set: BTreeSet<T::AccountId> = BTreeSet::new();
        // `iter_prefix` over a double map yields (uid, value) for the given first key.
        for (_uid, hotkey) in Keys::<T>::iter_prefix(netuid) {
            if !hotkey_set.insert(hotkey) {
                log::error!("Duplicate hotkeys detected for netuid {netuid}");
                return false;
            }
        }
        true
    }
}
