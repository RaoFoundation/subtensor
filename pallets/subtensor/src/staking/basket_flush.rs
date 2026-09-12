//! The pending-basket-deposit queue: how root dividend credits get from an epoch into a
//! validator's beta basket without doing valuation work inside epoch blocks.
//!
//! Epochs enqueue `(hotkey, origin subnet) -> alpha` credits into [`PendingBasketDeposits`]
//! (cheap map mutations). Credits are flushed per hotkey as one batched deposit — a single
//! full-NAV share mint for all queued origins ([`Pallet::deposit_root_alpha_batch`], the
//! deposit engine at the bottom of this module) — either by the one-hotkey-per-block
//! round-robin drain here, or eagerly by any operation that touches the hotkey's claimant
//! base or fund (claims, basket stakes, root stake changes, hotkey swaps). The eager
//! flushes are what make the deferral economically inert: the queue flushes before any
//! stake change, so arriving stake can't capture — and departing stake doesn't forfeit —
//! any flushable dividend. Only deliberately deferred sub-threshold dust ever crosses
//! staker sets.

use super::claim_root::BasketFunding;
use super::*;
use crate::weights::WeightInfo;
use frame_support::storage::{TransactionOutcome, with_transaction};
use frame_support::weights::Weight;
use pallet_alpha_assets::AlphaAssetsInterface;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUidStorageIndex;
use subtensor_swap_interface::{SwapFailureKind, SwapHandler};

/// Rows per axis that every basket weight envelope prices for one hotkey. A fund's
/// holdings, its queued dividend credits, and its weight-vector destinations each occupy at
/// most one row per subnet (`set_root_weights` bounds the vector by the subnet count, and
/// dissolution purges holdings and credits of a removed netuid), and the trade, deposit, and
/// claim envelopes already assume this many rows ([`crate::MAX_ROOT_CLAIM_WORK`]).
pub(crate) const MAX_BASKET_ROWS: u64 = crate::MAX_ROOT_CLAIM_WORK as u64;

/// Admission budget for the flush axis of a multi-hotkey claim: queued credit rows plus
/// weight-vector destinations, summed over every hotkey the claim will flush. One hotkey
/// always fits (each axis is at most [`MAX_BASKET_ROWS`]); a coldkey-wide claim that would
/// flush more than this must claim per hotkey instead.
pub(crate) const MAX_BASKET_FLUSH_ROWS: u64 = 2 * MAX_BASKET_ROWS;

/// Work counters of one pending-deposit flush, in the two units the flush weight prices
/// separately: read-only AMM `quotes` (NAV sweeps and the scan's spot reads — one sim-swap
/// valuation plus reads each) and executed `rows` (one origin sell or in-place credit per
/// credit, one buy per destination — each a swap plus stake, reserve, and queue writes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BasketFlushWork {
    pub quotes: u64,
    pub rows: u64,
}

impl BasketFlushWork {
    pub(crate) const fn new(quotes: u64, rows: u64) -> Self {
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
    /// Worst-case work of one [`Self::flush_basket_deposits_for_hotkey`] call, or of the
    /// flushes a coldkey-wide claim admitted by [`Self::basket_flush_fits_declared_budget`]
    /// performs in total. This is the flat pre-dispatch allowance every extrinsic that flushes
    /// declares; the actual work is refunded post-dispatch.
    ///
    /// Let `Q` be the queued rows scanned, `H` the holdings before the deposit, `D` the
    /// weight-vector destinations, and `C <= Q` the credits deposited. One flush is:
    /// * the scan: one quote per queued row, `Q`;
    /// * one deposit attempt ([`Self::deposit_root_alpha_batch`]). Curated: the pre-sale NAV
    ///   sweep `H`, the deployment's pre-buy sweep `H` and post-buy sweep over at most
    ///   `H + D` holdings — `3H + D` quotes — plus `C` sells and `D` buys — `C + D` rows.
    ///   Uncurated: `H + 2C` quotes and `C` rows, which is smaller;
    /// * the baseline stamp on success, one sweep over at most `H + D` rows — `H + D` quotes.
    ///
    /// Per-credit failures are isolated inside the attempt (the failing credit is re-queued
    /// and the batch continues), and a shared-phase failure re-queues the whole batch, so
    /// there is exactly one attempt: no per-credit retry can multiply the sweeps. Total:
    /// `Q + 4H + 2D` quotes and `C + D <= Q + D` rows.
    ///
    /// One hotkey has `Q, H, D <= MAX_BASKET_ROWS`: `7 * MAX_BASKET_ROWS` quotes and
    /// `2 * MAX_BASKET_ROWS` rows. A coldkey-wide claim over several hotkeys is admitted with
    /// `sum(H) <= MAX_BASKET_ROWS` (escrow rows, `root_claim_fits_declared_budget`) and
    /// `sum(Q + D) <= MAX_BASKET_FLUSH_ROWS`: `2 * sum(Q + D) + 4 * sum(H) <= 8 *
    /// MAX_BASKET_ROWS` quotes and `sum(Q + D) <= 2 * MAX_BASKET_ROWS` rows. The larger
    /// figures are the single allowance used everywhere.
    pub(crate) fn basket_flush_work_bound() -> BasketFlushWork {
        BasketFlushWork::new(MAX_BASKET_ROWS.saturating_mul(8), MAX_BASKET_FLUSH_ROWS)
    }

    /// Weight of the work reported by [`Self::flush_basket_deposits_for_hotkey`]: quotes are
    /// priced like NAV-sweep rows (read-only sim-swap valuations); executed rows — a swap with
    /// stake, reserve, and queue writes each — are priced like redeemed `claim_root` rows, the
    /// benchmarked figure for one swap-and-write row. Zero when nothing was queued.
    pub(crate) fn basket_flush_weight(work: BasketFlushWork) -> Weight {
        let quotes = if work.quotes == 0 {
            Weight::zero()
        } else {
            Self::basket_nav_sweep_weight(work.quotes)
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

    /// Flat pre-dispatch flush allowance: [`Self::basket_flush_work_bound`] priced as weight.
    pub fn basket_flush_weight_bound() -> Weight {
        Self::basket_flush_weight(Self::basket_flush_work_bound())
    }

    /// True when flushing every one of `hotkeys` fits the flush axis of the declared
    /// allowance: queued credit rows plus, for each hotkey with a queue, the raw length of
    /// its root weight vector (an upper bound on the destinations a curated deposit buys)
    /// sum to at most [`MAX_BASKET_FLUSH_ROWS`]. Stops as soon as the budget is exceeded, so
    /// the check itself is bounded work.
    pub(crate) fn basket_flush_fits_declared_budget(hotkeys: &[T::AccountId]) -> bool {
        let mut rows: u64 = 0;
        for hotkey in hotkeys {
            let mut queued: u64 = 0;
            for _ in PendingBasketDeposits::<T>::iter_key_prefix(hotkey) {
                queued = queued.saturating_add(1);
                rows = rows.saturating_add(1);
                if rows > MAX_BASKET_FLUSH_ROWS {
                    return false;
                }
            }
            if queued == 0 {
                continue;
            }
            let destinations = Uids::<T>::try_get(NetUid::ROOT, hotkey)
                .map(|uid| Weights::<T>::get(NetUidStorageIndex::ROOT, uid).len() as u64)
                .unwrap_or(0);
            rows = rows.saturating_add(destinations);
            if rows > MAX_BASKET_FLUSH_ROWS {
                return false;
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

    /// Deposit all of a hotkey's flushable queued credits into its basket as one batch.
    ///
    /// A credit is flushable when its origin subnet still exists and its spot value is at
    /// or above `RootClaimableThreshold[ROOT]`. The spot filter is deliberate and cheap
    /// (reserves ratio, no bignum pow): spot over-marks relative to the realizable quote,
    /// so anything it defers is certainly below the threshold. Sub-threshold credits stay
    /// queued and keep merging with future dividends until they are worth a deposit — this
    /// is what keeps dust from ever becoming a basket holding row (and what lets dust
    /// consolidation run on uncurated funds without fighting next epoch's accrual).
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
    pub(crate) fn flush_basket_deposits_for_hotkey(
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
            if !Self::if_subnet_exist(netuid) {
                Self::drop_pending_basket_deposit(hotkey, netuid, alpha);
                continue;
            }
            // Root straggler: recycle the pending credit back into the origin subnet and
            // drop the row. Do not touch basket holdings — only the unqueued dividend.
            if !on_root {
                Self::drop_pending_basket_deposit(hotkey, netuid, alpha);
                continue;
            }
            let spot: U64F64 = T::SwapInterface::current_alpha_price(netuid.into());
            #[cfg(test)]
            crate::tests::mock::inc_basket_quote_ops();
            let value: u64 = spot
                .saturating_mul(U64F64::saturating_from_num(alpha.to_u64()))
                .saturating_to_num::<u64>();
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
    /// a full cycle fits well inside a tempo, each flush is itself bounded work (holdings +
    /// 2 x origins quotes), and anything urgent is flushed eagerly by the touch hooks
    /// (claims, basket stakes, root stake changes). The drain is only the janitor for
    /// untouched funds. Runs right after coinbase.
    pub fn flush_pending_basket_deposits_block() {
        if crate::migrations::migrate_seed_beta_basket::seed_beta_basket_v2_in_progress::<T>() {
            return;
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
            return;
        };
        drop(keys);

        let (_work, last_key, _) = Self::flush_basket_deposits_for_hotkey(&hotkey);
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
    }

    /// Distributes a validator's root dividend (origin-subnet alpha, net of take) into its beta
    /// basket according to the validator's root weight vector `w` (set on subnet 0).
    ///
    /// Single-credit wrapper over [`Self::deposit_root_alpha_batch`]. Epochs no longer call
    /// this inline — they enqueue credits into [`PendingBasketDeposits`] and the queue
    /// flushes per hotkey in batches (see [`Self::flush_basket_deposits_for_hotkey`]) — but
    /// the deposit semantics described here are those of each batch.
    ///
    /// Curated flow: sell the origin alpha for TAO, then split that TAO across subnets per `w`,
    /// buying each subnet's alpha and staking it to the validator under the global escrow
    /// coldkey (a root-destination slice is held directly as the fund's root-stake cash
    /// position). The deposit then mints *fund shares* against the whole basket:
    /// `shares = value_added * P / N`, where `N` is the fund's pre-sale realizable NAV, `P` the
    /// outstanding shares, and `value_added` the realizable NAV the full sell-and-redeploy
    /// actually added (final NAV minus that pre-sale snapshot), so the deposit bears its own
    /// sell impact, buy slippage, and fees instead of socializing them, and existing holders
    /// are neither diluted nor taxed. Stakers accrue entitlement through the single
    /// per-validator `BasketRate += shares / total_root_stake` accumulator; no entitlement is
    /// ever denominated in a particular subnet's alpha, which is what allows holdings to be
    /// rebalanced without touching staker claims.
    ///
    /// Uncurated flow (no stored root weights, or explicit weights filtered to nothing): the
    /// dividend *accumulates in place* — the origin alpha is credited directly to the fund's
    /// holding on the origin subnet, with no sell and no redeploy. The default basket is
    /// therefore the emission-weighted portfolio the dividends themselves describe, the
    /// protocol executes zero trades (no swap fees, no slippage, no sell pressure) on behalf
    /// of a validator that expressed no preference, and shares still mint at NAV against the
    /// realizable value the alpha added (see
    /// [`Self::try_accumulate_root_alpha_batch`]).
    ///
    /// Attribution (both flows): the dividend was earned by the validator's WHOLE root stake,
    /// including the fund's own root-slot (escrow) position. Only the real stakers' fraction
    /// of the value mints shares; the escrow slot's fraction enters the fund unminted, so the
    /// fund's own cash yield accrues to existing share holders through N/P instead of
    /// leaking to root stakers as free shares.
    ///
    /// The whole operation is transactional. Each credit's own step (the origin sell, or the
    /// in-place accumulation) is isolated: an unknown swap/accounting failure there re-queues
    /// only that credit and the batch continues, so one borked origin cannot sink the rest. A
    /// failure in a shared phase (NAV sweep, deployment, or a dust mint) rolls back and
    /// re-queues every credit for a later flush. A terminally shallow origin credit is
    /// recycled, and a terminally shallow destination slice is retained as root cash, so one
    /// garbage subnet cannot pin the hotkey's queue indefinitely. Dividends are also recycled
    /// when the validator has no root stake to apportion against.
    ///
    /// Protocol-flow accounting is symmetric with redemption: the origin sell is booked as an
    /// outflow on the origin subnet and each redistribution buy as an inflow on its dest subnet,
    /// so that a deposit-then-claim round-trip nets to ~0 on the dest pools (the claim sell is
    /// booked as an outflow in `root_claim_for_hotkey`). An in-place accumulation moves no TAO
    /// through any pool, so it records nothing; the eventual claim sell is a genuine net
    /// extraction and books its outflow then.
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
    /// Semantics are those of [`Self::distribute_root_alpha_to_basket`] generalized to a
    /// batch. The batch is transactional as a whole and is attempted exactly once: a credit
    /// whose own step fails is re-queued from inside the attempt while the others proceed,
    /// and a shared-phase failure re-queues the whole batch for a later flush (credits may
    /// merge with future dividends and become depositable). There is deliberately no
    /// per-credit retry loop — it would multiply the NAV sweeps by the credit count and break
    /// the flat allowance [`Self::basket_flush_work_bound`] every flushing extrinsic declares.
    /// Credits are recycled when demonstrably unapportionable (no root stake), terminally
    /// untradeable, or when the seed migration owns the basket maps.
    ///
    /// Returns the approximate work performed (holdings valued as quotes, sells, buys, and
    /// in-place credits as executed rows), priced by [`Self::basket_flush_weight`] in the
    /// post-dispatch weight of callers that flush inside an extrinsic.
    pub(crate) fn deposit_root_alpha_batch(
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

        // No root stake to apportion against: recycle.
        if total_root.is_zero() {
            Self::recycle_basket_deposit_batch(batch);
            return BasketFlushWork::default();
        }

        // Approximate work executed, charged whether the deposit commits or rolls back (the
        // quotes and swaps ran either way). Uncurated: one NAV sweep plus two quotes per
        // origin, and one in-place credit row per origin. Curated: pre-sale NAV, the
        // deployment's pre-buy sweep, and its post-buy sweep (which may now include every
        // destination as a new row) as quotes; one sell per origin and one buy per
        // destination as executed rows.
        let holdings = Self::get_basket_holdings(hotkey).len() as u64;
        let credits = batch.len() as u64;
        let destinations = valid.len() as u64;
        let work = if valid.is_empty() {
            BasketFlushWork::new(holdings.saturating_add(credits.saturating_mul(2)), credits)
        } else {
            BasketFlushWork::new(
                holdings.saturating_mul(3).saturating_add(destinations),
                credits.saturating_add(destinations),
            )
        };

        let outcome = with_transaction(|| {
            let result = if valid.is_empty() {
                Self::try_accumulate_root_alpha_batch(
                    hotkey,
                    batch,
                    total_root.to_u64(),
                    escrow_root.to_u64(),
                )
            } else {
                Self::try_distribute_root_alpha_batch(
                    hotkey,
                    batch,
                    &valid,
                    total_root.to_u64(),
                    escrow_root.to_u64(),
                )
            };
            match result {
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

        // Soft failure in a shared phase (NAV sweep, deployment, dust mint): re-queue every
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

    /// Transactional body of [`Self::deposit_root_alpha_batch`]'s curated flow; any error
    /// rolls the whole batch back (the caller splits / re-queues).
    fn try_distribute_root_alpha_batch(
        hotkey: &T::AccountId,
        batch: &[(NetUid, AlphaBalance)],
        valid: &[(NetUid, u64)],
        total_root: u64,
        escrow_root: u64,
    ) -> DispatchResult {
        // A single-origin batch deploys straight from that origin's pot (the pre-queue
        // inline behavior — when a destination equals the origin no cash moves at all).
        // A multi-origin batch consolidates the cash on the root subnet account so the
        // deployment can fund every destination slice from a single pot.
        let mut origins = batch
            .iter()
            .filter(|(_, alpha)| !alpha.is_zero())
            .map(|(netuid, _)| *netuid);
        let funding_netuid = match (origins.next(), origins.next()) {
            (Some(origin), None) => origin,
            _ => NetUid::ROOT,
        };
        let root_account =
            Self::get_subnet_account_id(NetUid::ROOT).ok_or(Error::<T>::RootNetworkDoesNotExist)?;

        // Snapshot realizable NAV before any origin sell. The fund may already hold
        // origin-subnet alpha; selling the dividend moves those pools against that holding.
        // Minting against this pre-sale baseline (with value_added = final − pre-sale) makes
        // the deposit bear that sale impact instead of taxing existing shareholders. A
        // non-positive transformation fails the dust check in the mint and rolls back.
        let pre_sale_nav: u64 = Self::try_get_validator_basket_nav_tao(hotkey)?;

        // 1. Sell each origin credit for TAO, booked as protocol outflow (TAO left that
        // origin pool). Each sell is isolated: an unknown failure re-queues that credit
        // alone and the batch goes on with the others.
        let mut tao_total: u64 = 0;
        let mut sold_any = false;
        for (origin_netuid, root_alpha) in batch {
            if root_alpha.is_zero() {
                continue;
            }
            let sold = Self::isolate_basket_credit(hotkey, *origin_netuid, *root_alpha, || {
                Self::try_sell_basket_credit(
                    *origin_netuid,
                    *root_alpha,
                    funding_netuid,
                    &root_account,
                )
            });
            if let Some(Some(tao)) = sold {
                sold_any = true;
                tao_total = tao_total.saturating_add(tao);
            }
        }

        // A batch made exclusively of terminal garbage has been disposed of successfully;
        // there is no value against which to mint fund shares.
        if !sold_any {
            return Ok(());
        }

        // 2. Deploy the TAO across the basket per the weight vector. `deploy_tao_into_basket`
        // still returns its post-sale pre-buy snapshot and buy-side delta; fold those into
        // the full-transformation value against the pre-sale NAV.
        let (post_sale_nav, deploy_delta) = Self::deploy_tao_into_basket(
            hotkey,
            valid,
            tao_total,
            BasketFunding::Protocol {
                origin_netuid: funding_netuid,
            },
        )?;
        let value_added = post_sale_nav
            .saturating_add(deploy_delta)
            .saturating_sub(pre_sale_nav);

        // 3. Mint fund shares for the stakers' fraction of the value added.
        Self::mint_basket_dividend_shares(
            hotkey,
            pre_sale_nav,
            value_added,
            total_root,
            escrow_root,
        )
    }

    /// One credit's step of the curated flow: sell the origin alpha for TAO, book the
    /// outflow, and consolidate the cash on the root pot when the batch funds from root.
    /// `Ok(Some(tao))` sold; `Ok(None)` the origin is terminally shallow and the credit was
    /// recycled (disposed of, nothing to deploy); `Err` an unknown failure the caller
    /// isolates.
    fn try_sell_basket_credit(
        origin_netuid: NetUid,
        root_alpha: AlphaBalance,
        funding_netuid: NetUid,
        root_account: &T::AccountId,
    ) -> Result<Option<u64>, DispatchError> {
        if Self::try_realizable_tao_for_alpha(origin_netuid, root_alpha.to_u64())?.is_none() {
            // This dividend cannot be sold on a terminally shallow origin. Recycling the
            // still-unassigned credit is preferable to pinning every later flush for this
            // hotkey behind the bad subnet.
            Self::recycle_subnet_alpha(origin_netuid, root_alpha);
            return Ok(None);
        }
        let tao = match Self::swap_basket_alpha_for_tao_chunks(origin_netuid, root_alpha) {
            Ok(tao) => tao,
            Err(err)
                if T::SwapInterface::classify_failure(&err)
                    == SwapFailureKind::TerminalLiquidity =>
            {
                // The chunk helper is atomic, so a late terminal failure leaves the entire
                // credit untouched and safe to recycle here.
                Self::recycle_subnet_alpha(origin_netuid, root_alpha);
                return Ok(None);
            }
            Err(err) => return Err(err),
        };
        Self::record_protocol_outflow(origin_netuid, tao);
        if origin_netuid != funding_netuid && !origin_netuid.is_root() {
            Self::transfer_tao_from_subnet(origin_netuid, root_account, tao.into())?;
        }
        Ok(Some(tao.to_u64()))
    }

    /// Transactional body of [`Self::deposit_root_alpha_batch`]'s uncurated flow: each
    /// dividend credit is applied directly to the fund's holding on the subnet it arrived on.
    /// No swap runs — the alpha is already counted in `SubnetAlphaOut` (the recycle path
    /// decrements it when a credit is truly dropped), it just is not assigned to any stake
    /// position yet, so the whole deposit is a share-pool credit. Any error rolls the batch
    /// back (the caller splits / re-queues).
    ///
    /// Each credit is valued as the realizable delta on its origin holding alone: crediting
    /// stake moves no pool, so every other holding's quote is unchanged and the full-fund
    /// `nav_after` sweep the curated flow needs collapses to one extra quote per origin.
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
        let nav_before: u64 = Self::try_get_validator_basket_nav_tao(hotkey)?;

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

    /// One credit's step of the uncurated flow: credit the origin alpha to the fund's
    /// holding on that subnet and value it as the realizable delta on that holding alone.
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
        let Some(origin_before) = Self::try_realizable_tao_for_alpha(origin_netuid, held_before)?
        else {
            // Do not add fresh dividends to a holding which is already terminally
            // untradeable. The credit has not been assigned to a staker yet, so recycle it
            // rather than manufacturing shares with a zero mark.
            Self::recycle_subnet_alpha(origin_netuid, root_alpha);
            return Ok(None);
        };

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
        let Some(origin_after) = Self::try_realizable_tao_for_alpha(origin_netuid, held_after)?
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
        Ok(Some(origin_after.saturating_sub(origin_before)))
    }
}
