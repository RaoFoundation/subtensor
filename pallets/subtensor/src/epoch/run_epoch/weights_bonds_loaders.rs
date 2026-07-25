//! Load unnormalized weights/bonds from storage and convert kappa/rho/bonds_penalty to fixed-point.

use super::*;
use crate::epoch::math::*;
use safe_math::*;
use sp_std::vec;
use sp_std::vec::Vec;
use substrate_fixed::types::I32F32;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex};

impl<T: Config> Pallet<T> {
    /// Subnet `rho` hyperparameter as `I32F32` (consensus sigmoid steepness).
    pub fn rho_as_i32f32(netuid: NetUid) -> I32F32 {
        I32F32::saturating_from_num(Self::get_rho(netuid))
    }
    /// Subnet `kappa` as a `0..=1` proportion (`storage / u16::MAX`).
    pub fn kappa_proportion_as_i32f32(netuid: NetUid) -> I32F32 {
        I32F32::saturating_from_num(Self::get_kappa(netuid))
            .safe_div(I32F32::saturating_from_num(u16::MAX))
    }
    /// Bonds penalty hyperparameter as a `0..=1` proportion.
    pub fn bonds_penalty_proportion_as_i32f32(netuid: NetUid) -> I32F32 {
        I32F32::saturating_from_num(Self::get_bonds_penalty(netuid))
            .safe_div(I32F32::saturating_from_num(u16::MAX))
    }

    /// Per-uid registration block for outdated-weight masking (`0` if the uid slot is empty).
    pub fn neuron_block_at_registration(netuid: NetUid) -> Vec<u64> {
        let n = Self::get_subnetwork_n(netuid);
        let block_at_registration: Vec<u64> = (0..n)
            .map(|neuron_uid| {
                if Keys::<T>::contains_key(netuid, neuron_uid) {
                    Self::get_neuron_block_at_registration(netuid, neuron_uid)
                } else {
                    0
                }
            })
            .collect();
        block_at_registration
    }

    /// Output unnormalized sparse weights, input weights are assumed to be row max-upscaled in u16.
    pub fn get_weights_sparse(netuid_index: NetUidStorageIndex) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n = Self::get_subnetwork_n(netuid) as usize;
        let mut weights: Vec<Vec<(u16, I32F32)>> = vec![vec![]; n];
        for (uid_i, weights_i) in
            Weights::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, weight_ij) in weights_i.iter().filter(|(uid_j, _)| *uid_j < n as u16) {
                if let Some(row) = weights.get_mut(uid_i as usize) {
                    row.push((*uid_j, I32F32::saturating_from_num(*weight_ij)));
                } else {
                    log::error!("math error: uid_i {uid_i:?} is filtered to be less than n");
                }
            }
        }
        weights
    }

    /// Output unnormalized weights in [n, n] matrix, input weights are assumed to be row max-upscaled in u16.
    pub fn get_weights(netuid_index: NetUidStorageIndex) -> Vec<Vec<I32F32>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n = Self::get_subnetwork_n(netuid) as usize;
        let mut weights: Vec<Vec<I32F32>> = vec![vec![I32F32::saturating_from_num(0.0); n]; n];
        for (uid_i, weights_vec) in
            Weights::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, weight_ij) in weights_vec
                .into_iter()
                .filter(|(uid_j, _)| *uid_j < n as u16)
            {
                if let Some(cell) = weights
                    .get_mut(uid_i as usize)
                    .and_then(|row| row.get_mut(uid_j as usize))
                {
                    *cell = I32F32::saturating_from_num(weight_ij);
                }
            }
        }
        weights
    }

    /// Output unnormalized sparse bonds, input bonds are assumed to be column max-upscaled in u16.
    /// Sparse bonds from storage as `I32F32` (column max-upscaled u16 input; not row-normalized).
    pub fn unnormalized_bonds_sparse(netuid_index: NetUidStorageIndex) -> Vec<Vec<(u16, I32F32)>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n = Self::get_subnetwork_n(netuid) as usize;
        let mut bonds: Vec<Vec<(u16, I32F32)>> = vec![vec![]; n];
        for (uid_i, bonds_vec) in
            Bonds::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, bonds_ij) in bonds_vec {
                if let Some(row) = bonds.get_mut(uid_i as usize) {
                    row.push((uid_j, u16_to_fixed(bonds_ij)));
                } else {
                    // If the index is unexpectedly out of bounds, skip and log math error
                    log::error!(
                        "math error: bonds row index out of bounds (uid_i={uid_i}, n={n}, netuid_index={netuid_index})",
                    );
                }
            }
        }

        bonds
    }

    /// Output unnormalized bonds in [n, n] matrix, input bonds are assumed to be column max-upscaled in u16.
    pub fn get_bonds(netuid_index: NetUidStorageIndex) -> Vec<Vec<I32F32>> {
        let (netuid, _) = Self::get_netuid_and_subid(netuid_index).unwrap_or_default();
        let n: usize = Self::get_subnetwork_n(netuid) as usize;
        let mut bonds: Vec<Vec<I32F32>> = vec![vec![I32F32::saturating_from_num(0.0); n]; n];
        for (uid_i, bonds_vec) in
            Bonds::<T>::iter_prefix(netuid_index).filter(|(uid_i, _)| *uid_i < n as u16)
        {
            for (uid_j, bonds_ij) in bonds_vec.into_iter().filter(|(uid_j, _)| *uid_j < n as u16) {
                if let Some(row) = bonds.get_mut(uid_i as usize) {
                    if let Some(cell) = row.get_mut(uid_j as usize) {
                        *cell = u16_to_fixed(bonds_ij);
                    } else {
                        log::error!(
                            "math error: uid_j index out of bounds (uid_i={uid_i}, uid_j={uid_j}, n={n}, netuid_index={netuid_index})"
                        );
                    }
                } else {
                    log::error!(
                        "math error: uid_i row index out of bounds (uid_i={uid_i}, n={n}, netuid_index={netuid_index})"
                    );
                }
            }
        }

        bonds
    }

    pub fn get_bonds_fixed_proportion(netuid: NetUidStorageIndex) -> Vec<Vec<I32F32>> {
        let mut bonds = Self::get_bonds(netuid);
        bonds.iter_mut().for_each(|bonds_row| {
            bonds_row
                .iter_mut()
                .for_each(|bond| *bond = i32f32_as_u16_proportion(*bond));
        });
        bonds
    }

    pub fn bonds_sparse_as_u16_proportion(netuid: NetUidStorageIndex) -> Vec<Vec<(u16, I32F32)>> {
        let mut bonds = Self::unnormalized_bonds_sparse(netuid);
        bonds.iter_mut().for_each(|bonds_row| {
            bonds_row
                .iter_mut()
                .for_each(|(_, bond)| *bond = i32f32_as_u16_proportion(*bond));
        });
        bonds
    }
}
