//! The pending-basket-deposit queue: how root dividend credits get from an epoch into a
//! validator's beta basket without doing valuation work inside epoch blocks.
//!
//! Epochs enqueue `(hotkey, origin subnet) -> alpha` credits into [`PendingBasketDeposits`]
//! (cheap map mutations). Credits are flushed per hotkey as one batched deposit — a single
//! full-NAV share mint for all queued origins ([`Pallet::deposit_root_alpha_batch`], the
//! deposit engine at the bottom of this module) — by the one-hotkey-per-block round-robin
//! drain or synchronously before a user call changes the claimant base or fund. Dividend
//! valuation clamps live quotes to the immutable-in-block fast-price band, so synchronous
//! settlement cannot mint at a transient pump/dump while arriving stake cannot capture an
//! earlier dividend and departing stake cannot forfeit it. The existing dust policy remains:
//! sub-threshold credits merge until worth depositing and may cross staker sets, but
//! eligibility uses the fast mark, never live spot.

use super::*;
use crate::weights::WeightInfo;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;
use pallet_alpha_assets::AlphaAssetsInterface;
use safe_math::FixedExt;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_swap_interface::SwapHandler;

/// Rows per axis that every basket weight envelope prices for one hotkey. A fund's
/// holdings and its queued dividend credits each occupy at most one row per subnet
/// (dissolution purges holdings and credits of a removed netuid), and the trade, deposit,
/// and claim envelopes already assume this many rows ([`crate::MAX_ROOT_CLAIM_WORK`]).
pub(crate) const MAX_BASKET_ROWS: u64 = crate::MAX_ROOT_CLAIM_WORK as u64;

/// Admission budget for the flush axis of a multi-hotkey claim: queued credit rows summed
/// over every hotkey the claim will flush. Kept at twice the per-axis row cap so the flat
/// allowance ([`Pallet::basket_flush_work_bound`]) stays where every flushing extrinsic
/// already declares it; one hotkey always fits, and a coldkey-wide claim that would flush
/// more than this must claim per hotkey instead.
pub(crate) const MAX_BASKET_FLUSH_ROWS: u64 = 2 * MAX_BASKET_ROWS;

/// Work counters of one pending-deposit flush, in the two units the flush weight prices
/// separately: read-only AMM `quotes` (NAV sweeps, price-band checks, and the scan's spot
/// reads) and executed `rows` (one in-place credit per credit — stake and queue writes each).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BasketFlushWork {
    pub quotes: u64,
    pub rows: u64,
}

impl BasketFlushWork {
    pub const fn new(quotes: u64, rows: u64) -> Self {
        Self { quotes, rows }
    }

    pub(crate) fn saturating_add(self, other: Self) -> Self {
        Self {
            quotes: self.quotes.saturating_add(other.quotes),
            rows: self.rows.saturating_add(other.rows),
        }
    }

    /// Quote and row counters folded into one figure, for bound comparisons in tests.
    pub fn total(self) -> u64 {
        self.quotes.saturating_add(self.rows)
    }
}

impl<T: Config> Pallet<T> {
    /// Full-settlement work envelope for the bounded queue/holding dimensions below.
    /// The block drain and synchronous callers charge their actual work; wide calls use the
    /// aggregate admission envelope below.
    ///
    /// Let `Q` be the queued rows scanned, `H` the holdings before the deposit, and
    /// `C <= Q` the credits deposited. One flush is:
    /// * the scan: one quote per queued row, `Q`;
    /// * one deposit attempt ([`Self::deposit_root_alpha_batch`]): the pre-deposit NAV
    ///   sweep `H` plus two quotes per credit (the origin holding before and after the
    ///   in-place credit) — `H + 2C` quotes — and `C` in-place credit rows;
    /// * the baseline stamp on success, one sweep over at most `H + C` rows (every credit
    ///   may open a new holding row) — `H + C` quotes.
    ///
    /// Per-credit failures are isolated inside the attempt (the failing credit is re-queued
    /// and the batch continues), and a shared-phase failure re-queues the whole batch, so
    /// there is exactly one attempt: no per-credit retry can multiply the sweeps. Total:
    /// `Q + 2H + 3C <= 4Q + 2H` quotes and `C <= Q` rows.
    ///
    /// One hotkey has `Q, H <= MAX_BASKET_ROWS`: `6 * MAX_BASKET_ROWS` quotes and
    /// `MAX_BASKET_ROWS` rows. A coldkey-wide claim over several hotkeys is admitted with
    /// `sum(H) <= MAX_BASKET_ROWS` (escrow rows, `root_claim_fits_declared_budget`) and
    /// `sum(Q) <= MAX_BASKET_FLUSH_ROWS` ([`Self::basket_flush_fits_declared_budget`]):
    /// `4 * sum(Q) + 2 * sum(H) <= 10 * MAX_BASKET_ROWS` quotes and `sum(Q) <= 2 *
    /// MAX_BASKET_ROWS` rows. These figures bound the full-settlement regression fixtures.
    pub fn basket_flush_work_bound() -> BasketFlushWork {
        BasketFlushWork::new(MAX_BASKET_ROWS.saturating_mul(10), MAX_BASKET_FLUSH_ROWS)
    }

    /// Worst-case allowance for a single hotkey's bounded pending-dividend settlement.
    /// Call-specific stake weights use [`Self::root_pending_dividend_weight_for`] instead.
    pub fn root_pending_dividend_weight(on_root: bool) -> Weight {
        if on_root {
            Self::basket_flush_weight(BasketFlushWork::new(
                MAX_BASKET_ROWS.saturating_mul(6),
                MAX_BASKET_ROWS,
            ))
        } else {
            Weight::zero()
        }
    }

    /// Allowance sized to the pending rows and current holdings of `hotkey`. The settlement
    /// does at most `Q + H + 2Q + H + Q = 4Q + 2H` quotes and `Q` credit rows. This keeps
    /// ordinary root stake calls cheap while preserving the full bounded envelope for a
    /// hotkey that really has 256 queued credits and 256 holdings.
    pub fn root_pending_dividend_weight_for(hotkey: &T::AccountId, on_root: bool) -> Weight {
        if !on_root {
            return Weight::zero();
        }
        let queued = PendingBasketDeposits::<T>::iter_key_prefix(hotkey)
            .take(MAX_BASKET_ROWS.saturating_add(1) as usize)
            .count() as u64;
        if queued > MAX_BASKET_ROWS {
            return Self::root_pending_dividend_weight(true);
        }
        if queued == 0 {
            return Weight::zero();
        }
        let holdings = (Self::get_basket_holdings(hotkey).len() as u64).min(MAX_BASKET_ROWS);
        Self::basket_flush_weight(BasketFlushWork::new(
            queued
                .saturating_mul(4)
                .saturating_add(holdings.saturating_mul(2)),
            queued,
        ))
    }

    /// Weight of the work reported by [`Self::flush_basket_deposits_for_hotkey`]: quotes are
    /// priced like NAV-sweep rows (read-only sim-swap valuations), plus seven reads per
    /// stability check: existence, mechanism, fast price, and the swap price's mechanism,
    /// two reserves and balancer. The number of checks is bounded
    /// by the admitted holding rows plus executed credit rows, not every quote (the same rows
    /// are quoted repeatedly). Executed rows — stake and queue writes each — are priced like
    /// redeemed `claim_root` rows, the benchmarked figure for one swap-and-write row (an upper
    /// bound: an in-place credit swaps nothing). Zero when nothing was queued.
    pub fn basket_flush_weight(work: BasketFlushWork) -> Weight {
        let quotes = if work.quotes == 0 {
            Weight::zero()
        } else {
            let price_checks = work.quotes.min(MAX_BASKET_ROWS).saturating_add(work.rows);
            Self::basket_nav_sweep_weight(work.quotes)
                .saturating_add(T::DbWeight::get().reads(price_checks.saturating_mul(7)))
        };
        let rows = if work.rows == 0 {
            Weight::zero()
        } else {
            <T as crate::pallet::Config>::WeightInfo::claim_root(
                u32::try_from(work.rows).unwrap_or(u32::MAX),
            )
        };
        quotes.saturating_add(rows)
    }

    /// Admission envelope for a coldkey-wide operation. It covers full synchronous
    /// settlement over the bounded aggregate queue and holding dimensions.
    pub fn basket_pending_work_bound() -> BasketFlushWork {
        Self::basket_flush_work_bound()
    }

    /// Flat pre-dispatch queue-check allowance, separate from the full block-drain work.
    pub fn basket_flush_weight_bound() -> Weight {
        Self::basket_flush_weight(Self::basket_pending_work_bound())
    }

    /// True when flushing every one of `hotkeys` fits the flush axis of the declared
    /// allowance: the queued credit rows summed over the hotkeys are at most
    /// [`MAX_BASKET_FLUSH_ROWS`]. Stops as soon as the budget is exceeded, so the check
    /// itself is bounded work.
    #[cfg(test)]
    pub(crate) fn basket_flush_fits_declared_budget(hotkeys: &[T::AccountId]) -> bool {
        let mut rows: u64 = 0;
        for hotkey in hotkeys {
            for _ in PendingBasketDeposits::<T>::iter_key_prefix(hotkey) {
                rows = rows.saturating_add(1);
                if rows > MAX_BASKET_FLUSH_ROWS {
                    return false;
                }
            }
        }
        true
    }

    /// Queue a root dividend credit for later batched deposit into the hotkey's basket,
    /// merging with any credit already queued for the same origin.
    pub fn enqueue_basket_deposit(
        hotkey: &T::AccountId,
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
    ) {
        if root_alpha.is_zero() {
            return;
        }
        PendingBasketDeposits::<T>::mutate(hotkey, origin_netuid, |pending| {
            *pending = pending.saturating_add(root_alpha)
        });
    }

    /// Settle material credits before changing the claimant base or fund. Quotes are clamped
    /// to the immutable fast-price band, so a dispatch cannot choose a manipulated mint mark.
    pub(crate) fn flush_basket_deposits_for_hotkey(
        hotkey: &T::AccountId,
    ) -> (BasketFlushWork, Option<Vec<u8>>, bool) {
        Self::process_basket_deposits_for_hotkey(hotkey)
    }

    /// Simulate a block-drain turn without advancing unrelated emission machinery.
    #[cfg(test)]
    pub(crate) fn settle_basket_deposits_for_test(
        hotkey: &T::AccountId,
    ) -> (BasketFlushWork, Option<Vec<u8>>, bool) {
        Self::process_basket_deposits_for_hotkey(hotkey)
    }

    /// Deposit all of a hotkey's flushable queued credits into its basket as one batch.
    ///
    /// A credit is flushable when its origin subnet still exists and its fast-mark value is
    /// at or above `RootClaimableThreshold[ROOT]` (or the anchor is unavailable).
    /// This mark decides only dust eligibility, NOT the share mint price. Sub-threshold credits stay
    /// queued and keep merging with future dividends until they are worth a deposit — this
    /// is what keeps dust from ever becoming a basket holding row (and what lets dust
    /// consolidation run without fighting next epoch's accrual).
    /// Credits from dissolved subnets are recycled (issuance conservation) then dropped.
    /// `NetworksAdded` is cleared at dissolve start, so this path covers the in-progress
    /// window; the durable guarantee against netuid reuse is the
    /// `NetworkPendingBasketDeposits` dissolution phase, which purges every queued credit
    /// for the netuid before cleanup completes (and reuse becomes possible).
    ///
    /// Hotkeys no longer registered on root cannot earn more dividends to merge dust past
    /// the threshold, so every remaining queued credit is recycled and purged (the queued
    /// alpha only — basket holdings are untouched). Root replacement calls this after
    /// dropping membership so churn cannot leave permanent straggler rows.
    ///
    /// Returns `(work, last_key, completed)`: the approximate work done (quotes and executed
    /// rows, never above [`Self::basket_flush_work_bound`]; priced by
    /// [`Self::basket_flush_weight`] into the post-dispatch weight of every extrinsic that
    /// flushes), the raw storage key of the hotkey's last queue entry, and whether every
    /// credit selected for this flush was settled. The drain stores `last_key` as its cursor so a hotkey whose credits are all
    /// deferred dust still gets skipped past instead of pinning the queue head.
    fn process_basket_deposits_for_hotkey(
        hotkey: &T::AccountId,
    ) -> (BasketFlushWork, Option<Vec<u8>>, bool) {
        let threshold: u64 =
            RootClaimableThreshold::<T>::get(NetUid::ROOT).saturating_to_num::<u64>();
        let on_root = Self::is_hotkey_registered_on_network(NetUid::ROOT, hotkey);

        let mut work = BasketFlushWork::default();
        let mut last_netuid: Option<NetUid> = None;
        let mut batch: Vec<(NetUid, AlphaBalance)> = Vec::new();
        for (netuid, alpha) in PendingBasketDeposits::<T>::iter_prefix(hotkey) {
            last_netuid = Some(netuid);
            work.quotes = work.quotes.saturating_add(1);
            // Governance/network churn must not make any caller walk an arbitrarily long
            // queue. The extra row only detects overflow; no further price or write follows.
            if work.quotes > MAX_BASKET_ROWS {
                return (work, None, false);
            }
            if !Self::if_subnet_exist(netuid) {
                Self::drop_pending_basket_deposit(hotkey, netuid, alpha);
                work.rows = work.rows.saturating_add(1);
                continue;
            }
            // Root straggler: recycle the pending credit back into the origin subnet and
            // drop the row. Do not touch basket holdings — only the unqueued dividend.
            if !on_root {
                Self::drop_pending_basket_deposit(hotkey, netuid, alpha);
                work.rows = work.rows.saturating_add(1);
                continue;
            }
            // Dust eligibility must not change after an intra-block spot manipulation.
            // A missing anchor fails closed: it cannot disguise a pending credit as dust.
            let anchor = if netuid.is_root() || SubnetMechanism::<T>::get(netuid) != 1 {
                Some(U64F64::from_num(1))
            } else {
                SubnetFastMovingPrice::<T>::get(netuid)
                    .filter(|price| *price != U64F64::from_num(0))
            };
            #[cfg(test)]
            crate::tests::mock::inc_basket_quote_ops();
            let value: u64 = anchor.map_or(u64::MAX, |price| {
                price
                    .saturating_mul(U64F64::saturating_from_num(alpha.to_u64()))
                    .saturating_to_num::<u64>()
            });
            if value < threshold {
                continue;
            }
            batch.push((netuid, alpha));
        }

        let last_key =
            last_netuid.map(|netuid| PendingBasketDeposits::<T>::hashed_key_for(hotkey, netuid));

        if batch.is_empty() {
            return (work, last_key, true);
        }
        for (netuid, _) in batch.iter() {
            PendingBasketDeposits::<T>::remove(hotkey, netuid);
            #[cfg(test)]
            crate::tests::mock::inc_basket_write_ops();
        }

        work = work.saturating_add(Self::deposit_root_alpha_batch(hotkey, &batch));
        let completed = batch
            .iter()
            .all(|(netuid, _)| !PendingBasketDeposits::<T>::contains_key(hotkey, netuid));
        (work, last_key, completed)
    }

    /// The per-block queue drain: flush exactly one queued hotkey per block, round-robin
    /// via the stored cursor (a block that finds the cursor at the end of the map spends
    /// its turn resetting it). One per block is enough because root dividends only accrue
    /// to root-registered hotkeys and root churn finalizes (recycles + purges) any queued
    /// credits on the outgoing hotkey, so the queue stays capped by the root UID table —
    /// a full cycle fits well inside a tempo, and each flush is itself bounded work. User
    /// calls that need the same settlement may also flush their hotkey synchronously. This
    /// drain runs right after coinbase, before Scheduler and user extrinsics in the runtime's
    /// initialization order.
    pub fn flush_pending_basket_deposits_block() -> Weight {
        let overhead = T::DbWeight::get().reads_writes(4, 1);
        if crate::migrations::migrate_seed_beta_basket::seed_beta_basket_v2_in_progress::<T>() {
            return overhead;
        }

        let cursor = PendingBasketFlushCursor::<T>::get();
        let mut keys = match cursor {
            Some(raw) => PendingBasketDeposits::<T>::iter_keys_from(raw),
            None => PendingBasketDeposits::<T>::iter_keys(),
        };
        let Some((hotkey, _)) = keys.next() else {
            // End of the map (or empty queue): restart from the top next block.
            PendingBasketFlushCursor::<T>::kill();
            #[cfg(test)]
            crate::tests::mock::inc_basket_write_ops();
            return overhead;
        };
        drop(keys);

        let (work, last_key, _) = Self::process_basket_deposits_for_hotkey(&hotkey);
        match last_key {
            Some(last_key) => {
                PendingBasketFlushCursor::<T>::put(last_key);
                #[cfg(test)]
                crate::tests::mock::inc_basket_write_ops();
            }
            // Nothing was queued under this hotkey after all (racing removal);
            // clear the cursor so the next pass restarts cleanly.
            None => {
                PendingBasketFlushCursor::<T>::kill();
                #[cfg(test)]
                crate::tests::mock::inc_basket_write_ops();
            }
        }
        overhead.saturating_add(Self::basket_flush_weight(work))
    }

    /// Distributes a validator's root dividend (origin-subnet alpha, net of take) into its beta
    /// basket: the dividend *accumulates in place* — the origin alpha is credited directly to
    /// the fund's holding on the origin subnet, with no sell and no redeploy.
    ///
    /// Single-credit wrapper over [`Self::deposit_root_alpha_batch`]. Epochs no longer call
    /// this inline — they enqueue credits into [`PendingBasketDeposits`] and the queue
    /// flushes per hotkey in batches (see [`Self::flush_basket_deposits_for_hotkey`]) — but
    /// the deposit semantics described here are those of each batch.
    ///
    /// A fund's composition is therefore the emission-weighted portfolio its dividends
    /// describe, reshaped only by the validator's explicit `swap_basket` trades. The protocol
    /// executes zero trades on a dividend (no swap fees, no slippage, no sell pressure), and
    /// there is no target vector for inflows to chase. Shares still mint at NAV: the deposit
    /// mints *fund shares* against the whole basket, `shares = value_added * P / N`, where `N`
    /// is the fund's pre-deposit realizable NAV, `P` the outstanding shares, and `value_added`
    /// the realizable value the credited alpha added to its origin holding (see
    /// [`Self::try_accumulate_root_alpha_batch`]), so existing holders are neither diluted
    /// nor taxed. Stakers accrue entitlement through the single per-validator
    /// `BasketRate += shares / total_root_stake` accumulator; no entitlement is ever
    /// denominated in a particular subnet's alpha, which is what allows holdings to be traded
    /// without touching staker claims.
    ///
    /// Attribution: the dividend was earned by the validator's WHOLE root stake, including
    /// the fund's own root-slot (escrow) position. Only the real stakers' fraction of the
    /// value mints shares; the escrow slot's fraction enters the fund unminted, so the fund's
    /// own cash yield accrues to existing share holders through N/P instead of leaking to
    /// root stakers as free shares. The mint denominator includes that unminted yield:
    /// `shares = stakers_value * P / (N + fund_yield)`.
    ///
    /// The whole operation is transactional. Each credit's own step is isolated: an unknown
    /// valuation or accounting failure there re-queues only that credit and the batch
    /// continues, so one borked origin cannot sink the rest. A failure in a shared phase (NAV
    /// sweep or a dust mint) rolls back and re-queues every credit for a later flush. A credit
    /// on a terminally shallow origin is recycled so one garbage subnet cannot pin the
    /// hotkey's queue indefinitely. Dividends are also recycled when the validator has no
    /// root stake to apportion against.
    ///
    /// An in-place accumulation moves no TAO through any pool, so it records no protocol
    /// flow; the eventual claim sell is a genuine net extraction and books its outflow then
    /// (in `root_claim_for_hotkey`).
    #[cfg(test)]
    pub fn distribute_root_alpha_to_basket(
        hotkey: &T::AccountId,
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
    ) {
        if root_alpha.is_zero() {
            return;
        }
        Self::deposit_root_alpha_batch(hotkey, &[(origin_netuid, root_alpha)]);
    }

    /// Deposits a batch of root-dividend credits — each `(origin_netuid, alpha)`, at most
    /// one entry per origin — into a validator's basket with a single share mint. This is
    /// the whole point of the pending-deposit queue: a hotkey's credits from many subnet
    /// epochs share one full-NAV valuation instead of paying one per origin.
    ///
    /// The batch is transactional as a whole and is attempted exactly once: a credit
    /// whose own step fails is re-queued from inside the attempt while the others proceed,
    /// and a shared-phase failure re-queues the whole batch for a later flush (credits may
    /// merge with future dividends and become depositable). There is deliberately no
    /// per-credit retry loop — it would multiply the NAV sweeps by the credit count and break
    /// the flat allowance [`Self::basket_flush_work_bound`] every flushing extrinsic declares.
    /// Credits are recycled when demonstrably unapportionable (no claimant root
    /// stake *and* no escrow cash), terminally untradeable, or when the seed
    /// migration owns the basket maps.
    ///
    /// Returns the approximate work performed (holdings valued as quotes, in-place credits as
    /// executed rows), priced by [`Self::basket_flush_weight`] in the block hook's weight.
    fn deposit_root_alpha_batch(
        hotkey: &T::AccountId,
        batch: &[(NetUid, AlphaBalance)],
    ) -> BasketFlushWork {
        if batch.iter().all(|(_, alpha)| alpha.is_zero()) {
            return BasketFlushWork::default();
        }

        // Seed migration still converting legacy claim state. Coinbase only queues its
        // calculated per-hotkey credits; recycle an unexpected direct deposit defensively rather
        // than writing BasketRate/Shares that a later pass would overwrite.
        if crate::migrations::migrate_seed_beta_basket::seed_beta_basket_v2_in_progress::<T>() {
            Self::recycle_basket_deposit_batch(batch);
            return BasketFlushWork::default();
        }

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
        let shares = BasketShares::<T>::get(hotkey);

        // Same predicate as mint. Recycle when the credit cannot be kept:
        // no claimant root and (no escrow cash, or a shareless escrow slot).
        if !Self::can_keep_basket_dividend(total_root.to_u64(), escrow_root.to_u64(), shares) {
            Self::recycle_basket_deposit_batch(batch);
            return BasketFlushWork::default();
        }

        // Approximate work executed, charged whether the deposit commits or rolls back (the
        // quotes ran either way): one NAV sweep plus two quotes per origin, and one in-place
        // credit row per origin.
        let holdings = Self::get_basket_holdings(hotkey).len() as u64;
        let credits = batch.len() as u64;
        let work =
            BasketFlushWork::new(holdings.saturating_add(credits.saturating_mul(2)), credits);

        let outcome = with_transaction(|| {
            match Self::try_accumulate_root_alpha_batch(
                hotkey,
                batch,
                total_root.to_u64(),
                escrow_root.to_u64(),
            ) {
                Ok(()) => TransactionOutcome::Commit(Ok(())),
                Err(err) => TransactionOutcome::Rollback(Err(err)),
            }
        });

        if outcome.is_ok() {
            // A fund's very first successful mint stamps its frozen display baseline
            // (index splice). No-op (one read) for every later deposit.
            return work.saturating_add(BasketFlushWork::new(
                Self::stamp_beta_baseline_if_new(hotkey),
                0,
            ));
        }

        // Soft failure in a shared phase (NAV sweep, dust mint): re-queue every
        // credit — recoverable later (merge with future dividends, or a healthier pool)
        // rather than recycling healthy work away. Per-credit failures never reach here;
        // each credit's own step re-queues itself from inside the attempt.
        Self::requeue_basket_deposit_batch(hotkey, batch);
        work
    }

    /// Run one credit's own step of a batch deposit in a nested transaction. `Ok(value)`
    /// commits the step; `Err` rolls back only this credit's writes and re-queues the credit
    /// (inside the enclosing batch transaction, so a later batch rollback undoes the re-queue
    /// and the outer re-queue takes over), returning `None` so the batch continues without it.
    fn isolate_basket_credit<R>(
        hotkey: &T::AccountId,
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
        step: impl FnOnce() -> Result<R, DispatchError>,
    ) -> Option<R> {
        let outcome = with_transaction(|| match step() {
            Ok(value) => TransactionOutcome::Commit(Ok(value)),
            Err(err) => TransactionOutcome::Rollback(Err(err)),
        });
        match outcome {
            Ok(value) => Some(value),
            Err(err) => {
                log::debug!(
                    "basket credit re-queued after isolated failure: hotkey={hotkey:?} origin={origin_netuid:?} err={err:?}"
                );
                Self::requeue_basket_deposit_batch(hotkey, &[(origin_netuid, root_alpha)]);
                None
            }
        }
    }

    /// Recycle every credit in an unapportionable deposit batch back into its origin subnet.
    fn recycle_basket_deposit_batch(batch: &[(NetUid, AlphaBalance)]) {
        for (origin_netuid, root_alpha) in batch {
            if !root_alpha.is_zero() {
                Self::recycle_subnet_alpha(*origin_netuid, *root_alpha);
            }
        }
    }

    /// Put rolled-back deposit credits back onto the pending queue so a later flush can
    /// retry them (they may merge with future origin dividends and clear the dust bar).
    fn requeue_basket_deposit_batch(hotkey: &T::AccountId, batch: &[(NetUid, AlphaBalance)]) {
        for (origin_netuid, root_alpha) in batch {
            Self::enqueue_basket_deposit(hotkey, *origin_netuid, *root_alpha);
            #[cfg(test)]
            if !root_alpha.is_zero() {
                crate::tests::mock::inc_basket_write_ops();
            }
        }
    }

    /// Remove a queued pending-basket credit and recycle its alpha so earned dividends are
    /// not silently deleted from issuance. When `SubnetAlphaOut` still tracks the subnet
    /// (flush race / early dissolve), use the full recycle path; after stake teardown has
    /// removed that counter, recycle only the AlphaAssets side so we do not resurrect it.
    pub(crate) fn drop_pending_basket_deposit(
        hotkey: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) {
        PendingBasketDeposits::<T>::remove(hotkey, netuid);
        #[cfg(test)]
        crate::tests::mock::inc_basket_write_ops();
        if alpha.is_zero() {
            return;
        }
        if SubnetAlphaOut::<T>::contains_key(netuid) {
            Self::recycle_subnet_alpha(netuid, alpha);
        } else {
            let _ = T::AlphaAssets::recycle_alpha(netuid, alpha);
        }
    }

    /// Transactional body of [`Self::deposit_root_alpha_batch`]: each dividend credit is
    /// applied directly to the fund's holding on the subnet it arrived on.
    /// No swap runs — the alpha is already counted in `SubnetAlphaOut` (the recycle path
    /// decrements it when a credit is truly dropped), it just is not assigned to any stake
    /// position yet, so the whole deposit is a share-pool credit. Any error rolls the batch
    /// back (the caller splits / re-queues).
    ///
    /// Each credit is valued as the realizable delta on its origin holding alone: crediting
    /// stake moves no pool, so every other holding's quote is unchanged and a full-fund
    /// `nav_after` sweep collapses to one extra quote per origin. Both the pre-deposit NAV and
    /// each added-value delta use realizable quotes with their price clamped to the fast-EMA
    /// band. Inside the band this is exactly the live quote; outside it the same realizable
    /// quote is scaled back to the nearest trusted band edge. Settlement therefore remains
    /// live without letting a same-block pump or dump choose the share price.
    /// The origins are distinct pools, so the per-origin deltas are independent and sum to
    /// exactly the value the batch added against the shared `nav_before` snapshot.
    /// Realizable valuation keeps deposit pricing honest on thin pools exactly as it does for
    /// bought alpha — the marginal alpha of a large holding quotes below spot, so existing
    /// share holders are never diluted by an over-marked deposit.
    fn try_accumulate_root_alpha_batch(
        hotkey: &T::AccountId,
        batch: &[(NetUid, AlphaBalance)],
        total_root: u64,
        escrow_root: u64,
    ) -> DispatchResult {
        let escrow = Self::get_beta_escrow_account_id();
        let nav_before: u64 = Self::try_get_validator_basket_dividend_nav_tao(hotkey)?;

        // Each credit's accumulation is isolated: an unknown valuation failure re-queues
        // that credit alone and the batch goes on with the others.
        let mut value_added: u64 = 0;
        let mut accumulated_any = false;
        for (origin_netuid, root_alpha) in batch {
            if root_alpha.is_zero() {
                continue;
            }
            let delta = Self::isolate_basket_credit(hotkey, *origin_netuid, *root_alpha, || {
                Self::try_accumulate_basket_credit(hotkey, &escrow, *origin_netuid, *root_alpha)
            });
            if let Some(Some(delta)) = delta {
                accumulated_any = true;
                value_added = value_added.saturating_add(delta);
            }
        }

        if !accumulated_any {
            return Ok(());
        }

        Self::mint_basket_dividend_shares(hotkey, nav_before, value_added, total_root, escrow_root)
    }

    /// One credit's step of the batch: credit the origin alpha to the fund's holding on that
    /// subnet and value it as the fast-band-clamped realizable delta on that holding alone.
    /// `Ok(Some(delta))` accumulated; `Ok(None)` the holding is (or would become) terminally
    /// untradeable and the credit was recycled; `Err` an unknown failure the caller isolates.
    fn try_accumulate_basket_credit(
        hotkey: &T::AccountId,
        escrow: &T::AccountId,
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
    ) -> Result<Option<u64>, DispatchError> {
        let held_before: u64 =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, origin_netuid)
                .to_u64();
        let Some(origin_before_live) =
            Self::try_realizable_tao_for_alpha(origin_netuid, held_before)?
        else {
            // Do not add fresh dividends to a holding which is already terminally
            // untradeable. The credit has not been assigned to a staker yet, so recycle it
            // rather than manufacturing shares with a zero mark.
            Self::recycle_subnet_alpha(origin_netuid, root_alpha);
            return Ok(None);
        };
        let origin_before =
            Self::basket_dividend_holding_value(origin_netuid, held_before, origin_before_live);

        Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            escrow,
            origin_netuid,
            root_alpha,
        );

        // Re-read the holding after the credit so share-pool rounding is priced in.
        let held_after: u64 =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, escrow, origin_netuid)
                .to_u64();
        let Some(origin_after_live) =
            Self::try_realizable_tao_for_alpha(origin_netuid, held_after)?
        else {
            // Adding the credit crossed into terminal territory. Undo only the amount
            // actually assigned to the holding, recycle the whole unassigned credit, and
            // leave the pre-existing position untouched.
            let assigned = held_after.saturating_sub(held_before);
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                escrow,
                origin_netuid,
                assigned.into(),
            );
            Self::recycle_subnet_alpha(origin_netuid, root_alpha);
            return Ok(None);
        };
        let origin_after =
            Self::basket_dividend_holding_value(origin_netuid, held_after, origin_after_live);
        Ok(Some(origin_after.saturating_sub(origin_before)))
    }

    fn try_get_validator_basket_dividend_nav_tao(
        hotkey: &T::AccountId,
    ) -> Result<u64, DispatchError> {
        let mut nav = 0u64;
        for (netuid, alpha) in Self::get_basket_holdings(hotkey) {
            let live = Self::try_realizable_tao_for_alpha(netuid, alpha.to_u64())?.unwrap_or(0);
            nav = nav.saturating_add(Self::basket_dividend_holding_value(
                netuid,
                alpha.to_u64(),
                live,
            ));
        }
        Ok(nav)
    }

    /// Scale a realizable quote as if spot were at the nearest edge of the immutable fast
    /// EMA band. Scaling rather than marking `alpha * price` preserves the holding's
    /// slippage discount. Missing fast state falls back to the immutable slow EMA; dissolved
    /// pools and non-dynamic slots retain their realizable value.
    fn basket_dividend_holding_value(netuid: NetUid, alpha: u64, realizable: u64) -> u64 {
        if alpha == 0
            || realizable == 0
            || !Self::if_subnet_exist(netuid)
            || netuid.is_root()
            || SubnetMechanism::<T>::get(netuid) != 1
        {
            return realizable;
        }
        let fast = SubnetFastMovingPrice::<T>::get(netuid)
            .filter(|price| *price > U64F64::saturating_from_num(0))
            .unwrap_or_else(|| Self::get_moving_alpha_price(netuid));
        let spot = T::SwapInterface::current_alpha_price(netuid.into());
        if fast == U64F64::saturating_from_num(0) || spot == U64F64::saturating_from_num(0) {
            return Self::guarded_basket_holding_value(netuid, alpha, realizable);
        }

        let denominator = U64F64::saturating_from_num(10_000u64);
        let lower = fast
            .saturating_mul(U64F64::saturating_from_num(
                10_000u64.saturating_sub(crate::BASKET_TRADE_MAX_SLIPPAGE_BPS),
            ))
            .safe_div(denominator);
        let upper = fast
            .saturating_mul(U64F64::saturating_from_num(
                10_000u64.saturating_add(crate::BASKET_TRADE_MAX_SLIPPAGE_BPS),
            ))
            .safe_div(denominator);
        let clamped = spot.max(lower).min(upper);
        U64F64::saturating_from_num(realizable)
            .saturating_mul(clamped.safe_div(spot))
            .saturating_to_num::<u64>()
    }
}
