//! Shorts at subnet dissolution (SPEC section 7.4 phase E, P12, P16; Jacob, Nov 27).
//!
//! `pallet_subtensor` flags a subnet for dissolution once ([`crate::hooks::LendingInterface::on_dissolve_flag`],
//! with the pool's TAO at that moment). Its cleanup then calls [`crate::hooks::LendingInterface::on_dissolve`]
//! once, before the pot is distributed to stakers. That call settles every short of the subnet:
//!
//! 1. the short's interest is accrued to the dissolution block at the curve rate and credited to
//!    the subnet's pool TAO (`SubnetTAO`, not `TotalStake`: the flag already took the pool out of
//!    `TotalStake`);
//! 2. the dereg rate `r = min(T_flag, T_ema, T_A) / all_alpha` is read once, after every interest
//!    credit, so `T_A` includes them (P12);
//! 3. the short owes `q * r` (zone part per [`P16_ZONE_SETTLEMENT`]). It pays `min(owed, C + P)`
//!    from its collateral and proceeds into the subnet pot, and the rest of its custody, the
//!    deposit included, goes back to the holder. The minted alpha `q` leaves `SyntheticOutstanding`
//!    and stays in the pool, so no alpha is minted or burned (SPEC I2, I7).
//!
//! The settled TAO is recorded in [`Dissolving::settled_tao`]. `pallet_subtensor` pays it to the
//! stakers and takes it out of the pool reserve before the owner refund reads the pot, so the
//! refund is computed as it is for the same subnet with no lending (see the subtensor side).
//!
//! Longs are not settled here; see the TODO on `on_dissolve` in `hooks`.

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{
    dispatch::DispatchResult,
    ensure,
    storage::with_storage_layer,
    weights::{Weight, WeightMeter},
};
use frame_system::pallet_prelude::BlockNumberFor;
use scale_info::TypeInfo;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{NetUid, TaoBalance};

use crate::{
    Config, DissolvingSubnets, Error, Event, LendingParamsStore, LendingPoolInterface, Pallet,
    PoolTaoEma, PositionCount, Positions, ResidualAlpha, SyntheticOutstanding, TotalPositions,
    WeightInfo,
    math::{
        default::Launch,
        dereg::{rate, short_owed},
        fixed::{Price, mul_price_floor},
    },
    types::{Leg, Position, PositionId, Side},
};

/// P16 zone settlement: a short minted in the dereg zone settles at its own proceeds, the older
/// alpha at `r` (`math::dereg::short_owed`). Pending Jacob. The positions carry no zone fields
/// yet, so the zone part is zero and this switch changes no settlement until they do. Removing P16
/// is this one line plus the test that runs both settings ([`settle_short_amount_with`]).
pub const P16_ZONE_SETTLEMENT: bool = true; // pending Jacob

/// What a short owes the pot at dereg, with the P16 switch of the build ([`P16_ZONE_SETTLEMENT`]).
#[must_use]
pub fn settle_short_amount(q: u64, zone_minted: u64, zone_proceeds: u64, r: Price) -> u64 {
    settle_short_amount_with(P16_ZONE_SETTLEMENT, q, zone_minted, zone_proceeds, r)
}

/// [`settle_short_amount`] under an explicit P16 setting. With the zone on, the zone part settles
/// at its proceeds and the rest at `r`; with it off, every short settles at `r`.
#[must_use]
pub fn settle_short_amount_with(
    zone_settlement: bool,
    q: u64,
    zone_minted: u64,
    zone_proceeds: u64,
    r: Price,
) -> u64 {
    if zone_settlement {
        short_owed(q, zone_minted, zone_proceeds, r)
    } else {
        mul_price_floor(q, r)
    }
}

/// The lending record of a subnet being dissolved, kept until the owner refund has read it.
#[freeze_struct("5a1c5445e7a5ae40")]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct Dissolving {
    /// The pool's TAO when the subnet was flagged, in rao: `T_flag`.
    pub t_flag: u64,
    /// The lending alpha of the subnet at the flag (`SyntheticOutstanding` plus
    /// `ResidualAlpha`). Frozen until the refund, so the legacy owner-cut deduction reads the
    /// same value as it does for the subnet with no lending, whose issuance it was taken from.
    pub lending_alpha: u64,
    /// TAO the shorts settled into the pot, in rao: `S`. Paid to the stakers, not the refund.
    pub settled_tao: u64,
}

impl<T: Config> Pallet<T> {
    /// The lending alpha of `netuid` as it stands: the open shorts' `q` and the defaulted shorts'
    /// residual alpha.
    #[must_use]
    pub(crate) fn live_lending_alpha(netuid: NetUid) -> u64 {
        SyntheticOutstanding::<T>::get(netuid).saturating_add(ResidualAlpha::<T>::get(netuid))
    }

    /// Credits `amount` TAO from `from` to the subnet's pool reserve while the subnet is being
    /// dissolved: the TAO joins `SubnetTAO` and not `TotalStake`, which the flag already took the
    /// pool out of. Refuses a pool that credited any other amount.
    pub(crate) fn credit_dissolving(
        netuid: NetUid,
        from: &T::AccountId,
        amount: u64,
    ) -> DispatchResult {
        let moved = T::Pool::credit_dissolving_tao(netuid, from, amount)?;
        ensure!(moved == amount, Error::<T>::QuoteMismatch);
        Ok(())
    }

    /// The flag of `do_dissolve_network` (see the module doc). Records the subnet's lending
    /// values only when it has lending on it, so a subnet with none is unchanged.
    pub fn record_dissolve_flag(netuid: NetUid, pool_tao: TaoBalance) {
        let lending_alpha = Self::live_lending_alpha(netuid);
        if PositionCount::<T>::get(netuid) == 0 && lending_alpha == 0 {
            return;
        }
        DissolvingSubnets::<T>::insert(
            netuid,
            Dissolving {
                t_flag: pool_tao.into(),
                lending_alpha,
                settled_tao: 0,
            },
        );
    }

    /// The settlement of every short of `netuid` (see the module doc). Charges the work to
    /// `meter` at its measured weight ([`Self::dissolution_weight`]). The scan of `Positions` has
    /// no per-subnet index, so it reads every open position once (bounded by `TotalPositions`,
    /// not by the subnet).
    pub fn settle_shorts_at_dissolution(netuid: NetUid, meter: &mut WeightMeter) {
        let record = DissolvingSubnets::<T>::get(netuid);
        let book = SyntheticOutstanding::<T>::get(netuid);
        let residual = ResidualAlpha::<T>::get(netuid);
        let open = PositionCount::<T>::get(netuid);
        if record.is_none() && open == 0 && book == 0 && residual == 0 {
            meter.consume(Self::dissolution_weight(0, 0));
            return;
        }

        // The frozen values: the flag's when it ran, else the subnet's as it stands now.
        let t_flag = record.map_or_else(|| T::Pool::pool_tao(netuid), |rec| rec.t_flag);
        let lending_alpha =
            record.map_or_else(|| Self::live_lending_alpha(netuid), |rec| rec.lending_alpha);
        let settled_before = record.map_or(0, |rec| rec.settled_tao);

        // A subnet with no position at all has no short to look for.
        let (shorts, scanned) = if open == 0 {
            (Vec::new(), 0)
        } else {
            (Self::open_shorts(netuid), TotalPositions::<T>::get())
        };
        let found = u32::try_from(shorts.len()).unwrap_or(u32::MAX);
        meter.consume(Self::dissolution_weight(
            found,
            scanned.saturating_sub(found),
        ));
        let now = frame_system::Pallet::<T>::block_number();
        let lending = LendingParamsStore::<T>::get();
        let curve = lending.rate_curve();
        // Without the price rings a short settles with no accrual; the rings of a live subnet
        // always exist (SPEC I25), so this only guards a subnet that never had them.
        let refs = Self::price_refs(netuid).ok();

        // Pass 1: accrue every short to `now` and credit the interest to the pool, so that `T_A`
        // below reads the pool after all of it.
        let mut accrued: Vec<(
            PositionId<T::AccountId>,
            T::AccountId,
            Position<BlockNumberFor<T>>,
        )> = Vec::with_capacity(shorts.len());
        for (id, pos) in shorts {
            let custody = Self::position_custody(&id, pos.opened_at);
            let settled = match refs {
                Some(refs) => {
                    let (settled, charge) = pos.settle_with::<Launch>(&refs, &curve, now);
                    let paid = with_storage_layer(|| {
                        Self::pay_interest_via(
                            &id.0,
                            netuid,
                            Side::Short,
                            &custody,
                            charge.amount,
                            charge.rate_bps,
                            true,
                        )
                    });
                    match paid {
                        Ok(()) => {
                            Positions::<T>::insert(&id, settled);
                            settled
                        }
                        // The accrual is not charged: the short settles from its position as it was.
                        Err(_) => pos,
                    }
                }
                None => pos,
            };
            accrued.push((id, custody, settled));
        }

        // The dereg rate (P12), read once after the interest credits.
        let t_a = T::Pool::pool_tao(netuid);
        let all_alpha = u128::from(T::Pool::alpha_issuance(netuid));
        // No EMA means no constraint from it: the rate is then `min(T_flag, T_A)`.
        let t_ema = PoolTaoEma::<T>::get(netuid).unwrap_or(u64::MAX);
        let r = rate(t_flag, t_ema, t_a, all_alpha);

        // Pass 2: settle each short once.
        let mut settled_now = 0u64;
        for (id, custody, pos) in accrued {
            let Leg::Short { minted, proceeds } = pos.leg else {
                continue;
            };
            let holdings = pos.collateral.saturating_add(proceeds);
            let owed = settle_short_amount(minted, 0, 0, r);
            // The payment is drawn from the collateral and then the proceeds (the total is what
            // moves; the order only names the parts of it).
            let paid = owed.min(holdings);
            let refund = holdings.saturating_sub(paid).saturating_add(pos.deposit);
            let result = with_storage_layer(|| {
                Self::settle_one_short(&id, &pos, &custody, minted, paid, refund, r)
            });
            // On error the short stays open and is not settled; its records are as they were.
            if result.is_ok() {
                settled_now = settled_now.saturating_add(paid);
            }
        }

        ResidualAlpha::<T>::remove(netuid);
        DissolvingSubnets::<T>::insert(
            netuid,
            Dissolving {
                t_flag,
                lending_alpha,
                settled_tao: settled_before.saturating_add(settled_now),
            },
        );
    }

    /// The open shorts of `netuid`, read off `Positions`: one pass over every open position.
    pub(crate) fn open_shorts(
        netuid: NetUid,
    ) -> Vec<(PositionId<T::AccountId>, Position<BlockNumberFor<T>>)> {
        Positions::<T>::iter()
            .filter(|(id, _)| id.1 == netuid && id.2 == Side::Short)
            .collect()
    }

    /// Pays one short's settlement and removes it: the paid amount from custody into the pot,
    /// the rest of custody to the holder, `q` off the short book and the position with its
    /// records.
    fn settle_one_short(
        id: &PositionId<T::AccountId>,
        pos: &Position<BlockNumberFor<T>>,
        custody: &T::AccountId,
        minted: u64,
        paid: u64,
        refund: u64,
        r: Price,
    ) -> DispatchResult {
        let (who, netuid, _) = id;
        if paid > 0 {
            Self::credit_dissolving(*netuid, custody, paid)?;
        }
        if refund > 0 {
            Self::move_tao(custody, who, refund)?;
        }
        let book = SyntheticOutstanding::<T>::get(netuid)
            .checked_sub(minted)
            .ok_or(Error::<T>::Overflow)?;
        SyntheticOutstanding::<T>::insert(netuid, book);
        Self::forget_position(id, pos.charge_at);
        Self::deposit_event(Event::ShortSettledOnDissolution {
            who: who.clone(),
            netuid: *netuid,
            price: r,
            paid,
        });
        Ok(())
    }

    /// The measured weight of a settlement that settles `shorts` shorts and reads `others`
    /// positions of other subnets (or longs of this one) on its pass over `Positions`: the
    /// `on_dissolve` benchmark, whose shorts are every open position, plus the benchmark of the
    /// pass for the rest. With neither, it is the weight of a subnet with no lending.
    #[must_use]
    pub fn dissolution_weight(shorts: u32, others: u32) -> Weight {
        let settled = T::WeightInfo::on_dissolve(shorts);
        if others == 0 {
            settled
        } else {
            settled.saturating_add(T::WeightInfo::on_dissolve_scan(others))
        }
    }
}
