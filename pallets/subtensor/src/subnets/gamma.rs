//! Gamma: a subnet's service credit, minted by burning its alpha and valued in US dollars.
//!
//! Gamma is held in [`GammaDeposits`], one per `(netuid, hotkey)`, owned by a coldkey and
//! spent by its hotkey at services listed on the subnet; [`GammaSpent`] totals what each hotkey
//! has spent at each service. [`ColdkeyGammaHotkeys`] indexes a coldkey's deposits per subnet so
//! coldkey swaps move them in a bounded walk.

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
    pub(crate) fn credit_gamma(
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

    /// Refuses crediting another account's coldkey while it has a coldkey swap announced or
    /// disputed, or is reserved as a swap destination: deposits opened for it then could block
    /// the swap or change what it carries over.
    pub(crate) fn ensure_gamma_recipient(coldkey: &T::AccountId) -> DispatchResult {
        ensure!(
            !ColdkeySwapDisputes::<T>::contains_key(coldkey),
            Error::<T>::ColdkeySwapDisputed
        );
        ensure!(
            !ColdkeySwapAnnouncements::<T>::contains_key(coldkey),
            Error::<T>::ColdkeySwapAnnounced
        );
        Self::ensure_account_not_reserved_as_coldkey_swap_destination(coldkey)?;
        Ok(())
    }

    /// An on-behalf mint that opens another coldkey's deposit at `hotkey` credits at least
    /// [`crate::MIN_GAMMA_ON_BEHALF_OPEN`]; topping up an existing deposit takes any mint.
    pub(crate) fn ensure_gamma_on_behalf_open(
        netuid: NetUid,
        hotkey: &T::AccountId,
        gamma: GammaBalance,
    ) -> DispatchResult {
        ensure!(
            GammaDeposits::<T>::contains_key(netuid, hotkey)
                || gamma.to_u64() >= crate::MIN_GAMMA_ON_BEHALF_OPEN,
            Error::<T>::AmountTooLow
        );
        Ok(())
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

    /// Buys alpha with `amount` of the signer's TAO, burns it and credits its gamma to the
    /// deposit of `recipient`, or of the signer when `None`, at `hotkey`, which must have
    /// `daily_spend_cap` when it is given, or opens with it.
    pub fn do_mint_gamma(
        origin: OriginFor<T>,
        recipient: Option<T::AccountId>,
        hotkey: T::AccountId,
        netuid: NetUid,
        amount: TaoBalance,
        min_amount_out: GammaBalance,
        daily_spend_cap: Option<GammaBalance>,
    ) -> DispatchResult {
        let payer = ensure_signed(origin)?;
        let on_behalf = recipient
            .as_ref()
            .is_some_and(|recipient| *recipient != payer);
        if let Some(recipient) = recipient.as_ref().filter(|_| on_behalf) {
            Self::ensure_gamma_recipient(recipient)?;
        }
        let coldkey = recipient.unwrap_or_else(|| payer.clone());
        Self::ensure_gamma_mintable(netuid)?;
        ensure!(
            Self::can_remove_balance_from_coldkey_account(&payer, amount),
            Error::<T>::NotEnoughBalanceToStake
        );
        Self::ensure_add_stake_input_within_swap_limit(netuid, amount)?;
        let mark = Self::gamma_mark(netuid);
        let (tao, alpha) = Self::buy_and_burn_alpha(netuid, &payer, amount)?;
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        ensure!(gamma >= min_amount_out, Error::<T>::SlippageTooHigh);
        if on_behalf {
            Self::ensure_gamma_on_behalf_open(netuid, &hotkey, gamma)?;
        }
        let daily_spend_cap =
            Self::credit_gamma(netuid, &coldkey, &hotkey, gamma, daily_spend_cap)?;
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
    /// does, and credits its gamma to the deposit of `recipient`, or of the signer when `None`,
    /// at `destination_hotkey`, which must have `daily_spend_cap` when it is given, or opens
    /// with it.
    pub fn do_mint_gamma_with_stake(
        origin: OriginFor<T>,
        recipient: Option<T::AccountId>,
        origin_hotkey: T::AccountId,
        destination_hotkey: T::AccountId,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        min_amount_out: GammaBalance,
        daily_spend_cap: Option<GammaBalance>,
    ) -> DispatchResult {
        let signer = ensure_signed(origin.clone())?;
        let on_behalf = recipient
            .as_ref()
            .is_some_and(|recipient| *recipient != signer);
        if let Some(recipient) = recipient.as_ref().filter(|_| on_behalf) {
            Self::ensure_gamma_recipient(recipient)?;
        }
        let coldkey = recipient.unwrap_or(signer);
        Self::ensure_gamma_mintable(netuid)?;
        let mark = Self::gamma_mark(netuid);
        let alpha = Self::do_burn_alpha(origin, origin_hotkey, alpha_amount, netuid)?;
        let (value, gamma) = Self::gamma_for_alpha(alpha, mark)?;
        ensure!(gamma >= min_amount_out, Error::<T>::SlippageTooHigh);
        if on_behalf {
            Self::ensure_gamma_on_behalf_open(netuid, &destination_hotkey, gamma)?;
        }
        let daily_spend_cap = Self::credit_gamma(
            netuid,
            &coldkey,
            &destination_hotkey,
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
        let rest = deposit.balance.saturating_sub(amount);
        ensure!(
            !amount.is_zero()
                && (rest.is_zero()
                    || (amount.to_u64() >= crate::MIN_GAMMA_AMOUNT
                        && rest.to_u64() >= crate::MIN_GAMMA_AMOUNT)),
            Error::<T>::AmountTooLow
        );
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
