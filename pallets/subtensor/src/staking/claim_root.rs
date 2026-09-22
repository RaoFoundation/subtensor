use super::basket_flush::{BasketFlushWork, MAX_BASKET_ROWS};
use super::*;
use crate::migrations::migrate_alpha_v2::retired::Alpha;
use crate::weights::WeightInfo;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::{Weight, WeightMeter};
use sp_core::Get;
use sp_runtime::DispatchError;
use sp_runtime::traits::{AccountIdConversion, Zero};
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::{I96F32, U64F64};
use subtensor_runtime_common::clear_prefix_with_meter;
use subtensor_swap_interface::{SwapFailureKind, SwapHandler};

/// A drained fund (shares outstanding but NAV marked at zero) may revive via a par mint
/// only when the stale shares are rounding dust: at most `value / DRAINED_FUND_DUST_DIVISOR`
/// (so stale holders capture <= ~1% of the reviving deposit).
const DRAINED_FUND_DUST_DIVISOR: u64 = 100;

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
    /// Work spent flushing the hotkey's pending dividend credits before redeeming (priced
    /// by `basket_flush_weight`, the model every flushing extrinsic shares).
    pub flush: BasketFlushWork,
}

impl RootClaimOutcome {
    fn accumulate(&mut self, other: Self) {
        self.tao = self.tao.saturating_add(other.tao);
        self.rows = self.rows.saturating_add(other.rows);
        self.realized = self.realized.saturating_add(other.realized);
        self.swept = self.swept.saturating_add(other.swept);
        self.flush = self.flush.saturating_add(other.flush);
    }
}

/// One fund row as a claim sees it before redeeming: its pre-sale realizable quote, whether
/// the pool is terminally shallow (written off instead of sold), and whether this claim
/// leaves it in the fund as dust (see [`Pallet::basket_row_is_claim_dust`]).
pub(crate) struct ValuedHolding {
    pub(crate) netuid: NetUid,
    pub(crate) alpha: AlphaBalance,
    /// Pre-sale realizable quote (what a sale of the whole row would fetch now).
    pub(crate) value: u64,
    /// The same, capped at the fast-anchor value of the alpha
    /// ([`Pallet::anchored_basket_holding_value`]): what a same-block pump cannot move.
    /// Used only to decide whether the row is dust for this claim.
    pub(crate) anchored: u64,
    pub(crate) terminal_garbage: bool,
    pub(crate) dust: bool,
}

impl<T: Config> Pallet<T> {
    /// Reject basket / root-stake mutations and subnet dissolution while the
    /// `migrate_seed_beta_basket_v2` cursor is present. Deposits, claims, swaps, root stake
    /// add/remove/transfer, and dissolution hard-error here.
    pub(crate) fn ensure_beta_basket_seed_idle() -> Result<(), Error<T>> {
        ensure!(
            !crate::migrations::migrate_seed_beta_basket::seed_beta_basket_v2_in_progress::<T>(),
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

    /// Flush recycles iff this is false. Keep when real stakers exist, or when
    /// escrow cash earned a dividend for an existing fund (shares > 0). A
    /// shareless escrow-only credit cannot mint (`value / 0` → 0) and must not
    /// sit in the queue forever.
    pub(crate) fn can_keep_basket_dividend(total_root: u64, escrow_root: u64, shares: u64) -> bool {
        total_root > 0 || (escrow_root > 0 && shares > 0)
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
        ensure!(
            Self::can_keep_basket_dividend(total_root, escrow_root, shares_outstanding),
            DispatchError::Other("basket deposit unapportionable")
        );

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

        // Escrow-only keep: same predicate as flush recycle. The credit already
        // raised the holding (and NAV); minting nothing is correct.
        let escrow_only = total_root == 0;
        ensure!(
            escrow_only || (shares > 0 && increment != I96F32::saturating_from_num(0)),
            DispatchError::Other("basket deposit too small")
        );

        if escrow_only {
            BasketDepositedTao::<T>::mutate(hotkey, |total| {
                *total = total.saturating_add(value_added.into())
            });
            Self::deposit_event(Event::BasketDeposited {
                hotkey: hotkey.clone(),
                tao: value_added.into(),
                shares: 0,
            });
            return Ok(());
        }

        // `nav_before == 0` with outstanding shares means `basket_shares_for_value`
        // took its dust-revival branch: this par mint starts a new fund life, so the
        // previous life's display baseline/TWR must not describe it.
        if nav_before == 0 && shares_outstanding > 0 {
            Self::retire_beta_display_state(hotkey);
        }

        BasketShares::<T>::mutate(hotkey, |p| *p = p.saturating_add(shares));
        BasketRate::<T>::mutate(hotkey, |rate| *rate = rate.saturating_add(increment));
        // Canonical staker total-return series (display state, see `BasketTwr`).
        Self::accrue_basket_twr(hotkey, stakers_value, total_root);
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

    /// How a direct deposit is split across a fund, given its valued holdings (from
    /// [`Self::try_valued_basket_holdings`]).
    ///
    /// A fund with holdings is *mirrored*: each holding's slice is its realizable TAO value
    /// as a share of NAV, so the deposit buys exactly the exposure the fund already has.
    /// This is what makes a deposit's new shares a fair claim on the existing portfolio: a
    /// depositor could otherwise depress one holding, mint shares at the lower NAV without
    /// buying any of it, and redeem the recovery. Inflows never change composition; only
    /// [`Self::do_swap_basket`] does.
    ///
    /// A fund with no holdings has nothing to mirror. Its first deposit is held as TAO in
    /// the fund's root (cash) slot, valued 1:1, so the fund opens at par with no trade and no
    /// price impact on any pool; it takes on subnet exposure only as dividends arrive as
    /// alpha or the validator trades.
    ///
    /// A holding whose realizable value is zero (terminal garbage or unpriceable dust) has
    /// no fair slice and is left out of the mirror: it contributes nothing to the NAV the
    /// shares are priced at, so the deposit buys nothing of it and the new shares are owed
    /// nothing from it. Before spec 468 one such row made every deposit into the fund fail
    /// with `AmountTooLow`. A fund whose every row is worthless is treated like an empty
    /// fund. Every returned slice weight is positive, so the weight sum is positive.
    pub(super) fn basket_deployment_split(
        valued_holdings: &[(NetUid, AlphaBalance, u64)],
    ) -> Result<Vec<(NetUid, u64)>, DispatchError> {
        let split: Vec<(NetUid, u64)> = valued_holdings
            .iter()
            .filter(|(_, _, value)| *value > 0)
            .map(|(netuid, _, value)| (*netuid, *value))
            .collect();
        if split.is_empty() {
            return Ok(vec![(NetUid::ROOT, 1)]);
        }
        Ok(split)
    }

    /// The direct-deposit engine: values the fund's holdings once, splits `tao` across them
    /// by realizable value ([`Self::basket_deployment_split`]), moves each slice from
    /// `coldkey` to the destination subnet account, buys that subnet's alpha into the escrow
    /// position (swap fees charged), and holds a root-slot slice as root stake (TAO at 1:1,
    /// mirroring `swap_tao_for_alpha`'s reserve bookkeeping by hand). The last slot absorbs
    /// the rounding remainder so the split sums exactly. Each buy is booked as protocol
    /// inflow: claims book the escrow's sells as outflow regardless of how the alpha
    /// entered, so entries must record the matching inflow or round trips skew the flow EMA.
    ///
    /// The deposit must buy every mirrored holding: a slice that rounds to zero TAO, or a
    /// buy that rounds to zero alpha, rejects the deposit (`AmountTooLow`) — otherwise a
    /// heavily discounted holding would reopen the free-repricing window at integer
    /// precision, or the deposit would silently donate TAO to a pool.
    ///
    /// Returns `(nav_before, value_added, valued_holdings)`: the realizable NAV snapshotted
    /// immediately before the buys (the same valuation the split is taken from), the
    /// realizable NAV the deployment actually added (ΔNAV, post-buy minus pre-buy), and the
    /// pre-buy valuation of every holding so the caller can tell which rows the mirror
    /// bought. Both snapshots are marked identically in the same block, so the difference
    /// isolates exactly this deposit's effect — the deposit bears its own buy slippage/fees
    /// (a realizable delta is bounded by the TAO deployed, never amplified by the buys' own
    /// price impact on existing holdings the way a spot delta would be).
    ///
    /// Not transactional by itself: the caller runs it inside `with_transaction` and rolls
    /// back on error.
    pub(super) fn deploy_tao_into_basket(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        tao: u64,
    ) -> Result<(u64, u64, Vec<(NetUid, AlphaBalance, u64)>), DispatchError> {
        let escrow = Self::get_beta_escrow_account_id();
        let valued_holdings = Self::try_valued_basket_holdings(hotkey)?;
        let nav_before: u64 = valued_holdings
            .iter()
            .fold(0u64, |nav, (_, _, value)| nav.saturating_add(*value));
        let split = Self::basket_deployment_split(&valued_holdings)?;
        let weight_sum: u64 = split
            .iter()
            .fold(0u64, |sum, (_, weight)| sum.saturating_add(*weight));

        let mut spent: u64 = 0;
        let last_idx = split.len().saturating_sub(1);
        for (i, (dest_netuid, weight)) in split.iter().enumerate() {
            // Last slot absorbs the rounding remainder so Σ tao_s == tao exactly.
            let tao_s: u64 = if i == last_idx {
                tao.saturating_sub(spent)
            } else {
                Self::mul_div_u64(tao, *weight, weight_sum)
            };
            ensure!(tao_s > 0, Error::<T>::AmountTooLow);
            spent = spent.saturating_add(tao_s);

            // Physically move the staker's TAO to the destination subnet account.
            let transferred = Self::transfer_tao_to_subnet(*dest_netuid, coldkey, tao_s.into())?;
            ensure!(
                transferred == TaoBalance::from(tao_s),
                Error::<T>::InsufficientTaoBalance
            );

            if dest_netuid.is_root() {
                Self::credit_root_slot(hotkey, &escrow, tao_s.into());
                continue;
            }
            let bought = Self::swap_basket_tao_for_alpha_chunks(*dest_netuid, tao_s.into())?;
            ensure!(!bought.is_zero(), Error::<T>::AmountTooLow);
            // Record the buy as protocol inflow (TAO entered the pool).
            Self::record_protocol_inflow(*dest_netuid, tao_s.into());
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                &escrow,
                *dest_netuid,
                bought,
            );
        }

        let nav_after = Self::try_get_validator_basket_nav_tao(hotkey)?;
        Ok((
            nav_before,
            nav_after.saturating_sub(nav_before),
            valued_holdings,
        ))
    }

    /// Stakes `tao` from `coldkey`'s free balance directly into a root-registered
    /// validator's basket. The TAO enters by the fund's *current holdings*: it is split
    /// across them pro-rata by realizable value (each holding's share of NAV) and buys each
    /// one, so the deposit acquires exactly the exposure the fund already has (see
    /// [`Self::basket_deployment_split`] for why anything else is exploitable). A fund with
    /// no holdings holds its first deposit as the root (TAO cash) slot at NAV. Inflows never
    /// change a fund's composition; only the validator does, deliberately, with
    /// [`Self::do_swap_basket`].
    ///
    /// The resulting fund shares are credited to the staker through their signed claimed
    /// watermark — `owed = rate * root_stake - claimed`, so a negative watermark credit is
    /// an unconditional share grant that needs no root stake and survives stake-change
    /// rebasing (which is additive).
    ///
    /// Shares are priced at the pre-buy realizable NAV, then capped by the smallest fraction
    /// of any existing holding that the deposit actually acquired. The cap is the economic
    /// invariant required by proportional-alpha redemption: an immediate claim cannot sell
    /// more units of any holding than the deposit bought. Any acquisition beyond that common
    /// fraction remains in the fund for existing holders. Unlike dividend deposits there is no
    /// attribution split among root stakers. `BasketRate` is untouched — direct shares buy fund
    /// exposure, they do not change any staker's dividend accrual.
    pub fn do_stake_into_basket(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        tao: TaoBalance,
    ) -> Result<Weight, DispatchError> {
        Self::do_stake_into_basket_tracked(coldkey, hotkey, tao).map_err(|(_, err)| err)
    }

    /// The read-only preconditions of `stake_into_basket`: seed idle, hotkey exists and is
    /// on root, the caller's `StakingHotkeys` can grow, amount at least the minimum, and
    /// the caller can pay. Nothing is written before they pass, so a deposit refused here
    /// is charged [`Self::stake_into_basket_precheck_weight`] only.
    pub(crate) fn check_stake_into_basket(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        tao: TaoBalance,
    ) -> Result<(), Error<T>> {
        Self::ensure_beta_basket_seed_idle()?;
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );
        // Direct deposits open per-(caller, validator) entitlement state. Restricting
        // the target to a live root uid caps the validator axis at MaxAllowedUids on
        // root, the same bound as a normal root-stake position.
        ensure!(
            Self::is_hotkey_registered_on_network(NetUid::ROOT, hotkey),
            Error::<T>::HotKeyNotRegisteredInSubNet
        );
        // A deposit registers the validator in the caller's `StakingHotkeys` (claims walk it).
        Self::ensure_staking_hotkeys_can_grow(coldkey, hotkey)?;
        ensure!(tao >= DefaultMinStake::<T>::get(), Error::<T>::AmountTooLow);
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, tao.into()),
            Error::<T>::NotEnoughBalanceToStake
        );
        Ok(())
    }

    /// Reads [`Self::check_stake_into_basket`] performs at most: the seed state, the
    /// hotkey owner, root membership, the caller's `StakingHotkeys`, the minimum stake,
    /// and the caller's account.
    pub fn stake_into_basket_precheck_weight() -> Weight {
        T::DbWeight::get().reads(8)
    }

    /// [`Self::do_stake_into_basket`] that also reports the weight of the work a failed
    /// deposit did — its pre-checks, or the flush and the rolled-back deployment — so the
    /// dispatcher charges that instead of the declared 256-slot envelope.
    pub fn do_stake_into_basket_tracked(
        coldkey: T::AccountId,
        hotkey: T::AccountId,
        tao: TaoBalance,
    ) -> Result<Weight, (Weight, DispatchError)> {
        let precheck = Self::stake_into_basket_precheck_weight();
        Self::check_stake_into_basket(&coldkey, &hotkey, tao)
            .map_err(|err| (precheck, err.into()))?;
        // Deposit queued dividend credits first so the share mint below prices against
        // the fund's full, current NAV. The flush work is priced into the post-dispatch
        // weight; the declared weight carries its flat allowance.
        let (flush_work, _, _) = Self::flush_basket_deposits_for_hotkey(&hotkey);
        let flush_weight = Self::basket_flush_weight(flush_work);

        // The deployment slots are the holdings the deposit mirrors, or the single root
        // cash slot that opens an empty fund. Each slot can add at most one new holding, so
        // pre-deploy holdings plus the slot count bounds the holdings the two NAV
        // valuations will sweep.
        let holdings = Self::get_basket_holdings(&hotkey);
        let num_slots = (holdings.len() as u64).max(1);
        let num_holdings = (holdings.len() as u64).saturating_add(num_slots);
        // A rolled-back deployment still valued the fund and bought up to every slot
        // before failing; charge the whole deployment over the real counts.
        let failed_weight = Self::stake_into_basket_weight(num_slots, num_holdings)
            .saturating_add(flush_weight)
            .saturating_add(precheck);

        with_transaction(
            || match Self::try_stake_into_basket(&coldkey, &hotkey, tao, &holdings) {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            },
        )
        .map_err(|err| (failed_weight, err))?;

        // A fund's very first successful mint stamps its frozen display baseline
        // (index splice). No-op (one read) for every later deposit.
        let stamp_work = Self::stamp_beta_baseline_if_new(&hotkey);

        Ok(
            Self::stake_into_basket_weight(num_slots, num_holdings.saturating_add(stamp_work))
                .saturating_add(flush_weight),
        )
    }

    /// Transactional body of [`Self::do_stake_into_basket`]; any error rolls the whole
    /// deposit back, including the balance transfers.
    fn try_stake_into_basket(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        tao: TaoBalance,
        holdings_before: &[(NetUid, AlphaBalance)],
    ) -> DispatchResult {
        let shares_outstanding: u64 = BasketShares::<T>::get(hotkey);

        // Deploy the staker's TAO across the basket by its current holdings. Price the
        // result by its ΔNAV, then apply the per-holding quantity bound below.
        let (nav_before, value_added, valued_before) =
            Self::deploy_tao_into_basket(hotkey, coldkey, tao.to_u64())?;

        let nav_priced_shares: u64 =
            Self::basket_shares_for_value(value_added, nav_before, shares_outstanding);
        // Full-liquidation NAV is nonlinear: allocating TAO by holding value does not
        // necessarily buy the same asset fraction in every pool. Redemption, however, takes
        // the same share fraction of every holding. Bound the mint by the least-covered
        // holding so an immediate redemption cannot sell more units than this deposit added:
        //
        //     minted / P <= added_i / held_i
        //
        // which is equivalent to the post-mint redemption condition
        // `minted / (P + minted) * (held_i + added_i) <= added_i`.
        //
        // Rows the mirror left out because they realize nothing (see
        // `basket_deployment_split`) carry no value the new shares could redeem, so they
        // do not bound the mint.
        let shares = if holdings_before.is_empty() || shares_outstanding == 0 {
            nav_priced_shares
        } else {
            let escrow = Self::get_beta_escrow_account_id();
            valued_before
                .iter()
                .filter(|(_, _, value)| *value > 0)
                .fold(nav_priced_shares, |covered, (netuid, held_before, _)| {
                    let held_after =
                        Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, *netuid)
                            .to_u64();
                    let added = held_after.saturating_sub(held_before.to_u64());
                    covered.min(Self::mul_div_u64(
                        added,
                        shares_outstanding,
                        held_before.to_u64(),
                    ))
                })
        };
        ensure!(shares > 0, Error::<T>::AmountTooLow);

        // `nav_before == 0` with outstanding shares means `basket_shares_for_value`
        // took its dust-revival branch: this par mint starts a new fund life, so the
        // previous life's display baseline/TWR must not describe it.
        if nav_before == 0 && shares_outstanding > 0 {
            Self::retire_beta_display_state(hotkey);
        }

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

    /// Actual weight of a `stake_into_basket` call that deployed across `num_slots`
    /// deployment slots (the mirrored holdings, or the root cash slot of an empty fund) with
    /// `num_holdings` basket holdings. Per slot: a balance transfer to the subnet
    /// account, a swap, the escrow stake write, and protocol-flow bookkeeping. Per holding:
    /// two `sim_swap` valuations (the `nav_before` / `nav_after` sweeps), plus one stake-position
    /// lookup to verify the quantity acquired for direct-deposit share issuance.
    pub fn stake_into_basket_weight(num_slots: u64, num_holdings: u64) -> Weight {
        Weight::from_parts(25_000_000, 4000)
            .saturating_add(T::DbWeight::get().reads(6_u64))
            .saturating_add(T::DbWeight::get().writes(5_u64))
            .saturating_mul(num_slots.max(1))
            .saturating_add(Self::basket_nav_sweep_weight(num_holdings))
            .saturating_add(T::DbWeight::get().reads(num_holdings.saturating_mul(5_u64)))
            .saturating_add(T::DbWeight::get().reads_writes(8_u64, 6_u64))
    }

    /// Pre-dispatch weight of `stake_into_basket`: a cap sized for the row cap of holdings
    /// as deployment slots over the row cap of holdings, plus the flat pending-deposit flush allowance
    /// ([`Self::basket_flush_weight_bound`]) shared by every extrinsic that flushes. Refunded
    /// to actual post-dispatch.
    pub fn stake_into_basket_declared_weight() -> Weight {
        Self::stake_into_basket_weight(MAX_BASKET_ROWS, MAX_BASKET_ROWS)
            .saturating_add(Self::basket_flush_weight_bound())
    }

    /// Weight of one realizable-NAV sweep over `num_holdings` escrow rows: one sim-swap
    /// valuation plus reads per row. Shared by every basket path that values the fund.
    pub(crate) fn basket_nav_sweep_weight(num_holdings: u64) -> Weight {
        Weight::from_parts(10_000_000, 1000)
            .saturating_add(T::DbWeight::get().reads(4_u64))
            .saturating_mul(num_holdings.max(1))
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
    /// Redemption is fund-level and NAV-proportional: the staker's owed shares define a
    /// fraction `f = owed / P` of the fund's full-liquidation NAV. Exactly that fraction of
    /// every holding's alpha is removed and sold, but the claimant receives at most `f` of
    /// that holding's pre-sale realizable value. A concave AMM curve makes selling `f` of a
    /// holding realize more than `f` of the proceeds from selling the whole holding; that
    /// surplus is retained in the fund's root (TAO cash) slot for the remaining shareholders.
    /// The root-slot portion is reassigned directly because it is already TAO. A holding on a
    /// terminally shallow pool is removed as an explicit pro-rata write-off instead of aborting
    /// healthy slots; unknown swap or accounting errors still roll back the claim.
    ///
    /// Before redeeming, the fund's dust holdings (subnet rows realizably below the claim
    /// threshold) are consolidated into its root slot (see
    /// [`Self::consolidate_dust_basket_holdings`]); consolidation commits even when the
    /// redemption below no-ops or rolls back, so stale holding rows — and with them every
    /// staker's per-row claim weight — decay instead of persisting forever.
    ///
    /// Dust rows are not redeemed. A subnet row is skipped for this claim when the fund's
    /// whole holding on it is worth less than `min(`[`BasketClaimRowDustCapTao`]`,
    /// `[`BasketClaimRowDustBps`]` × anchored NAV)`, or the claimant's pro-rata slice of it
    /// is worth less than [`BasketClaimSliceDustTao`] — and, whichever rule matched, only if
    /// that slice is worth at most [`BasketClaimForfeitCapTao`]. All three are read at the
    /// anchored mark ([`Self::anchored_basket_holding_value`]: the live quote capped at the
    /// fast-EMA anchor `swap_basket` already uses). A skipped row is neither sold nor paid.
    /// The claim still burns the whole entitlement, so the claimant's slice of a skipped row
    /// stays in the fund for the remaining holders (`BasketClaimDustSkipped` reports an
    /// estimate). Hard guarantee: no single skipped slice exceeds the forfeit cap at the
    /// anchored mark; its live value can exceed the anchor only by the anchor gap (a 2 h
    /// EMA's lag). No price enters the share accounting: the mark only decides *whether* a
    /// slice is sold, never how many shares a claim burns or what anyone else is owed, so
    /// there is nothing to pump.
    /// The root cash slot (TAO 1:1, no swap) and terminal write-offs are never skipped, and
    /// a claimant redeeming the whole fund skips nothing — there is nobody left to hold the
    /// rest. Zero thresholds turn the skip off.
    ///
    /// Returns a [`RootClaimOutcome`]: the TAO realized (zero for every no-op path) plus
    /// the work counters the dispatcher charges weight from.
    pub fn root_claim_for_hotkey(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum_condition: bool,
    ) -> Result<RootClaimOutcome, DispatchError> {
        let mut outcome = RootClaimOutcome::default();
        Self::root_claim_for_hotkey_into(hotkey, coldkey, ignore_minimum_condition, &mut outcome)?;
        Ok(outcome)
    }

    /// [`Self::root_claim_for_hotkey`] writing its work counters into `outcome` as it goes,
    /// so a claim that fails still reports the flush, scan and swaps it performed.
    fn root_claim_for_hotkey_into(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        ignore_minimum_condition: bool,
        outcome: &mut RootClaimOutcome,
    ) -> DispatchResult {
        // Deposit any queued dividend credits first so the claim redeems against the
        // fund's full, current state. The flush work is priced into the outcome.
        let (flush_work, _, _) = Self::flush_basket_deposits_for_hotkey(hotkey);
        outcome.flush = flush_work;

        let owed_shares: u64 = Self::get_basket_owed_shares(hotkey, coldkey);
        if owed_shares == 0 {
            return Ok(()); // no-op
        }

        let shares_total: u64 = BasketShares::<T>::get(hotkey);
        // Nothing realizable yet (fund drained); leave the watermark untouched so the claim can
        // pay out once the fund has value again.
        if shares_total == 0 {
            return Ok(());
        }
        // A claim can never redeem more than the outstanding fund.
        let owed_shares = owed_shares.min(shares_total);

        // Consolidate dust holdings first, outside the redemption transaction, so the
        // cleanup sticks regardless of how the claim itself resolves.
        outcome.swept = Self::consolidate_dust_basket_holdings(hotkey);

        // Count the rows before valuing them: a valuation that fails mid-way (an unknown
        // swap error on one pool) still scanned every row up to it, and the refund on that
        // failure must charge the scan, not report zero rows.
        let holdings = Self::get_basket_holdings(hotkey);
        outcome.rows = holdings.len() as u32;
        let valued_holdings = Self::plan_basket_claim_rows(holdings, owed_shares, shares_total)?;

        let has_terminal_garbage = valued_holdings.iter().any(|row| row.terminal_garbage);
        let dust_rows: u32 = valued_holdings.iter().filter(|row| row.dust).count() as u32;
        // What the skipped slices would have paid at the pre-sale quote. Informational only
        // (the event): nothing in the accounting below depends on it. The claim burns the
        // whole entitlement, so this value stays in the fund for the remaining holders.
        let forfeited_est: u64 =
            valued_holdings
                .iter()
                .filter(|row| row.dust)
                .fold(0u64, |acc, row| {
                    acc.saturating_add(Self::basket_payout_from(
                        owed_shares,
                        row.value,
                        shares_total,
                    ))
                });
        // Threshold check against the payout the claim can actually make: the owed fraction
        // of the redeemed (non-dust) rows' realizable value. A claim that would sell nothing
        // is a no-op that burns nothing.
        let redeemable_nav: u64 = valued_holdings
            .iter()
            .filter(|row| !row.dust)
            .fold(0u64, |acc, row| acc.saturating_add(row.value));
        let estimated_payout: u64 =
            Self::basket_payout_from(owed_shares, redeemable_nav, shares_total);
        if !ignore_minimum_condition
            && !has_terminal_garbage
            && I96F32::saturating_from_num(estimated_payout)
                < RootClaimableThreshold::<T>::get(NetUid::ROOT)
        {
            log::debug!(
                "root claim skipped (below threshold): payout={estimated_payout:?} h={hotkey:?} c={coldkey:?}"
            );
            return Ok(()); // no-op
        }
        if estimated_payout == 0 && !has_terminal_garbage {
            return Ok(());
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
            let mut claimant_swapped_tao: u64 = 0;
            let mut retained_swapped_tao: u64 = 0;
            let mut written_off: u32 = 0;

            for ValuedHolding {
                netuid,
                alpha: slot_alpha,
                value: slot_value,
                terminal_garbage,
                dust,
                ..
            } in valued_holdings.iter()
            {
                // A dust row is left whole in the fund: no sale, no payout, no stake write.
                if *dust {
                    continue;
                }
                let slot_entitlement =
                    Self::basket_payout_from(owed_shares, *slot_value, shares_total);
                // This staker's pro-rata slice of the holding: slot_alpha * owed / P.
                let proportional_take =
                    Self::mul_div_u64(slot_alpha.to_u64(), owed_shares, shares_total);
                // A high-value alpha row can owe at least one rao even when its proportional
                // alpha slice floors to zero. Sell one atomic alpha unit, pay no more than the
                // marked entitlement below, and retain the sale surplus as fund root cash.
                // Root is already denominated in rao, so take == entitlement there; terminal
                // rows have no realizable entitlement and keep the ordinary floor.
                let take = if proportional_take == 0
                    && slot_entitlement > 0
                    && !netuid.is_root()
                    && !terminal_garbage
                {
                    1
                } else {
                    proportional_take
                };
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

                if *terminal_garbage {
                    Self::burn_subnet_alpha(*netuid, take.into());
                    Self::deposit_event(Event::BasketAlphaWrittenOff {
                        hotkey: hotkey.clone(),
                        netuid: *netuid,
                        alpha: take.into(),
                    });
                    written_off = written_off.saturating_add(1);
                    continue;
                }

                // Sell the slice to TAO.
                let tao = match Self::sell_basket_alpha_for_root_tao(*netuid, take.into()) {
                    Ok(tao) => tao,
                    Err(err)
                        if T::SwapInterface::classify_failure(&err)
                            == SwapFailureKind::TerminalLiquidity =>
                    {
                        // The sale helper rolls back all of its chunks on failure. The stake
                        // decrease above remains in this outer transaction, so the exact slice
                        // can be written off without disturbing any healthy slot.
                        Self::burn_subnet_alpha(*netuid, take.into());
                        Self::deposit_event(Event::BasketAlphaWrittenOff {
                            hotkey: hotkey.clone(),
                            netuid: *netuid,
                            alpha: take.into(),
                        });
                        written_off = written_off.saturating_add(1);
                        continue;
                    }
                    Err(err) => return TransactionOutcome::Rollback(Err(err)),
                };

                // Record root sell (reduces protocol cost).
                SubnetRootSellTao::<T>::mutate(*netuid, |total| {
                    *total = total.saturating_add(tao);
                });

                // Shares are minted and quoted against full-liquidation NAV. Selling a raw
                // alpha fraction on a concave AMM curve realizes more than the same NAV
                // fraction, so pay only the priced entitlement and retain the surplus as
                // fund cash. Otherwise a permissionless deposit followed by a claim can
                // extract the difference from earlier holders.
                let realized_tao = tao.to_u64();
                // A final claimant has no remaining holders to retain a surplus for. Give
                // them every realized rao so no root cash is stranded behind zero shares.
                let claimant_tao = if owed_shares == shares_total {
                    realized_tao
                } else {
                    realized_tao.min(slot_entitlement)
                };
                claimant_swapped_tao = claimant_swapped_tao.saturating_add(claimant_tao);
                retained_swapped_tao =
                    retained_swapped_tao.saturating_add(realized_tao.saturating_sub(claimant_tao));
            }

            let total_tao: u64 = root_slot_tao.saturating_add(claimant_swapped_tao);

            // Nothing was actually realized (every per-holding take floored to zero, or the
            // swaps returned zero TAO). The marked estimate above can be positive while the raw
            // alpha takes floor to zero (high-price, tiny-alpha holdings), so this must NOT
            // settle: roll back and leave the watermark untouched, otherwise the staker's owed
            // shares would be burned for a zero payout.
            if total_tao == 0 && written_off == 0 {
                return TransactionOutcome::Rollback(Ok(0));
            }

            // The sale surplus still belongs to the fund. It already landed in the root
            // subnet account, so book it into the fund's root cash slot before burning the
            // claimant's shares. Together, the remaining alpha and this cash retain the
            // unclaimed fraction of the pre-sale liquidation NAV (modulo integer floors).
            if retained_swapped_tao > 0 {
                Self::credit_root_slot(hotkey, &escrow, retained_swapped_tao.into());
            }

            // Stake the redeemed TAO on root for the staker. Only sold TAO is new on root;
            // the root-slot portion was already counted in the root reserves.
            if total_tao > 0 {
                Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey,
                    coldkey,
                    NetUid::ROOT,
                    total_tao.into(),
                );
            }
            if claimant_swapped_tao > 0 {
                Self::credit_root_reserves(claimant_swapped_tao.into());
            }

            // Claimed root stake must start (or refresh) the unlock hold, same as a
            // direct add_stake on root — otherwise JIT snipers can deposit → epoch →
            // claim → immediate remove_stake.
            if total_tao > 0 {
                Self::touch_root_stake_age(coldkey, hotkey);
            }

            // The staker's root stake just grew; rebase their claimed watermark so the new stake
            // does not retroactively inflate their claimable.
            if total_tao > 0 {
                Self::add_stake_adjust_root_claimed_for_hotkey_and_coldkey(
                    hotkey, coldkey, total_tao,
                );
            }

            // Consume the claimed shares and advance the watermark. The whole entitlement
            // is burned, skipped rows included: the fund keeps exactly `owed / P` of every
            // skipped row for its remaining holders, and no price enters this accounting.
            let remaining = BasketShares::<T>::mutate(hotkey, |p| {
                *p = p.saturating_sub(owed_shares);
                *p
            });
            if remaining == 0 {
                // This fund life just ended; retire its display baseline/TWR so a
                // future revival stamps fresh instead of inheriting a stale splice.
                Self::retire_beta_display_state(hotkey);
            }
            BasketClaimed::<T>::mutate(hotkey, coldkey, |claimed| {
                *claimed = claimed.saturating_add(i128::from(owed_shares));
            });
            BasketRedeemedTao::<T>::mutate(hotkey, |total| {
                *total = total.saturating_add(total_tao.into())
            });

            if dust_rows > 0 {
                Self::deposit_event(Event::BasketClaimDustSkipped {
                    hotkey: hotkey.clone(),
                    coldkey: coldkey.clone(),
                    rows: dust_rows,
                    forfeited_tao_est: forfeited_est.into(),
                });
            }
            Self::deposit_event(Event::BasketClaimed {
                hotkey: hotkey.clone(),
                coldkey: coldkey.clone(),
                tao: total_tao.into(),
            });

            TransactionOutcome::Commit(Ok::<u64, DispatchError>(total_tao))
        })?;

        Ok(())
    }

    /// Every row of `hotkey`'s fund as a claim of `owed_shares` out of `shares_total` would
    /// see it: pre-sale realizable quote, anchored value, terminal flag, and whether the claim
    /// skips it as dust. Shared by the claim itself and the claim preview view, so the SDK
    /// never re-implements the dust rules.
    ///
    /// Keep each slot's pre-sale value as well as the total: redemption caps every slot
    /// independently at the same NAV fraction. Without that cap, selling a raw alpha
    /// fraction on a concave AMM curve overpays the first redeemer and transfers the loss to
    /// the remaining shareholders.
    ///
    /// Dust rows are decided at the anchored mark (realizable capped at the fast-EMA value
    /// of the alpha, so a same-block pump cannot lift a row out of the dust band and a young
    /// subnet is not written down to a months-slow EMA), against a row floor that scales
    /// with the fund's anchored NAV. A claimant taking the whole fund skips nothing.
    pub(crate) fn plan_basket_claim(
        hotkey: &T::AccountId,
        owed_shares: u64,
        shares_total: u64,
    ) -> Result<Vec<ValuedHolding>, DispatchError> {
        Self::plan_basket_claim_rows(Self::get_basket_holdings(hotkey), owed_shares, shares_total)
    }

    /// [`Self::plan_basket_claim`] over an already-read set of holdings.
    fn plan_basket_claim_rows(
        holdings: Vec<(NetUid, AlphaBalance)>,
        owed_shares: u64,
        shares_total: u64,
    ) -> Result<Vec<ValuedHolding>, DispatchError> {
        let mut valued_holdings: Vec<ValuedHolding> = Vec::new();
        let mut anchored_nav: u64 = 0;
        for (netuid, alpha) in holdings {
            let (value, terminal_garbage) =
                match Self::try_realizable_tao_for_alpha(netuid, alpha.to_u64())? {
                    Some(value) => (value, false),
                    None => (0, true),
                };
            let anchored = Self::anchored_basket_holding_value(netuid, alpha.to_u64(), value);
            anchored_nav = anchored_nav.saturating_add(anchored);
            valued_holdings.push(ValuedHolding {
                netuid,
                alpha,
                value,
                anchored,
                terminal_garbage,
                dust: false,
            });
        }
        if owed_shares < shares_total {
            let row_dust = Self::basket_claim_row_dust_floor(anchored_nav);
            let slice_dust: u64 = BasketClaimSliceDustTao::<T>::get();
            let forfeit_cap: u64 = BasketClaimForfeitCapTao::<T>::get();
            for row in valued_holdings.iter_mut() {
                row.dust = !row.netuid.is_root()
                    && !row.terminal_garbage
                    && Self::basket_row_is_claim_dust(
                        row.anchored,
                        owed_shares,
                        shares_total,
                        row_dust,
                        slice_dust,
                        forfeit_cap,
                    );
            }
        }
        Ok(valued_holdings)
    }

    /// The anchored mark of `alpha` on `netuid` given its `realizable` quote:
    /// `min(realizable, alpha × fast EMA)`, the fast anchor ([`SubnetFastMovingPrice`], 2 h
    /// half-life, seeded at spot) that `swap_basket` already bounds its prices with. Nothing
    /// inside a block can move it, and it follows a young or rallying subnet within hours —
    /// unlike the slow (monthly) EMA behind [`Self::guarded_basket_holding_value`], which
    /// starts near zero for a new subnet and would mark its rows as worthless for months.
    /// Falls back to the slow-guarded mark while the fast series is unseeded. Root cash is
    /// TAO 1:1 and passes through.
    pub fn anchored_basket_holding_value(netuid: NetUid, alpha: u64, realizable: u64) -> u64 {
        if netuid.is_root() {
            return realizable;
        }
        match SubnetFastMovingPrice::<T>::get(netuid) {
            Some(fast) => realizable.min(
                fast.saturating_mul(U64F64::saturating_from_num(alpha))
                    .saturating_to_num::<u64>(),
            ),
            None => Self::guarded_basket_holding_value(netuid, alpha, realizable),
        }
    }

    /// The row-dust floor of a claim on a fund whose anchored NAV is `anchored_nav`:
    /// `min(`[`BasketClaimRowDustCapTao`]`, `[`BasketClaimRowDustBps`]` × anchored_nav)`.
    /// Relative so a small fund's rows are judged against its own size, capped so a large
    /// fund never skips a row worth more than the cap. Zero when either knob is zero.
    pub fn basket_claim_row_dust_floor(anchored_nav: u64) -> u64 {
        let cap: u64 = BasketClaimRowDustCapTao::<T>::get();
        let bps: u64 = u64::from(BasketClaimRowDustBps::<T>::get());
        cap.min(Self::mul_div_u64(anchored_nav, bps, 10_000))
    }

    /// Whether a claim of `owed_shares` out of `shares_total` leaves a row worth `anchored`
    /// (at [`Self::anchored_basket_holding_value`]) in the fund as dust: the whole row is
    /// worth less than `row_dust`, or the claimant's pro-rata slice of it is worth less than
    /// `slice_dust` — **and** that slice is worth at most `forfeit_cap`, whichever rule
    /// matched. The cap is the hard bound on what one skipped slice can leave in the fund:
    /// a claimant with a large slice of a small row sells it as before. Zero thresholds never
    /// match; a zero cap turns every skip off, including for slices whose anchored value
    /// rounds to zero. Callers exclude the root cash slot and terminal write-offs.
    pub fn basket_row_is_claim_dust(
        anchored: u64,
        owed_shares: u64,
        shares_total: u64,
        row_dust: u64,
        slice_dust: u64,
        forfeit_cap: u64,
    ) -> bool {
        // A zero cap is a hard off-switch: without the explicit check a slice whose anchored
        // value rounds to zero (positive live entitlement, anchor far below it) would pass
        // `slice > 0 == false` and still be skipped.
        if forfeit_cap == 0 {
            return false;
        }
        let slice = Self::basket_payout_from(owed_shares, anchored, shares_total);
        if slice > forfeit_cap {
            return false;
        }
        (row_dust > 0 && anchored < row_dust) || (slice_dust > 0 && slice < slice_dust)
    }

    /// Consolidates a fund's dust holdings into its root (TAO cash) slot: every subnet
    /// holding whose realizable value is below `RootClaimableThreshold` is sold in full and
    /// held as escrow root stake, deleting the holding row. Without this, dust rows live
    /// forever — a claim's pro-rata take floors to zero whenever `slot_alpha < P / owed`, so
    /// tiny holdings are never redeemed, yet every claim charges weight per holding row.
    ///
    /// The rule is keyed off the actual holdings alone; there is no exempt set. A holding
    /// below the claim threshold is by definition too small to pay any claimant, so nothing
    /// deliberate is lost by cashing it: a position bought with `swap_basket` is at least
    /// `DefaultMinStake` at entry, far above the threshold, and only reaches it after its
    /// value collapses. Nor does the sweep fight the next epoch's accrual — dividend credits
    /// are queue-gated by the same threshold in `flush_basket_deposits_for_hotkey`, so a
    /// swept row only re-forms from a deposit the gate valued at or above the threshold. The
    /// gate marks at spot while this sweep marks realizable, so a just-above-threshold
    /// deposit can still land a row realizably below it and get re-swept next claim; that
    /// churn is bounded by the spot-vs-realizable gap on a threshold-sized amount, not a
    /// treadmill. Without the sweep a fund's holding count only ever grows, and every
    /// deposit's NAV sweep pays a quote per row forever.
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

        let escrow = Self::get_beta_escrow_account_id();
        let mut swept: u32 = 0;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            if netuid.is_root() {
                continue;
            }
            match Self::try_realizable_tao_for_alpha(netuid, alpha.to_u64()) {
                Ok(Some(value)) if value < threshold => {
                    if Self::convert_basket_holding_to_root(hotkey, &escrow, netuid) {
                        swept = swept.saturating_add(1);
                    }
                }
                // A terminally shallow pool cannot recover merely by retrying this sell.
                // Convert the row through the same explicit write-off path used by claims.
                Ok(None) => {
                    if Self::convert_basket_holding_to_root(hotkey, &escrow, netuid) {
                        swept = swept.saturating_add(1);
                    }
                }
                Ok(Some(_)) => {}
                Err(err) => {
                    // Unknown failures remain retryable: do not silently value or delete the
                    // holding as zero.
                    log::warn!("Error valuing basket holding for dust conversion: {err:?}");
                }
            }
        }
        swept
    }

    /// Fixed admission budget for a coldkey-wide claim.
    pub fn root_claim_declared_work() -> u32 {
        crate::MAX_ROOT_CLAIM_WORK
    }

    /// Fixed admission budget for [`Pallet::claim_root_with_hotkey`].
    pub fn root_claim_hotkey_declared_work() -> u32 {
        crate::MAX_ROOT_CLAIM_HOTKEY_WORK
    }

    /// Weight of a claim over `units` hotkeys-plus-holdings (full claim work plus scan-only
    /// work for every unit) that flushes `flush` queued-deposit work first.
    pub fn root_claim_weight_for_work(units: u32, flush: BasketFlushWork) -> Weight {
        <T as crate::pallet::Config>::WeightInfo::claim_root(units)
            .saturating_add(<T as crate::pallet::Config>::WeightInfo::claim_root_scan(
                units,
            ))
            .saturating_add(Self::basket_flush_weight(flush))
    }

    /// Pre-dispatch weight for every independently bounded dimension: full claim work,
    /// scan-only work, and the flat pending-deposit flush allowance
    /// ([`Self::basket_flush_weight_bound`]) shared by every extrinsic that flushes.
    pub fn root_claim_declared_weight_for(limit: u32) -> Weight {
        Self::root_claim_weight_for_work(limit, Self::basket_flush_work_bound())
    }

    /// Coldkey-wide declared weight: the 256-unit envelope plus the flush allowance.
    pub(crate) fn root_claim_declared_weight() -> Weight {
        Self::root_claim_declared_weight_for(Self::root_claim_declared_work())
    }

    /// Single-hotkey declared weight: the 129-unit envelope plus the same flush allowance.
    pub(crate) fn root_claim_hotkey_declared_weight() -> Weight {
        Self::root_claim_declared_weight_for(Self::root_claim_hotkey_declared_work())
    }

    /// Hotkeys relevant to a coldkey-wide root claim. Ordinary subnet-only staking hotkeys
    /// are deliberately excluded. A negative basket watermark keeps an unstaked claimant
    /// eligible because it encodes shares which still need to be redeemed.
    pub(crate) fn root_claim_hotkeys(
        coldkey: &T::AccountId,
        staking_hotkeys: Vec<T::AccountId>,
    ) -> Vec<T::AccountId> {
        staking_hotkeys
            .into_iter()
            .filter(|hotkey| {
                !Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, NetUid::ROOT)
                    .is_zero()
                    || BasketClaimed::<T>::get(hotkey, coldkey) < 0
            })
            .collect()
    }

    /// True when the hotkeys plus the basket storage rows the claim will scan fit the fixed
    /// admission envelope, and the pending-deposit flushes the claim runs first fit the flat
    /// flush allowance ([`Self::basket_flush_fits_declared_budget`]). Count raw Alpha/AlphaV2
    /// rows so legacy duplicates and malformed zero rows are charged conservatively, and stop
    /// as soon as a bound is exceeded.
    pub(crate) fn root_claim_fits_budget(hotkeys: &[T::AccountId], budget: u32) -> bool {
        let mut work = u32::try_from(hotkeys.len()).unwrap_or(u32::MAX);
        if work > budget {
            return false;
        }

        let escrow = Self::get_beta_escrow_account_id();
        for hotkey in hotkeys {
            for _ in Alpha::<T>::iter_prefix((hotkey, &escrow)) {
                work = work.saturating_add(1);
                if work > budget {
                    return false;
                }
            }
            for _ in AlphaV2::<T>::iter_prefix((hotkey, &escrow)) {
                work = work.saturating_add(1);
                if work > budget {
                    return false;
                }
            }
        }
        Self::basket_flush_fits_declared_budget(hotkeys)
    }

    pub(crate) fn root_claim_fits_declared_budget(hotkeys: &[T::AccountId]) -> bool {
        Self::root_claim_fits_budget(hotkeys, Self::root_claim_declared_work())
    }

    pub(crate) fn root_claim_hotkey_fits_declared_budget(hotkey: &T::AccountId) -> bool {
        Self::root_claim_fits_budget(
            core::slice::from_ref(hotkey),
            Self::root_claim_hotkey_declared_work(),
        )
    }

    /// Actual post-dispatch weight of a root claim: full benchmark units for relationships
    /// classified and slots that did real work (redeemed or swept — a swap plus stake writes
    /// each, floored at the selected hotkey count) plus the cheap per-row scan cost for holdings
    /// that were only valued. This is what lets a fund's claim fee decay as dust rows are
    /// consolidated, and makes a below-threshold no-op cost a scan instead of a full claim.
    /// Work above the fixed admission budget
    /// is refused at dispatch (`RootClaimTooHeavy`) rather than admitted cheaply.
    pub(crate) fn root_claim_actual_weight(
        hotkey_count: u32,
        selection_scanned: u32,
        outcome: &RootClaimOutcome,
    ) -> Weight {
        let active = hotkey_count
            .max(outcome.realized.saturating_add(outcome.swept))
            // Classifying a StakingHotkeys relationship reads the position's share-pool state
            // and basket watermark. Price it conservatively as a full hotkey unit.
            .max(selection_scanned)
            .max(1);
        let scanned = outcome.rows.saturating_sub(outcome.realized);
        <T as crate::pallet::Config>::WeightInfo::claim_root(active)
            .saturating_add(<T as crate::pallet::Config>::WeightInfo::claim_root_scan(
                scanned,
            ))
            .saturating_add(Self::basket_flush_weight(outcome.flush))
    }

    /// Weight of a claim's admission scan, charged on top of the work done when an admitted
    /// claim fails: one read per hotkey-or-row unit the row count may walk, the
    /// staking-hotkeys read, and the pending-deposit queue scan at its bound
    /// ([`super::basket_flush::MAX_BASKET_FLUSH_ROWS`] rows). Bounds, not measurements, so
    /// the refund can only under-state the work in the claimant's disfavour.
    pub fn root_claim_admission_weight(units: u32) -> Weight {
        T::DbWeight::get().reads(
            u64::from(units)
                .saturating_add(1)
                .saturating_add(super::basket_flush::MAX_BASKET_FLUSH_ROWS),
        )
    }

    pub fn do_root_claim(
        coldkey: T::AccountId,
        hotkeys: Vec<T::AccountId>,
    ) -> Result<RootClaimOutcome, DispatchError> {
        Self::do_root_claim_tracked(coldkey, hotkeys).map_err(|(_, err)| err)
    }

    /// [`Self::do_root_claim`] that, on failure, also returns the work done before the
    /// failing hotkey aborted the (rolled-back) claim, so the dispatcher can charge that
    /// work instead of the declared envelope.
    pub fn do_root_claim_tracked(
        coldkey: T::AccountId,
        hotkeys: Vec<T::AccountId>,
    ) -> Result<RootClaimOutcome, (RootClaimOutcome, DispatchError)> {
        Self::ensure_beta_basket_seed_idle()
            .map_err(|err| (RootClaimOutcome::default(), err.into()))?;
        let mut total = RootClaimOutcome::default();
        let result: DispatchResult =
            with_transaction(
                || match Self::try_do_root_claim(coldkey, &hotkeys, &mut total) {
                    Ok(()) => TransactionOutcome::Commit(Ok(())),
                    Err(err) => TransactionOutcome::Rollback(Err(err)),
                },
            );
        match result {
            Ok(()) => Ok(total),
            Err(err) => Err((total, err)),
        }
    }

    fn try_do_root_claim(
        coldkey: T::AccountId,
        hotkeys: &[T::AccountId],
        total: &mut RootClaimOutcome,
    ) -> DispatchResult {
        for hotkey in hotkeys {
            let mut outcome = RootClaimOutcome::default();
            let result = Self::root_claim_for_hotkey_into(hotkey, &coldkey, false, &mut outcome);
            total.accumulate(outcome);
            result?;
        }

        Self::deposit_event(Event::RootClaimed {
            coldkey,
            tao: total.tao.into(),
        });

        Ok(())
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

        // Display state (frozen baseline + TWR) follows the fund; the clean-root gate
        // guarantees the destination holds none, so this is a pure move.
        Self::transfer_beta_display_state(old_hotkey, new_hotkey);

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
            let alpha_moved = Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                old_hotkey, &escrow, netuid, alpha,
            );
            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                new_hotkey,
                &escrow,
                netuid,
                alpha_moved,
            );
        }

        // Trading guardrails follow the fund so a hotkey swap can neither escape a
        // governance freeze nor refill the turnover bucket. The freeze is copied, not
        // moved: a later swap back onto the old hotkey must still find it frozen. The
        // bucket is carried conservatively: the lower level and the later refill block.
        if BasketTradingFrozen::<T>::contains_key(old_hotkey) {
            BasketTradingFrozen::<T>::insert(new_hotkey, ());
        }
        if let Some((old_level, old_block)) = BasketTradeBucket::<T>::take(old_hotkey) {
            let carried = match BasketTradeBucket::<T>::get(new_hotkey) {
                Some((new_level, new_block)) => {
                    (old_level.min(new_level), old_block.max(new_block))
                }
                None => (old_level, old_block),
            };
            BasketTradeBucket::<T>::insert(new_hotkey, carried);
        }

        // Destination-flow counters follow the fund so a hotkey swap cannot
        // reset wash headroom. Carry the higher used amount and the later block,
        // including zero-holding dests (post-unwind). Identity<NetUid> bounds
        // this prefix to 2^16 rows; each row is charged like claimed/pending so
        // the walk is not free. Do not drop leftovers: skipping a dest would
        // reset that dest's wash headroom.
        let used_rows: sp_std::vec::Vec<_> =
            BasketLiquidityUsed::<T>::iter_prefix(old_hotkey).collect();
        moved_rows = moved_rows.saturating_add(used_rows.len() as u32);
        for (netuid, (old_used, old_block)) in used_rows {
            BasketLiquidityUsed::<T>::remove(old_hotkey, netuid);
            let carried = match BasketLiquidityUsed::<T>::get(new_hotkey, netuid) {
                Some((new_used, new_block)) => (old_used.max(new_used), old_block.max(new_block)),
                None => (old_used, old_block),
            };
            BasketLiquidityUsed::<T>::insert(new_hotkey, netuid, carried);
        }

        moved_rows
    }

    /// Converts validators' basket holdings on a dissolving subnet into each fund's root
    /// (TAO) slot, metered and resumable via `last_key` over [`BasketShares`] keys.
    ///
    /// Escrow alpha is sold once per fund and held as root stake under the same escrow.
    /// Fund shares, rates, and watermarks are untouched — NAV is continuous across the
    /// conversion (minus slippage). A terminally shallow holding is explicitly written off;
    /// any unknown failure is logged and leaves the slot for generic teardown. Returns
    /// `(done, next_cursor)`.
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

        let terminal_garbage =
            match Self::try_realizable_tao_for_alpha(netuid, holding_alpha.to_u64()) {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(err) => {
                    log::error!("Error valuing basket holding before conversion: {err:?}");
                    return false;
                }
            };

        with_transaction(|| {
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                escrow,
                netuid,
                holding_alpha,
            );

            if terminal_garbage {
                // The position is economically unusable and no executable sale exists. Remove
                // it explicitly instead of allowing one bad subnet to pin claim/dissolution
                // progress forever. This is a proportional loss to the fund as a whole.
                Self::burn_subnet_alpha(netuid, holding_alpha);
                Self::deposit_event(Event::BasketAlphaWrittenOff {
                    hotkey: hotkey.clone(),
                    netuid,
                    alpha: holding_alpha,
                });
                return TransactionOutcome::Commit(Ok::<(), DispatchError>(()));
            }

            let total_stake_before_sale =
                (!NetworksAdded::<T>::get(netuid)).then(TotalStake::<T>::get);
            let tao = match Self::sell_basket_alpha_for_root_tao(netuid, holding_alpha) {
                Ok(tao) => tao,
                Err(err)
                    if T::SwapInterface::classify_failure(&err)
                        == SwapFailureKind::TerminalLiquidity =>
                {
                    // A late shallow-pool failure is safe to write off because the sale
                    // helper atomically rolled back every attempted chunk.
                    Self::burn_subnet_alpha(netuid, holding_alpha);
                    Self::deposit_event(Event::BasketAlphaWrittenOff {
                        hotkey: hotkey.clone(),
                        netuid,
                        alpha: holding_alpha,
                    });
                    return TransactionOutcome::Commit(Ok::<(), DispatchError>(()));
                }
                Err(err) => {
                    log::error!("Error converting basket holding to root: {err:?}");
                    return TransactionOutcome::Rollback(Err(err));
                }
            };

            // On a dissolved subnet the whole `SubnetTAO` already left `TotalStake` when the
            // network was removed (`do_dissolve_network`), so the sale's own `TotalStake`
            // decrement (`swap_alpha_for_tao`) took it out a second time. Restore the exact
            // pre-sale value rather than the nominal proceeds: the decrement saturates, so
            // adding all proceeds could restore more than it removed. The TAO now moves into
            // the fund's root slot, which `credit_root_slot` books once, and `TotalStake` stays
            // the sum of live subnet reserves. Finney drifted by exactly the converted amount
            // when subnet 108 dissolved (block 9111229).
            if let Some(total_stake_before_sale) = total_stake_before_sale {
                TotalStake::<T>::put(total_stake_before_sale);
            }

            // Hold the realized TAO as the fund's root-slot (cash) position.
            Self::credit_root_slot(hotkey, escrow, tao);

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
        let tao = Self::swap_basket_alpha_for_tao_chunks(netuid, alpha)
            .inspect_err(|err| log::warn!("Unable to swap basket alpha for TAO: {err:?}"))?;

        let root_subnet_account_id =
            Self::get_subnet_account_id(NetUid::ROOT).ok_or(Error::<T>::RootNetworkDoesNotExist)?;

        Self::transfer_tao_from_subnet(netuid, &root_subnet_account_id, tao.into())
            .inspect_err(|err| log::error!("Error transferring basket TAO from subnet: {err:?}"))?;

        Self::record_protocol_outflow(netuid, tao);

        Ok(tao)
    }

    /// Execute a fee-free protocol alpha sale in reserve-bounded chunks. This is both the
    /// money-moving implementation and the engine used under a rollback overlay for NAV quotes,
    /// so an oversized full holding is valued exactly as it would be liquidated.
    pub(crate) fn swap_basket_alpha_for_tao_chunks(
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError> {
        with_transaction(|| {
            let result = (|| {
                if alpha.is_zero() {
                    return Ok(TaoBalance::ZERO);
                }
                if SubnetMechanism::<T>::get(netuid) != 1 {
                    return Self::swap_alpha_for_tao(
                        netuid,
                        alpha,
                        T::SwapInterface::min_price::<TaoBalance>(),
                        true,
                    )
                    .map(|out| out.amount_paid_out);
                }

                let mut remaining = alpha.to_u64();
                let mut total_tao = 0u64;
                while remaining > 0 {
                    let maximum =
                        T::SwapInterface::max_swap_input::<GetTaoForAlpha<T>>(netuid).to_u64();
                    // Let the engine return its concrete failure when the input reserve is zero.
                    let chunk = if maximum == 0 {
                        remaining
                    } else {
                        remaining.min(maximum)
                    };
                    let out = Self::swap_alpha_for_tao(
                        netuid,
                        chunk.into(),
                        T::SwapInterface::min_price::<TaoBalance>(),
                        true,
                    )?;
                    let consumed = out
                        .amount_paid_in
                        .to_u64()
                        .saturating_add(out.fee_paid.to_u64());
                    ensure!(consumed > 0, Error::<T>::AmountTooLow);
                    remaining = remaining.saturating_sub(consumed);
                    total_tao = total_tao.saturating_add(out.amount_paid_out.to_u64());
                }
                Ok(total_tao.into())
            })();
            match result {
                Ok(tao) => TransactionOutcome::Commit(Ok(tao)),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        })
    }

    /// Buy basket alpha in reserve-bounded chunks for oversized user deposits. Swap fees are
    /// charged: the depositor pays their own entry cost.
    pub(crate) fn swap_basket_tao_for_alpha_chunks(
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        with_transaction(|| {
            let result = (|| {
                if tao.is_zero() {
                    return Ok(AlphaBalance::ZERO);
                }
                if SubnetMechanism::<T>::get(netuid) != 1 {
                    return Self::swap_tao_for_alpha(
                        netuid,
                        tao,
                        T::SwapInterface::max_price(),
                        false,
                    )
                    .map(|out| out.amount_paid_out);
                }

                let mut remaining = tao.to_u64();
                let mut total_alpha = 0u64;
                while remaining > 0 {
                    let maximum =
                        T::SwapInterface::max_swap_input::<GetAlphaForTao<T>>(netuid).to_u64();
                    let chunk = if maximum == 0 {
                        remaining
                    } else {
                        remaining.min(maximum)
                    };
                    let out = Self::swap_tao_for_alpha(
                        netuid,
                        chunk.into(),
                        T::SwapInterface::max_price(),
                        false,
                    )?;
                    let consumed = out
                        .amount_paid_in
                        .to_u64()
                        .saturating_add(out.fee_paid.to_u64());
                    ensure!(consumed > 0, Error::<T>::AmountTooLow);
                    remaining = remaining.saturating_sub(consumed);
                    total_alpha = total_alpha.saturating_add(out.amount_paid_out.to_u64());
                }
                Ok(total_alpha.into())
            })();
            match result {
                Ok(alpha) => TransactionOutcome::Commit(Ok(alpha)),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        })
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
    pub(super) fn credit_root_reserves(amount: TaoBalance) {
        SubnetTAO::<T>::mutate(NetUid::ROOT, |total| *total = total.saturating_add(amount));
        SubnetAlphaOut::<T>::mutate(NetUid::ROOT, |total| {
            *total = total.saturating_add(u64::from(amount).into())
        });
        TotalStake::<T>::mutate(|total| *total = total.saturating_add(amount));
    }

    /// Exact inverse of [`Self::credit_root_reserves`]: TAO leaving the root slot (e.g. a
    /// basket trade selling out of the fund's cash position) unwinds the same three storages.
    pub(super) fn debit_root_reserves(amount: TaoBalance) {
        SubnetTAO::<T>::mutate(NetUid::ROOT, |total| *total = total.saturating_sub(amount));
        SubnetAlphaOut::<T>::mutate(NetUid::ROOT, |total| {
            *total = total.saturating_sub(u64::from(amount).into())
        });
        TotalStake::<T>::mutate(|total| *total = total.saturating_sub(amount));
    }

    /// Place `tao` into the fund's root cash slot: the escrow's root stake row (TAO at 1:1,
    /// there is no pool to buy from) and the root reserves move together.
    pub(super) fn credit_root_slot(hotkey: &T::AccountId, escrow: &T::AccountId, tao: TaoBalance) {
        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            escrow,
            NetUid::ROOT,
            tao.to_u64().into(),
        );
        Self::credit_root_reserves(tao);
    }

    /// Exact inverse of [`Self::credit_root_slot`]: take `tao` out of the fund's root cash
    /// slot, moving the escrow's root stake row and the root reserves together. Returns the
    /// TAO that really left the slot; the reserves move by that amount, never by `tao`, so a
    /// short debit of the stake row cannot leave the reserves understated.
    pub(super) fn debit_root_slot(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        tao: TaoBalance,
    ) -> TaoBalance {
        let removed: TaoBalance = Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            escrow,
            NetUid::ROOT,
            tao.to_u64().into(),
        )
        .to_u64()
        .into();
        Self::debit_root_reserves(removed);
        removed
    }
}
