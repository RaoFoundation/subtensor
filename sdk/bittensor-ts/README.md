# `@bittensor/sdk`

`bittensor-ts` is the monorepo's TypeScript SDK. It lives in its own
`sdk/bittensor-ts` package. In Node.js it is a deliberately thin Node-API wrapper around the
sibling `bittensor-core` Rust crate. Browser applications use the explicit
`@bittensor/sdk/browser` entrypoint backed by `sdk/bittensor-core-wasm`.

Chain-defined work runs in Rust:

- sr25519/ed25519 keys, signatures, SS58, and wallet keyfiles;
- SCALE encoding and decoding, runtime metadata, storage keys, calls, and
  signed extrinsics;
- RFC-0078 metadata digests and Ledger proofs;
- drand timelock encryption and epoch-schedule simulation;
- ML-KEM/XChaCha20 MEV-shield envelopes; and
- Ledger HID signing.

The Node TypeScript layer is limited to JavaScript-friendly names and defaults,
lossless `Buffer`/`bigint`/`Map` boundary conversion, error classes,
signing-compatibility adapters for the signer objects expected by Polkadot.js,
Polkadot API, and Moonwall, wallet filesystem management, generated-style
descriptors, and extension-signer interop. On Node, the default `Client` uses
the native Rust client as the authoritative backend for connection, runtime
refresh and caching, storage reads, runtime calls, call composition, nonce
reads, fee estimation, signing plans, submission, inclusion/finalization
receipts, dispatch-error interpretation, and high-level transaction semantics.
The independent TypeScript JSON-RPC transport is exposed separately as
`BrowserChainClient` for browser/custom-transport use cases where the native
client cannot run. High-level TypeScript transaction helpers construct Rust
`IntentCall` values, and arbitrary pallet/function calls are classified as raw
by Rust policy before signing.

The package exposes both CommonJS and ESM entrypoints. It also exports the
raw generated Node-API module as `@bittensor/sdk/native`, so every native
entry point is callable even when an ergonomic wrapper has not yet been
added.

This package is currently monorepo-internal and intentionally remains marked
`"private": true`. A public npm release needs a cross-platform native binary
layout and CI matrix for Linux, macOS, and Windows before that flag is removed.

Browser bundlers should import the explicit `@bittensor/sdk/browser` subpath.
That entrypoint is a portable browser subset, not a method-for-method mirror of
the Node API. It does not load `native.cjs`, `.node` binaries, Node `Buffer`,
or native HID. It returns `Uint8Array` values and exposes browser-safe Rust
WASM operations: key generation, SS58, signing and verification, SCALE
encoding and decoding, runtime metadata parsing, storage keys, call and
extrinsic composition, RFC-0078 metadata proofs, ML-KEM sealing, and timelock
encryption/decryption when the caller fetches the drand signature. Host-only
features such as wallet keyfiles, encrypted JSON import, native Ledger HID,
direct drand fetching, and lower-level Node-native runtime introspection
helpers remain Node-only.

## Build locally

From the repository root:

```sh
cargo test -p bittensor-ts-native --all-features
npm --prefix sdk/bittensor-ts ci
npm --prefix sdk/bittensor-ts run check
```

The native crate is isolated under `sdk/bittensor-ts/native`; it links
`sdk/bittensor-core` directly and contains binding glue only. No chain
algorithm is reimplemented in TypeScript.

Node.js 22 or newer is required for `BrowserChainClient`'s default WSS path
because that transport uses the unflagged global `WebSocket`. Older Node
runtimes can still use HTTP endpoints with `BrowserChainClient` or pass
`webSocketFactory`/`webSocketConstructor` explicitly. The default Node `Client`
delegates transport to Rust.
Browser builds also require `wasm-pack` so `npm run build` can emit the
`dist/wasm/bittensor_core_wasm.js` bundle used by `@bittensor/sdk/browser`.

## Example

```ts
import {
  Keypair,
  Runtime,
  createKeyringPairFromUri,
  sealMevShieldTransaction,
} from '@bittensor/sdk'

const alice = Keypair.fromUri('//Alice')
const signature = alice.sign(Buffer.from('hello'))
console.log(alice.verify(Buffer.from('hello'), signature))

// Compatible with tx.signAsync(...) and Moonwall helpers, while the secret
// key and signing operation remain in Rust. This is signing compatibility,
// not full Polkadot.js KeyringPair keystore compatibility: PKCS#8/JSON export,
// lock/unlock, and VRF methods intentionally throw.
const signer = createKeyringPairFromUri('//Alice')

const runtime = new Runtime(metadataBytes, specVersion, transactionVersion)
const call = runtime.composeCall('System', 'remark', {
  remark: Buffer.from('hello'),
})

const ciphertext = sealMevShieldTransaction(mlKemPublicKey, call)
```

Large SCALE integers are returned as `bigint`; byte carriers are returned as
`Buffer`. A decoded SCALE dictionary with non-string keys is returned as a
`Map`, so no key information is lost.

## Chain client example

```ts
import { Balance, Client, Keypair, storage } from '@bittensor/sdk'

const client = await new Client('finney').connect()
const alice = Keypair.fromUri('//Alice')

const balance = await client.balances.get(alice.ss58Address)
const subnet = await client.subnets.info(1)
const metagraph = await client.subnets.metagraph(1)
const events = await client.query(storage.System.Events)

await client.transfer(alice, '5F...', Balance.fromTao('0.01'), {
  waitForFinalization: true,
})
await client.close()
```

`Client` is the Node-native high-level client. Raw JSON-RPC calls,
subscriptions, injected WebSockets, and endpoint fallback belong to the explicit
`BrowserChainClient` transport client:

```ts
import { BrowserChainClient } from '@bittensor/sdk'

const client = new BrowserChainClient('local', {
  endpoint: 'wss://primary.example',
  fallbackEndpoints: ['wss://fallback.example'],
  expectedGenesisHash: '0x...',
})
```

Fallback endpoints are validated against a trusted genesis hash before use.
Known mainnet aliases (`finney`, `archive`) use the checked-in mainnet genesis
hash. Custom endpoint sets, and named networks without a built-in trust anchor,
must pass `expectedGenesisHash` when `fallbackEndpoints` are configured.

Transaction amount inputs are intentionally explicit. Pass `Balance.fromTao("1.25")`
or `taoAmount("1.25")` for TAO-denominated values, and pass `123n` or
`raoAmount("123")` for rao. Raw `number` and `string` amounts are rejected by
transaction builders to avoid confusing `"1"` rao with `"1.0"` TAO. Decimal
TAO/alpha amounts with more than nine fractional digits are rejected.

`client.submit()` delegates automatic nonce selection to the Rust client when
submitting with a native `Keypair`. Low-level manual signing APIs such as
`signExtrinsic()` require an explicit `nonce`, and detached flows using
`submitSigned()` delegate encoded submission to Rust. `watchSigned()` is only
available on `BrowserChainClient`, where the TypeScript WebSocket transport owns
the subscription.

High-level transaction helpers such as `transfer()`, `staking.addStake()`,
`setWeights()`, registration, and serving route through Rust trusted
constructors on `IntentCall`. The same constructors are exported directly for
advanced callers:

```ts
import { Client, IntentCall, Policy } from '@bittensor/sdk'

const client = await new Client('finney').connect()
await client.submit(
  IntentCall.addStake(hotkey, 1, 1_000_000_000n),
  coldkeySigner,
  { policy: new Policy({ maxSpendRao: 1_000_000_000n, allowedNetuids: [1] }) },
)
```

Raw pallet/function calls are an explicit escape hatch. They are treated by
Rust as unbounded spend with unknown/all-subnet scope, so they require
`allowRawCall: true` or a `Policy` with `allowRawCalls: true`; spend and subnet
caps still fail closed for raw calls. Opaque pre-composed call bytes are more
restricted because the SDK cannot prove their spend or subnet scope.

`client.assertDescriptorSchema()` checks the exported storage, call, constant,
and runtime API descriptor tables against the chain metadata loaded for a block.
Run it in CI or application startup when relying on the convenience descriptor
exports.

Wallet private keyfiles are encrypted when `keyfilePassword` is supplied.
Plaintext private keyfile writes require `allowPlaintext: true`; `createNewColdkey()`,
`createNewHotkey()`, `setColdkey()`, and `setHotkey()` require one of those
choices instead of accepting empty options. Use `Wallet.generateColdkey()` or
`Wallet.generateHotkey()` when you need to generate key material and decide how
to persist it later. Private/public wallet keyfile pairs are written through one
native pair-write operation that rolls back on commit failure. Public-only
keyfiles are still written without encryption. Keyfile, wallet persistence,
Ledger, and Drand-backed timelock operations are Promise-based so blocking I/O
and expensive KDF work run off the JavaScript thread. The dangerous
compatibility helper that returns plaintext keyfile JSON is named
`dangerouslyDecryptKeyfileData()` and is also available from
`dangerousKeyfiles`. The environment password helpers are legacy compatibility
APIs (`legacyGetPasswordFromEnvironment()` and
`legacySavePasswordToEnvironment()`); they use reversible obfuscation, not
secure password storage.

## Browser example

```ts
import { Keypair, initBrowser } from '@bittensor/sdk/browser'

await initBrowser()

const alice = Keypair.fromUri('//Alice')
const message = new TextEncoder().encode('hello')
const signature = alice.sign(message)
console.log(alice.verify(message, signature))
```

Extensions or strict-CSP applications can pass their own loader:

```ts
await initBrowser(() => import('./vendor/bittensor_core_wasm.js'))
```

## Binding parity and secret derivation

Mnemonic, password, and secret-URI derivation state is retained only by the Rust
`Keypair`; TypeScript calls the native handle's `derive(path)` method and never
reconstructs a child secret URI. `npm run build` also generates binding
coverage from the Rust-side `#[napi]` and `#[wasm_bindgen]` annotations, then
checks that surface against the generated N-API declarations, `src/native.ts`,
the generated WASM declarations, `BrowserWasmModule`, and an explicit
public-browser-wrapper allowlist, so binding additions cannot silently
disappear from either TypeScript boundary.
