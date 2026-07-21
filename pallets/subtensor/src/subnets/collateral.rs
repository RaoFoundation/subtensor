//! Miner registration collateral.
//!
//! When a subnet sets a nonzero [`CollateralLockShare`] (p), the floating
//! registration price is split: the `(1 - p)` share is burned exactly like a
//! classic burned registration, and the `p` share is staked to the registering
//! hotkey and locked as collateral. The lock is released back to free stake at
//! [`CollateralDrainRatio`] (k) alpha per alpha of miner incentive earned, so
//! the only way to recover the collateral is validated work on that subnet.
//!
//! The lock survives deregistration and is credited against the collateral
//! requirement the next time the same hotkey registers, so a pruned miner
//! re-registers by paying only the burned share (plus any shortfall if the
//! requirement rose). There is no other exit path: collateral is never
//! directly withdrawable, and a hotkey that validators stop scoring keeps its
//! remaining collateral frozen indefinitely.

use safe_math::FixedExt;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::SwapHandler;

use super::*;

impl<T: Config> Pallet<T> {
    /// Collateral lock share (p) for a subnet as a fixed-point fraction in [0, 1).
    pub fn get_collateral_lock_share_float(netuid: NetUid) -> U64F64 {
        U64F64::saturating_from_num(CollateralLockShare::<T>::get(netuid))
            .safe_div(U64F64::saturating_from_num(u16::MAX))
    }

    /// Alpha currently locked as registration collateral for a hotkey on a subnet.
    pub fn get_miner_collateral_locked(netuid: NetUid, hotkey: &T::AccountId) -> AlphaBalance {
        MinerCollateral::<T>::get(netuid, hotkey)
            .map(|state| state.locked)
            .unwrap_or(AlphaBalance::ZERO)
    }

    /// Total alpha locked as registration collateral across all hotkeys owned
    /// by a coldkey on a subnet. Used by the unstake guard.
    pub fn total_miner_collateral_for_coldkey(
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> AlphaBalance {
        OwnedHotkeys::<T>::get(coldkey)
            .into_iter()
            .map(|hotkey| Self::get_miner_collateral_locked(netuid, &hotkey))
            .fold(AlphaBalance::ZERO, |acc, locked| acc.saturating_add(locked))
    }

    /// Ensures removing `amount` alpha from a hotkey leaves that hotkey with
    /// enough stake to still cover its own `MinerCollateral` lock.
    ///
    /// Collateral is keyed by `(netuid, hotkey)` and surfaced per-UID on the
    /// metagraph, so the bonded alpha must stay on that hotkey — covering it
    /// with free stake on a sibling hotkey of the same coldkey is not enough.
    pub fn ensure_hotkey_covers_collateral(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        let stake =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
        let collateral = Self::get_miner_collateral_locked(netuid, hotkey);
        let removable = stake.saturating_sub(collateral);
        ensure!(amount <= removable, Error::<T>::StakeUnavailable);
        Ok(())
    }

    /// Ensures an ownership-changing, same-subnet stake transfer leaves the
    /// origin coldkey with enough alpha to still cover its miner collateral.
    ///
    /// Unlike the general unstake guard, this only accounts for collateral, not
    /// conviction locks: on a same-subnet transfer the conviction lock follows
    /// the stake to the destination via `transfer_lock`, but miner collateral
    /// has no transfer exit and stays on the origin hotkey. Without this,
    /// transferring staked-and-locked alpha to a second coldkey would liberate
    /// collateral that is only meant to be recovered through earned incentive.
    /// Prefer [`ensure_hotkey_covers_collateral`] at call sites that know the
    /// origin hotkey; this coldkey-wide check remains as a belt-and-suspenders
    /// for ownership-changing transfers.
    pub fn ensure_transfer_respects_collateral(
        coldkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        let total = Self::total_coldkey_alpha_on_subnet(coldkey, netuid);
        let collateral = Self::total_miner_collateral_for_coldkey(coldkey, netuid);
        let transferable = total.saturating_sub(collateral);
        ensure!(amount <= transferable, Error::<T>::StakeUnavailable);
        Ok(())
    }

    /// The collateral requirement at registration: `p * registration_cost` in TAO.
    pub fn get_collateral_requirement_tao(
        netuid: NetUid,
        registration_cost: TaoBalance,
    ) -> TaoBalance {
        let lock_share = Self::get_collateral_lock_share_float(netuid);
        TaoBalance::from(
            U64F64::saturating_from_num(u64::from(registration_cost))
                .saturating_mul(lock_share)
                .saturating_to_num::<u64>(),
        )
    }

    /// TAO the coldkey must provide at registration on top of the burned
    /// share: the collateral requirement `p * registration_cost` minus the
    /// TAO value of collateral already locked for this hotkey.
    pub fn get_collateral_topup_tao(
        netuid: NetUid,
        hotkey: &T::AccountId,
        registration_cost: TaoBalance,
    ) -> TaoBalance {
        let requirement_tao: u64 =
            Self::get_collateral_requirement_tao(netuid, registration_cost).into();
        if requirement_tao == 0 {
            return TaoBalance::ZERO;
        }

        // Value the standing lock at the subnet's moving-average price rather
        // than instantaneous spot: a returning miner could otherwise pump spot
        // in the same block to inflate the credit and re-register while
        // under-collateralized. The EMA resists single-block manipulation.
        let locked_alpha = Self::get_miner_collateral_locked(netuid, hotkey);
        let alpha_price: U64F64 = Self::get_moving_alpha_price(netuid);
        let locked_value_tao: u64 = U64F64::saturating_from_num(locked_alpha.to_u64())
            .saturating_mul(alpha_price)
            .saturating_to_num();

        TaoBalance::from(requirement_tao.saturating_sub(locked_value_tao))
    }

    /// Stake `topup_tao` to the registering hotkey and lock the resulting
    /// alpha as registration collateral, re-snapshotting the subnet's drain
    /// ratio for the merged lock.
    ///
    /// Invariant: `coldkey` must own `hotkey`. The unstake guard charges
    /// `MinerCollateral(netuid, hotkey)` against the hotkey's owning coldkey, so
    /// the collateral stake must land on that same coldkey or it would be
    /// unguarded. The only caller, `do_register`, enforces this via
    /// `coldkey_owns_hotkey` before calling here; keep that check ahead of any
    /// future call site.
    pub fn lock_miner_collateral(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        topup_tao: TaoBalance,
    ) -> DispatchResult {
        // Zero top-up: standing collateral already covers the requirement.
        // Still re-snapshot the drain ratio so a returning registration picks
        // up current subnet terms.
        if topup_tao.is_zero() {
            Self::resnapshot_collateral_drain(netuid, hotkey);
            return Ok(());
        }

        // Stake the top-up as locked collateral. `stake_into_subnet` has no
        // DefaultMinStake floor — only a non-zero alpha-out requirement — so
        // registration-sized top-ups (often below the public stake minimum
        // when burn is near MinBurn) still lock correctly. Do not waive them:
        // a MinStake early-return would undercharge first registrations and
        // leave `p > 0` subnets with no bond at all.
        let locked_alpha = match Self::stake_into_subnet(
            hotkey,
            coldkey,
            netuid,
            topup_tao,
            T::SwapInterface::max_price(),
            false,
        ) {
            Ok(alpha) => alpha,
            Err(e) if e == Error::<T>::AmountTooLow.into() => {
                // Dust top-up (returning miner whose standing credit nearly
                // meets the requirement) can swap to zero alpha. Fold it into
                // the burn so registration still pays the full charge and
                // does not revert — the miner cannot top up more; the amount
                // is computed on-chain.
                Self::burn_registration_tao(netuid, coldkey, topup_tao)?;
                Self::resnapshot_collateral_drain(netuid, hotkey);
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        let total_locked =
            MinerCollateral::<T>::mutate(netuid, hotkey, |maybe_state| match maybe_state {
                Some(state) => {
                    state.locked = state.locked.saturating_add(locked_alpha);
                    state.drain_ratio = CollateralDrainRatio::<T>::get(netuid);
                    state.locked
                }
                None => {
                    *maybe_state = Some(MinerCollateralState {
                        locked: locked_alpha,
                        drain_ratio: CollateralDrainRatio::<T>::get(netuid),
                        min_locked: AlphaBalance::ZERO,
                        earned: AlphaBalance::ZERO,
                    });
                    locked_alpha
                }
            });

        Self::deposit_event(Event::CollateralLocked {
            netuid,
            hotkey: hotkey.clone(),
            locked: locked_alpha,
            total_locked,
        });

        Ok(())
    }

    /// Re-snapshot a standing collateral entry's drain ratio to the subnet's
    /// current `CollateralDrainRatio`. No-op when the hotkey has no entry.
    fn resnapshot_collateral_drain(netuid: NetUid, hotkey: &T::AccountId) {
        MinerCollateral::<T>::mutate_exists(netuid, hotkey, |maybe_state| {
            if let Some(state) = maybe_state {
                state.drain_ratio = CollateralDrainRatio::<T>::get(netuid);
            }
        });
    }

    /// Burn `tao` with the same pool mechanics as the registration burned
    /// share (transfer → swap → reduce `SubnetAlphaOut` → recycle counter).
    /// Used when a dust collateral top-up cannot form a stake position.
    fn burn_registration_tao(
        netuid: NetUid,
        coldkey: &T::AccountId,
        tao: TaoBalance,
    ) -> DispatchResult {
        let actual = Self::transfer_tao_to_subnet(netuid, coldkey, tao.into())?;
        let burned_alpha = Self::swap_tao_for_alpha(
            netuid,
            actual,
            T::SwapInterface::max_price(),
            false,
        )?
        .amount_paid_out;
        SubnetAlphaOut::<T>::mutate(netuid, |total| {
            *total = total.saturating_sub(burned_alpha.into())
        });
        Self::increase_rao_recycled(netuid, tao.into());
        Ok(())
    }

    /// Settle a miner's collateral against this tempo's earned incentive.
    /// Called from the incentive distribution path.
    ///
    /// Two directions around the miner-set floor (`min_locked`):
    /// - Below the floor, incentive is captured into the lock until the floor
    ///   is met. The captured share is staked to the miner hotkey itself (the
    ///   guarded position), never to an auto-stake destination.
    /// - Above the floor, `min(drain_ratio * incentive, locked - min_locked)`
    ///   is released back to withdrawable stake.
    ///
    /// Returns the captured amount; the caller credits only the remainder of
    /// the incentive to the miner's usual destination. The entry is removed
    /// once fully drained with no floor set.
    pub fn settle_miner_collateral(
        netuid: NetUid,
        hotkey: &T::AccountId,
        owner: &T::AccountId,
        incentive: AlphaBalance,
    ) -> AlphaBalance {
        if incentive.is_zero() {
            return AlphaBalance::ZERO;
        }
        MinerCollateral::<T>::mutate_exists(netuid, hotkey, |maybe_state| {
            let Some(state) = maybe_state else {
                return AlphaBalance::ZERO;
            };

            state.earned = state.earned.saturating_add(incentive);

            let shortfall = state.min_locked.saturating_sub(state.locked);
            if !shortfall.is_zero() {
                let captured = incentive.min(shortfall);
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey, owner, netuid, captured,
                );
                state.locked = state.locked.saturating_add(captured);
                return captured;
            }

            let release: u64 = U64F64::saturating_from_num(incentive.to_u64())
                .saturating_mul(state.drain_ratio)
                .saturating_to_num();
            let releasable = state.locked.saturating_sub(state.min_locked);
            state.locked = state
                .locked
                .saturating_sub(releasable.min(release.into()));
            if state.locked.is_zero() && state.min_locked.is_zero() {
                *maybe_state = None;
            }
            AlphaBalance::ZERO
        })
    }

    /// Voluntarily stake `tao` to the miner's own hotkey and lock it as
    /// additional registration collateral (e.g. per-machine deposits required
    /// by a subnet's validators). Keeps the existing drain-ratio snapshot: a
    /// top-up is not a new registration and does not re-price the contract.
    pub fn do_add_collateral(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: T::AccountId,
        tao: TaoBalance,
    ) -> dispatch::DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(!netuid.is_root(), Error::<T>::RegistrationNotPermittedOnRootSubnet);
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        // Invariant: the collateral stake must land on the hotkey's owning
        // coldkey or the unstake guard would not cover it.
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );
        ensure!(
            tao >= DefaultMinStake::<T>::get(),
            Error::<T>::AmountTooLow
        );

        let locked_alpha = Self::stake_into_subnet(
            &hotkey,
            &coldkey,
            netuid,
            tao,
            T::SwapInterface::max_price(),
            false,
        )?;

        let total_locked =
            MinerCollateral::<T>::mutate(netuid, &hotkey, |maybe_state| match maybe_state {
                Some(state) => {
                    state.locked = state.locked.saturating_add(locked_alpha);
                    state.locked
                }
                None => {
                    *maybe_state = Some(MinerCollateralState {
                        locked: locked_alpha,
                        drain_ratio: CollateralDrainRatio::<T>::get(netuid),
                        min_locked: AlphaBalance::ZERO,
                        earned: AlphaBalance::ZERO,
                    });
                    locked_alpha
                }
            });

        Self::deposit_event(Event::CollateralLocked {
            netuid,
            hotkey,
            locked: locked_alpha,
            total_locked,
        });

        Ok(())
    }

    /// Set the miner's collateral floor for a hotkey on a subnet. The lock
    /// self-maintains around the floor (drain stops at it; incentive fills a
    /// shortfall), so miners tracking a validator-published per-machine
    /// requirement do not need to keep re-locking drained collateral. Zero
    /// clears the floor.
    pub fn do_set_min_collateral(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: T::AccountId,
        min_locked: AlphaBalance,
    ) -> dispatch::DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(!netuid.is_root(), Error::<T>::RegistrationNotPermittedOnRootSubnet);
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        MinerCollateral::<T>::mutate_exists(netuid, &hotkey, |maybe_state| match maybe_state {
            Some(state) => {
                state.min_locked = min_locked;
                if state.locked.is_zero() && state.min_locked.is_zero() {
                    *maybe_state = None;
                }
            }
            None => {
                if !min_locked.is_zero() {
                    *maybe_state = Some(MinerCollateralState {
                        locked: AlphaBalance::ZERO,
                        drain_ratio: CollateralDrainRatio::<T>::get(netuid),
                        min_locked,
                        earned: AlphaBalance::ZERO,
                    });
                }
            }
        });

        Self::deposit_event(Event::MinCollateralSet {
            netuid,
            hotkey,
            min_locked,
        });

        Ok(())
    }

    /// Move the collateral entry when a hotkey is swapped. If the new hotkey
    /// already has collateral on the subnet, the locks merge, the smaller
    /// (slower) drain ratio is kept, and the floors add (they represent
    /// distinct per-machine commitments).
    pub fn swap_miner_collateral(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        netuid: NetUid,
    ) {
        let Some(old_state) = MinerCollateral::<T>::take(netuid, old_hotkey) else {
            return;
        };
        MinerCollateral::<T>::mutate(netuid, new_hotkey, |maybe_state| match maybe_state {
            Some(state) => {
                state.locked = state.locked.saturating_add(old_state.locked);
                state.drain_ratio = state.drain_ratio.min(old_state.drain_ratio);
                state.min_locked = state.min_locked.saturating_add(old_state.min_locked);
                state.earned = state.earned.saturating_add(old_state.earned);
            }
            None => *maybe_state = Some(old_state),
        });
    }
}
