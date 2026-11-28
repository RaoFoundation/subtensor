//! The route an order takes through the hub, and the arithmetic of the vaults.
//!
//! Every order follows one line:
//!
//! ```text
//! stablecoin <-> tUSD <-> TAO <-> subnet alpha -> service credits
//! ```
//!
//! [`Pallet::collect`] brings what the spoke took onto that line as tUSD or TAO in the hub
//! account, and [`Pallet::deliver`] walks it to what the order asked for. An order leaves
//! nothing in the hub account. Every step moves an amount that it was given, never a
//! balance that it read, so funds that someone sends to the hub account from outside
//! change no order: they rest there.
//!
//! ## Alpha tokens
//!
//! The alpha token of a subnet is a share of that subnet's vault. The vault stakes at the
//! subnet's index hotkey, so its alpha grows with emissions while the number of shares
//! stands still: a share is worth more alpha over time, and the token never rebases.
//!
//! ```text
//! shares minted = alpha staked * (shares + V) / (alpha in vault + V)
//! alpha paid    = shares burned * (alpha in vault + V) / (shares + V)
//! ```
//!
//! `V` is a thousandth of a unit of each ([`VIRTUAL`]): shares and alpha that nobody holds.
//! It fixes the first share at one alpha. It also bounds what rounding can cost a buyer:
//! a share is never worth more than `(alpha in vault + V) / V` units, so to make the next
//! buyer lose one unit to rounding, an attacker must give the vault a million units, and
//! loses them to the virtual shares. The price is that the virtual shares earn as real
//! ones do: `V / (shares + V)` of a vault's earnings stay in it unowned, a millionth for a
//! vault of a thousand alpha. Both divisions round against the trader.

use frame_support::{
    ensure,
    traits::{
        UnixTime,
        fungible::{Inspect, Mutate},
        tokens::{Fortitude, Preservation},
    },
};
use sp_arithmetic::{Rounding, helpers_128bit::multiply_by_rational_with_rounding};
use sp_core::H160;
use sp_runtime::{DispatchError, DispatchResult};
use subtensor_runtime_common::{
    NetUid, TaoBalance, Token, TusdBalance,
    rails::{BridgeId, BridgedPayment, Payer},
};

use crate::{
    AlphaSupply, Config, CurrencyOf, Destination, Error, Event, Markets, Message, NativeAsset,
    Order, Pallet, RailsStaking, SpokeAsset, SubnetStatus, TaoCap, TaoSupply, Tokens,
};

/// The virtual alpha and the virtual shares of every vault: a thousandth of a unit of each.
pub const VIRTUAL: u128 = 1_000_000;

/// The scale of a rate: a rate of `RATE_SCALE` is one alpha per share.
pub const RATE_SCALE: u128 = 1_000_000_000_000_000_000;

/// What the hub account holds for an order while it settles.
#[derive(Clone, Copy)]
enum Funds {
    Tusd(TusdBalance),
    Tao(TaoBalance),
}

/// The shares that `alpha` newly staked buys from a vault of `staked` alpha and `shares`.
pub fn shares_for_alpha(alpha: u64, staked: u64, shares: u64) -> Option<u64> {
    multiply_by_rational_with_rounding(
        u128::from(alpha),
        u128::from(shares).checked_add(VIRTUAL)?,
        u128::from(staked).checked_add(VIRTUAL)?,
        Rounding::Down,
    )
    .and_then(|minted| u64::try_from(minted).ok())
}

/// The alpha that `burned` shares redeem from a vault of `staked` alpha and `shares`: never
/// more than the vault holds.
pub fn alpha_for_shares(burned: u64, staked: u64, shares: u64) -> Option<u64> {
    multiply_by_rational_with_rounding(
        u128::from(burned),
        u128::from(staked).checked_add(VIRTUAL)?,
        u128::from(shares).checked_add(VIRTUAL)?,
        Rounding::Down,
    )
    .and_then(|paid| u64::try_from(paid).ok())
    .map(|paid| paid.min(staked))
}

/// The alpha one share of a vault of `staked` alpha and `shares` is worth, times
/// [`RATE_SCALE`].
pub fn rate(staked: u64, shares: u64) -> u128 {
    u128::from(staked)
        .saturating_add(VIRTUAL)
        .saturating_mul(RATE_SCALE)
        .checked_div(u128::from(shares).saturating_add(VIRTUAL))
        .unwrap_or(RATE_SCALE)
}

impl<T: Config> Pallet<T> {
    /// The alpha one share of the vault of `netuid` is worth now, times [`RATE_SCALE`].
    /// `None` if the subnet has no market or its market only winds down.
    pub fn alpha_rate(netuid: NetUid) -> Option<u128> {
        let market = Self::live_market(netuid).ok()?;
        let staked = T::Staking::staked(&Self::vault_account(netuid), netuid);
        Some(rate(staked.to_u64(), market.shares))
    }

    /// Whether the hub let the spoke of `bridge` mint the `amount` of `source` that the
    /// spoke says it burned. The stablecoin is not minted by the hub and always passes.
    pub(crate) fn ensure_backed(
        bridge: BridgeId,
        source: SpokeAsset,
        amount: u64,
    ) -> Result<(), DispatchError> {
        let supply = match source {
            SpokeAsset::Stable => return Ok(()),
            SpokeAsset::Tao => TaoSupply::<T>::get(bridge).to_u64(),
            SpokeAsset::Alpha(netuid) => AlphaSupply::<T>::get(netuid, bridge),
        };
        ensure!(amount <= supply, Error::<T>::Unbacked);
        Ok(())
    }

    /// Runs `order` from the spoke of `bridge` to its end. Returns what it delivered. The
    /// caller runs this in a storage layer and discards it on failure.
    pub(crate) fn route_order(
        bridge: BridgeId,
        order: &Order<T::AccountId>,
    ) -> Result<u64, DispatchError> {
        ensure!(
            T::Time::now().as_secs() <= order.deadline,
            Error::<T>::Expired
        );
        let same_asset = match (order.source, &order.destination) {
            (SpokeAsset::Stable, Destination::Stable { .. }) => true,
            (SpokeAsset::Tao, Destination::Tao { .. }) => true,
            (SpokeAsset::Alpha(sold), Destination::Alpha { netuid, .. }) => sold == *netuid,
            _ => false,
        };
        ensure!(!same_asset, Error::<T>::InvalidRoute);
        let funds = Self::collect(bridge, order.source, order.amount)?;
        Self::deliver(
            bridge,
            funds,
            &order.destination,
            order.min_out,
            Some(order.nonce),
        )
    }

    /// Runs a payment by `who`, an account on Bittensor, to a destination on the spoke of
    /// `bridge`. Returns what it delivered.
    pub(crate) fn route_native(
        who: &T::AccountId,
        bridge: BridgeId,
        source: NativeAsset,
        amount: u64,
        destination: &Destination<T::AccountId>,
        min_out: u64,
    ) -> Result<u64, DispatchError> {
        // An account on Bittensor swaps in the pool and mints gamma directly.
        ensure!(
            matches!(
                destination,
                Destination::Stable { .. } | Destination::Tao { .. } | Destination::Alpha { .. }
            ),
            Error::<T>::InvalidRoute
        );
        let hub = Self::hub_account();
        let funds = match source {
            NativeAsset::Tusd => {
                pallet_tusd::Pallet::<T>::do_transfer(who, &hub, amount.into())?;
                Funds::Tusd(amount.into())
            }
            NativeAsset::Tao => {
                Self::transfer_tao(who, &hub, amount.into())?;
                Funds::Tao(amount.into())
            }
        };
        Self::deliver(bridge, funds, destination, min_out, None)
    }

    /// Brings what the spoke took for an order into the hub account, as tUSD or TAO.
    fn collect(bridge: BridgeId, source: SpokeAsset, amount: u64) -> Result<Funds, DispatchError> {
        ensure!(amount != 0, Error::<T>::AmountTooLow);
        let hub = Self::hub_account();
        match source {
            // The portal locked the stablecoin: its bridge mints as much tUSD.
            SpokeAsset::Stable => {
                pallet_tusd::Pallet::<T>::mint(bridge, &hub, amount.into())?;
                Ok(Funds::Tusd(amount.into()))
            }
            // The portal burned canonical TAO: the reserve releases as much TAO.
            SpokeAsset::Tao => {
                let supply = TaoSupply::<T>::get(bridge)
                    .to_u64()
                    .checked_sub(amount)
                    .ok_or(Error::<T>::Unbacked)?;
                let reserve = Self::reserve_account();
                let left = CurrencyOf::<T>::balance(&reserve)
                    .to_u64()
                    .saturating_sub(amount);
                ensure!(
                    left == 0 || left >= CurrencyOf::<T>::minimum_balance().to_u64(),
                    Error::<T>::ReserveDust
                );
                TaoSupply::<T>::insert(bridge, TaoBalance::from(supply));
                Self::transfer_tao(&reserve, &hub, amount.into())?;
                Ok(Funds::Tao(amount.into()))
            }
            // The portal burned shares of a vault: the vault unstakes their alpha.
            SpokeAsset::Alpha(netuid) => {
                Self::redeem_shares(bridge, netuid, amount).map(Funds::Tao)
            }
        }
    }

    /// Walks `funds` in the hub account to `destination`, and queues what the spoke of
    /// `bridge` must pay out. Returns the amount delivered, at least `min_out`.
    fn deliver(
        bridge: BridgeId,
        funds: Funds,
        destination: &Destination<T::AccountId>,
        min_out: u64,
        order: Option<u64>,
    ) -> Result<u64, DispatchError> {
        let hub = Self::hub_account();
        let credit = |asset: SpokeAsset, to: H160, amount: u64, rate: u128| Message::Credit {
            asset,
            to,
            amount,
            order,
            refund: false,
            rate,
        };
        let amount_out = match destination {
            Destination::Stable { to } => {
                let tusd = Self::as_tusd(funds)?;
                ensure!(tusd.to_u64() >= min_out, Error::<T>::SlippageExceeded);
                pallet_tusd::Pallet::<T>::burn(bridge, &hub, tusd)?;
                Self::enqueue(bridge, credit(SpokeAsset::Stable, *to, tusd.to_u64(), 0))?;
                tusd.to_u64()
            }
            Destination::NativeTusd { to } => {
                let tusd = Self::as_tusd(funds)?;
                ensure!(tusd.to_u64() >= min_out, Error::<T>::SlippageExceeded);
                pallet_tusd::Pallet::<T>::do_transfer(&hub, to, tusd)?;
                tusd.to_u64()
            }
            Destination::Tao { to } => {
                let tao = Self::as_tao(funds)?;
                ensure!(tao.to_u64() >= min_out, Error::<T>::SlippageExceeded);
                ensure!(
                    Tokens::<T>::contains_key(bridge, SpokeAsset::Tao),
                    Error::<T>::TokenNotRegistered
                );
                let supply = TaoSupply::<T>::get(bridge)
                    .to_u64()
                    .checked_add(tao.to_u64())
                    .ok_or(Error::<T>::Overflow)?;
                ensure!(
                    supply <= TaoCap::<T>::get(bridge).to_u64(),
                    Error::<T>::TaoCapExceeded
                );
                Self::transfer_tao(&hub, &Self::reserve_account(), tao)?;
                TaoSupply::<T>::insert(bridge, TaoBalance::from(supply));
                Self::enqueue(bridge, credit(SpokeAsset::Tao, *to, tao.to_u64(), 0))?;
                tao.to_u64()
            }
            Destination::NativeTao { to } => {
                let tao = Self::as_tao(funds)?;
                ensure!(tao.to_u64() >= min_out, Error::<T>::SlippageExceeded);
                Self::transfer_tao(&hub, to, tao)?;
                tao.to_u64()
            }
            Destination::Alpha { netuid, to } => {
                let tao = Self::as_tao(funds)?;
                let (shares, rate) = Self::mint_shares(bridge, *netuid, tao)?;
                ensure!(shares >= min_out, Error::<T>::SlippageExceeded);
                Self::enqueue(
                    bridge,
                    credit(SpokeAsset::Alpha(*netuid), *to, shares, rate),
                )?;
                shares
            }
            Destination::Credits {
                netuid,
                service,
                beneficiary,
            } => {
                let tao = Self::as_tao(funds)?;
                // The hub bought this TAO in the pool. It counts for no more dollars than
                // the pool's price over time, so that a pool pushed cheap for a moment
                // cannot turn a dollar into more than a dollar of credit.
                let price_cap = pallet_canonical_pool::Pallet::<T>::twap_price()
                    .ok_or(Error::<T>::TwapUnavailable)?;
                let origin_chain = pallet_tusd::Bridges::<T>::get(bridge)
                    .ok_or(Error::<T>::BridgeDisabled)?
                    .route
                    .origin;
                // The subnet's vault pays: staking keeps the existential deposit in the
                // account that pays, and the hub account must end empty.
                let vault = Self::vault_account(*netuid);
                Self::transfer_tao(&hub, &vault, tao)?;
                let (alpha_burned, gamma) = T::Staking::spend_at_service(
                    &vault,
                    *netuid,
                    service,
                    tao,
                    price_cap,
                    BridgedPayment {
                        payer: Payer::Evm(*beneficiary),
                        origin_chain,
                        origin_nonce: order.unwrap_or_default(),
                    },
                )?;
                ensure!(gamma.to_u64() >= min_out, Error::<T>::SlippageExceeded);
                Self::deposit_event(Event::CreditsPurchased {
                    bridge,
                    nonce: order,
                    beneficiary: *beneficiary,
                    netuid: *netuid,
                    service: service.clone(),
                    tao,
                    alpha_burned,
                    gamma,
                });
                gamma.to_u64()
            }
        };
        Ok(amount_out)
    }

    /// `funds` as tUSD, selling TAO in the canonical pool if that is what they are.
    fn as_tusd(funds: Funds) -> Result<TusdBalance, DispatchError> {
        match funds {
            Funds::Tusd(tusd) => Ok(tusd),
            Funds::Tao(tao) => pallet_canonical_pool::Pallet::<T>::do_swap_tao_for_tusd(
                &Self::hub_account(),
                tao,
                TusdBalance::ZERO,
            ),
        }
    }

    /// `funds` as TAO, buying it in the canonical pool if they are tUSD.
    fn as_tao(funds: Funds) -> Result<TaoBalance, DispatchError> {
        match funds {
            Funds::Tao(tao) => Ok(tao),
            Funds::Tusd(tusd) => pallet_canonical_pool::Pallet::<T>::do_swap_tusd_for_tao(
                &Self::hub_account(),
                tusd,
                TaoBalance::ZERO,
            ),
        }
    }

    /// The market of `netuid`, if its subnet is the one it was opened for.
    fn live_market(netuid: NetUid) -> Result<crate::Market, DispatchError> {
        let market = Markets::<T>::get(netuid).ok_or(Error::<T>::MarketUnknown)?;
        match T::Staking::subnet_status(netuid) {
            SubnetStatus::Live { generation } if generation == market.generation => Ok(market),
            SubnetStatus::Dissolving => Err(Error::<T>::MarketSettling.into()),
            _ => Err(Error::<T>::MarketClosed.into()),
        }
    }

    /// Stakes `tao` from the hub account in the vault of `netuid` and counts the shares it
    /// buys to the spoke of `bridge`. Returns the shares and the rate after.
    fn mint_shares(
        bridge: BridgeId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<(u64, u128), DispatchError> {
        let mut market = Self::live_market(netuid)?;
        ensure!(
            Tokens::<T>::contains_key(bridge, SpokeAsset::Alpha(netuid)),
            Error::<T>::TokenNotRegistered
        );
        let vault = Self::vault_account(netuid);
        let staked = T::Staking::staked(&vault, netuid).to_u64();
        Self::transfer_tao(&Self::hub_account(), &vault, tao)?;
        let alpha = T::Staking::stake(&vault, netuid, tao)?.to_u64();
        let shares = shares_for_alpha(alpha, staked, market.shares).ok_or(Error::<T>::Overflow)?;
        ensure!(shares != 0, Error::<T>::AmountTooLow);
        let staked = staked.checked_add(alpha).ok_or(Error::<T>::Overflow)?;
        ensure!(staked <= market.cap.to_u64(), Error::<T>::MarketCapExceeded);
        market.shares = market
            .shares
            .checked_add(shares)
            .ok_or(Error::<T>::Overflow)?;
        Markets::<T>::insert(netuid, market);
        AlphaSupply::<T>::mutate(netuid, bridge, |supply| {
            *supply = supply.saturating_add(shares)
        });
        Ok((shares, rate(staked, market.shares)))
    }

    /// Takes `shares` that the spoke of `bridge` burned out of the vault of `netuid` and
    /// brings what they are worth into the hub account as TAO.
    ///
    /// While the market's subnet lives, the vault unstakes the shares' alpha. Once the
    /// subnet is gone, its stake has been paid out to the vault account in TAO, and the
    /// shares divide that TAO among them.
    fn redeem_shares(
        bridge: BridgeId,
        netuid: NetUid,
        shares: u64,
    ) -> Result<TaoBalance, DispatchError> {
        let mut market = Markets::<T>::get(netuid).ok_or(Error::<T>::MarketUnknown)?;
        let supply = AlphaSupply::<T>::get(netuid, bridge)
            .checked_sub(shares)
            .ok_or(Error::<T>::Unbacked)?;
        let vault = Self::vault_account(netuid);
        let tao = match Self::live_market(netuid) {
            Ok(_) => {
                let staked = T::Staking::staked(&vault, netuid).to_u64();
                let alpha =
                    alpha_for_shares(shares, staked, market.shares).ok_or(Error::<T>::Overflow)?;
                ensure!(alpha != 0, Error::<T>::AmountTooLow);
                T::Staking::unstake(&vault, netuid, alpha.into())?
            }
            Err(error) if error == Error::<T>::MarketClosed.into() => {
                let pot = CurrencyOf::<T>::reducible_balance(
                    &vault,
                    Preservation::Expendable,
                    Fortitude::Polite,
                );
                multiply_by_rational_with_rounding(
                    u128::from(shares),
                    u128::from(pot.to_u64()),
                    u128::from(market.shares),
                    Rounding::Down,
                )
                .and_then(|tao| u64::try_from(tao).ok())
                .ok_or(Error::<T>::Overflow)?
                .into()
            }
            Err(error) => return Err(error),
        };
        market.shares = market.shares.saturating_sub(shares);
        Markets::<T>::insert(netuid, market);
        AlphaSupply::<T>::insert(netuid, bridge, supply);
        Self::transfer_tao(&vault, &Self::hub_account(), tao)?;
        Ok(tao)
    }

    fn transfer_tao(from: &T::AccountId, to: &T::AccountId, amount: TaoBalance) -> DispatchResult {
        CurrencyOf::<T>::transfer(from, to, amount, Preservation::Expendable).map(|_| ())
    }
}
