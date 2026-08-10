import { describeSuite, expect } from "@moonwall/cli";
import { Keyring } from "@polkadot/keyring";
import { hexToU8a, stringToU8a, u8aToHex, u8aWrapBytes } from "@polkadot/util";
import { blake2AsU8a, ed25519PairFromSeed, ed25519Verify } from "@polkadot/util-crypto";
import {
    type Order,
    LEDGER_MAX_SIGN_SIZE,
    buildReadableSignedOrder,
    formatOrderMessage,
} from "../../../../utils/limit-orders.js";

// Hardware test vector for the human-readable ("clear-signing") signing form.
//
// A Ledger blake2_256-hashes a raw-signing (`signRaw`) payload longer than
// MAX_SIGN_SIZE = 256 bytes before signing it (`crypto_sign_ed25519` in
// `app/src/crypto.c` of the Zondax Polkadot app). The readable message is always
// over that limit, so a real device signature commits to
// `blake2_256(<Bytes> ++ message ++ </Bytes>)` — never to the payload bytes. This
// is NOT the symmetric rule in Substrate's `SignedPayload`/`GenericExtrinsicPayload`
// (that pair only governs *extrinsic* signing payloads, where signer and verifier
// both apply it). On the raw-message path only the device hashes: polkadot-js's
// `pair.sign()` applies a length rule for ecdsa only. Hence any verifier of an
// oversized Ledger-signed order must hash first, and the utils' readable signer
// mirrors that.
//
// Captured 2026-07-28 from a Nano S+ running Polkadot Generic v100.0.25, derivation
// path m/44'/354'/0'/0'/0'. The device rendered the whole order text across its
// screens and still signed the digest, so digest signing is NOT a blind-signing
// fallback — clear-signing works, and a shorter message would only cut page count.
// The probe matrix that ruled out the alternatives (unwrapped message, blake2_512,
// digest-as-hex) is re-run in T05.
//
// The vector is pinned as literal data on purpose: a vector must outlive the
// harness that produced it. The Rust half lives in
// `pallets/limit-orders/src/tests/ledger_vector.rs` and pins the same bytes against
// the pallet's own `render_order`.
//
// NOTE: the capture's device key is NOT the account named in the message's `signer`
// field, and the runtime verifies against `order.signer` — so this suite pins the
// rule and the renderer, not order execution. Order acceptance is covered by
// `test-execute-orders-readable.ts`. An executable vector needs a fresh capture
// whose message renders `signer` as the device's own address, with `chain_id`
// matching this environment and `expiry` in milliseconds (this one's 1793000000 is
// a seconds-scale value, i.e. long expired).

/** The exact text the device displayed and signed, SS58 prefix 42. */
const ORDER_MESSAGE =
    "TAO.com order v1: Limit buy 1000000000 on subnet 64, limit price 500000000, " +
    "expiry 1793000000, hotkey 5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN, " +
    "fee 8500000 to 5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY, " +
    "relayer 5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty, max slippage 7500000, " +
    "chain 1, partial fills true, signer 5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC";

const ORDER_MESSAGE_BYTE_LENGTH = 381;

/** `<Bytes>`-wrapped byte length — what actually reaches the device. */
const WRAPPED_BYTE_LENGTH = 396;

/** Exact bytes sent to the device's raw-sign instruction. */
// prettier-ignore
const WRAPPED_PAYLOAD_HEX =
    "0x3c42797465733e54414f2e636f6d206f726465722076313a204c696d6974206275792031303030303030303030206f6e207375626e65742036342c206c696d6974207072696365203530303030303030302c2065787069727920313739333030303030302c20686f746b65792035484b3574703674325335394479776d4852575042564a654a38365436314b6a75725971656f6f716a3873524570654e2c20666565203835303030303020746f2035474e4a715450794e71414e426b55564d4e314c50507278586e466f7557586f6532774e536d6d456f4c637478695a592c2072656c61796572203546486e655734367847586773356d5569766555347362547947427a6d73745573705a43393255686a4a4d36393474792c206d617820736c69707061676520373530303030302c20636861696e20312c207061727469616c2066696c6c7320747275652c207369676e657220354344395566467633464c643942525038744b3742756d70455976753279334b5a4d7568556e444168757a50626474433c2f42797465733e";

/** blake2_256 of the wrapped payload — the 32 bytes the device signs. */
const WRAPPED_PAYLOAD_BLAKE2_256 = "0x3c3ea88b51457189388906eecb582d5ebf481b1af5b66b6b5771e4e84b6e5ed7";

/**
 * Software half of the vector: reproducible in CI, no device needed. Holds a
 * signature over EACH form so both semantics have a fixture.
 */
const SOFTWARE_VECTOR = {
    seedHex: "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
    publicKeyHex: "0x79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
    /** ed25519(wrapped payload) — the shape a software `signRaw` emits. */
    signatureOverBlobHex:
        "0xc8d12ffcdc504a956b97fd67009de28c6541bf79dc33903092d9f1c279718c97" +
        "91cf5bc69a3889c6699a5aab18170cdc2366f81daea5ecd31c64108385b1b60d",
    /** ed25519(blake2_256(wrapped payload)) — the shape the device emits. */
    signatureOverHashHex:
        "0x81edd47a3e02b7f52cc6a7db02e9a8c023c1f101526a7d5fe4be118aff36090c" +
        "cf65f75136b31f6f64d8bcecc4e441b32322c37b4af51436a1e99096202b8608",
} as const;

/** Signature captured from real hardware. */
const DEVICE_VECTOR = {
    label: "Nano S+ · Polkadot Generic v100.0.25",
    derivationPath: "m/44'/354'/0'/0'/0'",
    publicKeyHex: "0x76e2815d89ea8f87a7fc62c21b3ee2fb81d78ca28a24d33a974f47b20bb70a63",
    signatureHex:
        "0x91a37e50d01eeb407d9d1902374fef24dc287c1edb81474dbe19e46157bcc23d" +
        "c5b7ba723ae7f8dd192004c150a8d0479a0ccb522c930dc8fcda7a15d6b45b08",
    /** What it signed over — recorded, not assumed, so a future app version is a new entry. */
    signedOver: "blake2_256",
} as const;

/** The order whose canonical rendering is `ORDER_MESSAGE`. */
const VECTOR_ORDER: Order = {
    signer: "5CD9UfFv3FLd9BRP8tK7BumpEYvu2y3KZMuhUnDAhuzPbdtC",
    hotkey: "5HK5tp6t2S59DywmHRWPBVJeJ86T61KjurYqeooqj8sREpeN",
    netuid: 64,
    order_type: "LimitBuy",
    amount: 1_000_000_000n,
    limit_price: 500_000_000n,
    expiry: 1_793_000_000n,
    fee_rate: 8_500_000,
    fee_recipient: "5GNJqTPyNqANBkUVMN1LPPrxXnFouWXoe2wNSmmEoLctxiZY",
    relayer: ["5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty"],
    max_slippage: 7_500_000,
    chain_id: 1n,
    partial_fills_enabled: true,
};

// ── Executable vector ────────────────────────────────────────────────────────
//
// The hardware capture cannot be submitted as an order: its message names
// 5CD9UfFv… as the signer while the device holds 5Ekanz…, and the runtime verifies
// against `order.signer`. This vector closes that gap without a device — it is
// minted from SOFTWARE_VECTOR.seedHex (which we hold) over the digest, and since
// ed25519 is deterministic and the device's transformation is fixed, these are
// exactly the bytes a Ledger holding that seed would return. It does NOT attest to
// device behaviour; the capture above does that.
//
// chain 945 matches the pallet mock, because the same constants back the Rust half
// in `pallets/limit-orders/src/tests/ledger_vector.rs`. Nothing here touches chain
// state: this asserts that our production signing helper emits the device shape.

/** SS58 of the account SOFTWARE_VECTOR.seedHex controls — the `signer` below. */
const SOFTWARE_ADDRESS = "5EpHX5foDtnhZngj4GsKq5eKGpUvuMqbpUG48ZfCCCs7EzKR";

/** Well-known dev accounts, prefix 42: Bob (hotkey) and Charlie (fee recipient). */
const BOB_SS58 = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";
const CHARLIE_SS58 = "5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y";

const EXECUTABLE_MESSAGE =
    "TAO.com order v1: Limit buy 1000 on subnet 1, limit price 1000000000, " +
    `expiry 18446744073709551615, hotkey ${BOB_SS58}, fee 0 to ${CHARLIE_SS58}, ` +
    `relayer none, max slippage none, chain 945, partial fills false, signer ${SOFTWARE_ADDRESS}`;

/** blake2_256 of the wrapped EXECUTABLE_MESSAGE (350 bytes wrapped, so hashed). */
const EXECUTABLE_DIGEST = "0xcd8f76e889c586d5efb73dd03433dc164b75fd727c52aaa4c8d07eb13dc98c12";

/** ed25519(seed, EXECUTABLE_DIGEST) — accepted by the runtime's `verify_readable`. */
const EXECUTABLE_SIGNATURE =
    "0xca9e4c33695072ffef1e3e1d0715979e3a4d8b553ee1ecc29e5da9ead5788910" +
    "4d4bc7760c4f9a867e82b046271e47b554609489d666168b549d75b39b559b04";

// `new Uint8Array(...)` is load-bearing: `@polkadot/util`'s `isU8a` tests
// `constructor === Uint8Array` by identity, so an array from another realm makes
// `u8aWrapBytes` stringify its input instead of wrapping it — silently producing the
// wrong bytes. Re-wrapping puts it in this realm.
const bytes = (text: string) => new Uint8Array(stringToU8a(text));
const wrapped = () => u8aWrapBytes(bytes(ORDER_MESSAGE));
const digest = () => blake2AsU8a(wrapped(), 256);

describeSuite({
    id: "DEV_SUB_LIMIT_ORDERS_LEDGER_VECTOR",
    title: "limit-orders — Ledger raw-sign vector for the oversized clear-signing payload",
    foundationMethods: "dev",
    testCases: ({ it }) => {
        it({
            id: "T01",
            title: "the TS formatter reproduces the message the device displayed and signed",
            test: () => {
                const msg = formatOrderMessage(VECTOR_ORDER);
                expect(msg).toBe(ORDER_MESSAGE);
                expect(bytes(msg)).toHaveLength(ORDER_MESSAGE_BYTE_LENGTH);
                for (let i = 0; i < msg.length; i++) {
                    const code = msg.charCodeAt(i);
                    expect(
                        code >= 0x20 && code <= 0x7e,
                        `char ${i} = 0x${code.toString(16)} is not printable ASCII, so the device would render hex`
                    ).toBe(true);
                }
            },
        });

        it({
            id: "T02",
            title: "the wrapped payload is over the threshold that switches the device to digest signing",
            test: () => {
                expect(wrapped()).toHaveLength(WRAPPED_BYTE_LENGTH);
                expect(WRAPPED_BYTE_LENGTH).toBeGreaterThan(LEDGER_MAX_SIGN_SIZE);
                expect(u8aToHex(wrapped())).toBe(WRAPPED_PAYLOAD_HEX);
                // The utils wrap with u8aWrapBytes; the runtime concatenates literal
                // tags. For printable ASCII both must produce identical bytes, or the
                // device would render one thing and the chain verify another.
                expect(u8aToHex(wrapped())).toBe(u8aToHex(bytes(`<Bytes>${ORDER_MESSAGE}</Bytes>`)));
            },
        });

        it({
            id: "T03",
            title: "the wrapped payload hashes to the pinned digest",
            test: () => {
                expect(u8aToHex(digest())).toBe(WRAPPED_PAYLOAD_BLAKE2_256);
            },
        });

        it({
            id: "T04",
            title: "the two signing forms are mutually unverifiable",
            test: () => {
                const publicKey = ed25519PairFromSeed(hexToU8a(SOFTWARE_VECTOR.seedHex)).publicKey;
                expect(u8aToHex(publicKey)).toBe(SOFTWARE_VECTOR.publicKeyHex);

                const overBlob = hexToU8a(SOFTWARE_VECTOR.signatureOverBlobHex);
                const overHash = hexToU8a(SOFTWARE_VECTOR.signatureOverHashHex);

                expect(ed25519Verify(wrapped(), overBlob, publicKey)).toBe(true);
                expect(ed25519Verify(digest(), overHash, publicKey)).toBe(true);
                // The whole hazard in two assertions: signer and verifier disagreeing
                // about which form is in play is a hard rejection, never a soft fallback.
                expect(ed25519Verify(digest(), overBlob, publicKey)).toBe(false);
                expect(ed25519Verify(wrapped(), overHash, publicKey)).toBe(false);
            },
        });

        it({
            id: "T05",
            title: `${DEVICE_VECTOR.label} signed over ${DEVICE_VECTOR.signedOver} and nothing else`,
            test: () => {
                const publicKey = hexToU8a(DEVICE_VECTOR.publicKeyHex);
                const signature = hexToU8a(DEVICE_VECTOR.signatureHex);

                expect(ed25519Verify(digest(), signature, publicKey)).toBe(true);

                // Every alternative the capture's probe matrix ruled out.
                const rejected: [string, Uint8Array][] = [
                    ["the raw wrapped payload", wrapped()],
                    ["the unwrapped message", bytes(ORDER_MESSAGE)],
                    ["blake2_256 of the unwrapped message", blake2AsU8a(bytes(ORDER_MESSAGE), 256)],
                    ["blake2_512 of the wrapped payload", blake2AsU8a(wrapped(), 512)],
                    ["the ASCII hex of the digest", bytes(u8aToHex(digest()).slice(2))],
                ];
                for (const [form, message] of rejected) {
                    expect(ed25519Verify(message, signature, publicKey), `must not verify over ${form}`).toBe(false);
                }
            },
        });

        it({
            id: "T06",
            title: "buildReadableSignedOrder emits the device shape for the executable vector",
            test: () => {
                const signer = new Keyring({ type: "ed25519" }).addFromSeed(hexToU8a(SOFTWARE_VECTOR.seedHex));
                expect(signer.address).toBe(SOFTWARE_ADDRESS);

                // `api` is unused by the readable builder (the payload is rendered from
                // the params, not from chain metadata), so no chain state is needed.
                const signed = buildReadableSignedOrder(null, {
                    signer,
                    hotkey: BOB_SS58,
                    netuid: 1,
                    orderType: "LimitBuy",
                    amount: 1_000n,
                    limitPrice: 1_000_000_000n,
                    expiry: 18_446_744_073_709_551_615n,
                    feeRate: 0,
                    feeRecipient: CHARLIE_SS58,
                    chainId: 945n,
                });

                // The message the order renders to, the digest it hashes to, and the
                // signature the helper produced must all match the frozen vector —
                // i.e. our signing path is byte-for-byte the one a Ledger takes.
                expect(formatOrderMessage(signed.order.V1)).toBe(EXECUTABLE_MESSAGE);
                const payload = u8aWrapBytes(bytes(EXECUTABLE_MESSAGE));
                expect(payload.length).toBeGreaterThan(LEDGER_MAX_SIGN_SIZE);
                expect(u8aToHex(blake2AsU8a(payload, 256))).toBe(EXECUTABLE_DIGEST);
                expect("Ed25519" in signed.signature).toBe(true);
                expect((signed.signature as { Ed25519: string }).Ed25519).toBe(EXECUTABLE_SIGNATURE);
            },
        });
    },
});
