#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
//! Unit tests for the human-readable ("clear-signing") signature path in
//! `pallet-limit-orders`: `render_account`, `render_order`, `verify_readable`,
//! and their acceptance/rejection through `is_order_valid`.
//!
//! These exercise the third verification branch
//! (`verify_order || verify_wrapped || verify_readable`), the SS58 rendering that
//! feeds it, the injectivity of the canonical message (a change to ANY order field
//! must produce a different message and therefore break the original signature),
//! and the deliberate `none` vs `[]` relayer-rendering distinction.

use frame_support::{
    assert_noop, assert_ok,
    traits::{ConstU32, Get},
    BoundedVec,
};
use sp_core::crypto::{Ss58AddressFormat, Ss58Codec};
use sp_core::{Pair, H256};
use sp_keyring::Sr25519Keyring as AccountKeyring;
use sp_runtime::{MultiSignature, Perbill};
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::OrderSwapInterface;

use crate::pallet::Pallet as LimitOrders;
use crate::{Error, Order, OrderType, VersionedOrder};

use super::mock::*;

/// The SS58 prefix accounts are rendered under, read from the same place
/// `render_account` reads it — the chain's own `frame_system::Config::SS58Prefix`
/// (pinned to 42 in the mock). Deliberately not a second hardcoded literal, so the
/// test cannot silently disagree with the runtime about the prefix.
fn ss58_prefix() -> u16 {
    <<Test as frame_system::Config>::SS58Prefix as Get<u16>>::get()
}

/// Canonical `Ss58Codec` reconstruction of an account, used to rebuild the expected
/// SS58 strings in the golden-message tests.
fn canonical_ss58(acct: &AccountId) -> String {
    acct.to_ss58check_with_version(Ss58AddressFormat::custom(ss58_prefix()))
}

/// Build the payload the readable path signs: the `<Bytes>…</Bytes>` `signRaw`
/// envelope wrapped around the canonical clear-signing message. Reconstructed
/// here from the same rendering the pallet uses so the test signs exactly what
/// `verify_readable` verifies.
fn readable_signing_payload(order: &VersionedOrder<AccountId>) -> Vec<u8> {
    let msg = LimitOrders::<Test>::render_order(order);
    [b"<Bytes>".as_slice(), &msg, b"</Bytes>".as_slice()].concat()
}

/// The bytes a signer actually puts through ed25519/sr25519 for the readable form —
/// i.e. what a Ledger emits. The device blake2_256-hashes a raw-signing payload
/// longer than `LEDGER_MAX_SIGN_SIZE` before signing it, and `verify_readable`
/// follows the same rule, so these tests must too.
fn readable_signed_bytes(order: &VersionedOrder<AccountId>) -> Vec<u8> {
    let payload = readable_signing_payload(order);
    if payload.len() > crate::LEDGER_MAX_SIGN_SIZE {
        sp_core::hashing::blake2_256(&payload).to_vec()
    } else {
        payload
    }
}

/// A fully-specified LimitBuy order that passes every non-signature guard in
/// `is_order_valid` under the default mock setup (netuid 1, chain 945, far-future
/// expiry, no relayer restriction, price condition met at price 1.0).
fn base_buy_order() -> Order<AccountId> {
    Order {
        signer: alice(),
        hotkey: bob(),
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
    }
}

/// Sign `order` with the readable (`<Bytes>` ++ render_order ++ `</Bytes>`) form
/// using an sr25519 keyring. The `order.signer` must correspond to `keyring`.
fn make_readable_signed_order(
    keyring: AccountKeyring,
    order: Order<AccountId>,
) -> crate::SignedOrder<AccountId> {
    let versioned = VersionedOrder::V1(order);
    let sig = keyring.pair().sign(&readable_signed_bytes(&versioned));
    crate::SignedOrder {
        order: versioned,
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A. SS58 prefix wiring
// ─────────────────────────────────────────────────────────────────────────────

/// `render_account` delegates encoding to sp-core's `Ss58Codec`, so the encoding
/// itself needs no cross-check. What this pins down is the *prefix wiring*: that
/// accounts are rendered under the chain's own `frame_system::Config::SS58Prefix`
/// and not some other value. If the pallet ever regressed to a local constant, or
/// read the prefix from the wrong place, this fails.
#[test]
fn render_account_uses_chain_ss58_prefix() {
    new_test_ext().execute_with(|| {
        assert_eq!(ss58_prefix(), 42, "mock must pin Bittensor's real prefix");
        let cases = vec![
            alice(),
            bob(),
            AccountId::new([0x00; 32]),
            AccountId::new([0xff; 32]),
        ];
        for acct in cases {
            let rendered = LimitOrders::<Test>::render_account(&acct);
            let canonical = canonical_ss58(&acct);
            assert_eq!(
                rendered, canonical,
                "render_account must encode at the chain's SS58 prefix for {acct:?}"
            );
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// B. render_order golden vectors
// ─────────────────────────────────────────────────────────────────────────────

struct ExpectedMessage<'a> {
    label: &'a str,
    price_word: &'a str,
    amount: u64,
    netuid_val: u16,
    limit_price: u64,
    expiry: u64,
    hotkey: &'a AccountId,
    fee_rate_ppb: u32,
    fee_recipient: &'a AccountId,
    relayer_str: &'a str,
    max_slippage_str: &'a str,
    chain_id: u64,
    partial: bool,
    signer: &'a AccountId,
}

/// Independently reconstruct the canonical message from expected values using
/// the SS58 oracle. Deliberately NOT a copy of production `format!`.
fn expected_message(expected: ExpectedMessage<'_>) -> String {
    let ExpectedMessage {
        label,
        price_word,
        amount,
        netuid_val,
        limit_price,
        expiry,
        hotkey,
        fee_rate_ppb,
        fee_recipient,
        relayer_str,
        max_slippage_str,
        chain_id,
        partial,
        signer,
    } = expected;
    format!(
        "TAO.com order v1: {label} {amount} on subnet {netuid_val}, \
{price_word} {limit_price}, expiry {expiry}, hotkey {hotkey}, \
fee {fee_rate_ppb} to {fee_recipient}, relayer {relayer_str}, \
max slippage {max_slippage_str}, chain {chain_id}, \
partial fills {partial}, signer {signer}",
        hotkey = canonical_ss58(hotkey),
        fee_recipient = canonical_ss58(fee_recipient),
        signer = canonical_ss58(signer),
    )
}

fn assert_all_printable_ascii(bytes: &[u8]) {
    for (i, b) in bytes.iter().enumerate() {
        assert!(
            (0x20..=0x7e).contains(b),
            "byte {i} = {b:#x} is not printable ASCII (Ledger-renderability invariant)"
        );
    }
}

#[test]
fn render_order_golden_limit_buy_relayer_none() {
    new_test_ext().execute_with(|| {
        let order = Order {
            signer: alice(),
            hotkey: bob(),
            netuid: NetUid::from(7u16),
            order_type: OrderType::LimitBuy,
            amount: 1_234_567,
            limit_price: 2_000_000_000,
            expiry: 9_999_999,
            fee_rate: Perbill::from_parts(5_000_000),
            fee_recipient: fee_recipient(),
            relayer: None,
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        };
        let expected = expected_message(ExpectedMessage {
            label: "Limit buy",
            price_word: "limit price",
            amount: 1_234_567,
            netuid_val: 7,
            limit_price: 2_000_000_000,
            expiry: 9_999_999,
            hotkey: &bob(),
            fee_rate_ppb: 5_000_000,
            fee_recipient: &fee_recipient(),
            relayer_str: "none",
            max_slippage_str: "none",
            chain_id: 945,
            partial: false,
            signer: &alice(),
        });
        let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V1(order));
        assert_eq!(String::from_utf8(rendered.clone()).unwrap(), expected);
        assert_all_printable_ascii(&rendered);
    });
}

#[test]
fn render_order_golden_stop_loss_trigger_price_and_slippage() {
    new_test_ext().execute_with(|| {
        // StopLoss → label "Stop-loss", price word "trigger price".
        // max_slippage Some(1%) → "10000000" ppb.
        let order = Order {
            signer: charlie(),
            hotkey: dave(),
            netuid: NetUid::from(2u16),
            order_type: OrderType::StopLoss,
            amount: 500,
            limit_price: 750_000_000,
            expiry: 42,
            fee_rate: Perbill::zero(),
            fee_recipient: alice(),
            relayer: None,
            max_slippage: Some(Perbill::from_percent(1)),
            chain_id: 945,
            partial_fills_enabled: true,
        };
        let expected = expected_message(ExpectedMessage {
            label: "Stop-loss",
            price_word: "trigger price",
            amount: 500,
            netuid_val: 2,
            limit_price: 750_000_000,
            expiry: 42,
            hotkey: &dave(),
            fee_rate_ppb: 0,
            fee_recipient: &alice(),
            relayer_str: "none",
            max_slippage_str: &Perbill::from_percent(1).deconstruct().to_string(),
            chain_id: 945,
            partial: true,
            signer: &charlie(),
        });
        let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V1(order));
        assert_eq!(String::from_utf8(rendered.clone()).unwrap(), expected);
        assert_all_printable_ascii(&rendered);
    });
}

#[test]
fn render_order_golden_take_profit_two_relayers() {
    new_test_ext().execute_with(|| {
        // Take-profit → label "Take-profit", price word "trigger price".
        // Two-relayer list → rendered accounts joined with '+'.
        let relayers: BoundedVec<AccountId, ConstU32<10>> =
            BoundedVec::try_from(vec![bob(), charlie()]).unwrap();
        let order = Order {
            signer: alice(),
            hotkey: dave(),
            netuid: NetUid::from(1u16),
            order_type: OrderType::TakeProfit,
            amount: 88,
            limit_price: 1_000_000_000,
            expiry: 100_000,
            fee_rate: Perbill::from_parts(1),
            fee_recipient: fee_recipient(),
            relayer: Some(relayers),
            max_slippage: None,
            chain_id: 945,
            partial_fills_enabled: false,
        };
        let relayer_str = format!("{}+{}", canonical_ss58(&bob()), canonical_ss58(&charlie()));
        let expected = expected_message(ExpectedMessage {
            label: "Take-profit",
            price_word: "trigger price",
            amount: 88,
            netuid_val: 1,
            limit_price: 1_000_000_000,
            expiry: 100_000,
            hotkey: &dave(),
            fee_rate_ppb: 1,
            fee_recipient: &fee_recipient(),
            relayer_str: &relayer_str,
            max_slippage_str: "none",
            chain_id: 945,
            partial: false,
            signer: &alice(),
        });
        let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V1(order));
        assert_eq!(String::from_utf8(rendered.clone()).unwrap(), expected);
        assert_all_printable_ascii(&rendered);
    });
}

#[test]
fn render_order_golden_relayer_empty_list() {
    new_test_ext().execute_with(|| {
        // Some(empty) must render as "[]", distinct from None → "none".
        let empty: BoundedVec<AccountId, ConstU32<10>> = BoundedVec::try_from(vec![]).unwrap();
        let order = Order {
            relayer: Some(empty),
            ..base_buy_order()
        };
        let expected = expected_message(ExpectedMessage {
            label: "Limit buy",
            price_word: "limit price",
            amount: 1_000,
            netuid_val: u16::from(netuid()),
            limit_price: u64::MAX,
            expiry: u64::MAX,
            hotkey: &bob(),
            fee_rate_ppb: 0,
            fee_recipient: &fee_recipient(),
            relayer_str: "[]",
            max_slippage_str: "none",
            chain_id: 945,
            partial: false,
            signer: &alice(),
        });
        let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V1(order));
        assert_eq!(String::from_utf8(rendered.clone()).unwrap(), expected);
        assert_all_printable_ascii(&rendered);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// C. verify_readable / is_order_valid accepts a readable-signed order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn is_order_valid_accepts_readable_sr25519_signature() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let order = base_buy_order();
        let signed = make_readable_signed_order(AccountKeyring::Alice, order);
        let id = LimitOrders::<Test>::derive_order_id(&signed.order);

        // Direct branch check.
        assert!(
            LimitOrders::<Test>::verify_readable(&signed),
            "readable-signed order must pass verify_readable"
        );
        // And through the full validation chain.
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
fn is_order_valid_accepts_readable_ed25519_signature() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // The signer field must be the ed25519 public key for verification.
        let ed_pair = sp_core::ed25519::Pair::from_legacy_string("//Alice", None);
        let ed_signer = AccountId::from(ed_pair.public());

        let order = Order {
            signer: ed_signer,
            ..base_buy_order()
        };
        let versioned = VersionedOrder::V1(order);
        let ed_sig = ed_pair.sign(&readable_signed_bytes(&versioned));
        let signed = crate::SignedOrder {
            order: versioned,
            signature: MultiSignature::Ed25519(ed_sig),
            partial_fill: None,
        };
        let id = LimitOrders::<Test>::derive_order_id(&signed.order);

        assert!(
            LimitOrders::<Test>::verify_readable(&signed),
            "ed25519 readable-signed order must pass verify_readable"
        );
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

// ─────────────────────────────────────────────────────────────────────────────
// D. Per-field mutation sweep (injectivity of the canonical message)
// ─────────────────────────────────────────────────────────────────────────────
//
// Sign a readable order, then for EACH order field build a clone that changes
// ONLY that field while KEEPING the original signature. The mutated order renders
// to a different message, so verify_readable (and verify_order / verify_wrapped)
// all fail → is_order_valid returns InvalidSignature — except where an earlier
// guard fires first (netuid==root, chain_id!=configured), noted per case.

/// Sign `base` readably, then swap in `mutated` (same signer) while KEEPING the
/// signature computed over `base`'s rendered message.
fn transplant_signature(
    keyring: AccountKeyring,
    base: Order<AccountId>,
    mutated: Order<AccountId>,
) -> (crate::SignedOrder<AccountId>, H256) {
    let signed_base = make_readable_signed_order(keyring, base);
    let versioned = VersionedOrder::V1(mutated);
    let id = LimitOrders::<Test>::derive_order_id(&versioned);
    let signed = crate::SignedOrder {
        order: versioned,
        signature: signed_base.signature,
        partial_fill: None,
    };
    (signed, id)
}

/// Run a mutation whose only changed field is `mutate(base)`, asserting the
/// transplanted signature is rejected as InvalidSignature.
fn assert_field_mutation_rejected(mutate: impl FnOnce(&mut Order<AccountId>)) {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let base = base_buy_order();
        let mut mutated = base.clone();
        mutate(&mut mutated);
        assert_ne!(
            base, mutated,
            "mutation must actually change the order (test bug otherwise)"
        );

        let (signed, id) = transplant_signature(AccountKeyring::Alice, base, mutated);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}

#[test]
fn mutation_signer_rejected() {
    // New signer renders differently AND the sig is verified against the new
    // signer's key → InvalidSignature. netuid non-root, chain_id 945 keep the
    // signature check reachable.
    assert_field_mutation_rejected(|o| o.signer = bob());
}

#[test]
fn mutation_hotkey_rejected() {
    assert_field_mutation_rejected(|o| o.hotkey = charlie());
}

#[test]
fn mutation_netuid_rejected() {
    // Mutate to a NON-root netuid (2) so RootNetUidNotAllowed does not pre-empt
    // the signature check.
    assert_field_mutation_rejected(|o| o.netuid = NetUid::from(2u16));
}

#[test]
fn mutation_order_type_rejected() {
    // LimitBuy → StopLoss changes both label and price word in the message.
    assert_field_mutation_rejected(|o| o.order_type = OrderType::StopLoss);
}

#[test]
fn mutation_amount_rejected() {
    assert_field_mutation_rejected(|o| o.amount = 2_000);
}

#[test]
fn mutation_limit_price_rejected() {
    assert_field_mutation_rejected(|o| o.limit_price = u64::MAX - 1);
}

#[test]
fn mutation_expiry_rejected() {
    assert_field_mutation_rejected(|o| o.expiry = u64::MAX - 1);
}

#[test]
fn mutation_fee_rate_rejected() {
    assert_field_mutation_rejected(|o| o.fee_rate = Perbill::from_parts(1));
}

#[test]
fn mutation_fee_recipient_rejected() {
    assert_field_mutation_rejected(|o| o.fee_recipient = charlie());
}

#[test]
fn mutation_relayer_rejected() {
    // None → Some([charlie]) changes the relayer rendering.
    assert_field_mutation_rejected(|o| {
        o.relayer = Some(BoundedVec::try_from(vec![charlie()]).unwrap())
    });
}

#[test]
fn mutation_max_slippage_rejected() {
    // None → Some(1%) changes "none" → "10000000".
    assert_field_mutation_rejected(|o| o.max_slippage = Some(Perbill::from_percent(1)));
}

#[test]
fn mutation_partial_fills_enabled_rejected() {
    assert_field_mutation_rejected(|o| o.partial_fills_enabled = true);
}

#[test]
fn mutation_chain_id_pre_empted_by_chain_id_guard() {
    // NOTE: chain_id is validated BEFORE the signature in `is_order_valid`
    // (`ensure!(order.chain_id == T::ChainId::get(), ChainIdMismatch)`).
    // Any change to chain_id makes it != 945, so ChainIdMismatch is reached
    // first and InvalidSignature is NOT reachable for this field. We assert the
    // specific reachable error instead. (The message still renders differently,
    // so the signature would also fail — but the guard short-circuits.)
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let base = base_buy_order();
        let mutated = Order {
            chain_id: 946,
            ..base.clone()
        };
        let (signed, id) = transplant_signature(AccountKeyring::Alice, base, mutated);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::ChainIdMismatch
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// E. Relayer None-vs-empty transplant
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn relayer_none_to_empty_transplant_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // Sign with relayer: None ("none").
        let base = Order {
            relayer: None,
            ..base_buy_order()
        };
        // Transplant onto relayer: Some(empty) ("[]"). The `none` vs `[]` rendering
        // distinction must make the message — and therefore the signature — differ.
        let empty: BoundedVec<AccountId, ConstU32<10>> = BoundedVec::try_from(vec![]).unwrap();
        let mutated = Order {
            relayer: Some(empty),
            ..base_buy_order()
        };
        assert_ne!(base, mutated, "None and Some(empty) must differ");

        let (signed, id) = transplant_signature(AccountKeyring::Alice, base, mutated);
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// F. ecdsa rejected on the readable path
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn readable_ecdsa_signature_rejected() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        // A well-formed ecdsa signature over the correct readable payload must
        // still be rejected: only sr25519 and ed25519 are accepted.
        let order = base_buy_order();
        let versioned = VersionedOrder::V1(order);
        let ecdsa_pair = sp_core::ecdsa::Pair::from_legacy_string("//Alice", None);
        let ecdsa_sig = ecdsa_pair.sign(&readable_signed_bytes(&versioned));
        let signed = crate::SignedOrder {
            order: versioned,
            signature: MultiSignature::Ecdsa(ecdsa_sig),
            partial_fill: None,
        };
        let id = LimitOrders::<Test>::derive_order_id(&signed.order);

        assert!(
            !LimitOrders::<Test>::verify_readable(&signed),
            "ecdsa signature must not pass verify_readable"
        );
        let price = MockSwap::current_alpha_price(netuid());
        assert_noop!(
            LimitOrders::<Test>::is_order_valid(&signed, id, 1_000_000, price, &bob()),
            Error::<Test>::InvalidSignature
        );
    });
}
