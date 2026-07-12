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
compatibility adapters for the signer objects expected by Polkadot.js,
Polkadot API, and Moonwall, plus the client responsibilities that deliberately
do not live in Rust: WebSocket/HTTP JSON-RPC transport, reconnect/fallback
handling, subscriptions, storage queries, runtime API calls, nonce tracking,
extrinsic submission and inclusion/finalization outcome handling, wallet
filesystem management, generated-style descriptors, and high-level Bittensor
operations for balances, subnets, metagraphs, staking, registration, serving,
transfers, and weights.

The package exposes both CommonJS and ESM entrypoints. It also exports the
raw generated Node-API module as `@bittensor/sdk/native`, so every native
entry point is callable even when an ergonomic wrapper has not yet been
added.

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

Node.js 22 or newer is required for the default WSS client path because the
SDK uses the unflagged global `WebSocket`. Older Node runtimes can still use
HTTP endpoints or pass `webSocketFactory`/`webSocketConstructor` explicitly.
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
// key and signing operation remain in Rust.
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
