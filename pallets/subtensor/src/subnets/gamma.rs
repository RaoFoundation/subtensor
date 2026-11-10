//! Gamma: a subnet's service credit, minted by burning its alpha and valued in US dollars.
//!
//! Gamma is held in [`GammaDeposits`], one per `(netuid, hotkey)`, owned by a coldkey and
//! spent by its spender: the spender of the subnet owner's service, otherwise the subnet owner
//! (lease beneficiary while its lease owns the subnet), when the deposit opens, unless the
//! coldkey sets another. [`ColdkeyGammaHotkeys`] indexes a
//! coldkey's deposits per subnet so coldkey swaps move them in a bounded walk.

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

    /// Opens a deposit and indexes it under the coldkey that funds and controls it.
    ///
    /// An unassociated hotkey can be used as an API credential without registering
    /// a staking owner. If it already has an owner, only that coldkey can open the
    /// deposit, preventing another funder from reserving an owned hotkey's slot.
    /// This also protects subnet-owner hotkeys used to receive gamma emission.
    ///
    /// Only `deposit.coldkey` controls the deposit's spender and daily cap, moves its
    /// balance between API keys, and closes it when empty. The funding coldkey can
    /// revoke an API key's access by moving the balance away. Neither holding that
    /// hotkey's private key nor being its staking owner grants deposit administration
    /// or on-chain spending authority; spending requires the named spender (or its
    /// authorized proxy). Service request authorization remains off-chain.
    ///
    /// There is one deposit per `(netuid, hotkey)`: the first opener occupies that
    /// slot until it is closed. Existing-deposit paths must still enforce
    /// `deposit.coldkey`; staking ownership does not grant control over a deposit
    /// that was opened before the hotkey became associated.
    fn insert_gamma_deposit(
        netuid: NetUid,
        hotkey: &T::AccountId,
        deposit: GammaDeposit<T::AccountId>,
    ) -> DispatchResult {
        ensure!(
            Owner::<T>::try_get(hotkey).map_or(true, |owner| owner == deposit.coldkey),
            Error::<T>::NonAssociatedColdKey
        );
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

    /// The spender a deposit opens with: the spender of the service of the subnet owner's
    /// hotkey while the owner owns it, otherwise the beneficiary of a subnet its lease still
    /// owns, otherwise the subnet owner.
    pub(crate) fn gamma_default_spender(netuid: NetUid) -> T::AccountId {
        let owner = SubnetOwner::<T>::get(netuid);
        let hotkey = SubnetOwnerHotkey::<T>::get(netuid);
        if Owner::<T>::get(&hotkey) == owner
            && let Some(service) = Services::<T>::get(netuid, &hotkey)
        {
            return service.spender;
        }
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
        let GammaDailySpendCap { cap, bucket } = deposit.daily_spend_cap?;
        let (available, block) = bucket;
        let cap = cap.to_u64();
        let refill = Self::mul_div_u64(cap, now.saturating_sub(block), crate::GAMMA_REFILL_BLOCKS);
        Some(available.to_u64().saturating_add(refill).min(cap).into())
    }

    /// A deposit's daily spend cap.
    pub(crate) fn gamma_cap(deposit: &GammaDeposit<T::AccountId>) -> Option<GammaBalance> {
        deposit.daily_spend_cap.map(|cap| cap.cap)
    }

    /// `cap` with a full bucket, as a deposit opens with.
    fn full_gamma_cap(cap: Option<GammaBalance>) -> Option<GammaDailySpendCap> {
        let now = Self::get_current_block_as_u64();
        cap.map(|cap| GammaDailySpendCap {
            cap,
            bucket: (cap, now),
        })
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

    /// Credits `gamma` to the coldkey's deposit at `hotkey`, which must name `spender` and
    /// have `daily_spend_cap` when they are given, or opens one with them, the default spender
    /// and no cap otherwise, and a full bucket. Returns the deposit's spender and cap.
    fn credit_gamma(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        gamma: GammaBalance,
        spender: Option<T::AccountId>,
        daily_spend_cap: Option<GammaBalance>,
    ) -> Result<(T::AccountId, Option<GammaBalance>), DispatchError> {
        match GammaDeposits::<T>::get(netuid, hotkey) {
            Some(mut deposit) => {
                ensure!(
                    deposit.coldkey == *coldkey,
                    Error::<T>::NonAssociatedColdKey
                );
                ensure!(
                    spender.is_none_or(|spender| spender == deposit.spender),
                    Error::<T>::GammaDepositSpenderMismatch
                );
                ensure!(
                    daily_spend_cap.is_none_or(|cap| Self::gamma_cap(&deposit) == Some(cap)),
                    Error::<T>::GammaDepositDailySpendCapMismatch
                );
                deposit.balance = deposit
                    .balance
                    .to_u64()
                    .checked_add(gamma.to_u64())
                    .ok_or(ArithmeticError::Overflow)?
                    .into();
                let terms = (deposit.spender.clone(), Self::gamma_cap(&deposit));
                GammaDeposits::<T>::insert(netuid, hotkey, deposit);
                Ok(terms)
            }
            None => {
                let spender = spender.unwrap_or_else(|| Self::gamma_default_spender(netuid));
                Self::insert_gamma_deposit(
                    netuid,
                    hotkey,
                    GammaDeposit {
                        coldkey: coldkey.clone(),
                        spender: spender.clone(),
                        balance: gamma,
                        daily_spend_cap: Self::full_gamma_cap(daily_spend_cap),
                    },
                )?;
                Ok((spender, daily_spend_cap))
            }
        }
    }

    /// Buys alpha with `amount` TAO, burns it and credits its gamma to the signer's deposit
    /// at `hotkey`, which must name `spender` and have `daily_spend_cap` when they are given, or
    /// opens with them.
    pub fn do_mint_gamma(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
        min_amount_out: GammaBalance,
        spender: Option<T::AccountId>,
        daily_spend_cap: Option<GammaBalance>,
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
        let (spender, daily_spend_cap) =
            Self::credit_gamma(netuid, &coldkey, &hotkey, gamma, spender, daily_spend_cap)?;
        Self::deposit_event(Event::GammaMinted {
            netuid,
            coldkey,
            hotkey,
            spender,
            daily_spend_cap,
            tao,
            alpha,
            value,
            gamma,
        });
        Ok(())
    }

    /// Burns up to `alpha_amount` of the signer's stake on `origin_hotkey`, as `burn_alpha`
    /// does, and credits its gamma to the signer's deposit at `destination_hotkey`, which
    /// must name `spender` and have `daily_spend_cap` when they are given, or opens with them.
    pub fn do_mint_gamma_with_stake(
        origin: OriginFor<T>,
        origin_hotkey: T::AccountId,
        destination_hotkey: T::AccountId,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        min_amount_out: GammaBalance,
        spender: Option<T::AccountId>,
        daily_spend_cap: Option<GammaBalance>,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin.clone())?;
        Self::ensure_gamma_mintable(netuid)?;
        let mark = Self::gamma_mark(netuid);
        let alpha = Self::do_burn_alpha(origin, origin_hotkey, alpha_amount, netuid)?;
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        ensure!(gamma >= min_amount_out, Error::<T>::SlippageTooHigh);
        let (spender, daily_spend_cap) = Self::credit_gamma(
            netuid,
            &coldkey,
            &destination_hotkey,
            gamma,
            spender,
            daily_spend_cap,
        )?;
        Self::deposit_event(Event::GammaMinted {
            netuid,
            coldkey,
            hotkey: destination_hotkey,
            spender,
            daily_spend_cap,
            tao: TaoBalance::ZERO,
            alpha,
            value,
            gamma,
        });
        Ok(())
    }

    /// Moves up to `amount` between two of the signer's deposits. Emptying the origin closes
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
                    daily_spend_cap: Self::full_gamma_cap(
                        deposit.daily_spend_cap.map(|cap| cap.cap),
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

    /// Sets the deposit's spender, or the default spender when `spender` is `None`.
    pub fn do_set_gamma_deposit_spender(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        spender: Option<T::AccountId>,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        let mut deposit = Self::own_gamma_deposit(netuid, &hotkey, &coldkey)?;
        let spender = spender.unwrap_or_else(|| Self::gamma_default_spender(netuid));
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

    /// Sets the deposit's daily spend cap, or removes it when `daily_spend_cap` is `None`. A cap
    /// set where there was none starts with a full bucket, and a changed cap keeps what the
    /// bucket allows now, up to the new cap.
    pub fn do_set_gamma_deposit_daily_spend_cap(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        daily_spend_cap: Option<GammaBalance>,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        let mut deposit = Self::own_gamma_deposit(netuid, &hotkey, &coldkey)?;
        let now = Self::get_current_block_as_u64();
        let allowance = Self::gamma_allowance(&deposit, now);
        deposit.daily_spend_cap = daily_spend_cap.map(|cap| GammaDailySpendCap {
            cap,
            bucket: (allowance.map_or(cap, |allowance| allowance.min(cap)), now),
        });
        GammaDeposits::<T>::insert(netuid, &hotkey, deposit);
        Self::deposit_event(Event::GammaDepositDailySpendCapSet {
            netuid,
            coldkey,
            hotkey,
            daily_spend_cap,
        });
        Ok(())
    }

    /// Burns, in order, each entry's amount from the deposits the signer spends. Entries the
    /// signer cannot spend, or whose amount is zero or above the deposit's balance or what its
    /// daily spend cap allows, are skipped with the reason. Returns how many entries spent.
    pub fn do_spend_gamma(
        origin: OriginFor<T>,
        netuid: NetUid,
        spends: &[(T::AccountId, GammaBalance, [u8; 32])],
    ) -> Result<u32, DispatchError> {
        let signer = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(!spends.is_empty(), Error::<T>::InvalidValue);
        let now = Self::get_current_block_as_u64();
        let mut spent: u32 = 0;
        for (hotkey, amount, reference) in spends {
            match Self::spend_gamma_entry(netuid, &signer, hotkey, *amount, now) {
                Ok(coldkey) => {
                    Self::deposit_event(Event::GammaSpent {
                        netuid,
                        coldkey,
                        hotkey: hotkey.clone(),
                        spender: signer.clone(),
                        amount: *amount,
                        reference: *reference,
                    });
                    spent = spent.saturating_add(1);
                }
                Err(reason) => Self::deposit_event(Event::GammaSpendSkipped {
                    netuid,
                    hotkey: hotkey.clone(),
                    spender: signer.clone(),
                    amount: *amount,
                    reference: *reference,
                    reason: reason.into(),
                }),
            }
        }
        Ok(spent)
    }

    /// Burns `amount` from the deposit at `hotkey` if `spender` can spend all of it now, and
    /// returns the deposit's coldkey; otherwise the error says why the entry is skipped.
    fn spend_gamma_entry(
        netuid: NetUid,
        spender: &T::AccountId,
        hotkey: &T::AccountId,
        amount: GammaBalance,
        now: u64,
    ) -> Result<T::AccountId, Error<T>> {
        let mut deposit =
            GammaDeposits::<T>::get(netuid, hotkey).ok_or(Error::<T>::GammaDepositDoesNotExist)?;
        ensure!(
            deposit.spender == *spender,
            Error::<T>::NotGammaDepositSpender
        );
        ensure!(!amount.is_zero(), Error::<T>::AmountTooLow);
        ensure!(
            amount <= deposit.balance,
            Error::<T>::InsufficientGammaBalance
        );
        let allowance = Self::gamma_allowance(&deposit, now);
        ensure!(
            allowance.is_none_or(|allowance| amount <= allowance),
            Error::<T>::GammaDepositDailySpendCapExceeded
        );
        deposit.balance = deposit.balance.saturating_sub(amount);
        if let (Some(allowance), Some(cap)) = (allowance, deposit.daily_spend_cap.as_mut()) {
            cap.bucket = (allowance.saturating_sub(amount), now);
        }
        let coldkey = deposit.coldkey.clone();
        GammaDeposits::<T>::insert(netuid, hotkey, deposit);
        Ok(coldkey)
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
