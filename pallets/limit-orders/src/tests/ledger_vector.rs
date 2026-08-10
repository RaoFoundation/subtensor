#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
//! Hardware test vector for the human-readable ("clear-signing") signing form.
//!
//! ## What this pins
//!
//! A Ledger blake2_256-hashes a raw-signing (`signRaw`) payload longer than
//! `MAX_SIGN_SIZE` = 256 bytes before signing it (`crypto_sign_ed25519` in
//! `app/src/crypto.c` of the Zondax Polkadot app; the same `app_sign_ed25519`
//! callback serves both `INS_SIGN` and `INS_SIGN_RAW`). The readable message is
//! always over that limit, so a real device signature commits to
//! `blake2_256(<Bytes> ++ message ++ </Bytes>)`, never to the payload bytes —
//! which is why `verify_readable` follows the same rule.
//!
//! That rule is NOT the symmetric one in `Encode for SignedPayload`
//! (`sp_runtime::generic::unchecked_extrinsic`): that impl is only reached when the
//! extrinsic machinery rebuilds a *transaction* signing payload, and both signer and
//! verifier go through it. Raw message signing has no such mirror — polkadot-js's
//! `pair.sign()` applies a length rule for ecdsa only — so on this path the device
//! hashes and a software signer does not.
//!
//! Two things are pinned here:
//!   1. `render_order` produces byte-for-byte the message the device displayed and
//!      signed, and its wrapped payload hashes to the digest the device signed over.
//!   2. That digest, not the payload, is what the recorded device signature verifies
//!      against — on real hardware.
//!
//! ## Provenance
//!
//! Captured 2026-07-28 from a Nano S+ running Polkadot Generic v100.0.25, derivation
//! path `m/44'/354'/0'/0'/0'`. The device rendered the whole order text across its
//! screens before signing, so digest signing is NOT a blind-signing fallback:
//! clear-signing works, and shrinking the message under 256 bytes would only reduce
//! the page count. A probe matrix ruled out the alternatives (unwrapped message,
//! `blake2_256` of the unwrapped message, blake2_512, ASCII hex of the digest) —
//! all of them are re-checked below.
//!
//! ## Why this is not an end-to-end order test
//!
//! The capture's device key is not the account named in the message's `signer` field
//! (the message was rendered for a different account), and `verify_readable` checks
//! the signature against `order.signer`. So the vector pins the *rule* and the
//! *rendering*, not order execution; the acceptance path itself is covered by
//! `tests/readable.rs`. An executable vector needs a fresh capture whose message
//! renders `signer` as the device's own address, with `chain_id` matching the test
//! environment and `expiry` in milliseconds (this one's `1793000000` is a
//! seconds-scale value, i.e. long expired).
//!
//! The *executable* vector at the bottom of this file closes that gap without a
//! device: the software half's seed is known, ed25519 is deterministic (RFC 8032),
//! and the device's only transformation is the conditional hash — so a signature
//! minted offline over `blake2_256(payload)` for a message naming that account as
//! `signer` is byte-identical to what a Ledger holding the seed would return, and it
//! goes through the full acceptance path.

use frame_support::{assert_ok, traits::Get};
use sp_core::{Pair, crypto::Ss58Codec, hexdisplay::HexDisplay};
use sp_runtime::{MultiSignature, Perbill, traits::Verify};
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::OrderSwapInterface;

use crate::pallet::Pallet as LimitOrders;
use crate::{LEDGER_MAX_SIGN_SIZE, Order, OrderType, VersionedOrder};

use super::mock::*;

// ── The vector ───────────────────────────────────────────────────────────────

/// The exact text the device displayed and signed: `render_order`'s output for
/// [`vector_order`], SS58 prefix 42. 381 bytes.
const ORDER_MESSAGE: &str = "TAO.com order v1: Limit buy 1000000000 on subnet 64, \
limit price 500000000, expiry 1793000000, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, max slippage 7500000, \
chain 1, partial fills true, signer 5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC";

/// Length of `ORDER_MESSAGE` in bytes.
const ORDER_MESSAGE_LEN: usize = 381;

/// Length of `<Bytes>` ++ `ORDER_MESSAGE` ++ `</Bytes>` — the blob that reaches the
/// device, and what it hashes because 396 > 256.
const WRAPPED_PAYLOAD_LEN: usize = 396;

/// `blake2_256(<Bytes> ++ ORDER_MESSAGE ++ </Bytes>)`: the 32 bytes the device signs.
const WRAPPED_PAYLOAD_DIGEST: [u8; 32] = [
    0x3c, 0x3e, 0xa8, 0x8b, 0x51, 0x45, 0x71, 0x89, 0x38, 0x89, 0x06, 0xee, 0xcb, 0x58, 0x2d, 0x5e,
    0xbf, 0x48, 0x1b, 0x1a, 0xf5, 0xb6, 0x6b, 0x6b, 0x57, 0x71, 0xe4, 0xe8, 0x4b, 0x6e, 0x5e, 0xd7,
];

/// ed25519 public key of the Nano S+ account that produced [`DEVICE_SIGNATURE`].
const DEVICE_PUBLIC_KEY: [u8; 32] = [
    0x76, 0xe2, 0x81, 0x5d, 0x89, 0xea, 0x8f, 0x87, 0xa7, 0xfc, 0x62, 0xc2, 0x1b, 0x3e, 0xe2, 0xfb,
    0x81, 0xd7, 0x8c, 0xa2, 0x8a, 0x24, 0xd3, 0x3a, 0x97, 0x4f, 0x47, 0xb2, 0x0b, 0xb7, 0x0a, 0x63,
];

/// The signature the device returned for the 396-byte wrapped payload.
const DEVICE_SIGNATURE: [u8; 64] = [
    0x91, 0xa3, 0x7e, 0x50, 0xd0, 0x1e, 0xeb, 0x40, 0x7d, 0x9d, 0x19, 0x02, 0x37, 0x4f, 0xef, 0x24,
    0xdc, 0x28, 0x7c, 0x1e, 0xdb, 0x81, 0x47, 0x4d, 0xbe, 0x19, 0xe4, 0x61, 0x57, 0xbc, 0xc2, 0x3d,
    0xc5, 0xb7, 0xba, 0x72, 0x3a, 0xe7, 0xf8, 0xdd, 0x19, 0x20, 0x04, 0xc1, 0x50, 0xa8, 0xd0, 0x47,
    0x9a, 0x0c, 0xcb, 0x52, 0x2c, 0x93, 0x0d, 0xc8, 0xfc, 0xda, 0x7a, 0x15, 0xd6, 0xb4, 0x5b, 0x08,
];

/// Software half of the vector: reproducible without a device. Seed `0x01..0x20`,
/// with a signature over EACH form so both semantics have a fixture.
const SOFTWARE_SEED: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

/// ed25519 public key derived from [`SOFTWARE_SEED`].
const SOFTWARE_PUBLIC_KEY: [u8; 32] = [
    0x79, 0xb5, 0x56, 0x2e, 0x8f, 0xe6, 0x54, 0xf9, 0x40, 0x78, 0xb1, 0x12, 0xe8, 0xa9, 0x8b, 0xa7,
    0x90, 0x1f, 0x85, 0x3a, 0xe6, 0x95, 0xbe, 0xd7, 0xe0, 0xe3, 0x91, 0x0b, 0xad, 0x04, 0x96, 0x64,
];

/// `ed25519(SOFTWARE_SEED, wrapped payload)` — the shape a software `signRaw` emits.
const SOFTWARE_SIGNATURE_OVER_PAYLOAD: [u8; 64] = [
    0xc8, 0xd1, 0x2f, 0xfc, 0xdc, 0x50, 0x4a, 0x95, 0x6b, 0x97, 0xfd, 0x67, 0x00, 0x9d, 0xe2, 0x8c,
    0x65, 0x41, 0xbf, 0x79, 0xdc, 0x33, 0x90, 0x30, 0x92, 0xd9, 0xf1, 0xc2, 0x79, 0x71, 0x8c, 0x97,
    0x91, 0xcf, 0x5b, 0xc6, 0x9a, 0x38, 0x89, 0xc6, 0x69, 0x9a, 0x5a, 0xab, 0x18, 0x17, 0x0c, 0xdc,
    0x23, 0x66, 0xf8, 0x1d, 0xae, 0xa5, 0xec, 0xd3, 0x1c, 0x64, 0x10, 0x83, 0x85, 0xb1, 0xb6, 0x0d,
];

/// `ed25519(SOFTWARE_SEED, blake2_256(wrapped payload))` — the shape the device emits.
const SOFTWARE_SIGNATURE_OVER_DIGEST: [u8; 64] = [
    0x81, 0xed, 0xd4, 0x7a, 0x3e, 0x02, 0xb7, 0xf5, 0x2c, 0xc6, 0xa7, 0xdb, 0x02, 0xe9, 0xa8, 0xc0,
    0x23, 0xc1, 0xf1, 0x01, 0x52, 0x6a, 0x7d, 0x5f, 0xe4, 0xbe, 0x11, 0x8a, 0xff, 0x36, 0x09, 0x0c,
    0xcf, 0x65, 0xf7, 0x51, 0x36, 0xb3, 0x1f, 0x6f, 0x64, 0xd8, 0xbc, 0xec, 0xc4, 0xe4, 0x41, 0xb3,
    0x23, 0x22, 0xc3, 0x7b, 0x4a, 0xf5, 0x14, 0x36, 0xa1, 0xe9, 0x90, 0x96, 0x20, 0x2b, 0x86, 0x08,
];

/// The order whose rendering is `ORDER_MESSAGE`. Accounts are decoded from the SS58
/// strings in the message itself, so the rendering assertion is a round-trip through
/// `Ss58Codec` rather than a comparison of one `render_account` call against another.
fn vector_order() -> Order<AccountId> {
    let account = |s: &str| AccountId::from_ss58check(s).expect("vector SS58 must decode");
    Order {
        signer: account("5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC"),
        hotkey: account("5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN"),
        netuid: NetUid::from(64u16),
        order_type: OrderType::LimitBuy,
        amount: 1_000_000_000,
        limit_price: 500_000_000,
        expiry: 1_793_000_000,
        fee_rate: Perbill::from_parts(8_500_000),
        fee_recipient: account("5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY"),
        relayer: Some(
            frame_support::BoundedVec::try_from(vec![account(
                "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty",
            )])
            .unwrap(),
        ),
        max_slippage: Some(Perbill::from_parts(7_500_000)),
        chain_id: 1,
        partial_fills_enabled: true,
    }
}

/// `<Bytes>` ++ `ORDER_MESSAGE` ++ `</Bytes>`, built from the pinned text rather than
/// from `render_order`, so the two can be compared.
fn wrapped_payload() -> Vec<u8> {
    [
        b"<Bytes>".as_slice(),
        ORDER_MESSAGE.as_bytes(),
        b"</Bytes>".as_slice(),
    ]
    .concat()
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// `render_order` must reproduce the message the device actually displayed and
/// signed. If this fails, the pallet and the hardware no longer agree on what a
/// signature means — every signature captured by a user becomes unverifiable.
#[test]
fn render_order_matches_the_device_displayed_message() {
    new_test_ext().execute_with(|| {
        assert_eq!(
            <<Test as frame_system::Config>::SS58Prefix as Get<u16>>::get(),
            42,
            "the vector was captured at SS58 prefix 42"
        );

        let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V1(vector_order()));
        assert_eq!(
            String::from_utf8(rendered.clone()).unwrap(),
            ORDER_MESSAGE,
            "render_order drifted from the message a real Ledger signed"
        );
        assert_eq!(rendered.len(), ORDER_MESSAGE_LEN);
        for (i, b) in rendered.iter().enumerate() {
            assert!(
                (0x20..=0x7e).contains(b),
                "byte {i} = {b:#x} is not printable ASCII, so the device would render \
                 it as hex instead of text"
            );
        }
    });
}

/// The wrapped payload is over Ledger's limit and hashes to the digest the device
/// signed. Ties our own bytes to the recorded hardware signature.
#[test]
fn wrapped_payload_is_oversized_and_hashes_to_the_signed_digest() {
    new_test_ext().execute_with(|| {
        let from_render = {
            let msg = LimitOrders::<Test>::render_order(&VersionedOrder::V1(vector_order()));
            [b"<Bytes>".as_slice(), &msg, b"</Bytes>".as_slice()].concat()
        };
        assert_eq!(from_render, wrapped_payload());
        assert_eq!(from_render.len(), WRAPPED_PAYLOAD_LEN);
        assert!(
            from_render.len() > LEDGER_MAX_SIGN_SIZE,
            "396 bytes must exceed the {LEDGER_MAX_SIGN_SIZE}-byte device limit"
        );
        assert_eq!(
            sp_core::hashing::blake2_256(&from_render),
            WRAPPED_PAYLOAD_DIGEST
        );
    });
}

/// The hardware fact this whole branch rests on: the Nano S+ signature verifies
/// against `blake2_256(payload)` and against nothing else. The rejected forms are
/// the alternatives the capture's probe matrix ruled out.
#[test]
fn device_signature_is_over_the_blake2_256_digest_only() {
    new_test_ext().execute_with(|| {
        let signer = AccountId::new(DEVICE_PUBLIC_KEY);
        let signature =
            MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(DEVICE_SIGNATURE));
        let payload = wrapped_payload();
        let message = ORDER_MESSAGE.as_bytes();

        assert!(
            signature.verify(&WRAPPED_PAYLOAD_DIGEST[..], &signer),
            "recorded device signature must verify over blake2_256(wrapped payload)"
        );

        for (form, bytes) in [
            ("the raw wrapped payload", payload.clone()),
            ("the unwrapped message", message.to_vec()),
            (
                "blake2_256 of the unwrapped message",
                sp_core::hashing::blake2_256(message).to_vec(),
            ),
            (
                "blake2_512 of the wrapped payload",
                sp_core::hashing::blake2_512(&payload).to_vec(),
            ),
            (
                // Lowercase hex without `0x`, i.e. what a JS `u8aToHex(d).slice(2)`
                // would have put on the wire.
                "the ASCII hex of the digest",
                format!("{}", HexDisplay::from(&WRAPPED_PAYLOAD_DIGEST)).into_bytes(),
            ),
        ] {
            assert!(
                !signature.verify(bytes.as_slice(), &signer),
                "device signature must NOT verify over {form}"
            );
        }
    });
}

/// The software half, and the reason a verifier cannot be lenient: the two forms are
/// mutually unverifiable, so signer and verifier disagreeing about which one is in
/// play is a hard rejection, never a soft fallback.
#[test]
fn software_vector_forms_are_mutually_unverifiable() {
    new_test_ext().execute_with(|| {
        let pair = sp_core::ed25519::Pair::from_seed(&SOFTWARE_SEED);
        assert_eq!(
            AccountId::from(pair.public()),
            AccountId::new(SOFTWARE_PUBLIC_KEY),
            "sp-core must derive the pinned public key from the pinned seed"
        );

        let signer = AccountId::new(SOFTWARE_PUBLIC_KEY);
        let over_payload = MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(
            SOFTWARE_SIGNATURE_OVER_PAYLOAD,
        ));
        let over_digest = MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(
            SOFTWARE_SIGNATURE_OVER_DIGEST,
        ));
        let payload = wrapped_payload();

        assert!(over_payload.verify(payload.as_slice(), &signer));
        assert!(!over_payload.verify(&WRAPPED_PAYLOAD_DIGEST[..], &signer));
        assert!(over_digest.verify(&WRAPPED_PAYLOAD_DIGEST[..], &signer));
        assert!(!over_digest.verify(payload.as_slice(), &signer));
    });
}

// ── Executable vector ────────────────────────────────────────────────────────
//
// The hardware capture above cannot be submitted as an order: its message names
// `5CD9UfFv…` as the signer while the device that signed holds `5Ekanz…`, and
// `verify_readable` checks the signature against `order.signer`. Rejecting it is
// correct, so the acceptance path needs a vector whose message names the signing
// account.
//
// This one is minted offline from `SOFTWARE_SEED` (which we hold), over
// `blake2_256(<Bytes> ++ message ++ </Bytes>)` — the same bytes the device signs.
// ed25519 is deterministic and the transformation is fixed, so these are exactly
// the bytes a Ledger holding that seed would return for this order. What it does
// NOT do is attest to device behaviour; that is what the capture above is for.
//
// Fields are chosen to clear every non-signature guard in `is_order_valid` under
// the default mock: netuid 1 (non-root), chain 945 (the mock's `ChainId`), expiry
// `u64::MAX`, no relayer restriction, and limit price 1.0 TAO/alpha so the LimitBuy
// trigger fires at the mock price. Hotkey and fee recipient are the well-known dev
// accounts Bob (sr25519) and Charlie, decoded from SS58 so the same constants are
// reusable from TypeScript.

/// Rendering of [`executable_vector_order`], signed by `SOFTWARE_SEED`'s account.
const EXECUTABLE_MESSAGE: &str = "TAO.com order v1: Limit buy 1000 on subnet 1, \
limit price 1000000000, expiry 18446744073709551615, \
hotkey 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, \
fee 0 to 5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y, \
relayer none, max slippage none, chain 945, \
partial fills false, signer 5EpHX5foDtnhZngj4GsKq5eKGpUvuMqbpUG48ZfCCCs7EzKR";

/// `blake2_256` of the wrapped [`EXECUTABLE_MESSAGE`] — 350 bytes wrapped, so hashed.
const EXECUTABLE_VECTOR_DIGEST: [u8; 32] = [
    0xcd, 0x8f, 0x76, 0xe8, 0x89, 0xc5, 0x86, 0xd5, 0xef, 0xb7, 0x3d, 0xd0, 0x34, 0x33, 0xdc, 0x16,
    0x4b, 0x75, 0xfd, 0x72, 0x7c, 0x52, 0xaa, 0xa4, 0xc8, 0xd0, 0x7e, 0xb1, 0x3d, 0xc9, 0x8c, 0x12,
];

/// `ed25519(SOFTWARE_SEED, EXECUTABLE_VECTOR_DIGEST)`.
const EXECUTABLE_VECTOR_SIGNATURE: [u8; 64] = [
    0xca, 0x9e, 0x4c, 0x33, 0x69, 0x50, 0x72, 0xff, 0xef, 0x1e, 0x3e, 0x1d, 0x07, 0x15, 0x97, 0x9e,
    0x3a, 0x4d, 0x8b, 0x55, 0x3e, 0xe1, 0xec, 0xc2, 0x9e, 0x5d, 0xa9, 0xea, 0xd5, 0x78, 0x89, 0x10,
    0x4d, 0x4b, 0xc7, 0x76, 0x0c, 0x4f, 0x9a, 0x86, 0x7e, 0x82, 0xb0, 0x46, 0x27, 0x1e, 0x47, 0xb5,
    0x54, 0x60, 0x94, 0x89, 0xd6, 0x66, 0x16, 0x8b, 0x54, 0x9d, 0x75, 0xb3, 0x9b, 0x55, 0x9b, 0x04,
];

/// The order [`EXECUTABLE_VECTOR_SIGNATURE`] authorises. Its `signer` IS the account
/// that signed, which is what makes it submittable.
fn executable_vector_order() -> Order<AccountId> {
    let account = |s: &str| AccountId::from_ss58check(s).expect("vector SS58 must decode");
    Order {
        signer: AccountId::new(SOFTWARE_PUBLIC_KEY),
        hotkey: account("5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty"),
        netuid: NetUid::from(1u16),
        order_type: OrderType::LimitBuy,
        amount: 1_000,
        limit_price: 1_000_000_000,
        expiry: u64::MAX,
        fee_rate: Perbill::zero(),
        fee_recipient: account("5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y"),
        relayer: None,
        max_slippage: None,
        chain_id: 945,
        partial_fills_enabled: false,
    }
}

/// The frozen signature must be the one `SOFTWARE_SEED` produces for this order's
/// rendering — pins the message, the digest, and the signature together, so any drift
/// in `render_order` fails here with the message diff rather than as an opaque
/// signature rejection.
#[test]
fn executable_vector_is_the_seeds_signature_over_the_rendered_message() {
    new_test_ext().execute_with(|| {
        let rendered =
            LimitOrders::<Test>::render_order(&VersionedOrder::V1(executable_vector_order()));
        assert_eq!(
            String::from_utf8(rendered.clone()).unwrap(),
            EXECUTABLE_MESSAGE,
            "render_order drifted from the message the frozen signature covers"
        );

        let payload = [b"<Bytes>".as_slice(), &rendered, b"</Bytes>".as_slice()].concat();
        assert!(
            payload.len() > LEDGER_MAX_SIGN_SIZE,
            "must be hashed, not signed bare"
        );
        assert_eq!(
            sp_core::hashing::blake2_256(&payload),
            EXECUTABLE_VECTOR_DIGEST
        );
        assert_eq!(
            sp_core::ed25519::Pair::from_seed(&SOFTWARE_SEED).sign(&EXECUTABLE_VECTOR_DIGEST),
            sp_core::ed25519::Signature::from_raw(EXECUTABLE_VECTOR_SIGNATURE),
            "ed25519 is deterministic, so the seed must reproduce the frozen signature"
        );
    });
}

/// The point of the whole exercise: a hardcoded, device-shaped signature is accepted
/// by `verify_readable` and clears the full validation chain.
#[test]
fn executable_vector_is_accepted_by_verify_readable_and_is_order_valid() {
    new_test_ext().execute_with(|| {
        MockTime::set(1_000_000);
        MockSwap::set_price(1.0);

        let signed = crate::SignedOrder {
            order: VersionedOrder::V1(executable_vector_order()),
            signature: MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(
                EXECUTABLE_VECTOR_SIGNATURE,
            )),
            partial_fill: None,
        };
        let id = LimitOrders::<Test>::derive_order_id(&signed.order);

        assert!(
            LimitOrders::<Test>::verify_readable(&signed),
            "a signature in the form a Ledger emits must pass verify_readable"
        );
        assert_ok!(LimitOrders::<Test>::is_order_valid(
            &signed,
            id,
            1_000_000,
            MockSwap::current_alpha_price(netuid()),
            &bob()
        ));
    });
}
