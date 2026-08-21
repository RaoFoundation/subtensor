//! # USD PSM pallet
//!
//! The runtime kernel of the canonical rails: a tUSD accounting ledger, a peg
//! stability module (PSM) registry of external USD assets, and the single
//! canonical tUSD/TAO constant-product pool.
//!
//! Entry point: the **gateway path**. The USD rails precompile calls
//! [`Pallet::do_gateway_execute`] with the caller's H160; only the registered
//! Gateway contract passes. Envelopes carry sequential nonces and execute in
//! strict order.
//!
//! The one product is canonical shares (CHUTES): [`GatewayAction::BuyShares`]
//! turns a USD deposit into staked alpha in the hub escrow and mints shares
//! on the spoke; [`GatewayAction::SellShares`] unwinds it back to USD. The
//! share index (escrowed alpha / shares outstanding) rises as the escrow
//! earns emissions and is pushed to the spoke on every mint plus a periodic
//! heartbeat.
//!
//! Once a deposit's backing is secured, the requested action never reverts —
//! failures fall back to a plain tUSD credit ([`FallbackReason`]). The
//! canonical tUSD/TAO pool is internal-only: there are no public swap
//! extrinsics; the pool exists to price buys and sells.
//!
//! tUSD is non-transferable in v1: it exists only as ledger entries moved by
//! this pallet's operations.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::PalletId;
use frame_support::pallet_prelude::*;
use frame_support::storage::with_storage_layer;
use frame_support::traits::fungible::{Inspect, Mutate};
use frame_support::traits::tokens::Preservation;
use frame_system::pallet_prelude::*;
use scale_info::TypeInfo;
use sp_core::H160;
use sp_runtime::traits::AccountIdConversion;
use sp_runtime::{AccountId32, Vec};
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::rails::{
    AssetId, FallbackReason, GatewayAction, GatewayEnvelope, InboundReceipt, RateWindow,
    UsdAssetId,
};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

pub use pallet::*;

/// Loose coupling to the staking engine in `pallet-subtensor`; implemented in
/// the runtime to avoid a crate cycle (subtensor must not depend on this
/// pallet, and vice versa).
pub trait RailsStaking<AccountId> {
    /// Stake `tao` from `coldkey`'s free balance into (`netuid`, `hotkey`).
    /// Fails if the alpha received is below `min_alpha`.
    fn stake(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        tao: TaoBalance,
        min_alpha: AlphaBalance,
    ) -> Result<AlphaBalance, DispatchError>;

    /// Unstake `alpha` from (`netuid`, `hotkey`), crediting TAO to `coldkey`'s
    /// free balance. Fails if the TAO received is below `min_tao`.
    fn unstake(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
        min_tao: TaoBalance,
    ) -> Result<TaoBalance, DispatchError>;

    /// Current alpha staked by `coldkey` under (`hotkey`, `netuid`). This is
    /// the live escrow reading that drives the share index: it grows as the
    /// stake earns emissions.
    fn stake_of(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> u64;
}

impl<AccountId> RailsStaking<AccountId> for () {
    fn stake(
        _coldkey: &AccountId,
        _hotkey: &AccountId,
        _netuid: NetUid,
        _tao: TaoBalance,
        _min_alpha: AlphaBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        Err(DispatchError::Unavailable)
    }

    fn unstake(
        _coldkey: &AccountId,
        _hotkey: &AccountId,
        _netuid: NetUid,
        _alpha: AlphaBalance,
        _min_tao: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Err(DispatchError::Unavailable)
    }

    fn stake_of(_hotkey: &AccountId, _coldkey: &AccountId, _netuid: NetUid) -> u64 {
        0
    }
}

/// Loose coupling to the outbound bridge leg: dispatching a Hyperlane
/// message from the runtime's keyless hub identity through the Bittensor-EVM
/// Mailbox. Implemented in the runtime (via `pallet-ethereum` with an
/// already-authenticated origin, so the dispatch lands in a real Ethereum
/// block and bridge agents can index its logs); mocked in tests.
pub trait RailsOutbound {
    /// Call `mailbox.dispatch(dest_domain, recipient, body)` with `sender` as
    /// the EVM transaction origin.
    fn dispatch_mailbox(
        mailbox: H160,
        sender: H160,
        dest_domain: u32,
        recipient: [u8; 32],
        body: Vec<u8>,
    ) -> DispatchResult;
}

impl RailsOutbound for () {
    fn dispatch_mailbox(
        _mailbox: H160,
        _sender: H160,
        _dest_domain: u32,
        _recipient: [u8; 32],
        _body: Vec<u8>,
    ) -> DispatchResult {
        Err(DispatchError::Unavailable)
    }
}

/// An external USD asset registered in the PSM.
#[freeze_struct("3ad6fadd2b61882a")]
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen,
    TypeInfo,
)]
pub struct PsmAsset {
    /// The canonical ERC-20 contract of this asset on the Bittensor EVM.
    pub erc20: H160,
    /// Inflow rate limit (per-block refill).
    pub window: RateWindow,
    /// Haircut applied on deposit, in basis points.
    pub haircut_bps: u16,
    /// Reserves currently attributed to this asset (ERC-20 units held by the
    /// escrow that back outstanding tUSD).
    pub reserves: u64,
    /// Deposits enabled.
    pub enabled: bool,
}

#[deny(missing_docs)]
#[frame_support::pallet]
#[allow(clippy::expect_used)]
pub mod pallet {
    use super::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// Configuration trait.
    #[pallet::config]
    pub trait Config: frame_system::Config<AccountId = AccountId32> {
        /// TAO currency (the Balances pallet).
        type Currency: Inspect<Self::AccountId, Balance = TaoBalance>
            + Mutate<Self::AccountId, Balance = TaoBalance>;

        /// Staking engine bridge, implemented in the runtime over
        /// `pallet-subtensor`.
        type Staking: RailsStaking<Self::AccountId>;

        /// Outbound bridge leg, implemented in the runtime over
        /// `pallet-ethereum`.
        type Outbound: RailsOutbound;

        /// Origin allowed to administer the PSM (register assets, set the
        /// gateway, initialize the pool).
        type AdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Pallet account holding the pool's TAO side (protocol-owned
        /// liquidity).
        #[pallet::constant]
        type PalletId: Get<PalletId>;
    }

    /// Per-account tUSD balances (9 decimals). Non-transferable in v1.
    #[pallet::storage]
    pub type TUsdBalances<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    /// Total tUSD issued (user ledger + pool reserve).
    #[pallet::storage]
    pub type TUsdTotalIssuance<T> = StorageValue<_, u64, ValueQuery>;

    /// PSM asset registry.
    #[pallet::storage]
    pub type PsmAssets<T> = StorageMap<_, Twox64Concat, UsdAssetId, PsmAsset, OptionQuery>;

    /// The registered Gateway contract on the Bittensor EVM. Only calls
    /// relayed by this contract may execute gateway envelopes.
    #[pallet::storage]
    pub type Gateway<T> = StorageValue<_, H160, OptionQuery>;

    /// The next inbound envelope nonce expected. Envelopes execute in strict
    /// nonce order: lower nonces are replays, higher nonces wait (delivery
    /// reverts and the relayer retries once the gap fills).
    #[pallet::storage]
    pub type NextNonce<T> = StorageValue<_, u64, ValueQuery>;

    /// Receipts of executed inbound envelopes, keyed by nonce (for status
    /// queries; the ordering guard is [`NextNonce`]).
    #[pallet::storage]
    pub type ProcessedNonces<T> = StorageMap<_, Twox64Concat, u64, InboundReceipt, OptionQuery>;

    /// TAO reserve of the canonical pool (mirrors the pallet account balance).
    #[pallet::storage]
    pub type PoolTaoReserve<T> = StorageValue<_, u64, ValueQuery>;

    /// tUSD reserve of the canonical pool.
    #[pallet::storage]
    pub type PoolTUsdReserve<T> = StorageValue<_, u64, ValueQuery>;

    /// Pool swap fee in basis points.
    #[pallet::storage]
    pub type PoolFeeBps<T> = StorageValue<_, u16, ValueQuery>;

    /// The Hyperlane Mailbox contract on the Bittensor EVM used for
    /// runtime-originated (outbound) messages.
    #[pallet::storage]
    pub type HubMailbox<T> = StorageValue<_, H160, OptionQuery>;

    /// Outbound routes: (destination domain, netuid) -> remote canonical
    /// share token (bytes32 Hyperlane recipient).
    #[pallet::storage]
    pub type RemoteRoutes<T> =
        StorageDoubleMap<_, Twox64Concat, u32, Twox64Concat, NetUid, [u8; 32], OptionQuery>;

    /// Canonical shares outstanding per netuid. The share index is
    /// `escrowed alpha / shares outstanding` where escrowed alpha is read
    /// live from the staking engine (so it grows with emissions).
    #[pallet::storage]
    pub type SharesOutstanding<T> = StorageMap<_, Twox64Concat, NetUid, u64, ValueQuery>;

    /// The hotkey the hub escrow stakes into, per netuid. Buys stake here;
    /// sells unstake from here.
    #[pallet::storage]
    pub type EscrowHotkeys<T: Config> =
        StorageMap<_, Twox64Concat, NetUid, T::AccountId, OptionQuery>;

    /// USD release routes: destination domain -> the spoke contract (bytes32
    /// Hyperlane recipient) that pays out USD on sells.
    #[pallet::storage]
    pub type UsdReleaseRoutes<T> = StorageMap<_, Twox64Concat, u32, [u8; 32], OptionQuery>;

    /// Default heartbeat interval (blocks) for pushing the share index.
    #[pallet::type_value]
    pub fn DefaultHeartbeatInterval<T: Config>() -> u32 {
        10
    }

    /// How often (in blocks) the share index is pushed to spokes without a
    /// triggering mint or burn.
    #[pallet::storage]
    pub type HeartbeatInterval<T: Config> =
        StorageValue<_, u32, ValueQuery, DefaultHeartbeatInterval<T>>;

    /// Outbound messages queued for dispatch in `on_idle`. Gateway envelopes
    /// execute inside an inbound EVM transaction (Mailbox -> Gateway ->
    /// precompile), and pallet-evm's reentrancy guard forbids dispatching a
    /// second EVM transaction from that context. Each entry is
    /// `(mailbox, destination domain, recipient route, message body)`.
    #[pallet::storage]
    #[pallet::unbounded]
    pub type OutboundQueue<T> =
        StorageValue<_, Vec<(H160, u32, [u8; 32], Vec<u8>)>, ValueQuery>;

    /// Events.
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A USD asset was registered or updated in the PSM.
        UsdAssetRegistered {
            /// PSM asset id.
            asset_id: UsdAssetId,
            /// ERC-20 contract address.
            erc20: H160,
        },
        /// The Gateway contract address was set.
        GatewaySet {
            /// Gateway H160.
            gateway: H160,
        },
        /// The canonical pool was initialized.
        PoolInitialized {
            /// TAO side.
            tao: u64,
            /// tUSD side.
            tusd: u64,
            /// Swap fee in basis points.
            fee_bps: u16,
        },
        /// tUSD was credited to an account.
        TUsdCredited {
            /// Beneficiary.
            who: T::AccountId,
            /// Amount credited.
            amount: u64,
        },
        /// tUSD was debited from an account.
        TUsdDebited {
            /// Account debited.
            who: T::AccountId,
            /// Amount debited.
            amount: u64,
        },
        /// tUSD was swapped for TAO through the canonical pool.
        SwappedTUsdForTao {
            /// Swapper.
            who: T::AccountId,
            /// tUSD in.
            tusd_in: u64,
            /// TAO out.
            tao_out: u64,
        },
        /// TAO was swapped for tUSD through the canonical pool.
        SwappedTaoForTUsd {
            /// Swapper.
            who: T::AccountId,
            /// TAO in.
            tao_in: u64,
            /// tUSD out.
            tusd_out: u64,
        },
        /// An inbound gateway envelope was executed.
        GatewayExecuted {
            /// Envelope nonce.
            nonce: u64,
            /// Destination account.
            dest: T::AccountId,
            /// Amount deposited (gross, before haircut).
            amount: u64,
            /// `None` if the requested action ran; `Some` if the fallback
            /// credited tUSD instead.
            fallback: Option<FallbackReason>,
        },
        /// The hub Mailbox address was set.
        HubMailboxSet {
            /// Mailbox H160 on the Bittensor EVM.
            mailbox: H160,
        },
        /// An outbound route was set.
        OutboundRouteSet {
            /// Destination Hyperlane domain.
            domain: u32,
            /// Subnet whose alpha the remote token wraps.
            netuid: NetUid,
            /// Remote canonical share token (bytes32 recipient).
            recipient: [u8; 32],
        },
        /// A USD release route was set for a domain.
        UsdRouteSet {
            /// Destination Hyperlane domain.
            domain: u32,
            /// Spoke contract (bytes32 recipient) paying out USD on sells.
            recipient: [u8; 32],
        },
        /// The escrow hotkey for a netuid was set.
        EscrowHotkeySet {
            /// Subnet.
            netuid: NetUid,
            /// Hotkey the hub escrow stakes into.
            hotkey: T::AccountId,
        },
        /// The heartbeat interval was set.
        HeartbeatIntervalSet {
            /// Blocks between index pushes.
            blocks: u32,
        },
        /// A buy executed: USD in, alpha staked into escrow, shares minted
        /// on the spoke.
        SharesBought {
            /// EVM address receiving the shares on the spoke.
            recipient: H160,
            /// Subnet.
            netuid: NetUid,
            /// tUSD consumed (post-haircut deposit).
            usd_in: u64,
            /// Alpha staked into the hub escrow.
            alpha_staked: u64,
            /// Shares minted.
            shares: u64,
            /// Share index (1e9 fixed point) after the buy.
            index_e9: u64,
            /// Destination domain.
            domain: u32,
        },
        /// A sell executed: shares burned on the spoke, alpha unstaked,
        /// USD released.
        SharesSold {
            /// EVM address receiving the USD on the spoke.
            recipient: H160,
            /// Subnet.
            netuid: NetUid,
            /// Shares burned.
            shares: u64,
            /// Alpha unstaked from the hub escrow.
            alpha_unstaked: u64,
            /// USD released to the spoke.
            usd_out: u64,
            /// Destination domain.
            domain: u32,
        },
        /// The share index was pushed to a spoke (heartbeat).
        IndexPushed {
            /// Subnet.
            netuid: NetUid,
            /// Destination domain.
            domain: u32,
            /// Share index (1e9 fixed point).
            index_e9: u64,
        },
    }

    /// Errors.
    #[pallet::error]
    pub enum Error<T> {
        /// The PSM asset id is not registered.
        AssetUnknown,
        /// The PSM asset is disabled.
        AssetDisabled,
        /// The inflow rate window has no headroom for this amount.
        CapExceeded,
        /// The envelope nonce is below the sequential counter (replay).
        NonceReplayed,
        /// The envelope nonce is ahead of the sequential counter; delivery
        /// reverts and the relayer retries once the gap fills.
        NonceOutOfOrder,
        /// The envelope bytes could not be decoded.
        BadEnvelope,
        /// The envelope's internal amount does not match the secured amount.
        AmountMismatch,
        /// No Gateway contract is registered.
        GatewayNotSet,
        /// The caller is not the registered Gateway contract.
        NotGateway,
        /// The canonical pool has not been initialized.
        PoolNotInitialized,
        /// The pool is already initialized.
        PoolAlreadyInitialized,
        /// The account's tUSD balance is insufficient.
        InsufficientTUsd,
        /// The pool cannot cover the requested output.
        InsufficientLiquidity,
        /// The swap output is below the caller's minimum.
        SlippageExceeded,
        /// PSM reserves cannot cover the requested withdrawal.
        InsufficientReserves,
        /// Amounts must be non-zero.
        AmountZero,
        /// Arithmetic overflow in pool math.
        Overflow,
        /// No hub Mailbox is configured for outbound messages.
        HubNotConfigured,
        /// No outbound route exists for this (domain, netuid).
        RouteNotSet,
        /// No escrow hotkey is configured for this netuid.
        EscrowHotkeyNotSet,
        /// The sell burns more shares than are outstanding.
        InsufficientShares,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Flush queued outbound messages (minted shares, USD releases) and
        /// push the live share index to every routed spoke on a fixed block
        /// cadence, so rebasing balances tick without any user action.
        fn on_idle(now: BlockNumberFor<T>, _remaining_weight: Weight) -> Weight {
            Self::flush_outbound().saturating_add(Self::heartbeat(now))
        }
    }

    /// Dispatchable calls.
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Register (or update) a USD asset in the PSM.
        #[pallet::call_index(0)]
        #[pallet::weight((Weight::from_parts(20_000_000, 0), DispatchClass::Operational))]
        pub fn register_usd_asset(
            origin: OriginFor<T>,
            asset_id: UsdAssetId,
            erc20: H160,
            cap_limit: u64,
            refill_per_block: u64,
            haircut_bps: u16,
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            PsmAssets::<T>::mutate(asset_id, |slot| {
                let reserves = slot.map(|a| a.reserves).unwrap_or_default();
                let mut window = RateWindow::new(cap_limit, refill_per_block);
                if let Some(existing) = slot {
                    window.used = existing.window.used;
                    window.last_update_block = existing.window.last_update_block;
                }
                *slot = Some(PsmAsset {
                    erc20,
                    window,
                    haircut_bps,
                    reserves,
                    enabled: true,
                });
            });
            Self::deposit_event(Event::UsdAssetRegistered { asset_id, erc20 });
            Ok(())
        }

        /// Enable or disable deposits for a PSM asset.
        #[pallet::call_index(1)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_asset_enabled(
            origin: OriginFor<T>,
            asset_id: UsdAssetId,
            enabled: bool,
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            PsmAssets::<T>::try_mutate(asset_id, |slot| {
                let asset = slot.as_mut().ok_or(Error::<T>::AssetUnknown)?;
                asset.enabled = enabled;
                Ok(())
            })
        }

        /// Register the Gateway contract address.
        #[pallet::call_index(2)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_gateway(origin: OriginFor<T>, gateway: H160) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            Gateway::<T>::put(gateway);
            Self::deposit_event(Event::GatewaySet { gateway });
            Ok(())
        }

        /// Initialize the canonical tUSD/TAO pool with protocol-owned
        /// liquidity: TAO is moved from `funder` to the pool account and the
        /// tUSD side is issued as a protocol liability backed by that TAO.
        #[pallet::call_index(3)]
        #[pallet::weight((Weight::from_parts(50_000_000, 0), DispatchClass::Operational))]
        pub fn init_pool(
            origin: OriginFor<T>,
            funder: T::AccountId,
            tao: u64,
            tusd: u64,
            fee_bps: u16,
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            ensure!(tao > 0 && tusd > 0, Error::<T>::AmountZero);
            ensure!(
                PoolTaoReserve::<T>::get() == 0 && PoolTUsdReserve::<T>::get() == 0,
                Error::<T>::PoolAlreadyInitialized
            );
            ensure!(fee_bps < 10_000, Error::<T>::Overflow);

            T::Currency::transfer(
                &funder,
                &Self::pool_account(),
                TaoBalance::from(tao),
                Preservation::Expendable,
            )?;
            PoolTaoReserve::<T>::put(tao);
            PoolTUsdReserve::<T>::put(tusd);
            TUsdTotalIssuance::<T>::mutate(|total| *total = total.saturating_add(tusd));
            PoolFeeBps::<T>::put(fee_bps);
            Self::deposit_event(Event::PoolInitialized { tao, tusd, fee_bps });
            Ok(())
        }

        // Call indices 4-7 retired: public pool swaps and hub-side products
        // (swap_tusd_for_tao, swap_tao_for_tusd, swap_usd_and_stake,
        // unstake_and_swap_to_usd). The pool is internal-only now.

        /// Set the Bittensor-EVM Mailbox used for outbound messages.
        #[pallet::call_index(8)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_hub_mailbox(origin: OriginFor<T>, mailbox: H160) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            HubMailbox::<T>::put(mailbox);
            Self::deposit_event(Event::HubMailboxSet { mailbox });
            Ok(())
        }

        /// Set the outbound route for (`domain`, `netuid`): the remote
        /// canonical share token to mint on wrap.
        #[pallet::call_index(9)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_outbound_route(
            origin: OriginFor<T>,
            domain: u32,
            netuid: NetUid,
            recipient: [u8; 32],
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            RemoteRoutes::<T>::insert(domain, netuid, recipient);
            Self::deposit_event(Event::OutboundRouteSet {
                domain,
                netuid,
                recipient,
            });
            Ok(())
        }

        /// Set the USD release route for a domain: the spoke contract that
        /// pays out USD on sells.
        #[pallet::call_index(10)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_usd_route(
            origin: OriginFor<T>,
            domain: u32,
            recipient: [u8; 32],
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            UsdReleaseRoutes::<T>::insert(domain, recipient);
            Self::deposit_event(Event::UsdRouteSet { domain, recipient });
            Ok(())
        }

        /// Set the hotkey the hub escrow stakes into for `netuid`.
        #[pallet::call_index(11)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_escrow_hotkey(
            origin: OriginFor<T>,
            netuid: NetUid,
            hotkey: T::AccountId,
        ) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            EscrowHotkeys::<T>::insert(netuid, &hotkey);
            Self::deposit_event(Event::EscrowHotkeySet { netuid, hotkey });
            Ok(())
        }

        /// Set the heartbeat interval (blocks) for index pushes.
        #[pallet::call_index(12)]
        #[pallet::weight((Weight::from_parts(15_000_000, 0), DispatchClass::Operational))]
        pub fn set_heartbeat_interval(origin: OriginFor<T>, blocks: u32) -> DispatchResult {
            T::AdminOrigin::ensure_origin(origin)?;
            ensure!(blocks > 0, Error::<T>::AmountZero);
            HeartbeatInterval::<T>::put(blocks);
            Self::deposit_event(Event::HeartbeatIntervalSet { blocks });
            Ok(())
        }
    }
}

impl<T: Config> Pallet<T> {
    /// The pool's protocol-owned-liquidity account.
    pub fn pool_account() -> T::AccountId {
        T::PalletId::get().into_account_truncating()
    }

    /// The hub escrow account: its staked alpha backs all remote share
    /// supply.
    pub fn hub_account() -> T::AccountId {
        T::PalletId::get().into_sub_account_truncating(b"hub_")
    }

    /// The keyless EVM identity of the runtime hub: the `from` address of
    /// outbound Mailbox transactions and the trusted `hubSender` configured
    /// on remote canonical share tokens.
    pub fn hub_evm_address() -> H160 {
        H160::from_slice(
            sp_io::hashing::blake2_256(b"rails/hub-evm")
                .get(..20)
                .expect("blake2_256 output is 32 bytes; qed"),
        )
    }

    /// ABI-encode the spoke share-token message:
    /// `abi.encode(address to, uint64 shares, uint64 indexE9)`. A zero `to`
    /// with zero `shares` is a pure index update (heartbeat).
    fn abi_encode_share_msg(to: H160, shares: u64, index_e9: u64) -> Vec<u8> {
        let mut body = Vec::with_capacity(96);
        body.extend_from_slice(&[0u8; 12]);
        body.extend_from_slice(to.as_bytes());
        body.extend_from_slice(&[0u8; 24]);
        body.extend_from_slice(&shares.to_be_bytes());
        body.extend_from_slice(&[0u8; 24]);
        body.extend_from_slice(&index_e9.to_be_bytes());
        body
    }

    /// ABI-encode the spoke USD-release body: `abi.encode(address, uint64)`.
    fn abi_encode_usd_release(to: H160, amount: u64) -> Vec<u8> {
        let mut body = Vec::with_capacity(64);
        body.extend_from_slice(&[0u8; 12]);
        body.extend_from_slice(to.as_bytes());
        body.extend_from_slice(&[0u8; 24]);
        body.extend_from_slice(&amount.to_be_bytes());
        body
    }

    /// Live alpha in the hub escrow for `netuid` (grows with emissions).
    pub fn escrowed_alpha(netuid: NetUid) -> u64 {
        match EscrowHotkeys::<T>::get(netuid) {
            Some(hotkey) => T::Staking::stake_of(&hotkey, &Self::hub_account(), netuid),
            None => 0,
        }
    }

    /// The share index in 1e9 fixed point: escrowed alpha per share. With no
    /// shares outstanding the index is 1.0 (1e9).
    pub fn share_index_e9(netuid: NetUid) -> u64 {
        let shares = SharesOutstanding::<T>::get(netuid);
        if shares == 0 {
            return 1_000_000_000;
        }
        u128::from(Self::escrowed_alpha(netuid))
            .saturating_mul(1_000_000_000)
            .checked_div(u128::from(shares))
            .and_then(|x| u64::try_from(x).ok())
            .unwrap_or(u64::MAX)
    }

    /// Supply attestation for `netuid`: (live escrowed alpha, shares
    /// outstanding, share index). Escrowed alpha >= shares * index / 1e9
    /// always holds (rounding aside), and the surplus is the yield not yet
    /// captured by an index push.
    pub fn alpha_attestation(netuid: NetUid) -> (u64, u64, u64) {
        (
            Self::escrowed_alpha(netuid),
            SharesOutstanding::<T>::get(netuid),
            Self::share_index_e9(netuid),
        )
    }

    /// Next expected inbound envelope nonce.
    pub fn next_nonce() -> u64 {
        NextNonce::<T>::get()
    }

    /// Queue an outbound mailbox message for dispatch in the next `on_idle`.
    /// Gateway actions run inside an inbound EVM transaction, where a nested
    /// EVM dispatch would trip pallet-evm's reentrancy guard.
    fn queue_outbound(mailbox: H160, domain: u32, recipient: [u8; 32], body: Vec<u8>) {
        OutboundQueue::<T>::mutate(|q| q.push((mailbox, domain, recipient, body)));
    }

    /// Dispatch every queued outbound message. Failures stay queued and are
    /// retried next block (the queue only ever holds messages the runtime
    /// itself produced, so a persistent failure means the rig is miswired).
    fn flush_outbound() -> Weight {
        let base = Weight::from_parts(10_000_000, 0);
        let queued = OutboundQueue::<T>::take();
        if queued.is_empty() {
            return base;
        }
        let mut weight = base;
        let mut retry: Vec<(H160, u32, [u8; 32], Vec<u8>)> = Vec::new();
        for (mailbox, domain, recipient, body) in queued {
            weight = weight.saturating_add(Weight::from_parts(200_000_000, 0));
            if let Err(e) = T::Outbound::dispatch_mailbox(
                mailbox,
                Self::hub_evm_address(),
                domain,
                recipient,
                body.clone(),
            ) {
                log::warn!(target: "rails", "outbound dispatch to domain {domain} failed, retrying next block: {e:?}");
                retry.push((mailbox, domain, recipient, body));
            }
        }
        if !retry.is_empty() {
            OutboundQueue::<T>::put(retry);
        }
        weight
    }

    /// Push the current share index to every routed spoke every
    /// [`HeartbeatInterval`] blocks.
    fn heartbeat(now: BlockNumberFor<T>) -> Weight {
        let base = Weight::from_parts(10_000_000, 0);
        let interval = HeartbeatInterval::<T>::get().max(1);
        let now_u32: u32 = now.try_into().unwrap_or_default();
        if !now_u32.is_multiple_of(interval) {
            return base;
        }
        let Some(mailbox) = HubMailbox::<T>::get() else {
            return base;
        };
        let mut weight = base;
        for (domain, netuid, route) in RemoteRoutes::<T>::iter() {
            weight = weight.saturating_add(Weight::from_parts(200_000_000, 0));
            if SharesOutstanding::<T>::get(netuid) == 0 {
                continue;
            }
            let index_e9 = Self::share_index_e9(netuid);
            let body = Self::abi_encode_share_msg(H160::zero(), 0, index_e9);
            if T::Outbound::dispatch_mailbox(
                mailbox,
                Self::hub_evm_address(),
                domain,
                route,
                body,
            )
            .is_ok()
            {
                Self::deposit_event(Event::IndexPushed {
                    netuid,
                    domain,
                    index_e9,
                });
            }
        }
        weight
    }

    /// Frontier's `HashedAddressMapping`: the substrate account owned by an
    /// EVM address.
    pub fn evm_account(address: &H160) -> T::AccountId {
        let mut data = [0u8; 24];
        if let Some(prefix) = data.get_mut(..4) {
            prefix.copy_from_slice(b"evm:");
        }
        if let Some(body) = data.get_mut(4..) {
            body.copy_from_slice(address.as_bytes());
        }
        AccountId32::new(sp_io::hashing::blake2_256(&data))
    }

    /// tUSD balance of an account.
    pub fn tusd_balance(who: &T::AccountId) -> u64 {
        TUsdBalances::<T>::get(who)
    }

    /// PSM asset lookup (for the precompile: which ERC-20 backs an asset id).
    pub fn psm_asset(asset_id: UsdAssetId) -> Option<PsmAsset> {
        PsmAssets::<T>::get(asset_id)
    }

    /// The registered Gateway contract.
    pub fn gateway() -> Option<H160> {
        Gateway::<T>::get()
    }

    /// Quote tUSD -> TAO through the canonical pool.
    pub fn quote_tusd_for_tao(amount_in: u64) -> Option<u64> {
        Self::quote(
            PoolTUsdReserve::<T>::get(),
            PoolTaoReserve::<T>::get(),
            amount_in,
        )
    }

    /// Quote TAO -> tUSD through the canonical pool.
    pub fn quote_tao_for_tusd(amount_in: u64) -> Option<u64> {
        Self::quote(
            PoolTaoReserve::<T>::get(),
            PoolTUsdReserve::<T>::get(),
            amount_in,
        )
    }

    /// Constant-product quote with the pool fee applied on input.
    fn quote(reserve_in: u64, reserve_out: u64, amount_in: u64) -> Option<u64> {
        if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
            return None;
        }
        let fee_bps = u128::from(PoolFeeBps::<T>::get());
        let in_net = u128::from(amount_in)
            .checked_mul(10_000u128.checked_sub(fee_bps)?)?
            .checked_div(10_000)?;
        let numerator = u128::from(reserve_out).checked_mul(in_net)?;
        let denominator = u128::from(reserve_in).checked_add(in_net)?;
        let out = numerator.checked_div(denominator)?;
        u64::try_from(out).ok()
    }

    fn credit_tusd(who: &T::AccountId, amount: u64) {
        if amount == 0 {
            return;
        }
        TUsdBalances::<T>::mutate(who, |b| *b = b.saturating_add(amount));
        TUsdTotalIssuance::<T>::mutate(|total| *total = total.saturating_add(amount));
        Self::deposit_event(Event::TUsdCredited {
            who: who.clone(),
            amount,
        });
    }

    fn debit_tusd(who: &T::AccountId, amount: u64) -> DispatchResult {
        TUsdBalances::<T>::try_mutate(who, |b| {
            *b = b.checked_sub(amount).ok_or(Error::<T>::InsufficientTUsd)?;
            Ok::<(), DispatchError>(())
        })?;
        TUsdTotalIssuance::<T>::mutate(|total| *total = total.saturating_sub(amount));
        Self::deposit_event(Event::TUsdDebited {
            who: who.clone(),
            amount,
        });
        Ok(())
    }

    /// Swap tUSD (ledger) for TAO (free balance). Returns TAO out.
    pub fn do_swap_tusd_for_tao(
        who: &T::AccountId,
        amount_in: u64,
        min_out: u64,
    ) -> Result<u64, DispatchError> {
        ensure!(amount_in > 0, Error::<T>::AmountZero);
        let reserve_tusd = PoolTUsdReserve::<T>::get();
        let reserve_tao = PoolTaoReserve::<T>::get();
        ensure!(
            reserve_tusd > 0 && reserve_tao > 0,
            Error::<T>::PoolNotInitialized
        );
        let out = Self::quote(reserve_tusd, reserve_tao, amount_in).ok_or(Error::<T>::Overflow)?;
        ensure!(out >= min_out, Error::<T>::SlippageExceeded);
        ensure!(out < reserve_tao, Error::<T>::InsufficientLiquidity);

        Self::debit_tusd(who, amount_in)?;
        // Debited tUSD becomes pool reserve (still issued, owned by the pool).
        TUsdTotalIssuance::<T>::mutate(|total| *total = total.saturating_add(amount_in));
        PoolTUsdReserve::<T>::put(reserve_tusd.saturating_add(amount_in));
        PoolTaoReserve::<T>::put(reserve_tao.saturating_sub(out));
        T::Currency::transfer(
            &Self::pool_account(),
            who,
            TaoBalance::from(out),
            Preservation::Expendable,
        )?;
        Self::deposit_event(Event::SwappedTUsdForTao {
            who: who.clone(),
            tusd_in: amount_in,
            tao_out: out,
        });
        Ok(out)
    }

    /// Swap TAO (free balance) for tUSD (ledger). Returns tUSD out.
    pub fn do_swap_tao_for_tusd(
        who: &T::AccountId,
        amount_in: u64,
        min_out: u64,
    ) -> Result<u64, DispatchError> {
        ensure!(amount_in > 0, Error::<T>::AmountZero);
        let reserve_tao = PoolTaoReserve::<T>::get();
        let reserve_tusd = PoolTUsdReserve::<T>::get();
        ensure!(
            reserve_tusd > 0 && reserve_tao > 0,
            Error::<T>::PoolNotInitialized
        );
        let out = Self::quote(reserve_tao, reserve_tusd, amount_in).ok_or(Error::<T>::Overflow)?;
        ensure!(out >= min_out, Error::<T>::SlippageExceeded);
        ensure!(out < reserve_tusd, Error::<T>::InsufficientLiquidity);

        T::Currency::transfer(
            who,
            &Self::pool_account(),
            TaoBalance::from(amount_in),
            Preservation::Expendable,
        )?;
        PoolTaoReserve::<T>::put(reserve_tao.saturating_add(amount_in));
        PoolTUsdReserve::<T>::put(reserve_tusd.saturating_sub(out));
        // Pool reserve tUSD leaves the pool ledger and enters the user ledger:
        // net issuance change is zero (debit pool bucket, credit user).
        TUsdTotalIssuance::<T>::mutate(|total| *total = total.saturating_sub(out));
        Self::credit_tusd(who, out);
        Self::deposit_event(Event::SwappedTaoForTUsd {
            who: who.clone(),
            tao_in: amount_in,
            tusd_out: out,
        });
        Ok(out)
    }

    /// Execute an inbound gateway envelope. `caller` is the EVM address that
    /// invoked the precompile — it must be the registered Gateway contract.
    ///
    /// Errors returned here revert the bridge delivery (funds remain secured
    /// at origin / the mint is rolled back with the EVM frame, and the
    /// relayer retries). Once the deposit is accounted, the requested action
    /// never reverts: failures fall back to a plain tUSD credit.
    pub fn do_gateway_execute(caller: H160, amount: u64, envelope_bytes: &[u8]) -> DispatchResult {
        let gateway = Gateway::<T>::get().ok_or(Error::<T>::GatewayNotSet)?;
        ensure!(caller == gateway, Error::<T>::NotGateway);

        let envelope =
            GatewayEnvelope::from_wire(envelope_bytes).map_err(|_| Error::<T>::BadEnvelope)?;
        ensure!(envelope.amount == amount, Error::<T>::AmountMismatch);

        // Sequential ordering guard: the envelope must carry exactly the
        // next expected nonce. Replays and gaps both revert the delivery
        // (the relayer retries out-of-order messages until the gap fills).
        let next = NextNonce::<T>::get();
        ensure!(envelope.nonce >= next, Error::<T>::NonceReplayed);
        ensure!(envelope.nonce == next, Error::<T>::NonceOutOfOrder);

        let (dest, fallback) = match &envelope.action {
            GatewayAction::SellShares {
                netuid,
                recipient,
                usd_asset,
                min_usd,
                domain,
            } => {
                // Sell path: shares were already burned on the spoke; the
                // envelope amount is the share count, no USD is deposited.
                // A failure is recorded in the receipt for follow-up.
                let recipient = H160::from(*recipient);
                let fb = Self::run_sell_shares(
                    recipient, *netuid, amount, *usd_asset, *min_usd, *domain,
                );
                (Self::evm_account(&recipient), fb)
            }
            action => {
                // Deposit path: secure the accounting (asset window +
                // reserves + haircut), credit tUSD, then run the action.
                // From the credit on, never revert: failures fall back to
                // the tUSD credit. Buys credit the buyer's mirror account so
                // a failed buy stays recoverable from MetaMask.
                let dest: T::AccountId = match action {
                    GatewayAction::BuyShares { recipient, .. } => {
                        Self::evm_account(&H160::from(*recipient))
                    }
                    _ => envelope.dest.clone(),
                };
                let credited = if amount > 0 {
                    let AssetId::Usd(asset_id) = envelope.asset else {
                        return Err(Error::<T>::AssetUnknown.into());
                    };
                    Self::psm_account_inflow(asset_id, amount)?
                } else {
                    0
                };
                Self::credit_tusd(&dest, credited);
                let fb = Self::run_gateway_action(&dest, credited, action);
                (dest, fb)
            }
        };

        NextNonce::<T>::put(next.saturating_add(1));
        let now = frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or_default();
        ProcessedNonces::<T>::insert(
            envelope.nonce,
            InboundReceipt {
                block: now,
                fallback,
            },
        );
        Self::deposit_event(Event::GatewayExecuted {
            nonce: envelope.nonce,
            dest,
            amount,
            fallback,
        });
        Ok(())
    }

    /// Run the envelope's requested action on top of the tUSD credit.
    /// Returns `None` on success or the fallback reason (tUSD stays credited).
    fn run_gateway_action(
        dest: &T::AccountId,
        credited: u64,
        action: &GatewayAction,
    ) -> Option<FallbackReason> {
        match action {
            GatewayAction::CreditTUsd => None,
            GatewayAction::Stake {
                netuid,
                hotkey,
                min_alpha,
            } => {
                let hotkey: T::AccountId = hotkey.clone();
                let result: DispatchResult = with_storage_layer(|| {
                    let tao_out = Self::do_swap_tusd_for_tao(dest, credited, 0)?;
                    T::Staking::stake(dest, &hotkey, *netuid, TaoBalance::from(tao_out), *min_alpha)?;
                    Ok(())
                });
                result.err().map(|e| {
                    log::warn!(target: "rails", "gateway stake failed: {e:?}");
                    FallbackReason::StakeFailed
                })
            }
            GatewayAction::BuyShares {
                netuid,
                recipient,
                min_alpha,
                domain,
            } => {
                let result: DispatchResult = with_storage_layer(|| {
                    Self::run_buy_shares(
                        dest,
                        H160::from(*recipient),
                        *netuid,
                        credited,
                        *min_alpha,
                        *domain,
                    )
                });
                result.err().map(|e| {
                    log::warn!(target: "rails", "gateway buy failed: {e:?}");
                    FallbackReason::BuyFailed
                })
            }
            // Handled before the deposit path in `do_gateway_execute`.
            GatewayAction::SellShares { .. } => Some(FallbackReason::UnknownAction),
            // Forward compatibility: an action variant added by a newer
            // client falls back to the tUSD credit instead of bouncing.
            _ => Some(FallbackReason::UnknownAction),
        }
    }

    /// The buy pipeline, on top of tUSD already credited to `buyer` (the
    /// recipient's mirror account): pool swap -> stake into the hub escrow
    /// -> mint shares at the pre-stake index -> dispatch the mint (with the
    /// post-stake index) to the spoke token.
    fn run_buy_shares(
        buyer: &T::AccountId,
        recipient: H160,
        netuid: NetUid,
        credited: u64,
        min_alpha: AlphaBalance,
        domain: u32,
    ) -> DispatchResult {
        ensure!(credited > 0, Error::<T>::AmountZero);
        let mailbox = HubMailbox::<T>::get().ok_or(Error::<T>::HubNotConfigured)?;
        let route = RemoteRoutes::<T>::get(domain, netuid).ok_or(Error::<T>::RouteNotSet)?;
        let escrow_hotkey =
            EscrowHotkeys::<T>::get(netuid).ok_or(Error::<T>::EscrowHotkeyNotSet)?;

        let hub = Self::hub_account();
        let tao_out = Self::do_swap_tusd_for_tao(buyer, credited, 0)?;
        T::Currency::transfer(
            buyer,
            &hub,
            TaoBalance::from(tao_out),
            Preservation::Expendable,
        )?;

        // Shares are priced at the index *before* this stake lands, so the
        // buyer pays the current alpha-per-share; the new alpha then backs
        // exactly the new shares.
        let index_before = Self::share_index_e9(netuid).max(1);
        let alpha: u64 = T::Staking::stake(
            &hub,
            &escrow_hotkey,
            netuid,
            TaoBalance::from(tao_out),
            min_alpha,
        )?
        .into();
        let shares = u128::from(alpha)
            .saturating_mul(1_000_000_000)
            .checked_div(u128::from(index_before))
            .and_then(|x| u64::try_from(x).ok())
            .ok_or(Error::<T>::Overflow)?;
        ensure!(shares > 0, Error::<T>::AmountZero);
        SharesOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_add(shares));

        let index_after = Self::share_index_e9(netuid);
        Self::queue_outbound(
            mailbox,
            domain,
            route,
            Self::abi_encode_share_msg(recipient, shares, index_after),
        );

        Self::deposit_event(Event::SharesBought {
            recipient,
            netuid,
            usd_in: credited,
            alpha_staked: alpha,
            shares,
            index_e9: index_after,
            domain,
        });
        Ok(())
    }

    /// The sell pipeline: unstake escrowed alpha worth `shares` at the
    /// current index, swap TAO -> tUSD, burn the tUSD against PSM reserves,
    /// and dispatch a USD release to the spoke. Returns the fallback reason
    /// on failure (shares were already burned remotely; nothing is credited
    /// and the receipt records it).
    fn run_sell_shares(
        recipient: H160,
        netuid: NetUid,
        shares: u64,
        usd_asset: UsdAssetId,
        min_usd: u64,
        domain: u32,
    ) -> Option<FallbackReason> {
        let result: DispatchResult = with_storage_layer(|| {
            ensure!(shares > 0, Error::<T>::AmountZero);
            let mailbox = HubMailbox::<T>::get().ok_or(Error::<T>::HubNotConfigured)?;
            let usd_route =
                UsdReleaseRoutes::<T>::get(domain).ok_or(Error::<T>::RouteNotSet)?;
            let escrow_hotkey =
                EscrowHotkeys::<T>::get(netuid).ok_or(Error::<T>::EscrowHotkeyNotSet)?;
            let outstanding = SharesOutstanding::<T>::get(netuid);
            ensure!(shares <= outstanding, Error::<T>::InsufficientShares);

            let index_e9 = Self::share_index_e9(netuid);
            let alpha = u128::from(shares)
                .saturating_mul(u128::from(index_e9))
                .checked_div(1_000_000_000)
                .and_then(|x| u64::try_from(x).ok())
                .ok_or(Error::<T>::Overflow)?;
            ensure!(alpha > 0, Error::<T>::AmountZero);

            let hub = Self::hub_account();
            let tao_out = T::Staking::unstake(
                &hub,
                &escrow_hotkey,
                netuid,
                AlphaBalance::from(alpha),
                TaoBalance::from(0u64),
            )?;
            let tusd_out = Self::do_swap_tao_for_tusd(&hub, tao_out.into(), min_usd)?;
            Self::debit_tusd(&hub, tusd_out)?;
            Self::psm_account_outflow(usd_asset, tusd_out)?;
            SharesOutstanding::<T>::mutate(netuid, |s| *s = s.saturating_sub(shares));

            Self::queue_outbound(
                mailbox,
                domain,
                usd_route,
                Self::abi_encode_usd_release(recipient, tusd_out),
            );

            Self::deposit_event(Event::SharesSold {
                recipient,
                netuid,
                shares,
                alpha_unstaked: alpha,
                usd_out: tusd_out,
                domain,
            });
            Ok(())
        });
        result.err().map(|e| {
            log::warn!(target: "rails", "gateway sell failed: {e:?}");
            FallbackReason::SellFailed
        })
    }

    /// Account a PSM inflow: rate window, reserves, haircut. Returns the tUSD
    /// amount to credit.
    fn psm_account_inflow(asset_id: UsdAssetId, amount: u64) -> Result<u64, DispatchError> {
        let now: u32 = frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or_default();
        PsmAssets::<T>::try_mutate(asset_id, |slot| {
            let asset = slot.as_mut().ok_or(Error::<T>::AssetUnknown)?;
            ensure!(asset.enabled, Error::<T>::AssetDisabled);
            ensure!(
                asset.window.try_reserve(now, amount),
                Error::<T>::CapExceeded
            );
            asset.reserves = asset.reserves.saturating_add(amount);
            let haircut = u128::from(amount)
                .checked_mul(u128::from(asset.haircut_bps))
                .and_then(|x| x.checked_div(10_000))
                .and_then(|x| u64::try_from(x).ok())
                .ok_or(Error::<T>::Overflow)?;
            Ok(amount.saturating_sub(haircut))
        })
    }

    /// Account a PSM outflow: reserves down, window headroom released.
    fn psm_account_outflow(asset_id: UsdAssetId, amount: u64) -> DispatchResult {
        let now: u32 = frame_system::Pallet::<T>::block_number()
            .try_into()
            .unwrap_or_default();
        PsmAssets::<T>::try_mutate(asset_id, |slot| {
            let asset = slot.as_mut().ok_or(Error::<T>::AssetUnknown)?;
            asset.reserves = asset
                .reserves
                .checked_sub(amount)
                .ok_or(Error::<T>::InsufficientReserves)?;
            asset.window.release(now, amount);
            Ok(())
        })
    }

}
