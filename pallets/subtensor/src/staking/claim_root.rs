use super::*;
use crate::weights::WeightInfo;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::{Weight, WeightMeter};
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_runtime::traits::AccountIdConversion;
use sp_std::collections::btree_map::BTreeMap;
use sp_std::collections::btree_set::BTreeSet;
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
pub(super) enum BasketFunding<'a, AccountId> {
    /// Dividend TAO realized by selling origin-subnet alpha. Swap fees are dropped. The sell
    /// drops `SubnetTAO[origin]` but leaves the free balance on the origin subnet account, so
    /// each destination slice must be physically moved from `origin_netuid` before that dest's
    /// reserves are credited — otherwise dest/root pots accrue an accounting surplus with no
    /// cash, and later `transfer_tao_from_subnet` on claim/unstake fails.
    Protocol { origin_netuid: NetUid },
    /// TAO transferred in from this user's balance; swap fees are charged.
    User(&'a AccountId),
}

/// Work actually performed by a fund-level root claim, used to size post-dispatch weight
/// (and aggregated across hotkeys for coldkey-wide claims).
#[derive(Default, Clone, Copy)]
pub struct RootClaimOutcome {
    /// TAO realized and staked back to root for the staker.
    pub tao: u64,
    /// Escrow holding rows scanned (each is a sim-swap valuation plus reads).
    pub rows: u32,
    /// Holdings actually redeemed (pro-rata take > 0: a swap plus stake writes).
    pub realized: u32,
    /// Dust holdings consolidated into the root slot (one swap each).
    pub swept: u32,
}

impl RootClaimOutcome {
    fn accumulate(&mut self, other: Self) {
        self.tao = self.tao.saturating_add(other.tao);
        self.rows = self.rows.saturating_add(other.rows);
        self.realized = self.realized.saturating_add(other.realized);
        self.swept = self.swept.saturating_add(other.swept);
    }
}

impl<T: Config> Pallet<T> {
    /// Reject basket / root-stake mutations and subnet dissolution while the
    /// `migrate_seed_beta_basket_v2` cursor is present or an epoch-allocated root dividend is
    /// still waiting to enter its basket. Deposits, claims, swaps, root stake
    /// add/remove/transfer, and dissolution hard-error here.
    pub(crate) fn ensure_beta_basket_seed_idle() -> Result<(), Error<T>> {
        ensure!(
            !crate::migrations::migrate_seed_beta_basket::seed_beta_basket_v2_in_progress::<T>()
                && DeferredRootAlphaDividends::<T>::iter().next().is_none(),
            Error::<T>::BetaBasketSeedInProgress
        );
        Ok(())
    }

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
    /// existing root weights plumbing. An empty result means the fund is *uncurated* —
    /// either no vector was ever stored, or explicit weights filtered to nothing — and
    /// dividends accumulate in place on the subnet they arrived on instead of being
    /// sold and redeployed (see [`Self::distribute_root_alpha_to_basket`]). Every
    /// returned weight is positive, so a non-empty vector always has a positive weight sum.
    pub fn get_valid_basket_weights(hotkey: &T::AccountId) -> Vec<(NetUid, u64)> {
        let maybe_uid = Uids::<T>::try_get(NetUid::ROOT, hotkey).ok();
        let weights = maybe_uid
            .map(|uid| Weights::<T>::get(NetUidStorageIndex::ROOT, uid))
            .unwrap_or_default();

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

    /// Shared tail of both dividend deposit flows: attribute the value added between real
    /// stakers and the fund's own escrow slot, mint fund shares at the pre-deposit NAV, and
    /// advance the per-validator claimable rate. Errors on a dust deposit so the caller rolls
    /// back and re-queues (or recycles only when the credit is unapportionable).
    pub(super) fn mint_basket_dividend_shares(
        hotkey: &T::AccountId,
        nav_before: u64,
        value_added: u64,
        total_root: u64,
        escrow_root: u64,
    ) -> DispatchResult {
        let shares_outstanding: u64 = BasketShares::<T>::get(hotkey);

        // Attribution: the dividend was earned by the whole root stake, escrow slot
        // included. Only the real stakers' fraction mints shares; the escrow slot's
        // fraction stays unminted so its value raises N/P for existing share holders
        // (the fund's own cash yield belongs to the fund).
        let stakers_value: u64 = Self::mul_div_u64(
            value_added,
            total_root,
            total_root.saturating_add(escrow_root),
        );

        // Mint fund shares at the pre-deposit NAV: shares = stakers_value * P / N. A
        // deposit into an already-compounded fund (N/P > 1) mints fewer shares than TAO
        // added, so N/P is left unchanged.
        let shares: u64 =
            Self::basket_shares_for_value(stakers_value, nav_before, shares_outstanding);

        // Per-staker claimable rate increment: fund shares per unit of root stake.
        let increment: I96F32 = I96F32::saturating_from_num(shares)
            .checked_div(I96F32::saturating_from_num(total_root))
            .unwrap_or(I96F32::saturating_from_num(0));

        // Dust deposit (shares or rate round to zero): roll everything back so
        // `Σ owed == BasketShares` is never broken by uncredited value. The caller
        // re-queues the credit for a later attempt.
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
    /// `funding` is the only difference between the two deposit paths: both must land free
    /// balance on each destination subnet account before that dest's reserves are credited.
    /// Protocol dividends move cash from the origin subnet pot (left there by the preceding
    /// sell); user deposits pull from the coldkey. Swap fees are dropped for protocol, charged
    /// for users.
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
    pub(super) fn deploy_tao_into_basket(
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

            match funding {
                BasketFunding::User(coldkey) => {
                    // Physically move the staker's TAO to the destination subnet account.
                    let transferred =
                        Self::transfer_tao_to_subnet(*dest_netuid, coldkey, tao_s.into())?;
                    ensure!(
                        transferred == TaoBalance::from(tao_s),
                        Error::<T>::InsufficientTaoBalance
                    );
                }
                BasketFunding::Protocol { origin_netuid } => {
                    // The origin sell left free TAO on the origin pot while dropping
                    // SubnetTAO[origin]. Move each slice onto the dest pot before crediting
                    // dest reserves. Same-subnet redeploy: the surplus already sits on dest.
                    if origin_netuid != *dest_netuid {
                        let dest_account = Self::get_subnet_account_id(*dest_netuid)
                            .ok_or(Error::<T>::SubnetNotExists)?;
                        Self::transfer_tao_from_subnet(origin_netuid, &dest_account, tao_s.into())?;
                    }
                }
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
                let drop_fees = matches!(funding, BasketFunding::Protocol { .. });
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
                    // dividend tolerates the dust slice and skips the escrow credit — the
                    // physical transfer (and any reserve bump from the zero-alpha swap)
                    // stays on dest as a pool donation, matching prior behaviour.
                    match funding {
                        BasketFunding::User(_) => return Err(Error::<T>::AmountTooLow.into()),
                        BasketFunding::Protocol { .. } => continue,
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
    /// An uncurated fund (no usable weight vector — dividends accumulate in place) has no
    /// vector to deploy a TAO deposit across, so the deposit *mirrors the fund*: it is
    /// deployed pro-rata across the current holdings by realizable value. A deposit then
    /// buys exactly the exposure the minted shares represent, existing holders' composition
    /// is untouched, and the deposit-then-claim round trip stays symmetric with redemption
    /// (claims redeem pro-rata of every holding) — without this, cycling cash deposits
    /// through claims would let anyone convert an uncurated fund's alpha into cash and push
    /// sell pressure through the escrow. An empty fund has nothing to mirror; that deposit
    /// is held as the fund's root (TAO cash) slot at NAV.
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
        Self::ensure_beta_basket_seed_idle()?;
        ensure!(
            Self::hotkey_account_exists(&hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        // Deposit queued dividend credits first so the share mint below prices against
        // the fund's full, current NAV.
        Self::flush_basket_deposits_for_hotkey(&hotkey);
        ensure!(tao >= DefaultMinStake::<T>::get(), Error::<T>::AmountTooLow);
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, tao.into()),
            Error::<T>::NotEnoughBalanceToStake
        );

        let mut valid = Self::get_valid_basket_weights(&hotkey);
        if valid.is_empty() {
            // Uncurated fund: mirror the fund — deploy pro-rata across current holdings by
            // realizable value (worthless rows carry no weight). Empty fund: nothing to
            // mirror, hold the deposit as the fund's root (TAO cash) slot.
            valid = Self::get_basket_holdings(&hotkey)
                .into_iter()
                .filter_map(|(netuid, alpha)| {
                    let value = Self::realizable_tao_for_alpha(netuid, alpha.to_u64());
                    (value > 0).then_some((netuid, value))
                })
                .collect();
            if valid.is_empty() {
                valid = vec![(NetUid::ROOT, 1)];
            }
        }

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
    /// Before redeeming, the fund's orphaned dust holdings (subnets outside the
    /// validator's current weight vector) are consolidated into its root slot (see
    /// [`Self::consolidate_dust_basket_holdings`]); consolidation commits even when the
    /// redemption below no-ops or rolls back, so stale holding rows — and with them every
    /// staker's per-row claim weight — decay instead of persisting forever.
    ///
    /// Returns a [`RootClaimOutcome`]: the TAO realized (zero for every no-op path) plus
    /// the work counters the dispatcher charges weight from.
    pub fn root_claim_for_hotkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum_condition: bool,
    ) -> Result<RootClaimOutcome, DispatchError> {
        let mut outcome = RootClaimOutcome::default();

        // Deposit any queued dividend credits first so the claim redeems against the
        // fund's full, current state. The flush work is scan-priced into the outcome.
        let (flush_work, _) = Self::flush_basket_deposits_for_hotkey(hotkey);
        outcome.rows = outcome
            .rows
            .saturating_add(u32::try_from(flush_work).unwrap_or(u32::MAX));

        let owed_shares: u64 = Self::get_basket_owed_shares(hotkey, coldkey);
        if owed_shares == 0 {
            return Ok(outcome); // no-op
        }

        let shares_total: u64 = BasketShares::<T>::get(hotkey);
        // Nothing realizable yet (fund drained); leave the watermark untouched so the claim can
        // pay out once the fund has value again.
        if shares_total == 0 {
            return Ok(outcome);
        }
        // A claim can never redeem more than the outstanding fund.
        let owed_shares = owed_shares.min(shares_total);

        // Consolidate dust holdings first, outside the redemption transaction, so the
        // cleanup sticks regardless of how the claim itself resolves.
        outcome.swept = Self::consolidate_dust_basket_holdings(hotkey);

        let holdings = Self::get_basket_holdings(hotkey);
        outcome.rows = holdings.len() as u32;

        // Dust check against the estimated payout (owed fraction of the marked NAV).
        // Same valuation as `get_validator_basket_nav_tao`, reusing the holdings read.
        let nav: u64 = holdings.iter().fold(0u64, |acc, (netuid, alpha)| {
            acc.saturating_add(Self::realizable_tao_for_alpha(*netuid, alpha.to_u64()))
        });
        let estimated_payout: u64 = Self::basket_payout_from(owed_shares, nav, shares_total);
        if !ignore_minimum_condition
            && I96F32::saturating_from_num(estimated_payout)
                < RootClaimableThreshold::<T>::get(NetUid::ROOT)
        {
            log::debug!(
                "root claim skipped (below threshold): payout={estimated_payout:?} h={hotkey:?} c={coldkey:?}"
            );
            return Ok(outcome); // no-op
        }
        if estimated_payout == 0 {
            return Ok(outcome);
        }

        // A claim that pays on some holdings but floors a valuable holding to take == 0 must
        // not settle: burning all owed shares would forfeit that holding to remaining
        // shareholders (e.g. 99/100 of a 1-alpha position → floor 0, then the leftover
        // 1-share holder owns the whole unit). Same posture as the all-zero-take rollback
        // below — leave the watermark untouched. Worthless rows (realizable 0) are ignored.
        for (netuid, slot_alpha) in holdings.iter() {
            let alpha = slot_alpha.to_u64();
            if alpha == 0 {
                continue;
            }
            if Self::mul_div_u64(alpha, owed_shares, shares_total) == 0
                && Self::realizable_tao_for_alpha(*netuid, alpha) > 0
            {
                return Ok(outcome);
            }
        }

        let escrow = Self::get_beta_escrow_account_id();

        // Redeemed slots are counted outside the transaction: a rolled-back redemption
        // still executed its swaps, so the work is charged either way.
        let realized = &mut outcome.realized;
        outcome.tao = with_transaction(|| {
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
                *realized = realized.saturating_add(1);

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

            // Claimed root stake must start (or refresh) the unlock hold, same as a
            // direct add_stake on root — otherwise JIT snipers can deposit → epoch →
            // claim → immediate remove_stake.
            Self::touch_root_stake_age(coldkey, hotkey);

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
        })?;

        Ok(outcome)
    }

    /// Consolidates a fund's *orphaned* dust holdings into its root (TAO cash) slot: every
    /// holding on a subnet the validator's current weight vector does not point at, whose
    /// realizable value is below `RootClaimableThreshold`, is sold in full and held as
    /// escrow root stake, deleting the holding row. Without this, dust rows live forever —
    /// a claim's pro-rata take floors to zero whenever `slot_alpha < P / owed`, so tiny
    /// holdings are never redeemed, yet every claim charges weight per holding row.
    ///
    /// Curated destinations are exempt: dividend deployment re-buys them every epoch, and a
    /// deliberate position must be allowed to compound past the threshold instead of being
    /// flattened to TAO on every claim. A curated dust row keeps charging the (cheap)
    /// per-row scan weight — the honest price of keeping it on the books.
    ///
    /// Uncurated funds have no exemptions: every sub-threshold row is swept. This does not
    /// fight the next epoch's accrual — dividend credits are queue-gated by the same
    /// threshold in `flush_basket_deposits_for_hotkey`, so a swept row only re-forms from
    /// a deposit the gate valued at or above the threshold. The gate marks at spot while
    /// this sweep marks realizable, so a just-above-threshold deposit can still land a row
    /// realizably below it and get re-swept next claim; that churn is bounded by the
    /// spot-vs-realizable gap on a threshold-sized amount, not a treadmill. Without the
    /// sweep an uncurated fund's holding count only ever grows, and every deposit's NAV
    /// sweep pays a quote per row forever.
    ///
    /// Consolidation is NAV-continuous (minus slippage on a sub-threshold amount) and
    /// touches no shares or watermarks. Best-effort per holding: a failed swap leaves the
    /// row for a later attempt. Returns the number of holdings converted.
    pub(crate) fn consolidate_dust_basket_holdings(hotkey: &T::AccountId) -> u32 {
        let threshold: u64 =
            RootClaimableThreshold::<T>::get(NetUid::ROOT).saturating_to_num::<u64>();
        if threshold == 0 {
            return 0;
        }

        let curated: BTreeSet<NetUid> = Self::get_valid_basket_weights(hotkey)
            .into_iter()
            .map(|(netuid, _)| netuid)
            .collect();

        let escrow = Self::get_beta_escrow_account_id();
        let mut swept: u32 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            if netuid.is_root() || curated.contains(&netuid) {
                continue;
            }
            if Self::realizable_tao_for_alpha(netuid, alpha.to_u64()) < threshold
                && Self::convert_basket_holding_to_root(hotkey, &escrow, netuid)
            {
                swept = swept.saturating_add(1);
            }
        }
        swept
    }

    /// Live existing-network count (including root). Ghost `NetworksAdded=false`
    /// leftovers are excluded so they cannot inflate the single-hotkey quote.
    pub(crate) fn root_claim_existing_networks() -> u32 {
        (Self::get_all_subnet_netuids().len() as u32).max(1)
    }

    /// Pre-dispatch work units for `claim_root_with_hotkey`: the live network
    /// count, or this hotkey's actual basket rows if leftover dissolved-net
    /// holdings outnumber existing networks. The hotkey is in the call data,
    /// so admission can cover every reachable row.
    pub(crate) fn root_claim_declared_work_for_hotkey(hotkey: &T::AccountId) -> u32 {
        let networks = Self::root_claim_existing_networks();
        let holdings = Self::get_basket_holdings(hotkey).len() as u32;
        networks
            .max(holdings)
            .max(1)
            .min(crate::MAX_ROOT_CLAIM_WORK)
    }

    /// Pre-dispatch work units for coldkey-wide `claim_root`. Call weight
    /// cannot see the signer, so this is the conservative
    /// [`crate::MAX_ROOT_CLAIM_WORK`] envelope; execution refuses a fat
    /// coldkey that would exceed it.
    pub(crate) fn root_claim_declared_work() -> u32 {
        crate::MAX_ROOT_CLAIM_WORK
    }

    /// True when a coldkey-wide claim's reachable work (hotkeys × existing
    /// networks, and the actual basket rows) fits the admission envelope.
    pub(crate) fn root_claim_fits_declared_budget(hotkeys: &[T::AccountId]) -> bool {
        let budget = Self::root_claim_declared_work();
        let networks = Self::root_claim_existing_networks();
        let hotkey_count = hotkeys.len() as u32;
        if hotkey_count.saturating_mul(networks) > budget {
            return false;
        }
        let mut rows: u32 = 0;
        for hotkey in hotkeys {
            rows = rows.saturating_add(Self::get_basket_holdings(hotkey).len() as u32);
            if rows > budget {
                return false;
            }
        }
        true
    }

    /// Actual post-dispatch weight of a root claim: full benchmark units for the slots
    /// that did real work (redeemed or swept — a swap plus stake writes each, floored at
    /// the hotkey count so walking empty hotkeys stays covered) plus the cheap per-row
    /// scan cost for holdings that were only valued. This is what lets a fund's claim fee
    /// decay as dust rows are consolidated, and makes a below-threshold no-op cost a scan
    /// instead of a full claim. Work above [`crate::MAX_ROOT_CLAIM_WORK`] is
    /// refused at dispatch (`RootClaimTooHeavy`) rather than admitted cheaply.
    pub(crate) fn root_claim_actual_weight(
        hotkey_count: u32,
        outcome: &RootClaimOutcome,
    ) -> Weight {
        let active = hotkey_count
            .max(outcome.realized.saturating_add(outcome.swept))
            .max(1);
        let scanned = outcome.rows.saturating_sub(outcome.realized);
        <T as crate::pallet::Config>::WeightInfo::claim_root(active).saturating_add(
            <T as crate::pallet::Config>::WeightInfo::claim_root_scan(scanned),
        )
    }

    pub fn do_root_claim(
        coldkey: T::AccountId,
        hotkeys: Vec<T::AccountId>,
    ) -> Result<RootClaimOutcome, DispatchError> {
        Self::ensure_beta_basket_seed_idle()?;
        with_transaction(|| match Self::try_do_root_claim(coldkey, &hotkeys) {
            Ok(outcome) => TransactionOutcome::Commit(Ok(outcome)),
            Err(err) => TransactionOutcome::Rollback(Err(err)),
        })
    }

    fn try_do_root_claim(
        coldkey: T::AccountId,
        hotkeys: &[T::AccountId],
    ) -> Result<RootClaimOutcome, DispatchError> {
        let mut total = RootClaimOutcome::default();
        for hotkey in hotkeys {
            let outcome = Self::root_claim_for_hotkey(hotkey, &coldkey, false)?;
            total.accumulate(outcome);
        }

        Self::deposit_event(Event::RootClaimed {
            coldkey,
            tao: total.tao.into(),
        });

        Ok(total)
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
    ///
    /// Returns the number of `BasketClaimed` plus queued `PendingBasketDeposits` rows moved
    /// so the caller can charge weight. Claimant rows are unbounded (same class of work as
    /// moving stake coldkeys): a popular root validator must still be able to hotkey-swap;
    /// the extrinsic pays the resulting weight rather than hard-failing above
    /// [`crate::MAX_ROOT_CLAIM_WORK`].
    pub fn transfer_basket_for_new_hotkey(
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
    ) -> u32 {
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

        // One row per historical coldkey — may be large; weight is charged by the caller.
        let claimed_entries: Vec<(T::AccountId, i128)> =
            BasketClaimed::<T>::iter_prefix(old_hotkey).collect();
        let mut moved_rows = claimed_entries.len() as u32;
        for (coldkey, claimed) in claimed_entries {
            BasketClaimed::<T>::remove(old_hotkey, &coldkey);
            BasketClaimed::<T>::mutate(new_hotkey, &coldkey, |c| {
                *c = c.saturating_add(claimed);
            });
        }

        // Queued dividend credits follow the fund. The clean-root guard doesn't inspect
        // the queue, so the new hotkey may hold threshold-deferred dust credits of its
        // own; per-origin amounts merge additively, which is exactly enqueue semantics.
        let pending: Vec<(NetUid, AlphaBalance)> =
            PendingBasketDeposits::<T>::drain_prefix(old_hotkey).collect();
        moved_rows = moved_rows.saturating_add(pending.len() as u32);
        for (netuid, alpha) in pending {
            PendingBasketDeposits::<T>::mutate(new_hotkey, netuid, |p| {
                *p = p.saturating_add(alpha);
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

        moved_rows
    }

    /// Converts validators' basket holdings on a dissolving subnet into each fund's root
    /// (TAO) slot, metered and resumable via `last_key` over [`BasketShares`] keys.
    ///
    /// Escrow alpha is sold once per fund and held as root stake under the same escrow.
    /// Fund shares, rates, and watermarks are untouched — NAV is continuous across the
    /// conversion (minus slippage). Best-effort: a failed swap is logged and the slot is
    /// left for generic teardown. Returns `(done, next_cursor)`.
    pub fn convert_subnet_basket_holdings_to_root(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        // Budget covers stake reads, AMM swap bookkeeping, TAO transfer, root-slot credit,
        // and the conversion event for one non-empty holding.
        let per_key = T::DbWeight::get().reads_writes(25, 20);
        let escrow = Self::get_beta_escrow_account_id();

        let mut keys = match &last_key {
            Some(raw_key) => BasketShares::<T>::iter_keys_from(raw_key.clone()),
            None => BasketShares::<T>::iter_keys(),
        };

        // Preserve the inbound cursor if this call cannot afford even one key, so a tight
        // weight budget does not rewind the scan and re-convert already-handled funds.
        let mut cursor = last_key;
        for hotkey in keys.by_ref() {
            if !weight_meter.can_consume(per_key) {
                return (false, cursor);
            }
            weight_meter.consume(per_key);
            Self::convert_basket_holding_to_root(&hotkey, &escrow, netuid);
            cursor = Some(BasketShares::<T>::hashed_key_for(hotkey));
        }

        (true, None)
    }

    /// Returns `true` when the holding was converted (false: nothing held, or the
    /// conversion rolled back on a failed swap).
    fn convert_basket_holding_to_root(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        netuid: NetUid,
    ) -> bool {
        let holding_alpha =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, netuid);
        if holding_alpha.is_zero() {
            return false;
        }

        with_transaction(|| {
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
        })
        .is_ok()
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
