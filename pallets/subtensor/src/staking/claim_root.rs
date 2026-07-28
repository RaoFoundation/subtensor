use super::*;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_runtime::traits::AccountIdConversion;
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{NetUidStorageIndex, clear_prefix_with_meter};
use subtensor_swap_interface::SwapHandler;

/// A drained fund (shares outstanding but NAV marked at zero) may revive via a par mint
/// only when the stale shares are rounding dust: at most `value / DRAINED_FUND_DUST_DIVISOR`
/// (so stale holders capture <= ~1% of the reviving deposit).
const DRAINED_FUND_DUST_DIVISOR: u64 = 100;

/// Where the TAO entering a basket deployment comes from. The two deposit paths share one
/// deployment engine (`deploy_tao_into_basket`); this is the only difference between them.
#[derive(Clone, Copy)]
enum BasketFunding<'a, AccountId> {
    /// Dividend TAO already inside the pools (protocol redeployment); swap fees are dropped.
    Protocol,
    /// TAO transferred in from this user's balance; swap fees are charged.
    User(&'a AccountId),
}

impl<T: Config> Pallet<T> {
    /// The single global escrow coldkey that custodies every validator's basket.
    ///
    /// A validator's basket (fund) holdings are positions `(validator_hotkey, this_account,
    /// netuid)` in the normal alpha share pool, so they count toward each validator's stake and
    /// compound with that validator's dividends, while the account itself stays inert (no user
    /// controls it). A single global coldkey is used deliberately: positions stay distinct per
    /// validator via the hotkey key, and hotkey swaps migrate them by value automatically.
    pub fn get_beta_escrow_account_id() -> T::AccountId {
        T::SubtensorPalletId::get().into_sub_account_truncating(b"beta/esc")
    }

    /// A validator's basket holdings: every `(netuid, alpha)` position the escrow custodies for
    /// this hotkey, including the root slot (the fund's TAO/cash position, valued 1:1).
    pub fn get_basket_holdings(hotkey: &T::AccountId) -> Vec<(NetUid, AlphaBalance)> {
        let escrow = Self::get_beta_escrow_account_id();
        Self::alpha_iter_prefix((hotkey, &escrow))
            .map(|(netuid, _)| {
                (
                    netuid,
                    Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, netuid),
                )
            })
            .filter(|(_, alpha)| !alpha.is_zero())
            .collect()
    }

    /// The validator's usable basket weight vector: entries pointing at root (the fund's
    /// TAO/cash slot) or an existing subnet, zero weights dropped. The vector follows the
    /// validator's root uid (so it survives hotkey swaps automatically) and reuses the
    /// existing root weights plumbing. An empty stored vector means "non-specific": deploy
    /// 100% to root (TAO in the fund's root slot) so stakers accrue by default. Returns an
    /// empty vector only when explicit weights filter to nothing. Every returned weight is
    /// positive, so a non-empty vector always has a positive weight sum.
    pub fn get_valid_basket_weights(hotkey: &T::AccountId) -> Vec<(NetUid, u64)> {
        let maybe_uid = Uids::<T>::try_get(NetUid::ROOT, hotkey).ok();
        let stored_weights = maybe_uid
            .map(|uid| Weights::<T>::get(NetUidStorageIndex::ROOT, uid))
            .unwrap_or_default();
        let weights = if stored_weights.is_empty() {
            vec![(u16::from(NetUid::ROOT), u16::MAX)]
        } else {
            stored_weights
        };

        // Keep weights that point at root (uid 0) or an existing subnet. Root is a valid
        // destination: that slice is held as the fund's root-stake (TAO) cash position instead of
        // being deployed into subnet alpha, letting a validator opt out of subnet exposure while
        // its stakers still accumulate (and compound) yield on root.
        weights
            .into_iter()
            .filter_map(|(dest, weight)| {
                let dest_netuid = NetUid::from(dest);
                if weight > 0 && (dest_netuid.is_root() || Self::if_subnet_exist(dest_netuid)) {
                    Some((dest_netuid, weight as u64))
                } else {
                    None
                }
            })
            .collect()
    }

    /// `a * b / denom` computed in u128 so the u64*u64 product cannot overflow, saturated
    /// back to u64 (a u64*u64 product can exceed U96F32's 96 integer bits at chain-scale
    /// magnitudes, which would silently saturate fixed-point math). Returns 0 when `denom`
    /// is zero.
    pub(crate) fn mul_div_u64(a: u64, b: u64, denom: u64) -> u64 {
        u128::from(a)
            .saturating_mul(u128::from(b))
            .checked_div(u128::from(denom))
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64
    }

    /// Fund shares to mint for `value` TAO of realizable value entering a fund with
    /// pre-deposit NAV `nav_before` and `shares_outstanding` (`P`) shares outstanding:
    /// `value * P / N` (deposit-at-NAV, so existing holders are neither diluted nor
    /// gifted). First deposit mints at par.
    fn basket_shares_for_value(value: u64, nav_before: u64, shares_outstanding: u64) -> u64 {
        if shares_outstanding == 0 {
            // Genuine first deposit: mint at par (1 share per TAO of value added).
            return value;
        }
        if nav_before == 0 {
            // Shares are outstanding but the fund marks to zero, so there is no NAV to
            // price a mint against. A par mint would hand the stale holders
            // `S_old / (S_old + minted)` of the fresh deposit. Tolerate that only when
            // the stale shares are rounding dust left by a full drain, so a drained fund
            // can revive; otherwise mint nothing and let the caller reject or recycle
            // the deposit rather than misprice it.
            if shares_outstanding <= value.saturating_div(DRAINED_FUND_DUST_DIVISOR) {
                return value;
            }
            return 0;
        }
        Self::mul_div_u64(value, shares_outstanding, nav_before)
    }

    /// Distributes a validator's root dividend (origin-subnet alpha, net of take) into its beta
    /// basket according to the validator's root weight vector `w` (set on subnet 0).
    ///
    /// Flow: sell the origin alpha for TAO, then split that TAO across subnets per `w`, buying
    /// each subnet's alpha and staking it to the validator under the global escrow coldkey (a
    /// root-destination slice is held directly as the fund's root-stake cash position). The
    /// deposit then mints *fund shares* against the whole basket: `shares = value_added * P / N`,
    /// where `N` is the fund's pre-buy realizable NAV, `P` the outstanding shares, and
    /// `value_added` the realizable NAV the deposit actually added (post-buy NAV minus the
    /// pre-buy snapshot), so the deposit bears its own buy slippage/fees instead of
    /// socializing them, and existing holders are neither diluted nor taxed. Stakers accrue
    /// entitlement through the single per-validator
    /// `BasketRate += shares / total_root_stake` accumulator; no entitlement is ever denominated
    /// in a particular subnet's alpha, which is what allows holdings to be rebalanced without
    /// touching staker claims.
    ///
    /// Attribution: the dividend was earned by the validator's WHOLE root stake, including
    /// the fund's own root-slot (escrow) position. Only the real stakers' fraction of the
    /// value mints shares; the escrow slot's fraction enters the fund unminted, so the
    /// fund's own cash yield accrues to existing share holders through N/P instead of
    /// leaking to root stakers as free shares.
    ///
    /// The whole operation is transactional: if any swap fails (or the deposit is dust), it is
    /// rolled back and the original alpha is recycled. Validators with no stored root weights
    /// default to 100% root (TAO in the fund's root slot). Dividends are recycled only when
    /// explicit weights filter to nothing, or when the validator has no root stake to apportion
    /// against.
    ///
    /// Protocol-flow accounting is symmetric with redemption: the origin sell is booked as an
    /// outflow on the origin subnet and each redistribution buy as an inflow on its dest subnet,
    /// so that a deposit-then-claim round-trip nets to ~0 on the dest pools (the claim sell is
    /// booked as an outflow in `root_claim_for_hotkey`).
    pub fn distribute_root_alpha_to_basket(
        hotkey: &T::AccountId,
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
    ) {
        if root_alpha.is_zero() {
            return;
        }

        let valid = Self::get_valid_basket_weights(hotkey);
        let escrow = Self::get_beta_escrow_account_id();

        // Claimant base = real stakers' root stake. The escrow custody account is not a claimant,
        // so its own root-slot holdings are excluded; otherwise the fund's claimable rate would
        // be diluted and a slice of shares would become unclaimable. The escrow slot is kept
        // separately: it earned its pro-rata slice of this dividend, which is credited to the
        // fund unminted below.
        let escrow_root =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, NetUid::ROOT);
        let total_root =
            Self::get_stake_for_hotkey_on_subnet(hotkey, NetUid::ROOT).saturating_sub(escrow_root);

        // Explicit weights that filter to nothing, or no root stake to apportion against: recycle.
        if valid.is_empty() || total_root.is_zero() {
            Self::recycle_subnet_alpha(origin_netuid, root_alpha);
            return;
        }

        let outcome = with_transaction(|| {
            match Self::try_distribute_root_alpha_to_basket(
                hotkey,
                origin_netuid,
                root_alpha,
                &valid,
                total_root.to_u64(),
                escrow_root.to_u64(),
            ) {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        });

        // On any failure the swaps were rolled back; recycle the original alpha.
        if outcome.is_err() {
            Self::recycle_subnet_alpha(origin_netuid, root_alpha);
        }
    }

    /// Transactional body of [`Self::distribute_root_alpha_to_basket`]; any error rolls the
    /// whole deposit back (the caller recycles the origin alpha).
    fn try_distribute_root_alpha_to_basket(
        hotkey: &T::AccountId,
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
        valid: &[(NetUid, u64)],
        total_root: u64,
        escrow_root: u64,
    ) -> DispatchResult {
        let shares_outstanding: u64 = BasketShares::<T>::get(hotkey);

        // 1. Sell the origin-subnet alpha for TAO, booked as protocol outflow (TAO left the
        // origin pool). The deployment below snapshots NAV only after this sell: the fund may
        // itself hold origin-subnet alpha, and the sell moves that price, so marking N any
        // earlier would misprice the mint against the state the deposit actually enters.
        let tao_total: TaoBalance = Self::swap_alpha_for_tao(
            origin_netuid,
            root_alpha,
            T::SwapInterface::min_price::<TaoBalance>(),
            true,
        )?
        .amount_paid_out;
        Self::record_protocol_outflow(origin_netuid, tao_total);

        // 2. Deploy the TAO across the basket per the weight vector and value the deposit at
        // the realizable NAV it actually added.
        let (nav_before, value_added) = Self::deploy_tao_into_basket(
            hotkey,
            valid,
            tao_total.to_u64(),
            BasketFunding::Protocol,
        )?;

        // 3. Attribution: the dividend was earned by the whole root stake, escrow slot
        // included. Only the real stakers' fraction mints shares; the escrow slot's
        // fraction stays unminted so its value raises N/P for existing share holders
        // (the fund's own cash yield belongs to the fund).
        let stakers_value: u64 = Self::mul_div_u64(
            value_added,
            total_root,
            total_root.saturating_add(escrow_root),
        );

        // 4. Mint fund shares at the pre-deposit NAV: shares = stakers_value * P / N. A
        // deposit into an already-compounded fund (N/P > 1) mints fewer shares than TAO
        // added, so N/P is left unchanged.
        let shares: u64 =
            Self::basket_shares_for_value(stakers_value, nav_before, shares_outstanding);

        // Per-staker claimable rate increment: fund shares per unit of root stake.
        let increment: I96F32 = I96F32::saturating_from_num(shares)
            .checked_div(I96F32::saturating_from_num(total_root))
            .unwrap_or(I96F32::saturating_from_num(0));

        // Dust deposit (shares or rate round to zero): roll everything back and recycle, so
        // `Σ owed == BasketShares` is never broken by uncredited value.
        ensure!(
            shares > 0 && increment != I96F32::saturating_from_num(0),
            DispatchError::Other("basket deposit too small")
        );

        BasketShares::<T>::mutate(hotkey, |p| *p = p.saturating_add(shares));
        BasketRate::<T>::mutate(hotkey, |rate| *rate = rate.saturating_add(increment));
        BasketDepositedTao::<T>::mutate(hotkey, |total| {
            *total = total.saturating_add(value_added.into())
        });

        Self::deposit_event(Event::BasketDeposited {
            hotkey: hotkey.clone(),
            tao: value_added.into(),
            shares,
        });

        Ok(())
    }

    /// The shared basket deployment engine: splits `tao` across the validator's weight vector
    /// `valid` (last slot absorbs the rounding remainder so the split sums exactly), buys each
    /// subnet slot's alpha into the escrow position, and holds root-slot slices as root stake
    /// (TAO at 1:1, mirroring `swap_tao_for_alpha`'s reserve bookkeeping by hand). Each buy is
    /// booked as protocol inflow: claims book the escrow's sells as outflow regardless of how
    /// the alpha entered, so entries must record the matching inflow or round trips skew the
    /// flow EMA.
    ///
    /// `funding` is the only difference between the two deposit paths: protocol dividends
    /// already live inside the pools (fees dropped), while user deposits are physically
    /// transferred from the coldkey's balance to each destination subnet account first (fees
    /// charged).
    ///
    /// Returns `(nav_before, value_added)`: the realizable NAV snapshotted immediately before
    /// the buys, and the realizable NAV the deployment actually added (ΔNAV, post-buy minus
    /// pre-buy). Both snapshots are marked identically in the same block, so the difference
    /// isolates exactly this deposit's effect — the deposit bears its own buy slippage/fees (a
    /// realizable delta is bounded by the TAO deployed, never amplified by the buys' own price
    /// impact on existing holdings the way a spot delta would be).
    ///
    /// Not transactional by itself: callers run it inside `with_transaction` and roll back on
    /// error.
    fn deploy_tao_into_basket(
        hotkey: &T::AccountId,
        valid: &[(NetUid, u64)],
        tao: u64,
        funding: BasketFunding<T::AccountId>,
    ) -> Result<(u64, u64), DispatchError> {
        let escrow = Self::get_beta_escrow_account_id();
        let weight_sum: u64 = valid.iter().map(|(_, w)| *w).sum();
        let nav_before: u64 = Self::get_validator_basket_nav_tao(hotkey).to_u64();

        let mut spent: u64 = 0;
        let last_idx = valid.len().saturating_sub(1);
        for (i, (dest_netuid, weight)) in valid.iter().enumerate() {
            // Last slot absorbs the rounding remainder so Σ tao_s == tao exactly.
            let tao_s: u64 = if i == last_idx {
                tao.saturating_sub(spent)
            } else {
                Self::mul_div_u64(tao, *weight, weight_sum)
            };
            spent = spent.saturating_add(tao_s);
            if tao_s == 0 {
                continue;
            }

            if let BasketFunding::User(coldkey) = funding {
                // Physically move the staker's TAO to the destination subnet account:
                // basket buys here are real user stake entering the system, unlike
                // dividend redeployments whose TAO already lives inside the pools.
                let transferred =
                    Self::transfer_tao_to_subnet(*dest_netuid, coldkey, tao_s.into())?;
                ensure!(
                    transferred == TaoBalance::from(tao_s),
                    Error::<T>::InsufficientTaoBalance
                );
            }

            if dest_netuid.is_root() {
                // Root slot: held as root stake (TAO at 1:1), no pool to buy from.
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    &escrow,
                    NetUid::ROOT,
                    tao_s.into(),
                );
                Self::credit_root_reserves(tao_s.into());
            } else {
                let drop_fees = matches!(funding, BasketFunding::Protocol);
                let bought = Self::swap_tao_for_alpha(
                    *dest_netuid,
                    tao_s.into(),
                    T::SwapInterface::max_price(),
                    drop_fees,
                )?
                .amount_paid_out;
                if bought.is_zero() {
                    // Dust slice whose swap rounds to zero alpha: a user deposit must not
                    // silently donate its TAO to the pool (mirrors `stake_into_subnet`'s
                    // zero-out rejection), so the whole deposit rolls back. A protocol
                    // dividend tolerates the dust slice and skips it — without booking an
                    // inflow that would never see a matching claim outflow.
                    match funding {
                        BasketFunding::User(_) => return Err(Error::<T>::AmountTooLow.into()),
                        BasketFunding::Protocol => continue,
                    }
                }
                // Record the buy as protocol inflow (TAO entered the pool).
                Self::record_protocol_inflow(*dest_netuid, tao_s.into());
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    &escrow,
                    *dest_netuid,
                    bought,
                );
            }
        }

        let nav_after: u64 = Self::get_validator_basket_nav_tao(hotkey).to_u64();
        Ok((nav_before, nav_after.saturating_sub(nav_before)))
    }

    /// Stakes `tao` from `coldkey`'s free balance directly into a validator's basket:
    /// the TAO is deployed across subnets per the validator's root weight vector (exactly
    /// like a dividend deposit), and the resulting fund shares are credited to the staker
    /// through their signed claimed watermark — `owed = rate * root_stake - claimed`, so a
    /// negative watermark credit is an unconditional share grant that needs no root stake
    /// and survives stake-change rebasing (which is additive).
    ///
    /// Shares are minted at the pre-buy realizable NAV against the realizable value the
    /// deposit added (`nav_after - nav_before`), so the depositor bears their own entry
    /// slippage and fees, and a deposit-then-claim round trip nets to ~0 (minus swap fees)
    /// at any basket size. Unlike dividend deposits there is no attribution split: the
    /// whole deposit belongs to the depositor. `BasketRate` is untouched — direct shares
    /// buy fund exposure, they do not change any staker's dividend accrual.
    pub fn do_stake_into_basket(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        tao: TaoBalance,
    ) -> Result<Weight, DispatchError> {
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        ensure!(tao >= DefaultMinStake::<T>::get(), Error::<T>::AmountTooLow);
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, tao.into()),
            Error::<T>::NotEnoughBalanceToStake
        );

        let valid = Self::get_valid_basket_weights(&hotkey);
        ensure!(!valid.is_empty(), Error::<T>::BasketHasNoWeights);

        // Each weight slot can add at most one new holding, so pre-deploy holdings plus the
        // slot count bounds the holdings the two NAV valuations will sweep.
        let num_holdings =
            (Self::get_basket_holdings(&hotkey).len() as u64).saturating_add(valid.len() as u64);

        with_transaction(
            || match Self::try_stake_into_basket(&coldkey, &hotkey, tao, &valid) {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            },
        )?;

        Ok(Self::stake_into_basket_weight(
            valid.len() as u64,
            num_holdings,
        ))
    }

    /// Transactional body of [`Self::do_stake_into_basket`]; any error rolls the whole
    /// deposit back, including the balance transfers.
    fn try_stake_into_basket(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        tao: TaoBalance,
        valid: &[(NetUid, u64)],
    ) -> DispatchResult {
        let shares_outstanding: u64 = BasketShares::<T>::get(hotkey);

        // Deploy the staker's TAO across the basket per the weight vector. ΔNAV valuation
        // means the depositor bears their own entry slippage/fees and cannot capture value
        // beyond the TAO they brought.
        let (nav_before, value_added) = Self::deploy_tao_into_basket(
            hotkey,
            valid,
            tao.to_u64(),
            BasketFunding::User(coldkey),
        )?;

        let shares: u64 =
            Self::basket_shares_for_value(value_added, nav_before, shares_outstanding);
        ensure!(shares > 0, Error::<T>::AmountTooLow);

        BasketShares::<T>::mutate(hotkey, |p| *p = p.saturating_add(shares));
        Self::grant_basket_shares(hotkey, coldkey, shares);
        BasketDepositedTao::<T>::mutate(hotkey, |total| {
            *total = total.saturating_add(value_added.into())
        });

        // Make sure claims (which walk `StakingHotkeys`) can find this position.
        let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
        if !staking_hotkeys.contains(hotkey) {
            staking_hotkeys.push(hotkey.clone());
            StakingHotkeys::<T>::insert(coldkey, staking_hotkeys);
        }
        Self::maybe_add_coldkey_index(coldkey);

        Self::deposit_event(Event::BasketStakedIn {
            hotkey: hotkey.clone(),
            coldkey: coldkey.clone(),
            tao,
            value: value_added.into(),
            shares,
        });

        Ok(())
    }

    /// Actual weight of a `stake_into_basket` call that deployed across `num_slots` weight
    /// slots with `num_holdings` basket holdings. Per slot: a balance transfer to the subnet
    /// account, a swap, the escrow stake write, and protocol-flow bookkeeping. Per holding:
    /// two `sim_swap` valuations (the `nav_before` / `nav_after` sweeps).
    pub(crate) fn stake_into_basket_weight(num_slots: u64, num_holdings: u64) -> Weight {
        Weight::from_parts(25_000_000, 4000)
            .saturating_add(T::DbWeight::get().reads(6_u64))
            .saturating_add(T::DbWeight::get().writes(5_u64))
            .saturating_mul(num_slots.max(1))
            .saturating_add(
                Weight::from_parts(10_000_000, 1000)
                    .saturating_add(T::DbWeight::get().reads(4_u64))
                    .saturating_mul(num_holdings.max(1)),
            )
            .saturating_add(T::DbWeight::get().reads_writes(8_u64, 6_u64))
    }

    /// A staker's gross *fund-share* entitlement on a validator: `BasketRate * root_stake`.
    /// Shares, not TAO — convert with `basket_payout_from` / `get_basket_payout_tao`.
    pub fn get_basket_claimable_shares(hotkey: &T::AccountId, coldkey: &T::AccountId) -> I96F32 {
        let root_stake: I96F32 = I96F32::saturating_from_num(
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT),
        );
        BasketRate::<T>::get(hotkey).saturating_mul(root_stake)
    }

    fn get_basket_owed_shares_float(hotkey: &T::AccountId, coldkey: &T::AccountId) -> I96F32 {
        let claimable = Self::get_basket_claimable_shares(hotkey, coldkey);

        // Subtract the already-claimed watermark (signed: unstake rebasing can push it below
        // zero) to avoid over- or under-claiming.
        let claimed: I96F32 = I96F32::saturating_from_num(BasketClaimed::<T>::get(hotkey, coldkey));

        claimable.saturating_sub(claimed)
    }

    /// A staker's net owed *fund shares* on a validator (floored at zero). Shares, not TAO.
    pub fn get_basket_owed_shares(hotkey: &T::AccountId, coldkey: &T::AccountId) -> u64 {
        let owed = Self::get_basket_owed_shares_float(hotkey, coldkey);
        if owed.is_negative() {
            0
        } else {
            owed.saturating_to_num::<u64>()
        }
    }

    /// Claims (redeems) a staker's share of a validator's basket.
    ///
    /// Redemption is fund-level and purely proportional: the staker's owed shares define a
    /// fraction `f = owed / P` of the fund, and exactly that fraction of *every* holding is
    /// redeemed — subnet alpha is sold to TAO (the staker bears slippage), the root-slot portion
    /// is reassigned as root stake directly (no swap). Because every claim preserves the fund's
    /// composition, claims and (future) validator-directed rebalancing never interfere. All
    /// realized TAO is staked on root for the staker.
    ///
    /// Returns the TAO realized and staked for the staker (zero for every no-op path).
    pub fn root_claim_for_hotkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum_condition: bool,
    ) -> Result<u64, DispatchError> {
        let owed_shares: u64 = Self::get_basket_owed_shares(hotkey, coldkey);
        if owed_shares == 0 {
            return Ok(0); // no-op
        }

        let shares_total: u64 = BasketShares::<T>::get(hotkey);
        // Nothing realizable yet (fund drained); leave the watermark untouched so the claim can
        // pay out once the fund has value again.
        if shares_total == 0 {
            return Ok(0);
        }
        // A claim can never redeem more than the outstanding fund.
        let owed_shares = owed_shares.min(shares_total);

        // Dust check against the estimated payout (owed fraction of the marked NAV).
        let nav = Self::get_validator_basket_nav_tao(hotkey).to_u64();
        let estimated_payout: u64 = Self::basket_payout_from(owed_shares, nav, shares_total);
        if !ignore_minimum_condition
            && I96F32::saturating_from_num(estimated_payout)
                < RootClaimableThreshold::<T>::get(NetUid::ROOT)
        {
            log::debug!(
                "root claim skipped (below threshold): payout={estimated_payout:?} h={hotkey:?} c={coldkey:?}"
            );
            return Ok(0); // no-op
        }
        if estimated_payout == 0 {
            return Ok(0);
        }

        let escrow = Self::get_beta_escrow_account_id();
        let holdings = Self::get_basket_holdings(hotkey);

        with_transaction(|| {
            // TAO credited to the staker's root stake, split by source: the root-slot portion is
            // a stake reassignment (no new TAO on root), while subnet sells realize new TAO that
            // must also be credited to the root reserves.
            let mut root_slot_tao: u64 = 0;
            let mut swapped_tao: u64 = 0;

            for (netuid, slot_alpha) in holdings.iter() {
                // This staker's pro-rata slice of the holding: slot_alpha * owed / P.
                let take: u64 = Self::mul_div_u64(slot_alpha.to_u64(), owed_shares, shares_total);
                if take == 0 {
                    continue;
                }

                Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    &escrow,
                    *netuid,
                    take.into(),
                );

                if netuid.is_root() {
                    // Root slot: already TAO (1:1), just reassign custody escrow -> staker below.
                    root_slot_tao = root_slot_tao.saturating_add(take);
                    continue;
                }

                // Sell the slice to TAO.
                let tao = match Self::sell_basket_alpha_for_root_tao(*netuid, take.into()) {
                    Ok(tao) => tao,
                    Err(err) => return TransactionOutcome::Rollback(Err(err)),
                };

                // Record root sell (reduces protocol cost).
                SubnetRootSellTao::<T>::mutate(*netuid, |total| {
                    *total = total.saturating_add(tao);
                });

                swapped_tao = swapped_tao.saturating_add(tao.to_u64());
            }

            let total_tao: u64 = root_slot_tao.saturating_add(swapped_tao);

            // Nothing was actually realized (every per-holding take floored to zero, or the
            // swaps returned zero TAO). The marked estimate above can be positive while the raw
            // alpha takes floor to zero (high-price, tiny-alpha holdings), so this must NOT
            // settle: roll back and leave the watermark untouched, otherwise the staker's owed
            // shares would be burned for a zero payout.
            if total_tao == 0 {
                return TransactionOutcome::Rollback(Ok(0));
            }

            // Stake the redeemed TAO on root for the staker. Only the swapped portion is new TAO
            // on root (the root-slot portion was already counted in the root reserves).
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                coldkey,
                NetUid::ROOT,
                total_tao.into(),
            );
            if swapped_tao > 0 {
                Self::credit_root_reserves(swapped_tao.into());
            }

            // The staker's root stake just grew; rebase their claimed watermark so the new stake
            // does not retroactively inflate their claimable.
            Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(hotkey, coldkey, total_tao);

            // Consume the claimed shares and advance the watermark.
            BasketShares::<T>::mutate(hotkey, |p| *p = p.saturating_sub(owed_shares));
            BasketClaimed::<T>::mutate(hotkey, coldkey, |claimed| {
                *claimed = claimed.saturating_add(i128::from(owed_shares));
            });
            BasketRedeemedTao::<T>::mutate(hotkey, |total| {
                *total = total.saturating_add(total_tao.into())
            });

            Self::deposit_event(Event::BasketClaimed {
                hotkey: hotkey.clone(),
                coldkey: coldkey.clone(),
                tao: total_tao.into(),
            });

            TransactionOutcome::Commit(Ok::<u64, DispatchError>(total_tao))
        })
    }

    fn root_claim_weight(num_holdings: u64) -> Weight {
        // Per-holding: escrow stake read/write + swap + protocol-flow bookkeeping.
        Weight::from_parts(20_000_000, 3000)
            .saturating_add(T::DbWeight::get().reads(4_u64))
            .saturating_add(T::DbWeight::get().writes(3_u64))
            .saturating_mul(num_holdings.max(1))
            .saturating_add(T::DbWeight::get().reads_writes(4_u64, 3_u64))
    }

    pub fn do_root_claim(coldkey: T::AccountId) -> Result<Weight, DispatchError> {
        with_transaction(|| match Self::try_do_root_claim(coldkey) {
            Ok(weight) => TransactionOutcome::Commit(Ok(weight)),
            Err(err) => TransactionOutcome::Rollback(Err(err)),
        })
    }

    fn try_do_root_claim(coldkey: T::AccountId) -> Result<Weight, DispatchError> {
        let mut weight = Weight::default();

        let hotkeys = StakingHotkeys::<T>::get(&coldkey);
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        let mut total_tao: u64 = 0;
        for hotkey in hotkeys.iter() {
            let num_holdings = Self::get_basket_holdings(hotkey).len() as u64;
            let realized = Self::root_claim_for_hotkey(hotkey, &coldkey, false)?;
            total_tao = total_tao.saturating_add(realized);
            weight.saturating_accrue(Self::root_claim_weight(num_holdings));
        }

        Self::deposit_event(Event::RootClaimed {
            coldkey,
            tao: total_tao.into(),
        });

        Ok(weight)
    }

    pub fn maybe_add_coldkey_index(coldkey: &T::AccountId) {
        if !StakingColdkeys::<T>::contains_key(coldkey) {
            let n = NumStakingColdkeys::<T>::get();
            StakingColdkeysByIndex::<T>::insert(n, coldkey.clone());
            StakingColdkeys::<T>::insert(coldkey.clone(), n);
            NumStakingColdkeys::<T>::mutate(|n| *n = n.saturating_add(1));
        }
    }

    /// Returns true if `coldkey` still holds any root (netuid 0) stake on any of its
    /// staking hotkeys. Used to decide whether the coldkey should remain indexed in the
    /// staking-coldkey index.
    pub fn coldkey_has_root_stake(coldkey: &T::AccountId) -> bool {
        StakingHotkeys::<T>::get(coldkey).iter().any(|hotkey| {
            !Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
                .is_zero()
        })
    }

    /// Remove `coldkey` from the staking-coldkey index, compacting by moving the last
    /// entry into the freed slot so the index stays dense in `[0, n)`. This is the inverse
    /// of `maybe_add_coldkey_index` and keeps the
    /// `StakingColdkeys[c] == i <=> StakingColdkeysByIndex[i] == c` bijection consistent.
    pub fn maybe_remove_coldkey_index(coldkey: &T::AccountId) {
        if let Some(idx) = StakingColdkeys::<T>::take(coldkey) {
            let last = NumStakingColdkeys::<T>::get().saturating_sub(1);
            if idx != last
                && let Some(moved) = StakingColdkeysByIndex::<T>::take(last)
            {
                StakingColdkeysByIndex::<T>::insert(idx, moved.clone());
                StakingColdkeys::<T>::insert(moved, idx);
            } else {
                StakingColdkeysByIndex::<T>::remove(idx);
            }
            NumStakingColdkeys::<T>::put(last);
        }
    }

    /// Rebase a staker's claimed watermark by `rate * stake_delta` after their root stake
    /// changed, so a stake change never retroactively grants or destroys accrued claimable.
    /// The watermark is signed and may legitimately go negative (e.g. claim, then unstake).
    fn rebase_basket_claimed_for_stake_delta(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        stake_delta: i128,
    ) {
        let rate = BasketRate::<T>::get(hotkey);
        if rate == I96F32::saturating_from_num(0) {
            return;
        }
        BasketClaimed::<T>::mutate(hotkey, coldkey, |claimed| {
            *claimed = claimed.saturating_add(
                rate.saturating_mul(I96F32::saturating_from_num(stake_delta))
                    .saturating_to_num::<i128>(),
            );
        });
    }

    /// Grant `shares` fund shares to a staker unconditionally by decrementing their signed
    /// claimed watermark: `owed = rate * root_stake - claimed`, so a negative watermark is a
    /// share grant that needs no root stake and survives stake-change rebasing (which is
    /// additive). The caller must mint the same `shares` into [`BasketShares`], preserving
    /// `Σ owed == BasketShares`.
    fn grant_basket_shares(hotkey: &T::AccountId, coldkey: &T::AccountId, shares: u64) {
        BasketClaimed::<T>::mutate(hotkey, coldkey, |claimed| {
            *claimed = claimed.saturating_sub(i128::from(shares));
        });
    }

    /// Watermark rebase for a root-stake increase of `amount`.
    pub fn add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        amount: u64,
    ) {
        Self::rebase_basket_claimed_for_stake_delta(hotkey, coldkey, i128::from(amount));
    }

    /// Watermark rebase for a root-stake decrease of `amount`.
    pub fn remove_stake_adjust_root_claimed_for_hotkey_and_coldkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        amount: AlphaBalance,
    ) {
        Self::rebase_basket_claimed_for_stake_delta(
            hotkey,
            coldkey,
            i128::from(u64::from(amount)).saturating_neg(),
        );
    }

    /// Moves a staker's claimed watermark on `hotkey` to a new coldkey (used by coldkey swaps;
    /// hotkey swaps migrate all watermarks via `transfer_basket_for_new_hotkey`).
    pub fn transfer_basket_claimed_for_new_coldkey(
        hotkey: &T::AccountId,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) {
        // Sum the two already-claimed watermarks. When BOTH the source and the destination
        // hold a legitimate watermark — e.g. a coldkey swap onto a hotkey the new coldkey has
        // already staked to — the merged "already claimed" total is old + new. Taking the max
        // would drop one side, under-count what has already been claimed, and cause a future
        // over-payment / double-claim (see GHSA-2026-010 for the hotkey-swap analog, which is
        // prevented upstream by the root-swap cleanliness gate in `do_swap_hotkey`).
        let old_claimed: i128 = BasketClaimed::<T>::take(hotkey, old_coldkey);
        if old_claimed != 0 {
            BasketClaimed::<T>::mutate(hotkey, new_coldkey, |claimed| {
                *claimed = claimed.saturating_add(old_claimed);
            });
        }
    }

    /// Migrates a validator's entire fund to a new hotkey: shares, rate, per-coldkey watermarks,
    /// and every escrow holding, moved by value. The caller must guarantee the new hotkey is
    /// clean on root (enforced by `do_swap_hotkey`), so this is a move, not a merge.
    pub fn transfer_basket_for_new_hotkey(old_hotkey: &T::AccountId, new_hotkey: &T::AccountId) {
        let shares = BasketShares::<T>::take(old_hotkey);
        if shares != 0 {
            BasketShares::<T>::mutate(new_hotkey, |p| *p = p.saturating_add(shares));
        }

        let rate = BasketRate::<T>::take(old_hotkey);
        if rate != I96F32::saturating_from_num(0) {
            BasketRate::<T>::mutate(new_hotkey, |r| *r = r.saturating_add(rate));
        }

        // Lifetime performance counters follow the fund.
        let deposited = BasketDepositedTao::<T>::take(old_hotkey);
        if !deposited.is_zero() {
            BasketDepositedTao::<T>::mutate(new_hotkey, |t| *t = t.saturating_add(deposited));
        }
        let redeemed = BasketRedeemedTao::<T>::take(old_hotkey);
        if !redeemed.is_zero() {
            BasketRedeemedTao::<T>::mutate(new_hotkey, |t| *t = t.saturating_add(redeemed));
        }

        let claimed_entries: Vec<(T::AccountId, i128)> =
            BasketClaimed::<T>::drain_prefix(old_hotkey).collect();
        for (coldkey, claimed) in claimed_entries {
            BasketClaimed::<T>::mutate(new_hotkey, &coldkey, |c| {
                *c = c.saturating_add(claimed);
            });
        }

        let escrow = Self::get_beta_escrow_account_id();
        for (netuid, alpha) in Self::get_basket_holdings(old_hotkey) {
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                old_hotkey, &escrow, netuid, alpha,
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                new_hotkey, &escrow, netuid, alpha,
            );
        }
    }

    /// Converts every validator's basket holding on a dissolving subnet into the fund's root
    /// (TAO) slot: the escrow alpha is sold once and the proceeds are held as root stake under
    /// the same escrow position. Fund shares, rates, and watermarks are untouched — the fund's
    /// NAV is continuous across the conversion (minus slippage), so no per-staker accounting is
    /// needed. Best-effort: a failed swap is logged and the slot is left for generic teardown.
    pub fn convert_subnet_basket_holdings_to_root(netuid: NetUid) {
        let escrow = Self::get_beta_escrow_account_id();
        let hotkeys: Vec<T::AccountId> = BasketShares::<T>::iter_keys().collect();

        for hotkey in hotkeys.iter() {
            Self::convert_basket_holding_to_root(hotkey, &escrow, netuid);
        }
    }

    fn convert_basket_holding_to_root(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        netuid: NetUid,
    ) {
        let holding_alpha =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, netuid);
        if holding_alpha.is_zero() {
            return;
        }

        let _ = with_transaction(|| {
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                escrow,
                netuid,
                holding_alpha,
            );

            let tao = match Self::sell_basket_alpha_for_root_tao(netuid, holding_alpha) {
                Ok(tao) => tao,
                Err(err) => {
                    log::error!("Error converting basket holding to root: {err:?}");
                    return TransactionOutcome::Rollback(Err(err));
                }
            };

            // Hold the realized TAO as the fund's root-slot (cash) position.
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                escrow,
                NetUid::ROOT,
                tao.to_u64().into(),
            );
            Self::credit_root_reserves(tao);

            Self::deposit_event(Event::BasketHoldingConverted {
                hotkey: hotkey.clone(),
                netuid,
                tao,
            });

            TransactionOutcome::Commit(Ok::<(), DispatchError>(()))
        });
    }

    /// Sells basket `alpha` on `netuid` for TAO and lands it in the root subnet account, booking
    /// the protocol outflow. The alpha must already have been removed from the escrow position.
    /// Shared by claim redemption and dissolution conversion; callers stay transactional.
    fn sell_basket_alpha_for_root_tao(
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let out = Self::swap_alpha_for_tao(
            netuid,
            alpha,
            T::SwapInterface::min_price::<TaoBalance>(),
            true,
        )
        .inspect_err(|err| log::error!("Error swapping basket alpha for TAO: {err:?}"))?;

        let root_subnet_account_id =
            Self::get_subnet_account_id(NetUid::ROOT).ok_or(Error::<T>::RootNetworkDoesNotExist)?;

        Self::transfer_tao_from_subnet(netuid, &root_subnet_account_id, out.amount_paid_out.into())
            .inspect_err(|err| log::error!("Error transferring basket TAO from subnet: {err:?}"))?;

        Self::record_protocol_outflow(netuid, out.amount_paid_out);

        Ok(out.amount_paid_out)
    }

    /// Drop a dissolving subnet's entries from the LEGACY per-subnet claimable rates. The
    /// live basket state is fund-level (no per-subnet entitlement), so only the legacy
    /// storage — kept for `migrate_seed_beta_basket` — needs per-subnet cleanup.
    pub fn clean_up_root_claimable_for_subnet(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => RootClaimable::<T>::iter_from(raw_key),
            None => RootClaimable::<T>::iter(),
        };

        fn filter_claimable(
            claimable: &BTreeMap<NetUid, I96F32>,
            netuid: NetUid,
        ) -> BTreeMap<NetUid, I96F32> {
            let mut result = claimable.clone();
            if result.contains_key(&netuid) {
                result.remove(&netuid);
            }
            result
        }

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, _)| true,
            |(hotkey, claimable)| (hotkey.clone(), claimable.clone()),
            |(hotkey, claimable)| {
                RootClaimable::<T>::insert(hotkey, filter_claimable(claimable, netuid))
            },
            1,
        );

        (
            read_all,
            last_item.map(|(hotkey, _)| RootClaimable::<T>::hashed_key_for(&hotkey)),
        )
    }

    /// Drop a dissolving subnet's LEGACY claimed watermarks (kept for `migrate_seed_beta_basket`).
    pub fn clean_up_root_claimed_for_subnet(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
    ) -> bool {
        clear_prefix_with_meter(weight_meter, T::DbWeight::get().writes(1), |limit| {
            RootClaimed::<T>::clear_prefix((netuid,), limit, None)
        })
    }

    /// Credit `amount` TAO onto the root pool's reserves. Root has no AMM pool, so whenever TAO is
    /// placed on root these three storages must be moved in lockstep by hand (subnets get this for
    /// free inside `swap_tao_for_alpha`). Single source of truth for that invariant.
    fn credit_root_reserves(amount: TaoBalance) {
        SubnetTAO::<T>::mutate(NetUid::ROOT, |total| *total = total.saturating_add(amount));
        SubnetAlphaOut::<T>::mutate(NetUid::ROOT, |total| {
            *total = total.saturating_add(u64::from(amount).into())
        });
        TotalStake::<T>::mutate(|total| *total = total.saturating_add(amount));
    }
}
