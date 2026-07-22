//! Miner registration collateral.
//!
//! When a subnet sets a nonzero [`CollateralLockShare`] (p), the floating
//! registration price is split: the `(1 - p)` share is burned exactly like a
//! classic burned registration, and the `p` share is staked to the registering
//! hotkey and locked as collateral. The lock is released back to free stake at
//! [`CollateralDrainRatio`] (k) alpha per alpha of hotkey emission earned
//! (miner incentive and validator dividends), so the only way to recover the
//! collateral is to earn emission on that subnet.
//!
//! Collateral is keyed by `(netuid, hotkey, coldkey)` — the bonded stake
//! position — so nominators on the same hotkey are never frozen by the owner's
//! bond. The lock survives deregistration and is credited against the
//! collateral requirement the next time the same `(hotkey, coldkey)` registers,
//! so a pruned miner re-registers by paying only the burned share (plus any
//! shortfall if the requirement rose). There is no other exit path: collateral
//! is never directly withdrawable, and a position that stops earning emission
//! keeps its remaining collateral frozen indefinitely.

use frame_support::storage::{TransactionOutcome, with_transaction};
use safe_math::FixedExt;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AuthorshipInfo, NetUid};
use subtensor_swap_interface::SwapHandler;

use super::*;

impl<T: Config> Pallet<T> {
    /// Collateral lock share (p) for a subnet as a fixed-point fraction in [0, 1).
    pub fn get_collateral_lock_share_float(netuid: NetUid) -> U64F64 {
        U64F64::saturating_from_num(CollateralLockShare::<T>::get(netuid))
            .safe_div(U64F64::saturating_from_num(u16::MAX))
    }

    /// Alpha currently locked as registration collateral on a
    /// `(hotkey, coldkey)` stake position.
    pub fn get_miner_collateral_locked(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> AlphaBalance {
        MinerCollateral::<T>::get((netuid, hotkey, coldkey))
            .map(|state| state.locked)
            .unwrap_or(AlphaBalance::ZERO)
    }

    /// Whether `(hotkey, coldkey)` has standing miner collateral on `netuid`
    /// (or any subnet when `netuid` is `None`).
    pub fn has_miner_collateral(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: Option<NetUid>,
    ) -> bool {
        match netuid {
            Some(netuid) => MinerCollateral::<T>::contains_key((netuid, hotkey, coldkey)),
            None => Self::get_all_subnet_netuids()
                .into_iter()
                .any(|netuid| MinerCollateral::<T>::contains_key((netuid, hotkey, coldkey))),
        }
    }

    /// Total alpha locked as registration collateral across all hotkeys owned
    /// by a coldkey on a subnet. Used by the unstake guard.
    pub fn total_miner_collateral_for_coldkey(
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> AlphaBalance {
        ColdkeyMinerCollateral::<T>::get(netuid, coldkey)
    }

    /// Keep [`ColdkeyMinerCollateral`] in sync when a position's locked amount
    /// changes.
    fn adjust_coldkey_miner_collateral(
        coldkey: &T::AccountId,
        netuid: NetUid,
        old_locked: AlphaBalance,
        new_locked: AlphaBalance,
    ) {
        if old_locked == new_locked {
            return;
        }
        ColdkeyMinerCollateral::<T>::mutate(netuid, coldkey, |total| {
            *total = total.saturating_sub(old_locked).saturating_add(new_locked);
        });
        if ColdkeyMinerCollateral::<T>::get(netuid, coldkey).is_zero() {
            ColdkeyMinerCollateral::<T>::remove(netuid, coldkey);
        }
    }

    /// Record `hotkey` in [`ColdkeyCollateralHotkeys`] when a collateral row
    /// appears for `(netuid, coldkey)`.
    fn index_coldkey_collateral_hotkey(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> Result<(), Error<T>> {
        ColdkeyCollateralHotkeys::<T>::try_mutate(netuid, coldkey, |hotkeys| {
            if hotkeys.contains(hotkey) {
                return Ok(());
            }
            hotkeys
                .try_push(hotkey.clone())
                .map_err(|_| Error::<T>::ColdkeyCollateralPositionsFull)
        })
    }

    /// Drop `hotkey` from [`ColdkeyCollateralHotkeys`] when its collateral row
    /// is removed.
    fn deindex_coldkey_collateral_hotkey(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) {
        let mut hotkeys = ColdkeyCollateralHotkeys::<T>::get(netuid, coldkey);
        let before = hotkeys.len();
        hotkeys.retain(|h| h != hotkey);
        if hotkeys.len() == before {
            return;
        }
        if hotkeys.is_empty() {
            ColdkeyCollateralHotkeys::<T>::remove(netuid, coldkey);
        } else {
            ColdkeyCollateralHotkeys::<T>::insert(netuid, coldkey, hotkeys);
        }
    }

    /// Keep [`ColdkeyCollateralHotkeys`] aligned with whether a
    /// [`MinerCollateral`] row exists for `(netuid, hotkey, coldkey)`.
    fn sync_coldkey_collateral_hotkey_index(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Result<(), Error<T>> {
        if MinerCollateral::<T>::contains_key((netuid, hotkey, coldkey)) {
            Self::index_coldkey_collateral_hotkey(netuid, coldkey, hotkey)
        } else {
            Self::deindex_coldkey_collateral_hotkey(netuid, coldkey, hotkey);
            Ok(())
        }
    }

    /// Alpha that can leave this `(coldkey, hotkey, netuid)` position without
    /// violating that position's miner collateral or the coldkey's conviction
    /// lock.
    ///
    /// Used by alpha fee withdrawal and as the free balance under
    /// [`Self::ensure_hotkey_covers_collateral`].
    pub fn available_to_unstake_from_hotkey(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
    ) -> AlphaBalance {
        let stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
        let collateral = Self::get_miner_collateral_locked(netuid, hotkey, coldkey);
        let position_free = stake.saturating_sub(collateral);
        position_free.min(Self::available_to_unstake(coldkey, netuid))
    }

    /// Ensures removing `amount` alpha from a `(hotkey, coldkey)` position
    /// leaves enough stake to still cover that position's `MinerCollateral`.
    ///
    /// Collateral is keyed by `(netuid, hotkey, coldkey)`, so a nominator's
    /// stake on the same hotkey is unaffected by the owner's bond.
    pub fn ensure_hotkey_covers_collateral(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> Result<(), Error<T>> {
        let stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);
        let collateral = Self::get_miner_collateral_locked(netuid, hotkey, coldkey);
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
    /// has no transfer exit and stays on the origin `(hotkey, coldkey)`.
    /// Without this, transferring staked-and-locked alpha to a second coldkey
    /// would liberate collateral that is only meant to be recovered through
    /// earned emission. Prefer [`ensure_hotkey_covers_collateral`] at call
    /// sites that know the origin hotkey; this coldkey-wide check remains as a
    /// belt-and-suspenders for ownership-changing transfers.
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
    /// TAO value of collateral already locked for this `(hotkey, coldkey)`.
    pub fn get_collateral_topup_tao(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
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
        let locked_alpha = Self::get_miner_collateral_locked(netuid, hotkey, coldkey);
        let alpha_price: U64F64 = Self::get_moving_alpha_price(netuid);
        let locked_value_tao: u64 = U64F64::saturating_from_num(locked_alpha.to_u64())
            .saturating_mul(alpha_price)
            .saturating_to_num();

        TaoBalance::from(requirement_tao.saturating_sub(locked_value_tao))
    }

    /// Worst alpha price (RAO per alpha) accepted for a collateral AMM buy.
    ///
    /// Spot × (1 + 5%). Callers that already took a user-supplied limit (e.g.
    /// `add_collateral`) should pass that through instead; this bound is for
    /// registration paths that have no separate AMM limit argument.
    pub fn collateral_purchase_limit_price(netuid: NetUid) -> Result<TaoBalance, DispatchError> {
        let spot = T::SwapInterface::current_alpha_price(netuid);
        ensure!(
            spot > U64F64::saturating_from_num(0),
            Error::<T>::InsufficientLiquidity
        );
        // 5% above spot, in RAO-per-alpha (same units as `add_stake_limit`).
        let limited = spot
            .saturating_mul(U64F64::saturating_from_num(105))
            .safe_div(U64F64::saturating_from_num(100));
        let as_rao = limited
            .saturating_mul(U64F64::saturating_from_num(1_000_000_000u64))
            .saturating_to_num::<u64>();
        ensure!(as_rao > 0, Error::<T>::InsufficientLiquidity);
        Ok(TaoBalance::from(as_rao))
    }

    /// Pay the registration charge as one transfer + one swap, then split the
    /// resulting alpha by the TAO weights of the burned share vs collateral
    /// top-up.
    ///
    /// Dust that swaps to zero alpha is treated as fully burned — there is no
    /// second transfer. A zero top-up (standing collateral already covers the
    /// requirement) still re-snapshots the drain ratio.
    ///
    /// Callers that also mutate registration state should wrap this in
    /// `with_transaction` so a later failure rolls the payment back.
    pub fn pay_registration(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        burned_share: TaoBalance,
        collateral_topup: TaoBalance,
    ) -> DispatchResult {
        let total_charge = burned_share.saturating_add(collateral_topup);
        if total_charge.is_zero() {
            Self::resnapshot_collateral_drain(netuid, hotkey, coldkey);
            return Ok(());
        }

        let tao_paid = Self::transfer_tao_to_subnet(netuid, coldkey, total_charge)?;
        // `transfer_tao_to_subnet` clips to keep-alive; never accept a short fill.
        ensure!(
            tao_paid == total_charge,
            Error::<T>::NotEnoughBalanceToStake
        );
        // Bound the AMM fill whenever any of the charge is collateral. A naked
        // `max_price()` lets a delayed/shielded inclusion clear at an
        // arbitrarily worse rate; burn-only registrations keep the historical
        // unbounded path (the burn share is destroyed, not kept as a position).
        let limit_price = if collateral_topup.is_zero() {
            T::SwapInterface::max_price()
        } else {
            Self::collateral_purchase_limit_price(netuid)?
        };
        let swap_result = Self::swap_tao_for_alpha(netuid, tao_paid, limit_price, false)?;

        // Fee to block author (same as `stake_into_subnet`).
        let maybe_block_author_coldkey = T::AuthorshipProvider::author();
        if let Some(block_author_coldkey) = maybe_block_author_coldkey {
            Self::transfer_tao_from_subnet(
                netuid,
                &block_author_coldkey,
                swap_result.fee_to_block_author.into(),
            )?;
        } else if let Some(subnet_account_id) = Self::get_subnet_account_id(netuid) {
            let _ = Self::burn_tao(&subnet_account_id, swap_result.fee_to_block_author.into());
        }

        let consumed_tao = swap_result
            .amount_paid_in
            .saturating_add(swap_result.fee_paid);
        let refund_tao = tao_paid.saturating_sub(consumed_tao);
        if !refund_tao.is_zero() {
            Self::transfer_tao_from_subnet(netuid, coldkey, refund_tao)?;
            TotalStake::<T>::mutate(|total| *total = total.saturating_sub(refund_tao));
        }

        Self::record_tao_inflow(netuid, swap_result.amount_paid_in.into());

        let total_alpha: AlphaBalance = swap_result.amount_paid_out.into();
        if total_alpha.is_zero() {
            // Dust: payment already settled via the single swap; nothing to
            // stake or remove from AlphaOut.
            Self::resnapshot_collateral_drain(netuid, hotkey, coldkey);
            return Ok(());
        }

        let burn_w = u64::from(burned_share) as u128;
        let lock_w = u64::from(collateral_topup) as u128;
        let total_w = burn_w.saturating_add(lock_w).max(1);
        let lock_alpha = if lock_w == 0 {
            AlphaBalance::ZERO
        } else if burn_w == 0 {
            total_alpha
        } else {
            AlphaBalance::from(
                (total_alpha.to_u64() as u128)
                    .saturating_mul(lock_w)
                    .checked_div(total_w)
                    .unwrap_or(0) as u64,
            )
        };
        let burn_alpha = total_alpha.saturating_sub(lock_alpha);

        if !burn_alpha.is_zero() {
            SubnetAlphaOut::<T>::mutate(netuid, |total| {
                *total = total.saturating_sub(burn_alpha.into())
            });
        }

        if lock_alpha.is_zero() {
            Self::resnapshot_collateral_drain(netuid, hotkey, coldkey);
            return Ok(());
        }

        ensure!(
            Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, netuid, lock_alpha,),
            Error::<T>::InsufficientLiquidity
        );

        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, lock_alpha);

        let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
        if !staking_hotkeys.contains(hotkey) {
            staking_hotkeys.push(hotkey.clone());
            StakingHotkeys::<T>::insert(coldkey, staking_hotkeys);
        }

        Self::cleanup_lock_if_zero(coldkey, netuid);
        LastColdkeyHotkeyStakeBlock::<T>::insert(coldkey, hotkey, Self::get_current_block_as_u64());

        let lock_tao = if total_w == 0 {
            TaoBalance::ZERO
        } else {
            TaoBalance::from(
                (u64::from(tao_paid) as u128)
                    .saturating_mul(lock_w)
                    .checked_div(total_w)
                    .unwrap_or(0) as u64,
            )
        };
        Self::deposit_event(Event::StakeAdded(
            coldkey.clone(),
            hotkey.clone(),
            lock_tao,
            lock_alpha,
            netuid,
            swap_result.fee_paid.to_u64(),
        ));

        let total_locked = Self::credit_miner_collateral(
            netuid, hotkey, coldkey, lock_alpha,
            true, // re-snapshot drain ratio on registration
        )?;

        Self::deposit_event(Event::CollateralLocked {
            netuid,
            hotkey: hotkey.clone(),
            locked: lock_alpha,
            total_locked,
        });

        Ok(())
    }

    /// Credit `alpha` onto `(netuid, hotkey, coldkey)` collateral. When
    /// `resnapshot_drain` is set, refresh the drain-ratio snapshot (registration
    /// / returning registration). Voluntary top-ups leave the ratio alone.
    fn credit_miner_collateral(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        alpha: AlphaBalance,
        resnapshot_drain: bool,
    ) -> Result<AlphaBalance, Error<T>> {
        // Reserve the index slot before writing the row so a full cap fails
        // closed with no storage side effects.
        if !MinerCollateral::<T>::contains_key((netuid, hotkey, coldkey)) {
            Self::index_coldkey_collateral_hotkey(netuid, coldkey, hotkey)?;
        }
        let old_locked = Self::get_miner_collateral_locked(netuid, hotkey, coldkey);
        let new_locked =
            MinerCollateral::<T>::mutate(
                (netuid, hotkey, coldkey),
                |maybe_state| match maybe_state {
                    Some(state) => {
                        state.locked = state.locked.saturating_add(alpha);
                        if resnapshot_drain {
                            state.drain_ratio = CollateralDrainRatio::<T>::get(netuid);
                        }
                        state.locked
                    }
                    None => {
                        *maybe_state = Some(MinerCollateralState {
                            locked: alpha,
                            drain_ratio: CollateralDrainRatio::<T>::get(netuid),
                            min_locked: AlphaBalance::ZERO,
                            earned: AlphaBalance::ZERO,
                        });
                        alpha
                    }
                },
            );
        Self::adjust_coldkey_miner_collateral(coldkey, netuid, old_locked, new_locked);
        Ok(new_locked)
    }

    /// Re-snapshot a standing collateral entry's drain ratio to the subnet's
    /// current `CollateralDrainRatio`. No-op when the position has no entry.
    fn resnapshot_collateral_drain(netuid: NetUid, hotkey: &T::AccountId, coldkey: &T::AccountId) {
        MinerCollateral::<T>::mutate_exists((netuid, hotkey, coldkey), |maybe_state| {
            if let Some(state) = maybe_state {
                state.drain_ratio = CollateralDrainRatio::<T>::get(netuid);
            }
        });
    }

    /// Settle a hotkey's collateral against this tempo's earned emission
    /// (miner incentive or validator dividends). Called from the emission
    /// distribution path.
    ///
    /// `emission` drives lifetime earned and the release rate
    /// (`k × emission`). `capturable` is the maximum that may be diverted
    /// into the lock when below the floor — it must be value that already
    /// belongs to `owner` (full miner incentive, or only the validator's
    /// take from a dividend pool). Nominator / root-claimable shares must
    /// never be passed as capturable.
    ///
    /// Two directions around the miner-set floor (`min_locked`):
    /// - Below the floor, up to `min(capturable, shortfall)` is captured into
    ///   the lock (staked to the hotkey itself, never an auto-stake
    ///   destination).
    /// - Above the floor, `min(drain_ratio * emission, locked - min_locked)`
    ///   is released back to withdrawable stake.
    ///
    /// Returns the captured amount; the caller credits only the remainder of
    /// the capturable slice to the owner. The entry is removed once fully
    /// drained with no floor set.
    pub fn settle_miner_collateral(
        netuid: NetUid,
        hotkey: &T::AccountId,
        owner: &T::AccountId,
        emission: AlphaBalance,
        capturable: AlphaBalance,
    ) -> AlphaBalance {
        if emission.is_zero() {
            return AlphaBalance::ZERO;
        }
        let old_locked = Self::get_miner_collateral_locked(netuid, hotkey, owner);
        let captured =
            MinerCollateral::<T>::mutate_exists((netuid, hotkey, owner), |maybe_state| {
                let Some(state) = maybe_state else {
                    return AlphaBalance::ZERO;
                };

                state.earned = state.earned.saturating_add(emission);

                let shortfall = state.min_locked.saturating_sub(state.locked);
                if !shortfall.is_zero() {
                    let captured = capturable.min(shortfall);
                    if captured.is_zero() {
                        return AlphaBalance::ZERO;
                    }
                    Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey, owner, netuid, captured,
                    );
                    state.locked = state.locked.saturating_add(captured);
                    return captured;
                }

                let release: u64 = U64F64::saturating_from_num(emission.to_u64())
                    .saturating_mul(state.drain_ratio)
                    .saturating_to_num();
                let releasable = state.locked.saturating_sub(state.min_locked);
                state.locked = state.locked.saturating_sub(releasable.min(release.into()));
                if state.locked.is_zero() && state.min_locked.is_zero() {
                    *maybe_state = None;
                }
                AlphaBalance::ZERO
            });
        let new_locked = Self::get_miner_collateral_locked(netuid, hotkey, owner);
        Self::adjust_coldkey_miner_collateral(owner, netuid, old_locked, new_locked);
        // Settle only updates / removes existing rows; index shrink cannot fail.
        let _ = Self::sync_coldkey_collateral_hotkey_index(netuid, hotkey, owner);
        captured
    }

    /// Lock `alpha` of additional registration collateral on the signer's
    /// own hotkey (e.g. per-machine deposits required by a subnet's
    /// validators).
    ///
    /// Prefers free alpha already staked on `(hotkey, coldkey, netuid)` and
    /// only buys the shortfall with TAO (priced at the subnet moving-average
    /// price). Keeps the existing drain-ratio snapshot: a top-up is not a new
    /// registration and does not re-price the contract. The buy leg is
    /// fill-or-kill against `limit_price` (same units as `add_stake_limit`),
    /// and the whole path is transactional.
    pub fn do_add_collateral(
        origin: OriginFor<T>,
        netuid: NetUid,
        hotkey: T::AccountId,
        alpha: AlphaBalance,
        limit_price: TaoBalance,
    ) -> dispatch::DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );
        ensure!(!alpha.is_zero(), Error::<T>::AmountTooLow);

        let stake = Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let already_locked = Self::get_miner_collateral_locked(netuid, &hotkey, &coldkey);
        let free_alpha = stake.saturating_sub(already_locked);
        let from_stake = free_alpha.min(alpha);
        let shortfall_alpha = alpha.saturating_sub(from_stake);

        let tao_to_buy = if shortfall_alpha.is_zero() {
            TaoBalance::ZERO
        } else {
            let alpha_price = Self::get_moving_alpha_price(netuid);
            ensure!(
                alpha_price > U64F64::saturating_from_num(0),
                Error::<T>::InsufficientLiquidity
            );
            TaoBalance::from(
                U64F64::saturating_from_num(shortfall_alpha.to_u64())
                    .saturating_mul(alpha_price)
                    .saturating_to_num::<u64>(),
            )
        };

        if !tao_to_buy.is_zero() {
            Self::ensure_add_stake_input_within_swap_limit(netuid, tao_to_buy)?;
            Self::validate_add_stake(&coldkey, &hotkey, netuid, tao_to_buy, tao_to_buy, false)?;
            // `transfer_tao_to_subnet` uses Preservation::Preserve and silently
            // clips to keep-alive balance. Reject that partial fill up front.
            ensure!(
                Self::get_keep_alive_balance(&coldkey) >= tao_to_buy.into(),
                Error::<T>::NotEnoughBalanceToStake
            );
        }

        with_transaction(|| {
            let result = (|| -> DispatchResult {
                let mut added = AlphaBalance::ZERO;

                if !from_stake.is_zero() {
                    // Re-check coverage inside the transaction: stake may have
                    // moved since the preflight read.
                    let stake_now =
                        Self::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
                    let locked_now = Self::get_miner_collateral_locked(netuid, &hotkey, &coldkey);
                    ensure!(
                        stake_now.saturating_sub(locked_now) >= from_stake,
                        Error::<T>::StakeUnavailable
                    );
                    Self::credit_miner_collateral(netuid, &hotkey, &coldkey, from_stake, false)?;
                    added = added.saturating_add(from_stake);
                }

                if !tao_to_buy.is_zero() {
                    // Preflight the limit the same way as `add_stake_limit` so a
                    // too-tight bound fails before transferring TAO.
                    let max_amount: TaoBalance =
                        Self::get_max_amount_add(netuid, limit_price)?.into();
                    ensure!(tao_to_buy <= max_amount, Error::<T>::SlippageTooHigh);

                    let bought = Self::stake_into_subnet(
                        &hotkey,
                        &coldkey,
                        netuid,
                        tao_to_buy,
                        limit_price,
                        false,
                    )?;
                    ensure!(!bought.is_zero(), Error::<T>::AmountTooLow);
                    Self::credit_miner_collateral(netuid, &hotkey, &coldkey, bought, false)?;
                    added = added.saturating_add(bought);
                }

                ensure!(!added.is_zero(), Error::<T>::AmountTooLow);

                let total_locked = Self::get_miner_collateral_locked(netuid, &hotkey, &coldkey);
                Self::deposit_event(Event::CollateralLocked {
                    netuid,
                    hotkey: hotkey.clone(),
                    locked: added,
                    total_locked,
                });
                Ok(())
            })();

            match result {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(e) => TransactionOutcome::Rollback(Err(e)),
            }
        })
    }

    /// Set the miner's collateral floor for a hotkey on a subnet. The lock
    /// self-maintains around the floor (drain stops at it; emission fills a
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
        ensure!(
            !netuid.is_root(),
            Error::<T>::RegistrationNotPermittedOnRootSubnet
        );
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        ensure!(
            Self::coldkey_owns_hotkey(&coldkey, &hotkey),
            Error::<T>::NonAssociatedColdKey
        );

        let existed = MinerCollateral::<T>::contains_key((netuid, &hotkey, &coldkey));
        if !existed && !min_locked.is_zero() {
            // Fail before creating a row if the coldkey is already at the cap.
            Self::index_coldkey_collateral_hotkey(netuid, &coldkey, &hotkey)?;
        }

        MinerCollateral::<T>::mutate_exists((netuid, &hotkey, &coldkey), |maybe_state| {
            match maybe_state {
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
            }
        });
        if !MinerCollateral::<T>::contains_key((netuid, &hotkey, &coldkey)) {
            Self::deindex_coldkey_collateral_hotkey(netuid, &coldkey, &hotkey);
        }

        Self::deposit_event(Event::MinCollateralSet {
            netuid,
            hotkey,
            min_locked,
        });

        Ok(())
    }

    /// Merge `from` into an existing or vacant `MinerCollateral` slot.
    ///
    /// Sums `locked` / `min_locked` / `earned` and keeps the slower (`min`)
    /// drain ratio. Shared by hotkey-leg and coldkey-leg moves.
    fn merge_miner_collateral_state(
        maybe_state: &mut Option<MinerCollateralState>,
        from: MinerCollateralState,
    ) {
        match maybe_state {
            Some(state) => {
                state.locked = state.locked.saturating_add(from.locked);
                state.drain_ratio = state.drain_ratio.min(from.drain_ratio);
                state.min_locked = state.min_locked.saturating_add(from.min_locked);
                state.earned = state.earned.saturating_add(from.earned);
            }
            None => *maybe_state = Some(from),
        }
    }

    /// Read-only check that a hotkey collateral rename can proceed without
    /// exceeding [`crate::MAX_COLDKEY_COLLATERAL_HOTKEYS`]. Does not mutate
    /// storage — call before charging fees or writing swap state.
    pub fn ensure_hotkey_collateral_index_can_swap(
        netuid: NetUid,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Result<(), Error<T>> {
        if !MinerCollateral::<T>::contains_key((netuid, old_hotkey, coldkey)) {
            return Ok(());
        }
        let hotkeys = ColdkeyCollateralHotkeys::<T>::get(netuid, coldkey);
        if hotkeys.contains(new_hotkey) || hotkeys.iter().any(|h| h == old_hotkey) {
            // Destination already indexed, or source can be renamed in place.
            return Ok(());
        }
        ensure!(
            (hotkeys.len() as u32) < crate::MAX_COLDKEY_COLLATERAL_HOTKEYS,
            Error::<T>::ColdkeyCollateralPositionsFull
        );
        Ok(())
    }

    /// Preflight index capacity for every subnet that will migrate collateral
    /// during a hotkey swap. Call before charging fees or writing swap state.
    pub fn ensure_hotkey_collateral_swappable(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: Option<NetUid>,
    ) -> Result<(), Error<T>> {
        match netuid {
            Some(netuid) => Self::ensure_hotkey_collateral_index_can_swap(
                netuid, old_hotkey, new_hotkey, coldkey,
            ),
            None => {
                for netuid in Self::get_all_subnet_netuids() {
                    if MinerCollateral::<T>::contains_key((netuid, old_hotkey, coldkey)) {
                        Self::ensure_hotkey_collateral_index_can_swap(
                            netuid, old_hotkey, new_hotkey, coldkey,
                        )?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Ensure the destination hotkey will have an index slot before any
    /// collateral mutation. Renames the old index entry in place when possible
    /// so a coldkey already at the cap can still swap; otherwise reserves a
    /// new slot (failing closed if the cap is full).
    fn prepare_hotkey_collateral_index_swap(
        netuid: NetUid,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Result<(), Error<T>> {
        // Fail closed before any index write (same capacity rules as the
        // read-only preflight).
        Self::ensure_hotkey_collateral_index_can_swap(netuid, old_hotkey, new_hotkey, coldkey)?;
        if !MinerCollateral::<T>::contains_key((netuid, old_hotkey, coldkey)) {
            return Ok(());
        }
        let mut hotkeys = ColdkeyCollateralHotkeys::<T>::get(netuid, coldkey);
        let old_pos = hotkeys.iter().position(|h| h == old_hotkey);
        let new_indexed = hotkeys.contains(new_hotkey);
        if new_indexed {
            return Ok(());
        }
        if let Some(i) = old_pos {
            let Some(slot) = hotkeys.get_mut(i) else {
                // `position` implies a valid index; reserve instead if not.
                return Self::index_coldkey_collateral_hotkey(netuid, coldkey, new_hotkey);
            };
            *slot = new_hotkey.clone();
            ColdkeyCollateralHotkeys::<T>::insert(netuid, coldkey, hotkeys);
            return Ok(());
        }
        // Legacy / unindexed source row: require a free destination slot now.
        Self::index_coldkey_collateral_hotkey(netuid, coldkey, new_hotkey)
    }

    /// Move the collateral entry when a hotkey is swapped. The coldkey is
    /// unchanged (ownership stays with the same coldkey); only the hotkey leg
    /// of the key moves. If the new `(hotkey, coldkey)` already has collateral
    /// on the subnet, the locks merge via [`Self::merge_miner_collateral_state`].
    pub fn swap_miner_collateral(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> Result<(), Error<T>> {
        // Reserve / rename the index slot before mutating rows so a full-cap
        // failure cannot leave the swap half-applied.
        Self::prepare_hotkey_collateral_index_swap(netuid, old_hotkey, new_hotkey, coldkey)?;
        let Some(old_state) = MinerCollateral::<T>::take((netuid, old_hotkey, coldkey)) else {
            return Ok(());
        };
        MinerCollateral::<T>::mutate((netuid, new_hotkey, coldkey), |maybe_state| {
            Self::merge_miner_collateral_state(maybe_state, old_state);
        });
        // Drop any leftover old-hotkey index entry (no-op if already renamed).
        Self::deindex_coldkey_collateral_hotkey(netuid, coldkey, old_hotkey);
        Ok(())
    }

    /// Move the collateral entry when a coldkey is swapped. The hotkey is
    /// unchanged; only the coldkey leg of the key moves. Merge rules match
    /// [`Self::swap_miner_collateral`]. Updates [`ColdkeyMinerCollateral`] and
    /// [`ColdkeyCollateralHotkeys`] for both coldkeys. No-op when the source
    /// row is absent.
    pub fn transfer_miner_collateral_coldkey(
        netuid: NetUid,
        hotkey: &T::AccountId,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> Result<(), Error<T>> {
        if old_coldkey == new_coldkey {
            return Ok(());
        }
        if !MinerCollateral::<T>::contains_key((netuid, hotkey, old_coldkey)) {
            return Ok(());
        }
        // Reserve the destination index slot before mutating so a full-cap
        // failure cannot leave the coldkey swap half-applied.
        if !MinerCollateral::<T>::contains_key((netuid, hotkey, new_coldkey)) {
            Self::index_coldkey_collateral_hotkey(netuid, new_coldkey, hotkey)?;
        }
        let Some(old_state) = MinerCollateral::<T>::take((netuid, hotkey, old_coldkey)) else {
            return Ok(());
        };
        let moved_locked = old_state.locked;
        Self::adjust_coldkey_miner_collateral(
            old_coldkey,
            netuid,
            moved_locked,
            AlphaBalance::ZERO,
        );
        Self::deindex_coldkey_collateral_hotkey(netuid, old_coldkey, hotkey);
        let dest_before = Self::get_miner_collateral_locked(netuid, hotkey, new_coldkey);
        MinerCollateral::<T>::mutate((netuid, hotkey, new_coldkey), |maybe_state| {
            Self::merge_miner_collateral_state(maybe_state, old_state);
        });
        let dest_after = Self::get_miner_collateral_locked(netuid, hotkey, new_coldkey);
        Self::adjust_coldkey_miner_collateral(new_coldkey, netuid, dest_before, dest_after);
        Ok(())
    }

    /// Migrate every miner-collateral row for `old_coldkey` on `netuid` onto
    /// `new_coldkey`.
    ///
    /// Walks the bounded [`ColdkeyCollateralHotkeys`] index (not unbounded
    /// `StakingHotkeys` / `OwnedHotkeys` association vectors). Skips when the
    /// aggregate is already zero. Requires the aggregate to clear afterward so
    /// an unindexed / orphaned row fails closed rather than under-locking the
    /// destination unstake guard.
    pub fn transfer_coldkey_miner_collateral(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        if ColdkeyMinerCollateral::<T>::get(netuid, old_coldkey).is_zero() {
            return Ok(());
        }
        let hotkeys = ColdkeyCollateralHotkeys::<T>::get(netuid, old_coldkey);
        for hotkey in hotkeys {
            Self::transfer_miner_collateral_coldkey(netuid, &hotkey, old_coldkey, new_coldkey)?;
        }
        ensure!(
            ColdkeyMinerCollateral::<T>::get(netuid, old_coldkey).is_zero(),
            Error::<T>::ColdkeyCollateralIncomplete
        );
        Ok(())
    }
}
