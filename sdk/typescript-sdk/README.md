# `@bittensor/sdk`

The monorepo's TypeScript SDK. It lives in its own `sdk/typescript-sdk`
package and is a deliberately thin Node-API wrapper around the sibling
`bittensor-core` Rust crate.

All chain-defined work runs in Rust:

- sr25519/ed25519 keys, signatures, SS58, and wallet keyfiles;
- SCALE encoding and decoding, runtime metadata, storage keys, calls, and
  signed extrinsics;
- RFC-0078 metadata digests and Ledger proofs;
- drand timelock encryption and epoch-schedule simulation;
- ML-KEM/XChaCha20 MEV-shield envelopes; and
- Ledger HID signing.

The TypeScript layer is limited to JavaScript-friendly names and defaults,
lossless `Buffer`/`bigint`/`Map` boundary conversion, error classes, and
compatibility adapters for the signer objects expected by Polkadot.js,
Polkadot API, and Moonwall.

The package exposes both CommonJS and ESM entrypoints. It also exports the
raw generated Node-API module as `@bittensor/sdk/native`, so every native
entry point is callable even when an ergonomic wrapper has not yet been
added.

## Build locally

From the repository root:

```sh
cargo test -p bittensor-typescript-sdk-native --all-features
npm --prefix sdk/typescript-sdk ci
npm --prefix sdk/typescript-sdk run check
```

The native crate is isolated under `sdk/typescript-sdk/native`; it links
`sdk/bittensor-core` directly and contains binding glue only. No chain
algorithm is reimplemented in TypeScript.

Node.js 20.17 or newer is required.

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
