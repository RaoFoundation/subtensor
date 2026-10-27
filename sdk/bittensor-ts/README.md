---
title: "Bittensor TypeScript SDK"
description: "Chain reads, transaction workflows, signing, wallets, and EVM APIs for TypeScript backends and frontends."
---

# `@bittensor/sdk`

The TypeScript SDK exposes the current Rust client and Python SDK workflows as
application APIs. Import `@bittensor/sdk` in Node.js or `@bittensor/sdk/browser`
in a frontend. Both entry points provide the full registered read and semantic
intent catalogs, snapshots, transaction planning, external signing, and EVM APIs.

Rust remains authoritative for keys, signatures, SS58, SCALE, metadata proofs,
timelock encryption, and MEV envelopes. Node uses the native client; the browser
uses WASM and the portable JSON-RPC transport. Browser code has no native addon,
Node `Buffer`, filesystem, or HID dependency. Filesystem wallets, encrypted
Substrate keyfile imports, native Ledger HID, the extension bridge server, and
CPU/OpenCL mining are Node APIs. Frontends can sign through injected extensions,
Vault callbacks, or WASM keypairs and supply a worker/service to the portable
registration-mining workflow.

The package is monorepo-internal (`"private": true`). A public npm release still
requires a cross-platform native binary publishing layout and CI matrix.

## Build locally

From the repository root:

```sh
cargo test --manifest-path sdk/bittensor-ts/native/Cargo.toml --all-features
npm --prefix sdk/bittensor-ts ci
npm --prefix sdk/bittensor-ts run check
```

The native crate is isolated under `sdk/bittensor-ts/native`; it links
`sdk/bittensor-core` directly and contains binding glue only. Protocol cryptography and SCALE encoding stay in Rust. Portable TypeScript
implements application workflows and preflight calculations matching Python.

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
// Use a wallet or external signer in production; //Alice is a development key.
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
`submitSigned()` and `watchSigned()` delegate encoded submission and receipt
tracking to Rust. On `BrowserChainClient`, `watchSigned()` uses the TypeScript
WebSocket transport for custom/browser subscription handling.

Automatic submissions share a per-account queue. Shielded submissions reserve
both the carrier and inner nonce, including when the caller returns before
inclusion or loses the response. Plans and fee estimates do not reserve nonces.
After reconciling an abandoned or expired transaction, use
`await client.invalidateNonce(address)` before retrying with automatic nonces.
Do not clear a reservation while its carrier or inner transaction may still
execute. Explicit nonces remain the application's responsibility.

Mortal transactions anchor to the previous block, whose hash is available to
parent-state runtime validation, while using current runtime metadata.

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

## Semantic workflows

`listReads()` describes all 105 registered reads. `listIntentTools()` describes
all 86 semantic operations, their inputs, defaults, signer roles, and safety
requirements. Calls, storage, constants, and runtime API descriptors are generated
from the Python metadata catalog; precompile ABIs come from canonical Solidity.
`buildIntent()` and `intentBuilders` provide operation-specific argument types,
including nested batch and multisig calls.

```ts
import { Client, Wallet, buildIntent } from '@bittensor/sdk'

const client = await new Client('finney').connect()
const wallet = new Wallet({ name: 'builder' })
const intent = buildIntent('add_stake', {
  hotkey_ss58: '5F...', netuid: 1, amount_tao: '1.25',
})
const policy = { maxSpendRao: 1_250_000_000n, maxFeeRao: 10_000_000n, allowedNetuids: [1] }
const plan = await client.workflows().plan(intent, wallet, { policy })
if (!plan.ok) throw new Error(plan.violations.join('; '))
// Execution rechecks current state before unlocking or requesting a signature.
const result = await client.execute(intent, wallet, { policy, waitForFinalization: true })
await client.close()
```

Semantic intent money fields accept exact decimal TAO/alpha strings or a
unit-checked `Balance`. Integer protocol fields use `bigint`. The existing
low-level `IntentCall` constructors continue to accept explicit rao amounts.
Use semantic workflows when you need state-dependent admission and shielding
rules; low-level call construction alone does not perform these checks.
`"all"` remains unbounded for policy checks after resolving a current balance.
Slippage, claim admission, owner hyperparameters, Null weights, multisig deposits,
subnet scope, mandatory shielding, and fee payer selection are checked before
signing. MEV fee limits include the inner transaction, carrier, and both tips.

Additional workflow APIs include:

- Root baskets, allocations, claims, swaps, stake valuation and availability;
  subnet lifecycle, emissions, collateral, conviction, locks, leases, identities,
  commitments, and typed metagraphs.
- Proxy/batch/sudo/multisig composition, detached signatures, extension signing,
  Vault framing, HTTP request authentication, and Ledger signing in Node.
- `EvmRpc`, `getPrecompile`, `EvmContract`, `createEvmWorkflows`, and Node
  `EvmKeystore` for keys, funding, association, staking, contract deployment/calls,
  and every canonical precompile.
- Timelock duration/reveal APIs, bounded PoW registration, transfer history,
  explicit application preferences/address books, runtime upgrade verification,
  and restartable Null pruning with a persistent submission journal.
- Basket index pricing, endpoint latency probes, and structured chain error
  classification for all current module errors and transaction pool rejections.

Failed receipts expose `chainError` with a semantic `code`, exact error name,
metadata documentation, and remediation text. `classifyChainError()` also accepts
RPC failures; `chainErrorFromDispatch()` resolves module indices against the
execution metadata supplied by your application. Unknown historical errors stay
unknown rather than using an unrelated current index.

`SdkPreferences` accepts a browser, memory, or custom JSON store. Node
`FileJsonStore` uses Python-compatible public book files. Preferences are applied
only when your application asks for them; importing the SDK never changes files.

## Browser example

```ts
import { Client, buildIntent, connectExtensionSigner, initBrowser } from '@bittensor/sdk/browser'

await initBrowser()
const client = await new Client('finney').connect()
const signer = await connectExtensionSigner('My app', { address: selectedAddress })
const snapshot = await client.at()
const balance = await snapshot.read('balance', { coldkey_ss58: signer.address })
const intent = buildIntent('transfer', { dest_ss58: recipient, amount_tao: '0.01' })
const result = await client.execute(intent, signer, {
  policy: { maxSpendRao: 10_000_000n, maxFeeRao: 10_000_000n },
  waitForFinalization: true,
})
await client.close()
```

Use the same snapshot for related reads; it pins storage and runtime metadata to
one block hash. Browser amounts are `bigint` and bytes are `Uint8Array`.
`blocks()` is an async iterable and supports cancellation. Frontends should keep
secret material in their chosen wallet/signing backend.

Extensions or strict-CSP applications can provide a WASM loader:

```ts
await initBrowser(() => import('./vendor/bittensor_core_wasm.js'))
```

## Keeping source parity

`npm run check:source-parity` detects changes anywhere in the upstream Rust core,
Python SDK/CLI, canonical ABIs, and generated TypeScript catalogs, plus the runtime
fee and admission rules mirrored by the planners. A source hash
is a drift alarm; it does not prove behavioral equivalence. Review affected APIs,
regenerate their catalogs with the scripts in `scripts/`, run the behavior tests,
and deliberately refresh the reviewed snapshot with `npm run parity:record`.
`npm run check:catalogs` checks regeneration against the Python development
environment (set `PYTHON` if it is not at `sdk/python/.venv/bin/python`).

The test suite exercises each semantic operation, read domains, fixed-point
amounts, signing/authentication vectors, wallet storage, and real Chromium/WASM
integration. `npm run test:localnet` additionally submits against an explicitly
configured loopback chain with funded development accounts, working finality,
and an MEV-capable authority. Set `BT_CHAIN_ENDPOINT` to its WebSocket endpoint;
the live files run serially because they share development signers. Mock RPC
tests and browser codec tests do not establish
that every runtime operation succeeds on a live network.

## Binding parity and secret derivation

Mnemonic, password, and secret-URI derivation state is retained only by the Rust
`Keypair`; TypeScript calls the native handle's `derive(path)` method and never
reconstructs a child secret URI. `npm run build` also generates binding
coverage from the Rust-side `#[napi]` and `#[wasm_bindgen]` annotations, then
checks that surface against the generated N-API declarations, `src/native.ts`,
the generated WASM declarations, `BrowserWasmModule`, and an explicit
public-browser-wrapper allowlist, so binding additions cannot silently
disappear from either TypeScript boundary.
