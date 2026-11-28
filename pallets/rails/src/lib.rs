//! # Rails
//!
//! The hub of the canonical rails. Portals on other chains ("spokes") lock a stablecoin or
//! burn a canonical token and send an [`Order`]; the hub settles it along the one route
//!
//! ```text
//! stablecoin <-> tUSD <-> TAO <-> subnet alpha -> service credits
//! ```
//!
//! and answers with a [`Message`] that tells the spoke what to pay out. A dollar reaches
//! alpha only through TAO, and TAO only through the canonical pool.
//!
//! ## What an order can buy
//!
//! - **Canonical TAO** on the spoke, backed one for one by TAO in [`Pallet::reserve_account`].
//! - **A subnet's alpha token** on the spoke: shares of that subnet's vault, which stakes at
//!   the subnet's index hotkey. A share is worth more alpha as the stake earns; the number
//!   of tokens never changes (see [`router`]).
//! - **Service credits**: the alpha is burned and its gamma spent at a service for an
//!   address, at once. Nothing is left to redeem.
//! - **The stablecoin back**, by selling a canonical token.
//! - **TAO or tUSD at an account on Bittensor.**
//!
//! Accounts on Bittensor pay with TAO or tUSD through [`Pallet::bridge_out`].
//!
//! ## How an order is settled
//!
//! The gateway contract on Bittensor's EVM is the only caller of [`Pallet::execute`]. It
//! is called by the transport after the transport's security module accepted a message
//! from a portal. The hub then:
//!
//! 1. finds the bridge of the portal, and refuses a portal it does not know;
//! 2. takes the order only if its nonce is the next of that bridge: each order settles
//!    once, in sequence;
//! 3. runs the route in a storage layer. If any step fails (slippage, a cap, a deadline, a
//!    closed market), everything is rolled back and the spoke is told to return to the
//!    payer exactly what it took;
//! 4. records a [`Receipt`].
//!
//! Only a message that cannot be taken at all makes `execute` fail: then the transport
//! tries again later, and what the payer locked stays locked on the spoke.
//!
//! ## How an answer leaves, and who pays for it
//!
//! The hub cannot call a contract. It puts each message for a spoke in that bridge's
//! queue ([`Outbox`]). Settling an order and handing over its answer are two steps: the
//! gateway takes messages out when anyone asks it to, and hands them to the transport.
//! So nothing that happens on the way out can undo an order.
//!
//! Each bridge has its own queue, and the gateway takes its messages in order. A message
//! that cannot leave now, because the transport charges more for it than is paid, is
//! deferred ([`Pallet::defer_outbound`]): it leaves the line and stays in the hub, with
//! its reserve. Anyone can have the gateway send it later, and the messages behind it go
//! on. The spoke takes the hub's messages in any order.
//!
//! A deferred message takes no place in the queue ([`Config::MaxOutbox`]). It holds its
//! reserve, as every message does that has not left, and that bounds their number.
//!
//! A message that the gateway cannot hand over, because the transport refuses it, stays
//! at the head of its queue. When it has been there for [`Config::StaleAfter`] blocks,
//! anyone can defer it, so that the messages behind it do not wait for it for ever.
//!
//! ## Order slots: what a spoke may send
//!
//! A spoke must not take a payer's funds for an order that the hub cannot answer. So the
//! hub gives the portal of each bridge order slots. A slot is the hub's promise to answer
//! one order: the hub has set aside a place in the bridge's queue and a reserve in the
//! bridge's account for that answer. The portal takes an order only if it holds a slot,
//! and each order uses one.
//!
//! Slots go to the portal with the hub's messages. Each payment that the hub queues for
//! a portal takes along the slots that the portal is missing ([`MaxOrderSlots`]), as far
//! as the queue and the account have room for them. When a portal is short of slots and
//! no payment is on its way, anyone can have the hub send slots alone
//! ([`Pallet::grant_slots`]). The hub counts the slots that are out ([`OrderSlots`]): at
//! the portal, on their way to it, or used by an order that the hub has not taken yet.
//!
//! So every order that a portal sends has its answer paid for, whether the hub settles
//! the order or refunds it. If the hub's messages do not leave, or nobody pays into the
//! bridge's account, the portal runs out of slots and takes no more orders. Its payers
//! keep their funds.
//!
//! The transport charges the gateway for each message. The TAO for that is the hub's, not
//! the gateway's. Each bridge has an account ([`Pallet::message_account`]) that anyone
//! fills ([`Pallet::fund_messages`]), and the messages to a bridge's spoke are paid for
//! from its account alone:
//!
//! - The admin sets two amounts for the bridge. The reserve ([`MessageReserve`]) is what
//!   the hub sets aside for each message. The ceiling ([`MessageCeiling`]) is the most it
//!   pays for one. Until they are set the bridge takes no order.
//! - The account always holds the reserve for every message of the bridge that has not
//!   left: in line or deferred. The hub queues a message only if that still holds with
//!   the new message. With less, the hub does not take the bridge's next order: it stays
//!   with the transport.
//! - An account that pays through [`Pallet::bridge_out`] first pays the reserve into the
//!   bridge's account itself.
//! - When the gateway takes a message out, it says what the transport charges for it. The
//!   hub moves that much from the bridge's account to the gateway
//!   ([`Pallet::ack_outbound`]), which pays the transport in the same transaction. Up to
//!   the reserve, the message has its own TAO. Above the reserve and up to the ceiling,
//!   the hub pays from what the account holds beyond the reserves of the waiting
//!   messages: never from the reserve of another message. What the transport charges
//!   beyond that is for whoever asks the gateway to send the message.
//!
//! So a message that the hub queued can always leave at a charge up to the reserve,
//! whatever the messages before it cost, and messages that a higher charge defers leave
//! as soon as the account holds enough to spare. The gateway holds no TAO between
//! transactions, and the messages of one bridge never use what was paid in for another.
//!
//! ## Invariants
//!
//! 1. [`Pallet::reserve_account`] holds at least the sum of [`TaoSupply`].
//! 2. `Markets(n).shares` equals the sum of `AlphaSupply(n, ·)`.
//! 3. The hub tells a spoke to mint a canonical token only when it added as much to that
//!    spoke's supply, and takes a burned token only up to that supply.
//! 4. An order nonce is settled at most once, and a message nonce is issued at most once.
//! 5. The account of a bridge holds, on top of the existential deposit, at least the
//!    bridge's reserve for each message of the bridge that has not left and for each of
//!    its order slots that are out. TAO leaves that account only when a message of the
//!    bridge leaves the hub: what the gateway says the transport charges, at most the
//!    bridge's ceiling.
//! 6. A message leaves the hub only when the gateway takes it to hand it to the
//!    transport. A deferred message stays in [`Outbox`], and [`Deferred`] counts it.
//! 7. For each order slot that is out, the bridge's queue keeps a place and the bridge's
//!    account holds a reserve, on top of those of the messages that have not left.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod mock;
pub mod router;
pub mod runtime_api;
#[cfg(test)]
mod tests;
pub mod types;
pub mod weights;

use alloc::vec::Vec;
use frame_support::{
    PalletId,
    pallet_prelude::*,
    storage::with_storage_layer,
    traits::{
        EnsureOrigin, UnixTime,
        fungible::{Inspect, Mutate},
        tokens::{Fortitude, Preservation},
    },
};
use frame_system::pallet_prelude::*;
use sp_core::{H160, H256};
use sp_runtime::traits::AccountIdConversion;
pub use subtensor_runtime_common::rails::{RailsStaking, SubnetStatus};
use subtensor_runtime_common::{
    AlphaBalance, GammaBalance, NetUid, TaoBalance, Token,
    rails::{BridgeId, StableAsset},
};

pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

/// The currency the hub moves TAO in: the pool's.
pub type CurrencyOf<T> = <T as pallet_canonical_pool::Config>::Currency;

/// Passes an origin signed by the account in [`Guardian`]. Use it, joined with the admin
/// origin, as the pause origin of this pallet, of the pool and of tUSD.
pub struct EnsureGuardian<T>(PhantomData<T>);

impl<T: Config> EnsureOrigin<T::RuntimeOrigin> for EnsureGuardian<T> {
    type Success = T::AccountId;

    fn try_origin(origin: T::RuntimeOrigin) -> Result<Self::Success, T::RuntimeOrigin> {
        match (origin.clone().into(), Guardian::<T>::get()) {
            (Ok(frame_system::RawOrigin::Signed(who)), Some(guardian)) if who == guardian => {
                Ok(who)
            }
            _ => Err(origin),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<T::RuntimeOrigin, ()> {
        Guardian::<T>::get()
            .map(|guardian| frame_system::RawOrigin::Signed(guardian).into())
            .ok_or(())
    }
}

// The FRAME macros expand to `expect` on infallible codec paths.
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config:
        frame_system::Config + pallet_tusd::Config + pallet_canonical_pool::Config
    {
        /// Staking at index hotkeys and spending gamma, over the subtensor pallet.
        type Staking: RailsStaking<Self::AccountId>;

        /// The clock that order deadlines are read against.
        type Time: UnixTime;

        /// Sets the gateway and the guardian, opens markets, sets caps, lifts a pause.
        type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// May pause the hub, and nothing else.
        type PauseOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Derives the accounts of the hub.
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// The places in the queue of one bridge: for the messages that wait in it, and
        /// for the answers to the orders that the bridge's order slots allow. Deferred
        /// messages do not count: like every message that has not left, each holds its
        /// reserve in the bridge's account, and that is their bound.
        #[pallet::constant]
        type MaxOutbox: Get<u32>;

        /// How many blocks a message stays at the head of a queue before anyone can
        /// defer it. A message that the gateway can hand over leaves long before that.
        #[pallet::constant]
        type StaleAfter: Get<u32>;

        /// How many receipts the hub keeps for each bridge. Older ones are dropped.
        #[pallet::constant]
        type ReceiptsKept: Get<u64>;

        /// Weights of this pallet's calls.
        type WeightInfo: WeightInfo;
    }

    /// ITEM --> account | The account of the gateway contract: the only caller of `execute`
    /// and `ack_outbound`.
    #[pallet::storage]
    pub type Gateway<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// ITEM --> account | The account that may pause the rails and nothing else.
    #[pallet::storage]
    pub type Guardian<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;

    /// ITEM --> bool | While set, the hub takes no order, queues no message and hands none
    /// to the gateway.
    #[pallet::storage]
    pub type Paused<T> = StorageValue<_, bool, ValueQuery>;

    /// MAP ( bridge ) --> nonce | The next order the hub takes from the bridge.
    #[pallet::storage]
    pub type NextOrder<T> = StorageMap<_, Twox64Concat, BridgeId, u64, ValueQuery>;

    /// DMAP ( bridge, nonce ) --> Receipt | What became of the bridge's recent orders.
    #[pallet::storage]
    pub type Receipts<T> =
        StorageDoubleMap<_, Twox64Concat, BridgeId, Twox64Concat, u64, Receipt, OptionQuery>;

    /// MAP ( bridge ) --> nonce | The nonce of the next message to the bridge: the tail of
    /// its queue.
    #[pallet::storage]
    pub type NextMessage<T> = StorageMap<_, Twox64Concat, BridgeId, u64, ValueQuery>;

    /// MAP ( bridge ) --> nonce | The nonce of the bridge's first message that waits in
    /// line: the head of its queue. A message below it that is still in [`Outbox`] is
    /// deferred.
    #[pallet::storage]
    pub type NextOutbound<T> = StorageMap<_, Twox64Concat, BridgeId, u64, ValueQuery>;

    /// DMAP ( bridge, nonce ) --> Message | The messages that have not left: for each
    /// bridge, those in line from the head on, and the deferred ones below the head.
    #[pallet::storage]
    pub type Outbox<T> =
        StorageDoubleMap<_, Twox64Concat, BridgeId, Twox64Concat, u64, Message, OptionQuery>;

    /// MAP ( bridge ) --> count | How many of the bridge's messages are deferred: they
    /// left the line and have not left the hub. Each keeps its reserve in the bridge's
    /// account.
    #[pallet::storage]
    pub type Deferred<T> = StorageMap<_, Twox64Concat, BridgeId, u32, ValueQuery>;

    /// MAP ( bridge ) --> block | The block at which the message at the head of the
    /// bridge's queue became the head.
    #[pallet::storage]
    pub type HeadSince<T> = StorageMap<_, Twox64Concat, BridgeId, u32, ValueQuery>;

    /// MAP ( bridge ) --> count | The order slots of the bridge that are out: the hub gave
    /// them with a message, and no order that the hub took has used them yet. They are at
    /// the portal, on their way to it, or used by an order that is on its way to the hub.
    /// For each, the hub keeps a place in the bridge's queue and a reserve in the bridge's
    /// account, so an order that used a slot always has its answer paid for.
    #[pallet::storage]
    pub type OrderSlots<T> = StorageMap<_, Twox64Concat, BridgeId, u32, ValueQuery>;

    /// MAP ( bridge ) --> count | The most order slots of the bridge that are out at one
    /// time, as the admin set it. While it is zero the hub gives the bridge's portal no
    /// slot, and the portal takes no order.
    #[pallet::storage]
    pub type MaxOrderSlots<T> = StorageMap<_, Twox64Concat, BridgeId, u32, ValueQuery>;

    /// MAP ( bridge ) --> TAO | What the hub sets aside for each message to the bridge's
    /// spoke, as the admin set it. The bridge's account holds this much for each message
    /// of the bridge that has not left and for each order slot that is out, and an exit
    /// pays it in. Set it above what the transport charges: what a message does not use
    /// stays in the account. While it is not set, the hub takes no order from the bridge
    /// and no payment to it.
    #[pallet::storage]
    pub type MessageReserve<T> = StorageMap<_, Twox64Concat, BridgeId, TaoBalance, OptionQuery>;

    /// MAP ( bridge ) --> TAO | The most the hub pays the transport for one message to the
    /// bridge's spoke, as the admin set it: at least the reserve. What a message costs
    /// above the reserve comes from what the bridge's account holds beyond the reserves
    /// of the messages that have not left and of the order slots that are out.
    #[pallet::storage]
    pub type MessageCeiling<T> = StorageMap<_, Twox64Concat, BridgeId, TaoBalance, ValueQuery>;

    /// DMAP ( bridge, asset ) --> TokenInfo | The canonical tokens: the one registry of
    /// which contract on which chain stands for which asset.
    #[pallet::storage]
    pub type Tokens<T> = StorageDoubleMap<
        _,
        Twox64Concat,
        BridgeId,
        Blake2_128Concat,
        SpokeAsset,
        TokenInfo,
        OptionQuery,
    >;

    /// MAP ( bridge ) --> TAO | Canonical TAO outstanding on the bridge's spoke.
    #[pallet::storage]
    pub type TaoSupply<T> = StorageMap<_, Twox64Concat, BridgeId, TaoBalance, ValueQuery>;

    /// MAP ( bridge ) --> TAO | The most canonical TAO the bridge's spoke may have.
    #[pallet::storage]
    pub type TaoCap<T> = StorageMap<_, Twox64Concat, BridgeId, TaoBalance, ValueQuery>;

    /// MAP ( netuid ) --> Market | The subnets with an alpha token.
    #[pallet::storage]
    pub type Markets<T> = StorageMap<_, Twox64Concat, NetUid, Market, OptionQuery>;

    /// DMAP ( netuid, bridge ) --> shares | Shares of the subnet's vault outstanding on the
    /// bridge's spoke, as its alpha token.
    #[pallet::storage]
    pub type AlphaSupply<T> =
        StorageDoubleMap<_, Twox64Concat, NetUid, Twox64Concat, BridgeId, u64, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// An order was settled.
        OrderSettled {
            bridge: BridgeId,
            nonce: u64,
            payer: H160,
            source: SpokeAsset,
            amount: u64,
            destination: Destination<T::AccountId>,
            amount_out: u64,
        },
        /// An order could not be settled; the spoke returns to the payer what it took.
        OrderRefunded {
            bridge: BridgeId,
            nonce: u64,
            payer: H160,
            source: SpokeAsset,
            amount: u64,
            error: DispatchError,
        },
        /// An order was dropped: its spoke burned more than the hub let it mint.
        OrderRejected {
            bridge: BridgeId,
            nonce: u64,
            error: DispatchError,
        },
        /// The admin origin passed over an order.
        OrderSkipped { bridge: BridgeId, nonce: u64 },
        /// Service credits were bought: `gamma` was burned at `service` for `beneficiary`.
        /// The service credits the beneficiary by the growth of the payer total in the
        /// subtensor pallet; this event tells it which total to read.
        CreditsPurchased {
            bridge: BridgeId,
            /// The order that paid, or `None` if an account on Bittensor paid.
            nonce: Option<u64>,
            beneficiary: H160,
            netuid: NetUid,
            service: T::AccountId,
            tao: TaoBalance,
            alpha_burned: AlphaBalance,
            gamma: GammaBalance,
        },
        /// An account on Bittensor paid for something delivered on a spoke, and paid `fee`,
        /// the bridge's reserve, into the bridge's account for the message.
        BridgedOut {
            who: T::AccountId,
            bridge: BridgeId,
            source: NativeAsset,
            amount: u64,
            destination: Destination<T::AccountId>,
            amount_out: u64,
            fee: TaoBalance,
        },
        /// Message `nonce` was queued for the gateway in the queue of `bridge`.
        MessageQueued { bridge: BridgeId, nonce: u64 },
        /// Message `nonce` of `bridge` takes `count` order slots to the bridge's portal.
        OrderSlotsGranted {
            bridge: BridgeId,
            nonce: u64,
            count: u32,
        },
        /// The most order slots of `bridge` that are out at one time was set.
        MaxOrderSlotsSet { bridge: BridgeId, max: u32 },
        /// Message `nonce` of `bridge` was deferred: it left the line, so that the messages
        /// behind it can leave. It is still in the hub, and anyone can have the gateway
        /// send it.
        MessageDeferred { bridge: BridgeId, nonce: u64 },
        /// The gateway took message `nonce` of `bridge` to hand it to the transport, and
        /// the bridge's account paid the gateway `paid` for it.
        MessageDispatched {
            bridge: BridgeId,
            nonce: u64,
            paid: TaoBalance,
        },
        /// The gateway account was set.
        GatewaySet { gateway: Option<T::AccountId> },
        /// The guardian account was set.
        GuardianSet { guardian: Option<T::AccountId> },
        /// The hub was paused or resumed.
        PausedSet { paused: bool },
        /// A canonical token was entered in the registry.
        TokenRegistered {
            bridge: BridgeId,
            asset: SpokeAsset,
            token: TokenInfo,
        },
        /// The cap on canonical TAO on a spoke was set.
        TaoCapSet { bridge: BridgeId, cap: TaoBalance },
        /// What the hub sets aside for a message to a bridge's spoke, and the most it pays
        /// for one, were set.
        MessageReserveSet {
            bridge: BridgeId,
            reserve: TaoBalance,
            ceiling: TaoBalance,
        },
        /// `who` paid `amount` into the account of `bridge`, for the transport of messages
        /// to the bridge's spoke.
        MessagesFunded {
            who: T::AccountId,
            bridge: BridgeId,
            amount: TaoBalance,
        },
        /// A subnet's market was opened.
        MarketOpened { netuid: NetUid, generation: u64 },
        /// The cap on a market's vault was set.
        MarketCapSet { netuid: NetUid, cap: AlphaBalance },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The caller is not the gateway.
        NotGateway,
        /// The hub is paused.
        Paused,
        /// No bridge has this portal on this domain.
        UnknownPortal,
        /// The bridge is unknown or disabled.
        BridgeDisabled,
        /// The order is not the next of its bridge: it was settled before, or an earlier
        /// one has not arrived.
        OrderOutOfSequence,
        /// The destination is the asset the order pays with, or cannot be paid for this way.
        InvalidRoute,
        /// The order's deadline has passed.
        Expired,
        /// The order would buy less than its minimum.
        SlippageExceeded,
        /// The amount is too small to settle.
        AmountTooLow,
        /// The spoke burned more of a token than it has outstanding.
        Unbacked,
        /// The destination token is not in the registry for this spoke.
        TokenNotRegistered,
        /// The spoke would hold more canonical TAO than its cap.
        TaoCapExceeded,
        /// The reserve would be left with less TAO than an account can hold.
        ReserveDust,
        /// The subnet has no market.
        MarketUnknown,
        /// The subnet already has a market.
        MarketExists,
        /// The market's subnet is gone: its token can be sold, not bought.
        MarketClosed,
        /// The market's subnet is being dissolved; its token can be sold once that is done.
        MarketSettling,
        /// The vault would hold more alpha than the market's cap.
        MarketCapExceeded,
        /// There is no subnet at this netuid to open a market for.
        SubnetUnknown,
        /// The pool has no time-weighted price yet to value credits with.
        TwapUnavailable,
        /// The bridge's queue has no place left: its messages in line and its order slots
        /// that are out take all of them.
        OutboxFull,
        /// The message is not the head of its bridge's queue, and, where a deferred
        /// message will do, is not one of those either.
        NotOutboxHead,
        /// Arithmetic overflowed.
        Overflow,
        /// The bridge's reserve for a message is not set, or the bridge's account does not
        /// hold the reserve for every message that has not left, for every order slot
        /// that is out, and for one more message.
        MessagesNotFunded,
        /// The gateway asked for more than the hub pays for the message: more than the
        /// bridge's ceiling, or more than its reserve while the bridge's account has
        /// nothing to spare.
        FeeAboveLimit,
        /// The most the hub pays for a message cannot be less than what it sets aside
        /// for one.
        CeilingBelowReserve,
        /// The account does not hold the existential deposit, so it could not hold what
        /// the hub hands the gateway for the transport.
        GatewayNotFunded,
        /// The hub can give the bridge's portal no order slot now. The portal is not short
        /// of them, or the bridge's queue or account has no room for more.
        NoSlotsToGrant,
        /// The message has not been at the head of its queue for long enough: only the
        /// gateway and the admin may defer it now.
        NotStale,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// The hub keeps funds apart by account. The account id must be wide enough for the
        /// accounts derived from the pallet id to differ.
        fn integrity_test() {
            let accounts = [
                Self::hub_account(),
                Self::reserve_account(),
                Self::vault_account(NetUid::from(0)),
                Self::vault_account(NetUid::from(1)),
                Self::vault_account(NetUid::from(256)),
                Self::message_account(0),
                Self::message_account(1),
                Self::message_account(256),
            ];
            for (position, account) in accounts.iter().enumerate() {
                assert!(
                    !accounts
                        .iter()
                        .skip(position.saturating_add(1))
                        .any(|other| other == account),
                    "the account id is too narrow to keep the hub's accounts apart"
                );
            }
        }

        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state()
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #![deny(clippy::expect_used)]

        /// Settles the order that the portal `sender` on transport domain `domain` sent.
        /// Only the gateway may call this.
        ///
        /// Fails, and so leaves the order with the transport to deliver again, only if the
        /// hub is paused, the portal or its bridge is not live, or the order is out of
        /// sequence. An order that cannot be settled for any other reason is taken and
        /// refunded. The answer always has room: the portal used an order slot for the
        /// order, and the hub set aside a place in the queue and a reserve when it gave the
        /// slot ([`OrderSlots`]). An order from a portal that used no slot is taken only
        /// if its answer has room all the same.
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::execute().saturating_add(T::Staking::weight()))]
        pub fn execute(
            origin: OriginFor<T>,
            domain: u32,
            sender: H256,
            order: Order<T::AccountId>,
        ) -> DispatchResult {
            Self::ensure_gateway(origin)?;
            ensure!(!Paused::<T>::get(), Error::<T>::Paused);
            let bridge = pallet_tusd::Pallet::<T>::bridge_of(domain, sender)
                .ok_or(Error::<T>::UnknownPortal)?;
            Self::ensure_bridge_enabled(bridge)?;
            ensure!(
                order.nonce == NextOrder::<T>::get(bridge),
                Error::<T>::OrderOutOfSequence
            );
            // Every order has an answer, if only its refund. The order's slot kept room
            // for it.
            Self::use_slot(bridge)?;
            NextOrder::<T>::insert(bridge, order.nonce.saturating_add(1));
            let outcome = Self::settle(bridge, &order)?;
            Self::record(bridge, order.nonce, outcome);
            Ok(())
        }

        /// Takes message `nonce` of `bridge` out of the hub, and moves `paid`, what the
        /// transport charges the gateway for the message, from the bridge's account to the
        /// gateway. The message is the head of the bridge's queue, or a deferred one. Only
        /// the gateway may call this, in the transaction in which it pays the transport and
        /// hands it the message: if that fails, this is undone with it.
        ///
        /// Fails if `paid` is more than the hub pays for a message of the bridge now
        /// ([`Pallet::message_limit`]). The bridge's account holds the reserve for every
        /// message that has not left, so a charge up to the reserve can always be paid.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::ack_outbound())]
        pub fn ack_outbound(
            origin: OriginFor<T>,
            bridge: BridgeId,
            nonce: u64,
            paid: TaoBalance,
        ) -> DispatchResult {
            let gateway = Self::ensure_gateway(origin)?;
            ensure!(!Paused::<T>::get(), Error::<T>::Paused);
            let head = NextOutbound::<T>::get(bridge);
            ensure!(
                nonce <= head && Outbox::<T>::contains_key(bridge, nonce),
                Error::<T>::NotOutboxHead
            );
            ensure!(
                paid <= Self::message_limit(bridge),
                Error::<T>::FeeAboveLimit
            );
            if !paid.is_zero() {
                CurrencyOf::<T>::transfer(
                    &Self::message_account(bridge),
                    &gateway,
                    paid,
                    Preservation::Preserve,
                )?;
            }
            Outbox::<T>::remove(bridge, nonce);
            if nonce == head {
                Self::advance_head(bridge, head);
            } else {
                Deferred::<T>::mutate(bridge, |deferred| *deferred = deferred.saturating_sub(1));
            }
            Self::deposit_event(Event::MessageDispatched {
                bridge,
                nonce,
                paid,
            });
            Ok(())
        }

        /// Defers message `nonce`, the head of the queue of `bridge`: takes it out of the
        /// line, so that the messages behind it can leave. The message stays in the hub
        /// with its reserve, and the gateway can send it at any time.
        ///
        /// The gateway calls this for a message that is not paid for: the transport
        /// charges more for it than the hub and the caller pay. The admin may call it at
        /// any time. Anyone may call it for a message that has been at the head of its
        /// queue for [`Config::StaleAfter`] blocks: the gateway could not hand it over in
        /// that time, because the transport refuses it or because no keeper asked.
        #[pallet::call_index(14)]
        #[pallet::weight(<T as Config>::WeightInfo::defer_outbound())]
        pub fn defer_outbound(
            origin: OriginFor<T>,
            bridge: BridgeId,
            nonce: u64,
        ) -> DispatchResult {
            if <T as Config>::AdminOrigin::try_origin(origin.clone()).is_err() {
                let who = ensure_signed(origin)?;
                let waited = Self::now().saturating_sub(HeadSince::<T>::get(bridge));
                ensure!(
                    Gateway::<T>::get().as_ref() == Some(&who) || waited >= T::StaleAfter::get(),
                    Error::<T>::NotStale
                );
            }
            ensure!(!Paused::<T>::get(), Error::<T>::Paused);
            ensure!(
                nonce == NextOutbound::<T>::get(bridge) && nonce < NextMessage::<T>::get(bridge),
                Error::<T>::NotOutboxHead
            );
            Self::advance_head(bridge, nonce);
            Deferred::<T>::mutate(bridge, |deferred| *deferred = deferred.saturating_add(1));
            Self::deposit_event(Event::MessageDeferred { bridge, nonce });
            Ok(())
        }

        /// Pays `amount` of the signer's TAO or tUSD for `destination`, delivered on the
        /// spoke of `bridge`: the stablecoin, canonical TAO or an alpha token. Fails as a
        /// whole if it would deliver less than `min_out`. The signer also pays the
        /// bridge's [`MessageReserve`] in TAO into the bridge's account, for the message.
        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::bridge_out().saturating_add(T::Staking::weight()))]
        pub fn bridge_out(
            origin: OriginFor<T>,
            bridge: BridgeId,
            source: NativeAsset,
            amount: u64,
            destination: Destination<T::AccountId>,
            min_out: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(!Paused::<T>::get(), Error::<T>::Paused);
            Self::ensure_bridge_enabled(bridge)?;
            let fee = MessageReserve::<T>::get(bridge).ok_or(Error::<T>::MessagesNotFunded)?;
            let amount_out =
                Self::route_native(&who, bridge, source, amount, &destination, min_out)?;
            Self::deposit_event(Event::BridgedOut {
                who,
                bridge,
                source,
                amount,
                destination,
                amount_out,
                fee,
            });
            Ok(())
        }

        /// Sets the account of the gateway contract, or removes it. The account must hold
        /// the existential deposit already: the hub hands it only what it pays on to the
        /// transport. The TAO for messages is in the bridges' accounts, so a new gateway
        /// starts with all of it.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::set_gateway())]
        pub fn set_gateway(origin: OriginFor<T>, gateway: Option<T::AccountId>) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            if let Some(account) = &gateway {
                ensure!(
                    CurrencyOf::<T>::balance(account) >= CurrencyOf::<T>::minimum_balance(),
                    Error::<T>::GatewayNotFunded
                );
            }
            Gateway::<T>::set(gateway.clone());
            Self::deposit_event(Event::GatewaySet { gateway });
            Ok(())
        }

        /// Sets the account that may pause the rails, or removes it.
        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::set_guardian())]
        pub fn set_guardian(
            origin: OriginFor<T>,
            guardian: Option<T::AccountId>,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            Guardian::<T>::set(guardian.clone());
            Self::deposit_event(Event::GuardianSet { guardian });
            Ok(())
        }

        /// Pauses or resumes the hub. The pause origin may only pause.
        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::set_paused())]
        pub fn set_paused(origin: OriginFor<T>, paused: bool) -> DispatchResult {
            let may_pause =
                paused && <T as Config>::PauseOrigin::try_origin(origin.clone()).is_ok();
            if !may_pause {
                <T as Config>::AdminOrigin::ensure_origin(origin)?;
            }
            Paused::<T>::put(paused);
            Self::deposit_event(Event::PausedSet { paused });
            Ok(())
        }

        /// Enters in the registry the contract that stands for the stablecoin or for
        /// canonical TAO on the spoke of `bridge`, or corrects the address of an alpha
        /// token. An alpha token enters through [`Pallet::list_market`], which also has the
        /// spoke create it; this call only changes the address the registry shows for it.
        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::register_token())]
        pub fn register_token(
            origin: OriginFor<T>,
            bridge: BridgeId,
            asset: SpokeAsset,
            address: H160,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            let stable = pallet_tusd::Bridges::<T>::get(bridge)
                .ok_or(Error::<T>::BridgeDisabled)?
                .route
                .asset;
            let named = |name: &[u8], symbol: &[u8]| TokenInfo {
                address,
                name: TokenName::truncate_from(name.to_vec()),
                symbol: TokenSymbol::truncate_from(symbol.to_vec()),
            };
            let token = match (asset, stable) {
                (SpokeAsset::Tao, _) => named(b"Bittensor", b"TAO"),
                (SpokeAsset::Stable, StableAsset::Usdc) => named(b"USD Coin", b"USDC"),
                (SpokeAsset::Stable, StableAsset::Usdt) => named(b"Tether USD", b"USDT"),
                // A stablecoin this runtime does not know yet.
                (SpokeAsset::Stable, _) => return Err(Error::<T>::InvalidRoute.into()),
                (SpokeAsset::Alpha(_), _) => TokenInfo {
                    address,
                    ..Tokens::<T>::get(bridge, asset).ok_or(Error::<T>::TokenNotRegistered)?
                },
            };
            Tokens::<T>::insert(bridge, asset, token.clone());
            Self::deposit_event(Event::TokenRegistered {
                bridge,
                asset,
                token,
            });
            Ok(())
        }

        /// Sets the most canonical TAO the spoke of `bridge` may have outstanding.
        #[pallet::call_index(7)]
        #[pallet::weight(<T as Config>::WeightInfo::set_tao_cap())]
        pub fn set_tao_cap(
            origin: OriginFor<T>,
            bridge: BridgeId,
            cap: TaoBalance,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            TaoCap::<T>::insert(bridge, cap);
            Self::deposit_event(Event::TaoCapSet { bridge, cap });
            Ok(())
        }

        /// Opens a market for the subnet at `netuid`: opens its index hotkey, owned by the
        /// market's vault, and lets the vault hold up to `cap` alpha.
        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::open_market().saturating_add(T::Staking::weight()))]
        pub fn open_market(
            origin: OriginFor<T>,
            netuid: NetUid,
            cap: AlphaBalance,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            ensure!(
                !Markets::<T>::contains_key(netuid),
                Error::<T>::MarketExists
            );
            let SubnetStatus::Live { generation } = T::Staking::subnet_status(netuid) else {
                return Err(Error::<T>::SubnetUnknown.into());
            };
            T::Staking::open_index(netuid, &Self::vault_account(netuid))?;
            Markets::<T>::insert(
                netuid,
                Market {
                    generation,
                    shares: 0,
                    cap,
                },
            );
            Self::deposit_event(Event::MarketOpened { netuid, generation });
            Ok(())
        }

        /// Lists the alpha token of `netuid` on the spoke of `bridge`: enters it in the
        /// registry at `address`, under the name the subnet published on chain, and tells
        /// the spoke to create it. The portal creates the token at its own address for the
        /// subnet (`Portal.alphaTokenAddress`). If `address` is another, the portal reports
        /// the difference and [`Pallet::register_token`] corrects the registry.
        #[pallet::call_index(9)]
        #[pallet::weight(<T as Config>::WeightInfo::list_market())]
        pub fn list_market(
            origin: OriginFor<T>,
            netuid: NetUid,
            bridge: BridgeId,
            address: H160,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            ensure!(
                Markets::<T>::contains_key(netuid),
                Error::<T>::MarketUnknown
            );
            Self::ensure_bridge_enabled(bridge)?;
            let asset = SpokeAsset::Alpha(netuid);
            ensure!(
                !Tokens::<T>::contains_key(bridge, asset),
                Error::<T>::MarketExists
            );
            let (name, symbol) = Self::token_identity(netuid);
            let token = TokenInfo {
                address,
                name: name.clone(),
                symbol: symbol.clone(),
            };
            Tokens::<T>::insert(bridge, asset, token.clone());
            Self::enqueue(
                bridge,
                Message::RegisterToken {
                    netuid,
                    token: address,
                    name,
                    symbol,
                },
            )?;
            Self::deposit_event(Event::TokenRegistered {
                bridge,
                asset,
                token,
            });
            Ok(())
        }

        /// Sets the most alpha the vault of `netuid` may hold after a purchase.
        #[pallet::call_index(10)]
        #[pallet::weight(<T as Config>::WeightInfo::set_market_cap())]
        pub fn set_market_cap(
            origin: OriginFor<T>,
            netuid: NetUid,
            cap: AlphaBalance,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            Markets::<T>::try_mutate(netuid, |market| {
                market.as_mut().ok_or(Error::<T>::MarketUnknown)?.cap = cap;
                Ok::<(), Error<T>>(())
            })?;
            Self::deposit_event(Event::MarketCapSet { netuid, cap });
            Ok(())
        }

        /// Sets what the hub pays the transport for one message to the spoke of `bridge`.
        ///
        /// - `reserve`: what the hub sets aside for each message that has not left. Set it
        ///   above what the transport charges. Zero says that the hub sets nothing aside.
        /// - `ceiling`: the most the hub pays for one message, at least `reserve`. What a
        ///   message costs above the reserve comes from what the bridge's account holds
        ///   beyond the reserves of those messages. It is the bound on what a wrong price
        ///   from the transport can take for one message.
        ///
        /// A bridge takes no order before this is set. Fails if the bridge's account does
        /// not hold the new reserve for every message that has not left and every order
        /// slot that is out: pay in first ([`Pallet::fund_messages`]).
        #[pallet::call_index(12)]
        #[pallet::weight(<T as Config>::WeightInfo::set_message_reserve())]
        pub fn set_message_reserve(
            origin: OriginFor<T>,
            bridge: BridgeId,
            reserve: TaoBalance,
            ceiling: TaoBalance,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            Self::ensure_bridge_known(bridge)?;
            ensure!(ceiling >= reserve, Error::<T>::CeilingBelowReserve);
            ensure!(
                Self::holds_reserve(bridge, reserve, Self::backed(bridge)),
                Error::<T>::MessagesNotFunded
            );
            MessageReserve::<T>::insert(bridge, reserve);
            MessageCeiling::<T>::insert(bridge, ceiling);
            Self::deposit_event(Event::MessageReserveSet {
                bridge,
                reserve,
                ceiling,
            });
            Ok(())
        }

        /// Pays `amount` of the signer's TAO into the account of `bridge`, for the
        /// transport of messages to the bridge's spoke. Anyone may pay. The operator of a
        /// spoke pays in what the spoke's portal collected as answer fees. What is paid in
        /// is spent on messages and cannot be taken back.
        #[pallet::call_index(13)]
        #[pallet::weight(<T as Config>::WeightInfo::fund_messages())]
        pub fn fund_messages(
            origin: OriginFor<T>,
            bridge: BridgeId,
            amount: TaoBalance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_bridge_known(bridge)?;
            Self::pay_for_messages(&who, bridge, amount)
        }

        /// Passes over the next order of `bridge` without reading it, so that the orders
        /// behind it can settle. For an order that the gateway cannot decode: what its
        /// payer locked stays on the spoke and has to be returned by governance.
        #[pallet::call_index(11)]
        #[pallet::weight(<T as Config>::WeightInfo::skip_order())]
        pub fn skip_order(origin: OriginFor<T>, bridge: BridgeId, nonce: u64) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            ensure!(
                nonce == NextOrder::<T>::get(bridge),
                Error::<T>::OrderOutOfSequence
            );
            // The portal used an order slot for the order. No answer needs its room.
            OrderSlots::<T>::mutate(bridge, |slots| *slots = slots.saturating_sub(1));
            NextOrder::<T>::insert(bridge, nonce.saturating_add(1));
            Self::record(bridge, nonce, Outcome::Skipped);
            Ok(())
        }

        /// Sets the most order slots of `bridge` that are out at one time: the most orders
        /// that the bridge's portal can have on their way before an answer arrives. The
        /// hub keeps a place in the bridge's queue and a reserve for each, so set it well
        /// below the queue's size. Lowering it takes no slot back: the hub gives none
        /// until fewer are out.
        #[pallet::call_index(15)]
        #[pallet::weight(<T as Config>::WeightInfo::set_max_order_slots())]
        pub fn set_max_order_slots(
            origin: OriginFor<T>,
            bridge: BridgeId,
            max: u32,
        ) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
            Self::ensure_bridge_known(bridge)?;
            MaxOrderSlots::<T>::insert(bridge, max);
            Self::deposit_event(Event::MaxOrderSlotsSet { bridge, max });
            Ok(())
        }

        /// Sends the portal of `bridge` the order slots that it is missing, in a message of
        /// their own. Anyone may call this.
        ///
        /// Slots go to a portal with the hub's payments. This is for a portal that is
        /// short of slots while no payment is on its way: at the start, after orders that
        /// needed no payment on the spoke, or after the bridge's account was paid into.
        /// Fails unless at most half of [`MaxOrderSlots`] are out, and unless the queue
        /// and the bridge's account have room for the message and for one slot at least.
        #[pallet::call_index(16)]
        #[pallet::weight(<T as Config>::WeightInfo::grant_slots())]
        pub fn grant_slots(origin: OriginFor<T>, bridge: BridgeId) -> DispatchResult {
            ensure_signed(origin)?;
            ensure!(!Paused::<T>::get(), Error::<T>::Paused);
            Self::ensure_bridge_enabled(bridge)?;
            let out = u64::from(OrderSlots::<T>::get(bridge));
            ensure!(
                out.saturating_mul(2) <= u64::from(MaxOrderSlots::<T>::get(bridge)),
                Error::<T>::NoSlotsToGrant
            );
            Self::enqueue(bridge, Message::Slots { count: 0 })
        }
    }
}

impl<T: Config> Pallet<T> {
    /// The account in which an order's funds rest while it settles. Empty between orders.
    pub fn hub_account() -> T::AccountId {
        <T as Config>::PalletId::get().into_account_truncating()
    }

    /// The account that holds the TAO behind all canonical TAO.
    pub fn reserve_account() -> T::AccountId {
        <T as Config>::PalletId::get().into_sub_account_truncating(*b"tao/")
    }

    /// The account that stakes and spends on `netuid` for the hub, and owns the subnet's
    /// index hotkey. Staking keeps the existential deposit in it: the first stake or spend
    /// uses that much less than it was given, once.
    pub fn vault_account(netuid: NetUid) -> T::AccountId {
        <T as Config>::PalletId::get().into_sub_account_truncating((*b"vlt/", netuid))
    }

    /// The account that holds the TAO for the transport of messages to the spoke of
    /// `bridge`. It keeps the existential deposit; the rest is the bridge's budget.
    pub fn message_account(bridge: BridgeId) -> T::AccountId {
        <T as Config>::PalletId::get().into_sub_account_truncating((*b"msg/", bridge))
    }

    /// What the account of `bridge` can pay the transport: its balance less the existential
    /// deposit, which it keeps.
    pub fn message_budget(bridge: BridgeId) -> TaoBalance {
        CurrencyOf::<T>::reducible_balance(
            &Self::message_account(bridge),
            Preservation::Preserve,
            Fortitude::Polite,
        )
    }

    /// The most the hub pays the transport for one message of `bridge` now: the bridge's
    /// reserve, and above it what the account holds beyond the reserves in use, up to the
    /// bridge's ceiling. The reserves in use are those of the messages that have not left
    /// and of the order slots that are out.
    pub fn message_limit(bridge: BridgeId) -> TaoBalance {
        let reserve = MessageReserve::<T>::get(bridge)
            .unwrap_or_default()
            .to_u64();
        let reserved = u128::from(reserve).saturating_mul(u128::from(Self::backed(bridge)));
        let spare = u128::from(Self::message_budget(bridge).to_u64()).saturating_sub(reserved);
        let ceiling = MessageCeiling::<T>::get(bridge).to_u64().max(reserve);
        u64::try_from(u128::from(reserve).saturating_add(spare))
            .unwrap_or(u64::MAX)
            .min(ceiling)
            .into()
    }

    /// The message at the head of the queue of `bridge`, and where the gateway sends it:
    /// the transport domain and portal of the bridge.
    pub fn next_outbound(bridge: BridgeId) -> Option<(u32, H256, Outbound)> {
        let head = NextOutbound::<T>::get(bridge);
        (head < NextMessage::<T>::get(bridge))
            .then(|| Self::outbound(bridge, head))
            .flatten()
    }

    /// Message `nonce` of `bridge`, if the gateway can take it now, and where the gateway
    /// sends it. The gateway can take the head of the bridge's queue and the deferred
    /// messages.
    pub fn outbound(bridge: BridgeId, nonce: u64) -> Option<(u32, H256, Outbound)> {
        if nonce > NextOutbound::<T>::get(bridge) || Paused::<T>::get() {
            return None;
        }
        let message = Outbox::<T>::get(bridge, nonce)?;
        let route = pallet_tusd::Bridges::<T>::get(bridge)?.route;
        let outbound = Outbound {
            bridge,
            nonce,
            message,
        };
        Some((route.domain, route.portal, outbound))
    }

    /// What an order from the spoke of `bridge` would deliver now, or why it would be
    /// refunded. Runs the order as [`Pallet::execute`] would and undoes it. As there, the
    /// order uses an order slot. If none is out, the quote fails unless the answer has
    /// room all the same: the portal holds no slot then, and takes no order.
    pub fn quote_order(
        bridge: BridgeId,
        source: SpokeAsset,
        amount: u64,
        destination: Destination<T::AccountId>,
    ) -> Result<u64, DispatchError> {
        let order = Order {
            nonce: NextOrder::<T>::get(bridge),
            payer: H160::zero(),
            source,
            amount,
            destination,
            min_out: 0,
            deadline: u64::MAX,
        };
        ensure!(!Paused::<T>::get(), Error::<T>::Paused);
        Self::ensure_bridge_enabled(bridge)?;
        Self::ensure_backed(bridge, source, amount)?;
        Self::undone(|| {
            Self::use_slot(bridge)?;
            Self::route_order(bridge, &order)
        })
    }

    /// What a payment of `amount` of `who`'s `source` would deliver to `destination` on the
    /// spoke of `bridge` now, or why it would fail. Runs the payment as
    /// [`Pallet::bridge_out`] would and undoes it.
    pub fn quote_bridge_out(
        who: &T::AccountId,
        bridge: BridgeId,
        source: NativeAsset,
        amount: u64,
        destination: Destination<T::AccountId>,
    ) -> Result<u64, DispatchError> {
        ensure!(!Paused::<T>::get(), Error::<T>::Paused);
        Self::ensure_bridge_enabled(bridge)?;
        Self::undone(|| Self::route_native(who, bridge, source, amount, &destination, 0))
    }

    /// Runs `route` and rolls back whatever it changed: a quote changes nothing.
    fn undone(route: impl FnOnce() -> Result<u64, DispatchError>) -> Result<u64, DispatchError> {
        let mut quote = Err(Error::<T>::Overflow.into());
        let _ = with_storage_layer(|| {
            quote = route();
            Err::<(), DispatchError>(DispatchError::Other("quote"))
        });
        quote
    }

    /// Checks the invariants in the module documentation that hold between extrinsics.
    /// Runs in `try_state`; tests call it after every operation.
    ///
    /// Only what a transfer from outside cannot falsify is checked here. That an order
    /// leaves nothing in the hub account is true of orders, and tests assert it, but
    /// anyone can send TAO or tUSD to that account.
    pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
        let canonical_tao = TaoSupply::<T>::iter_values().fold(0u128, |sum, supply| {
            sum.saturating_add(u128::from(supply.to_u64()))
        });
        ensure!(
            u128::from(CurrencyOf::<T>::balance(&Self::reserve_account()).to_u64())
                >= canonical_tao,
            "the reserve holds less TAO than the canonical TAO outstanding"
        );
        for (netuid, market) in Markets::<T>::iter() {
            let on_spokes = AlphaSupply::<T>::iter_prefix_values(netuid)
                .fold(0u128, |sum, shares| sum.saturating_add(u128::from(shares)));
            ensure!(
                on_spokes == u128::from(market.shares),
                "a market's shares differ from what its spokes have outstanding"
            );
        }
        for (bridge, tail) in NextMessage::<T>::iter() {
            let head = NextOutbound::<T>::get(bridge);
            let places = tail
                .saturating_sub(head)
                .saturating_add(u64::from(OrderSlots::<T>::get(bridge)));
            ensure!(
                head <= tail && places <= u64::from(T::MaxOutbox::get()),
                "a bridge's queue is out of bounds"
            );
            // Every message from the head on is there, and the messages below the head
            // that are there are the deferred ones.
            let (in_line, deferred) = Outbox::<T>::iter_key_prefix(bridge).fold(
                (0u64, 0u64),
                |(in_line, deferred), nonce| {
                    if nonce >= head {
                        (in_line.saturating_add(1), deferred)
                    } else {
                        (in_line, deferred.saturating_add(1))
                    }
                },
            );
            ensure!(
                in_line == tail.saturating_sub(head)
                    && deferred == u64::from(Deferred::<T>::get(bridge)),
                "a bridge's messages differ from the count of them"
            );
            ensure!(
                Self::holds_reserve(
                    bridge,
                    MessageReserve::<T>::get(bridge).unwrap_or_default(),
                    Self::backed(bridge)
                ),
                "a bridge's account holds less than its reserves in use"
            );
        }
        Ok(())
    }

    /// The account of the gateway, if `origin` is signed by it.
    fn ensure_gateway(origin: OriginFor<T>) -> Result<T::AccountId, DispatchError> {
        let who = ensure_signed(origin)?;
        ensure!(
            Gateway::<T>::get().as_ref() == Some(&who),
            Error::<T>::NotGateway
        );
        Ok(who)
    }

    fn ensure_bridge_known(bridge: BridgeId) -> DispatchResult {
        ensure!(
            pallet_tusd::Bridges::<T>::contains_key(bridge),
            Error::<T>::BridgeDisabled
        );
        Ok(())
    }

    fn ensure_bridge_enabled(bridge: BridgeId) -> DispatchResult {
        ensure!(
            pallet_tusd::Bridges::<T>::get(bridge).is_some_and(|terms| terms.enabled),
            Error::<T>::BridgeDisabled
        );
        Ok(())
    }

    /// Settles `order` or arranges its refund. Fails only if the answer cannot be queued.
    fn settle(bridge: BridgeId, order: &Order<T::AccountId>) -> Result<Outcome, DispatchError> {
        if let Err(error) = Self::ensure_backed(bridge, order.source, order.amount) {
            Self::deposit_event(Event::OrderRejected {
                bridge,
                nonce: order.nonce,
                error,
            });
            return Ok(Outcome::Rejected { error });
        }
        match with_storage_layer(|| Self::route_order(bridge, order)) {
            Ok(amount_out) => {
                Self::deposit_event(Event::OrderSettled {
                    bridge,
                    nonce: order.nonce,
                    payer: order.payer,
                    source: order.source,
                    amount: order.amount,
                    destination: order.destination.clone(),
                    amount_out,
                });
                Ok(Outcome::Settled { amount_out })
            }
            Err(error) => {
                Self::enqueue(
                    bridge,
                    Message::Credit {
                        asset: order.source,
                        to: order.payer,
                        amount: order.amount,
                        order: Some(order.nonce),
                        refund: true,
                        rate: 0,
                        slots: 0,
                    },
                )?;
                Self::deposit_event(Event::OrderRefunded {
                    bridge,
                    nonce: order.nonce,
                    payer: order.payer,
                    source: order.source,
                    amount: order.amount,
                    error,
                });
                Ok(Outcome::Refunded { error })
            }
        }
    }

    /// Keeps the receipt of an order, and drops the one that fell out of the kept range.
    fn record(bridge: BridgeId, nonce: u64, outcome: Outcome) {
        if let Outcome::Skipped = outcome {
            Self::deposit_event(Event::OrderSkipped { bridge, nonce });
        }
        let block = Self::now();
        Receipts::<T>::insert(bridge, nonce, Receipt { block, outcome });
        if let Some(expired) = nonce.checked_sub(T::ReceiptsKept::get()) {
            Receipts::<T>::remove(bridge, expired);
        }
    }

    /// Queues `message` for the portal of `bridge` under the bridge's next nonce, if the
    /// queue and the bridge's account have room for it ([`Pallet::ensure_room`]). A
    /// payment takes along the order slots that the portal is missing, as far as there
    /// is room for those too ([`Pallet::grantable`]). A message of slots alone is queued
    /// only if it has one slot to take.
    pub(crate) fn enqueue(bridge: BridgeId, mut message: Message) -> DispatchResult {
        Self::ensure_room(bridge)?;
        let nonce = NextMessage::<T>::get(bridge);
        if nonce == NextOutbound::<T>::get(bridge) {
            // The queue was empty: this message is its head from now.
            HeadSince::<T>::insert(bridge, Self::now());
        }
        NextMessage::<T>::insert(bridge, nonce.saturating_add(1));
        // The message has its place and its reserve. What is left decides the slots.
        let granted = match &mut message {
            Message::Credit { slots, .. } => {
                *slots = Self::grantable(bridge);
                *slots
            }
            Message::Slots { count } => {
                *count = Self::grantable(bridge);
                ensure!(*count != 0, Error::<T>::NoSlotsToGrant);
                *count
            }
            Message::RegisterToken { .. } => 0,
        };
        if granted != 0 {
            OrderSlots::<T>::mutate(bridge, |out| *out = out.saturating_add(granted));
            Self::deposit_event(Event::OrderSlotsGranted {
                bridge,
                nonce,
                count: granted,
            });
        }
        Outbox::<T>::insert(bridge, nonce, message);
        Self::deposit_event(Event::MessageQueued { bridge, nonce });
        Ok(())
    }

    /// Passes only if `bridge` can have one more message. The queue must have a place
    /// that neither a message in line nor an order slot takes ([`Config::MaxOutbox`]). And
    /// the bridge's account must hold the bridge's [`MessageReserve`] for every reserve
    /// in use and for one more ([`Pallet::backed`]). The hub queues nothing that it could
    /// not pay the transport for.
    ///
    /// Deferred messages take no place in the queue. So a bridge whose messages are
    /// deferred, because nobody pays what the transport charges, still has room.
    pub(crate) fn ensure_room(bridge: BridgeId) -> DispatchResult {
        let places = Self::in_line(bridge).saturating_add(u64::from(OrderSlots::<T>::get(bridge)));
        ensure!(
            places < u64::from(T::MaxOutbox::get()),
            Error::<T>::OutboxFull
        );
        let reserve = MessageReserve::<T>::get(bridge).ok_or(Error::<T>::MessagesNotFunded)?;
        ensure!(
            Self::holds_reserve(bridge, reserve, Self::backed(bridge).saturating_add(1)),
            Error::<T>::MessagesNotFunded
        );
        Ok(())
    }

    /// Uses an order slot of `bridge` for an order that arrived. The place and the reserve
    /// that the hub kept for the slot are now free for the order's answer. If no slot is
    /// out, the portal sent the order without one: the hub takes it only if its answer
    /// has room all the same.
    fn use_slot(bridge: BridgeId) -> DispatchResult {
        match OrderSlots::<T>::get(bridge) {
            0 => Self::ensure_room(bridge),
            out => {
                OrderSlots::<T>::insert(bridge, out.saturating_sub(1));
                Ok(())
            }
        }
    }

    /// How many order slots the hub can give the portal of `bridge` now: what the portal
    /// is missing of [`MaxOrderSlots`], as far as the bridge's queue has places and the
    /// bridge's account holds reserves beyond those in use.
    pub fn grantable(bridge: BridgeId) -> u32 {
        let Some(reserve) = MessageReserve::<T>::get(bridge) else {
            return 0;
        };
        let out = OrderSlots::<T>::get(bridge);
        let missing = MaxOrderSlots::<T>::get(bridge).saturating_sub(out);
        let places = u64::from(T::MaxOutbox::get())
            .saturating_sub(Self::in_line(bridge))
            .saturating_sub(u64::from(out));
        let in_use = u128::from(reserve.to_u64()).saturating_mul(u128::from(Self::backed(bridge)));
        let free = u128::from(Self::message_budget(bridge).to_u64()).saturating_sub(in_use);
        let affordable = free
            .checked_div(u128::from(reserve.to_u64()))
            .unwrap_or(u128::MAX);
        let room = u128::from(places).min(affordable);
        missing.min(u32::try_from(room).unwrap_or(u32::MAX))
    }

    /// Moves the head of the queue of `bridge` on from message `head`, which leaves the
    /// line. The next message, if there is one, is the head from this block.
    fn advance_head(bridge: BridgeId, head: u64) {
        NextOutbound::<T>::insert(bridge, head.saturating_add(1));
        HeadSince::<T>::insert(bridge, Self::now());
    }

    /// The number of this block.
    fn now() -> u32 {
        frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// How many messages wait in the queue of `bridge`.
    fn in_line(bridge: BridgeId) -> u64 {
        NextMessage::<T>::get(bridge).saturating_sub(NextOutbound::<T>::get(bridge))
    }

    /// How many messages of `bridge` have not left: those in line and those deferred.
    pub fn waiting(bridge: BridgeId) -> u64 {
        Self::in_line(bridge).saturating_add(u64::from(Deferred::<T>::get(bridge)))
    }

    /// How many reserves of `bridge` are in use: one for each message that has not left,
    /// and one for each order slot that is out.
    pub fn backed(bridge: BridgeId) -> u64 {
        Self::waiting(bridge).saturating_add(u64::from(OrderSlots::<T>::get(bridge)))
    }

    /// Whether the account of `bridge` can pay `reserve` for each of `messages` messages.
    fn holds_reserve(bridge: BridgeId, reserve: TaoBalance, messages: u64) -> bool {
        u128::from(Self::message_budget(bridge).to_u64())
            >= u128::from(reserve.to_u64()).saturating_mul(u128::from(messages))
    }

    /// Moves `amount` of `who`'s TAO into the account of `bridge`.
    pub(crate) fn pay_for_messages(
        who: &T::AccountId,
        bridge: BridgeId,
        amount: TaoBalance,
    ) -> DispatchResult {
        CurrencyOf::<T>::transfer(
            who,
            &Self::message_account(bridge),
            amount,
            Preservation::Expendable,
        )?;
        Self::deposit_event(Event::MessagesFunded {
            who: who.clone(),
            bridge,
            amount,
        });
        Ok(())
    }

    /// The name and symbol of the alpha token of `netuid`, from the identity the subnet
    /// published on chain.
    ///
    /// The name is the subnet's name. The symbol is its first run of ASCII letters and
    /// digits, in upper case, cut to what wallets show: "Chutes" gives `CHUTES` and
    /// "lium.io" gives `LIUM`. A subnet without a usable name is `Subnet 64` and `SN64`.
    pub fn token_identity(netuid: NetUid) -> (TokenName, TokenSymbol) {
        let published = T::Staking::subnet_name(netuid).unwrap_or_default();
        let symbol: Vec<u8> = published
            .iter()
            .skip_while(|byte| !byte.is_ascii_alphanumeric())
            .take_while(|byte| byte.is_ascii_alphanumeric())
            .map(u8::to_ascii_uppercase)
            .take(MAX_TOKEN_SYMBOL as usize)
            .collect();
        // The name must still be text after it is cut to length.
        let name: Vec<u8> = core::str::from_utf8(&published)
            .unwrap_or_default()
            .chars()
            .filter(|character| !character.is_control())
            .scan(0usize, |length, character| {
                *length = length.saturating_add(character.len_utf8());
                (*length <= MAX_TOKEN_NAME as usize).then_some(character)
            })
            .collect::<alloc::string::String>()
            .into_bytes();
        if symbol.is_empty() || name.is_empty() {
            let number = alloc::format!("{}", u16::from(netuid));
            return (
                TokenName::truncate_from([b"Subnet ", number.as_bytes()].concat()),
                TokenSymbol::truncate_from([b"SN", number.as_bytes()].concat()),
            );
        }
        (
            TokenName::truncate_from(name),
            TokenSymbol::truncate_from(symbol),
        )
    }
}
