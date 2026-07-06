#![allow(clippy::unwrap_used)]
//! Minimal mock runtime for `pallet-limit-orders` unit tests.
//!
//! `AccountId` is `sp_runtime::AccountId32` so that `MultiSignature` works
//! out of the box; test keys come from `sp_keyring::AccountKeyring`.

use std::cell::RefCell;
use std::collections::HashMap;

use codec::Encode;
use frame_support::{
    BoundedVec, PalletId, construct_runtime, derive_impl, parameter_types,
    traits::{ConstU32, ConstU64, Everything},
};
use frame_system as system;
use sp_core::{H256, Pair};
use sp_keyring::Sr25519Keyring as AccountKeyring;
use sp_runtime::{
    AccountId32, BuildStorage, MultiSignature,
    traits::{AccountIdConversion, BlakeTwo256, IdentityLookup},
};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::OrderSwapInterface;

use crate::{self as pallet_limit_orders, LimitOrdersEnabled};

// ── Runtime ──────────────────────────────────────────────────────────────────

construct_runtime!(
    pub enum Test {
        System: system = 0,
        LimitOrders: pallet_limit_orders = 1,
    }
);

pub type Block = frame_system::mocking::MockBlock<Test>;
pub type AccountId = AccountId32;

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BaseCallFilter = Everything;
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type PalletInfo = PalletInfo;
    type MaxConsumers = ConstU32<16>;
    type Nonce = u64;
    type Block = Block;
}

// ── MockSwap ─────────────────────────────────────────────────────────────────
//
// Records every call so tests can assert that the right transfers happened.

#[derive(Debug, Clone, PartialEq)]
pub enum SwapCall {
    BuyAlpha {
        coldkey: AccountId,
        hotkey: AccountId,
        netuid: NetUid,
        tao: u64,
        limit_price: u64,
    },
    SellAlpha {
        coldkey: AccountId,
        hotkey: AccountId,
        netuid: NetUid,
        alpha: u64,
        limit_price: u64,
    },
    TransferTao {
        from: AccountId,
        to: AccountId,
        amount: u64,
    },
    TransferStakedAlpha {
        from_coldkey: AccountId,
        from_hotkey: AccountId,
        to_coldkey: AccountId,
        to_hotkey: AccountId,
        netuid: NetUid,
        amount: u64,
    },
}

thread_local! {
    /// Log of every `OrderSwapInterface` call made during a test.
    pub static SWAP_LOG: RefCell<Vec<SwapCall>> = const { RefCell::new(Vec::new()) };
    /// Fixed price returned by `current_alpha_price` (default 1.0).
    pub static MOCK_PRICE: RefCell<U64F64> = RefCell::new(U64F64::from_num(1u32));
    /// Fixed alpha returned by `buy_alpha` (default 0 — tests override as needed).
    pub static MOCK_BUY_ALPHA_RETURN: RefCell<u64> = const { RefCell::new(0u64) };
    /// Fixed TAO returned by `sell_alpha` (default 0 — tests override as needed).
    pub static MOCK_SELL_TAO_RETURN: RefCell<u64> = const { RefCell::new(0u64) };
    /// In-memory staked alpha ledger: (coldkey, hotkey, netuid) → balance.
    /// `transfer_staked_alpha` debits/credits this map so tests can assert
    /// on residual balances after distribution.
    pub static ALPHA_BALANCES: RefCell<HashMap<(AccountId, AccountId, NetUid), u64>> =
        RefCell::new(HashMap::new());
    /// In-memory free TAO ledger: account → balance.
    /// `transfer_tao` debits/credits this map so tests can assert
    /// on residual balances after distribution.
    pub static TAO_BALANCES: RefCell<HashMap<AccountId, u64>> =
        RefCell::new(HashMap::new());
    /// When set to `true`, `transfer_tao` returns `Err(CannotLookup)` so
    /// tests can exercise the fee-transfer-failure path.
    pub static FAIL_FEE_TRANSFER: RefCell<bool> = const { RefCell::new(false) };
    /// When `true`, `buy_alpha` and `sell_alpha` return `DispatchError::Other("pool error")`.
    pub static MOCK_SWAP_FAIL: RefCell<bool> = const { RefCell::new(false) };
    /// When `true`, swap calls enforce their `limit_price` argument against `MOCK_PRICE`:
    /// `buy_alpha` fails if `market_price > limit_price` (ceiling exceeded);
    /// `sell_alpha` fails if `market_price < limit_price` (floor not met).
    pub static MOCK_ENFORCE_PRICE_LIMIT: RefCell<bool> = const { RefCell::new(false) };
    /// When `true`, `buy_alpha` and `sell_alpha` return a slippage error to simulate
    /// the case where the AMM price limit stops the swap before the full amount is consumed.
    pub static MOCK_SIMULATE_PARTIAL_FILL: RefCell<bool> = const { RefCell::new(false) };
    /// Rate-limit flags set by `transfer_staked_alpha` when `set_receiver_limit` is true.
    /// Key: (hotkey, coldkey, netuid) — mirrors `StakingOperationRateLimiter` in subtensor.
    pub static RATE_LIMITS: RefCell<std::collections::HashSet<(AccountId, AccountId, NetUid)>> =
        RefCell::new(std::collections::HashSet::new());
    /// Registered (coldkey, hotkey) ownership pairs — mirrors `Owner` storage in subtensor.
    pub static HOTKEY_REGISTRATIONS: RefCell<std::collections::HashSet<(AccountId, AccountId)>> =
        RefCell::new(std::collections::HashSet::new());
}

pub struct MockSwap;

impl MockSwap {
    pub fn set_price(price: f64) {
        MOCK_PRICE.with(|p| *p.borrow_mut() = U64F64::from_num(price));
    }
    pub fn set_buy_alpha_return(alpha: u64) {
        MOCK_BUY_ALPHA_RETURN.with(|v| *v.borrow_mut() = alpha);
    }
    pub fn set_sell_tao_return(tao: u64) {
        MOCK_SELL_TAO_RETURN.with(|v| *v.borrow_mut() = tao);
    }
    pub fn set_swap_fail(fail: bool) {
        MOCK_SWAP_FAIL.with(|v| *v.borrow_mut() = fail);
    }
    pub fn set_enforce_price_limit(enforce: bool) {
        MOCK_ENFORCE_PRICE_LIMIT.with(|v| *v.borrow_mut() = enforce);
    }
    pub fn set_simulate_partial_fill(val: bool) {
        MOCK_SIMULATE_PARTIAL_FILL.with(|v| *v.borrow_mut() = val);
    }
    pub fn clear_log() {
        SWAP_LOG.with(|l| l.borrow_mut().clear());
        ALPHA_BALANCES.with(|b| b.borrow_mut().clear());
        TAO_BALANCES.with(|b| b.borrow_mut().clear());
        RATE_LIMITS.with(|r| r.borrow_mut().clear());
        HOTKEY_REGISTRATIONS.with(|r| r.borrow_mut().clear());
        MOCK_ENFORCE_PRICE_LIMIT.with(|v| *v.borrow_mut() = false);
        MOCK_SIMULATE_PARTIAL_FILL.with(|v| *v.borrow_mut() = false);
    }
    pub fn is_rate_limited(hotkey: &AccountId, coldkey: &AccountId, netuid: NetUid) -> bool {
        RATE_LIMITS.with(|r| {
            r.borrow()
                .contains(&(hotkey.clone(), coldkey.clone(), netuid))
        })
    }
    /// Seed a staked alpha balance for a (coldkey, hotkey, netuid) triple.
    pub fn set_alpha_balance(coldkey: AccountId, hotkey: AccountId, netuid: NetUid, amount: u64) {
        ALPHA_BALANCES.with(|b| {
            b.borrow_mut().insert((coldkey, hotkey, netuid), amount);
        });
    }
    /// Query the current staked alpha balance for a (coldkey, hotkey, netuid) triple.
    pub fn alpha_balance(coldkey: &AccountId, hotkey: &AccountId, netuid: NetUid) -> u64 {
        ALPHA_BALANCES.with(|b| {
            *b.borrow()
                .get(&(coldkey.clone(), hotkey.clone(), netuid))
                .unwrap_or(&0)
        })
    }
    /// Seed a free TAO balance for an account.
    pub fn set_tao_balance(account: AccountId, amount: u64) {
        TAO_BALANCES.with(|b| {
            b.borrow_mut().insert(account, amount);
        });
    }
    /// Query the current free TAO balance for an account.
    pub fn tao_balance(account: &AccountId) -> u64 {
        TAO_BALANCES.with(|b| *b.borrow().get(account).unwrap_or(&0))
    }
    pub fn log() -> Vec<SwapCall> {
        SWAP_LOG.with(|l| l.borrow().clone())
    }
    /// Returns the `limit_price` argument from every `buy_alpha` call, in order.
    pub fn buy_alpha_limit_prices() -> Vec<u64> {
        Self::log()
            .into_iter()
            .filter_map(|c| {
                if let SwapCall::BuyAlpha { limit_price, .. } = c {
                    Some(limit_price)
                } else {
                    None
                }
            })
            .collect()
    }
    /// Returns the `limit_price` argument from every `sell_alpha` call, in order.
    pub fn sell_alpha_limit_prices() -> Vec<u64> {
        Self::log()
            .into_iter()
            .filter_map(|c| {
                if let SwapCall::SellAlpha { limit_price, .. } = c {
                    Some(limit_price)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn tao_transfers() -> Vec<(AccountId, AccountId, u64)> {
        Self::log()
            .into_iter()
            .filter_map(|c| {
                if let SwapCall::TransferTao { from, to, amount } = c {
                    Some((from, to, amount))
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn alpha_transfers() -> Vec<(AccountId, AccountId, AccountId, AccountId, NetUid, u64)> {
        Self::log()
            .into_iter()
            .filter_map(|c| {
                if let SwapCall::TransferStakedAlpha {
                    from_coldkey,
                    from_hotkey,
                    to_coldkey,
                    to_hotkey,
                    netuid,
                    amount,
                } = c
                {
                    Some((
                        from_coldkey,
                        from_hotkey,
                        to_coldkey,
                        to_hotkey,
                        netuid,
                        amount,
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl OrderSwapInterface<AccountId> for MockSwap {
    fn buy_alpha(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        tao_amount: TaoBalance,
        limit_price: TaoBalance,
        _apply_limits: bool,
    ) -> Result<AlphaBalance, frame_support::pallet_prelude::DispatchError> {
        if MOCK_SWAP_FAIL.with(|v| *v.borrow()) {
            return Err(frame_support::pallet_prelude::DispatchError::Other(
                "pool error",
            ));
        }
        if MOCK_SIMULATE_PARTIAL_FILL.with(|v| *v.borrow()) {
            return Err(frame_support::pallet_prelude::DispatchError::Other(
                "slippage too high",
            ));
        }
        let tao = tao_amount.to_u64();
        // Record the call (including rejected ones) so tests can verify the limit was passed.
        SWAP_LOG.with(|l| {
            l.borrow_mut().push(SwapCall::BuyAlpha {
                coldkey: coldkey.clone(),
                hotkey: hotkey.clone(),
                netuid,
                tao,
                limit_price: limit_price.to_u64(),
            })
        });
        if MOCK_ENFORCE_PRICE_LIMIT.with(|v| *v.borrow()) {
            let price = MOCK_PRICE.with(|p| {
                p.borrow()
                    .saturating_mul(U64F64::from_num(1_000_000_000u64))
                    .saturating_to_num::<u64>()
            });
            if price > limit_price.to_u64() {
                return Err(frame_support::pallet_prelude::DispatchError::Other(
                    "price limit exceeded",
                ));
            }
        }
        let alpha_out = MOCK_BUY_ALPHA_RETURN.with(|v| *v.borrow());
        // Debit TAO from coldkey, credit alpha to (coldkey, hotkey, netuid).
        TAO_BALANCES.with(|b| {
            let mut map = b.borrow_mut();
            let bal = map.entry(coldkey.clone()).or_insert(0);
            *bal = bal.saturating_sub(tao);
        });
        ALPHA_BALANCES.with(|b| {
            let mut map = b.borrow_mut();
            let bal = map
                .entry((coldkey.clone(), hotkey.clone(), netuid))
                .or_insert(0);
            *bal = bal.saturating_add(alpha_out);
        });
        Ok(AlphaBalance::from(alpha_out))
    }

    fn sell_alpha(
        coldkey: &AccountId,
        hotkey: &AccountId,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        limit_price: TaoBalance,
        _apply_limits: bool,
    ) -> Result<TaoBalance, frame_support::pallet_prelude::DispatchError> {
        if MOCK_SWAP_FAIL.with(|v| *v.borrow()) {
            return Err(frame_support::pallet_prelude::DispatchError::Other(
                "pool error",
            ));
        }
        if MOCK_SIMULATE_PARTIAL_FILL.with(|v| *v.borrow()) {
            return Err(frame_support::pallet_prelude::DispatchError::Other(
                "slippage too high",
            ));
        }
        let alpha = alpha_amount.to_u64();
        // Record the call (including rejected ones) so tests can verify the limit was passed.
        SWAP_LOG.with(|l| {
            l.borrow_mut().push(SwapCall::SellAlpha {
                coldkey: coldkey.clone(),
                hotkey: hotkey.clone(),
                netuid,
                alpha,
                limit_price: limit_price.to_u64(),
            })
        });
        // Only enforce if a non-zero floor was requested (0 means no constraint).
        if MOCK_ENFORCE_PRICE_LIMIT.with(|v| *v.borrow()) && limit_price.to_u64() > 0 {
            let price = MOCK_PRICE.with(|p| {
                p.borrow()
                    .saturating_mul(U64F64::from_num(1_000_000_000u64))
                    .saturating_to_num::<u64>()
            });
            if price < limit_price.to_u64() {
                return Err(frame_support::pallet_prelude::DispatchError::Other(
                    "price limit exceeded",
                ));
            }
        }
        let tao_out = MOCK_SELL_TAO_RETURN.with(|v| *v.borrow());
        // Debit alpha from (coldkey, hotkey, netuid), credit TAO to coldkey.
        ALPHA_BALANCES.with(|b| {
            let mut map = b.borrow_mut();
            let bal = map
                .entry((coldkey.clone(), hotkey.clone(), netuid))
                .or_insert(0);
            *bal = bal.saturating_sub(alpha);
        });
        TAO_BALANCES.with(|b| {
            let mut map = b.borrow_mut();
            let bal = map.entry(coldkey.clone()).or_insert(0);
            *bal = bal.saturating_add(tao_out);
        });
        Ok(TaoBalance::from(tao_out))
    }

    fn current_alpha_price(_netuid: NetUid) -> U64F64 {
        MOCK_PRICE.with(|p| *p.borrow())
    }

    fn transfer_tao(
        from: &AccountId,
        to: &AccountId,
        amount: TaoBalance,
    ) -> frame_support::pallet_prelude::DispatchResult {
        if FAIL_FEE_TRANSFER.with(|f| *f.borrow()) {
            return Err(frame_support::pallet_prelude::DispatchError::CannotLookup);
        }
        let amt = amount.to_u64();
        TAO_BALANCES.with(|b| {
            let mut map = b.borrow_mut();
            let from_bal = map.entry(from.clone()).or_insert(0);
            *from_bal = from_bal.saturating_sub(amt);
            let to_bal = map.entry(to.clone()).or_insert(0);
            *to_bal = to_bal.saturating_add(amt);
        });
        SWAP_LOG.with(|l| {
            l.borrow_mut().push(SwapCall::TransferTao {
                from: from.clone(),
                to: to.clone(),
                amount: amt,
            })
        });
        Ok(())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_netuid_for_benchmark(_netuid: NetUid) {
        // Mock price is already set; no subnet state to initialise.
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_acc_for_benchmark(hotkey: &AccountId, coldkey: &AccountId) {
        // Provide non-zero swap returns so batched-order benchmarks don't hit
        // `SwapReturnedZero`.  Also seed TAO and alpha balances so transfers
        // succeed in the mock ledgers.
        MockSwap::set_buy_alpha_return(1_000_000);
        MockSwap::set_sell_tao_return(1_000_000);
        MockSwap::set_tao_balance(coldkey.clone(), u64::MAX / 2);
        MockSwap::set_alpha_balance(
            coldkey.clone(),
            hotkey.clone(),
            NetUid::from(1u16),
            u64::MAX / 2,
        );
    }

    fn register_pallet_hotkey(
        coldkey: &AccountId,
        hotkey: &AccountId,
    ) -> frame_support::pallet_prelude::DispatchResult {
        HOTKEY_REGISTRATIONS.with(|r| {
            r.borrow_mut().insert((coldkey.clone(), hotkey.clone()));
        });
        Ok(())
    }

    fn pallet_hotkey_registered(coldkey: &AccountId, hotkey: &AccountId) -> bool {
        HOTKEY_REGISTRATIONS.with(|r| r.borrow().contains(&(coldkey.clone(), hotkey.clone())))
    }

    fn transfer_staked_alpha(
        from_coldkey: &AccountId,
        from_hotkey: &AccountId,
        to_coldkey: &AccountId,
        to_hotkey: &AccountId,
        netuid: NetUid,
        amount: AlphaBalance,
        validate_sender: bool,
        set_receiver_limit: bool,
    ) -> frame_support::pallet_prelude::DispatchResult {
        if validate_sender {
            let rate_limited = RATE_LIMITS.with(|r| {
                r.borrow()
                    .contains(&(from_hotkey.clone(), from_coldkey.clone(), netuid))
            });
            if rate_limited {
                return Err(frame_support::pallet_prelude::DispatchError::Other(
                    "StakingOperationRateLimitExceeded",
                ));
            }
        }
        let amt = amount.to_u64();
        ALPHA_BALANCES.with(|b| {
            let mut map = b.borrow_mut();
            let from_bal = map
                .entry((from_coldkey.clone(), from_hotkey.clone(), netuid))
                .or_insert(0);
            *from_bal = from_bal.saturating_sub(amt);
            let to_bal = map
                .entry((to_coldkey.clone(), to_hotkey.clone(), netuid))
                .or_insert(0);
            *to_bal = to_bal.saturating_add(amt);
        });
        if set_receiver_limit {
            RATE_LIMITS.with(|r| {
                r.borrow_mut()
                    .insert((to_hotkey.clone(), to_coldkey.clone(), netuid));
            });
        }
        SWAP_LOG.with(|l| {
            l.borrow_mut().push(SwapCall::TransferStakedAlpha {
                from_coldkey: from_coldkey.clone(),
                from_hotkey: from_hotkey.clone(),
                to_coldkey: to_coldkey.clone(),
                to_hotkey: to_hotkey.clone(),
                netuid,
                amount: amt,
            })
        });
        Ok(())
    }
}

// ── MockTime ─────────────────────────────────────────────────────────────────

thread_local! {
    pub static MOCK_TIME_MS: RefCell<u64> = const { RefCell::new(1_000_000u64) };
}

pub struct MockTime;

impl MockTime {
    pub fn set(ms: u64) {
        MOCK_TIME_MS.with(|t| *t.borrow_mut() = ms);
    }
}

impl frame_support::traits::UnixTime for MockTime {
    fn now() -> core::time::Duration {
        let ms = MOCK_TIME_MS.with(|t| *t.borrow());
        core::time::Duration::from_millis(ms)
    }
}

// ── Pallet config ─────────────────────────────────────────────────────────────

parameter_types! {
    pub const LimitOrdersPalletId: PalletId = PalletId(*b"lmt/ordr");
    pub const PalletHotkeyAccount: AccountId = AccountId::new([0xaa; 32]);
}

/// A fixed account used in tests as the fee recipient when a concrete
/// recipient is needed but the test isn't specifically about fees.
pub fn fee_recipient() -> AccountId {
    AccountId::new([0xfe; 32])
}

impl pallet_limit_orders::Config for Test {
    type SwapInterface = MockSwap;
    type TimeProvider = MockTime;
    type MaxOrdersPerBatch = ConstU32<64>;
    type PalletId = LimitOrdersPalletId;
    type PalletHotkey = PalletHotkeyAccount;
    type WeightInfo = ();
    type ChainId = ConstU64<945>;
}

// ── Shared test helpers ───────────────────────────────────────────────────────

pub fn alice() -> AccountId {
    AccountKeyring::Alice.to_account_id()
}
pub fn bob() -> AccountId {
    AccountKeyring::Bob.to_account_id()
}
pub fn charlie() -> AccountId {
    AccountKeyring::Charlie.to_account_id()
}
pub fn dave() -> AccountId {
    AccountKeyring::Dave.to_account_id()
}
pub fn netuid() -> NetUid {
    NetUid::from(1u16)
}

pub const FAR_FUTURE: u64 = u64::MAX;

#[allow(clippy::too_many_arguments)]
pub fn make_signed_order(
    keyring: AccountKeyring,
    hotkey: AccountId,
    netuid: NetUid,
    order_type: crate::OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    fee_rate: sp_runtime::Perbill,
    fee_recipient: AccountId,
    relayer: Option<BoundedVec<AccountId, ConstU32<10>>>,
) -> crate::SignedOrder<AccountId> {
    let signer = keyring.to_account_id();
    let order = crate::VersionedOrder::V1(crate::Order {
        signer,
        hotkey,
        netuid,
        order_type,
        amount,
        limit_price,
        expiry,
        fee_rate,
        fee_recipient,
        relayer,
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: false,
    });
    let sig = keyring.pair().sign(&order.encode());
    crate::SignedOrder {
        order,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

/// Build a signed order with partial fills enabled and a relayer set.
/// `partial_fill` is the fill amount to inject into the `SignedOrder` envelope.
#[allow(clippy::too_many_arguments)]
pub fn make_partial_fill_order(
    keyring: AccountKeyring,
    hotkey: AccountId,
    netuid: NetUid,
    order_type: crate::OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    relayer: AccountId,
    partial_fill: u64,
) -> crate::SignedOrder<AccountId> {
    let signer = keyring.to_account_id();
    let order = crate::VersionedOrder::V1(crate::Order {
        signer,
        hotkey,
        netuid,
        order_type,
        amount,
        limit_price,
        expiry,
        fee_rate: sp_runtime::Perbill::zero(),
        fee_recipient: fee_recipient(),
        relayer: Some(BoundedVec::try_from(vec![relayer]).unwrap()),
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: true,
    });
    let sig = keyring.pair().sign(&order.encode());
    crate::SignedOrder {
        order,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: Some(partial_fill),
    }
}

pub fn bounded(
    v: Vec<crate::SignedOrder<AccountId>>,
) -> BoundedVec<crate::SignedOrder<AccountId>, ConstU32<64>> {
    BoundedVec::try_from(v).unwrap()
}

pub fn order_id(order: &crate::VersionedOrder<AccountId>) -> H256 {
    crate::pallet::Pallet::<Test>::derive_order_id(order)
}

// ── Test externalities ────────────────────────────────────────────────────────

pub fn new_test_ext() -> sp_io::TestExternalities {
    let storage = system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    // Register a keystore so `sp_io::crypto` functions work in benchmark tests.
    let keystore = sp_keystore::testing::MemoryKeystore::new();
    ext.register_extension(sp_keystore::KeystoreExt::new(keystore));
    ext.execute_with(|| {
        System::set_block_number(1);
        MockSwap::clear_log();
        // Simulate genesis_build: register the pallet hotkey and enable the pallet.
        let pallet_acct: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        let _ = MockSwap::register_pallet_hotkey(&pallet_acct, &PalletHotkeyAccount::get());
        LimitOrdersEnabled::<Test>::set(true);
    });
    ext
}
