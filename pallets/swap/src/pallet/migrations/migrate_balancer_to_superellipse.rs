use super::*;
#[cfg(feature = "try-runtime")]
use crate::ExtractedReserves;
use crate::{HasMigrationRun, PalSwapInitialized, SwapBalancer, SwapSuperellipse};
use frame_support::traits::{Get, GetStorageVersion, OnRuntimeUpgrade};
#[cfg(feature = "try-runtime")]
use sp_std::vec::Vec;
use sp_std::{collections::btree_set::BTreeSet, marker::PhantomData};

pub const MIGRATION_NAME: &[u8] = b"migrate_balancer_to_superellipse";

fn candidates<T: Config>() -> BTreeSet<NetUid> {
    PalSwapInitialized::<T>::iter_keys()
        .chain(SwapBalancer::<T>::iter_keys())
        .collect()
}

/// Preserve each old pool's price and initial local sensitivity. Lending funding
/// extracts only unreachable floors in a separate migration stage, preserving
/// active-plus-vault accounting and the baseline curve's trading capacity.
pub fn migrate_balancer_to_superellipse<T: Config>() -> Weight {
    let name = BoundedVec::truncate_from(MIGRATION_NAME.to_vec());
    let mut weight = T::DbWeight::get().reads(1);
    if HasMigrationRun::<T>::get(&name) {
        weight.saturating_accrue(T::DbWeight::get().reads(1));
        if crate::pallet::STORAGE_VERSION > Pallet::<T>::on_chain_storage_version() {
            crate::pallet::STORAGE_VERSION.put::<Pallet<T>>();
            weight.saturating_accrue(T::DbWeight::get().writes(1));
        }
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
        weight.saturating_accrue(T::CurveInitializationWeight::get());
        match crate::pallet::superellipse::Superellipse::from_balancer(x, y, old.get_quote_weight())
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
        crate::pallet::STORAGE_VERSION.put::<Pallet<T>>();
        weight.saturating_accrue(T::DbWeight::get().writes(2));
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
#[subtensor_macros::freeze_struct("b8695b4ded903a8c")]
#[derive(codec::Encode, codec::Decode)]
struct PoolSnapshot {
    netuid: NetUid,
    alpha: u64,
    tao: u64,
    pending_alpha: u64,
    pending_tao: u64,
    old_price: u128,
    extracted_alpha: u64,
    extracted_tao: u64,
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
                extracted_alpha: ExtractedReserves::<T>::get(netuid).0.into(),
                extracted_tao: ExtractedReserves::<T>::get(netuid).1.into(),
                curve_before: SwapSuperellipse::<T>::get(netuid).map(|curve| curve.encode()),
            }
        })
        .collect();
    let name = BoundedVec::truncate_from(MIGRATION_NAME.to_vec());
    Ok((HasMigrationRun::<T>::get(name), pools).encode())
}

#[cfg(feature = "try-runtime")]
pub fn post_upgrade<T: Config>(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
    use codec::Decode;
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
        let (extracted_alpha, extracted_tao) = ExtractedReserves::<T>::get(netuid);
        let take_alpha = u64::from(extracted_alpha)
            .checked_sub(before.extracted_alpha)
            .ok_or("Extracted alpha counter decreased")?;
        let take_tao = u64::from(extracted_tao)
            .checked_sub(before.extracted_tao)
            .ok_or("Extracted TAO counter decreased")?;
        let active_alpha = u64::from(T::AlphaReserve::reserve(netuid));
        let active_tao = u64::from(T::TaoReserve::reserve(netuid));
        ensure!(
            active_alpha.checked_add(take_alpha) == Some(before.alpha),
            "Migration changed active-plus-extracted alpha reserves"
        );
        ensure!(
            active_tao.checked_add(take_tao) == Some(before.tao),
            "Migration changed active-plus-extracted TAO reserves"
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
            let mut expected = crate::pallet::Superellipse::decode(&mut &encoded[..])
                .map_err(|_| "Snapshot ellipse must decode")?;
            let safe = expected
                .extractable_reserves(before.alpha, before.tao, T::MinimumReserve::get().get())
                .map_err(|_| "Cannot certify existing ellipse extraction")?;
            ensure!(
                take_alpha <= safe.0 && take_tao <= safe.1,
                "Extraction exceeds existing unreachable floors"
            );
            expected
                .withdraw_liquidity(take_alpha, take_tao)
                .map_err(|_| "Extracted reserves cannot translate snapshot ellipse")?;
            ensure!(
                curve == expected,
                "Migration changed an existing ellipse beyond extraction"
            );
            continue;
        }
        let mut expected = crate::pallet::superellipse::Superellipse::from_balancer(
            before.alpha,
            before.tao,
            SwapBalancer::<T>::get(netuid).get_quote_weight(),
        )
        .map_err(|_| "Funded pool cannot initialize its baseline curve")?;
        let safe = expected
            .extractable_reserves(before.alpha, before.tao, T::MinimumReserve::get().get())
            .map_err(|_| "Cannot certify extracted reserve floors")?;
        ensure!(
            take_alpha <= safe.0 && take_tao <= safe.1,
            "Extraction exceeds unreachable floors"
        );
        expected
            .withdraw_liquidity(take_alpha, take_tao)
            .map_err(|_| "Extraction cannot translate baseline ellipse")?;
        ensure!(
            curve == expected,
            "Migration changed baseline price sensitivity or trading capacity beyond extraction"
        );
        let price = curve
            .calculate_price(active_alpha, active_tao)
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
