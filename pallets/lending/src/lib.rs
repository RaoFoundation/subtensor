//! Fixed-principal loans funded exclusively by the AMM's extracted reserve floors.
//!
//! Shorts sell borrowed alpha into custodial TAO proceeds. Longs transfer borrowed TAO
//! to their owner. Both lock collateral, pay a fixed annual opening-value coupon,
//! and have no price-triggered liquidation. Both coupons burn TAO; long coupons first
//! sell alpha through the guarded AMM. Exhausted collateral forfeits the position.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
pub use pallet::*;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{PalletId, pallet_prelude::*, transactional, weights::WeightMeter};
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_runtime::traits::{AccountIdConversion, Hash, SaturatedConversion, Saturating};
use substrate_fixed::{
    transcendental::{exp, ln},
    types::{I64F64, U64F64},
};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::OrderSwapInterface;
use weights::WeightInfo;

/// Full, fee-inclusive quotes and subnet lifecycle access. Implementations must reject
/// partial fills, preserve alpha issuance, and make every asset-moving method transactional.
pub trait LendingPoolInterface<AccountId>: OrderSwapInterface<AccountId> {
    fn subnet_exists(netuid: NetUid) -> bool;
    fn owner_allowed(_owner: &AccountId) -> bool {
        true
    }
    fn quote_sell(netuid: NetUid, alpha: AlphaBalance) -> Result<TaoBalance, DispatchError>;
    fn quote_buy(netuid: NetUid, tao: TaoBalance) -> Result<AlphaBalance, DispatchError>;
    /// An upper bound on the alpha claims sharing a funded deregistration payout.
    /// Must fail while aggregate staking counters are incomplete.
    fn redemption_alpha_supply(netuid: NetUid) -> Result<u128, DispatchError>;
    /// A conservative executable gross TAO input, including the finite curve boundary.
    fn max_buy_input(_netuid: NetUid) -> TaoBalance {
        u64::MAX.into()
    }
    /// TAO this custody account can spend under the AMM's account-preservation policy.
    fn buy_spendable_tao(_account: &AccountId) -> TaoBalance {
        u64::MAX.into()
    }
    /// Terminal refunds must not strand a dissolution on a reaped recipient's dust.
    /// Return the amount actually credited; only sub-existential dust may be recycled.
    fn refund_dissolution_tao(
        from: &AccountId,
        to: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }
    /// Withdraw both legs as one credit before resolving recipients, so reaping the
    /// sender cannot destroy a small second leg between transfers.
    fn terminal_split_tao(
        from: &AccountId,
        vault: &AccountId,
        recipient: &AccountId,
        recovery: TaoBalance,
        refund: TaoBalance,
    ) -> Result<(TaoBalance, TaoBalance), DispatchError> {
        let recovered = if recovery.is_zero() {
            recovery
        } else {
            Self::refund_dissolution_tao(from, vault, recovery)?
        };
        let refunded = if refund.is_zero() {
            refund
        } else {
            Self::refund_dissolution_tao(from, recipient, refund)?
        };
        Ok((recovered, refunded))
    }
    /// Credit an assessed coupon, reporting any explicit account-creation dust loss.
    fn collect_interest_tao(
        from: &AccountId,
        vault: &AccountId,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::transfer_tao(from, vault, amount)?;
        Ok(amount)
    }
    /// Burn exactly the collected fee using the chain's canonical TAO burn account.
    /// Failure must preserve both balances, including unrelated custody principal.
    fn burn_interest_tao(account: &AccountId, amount: TaoBalance) -> DispatchResult;
    /// Repay the fixed alpha principal without applying unrelated spot-valued
    /// minimum-transfer rules. Source ownership, available stake and locks still apply.
    fn repay_alpha(
        from: &AccountId,
        source_hotkey: &AccountId,
        vault: &AccountId,
        custody_hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        Self::transfer_staked_alpha(
            from,
            source_hotkey,
            vault,
            custody_hotkey,
            netuid,
            amount,
            true,
            false,
        )
    }
    /// Move only lending custody alpha at its frozen cutoff after subnet removal.
    fn transfer_dissolution_alpha(
        from: &AccountId,
        to: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        Self::transfer_staked_alpha(from, hotkey, to, hotkey, netuid, amount, false, false)
    }
    /// Return uncommitted lending inventory to the dissolution pot before its denominator
    /// is fixed. Collected fees remain separate. No swap is permitted here.
    fn return_dissolution_reserves(
        netuid: NetUid,
        account: &AccountId,
        hotkey: &AccountId,
        tao: TaoBalance,
        alpha: AlphaBalance,
    ) -> DispatchResult;
}

/// Subtensor's migration and dissolution integration. The no-op implementation keeps
/// independent Subtensor mocks and runtimes that do not include lending compatible.
pub trait LendingInterface<AccountId> {
    fn custody_accounts(netuid: NetUid) -> Option<(AccountId, AccountId)>;
    fn has_vault(netuid: NetUid) -> bool;
    fn has_funding_capacity() -> bool;
    fn has_positions(owner: &AccountId) -> bool;
    fn has_hotkey_positions(hotkey: &AccountId) -> bool;
    fn max_positions() -> u32;
    fn on_hotkey_swap(old: &AccountId, new: &AccountId, netuid: Option<NetUid>) -> DispatchResult;
    fn fund_reserves(
        netuid: NetUid,
        tao: TaoBalance,
        alpha: AlphaBalance,
        reference: U64F64,
        has_history: bool,
    ) -> DispatchResult;
    fn start_dissolution(netuid: NetUid) -> DispatchResult;
    fn settle_shorts(netuid: NetUid, meter: &mut WeightMeter) -> bool;
    fn on_alpha_redemption(
        netuid: NetUid,
        coldkey: &AccountId,
        tao_paid: TaoBalance,
    ) -> DispatchResult;
    fn settle_remaining_longs(netuid: NetUid, meter: &mut WeightMeter) -> bool;
    fn finish_dissolution(netuid: NetUid) -> DispatchResult;
}

impl<AccountId> LendingInterface<AccountId> for () {
    fn custody_accounts(_: NetUid) -> Option<(AccountId, AccountId)> {
        None
    }
    fn has_vault(_: NetUid) -> bool {
        false
    }
    fn has_funding_capacity() -> bool {
        false
    }
    fn has_positions(_: &AccountId) -> bool {
        false
    }
    fn has_hotkey_positions(_: &AccountId) -> bool {
        false
    }
    fn max_positions() -> u32 {
        0
    }
    fn on_hotkey_swap(_: &AccountId, _: &AccountId, _: Option<NetUid>) -> DispatchResult {
        Ok(())
    }
    fn fund_reserves(
        _: NetUid,
        _: TaoBalance,
        _: AlphaBalance,
        _: U64F64,
        _: bool,
    ) -> DispatchResult {
        Ok(())
    }
    fn start_dissolution(_: NetUid) -> DispatchResult {
        Ok(())
    }
    fn settle_shorts(_: NetUid, _: &mut WeightMeter) -> bool {
        true
    }
    fn on_alpha_redemption(_: NetUid, _: &AccountId, _: TaoBalance) -> DispatchResult {
        Ok(())
    }
    fn settle_remaining_longs(_: NetUid, _: &mut WeightMeter) -> bool {
        true
    }
    fn finish_dissolution(_: NetUid) -> DispatchResult {
        Ok(())
    }
}

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
)]
pub enum Side {
    Short,
    Long,
}

#[freeze_struct("400b840d2e2a9837")]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Default,
    Debug,
    PartialEq,
    Eq,
)]
pub struct Vault {
    pub available_tao: u64,
    pub available_alpha: u64,
    pub outstanding_tao: u64,
    pub outstanding_alpha: u64,
    pub lost_tao: u64,
    pub lost_alpha: u64,
    pub pending_tao: u64,
    pub pending_alpha: u64,
}

#[freeze_struct("5845ea2961d09261")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct Position<AccountId, BlockNumber> {
    pub side: Side,
    pub hotkey: AccountId,
    /// Fixed debt in the borrowed token: alpha for Short, TAO for Long.
    pub principal: u64,
    /// Remaining collateral in TAO for Short, alpha for Long.
    pub collateral: u64,
    /// Short sale proceeds; long funded TAO receipts accumulated only during dissolution.
    pub proceeds: u64,
    /// 100% nominal annual interest, fixed in opening collateral units.
    pub annual_interest: u64,
    pub last_accrued: BlockNumber,
    /// Fractional collateral atoms, numerator modulo BlocksPerYear.
    pub interest_remainder: u64,
    pub due: BlockNumber,
}

#[freeze_struct("78138cda5aaf128")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct LendingReference<BlockNumber> {
    pub price: U64F64,
    pub last_updated: BlockNumber,
    pub valid_after: BlockNumber,
}

#[freeze_struct("3a89334a6e24f568")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct Dissolution<BlockNumber> {
    pub price: U64F64,
    pub frozen_at: BlockNumber,
    pub reserves_returned: bool,
}

#[freeze_struct("c9a092f7f943650f")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct OpeningQuote {
    pub principal: u64,
    pub annual_interest: u64,
    pub opening_value: u64,
}

#[freeze_struct("68b43b0f1faa9c7a")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, Debug, PartialEq, Eq,
)]
pub struct ClosingQuote {
    pub payment: u64,
    pub refund: u64,
}

/// Geometric EMA update weight: 1 - 2^(-1 / 7200), a 24-hour half-life at 12s blocks.
const EMA_WEIGHT_BITS: u128 = 1_775_790_721_272_497;
/// Bound internal orders while allowing input-reserve guards to expand after each buy.
pub const MAX_BUYBACK_STEPS: u32 = 6;

#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type Pool: LendingPoolInterface<Self::AccountId>;
        #[pallet::constant]
        type PalletId: Get<PalletId>;
        #[pallet::constant]
        type MinimumLoanValue: Get<u64>;
        #[pallet::constant]
        type InterestPeriod: Get<BlockNumberFor<Self>>;
        #[pallet::constant]
        type BlocksPerYear: Get<u64>;
        #[pallet::constant]
        type ReferenceWarmup: Get<BlockNumberFor<Self>>;
        #[pallet::constant]
        type MaxPositionsPerSubnet: Get<u32>;
        #[pallet::constant]
        type MaxTotalPositions: Get<u32>;
        #[pallet::constant]
        type MaxFundedSubnets: Get<u32>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::storage]
    pub type Enabled<T> = StorageValue<_, bool, ValueQuery>;
    #[pallet::storage]
    pub type Vaults<T> = StorageMap<_, Identity, NetUid, Vault, OptionQuery>;
    #[pallet::storage]
    pub type VaultCount<T> = StorageValue<_, u32, ValueQuery>;
    #[pallet::storage]
    pub type Positions<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Identity,
        NetUid,
        Position<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;
    #[pallet::storage]
    pub type OpenByNetuid<T: Config> =
        StorageDoubleMap<_, Identity, NetUid, Blake2_128Concat, T::AccountId, (), OptionQuery>;
    #[pallet::storage]
    pub type PositionCount<T> = StorageMap<_, Identity, NetUid, u32, ValueQuery>;
    #[pallet::storage]
    pub type TotalPositions<T> = StorageValue<_, u32, ValueQuery>;
    #[pallet::storage]
    pub type LoanHotkeys<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;
    #[pallet::storage]
    pub type EscrowOwner<T: Config> = StorageDoubleMap<
        _,
        Identity,
        NetUid,
        Blake2_128Concat,
        T::AccountId,
        T::AccountId,
        OptionQuery,
    >;
    #[pallet::storage]
    pub type References<T: Config> =
        StorageMap<_, Identity, NetUid, LendingReference<BlockNumberFor<T>>, OptionQuery>;
    #[pallet::storage]
    pub type Dissolutions<T: Config> =
        StorageMap<_, Identity, NetUid, Dissolution<BlockNumberFor<T>>, OptionQuery>;
    #[pallet::storage]
    pub type DissolutionCursor<T: Config> =
        StorageMap<_, Identity, NetUid, T::AccountId, OptionQuery>;
    #[pallet::storage]
    pub type Due<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        BlockNumberFor<T>,
        Blake2_128Concat,
        (T::AccountId, NetUid),
        (),
        OptionQuery,
    >;
    #[pallet::storage]
    pub type NextDue<T: Config> = StorageValue<_, BlockNumberFor<T>, OptionQuery>;
    #[pallet::storage]
    pub type ConversionCursor<T> = StorageValue<_, NetUid, OptionQuery>;
    #[pallet::storage]
    pub type PalletHotkey<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        Opened {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal: u64,
            collateral: u64,
            proceeds: u64,
            annual_interest: u64,
        },
        Closed {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            refund: u64,
        },
        InterestCollected {
            owner: T::AccountId,
            netuid: NetUid,
            collateral_paid: u64,
        },
        Forfeited {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_lost: u64,
        },
        ReservesFunded {
            netuid: NetUid,
            tao: u64,
            alpha: u64,
        },
        /// Retained for event codec compatibility; new coupons emit InterestBurned.
        InterestConverted {
            netuid: NetUid,
            side: Side,
            input: u64,
            output: u64,
        },
        DissolutionFrozen {
            netuid: NetUid,
            price: U64F64,
            frozen_at: BlockNumberFor<T>,
        },
        DissolutionSettled {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal_recovered: u64,
            principal_lost: u64,
            tao_refund: u64,
        },
        EnabledSet {
            enabled: bool,
        },
        DustForfeited {
            netuid: NetUid,
            recipient: T::AccountId,
            tao: u64,
        },
        InterestBurned {
            netuid: NetUid,
            side: Side,
            tao: u64,
        },
        /// Additional collateral, debt, proceeds and coupon on an existing position.
        Increased {
            owner: T::AccountId,
            netuid: NetUid,
            side: Side,
            principal: u64,
            collateral: u64,
            proceeds: u64,
            annual_interest: u64,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        Disabled,
        SubnetUnavailable,
        ReferenceUnavailable,
        ReferenceWarmingUp,
        PositionExists,
        PositionMissing,
        AmountTooSmall,
        InsufficientReserves,
        BorrowingLimit,
        TooManyPositions,
        TooManySubnets,
        BelowMinimumBorrow,
        AboveMaximumPayment,
        BelowMinimumRefund,
        InsufficientEscrow,
        InvalidQuote,
        Arithmetic,
        CustodyUnavailable,
        AlreadyDissolving,
        BelowMinimumProceeds,
        RedemptionUnavailable,
        InsufficientRedemptionBacking,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            assert!(T::BlocksPerYear::get() > 0);
            assert!(T::InterestPeriod::get() > BlockNumberFor::<T>::default());
            assert!(T::MinimumLoanValue::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() > 0);
            assert!(T::MaxPositionsPerSubnet::get() <= T::MaxTotalPositions::get());
            assert!(T::MaxFundedSubnets::get() > 0);
        }
        fn on_initialize(_: BlockNumberFor<T>) -> Weight {
            // Reserve finalize work in advance; the migration enforces MaxFundedSubnets.
            T::WeightInfo::update_reference()
                .saturating_mul(u64::from(VaultCount::<T>::get()))
                .saturating_add(T::DbWeight::get().reads(2))
        }

        fn on_finalize(now: BlockNumberFor<T>) {
            for (netuid, _) in Vaults::<T>::iter() {
                if Dissolutions::<T>::contains_key(netuid) {
                    continue;
                }
                References::<T>::mutate(netuid, |reference| {
                    if let Some(reference) = reference {
                        if reference.last_updated >= now {
                            return;
                        }
                        let spot = T::Pool::current_alpha_price(netuid);
                        if let Some(next) = Self::ema_update(reference.price, spot) {
                            reference.price = next;
                            reference.last_updated = now;
                        }
                    }
                });
            }
        }

        fn on_idle(now: BlockNumberFor<T>, remaining: Weight) -> Weight {
            let mut meter = WeightMeter::with_limit(remaining);
            Self::collect_due(now, &mut meter);
            Self::convert_pending(now, &mut meter);
            meter.consumed()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Open or increase one loan on a subnet, using the same side and collateral hotkey.
        /// Collateral and caller bounds apply to the additional loan, not existing totals.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::open())]
        #[transactional]
        pub fn open(
            origin: OriginFor<T>,
            netuid: NetUid,
            side: Side,
            collateral: u64,
            hotkey: T::AccountId,
            min_borrow: u64,
            min_proceeds: u64,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            ensure!(Enabled::<T>::get(), Error::<T>::Disabled);
            ensure!(
                T::Pool::owner_allowed(&owner),
                Error::<T>::SubnetUnavailable
            );
            let existing = Positions::<T>::get(&owner, netuid);
            if let Some(position) = &existing {
                Self::ensure_matching_position(position, side, &hotkey)?;
            } else {
                ensure!(
                    PositionCount::<T>::get(netuid) < T::MaxPositionsPerSubnet::get(),
                    Error::<T>::TooManyPositions
                );
                ensure!(
                    TotalPositions::<T>::get() < T::MaxTotalPositions::get(),
                    Error::<T>::TooManyPositions
                );
            }
            let quote = Self::quote_open_inner(
                netuid,
                side,
                collateral,
                existing.as_ref().map(|position| (&owner, position)),
            )?;
            ensure!(
                quote.principal >= min_borrow,
                Error::<T>::BelowMinimumBorrow
            );
            if side == Side::Short {
                ensure!(
                    quote.opening_value >= min_proceeds,
                    Error::<T>::BelowMinimumProceeds
                );
            }
            let now = frame_system::Pallet::<T>::block_number();
            let increasing = existing.is_some();
            let mut position = if let Some(mut position) = existing {
                // Settle only whole atoms and retain fractional interest. Increasing
                // a loan cannot erase accrued fees or postpone its scheduled collection.
                Self::charge_interest(&owner, netuid, &mut position, now, false)?;
                position
            } else {
                Position {
                    side,
                    hotkey: hotkey.clone(),
                    principal: 0,
                    collateral: 0,
                    proceeds: 0,
                    annual_interest: 0,
                    last_accrued: now,
                    interest_remainder: 0,
                    due: now.saturating_add(T::InterestPeriod::get()),
                }
            };
            let vault = Self::reserve_account(netuid);
            let escrow = Self::position_account(&owner, netuid);
            let custody = Self::custody_hotkey()?;
            let proceeds = match side {
                Side::Short => {
                    T::Pool::transfer_tao(&owner, &escrow, collateral.into())?;
                    T::Pool::transfer_staked_alpha(
                        &vault,
                        &custody,
                        &escrow,
                        &custody,
                        netuid,
                        quote.principal.into(),
                        false,
                        false,
                    )?;
                    let received = T::Pool::sell_alpha(
                        &escrow,
                        &custody,
                        netuid,
                        quote.principal.into(),
                        TaoBalance::ZERO,
                        false,
                    )?
                    .to_u64();
                    ensure!(received == quote.opening_value, Error::<T>::InvalidQuote);
                    ensure!(received >= min_proceeds, Error::<T>::BelowMinimumProceeds);
                    received
                }
                Side::Long => {
                    T::Pool::transfer_staked_alpha(
                        &owner,
                        &hotkey,
                        &escrow,
                        &custody,
                        netuid,
                        collateral.into(),
                        true,
                        false,
                    )?;
                    T::Pool::transfer_tao(&vault, &owner, quote.principal.into())?;
                    0
                }
            };
            Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
                let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
                match side {
                    Side::Short => {
                        vault.available_alpha = Self::sub(vault.available_alpha, quote.principal)?;
                        vault.outstanding_alpha =
                            Self::add(vault.outstanding_alpha, quote.principal)?;
                    }
                    Side::Long => {
                        vault.available_tao = Self::sub(vault.available_tao, quote.principal)?;
                        vault.outstanding_tao = Self::add(vault.outstanding_tao, quote.principal)?;
                    }
                }
                Ok(())
            })?;
            position.principal = Self::add(position.principal, quote.principal)?;
            position.collateral = Self::add(position.collateral, collateral)?;
            position.proceeds = Self::add(position.proceeds, proceeds)?;
            position.annual_interest = Self::add(position.annual_interest, quote.annual_interest)?;
            if !increasing {
                let due = position.due;
                LoanHotkeys::<T>::mutate(&hotkey, |count| *count = count.saturating_add(1));
                OpenByNetuid::<T>::insert(netuid, &owner, ());
                EscrowOwner::<T>::insert(netuid, &escrow, &owner);
                PositionCount::<T>::mutate(netuid, |n| *n = n.saturating_add(1));
                TotalPositions::<T>::mutate(|n| *n = n.saturating_add(1));
                Due::<T>::insert(due, (&owner, netuid), ());
                NextDue::<T>::mutate(|next| {
                    if next.is_none_or(|existing| existing > due) {
                        *next = Some(due);
                    }
                });
            }
            Positions::<T>::insert(&owner, netuid, position);
            Self::deposit_event(if increasing {
                Event::Increased {
                    owner,
                    netuid,
                    side,
                    principal: quote.principal,
                    collateral,
                    proceeds,
                    annual_interest: quote.annual_interest,
                }
            } else {
                Event::Opened {
                    owner,
                    netuid,
                    side,
                    principal: quote.principal,
                    collateral,
                    proceeds,
                    annual_interest: quote.annual_interest,
                }
            });
            Ok(())
        }

        /// Repay the fixed principal and release remaining collateral. A short may buy back
        /// from escrow or supply alpha from its opening hotkey; a long supplies free TAO.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::close())]
        #[transactional]
        pub fn close(
            origin: OriginFor<T>,
            netuid: NetUid,
            repay_from_wallet: bool,
            max_payment: u64,
            min_refund: u64,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            ensure!(
                !Dissolutions::<T>::contains_key(netuid),
                Error::<T>::SubnetUnavailable
            );
            let mut position =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            let now = frame_system::Pallet::<T>::block_number();
            Self::charge_interest(&owner, netuid, &mut position, now, true)?;
            ensure!(position.collateral > 0, Error::<T>::InsufficientEscrow);
            let escrow = Self::position_account(&owner, netuid);
            let vault = Self::reserve_account(netuid);
            let custody = Self::custody_hotkey()?;
            let refund = match position.side {
                Side::Short => {
                    let pot = Self::add(position.collateral, position.proceeds)?;
                    if repay_from_wallet {
                        ensure!(
                            position.principal <= max_payment,
                            Error::<T>::AboveMaximumPayment
                        );
                        T::Pool::repay_alpha(
                            &owner,
                            &position.hotkey,
                            &vault,
                            &custody,
                            netuid,
                            position.principal.into(),
                        )?;
                        Self::credit_principal(
                            netuid,
                            Side::Short,
                            position.principal,
                            position.principal,
                        )?;
                        ensure!(pot >= min_refund, Error::<T>::BelowMinimumRefund);
                        T::Pool::transfer_tao(&escrow, &owner, pot.into())?;
                        pot
                    } else {
                        let (payment, alpha) = Self::execute_buyback(
                            &escrow,
                            netuid,
                            position.principal,
                            pot.min(max_payment),
                        )?;
                        ensure!(payment <= max_payment, Error::<T>::AboveMaximumPayment);
                        let refund = Self::sub(pot, payment)?;
                        ensure!(refund >= min_refund, Error::<T>::BelowMinimumRefund);
                        ensure!(alpha >= position.principal, Error::<T>::InvalidQuote);
                        T::Pool::transfer_staked_alpha(
                            &escrow,
                            &custody,
                            &vault,
                            &custody,
                            netuid,
                            alpha.into(),
                            false,
                            false,
                        )?;
                        Self::credit_principal(netuid, Side::Short, position.principal, alpha)?;
                        T::Pool::transfer_tao(&escrow, &owner, refund.into())?;
                        refund
                    }
                }
                Side::Long => {
                    ensure!(
                        position.principal <= max_payment,
                        Error::<T>::AboveMaximumPayment
                    );
                    ensure!(
                        position.collateral >= min_refund,
                        Error::<T>::BelowMinimumRefund
                    );
                    T::Pool::transfer_tao(&owner, &vault, position.principal.into())?;
                    T::Pool::transfer_staked_alpha(
                        &escrow,
                        &custody,
                        &owner,
                        &position.hotkey,
                        netuid,
                        position.collateral.into(),
                        false,
                        true,
                    )?;
                    Self::credit_principal(
                        netuid,
                        Side::Long,
                        position.principal,
                        position.principal,
                    )?;
                    position.collateral
                }
            };
            Self::remove_position(&owner, netuid, &position);
            Self::deposit_event(Event::Closed {
                owner,
                netuid,
                side: position.side,
                refund,
            });
            Ok(())
        }

        /// Pause new loans. Repayment, collection and terminal settlement remain available.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::set_enabled())]
        pub fn set_enabled(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;
            Enabled::<T>::put(enabled);
            Self::deposit_event(Event::EnabledSet { enabled });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    pub fn recovery_account() -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating(b"recovery")
    }
    pub fn reserve_account(netuid: NetUid) -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating((b"vault", netuid))
    }

    pub fn position_account(owner: &T::AccountId, netuid: NetUid) -> T::AccountId {
        // Hash first: truncating a raw owner tuple can silently discard its unique suffix.
        let key = T::Hashing::hash_of(&(b"position", owner, netuid));
        T::PalletId::get().into_sub_account_truncating(key)
    }

    pub fn initialize_custody() -> DispatchResult {
        if PalletHotkey::<T>::exists() {
            return Ok(());
        }
        let coldkey: T::AccountId = T::PalletId::get().into_account_truncating();
        let key =
            T::Hashing::hash_of(&(b"lending/hotkey", frame_system::Pallet::<T>::parent_hash()));
        let hotkey = T::PalletId::get().into_sub_account_truncating(key);
        T::Pool::register_pallet_hotkey(&coldkey, &hotkey)?;
        ensure!(
            T::Pool::pallet_hotkey_registered(&coldkey, &hotkey),
            Error::<T>::CustodyUnavailable
        );
        PalletHotkey::<T>::put(hotkey);
        Ok(())
    }

    pub fn custody_hotkey() -> Result<T::AccountId, DispatchError> {
        PalletHotkey::<T>::get().ok_or(Error::<T>::CustodyUnavailable.into())
    }

    /// Record assets already transferred to the vault by the pool migration adapter.
    /// Re-funding an existing vault never resets its historical reference.
    #[transactional]
    pub fn fund_reserves(
        netuid: NetUid,
        tao: TaoBalance,
        alpha: AlphaBalance,
        initial_reference: U64F64,
        has_history: bool,
    ) -> DispatchResult {
        ensure!(
            initial_reference.to_bits() > 0,
            Error::<T>::ReferenceUnavailable
        );
        Self::initialize_custody()?;
        if !Vaults::<T>::contains_key(netuid) {
            ensure!(
                VaultCount::<T>::get() < T::MaxFundedSubnets::get(),
                Error::<T>::TooManySubnets
            );
            VaultCount::<T>::mutate(|count| *count = count.saturating_add(1));
            let now = frame_system::Pallet::<T>::block_number();
            let valid_after = if has_history {
                now
            } else {
                now.saturating_add(T::ReferenceWarmup::get())
            };
            References::<T>::insert(
                netuid,
                LendingReference {
                    price: initial_reference,
                    last_updated: now,
                    valid_after,
                },
            );
        }
        Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
            let vault = vault.get_or_insert_with(Vault::default);
            vault.available_tao = Self::add(vault.available_tao, tao.to_u64())?;
            vault.available_alpha = Self::add(vault.available_alpha, alpha.to_u64())?;
            Ok(())
        })?;
        Self::deposit_event(Event::ReservesFunded {
            netuid,
            tao: tao.to_u64(),
            alpha: alpha.to_u64(),
        });
        Ok(())
    }

    pub fn quote_open(
        netuid: NetUid,
        side: Side,
        collateral: u64,
    ) -> Result<OpeningQuote, DispatchError> {
        Self::quote_open_inner(netuid, side, collateral, None)
    }

    /// Quote additional debt and its coupon for the same path used by `open`.
    /// The ownerless quote remains available for a genuinely new position.
    pub fn quote_open_for(
        owner: &T::AccountId,
        netuid: NetUid,
        side: Side,
        collateral: u64,
        hotkey: &T::AccountId,
    ) -> Result<OpeningQuote, DispatchError> {
        let existing = Positions::<T>::get(owner, netuid);
        if let Some(position) = &existing {
            Self::ensure_matching_position(position, side, hotkey)?;
        }
        Self::quote_open_inner(
            netuid,
            side,
            collateral,
            existing.as_ref().map(|position| (owner, position)),
        )
    }

    fn ensure_matching_position(
        position: &Position<T::AccountId, BlockNumberFor<T>>,
        side: Side,
        hotkey: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            position.side == side && position.hotkey == *hotkey,
            Error::<T>::PositionExists
        );
        Ok(())
    }

    fn quote_open_inner(
        netuid: NetUid,
        side: Side,
        collateral: u64,
        existing: Option<(&T::AccountId, &Position<T::AccountId, BlockNumberFor<T>>)>,
    ) -> Result<OpeningQuote, DispatchError> {
        ensure!(
            T::Pool::subnet_exists(netuid) && !Dissolutions::<T>::contains_key(netuid),
            Error::<T>::SubnetUnavailable
        );
        ensure!(collateral > 0, Error::<T>::AmountTooSmall);
        let reference = References::<T>::get(netuid).ok_or(Error::<T>::ReferenceUnavailable)?;
        ensure!(
            frame_system::Pallet::<T>::block_number() >= reference.valid_after,
            Error::<T>::ReferenceWarmingUp
        );
        ensure!(
            reference.price.to_bits() > 0,
            Error::<T>::ReferenceUnavailable
        );
        let (combined_collateral, old_principal) = if let Some((_, position)) = existing {
            let now = frame_system::Pallet::<T>::block_number();
            let (interest, remainder) = Self::interest_due(position, now, false)?;
            // Reserve a whole atom for the preserved fractional coupon. A loan
            // whose old collateral is exhausted cannot be revived by discarding fees.
            let accrued = Self::add(interest, u64::from(remainder > 0))?;
            let remaining = position
                .collateral
                .checked_sub(accrued)
                .filter(|remaining| *remaining > 0)
                .ok_or(Error::<T>::InsufficientEscrow)?;
            // Actual custody retains the fractional atom until it is charged;
            // validate that addition too, even though valuation reserves the atom.
            Self::add(Self::sub(position.collateral, interest)?, collateral)?;
            (Self::add(remaining, collateral)?, position.principal)
        } else {
            (collateral, 0)
        };
        let quarter = combined_collateral
            .checked_div(4)
            .ok_or(Error::<T>::Arithmetic)?;
        ensure!(quarter > 0, Error::<T>::AmountTooSmall);
        let vault = Vaults::<T>::get(netuid).ok_or(Error::<T>::InsufficientReserves)?;
        let quote = match side {
            Side::Short => {
                let limit = Self::alpha_for_tao(quarter, reference.price, false)?
                    .checked_sub(old_principal)
                    .ok_or(Error::<T>::InsufficientEscrow)?;
                // Historical marking limits debt; the real opening sell independently limits
                // executable exposure. Binary search is bounded by the u64 input domain.
                let mut low = 0_u64;
                let mut high = limit;
                while low < high {
                    let mid = low.saturating_add(
                        high.saturating_sub(low)
                            .saturating_add(1)
                            .checked_div(2)
                            .unwrap_or_default(),
                    );
                    let combined = Self::add(old_principal, mid)?;
                    let fits = T::Pool::quote_sell(netuid, combined.into())
                        .is_ok_and(|out| out.to_u64() <= quarter);
                    if fits {
                        low = mid;
                    } else {
                        high = mid.saturating_sub(1);
                    }
                }
                ensure!(low > 0, Error::<T>::AmountTooSmall);
                let opening_value = T::Pool::quote_sell(netuid, low.into())?.to_u64();
                OpeningQuote {
                    principal: low,
                    annual_interest: opening_value,
                    opening_value,
                }
            }
            Side::Long => {
                let historical = Self::tao_for_alpha(quarter, reference.price, false)?
                    .checked_sub(old_principal)
                    .ok_or(Error::<T>::InsufficientEscrow)?;
                let executable = T::Pool::quote_sell(netuid, quarter.into())?
                    .to_u64()
                    .checked_sub(old_principal)
                    .ok_or(Error::<T>::InsufficientEscrow)?;
                let funded = Self::funded_long_limit_for(
                    netuid,
                    combined_collateral,
                    vault.available_tao,
                    old_principal,
                    existing.map(|(owner, _)| owner),
                )?;
                let principal = historical.min(executable).min(funded);
                OpeningQuote {
                    principal,
                    annual_interest: Self::alpha_for_tao(principal, reference.price, true)?,
                    opening_value: principal,
                }
            }
        };
        ensure!(
            quote.principal > 0
                && quote.annual_interest > 0
                && quote.opening_value >= T::MinimumLoanValue::get(),
            Error::<T>::AmountTooSmall
        );
        // Quotes must reject exactly the checked additions dispatch will perform.
        if let Some((_, position)) = existing {
            Self::add(position.principal, quote.principal)?;
            Self::add(position.annual_interest, quote.annual_interest)?;
            if side == Side::Short {
                Self::add(position.proceeds, quote.opening_value)?;
            }
        }
        let (available, outstanding) = match side {
            Side::Short => (vault.available_alpha, vault.outstanding_alpha),
            Side::Long => (vault.available_tao, vault.outstanding_tao),
        };
        ensure!(
            quote.principal <= available,
            Error::<T>::InsufficientReserves
        );
        let funded = u128::from(available).saturating_add(u128::from(outstanding));
        let borrowed = u128::from(outstanding).saturating_add(u128::from(quote.principal));
        ensure!(
            borrowed.saturating_mul(10) <= funded,
            Error::<T>::BorrowingLimit
        );
        Ok(quote)
    }

    /// Protect ordinary pro-rata redemption even if all active AMM TAO is sold out.
    /// Only physically unloaned vault TAO is backing; coupons and expected recoveries
    /// are excluded. Surplus collateral belongs to its owner, so every loan is checked
    /// independently rather than pooling borrowers' collateral.
    #[cfg(any(test, feature = "runtime-benchmarks"))]
    fn funded_long_limit(
        netuid: NetUid,
        collateral: u64,
        backing: u64,
    ) -> Result<u64, DispatchError> {
        Self::funded_long_limit_for(netuid, collateral, backing, 0, None)
    }

    fn funded_long_limit_for(
        netuid: NetUid,
        collateral: u64,
        backing: u64,
        old_principal: u64,
        growing_owner: Option<&T::AccountId>,
    ) -> Result<u64, DispatchError> {
        let supply = T::Pool::redemption_alpha_supply(netuid)
            .map_err(|_| Error::<T>::RedemptionUnavailable)?;
        ensure!(supply > 0, Error::<T>::RedemptionUnavailable);
        let denominator = supply
            .checked_mul(4)
            .and_then(|n| n.checked_add(u128::from(collateral)))
            .ok_or(Error::<T>::Arithmetic)?;
        // Solve 4 * (old_debt + new_debt) * supply <= collateral * (backing - new_debt).
        let numerator = u128::from(collateral)
            .checked_mul(u128::from(backing))
            .ok_or(Error::<T>::Arithmetic)?;
        let existing_claim = u128::from(old_principal)
            .checked_mul(supply)
            .and_then(|claim| claim.checked_mul(4))
            .ok_or(Error::<T>::Arithmetic)?;
        let numerator = numerator
            .checked_sub(existing_claim)
            .ok_or(Error::<T>::InsufficientRedemptionBacking)?;
        let funded = numerator
            .checked_div(denominator)
            .ok_or(Error::<T>::Arithmetic)?;
        let mut limit = u64::try_from(funded).map_err(|_| Error::<T>::Arithmetic)?;
        let now = frame_system::Pallet::<T>::block_number();
        let max_positions = T::MaxPositionsPerSubnet::get();
        let mut count = 0_u32;
        for (owner, ()) in
            OpenByNetuid::<T>::iter_prefix(netuid).take(max_positions.saturating_add(1) as usize)
        {
            count = count.checked_add(1).ok_or(Error::<T>::Arithmetic)?;
            ensure!(count <= max_positions, Error::<T>::TooManyPositions);
            let position =
                Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
            // This owner's combined position already passes the stricter 25% check
            // with its new collateral. Every other loan still needs independent coverage.
            if growing_owner == Some(&owner) {
                continue;
            }
            if position.side != Side::Long {
                continue;
            }
            let (interest, _) = Self::interest_due(&position, now, true)?;
            let remaining = position
                .collateral
                .checked_sub(interest)
                .filter(|remaining| *remaining > 0)
                .ok_or(Error::<T>::InsufficientRedemptionBacking)?;
            let claim = u128::from(position.principal)
                .checked_mul(supply)
                .ok_or(Error::<T>::Arithmetic)?;
            let remaining = u128::from(remaining);
            // Ceiling division without overflowing by adding the divisor first.
            let quotient = claim.checked_div(remaining).ok_or(Error::<T>::Arithmetic)?;
            let remainder = claim.checked_rem(remaining).ok_or(Error::<T>::Arithmetic)?;
            let required = quotient
                .checked_add(u128::from(remainder != 0))
                .ok_or(Error::<T>::Arithmetic)?;
            let required =
                u64::try_from(required).map_err(|_| Error::<T>::InsufficientRedemptionBacking)?;
            let allowance = backing
                .checked_sub(required)
                .ok_or(Error::<T>::InsufficientRedemptionBacking)?;
            limit = limit.min(allowance);
        }
        ensure!(limit > 0, Error::<T>::InsufficientRedemptionBacking);
        Ok(limit)
    }

    pub fn quote_close(
        owner: &T::AccountId,
        netuid: NetUid,
        repay_from_wallet: bool,
    ) -> Result<ClosingQuote, DispatchError> {
        ensure!(
            !Dissolutions::<T>::contains_key(netuid),
            Error::<T>::SubnetUnavailable
        );
        let mut position = Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
        let (interest, _) =
            Self::interest_due(&position, frame_system::Pallet::<T>::block_number(), true)?;
        ensure!(
            position.collateral > interest,
            Error::<T>::InsufficientEscrow
        );
        let remaining = position.collateral.saturating_sub(interest);
        match position.side {
            Side::Short => {
                let pot = Self::add(remaining, position.proceeds)?;
                let payment = if repay_from_wallet {
                    position.principal
                } else {
                    use frame_support::storage::{TransactionOutcome, with_transaction};
                    let escrow = Self::position_account(owner, netuid);
                    with_transaction(|| {
                        let result = (|| {
                            Self::charge_interest(
                                owner,
                                netuid,
                                &mut position,
                                frame_system::Pallet::<T>::block_number(),
                                true,
                            )?;
                            Self::execute_buyback(&escrow, netuid, position.principal, pot)
                                .map(|(payment, _)| payment)
                        })();
                        TransactionOutcome::Rollback(result)
                    })?
                };
                let refund = if repay_from_wallet {
                    pot
                } else {
                    Self::sub(pot, payment)?
                };
                Ok(ClosingQuote { payment, refund })
            }
            Side::Long => Ok(ClosingQuote {
                payment: position.principal,
                refund: remaining,
            }),
        }
    }

    fn buyback_input(netuid: NetUid, principal: u64, budget: u64) -> Result<u64, DispatchError> {
        ensure!(budget > 0, Error::<T>::InsufficientEscrow);
        // Capacity is supplied independently of quote dust errors. Positive-input
        // quotes whose rounded output is zero are lower-bound failures, never evidence
        // that larger inputs cross the finite buy endpoint.
        let mut high = budget.min(T::Pool::max_buy_input(netuid).to_u64());
        ensure!(high > 0, Error::<T>::InsufficientEscrow);
        ensure!(
            T::Pool::quote_buy(netuid, high.into())?.to_u64() >= principal,
            Error::<T>::InsufficientEscrow
        );
        let mut low = 1_u64;
        while low < high {
            let mid =
                low.saturating_add(high.saturating_sub(low).checked_div(2).unwrap_or_default());
            if T::Pool::quote_buy(netuid, mid.into()).is_ok_and(|out| out.to_u64() >= principal) {
                high = mid;
            } else {
                low = mid.saturating_add(1);
            }
        }
        Ok(low)
    }

    fn execute_buyback(
        escrow: &T::AccountId,
        netuid: NetUid,
        principal: u64,
        budget: u64,
    ) -> Result<(u64, u64), DispatchError> {
        let custody = Self::custody_hotkey()?;
        let mut payment = 0_u64;
        let mut alpha = 0_u64;
        for _ in 0..MAX_BUYBACK_STEPS {
            let remaining = principal.saturating_sub(alpha);
            if remaining == 0 {
                return Ok((payment, alpha));
            }
            let capacity = budget
                .saturating_sub(payment)
                .min(T::Pool::buy_spendable_tao(escrow).to_u64())
                .min(T::Pool::max_buy_input(netuid).to_u64());
            ensure!(capacity > 0, Error::<T>::InsufficientEscrow);
            let maximum = T::Pool::quote_buy(netuid, capacity.into())?.to_u64();
            ensure!(maximum > 0, Error::<T>::InsufficientEscrow);
            let input = if maximum >= remaining {
                Self::buyback_input(netuid, remaining, capacity)?
            } else {
                capacity
            };
            let expected = T::Pool::quote_buy(netuid, input.into())?.to_u64();
            let received = T::Pool::buy_alpha(
                escrow,
                &custody,
                netuid,
                input.into(),
                u64::MAX.into(),
                false,
            )?
            .to_u64();
            ensure!(received == expected, Error::<T>::InvalidQuote);
            payment = Self::add(payment, input)?;
            alpha = Self::add(alpha, received)?;
        }
        ensure!(alpha >= principal, Error::<T>::InsufficientEscrow);
        Ok((payment, alpha))
    }

    fn tao_for_alpha(alpha: u64, price: U64F64, round_up: bool) -> Result<u64, DispatchError> {
        let value = price
            .checked_mul(U64F64::from_num(alpha))
            .ok_or(Error::<T>::Arithmetic)?;
        let rounded = if round_up {
            value.checked_ceil().ok_or(Error::<T>::Arithmetic)?
        } else {
            value.floor()
        };
        Ok(rounded.saturating_to_num())
    }

    fn alpha_for_tao(tao: u64, price: U64F64, round_up: bool) -> Result<u64, DispatchError> {
        let value = U64F64::from_num(tao)
            .checked_div(price)
            .ok_or(Error::<T>::Arithmetic)?;
        let rounded = if round_up {
            value.checked_ceil().ok_or(Error::<T>::Arithmetic)?
        } else {
            value.floor()
        };
        Ok(rounded.saturating_to_num())
    }

    fn add(a: u64, b: u64) -> Result<u64, DispatchError> {
        a.checked_add(b).ok_or(Error::<T>::Arithmetic.into())
    }
    fn sub(a: u64, b: u64) -> Result<u64, DispatchError> {
        a.checked_sub(b).ok_or(Error::<T>::Arithmetic.into())
    }

    pub fn ema_update(previous: U64F64, observed: U64F64) -> Option<U64F64> {
        if previous.to_bits() == 0 || observed.to_bits() == 0 {
            return None;
        }
        let half = previous.checked_div(U64F64::from_num(2))?;
        let twice = previous.saturating_mul(U64F64::from_num(2));
        let bounded = observed.clamp(half, twice);
        let ratio = bounded.checked_div(previous)?;
        let log_ratio: I64F64 = ln(I64F64::checked_from_num(ratio)?).ok()?;
        let exponent =
            log_ratio.checked_mul(I64F64::from_num(U64F64::from_bits(EMA_WEIGHT_BITS)))?;
        let multiplier: I64F64 = exp(exponent).ok()?;
        previous.checked_mul(U64F64::checked_from_num(multiplier)?)
    }

    fn interest_due(
        position: &Position<T::AccountId, BlockNumberFor<T>>,
        until: BlockNumberFor<T>,
        final_charge: bool,
    ) -> Result<(u64, u64), DispatchError> {
        let elapsed: u64 = until.saturating_sub(position.last_accrued).saturated_into();
        let denominator = u128::from(T::BlocksPerYear::get());
        ensure!(denominator > 0, Error::<T>::Arithmetic);
        let numerator = u128::from(position.annual_interest)
            .saturating_mul(u128::from(elapsed))
            .saturating_add(u128::from(position.interest_remainder));
        let whole = numerator
            .checked_div(denominator)
            .ok_or(Error::<T>::Arithmetic)?;
        let remainder = numerator
            .checked_rem(denominator)
            .ok_or(Error::<T>::Arithmetic)?;
        let paid = if final_charge && remainder > 0 {
            whole.saturating_add(1)
        } else {
            whole
        };
        Ok((
            paid.min(u128::from(u64::MAX)) as u64,
            if final_charge { 0 } else { remainder as u64 },
        ))
    }

    fn charge_interest(
        owner: &T::AccountId,
        netuid: NetUid,
        position: &mut Position<T::AccountId, BlockNumberFor<T>>,
        until: BlockNumberFor<T>,
        final_charge: bool,
    ) -> DispatchResult {
        let (due, remainder) = Self::interest_due(position, until, final_charge)?;
        let paid = due.min(position.collateral);
        if paid > 0 {
            let escrow = Self::position_account(owner, netuid);
            let vault = Self::reserve_account(netuid);
            let mut credited = paid;
            match position.side {
                Side::Short => {
                    credited =
                        T::Pool::collect_interest_tao(&escrow, &vault, paid.into())?.to_u64();
                    ensure!(credited <= paid, Error::<T>::InvalidQuote);
                    if credited < paid {
                        Self::deposit_event(Event::DustForfeited {
                            netuid,
                            recipient: vault,
                            tao: paid.saturating_sub(credited),
                        });
                    }
                }
                Side::Long => {
                    let custody = Self::custody_hotkey()?;
                    if Dissolutions::<T>::contains_key(netuid) {
                        T::Pool::transfer_dissolution_alpha(
                            &escrow,
                            &vault,
                            &custody,
                            netuid,
                            paid.into(),
                        )?;
                    } else {
                        T::Pool::transfer_staked_alpha(
                            &escrow,
                            &custody,
                            &vault,
                            &custody,
                            netuid,
                            paid.into(),
                            false,
                            false,
                        )?;
                    }
                }
            }
            Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
                let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
                match position.side {
                    Side::Short => vault.pending_tao = Self::add(vault.pending_tao, credited)?,
                    Side::Long => vault.pending_alpha = Self::add(vault.pending_alpha, credited)?,
                }
                Ok(())
            })?;
            position.collateral = Self::sub(position.collateral, paid)?;
            Self::deposit_event(Event::InterestCollected {
                owner: owner.clone(),
                netuid,
                collateral_paid: paid,
            });
        }
        position.last_accrued = until;
        position.interest_remainder = remainder;
        Ok(())
    }

    fn credit_principal(
        netuid: NetUid,
        side: Side,
        principal: u64,
        received: u64,
    ) -> DispatchResult {
        Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
            let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
            match side {
                Side::Short => {
                    vault.outstanding_alpha = Self::sub(vault.outstanding_alpha, principal)?;
                    vault.available_alpha = Self::add(vault.available_alpha, received)?;
                }
                Side::Long => {
                    vault.outstanding_tao = Self::sub(vault.outstanding_tao, principal)?;
                    vault.available_tao = Self::add(vault.available_tao, received)?;
                }
            }
            Ok(())
        })
    }

    fn remove_position(
        owner: &T::AccountId,
        netuid: NetUid,
        position: &Position<T::AccountId, BlockNumberFor<T>>,
    ) {
        Positions::<T>::remove(owner, netuid);
        OpenByNetuid::<T>::remove(netuid, owner);
        EscrowOwner::<T>::remove(netuid, Self::position_account(owner, netuid));
        Due::<T>::remove(position.due, (owner, netuid));
        PositionCount::<T>::mutate(netuid, |count| *count = count.saturating_sub(1));
        let remaining = TotalPositions::<T>::mutate(|count| {
            *count = count.saturating_sub(1);
            *count
        });
        if remaining == 0 {
            NextDue::<T>::kill();
        }
        LoanHotkeys::<T>::mutate_exists(&position.hotkey, |count| {
            let remaining = count.unwrap_or_default().saturating_sub(1);
            *count = if remaining == 0 {
                None
            } else {
                Some(remaining)
            };
        });
    }

    /// Follow a real stake migration without letting a nominated borrower veto a
    /// validator's hotkey change. The global position limit bounds this scan.
    #[transactional]
    pub fn on_hotkey_swap(
        old: &T::AccountId,
        new: &T::AccountId,
        netuid: Option<NetUid>,
    ) -> DispatchResult {
        if old == new || LoanHotkeys::<T>::get(old) == 0 {
            return Ok(());
        }
        let mut moved = 0_u32;
        for (owner, this_netuid, mut position) in Positions::<T>::iter() {
            if position.hotkey == *old && netuid.is_none_or(|id| id == this_netuid) {
                position.hotkey = new.clone();
                Positions::<T>::insert(owner, this_netuid, position);
                moved = moved.checked_add(1).ok_or(Error::<T>::Arithmetic)?;
            }
        }
        if moved > 0 {
            LoanHotkeys::<T>::try_mutate_exists(old, |count| -> DispatchResult {
                let remaining = count
                    .unwrap_or_default()
                    .checked_sub(moved)
                    .ok_or(Error::<T>::Arithmetic)?;
                *count = (remaining > 0).then_some(remaining);
                Ok(())
            })?;
            LoanHotkeys::<T>::try_mutate(new, |count| -> DispatchResult {
                *count = count.checked_add(moved).ok_or(Error::<T>::Arithmetic)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn refund_terminal(
        netuid: NetUid,
        from: &T::AccountId,
        recipient: &T::AccountId,
        amount: u64,
    ) -> Result<u64, DispatchError> {
        if amount == 0 {
            return Ok(0);
        }
        let credited = T::Pool::refund_dissolution_tao(from, recipient, amount.into())?.to_u64();
        ensure!(credited <= amount, Error::<T>::InvalidQuote);
        if credited < amount {
            Self::deposit_event(Event::DustForfeited {
                netuid,
                recipient: recipient.clone(),
                tao: amount.saturating_sub(credited),
            });
        }
        Ok(credited)
    }

    fn disburse_terminal(
        netuid: NetUid,
        from: &T::AccountId,
        recipient: &T::AccountId,
        recovery: u64,
        refund: u64,
    ) -> Result<(u64, u64), DispatchError> {
        let vault = Self::reserve_account(netuid);
        let (recovered, refunded) =
            T::Pool::terminal_split_tao(from, &vault, recipient, recovery.into(), refund.into())?;
        let (recovered, refunded) = (recovered.to_u64(), refunded.to_u64());
        ensure!(
            recovered <= recovery && refunded <= refund,
            Error::<T>::InvalidQuote
        );
        for (recipient, requested, credited) in [
            (vault, recovery, recovered),
            (recipient.clone(), refund, refunded),
        ] {
            if credited < requested {
                Self::deposit_event(Event::DustForfeited {
                    netuid,
                    recipient,
                    tao: requested.saturating_sub(credited),
                });
            }
        }
        Ok((recovered, refunded))
    }

    fn forfeit(
        owner: &T::AccountId,
        netuid: NetUid,
        position: &Position<T::AccountId, BlockNumberFor<T>>,
    ) -> DispatchResult {
        if position.proceeds > 0 {
            T::Pool::transfer_tao(
                &Self::position_account(owner, netuid),
                &Self::reserve_account(netuid),
                position.proceeds.into(),
            )?;
        }
        Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
            let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
            match position.side {
                Side::Short => {
                    vault.outstanding_alpha =
                        Self::sub(vault.outstanding_alpha, position.principal)?;
                    vault.lost_alpha = Self::add(vault.lost_alpha, position.principal)?;
                    vault.available_tao = Self::add(vault.available_tao, position.proceeds)?;
                }
                Side::Long => {
                    vault.outstanding_tao = Self::sub(vault.outstanding_tao, position.principal)?;
                    vault.lost_tao = Self::add(vault.lost_tao, position.principal)?;
                }
            }
            Ok(())
        })?;
        Self::remove_position(owner, netuid, position);
        Self::deposit_event(Event::Forfeited {
            owner: owner.clone(),
            netuid,
            side: position.side,
            principal_lost: position.principal,
        });
        Ok(())
    }

    fn collect_due(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
        use frame_support::storage::{TransactionOutcome, with_transaction};
        if meter
            .try_consume(T::DbWeight::get().reads_writes(2, 1))
            .is_err()
        {
            return;
        }
        if TotalPositions::<T>::get() == 0 {
            NextDue::<T>::kill();
            return;
        }
        let Some(mut due) = NextDue::<T>::get() else {
            return;
        };
        while due <= now {
            if meter
                .try_consume(T::DbWeight::get().reads_writes(1, 1))
                .is_err()
            {
                break;
            }
            let Some(((owner, netuid), ())) = Due::<T>::iter_prefix(due).next() else {
                due = due.saturating_add(1_u32.into());
                continue;
            };
            if meter.try_consume(T::WeightInfo::collect()).is_err() {
                break;
            }
            let result: DispatchResult = with_transaction(|| {
                let result = (|| -> DispatchResult {
                    let Some(mut position) = Positions::<T>::get(&owner, netuid) else {
                        Due::<T>::remove(due, (&owner, netuid));
                        return Ok(());
                    };
                    // Terminal settlement owns frozen accrual. Remove it from the active queue.
                    if Dissolutions::<T>::contains_key(netuid) {
                        Due::<T>::remove(due, (&owner, netuid));
                        return Ok(());
                    }
                    Self::charge_interest(&owner, netuid, &mut position, now, false)?;
                    if position.collateral == 0 {
                        Self::forfeit(&owner, netuid, &position)?;
                    } else {
                        Due::<T>::remove(position.due, (&owner, netuid));
                        position.due = now.saturating_add(T::InterestPeriod::get());
                        Due::<T>::insert(position.due, (&owner, netuid), ());
                        Positions::<T>::insert(&owner, netuid, position);
                    }
                    Ok(())
                })();
                if result.is_ok() {
                    TransactionOutcome::Commit(result)
                } else {
                    TransactionOutcome::Rollback(result)
                }
            });
            if result.is_err() {
                // One failing transfer must not starve every other weekly payment. Debt keeps
                // accruing from the untouched clock and collection retries next block.
                let retry = now.saturating_add(1_u32.into());
                Due::<T>::remove(due, (&owner, netuid));
                Due::<T>::insert(retry, (&owner, netuid), ());
                Positions::<T>::mutate(&owner, netuid, |position| {
                    if let Some(position) = position {
                        position.due = retry;
                    }
                });
            }
            if TotalPositions::<T>::get() == 0 {
                break;
            }
        }
        if TotalPositions::<T>::get() == 0 {
            NextDue::<T>::kill();
        } else {
            NextDue::<T>::put(due);
        }
    }

    /// Alpha fee sales require at least 98% of the mature EMA's fair TAO output,
    /// including fees and depth. Floor once to output atoms; favorable execution
    /// is unrestricted. Failed chunks remain physically backed pending alpha.
    fn coupon_minimum_output(input: u64, reference: U64F64) -> Result<u64, DispatchError> {
        let fair = U64F64::from_num(input)
            .checked_mul(reference)
            .ok_or(Error::<T>::Arithmetic)?;
        let minimum = fair
            .checked_div(U64F64::from_num(50))
            .and_then(|value| value.checked_mul(U64F64::from_num(49)))
            .ok_or(Error::<T>::Arithmetic)?;
        minimum
            .floor()
            .checked_to_num()
            .ok_or(Error::<T>::Arithmetic.into())
    }

    /// Short coupons already are TAO, so burning requires no quote or price reference.
    #[transactional]
    fn burn_pending_tao(netuid: NetUid) -> DispatchResult {
        let amount = Vaults::<T>::get(netuid)
            .ok_or(Error::<T>::InsufficientReserves)?
            .pending_tao;
        if amount == 0 {
            return Ok(());
        }
        T::Pool::burn_interest_tao(&Self::reserve_account(netuid), amount.into())?;
        Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
            let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
            vault.pending_tao = Self::sub(vault.pending_tao, amount)?;
            Ok(())
        })?;
        Self::deposit_event(Event::InterestBurned {
            netuid,
            side: Side::Short,
            tao: amount,
        });
        Ok(())
    }

    fn convert_pending(now: BlockNumberFor<T>, meter: &mut WeightMeter) {
        use frame_support::storage::{TransactionOutcome, with_transaction};
        // Reserve cursor setup, iterator termination and the final cursor removal,
        // including the empty-vault case, before accessing storage.
        if meter
            .try_consume(T::DbWeight::get().reads_writes(2, 1))
            .is_err()
        {
            return;
        }
        let iter = if let Some(cursor) = ConversionCursor::<T>::get() {
            Vaults::<T>::iter_from(Vaults::<T>::hashed_key_for(cursor))
        } else {
            Vaults::<T>::iter()
        };
        let mut finished = true;
        for (netuid, vault) in iter {
            if meter
                .try_consume(T::DbWeight::get().reads_writes(4, 1))
                .is_err()
            {
                finished = false;
                break;
            }
            if Dissolutions::<T>::contains_key(netuid)
                || (vault.pending_tao == 0 && vault.pending_alpha == 0)
            {
                ConversionCursor::<T>::put(netuid);
                continue;
            }
            if meter.try_consume(T::WeightInfo::collect()).is_err() {
                finished = false;
                break;
            }
            ConversionCursor::<T>::put(netuid);
            // A failed burn retains backed TAO for retry without blocking collection
            // or a safe alpha fee sale for the other side.
            let _ = Self::burn_pending_tao(netuid);
            if vault.pending_alpha == 0 {
                continue;
            }
            let Some(reference) = References::<T>::get(netuid)
                .filter(|reference| reference.price.to_bits() > 0 && now >= reference.valid_after)
            else {
                continue;
            };
            // At most 64 halvings find a nonzero executable, price-guarded chunk.
            let mut input = vault.pending_alpha;
            let mut output = 0_u64;
            while input > 0 {
                if let Ok(quoted) = T::Pool::quote_sell(netuid, input.into())
                    && !quoted.is_zero()
                    && Self::coupon_minimum_output(input, reference.price)
                        .is_ok_and(|minimum| quoted.to_u64() >= minimum)
                {
                    output = quoted.to_u64();
                    break;
                }
                input = input.checked_div(2).unwrap_or_default();
            }
            if input == 0 || output == 0 {
                continue;
            }
            let _: DispatchResult = with_transaction(|| {
                let result = (|| -> DispatchResult {
                    let account = Self::reserve_account(netuid);
                    let received = T::Pool::sell_alpha(
                        &account,
                        &Self::custody_hotkey()?,
                        netuid,
                        input.into(),
                        TaoBalance::ZERO,
                        false,
                    )?
                    .to_u64();
                    ensure!(received == output, Error::<T>::InvalidQuote);
                    // Sale and burn share one transaction: a failed exact burn also
                    // rolls back the AMM trade, leaving the original alpha fee intact.
                    T::Pool::burn_interest_tao(&account, received.into())?;
                    Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
                        let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
                        vault.pending_alpha = Self::sub(vault.pending_alpha, input)?;
                        Ok(())
                    })?;
                    Self::deposit_event(Event::InterestBurned {
                        netuid,
                        side: Side::Long,
                        tao: received,
                    });
                    Ok(())
                })();
                if result.is_ok() {
                    TransactionOutcome::Commit(result)
                } else {
                    TransactionOutcome::Rollback(result)
                }
            });
        }
        if finished {
            ConversionCursor::<T>::kill();
        }
    }

    /// Freeze once at the deregistration trigger, before cleanup may be deferred.
    pub fn start_dissolution(netuid: NetUid) -> DispatchResult {
        ensure!(
            !Dissolutions::<T>::contains_key(netuid),
            Error::<T>::AlreadyDissolving
        );
        let Some(reference) = References::<T>::get(netuid) else {
            return Ok(());
        };
        let price = reference.price.max(T::Pool::current_alpha_price(netuid));
        let frozen_at = frame_system::Pallet::<T>::block_number();
        Dissolutions::<T>::insert(
            netuid,
            Dissolution {
                price,
                frozen_at,
                reserves_returned: false,
            },
        );
        Self::deposit_event(Event::DissolutionFrozen {
            netuid,
            price,
            frozen_at,
        });
        Ok(())
    }

    /// Settle custodial short cash first, then return unlent vault inventory. Every
    /// resulting asset enters the funded pot before ordinary alpha payouts are fixed.
    pub fn settle_shorts(netuid: NetUid, meter: &mut WeightMeter) -> bool {
        if meter
            .try_consume(T::DbWeight::get().reads_writes(3, 1))
            .is_err()
        {
            return false;
        }
        let Some(dissolution) = Dissolutions::<T>::get(netuid) else {
            return true;
        };
        if dissolution.reserves_returned {
            return true;
        }
        let positions = if let Some(cursor) = DissolutionCursor::<T>::get(netuid) {
            OpenByNetuid::<T>::iter_prefix_from(
                netuid,
                OpenByNetuid::<T>::hashed_key_for(netuid, cursor),
            )
        } else {
            OpenByNetuid::<T>::iter_prefix(netuid)
        };
        for (owner, ()) in positions {
            if meter
                .try_consume(T::DbWeight::get().reads_writes(2, 1))
                .is_err()
            {
                return false;
            }
            let Some(position) = Positions::<T>::get(&owner, netuid) else {
                continue;
            };
            if meter.try_consume(T::WeightInfo::settle()).is_err() {
                return false;
            }
            let settled = match position.side {
                Side::Short => Self::settle_short(netuid, &owner, position, &dissolution),
                Side::Long => Self::freeze_long_interest(netuid, &owner),
            };
            if settled.is_err() {
                return false;
            }
            DissolutionCursor::<T>::insert(netuid, owner);
        }
        if !dissolution.reserves_returned {
            if meter.try_consume(T::WeightInfo::settle()).is_err() {
                return false;
            }
            if Self::return_terminal_reserves(netuid).is_err() {
                return false;
            }
        }
        DissolutionCursor::<T>::remove(netuid);
        true
    }

    #[transactional]
    fn settle_short(
        netuid: NetUid,
        owner: &T::AccountId,
        mut position: Position<T::AccountId, BlockNumberFor<T>>,
        dissolution: &Dissolution<BlockNumberFor<T>>,
    ) -> DispatchResult {
        Self::charge_interest(owner, netuid, &mut position, dissolution.frozen_at, true)?;
        let pot = Self::add(position.collateral, position.proceeds)?;
        // Terminal cash settlement must remain possible even when the frozen mark
        // makes debt exceed the entire u64 TAO domain.
        let exact_owed = dissolution
            .price
            .checked_mul(U64F64::from_num(position.principal))
            .and_then(|value| value.checked_ceil())
            .map(|value| value.to_num::<u64>());
        let owed = exact_owed.unwrap_or(u64::MAX);
        let paid = pot.min(owed);
        let refund = pot.saturating_sub(paid);
        let escrow = Self::position_account(owner, netuid);
        let (paid, refund) = Self::disburse_terminal(netuid, &escrow, owner, paid, refund)?;
        let recovered = if exact_owed.is_some_and(|owed| paid >= owed) {
            position.principal
        } else {
            Self::alpha_for_tao(paid, dissolution.price, false)?.min(position.principal)
        };
        let lost = position.principal.saturating_sub(recovered);
        Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
            let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
            vault.outstanding_alpha = Self::sub(vault.outstanding_alpha, position.principal)?;
            vault.available_tao = Self::add(vault.available_tao, paid)?;
            vault.lost_alpha = Self::add(vault.lost_alpha, lost)?;
            Ok(())
        })?;
        Self::remove_position(owner, netuid, &position);
        Self::deposit_event(Event::DissolutionSettled {
            owner: owner.clone(),
            netuid,
            side: Side::Short,
            principal_recovered: recovered,
            principal_lost: lost,
            tao_refund: refund,
        });
        Ok(())
    }

    #[transactional]
    fn return_terminal_reserves(netuid: NetUid) -> DispatchResult {
        Self::burn_pending_tao(netuid)?;
        let vault = Vaults::<T>::get(netuid).ok_or(Error::<T>::InsufficientReserves)?;
        T::Pool::return_dissolution_reserves(
            netuid,
            &Self::reserve_account(netuid),
            &Self::custody_hotkey()?,
            vault.available_tao.into(),
            vault.available_alpha.into(),
        )?;
        Vaults::<T>::mutate(netuid, |vault| {
            if let Some(vault) = vault {
                vault.available_tao = 0;
                vault.available_alpha = 0;
                vault.pending_tao = 0;
            }
        });
        Dissolutions::<T>::mutate(netuid, |state| {
            if let Some(state) = state {
                state.reserves_returned = true;
            }
        });
        Ok(())
    }

    /// Record every actual funded redemption. An escrow may hold alpha under multiple
    /// hotkeys, so debt retirement waits until all ordinary payouts have finished.
    #[transactional]
    pub fn on_alpha_redemption(
        netuid: NetUid,
        coldkey: &T::AccountId,
        tao_paid: TaoBalance,
    ) -> DispatchResult {
        // Alpha fees remain ordinary vault stake until the global funded payout.
        // Burn only actual receipts, with no frozen-price mint or terminal swap.
        // Multiple hotkeys/pages can pay the same vault, so retain the alpha ledger
        // until the caller has finished every ordinary redemption.
        if coldkey == &Self::reserve_account(netuid)
            && Vaults::<T>::get(netuid).is_some_and(|vault| vault.pending_alpha > 0)
        {
            ensure!(
                Dissolutions::<T>::get(netuid).is_some_and(|state| state.reserves_returned),
                Error::<T>::SubnetUnavailable
            );
            if !tao_paid.is_zero() {
                T::Pool::burn_interest_tao(coldkey, tao_paid)?;
                Self::deposit_event(Event::InterestBurned {
                    netuid,
                    side: Side::Long,
                    tao: tao_paid.to_u64(),
                });
            }
            return Ok(());
        }
        let Some(owner) = EscrowOwner::<T>::get(netuid, coldkey) else {
            return Ok(());
        };
        let mut position =
            Positions::<T>::get(&owner, netuid).ok_or(Error::<T>::PositionMissing)?;
        ensure!(
            position.side == Side::Long && Dissolutions::<T>::contains_key(netuid),
            Error::<T>::InvalidQuote
        );
        position.proceeds = Self::add(position.proceeds, tao_paid.to_u64())?;
        Positions::<T>::insert(owner, netuid, position);
        Ok(())
    }

    #[transactional]
    fn settle_long(netuid: NetUid, owner: &T::AccountId) -> DispatchResult {
        let position = Positions::<T>::get(owner, netuid).ok_or(Error::<T>::PositionMissing)?;
        ensure!(
            position.side == Side::Long && Dissolutions::<T>::contains_key(netuid),
            Error::<T>::InvalidQuote
        );
        let recovered = position.proceeds.min(position.principal);
        let refund = position.proceeds.saturating_sub(recovered);
        let coldkey = Self::position_account(owner, netuid);
        let (recovered, refund) =
            Self::disburse_terminal(netuid, &coldkey, owner, recovered, refund)?;
        let lost = position.principal.saturating_sub(recovered);
        Vaults::<T>::try_mutate(netuid, |vault| -> DispatchResult {
            let vault = vault.as_mut().ok_or(Error::<T>::InsufficientReserves)?;
            vault.outstanding_tao = Self::sub(vault.outstanding_tao, position.principal)?;
            vault.available_tao = Self::add(vault.available_tao, recovered)?;
            vault.lost_tao = Self::add(vault.lost_tao, lost)?;
            Ok(())
        })?;
        Self::remove_position(owner, netuid, &position);
        Self::deposit_event(Event::DissolutionSettled {
            owner: owner.clone(),
            netuid,
            side: Side::Long,
            principal_recovered: recovered,
            principal_lost: lost,
            tao_refund: refund,
        });
        Ok(())
    }

    /// Flush long coupons only until the common cutoff, before alpha denominator
    /// calculation. This is separate from short settlement so all collateral rows
    /// are final before the ordinary dissolution values them.
    #[transactional]
    pub fn freeze_long_interest(netuid: NetUid, owner: &T::AccountId) -> DispatchResult {
        let dissolution = Dissolutions::<T>::get(netuid).ok_or(Error::<T>::SubnetUnavailable)?;
        let Some(mut position) = Positions::<T>::get(owner, netuid) else {
            return Ok(());
        };
        if position.side != Side::Long {
            return Ok(());
        }
        Self::charge_interest(owner, netuid, &mut position, dissolution.frozen_at, true)?;
        Positions::<T>::insert(owner, netuid, position);
        Ok(())
    }

    pub fn settle_remaining_longs(netuid: NetUid, meter: &mut WeightMeter) -> bool {
        if meter.try_consume(T::DbWeight::get().reads(1)).is_err() {
            return false;
        }
        for (owner, ()) in OpenByNetuid::<T>::iter_prefix(netuid) {
            if meter
                .try_consume(
                    T::DbWeight::get()
                        .reads(1)
                        .saturating_add(T::WeightInfo::settle()),
                )
                .is_err()
            {
                return false;
            }
            if Self::settle_long(netuid, &owner).is_err() {
                return false;
            }
        }
        // This lifecycle hook runs only after all ordinary funded payouts. Alpha
        // fees with no funded receipt are retired without claiming a TAO burn.
        if meter
            .try_consume(T::DbWeight::get().reads_writes(1, 1))
            .is_err()
        {
            return false;
        }
        Vaults::<T>::mutate(netuid, |vault| {
            if let Some(vault) = vault {
                vault.pending_alpha = 0;
            }
        });
        true
    }

    /// Generation cleanup; recovered TAO remains in the vault account as protocol
    /// recovery inventory. It must not seed a new subnet reusing this netuid.
    #[transactional]
    pub fn finish_dissolution(netuid: NetUid) -> DispatchResult {
        ensure!(
            PositionCount::<T>::get(netuid) == 0,
            Error::<T>::TooManyPositions
        );
        if let Some(vault) = Vaults::<T>::get(netuid) {
            ensure!(
                vault.available_alpha == 0 && vault.pending_alpha == 0 && vault.pending_tao == 0,
                Error::<T>::InvalidQuote
            );
            if vault.available_tao > 0 {
                Self::refund_terminal(
                    netuid,
                    &Self::reserve_account(netuid),
                    &Self::recovery_account(),
                    vault.available_tao,
                )?;
            }
            Vaults::<T>::remove(netuid);
            VaultCount::<T>::mutate(|count| *count = count.saturating_sub(1));
        }
        References::<T>::remove(netuid);
        Dissolutions::<T>::remove(netuid);
        DissolutionCursor::<T>::remove(netuid);
        PositionCount::<T>::remove(netuid);
        Ok(())
    }
}

impl<T: Config> LendingInterface<T::AccountId> for Pallet<T> {
    fn custody_accounts(netuid: NetUid) -> Option<(T::AccountId, T::AccountId)> {
        Self::initialize_custody().ok()?;
        Some((Self::reserve_account(netuid), Self::custody_hotkey().ok()?))
    }
    fn has_vault(netuid: NetUid) -> bool {
        Vaults::<T>::contains_key(netuid)
    }
    fn has_funding_capacity() -> bool {
        VaultCount::<T>::get() < T::MaxFundedSubnets::get()
    }
    fn has_positions(owner: &T::AccountId) -> bool {
        use frame_support::StorageDoubleMap as _;
        Positions::<T>::contains_prefix(owner)
    }
    fn has_hotkey_positions(hotkey: &T::AccountId) -> bool {
        LoanHotkeys::<T>::get(hotkey) > 0
    }
    fn max_positions() -> u32 {
        T::MaxTotalPositions::get()
    }
    fn on_hotkey_swap(
        old: &T::AccountId,
        new: &T::AccountId,
        netuid: Option<NetUid>,
    ) -> DispatchResult {
        Self::on_hotkey_swap(old, new, netuid)
    }
    fn fund_reserves(
        netuid: NetUid,
        tao: TaoBalance,
        alpha: AlphaBalance,
        reference: U64F64,
        has_history: bool,
    ) -> DispatchResult {
        Self::fund_reserves(netuid, tao, alpha, reference, has_history)
    }
    fn start_dissolution(netuid: NetUid) -> DispatchResult {
        Self::start_dissolution(netuid)
    }
    fn settle_shorts(netuid: NetUid, meter: &mut WeightMeter) -> bool {
        Self::settle_shorts(netuid, meter)
    }
    fn on_alpha_redemption(
        netuid: NetUid,
        coldkey: &T::AccountId,
        tao_paid: TaoBalance,
    ) -> DispatchResult {
        Self::on_alpha_redemption(netuid, coldkey, tao_paid)
    }
    fn settle_remaining_longs(netuid: NetUid, meter: &mut WeightMeter) -> bool {
        Self::settle_remaining_longs(netuid, meter)
    }
    fn finish_dissolution(netuid: NetUid) -> DispatchResult {
        Self::finish_dissolution(netuid)
    }
}
