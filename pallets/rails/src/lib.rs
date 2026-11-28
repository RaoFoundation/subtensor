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
//! ## Invariants
//!
//! 1. [`Pallet::reserve_account`] holds at least the sum of [`TaoSupply`].
//! 2. `Markets(n).shares` equals the sum of `AlphaSupply(n, ·)`.
//! 3. The hub tells a spoke to mint a canonical token only when it added as much to that
//!    spoke's supply, and takes a burned token only up to that supply.
//! 4. An order nonce is settled at most once, and a message nonce is issued at most once.

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
    traits::{EnsureOrigin, UnixTime, fungible::Inspect},
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

        /// The most messages that may wait for the gateway.
        #[pallet::constant]
        type MaxOutbox: Get<u32>;

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

    /// MAP ( bridge ) --> nonce | The nonce of the next message to the bridge.
    #[pallet::storage]
    pub type NextMessage<T> = StorageMap<_, Twox64Concat, BridgeId, u64, ValueQuery>;

    /// MAP ( position ) --> Outbound | Messages waiting for the gateway, first in first out.
    #[pallet::storage]
    pub type Outbox<T> = StorageMap<_, Twox64Concat, u64, Outbound, OptionQuery>;

    /// ITEM --> ( head, tail ) | The positions of the outbox: `head` is handed over next and
    /// `tail` is filled next.
    #[pallet::storage]
    pub type OutboxBounds<T> = StorageValue<_, (u64, u64), ValueQuery>;

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
        /// An account on Bittensor paid for something delivered on a spoke.
        BridgedOut {
            who: T::AccountId,
            bridge: BridgeId,
            source: NativeAsset,
            amount: u64,
            destination: Destination<T::AccountId>,
            amount_out: u64,
        },
        /// A message was queued for the gateway at `position`.
        MessageQueued {
            position: u64,
            bridge: BridgeId,
            nonce: u64,
        },
        /// The gateway took the message at `position` to hand it to the transport.
        MessageDispatched { position: u64 },
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
        /// Too many messages wait for the gateway.
        OutboxFull,
        /// The position is not the head of the outbox.
        NotOutboxHead,
        /// Arithmetic overflowed.
        Overflow,
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
        /// hub is paused, the portal or its bridge is not live, the order is out of
        /// sequence, or the answer cannot be queued. An order that cannot be settled for
        /// any other reason is taken and refunded.
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
            NextOrder::<T>::insert(bridge, order.nonce.saturating_add(1));
            let outcome = Self::settle(bridge, &order)?;
            Self::record(bridge, order.nonce, outcome);
            Ok(())
        }

        /// Takes the message at `position`, the head of the outbox, out of it. Only the
        /// gateway may call this, in the transaction in which it hands the message to the
        /// transport: if that fails, this is undone with it.
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::ack_outbound())]
        pub fn ack_outbound(origin: OriginFor<T>, position: u64) -> DispatchResult {
            Self::ensure_gateway(origin)?;
            ensure!(!Paused::<T>::get(), Error::<T>::Paused);
            let (head, tail) = OutboxBounds::<T>::get();
            ensure!(position == head && head < tail, Error::<T>::NotOutboxHead);
            Outbox::<T>::remove(position);
            OutboxBounds::<T>::put((head.saturating_add(1), tail));
            Self::deposit_event(Event::MessageDispatched { position });
            Ok(())
        }

        /// Pays `amount` of the signer's TAO or tUSD for `destination`, delivered on the
        /// spoke of `bridge`: the stablecoin, canonical TAO or an alpha token. Fails as a
        /// whole if it would deliver less than `min_out`.
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
            let amount_out =
                Self::route_native(&who, bridge, source, amount, &destination, min_out)?;
            Self::deposit_event(Event::BridgedOut {
                who,
                bridge,
                source,
                amount,
                destination,
                amount_out,
            });
            Ok(())
        }

        /// Sets the account of the gateway contract, or removes it.
        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::set_gateway())]
        pub fn set_gateway(origin: OriginFor<T>, gateway: Option<T::AccountId>) -> DispatchResult {
            <T as Config>::AdminOrigin::ensure_origin(origin)?;
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
            NextOrder::<T>::insert(bridge, nonce.saturating_add(1));
            Self::record(bridge, nonce, Outcome::Skipped);
            Ok(())
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

    /// The message at the head of the outbox, with its position and where the gateway sends
    /// it: the transport domain and portal of its bridge.
    pub fn next_outbound() -> Option<(u64, u32, H256, Outbound)> {
        let (head, tail) = OutboxBounds::<T>::get();
        if head >= tail || Paused::<T>::get() {
            return None;
        }
        let outbound = Outbox::<T>::get(head)?;
        let route = pallet_tusd::Bridges::<T>::get(outbound.bridge)?.route;
        Some((head, route.domain, route.portal, outbound))
    }

    /// What an order from the spoke of `bridge` would deliver now, or why it would be
    /// refunded. Runs the order as [`Pallet::execute`] would and undoes it.
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
        Self::undone(|| Self::route_order(bridge, &order))
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
        let (head, tail) = OutboxBounds::<T>::get();
        ensure!(
            head <= tail && tail.saturating_sub(head) <= u64::from(T::MaxOutbox::get()),
            "the outbox is out of bounds"
        );
        Ok(())
    }

    fn ensure_gateway(origin: OriginFor<T>) -> DispatchResult {
        let who = ensure_signed(origin)?;
        ensure!(Gateway::<T>::get() == Some(who), Error::<T>::NotGateway);
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
        let block = frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or(u32::MAX);
        Receipts::<T>::insert(bridge, nonce, Receipt { block, outcome });
        if let Some(expired) = nonce.checked_sub(T::ReceiptsKept::get()) {
            Receipts::<T>::remove(bridge, expired);
        }
    }

    /// Queues `message` for the portal of `bridge` under the bridge's next nonce.
    pub(crate) fn enqueue(bridge: BridgeId, message: Message) -> DispatchResult {
        let (head, tail) = OutboxBounds::<T>::get();
        ensure!(
            tail.saturating_sub(head) < u64::from(T::MaxOutbox::get()),
            Error::<T>::OutboxFull
        );
        let nonce = NextMessage::<T>::get(bridge);
        NextMessage::<T>::insert(bridge, nonce.saturating_add(1));
        Outbox::<T>::insert(
            tail,
            Outbound {
                bridge,
                nonce,
                message,
            },
        );
        OutboxBounds::<T>::put((head, tail.saturating_add(1)));
        Self::deposit_event(Event::MessageQueued {
            position: tail,
            bridge,
            nonce,
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
