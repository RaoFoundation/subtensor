---
title: "ML-DSA authority and wallet audit follow-up"
description: "Conditional address-disclosure reachability, registration-ordering regressions, and a completed second source pass over wallet authority and recovery."
---

This follow-up reviews baseline `2d5089da3bcb292cbf557d9b22ff34582e23b3f4`
plus the local release-validation corrections. It re-evaluates the disputed
association finding and completes a second source pass over the authority and
wallet integration boundaries below. No additional production defect was
confirmed in this pass. The association condition remains reproducible; no
protocol change or removal of the owner guard was made.

## The address-disclosure condition

An attacker cannot feasibly guess a fresh, undisclosed descriptor-derived
AccountId. If the account is registered and finalized before disclosure, the
weak-owner registration trap does not apply. That part of the user's objection
is correct.

The finding does not require a hash preimage or the victim's descriptor/private
key. A bare native transfer includes its destination AccountId in the public
transaction. An attacker can read that destination after the transfer is already
included, associate it with an ordinary coldkey, and thereby prevent subsequent
MS registration. No race with the original payment is necessary. Public payment
requests and unshielded pending registration are other possible disclosure
sources, but the regression does not depend on either of them.

The revised real-signature Executive regression is
`disclosed_unregistered_ms_address_can_be_blocked_by_hostile_hotkey_association`
in `runtime/src/mldsa_dispatch_matrix_tests.rs:412`. Its explicit sequence is:

1. An ordinary ED sender transfers 100 TAO to the unregistered bare AccountId.
2. The attacker decodes that included transaction's public bytes and extracts
   only the destination. A separate SR signature claims it through
   `try_associate_hotkey`; no victim proof or descriptor is supplied.
3. The ED sponsor's descriptor registration fails with
   `MlDsaHotkeyRequiresPqOwner`.
4. A genuine initial MS proof cannot spend the balance because authorization
   requires an existing registry record. The 100 TAO remains at the same native
   AccountId; it is not stolen or destroyed. Victim-controlled recovery through
   the current ordinary wallet path is unavailable while the relationship stays.

This is an activation-gated denial of setup and recovery for **disclosed,
unregistered, bare-funded addresses**, not a compromise of an undisclosed hash,
a registered MS signature, or every receiving workflow. A hostile same-security
hotkey association can also affect ordinary hotkey setup, but it does not create
this weaker-owner registration rejection; that separate legacy ownership policy
is not being claimed as a new MS cryptographic defect.

The critical implementation points are
`pallets/subtensor/src/macros/dispatches.rs:1701`,
`pallets/subtensor/src/staking/helpers.rs:132`,
`runtime/src/hashed_auth.rs:315`, and
`pallets/hashed-accounts/src/mldsa.rs:409`.
The classical-hashed guard at `runtime/src/hashed_auth.rs:604` establishes the
same static registration condition for hashed SR/ED. These new Executive tests
exercise MS; they do not newly claim executed hashed-SR/ED attack coverage.

## Safe ordering and atomicity

Two new Executive regressions establish the opposing cases using the fixed and
rotating profiles:

- `registered_ms_address_rejects_hostile_weak_owner_and_remains_spendable`:
  registration precedes attack, weak association returns
  `HotkeyOwnerPolicyViolation`, registry and Owner state remain unchanged, and a
  genuine MS transfer succeeds and consumes exactly one generation.
- `atomic_ms_registration_payment_does_not_fund_a_hostile_association`:
  a weak relationship exists first; `Utility.batch_all(register, transfer)`
  fails, recipient balance stays zero, no registration deposit remains reserved,
  and neither protected identity nor EVM alias is installed. Normal inclusion
  fees remain payable by the sender.

The SDK retains descriptors in eligible direct payment calls and uses this
atomic registration/payment pattern. Nonbatchable receiving paths and order fee
recipients require matching finalized registration. A bare SS58 address contains
only an AccountId, so the SDK cannot recover its undisclosed descriptor or detect
its intended scheme. The existing receiving round-trip unit test proves that a
public-only descriptor suffices to derive its AccountId without a secret key.

## Second source pass

| Boundary | Revalidated invariant and conclusion |
| --- | --- |
| Decoding and signature carrier | `runtime/src/hashed_extrinsic.rs:79-145,225-295`: exact bounded raw-address lengths, standard signed-v4 frame, exact nonzero carrier marker, full payload including implicit chain/runtime values; authority comes from the signer bytes and current registry. No ordinary-signature bypass identified. |
| Replay, nonce and rollback | `runtime/src/hashed_auth.rs:188-273` and `hashed_extrinsic.rs:328-356`: prepare rechecks generation/commitment; included inner dispatch failure consumes nonce/generation, validity/preparation error rolls back authorization. Shared pool tags prevent concurrent use of one generation. |
| Registration and sponsor | `pallets/hashed-accounts/src/lib.rs:346-368`, `mldsa.rs:197-238`: target is derived from a supported descriptor, sponsor obtains no signing authority, idempotence does not reset state, and reserve/alias/registry effects remain transactional. Weak existing ownership is deliberately rejected. |
| Derived authority | `mldsa.rs:249-402`, `runtime/src/hashed_auth.rs:357-528,674-711`: canonical existing account derivation, sorted unique members, every quorum intersects a permanently PQ-protected member, immutable dependency bounds/cycle checks, separate owner and deposit consent. No unauthorized sponsor or weak-quorum path found. |
| Proxy and utility | `pallets/proxy/src/lib.rs:1050-1093,1204-1265`, `runtime/src/proxy_filters/mod.rs:52-144`, `pallets/utility/src/lib.rs:298-310,634-673`: grant/use retain authority floor, nested proxy filters intersect, restricted grants cannot acquire Any or dispatch EVM/multisig/deferred authority; protected derivative setup retains inherited filters. |
| Contracts compared with EVM | `chain-extensions/src/lib.rs:1036-1038`: native chain-extension origin is the contract's address, not its caller/principal. Therefore permitted contract calls do not recreate the unrestricted principal origin that required excluding EVM from restricted proxies. |
| EVM identity | `runtime/src/evm_origin.rs:28-169`, `runtime/src/hashed_evm_runner.rs`: full AccountId owns alias permanently; independent EVM nonce reservation prevents CREATE/CREATE2 collision; ordinary Ethereum and EIP-7702 cannot authorize a protected alias. Native EVM operations retain MS authority. Raw Ethereum RLP intentionally remains secp256k1. |
| Capabilities and orders | `pallets/hashed-accounts/src/capabilities.rs`, `pallets/limit-orders/src/authorization.rs`, `runtime/src/lib.rs:1512-1584`: owner-issued role/epoch/key/expiry/scope binding, permanent epoch tombstones, independent TAO/alpha budgets, complete order terms and nonce binding, strict mode separation, transactional aggregate budget consumption. No spending-key substitution or replay path found. |
| Receiving and nested calls | `sdk/python/bittensor/receiving.py`, `hashed.py:377-541`, `executor.py:194-260`, `limit_orders.py:67-108`: descriptors retained until atomic setup/payment or a matching finalized check; nested wrappers retain guards and pinned call bytes; unsupported first-use composition fails before signing. Bare AccountIds remain inherently untyped. |
| Online signing and Shield | `_transport/interface.py:599-780`: consistent authority snapshot, descriptor/current commitment validation, explicit nonce preservation, no private signing ahead of finalized MS state, current/current+1 generation pairing for Shield. Concurrent stale signing is rejected rather than advancing local key state. |
| Offline and device signing | `mldsa_signing.py`: full unshortened payload, trusted genesis and codec reconstruction, wallet/descriptor/commitment checks, proof verification before assembly and current-authority revalidation on submission. Legacy device protocols are not silently treated as ML-DSA-capable. |
| Wallet backup/recovery | `sdk/bittensor-core/src/keys/mldsa.rs`, `keyfiles/mod.rs:304-400`, `sdk/python/bittensor/keyfiles.py:128-177`: profile/role/generation-separated deterministic seeds; public views omit master seed; import rederives and matches descriptor/account fields; complete atomic publication prevents short-write truncation. No persistent mutable generation counter is required for recovery. |
| HTTP/message capability | `sdk/python/bittensor/mldsa.py`, `http_auth.py:133-272`: freshly resolved external authority, audience/receiver/chain/epoch/method/path/body binding, bounded binary proofs and post-verification nonce storage. Applications must actually refresh capability state and use a shared replay store across workers. |

The complete reviewed-source hashes and fresh command results are recorded in
`authority-followup-results.json`. The final runtime run passed **3 tests**,
including the strengthened public-transfer-first attack and both opposing safe
cases; each runs the fixed, rotating and mixed profile configurations. The
focused SDK run passed **516 tests**.
Earlier release evidence remains in the `runtime/`, `sdk/`, `crypto/` and `local/`
packages, including the 53 accepted live cases, full finite call-shape matrix,
backup fault injection, cryptographic vectors and pinned mainnet-state rehearsal.
This follow-up did not start another chain or repeat those unchanged live suites.

## Audit completion and limits

This completes the local source-level authority/wallet audit for the declared
scope, combined with the primitive review in the separate crypto follow-up.
No new confirmed authentication bypass or wallet-recovery defect was found beyond
the conditional registration finding. It is not a mathematical proof of absence
of all bugs, a claim that every business state ran successfully, an external
professional audit, or a FIPS certification.

The master switch retains its specified authentication/registration/capability
pause semantics; previously approved orders or mixed authorities can still
complete through an ordinary relayer. A global emergency freeze would require a
separate policy decision. RPC workload repair is owned by the parallel reviewer
and must be evaluated from its final code/results rather than this snapshot.

No commits, pushes, production state changes, measured weights, spec-version
changes or lockfile changes were made by this reviewer. The original owner guard
remains intact; weakening it would reintroduce alternative classical authority.
