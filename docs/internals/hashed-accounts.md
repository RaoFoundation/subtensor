---
title: "Hashed accounts"
description: "Stable accounts with mnemonic-derived rotating signing keys, wallet recovery, and the first sr25519 profile."
---

# Hashed accounts

`btcli wallet create --type hashed` creates a new account whose identity is
independent of its current signing key. V1 wraps sr25519. This is public-key
concealment and rotation, **not post-quantum cryptography**: key recovery after
publication can still allow competing authorizations before finality. Solving
that exposure window, including a suitable encrypted-commitment protocol, is
separate work. Current MEV Shield does not establish that guarantee.

The production runtime keeps registration and authorization disabled pending
reference weights and deployment review. Functional tests exercise the enabled
protocol with explicitly substituted test weights. No deployed support is implied.
EVM ownership adapters are likewise compiled into the active path only in tests
and benchmark builds. Their activation must be permanent once hashed accounts
exist; disabling ownership checks later would expose alternate signing routes.

## User model

Create a new hashed wallet, then fund it or use the normal coldkey swap flow.
The address remains unchanged as signing keys rotate; balance, stake, UID and
ownership do not move on each transaction. Coldkey and hotkey remain separate
wallet roles and retain separate recovery phrases. `--type hashed` selects the
hashed scheme for both roles unless the hotkey scheme is explicitly overridden.

Wallet creation and mnemonic recovery are offline for network presets whose
genesis hash is bundled with the SDK. A custom network needs one genesis lookup
before keyfiles are written, so its receiving address cannot name the wrong
chain. Key derivation itself remains offline. On a supporting chain the
SDK reads the current account generation, verifies its commitment against the
locally derived key and obtains the current transaction nonce before signing.
There is no growing secret key history to back up. Restore the original mnemonic
with the hashed type on another device; derivation version 1 reconstructs the
same initial descriptor, address and every generation's key. A derivation
passphrase, if used, remains part of the recovery secret.

Each generation is derived independently from a secret master seed, with fixed
domain labels and a generation index. A current signing secret is not the seed
for the next key. Unknown versions and schemes fail closed. Adding another
signature family later requires an explicit bounded proof format and verifier;
it does not reinterpret existing wallet backups or transaction bytes.

## Registration and receiving funds

The account identity commits to a versioned descriptor containing the underlying
scheme and initial public-key commitment. Registration publishes this descriptor,
not the current public key. It is permanent and idempotent, and does not let the
sponsor control the recipient, reset its generation or reset its nonce.

Wallets display a single **104-character `bth1_` receiving address**. The user
shares that address and the sender uses the normal transfer command. The address
carries the full chain genesis hash, original descriptor and an error-detection
checksum. It contains no signing public key and stays unchanged across rotations
or mnemonic recovery. There is no separate descriptor to exchange or registration
service to operate.

An updated sender wallet decodes the address, checks the connected chain and
composes `Utility.batch_all([HashedAccounts.register(descriptor), transfer])`.
The chain receives its existing AccountId32 and descriptor types; it does not
guess whether arbitrary 32-byte data is a hash. Registration and payment either
both succeed or both roll back. The guard remains in later payments even when a
read shows the recipient is registered: a reorg can invalidate that read. Its
idempotent execution preserves the current generation, commitment and nonce,
and does not charge another registration reserve.

The new receiving address requires an updated sender wallet. Older wallets do
not understand it. Do not replace it with the internal SS58 AccountId before
registration: that loses the setup information and may expose the account to a
legacy signing interpretation. Native keypair `ss58_address` remains an internal
chain identifier for compatibility; use the receiving-address API for sharing.
Contacts, previews and supported high-level SDK calls preserve the complete
receiving address until call construction. Raw call paths that cannot retain
setup information reject it.

The wire format is `bth1_` followed by canonical, unpadded base64url of
`genesis_hash[32] || SCALE(descriptor)[34] || checksum[8]`. The checksum is the
first eight bytes of BLAKE2-256 over
`b"bittensor/hashed/v1/receiving" || genesis_hash || descriptor`. It detects
copying mistakes; it does not authenticate a payee. Unknown versions, malformed
encoding, unsupported schemes, zero commitments and a different connected
genesis fail before payment construction. The codec lives in the native SDK
and is shared by Python bindings.

A sponsor pays the existing storage-price-based registration reserve and fee.
The reserve stays locked on the sponsor for the permanent registration; it is
not a transaction execution weight estimate or a spendable recipient balance.
The preview and spend policy include the maximum possible reserve, even for an
already registered recipient that could disappear in a reorg. First-payment
affordability includes the complete batch fee and the sender's existential
deposit. A send-all executes after registration and sweeps the remaining
transferable balance; the permanent reserve also prevents reaping the sponsor.

First-time setup supports a direct payment or the runtime's permitted direct
migration/registration operation. Initial setup inside a general batch, proxy,
multisig or sudo call is rejected with an actionable error. Existing recipient
guards are flattened inside batches because the runtime rejects nested batches.
If a reorg makes an unsupported wrapped recipient unregistered, its guard fails
and the atomic operation rolls back. Imported multisig bytes with recipient
setup metadata are rejected because display metadata cannot prove those bytes
contain the correct guard. Normal coldkey-swap delay and scope still apply; a
liquid transfer alone does not move stake or external relationships.

**Activation blocker:** a third party can use the existing unconsented hotkey
association call to assign a published, not-yet-registered account to a classical
coldkey. Hashed registration then rejects that owner. The atomic payment fails
without sending funds, but the advertised address cannot activate. The runtime
test documents this denial of service; it does not resolve it. Fix the ownership
consent/preassociation policy before enabling the feature. Removing existing
ownership records would risk stake and accounting relationships and is not an
acceptable local workaround.

## EVM receiving boundary

Registered protected H160 aliases map to their complete hashed native accounts.
SDK native funding resolves and validates this mapping, then uses the same
guarded receiving-address transfer. EVM deposits into a protected alias already
credit its native account, so a legacy claim/withdraw route is refused.

When hashed aliases are enabled, native funding to an absent alias is refused:
a concurrent registration could change the account behind that H160 after a
wallet read. Use an EVM-side transfer for those H160 recipients until there is
an atomic runtime funding route. Disabled chains retain their legacy behavior.
An offline `ss58_mirror` property denotes the legacy mapping only and must not be
treated as a protected alias's deposit address. An arbitrary EVM RPC override
does not establish the matching native genesis, so its balance output omits
unverified native receiving guidance.

## Transaction and recovery contract

The runtime routes legacy transaction bytes unchanged and uses v5 General
transactions with extension version 1 for hashed authorization. A proof supplies
the generation, current public key, next commitment and signature. The signature
binds the stable account, scheme, generation, next commitment, complete call,
normal transaction extensions and implicit chain/version/mortality data.

The chain verifies the public key against the stored commitment and establishes
the ordinary `Signed(account)` origin. Generation and next commitment advance
when the transaction is accepted, even if the dispatched call fails. An invalid
transaction does not advance them. Generation is distinct from the normal nonce:
registration initializes an EVM-safe nonce, and native EVM execution can affect
nonce separately. The permanent provider preserves replay state at zero balance.

Only one pending signing generation per account is supported initially. Competing
devices can derive the same key but must coordinate submission. After a reorg or
a dropped transaction, the wallet must resynchronize; this does not erase public
keys disclosed by abandoned transactions. The quantum exposure limitation still
applies to such disclosures.

Registered accounts reject the legacy native signature route. EVM aliases bind
to their complete identity and reject classical Ethereum/EIP-7702 authorizations.
Proxy grants cannot introduce a classical delegate. Off-chain messages and limit
orders that require legacy signatures do not automatically inherit this adapter;
generic legacy signing is rejected instead of revealing a rotating spending key.
Downstream miner authentication integrations require their own versioned support.
Hashed hotkeys require a hashed coldkey owner, and a hashed coldkey cannot swap
its authority to a classical coldkey. The chain's existing first-association
semantics still do not require hotkey consent; this change does not solve that
separate owner-association problem.

V1 authorization adds 71 bytes to the ordinary sr25519 transaction envelope
before any change in its compact length prefix. The stored account record is
74 encoded bytes, plus a 32-byte EVM alias value, storage keys and system-account
bookkeeping. Execution fees require measured weights; these byte counts are not
a fee estimate.

## Deployment requirements

- Measure registration hooks and authorization, including message length, on
  reference hardware; replace fail-closed unmeasured weights through the normal
  benchmark process.
- Meter EVM alias mapping reads and authorization-list checks on successful paths
  as well as rejection paths before enabling the EVM adapters in production.
- Run the repository preflight, generated binding/documentation checks and funded
  localnet flows, including recovery and successive-generation transactions.
- Review the complete authority graph: coldkey/hotkey consent, swaps, proxies,
  derived accounts, EVM and off-chain protocols. Existing classical consensus and
  delegated authority do not become quantum-resistant through this account type.
- Resolve the preassociation denial of service before accepting newly published
  receiving addresses on an enabled chain.
- Upgrade sender wallets and receiving integrations together. Standard SS58-only
  wallets, exchanges and explorers cannot infer the missing descriptor.
- Validate explorer/indexer decoding of the additional v5 pipeline.
- Publish the native core and Python SDK together with an updated native minimum
  dependency version, so installed wallets have the new key APIs.
- Obtain the explicit runtime-version and activation decisions. This work does
  not change the runtime spec version or enable production authorization.
