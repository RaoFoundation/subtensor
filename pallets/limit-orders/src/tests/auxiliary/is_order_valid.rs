//! Helper tests: `is_order_valid`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// is_order_valid
// ─────────────────────────────────────────────────────────────────────────────

use crate::Error;
use codec::Encode;
use sp_core::Pair;
use sp_runtime::{
    MultiSignature, MultiSigner,
    traits::{IdentifyAccount, Verify},
};
use subtensor_swap_interface::OrderSwapInterface;

fn make_valid_signed_order() -> (crate::SignedOrder<AccountId>, sp_core::H256) {
    let keyring = AccountKeyring::Alice;
    let order = crate::VersionedOrder::V1(crate::Order {
        signer: keyring.to_account_id(),
        hotkey: AccountKeyring::Bob.to_account_id(),
        netuid: netuid(),
        order_type: OrderType::LimitBuy,
        amount: 1_000,
        limit_price: u64::MAX,
        expiry: u64::MAX,
        fee_rate: Perbill::zero(),
        fee_recipient: fee_recipient(),
        relayer: None,
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: false,
    });
    let id = H256(sp_io::hashing::blake2_256(&order.encode()));
    let sig = keyring.pair().sign(&order.encode());
    let signed = crate::SignedOrder {
        order,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    };
    (signed, id)
}

#[test]
fn is_order_valid_returns_ok_for_well_formed_order() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, id) = make_valid_signed_order();
        let price = MockSwap::current_alpha_price(netuid());
        assert_ok!(LimitOrders::<Test>::is_order_valid(
            &signed,
            id,
            1_000_000,
            price,
            &bob()
        ));
    });
}

#[test]
fn is_order_valid_invalid_signature_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (mut signed, id) = make_valid_signed_order();
        // Replace with a signature from a different key.
        let wrong_sig = AccountKeyring::Bob.pair().sign(&signed.order.encode());
        signed.signature = MultiSignature::Sr25519(wrong_sig);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}

#[test]
fn is_order_valid_accepts_raw_ed25519_signature() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, _) = make_valid_signed_order();
        let ed_pair = sp_core::ed25519::Pair::from_legacy_string("//Alice", None);
        let order = crate::VersionedOrder::V1(crate::Order {
            signer: AccountId::from(ed_pair.public()),
            ..signed.order.inner().clone()
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let signature = ed_pair.sign(&order.encode());
        let signed = crate::SignedOrder {
            order,
            signature: MultiSignature::Ed25519(signature),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_ok!(LimitOrders::<Test>::is_order_valid(
            &signed,
            id,
            1_000_000,
            price,
            &bob()
        ));
    });
}

#[test]
fn is_order_valid_accepts_wrapped_sr25519_signature() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (mut signed, id) = make_valid_signed_order();
        let payload = [b"<Bytes>".as_slice(), id.as_bytes(), b"</Bytes>".as_slice()].concat();
        signed.signature = MultiSignature::Sr25519(AccountKeyring::Alice.pair().sign(&payload));
        let price = MockSwap::current_alpha_price(netuid());
        assert_ok!(LimitOrders::<Test>::is_order_valid(
            &signed,
            id,
            1_000_000,
            price,
            &bob()
        ));
    });
}

#[test]
fn is_order_valid_accepts_wrapped_ed25519_signature() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, _) = make_valid_signed_order();
        let ed_pair = sp_core::ed25519::Pair::from_legacy_string("//Alice", None);
        let order = crate::VersionedOrder::V1(crate::Order {
            signer: AccountId::from(ed_pair.public()),
            ..signed.order.inner().clone()
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let payload = [b"<Bytes>".as_slice(), id.as_bytes(), b"</Bytes>".as_slice()].concat();
        let signed = crate::SignedOrder {
            order,
            signature: MultiSignature::Ed25519(ed_pair.sign(&payload)),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_ok!(LimitOrders::<Test>::is_order_valid(
            &signed,
            id,
            1_000_000,
            price,
            &bob()
        ));
    });
}

#[test]
fn is_order_valid_ecdsa_signature_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, _) = make_valid_signed_order();
        let pair = sp_core::ecdsa::Pair::from_legacy_string("//Alice", None);
        let signer = MultiSigner::from(pair.public()).into_account();
        let order = crate::VersionedOrder::V1(crate::Order {
            signer,
            ..signed.order.inner().clone()
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let signature = MultiSignature::Ecdsa(pair.sign(&order.encode()));
        assert!(signature.verify(order.encode().as_slice(), &order.inner().signer));
        let signed = crate::SignedOrder {
            order,
            signature,
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}

#[test]
fn is_order_valid_already_processed_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let (signed, id) = make_valid_signed_order();
        Orders::<Test>::insert(id, crate::OrderStatus::Fulfilled);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::OrderAlreadyProcessed
        );
    });
}

#[test]
fn is_order_valid_expired_order_returns_error() {
    new_test_ext().execute_with(|| {
        MockSwap::set_price(1.0);
        let (signed, _id) = make_valid_signed_order();
        // now_ms (2_000_001) > expiry (u64::MAX is fine, so use a low expiry order).
        // Re-build a signed order with a past expiry.
        let keyring = AccountKeyring::Alice;
        let order = crate::VersionedOrder::V1(crate::Order {
            expiry: 500_000,
            ..signed.order.inner().clone()
        });
        let id2 = H256(sp_io::hashing::blake2_256(&order.encode()));
        let sig = keyring.pair().sign(&order.encode());
        let signed2 = crate::SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed2, id2, 1_000_000, price, &bob()),
            Error::<Test>::OrderExpired
        );
    });
}

#[test]
fn is_order_valid_price_condition_not_met_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        // Price 5.0, scaled = 5_000_000_000 > limit_price 2_000_000_000 (2.0 in ×10⁹) → LimitBuy condition (scaled ≤ limit) not met.
        MockSwap::set_price(5.0);
        let keyring = AccountKeyring::Alice;
        let order = crate::VersionedOrder::V1(crate::Order {
            signer: keyring.to_account_id(),
            hotkey: AccountKeyring::Bob.to_account_id(),
            netuid: netuid(),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: 2_000_000_000, // 2.0 in ×10⁹ scale
            expiry: u64::MAX,
            fee_rate: Perbill::zero(),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let sig = keyring.pair().sign(&order.encode());
        let signed = crate::SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::PriceConditionNotMet
        );
    });
}

#[test]
fn is_order_valid_wrong_chain_id_returns_error() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);
        let keyring = AccountKeyring::Alice;
        // Build an order with a chain_id that doesn't match the mock config (945).
        let order = crate::VersionedOrder::V1(crate::Order {
            chain_id: 9999,
            ..make_valid_signed_order().0.order.inner().clone()
        });
        let id = H256(sp_io::hashing::blake2_256(&order.encode()));
        let sig = keyring.pair().sign(&order.encode());
        let signed = crate::SignedOrder {
            order,
            signature: MultiSignature::Sr25519(sig),
            partial_fill: None,
        };
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::ChainIdMismatch
        );
    });
}
