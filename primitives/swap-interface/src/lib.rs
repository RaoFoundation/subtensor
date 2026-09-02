#![cfg_attr(not(feature = "std"), no_std)]
#![allow(clippy::too_many_arguments)]
use core::ops::Neg;

use frame_support::pallet_prelude::*;
use frame_support::weights::WeightMeter;
pub use order::*;
pub use sp_arithmetic::Perquintill;
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};

mod order;

/// Coarse failure classification for protocol-owned basket operations. Callers may recover from
/// an oversized input by chunking, and may explicitly write off a terminally untradeable asset;
/// every other error remains fatal so accounting/transfer failures are never mistaken for dust.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwapFailureKind {
    InputTooLarge,
    TerminalLiquidity,
    Other,
}

pub trait SwapEngine<O: Order>: DefaultPriceLimit<O::PaidIn, O::PaidOut> {
    fn swap(
        netuid: NetUid,
        order: O,
        price_limit: TaoBalance,
        drop_fees: bool,
        should_rollback: bool,
    ) -> Result<SwapResult<O::PaidIn, O::PaidOut>, DispatchError>;
}

pub trait SwapHandler {
    fn swap<O: Order>(
        netuid: NetUid,
        order: O,
        price_limit: TaoBalance,
        drop_fees: bool,
        should_rollback: bool,
    ) -> Result<SwapResult<O::PaidIn, O::PaidOut>, DispatchError>
    where
        Self: SwapEngine<O>;
    fn sim_swap<O: Order>(
        netuid: NetUid,
        order: O,
    ) -> Result<SwapResult<O::PaidIn, O::PaidOut>, DispatchError>
    where
        Self: SwapEngine<O>;

    fn approx_fee_amount<T: Token>(netuid: NetUid, amount: T) -> T;
    fn current_alpha_price(netuid: NetUid) -> U64F64;
    fn max_price<C: Token>() -> C;
    fn min_price<C: Token>() -> C;
    fn adjust_protocol_liquidity(
        netuid: NetUid,
        tao_delta: TaoBalance,
        alpha_delta: AlphaBalance,
    ) -> (TaoBalance, AlphaBalance);
    fn protocol_alpha_reservoir(netuid: NetUid) -> AlphaBalance;
    fn protocol_tao_reservoir(netuid: NetUid) -> TaoBalance;
    fn clear_protocol_liquidity_reservoirs(netuid: NetUid);
    fn clear_protocol_liquidity(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool;
    fn init_swap(netuid: NetUid, maybe_price: Option<U64F64>);
    fn get_alpha_amount_for_tao(netuid: NetUid, tao_amount: TaoBalance) -> AlphaBalance;

    /// Exact (slippage-aware, fee-free) TAO input needed to buy `alpha_amount` from the pool.
    /// Returns `TaoBalance::MAX` when the pool cannot supply that much alpha.
    fn tao_needed_for_alpha(netuid: NetUid, alpha_amount: AlphaBalance) -> TaoBalance;

    /// Exact (slippage-aware, fee-free) alpha input needed to obtain `tao_amount` from the pool.
    /// Returns `AlphaBalance::MAX` when the pool cannot supply that much TAO.
    fn alpha_needed_for_tao(netuid: NetUid, tao_amount: TaoBalance) -> AlphaBalance;

    /// Maximum conservative gross input accepted by one swap for this order. Protocol basket
    /// swaps drop fees, so the input-reserve multiple is exact for their use. Larger operations
    /// must be executed in sequential chunks so each chunk observes the reserves left by the
    /// previous one.
    fn max_swap_input<O: Order>(netuid: NetUid) -> O::PaidIn
    where
        Self: SwapEngine<O>;

    /// Classify a swap error without making callers depend on a concrete swap pallet's module
    /// error indices.
    fn classify_failure(error: &DispatchError) -> SwapFailureKind;
}

/// Combined swap + balance execution interface for limit orders.
///
/// Wraps the complete buy/sell operation: AMM state update (via `SwapHandler`),
/// pool reserve accounting, and user balance changes (TAO free balance /
/// alpha staking). Implemented by `pallet_subtensor::Pallet<T>` using
/// `stake_into_subnet` / `unstake_from_subnet`.
pub trait OrderSwapInterface<AccountId> {
    /// Buy alpha with TAO: debit `tao_amount` from `coldkey`'s free balance,
    /// credit resulting alpha as stake at `hotkey` on `netuid`.
    ///
    /// When `validate` is `true` the implementation enforces subnet
    /// existence, hotkey registration, minimum stake amount, sufficient
    /// coldkey balance, and sets the staking rate-limit flag for `(hotkey,
    /// coldkey, netuid)` after a successful stake. Pass `false` for internal
    /// pallet-intermediary swaps that must bypass these user-facing guards.
    /// Buy alpha with TAO: debit `tao_amount` from `coldkey`'s free balance,
    /// credit resulting alpha as stake at `hotkey` on `netuid`.
    ///
    /// **Implementations MUST be transactional** (wrap in
    /// `frame_support::storage::with_transaction` or annotate with
    /// `#[frame_support::transactional]`). The implementation debits the
    /// caller's balance before the pool swap; if the swap fails the debit
    /// must be rolled back to leave the caller's state unchanged.
    fn buy_alpha(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        tao_amount: TaoBalance,
        limit_price: TaoBalance,
        validate: bool,
    ) -> Result<AlphaBalance, DispatchError>;

    /// Sell alpha for TAO: remove `alpha_amount` from `coldkey`'s stake at
    /// `hotkey` on `netuid`, credit resulting TAO to `coldkey`'s free balance.
    ///
    /// When `validate` is `true` the implementation enforces subnet
    /// existence, hotkey registration, minimum stake amount, sufficient alpha
    /// balance, and checks that the staking rate-limit flag is not set for
    /// `(hotkey, coldkey, netuid)` (i.e. the account did not stake this
    /// block). Pass `false` for internal pallet-intermediary swaps.
    /// Sell alpha for TAO: remove `alpha_amount` from `coldkey`'s stake at
    /// `hotkey` on `netuid`, credit resulting TAO to `coldkey`'s free balance.
    ///
    /// **Implementations MUST be transactional** (wrap in
    /// `frame_support::storage::with_transaction` or annotate with
    /// `#[frame_support::transactional]`). The implementation decrements the
    /// caller's stake before the pool swap; if the swap fails the decrement
    /// must be rolled back to leave the caller's state unchanged.
    fn sell_alpha(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        limit_price: TaoBalance,
        validate: bool,
    ) -> Result<TaoBalance, DispatchError>;

    /// Current spot price: TAO per alpha, same scale as
    /// `SwapHandler::current_alpha_price`.
    fn current_alpha_price(netuid: NetUid) -> U64F64;

    /// Transfer `amount` TAO from `from`'s free balance to `to`'s free balance.
    ///
    /// Used by the batch executor to collect TAO from buy-order signers into
    /// the pallet intermediary account and to distribute TAO to sell-order
    /// signers after internal matching.
    fn transfer_tao(from: &AccountId, to: &AccountId, amount: TaoBalance) -> DispatchResult;

    /// Move `amount` staked alpha directly between two (coldkey, hotkey) pairs
    /// on `netuid` **without going through the AMM pool**.
    ///
    /// This is a pure stake-accounting transfer used for internal order
    /// matching in `execute_batched_orders`: it lets the pallet collect alpha
    /// from sell-order signers into its intermediary account, and later
    /// distribute alpha to buy-order signers, all without touching the pool.
    ///
    /// When `validate_sender` is `true`, the sender side is validated before
    /// the transfer: subnet existence, subtoken enabled, minimum stake amount,
    /// and the staking rate-limit flag for `(from_hotkey, from_coldkey,
    /// netuid)` is checked — the transfer is rejected if `from_coldkey`
    /// already staked this block.
    ///
    /// When `validate_receiver` is `true`, the staking rate-limit flag for
    /// `(to_hotkey, to_coldkey, netuid)` is set after the transfer, marking
    /// that `to_coldkey` has received stake this block.
    ///
    /// The two flags are intentionally separate so that each call site can
    /// opt into only the half it needs:
    /// - Collecting alpha from users into the pallet intermediary:
    ///   `validate_sender: true, validate_receiver: false` — validates the
    ///   user but does not rate-limit the intermediary account.
    /// - Distributing alpha from the pallet intermediary to buyers:
    ///   `validate_sender: false, validate_receiver: true` — skips checking
    ///   the intermediary (which would fail) and rate-limits the buyer.
    fn transfer_staked_alpha(
        from_coldkey: &AccountId,
        from_hotkey: &AccountId,
        to_coldkey: &AccountId,
        to_hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
        validate_sender: bool,
        validate_receiver: bool,
    ) -> DispatchResult;

    /// Set up a subnet for benchmark execution.
    ///
    /// Called once per benchmark before any orders are built. Implementations
    /// should initialise the subnet (registers it, enables the subtoken, seeds
    /// pool reserves) so that price queries and swaps succeed.
    /// The default is a no-op; override in runtime implementations.
    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_netuid_for_benchmark(_netuid: NetUid) {}

    /// Register `hotkey` as owned by `coldkey`.
    ///
    /// Called during `on_genesis` and `on_runtime_upgrade` to claim ownership of
    /// the pallet's hotkey before any external actor can register it. Safe to call
    /// multiple times — is a no-op if the hotkey account already exists.
    fn register_pallet_hotkey(coldkey: &AccountId, hotkey: &AccountId) -> DispatchResult;

    /// Returns `true` if `coldkey` is the registered owner of `hotkey`.
    fn pallet_hotkey_registered(coldkey: &AccountId, hotkey: &AccountId) -> bool;

    /// Set up accounts for benchmark execution.
    ///
    /// Called once per order before the benchmarked extrinsic runs. Implementations
    /// should fund `coldkey` with sufficient TAO (and alpha for sell orders) and
    /// register `hotkey` on the relevant subnet so that swap operations succeed.
    /// The default is a no-op; override in runtime implementations.
    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_acc_for_benchmark(_hotkey: &AccountId, _coldkey: &AccountId) {}
}

/// Pool primitives needed by `pallet-derivatives` to borrow a slice of a subnet's liquidity
/// and hand it back later.
///
/// Every method here touches pool reserves and stake accounting directly and **must not**
/// record user TAO flow (`SubnetTaoFlow`): a derivative position is a loan from the pool, not
/// a stake or unstake by the user. All internal swaps run with fees dropped. Implemented by
/// `pallet_subtensor::Pallet<T>`; consumers also rely on [`OrderSwapInterface`] for plain TAO
/// and stake transfers.
pub trait DerivativesPoolInterface<AccountId> {
    /// `true` when `netuid` exists, is a dynamic (AMM-priced) subnet and its subtoken is
    /// enabled. Positions may only be opened on such subnets.
    fn is_dynamic(netuid: NetUid) -> bool;

    /// Current price-active reserves `(SubnetTAO, SubnetAlphaIn)`.
    fn reserves(netuid: NetUid) -> (TaoBalance, AlphaBalance);

    /// Remove the fraction `phi` of both reserves from the pool without moving price. The TAO
    /// lands on `to_coldkey`'s free balance, the alpha becomes stake at
    /// `(to_hotkey, to_coldkey)`. Returns the exact `(tao, alpha)` removed (rounded down).
    fn lift_liquidity(
        netuid: NetUid,
        phi: Perquintill,
        to_coldkey: &AccountId,
        to_hotkey: &AccountId,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError>;

    /// Put `tao` (from `from_coldkey`'s free balance) and `alpha` (from stake at
    /// `(from_hotkey, from_coldkey)`) back into the pool. The pair may be unbalanced; the pool
    /// reweights so that price does not move. Also works while the subnet is dissolving, so
    /// borrowed liquidity can be handed back before stakes are converted.
    fn return_liquidity(
        netuid: NetUid,
        tao: TaoBalance,
        alpha: AlphaBalance,
        from_coldkey: &AccountId,
        from_hotkey: &AccountId,
    ) -> DispatchResult;

    /// Sell `alpha` staked at `(hotkey, coldkey)` into the pool, fee-free, without recording
    /// TAO flow. Resulting TAO is credited to `coldkey`.
    fn sell_alpha_internal(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Buy alpha with `tao` from `coldkey`'s free balance, fee-free, without recording TAO
    /// flow. Resulting alpha becomes stake at `(hotkey, coldkey)`.
    fn buy_alpha_internal(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError>;

    /// Buy at least `want` alpha for `(hotkey, coldkey)`, spending at most `budget` of
    /// `coldkey`'s free TAO. Same accounting as [`Self::buy_alpha_internal`]. Returns
    /// `(tao_spent, alpha_bought)`; `alpha_bought` is below `want` only when the whole budget
    /// was spent, so the caller can treat any remaining gap as a true shortfall.
    fn buy_alpha_for(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        want: AlphaBalance,
        budget: TaoBalance,
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError>;

    /// Sell at most `budget` alpha staked at `(hotkey, coldkey)` to raise at least `want` TAO
    /// for `coldkey`. Same accounting as [`Self::sell_alpha_internal`]. Returns
    /// `(alpha_sold, tao_raised)`; `tao_raised` is below `want` only when the whole budget was
    /// sold.
    fn sell_alpha_for(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        want: TaoBalance,
        budget: AlphaBalance,
    ) -> Result<(AlphaBalance, TaoBalance), DispatchError>;

    /// Move `amount` staked alpha between two `(coldkey, hotkey)` pairs with no validation
    /// beyond the sender's balance and the destination hotkey still existing. Also works while
    /// the subnet is dissolving. Used to hand a cushion back to its owner; user-facing deposits
    /// go through [`OrderSwapInterface::transfer_staked_alpha`] with validation on.
    fn transfer_stake_internal(
        from_coldkey: &AccountId,
        from_hotkey: &AccountId,
        to_coldkey: &AccountId,
        to_hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult;

    /// Make `netuid` a live dynamic subnet with a funded, price-initialised pool that
    /// [`Self::is_dynamic`] accepts. `OrderSwapInterface::set_up_netuid_for_benchmark` only
    /// seeds reserves, which is not enough to open a position against.
    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_pool_for_benchmark(_netuid: NetUid) {}

    /// Drop `hotkey`'s owner record, as a hotkey swap does, so stake can no longer be moved onto
    /// it. Lets the derivatives `close` benchmark exercise the sell-instead-of-return path.
    #[cfg(feature = "runtime-benchmarks")]
    fn forget_hotkey_for_benchmark(_hotkey: &AccountId) {}
}

pub trait DefaultPriceLimit<PaidIn, PaidOut>
where
    PaidIn: Token,
    PaidOut: Token,
{
    fn default_price_limit<C: Token>() -> C;
}

/// Externally used swap result (for RPC)
#[freeze_struct("6a03533fc53ccfb8")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct SwapResult<PaidIn, PaidOut>
where
    PaidIn: Token,
    PaidOut: Token,
{
    pub amount_paid_in: PaidIn,
    pub amount_paid_out: PaidOut,
    pub fee_paid: PaidIn,
    pub fee_to_block_author: PaidIn,
}

impl<PaidIn, PaidOut> SwapResult<PaidIn, PaidOut>
where
    PaidIn: Token,
    PaidOut: Token,
{
    pub fn paid_in_reserve_delta(&self) -> i128 {
        self.amount_paid_in.to_u64() as i128
    }

    pub fn paid_in_reserve_delta_i64(&self) -> i64 {
        self.paid_in_reserve_delta()
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    pub fn paid_out_reserve_delta(&self) -> i128 {
        (self.amount_paid_out.to_u64() as i128).neg()
    }

    pub fn paid_out_reserve_delta_i64(&self) -> i64 {
        (self.amount_paid_out.to_u64() as i128)
            .neg()
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
}
