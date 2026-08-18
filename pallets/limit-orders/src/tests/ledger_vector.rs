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
use sp_core::{H256, Pair, crypto::Ss58Codec, hexdisplay::HexDisplay};
use sp_runtime::{MultiSignature, Perbill, traits::Verify};
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::OrderSwapInterface;

use crate::pallet::Pallet as LimitOrders;
use crate::{LEDGER_MAX_SIGN_SIZE, Order, OrderAmount, OrderType, OrderV2, VersionedOrder};

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

/// `<Bytes>` ++ `message` ++ `</Bytes>` — the blob a Ledger `signRaw` actually sees.
fn wrap(message: &[u8]) -> Vec<u8> {
    [b"<Bytes>".as_slice(), message, b"</Bytes>".as_slice()].concat()
}

/// `<Bytes>` ++ `ORDER_MESSAGE` ++ `</Bytes>`, built from the pinned text rather than
/// from `render_order`, so the two can be compared.
fn wrapped_payload() -> Vec<u8> {
    wrap(ORDER_MESSAGE.as_bytes())
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
            wrap(&msg)
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

        let payload = wrap(&rendered);
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

// ── v2 device vectors ────────────────────────────────────────────────────────
//
// Five v2 messages captured 2026-08-10 from the same Nano S+ (Polkadot Generic
// v100.0.25) and the same derivation path `m/44'/354'/0'/0'/0'` as the v1 vector
// above — `DEVICE_PUBLIC_KEY` is byte-identical, so the two captures are one
// provenance chain rather than two unrelated ones.
//
// These are stronger than the v1 capture in one respect: every message renders
// `signer` as the device's OWN account, so they run through `verify_readable`
// end to end instead of only pinning a rendering. What they still cannot do is
// clear `validate_order` — `chain_id` is 1, `u64::MAX` and 0 against the mock's
// 945, netuid 0 is root, and the shortest case expires at 0 — so full order
// acceptance stays with the executable vector above.
//
// Together with the 255/256/257-byte probes captured in the same session, they
// settle the length rule by measurement rather than inference: those three
// bracket the switch exactly at `> LEDGER_MAX_SIGN_SIZE` (255 and 256 signed over
// the blob, 257 over the digest), and the 341–524-byte range here shows no real
// order can reach the blob branch. 341 is the floor: three SS58 addresses plus
// fixed wording, every numeric field zero.

/// Provider `order_id` named by [`device_v2_take_profit_linked_pct`].
const PROVIDER_A: [u8; 32] = [
    0x9f, 0x2c, 0x7e, 0x1d, 0x4b, 0x6a, 0x03, 0xf5, 0x8e, 0x7d, 0x21, 0xc4, 0xa0, 0x9b, 0x65, 0x38,
    0xef, 0x12, 0x47, 0xac, 0x9d, 0x0b, 0x3e, 0x65, 0x21, 0x74, 0x8f, 0xca, 0x35, 0xd0, 0x9b, 0x6e,
];

/// The device's own account, as rendered in every v2 capture's `signer` field.
const DEVICE_ADDRESS: &str = "5EkanzEuqrGX8vyUNrHfBJutwD634HDph1amvAkiEdUZGJ9K";

fn account(s: &str) -> AccountId {
    AccountId::from_ss58check(s).expect("vector SS58 must decode")
}

fn relayers(
    list: &[&str],
) -> frame_support::BoundedVec<AccountId, frame_support::traits::ConstU32<10>> {
    frame_support::BoundedVec::try_from(list.iter().map(|s| account(s)).collect::<Vec<_>>())
        .expect("at most 10 relayers")
}

/// The fields shared by the captures. Each `device_v2_*` below varies only what
/// its name says it varies, so a diff between two vectors is a diff between two
/// device screens.
fn device_v2_base() -> OrderV2<AccountId> {
    OrderV2 {
        signer: AccountId::new(DEVICE_PUBLIC_KEY),
        hotkey: account("5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN"),
        netuid: NetUid::from(64u16),
        order_type: OrderType::LimitBuy,
        amount: OrderAmount::Fixed(1_000_000_000),
        limit_price: 500_000_000,
        expiry: 1_793_000_000_000,
        fee_rate: Perbill::from_parts(8_500_000),
        fee_recipient: account("5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY"),
        relayer: Some(relayers(&[
            "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty",
        ])),
        max_slippage: Some(Perbill::from_parts(7_500_000)),
        chain_id: 1,
        partial_fills_enabled: true,
        has_linked_order: false,
    }
}

fn device_v2_limit_buy_fixed() -> OrderV2<AccountId> {
    device_v2_base()
}

fn device_v2_take_profit_linked_pct() -> OrderV2<AccountId> {
    OrderV2 {
        order_type: OrderType::TakeProfit,
        amount: OrderAmount::LinkedPercentage {
            provider: H256(PROVIDER_A),
            pct: Perbill::from_parts(250_000_000),
        },
        has_linked_order: true,
        ..device_v2_base()
    }
}

fn device_v2_stop_loss_empty_relayer() -> OrderV2<AccountId> {
    OrderV2 {
        order_type: OrderType::StopLoss,
        relayer: Some(relayers(&[])),
        max_slippage: Some(Perbill::zero()),
        partial_fills_enabled: false,
        has_linked_order: true,
        ..device_v2_base()
    }
}

fn device_v2_two_relayers_saturated() -> OrderV2<AccountId> {
    OrderV2 {
        amount: OrderAmount::Fixed(u64::MAX),
        limit_price: u64::MAX,
        expiry: u64::MAX,
        fee_rate: Perbill::one(),
        relayer: Some(relayers(&[
            "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty",
            "5DAAnrj7VHTznn2AWBemMuyBwZWs6FNFjdyVXUeYum3PTXFy",
        ])),
        max_slippage: Some(Perbill::one()),
        chain_id: u64::MAX,
        has_linked_order: true,
        ..device_v2_base()
    }
}

fn device_v2_shortest_possible() -> OrderV2<AccountId> {
    OrderV2 {
        netuid: NetUid::from(0u16),
        amount: OrderAmount::Fixed(0),
        limit_price: 0,
        expiry: 0,
        fee_rate: Perbill::zero(),
        relayer: None,
        max_slippage: None,
        chain_id: 0,
        partial_fills_enabled: false,
        has_linked_order: false,
        ..device_v2_base()
    }
}

const LIMIT_BUY_FIXED_MESSAGE: &str = "TAO.com order v2: Limit buy 1000000000 on subnet 64, \
limit price 500000000, expiry 1793000000000, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, max slippage 7500000, \
chain 1, partial fills true, signer 5EkanzEuqrGX8vyUNrHfBJutwD634HDph1amvAkiEdUZGJ9K, \
has-linked-order false";

const LIMIT_BUY_FIXED_DIGEST: [u8; 32] = [
    0xb3, 0x7d, 0x9b, 0x6e, 0x7f, 0x33, 0xe1, 0x0d, 0x42, 0x8d, 0xb6, 0x6a, 0xd5, 0x81, 0xb7, 0xf8,
    0x35, 0x58, 0xe0, 0x87, 0xdb, 0xf8, 0xe5, 0x47, 0xaf, 0x8b, 0x5b, 0x71, 0x8a, 0x45, 0xf6, 0x3a,
];

const LIMIT_BUY_FIXED_SIGNATURE: [u8; 64] = [
    0x28, 0xd4, 0x4e, 0xf8, 0x02, 0xff, 0x7e, 0xa7, 0x85, 0xa5, 0x18, 0x30, 0x45, 0xed, 0xac, 0x1b,
    0x35, 0x84, 0x27, 0x4b, 0x53, 0xfe, 0x89, 0xef, 0xc0, 0x0d, 0x14, 0x63, 0x49, 0x06, 0x10, 0x3b,
    0xeb, 0x14, 0xd8, 0xd6, 0xaa, 0x36, 0x85, 0x98, 0x17, 0x03, 0x09, 0xfa, 0xf2, 0xa6, 0x7c, 0x9e,
    0xe6, 0x20, 0x29, 0x37, 0x7c, 0xdd, 0x07, 0x6a, 0xff, 0x82, 0x95, 0xf0, 0x19, 0x96, 0xc1, 0x0a,
];

const TAKE_PROFIT_LINKED_PCT_MESSAGE: &str = "TAO.com order v2: Take-profit 250000000 ppb of \
order 0x9f2c7e1d4b6a03f58e7d21c4a09b6538ef1247ac9d0b3e6521748fca35d09b6e output on subnet 64, \
trigger price 500000000, expiry 1793000000000, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, max slippage 7500000, \
chain 1, partial fills true, signer 5EkanzEuqrGX8vyUNrHfBJutwD634HDph1amvAkiEdUZGJ9K, \
has-linked-order true";

const TAKE_PROFIT_LINKED_PCT_DIGEST: [u8; 32] = [
    0x48, 0x72, 0x61, 0x1a, 0x36, 0x56, 0xed, 0x87, 0xcc, 0x8d, 0x7f, 0x3e, 0x78, 0xb9, 0xf6, 0xc9,
    0xe1, 0xf7, 0x0a, 0x5d, 0x4e, 0xaf, 0x86, 0xa2, 0x0a, 0xe4, 0xa9, 0x16, 0xf2, 0x1f, 0x81, 0x90,
];

const TAKE_PROFIT_LINKED_PCT_SIGNATURE: [u8; 64] = [
    0xb1, 0xb3, 0xdc, 0xa4, 0x34, 0x5b, 0xee, 0x1b, 0x06, 0xf9, 0xd9, 0xd0, 0x40, 0xcd, 0x88, 0xc7,
    0xab, 0x6b, 0x6f, 0x42, 0x7f, 0x15, 0xf4, 0x7b, 0xff, 0xbd, 0xcb, 0x91, 0x7f, 0x32, 0xa5, 0x40,
    0x47, 0xdf, 0xc6, 0xec, 0xaf, 0x4b, 0x6e, 0x6d, 0xe1, 0x19, 0xee, 0xd8, 0x55, 0xb4, 0xb8, 0xa1,
    0x7d, 0x18, 0x83, 0xe8, 0x5a, 0x8e, 0x15, 0xeb, 0x25, 0x60, 0xf7, 0x19, 0x59, 0x8d, 0x05, 0x09,
];

const STOP_LOSS_EMPTY_RELAYER_MESSAGE: &str = "TAO.com order v2: Stop-loss 1000000000 on \
subnet 64, trigger price 500000000, expiry 1793000000000, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer [], max slippage 0, \
chain 1, partial fills false, signer 5EkanzEuqrGX8vyUNrHfBJutwD634HDph1amvAkiEdUZGJ9K, \
has-linked-order true";

const STOP_LOSS_EMPTY_RELAYER_DIGEST: [u8; 32] = [
    0x03, 0x84, 0xac, 0x17, 0x9e, 0xd6, 0xf4, 0xe1, 0x4c, 0xea, 0x65, 0x60, 0x36, 0x8e, 0x0c, 0x72,
    0xc7, 0x05, 0xdb, 0x36, 0x38, 0xdf, 0x81, 0xae, 0x0a, 0x23, 0x50, 0x36, 0xa1, 0xee, 0x54, 0x45,
];

const STOP_LOSS_EMPTY_RELAYER_SIGNATURE: [u8; 64] = [
    0x35, 0xa9, 0x2e, 0x28, 0xf4, 0x01, 0x54, 0xcc, 0xa1, 0xfd, 0x63, 0xe1, 0x4c, 0x1c, 0x1f, 0x52,
    0xdd, 0x0c, 0xa3, 0x68, 0x78, 0x76, 0x5e, 0x39, 0x65, 0x3d, 0xcc, 0x3d, 0xb3, 0xe4, 0x10, 0x0e,
    0x66, 0x64, 0x96, 0x6f, 0x73, 0x8b, 0x8e, 0x63, 0x96, 0x49, 0x35, 0x06, 0x56, 0x75, 0xf1, 0x20,
    0x83, 0xb6, 0x1a, 0x86, 0xdf, 0xa5, 0x5b, 0x31, 0x31, 0xb9, 0x92, 0xbd, 0x3f, 0x5e, 0x55, 0x0d,
];

const TWO_RELAYERS_SATURATED_MESSAGE: &str = "TAO.com order v2: Limit buy 18446744073709551615 \
on subnet 64, limit price 18446744073709551615, expiry 18446744073709551615, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 1000000000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty+\
5DAAnrj7VHTznn2AWBemMuyBwZWs6FNFjdyVXUeYum3PTXFy, max slippage 1000000000, \
chain 18446744073709551615, partial fills true, \
signer 5EkanzEuqrGX8vyUNrHfBJutwD634HDph1amvAkiEdUZGJ9K, has-linked-order true";

const TWO_RELAYERS_SATURATED_DIGEST: [u8; 32] = [
    0x75, 0x57, 0x94, 0xcb, 0x99, 0x34, 0x64, 0x8f, 0x39, 0x39, 0xdd, 0x1a, 0x65, 0xc6, 0x99, 0xdc,
    0xd0, 0x71, 0x05, 0xc5, 0x54, 0x4f, 0x91, 0xf3, 0xfd, 0xc0, 0xdf, 0xb5, 0x78, 0xbd, 0xec, 0x39,
];

const TWO_RELAYERS_SATURATED_SIGNATURE: [u8; 64] = [
    0x1a, 0x9d, 0x59, 0xea, 0xf0, 0xf5, 0xcd, 0x45, 0xdc, 0xab, 0x8c, 0xbe, 0x81, 0xd6, 0xf1, 0x36,
    0x79, 0x30, 0x88, 0xf0, 0xff, 0x7f, 0x9c, 0x3e, 0x8e, 0xa4, 0x39, 0xf0, 0xaf, 0x2c, 0xe4, 0x4a,
    0xbf, 0x04, 0xc0, 0x6f, 0x87, 0x2c, 0x45, 0x92, 0xa6, 0x15, 0xb9, 0x98, 0x52, 0x63, 0x18, 0x79,
    0xfd, 0xe9, 0x93, 0x29, 0x96, 0xf8, 0x32, 0x0d, 0x17, 0xe8, 0x26, 0x1a, 0x4d, 0x7d, 0xa2, 0x05,
];

const SHORTEST_POSSIBLE_MESSAGE: &str = "TAO.com order v2: Limit buy 0 on subnet 0, \
limit price 0, expiry 0, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 0 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer none, max slippage none, \
chain 0, partial fills false, signer 5EkanzEuqrGX8vyUNrHfBJutwD634HDph1amvAkiEdUZGJ9K, \
has-linked-order false";

const SHORTEST_POSSIBLE_DIGEST: [u8; 32] = [
    0x73, 0x26, 0xe6, 0xc0, 0xba, 0x50, 0x8f, 0x55, 0x23, 0x07, 0x35, 0x4a, 0x82, 0x32, 0x5b, 0xd7,
    0x95, 0xf3, 0x62, 0x23, 0xb8, 0x8f, 0x34, 0xb8, 0xcc, 0xe4, 0x43, 0xcb, 0x61, 0xd8, 0xf6, 0x5d,
];

const SHORTEST_POSSIBLE_SIGNATURE: [u8; 64] = [
    0x5c, 0xcb, 0xec, 0x9c, 0x7c, 0x9c, 0xaa, 0x4c, 0x39, 0x9b, 0xc8, 0x0a, 0x57, 0x96, 0x4f, 0xed,
    0xc1, 0x16, 0x2e, 0x68, 0xe9, 0xe9, 0x56, 0x77, 0xc1, 0xad, 0x4d, 0x9f, 0x3f, 0x2d, 0x26, 0x8c,
    0xf4, 0x9d, 0xeb, 0xbe, 0x5a, 0xd9, 0xd3, 0x86, 0x5a, 0x26, 0x18, 0xac, 0x36, 0xb7, 0xf4, 0x28,
    0x97, 0x90, 0x6c, 0xce, 0x7e, 0xa3, 0x32, 0xdd, 0xa3, 0xc8, 0x34, 0x38, 0x2d, 0x5f, 0x54, 0x0a,
];

/// One captured device signature and everything needed to check it: the order it
/// covers, the text the device displayed, and the digest it actually signed.
struct DeviceV2Vector {
    name: &'static str,
    order: fn() -> OrderV2<AccountId>,
    message: &'static str,
    payload_len: usize,
    digest: [u8; 32],
    signature: [u8; 64],
}

fn device_v2_vectors() -> [DeviceV2Vector; 5] {
    [
        DeviceV2Vector {
            name: "device-v2-limit-buy-fixed",
            order: device_v2_limit_buy_fixed,
            message: LIMIT_BUY_FIXED_MESSAGE,
            payload_len: 423,
            digest: LIMIT_BUY_FIXED_DIGEST,
            signature: LIMIT_BUY_FIXED_SIGNATURE,
        },
        DeviceV2Vector {
            name: "device-v2-take-profit-linked-pct",
            order: device_v2_take_profit_linked_pct,
            message: TAKE_PROFIT_LINKED_PCT_MESSAGE,
            payload_len: 512,
            digest: TAKE_PROFIT_LINKED_PCT_DIGEST,
            signature: TAKE_PROFIT_LINKED_PCT_SIGNATURE,
        },
        DeviceV2Vector {
            name: "device-v2-stop-loss-empty-relayer",
            order: device_v2_stop_loss_empty_relayer,
            message: STOP_LOSS_EMPTY_RELAYER_MESSAGE,
            payload_len: 373,
            digest: STOP_LOSS_EMPTY_RELAYER_DIGEST,
            signature: STOP_LOSS_EMPTY_RELAYER_SIGNATURE,
        },
        DeviceV2Vector {
            name: "device-v2-two-relayers-saturated",
            order: device_v2_two_relayers_saturated,
            message: TWO_RELAYERS_SATURATED_MESSAGE,
            payload_len: 524,
            digest: TWO_RELAYERS_SATURATED_DIGEST,
            signature: TWO_RELAYERS_SATURATED_SIGNATURE,
        },
        DeviceV2Vector {
            name: "device-v2-shortest-possible",
            order: device_v2_shortest_possible,
            message: SHORTEST_POSSIBLE_MESSAGE,
            payload_len: 341,
            digest: SHORTEST_POSSIBLE_DIGEST,
            signature: SHORTEST_POSSIBLE_SIGNATURE,
        },
    ]
}

fn device_v2_signed(vector: &DeviceV2Vector) -> crate::SignedOrder<AccountId> {
    crate::SignedOrder {
        order: VersionedOrder::V2((vector.order)()),
        signature: MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(vector.signature)),
        partial_fill: None,
    }
}

/// `render_order` must reproduce every message the device displayed. A drift here
/// makes every v2 signature a user has already captured unverifiable.
#[test]
fn device_v2_render_order_matches_each_captured_message() {
    new_test_ext().execute_with(|| {
        assert_eq!(
            <<Test as frame_system::Config>::SS58Prefix as Get<u16>>::get(),
            42,
            "the vectors were captured at SS58 prefix 42"
        );
        assert_eq!(
            LimitOrders::<Test>::render_account(&AccountId::new(DEVICE_PUBLIC_KEY)),
            DEVICE_ADDRESS,
            "the v2 captures were signed by the same device account as the v1 one"
        );

        for vector in device_v2_vectors() {
            let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V2((vector.order)()));
            assert_eq!(
                String::from_utf8(rendered.clone()).unwrap(),
                vector.message,
                "{}: render_order drifted from the message the device displayed",
                vector.name
            );
            for (i, b) in rendered.iter().enumerate() {
                assert!(
                    (0x20..=0x7e).contains(b),
                    "{}: byte {i} = {b:#x} is not printable ASCII, so the device would \
                     render it as hex instead of text",
                    vector.name
                );
            }
        }
    });
}

/// Every real order is over the device's limit, so the blob branch of
/// `verify_readable` is unreachable for orders — 341 bytes is the floor.
#[test]
fn device_v2_payloads_are_all_oversized_and_hash_to_the_captured_digests() {
    new_test_ext().execute_with(|| {
        for vector in device_v2_vectors() {
            let rendered = LimitOrders::<Test>::render_order(&VersionedOrder::V2((vector.order)()));
            let payload = wrap(&rendered);
            assert_eq!(
                payload.len(),
                vector.payload_len,
                "{}: wrapped payload length drifted",
                vector.name
            );
            assert!(
                payload.len() > LEDGER_MAX_SIGN_SIZE,
                "{}: {} bytes must exceed the {LEDGER_MAX_SIGN_SIZE}-byte device limit — \
                 no order may reach the blob branch",
                vector.name,
                payload.len()
            );
            assert_eq!(
                sp_core::hashing::blake2_256(&payload),
                vector.digest,
                "{}: wrapped payload does not hash to the digest the device signed",
                vector.name
            );
        }
    });
}

/// Each captured v2 signature verifies over `blake2_256(payload)` and over
/// nothing else.
#[test]
fn device_v2_signatures_are_over_the_blake2_256_digest_only() {
    new_test_ext().execute_with(|| {
        let signer = AccountId::new(DEVICE_PUBLIC_KEY);

        for vector in device_v2_vectors() {
            let signature =
                MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(vector.signature));
            let payload = wrap(vector.message.as_bytes());

            assert!(
                signature.verify(&vector.digest[..], &signer),
                "{}: device signature must verify over blake2_256(wrapped payload)",
                vector.name
            );
            for (form, bytes) in [
                ("the raw wrapped payload", payload.clone()),
                ("the unwrapped message", vector.message.as_bytes().to_vec()),
                (
                    "blake2_256 of the unwrapped message",
                    sp_core::hashing::blake2_256(vector.message.as_bytes()).to_vec(),
                ),
            ] {
                assert!(
                    !signature.verify(bytes.as_slice(), &signer),
                    "{}: device signature must NOT verify over {form}",
                    vector.name
                );
            }
        }
    });
}

/// These messages name the signing device as `signer`, so the recorded
/// signatures go through `verify_readable` itself.
#[test]
fn device_v2_signatures_are_accepted_by_verify_readable() {
    new_test_ext().execute_with(|| {
        for vector in device_v2_vectors() {
            assert!(
                LimitOrders::<Test>::verify_readable(&device_v2_signed(&vector)),
                "{}: a real Ledger signature over a v2 order must pass verify_readable",
                vector.name
            );
        }
    });
}

/// Transplanting any signature onto any other captured order must fail.
#[test]
fn device_v2_signatures_do_not_transplant_between_orders() {
    new_test_ext().execute_with(|| {
        let vectors = device_v2_vectors();

        for (i, source) in vectors.iter().enumerate() {
            for (j, target) in vectors.iter().enumerate() {
                if i == j {
                    continue;
                }
                let transplanted = crate::SignedOrder {
                    order: VersionedOrder::V2((target.order)()),
                    signature: MultiSignature::Ed25519(sp_core::ed25519::Signature::from_raw(
                        source.signature,
                    )),
                    partial_fill: None,
                };
                assert!(
                    !LimitOrders::<Test>::verify_readable(&transplanted),
                    "{}'s signature must not verify against {}",
                    source.name,
                    target.name
                );
            }
        }
    });
}

// ── The `0X`-prefix client defect ────────────────────────────────────────────
//
// A client rendering the provider id from a string strips the `0x` prefix with a
// case-sensitive match, then lowercases what is left. Fed `0XFF…FF`, the prefix
// survives the strip, gets lowercased, and the amount renders with a DOUBLE
// prefix: `… ppb of order 0x0xffff… output`.
//
// The pallet cannot produce that. `provider` is an `H256`, so there is no string
// to strip: the `0x` is a literal in `OrderAmount::render`'s format string and
// `HexDisplay` emits 64 bare lowercase hex characters after it.

/// What a client with the case-sensitive strip emits. A defect report, not a target.
const DEFECT_CLIENT_MESSAGE: &str = "TAO.com order v2: Limit buy 1000000000 ppb of \
order 0x0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff output on subnet 64, \
limit price 500000000, expiry 1793000000000, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, max slippage 7500000, \
chain 1, partial fills true, signer 5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC, \
has-linked-order true";

/// What the pallet renders for the very same order — one prefix.
const DEFECT_CORRECTED_MESSAGE: &str = "TAO.com order v2: Limit buy 1000000000 ppb of \
order 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff output on subnet 64, \
limit price 500000000, expiry 1793000000000, \
hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, \
fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, \
relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, max slippage 7500000, \
chain 1, partial fills true, signer 5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC, \
has-linked-order true";

fn defect_order() -> OrderV2<AccountId> {
    OrderV2 {
        signer: account("5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC"),
        amount: OrderAmount::LinkedPercentage {
            provider: H256::repeat_byte(0xFF),
            pct: Perbill::one(),
        },
        has_linked_order: true,
        ..device_v2_base()
    }
}

/// The pallet renders one prefix and cannot be made to render two.
#[test]
fn defect_rendering_is_unreachable_from_an_h256_provider() {
    new_test_ext().execute_with(|| {
        let rendered = String::from_utf8(LimitOrders::<Test>::render_order(&VersionedOrder::V2(
            defect_order(),
        )))
        .unwrap();

        assert_eq!(
            rendered, DEFECT_CORRECTED_MESSAGE,
            "an H256 provider must render as one `0x` followed by 64 lowercase hex"
        );
        assert_ne!(
            rendered, DEFECT_CLIENT_MESSAGE,
            "render_order must never emit the doubled-prefix form"
        );
        assert_eq!(
            DEFECT_CLIENT_MESSAGE.replace("of order 0x0x", "of order 0x"),
            DEFECT_CORRECTED_MESSAGE,
            "the defect is a doubled prefix and nothing else"
        );
        assert_eq!(
            rendered.matches("of order 0x").count(),
            1,
            "exactly one provider prefix"
        );
        assert!(
            !rendered.contains("0x0x"),
            "no doubled prefix anywhere in the message"
        );
    });
}

/// A signature the right key produced over the buggy rendering is rejected,
/// while the same key over the pallet's rendering of the same order is accepted.
#[test]
fn defect_rendering_is_rejected_by_verify_readable() {
    new_test_ext().execute_with(|| {
        let order = OrderV2 {
            signer: AccountId::new(SOFTWARE_PUBLIC_KEY),
            ..defect_order()
        };
        let versioned = VersionedOrder::V2(order);
        let pair = sp_core::ed25519::Pair::from_seed(&SOFTWARE_SEED);

        let honest = String::from_utf8(LimitOrders::<Test>::render_order(&versioned)).unwrap();
        let defective = honest.replace("of order 0x", "of order 0x0x");
        assert_ne!(honest, defective, "the two renderings must differ");

        let sign = |message: &str| {
            let payload = wrap(message.as_bytes());
            assert!(payload.len() > LEDGER_MAX_SIGN_SIZE);
            MultiSignature::Ed25519(pair.sign(&sp_core::hashing::blake2_256(&payload)))
        };

        let signed_over_defect = crate::SignedOrder {
            order: versioned.clone(),
            signature: sign(&defective),
            partial_fill: None,
        };
        assert!(
            !LimitOrders::<Test>::verify_readable(&signed_over_defect),
            "a signature over the doubled-prefix rendering must be rejected"
        );

        let signed_over_honest = crate::SignedOrder {
            order: versioned,
            signature: sign(&honest),
            partial_fill: None,
        };
        assert!(
            LimitOrders::<Test>::verify_readable(&signed_over_honest),
            "the same key over the pallet's own rendering must be accepted — the \
             doubled prefix is the only reason the other one failed"
        );
    });
}
