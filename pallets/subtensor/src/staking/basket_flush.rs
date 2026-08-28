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
use frame_support::storage::{TransactionOutcome, with_transaction};
use pallet_alpha_assets::AlphaAssetsInterface;
use substrate_fixed::types::U64F64;
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
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
    /// Returns `(work, last_key, completed)`: the approximate quote work done (scan-priced
    /// into claim weights by `root_claim_for_hotkey`), the raw storage key of the hotkey's
    /// last queue entry, and whether every credit selected for this flush was settled. The
    /// drain stores `last_key` as its cursor so a hotkey whose credits are all deferred dust
    /// still gets skipped past instead of pinning the queue head.
    pub(crate) fn flush_basket_deposits_for_hotkey(
        hotkey: &T::AccountId,
    ) -> (u64, Option<Vec<u8>>, bool) {
        let threshold: u64 =
            RootClaimableThreshold::<T>::get(NetUid::ROOT).saturating_to_num::<u64>();
        let on_root = Self::is_hotkey_registered_on_network(NetUid::ROOT, hotkey);

        let mut work: u64 = 0;
        let mut last_netuid: Option<NetUid> = None;
        let mut batch: Vec<(NetUid, AlphaBalance)> = Vec::new();
        for (netuid, alpha) in PendingBasketDeposits::<T>::iter_prefix(hotkey) {
            last_netuid = Some(netuid);
            work = work.saturating_add(1);
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
    /// The whole operation is transactional. Unknown swap/accounting failures (or a dust mint)
    /// roll back and re-queue the original alpha for a later flush, with multi-credit batches
    /// split into per-origin retries first. A terminally shallow origin credit is recycled, and
    /// a terminally shallow destination slice is retained as root cash, so one garbage subnet
    /// cannot pin the hotkey's queue indefinitely. Dividends are also recycled when the
    /// validator has no root stake to apportion against.
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
    /// batch. The batch is transactional as a whole; on soft failure a multi-credit batch
    /// splits into per-origin retries so one borked origin cannot sink healthy credits, and
    /// any credit that still fails is re-queued for a later flush (it may merge with future
    /// dividends and become depositable). Credits are recycled when demonstrably
    /// unapportionable (no root stake), terminally untradeable, or when the seed migration owns
    /// the basket maps.
    ///
    /// Returns the approximate quote work performed (holdings valued plus origin quotes),
    /// scan-priced into claim weights by callers that flush inside an extrinsic.
    pub(crate) fn deposit_root_alpha_batch(
        hotkey: &T::AccountId,
        batch: &[(NetUid, AlphaBalance)],
    ) -> u64 {
        if batch.iter().all(|(_, alpha)| alpha.is_zero()) {
            return 0;
        }

        // Seed migration still converting legacy claim state. Coinbase only queues its
        // calculated per-hotkey credits; recycle an unexpected direct deposit defensively rather
        // than writing BasketRate/Shares that a later pass would overwrite.
        if crate::migrations::migrate_seed_beta_basket::seed_beta_basket_v2_in_progress::<T>() {
            Self::recycle_basket_deposit_batch(batch);
            return 0;
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
            return 0;
        }

        // Approximate quote units executed, charged whether the deposit commits or rolls
        // back (the quotes ran either way). Uncurated: one NAV sweep plus two quotes per
        // origin. Curated: pre-sale NAV, deployment's pre/post-buy NAV sweeps, one buy per
        // destination, plus one sell per origin.
        let holdings = Self::get_basket_holdings(hotkey).len() as u64;
        let credits = batch.len() as u64;
        let work = if valid.is_empty() {
            holdings.saturating_add(credits.saturating_mul(2))
        } else {
            holdings
                .saturating_mul(3)
                .saturating_add(valid.len() as u64)
                .saturating_add(credits)
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
            return work.saturating_add(Self::stamp_beta_baseline_if_new(hotkey));
        }

        // Soft failure: split a multi-credit batch so one bad origin cannot sink the rest.
        // Singleton failures re-queue — recoverable later (merge with future dividends, or a
        // healthier pool) rather than recycling healthy work away.
        if credits > 1 {
            let mut total = work;
            for credit in batch.iter().copied() {
                if credit.1.is_zero() {
                    continue;
                }
                total = total.saturating_add(Self::deposit_root_alpha_batch(hotkey, &[credit]));
            }
            return total;
        }

        Self::requeue_basket_deposit_batch(hotkey, batch);
        work
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
        // origin pool).
        let mut tao_total: u64 = 0;
        let mut sold_any = false;
        for (origin_netuid, root_alpha) in batch {
            if root_alpha.is_zero() {
                continue;
            }
            match Self::try_realizable_tao_for_alpha(*origin_netuid, root_alpha.to_u64())? {
                Some(_) => {}
                None => {
                    // This dividend cannot be sold on a terminally shallow origin. Recycling
                    // the still-unassigned credit is preferable to pinning every later flush
                    // for this hotkey behind the bad subnet.
                    Self::recycle_subnet_alpha(*origin_netuid, *root_alpha);
                    continue;
                }
            }
            let tao = match Self::swap_basket_alpha_for_tao_chunks(*origin_netuid, *root_alpha) {
                Ok(tao) => tao,
                Err(err)
                    if T::SwapInterface::classify_failure(&err)
                        == subtensor_swap_interface::SwapFailureKind::TerminalLiquidity =>
                {
                    // The chunk helper is atomic, so a late terminal failure leaves the
                    // entire credit untouched and safe to recycle here.
                    Self::recycle_subnet_alpha(*origin_netuid, *root_alpha);
                    continue;
                }
                Err(err) => return Err(err),
            };
            sold_any = true;
            Self::record_protocol_outflow(*origin_netuid, tao);
            if *origin_netuid != funding_netuid && !origin_netuid.is_root() {
                Self::transfer_tao_from_subnet(*origin_netuid, &root_account, tao.into())?;
            }
            tao_total = tao_total.saturating_add(tao.to_u64());
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

        let mut value_added: u64 = 0;
        let mut accumulated_any = false;
        for (origin_netuid, root_alpha) in batch {
            if root_alpha.is_zero() {
                continue;
            }
            let held_before: u64 =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, *origin_netuid)
                    .to_u64();
            let origin_before =
                match Self::try_realizable_tao_for_alpha(*origin_netuid, held_before)? {
                    Some(value) => value,
                    None => {
                        // Do not add fresh dividends to a holding which is already terminally
                        // untradeable. The credit has not been assigned to a staker yet, so recycle
                        // it rather than manufacturing shares with a zero mark.
                        Self::recycle_subnet_alpha(*origin_netuid, *root_alpha);
                        continue;
                    }
                };

            Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey,
                &escrow,
                *origin_netuid,
                *root_alpha,
            );

            // Re-read the holding after the credit so share-pool rounding is priced in.
            let held_after: u64 =
                Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, &escrow, *origin_netuid)
                    .to_u64();
            let origin_after = match Self::try_realizable_tao_for_alpha(*origin_netuid, held_after)?
            {
                Some(value) => value,
                None => {
                    // Adding the credit crossed into terminal territory. Undo only the amount
                    // actually assigned to the holding, recycle the whole unassigned credit,
                    // and leave the pre-existing position untouched.
                    let assigned = held_after.saturating_sub(held_before);
                    Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey,
                        &escrow,
                        *origin_netuid,
                        assigned.into(),
                    );
                    Self::recycle_subnet_alpha(*origin_netuid, *root_alpha);
                    continue;
                }
            };
            accumulated_any = true;
            value_added = value_added.saturating_add(origin_after.saturating_sub(origin_before));
        }

        if !accumulated_any {
            return Ok(());
        }

        Self::mint_basket_dividend_shares(hotkey, nav_before, value_added, total_root, escrow_root)
    }
}
