//! Gamma: a subnet's service credit, minted by burning its alpha and valued in US dollars.
//!
//! Gamma is held in [`GammaDeposits`], one per `(netuid, hotkey)`, owned by a coldkey and
//! spent by its hotkey at services listed on the subnet; [`GammaSpent`] totals what each hotkey
//! has spent at each service. [`ColdkeyGammaHotkeys`] indexes a coldkey's deposits per subnet so
//! coldkey swaps move them in a bounded walk. [`GammaBalances`] holds a coldkey's gamma on no
//! hotkey: it is sent between coldkeys and deposited at a hotkey to be spent.

use frame_support::storage::with_storage_layer;
use safe_math::*;
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
    /// Only `deposit.coldkey` sets the deposit's daily cap, moves its balance between API
    /// keys, and closes it when empty; only the hotkey spends it. The funding coldkey can
    /// revoke an API key's access by moving the balance away. Being the hotkey's staking
    /// owner grants no deposit administration.
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

    /// Refuses another coldkey opening a deposit for `coldkey` on `netuid` once its list there
    /// holds [`crate::MAX_THIRD_PARTY_GAMMA_DEPOSITS`], or while its coldkey swap is announced or
    /// disputed, or it is reserved as a swap destination. During a swap the coldkey cannot close
    /// deposits, and more of them could make the swap too heavy to execute.
    fn ensure_third_party_gamma_deposit_can_open(
        netuid: NetUid,
        coldkey: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            !ColdkeySwapDisputes::<T>::contains_key(coldkey),
            Error::<T>::ColdkeySwapDisputed
        );
        ensure!(
            !ColdkeySwapAnnouncements::<T>::contains_key(coldkey),
            Error::<T>::ColdkeySwapAnnounced
        );
        Self::ensure_account_not_reserved_as_coldkey_swap_destination(coldkey)?;
        ensure!(
            ColdkeyGammaHotkeys::<T>::decode_len(netuid, coldkey).unwrap_or_default()
                < crate::MAX_THIRD_PARTY_GAMMA_DEPOSITS as usize,
            Error::<T>::TooManyGammaDeposits
        );
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

    /// A daily spend cap is 0, which pauses the deposit, or at least [`crate::MIN_GAMMA_AMOUNT`],
    /// the least a spend that leaves a balance takes.
    fn ensure_gamma_cap(cap: Option<GammaBalance>) -> DispatchResult {
        ensure!(
            cap.is_none_or(|cap| cap.is_zero() || cap.to_u64() >= crate::MIN_GAMMA_AMOUNT),
            Error::<T>::AmountTooLow
        );
        Ok(())
    }

    /// `cap` with a full bucket, as a deposit opens with.
    pub(crate) fn full_gamma_cap(cap: Option<GammaBalance>) -> Option<GammaDailySpendCap> {
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

    /// Credits `gamma` to the coldkey's deposit at `hotkey`, which must have `daily_spend_cap`
    /// when it is given, or opens one with it, or no cap otherwise, and a full bucket. Returns the
    /// deposit's cap.
    fn credit_gamma(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        gamma: GammaBalance,
        daily_spend_cap: Option<GammaBalance>,
    ) -> Result<Option<GammaBalance>, DispatchError> {
        match GammaDeposits::<T>::get(netuid, hotkey) {
            Some(mut deposit) => {
                ensure!(
                    deposit.coldkey == *coldkey,
                    Error::<T>::NonAssociatedColdKey
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
                let cap = Self::gamma_cap(&deposit);
                GammaDeposits::<T>::insert(netuid, hotkey, deposit);
                Ok(cap)
            }
            None => {
                Self::ensure_gamma_cap(daily_spend_cap)?;
                Self::insert_gamma_deposit(
                    netuid,
                    hotkey,
                    GammaDeposit {
                        coldkey: coldkey.clone(),
                        balance: gamma,
                        daily_spend_cap: Self::full_gamma_cap(daily_spend_cap),
                    },
                )?;
                Ok(daily_spend_cap)
            }
        }
    }

    /// Credits minted `gamma` to the coldkey's deposit at `hotkey` as [`Self::credit_gamma`]
    /// does, or to its balance with no `hotkey`, which takes no daily spend cap. Returns the
    /// deposit's cap.
    fn credit_minted_gamma(
        netuid: NetUid,
        coldkey: &T::AccountId,
        hotkey: Option<&T::AccountId>,
        gamma: GammaBalance,
        daily_spend_cap: Option<GammaBalance>,
    ) -> Result<Option<GammaBalance>, DispatchError> {
        match hotkey {
            Some(hotkey) => Self::credit_gamma(netuid, coldkey, hotkey, gamma, daily_spend_cap),
            None => {
                ensure!(
                    daily_spend_cap.is_none(),
                    Error::<T>::GammaDepositDailySpendCapMismatch
                );
                Self::credit_gamma_balance(netuid, coldkey, gamma)?;
                Ok(None)
            }
        }
    }

    /// The AMM price limits of gamma emission trades on `netuid`: a sale floor
    /// [`crate::GAMMA_EMISSION_MAX_SLIPPAGE_BPS`] below the higher of its fast moving and spot
    /// prices, and a purchase ceiling as far above the lower.
    pub(crate) fn gamma_emission_price_limits(
        netuid: NetUid,
    ) -> Result<(TaoBalance, TaoBalance), DispatchError> {
        let fast = SubnetFastMovingPrice::<T>::get(netuid).ok_or(Error::<T>::SlippageTooHigh)?;
        let spot: U64F64 = T::SwapInterface::current_alpha_price(netuid.into());
        let limit = |price: U64F64, bps: u64| -> TaoBalance {
            price
                .saturating_mul(U64F64::saturating_from_num(bps))
                .safe_div(U64F64::saturating_from_num(10_000u64))
                .saturating_mul(U64F64::saturating_from_num(1_000_000_000u64))
                .saturating_to_num::<u64>()
                .into()
        };
        let bps = crate::GAMMA_EMISSION_MAX_SLIPPAGE_BPS;
        Ok((
            limit(fast.max(spot), 10_000u64.saturating_sub(bps)),
            limit(fast.min(spot), 10_000u64.saturating_add(bps)),
        ))
    }

    /// Allots the subnet's pending gamma emission to `shares`, its gamma shares, in proportion
    /// to their size. The rounding remainder stays pending.
    pub(crate) fn allot_gamma_emission(netuid: NetUid, shares: &[(NetUid, u64)]) {
        let pending = PendingGammaEmission::<T>::get(netuid).to_u64();
        let total = shares
            .iter()
            .fold(0u64, |total, (_, share)| total.saturating_add(*share));
        if pending == 0 || total == 0 {
            return;
        }
        let mut allotted = 0u64;
        for (target, share) in shares {
            let alpha = Self::mul_div_u64(pending, *share, total);
            UnconvertedGammaEmission::<T>::mutate(netuid, target, |unconverted| {
                *unconverted = unconverted.saturating_add(alpha.into());
            });
            allotted = allotted.saturating_add(alpha);
        }
        PendingGammaEmission::<T>::insert(
            netuid,
            AlphaBalance::from(pending.saturating_sub(allotted)),
        );
    }

    /// Allots the subnet's pending gamma emission at its epoch and converts each gamma share on
    /// its own. A share that cannot convert keeps its alpha for the next epoch.
    pub(crate) fn mint_gamma_emission(netuid: NetUid) {
        let shares = SubnetGammaEmission::<T>::get(netuid);
        Self::allot_gamma_emission(netuid, &shares);
        for (target, _) in shares {
            let _ = with_storage_layer(|| Self::mint_gamma_emission_share(netuid, target));
        }
    }

    /// Sells the alpha allotted to the subnet's share of `target`, within the sale floor, the
    /// swap input limit and as much as the target's pool takes within the purchase ceiling, buys
    /// and burns the target's alpha with all of the TAO and credits the gamma to the subnet
    /// owner's deposit at the subnet owner hotkey on the target. A deposit that this opens has
    /// no daily spend cap, so the hotkey can spend the gamma at once. An existing deposit keeps
    /// its cap, a pause included.
    pub(crate) fn mint_gamma_emission_share(netuid: NetUid, target: NetUid) -> DispatchResult {
        let unconverted = UnconvertedGammaEmission::<T>::get(netuid, target);
        if unconverted.is_zero() {
            return Ok(());
        }
        Self::ensure_subtoken_enabled(netuid)?;
        Self::ensure_gamma_mintable(target)?;
        // Paid like the owner cut; crediting fails while the owner coldkey does not own the hotkey.
        let coldkey = SubnetOwner::<T>::get(netuid);
        let hotkey = SubnetOwnerHotkey::<T>::get(netuid);
        let (floor, _) = Self::gamma_emission_price_limits(netuid)?;
        let (_, ceiling) = Self::gamma_emission_price_limits(target)?;
        // A sale returns at most the spot price per alpha, so its TAO fits under the ceiling. The
        // engine refuses a sale above its input limit.
        let room = U64F64::saturating_from_num(Self::get_max_amount_add(target, ceiling)?)
            .safe_div(T::SwapInterface::current_alpha_price(netuid.into()))
            .saturating_to_num::<u64>()
            .min(T::SwapInterface::max_swap_input::<GetTaoForAlpha<T>>(netuid).to_u64());
        let mark = Self::gamma_mark(target);
        let sale = Self::swap_alpha_for_tao(netuid, unconverted.min(room.into()), floor, false)?;
        let alpha_sold = sale.amount_paid_in.saturating_add(sale.fee_paid);
        let fee = Self::settle_alpha_fee_to_author(netuid, sale.fee_to_block_author)?;
        Self::record_tao_outflow(netuid, sale.amount_paid_out.saturating_add(fee));
        let account = Self::get_subnet_account_id(netuid).ok_or(Error::<T>::SubnetNotExists)?;
        let (tao, alpha) = Self::buy_and_burn_alpha(target, &account, sale.amount_paid_out)?;
        ensure!(tao == sale.amount_paid_out, Error::<T>::SlippageTooHigh);
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        // Names no cap: a new deposit opens without one, and an existing deposit keeps its own.
        Self::credit_gamma(target, &coldkey, &hotkey, gamma, None)?;
        let rest = unconverted.saturating_sub(alpha_sold);
        if rest.is_zero() {
            UnconvertedGammaEmission::<T>::remove(netuid, target);
        } else {
            UnconvertedGammaEmission::<T>::insert(netuid, target, rest);
        }
        Self::deposit_event(Event::GammaEmissionMinted {
            netuid,
            target,
            coldkey,
            hotkey,
            alpha_sold,
            tao,
            alpha,
            value,
            gamma,
        });
        Ok(())
    }

    /// Buys alpha with `amount` TAO, burns it and credits its gamma to the signer's deposit
    /// at `hotkey`, which must have `daily_spend_cap` when it is given, or opens with it, or to
    /// its balance with no `hotkey`.
    pub fn do_mint_gamma(
        origin: OriginFor<T>,
        hotkey: Option<T::AccountId>,
        netuid: NetUid,
        amount: TaoBalance,
        min_amount_out: GammaBalance,
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
        let daily_spend_cap =
            Self::credit_minted_gamma(netuid, &coldkey, hotkey.as_ref(), gamma, daily_spend_cap)?;
        Self::deposit_event(Event::GammaMinted {
            netuid,
            coldkey,
            hotkey,
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
    /// must have `daily_spend_cap` when it is given, or opens with it, or to its balance with
    /// no `destination_hotkey`.
    pub fn do_mint_gamma_with_stake(
        origin: OriginFor<T>,
        origin_hotkey: T::AccountId,
        destination_hotkey: Option<T::AccountId>,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        min_amount_out: GammaBalance,
        daily_spend_cap: Option<GammaBalance>,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin.clone())?;
        Self::ensure_gamma_mintable(netuid)?;
        let mark = Self::gamma_mark(netuid);
        let alpha = Self::do_burn_alpha(origin, origin_hotkey, alpha_amount, netuid)?;
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        ensure!(gamma >= min_amount_out, Error::<T>::SlippageTooHigh);
        let daily_spend_cap = Self::credit_minted_gamma(
            netuid,
            &coldkey,
            destination_hotkey.as_ref(),
            gamma,
            daily_spend_cap,
        )?;
        Self::deposit_event(Event::GammaMinted {
            netuid,
            coldkey,
            hotkey: destination_hotkey,
            daily_spend_cap,
            tao: TaoBalance::ZERO,
            alpha,
            value,
            gamma,
        });
        Ok(())
    }

    /// Moves up to `amount` between two of the signer's deposits: the whole balance, or at least
    /// [`crate::MIN_GAMMA_AMOUNT`] leaving at least as much, so a move makes no dust. Emptying
    /// the origin closes it; a destination it opens takes the origin's daily spend cap, with a
    /// full bucket.
    pub fn do_move_gamma(
        origin: OriginFor<T>,
        origin_hotkey: T::AccountId,
        destination_hotkey: T::AccountId,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        Self::move_coldkey_gamma(coldkey, origin_hotkey, destination_hotkey, netuid, amount)
    }

    fn move_coldkey_gamma(
        coldkey: T::AccountId,
        origin_hotkey: T::AccountId,
        destination_hotkey: T::AccountId,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        ensure!(
            origin_hotkey != destination_hotkey,
            Error::<T>::InvalidValue
        );
        let mut deposit = Self::own_gamma_deposit(netuid, &origin_hotkey, &coldkey)?;
        let amount = amount.min(deposit.balance);
        let rest = Self::ensure_gamma_move_amount(amount, deposit.balance)?;
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
        deposit.balance = rest;
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
        Self::ensure_gamma_cap(daily_spend_cap)?;
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

    /// Burns `amount` from the signer's deposit on `netuid` and adds it to what the signer has
    /// spent at `service`.
    pub fn do_spend_gamma(
        origin: OriginFor<T>,
        service: T::AccountId,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        let hotkey = ensure_signed(origin)?;
        let deposit = Self::gamma_deposit_after_spend(netuid, &hotkey, &service, amount)?;
        GammaDeposits::<T>::insert(netuid, &hotkey, deposit);
        GammaSpent::<T>::mutate((netuid, &service, &hotkey), |spent| {
            *spent = spent.saturating_add(amount)
        });
        Self::deposit_event(Event::GammaSpent {
            netuid,
            hotkey,
            service,
            amount,
        });
        Ok(())
    }

    /// The deposit at `hotkey` once it spends `amount` at `service` now, if it can: `service` has
    /// a service on the subnet, and the amount is at least [`crate::MIN_GAMMA_AMOUNT`] or the
    /// whole balance, and within the balance and what the daily spend cap allows.
    pub(crate) fn gamma_deposit_after_spend(
        netuid: NetUid,
        hotkey: &T::AccountId,
        service: &T::AccountId,
        amount: GammaBalance,
    ) -> Result<GammaDeposit<T::AccountId>, Error<T>> {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Services::<T>::contains_key(netuid, service),
            Error::<T>::ServiceDoesNotExist
        );
        let mut deposit =
            GammaDeposits::<T>::get(netuid, hotkey).ok_or(Error::<T>::GammaDepositDoesNotExist)?;
        ensure!(
            !amount.is_zero()
                && (amount.to_u64() >= crate::MIN_GAMMA_AMOUNT || amount == deposit.balance),
            Error::<T>::AmountTooLow
        );
        ensure!(
            amount <= deposit.balance,
            Error::<T>::InsufficientGammaBalance
        );
        let now = Self::get_current_block_as_u64();
        let allowance = Self::gamma_allowance(&deposit, now);
        ensure!(
            allowance.is_none_or(|allowance| amount <= allowance),
            Error::<T>::GammaDepositDailySpendCapExceeded
        );
        deposit.balance = deposit.balance.saturating_sub(amount);
        if let (Some(allowance), Some(cap)) = (allowance, deposit.daily_spend_cap.as_mut()) {
            cap.bucket = (allowance.saturating_sub(amount), now);
        }
        Ok(deposit)
    }

    /// What taking `amount` of `available` leaves. Refuses more than `available`, and an amount
    /// that is zero or leaves dust: it must be the whole of it, or at least
    /// [`crate::MIN_GAMMA_AMOUNT`] leaving at least as much.
    fn ensure_gamma_move_amount(
        amount: GammaBalance,
        available: GammaBalance,
    ) -> Result<GammaBalance, DispatchError> {
        ensure!(amount <= available, Error::<T>::InsufficientGammaBalance);
        let rest = available.saturating_sub(amount);
        ensure!(
            !amount.is_zero()
                && (rest.is_zero()
                    || (amount.to_u64() >= crate::MIN_GAMMA_AMOUNT
                        && rest.to_u64() >= crate::MIN_GAMMA_AMOUNT)),
            Error::<T>::AmountTooLow
        );
        Ok(rest)
    }

    /// Takes exactly `amount` from the coldkey's balance on `netuid`.
    fn debit_gamma_balance(
        netuid: NetUid,
        coldkey: &T::AccountId,
        amount: GammaBalance,
    ) -> DispatchResult {
        GammaBalances::<T>::try_mutate_exists(netuid, coldkey, |balance| {
            let rest = Self::ensure_gamma_move_amount(amount, balance.unwrap_or_default())?;
            *balance = (!rest.is_zero()).then_some(rest);
            Ok(())
        })
    }

    /// Adds `amount` to the coldkey's balance on `netuid`.
    fn credit_gamma_balance(
        netuid: NetUid,
        coldkey: &T::AccountId,
        amount: GammaBalance,
    ) -> DispatchResult {
        GammaBalances::<T>::try_mutate(netuid, coldkey, |balance| {
            *balance = balance
                .to_u64()
                .checked_add(amount.to_u64())
                .ok_or(ArithmeticError::Overflow)?
                .into();
            Ok(())
        })
    }

    /// Takes exactly `amount` from `deposit`, the coldkey's deposit at `hotkey`, closing it when
    /// emptied. Returns whether it closed.
    fn debit_gamma_deposit(
        netuid: NetUid,
        hotkey: &T::AccountId,
        mut deposit: GammaDeposit<T::AccountId>,
        amount: GammaBalance,
    ) -> Result<bool, DispatchError> {
        deposit.balance = Self::ensure_gamma_move_amount(amount, deposit.balance)?;
        let closes = deposit.balance.is_zero();
        if closes {
            Self::remove_gamma_deposit(netuid, hotkey, &deposit.coldkey);
        } else {
            GammaDeposits::<T>::insert(netuid, hotkey, deposit);
        }
        Ok(closes)
    }

    /// Moves up to `amount` from the signer's balance to its deposit at `hotkey`, under the
    /// amount rule of [`Self::do_move_gamma`], opening one with no daily spend cap if there is
    /// none.
    pub fn do_deposit_gamma(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        let amount = amount.min(GammaBalances::<T>::get(netuid, &coldkey));
        Self::debit_gamma_balance(netuid, &coldkey, amount)?;
        Self::credit_gamma(netuid, &coldkey, &hotkey, amount, None)?;
        Self::deposit_event(Event::GammaDeposited {
            netuid,
            coldkey,
            hotkey,
            amount,
        });
        Ok(())
    }

    /// Moves up to `amount` from the signer's deposit at `hotkey` to its balance, under the
    /// amount rule of [`Self::do_move_gamma`]. Emptying the deposit closes it.
    pub fn do_withdraw_gamma(
        origin: OriginFor<T>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        let deposit = Self::own_gamma_deposit(netuid, &hotkey, &coldkey)?;
        let amount = amount.min(deposit.balance);
        let closes = Self::debit_gamma_deposit(netuid, &hotkey, deposit, amount)?;
        Self::credit_gamma_balance(netuid, &coldkey, amount)?;
        Self::deposit_event(Event::GammaWithdrawn {
            netuid,
            coldkey: coldkey.clone(),
            hotkey: hotkey.clone(),
            amount,
        });
        if closes {
            Self::deposit_event(Event::GammaDepositClosed {
                netuid,
                coldkey,
                hotkey,
            });
        }
        Ok(())
    }

    /// Sends exactly `amount` from the signer's balance, or its deposit at `origin_hotkey`, to
    /// `destination_coldkey`'s balance, or its deposit at `destination_hotkey`, opening one with
    /// no daily spend cap if there is none. Emptying a source deposit closes it.
    pub fn do_transfer_gamma(
        origin: OriginFor<T>,
        destination_coldkey: T::AccountId,
        origin_hotkey: Option<T::AccountId>,
        destination_hotkey: Option<T::AccountId>,
        netuid: NetUid,
        amount: GammaBalance,
    ) -> DispatchResult {
        let coldkey = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            coldkey != destination_coldkey || origin_hotkey != destination_hotkey,
            Error::<T>::InvalidValue
        );
        ensure!(
            amount.to_u64() >= crate::MIN_GAMMA_AMOUNT,
            Error::<T>::AmountTooLow
        );
        if let Some(hotkey) = &destination_hotkey {
            match GammaDeposits::<T>::get(netuid, hotkey) {
                Some(deposit) => ensure!(
                    deposit.coldkey == destination_coldkey,
                    Error::<T>::NonAssociatedColdKey
                ),
                None if coldkey != destination_coldkey => {
                    Self::ensure_third_party_gamma_deposit_can_open(netuid, &destination_coldkey)?
                }
                None => {}
            }
        }
        let closes = match &origin_hotkey {
            Some(hotkey) => {
                let deposit = Self::own_gamma_deposit(netuid, hotkey, &coldkey)?;
                Self::debit_gamma_deposit(netuid, hotkey, deposit, amount)?
            }
            None => {
                Self::debit_gamma_balance(netuid, &coldkey, amount)?;
                false
            }
        };
        match &destination_hotkey {
            Some(hotkey) => {
                Self::credit_gamma(netuid, &destination_coldkey, hotkey, amount, None)?;
            }
            None => Self::credit_gamma_balance(netuid, &destination_coldkey, amount)?,
        }
        Self::deposit_event(Event::GammaTransferred {
            netuid,
            origin_coldkey: coldkey.clone(),
            destination_coldkey,
            origin_hotkey: origin_hotkey.clone(),
            destination_hotkey,
            amount,
        });
        if let (true, Some(hotkey)) = (closes, origin_hotkey) {
            Self::deposit_event(Event::GammaDepositClosed {
                netuid,
                coldkey,
                hotkey,
            });
        }
        Ok(())
    }

    /// Takes spending away from `old_hotkey` in a hotkey swap: the coldkey's deposit there moves
    /// to `new_hotkey` as a move of its whole balance, or pauses when `new_hotkey` holds another
    /// coldkey's deposit. What `old_hotkey` spent stays in [`GammaSpent`]. Returns whether it
    /// changed a deposit.
    pub(crate) fn swap_hotkey_gamma_deposit(
        netuid: NetUid,
        old_hotkey: &T::AccountId,
        new_hotkey: &T::AccountId,
        coldkey: &T::AccountId,
    ) -> Result<bool, DispatchError> {
        let Some(mut deposit) = GammaDeposits::<T>::get(netuid, old_hotkey)
            .filter(|deposit| deposit.coldkey == *coldkey && !deposit.balance.is_zero())
        else {
            return Ok(false);
        };
        if GammaDeposits::<T>::get(netuid, new_hotkey)
            .is_none_or(|destination| destination.coldkey == *coldkey)
        {
            Self::move_coldkey_gamma(
                coldkey.clone(),
                old_hotkey.clone(),
                new_hotkey.clone(),
                netuid,
                deposit.balance,
            )?;
        } else {
            deposit.daily_spend_cap = Self::full_gamma_cap(Some(GammaBalance::ZERO));
            GammaDeposits::<T>::insert(netuid, old_hotkey, deposit);
            Self::deposit_event(Event::GammaDepositDailySpendCapSet {
                netuid,
                coldkey: coldkey.clone(),
                hotkey: old_hotkey.clone(),
                daily_spend_cap: Some(GammaBalance::ZERO),
            });
        }
        Ok(true)
    }

    /// Moves the old coldkey's gamma balance and deposits on `netuid` to the new coldkey, terms
    /// unchanged; fails if the new coldkey would hold more than [`crate::MAX_GAMMA_DEPOSITS`] there.
    pub(crate) fn transfer_coldkey_gamma_deposits(
        netuid: NetUid,
        old_coldkey: &T::AccountId,
        new_coldkey: &T::AccountId,
    ) -> DispatchResult {
        let balance = GammaBalances::<T>::take(netuid, old_coldkey);
        if !balance.is_zero() {
            Self::credit_gamma_balance(netuid, new_coldkey, balance)?;
        }
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

    /// Whether `coldkey` holds a gamma balance on any subnet.
    pub(crate) fn coldkey_has_gamma_balance(coldkey: &T::AccountId) -> bool {
        Self::get_all_subnet_netuids()
            .into_iter()
            .any(|netuid| !GammaBalances::<T>::get(netuid, coldkey).is_zero())
    }

    /// Whether `coldkey` holds gamma on any subnet, in a deposit or as a balance.
    #[cfg(test)]
    pub(crate) fn coldkey_holds_gamma(coldkey: &T::AccountId) -> bool {
        Self::coldkey_gamma_deposit_count(coldkey) != 0 || Self::coldkey_has_gamma_balance(coldkey)
    }
}
