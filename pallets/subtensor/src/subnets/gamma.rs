//! Gamma: a subnet's service credit, minted by burning its alpha and valued in US dollars.
//!
//! Gamma is held in [`GammaDeposits`], one per `(netuid, hotkey)`, owned by a coldkey and
//! spent by its spender: the subnet owner (lease beneficiary on a leased subnet) when the
//! deposit opens, unless the coldkey sets another. [`ColdkeyGammaHotkeys`] indexes a coldkey's
//! deposits per subnet so coldkey swaps move them in a bounded walk.

use sp_runtime::ArithmeticError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, GammaBalance, NetUid, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::*;

impl<T: Config> Pallet<T> {
    /// Requires an existing subnet with its subtoken enabled, and gamma enabled both
    /// network-wide and on the subnet.
    pub(crate) fn ensure_gamma_mintable(netuid: NetUid) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        Self::ensure_subtoken_enabled(netuid)?;
        ensure!(
            GammaEnabled::<T>::get() && SubnetGammaEnabled::<T>::get(netuid),
            Error::<T>::GammaDisabled
        );
        Ok(())
    }

    /// The signer's own deposit at `hotkey` on an existing subnet.
    fn own_gamma_deposit(
        netuid: NetUid,
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Result<GammaDeposit<T::AccountId>, DispatchError> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let deposit =
            GammaDeposits::<T>::get(netuid, hotkey).ok_or(Error::<T>::GammaDepositDoesNotExist)?;
        ensure!(
            deposit.coldkey == *coldkey,
            Error::<T>::NonAssociatedColdKey
        );
        Ok(deposit)
    }

    /// Opens a deposit and lists its hotkey under its coldkey.
    fn insert_gamma_deposit(
        netuid: NetUid,
        hotkey: &T::AccountId,
        deposit: GammaDeposit<T::AccountId>,
    ) -> DispatchResult {
        ColdkeyGammaHotkeys::<T>::try_mutate(netuid, &deposit.coldkey, |hotkeys| {
            hotkeys
                .try_push(hotkey.clone())
                .map_err(|_| Error::<T>::TooManyGammaDeposits)
        })?;
        GammaDeposits::<T>::insert(netuid, hotkey, deposit);
        Ok(())
    }

    /// Closes a deposit and drops its hotkey from its coldkey's list.
    fn remove_gamma_deposit(netuid: NetUid, hotkey: &T::AccountId, coldkey: &T::AccountId) {
        GammaDeposits::<T>::remove(netuid, hotkey);
        ColdkeyGammaHotkeys::<T>::mutate_exists(netuid, coldkey, |hotkeys| {
            if let Some(list) = hotkeys {
                list.retain(|listed| listed != hotkey);
                if list.is_empty() {
                    *hotkeys = None;
                }
            }
        });
    }

    /// The spender a deposit opens with: the beneficiary of a subnet its lease still owns,
    /// otherwise the subnet owner.
    pub(crate) fn gamma_default_spender(netuid: NetUid) -> T::AccountId {
        let owner = SubnetOwner::<T>::get(netuid);
        SubnetUidToLeaseId::<T>::get(netuid)
            .and_then(SubnetLeases::<T>::get)
            .filter(|lease| lease.coldkey == owner)
            .map_or(owner, |lease| lease.beneficiary)
    }

    /// What a deposit's daily spend cap allows at `now`: its bucket refilled by the cap
    /// every `GAMMA_REFILL_BLOCKS`, up to the cap. `None` when the deposit has no cap.
    pub(crate) fn gamma_allowance(
        deposit: &GammaDeposit<T::AccountId>,
        now: u64,
    ) -> Option<GammaBalance> {
        let cap = deposit.daily_spend_cap?.to_u64();
        let (available, block) = deposit.bucket;
        let refill = Self::mul_div_u64(cap, now.saturating_sub(block), crate::GAMMA_REFILL_BLOCKS);
        Some(available.to_u64().saturating_add(refill).min(cap).into())
    }

    /// The price a mint values alpha at, read before its trade: the lower of the gamma price and
    /// the spot price.
    pub(crate) fn gamma_mark(netuid: NetUid) -> U64F64 {
        let spot: U64F64 = T::SwapInterface::current_alpha_price(netuid.into());
        SubnetGammaPrice::<T>::get(netuid).min(spot)
    }

    /// The TAO value and gamma of `alpha` burned at `mark`, at the mint TAO/USD price.
    pub(crate) fn gamma_for_alpha(
        alpha: AlphaBalance,
        mark: U64F64,
    ) -> Result<(TaoBalance, GammaBalance), DispatchError> {
        let value = mark
            .saturating_mul(U64F64::saturating_from_num(alpha.to_u64()))
            .saturating_to_num::<u64>();
        let price = Self::mint_tao_usd_price(Self::get_current_block_as_u64())?;
        let gamma = Self::mul_div_u64(value, price, 1_000_000_000);
        ensure!(gamma >= crate::MIN_GAMMA_AMOUNT, Error::<T>::AmountTooLow);
        Ok((value.into(), gamma.into()))
    }

    /// Buys alpha with `amount` of the coldkey's TAO, as staking does without a stake
    /// position, and burns it. Returns the TAO taken and the alpha burned.
    fn buy_and_burn_alpha(
        netuid: NetUid,
        coldkey: &T::AccountId,
        amount: TaoBalance,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
        let tao_in = Self::transfer_tao_to_subnet(netuid, coldkey, amount)?;
        let swap = Self::swap_tao_for_alpha(netuid, tao_in, T::SwapInterface::max_price(), false)?;
        let alpha: AlphaBalance = swap.amount_paid_out;
        Self::settle_tao_fee_to_author(netuid, swap.fee_to_block_author)?;
        let refund = tao_in.saturating_sub(swap.amount_paid_in.saturating_add(swap.fee_paid));
        if !refund.is_zero() {
            Self::transfer_tao_from_subnet(netuid, coldkey, refund)?;
        }
        Self::record_tao_inflow(netuid, swap.amount_paid_in);
        Self::burn_subnet_alpha(netuid, alpha);
        Ok((tao_in.saturating_sub(refund), alpha))
    }

    /// Credits `gamma` to the coldkey's deposit at `hotkey`, opening one with the default
    /// spender and no daily spend cap if there is none, and returns the deposit's spender.
    fn credit_gamma(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        gamma: GammaBalance,
    ) -> Result<T::AccountId, DispatchError> {
        match GammaDeposits::<T>::get(netuid, hotkey) {
            Some(mut deposit) => {
                ensure!(
                    deposit.coldkey == *coldkey,
                    Error::<T>::NonAssociatedColdKey
                );
                deposit.balance = deposit
                    .balance
                    .to_u64()
                    .checked_add(gamma.to_u64())
                    .ok_or(ArithmeticError::Overflow)?
                    .into();
                let spender = deposit.spender.clone();
                GammaDeposits::<T>::insert(netuid, hotkey, deposit);
                Ok(spender)
            }
            None => {
                let spender = Self::gamma_default_spender(netuid);
                Self::insert_gamma_deposit(
                    netuid,
                    hotkey,
                    GammaDeposit {
                        coldkey: coldkey.clone(),
                        spender: spender.clone(),
                        balance: gamma,
                        daily_spend_cap: None,
                        bucket: (GammaBalance::ZERO, Self::get_current_block_as_u64()),
                    },
                )?;
                Ok(spender)
            }
        }
    }

    /// Buys alpha with `amount` TAO, burns it and credits its gamma to the signer's deposit
    /// at `hotkey`.
    pub fn do_mint_gamma(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
        min_amount_out: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        Self::ensure_gamma_mintable(netuid)?;
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&coldkey, amount),
            Error::<T>::NotEnoughBalanceToStake
        );
        Self::ensure_add_stake_input_within_swap_limit(netuid, amount)?;
        let mark = Self::gamma_mark(netuid);
        let (tao, alpha) = Self::buy_and_burn_alpha(netuid, &coldkey, amount)?;
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        ensure!(gamma >= min_amount_out, Error::<T>::SlippageTooHigh);
        let spender = Self::credit_gamma(netuid, &coldkey, &hotkey, gamma)?;
        Self::deposit_event(Event::GammaMinted {
            netuid,
            coldkey,
            hotkey,
            spender,
            tao,
            alpha,
            value,
            gamma,
        });
        Ok(())
    }

    /// Burns up to `alpha_amount` of the signer's stake on `origin_hotkey`, as `burn_alpha`
    /// does, and credits its gamma to the signer's deposit at `destination_hotkey`.
    pub fn do_mint_gamma_with_stake(
        origin: OriginFor<T>,
        origin_hotkey: T::AccountId,
        destination_hotkey: T::AccountId,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        min_amount_out: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin.clone())?;
        Self::ensure_gamma_mintable(netuid)?;
        let mark = Self::gamma_mark(netuid);
        let alpha = Self::do_burn_alpha(origin, origin_hotkey, alpha_amount, netuid)?;
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        ensure!(gamma >= min_amount_out, Error::<T>::SlippageTooHigh);
        let spender = Self::credit_gamma(netuid, &coldkey, &destination_hotkey, gamma)?;
        Self::deposit_event(Event::GammaMinted {
            netuid,
            coldkey,
            hotkey: destination_hotkey,
            spender,
            tao: TaoBalance::ZERO,
            alpha,
            value,
            gamma,
        });
        Ok(())
    }

    /// Moves up to `amount` between two of the signer's hotkeys. Emptying the origin closes
    /// it; a destination it opens takes the origin's spender and daily spend cap, with a
    /// full bucket.
    pub fn do_move_gamma(
        origin: OriginFor<T>,
        origin_hotkey: T::AccountId,
        destination_hotkey: T::AccountId,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(
            origin_hotkey != destination_hotkey,
            Error::<T>::InvalidValue
        );
        let mut deposit = Self::own_gamma_deposit(netuid, &origin_hotkey, &coldkey)?;
        let amount = amount.min(deposit.balance);
        ensure!(!amount.is_zero(), Error::<T>::AmountTooLow);
        let destination = match GammaDeposits::<T>::get(netuid, &destination_hotkey) {
            Some(mut destination) => {
                ensure!(
                    destination.coldkey == coldkey,
                    Error::<T>::NonAssociatedColdKey
                );
                destination.balance = destination
                    .balance
                    .to_u64()
                    .checked_add(amount.to_u64())
                    .ok_or(ArithmeticError::Overflow)?
                    .into();
                Some(destination)
            }
            None => None,
        };
        deposit.balance = deposit.balance.saturating_sub(amount);
        let closes = deposit.balance.is_zero();
        if closes {
            Self::remove_gamma_deposit(netuid, &origin_hotkey, &coldkey);
        } else {
            GammaDeposits::<T>::insert(netuid, &origin_hotkey, &deposit);
        }
        match destination {
            Some(destination) => {
                GammaDeposits::<T>::insert(netuid, &destination_hotkey, destination)
            }
            None => Self::insert_gamma_deposit(
                netuid,
                &destination_hotkey,
                GammaDeposit {
                    coldkey: coldkey.clone(),
                    spender: deposit.spender,
                    balance: amount,
                    daily_spend_cap: deposit.daily_spend_cap,
                    bucket: (
                        deposit.daily_spend_cap.unwrap_or_default(),
                        Self::get_current_block_as_u64(),
                    ),
                },
            )?,
        }
        Self::deposit_event(Event::GammaMoved {
            netuid,
            coldkey: coldkey.clone(),
            origin_hotkey: origin_hotkey.clone(),
            destination_hotkey,
            amount,
        });
        if closes {
            Self::deposit_event(Event::GammaDepositClosed {
                netuid,
                coldkey,
                hotkey: origin_hotkey,
            });
        }
        Ok(())
    }

    /// Closes the signer's empty deposit, releasing the hotkey.
    pub fn do_close_gamma_deposit(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        let deposit = Self::own_gamma_deposit(netuid, &hotkey, &coldkey)?;
        ensure!(deposit.balance.is_zero(), Error::<T>::GammaDepositNotEmpty);
        Self::remove_gamma_deposit(netuid, &hotkey, &coldkey);
        Self::deposit_event(Event::GammaDepositClosed {
            netuid,
            coldkey,
            hotkey,
        });
        Ok(())
    }

    /// Sets the deposit's spender.
    pub fn do_set_gamma_deposit_spender(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        spender: T::AccountId,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        let mut deposit = Self::own_gamma_deposit(netuid, &hotkey, &coldkey)?;
        deposit.spender = spender.clone();
        GammaDeposits::<T>::insert(netuid, &hotkey, deposit);
        Self::deposit_event(Event::GammaDepositSpenderSet {
            netuid,
            coldkey,
            hotkey,
            spender,
        });
        Ok(())
    }

    /// Sets the deposit's daily spend cap. A new cap starts with a full bucket, raising it
    /// adds the increase to the bucket, lowering it clamps the bucket, and `None` removes it.
    pub fn do_set_gamma_deposit_daily_spend_cap(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        daily_spend_cap: Option<GammaBalance>,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        let mut deposit = Self::own_gamma_deposit(netuid, &hotkey, &coldkey)?;
        let now = Self::get_current_block_as_u64();
        let available = match (Self::gamma_allowance(&deposit, now), daily_spend_cap) {
            (Some(allowance), Some(cap)) => allowance
                .saturating_add(cap.saturating_sub(deposit.daily_spend_cap.unwrap_or_default()))
                .min(cap),
            _ => daily_spend_cap.unwrap_or_default(),
        };
        deposit.daily_spend_cap = daily_spend_cap;
        deposit.bucket = (available, now);
        GammaDeposits::<T>::insert(netuid, &hotkey, deposit);
        Self::deposit_event(Event::GammaDepositDailySpendCapSet {
            netuid,
            coldkey,
            hotkey,
            daily_spend_cap,
        });
        Ok(())
    }

    /// Burns, in order, each entry's amount from the deposits the signer spends, within each
    /// deposit's balance and daily spend cap. Entries the signer cannot spend, or that spend
    /// nothing, are skipped.
    pub fn do_spend_gamma(
        origin: OriginFor<T>,
        netuid: NetUid,
        spends: &[(T::AccountId, GammaBalance)],
    ) -> DispatchResult {
        let signer = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(!spends.is_empty(), Error::<T>::InvalidValue);
        let now = Self::get_current_block_as_u64();
        for (hotkey, amount) in spends {
            let Some(mut deposit) = GammaDeposits::<T>::get(netuid, hotkey) else {
                continue;
            };
            if deposit.spender != signer {
                continue;
            }
            let allowance = Self::gamma_allowance(&deposit, now);
            let spent = (*amount)
                .min(deposit.balance)
                .min(allowance.unwrap_or(deposit.balance));
            if spent.is_zero() {
                continue;
            }
            deposit.balance = deposit.balance.saturating_sub(spent);
            if let Some(allowance) = allowance {
                deposit.bucket = (allowance.saturating_sub(spent), now);
            }
            let coldkey = deposit.coldkey.clone();
            GammaDeposits::<T>::insert(netuid, hotkey, deposit);
            Self::deposit_event(Event::GammaSpent {
                netuid,
                coldkey,
                hotkey: hotkey.clone(),
                amount: spent,
            });
        }
        Ok(())
    }

    /// Moves the old coldkey's deposits on `netuid` to the new coldkey, terms unchanged; fails
    /// if the new coldkey would hold more than [`crate::MAX_GAMMA_DEPOSITS`] there.
    pub(crate) fn transfer_coldkey_gamma_deposits(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        let hotkeys = ColdkeyGammaHotkeys::<T>::get(netuid, old_coldkey);
        if hotkeys.is_empty() {
            return Ok(());
        }
        ColdkeyGammaHotkeys::<T>::remove(netuid, old_coldkey);
        let mut merged = ColdkeyGammaHotkeys::<T>::get(netuid, new_coldkey);
        for hotkey in hotkeys {
            GammaDeposits::<T>::mutate(netuid, &hotkey, |deposit| {
                if let Some(deposit) = deposit {
                    deposit.coldkey = new_coldkey.clone();
                }
            });
            merged
                .try_push(hotkey)
                .map_err(|_| Error::<T>::TooManyGammaDeposits)?;
        }
        ColdkeyGammaHotkeys::<T>::insert(netuid, new_coldkey, merged);
        Ok(())
    }

    /// The number of gamma deposits `coldkey` holds across subnets.
    pub(crate) fn coldkey_gamma_deposit_count(coldkey: &T::AccountId) -> u32 {
        Self::get_all_subnet_netuids()
            .into_iter()
            .map(|netuid| ColdkeyGammaHotkeys::<T>::decode_len(netuid, coldkey).unwrap_or(0) as u32)
            .fold(0, u32::saturating_add)
    }
}
