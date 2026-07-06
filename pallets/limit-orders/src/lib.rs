#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
pub(crate) mod migrations;
#[cfg(test)]
mod tests;
pub mod weights;

type MigrationKeyMaxLen = frame_support::traits::ConstU32<128>;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{BoundedVec, traits::ConstU32};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_runtime::{
    AccountId32, MultiSignature, Perbill,
    traits::{ConstBool, Verify},
};
use substrate_fixed::types::U64F64;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::OrderSwapInterface;

// ── Data structures ──────────────────────────────────────────────────────────

/// Internal direction of a net pool trade. Used only for `GroupExecutionSummary`
/// and pool-swap bookkeeping; not part of the public order payload.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// The user-facing order type. Each variant encodes both the execution action
/// (buy alpha / sell alpha) and the price-trigger direction.
///
/// | Variant      | Action | Triggers when       |
/// |--------------|--------|---------------------|
/// | `LimitBuy`   | Buy    | price ≤ limit_price |
/// | `TakeProfit` | Sell   | price ≥ limit_price |
/// | `StopLoss`   | Sell   | price ≤ limit_price |
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub enum OrderType {
    LimitBuy,
    TakeProfit,
    StopLoss,
}

impl OrderType {
    /// `true` if this order results in buying alpha (staking into subnet).
    pub fn is_buy(&self) -> bool {
        matches!(self, OrderType::LimitBuy)
    }
}

/// The canonical order payload that users sign off-chain.
/// Only its H256 hash is stored on-chain; the full struct is submitted by the
/// admin at execution time (or by the user at cancellation time).
#[allow(clippy::multiple_bound_locations)] // bounds on AccountId required by FRAME derives
#[freeze_struct("27c7eedb92261456")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct Order<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
    /// The coldkey that authorised this order (pays TAO for buys; owns the
    /// staked alpha for sells).
    pub signer: AccountId,
    /// The hotkey to stake to (buy) or unstake from (sell).
    pub hotkey: AccountId,
    /// Target subnet.
    pub netuid: NetUid,
    /// Order type (LimitBuy, TakeProfit, or StopLoss).
    pub order_type: OrderType,
    /// Input amount: TAO (raw) for Buy, alpha (raw) for Sell.
    pub amount: u64,
    /// Price threshold in ×10⁹ scale (same as the `current_alpha_price` RPC endpoint).
    /// A value of `1_000_000_000` represents a price of 1.0 TAO/alpha.
    /// Sub-unity prices (e.g. 0.5 TAO/alpha) are expressed as `500_000_000`.
    /// Buy: maximum acceptable price.  Sell: minimum acceptable price.
    /// `u64::MAX` means no ceiling (buy at any price); `0` means no floor (sell at any price).
    pub limit_price: u64,
    /// Unix timestamp in milliseconds after which this order must not be executed.
    pub expiry: u64,
    /// Fee rate applied to this order's TAO amount (input for buys, output for sells).
    pub fee_rate: Perbill,
    /// Account that receives the fee collected from this order.
    pub fee_recipient: AccountId,
    /// Accounts authorized to relay this order. When set, only an account present
    /// in this list may submit the execution transaction. Supports up to 10 relayers.
    pub relayer: Option<BoundedVec<AccountId, ConstU32<10>>>,
    /// Maximum slippage tolerance in parts per billion applied to `limit_price`
    /// at execution time. `None` = no protection (execute at market).
    /// - Buy:  effective price ceiling = `limit_price + limit_price * max_slippage`
    /// - Sell: effective price floor   = `limit_price - limit_price * max_slippage`
    pub max_slippage: Option<Perbill>,
    /// EVM-compatible chain ID that this order is bound to.
    /// Prevents replay of testnet-signed orders on mainnet and vice versa.
    pub chain_id: u64,
    /// Wether partial fills are enabled
    pub partial_fills_enabled: bool,
}

/// Versioned wrapper around an order payload.
///
/// Adding a new variant in the future (e.g. `V2`) lets the pallet accept orders
/// signed against either schema simultaneously, preventing old signed orders from
/// being invalidated by a schema upgrade.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub enum VersionedOrder<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
    V1(Order<AccountId>),
}

impl<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> VersionedOrder<AccountId> {
    /// Returns a reference to the inner order regardless of version.
    pub fn inner(&self) -> &Order<AccountId> {
        match self {
            VersionedOrder::V1(order) => order,
        }
    }
}

/// The envelope the admin submits on-chain: the versioned order payload plus
/// the user's signature over the SCALE-encoded `VersionedOrder`.
///
/// Signature verification is performed against `order.inner().signer` (the AccountId)
/// directly. Only sr25519 signatures are accepted; ed25519 and ecdsa variants
/// of `MultiSignature` are rejected at validation time.
#[freeze_struct("9dd5a8ac812dc504")]
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub struct SignedOrder<AccountId: Encode + Decode + TypeInfo + MaxEncodedLen + Clone> {
    pub order: VersionedOrder<AccountId>,
    /// Sr25519 signature over `SCALE_ENCODE(VersionedOrder)`.
    pub signature: MultiSignature,
    /// Whether we want a partial fill for this order
    pub partial_fill: Option<u64>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
pub enum OrderStatus {
    /// The order was successfully executed.
    Fulfilled,
    /// The order was partially filled, with the amount already fulfilled in the enum
    PartiallyFilled(u64),
    /// The user registered a cancellation intent before execution.
    Cancelled,
}

/// Classified, fee-adjusted entry produced by `validate_and_classify`.
/// Used in every in-memory batch pipeline step; never stored on-chain.
#[derive(Debug, PartialEq)]
pub(crate) struct OrderEntry<AccountId> {
    pub(crate) order_id: H256,
    pub(crate) signer: AccountId,
    pub(crate) hotkey: AccountId,
    pub(crate) side: OrderType,
    /// Actual input amount being processed this execution (partial or full, before fee).
    pub(crate) gross: u64,
    /// Full order amount as signed by the user. Used to determine terminal status.
    pub(crate) order_amount: u64,
    /// Net input amount (after fee).
    /// For buys: `gross - fee_rate * gross`. For sells: equals `gross` (fee on TAO output).
    pub(crate) net: u64,
    /// Per-order fee rate.
    pub(crate) fee_rate: Perbill,
    /// Per-order fee recipient.
    pub(crate) fee_recipient: AccountId,
    /// Effective price limit passed to the pool swap.
    /// For buys: ceiling (max TAO per alpha the pool may charge).
    /// For sells: floor (min TAO per alpha the pool must return).
    /// Derived from `limit_price` and `max_slippage` during classification.
    pub(crate) effective_swap_limit: u64,
    /// Present when this execution covers only part of the order.
    pub(crate) partial_fill: Option<u64>,
}

// ── Pallet ───────────────────────────────────────────────────────────────────

#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;
    use crate::weights::WeightInfo as _;
    use frame_support::{
        PalletId,
        pallet_prelude::*,
        traits::{Get, UnixTime},
        transactional,
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::AccountIdConversion;
    use sp_std::collections::btree_set::BTreeSet;
    use sp_std::vec::Vec;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config<AccountId = AccountId32> {
        /// Full swap + balance execution interface (see [`OrderSwapInterface`]).
        type SwapInterface: OrderSwapInterface<Self::AccountId>;

        /// Time provider for expiry checks.
        type TimeProvider: UnixTime;

        /// Maximum number of orders in a single `execute_orders` call.
        /// Should equal `floor(max_block_weight / per_order_weight)`.
        #[pallet::constant]
        type MaxOrdersPerBatch: Get<u32>;

        /// PalletId used to derive the intermediary account for batch execution.
        ///
        /// The derived account temporarily holds pooled TAO and staked alpha
        /// during `execute_batched_orders` before distributing to order signers.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// Hotkey registered in each subnet that the pallet's intermediary
        /// account stakes to/from during batch execution.
        ///
        /// This must be a hotkey registered on every subnet the pallet may
        /// operate on. Operators should register a dedicated hotkey and set
        /// this in the runtime configuration.
        #[pallet::constant]
        type PalletHotkey: Get<Self::AccountId>;

        /// Weight information for the pallet's extrinsics.
        type WeightInfo: crate::weights::WeightInfo;

        /// EVM-compatible chain ID used to bind orders to a specific chain.
        /// Wire to `pallet_evm_chain_id` in the runtime via `ConfigurableChainId`.
        type ChainId: Get<u64>;
    }

    // ── Storage ───────────────────────────────────────────────────────────────

    /// Tracks the on-chain status of a known `OrderId`.
    /// Absent ⇒ never seen (still executable if valid).
    /// Present ⇒ Fulfilled or Cancelled (both are terminal).
    #[pallet::storage]
    pub type Orders<T: Config> = StorageMap<_, Blake2_128Concat, H256, OrderStatus, OptionQuery>;

    /// Switch to enable/disable the pallet.
    /// Defaults to `false` so bare node deployments are safe; genesis sets it to `true`.
    #[pallet::storage]
    pub type LimitOrdersEnabled<T: Config> = StorageValue<_, bool, ValueQuery, ConstBool<false>>;

    /// Tracks which named migrations have already been applied.
    /// Keyed by a short migration name; value is always `true`.
    #[pallet::storage]
    pub type HasMigrationRun<T: Config> =
        StorageMap<_, Identity, BoundedVec<u8, MigrationKeyMaxLen>, bool, ValueQuery>;

    // ── Events ────────────────────────────────────────────────────────────────

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A limit order was successfully executed.
        OrderExecuted {
            order_id: H256,
            signer: T::AccountId,
            netuid: NetUid,
            order_type: OrderType,
            /// Input amount: TAO (raw) for Buy orders, alpha (raw) for Sell orders.
            amount_in: u64,
            /// Output amount: alpha (raw) received for Buy orders, TAO (raw) received for Sell orders (after fee).
            amount_out: u64,
        },
        /// An order was skipped during execution.
        OrderSkipped {
            order_id: H256,
            reason: sp_runtime::DispatchError,
        },
        /// A user registered a cancellation intent for their order.
        OrderCancelled {
            order_id: H256,
            signer: T::AccountId,
        },
        /// Summary emitted once per `execute_batched_orders` call.
        GroupExecutionSummary {
            /// The subnet all orders in this batch belong to.
            netuid: NetUid,
            /// Direction of the net pool trade (Buy = net TAO into pool).
            net_side: OrderSide,
            /// Net amount sent to the pool (TAO for Buy, alpha for Sell).
            /// Zero when buys and sells perfectly offset each other.
            net_amount: u64,
            /// Tokens received back from the pool.
            /// Zero when `net_amount` is zero.
            actual_out: u64,
            /// Number of orders that were successfully executed.
            executed_count: u32,
        },
        /// Root has either enabled(true) or disabled(false) the pallet
        LimitOrdersPalletStatusChanged { enabled: bool },
    }

    // ── Errors ────────────────────────────────────────────────────────────────

    #[pallet::error]
    pub enum Error<T> {
        /// The provided signature does not match the order payload and signer.
        InvalidSignature,
        /// The order has already been Fulfilled or Cancelled.
        OrderAlreadyProcessed,
        /// Order has been cancelled
        OrderCancelled,
        /// The order's expiry timestamp is in the past.
        OrderExpired,
        /// The current market price does not satisfy the order's limit price.
        PriceConditionNotMet,
        /// Caller is not the order signer (required for cancellation).
        Unauthorized,
        /// The pool swap returned zero output for a non-zero input.
        SwapReturnedZero,
        /// Root netuid (0) is not allowed for limit orders.
        RootNetUidNotAllowed,
        /// An order in the batch targets a different netuid than the batch netuid parameter.
        OrderNetUidMismatch,
        /// Limit orders are disabled
        LimitOrdersDisabled,
        /// Relayer not the same as specified in the order
        RelayerMissMatch,
        /// Partial fills not enabled for this order
        PartialFillsNotEnabled,
        /// Incorrect partial fill amount provided
        IncorrectPartialFillAmount,
        /// A relayer must be set on the order when using partial fills
        RelayerRequiredForPartialFill,
        /// The order's chain_id does not match the current chain.
        ChainIdMismatch,
        /// The pallet hotkey has not been registered to the pallet account.
        /// Call on_runtime_upgrade or wait for genesis to complete registration
        /// before enabling the pallet.
        PalletHotkeyNotRegistered,
        /// A TAO -> alpha conversion overflowed the fixed-point range.
        ArithmeticOverflow,
        /// The same order appears more than once in a single batch.
        DuplicateOrderInBatch,
        /// An order's pro-rata share in the batch rounded down to zero. The whole
        /// batch is rejected so the order's input is never consumed without
        /// delivering any output (conservation), and the order stays retryable in a
        /// differently-composed batch.
        ZeroShareInBatch,
    }

    // ── Hooks ─────────────────────────────────────────────────────────────────

    // ── Genesis ───────────────────────────────────────────────────────────────

    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        #[serde(skip)]
        pub _phantom: core::marker::PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            let _ = T::SwapInterface::register_pallet_hotkey(
                &Pallet::<T>::pallet_account(),
                &T::PalletHotkey::get(),
            );
            // Enable the pallet on all networks that start from this genesis.
            // The storage default is `false` (safe for bare upgrades); genesis
            // explicitly opts new chains in.
            LimitOrdersEnabled::<T>::set(true);
        }
    }

    // ── Hooks ─────────────────────────────────────────────────────────────────

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_runtime_upgrade() -> frame_support::weights::Weight {
            let mut weight = frame_support::weights::Weight::from_parts(0, 0);

            weight = weight.saturating_add(migrations::migrate_register_pallet_hotkey::<T>());

            weight
        }
    }

    // ── Extrinsics ────────────────────────────────────────────────────────────

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Execute a batch of signed limit orders. Admin-gated.
        ///
        /// The `should_fail` flag controls how individual order failures are
        /// handled:
        ///
        /// - When `false` (best-effort): orders whose price condition is not yet
        ///   met are silently skipped so that a single stale order cannot block
        ///   the rest of the batch. Orders that fail for any other reason
        ///   (expired, bad signature, etc.) are also skipped; the admin is
        ///   expected to filter these off-chain.
        /// - When `true` (all-or-nothing): the first order failure aborts the
        ///   whole batch by returning the underlying error, reverting any orders
        ///   already executed in this call.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::execute_orders(orders.len() as u32))]
        pub fn execute_orders(
            origin: OriginFor<T>,
            orders: BoundedVec<SignedOrder<T::AccountId>, T::MaxOrdersPerBatch>,
            should_fail: bool,
        ) -> DispatchResult {
            let relayer = ensure_signed(origin)?;
            ensure!(
                LimitOrdersEnabled::<T>::get(),
                Error::<T>::LimitOrdersDisabled
            );

            for signed_order in orders {
                let order_id = Self::derive_order_id(&signed_order.order);
                if let Err(reason) = Self::try_execute_order(signed_order, order_id, &relayer) {
                    if should_fail {
                        // All-or-nothing: abort the batch, reverting prior orders.
                        return Err(reason);
                    }
                    // Best-effort: individual order failures do not revert the batch.
                    Self::deposit_event(Event::OrderSkipped { order_id, reason });
                }
            }

            Ok(())
        }

        /// Execute a batch of signed limit orders for a single subnet using
        /// aggregated (netted) pool interaction.
        ///
        /// Unlike `execute_orders`, which hits the pool once per order, this
        /// extrinsic:
        ///
        /// 1. Validates all orders (bad signature / expired / already processed /
        ///    price-not-met orders are skipped and emit `OrderSkipped`).
        /// 2. Fetches the current price once.
        /// 3. Aggregates all valid buy inputs (TAO) and sell inputs (alpha).
        /// 4. Nets the two sides: only the residual amount touches the pool in
        ///    a single swap, minimising price impact.
        /// 5. Distributes outputs pro-rata:
        ///    - Dominant-side orders split the pool output proportionally to
        ///      their individual net amounts.
        ///    - Offset-side orders are filled internally at the current price
        ///      (no pool interaction for them).
        /// 6. Collects protocol fees (TAO for buy orders, alpha → TAO for sell
        ///    orders) and routes them to `FeeCollector`.
        ///
        /// All orders in the batch must target `netuid`. Orders for a different
        /// subnet are skipped.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::execute_batched_orders(orders.len() as u32))]
        pub fn execute_batched_orders(
            origin: OriginFor<T>,
            netuid: NetUid,
            orders: BoundedVec<SignedOrder<T::AccountId>, T::MaxOrdersPerBatch>,
        ) -> DispatchResult {
            let relayer = ensure_signed(origin)?;
            ensure!(
                LimitOrdersEnabled::<T>::get(),
                Error::<T>::LimitOrdersDisabled
            );

            Self::do_execute_batched_orders(netuid, orders, relayer)
        }

        /// Register a cancellation intent for an order.
        ///
        /// Must be called by the order's signer. The full `Order` payload is
        /// provided so the pallet can derive the `OrderId`. Once marked
        /// Cancelled, the order can never be executed.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::cancel_order())]
        pub fn cancel_order(
            origin: OriginFor<T>,
            order: VersionedOrder<T::AccountId>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(order.inner().signer == who, Error::<T>::Unauthorized);

            let order_id = Self::derive_order_id(&order);

            ensure!(
                Orders::<T>::get(order_id).is_none(),
                Error::<T>::OrderAlreadyProcessed
            );

            Orders::<T>::insert(order_id, OrderStatus::Cancelled);
            Self::deposit_event(Event::OrderCancelled {
                order_id,
                signer: who,
            });

            Ok(())
        }

        /// Set a status for the limit orders pallet
        ///
        /// Must be called by root
        /// It allows disabling or enabling the pallet
        /// true means enabling, false means disabling
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::set_pallet_status())]
        pub fn set_pallet_status(origin: OriginFor<T>, enabled: bool) -> DispatchResult {
            ensure_root(origin)?;

            if enabled {
                ensure!(
                    T::SwapInterface::pallet_hotkey_registered(
                        &Self::pallet_account(),
                        &T::PalletHotkey::get(),
                    ),
                    Error::<T>::PalletHotkeyNotRegistered
                );
            }

            LimitOrdersEnabled::<T>::set(enabled);

            Self::deposit_event(Event::LimitOrdersPalletStatusChanged { enabled });

            Ok(())
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    impl<T: Config> Pallet<T> {
        /// Compute the effective price limit passed to the pool swap.
        ///
        /// - `None` slippage → no constraint: `u64::MAX` for buys (no ceiling),
        ///   `0` for sells (no floor).
        /// - `Some(p)` → widens `limit_price` by the slippage fraction:
        ///   - Buy:  ceiling = `limit_price + limit_price * p`  (saturating)
        ///   - Sell: floor   = `limit_price - limit_price * p`  (saturating)
        pub(crate) fn compute_effective_swap_limit(
            is_buy: bool,
            limit_price: u64,
            max_slippage: Option<Perbill>,
        ) -> u64 {
            match max_slippage {
                None => {
                    if is_buy {
                        u64::MAX
                    } else {
                        0
                    }
                }
                Some(slippage) => {
                    let delta = slippage.mul_floor(limit_price);
                    if is_buy {
                        limit_price.saturating_add(delta)
                    } else {
                        limit_price.saturating_sub(delta)
                    }
                }
            }
        }

        /// Derive the on-chain `OrderId` as blake2_256 over the SCALE-encoded order.
        pub fn derive_order_id(order: &VersionedOrder<T::AccountId>) -> H256 {
            H256(sp_core::hashing::blake2_256(&order.encode()))
        }

        /// Account derived from the pallet's `PalletId`.
        pub(crate) fn pallet_account() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        /// Transfer `fee_tao` from `signer` to `recipient`.
        /// Returns an error if the transfer fails, causing the surrounding operation to revert.
        /// Does nothing when `fee_tao` is zero.
        fn forward_fee(
            signer: &T::AccountId,
            recipient: &T::AccountId,
            fee_tao: TaoBalance,
        ) -> DispatchResult {
            if fee_tao.is_zero() {
                return Ok(());
            }
            T::SwapInterface::transfer_tao(signer, recipient, fee_tao)
        }

        /// Validates all execution preconditions for a signed order.
        /// Checks that the order's netuid is not root (0), that the signature is valid,
        /// the order has not been processed, is not expired, and the price condition is met.
        /// The batch netuid match (order.netuid == batch netuid) is checked separately by callers.
        pub(crate) fn is_order_valid(
            signed_order: &SignedOrder<T::AccountId>,
            order_id: H256,
            now_ms: u64,
            current_price: U64F64,
            relayer: &T::AccountId,
        ) -> DispatchResult {
            let order = signed_order.order.inner();
            ensure!(!order.netuid.is_root(), Error::<T>::RootNetUidNotAllowed);
            ensure!(
                order.chain_id == T::ChainId::get(),
                Error::<T>::ChainIdMismatch
            );
            ensure!(
                matches!(signed_order.signature, MultiSignature::Sr25519(_))
                    && signed_order
                        .signature
                        .verify(signed_order.order.encode().as_slice(), &order.signer),
                Error::<T>::InvalidSignature
            );
            let order_status = Orders::<T>::get(order_id);
            ensure!(
                order_status != Some(OrderStatus::Fulfilled),
                Error::<T>::OrderAlreadyProcessed
            );
            ensure!(
                order_status != Some(OrderStatus::Cancelled),
                Error::<T>::OrderCancelled
            );
            ensure!(now_ms <= order.expiry, Error::<T>::OrderExpired);
            // Scale current_price to ×10⁹ to match the limit_price field, which is
            // expressed in the same ×10⁹ scale as the `current_alpha_price` RPC endpoint.
            // This allows sub-unity prices (e.g. 0.5 TAO/alpha = 500_000_000) to be
            // represented and compared correctly.
            let scaled_price = current_price
                .saturating_mul(U64F64::from_num(1_000_000_000u64))
                .saturating_to_num::<u64>();
            ensure!(
                match order.order_type {
                    OrderType::TakeProfit => scaled_price >= order.limit_price,
                    OrderType::StopLoss | OrderType::LimitBuy => scaled_price <= order.limit_price,
                },
                Error::<T>::PriceConditionNotMet
            );
            if let Some(forced_relayers) = order.relayer.as_ref() {
                ensure!(
                    forced_relayers.contains(relayer),
                    Error::<T>::RelayerMissMatch
                );
            }
            if let Some(partial_fill) = signed_order.partial_fill {
                ensure!(
                    order.relayer.is_some(),
                    Error::<T>::RelayerRequiredForPartialFill
                );
                ensure!(
                    order.partial_fills_enabled,
                    Error::<T>::PartialFillsNotEnabled
                );
                let max_fill =
                    if let Some(OrderStatus::PartiallyFilled(already_filled)) = order_status {
                        order.amount.saturating_sub(already_filled)
                    } else {
                        order.amount
                    };
                ensure!(
                    partial_fill > 0 && partial_fill <= max_fill,
                    Error::<T>::IncorrectPartialFillAmount
                );
            } else {
                // `partial_fill == None` means a one-shot full execution of `order.amount`
                // (see `compute_order_status`, which returns `Fulfilled` for `None`). This is
                // only valid for an order that has not been filled yet. Against an order that is
                // already `PartiallyFilled(n)` the cumulative-fill cap above is skipped, so the
                // execution path would re-run the full `order.amount` on top of the `n` already
                // filled — over-debiting the signer and breaking the `sum(fills) <= order.amount`
                // conservation invariant. Reject it: the remaining amount must be completed with an
                // explicit `partial_fill = Some(order.amount - n)`.
                ensure!(
                    !matches!(order_status, Some(OrderStatus::PartiallyFilled(_))),
                    Error::<T>::IncorrectPartialFillAmount
                );
            }
            Ok(())
        }

        /// Compute the new `OrderStatus` to write after filling `fill_amount` of an order.
        ///
        /// Reads the current on-chain status to find any already-filled amount, adds
        /// `fill_amount`, and returns `Fulfilled` when the total reaches `order_amount`.
        /// Pass `None` for `fill_amount` when the order is being fully executed in one shot.
        pub(crate) fn compute_order_status(
            order_id: H256,
            fill_amount: Option<u64>,
            order_amount: u64,
        ) -> OrderStatus {
            let Some(fill) = fill_amount else {
                return OrderStatus::Fulfilled;
            };
            let already_filled =
                if let Some(OrderStatus::PartiallyFilled(n)) = Orders::<T>::get(order_id) {
                    n
                } else {
                    0
                };
            let new_total = already_filled.saturating_add(fill);
            if new_total >= order_amount {
                OrderStatus::Fulfilled
            } else {
                OrderStatus::PartiallyFilled(new_total)
            }
        }

        /// Attempt to execute one signed order. Returns an error on any
        /// validation or execution failure without panicking.
        ///
        /// `#[transactional]` makes the whole body a single storage layer: the
        /// swap (`buy_alpha`/`sell_alpha`, themselves transactional), the fee
        /// transfer, and the `Orders::insert` either all commit together or all
        /// roll back together.
        #[transactional]
        fn try_execute_order(
            signed_order: SignedOrder<T::AccountId>,
            order_id: H256,
            relayer: &T::AccountId,
        ) -> DispatchResult {
            let order = signed_order.order.inner();
            let now_ms = T::TimeProvider::now().as_millis() as u64;
            let current_price = T::SwapInterface::current_alpha_price(order.netuid);

            Self::is_order_valid(&signed_order, order_id, now_ms, current_price, relayer)?;

            let effective_swap_limit = Self::compute_effective_swap_limit(
                order.order_type.is_buy(),
                order.limit_price,
                order.max_slippage,
            );

            // Execute the swap, taking the order's fee from the input (buys) or output (sells).
            // `effective_swap_limit` enforces slippage protection: for buys it caps the price
            // ceiling; for sells it sets a minimum floor.  When `max_slippage` is None the
            // limit is u64::MAX (buys) or 0 (sells), matching previous market-order behaviour.
            let (amount_in, amount_out) = if order.order_type.is_buy() {
                // partial fill validations have passed, it is safe here to do this
                let tao_in = TaoBalance::from(signed_order.partial_fill.unwrap_or(order.amount));
                // Deduct fee from TAO input before swapping.
                let fee_tao = TaoBalance::from(order.fee_rate.mul_floor(tao_in.to_u64()));
                let tao_after_fee = tao_in.saturating_sub(fee_tao);

                let alpha_out = T::SwapInterface::buy_alpha(
                    &order.signer,
                    &order.hotkey,
                    order.netuid,
                    tao_after_fee,
                    TaoBalance::from(effective_swap_limit),
                    true,
                )?;

                // Forward the fee TAO to the order's fee recipient.
                Self::forward_fee(&order.signer, &order.fee_recipient, fee_tao)?;
                (tao_after_fee.to_u64(), alpha_out.to_u64())
            } else {
                // partial fill validations have passed, it is safe here to do this
                let alpha_in =
                    AlphaBalance::from(signed_order.partial_fill.unwrap_or(order.amount));

                // Sell the full alpha amount; fee is taken from the TAO output.
                let tao_out = T::SwapInterface::sell_alpha(
                    &order.signer,
                    &order.hotkey,
                    order.netuid,
                    alpha_in,
                    TaoBalance::from(effective_swap_limit),
                    true,
                )?;

                // Deduct fee from TAO output and forward to the order's fee recipient.
                let fee_tao = TaoBalance::from(order.fee_rate.mul_floor(tao_out.to_u64()));
                Self::forward_fee(&order.signer, &order.fee_recipient, fee_tao)?;
                (alpha_in.to_u64(), tao_out.saturating_sub(fee_tao).to_u64())
            };

            // Mark as fulfilled or partially filled and emit event.
            let status =
                Self::compute_order_status(order_id, signed_order.partial_fill, order.amount);
            Orders::<T>::insert(order_id, status);
            Self::deposit_event(Event::OrderExecuted {
                order_id,
                signer: order.signer.clone(),
                netuid: order.netuid,
                order_type: order.order_type.clone(),
                amount_in,
                amount_out,
            });

            Ok(())
        }

        /// Thin orchestrator for `execute_batched_orders`.
        ///
        /// All-or-nothing: any `Err` returned here (e.g. a `ZeroShareInBatch` rejection
        /// during distribution) rolls back the whole batch — including the up-front
        /// `collect_assets` debits and the pool swap — via FRAME's default per-dispatch
        /// storage layer, so no signer is left debited without receiving output.
        fn do_execute_batched_orders(
            netuid: NetUid,
            orders: BoundedVec<SignedOrder<T::AccountId>, T::MaxOrdersPerBatch>,
            relayer: T::AccountId,
        ) -> DispatchResult {
            ensure!(!netuid.is_root(), Error::<T>::RootNetUidNotAllowed);

            let now_ms = T::TimeProvider::now().as_millis() as u64;
            let current_price = T::SwapInterface::current_alpha_price(netuid);

            // Validate all orders; any invalid order causes the entire batch to fail.
            let (valid_buys, valid_sells) =
                Self::validate_and_classify(netuid, &orders, now_ms, current_price, relayer)?;

            let executed_count = valid_buys.len().saturating_add(valid_sells.len()) as u32;
            if executed_count == 0 {
                return Ok(());
            }

            let total_buy_net: u128 = valid_buys.iter().map(|e| e.net as u128).sum();
            let total_sell_net: u128 = valid_sells.iter().map(|e| e.net as u128).sum();
            let total_sell_tao_equiv: u128 = Self::alpha_to_tao(total_sell_net, current_price);

            let pallet_acct = Self::pallet_account();
            let pallet_hotkey = T::PalletHotkey::get();

            // Pull all input assets into the pallet intermediary before touching the pool.
            Self::collect_assets(
                &valid_buys,
                &valid_sells,
                &pallet_acct,
                &pallet_hotkey,
                netuid,
            )?;

            // Derive the tightest slippage constraint from the dominant side:
            // buy-dominant → min of all buy ceilings; sell-dominant → max of all sell floors.
            let pool_price_limit = if total_buy_net >= total_sell_tao_equiv {
                valid_buys
                    .iter()
                    .map(|e| e.effective_swap_limit)
                    .min()
                    .unwrap_or(u64::MAX)
            } else {
                valid_sells
                    .iter()
                    .map(|e| e.effective_swap_limit)
                    .max()
                    .unwrap_or(0)
            };

            // Execute a single pool swap for the residual (buy TAO minus sell TAO-equiv, or vice versa).
            let (net_side, actual_out) = Self::net_pool_swap(
                total_buy_net,
                total_sell_net,
                total_sell_tao_equiv,
                current_price,
                &pallet_acct,
                &pallet_hotkey,
                netuid,
                pool_price_limit,
            )?;

            // Give every buyer their pro-rata share of (pool alpha output + offset sell alpha).
            Self::distribute_alpha_pro_rata(
                &valid_buys,
                actual_out,
                total_buy_net,
                total_sell_net,
                &net_side,
                current_price,
                &pallet_acct,
                &pallet_hotkey,
                netuid,
            )?;

            // Give every seller their pro-rata share of (pool TAO output + offset buy TAO),
            // deducting per-order fees from each payout; returns accumulated sell fees by recipient.
            let sell_fees = Self::distribute_tao_pro_rata(
                &valid_sells,
                actual_out,
                total_buy_net,
                total_sell_tao_equiv,
                &net_side,
                current_price,
                &pallet_acct,
                netuid,
            )?;

            // Merge buy and sell fees by recipient and transfer once per unique recipient.
            Self::collect_fees(&valid_buys, sell_fees, &pallet_acct)?;

            let net_amount = Self::net_amount_for_event(
                &net_side,
                total_buy_net,
                total_sell_net,
                total_sell_tao_equiv,
                current_price,
            )?;
            Self::deposit_event(Event::GroupExecutionSummary {
                netuid,
                net_side,
                net_amount,
                actual_out: actual_out as u64,
                executed_count,
            });

            Ok(())
        }

        /// Validate every order against `netuid`, signature, expiry, and price.
        /// Valid orders are split into two BoundedVecs by side.
        /// Each entry is `(order_id, signer, hotkey, gross, net, fee)`.
        ///
        /// Returns an error immediately if any order fails validation (wrong netuid,
        /// invalid signature, expired, already processed, or price condition not met).
        pub(crate) fn validate_and_classify(
            netuid: NetUid,
            orders: &BoundedVec<SignedOrder<T::AccountId>, T::MaxOrdersPerBatch>,
            now_ms: u64,
            current_price: U64F64,
            relayer: T::AccountId,
        ) -> Result<
            (
                BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
                BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
            ),
            DispatchError,
        > {
            let mut buys = BoundedVec::new();
            let mut sells = BoundedVec::new();

            // Track which order_ids we have already seen in this batch. A repeated
            // order_id is never legitimate within a single batch.
            let mut seen_order_ids: BTreeSet<H256> = BTreeSet::new();

            for signed_order in orders.iter() {
                let order_id = Self::derive_order_id(&signed_order.order);

                // Hard-fail on the first duplicate order_id in the batch (covers both
                // buys and sells). BTreeSet::insert returns false if already present.
                ensure!(
                    seen_order_ids.insert(order_id),
                    Error::<T>::DuplicateOrderInBatch
                );

                let order = signed_order.order.inner();

                // Hard-fail if the order targets a different subnet than the batch netuid.
                ensure!(order.netuid == netuid, Error::<T>::OrderNetUidMismatch);

                // Hard-fail on any per-order validation error (signature, expiry, price, root).
                Self::is_order_valid(signed_order, order_id, now_ms, current_price, &relayer)?;

                let amount_in = signed_order.partial_fill.unwrap_or(order.amount);
                let net = if order.order_type.is_buy() {
                    // Buy: fee on TAO input — net is the amount that reaches the pool.
                    amount_in.saturating_sub(order.fee_rate.mul_floor(amount_in))
                } else {
                    // Sell: fee on TAO output — full alpha enters the pool; the fee is
                    // deducted from the TAO payout later in `distribute_tao_pro_rata`.
                    amount_in
                };

                let effective_swap_limit = Self::compute_effective_swap_limit(
                    order.order_type.is_buy(),
                    order.limit_price,
                    order.max_slippage,
                );

                let entry = OrderEntry {
                    order_id,
                    signer: order.signer.clone(),
                    hotkey: order.hotkey.clone(),
                    side: order.order_type.clone(),
                    gross: amount_in,
                    order_amount: order.amount,
                    net,
                    fee_rate: order.fee_rate,
                    fee_recipient: order.fee_recipient.clone(),
                    effective_swap_limit,
                    partial_fill: signed_order.partial_fill,
                };

                // try_push cannot fail: both vecs share the same bound as `orders`.
                if entry.side.is_buy() {
                    let _ = buys.try_push(entry);
                } else {
                    let _ = sells.try_push(entry);
                }
            }

            Ok((buys, sells))
        }

        /// Pull gross TAO from each buyer and gross staked alpha from each seller
        /// into the pallet intermediary account, bypassing the pool.
        fn collect_assets(
            buys: &BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
            sells: &BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
            pallet_acct: &T::AccountId,
            pallet_hotkey: &T::AccountId,
            netuid: NetUid,
        ) -> DispatchResult {
            for e in buys.iter() {
                T::SwapInterface::transfer_tao(&e.signer, pallet_acct, TaoBalance::from(e.gross))?;
            }
            for e in sells.iter() {
                T::SwapInterface::transfer_staked_alpha(
                    &e.signer,
                    &e.hotkey,
                    pallet_acct,
                    pallet_hotkey,
                    netuid,
                    AlphaBalance::from(e.gross),
                    true,  // validate_sender: check user's rate limit, subnet, min stake
                    false, // set_receiver_limit: do not rate-limit the pallet intermediary
                )?;
            }
            Ok(())
        }

        /// Execute a single pool swap for the net (residual) amount.
        /// Returns `(net_side, actual_out)` where `actual_out` is in the output
        /// token units (alpha for Buy, TAO for Sell).
        ///
        /// `price_limit` encodes the tightest slippage constraint across all dominant-side
        /// orders: a ceiling for buy-dominant swaps, a floor for sell-dominant swaps.
        #[allow(clippy::too_many_arguments)]
        fn net_pool_swap(
            total_buy_net: u128,
            total_sell_net: u128,
            total_sell_tao_equiv: u128,
            current_price: U64F64,
            pallet_acct: &T::AccountId,
            pallet_hotkey: &T::AccountId,
            netuid: NetUid,
            price_limit: u64,
        ) -> Result<(OrderSide, u128), DispatchError> {
            if total_buy_net >= total_sell_tao_equiv {
                let net_tao = (total_buy_net.saturating_sub(total_sell_tao_equiv)) as u64;
                let actual_alpha = if net_tao > 0 {
                    let out = T::SwapInterface::buy_alpha(
                        pallet_acct,
                        pallet_hotkey,
                        netuid,
                        TaoBalance::from(net_tao),
                        TaoBalance::from(price_limit),
                        false,
                    )?
                    .to_u64() as u128;
                    ensure!(out > 0, Error::<T>::SwapReturnedZero);
                    out
                } else {
                    0u128
                };
                Ok((OrderSide::Buy, actual_alpha))
            } else {
                let total_buy_alpha_equiv = Self::tao_to_alpha(total_buy_net, current_price)?;
                let net_alpha = (total_sell_net.saturating_sub(total_buy_alpha_equiv)) as u64;
                let actual_tao = if net_alpha > 0 {
                    let out = T::SwapInterface::sell_alpha(
                        pallet_acct,
                        pallet_hotkey,
                        netuid,
                        AlphaBalance::from(net_alpha),
                        TaoBalance::from(price_limit),
                        false,
                    )?
                    .to_u64() as u128;
                    ensure!(out > 0, Error::<T>::SwapReturnedZero);
                    out
                } else {
                    0u128
                };
                Ok((OrderSide::Sell, actual_tao))
            }
        }

        /// Distribute alpha pro-rata to ALL buyers and mark their orders fulfilled.
        ///
        /// - Buy-dominant: total alpha = pool output + sell-side alpha (passed through).
        /// - Sell-dominant: total alpha = buy-side TAO converted at `current_price`.
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn distribute_alpha_pro_rata(
            buys: &BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
            actual_out: u128,
            total_buy_net: u128,
            total_sell_net: u128,
            net_side: &OrderSide,
            current_price: U64F64,
            pallet_acct: &T::AccountId,
            pallet_hotkey: &T::AccountId,
            netuid: NetUid,
        ) -> DispatchResult {
            let total_alpha: u128 = match net_side {
                OrderSide::Buy => actual_out.saturating_add(total_sell_net),
                OrderSide::Sell => Self::tao_to_alpha(total_buy_net, current_price)?,
            };

            for e in buys.iter() {
                let share: u64 = if total_buy_net > 0 {
                    total_alpha
                        .saturating_mul(e.net as u128)
                        .checked_div(total_buy_net)
                        .unwrap_or(0) as u64
                } else {
                    0
                };
                // A floored-to-zero share means this buyer's input was already collected
                // by `collect_assets` but the pool/offset alpha cannot pay them even one
                // unit. Hard-fail the whole batch (FRAME's per-dispatch storage layer rolls
                // back `collect_assets` and the pool swap) rather than silently skipping the
                // transfer while still marking the order terminal — which would consume the
                // signer's TAO for zero alpha and permanently close the order.
                ensure!(share > 0, Error::<T>::ZeroShareInBatch);
                T::SwapInterface::transfer_staked_alpha(
                    pallet_acct,
                    pallet_hotkey,
                    &e.signer,
                    &e.hotkey,
                    netuid,
                    AlphaBalance::from(share),
                    false, // validate_sender: skip — pallet intermediary needs no validation
                    true,  // set_receiver_limit: rate-limit the buyer after they receive stake
                )?;
                let status = Self::compute_order_status(e.order_id, e.partial_fill, e.order_amount);
                Orders::<T>::insert(e.order_id, status);
                Self::deposit_event(Event::OrderExecuted {
                    order_id: e.order_id,
                    signer: e.signer.clone(),
                    netuid,
                    order_type: e.side.clone(),
                    amount_in: e.gross,
                    amount_out: share,
                });
            }
            Ok(())
        }

        /// Distribute TAO pro-rata to ALL sellers and mark their orders fulfilled.
        ///
        /// - Sell-dominant: total TAO = pool output + buy-side TAO (passed through).
        /// - Buy-dominant: each seller receives their alpha valued at `current_price`.
        ///
        /// Fee on TAO output: `ppb(share)` is withheld from each seller's payout and
        /// left in the pallet account. Returns the total sell-side fee TAO accumulated.
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn distribute_tao_pro_rata(
            sells: &BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
            actual_out: u128,
            total_buy_net: u128,
            total_sell_tao_equiv: u128,
            net_side: &OrderSide,
            current_price: U64F64,
            pallet_acct: &T::AccountId,
            netuid: NetUid,
        ) -> Result<Vec<(T::AccountId, u64)>, DispatchError> {
            let total_tao: u128 = match net_side {
                OrderSide::Sell => actual_out.saturating_add(total_buy_net),
                OrderSide::Buy => total_sell_tao_equiv,
            };

            // Accumulate sell-side fees by recipient (one entry per unique recipient).
            let mut sell_fees: Vec<(T::AccountId, u64)> = Vec::new();

            for e in sells.iter() {
                let sell_tao_equiv = Self::alpha_to_tao(e.net as u128, current_price);
                let gross_share: u64 = if total_sell_tao_equiv > 0 {
                    total_tao
                        .saturating_mul(sell_tao_equiv)
                        .checked_div(total_sell_tao_equiv)
                        .unwrap_or(0) as u64
                } else {
                    0u64
                };
                let fee = e.fee_rate.mul_floor(gross_share);
                let net_share = gross_share.saturating_sub(fee);

                // A floored-to-zero payout means this seller's alpha was already collected
                // by `collect_assets` but the pool/offset TAO cannot pay them even one unit
                // (after fee). Hard-fail the whole batch (rolled back by the dispatch storage
                // layer) rather than no-op the transfer while still marking the order terminal,
                // which would consume the seller's alpha for zero TAO and close the order.
                ensure!(net_share > 0, Error::<T>::ZeroShareInBatch);

                if fee > 0 {
                    if let Some(entry) = sell_fees.iter_mut().find(|(r, _)| r == &e.fee_recipient) {
                        entry.1 = entry.1.saturating_add(fee);
                    } else {
                        sell_fees.push((e.fee_recipient.clone(), fee));
                    }
                }

                T::SwapInterface::transfer_tao(
                    pallet_acct,
                    &e.signer,
                    TaoBalance::from(net_share),
                )?;
                let status = Self::compute_order_status(e.order_id, e.partial_fill, e.order_amount);
                Orders::<T>::insert(e.order_id, status);
                Self::deposit_event(Event::OrderExecuted {
                    order_id: e.order_id,
                    signer: e.signer.clone(),
                    netuid,
                    order_type: e.side.clone(),
                    amount_in: e.gross,
                    amount_out: net_share,
                });
            }
            Ok(sell_fees)
        }

        /// Forward accumulated fees to their respective recipients.
        ///
        /// Merges buy-side fees (withheld from TAO input) and sell-side fees
        /// (withheld from TAO output, passed in as `sell_fees`) by recipient,
        /// then performs one TAO transfer per unique `fee_recipient`.
        /// All transfers are best-effort and do not revert the batch on failure.
        pub(crate) fn collect_fees(
            buys: &BoundedVec<OrderEntry<T::AccountId>, T::MaxOrdersPerBatch>,
            sell_fees: Vec<(T::AccountId, u64)>,
            pallet_acct: &T::AccountId,
        ) -> DispatchResult {
            // Start with sell fees; fold in buy fees.
            // Buy fee was already computed in `validate_and_classify` as `gross - net`,
            // so we recover it here without recomputing.
            let mut fees: Vec<(T::AccountId, u64)> = sell_fees;
            for e in buys.iter() {
                let fee = e.gross.saturating_sub(e.net);
                if fee > 0 {
                    if let Some(entry) = fees.iter_mut().find(|(r, _)| r == &e.fee_recipient) {
                        entry.1 = entry.1.saturating_add(fee);
                    } else {
                        fees.push((e.fee_recipient.clone(), fee));
                    }
                }
            }

            // One transfer per unique fee recipient.
            for (recipient, amount) in fees {
                Self::forward_fee(pallet_acct, &recipient, TaoBalance::from(amount))?;
            }

            // TODO: sweep rounding dust and any emissions accrued on the pallet account.
            // Pro-rata integer division leaves small alpha residuals in (pallet_account,
            // pallet_hotkey) after each batch.  Over time these accumulate and, if an
            // emission epoch fires while the dust is present, the pallet earns emissions
            // it never distributes.  Fix: add `staked_alpha(coldkey, hotkey, netuid) ->
            // AlphaBalance` to `OrderSwapInterface`, then sell the full remaining balance
            // here and forward the TAO to `FeeCollector`.
            Ok(())
        }

        /// Compute the net amount field for the `GroupExecutionSummary` event.
        pub(crate) fn net_amount_for_event(
            net_side: &OrderSide,
            total_buy_net: u128,
            total_sell_net: u128,
            total_sell_tao_equiv: u128,
            current_price: U64F64,
        ) -> Result<u64, DispatchError> {
            match net_side {
                OrderSide::Buy => Ok((total_buy_net.saturating_sub(total_sell_tao_equiv)) as u64),
                OrderSide::Sell => {
                    let buy_alpha_equiv = Self::tao_to_alpha(total_buy_net, current_price)? as u64;
                    Ok((total_sell_net as u64).saturating_sub(buy_alpha_equiv))
                }
            }
        }

        /// Convert a TAO amount to alpha at `price` (TAO/alpha).
        ///
        /// A zero `price` yields `Ok(0)` (no alpha is purchasable). A genuine
        /// fixed-point overflow returns `Err(ArithmeticOverflow)` so the caller
        /// aborts the batch.
        fn tao_to_alpha(tao: u128, price: U64F64) -> Result<u128, DispatchError> {
            if price == U64F64::from_num(0u32) {
                return Ok(0u128);
            }
            U64F64::saturating_from_num(tao)
                .checked_div(price)
                .map(|alpha| alpha.saturating_to_num::<u128>())
                .ok_or(Error::<T>::ArithmeticOverflow.into())
        }

        /// Convert an alpha amount to TAO at `price` (TAO/alpha).
        fn alpha_to_tao(alpha: u128, price: U64F64) -> u128 {
            price
                .saturating_mul(U64F64::saturating_from_num(alpha))
                .saturating_to_num::<u128>()
        }
    }
}
