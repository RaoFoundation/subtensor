#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::too_many_arguments
)]

use codec::Encode;
use frame_support::{
    BoundedVec, PalletId, assert_noop, assert_ok,
    traits::{ConstU32, Hooks},
};
use node_subtensor_runtime::{
    BuildStorage, LimitOrders, Runtime, RuntimeEvent, RuntimeGenesisConfig, RuntimeOrigin,
    SubtensorModule, System, pallet_subtensor,
};
use pallet_limit_orders::{
    HasMigrationRun, LimitOrdersEnabled, Order, OrderStatus, OrderType, Orders, SignedOrder,
    VersionedOrder,
};
use pallet_subtensor::{SubnetAlphaIn, SubnetMechanism, SubnetTAO};
use sp_core::{Get, H256, Pair};
use sp_keyring::Sr25519Keyring;
use sp_runtime::traits::AccountIdConversion;
use sp_runtime::{MultiSignature, Perbill};
use subtensor_runtime_common::{AccountId, AlphaBalance, NetUid, TaoBalance, Token};

fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

/// Initialise a subnet so that limit-order execution has a pool to interact with.
///
/// We use the stable mechanism (mechanism_id = 0, the default), which swaps at a
/// fixed 1 TAO : 1 alpha rate without requiring pre-seeded AMM liquidity.
fn setup_subnet(netuid: NetUid) {
    SubtensorModule::init_new_network(netuid, 0);
    // Genesis forces netuid 1 to dynamic (mechanism_id = 1); override to stable
    // (mechanism_id = 0) so that swaps are 1:1 with no AMM fees, matching the
    // intent of every test that calls this helper.
    pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 0u16);
    pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
}

fn min_default_stake() -> TaoBalance {
    pallet_subtensor::DefaultMinStake::<Runtime>::get()
}

fn add_balance_to_coldkey_account(coldkey: &AccountId, tao: TaoBalance) {
    let credit = SubtensorModule::mint_tao(tao);
    let _ = SubtensorModule::spend_tao(coldkey, credit, tao);
}

fn seed_subnet_tao(netuid: NetUid, amount: TaoBalance) {
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
    add_balance_to_coldkey_account(&subnet_account, amount);
}

fn fund_account(id: &AccountId) {
    add_balance_to_coldkey_account(id, min_default_stake() * 10u64.into());
}

fn order_id(order: &VersionedOrder<AccountId>) -> H256 {
    H256(sp_io::hashing::blake2_256(&order.encode()))
}

fn make_order_batch(
    orders: Vec<SignedOrder<AccountId>>,
) -> BoundedVec<SignedOrder<AccountId>, <Runtime as pallet_limit_orders::Config>::MaxOrdersPerBatch>
{
    orders.try_into().unwrap()
}

fn setup_buyer_seller(
    netuid: NetUid,
    alice_id: &AccountId,
    charlie_id: &AccountId,
    bob_id: &AccountId,
    dave_id: &AccountId,
) {
    fund_account(alice_id);
    let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        dave_id,
        bob_id,
        netuid,
        initial_alpha,
    );
    seed_subnet_tao(netuid, TaoBalance::from(initial_alpha.to_u64()));
    let _ = SubtensorModule::create_account_if_non_existent(alice_id, charlie_id);
    let _ = SubtensorModule::create_account_if_non_existent(bob_id, dave_id);
}

struct OrderParams {
    order_type: OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    fee_rate: Perbill,
    fee_recipient: AccountId,
    relayer: Option<BoundedVec<AccountId, ConstU32<10>>>,
    max_slippage: Option<Perbill>,
    partial_fills_enabled: bool,
}

/// Shared implementation: constructs and signs a `VersionedOrder::V1` from an
/// `OrderParams` and returns a `SignedOrder` with `partial_fill = None`.
/// All three public factory functions delegate here so that adding a new field
/// to `Order` requires updating only this function.
fn make_signed_order_inner(
    keyring: Sr25519Keyring,
    hotkey: AccountId,
    netuid: NetUid,
    params: OrderParams,
) -> SignedOrder<AccountId> {
    let order = VersionedOrder::V1(Order {
        signer: keyring.to_account_id(),
        hotkey,
        netuid,
        order_type: params.order_type,
        amount: params.amount,
        limit_price: params.limit_price,
        expiry: params.expiry,
        fee_rate: params.fee_rate,
        fee_recipient: params.fee_recipient,
        relayer: params.relayer,
        max_slippage: params.max_slippage,
        partial_fills_enabled: params.partial_fills_enabled,
        // chain_id 0 matches the default pallet_evm_chain_id genesis value in tests
        chain_id: 0,
    });
    let sig = keyring.pair().sign(&order.encode());
    SignedOrder {
        order,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

fn make_signed_order(
    keyring: Sr25519Keyring,
    hotkey: AccountId,
    netuid: NetUid,
    order_type: OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    fee_rate: Perbill,
    fee_recipient: AccountId,
) -> SignedOrder<AccountId> {
    make_signed_order_inner(
        keyring,
        hotkey,
        netuid,
        OrderParams {
            order_type,
            amount,
            limit_price,
            expiry,
            fee_rate,
            fee_recipient,
            relayer: None,
            max_slippage: None,
            partial_fills_enabled: false,
        },
    )
}

/// Set up a dynamic-mechanism (Uniswap v3-style) subnet with equal TAO and
/// alpha reserves, giving an initial pool price of exactly 1.0 TAO/alpha.
///
/// The stable mechanism (mechanism_id = 0) ignores the `price_limit` parameter
/// entirely and always executes at 1:1, so slippage enforcement can only be
/// tested against a dynamic subnet.
fn setup_dynamic_subnet(netuid: NetUid) {
    SubtensorModule::init_new_network(netuid, 0);
    // Override the mechanism to 1 (dynamic / Uniswap v3).
    SubnetMechanism::<Runtime>::insert(netuid, 1u16);
    pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
    // Equal reserves → price = tao_reserve / alpha_reserve = 1.0
    SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(1_000_000_000_000_u64));
    SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(1_000_000_000_000_u64));
    seed_subnet_tao(netuid, TaoBalance::from(1_000_000_000_000_u64));
}

/// Build a signed order with an explicit `max_slippage` value.
fn make_signed_order_with_slippage_rt(
    keyring: Sr25519Keyring,
    hotkey: AccountId,
    netuid: NetUid,
    order_type: OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    fee_rate: Perbill,
    fee_recipient: AccountId,
    max_slippage: Option<Perbill>,
) -> SignedOrder<AccountId> {
    make_signed_order_inner(
        keyring,
        hotkey,
        netuid,
        OrderParams {
            order_type,
            amount,
            limit_price,
            expiry,
            fee_rate,
            fee_recipient,
            relayer: None,
            max_slippage,
            partial_fills_enabled: false,
        },
    )
}

/// Build a `SignedOrder` with `partial_fills_enabled = true` and the relayer set
/// to `relayer`.  The `partial_fill` field on the envelope is supplied separately
/// by each test so that the *same* `VersionedOrder` payload (and therefore the
/// same order-id) can be re-used across multiple submissions.
fn make_partial_fill_order(
    keyring: Sr25519Keyring,
    hotkey: AccountId,
    netuid: NetUid,
    order_type: OrderType,
    amount: u64,
    limit_price: u64,
    expiry: u64,
    fee_recipient: AccountId,
    relayer: AccountId,
    partial_fill: Option<u64>,
) -> SignedOrder<AccountId> {
    let mut signed = make_signed_order_inner(
        keyring,
        hotkey,
        netuid,
        OrderParams {
            order_type,
            amount,
            limit_price,
            expiry,
            fee_rate: Perbill::zero(),
            fee_recipient,
            relayer: Some(BoundedVec::try_from(vec![relayer]).unwrap()),
            max_slippage: None,
            partial_fills_enabled: true,
        },
    );
    signed.partial_fill = partial_fill;
    signed
}

// ─────────────────────────────────────────────────────────────────────────────

/// Signing and cancelling an order writes the order id to storage as Cancelled
/// and emits OrderCancelled. No subnet or balance setup required.
#[test]
fn cancel_order_works() {
    new_test_ext().execute_with(|| {
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let fee_recipient = Sr25519Keyring::Charlie.to_account_id();

        let order = VersionedOrder::V1(Order {
            signer: alice_id.clone(),
            hotkey: bob_id,
            netuid: NetUid::from(1u16),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient,
            relayer: None,
            max_slippage: None,
            partial_fills_enabled: false,
            // chain_id 0 matches the default pallet_evm_chain_id genesis value in tests
            chain_id: 0,
        });
        let id = order_id(&order);

        assert_ok!(LimitOrders::cancel_order(
            RuntimeOrigin::signed(alice_id),
            order,
        ));

        assert_eq!(Orders::<Runtime>::get(id), Some(OrderStatus::Cancelled));
    });
}

/// An order signed with an Ed25519 key is rejected at validation time even
/// though the signature itself is cryptographically valid. The order must not
/// appear in the Orders storage map after the batch runs.
#[test]
fn execute_orders_ed25519_signature_rejected() {
    new_test_ext().execute_with(|| {
        let alice_id = Sr25519Keyring::Alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let fee_recipient = Sr25519Keyring::Charlie.to_account_id();

        let order = VersionedOrder::V1(Order {
            signer: alice_id.clone(),
            hotkey: bob_id,
            netuid: NetUid::from(1u16),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient,
            relayer: None,
            max_slippage: None,
            partial_fills_enabled: false,
            // chain_id 0 matches the default pallet_evm_chain_id genesis value in tests
            chain_id: 0,
        });
        let id = order_id(&order);

        // Sign with ed25519 — valid signature, wrong scheme.
        let ed_pair = sp_core::ed25519::Pair::from_legacy_string("//Alice", None);
        let ed_sig = ed_pair.sign(&order.encode());
        let signed = SignedOrder {
            order,
            signature: MultiSignature::Ed25519(ed_sig),
            partial_fill: None,
        };

        let orders = make_order_batch(vec![signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(alice_id),
            orders,
            false,
        ));

        // Order was silently skipped — nothing written to storage.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

/// An order carrying a wrong chain_id is silently skipped by `execute_orders`
/// (the per-order error path) and must not appear in the Orders storage map.
#[test]
fn execute_orders_chain_id_mismatch_rejected() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        setup_subnet(netuid);

        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let fee_recipient = Sr25519Keyring::Charlie.to_account_id();
        fund_account(&alice_id);

        // Build an order with a chain_id that doesn't match the runtime (0).
        let order = VersionedOrder::V1(Order {
            signer: alice_id.clone(),
            hotkey: bob_id,
            netuid,
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: u64::MAX,
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient,
            relayer: None,
            max_slippage: None,
            partial_fills_enabled: false,
            chain_id: 9999, // wrong chain — should be rejected
        });
        let id = order_id(&order);
        let sig = alice.pair().sign(&order.encode());
        let signed = SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(alice_id),
            make_order_batch(vec![signed]),
            false,
        ));

        // Order was silently skipped — nothing written to storage.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

/// A LimitBuy order whose price condition is satisfied executes against the pool,
/// marks the order as Fulfilled, and credits staked alpha to the buyer.
#[test]
fn limit_buy_order_executes_and_stakes_alpha() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Fund Alice so buy_alpha can debit her balance.
        fund_account(&alice_id);

        // Create the hot-key association.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // limit_price = u64::MAX → current_price (1.0) ≤ MAX → condition always met.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(), // default min stake units of TAO to spend
            u64::MAX,                   // price ceiling — always satisfied
            u64::MAX,                   // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must be marked as executed.
        assert_eq!(Orders::<Runtime>::get(id), Some(OrderStatus::Fulfilled));

        // Alice must now have staked alpha delegated through Bob on this subnet.
        // AMM pool output has slight slippage even with the stable mechanism; check within 1%.
        let staked =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        let expected_alpha = min_default_stake().to_u64();
        assert!(
            staked >= AlphaBalance::from(expected_alpha * 99 / 100)
                && staked <= AlphaBalance::from(expected_alpha),
            "alice should hold approximately min_default_stake alpha after a LimitBuy order executes (got {staked:?})"
        );
    });
}

/// A TakeProfit order whose price condition is satisfied executes against the pool,
/// marks the order as Fulfilled, and burns the seller's staked alpha position.
#[test]
fn take_profit_order_executes_and_unstakes_alpha() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Create the hot-key association.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Seed Alice with staked alpha through Bob so she has something to sell.
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );
        seed_subnet_tao(netuid, TaoBalance::from(initial_alpha.to_u64()));

        // limit_price = 0 → current_price (1.0) ≥ 0 → condition always met.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().into(), // sell min default alpha units
            0,                          // price floor — always satisfied
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must be marked as executed.
        assert_eq!(Orders::<Runtime>::get(id), Some(OrderStatus::Fulfilled));

        // Alice's staked alpha must have decreased by exactly min_default_stake after the sell.
        // Stable mechanism 1:1, zero fee: initial_alpha = min_default_stake * 10,
        // sold min_default_stake alpha, so remaining = min_default_stake * 9.
        let remaining =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert_eq!(
            remaining,
            AlphaBalance::from(min_default_stake().to_u64() * 9u64),
            "alice's staked alpha should be min_default_stake*9 after a TakeProfit order executes"
        );
    });
}

/// A StopLoss order whose price condition is satisfied (price ≤ limit_price) executes
/// against the pool, marks the order as Fulfilled, decreases the seller's staked alpha,
/// and credits free TAO to the seller.
#[test]
fn stop_loss_order_executes_and_unstakes_alpha() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Create the hot-key association.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Seed Alice with staked alpha through Bob so she has something to sell.
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );
        seed_subnet_tao(netuid, TaoBalance::from(initial_alpha.to_u64()));

        // limit_price = 1_000_000_000 (1.0 × 10⁹) → scaled_price (1_000_000_000) ≤ 1_000_000_000
        // → StopLoss condition always met. Stable mechanism ignores the AMM floor, so any
        // value ≥ 1_000_000_000 works here.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::StopLoss,
            min_default_stake().into(), // sell min_default_stake alpha units
            1_000_000_000,             // price ceiling in ×10⁹ scale (1.0) — always met
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must be marked as executed.
        assert_eq!(Orders::<Runtime>::get(id), Some(OrderStatus::Fulfilled));

        // Alice's staked alpha must have decreased by exactly min_default_stake.
        // Stable mechanism 1:1, zero fee: initial_alpha = min_default_stake * 10,
        // sold min_default_stake alpha, so remaining = min_default_stake * 9.
        let remaining =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert_eq!(
            remaining,
            AlphaBalance::from(min_default_stake().to_u64() * 9u64),
            "alice's staked alpha should be min_default_stake*9 after a StopLoss order executes"
        );

        // Alice must have received TAO from the sale. Pool output has slight slippage; check within 1%.
        let alice_tao = SubtensorModule::get_coldkey_balance(&alice_id);
        let expected_tao = min_default_stake().to_u64();
        assert!(
            alice_tao >= TaoBalance::from(expected_tao * 99 / 100)
                && alice_tao <= TaoBalance::from(expected_tao),
            "alice should receive approximately min_default_stake TAO after a StopLoss order executes (got {alice_tao:?})"
        );
    });
}

// ── Batched execution ─────────────────────────────────────────────────────────

/// Buy side (5 000 TAO) exceeds sell side (2 000 alpha ≈ 2 000 TAO at 1:1).
///
/// Residual 3 000 TAO goes to the pool; buyers receive pool alpha + seller passthrough
/// alpha. Sellers receive the passthrough TAO that corresponds to their alpha.
///
/// With the stable mechanism (1 TAO = 1 alpha):
///   • Alice (buyer 5 000 TAO) → 5 000 alpha staked to Dave
///   • Bob  (seller 2 000 α)   → 2 000 free TAO
#[test]
fn batched_buy_dominant_executes_correctly() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);

        setup_buyer_seller(netuid, &alice_id, &charlie_id, &bob_id, &dave_id);

        let buy = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().to_u64() * 2u64,
            u64::MAX,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let sell = make_signed_order(
            bob,
            dave_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().into(),
            0,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy, sell]);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            netuid,
            orders,
        ));

        // Alice spent TAO and must hold the resulting staked alpha.
        // Buy-dominant: Alice buys min_default_stake*2 TAO, Bob sells min_default_stake alpha.
        // total_sell_tao_equiv = min_default_stake (at 1:1). residual_buy = min_default_stake.
        // pool returns min_default_stake alpha; plus Bob's passthrough = min_default_stake.
        // Alice receives Bob's passthrough alpha + pool alpha for the residual TAO.
        // Pool output has slight slippage; check within 1% of expected min_default_stake*2.
        let alice_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &charlie_id,
            &alice_id,
            netuid,
        );
        let expected_alice_alpha = min_default_stake().to_u64() * 2u64;
        assert!(
            alice_alpha >= AlphaBalance::from(expected_alice_alpha * 99 / 100)
                && alice_alpha <= AlphaBalance::from(expected_alice_alpha),
            "alice should hold approximately min_default_stake*2 alpha after buy-dominant batch (got {alice_alpha:?})"
        );

        // Bob sold alpha and must hold the resulting free TAO.
        // In buy-dominant, total_tao = total_sell_tao_equiv = min_default_stake.
        // Bob's gross_share = (min_default_stake * min_default_stake) / min_default_stake
        //                   = min_default_stake (exact). Zero fee => net_share = min_default_stake.
        let bob_tao = SubtensorModule::get_coldkey_balance(&bob_id);
        assert_eq!(
            bob_tao,
            TaoBalance::from(min_default_stake().to_u64()),
            "bob should hold exactly min_default_stake TAO after buy-dominant batch"
        );
    });
}

/// Regression (real-storage rollback): the same fully-signed `LimitBuy` order
/// appearing twice in one batch must hard-fail with `DuplicateOrderInBatch` and
/// leave the signer's balances completely untouched.
///
/// Balances are real substrate storage, so the
/// all-or-nothing rollback is faithfully observable: free TAO and staked alpha
/// must match their pre-call values exactly, and the order must never be
/// recorded in `Orders`.
#[test]
fn batched_full_fill_duplicate_rejected_and_rolled_back() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);
        setup_buyer_seller(netuid, &alice_id, &charlie_id, &bob_id, &dave_id);

        // Open-relay (relayer: None) fully-signed LimitBuy from Alice, staking
        // to her hotkey (charlie).
        let order = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&order.order);

        // Snapshot the signer's real balances before the call.
        let alice_tao_before = SubtensorModule::get_coldkey_balance(&alice_id);
        let alice_alpha_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &charlie_id,
            &alice_id,
            netuid,
        );

        // The same order twice in one batch — must hard-fail the whole batch.
        let orders = make_order_batch(vec![order.clone(), order]);
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie_id.clone()),
                netuid,
                orders,
            ),
            pallet_limit_orders::Error::<Runtime>::DuplicateOrderInBatch
        );

        // Full rollback: balances unchanged and no order status recorded.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&alice_id),
            alice_tao_before,
            "signer's free TAO must be unchanged after a duplicate-order batch rollback"
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &charlie_id,
                &alice_id,
                netuid,
            ),
            alice_alpha_before,
            "signer's staked alpha must be unchanged after a duplicate-order batch rollback"
        );
        assert!(
            Orders::<Runtime>::get(id).is_none(),
            "no order status must be recorded when the batch is rolled back"
        );
    });
}

/// Sell side (min_default_stake()*2 alpha ≈ min_default_stake()*2 TAO at 1:1) exceeds buy side (min_default_stake() TAO).
///
/// Residual min_default_stake() alpha goes to the pool; sellers receive pool TAO + buyer
/// passthrough TAO. Buyers receive the passthrough alpha corresponding to their TAO.
///
/// With the stable mechanism (1 TAO = 1 alpha):
///   • Alice (buyer min_default_stake() TAO) →  alpha staked to Dave
///   • Bob  (seller min_default_stake()*2 α)   → min_default_stake()*2 free TAO
#[test]
fn batched_sell_dominant_executes_correctly() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);

        setup_buyer_seller(netuid, &alice_id, &charlie_id, &bob_id, &dave_id);

        let buy = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let sell = make_signed_order(
            bob,
            dave_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().to_u64() * 2u64,
            0,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy, sell]);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            netuid,
            orders,
        ));

        // Alice spent TAO and must hold the resulting staked alpha.
        // Sell-dominant: Alice buys min_default_stake TAO, Bob sells min_default_stake*2 alpha.
        // total_buy_alpha_equiv = tao_to_alpha(min_default_stake, 1.0) = min_default_stake (exact).
        // Alice's pro-rata share = (min_default_stake * min_default_stake) / min_default_stake
        //                        = min_default_stake (exact, no floor rounding).
        let alice_alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &charlie_id,
            &alice_id,
            netuid,
        );
        assert_eq!(
            alice_alpha,
            AlphaBalance::from(min_default_stake().to_u64()),
            "alice should hold exactly min_default_stake alpha after sell-dominant batch"
        );

        // Bob receives Alice's passthrough TAO + pool TAO for the residual alpha.
        // Pool output has slight slippage; check within 1% of expected min_default_stake*2.
        let bob_tao = SubtensorModule::get_coldkey_balance(&bob_id);
        let expected_bob_tao = min_default_stake().to_u64() * 2u64;
        assert!(
            bob_tao >= TaoBalance::from(expected_bob_tao * 99 / 100)
                && bob_tao <= TaoBalance::from(expected_bob_tao),
            "bob should hold approximately min_default_stake*2 TAO after sell-dominant batch (got {bob_tao:?})"
        );
    });
}

#[test]
fn batched_fails_if_executing_below_minimum_on_sell() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);

        setup_buyer_seller(netuid, &alice_id, &charlie_id, &bob_id, &dave_id);

        let buy = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let sell = make_signed_order(
            bob,
            dave_id.clone(),
            netuid,
            OrderType::TakeProfit,
            1u64,
            0,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy, sell]);

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie_id.clone()),
                netuid,
                orders,
            ),
            pallet_subtensor::Error::<Runtime>::AmountTooLow
        );
    });
}

#[test]
fn batched_fails_if_executing_without_hot_key_association() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);

        // Create the hot-key association. Alice is not associating to charlie

        // Alice has free TAO to spend on a buy order.
        fund_account(&alice_id);

        // Seed Bob with staked alph so he has something to sell.
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &dave_id,
            &bob_id,
            netuid,
            initial_alpha,
        );
        seed_subnet_tao(netuid, TaoBalance::from(initial_alpha.to_u64()));

        let buy = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let sell = make_signed_order(
            bob,
            dave_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().to_u64() * 2u64,
            0,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy, sell]);

        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie_id.clone()),
                netuid,
                orders,
            ),
            pallet_subtensor::Error::<Runtime>::HotKeyAccountNotExists
        );
    });
}

/// `execute_batched_orders` fails when the target subnet does not exist.
/// The subnet is never initialised (no `setup_subnet`), so `buy_alpha`
/// returns `SubnetNotExists` during the pool-swap step.
#[test]
fn batched_fails_for_nonexistent_subnet() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(2u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        // Fund Alice so that `transfer_tao` succeeds; the subnet check happens
        // later inside `buy_alpha`.
        fund_account(&alice_id);

        let buy = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy]);

        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::signed(charlie_id), netuid, orders,),
            pallet_subtensor::Error::<Runtime>::SubnetNotExists
        );
    });
}

/// `execute_batched_orders` fails when the subnet exists but its subtoken is
/// not enabled. The order passes validation (price condition is met) and the
/// TAO transfer succeeds, but `buy_alpha` then returns `SubtokenDisabled`.
#[test]
fn batched_fails_if_subtoken_not_enabled() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        // Initialise the network but deliberately skip setting SubtokenEnabled.
        SubtensorModule::init_new_network(netuid, 0);

        // Fund Alice so that the TAO transfer in `collect_assets` succeeds.
        fund_account(&alice_id);

        let buy = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy]);

        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::signed(charlie_id), netuid, orders,),
            pallet_subtensor::Error::<Runtime>::SubtokenDisabled
        );
    });
}

/// An order whose `expiry` is in the past causes `execute_batched_orders` to
/// fail with `OrderExpired`.
#[test]
fn batched_fails_for_expired_order() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Advance the runtime timestamp so that `now_ms` exceeds the order's expiry.
        // `pallet_timestamp::Now` stores milliseconds; set it to 100_000 ms.
        pallet_timestamp::Now::<Runtime>::put(100_000u64);

        // Build an order that expired at 50_000 ms — already in the past.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            50_000, // expiry in ms — before current timestamp of 100_000
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![signed]);

        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::signed(charlie_id), netuid, orders,),
            pallet_limit_orders::Error::<Runtime>::OrderExpired
        );
    });
}

/// An order whose price condition is not met causes `execute_batched_orders` to
/// fail with `PriceConditionNotMet`. A `LimitBuy` with `limit_price = 0`
/// requires `current_price <= 0`; since the stable mechanism prices alpha at
/// 1.0 TAO the condition is never met.
#[test]
fn batched_fails_if_price_condition_not_met() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // limit_price = 0 requires current_price <= 0, but current_price ~= 1.0 → fails.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            0,        // price ceiling of 0 — never satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![signed]);

        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::signed(charlie_id), netuid, orders,),
            pallet_limit_orders::Error::<Runtime>::PriceConditionNotMet
        );
    });
}

/// `execute_batched_orders` fails immediately with `RootNetUidNotAllowed` when
/// called with `netuid = 0` (the root network).
#[test]
fn batched_fails_for_root_netuid() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(0u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        // Fund Alice so the call gets past any balance checks before hitting the root guard.
        fund_account(&alice_id);

        let buy = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );

        let orders = make_order_batch(vec![buy]);

        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::signed(charlie_id), netuid, orders,),
            pallet_limit_orders::Error::<Runtime>::RootNetUidNotAllowed
        );
    });
}

// ── execute_orders — silent-skip behaviour ────────────────────────────────────

/// `execute_orders` silently skips an expired order: the call returns `Ok`
/// and the order is NOT written to the `Orders` storage map.
#[test]
fn execute_orders_skips_expired_order() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Advance the runtime timestamp so that `now_ms` exceeds the order's expiry.
        pallet_timestamp::Now::<Runtime>::put(100_000u64);

        // Build an order that expired at 50_000 ms — already in the past.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            50_000, // expiry in ms — before current timestamp of 100_000
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // The call must succeed even though the order is expired.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Expired order silently skipped — nothing written to storage.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

/// `execute_orders` processes a mixed batch: the valid order executes and is
/// stored as `Fulfilled`; the expired order is silently skipped and is NOT
/// written to storage.  The call always returns `Ok`.
#[test]
fn execute_orders_valid_and_invalid_mixed() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Fund Alice so that her LimitBuy order can execute.
        fund_account(&alice_id);

        // Create the hotkey association for Alice so buy_alpha succeeds.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Timestamp at 100_000 ms — Bob's order (expiry 50_000) will be expired.
        pallet_timestamp::Now::<Runtime>::put(100_000u64);

        // Valid order: LimitBuy with price ceiling always satisfied and no expiry.
        let valid = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );
        // Invalid order: already expired.
        let expired = make_signed_order(
            bob,
            alice_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            50_000, // expiry in ms — before current timestamp of 100_000
            Perbill::zero(),
            charlie_id.clone(),
        );
        let valid_id = order_id(&valid.order);
        let expired_id = order_id(&expired.order);

        let orders = make_order_batch(vec![valid, expired]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Valid order executed — stored as Fulfilled.
        assert_eq!(
            Orders::<Runtime>::get(valid_id),
            Some(OrderStatus::Fulfilled)
        );
        // Expired order silently skipped — not written to storage.
        assert!(Orders::<Runtime>::get(expired_id).is_none());
    });
}

// ── execute_orders — all-or-nothing (should_fail = true) ──────────────────────

/// `execute_orders` with `should_fail = true` aborts the whole call as soon as
/// it hits a failing order. A single expired order makes the extrinsic return
/// `OrderExpired`, and nothing is written to the `Orders` storage map.
#[test]
fn execute_orders_should_fail_aborts_on_expired_order() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Advance the runtime timestamp so that `now_ms` exceeds the order's expiry.
        pallet_timestamp::Now::<Runtime>::put(100_000u64);

        // Build an order that expired at 50_000 ms — already in the past.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            50_000, // expiry in ms — before current timestamp of 100_000
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // should_fail = true → the expired order surfaces its error to the caller
        // and the whole call reverts (nothing written to storage).
        assert_noop!(
            LimitOrders::execute_orders(RuntimeOrigin::signed(charlie_id), orders, true),
            pallet_limit_orders::Error::<Runtime>::OrderExpired
        );

        // Order was never stored — the call aborted.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

/// Contrast with `execute_orders_valid_and_invalid_mixed`: the SAME mixed batch
/// (a valid LimitBuy followed by an expired LimitBuy) submitted with
/// `should_fail = true` reverts the WHOLE batch. The valid order's stake and
/// balance effects are NOT applied — dispatchables are transactional.
#[test]
fn execute_orders_should_fail_reverts_valid_order_in_mixed_batch() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Fund Alice so that her LimitBuy order would execute (absent the abort).
        fund_account(&alice_id);

        // Create the hotkey association for Alice so buy_alpha would succeed.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Snapshot Alice's balance and stake before submitting the batch.
        let alice_balance_before = SubtensorModule::get_coldkey_balance(&alice_id);
        let alice_stake_before =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);

        // Timestamp at 100_000 ms — Bob's order (expiry 50_000) will be expired.
        pallet_timestamp::Now::<Runtime>::put(100_000u64);

        // Valid order: LimitBuy with price ceiling always satisfied and no expiry.
        let valid = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );
        // Invalid order: already expired. It follows the valid order in the batch,
        // so the valid order is executed first and must be rolled back on abort.
        let expired = make_signed_order(
            bob,
            alice_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX,
            50_000, // expiry in ms — before current timestamp of 100_000
            Perbill::zero(),
            charlie_id.clone(),
        );
        let valid_id = order_id(&valid.order);
        let expired_id = order_id(&expired.order);

        let orders = make_order_batch(vec![valid, expired]);

        // should_fail = true → the expired order aborts the whole call and reverts
        // the already-executed valid order.
        assert_noop!(
            LimitOrders::execute_orders(RuntimeOrigin::signed(charlie_id), orders, true),
            pallet_limit_orders::Error::<Runtime>::OrderExpired
        );

        // Neither order is stored — the entire batch was rolled back.
        assert!(
            Orders::<Runtime>::get(valid_id).is_none(),
            "valid order must be rolled back, not stored, when should_fail aborts"
        );
        assert!(Orders::<Runtime>::get(expired_id).is_none());

        // The valid order's effects must NOT have been applied: Alice's TAO balance
        // and her staked alpha are exactly what they were before the call.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&alice_id),
            alice_balance_before,
            "alice's TAO must be unchanged after an aborted all-or-nothing batch"
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid),
            alice_stake_before,
            "alice's staked alpha must be unchanged after an aborted all-or-nothing batch"
        );
    });
}

/// `execute_orders` silently skips an order whose signer has no hotkey
/// association: the call returns `Ok` and the order is NOT written to the
/// `Orders` storage map.
#[test]
fn execute_orders_skips_order_with_unassociated_hotkey() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Fund Alice so that any balance check is not the reason for skipping.
        fund_account(&alice_id);

        // Deliberately do NOT call create_account_if_non_existent — Alice has no
        // hotkey association, so the order should be silently skipped.

        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // The call must succeed even though the hotkey association is missing.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order was silently skipped — nothing written to storage.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

/// `execute_orders` silently skips an order whose amount is below the minimum
/// stake threshold: the call returns `Ok` and the order is NOT written to the
/// `Orders` storage map.
#[test]
fn execute_orders_skips_order_below_minimum_stake() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Fund Alice so that any balance check is not the reason for skipping.
        fund_account(&alice_id);

        // Create the hotkey association so that is not the reason for skipping.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // amount = 1 is well below min_default_stake(), triggering AmountTooLow.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            1u64,
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // The call must succeed even though the amount is below the minimum.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order was silently skipped — nothing written to storage.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

/// `execute_orders` silently skips an order targeting a subnet that does not
/// exist: the call returns `Ok` and the order is NOT written to the `Orders`
/// storage map.
#[test]
fn execute_orders_skips_order_for_nonexistent_subnet() {
    new_test_ext().execute_with(|| {
        // netuid 2 is not initialised — no setup_subnet call.
        let netuid = NetUid::from(2u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        // Fund Alice so that any balance check is not the reason for skipping.
        fund_account(&alice_id);

        // Create the hotkey association so that is not the reason for skipping.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // The call must succeed even though the subnet does not exist.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order was silently skipped — nothing written to storage.
        assert!(Orders::<Runtime>::get(id).is_none());
    });
}

// ── Fee-correctness tests ─────────────────────────────────────────────────────

/// `execute_orders` (non-batched) correctly forwards the buy-order fee to the
/// designated fee recipient and charges Alice exactly `amount` TAO in total.
///
/// Fee mechanics for a non-batched LimitBuy:
///   fee_tao = fee_rate * tao_in  (computed from input BEFORE swap, exact integer arithmetic)
///   tao_after_fee = tao_in - fee_tao  (goes to the pool)
///   fee transferred directly from signer to fee_recipient via transfer_tao
///
/// We use amount = min_default_stake() * 2 so that tao_after_fee = 90% * 2 * min_default_stake()
/// = 1.8 * min_default_stake() > min_default_stake(), satisfying the minimum-stake validation
/// inside buy_alpha. With fee_rate = 10%:
///   fee_tao = 10% * (min_default_stake() * 2) = min_default_stake() / 5 (exact integer result)
///   Alice pays min_default_stake()*2 total and has min_default_stake()*8 remaining.
///   Charlie (fee recipient) receives exactly fee_tao.
#[test]
fn execute_orders_fee_forwarded_to_recipient() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Fund Alice with 10× min_default_stake so she can cover the order amount and a margin.
        fund_account(&alice_id);

        // Create the hotkey association Alice → Bob.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Charlie starts with zero balance — verify before submitting.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&charlie_id),
            TaoBalance::from(0u64),
            "charlie should start with zero balance"
        );

        // Use 2× min_default_stake so tao_after_fee (90%) stays above the minimum-stake threshold.
        let order_amount = min_default_stake().to_u64() * 2u64;

        // limit_price = u64::MAX → condition always met; fee_recipient = Charlie.
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            order_amount,
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::from_percent(10),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            orders,
            false,
        ));

        // Order must be marked as executed.
        assert_eq!(Orders::<Runtime>::get(id), Some(OrderStatus::Fulfilled));

        // Buy fee is computed from input: fee = 10% * order_amount. Exact integer arithmetic.
        let expected_fee = Perbill::from_percent(10) * order_amount;
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&charlie_id),
            TaoBalance::from(expected_fee),
            "charlie (fee recipient) should receive exactly the buy fee"
        );

        // Alice spent exactly order_amount TAO (fee is deducted from the order amount,
        // not charged on top), so she has min_default_stake()*10 - order_amount remaining.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&alice_id),
            min_default_stake() * 8u64.into(),
            "alice should have min_default_stake()*8 TAO remaining after the order"
        );

        // Alice must have received staked alpha through Bob. The pool received
        // tao_after_fee = order_amount - fee; check within 1% of that expected alpha.
        let tao_after_fee = order_amount - expected_fee;
        let staked =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert!(
            staked >= AlphaBalance::from(tao_after_fee * 99 / 100)
                && staked <= AlphaBalance::from(tao_after_fee),
            "alice should hold approximately tao_after_fee alpha after the LimitBuy with fee (got {staked:?})"
        );
    });
}

/// `execute_batched_orders` correctly forwards fees to a shared fee recipient (Eve)
/// when both a buy and a sell order designate the same recipient.
///
/// Fee mechanics for batched orders:
///   Buy: fee = gross - net = fee_rate * gross (withheld from pool input, transferred from pallet).
///   Sell: fee = fee_rate * gross_share (withheld from TAO pool output, inherits slippage).
///
/// The buy fee is exact; the sell fee is approximate (pool slippage).
#[test]
fn batched_fee_forwarded_to_recipient() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();
        let eve_id = Sr25519Keyring::Eve.to_account_id();

        setup_subnet(netuid);

        setup_buyer_seller(netuid, &alice_id, &charlie_id, &bob_id, &dave_id);

        // Eve (shared fee recipient) starts with zero balance.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&eve_id),
            TaoBalance::from(0u64),
            "eve should start with zero balance"
        );

        let buy = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::from_percent(10),
            eve_id.clone(), // fee goes to Eve
        );
        let sell = make_signed_order(
            bob,
            dave_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().into(),
            0,        // price floor — always satisfied
            u64::MAX, // no expiry
            Perbill::from_percent(10),
            eve_id.clone(), // fee goes to Eve
        );
        let buy_id = order_id(&buy.order);
        let sell_id = order_id(&sell.order);

        let orders = make_order_batch(vec![buy, sell]);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            netuid,
            orders,
        ));

        // Both orders must be fulfilled.
        assert_eq!(Orders::<Runtime>::get(buy_id), Some(OrderStatus::Fulfilled));
        assert_eq!(
            Orders::<Runtime>::get(sell_id),
            Some(OrderStatus::Fulfilled)
        );

        // Buy fee is exact: fee = 10% * min_default_stake().
        let buy_fee = Perbill::from_percent(10) * min_default_stake().to_u64();

        // Sell fee is approximate (pool slippage). Lower bound: 10% of 99% of amount.
        let sell_fee_lower_bound =
            Perbill::from_percent(10) * (min_default_stake().to_u64() * 99 / 100);

        // Eve must have received at least buy_fee + sell_fee_lower_bound,
        // and at most buy_fee + 10% * amount (upper bound on sell fee with no slippage).
        let sell_fee_upper_bound = Perbill::from_percent(10) * min_default_stake().to_u64();
        let eve_balance = SubtensorModule::get_coldkey_balance(&eve_id);
        assert!(
            eve_balance >= TaoBalance::from(buy_fee + sell_fee_lower_bound)
                && eve_balance <= TaoBalance::from(buy_fee + sell_fee_upper_bound),
            "eve should receive combined buy+sell fee within tolerance (got {eve_balance:?})"
        );
    });
}

/// `execute_batched_orders` routes fees to the correct recipient when two orders
/// in the same batch designate different fee recipients (Charlie for the buy,
/// Dave for the sell).
///
/// Verifies that:
///   - Charlie receives exactly the buy fee (no pool slippage on input).
///   - Dave receives approximately the sell fee (within 1%, due to pool slippage).
///   - Neither recipient received both fees.
#[test]
fn batched_multiple_fee_recipients_each_receive_correct_amount() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob = Sr25519Keyring::Bob;
        let bob_id = bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);

        setup_buyer_seller(netuid, &alice_id, &charlie_id, &bob_id, &dave_id);

        // Charlie and Dave start with zero free balance (they are hotkeys; no initial funding).
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&charlie_id),
            TaoBalance::from(0u64),
            "charlie should start with zero balance"
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&dave_id),
            TaoBalance::from(0u64),
            "dave should start with zero balance"
        );

        // Alice: LimitBuy, fee goes to Charlie.
        let buy = make_signed_order(
            alice,
            charlie_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::from_percent(10),
            charlie_id.clone(), // buy fee to Charlie
        );
        // Bob: TakeProfit, fee goes to Dave.
        let sell = make_signed_order(
            bob,
            dave_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().into(),
            0,        // price floor — always satisfied
            u64::MAX, // no expiry
            Perbill::from_percent(10),
            dave_id.clone(), // sell fee to Dave
        );
        let buy_id = order_id(&buy.order);
        let sell_id = order_id(&sell.order);

        let orders = make_order_batch(vec![buy, sell]);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            netuid,
            orders,
        ));

        // Both orders must be fulfilled.
        assert_eq!(Orders::<Runtime>::get(buy_id), Some(OrderStatus::Fulfilled));
        assert_eq!(
            Orders::<Runtime>::get(sell_id),
            Some(OrderStatus::Fulfilled)
        );

        // Charlie receives exactly the buy fee: 10% * min_default_stake().
        let expected_buy_fee = Perbill::from_percent(10) * min_default_stake().to_u64();
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&charlie_id),
            TaoBalance::from(expected_buy_fee),
            "charlie (buy fee recipient) should receive exactly the buy fee"
        );

        // Dave receives approximately the sell fee (pool slippage ≤ 1%).
        // Expected sell fee ≈ 10% of min_default_stake (the seller's gross TAO share).
        let expected_sell_fee = Perbill::from_percent(10) * min_default_stake().to_u64();
        let sell_fee_lower_bound =
            Perbill::from_percent(10) * (min_default_stake().to_u64() * 99 / 100);
        let dave_balance = SubtensorModule::get_coldkey_balance(&dave_id);
        assert!(
            dave_balance >= TaoBalance::from(sell_fee_lower_bound)
                && dave_balance <= TaoBalance::from(expected_sell_fee),
            "dave (sell fee recipient) should receive approximately the sell fee within 1% (got {dave_balance:?})"
        );

        // Verify fees are separate: neither recipient received both fees.
        // Charlie's balance is exactly buy_fee (not buy_fee + sell_fee).
        let charlie_balance = SubtensorModule::get_coldkey_balance(&charlie_id);
        assert!(
            charlie_balance <= TaoBalance::from(expected_buy_fee),
            "charlie should not have received the sell fee (got {charlie_balance:?})"
        );
        // Dave's balance is ≤ sell_fee (not sell_fee + buy_fee).
        assert!(
            dave_balance <= TaoBalance::from(expected_sell_fee),
            "dave should not have received the buy fee (got {dave_balance:?})"
        );
    });
}

// ── max_slippage enforcement against the real dynamic-mechanism AMM ───────────

/// A StopLoss order whose price condition is met (`current_price ≤ limit_price`)
/// but whose `max_slippage`-derived floor exceeds the pool's actual price is
/// silently skipped by `execute_orders`.
///
/// Setup:
///   Dynamic subnet, equal reserves → pool price = 1.0 (raw ratio, i.e. 1 rao/alpha).
///   limit_price = 2_000_000_000 (2.0 × 10⁹)  →  StopLoss trigger: 1.0 ≤ 2.0 ✓
///   max_slippage = 10%  →  effective AMM floor = 2_000_000_000 − 10% × 2_000_000_000 = 1_800_000_000.
///   Pool price = 1_000_000_000 (1.0 × 10⁹) < 1_800_000_000  →  PriceLimitExceeded.
///   `execute_orders` catches the error and skips the order (no storage write).
///   Because `sell_alpha` is `#[transactional]`, the stake decrement is rolled back.
#[test]
fn execute_orders_stoploss_max_slippage_exceeds_pool_price_skipped() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_dynamic_subnet(netuid);

        // Alice needs staked alpha so the sell can debit her position.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );

        // limit_price = 2_000_000_000 (2.0 × 10⁹): StopLoss triggers when price ≤ 2.0; pool is at 1.0 → met.
        // max_slippage = 10%  →  effective AMM floor = 1_800_000_000.
        // Pool price = 1_000_000_000 < 1_800_000_000  →  PriceLimitExceeded → order skipped.
        let signed = make_signed_order_with_slippage_rt(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::StopLoss,
            min_default_stake().into(),
            2_000_000_000, // trigger at price 2.0 × 10⁹; pool is at 1.0 — condition met
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
            Some(Perbill::from_percent(10)),
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // execute_orders is best-effort: the call succeeds even though the order
        // is rejected by the AMM.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must NOT have been written to storage — it was silently skipped.
        assert!(
            Orders::<Runtime>::get(id).is_none(),
            "order should have been skipped, not stored"
        );

        // `try_execute_order` is #[transactional]: the stake decrement inside
        // `unstake_from_subnet` is rolled back when the AMM rejects the swap,
        // so alice's alpha is unchanged.
        let remaining =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert_eq!(
            remaining, initial_alpha,
            "alice's staked alpha should be unchanged when the order is rolled back"
        );
    });
}

/// Contrasting test: the same StopLoss order without `max_slippage` executes
/// successfully against the dynamic-mechanism pool.
///
/// This confirms that the price condition alone is not the blocker and that
/// the previous test's skip is genuinely caused by the slippage floor.
#[test]
fn execute_orders_stoploss_no_slippage_executes_on_dynamic_subnet() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_dynamic_subnet(netuid);

        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );

        // Same limit_price — trigger still met.  max_slippage = None → floor = 0
        // → AMM limit = 0 → no floor constraint → pool executes the sell.
        //
        // Sell 5× min_default_stake: the dynamic AMM deducts a small fee (~0.05%)
        // from the alpha input before swapping, so the TAO output is slightly below
        // the sell amount.  The `validate_remove_stake` sim-swap check verifies that
        // the TAO equivalent is ≥ DefaultMinStake — selling 5× ensures the fee cannot
        // drag the output below that floor even on a lightly-loaded pool.
        let sell_amount = min_default_stake().to_u64() * 5;
        let signed = make_signed_order_with_slippage_rt(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::StopLoss,
            sell_amount,
            2_000_000_000,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
            None,
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must be marked as fulfilled.
        assert_eq!(
            Orders::<Runtime>::get(id),
            Some(OrderStatus::Fulfilled),
            "order should be fulfilled when no slippage floor is set"
        );

        // Alice's staked alpha must have decreased by the sold amount (5× min_default_stake).
        let remaining =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert_eq!(
            remaining,
            AlphaBalance::from(min_default_stake().to_u64() * 5u64),
            "alice's staked alpha should decrease by 5×min_default_stake after StopLoss executes"
        );
    });
}

// ── Partial fill tests ────────────────────────────────────────────────────────

/// A LimitBuy order with `partial_fills_enabled` is partially filled on the
/// first `execute_orders` call, then fully filled (Fulfilled) on a second call
/// carrying the remaining amount.
///
/// The signed payload (`VersionedOrder`) is identical in both submissions so
/// both calls share the same order-id.  Only `SignedOrder::partial_fill` changes.
#[test]
fn execute_orders_partial_fill_then_complete() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        // Alice funds two fills: partial_amount + remaining_amount = order amount.
        let order_amount = min_default_stake().to_u64() * 4u64;
        let partial_amount = min_default_stake().to_u64() * 3u64;
        let remaining_amount = order_amount - partial_amount;

        add_balance_to_coldkey_account(&alice_id, TaoBalance::from(order_amount * 2u64));

        // Create the hotkey association Alice → Bob.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Build the base signed order — this exact payload is re-used for both submissions.
        let first_signed = make_partial_fill_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            order_amount,
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            charlie_id.clone(),
            charlie_id.clone(), // relayer = caller
            Some(partial_amount),
        );
        let id = order_id(&first_signed.order);

        // ── First submission: partial fill ────────────────────────────────────
        let orders = make_order_batch(vec![first_signed.clone()]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            orders,
            false,
        ));

        // After the first execution the order must be partially filled.
        assert_eq!(
            Orders::<Runtime>::get(id),
            Some(OrderStatus::PartiallyFilled(partial_amount)),
            "order should be PartiallyFilled({partial_amount}) after first execution"
        );

        // ── Second submission: fill the remainder ─────────────────────────────
        // Clone the order payload from the first signed order (same VersionedOrder,
        // same order-id) but set partial_fill to the remaining amount.
        let second_signed = SignedOrder {
            order: first_signed.order.clone(),
            signature: first_signed.signature.clone(),
            partial_fill: Some(remaining_amount),
        };

        let orders2 = make_order_batch(vec![second_signed]);

        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            orders2,
            false,
        ));

        // After the second execution the order must be fulfilled.
        assert_eq!(
            Orders::<Runtime>::get(id),
            Some(OrderStatus::Fulfilled),
            "order should be Fulfilled after the remaining amount is filled"
        );
    });
}

/// Same partial-fill-then-complete scenario exercised through
/// `execute_batched_orders`.
///
/// The buy order is the only order in the batch both times, so the batch is
/// buy-dominant and routes all TAO through the pool.  The signed payload is
/// identical between submissions; only `SignedOrder::partial_fill` changes.
#[test]
fn execute_batched_orders_partial_fill_then_complete() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);

        let order_amount = min_default_stake().to_u64() * 4u64;
        let partial_amount = min_default_stake().to_u64() * 3u64;
        let remaining_amount = order_amount - partial_amount;

        add_balance_to_coldkey_account(&alice_id, TaoBalance::from(order_amount * 2u64));

        // Create the hotkey association Alice → Bob.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Build the base signed order — identical payload reused in both batches.
        let first_signed = make_partial_fill_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            order_amount,
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            charlie_id.clone(),
            charlie_id.clone(), // relayer = caller
            Some(partial_amount),
        );
        let id = order_id(&first_signed.order);

        // ── First batch: partial fill ─────────────────────────────────────────
        let orders = make_order_batch(vec![first_signed.clone()]);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            netuid,
            orders,
        ));

        assert_eq!(
            Orders::<Runtime>::get(id),
            Some(OrderStatus::PartiallyFilled(partial_amount)),
            "order should be PartiallyFilled({partial_amount}) after first batch"
        );

        // ── Second batch: fill the remainder ──────────────────────────────────
        let second_signed = SignedOrder {
            order: first_signed.order.clone(),
            signature: first_signed.signature.clone(),
            partial_fill: Some(remaining_amount),
        };

        let orders2 = make_order_batch(vec![second_signed]);

        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(charlie_id.clone()),
            netuid,
            orders2,
        ));

        assert_eq!(
            Orders::<Runtime>::get(id),
            Some(OrderStatus::Fulfilled),
            "order should be Fulfilled after the remaining amount is filled in the second batch"
        );
    });
}

// ── sim-swap partial-fill guard ───────────────────────────────────────────────

/// A LimitBuy order with 1 ppb max_slippage is silently skipped by
/// `execute_orders` because the sim-swap detects that the AMM would only
/// consume a microscopic fraction of the input before the price ceiling is
/// breached (partial fill).
///
/// Setup: dynamic subnet, equal 1T TAO / 1T alpha reserves → pool price = 1.0.
/// limit_price = 1_000_000_000 (1.0 × 10⁹): LimitBuy triggers when price ≤ 1.0 — met.
/// max_slippage = 1 ppb → ceiling = 1_000_000_001, barely above pool price.
/// Sending any real TAO amount immediately pushes the price above the ceiling,
/// so sim.amount_paid_in + sim.fee_paid < input_amount → SlippageTooHigh.
/// `execute_orders` is best-effort: it catches the error and skips the order.
#[test]
fn execute_orders_buy_tight_slippage_partial_fill_skipped() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_dynamic_subnet(netuid);

        // Alice needs a hotkey association for the buy to validate.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);
        // Alice needs TAO to fund the buy.
        fund_account(&alice_id);

        let initial_balance = SubtensorModule::get_coldkey_balance(&alice_id);

        // limit_price = 1_000_000_000 (= 1.0 × 10⁹): LimitBuy trigger (spot ≤ 1.0) met.
        // max_slippage = 1 ppb → price ceiling = 1_000_000_001, just above pool price.
        // Any real TAO amount pushes the price above the ceiling → partial fill detected.
        let signed = make_signed_order_with_slippage_rt(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            1_000_000_000, // price ceiling at exactly 1.0 × 10⁹ — trigger met
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
            Some(Perbill::from_parts(1)), // 1 ppb — ceiling barely above spot
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // execute_orders is best-effort: the call succeeds even though the guard
        // rejects the order due to partial fill.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must NOT have been written to storage — it was silently skipped.
        assert!(
            Orders::<Runtime>::get(id).is_none(),
            "order should have been skipped, not stored"
        );

        // No funds should have been debited from Alice — the rollback guard
        // prevents any state change when partial fill is detected.
        let final_balance = SubtensorModule::get_coldkey_balance(&alice_id);
        assert_eq!(
            final_balance, initial_balance,
            "alice's TAO balance should be unchanged when the order is rolled back"
        );
    });
}

/// Same setup as `execute_orders_buy_tight_slippage_partial_fill_skipped` but
/// submitted via `execute_batched_orders`.  The batch hard-fails with
/// `SlippageTooHigh` because batched execution is not best-effort.
#[test]
fn execute_batched_orders_buy_tight_slippage_partial_fill_fails() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_dynamic_subnet(netuid);

        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);
        fund_account(&alice_id);

        // Identical order to the execute_orders variant above.
        let signed = make_signed_order_with_slippage_rt(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            min_default_stake().into(),
            1_000_000_000,
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
            Some(Perbill::from_parts(1)),
        );

        let orders = make_order_batch(vec![signed]);

        // Batched execution hard-fails: the partial-fill guard surfaces the error
        // directly to the caller instead of silently skipping.
        assert_noop!(
            LimitOrders::execute_batched_orders(RuntimeOrigin::signed(charlie_id), netuid, orders,),
            pallet_subtensor::Error::<Runtime>::SlippageTooHigh
        );
    });
}

/// A TakeProfit order with 1 ppb max_slippage is silently skipped by
/// `execute_orders` because the sim-swap detects that selling any real alpha
/// amount immediately pushes the pool price below the 1 ppb floor.
///
/// Setup: dynamic subnet, equal 1T TAO / 1T alpha reserves → pool price = 1.0.
/// limit_price = 1_000_000_000 (1.0 × 10⁹): TakeProfit triggers when price ≥ 1.0 — met.
/// max_slippage = 1 ppb → floor = 999_999_999, barely below pool price.
/// Selling any real alpha amount moves the price below the floor,
/// so sim.amount_paid_in + sim.fee_paid < input_amount → SlippageTooHigh.
/// `execute_orders` is best-effort: it catches the error and skips the order.
#[test]
fn execute_orders_sell_tight_slippage_partial_fill_skipped() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_dynamic_subnet(netuid);

        // Alice needs a hotkey association and staked alpha for the sell.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 10u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );

        // limit_price = 1_000_000_000 (= 1.0 × 10⁹): TakeProfit trigger (spot ≥ 1.0) met.
        // max_slippage = 1 ppb → price floor = 999_999_999, just below pool price.
        // Any real alpha sale pushes the price below the floor → partial fill detected.
        let signed = make_signed_order_with_slippage_rt(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().into(),
            1_000_000_000, // price floor at exactly 1.0 × 10⁹ — trigger met
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
            Some(Perbill::from_parts(1)), // 1 ppb — floor barely below spot
        );
        let id = order_id(&signed.order);

        let orders = make_order_batch(vec![signed]);

        // execute_orders is best-effort: the call succeeds even though the guard
        // rejects the order due to partial fill.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            orders,
            false,
        ));

        // Order must NOT have been written to storage — it was silently skipped.
        assert!(
            Orders::<Runtime>::get(id).is_none(),
            "order should have been skipped, not stored"
        );

        // Alice's staked alpha must be unchanged — the rollback guard prevents
        // any state change when partial fill is detected.
        let remaining_alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert_eq!(
            remaining_alpha, initial_alpha,
            "alice's staked alpha should be unchanged when the order is rolled back"
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Migration integration tests
// ─────────────────────────────────────────────────────────────────────────────

fn migration_key() -> BoundedVec<u8, ConstU32<128>> {
    BoundedVec::truncate_from(b"migrate_register_pallet_hotkey".to_vec())
}

fn pallet_acct() -> AccountId {
    PalletId(*b"bt/limit").into_account_truncating()
}

fn pallet_hotkey() -> AccountId {
    PalletId(*b"bt/lmhky").into_account_truncating()
}

/// `on_runtime_upgrade` registers the pallet hotkey and marks the migration as run.
///
/// Starting from the default genesis (which already registers the hotkey and
/// enables the pallet via `GenesisConfig::build`), the upgrade hook must:
/// - set `HasMigrationRun[migration_key]` to `true`
/// - leave `LimitOrdersEnabled` untouched (still `true`)
/// - leave the hotkey registration intact
#[test]
fn on_runtime_upgrade_marks_migration_run_without_touching_pallet_status() {
    new_test_ext().execute_with(|| {
        assert!(LimitOrdersEnabled::<Runtime>::get());
        assert!(!HasMigrationRun::<Runtime>::get(migration_key()));
        assert!(SubtensorModule::coldkey_owns_hotkey(
            &pallet_acct(),
            &pallet_hotkey()
        ));

        <LimitOrders as Hooks<u32>>::on_runtime_upgrade();

        assert!(
            HasMigrationRun::<Runtime>::get(migration_key()),
            "migration must be marked as run"
        );
        assert!(
            LimitOrdersEnabled::<Runtime>::get(),
            "upgrade must not change LimitOrdersEnabled"
        );
        assert!(SubtensorModule::coldkey_owns_hotkey(
            &pallet_acct(),
            &pallet_hotkey()
        ));
    });
}

/// Running `on_runtime_upgrade` twice is a no-op on the second call.
#[test]
fn on_runtime_upgrade_is_idempotent() {
    new_test_ext().execute_with(|| {
        <LimitOrders as Hooks<u32>>::on_runtime_upgrade();
        assert!(HasMigrationRun::<Runtime>::get(migration_key()));

        // Second run must not change any state.
        LimitOrdersEnabled::<Runtime>::set(false);
        <LimitOrders as Hooks<u32>>::on_runtime_upgrade();

        assert!(
            !LimitOrdersEnabled::<Runtime>::get(),
            "second upgrade must not touch LimitOrdersEnabled"
        );
    });
}

// ── Conviction-lock protection ────────────────────────────────────────────────

/// A sell order whose alpha is fully conviction-locked is silently skipped by
/// `execute_orders` (best-effort path): the extrinsic returns `Ok`, the order
/// is never written to `Orders` storage, and the seller's staked alpha is
/// unchanged.
#[test]
fn individual_sell_order_skipped_when_alpha_is_conviction_locked() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Give alice staked alpha through bob.
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 3u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );
        seed_subnet_tao(netuid, TaoBalance::from(initial_alpha.to_u64()));

        // Lock ALL of alice's alpha with conviction — nothing is available to sell.
        assert_ok!(SubtensorModule::do_lock_stake(
            &alice_id,
            netuid,
            &bob_id,
            initial_alpha,
        ));

        let sell_amount = min_default_stake().to_u64();
        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::TakeProfit,
            sell_amount,
            0, // price floor — always satisfied
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        // Best-effort: the locked order is silently skipped, extrinsic still returns Ok.
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(charlie_id),
            make_order_batch(vec![signed]),
            false,
        ));

        // Order must NOT be in storage — it was skipped, not fulfilled.
        assert_eq!(
            Orders::<Runtime>::get(id),
            None,
            "order should be skipped when alpha is conviction-locked"
        );

        // Alice's staked alpha must be completely unchanged.
        let remaining =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        assert_eq!(
            remaining, initial_alpha,
            "conviction-locked alpha must not be moved by a skipped sell order"
        );
    });
}

/// A batched sell order whose alpha is fully conviction-locked causes the
/// entire `execute_batched_orders` call to fail atomically with
/// `StakeUnavailable` — no state is committed.
#[test]
fn batched_sell_order_fails_when_alpha_is_conviction_locked() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();

        setup_subnet(netuid);
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // Give alice staked alpha through bob.
        let initial_alpha: AlphaBalance = (min_default_stake().to_u64() * 3u64).into();
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob_id,
            &alice_id,
            netuid,
            initial_alpha,
        );
        seed_subnet_tao(netuid, TaoBalance::from(initial_alpha.to_u64()));

        // Lock ALL of alice's alpha with conviction — nothing is available to sell.
        assert_ok!(SubtensorModule::do_lock_stake(
            &alice_id,
            netuid,
            &bob_id,
            initial_alpha,
        ));

        let sell = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::TakeProfit,
            min_default_stake().to_u64(),
            0, // price floor — always satisfied
            u64::MAX,
            Perbill::zero(),
            charlie_id.clone(),
        );

        // Atomic path: the lock violation must revert the entire batch.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(charlie_id),
                netuid,
                make_order_batch(vec![sell]),
            ),
            pallet_subtensor::Error::<Runtime>::StakeUnavailable
        );
    });
}

/// Regression test for `#[transactional]` on `try_execute_order`.
///
/// Invariant: a single buy order is **atomic** — either the TAO→alpha swap AND
/// the fee transfer both commit (and the order is recorded), or neither does.
///
/// ## Trigger (buy path)
/// `buy_alpha` only checks the signer can remove `tao_after_fee`, NOT the full
/// `tao_in`.  So if the signer's free TAO sits in the window
/// `[tao_after_fee, tao_in)`, `buy_alpha` succeeds but the subsequent
/// `forward_fee` of `fee_tao` fails for insufficient funds.  In best-effort mode
/// (`should_fail = false`) the caller catches that `Err`, emits `OrderSkipped`,
/// and returns `Ok(())`.  Without `#[transactional]` the orphaned `buy_alpha`
/// swap would be committed by the outer storage layer; with it, the whole order
/// rolls back.
///
/// ## Arithmetic (ED = 500, min_default_stake = 2_000_000)
/// - `amount = tao_in = min_default_stake * 10 = 20_000_000`
/// - `fee_rate = 10%` → `fee_tao = 2_000_000`, `tao_after_fee = 18_000_000`
///   (≥ min_default_stake, so it clears the `AmountTooLow` check).
/// - Fund the signer with `B = 19_000_000`, which sits strictly inside the
///   vulnerable window `[18_000_000, 20_000_000)`:
///     * `buy_alpha` passes: `tao_after_fee (18_000_000) ≤ B`, and
///       `stake_into_subnet` debits the full `18_000_000` because
///       `B - ED = 18_999_500 ≥ 18_000_000` (no ED clamp).  Balance → 1_000_000.
///     * `forward_fee` of `fee_tao (2_000_000)` then fails: only 1_000_000 left.
#[test]
fn fee_failure_after_buy_rolls_back_swap() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1u16);
        let alice = Sr25519Keyring::Alice;
        let alice_id = alice.to_account_id();
        let bob_id = Sr25519Keyring::Bob.to_account_id();
        // Fee recipient distinct from the signer (Alice) and the relayer.
        let charlie_id = Sr25519Keyring::Charlie.to_account_id();
        // Relayer that submits the batch — kept distinct so its balance is irrelevant.
        let dave_id = Sr25519Keyring::Dave.to_account_id();

        setup_subnet(netuid);

        // Create the hotkey association so buy_alpha's validation passes.
        let _ = SubtensorModule::create_account_if_non_existent(&alice_id, &bob_id);

        // amount = tao_in = min_default_stake * 10; fee_rate = 10%.
        //   fee_tao        = 2_000_000
        //   tao_after_fee  = 18_000_000  (≥ min_default_stake, clears AmountTooLow)
        let tao_in = min_default_stake().to_u64() * 10u64;

        // Fund Alice's coldkey so her free TAO is inside the vulnerable window
        // [tao_after_fee, tao_in) = [18_000_000, 20_000_000): 19_000_000.
        //   buy_alpha passes (18_000_000 ≤ 19_000_000, and 19_000_000 - ED clears the
        //   full debit), leaving 1_000_000 — which is < fee_tao (2_000_000), so
        //   forward_fee fails for insufficient funds.
        let signer_balance = TaoBalance::from(19_000_000u64);
        add_balance_to_coldkey_account(&alice_id, signer_balance);

        let signed = make_signed_order(
            alice,
            bob_id.clone(),
            netuid,
            OrderType::LimitBuy,
            tao_in,
            u64::MAX, // price ceiling — always satisfied
            u64::MAX, // no expiry
            Perbill::from_percent(10),
            charlie_id.clone(),
        );
        let id = order_id(&signed.order);

        // Snapshot the observable state before the call.
        let alice_balance_before = SubtensorModule::get_coldkey_balance(&alice_id);
        let alice_stake_before =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid);
        let charlie_balance_before = SubtensorModule::get_coldkey_balance(&charlie_id);

        // Sanity: Alice really is funded to 19_000_000, inside the window.
        assert_eq!(alice_balance_before, signer_balance);

        let orders = make_order_batch(vec![signed]);

        // Best-effort path: the per-order error is caught, OrderSkipped is emitted,
        // and the extrinsic returns Ok(()).
        assert_ok!(LimitOrders::execute_orders(
            RuntimeOrigin::signed(dave_id),
            orders,
            false,
        ));

        // ── Atomic rollback assertions ───────────────────────────────────────────

        // 1. The signer's free TAO is unchanged: the buy_alpha debit was rolled back.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&alice_id),
            alice_balance_before,
            "signer's TAO must be unchanged: the orphaned buy_alpha swap must roll back when forward_fee fails"
        );

        // 2. No alpha was credited to the signer: the swap was rolled back.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&bob_id, &alice_id, netuid),
            alice_stake_before,
            "signer's staked alpha must be unchanged: no alpha may be credited from a rolled-back buy"
        );

        // 3. The order was NOT recorded — it must remain replayable-free, i.e. absent.
        assert!(
            Orders::<Runtime>::get(id).is_none(),
            "order must not be recorded when its execution failed and rolled back"
        );

        // 4. The fee recipient received nothing.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&charlie_id),
            charlie_balance_before,
            "fee recipient's balance must be unchanged: the fee transfer failed and rolled back"
        );

        // 5. An OrderSkipped event was emitted for this order id.
        let skipped = System::events().into_iter().any(|record| {
            matches!(
                record.event,
                RuntimeEvent::LimitOrders(pallet_limit_orders::Event::OrderSkipped {
                    order_id: skipped_id,
                    ..
                }) if skipped_id == id
            )
        });
        assert!(
            skipped,
            "an OrderSkipped event must be emitted for the failed order"
        );
    });
}
