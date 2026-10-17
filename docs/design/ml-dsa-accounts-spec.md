---
title: "ML-DSA account specification"
description: "Quantum-resistant account authorization with standard client decoding, multisig support, and complete account operation coverage."
---

# ML-DSA account specification

**Status:** Implemented protocol candidate; production activation remains blocked on the release gates below. **Date:** October 15, 2026. **Baseline:** [PR #3220](https://github.com/RaoFoundation/subtensor/pull/3220), head `f867191868d29c5b28d3afdf13f967a12f0af128`, base `f4786f9087cac84cfab37cbdf1e01c399042653a`. Reconcile any subsequent merge changes before release. MUST, MUST NOT, SHOULD, and MAY express requirements within this specification. Local tests and benchmark definitions are not reference measurements, an independent cryptographic audit, or proof of production readiness.

Add an **ML-DSA-65 account type** that authenticates ordinary Subtensor calls as the account's permanent `Signed(AccountId32)` origin. Preserve signed-v4 extrinsic framing, `MultiAddress`, `MultiSignature`, existing signed-extension bytes, and ordinary call encoding. Carry the ML-DSA public key and signature inside the existing variable-length `MultiAddress::Raw` variant. Existing block decoders must continue to decode mixed blocks without a custom signature enum or type override.

Multisig, proxies, staking, hotkey and coldkey operations, MEV Shield, limit orders, and native EVM operations are release requirements. Support for simple transfers alone is insufficient. This proposal includes the additional authorization work those operations require; changing only the transaction verifier does not provide full compatibility.

# Compatibility contract

Public events, storage, constants, and SDK APIs use descriptive scheme names, not revision suffixes. The descriptor tag `2` and literal `/v2/` cryptographic domain bytes are frozen protocol inputs: renaming them would change wallet identities, derivation, or signatures. ML-DSA receiving addresses use the `tao_` prefix. Existing economic order V1/V2 variants are independent of account authentication and remain unchanged. Historical evidence files retain the names captured at their recorded commits; they are not current metadata.

| Surface | Required behavior |
| --- | --- |
| Existing ordinary wallets | Existing keys, addresses, signatures, transaction bytes, and account behavior remain valid. No migration or signing update is required. |
| PR #3220 v1 hashed wallets | Existing descriptors, derivation, 106-byte Raw signers, signature verification, storage, and rotation semantics remain unchanged. |
| Existing metadata-aware readers | Decode every extrinsic in a mixed block, preserve transaction hashes, resolve ordinary calls and events, and continue subscriptions using the normal metadata upgrade mechanism. |
| ML-DSA wallets | Updated software creates the new proof and submits standard signed-v4 bytes. The new signing algorithm cannot appear automatically in an old wallet. |
| Runtime account operations | Authentication yields the permanent `Signed(AccountId32)` before normal nonce, fees, policy, and dispatch handling. No key-specific transfer wrapper is introduced. |
| Multisig | Existing pallet calls and account derivation work with classical, v1 hashed, and ML-DSA signatories, including mixed groups. Protected multisig authorities additionally satisfy the security policy below. |

“All clients” means preserving the established wire and metadata contract for conforming clients, backed by a published version matrix. A client hard-coded to accept only `MultiAddress::Id`, fixed legacy metadata, or a 64-byte classical proof cannot be made universally compatible by a runtime change. An old client's optional classical signature verifier will not verify an ML-DSA transaction; its block decoder must still work. Displaying the Raw signer as hex is acceptable for decoding, but supported explorers must be tested for usable account navigation.

Account operation parity preserves each operation's authorization, origin filters, limits, and failure behavior. It does not grant a signed account Root authority, bypass a proxy restriction, or make a classical delegate quantum-resistant.

# Cryptographic profile

Use **ML-DSA-65 in pure mode** from FIPS 204. Its public key is 1,952 bytes and signature is 3,309 bytes; its parameter set has NIST security category 3. A KeyGen seed is 32 bytes. Do not substitute pre-standard Dilithium, another parameter set, or HashML-DSA under this identifier. [FIPS 204](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)

The account uses ML-DSA alone for authorization. Neither the carrier field nor an Sr25519, Ed25519, or ECDSA signature is an alternative authority. A future hybrid algorithm must use a separate identifier and explicitly require both proofs.

Use 64-byte SHAKE256 commitments for ML-DSA keys. Keep the 32-byte account identifier for chain compatibility. Consequently, the complete account system must not be advertised as having category 3 security in every component: identifier hashing, Ethereum aliases, existing multisig call hashes, consensus, and application contracts retain their existing assumptions. In particular, targeted search against a uniformly distributed 256-bit identifier has a generic quantum preimage bound around 128 bits; collision security is a different property. The promise is quantum-resistant account authorization under these documented surrounding assumptions.

ML-DSA is cryptographically stateless. Per-transaction key rotation is an operational policy inherited from hashed accounts, not a stateful-signature usage limit. Revealing an ML-DSA public key in the mempool does not create the classical key-disclosure vulnerability that motivated v1 concealment.

# Identifiers and permanent state

Allocate a distinct ML-DSA descriptor and record. Do not change the size or interpretation of v1 `Descriptor`, `AccountRecord`, or their storage values. Algorithm value `4` is defined in the separate ML-DSA namespace; it does not append a variant to a legacy signature or scheme enum.

```text
MlDsaDescriptor = SCALE(
    version: u8 = 2,
    algorithm: u8 = 4 or 5,            // ML-DSA-65: rotating (4) or fixed (5)
    initial_commitment: [u8; 64]
)

commitment(pk) = SHAKE256(
    ASCII("bittensor/ms/v2/key") || 0x02 || 0x04 || pk,
    output_bytes = 64
)

account = Blake2_256(
    ASCII("bittensor/ms/v2/account") || MlDsaDescriptor
)

MlDsaAccounts[account] = {
    descriptor: MlDsaDescriptor,
    generation: u64,
    commitment: [u8; 64]
}
```

ASCII domain strings above have no trailing NUL or SCALE length prefix. Fixed byte arrays have no length prefix. Integers use SCALE's declared type encoding; `u64` is fixed-width little-endian, not compact. Reject unknown version/algorithm pairs, zero commitments, conflicting account registrations, and trailing descriptor bytes. Never auto-detect an algorithm from signature length.

The wallet sets `initial_commitment = commitment(pk(role=0, generation=0))`. New registration initializes `generation=0` and the current commitment to that initial commitment. The first accepted authorization advances to generation 1. Registration is permanent and idempotent only for the identical descriptor; idempotence never resets either field. Reaping balances, account death, disabling new registrations, and runtime upgrades must not erase authority protection or reset generations. A disabled verifier must fail closed, never expose the same AccountId to ordinary classical signature verification.

Preserve v1 `Accounts` and its getter. Add `MlDsaAccounts` and explicit queries for `authentication_kind(account)`, `is_protected(account)`, and `authority_meets(account, security_floor)`. Direct signing records and derived authorities must remain distinct. Replace security-relevant checks that currently consult only v1 registration with the appropriate unified query.

Use the original `register` and `check_registered` calls at indices 0 and 1 with `RegistrationDescriptor::{Classical(Descriptor), MlDsa65(MlDsaDescriptor)}` (tags 0 and 1). HashedAccounts has not been deployed; this selector is part of its first published API. Inner descriptor encodings and account IDs remain unchanged. There are no duplicate registration dispatchables. Registration includes EVM alias collision checks, provider references, nonce initialization, ownership checks, storage deposits, and alternate-authority exclusion. The candidate uses the existing storage-price function with `deposit(6, 1024)` for direct ML-DSA and `deposit(6, 8192)` for protected derived accounts, including bounded capability records and permanent epoch tombstones. These storage prices are separate from verifier weights; reference measurements and storage-proof accounting remain release gates.

Anyone may pay to register a direct ML-DSA descriptor without gaining authority. A sponsor must not use registration to erase another account's existing privileges or reinterpret a conflicting identity. Reject conflicting preexisting ownership, EVM associations, or unsafe control paths; require an explicit authorized migration where needed. New accounts must register before receiving assets or privileged roles. SDK funding and role-assignment flows must preserve the descriptor and use atomic registration/registration guards, including after reorgs.

Keep SS58 as the representation of the permanent AccountId. Define the receiving address as `"tao_" + base64url_no_pad(MlDsaDescriptor || checksum8)`, where `checksum8` is the first eight bytes of `Blake2_256(ASCII("bittensor/ms/v2/receiving") || MlDsaDescriptor)`. The descriptor is 66 bytes, decoded body 74 bytes, and complete string 103 characters. Require canonical base64 re-encoding, exact case/length, a valid checksum, and a supported descriptor. There is no network field; registration remains chain-specific. Preserve all v1 parsing. Publish exact roundtrip vectors. Older wallets may reject the new receiving descriptor clearly. Existing clients can use its SS58 identity after registration. SDK identity-only fields normalize to AccountId; payment and setup fields retain the full descriptor.

Protection begins with finalized registration. The wallet must complete registration before publishing its new identity or receiving assets/roles; initial registration and hostile preexisting state need explicit race tests. Existing ordinary or v1 accounts are not silently reinterpreted as ML-DSA. Migration creates a new ML-DSA identity, registers it, and transfers balances, stake, and ownership through existing authorized transfer/coldkey-swap workflows.

# Exact signed-v4 wire format

```text
Compact<u32>(body_length)
0x84                                      // signed extrinsic v4
0x02                                      // MultiAddress::Raw
Compact<u32>(5367)                         // bytes dd 53
MlDsaSignerAddress[5367]
0x00                                      // MultiSignature::Ed25519
0x4d repeated 64 times                     // canonical nonzero carrier
existing encoded TxExtension fields
existing encoded RuntimeCall
```

`MlDsaSignerAddress` has exactly this layout:

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 32 | Permanent AccountId |
| 32 | 1 | Version `2` |
| 33 | 1 | Algorithm `4` |
| 34 | 8 | Generation, little-endian `u64` |
| 42 | 1,952 | Current ML-DSA-65 public key |
| 1,994 | 64 | Next generation key commitment |
| 2,058 | 3,309 | ML-DSA-65 signature |
| Total | 5,367 | Exact Raw byte count |

The permanent AccountId stays first, matching v1's explorer-display convention. The fixed signed body overhead is 5,436 bytes before extensions and call bytes, plus the outer compact length prefix. This is 5,337 bytes more than an ordinary Ed25519/Sr25519 `Id` signer, or 5,261 more than the v1 Raw signer, before any outer length-prefix changes.

The carrier is a **format marker, not an Ed25519 authorization**. It is public, constant, and must match exactly. It requires no classical private key. The runtime accepts it only in the validated ML-DSA Raw route and still requires a valid ML-DSA proof. Its value must never activate an exception for an ordinary `Id`, v1 address, bare extrinsic, general extrinsic, or Ethereum transaction.

An all-zero carrier is forbidden. Polkadot.js derives its signed flag and encoded signature length from whether the signature bytes are empty; zero bytes can therefore move the decoded call offset and break the block. A nonzero value retains the standard layout. [Polkadot.js ExtrinsicSignatureV4](https://github.com/polkadot-js/api/blob/master/packages/types/src/extrinsic/v4/ExtrinsicSignature.ts)

Do not add an ML-DSA variant to `sp_runtime::MultiSignature`, replace its metadata with a custom enum, change `MultiAddress`, add hidden serialized extension fields, or place the proof before the existing extrinsic header. Preserve decoding of the other extrinsic formats currently supported by the runtime, without allowing ML-DSA proofs to bypass their authentication rules.

The Raw decoder must admit only exact supported tuples: v1's existing length/version/scheme combinations and ML-DSA `(5367, 2, 4)` and `(5367, 2, 5)`. Read and bound the compact length before allocation. Enforce canonical encodings, exact consumption, enclosing memory tracking, call depth limits, and block/extrinsic length limits. Metadata must describe the actual standard outer bytes. Preserve the flattened signed-extension metadata identifiers, order, field types, and additional-signed types, continuing the current metadata-forwarding behavior. Internal authentication context is skipped by SCALE and must be reconstructed from the decoded address, never supplied by a caller.

# Signature transcript

Let `MlDsaHeader` be the first 2,058 bytes of `MlDsaSignerAddress`, excluding its signature. Define:

```text
context = ASCII("bittensor/ms/v2/tx")

transcript =
    ASCII("bittensor/ms/v2/transaction") ||
    MlDsaHeader ||
    SCALE((RuntimeCall, TxExtension, TxExtension.implicit()))

proof = ML-DSA-65.Sign(secret_key, transcript, context)
```

The implicit extension data must be exactly the data used by the runtime for that transaction: genesis hash, runtime and transaction versions, mortality checkpoint, metadata hash when enabled, and all other signed extension values. Bind the exact call bytes, nonce, tip/payment options, account, algorithm, generation, public key, and next commitment. There is no caller-supplied override for implicit data.

Do not use the generic Substrate rule that first replaces a payload longer than 256 bytes with Blake2-256. Do not include the ML-DSA signature in its own transcript. The carrier and outer framing are uniquely fixed by canonical decoding; alternative carrier bytes are rejected before authentication.

A streaming implementation may compute the standard pure-mode internal message representative locally and verify it:

```text
tr = SHAKE256(public_key, 64)
mu = SHAKE256(tr || 0x00 || u8(context_length) || context || transcript, 64)
```

It must produce exactly the same decision as pure external verification. Never accept an externally supplied `mu` or application digest as the transaction authority. A counted SCALE output adapter can hash the full bounded payload without creating another large buffer. [NIST clarification on external message representatives](https://csrc.nist.gov/csrc/media/Projects/post-quantum-cryptography/documents/faq/fips204-sec6-03192025.pdf)

# Validation and state transition

1. Decode the standard envelope under strict byte, memory, and depth bounds. Check the ML-DSA tuple and exact carrier. Reject self-contained Ethereum calls on this signed route.
2. Perform cheap feature, account, generation, commitment, nonzero next-commitment, and resource checks. Require the proof profile to equal the immutable registered descriptor profile and the current public key's commitment to equal the registered commitment. For rotating profile 4 require a different nonzero next commitment. For fixed profile 5 require current and next commitments both to equal the descriptor's initial commitment. Reject generation overflow. Account for reads and hashing before doing expensive work.
3. Verify the ML-DSA proof over the complete transcript. A decoded carrier alone never constructs a trusted origin. Verification errors, malformed keys/signatures, and deterministic resource exhaustion fail closed.
4. Construct the permanent `Signed(AccountId32)` and run all normal transaction extensions with that signer: nonce, mortality, versions, fees, filters, and account-specific policy. Carry a private validated ticket containing account, profile, generation, old commitment, and next commitment.
5. Pool validation is read-only. Use the existing nonce dependencies and an exclusive `(account, generation)` provides tag; competing transactions at one generation cannot both execute. Do not promise future-generation pipeline validation against uncommitted state.
6. Immediately before dispatch, recheck the ticket against current state and atomically advance the authorization generation in extension preparation. Profile 4 advances the key commitment; profile 5 retains its initial commitment and emits `AuthorizedFixed` instead of `RotatedMlDsa`. Preserve normal fee/nonce handling. Any invalid-extrinsic application must roll back preparation changes; an included valid extrinsic whose call returns a dispatch error still consumes the authorization exactly once.
7. Dispatch the original call under the original filter stack. Do not dispatch it through an unrestricted wrapper or change the account to the revealed key. Normal events, post-dispatch refunds, and nested-call semantics apply.

Signing, broadcasting, a dropped transaction, pool eviction, simulation, and fee quotation do not rotate state. For profile 4, a successful on-chain `System.remark` is sufficient for an explicit rotation transaction. Profile 5 keeps its signing key; every included authorization still consumes its generation counter, including dispatch failures and both Shield transactions. Reorg recovery reads canonical state again; inclusion alone is not finality. Wallets coordinate a single outstanding generation per account unless they implement a tested sequential inclusion workflow.

The account's system nonce and authorization generation are separate counters; the latter also selects a new transaction key only for rotating profile 4. Preserve this distinction through EVM nonce activity, sponsored transactions, multisig execution, and MEV Shield. Restore from finalized descriptor/generation and verify the derived current commitment before signing; stale public wallet metadata cannot select authority.

# Multisig and other derived authorities

## Ordinary multisig behavior

Keep the existing multisig account derivation, sorted unique signatories, threshold semantics, call hashes, deposits, timepoints, approval storage, and call indices. Members are permanent AccountIds. An ML-DSA public key, Raw envelope, descriptor string, or generation must never become a multisig signatory identifier.

Every ML-DSA member independently signs its outer approval, execution, or cancellation transaction. That member's generation advances on inclusion, even if the final inner call fails. The composite account does not inherit or consume a member's generation. Its dispatched call has the same composite `Signed` origin as it does today. Preserve the distinction between the outer transaction fee payer, multisig deposit payer, and any inner call balance movement.

Support threshold one, approval by hash, execution with call bytes, repeated approvals, cancellation, maximum supported threshold, mixed classical/v1/ML-DSA members, and all-ML-DSA groups. Rotation between approvals must not invalidate an approval because identity is permanent.

The SDK must pin the exact approved call bytes, including receiving guards, price/slippage inputs, nested wrappers, and setup decisions. Later validation must not rebuild a price-sensitive call from current state. Display inner events and actual dispatch outcomes; an outer success or `BatchCompleted` is not proof that a proxied or multisig inner call succeeded.

## Protected composite accounts

Ordinary multisig functionality and quantum protection are separate properties. Add a permanent `DerivedAuthorities` registry for a composite that will hold protected funds, stake or roles, install capabilities, or control an ML-DSA account. New all-ML-DSA and qualifying mixed multisigs must establish this protection by default before funding; merely choosing ML-DSA members is insufficient. Historical groups retain authorized opt-in, and unprotected composites must not be presented as PQ-protected. Bind the existing AccountId to its immutable derivation data: multisig members/threshold, derivative parent/index, or pure-proxy derivation and security floor. Recompute the existing derivation formula; never accept an arbitrary claimed AccountId.

Restricted proxy grants must not reach native EVM execution: an EVM contract or dispatch precompile can reconstruct an unrestricted native origin from the caller's protected alias. `NonCritical` therefore excludes the EVM call group, just like other restricted grants; direct owners and `Any` proxies retain EVM access. This restriction applies to direct, batched, and announced proxy dispatch and does not depend on current ML-DSA activation flags.

Direct classical signature verification and legacy embedded-signature paths must reject registered protected composites. They may be controlled only through their verified runtime dispatch path. Registration must also atomically establish the permanent EVM alias, correct backing-account/nonce mapping, and exclusion of ECDSA authority, with collision and conflicting-state checks equivalent to direct ML-DSA registration. These protections survive reaping and pure-proxy removal. Registration does not mint `Signed` origins, replace the threshold check, or bypass inherited filters. Test every route that can manufacture a signed derived origin against the recorded derivation.

A threshold group is resistant to a quantum attacker forging all non-PQ members only when:

```text
threshold > number_of_members_without_PQ_protection
```

For example, 2-of-3 with two protected ML-DSA members and one classical member needs at least one ML-DSA approval; 2-of-3 with two classical members can be compromised through those two members. Both groups must work as ordinary multisigs. Only the first qualifies as a PQ authority. This condition does not imply that two PQ approvals are required or protect against collusion by already-authorized members.

The classification must include every alternate control route of a member, not just its primary key. Each protected record freezes the sufficient `pq_dependencies` selected from the members already protected at registration, alongside its immutable descriptor, depth, and expanded node count. The threshold must exceed the count of initially weak members. The stored PQ proof is acyclic and bounded; later registration of an initially weak member does not retroactively enlarge an ancestor's proof. Full multisig membership can contain other references, but those references cannot create a circular proof of PQ authority. Cached classifications are valid only because selected dependencies cannot downgrade. Bounds are 100 multisig members, proof depth 16, and 1,024 expanded proof nodes, counting repeated shared dependencies. Reject unsupported shapes and over-budget proofs deterministically.

Every protected multisig registration, including a supposedly unused address, requires the composite's own `Signed` origin produced by its actual threshold. Never infer freshness or consent from a zero balance, zero nonce, absent System account, reaping, or lack of one particular application record: stake, roles, contracts, and other authorizations can outlive those observations. A single member or outside sponsor cannot unilaterally change an existing or new multisig's assurance level.

Use `register_derived(descriptor)` when that authenticated composite pays its own permanent deposit. To avoid funding an unprotected composite, approve and execute `register_derived_sponsored(descriptor, deposit_payer)` through the existing multisig quorum. Its exact descriptor and payer are part of the pinned inner call bytes and existing multisig call hash. The executing outer signer must be the named payer; the runtime reconstructs that signer from the authenticated extrinsic and confirms the exact sponsored-registration call occurs in the bounded supported wrapper tree. The callback rejects extrinsics exceeding 64 KiB before allocating their full encoded bytes, and bounds wrapper traversal to eight levels. Payer consent does not replace the composite's quorum consent. Later execution must not substitute another payer or rebuild an approved registration from changing state.

Derivative registration requires its authenticated parent or child. A pure-proxy spawner may establish protection only for an actual creation in that same block/extrinsic; an existing pure account must opt in through its own authenticated origin. Establish protection before funding or assigning roles. Reject incompatible proxies, pending multisig approvals/proxy announcements, ownership, swaps, and EVM associations; do not silently erase them. Historical approvals must not be relabeled as satisfying a newly strengthened policy. A failed transition leaves prior state intact.

Protected pure proxies must be registered atomically at creation, constrain every effective delegate, and keep the direct-signature ban after `kill_pure`. Protected `Utility.as_derivative` identities inherit the verified parent's floor and need registration before receiving protected assets/roles. The appended protected derivative call preserves the ordinary inner-error contract: failure rolls back fresh registration and child state together, leaves existing permanent protection intact, and still consumes an included outer ML-DSA authorization's generation. Preserve existing derivation bytes in both cases. Arbitrary contract accounts cannot be classified as PQ authorities merely from their address; an explicit authorization adapter is required if such authority is offered.

`Proxy.create_protected_pure` and `Utility.as_protected_derivative` can permanently reserve the principal's registration deposit and install authority records. They require the account's full authority; among proxy grants, only `Any` may dispatch them. Every restricted proxy type must deny both calls, including through utility batches and nested proxies. This conservative rule also applies when the derived account already exists; registration must not become a hidden spending permission inside an otherwise allowed wrapper.

# Preventing weaker alternate authorities

PR #3220's baseline `compatible_authority` test checks whether both accounts are registered hashed accounts. That is insufficient for ML-DSA because a registered Sr25519 or Ed25519 delegate remains classical. [Pinned policy implementation](https://github.com/RaoFoundation/subtensor/blob/f867191868d29c5b28d3afdf13f967a12f0af128/runtime/src/hashed_auth.rs#L331-L344)

Set a permanent `PqRequired` floor for direct ML-DSA accounts and opted-in protected derived accounts. Enforce it when granting authority and when using existing authority, across immediate/delayed proxies, pure-proxy creation, hotkey ownership, coldkey swaps/recovery, EVM associations, and any newly discovered origin or signature bypass. V1 and ordinary accounts retain their established policy.

A protected ML-DSA account may use a qualifying protected multisig or another protected ML-DSA account as its authority. It may also act as a proxy or signer for an ordinary account. It must not grant control to a classical or v1-only delegate under a claim of PQ protection. Provide an explicit SDK policy error rather than silently falling back to a classical signer.

Coldkey swaps and ownership changes must support protected targets while retaining operational state transfer, disputes, and scheduling semantics. Moving to a weaker authority is not a transparent key rotation; the protected identity's floor cannot be lowered. If the product offers an explicit asset migration to a weaker destination, it must remain a separately authorized migration with its consequences shown to the user.

# Coverage of chain operations

| Operation family | Required integration and acceptance |
| --- | --- |
| Transfers and balances | Transfer variants, keep-alive/reaping behavior, descriptor funding, fees, reserves, refunds, locks, and existential-deposit edges behave normally. Protection survives zero balance. |
| Staking and economics | Add/remove/move/transfer stake, limits, alpha fees, root claims, basket operations, and subnet administration preserve balances, ownership, fee selection, and error semantics. |
| Neurons and subnet roles | Burn/PoW registration where applicable, hotkey association, weights/commit-reveal, children/delegation, subnet ownership, and role changes accept the permanent account and enforce owner security. |
| Coldkey lifecycle | Announcement, dispute, execution, cancellation, stake/ownership movement, and restoration work with direct and qualifying composite PQ authorities. |
| Utility and proxy | Batch, batch-all, force-batch, supported nesting, derivative calls, immediate/delayed proxies, announcements, rejection, removal, and pure-proxy lifecycle preserve filters and inner outcomes. |
| Multisig | All classical/v1/ML-DSA mixes, approval/execute/cancel paths, pinned bytes, deposits, and protected-authority use pass the requirements above. |
| Scheduled/governance calls | Any path already available to an ordinary signed account remains available with the same origins, deposits, and filters. Root-only calls remain Root-only. |
| Native EVM | Native `EVM.call/create/create2/withdraw`, deployed contracts, precompiles, aliases, balance accounting, and gas/refund handling accept the authenticated permanent account. |
| MEV Shield | Both authenticated carrier and decrypted inner transaction work, including rotation, nonce offsets, cancellation/expiry/failure, and larger payload bounds. |
| Limit orders | Direct ML-DSA authorization and multisig-approved orders use the original execution and cancellation calls described below. |
| Messages and HTTP authentication | Versioned ML-DSA proofs verify under purpose-specific authorization, with replay, expiry, revocation, and finalized-state rules. |

Produce a machine-readable inventory from runtime metadata of every dispatchable, its required origin, and any embedded proof. Each entry must have a passing test, a justified equivalent path, or an existing origin restriction. Search beyond `ensure_signed`: account-id-as-public-key assumptions, fixed signature lengths, ECDSA recovery, embedded `MultiSignature`, and custom origin constructors need separate review. Do not declare complete parity while an unexplained key-specific rejection remains.

# Native EVM boundaries

Keep the account's registered native EVM alias and resolve all native execution back to the permanent AccountId. Enforce alias collision prevention, protected-address restrictions, account creation checks, fee policy, and precompile origin mapping. Charge every account-policy read before the read, including failure paths and dynamic opcodes. Test contract creation, CREATE2, value transfer, reverts, precompiles, SELFDESTRUCT effects, and native gas refunds with ML-DSA signers.

An Ethereum `eth_sendRawTransaction` payload and `ecrecover` still use Ethereum's existing ECDSA rules. An ML-DSA signature cannot be substituted into that format while preserving Ethereum compatibility. Provide equivalent native EVM call/deploy workflows in SDK and CLI. Contracts requiring ECDSA `permit`, `personal_sign`, EIP-712, or EIP-7702 authority need an explicit contract/account adapter; never grant a classical EOA an override of the protected native identity. Future ML-DSA verification precompiles or ERC-1271 adapters are separately versioned application interfaces, not changes to existing ECDSA semantics.

Aura/GRANDPA/session and node networking keys are separate cryptographic roles. An ML-DSA account can perform permitted staking and session-key-management calls, but this account feature does not replace consensus keys or make finality quantum-resistant.

# MEV Shield integration

The baseline Shield ciphertext bound is 8,192 bytes. A 5,367-byte signer materially reduces the usable call budget; support cannot be assumed from successful small transfers. [Pinned Shield implementation](https://github.com/RaoFoundation/subtensor/blob/f867191868d29c5b28d3afdf13f967a12f0af128/pallets/shield/src/lib.rs)

The original `submit_encrypted` call at index 1 now accepts up to 32,768 ciphertext bytes. Its vector encoding and call index are unchanged. Payloads up to 8,192 bytes retain the existing weight; larger payloads use the separately benchmarked `submit_encrypted_large` weight, which currently fails closed. Proposer extraction, mortality checks, the Shield extension, decryption, and SDK framing use this single interface. The complete encrypted inner extrinsic, encryption overhead, and any outer proof must fit their respective bounds. This larger capacity is a protocol bound, not a measured proposer throughput claim; the larger tier remains behind uncalibrated weight gates until the required measurements pass.

When the same rotating account signs carrier and inner transaction, prepare the carrier at `(nonce n, generation g)` and the inner at `(n+1, g+1)`, with consistent chained commitments and the actual runtime's nonce semantics. An unrelated sponsor does not consume the inner account's generation. Validate the full decrypted signed extrinsic; do not reinterpret ciphertext as authorized call bytes. Carrier inclusion, inner failure, missing decryption, expiry, retries, reorgs, and an intervening account transaction must have explicit tested recovery behavior.

# Limit orders and off-chain authorization

The baseline order format embeds `MultiSignature`, and its signer filter excludes registered hashed accounts. Adding Raw transaction authentication does not fix that path. Preserve every legacy order encoding and matcher rule; extend the original calls through metadata-defined local order types. The outer extrinsic `MultiSignature` is unchanged. [Pinned order verification](https://github.com/RaoFoundation/subtensor/blob/f867191868d29c5b28d3afdf13f967a12f0af128/pallets/limit-orders/src/lib.rs#L736-L767), [pinned signer filter](https://github.com/RaoFoundation/subtensor/blob/f867191868d29c5b28d3afdf13f967a12f0af128/runtime/src/lib.rs#L1490-L1501)

Two authorization methods are required:

1. **Account-approved orders:** a normal signed `authorize_order(order: VersionedOrder::Authorized { nonce, terms })` records the owner's canonical 64-byte approval commitment and reserves the order nonce. This works through direct ML-DSA, multisig, or a full-authority `Any` proxy. Approval grants later asset movement after the approving origin's filter is gone, so every restricted proxy type MUST deny this call, including through utility batches and nested proxies. Relaying an independently approved order does not grant permission to create that approval. Matchers use the original execution methods, sharing existing fill, price, balance, cancellation, expiry, replay, and atomicity logic.
2. **Detached ML-DSA orders:** the account registers a purpose-specific ML-DSA order-key commitment under a revocable authorization epoch. `SignedOrder` retains its original field order: `order`, `signature`, `partial_fill`. The local `OrderSignature` preserves the exact Ed25519/Sr25519/Ecdsa tags 0/1/2 and signature bytes, then adds `AccountApproved` at tag 3 and `MlDsa { epoch: u64, public_key: [u8; 1952], signature: BoundedVec<u8, 3309> }` at tag 4. These modes require `VersionedOrder::Authorized { nonce, terms }` at tag 2, whose nonrecursive `terms` enum contains the exact original V1/V2 payloads. Legacy signature modes cannot accompany authorized terms, and new modes cannot accompany plain legacy terms. The signature vector must contain exactly 3,309 bytes before verification. Its compact length prefix is part of the new order-proof encoding. Fixed arrays larger than 2,048 elements are forbidden in exposed runtime metadata because unmodified Polkadot.js rejects them while constructing a call decoder; the runtime regression checks all exported metadata versions. The owner is the existing order's signer field. The capability authorizes only the order APIs; it cannot sign account transactions or change account policy. Composite accounts install/revoke it through their normal threshold path.

Freeze these distinct order bindings, with no SCALE prefix around the ASCII domain and normal SCALE encoding for the following tuple:

```text
order_id = Blake2_256(
    ASCII("bittensor/ms/v2/order-id") || SCALE((genesis, owner, nonce_u64, order))
)

approval = SHAKE256(
    ASCII("bittensor/ms/v2/order-approval") || SCALE((genesis, owner, nonce_u64, order)),
    output_bytes = 64
)

order_transcript = ASCII("bittensor/ms/v2/order") ||
                   SCALE((genesis, owner, epoch_u64, nonce_u64, order))
order_context = ASCII("bittensor/ms/v2/order")
```

The 32-byte ID is bookkeeping, not the authorization commitment: execution must compare the complete 64-byte approval value. A detached proof signs the entire order transcript using pure ML-DSA-65. Both bindings include the order version tag and every economic term. The relayer chooses `partial_fill` only within the already signed order's partial-fill policy; it cannot alter the signed amount or enable partial fills. `OrderNonces[(owner, nonce)]` permanently binds a nonce to one ID. Status is retained in the existing order-status map; a fulfilled or cancelled order cannot execute again. Rotation of the owner's transaction key does not change these bindings.

SDK order preparation, authorization and both execution routes retain descriptor-bearing fee recipients until a matching immutable registration is verified at a finalized block. Only then may the descriptor be reduced to its AccountId for signing or dispatch. Missing, unfinalized or mismatched registrations fail before composition, including for classical orders and when ML-DSA registration is disabled. Existing finalized registrations remain usable while new registration is paused. Serialized intents must retain descriptors until this check; scoped capability grants apply the same rule to their fee recipient.

The original sequential `execute_orders(orders, should_fail)` and netted `execute_batched_orders(netuid, orders)` accept mixed legacy and authorized orders within the existing total batch bound. At most eight entries may use authorized terms or a new proof mode, including malformed combinations; this is checked before verification and mutation. Legacy-only batches retain their existing weights; mixed batches use weights parameterized by total and authenticated counts, with the total conservatively floored at eight to stay within the benchmark range. Sequential best-effort execution isolates each order in a transaction; `should_fail=true` rolls back the whole batch on an error. Netted execution shares the legacy collection, residual pool swap, distribution, fees, and linked-output logic. It validates every order, reserves its nonce transactionally, performs all fills, then consumes capability budgets. Aggregate budget failure rolls back every asset transfer, fee, reserve/share update, fill status, nonce, and capability counter. The original `cancel_order` accepts the same `Authorized { nonce, terms }` wrapper and permits cancelling a partially filled remainder. Plain legacy cancellation bytes and order IDs remain unchanged. Order transcript bindings below encode `terms` as its original V1/V2 order, with the nonce included exactly once.

Every authorize/register/revoke call derives its owner from the authenticated `Signed` origin, rejects any claimed-owner mismatch, and preserves proxy filters. A composite granting one detached order key deliberately delegates trading without a threshold approval for each order; the wallet must show that grant. Store and enforce its allowed assets/netuids, recipients, fee bounds, aggregate spend, and expiry, or require explicit authorization for unrestricted trading. Both execution routes must retain coldkey-swap announcement/dispute freezes, relayer restrictions, cancellation, partial-fill accounting, expiry, and balance checks. An authenticated account-approved order does not bypass these execution rules.

Do not tie long-lived orders solely to the current rotating transaction generation: ordinary transactions would invalidate outstanding orders. `CapabilityEpochs[(owner, role)]` is a permanent checked `u64` tombstone and `ActiveCapabilities` stores the current grant. Roles are explicitly `1=Order` and `2=Message`. Installation requires `expected_epoch = tombstone + 1`, a nonzero commitment, a valid scope, an already protected PQ owner, and an expiry strictly after the current block. Replacement and revocation advance the epoch; revocation at `u64::MAX` still removes the grant while issuance remains permanently exhausted. No deletion, reaping, or re-registration may reuse an epoch.

Grant expiry is an inclusive `u64` **block number**; existing order expiry remains milliseconds. A restricted trading scope names one netuid, hotkey, fee recipient, maximum fee per billion, per-order TAO/alpha limits, and lifetime TAO/alpha allowances. Buy spending counts gross TAO input including order fees; sell spending counts alpha input. Separate `u128` spent counters prevent mixing denominations or wrapping. A zero allowance disables that direction. `UnrestrictedTrading` requires an explicit owner grant. Message scope contains 1–128 audience bytes. Authorization requires the ML-DSA authorization flag; revocation remains callable while it is disabled. Check the current grant, scope, expiry, epoch, and fill/cancellation state at execution. An outer matcher's key scheme never substitutes for the owner's authorization.

Use the same explicit capability principle for durable message/HTTP signing when independence from transaction rotation is needed. Derive separate keys for transaction, order, and message roles. Bind distinct signing contexts and structured domain data; HTTP proofs include audience, method, path, body digest, challenge/nonce, expiry, and authorization epoch. Define current-authority verification separately from historical proof-at-finalized-block verification. Never accept the extrinsic carrier as a message signature or infer authority from the public key alone. Offline verification of historical identity needs a trusted state anchor/proof, not an untrusted wallet companion file.

The implemented message proof is exactly 5,336 bytes: `SCALE((2_u8, 4_u8, 2_u8, owner[32], genesis[32], epoch_u64, public_key[1952], signature[3309]))`. Its pure-mode context is `bittensor/ms/v2/message`; its transcript is `ASCII("bittensor/ms/v2/capability") || encoded_header_without_signature || application_payload`. Verification compares owner, genesis, role, epoch, and the full key commitment with independently resolved trusted state. Generic message signing rejects role 1; orders use the separate order transcript above.

Preserve `btauth/1` HTTP transport. `btauth/2` carries short version/owner/receiver/nonce/crypto headers and a binary body `ASCII("btauth/2") || 0x00 || proof[5336] || original_application_body`. The default application-body limit is 1 MiB, excluding the fixed 5,345-byte prefix. The server must enforce its total body limit before reading the request, verify the envelope, and pass only the original body to the application. This avoids the 10,522-character public-key/signature hex header.

The HTTP application payload is UTF-8 lines joined with a single LF and no trailing LF, in this order: `btauth/2`, `mldsa65`, lowercase genesis hex, canonical decimal epoch, lowercase audience hex, uppercase method, exact path including query, lowercase **SHAKE256-512 hex of the original application body**, canonical decimal nanosecond nonce, owner SS58, receiver SS58. Method, path, and receiver must be nonempty, exclude CR/LF, and fit 32, 8,192, and 128 encoded bytes respectively. The nonce is a checked `u64`; receiver binding is mandatory. Never hash the proof-bearing outer body or replace the 64-byte body digest with SHA-256.

Resolve the active message capability and its expiry at a consistent finalized block, using the server's configured audience. Refresh it per request or document and enforce a revocation-latency bound; an untrusted request or indefinitely cached wallet record is not registry authority. Verification checks a bounded freshness window (defaults: 10 seconds old, 2 seconds future skew), then records `(owner, nonce)` in a replay store whose retention covers that entire window. Validate actual server/reverse-proxy limits and application body handling before declaring deployment compatibility.

# Wallet derivation and SDK requirements

Generate new wallets from fresh 256-bit entropy encoded as an English 24-word BIP39 mnemonic. The exact mnemonic import is `master = BIP39.to_seed(passphrase)[0..32]`, where `to_seed` is the standard 64-byte BIP39 seed derivation, including its normalization and optional passphrase; an omitted passphrase is the empty string. It is **not** the mnemonic's raw entropy bytes or Substrate's `Pair::from_phrase` seed. Reject shorter mnemonic imports for this wallet type. A direct 32-byte seed import is interpreted as the master itself. Cross-language vectors must cover both forms and a nonempty passphrase. Do not reuse a classical direct-key seed as the master, and do not claim that expanding a shorter mnemonic creates more entropy.

Define the ML-DSA wallet derivation as:

```text
PRK = HKDF-Extract-SHA512(
    salt = ASCII("bittensor/ms/v2/derive"),
    IKM = master[32]
)

key_seed = HKDF-Expand-SHA512(
    PRK,
    info = SCALE((2_u8, profile_u8, role_u8, index_u64)),
    length = 32
)

profile 4 = rotating; profile 5 = fixed
role 0 = transaction key; index = generation for profile 4, always 0 for profile 5
role 1 = order capability key; index = authorization epoch
role 2 = message capability key; index = authorization epoch
```

Use that seed as the FIPS ML-DSA-65 KeyGen seed. Each key derives independently from the master; a current generation's secret must not reveal future keys. Capability registration must verify that the chosen key/epoch match the intended owner policy. The domain constants and role values are protocol commitments, not implementation conveniences. [HKDF specification](https://www.rfc-editor.org/rfc/rfc5869.html)

Use hedged signing by default with an operating-system CSPRNG; deterministic signing is permitted for interoperable vectors and supported offline devices. The verifier accepts both. Store seed-form private material only in the encrypted wallet, zeroize temporary secrets, and distinguish the master from a single exported generation seed. Restoring the master recovers the account; it does not recover security after master compromise. [NIST seed-format guidance](https://csrc.nist.gov/Projects/post-quantum-cryptography/faqs)

The canonical CLI separates signing scheme from rotation policy:

```bash
btcli wallet create --crypto-type sr
btcli wallet create --crypto-type ed
btcli wallet create --crypto-type ms
btcli wallet create --crypto-type sr --type hashed
btcli wallet create --crypto-type ed --type hashed
btcli wallet create --crypto-type ms --type hashed
```

The first three commands create fixed-key wallets; `--type hashed` selects rotating keys for every scheme. `--type standard` is the explicit fixed-key equivalent. Both coldkey and hotkey inherit this selection unless separately overridden. Creation, mnemonic/seed/private-key recovery, encrypted backups, public companion files, receiving addresses and signing must preserve the chosen scheme and mode.

Fixed ML-DSA uses SDK crypto identifier `8` and descriptor/header profile `5`; rotating ML-DSA keeps SDK identifier `7` and profile `4`. The descriptor includes the profile in AccountId derivation and the KDF includes it for every role. The same master therefore produces distinct identities and private keys in the two modes. Profile 4 derivation, existing files and receiving addresses are unchanged. To preserve existing experimental backups, legacy aliases `mldsa65`, `mldsa`, `ml-dsa-65`, and numeric `7` retain rotating behavior when mode is omitted; an explicit mode takes precedence. New aliases `ms` and `ml-dsa` default to fixed. Numeric `8` identifies fixed. Wallet `signingScheme` remains `mldsa65` for both; `accountType` records `standard` or `hashed` and must agree with the descriptor.

Fixed ML-DSA also requires descriptor registration and a `tao_` receiving address for first funding. It uses the same permanent AccountId32, protected-origin checks, PQ security floor, capability handling, original extrinsics, and full-payload v4 proof route. It must never fall back to a classical signature route. The immutable profile forbids downgrading an existing rotating account to fixed or rotating a fixed identity in place. Changing mode requires a separately created identity and explicitly authorized asset/ownership migration. Signing checks the finalized authorization generation even though a fixed wallet's transaction key is constant. Detached capability proofs retain cryptographic algorithm 4 and owner/role/epoch binding; their private derivation uses the owning wallet's profile.

Runtime metadata advertises `HashedAccounts.MldsaAccountProfiles = [4, 5]`. New fixed-profile clients require profile 5 support before registration/signing; older profile-4 runtimes fail closed. The Raw proof remains 5,367 bytes, descriptor 66 bytes, and outer `MultiAddress`, `MultiSignature`, signed-extension layout and call indices remain unchanged. Preserve old wallet-file versions and APIs. The versioned signer capability returns the complete ML-DSA authorization rather than forcing it through a 64/65-byte signature result.

Required SDK/CLI coverage includes creation/import/export/watch-only/restore; receiving descriptors and contacts; finalized-state generation recovery; fee estimation using the correct proof size; offline and hardware signing payloads; multisig/pinned-call tools; proxy and shield composition; native EVM flows; messages/HTTP; and clear unsupported-wallet errors. New wallets must verify chain support and activation before registration or signing. Publish compatible native-core and Python releases together, and provide TypeScript integration vectors and helpers.

Offline/device requests use the explicit format identifier `bittensor/ms/v2/extrinsic/1`. They carry the full native payload, separately encoded call/extensions/implicit fields, reviewed chain/runtime/era/fee data, and an authority snapshot. Before private-key use, signing requires an independently trusted codec and `expected_genesis_hash`; it reconstructs every payload component from the declared fields, checks runtime/transaction versions, confirms the descriptor and current derived commitment, and signs the full payload. Connection-level proof attachment/submission rechecks finalized authority, generation and nonce and verifies the complete returned proof. A request's included snapshot is not evidence of its own authenticity. Existing hardware wallets or Polkadot signer payload APIs are not claimed to implement this new protocol; devices must explicitly add it.

# Bounded verification and economics

Ship a portable deterministic `no_std` verifier in the runtime WASM first. It must require no new host import, OS randomness, signing code, network, clock, or floating point. Pin the audited implementation, features, dependency versions, and test-vector revision. A crate's existence or performance is not audit evidence.

The implementation candidate vendors `fips204` 0.4.6 at upstream commit `8d456d989dce681233ef963d3c3306da8f76d7c7`, enabling ML-DSA-65 only and sharing bounded verification arithmetic between external-message and locally reconstructed `mu` entry points. Its provenance and local semantic/formatting changes are recorded in `vendor/fips204/SUBTENSOR.md`. An independent audit of this release and patch has not been established. Neither vendor unit tests nor successful `wasm32-unknown-unknown` compilation substitutes for independent review or native/WASM execution comparison. Runtime payload hashing uses a counted streaming SCALE output with a 16 MiB payload ceiling; the chain's lower extrinsic/block limits still apply.

Key parsing may already expand ML-DSA's matrix; bound and account for that work before constructing a verifier, not just before `verify()`. Hashing, allocations, cold-key expansion, signature checks, database reads, and failure paths all belong in admission and benchmark accounting. Rotating keys require cold verification measurements. [Example verifier implementation](https://github.com/RustCrypto/signatures/blob/master/ml-dsa/src/verifying.rs)

Specify deterministic rejection-sampling ceilings and identical failure behavior in native and WASM execution. This profile caps each `RejNTTPoly` invocation at 894 XOF bytes and each complete `SampleInBall` invocation at 221 bytes, including its initial eight sign bytes; reject before requesting bytes 895 or 222 respectively. FIPS Appendix C's 121-trial bound concerns an individual inner rejection loop, not all trials across `SampleInBall`. Freeze byte accounting and boundary tests with the verifier. [FIPS 204 Appendix C](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)

On-chain weight does not itself prevent unpaid invalid proofs from exhausting transaction-pool RPC validation. Apply bounded validation concurrency, byte quotas, peer/RPC rate limits, and measured worst-case verification budgets before expensive checks. Any verification cache must be bounded and must never replace current-state/generation validation. Native acceleration is permitted only with identical acceptance semantics and safe deployment to every validating executor; unresolved WASM imports are not a usable fallback.

The node candidate wraps RPC, network, and local/offchain pool admission. Requests containing signed-v4 Raw addresses of the ML-DSA length share limits of 64 pending submissions, 64 MiB pending bytes, 10 MiB per extrinsic, and two outstanding ML-DSA authorizations. Mixed batches count every contained extrinsic against the count and byte limits; legacy-only requests retain the existing pool behavior and bypass the new pool admission budgets and detached tasks. At most one admitted attempt per ML-DSA owner may be outstanding. A global ML-DSA attempt bucket starts at 16 and replenishes one attempt per 125 ms; invalid proofs and caller cancellation do not refund attempts. Classification reads only a bounded prefix and charges any matching Raw length even for malformed profiles/carriers. Cancelled RPC requests retain their permits until the actual validation task completes.

A second shared guard at the Wasm executor boundary covers offchain runtime execution reached through raw `state_call`, chain-head/archive calls, dry-run, simulations, pool validation, and pool revalidation. It partitions ML-DSA transaction validation, EVM simulations, contracts simulations, and other execution (including unknown exports) into four independent budgets shared by executor clones. Each permits two simultaneous calls, at most 16 MiB input per call and 16 MiB active input per service, with a burst of 16 calls replenished at one per 125 ms. Quota is not borrowed between services: invalid requests to one service cannot consume another service's slots, byte budget or refill. Across four services the concurrency/input limits imply at most eight active executions and 64 MiB input. Calls within one service still share its quota; this is service isolation, not authenticated per-caller fairness. Synchronous execution holds its permit until Wasm returns, even after caller cancellation. Invalid calls consume rate credit. Explicitly reviewed read/conversion APIs, inherent construction/checking, and legacy transaction validation retain their existing behavior; unknown exports are guarded. Execution-capable exports such as `BlockBuilder_apply_extrinsic`, `Core_execute_block`, pending-block/EVM/contracts simulation, benchmarks, and try-runtime are guarded regardless of the outer signer, because a legacy relayer can dispatch detached ML-DSA order proofs. One execution call may contain multiple proofs; these limits do not imply two signatures globally or eight verifications per second. Executor failures propagate as local runtime failures, not consensus-invalid transactions or peer offences. Ethereum gas estimation and calls retry only the exact host-capacity error, asynchronously at 125 ms intervals, with a five-second retry deadline. An estimate shares that deadline across all binary-search probes; every attempt still passes executor admission. Reverts and other failures return immediately, and cancelling a waiting request prevents further retries.

Block authoring and import explicitly use the pinned SDK's `Onchain` execution context and bypass local admission. Aura/Babe inherent prechecks use `Offchain` but are explicitly exempt: they neither authenticate account signatures nor dispatch arbitrary calls. The current Drand offchain worker is also exempt because genuine import callbacks use `Offchain` and the hook performs no ML-DSA verification. The proposer's `ShieldApi_try_decode_shielded_tx` is a syntactic recognizer of signed outer carriers, not proof verification; even a malformed signature can be recognized, but Executive must accept the outer before the proposer decrypts or applies the inner. `ShieldApi_try_unshield_tx` performs the existing ML-KEM/AEAD decryption and bounded extrinsic decoding without authenticating a signature. Both proposer helpers are exempt so RPC overload cannot disrupt Shield selection; the generic pallet recognition helper retains signature verification for its existing direct callers.

The pinned `sc-transaction-pool` 40.1.0 has a narrow maintenance patch: indeterminate runtime-API errors, including local execution overload, retain already admitted entries without refreshing cached validity, expiry, or mempool validation height. This applies to single-state maintenance and both fork-aware maintenance paths. Fork-aware mempool batches order eligible entries by a separate last-attempt timestamp, so persistent errors cannot monopolize the 1,000-entry batch; that timestamp never delays retry eligibility or changes validity. Initial plain/watched submissions still fail on indeterminate errors but do not ban the hash. Failed fork-aware watch creation cleans up only that attempt's controller, preserving existing watchers after duplicate rejection. Single-state reorg candidates that encounter indeterminate fresh validation enter a separate raw retry queue, with independent count/byte caps equal to the configured ready-plus-future limits. They never enter ready/future without fresh successful validation. Each enactment retries at most 16 queued candidates with rotating order; each candidate expires after 60 seconds or 64 block heights from its first retraction, without deadline renewal. Explicit invalidity, expiry, or exhausted queue capacity discards candidates. Explicit invalid results still remove admitted entries and ban invalid initial submissions; existing retained-pool expiry/size rules remain active. Existing RPC message limits and the SDK's retained-pool/worker limits still apply outside these guards. These are conservative operator limits, not consensus rules or measured throughput; onchain execution, existing Shield decryption workload, peer fairness, output memory, and sustained overload require separate deployment tests and capacity measurements.

Benchmark registration, authorization, bad proofs, maximum calls, protected-authority checks, order batches, Shield, and EVM paths on approved reference hardware. Include both execution time and proof-size/storage effects. Use generated `WeightInfo`; fail closed until calibrated. Length fees cover actual wire bytes, authorization fees cover verification, and post-dispatch or EVM refunds must not erase already-consumed authentication cost. Preserve normal real-fee-payer, alpha-fee, tips, and multisig deposit semantics. Publish fee and throughput measurements rather than estimates presented as capacity proof.

Benchmark definitions must exercise full supported bounds, not just easy valid states: cold-key verification across the 10 MiB call range; a late-invalid proof; 100-member derived records at depth 16 and 1,024 expanded nodes; all 20 proxy delegates with 75 unrelated announcements each; a nonempty safe EVM alias balance swept during registration; a near-64-KiB sponsored consent envelope with the matching call last at the traversal limit; restricted order scopes, maximum encoded order fields, and eight genuine detached proofs sharing the actual execution paths. Protected child creation must begin from a valid depth-15, 1,023-node parent. Fixture construction occurs outside measurement, while policy reads, full decoding/hashing/verification, writes, and economic execution belong inside the relevant measured block. These fixtures and benchmark unit tests define coverage; they do not supply calibrated production weights.

# Implementation boundaries

The original APIs support every account profile. Finalized mainnet metadata at block `0x51acfdbaeff6cfea99318e35840dde8a7f995169df4d0f727f5023c7229ded2f` (spec 479) contains LimitOrders and MevShield but no HashedAccounts. Preserve deployed call indices and legacy order/ciphertext bytes. HashedAccounts' new selector is finalized before its first deployment. Reserved holes from the unreleased duplicate APIs are not reused.

| Pallet | Index | Call |
| --- | --- | --- |
| HashedAccounts | 0 / 1 | `register(descriptor)` / `check_registered(descriptor)`, both descriptor profiles |
| HashedAccounts | 4 / 5 | `register_derived(descriptor)` / `check_derived(descriptor)` |
| HashedAccounts | 7 / 8 | `install_capability(expected_epoch, key_commitment, expires_at, scope)` / `revoke_capability(role)` |
| HashedAccounts | 9 | `register_derived_sponsored(descriptor, deposit_payer)` |
| Proxy | 12 | `create_protected_pure(proxy_type, delay, index)` |
| Utility | 8 | `as_protected_derivative(index, call)` |
| MevShield | 1 | `submit_encrypted(ciphertext)`, bound 32,768 bytes |
| LimitOrders | 0 / 1 / 2 | Original `execute_orders`, `execute_batched_orders`, `cancel_order` |
| LimitOrders | 5 | `authorize_order(order)`, a new owner-approval capability |

The protected-child operations remain distinct because they require explicit consent to permanent authority/deposit policies. Cryptographic format versions, commitment domains, and existing V1/V2 order payload tags remain unchanged. The undeployed ML-DSA storage items use scheme-specific names. They are not duplicate dispatchable APIs.

Production ML-DSA, derived-account, authenticated-order and large Shield weights use approved reference measurements from run `39060946210`. The runtime rejects ML-DSA authorization whose worst valid/invalid verifier weight cannot fit the block, before expanding the key matrix. Nested dispatch retains normal weight accounting. The remaining formulas conservatively retain the measured cost of the removed ML-DSA subgate reads; cleanup does not invent lower weights.

`AdminUtils.sudo_set_hashed_accounts_enabled` is the sole activation control. Its `HashedAccountsEnabled` value defaults to false and gates registration and authentication of hashed SR/ED, fixed MS, and hashed MS accounts, including ML-DSA capability installation and use. Ordinary SR/ED signing is unaffected. Disabling it never removes account protection, alias binding, the direct-signature ban, or epoch/generation tombstones. Registration guards reject while it is off. Capability revocation remains available without granting new authority. Call index 6 remains unused; the surviving `AuthorizedFixed` event retains index 8.


| Area | Work |
| --- | --- |
| `primitives/hashed` | Separate ML-DSA descriptor/header/proof types, domains, exact codec, commitment functions, and vectors. Preserve v1 types. |
| `runtime/src/hashed_extrinsic.rs` | Bounded dual-version Raw decoding, canonical carrier, ML-DSA transcript and verifier, protected-account fallback rejection. |
| `runtime/src/hashed_auth.rs` | Wire-transparent authentication context, weights, permanent signer, validated tickets, rotation, unified security policies. |
| `pallets/hashed-accounts` | ML-DSA storage/registration/guards, permanent derived-authority records, activation controls, bounded policy queries. |
| `pallets/proxy`, `pallets/utility`, multisig integration | Protected derived origins, grant/use checks, pure lifecycle, unchanged ordinary derivation and filtering. |
| Subtensor ownership and coldkey paths | Security-floor checks for registration, owners, swaps, delegation, and recovery. |
| EVM/runtime/Frontier paths | Unified alias/protection queries, native origin and fee/refund integration, metering of added reads. |
| `pallets/shield` and proposer/client | Larger bound on the original carrier, tiered weights, full inner validation, correct two-transaction lifecycle. |
| `pallets/limit-orders` | Original execute/cancel APIs supporting legacy, account-approved, and detached ML-DSA orders. |
| Native core, Python, CLI, TypeScript | New wallet/signer capability, derivation, full operation composition, vectors, and client fixtures. |

Any new dispatchable needs benchmarks and `WeightInfo` wiring. Regenerate bindings, canonical-ABI-derived SDK artifacts when applicable, and generated documentation through repository generators. Preserve pinned dependencies and stop on unrelated lockfile drift. The outer extrinsic signature enum is unchanged; the local order authorization enum is extended with explicit metadata. Generated files are always regenerated.

# Acceptance tests and evidence

## Wire and client gate

Pin supported versions of unmodified Polkadot.js API/types and Apps, PAPI, Python substrate interfaces, Rust/Subxt and relevant indexers. Exercise supported metadata versions, standard metadata discovery, and runtime-upgrade boundaries. No custom signature definitions or decoder forks are allowed in the compatibility tests.

Construct finalized mixed blocks with ordinary Ed25519/Sr25519/ECDSA transactions, v1 hashed transactions, ML-DSA transactions at every position, inherents, and existing Ethereum transactions. Verify complete block decoding, exact extrinsic byte roundtrips and hashes, `isSigned`, signer identity/display, call arguments, extensions, events, storage queries, subscriptions, and subsequent ordinary wallet submissions. Repeat using historical metadata at pre-upgrade blocks and current metadata after activation.

Test truncation, overlong/underlong/noncanonical compact lengths, trailing bytes, invalid versions and algorithm tags, wrong carrier variant, zero carrier, malformed public keys/signatures, deep calls, memory exhaustion, and maximum payloads. Test fee APIs and simulation separately from real validation; a dummy proof used for quotation must never become execution authority.

**Initial codec evidence:** unmodified `@polkadot/types` 16.5.6 decoded the proposed 5,367-byte Raw envelope and nonzero carrier using metadata v14/v15/v16. It preserved bytes, hashes, extensions, signed status, and account prefix in 15 mixed-block insertion tests. All three zero-carrier cases failed call decoding. This used synthetic keys/signatures and an October 13 metadata fixture, not a new runtime or valid ML-DSA transaction. It establishes the carrier decision only; the exact-release client matrix remains mandatory. Fixture SHA-256: `041462b93d4916a4433e56367eeb3cbf52c573781840b90828870fc996621715`.

**Implementation codec evidence:** the runtime exporter checks a real ML-DSA transaction with the runtime verifier. Its signed envelopes were then decoded against metadata v14/v15/v16 read from the local release node, whose served Wasm exactly matches the compiled artifact. Unmodified `@polkadot/types` 16.5.6 passed 21 insertion cases: that transaction in each of seven positions among ordinary Sr25519/Ed25519/ECDSA, v1 Sr25519/Ed25519, and bare extrinsics. Fixture SHA-256: `f15f83394cd873f1a6539d724f45a9dfb19325812a7a7a9b0bc7166297a30c28`. Results are in `docs/design/evidence/ml-dsa-wire-codec-results.json`. This is local runtime/codec evidence against the uncommitted candidate, not a finalized block, released client matrix, or deployment approval.

All 32 SDK-composed ML-DSA-signed registration, guard, order, and Shield transactions (16 cases under each of the fixed and rotating profiles) also decode and re-encode byte-for-byte in both Polkadot.js and PAPI under each metadata version. Polkadot.js additionally roundtrips a 39-extrinsic block interleaving those transactions with all seven original fixtures. Both native bindings decode the 32 additional transactions under their supported metadata v14/v15. This exposed and fixed Polkadot.js's rejection of the originally proposed 3,309-element fixed order-signature array. The final bounded-vector representation is checked by an all-metadata regression. Separately, 29 legacy order/Shield call cases per metadata version preserve the exact finalized-mainnet encoding, including every classical signature variant and V1/V2 terms.

Additional checks against that same fixture passed with PAPI 1.23.3 and both the installed legacy `bittensor-core` 0.1.8 binding and the current native binding. PAPI's transaction-analysis path and full metadata-driven body roundtrips passed all 21 metadata-version/transaction cases. The legacy native binding has no ML-DSA key-creation support, but both native versions decode all seven extrinsics and all mixed-block positions with metadata v14/v15, preserving the Raw signer and carrier. Both already reject metadata v16 before decoding any extrinsic; this is an existing client limitation, not claimed support. Portable scripts, source/binary hashes, results, and reproduction commands are recorded in `docs/design/evidence/README.md`.

The accompanying `docs/design/evidence/` directory contains the portable experiment, compact result record, and instructions to export fresh runtime metadata. Historical fixture provenance and codec-only limitations are retained in that record.

## Cryptographic and lifecycle gate

Use pinned NIST vectors, independent implementation differential tests, and Rust/Python/TypeScript golden vectors for master derivation, descriptors, commitments, account IDs, pure-mode transcript, signature, and complete extrinsic bytes. Verify native/WASM agreement on valid and malformed cases. [NIST verification vectors](https://github.com/usnistgov/ACVP-Server/tree/master/gen-val/json-files/ML-DSA-sigVer-FIPS204)

Mutating any transcript field must invalidate the proof. Cover cross-chain, cross-version, cross-account, cross-role, old-generation and expired-era replays; same-generation races; forged next commitments; generation overflow; fee failure; failed dispatch; batch rollback boundaries; simulation; inclusion/finality/reorgs; zero-balance account recovery; and restoration from fresh wallet storage. Prove that invalid extrinsic application cannot leave rotation or fee state behind.

## Operation parity gate

Run the metadata-derived operation inventory against direct ML-DSA, ML-DSA acting through proxy, all-ML-DSA multisig, and mixed multisig where the origin policy permits. Compare business-state outcomes with classical controls, allowing only intentional authentication costs and security-floor differences. Verify inner events and balance/state changes rather than counting successful outer extrinsics.

Multisig tests must include separate blocks and rotations between approvals; 2-of-3 and higher thresholds; approval/execute/cancel by each scheme; unchanged account ID after rotation; pinned receiving guards; a price change between approvals; mixed security-floor examples; attempted direct signatures for protected composites; pending approvals at protection opt-in; and nested wrappers at allowed and rejected depth.

Security tests must show that classical/v1 delegates, unsafe coldkey owners, ECDSA associations, stale proxy announcements, legacy limit-order signatures, pure-proxy creation, derivative origins, and feature disabling cannot bypass a protected authority. Include calls to and from contracts and all dynamic EVM read/failure paths. Complete detached-order replay/revocation and Shield maximum-payload/failure tests.

# Local implementation validation

The implementation is on `codex/ml-dsa-accounts-v2`, based exactly on
`codex/hashed-accounts` at `f867191868d29c5b28d3afdf13f967a12f0af128`.
These results describe the local, uncommitted candidate on October 15, 2026;
they do not describe a deployed release.

| Check | Fixed/rotating wallet validation result |
| --- | --- |
| `SKIP_WASM_BUILD=1 cargo test --locked --offline --workspace --all-features` | 3,503 passed, zero failed, 47 ignored across 88 test-target summaries. A final 154-test runtime rerun passed after explicitly naming the new metadata profile constant; it includes both modes and mixed-mode operation regressions. |
| Full Python unit suite with the implementation's native extension | 2,844 passed, one skipped, including the six-command wallet matrix, recovery, immutable mode selection, old-runtime rejection, EVM routes, and message-verification mode binding. |
| Workspace Clippy, all targets, default and all features, `-D warnings` | Both passed. |
| `env -u SKIP_WASM_BUILD cargo build --locked --offline --release -p node-subtensor` | Passed, including the actual runtime Wasm build. |
| Production-profile Wasm with `try-runtime` | Built successfully; SHA-256 `f14681d75696dbed9c0e61d86a0ea182fc4162dd0b2fc8e6eba6e5570b5931d7`. |
| Finalized-mainnet upgrade replay | Passed all upgrade checks from spec 479 to 480 at block 9,290,353, with the snapshot's loaded state root matching the finalized header, spec-name/version checks enabled, and migrations verified idempotent. |
| Ten post-upgrade blocks | Local fast-forward passed with migrations and `--try-state all`; the saved evidence separately checks each block's signature/state-root checks and completion. This is simulated execution over mainnet state, not ten newly finalized mainnet blocks. |
| Release-node metadata versus generated SDK bindings | `codegen.check --drift` passed: no drift. |
| Live raw-runtime RPC admission | All 64 unknown-export calls failed; 48 received the local capacity error. The exempt `Core_version` read still succeeded. |
| Release Wasm identity | The local node's served `:code` exactly matched the built compressed Wasm, SHA-256 `3930cb27c6910a0d36d21759fc9e618301847291604715d112600b7d25dcabcb`. |
| Legacy reader compatibility | Polkadot.js/PAPI pass 32 consolidated transactions per metadata version plus the seven-extrinsic matrix; Polkadot.js also passes a 39-extrinsic interleaved block. Installed legacy native decoding passes all 32 additional transactions within v14/v15 support. |
| Deployed API compatibility | 29 legacy order/Shield cases retain identical mainnet bytes per metadata version; 16 new authorization-call cases decode with the installed native reader. |
| Operation coverage inventory | All 275 SDK-scoped dispatchables are classified against release metadata and source. Twenty-five map to explicit scoped regressions, 158 have generic origin-path reasoning, and 92 retain restricted/special origins or lack targeted mappings. This inventory is not a complete per-operation execution matrix. |
| Offline proof acceptance | Rotating zero/reused-next and both modes' exhausted-generation proofs reject in the SDK. Fixed proofs must preserve the initial commitment and still advance the authorization counter. The last usable generation remains accepted; mode-conflicting and unsupported-runtime proofs reject before signing. |
| Release native/Wasm shared-verifier differential | Twelve valid/malformed ML-DSA order-proof cases agreed on API return bytes, events, and the entire resulting storage root using the unchanged release Wasm. Isolated root unchecked dispatch reaches the verifier without changing production admission weights; this does not activate direct ML-DSA transactions. |
| Repository cheap gates | Formatting, whitespace, Rust path ownership, feature propagation, Ruff, binding coverage/names, beta baselines, and generated documentation all passed. |
| Complete preflight | Still fails the push-actor gate: the configured credential resolves to `UnArbosSix`, but repository policy requires `unarbos`. The compile-heavy checks above were run separately because preflight skips them after a cheap-gate failure. |

The six-command matrix, implementation diff identity, artifact/log hashes, and invariant coverage are recorded in `docs/design/evidence/ml-dsa-wallet-modes-results.json`.

A subsequent [live localnet execution report](evidence/ml-dsa-localnet-report.md)
adds finalized six-wallet blocks, real CLI operations and cross-validator Shield
testing. It also records the SDK corrections discovered during execution. These
functional tests use an explicitly isolated test-weight overlay; the production
Wasm hash above and its fail-closed weights remain unchanged. Earlier unit and
fixture results remain historical evidence for their recorded artifacts.

The reproducible live-node probe and its result are in
`docs/design/evidence/ml-dsa-node-admission.py` and
`docs/design/evidence/ml-dsa-node-admission-results.json`. The temporary local
node was stopped after validation. The runtime remains at spec version 480,
transaction version 1; neither a spec-version change nor activation was performed.
Production ML-DSA registration/authorization remain disabled, and uncalibrated new
weights fail closed. Reference weight generation is excluded from this validation
run at the owner's request. The shared pure-mode verifier now has exact-release
native/Wasm differential evidence. The direct transaction path rejects before
cryptography at its compiled `Weight::MAX` gate, so storage flags alone cannot
exercise that path or finalize an ML-DSA transaction on this artifact. Its
transaction-specific streaming transcript has native regressions, but its Wasm
differential and activated end-to-end workflows still require an artifact with
approved admission weights. Independent cryptographic review, finalized
mixed-block/UI coverage, physical signer integrations, and the remaining rollout
gates below are also unverified. The live RPC probe and mainnet-state simulation
do not establish those activation properties or production throughput.

# Rollout and completion criteria

1. Freeze this protocol's byte layout, domains, KDF, storage separation, authority rules, and positive/negative vectors after cryptographic and client review. Reconcile with the actual merge of PR #3220.
2. Implement runtime support with ML-DSA registration and use disabled by default. Preserve all v1 behavior. Keep separate subordinate switches for allowing new registrations and pausing ML-DSA authorization beneath the AdminUtils master gate; neither switch removes protection. Pausing existing users is an emergency action with an explicit operational recovery plan.
3. Complete wallet releases, operation parity, protected multisig/derived authority support, order integration, Shield capacity work, and client fixtures before advertising full support. Deploy verifier support before accepting ML-DSA transactions.
4. Complete independent crypto/security review and reference benchmarks. Run exact-release native/WASM, migration, mixed-block, historical decoding, and live localnet/testnet workflows. Published evidence identifies runtime/WASM hashes, SDK/client versions, vectors, operation inventory, measurements, and outstanding restrictions.
5. Run the mandatory repository preflight before every push, install/use the pre-push hook, and resolve required CI checks. The runtime spec-version change or exemption requires the owner's explicit instruction; benchmark labels and measurements follow the repository's hardware policy.
6. Activate only after the full acceptance gates pass. Never enable with placeholder weights. Preserve v1 and ordinary account support throughout. Once funded ML-DSA accounts exist, rollback must retain their state and fail-closed authorization protections; reverting to a runtime that treats them as ordinary classical keys is forbidden.

Completion means a valid ML-DSA transaction authenticates an ordinary permanent account, every supported account operation has proven behavior including multisig, no weaker authority bypass remains, and the declared unmodified client matrix decodes mixed blocks. Passing the initial codec experiment alone does not meet that definition.
