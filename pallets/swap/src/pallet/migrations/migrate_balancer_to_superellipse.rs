use super::*;
use crate::{HasMigrationRun, PalSwapInitialized, SwapBalancer, SwapSuperellipse};
use frame_support::traits::{Get, OnRuntimeUpgrade};
#[cfg(feature = "try-runtime")]
use sp_std::vec::Vec;
use sp_std::{collections::btree_set::BTreeSet, marker::PhantomData};

pub const MIGRATION_NAME: &[u8] = b"migrate_balancer_to_superellipse";

fn candidates<T: Config>() -> BTreeSet<NetUid> {
    PalSwapInitialized::<T>::iter_keys()
        .chain(SwapBalancer::<T>::iter_keys())
        .collect()
}

/// Anchor the translated ellipse at the old pool's price and local sensitivity.
/// No balances or pending protocol liquidity are moved by this migration.
pub fn migrate_balancer_to_superellipse<T: Config>() -> Weight {
    let name = BoundedVec::truncate_from(MIGRATION_NAME.to_vec());
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&name) {
        return weight;
    }
    let netuids = candidates::<T>();
    // Each union member may occur in both maps. Also charge the terminating
    // read of each prefix iterator, including when the maps are empty.
    let scan_reads = (netuids.len() as u64).saturating_mul(2).saturating_add(2);
    weight.saturating_accrue(T::DbWeight::get().reads(scan_reads));
    let mut complete = true;
    for netuid in netuids {
        weight.saturating_accrue(T::DbWeight::get().reads(2));
        if !T::SubnetInfo::exists(netuid) || T::SubnetInfo::mechanism(netuid) != 1 {
            continue;
        }
        weight.saturating_accrue(T::DbWeight::get().reads(3));
        if SwapSuperellipse::<T>::contains_key(netuid) {
            continue;
        }
        let x = T::AlphaReserve::reserve(netuid).into();
        let y = T::TaoReserve::reserve(netuid).into();
        if x == 0 || y == 0 {
            // Degenerate pools initialize when both reserves become funded.
            continue;
        }
        weight.saturating_accrue(T::DbWeight::get().reads(1));
        let old = SwapBalancer::<T>::get(netuid);
        match crate::pallet::superellipse::Superellipse::from_balancer(x, y, old.get_quote_weight())
            .and_then(|curve| curve.calculate_price(x, y).map(|_| curve))
        {
            Ok(curve) => {
                SwapSuperellipse::<T>::insert(netuid, curve);
                weight.saturating_accrue(T::DbWeight::get().writes(1));
                weight.saturating_accrue(T::DbWeight::get().reads(1));
                if !PalSwapInitialized::<T>::get(netuid) {
                    PalSwapInitialized::<T>::insert(netuid, true);
                    weight.saturating_accrue(T::DbWeight::get().writes(1));
                }
            }
            Err(error) => {
                complete = false;
                log::error!(
                    "Superellipse migration failed for subnet {netuid}: {error:?}; preserving old parameters for retry"
                );
            }
        }
    }
    if complete {
        HasMigrationRun::<T>::insert(name, true);
        weight.saturating_accrue(T::DbWeight::get().writes(1));
    }
    weight
}

/// Runtime migration wrapper, including snapshot validation for try-runtime.
pub struct Migration<T: Config>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for Migration<T> {
    fn on_runtime_upgrade() -> Weight {
        migrate_balancer_to_superellipse::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
        pre_upgrade::<T>()
    }

    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
        post_upgrade::<T>(state)
    }
}

#[cfg(feature = "try-runtime")]
#[subtensor_macros::freeze_struct("e81e669c8fa67a1")]
#[derive(codec::Encode, codec::Decode)]
struct PoolSnapshot {
    netuid: NetUid,
    alpha: u64,
    tao: u64,
    pending_alpha: u64,
    pending_tao: u64,
    old_price: u128,
    curve_before: Option<Vec<u8>>,
}

#[cfg(feature = "try-runtime")]
pub fn pre_upgrade<T: Config>() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
    use codec::Encode;
    let pools: Vec<_> = candidates::<T>()
        .into_iter()
        .filter(|netuid| T::SubnetInfo::exists(*netuid) && T::SubnetInfo::mechanism(*netuid) == 1)
        .map(|netuid| {
            let alpha = T::AlphaReserve::reserve(netuid).into();
            let tao = T::TaoReserve::reserve(netuid).into();
            PoolSnapshot {
                netuid,
                alpha,
                tao,
                pending_alpha: BalancerAlphaReservoir::<T>::get(netuid).into(),
                pending_tao: BalancerTaoReservoir::<T>::get(netuid).into(),
                old_price: SwapBalancer::<T>::get(netuid)
                    .calculate_price(alpha, tao)
                    .to_bits(),
                curve_before: SwapSuperellipse::<T>::get(netuid).map(|curve| curve.encode()),
            }
        })
        .collect();
    let name = BoundedVec::truncate_from(MIGRATION_NAME.to_vec());
    Ok((HasMigrationRun::<T>::get(name), pools).encode())
}

#[cfg(feature = "try-runtime")]
pub fn post_upgrade<T: Config>(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
    use codec::{Decode, Encode};
    use frame_support::ensure;
    use sp_core::U256;
    let (already_ran, pools) = <(bool, Vec<PoolSnapshot>)>::decode(&mut &state[..])
        .map_err(|_| "Superellipse migration snapshot must decode")?;
    let name = BoundedVec::truncate_from(MIGRATION_NAME.to_vec());
    ensure!(
        HasMigrationRun::<T>::get(name),
        "Superellipse migration must complete"
    );
    for before in pools {
        let netuid = before.netuid;
        ensure!(
            u64::from(T::AlphaReserve::reserve(netuid)) == before.alpha,
            "Migration changed alpha reserves"
        );
        ensure!(
            u64::from(T::TaoReserve::reserve(netuid)) == before.tao,
            "Migration changed TAO reserves"
        );
        ensure!(
            u64::from(BalancerAlphaReservoir::<T>::get(netuid)) == before.pending_alpha,
            "Migration changed pending alpha"
        );
        ensure!(
            u64::from(BalancerTaoReservoir::<T>::get(netuid)) == before.pending_tao,
            "Migration changed pending TAO"
        );
        if before.alpha == 0 || before.tao == 0 {
            continue;
        }
        if already_ran && before.curve_before.is_none() {
            // A funded pool awaiting lazy initialization is outside this already
            // completed migration. A repeated upgrade must not create its curve.
            ensure!(
                !SwapSuperellipse::<T>::contains_key(netuid),
                "Repeated migration initialized a previously absent ellipse"
            );
            continue;
        }
        let curve = SwapSuperellipse::<T>::get(netuid).ok_or("Funded pool has no ellipse")?;
        if let Some(encoded) = before.curve_before {
            ensure!(
                curve.encode() == encoded,
                "Migration changed an existing ellipse"
            );
            continue;
        }
        let expected = crate::pallet::superellipse::Superellipse::from_balancer(
            before.alpha,
            before.tao,
            SwapBalancer::<T>::get(netuid).get_quote_weight(),
        )
        .map_err(|_| "Funded pool cannot be calibrated")?;
        ensure!(
            curve == expected,
            "Migration did not match local sensitivity calibration"
        );
        let price = curve
            .calculate_price(before.alpha, before.tao)
            .map_err(|_| "Migrated ellipse has invalid price")?
            .to_bits();
        // Q32 parameters permit a small relative quantization error for tiny pools.
        let error = U256::from(price.abs_diff(before.old_price));
        let tolerance = U256::from(before.old_price)
            .checked_div(U256::from(10_000_000u64))
            .ok_or("Invalid price tolerance divisor")?
            .saturating_add(U256::from(16u64));
        ensure!(
            error <= tolerance,
            "Migration changed spot price beyond rounding tolerance"
        );
    }
    Ok(())
}
