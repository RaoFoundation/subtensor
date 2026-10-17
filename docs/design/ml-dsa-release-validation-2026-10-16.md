---
title: "ML-DSA local release validation, October 16, 2026"
description: "Pinned cryptography, runtime, SDK, live-chain and mainnet-rehearsal evidence, with unresolved release blockers and explicit coverage limits."
---

The candidate is **not ready for mainnet activation, even excluding weights**.
Local validation confirmed a conditional pre-registration fund-recovery failure.
The pending Ethereum RPC availability and simulation defects are repaired locally
and passed the corrected production-node comparison. The additional local
cryptographic and wallet-authority audits are complete; independently implemented
ML-DSA interoperability checks also passed. Passing tests do not supersede a
confirmed runtime finding or constitute an external certification.

Validation includes **53 accepted feature live cases**, additional production-node
pending RPC/transfer/contract regressions, full local preflights, cryptographic
checks, block-codec checks and a pinned mainnet-state rehearsal.
The adjacent evidence directory preserves outcome counts, artifact hashes,
superseded fixture failures and shutdown records.

All validators from the original five localnets and the subsequent RPC test
launches are stopped. The final process scan found no `node-subtensor` or localnet
launcher, and all **39 checked ports** were closed. Changes remain local and
uncommitted.

## Candidate and execution boundaries

- Baseline commit: `2d5089da3bcb292cbf557d9b22ff34582e23b3f4` on
  `codex/ml-dsa-accounts-v2`, plus the local corrections and tests described here.
- PR base: `b2ebaeef436b1b5288da066bf9d0a9244fdb1908`.
- No commit, push, production transaction, activation, spec-version change,
  reference benchmark or production weight edit was performed for this run.
- Five isolated three-validator localnets used normal 12-second slots and archive
  state. Distinct genesis hashes and peer namespaces prevented cross-chain peers.
- Production default flags and `Weight::MAX` activation gates were tested first.
  Live activated business tests necessarily used a separately built test Wasm
  replacing the five relevant placeholder weight implementations with existing
  development weights. This is functional evidence, not throughput, resource
  safety or production activation evidence. The normal timing and production
  7,200-block EVM association cooldown were preserved.
- The full workspace all-features tests include development-only features. A
  separate default-feature runtime test establishes the production cooldown;
  all-features tests alone cannot establish it.
- Live fixtures use supported root settings for disposable balances and subnet
  setup. The coldkey migration fixture sets its delay to two real blocks, and
  the validator fixture uses a six-block subnet tempo. These prove the state
  transitions with normal block timing, not that the default mainnet waiting
  durations elapsed. No validator permit or revealed commitment is fabricated.

## Unresolved release blockers

### High: hostile association can prevent registration and trap bare-funded TAO

`SubtensorModule.try_associate_hotkey` allows a signed caller to associate an
arbitrary, previously unassociated hotkey AccountId with itself. An unregistered
descriptor-derived identity is not yet protected. An attacker therefore needs
neither the victim's private key nor a subnet registration burn to write the
victim's `Owner` relationship.

This requires the address to be disclosed before registration. An attacker
cannot feasibly guess an undisclosed descriptor-derived address; registration
finalized before publication prevents the weak-owner trap. The strengthened
regression first includes an ordinary friend's bare transfer, then lets the
attacker decode its public destination and associate that exact AccountId.
Neither a preimage guess nor a race with the payment is necessary.

Descriptor registration subsequently rejects that weak owner. The genuine MS
owner cannot authenticate before registration, so native funds sent directly to
the bare AccountId cannot be spent through the ordinary wallet path. The victim
has no unilateral owner-removal operation. Attacker cooperation or privileged
remediation can clear the relationship; that is not wallet-controlled recovery.
The executable regression demonstrates this for both MS profiles. Static tracing
establishes the same registration guard for hashed SR and ED.

Atomic descriptor registration plus payment prevents a sender adding funds after
this setup failure. It does not prevent disclosure/front-running denial of setup,
nor recover a balance already sent to the bare address. Register and finalize
before publishing the account until a safe protocol remedy exists.

Do not simply remove the guard: weak-owner consumers still have authority over
hotkey relationships. A safe remedy requires authenticated recovery or a complete
policy for quarantining those existing privileges. No new recovery protocol was
introduced during this validation.

Source: [hotkey association](../../pallets/subtensor/src/macros/dispatches.rs),
[owner updates](../../pallets/subtensor/src/staking/helpers.rs) and
[registration guards](../../runtime/src/hashed_auth.rs).
The [Executive regression](../../runtime/src/mldsa_dispatch_matrix_tests.rs)
`disclosed_unregistered_ms_address_can_be_blocked_by_hostile_hotkey_association`
reproduces the failure with real signatures.
Two converse regressions prove that registering first prevents the weak
association and preserves MS spending, and that atomic registration/payment
does not fund a poisoned account or leave a registration deposit reserved.
All three passed, along with 516 focused SDK tests. See the completed
[authority follow-up](evidence/ml-dsa-release-2026-10-16/authority-followup.md).

### Pre-existing: Ethereum receipt fees can overstate the actual debit

The additional ordinary-Ethereum test confirmed a separate existing RPC reporting
error. The runtime explicitly discards the priority fee and charges the block's
base fee, while legacy receipts report the submitted gas price. With a submitted
10-gwei price and 5-gwei base fee, 32 finalized transfers charged 4,260,000 rao
but their receipt fields imply 8,520,000 rao. Archived receipts, native balances
and canonical block fees establish the discrepancy.

This policy and receipt implementation predate the MS branch. Funds were not
unexpectedly debited: actual balances match the runtime's base-fee policy.
Receipt-based accounting is nevertheless inaccurate. It remains an explicit
unfixed observation outside the pending-availability repair; a successful
balance/nonce regression is not a passing receipt-accuracy test. See the
[fee observation](evidence/ml-dsa-release-2026-10-16/rpc-followup/receipt-fee-observation.md).

## Pending Ethereum RPC repair

Trusted pending reconstruction now has a host-only capability and a separate
reservation split from the former Other quota. Unknown raw exports cannot obtain
that capability by naming a pending method. Total concurrency, active input,
burst and refill ceilings are unchanged. Failed capacity attempts roll back and
retry the same step under one bounded deadline, retaining earlier work. Pending
call and gas estimation use that retained overlay with transactional simulation
rollback; code/storage getters propagate failures.

The corrected before-and-after production-node comparison passed with normal
12-second blocks and the MS master switch off. On the baseline, a valid pending
value call cannot see pending funding, malformed raw calls block pending reads,
and a 32-entry native transfer pool exhausts reconstruction capacity. The repaired
node returns the complete pending balance, successfully simulates the transfer,
remains available under the unrelated flood, and includes all 32 transfers. Their
32-TAO native balance was subsequently verified in finalized state. A separate
32-transaction Ethereum run proved pending nonce/value before the next block and
exact finalized native/EVM balances under the existing fee policy. Real contract
tests passed creation, storage/revert rollback, EIP-1559 field handling, and
access-list warming; the temporary creator allowlist was restored.

Each pending job has a cooperative 15-second deadline, 256-entry/8-MiB snapshot
cap and shared attempt budget; at most two jobs hold leases. Same-service overload
can still reject explicitly. Nonempty state overrides with `pending` now reject
explicitly rather than simulate against the wrong state. Latest/historical
overrides retain their original behavior. These are documented compatibility
and resource boundaries, not unlimited-load guarantees.

The first 1-rao simulation fixture was below the recipient existential deposit
and could not establish an overlay defect. Its failure is preserved and excluded
from accepted evidence. The corrected fixture uses 1,000 rao, and separately
checks the minimum-balance failure after funding finalizes.

See the [RPC follow-up](evidence/ml-dsa-release-2026-10-16/rpc-followup/README.md)
for exact source/artifact hashes, the corrected comparison and additional
Ethereum transaction/contract regressions.

## Local cryptographic audit completed; external assurance remains distinct

The requested local source audit is complete. An additional independently
implemented reference, pinned liboqs/mldsa-native with portable C and AVX2
backends, passed bidirectional verification and exact deterministic comparisons
against the production Rust primitive. It also verified SDK derivation and
transaction, order and message-capability transcripts. No new exploitable
cryptographic authorization defect was established. The current NIST errata
required no algorithm correction.

The [completed audit](evidence/ml-dsa-release-2026-10-16/crypto/followup/audit.md)
records 128 primitive cases, 5,261 individual byte mutations, 49 malformed-hint
cases, message/context limits, 16 wallet identities and 128 generations. It
distinguishes independent implementation evidence from a separately commissioned
professional review. `vendor/fips204/SUBTENSOR.md` and the feature specification
retain an independent security-audit requirement; no external review or FIPS
validation/certification is claimed. Universal side-channel resistance and
complete process-memory erasure are not established by these tests.

## Corrections made locally

| Correction | Failure prevented | Regression evidence |
| --- | --- | --- |
| Preserve explicit nonce in hashed signing | Shield inner-envelope fee quotes used nonce n instead of requested n+1 at SCALE compact boundaries | 48 cases covering four schemes, three nonce boundaries and four ciphertext sizes; old implementation fails |
| Atomic complete keyfile writes | A short `os.write` could replace a valid wallet backup with truncated data | Short/zero writes, write/fsync/replace failure, concurrency and symlink tests; original backup preserved before publication, complete recoverable new file after publication |
| Reserve independent EVM alias nonce | New protected alias remained eligible for CREATE/CREATE2 collision because only its native owner nonce was reserved | Synthetic CREATE2 collision, no installed code after protection, reaping and pause invariants |
| Isolate and preserve pending RPC execution | Malformed exports starved pending work; capacity errors could omit work or lose progress; call/estimate discarded pending funding | Host admission/budget tests plus corrected production-node flood, 32-transfer and pending-simulation comparisons |
| Validate faucet work length | Development faucet converted an unconstrained vector to H256 and panicked | 20 Executive cases across SR/ED and both MS profiles; default production does not expose faucet |

These changes do not weaken authority floors, discard receiving descriptors,
reset signing generations, add new signature enum variants or enable production
activation. Alias nonce reservation adds storage work for the separately required
reference-weight process.

## Cryptography and wallet storage

The primitive validation performed 4,838 checks: 25 NIST key-generation vectors,
20 signature-generation vectors, 15 signature-verification vectors, 128 upstream
cross-check cases, 3,309 single-byte signature mutations, 1,280 selected mutations
and 61 malformed-hint cases. All 35 vendored primitive tests passed.

The review traced pure-mode domain separation and context encoding, shared-mu
verification, canonical signature/hint decoding, bounded samplers, descriptor
and commitment hashes, seed derivation, generation-specific keys, detached role
capabilities, chain/payload/nonce binding and zeroization. It compared the pinned
upstream source with the local patch; no new primitive authentication failure was
found. There were 218 focused crypto/recovery Python tests and 38 encrypted-backup
recovery checks spanning both MS profiles and generations through `2^64 - 2`.

The keyfile publication test also injects a directory-only fsync failure after
atomic publication: the error propagates, and the complete new MS wallet remains
recoverable. This is distinct from failures before publication, which preserve
the old wallet. A failed directory sync cannot establish crash durability.

The complete SDK unit suite passed **3,115 tests**, with one legacy-encryption
fixture skipped. This evidence does not prove side-channel resistance on every
platform, hardware-wallet interoperability or correctness of every filesystem.
The final default-feature production Wasm also passed 12 native/Wasm differential
cases covering valid signatures, changed messages/contexts and malformed proofs.

The full local preflight passed, including both Clippy configurations, the release
node build, generated bindings/docs and formatting checks. Its workspace run
reported **3,568 passed, zero failed and 47 ignored** across unit/doc-test targets.
Ignored cases include existing documentation examples, obsolete economics tests,
long mathematical sweeps, plot generation and a physical OpenCL GPU test; they
are not counted as passes. The standalone mainnet rehearsal below was additional
to preflight, which skipped migration execution because the local fixes do not
change migration code.

After the RPC repair, the full preflight ran again on every local correction:
**2,277 passed, zero failed and 19 ignored** across 24 targets. Both Clippy
configurations, release node build and metadata drift checks passed. These totals
overlap the previous PR-wide run; they are not added together. Final-source hashes
match the tested snapshot, and the production runtime Wasm is unchanged by the
node-only RPC repair.

## Runtime and operation coverage

The metadata-driven Executive matrix covers all **326** dispatchable variants:
**324 constructible calls × four signature modes = 1,296 cases**. The two GRANDPA
equivocation variants use uninhabited `sp_core::Void` proofs. It constructs real
SR, ED, fixed-MS and rotating-MS envelopes, checks pool acceptance, applies through
Executive, and verifies nonce/generation effects. Eight exact differences are
intentional proxy/derived-account/capability policies and are explicitly asserted.
Ethereum's self-contained transaction path correctly rejects all four native
signed wrappers.

This proves complete finite call-shape/envelope coverage. A root-only error or a
business-precondition error is not successful execution of that operation.
Separately, 67 focused hashed-runtime tests passed, and the default-feature test
proved the exact production EVM association boundary for all six wallet types:
7,199 rejects, pending inclusion 7,200 admits, actual block 7,200 succeeds with real
EIP-191 proofs and correct indexes.

All **53 unique live cases** passed across these suites:

| Suite | Cases | Successful paths and checked failures |
| --- | ---: | --- |
| Release safety | 17 | Late registration, failed-payment rollback/recovery, master/subgate pause/resume, Sudo, chain/payload proof binding and Shield nonce/tier boundaries |
| Operation families | 8 | Both MS profiles: staking, children, identity, serving, locks/collateral, hotkey replacement, utility/preimages, crowdloans and contracts |
| CLI parity | 6 | Every wallet mode, all 36 sender/recipient combinations, neuron/subnet registration and owner controls |
| Wallet lifecycle | 6 | Creation/recovery, capabilities, native EVM, proxy, root staking and coldkey migration |
| Shield lifecycle | 6 | Encrypted transaction execution for every wallet mode |
| MS authority and orders | 8 | Offline proofs, replay rejection, capabilities, EVM call/create/create2, protected derivatives/pure proxies, mixed/all-PQ multisig quorums, orders/revocation/budgets |
| Validators and commitments | 2 | Both MS profiles: Yuma and stake-selected Null weights, plain/timelocked metadata, automatically revealed encrypted weights and legacy salt commit/reveal |

Every retained release proof rejection was independently checked for an exact
bad-signature error. Separate read-only checks verified 135 root-suite receipts
against canonical finalized blocks, and 48 validator receipts including the
preserved first attempt. MS replay cases record the explicit pool ban separately
from malformed-signature rejection. Earlier fixture failures and canceled runs
are retained but are not counted as full-suite passes.

Receipt and run provenance are in the adjacent local, crypto, SDK and runtime
packages. A listed family is not a claim that every argument, business state,
economic history or external-client version was exercised. For example, these
runs do not exhaust all emission histories, basket payouts, lease settlement,
contract chain extensions or delayed-proxy compositions.

The master switch is currently an authentication/registration/capability-use
pause. Previously approved order or mixed authority may still complete through
an ordinary relayer. It is not a global freeze of every effect on a protected
account. No policy change was made without resolving that distinction.

## Block decoding and API compatibility

A newly finalized local block contains all six wallet envelope modes and 44
extrinsics in total. Polkadot.js 16.5.6 and PAPI round-tripped all 44 using metadata
V14, V15 and V16. Both the current native SDK and the installed legacy binding
decoded V14 and V15 and explicitly rejected unsupported V16. Only Polkadot.js
and PAPI established V16 decoding in this run. This is codec execution evidence,
not an interactive test of every application UI.

Against freshly queried mainnet metadata, 29 legacy call encodings were unchanged
and 16 new calls decoded, with no duplicate APIs in V14/V15. These tests preserve
the legacy signed-v4 carrier and do not add an ML-DSA MultiSignature variant that
would break generic block readers.

## Mainnet state rehearsal

The read-only snapshot is pinned to finalized mainnet block **9,298,128**:

- Block: `0x39bc186855bfbf78b4ab9f44138094ea3b19fa64a079161bf617397830aadc1c`
- State root: `0xe6a92abe52fb94bfe734d795e997025c5a390b20160ee69696a1bc6b5058fb06`
- Source runtime: 479; candidate runtime: 480.

The logged acquisition includes **3,304,807 unique top-level keys and 56 child
tries**. A repeated download had the same pinned storage root but a different
serialized snapshot hash, so the complete strict runner was repeated against
that exact logged snapshot. The unchanged full log checker passed. Both
acquisitions and their separate hashes are retained in the mainnet evidence.

The strict local upgrade succeeded with all checks and an idempotent second
migration. Ten contiguous synthetic empty post-upgrade blocks, 9,298,129 through
9,298,138, passed local fast-forward execution with state-root checking, signature
checking, try-state All and full state decoding. This is not replay of the
canonical mainnet transaction bodies. Snapshot acquisition and candidate hashes
are preserved with the evidence. The upgrade runner suppresses weight warnings
and multi-block migration weight checks under the excluded weight scope. This
rehearsal does not simulate future network load, upgrade governance or all
production-state histories.

## Reproduction and interpretation

Durable evidence is under
[`evidence/ml-dsa-release-2026-10-16/`](evidence/ml-dsa-release-2026-10-16/).
Large snapshots, binaries, archive databases and private temporary wallet files
are intentionally not included. The manifest records the retained public
evidence and exact artifact hashes. The new regression tests are part of the
local source diff.

- [Local tests, compatibility and preflight](evidence/ml-dsa-release-2026-10-16/local/README.md)
- [Cryptography and recovery](evidence/ml-dsa-release-2026-10-16/crypto/review.md)
- [Completed crypto audit and independent C reference](evidence/ml-dsa-release-2026-10-16/crypto/followup/audit.md)
- [Completed wallet-authority audit and address-disclosure boundary](evidence/ml-dsa-release-2026-10-16/authority-followup.md)
- [Wallet lifecycle and Shield](evidence/ml-dsa-release-2026-10-16/crypto/lifecycle-review.md)
- [CLI parity, MS authority and orders](evidence/ml-dsa-release-2026-10-16/sdk/README.md)
- [Runtime review and call matrix](evidence/ml-dsa-release-2026-10-16/runtime/README.md)
- [Validator commitments and reveals](evidence/ml-dsa-release-2026-10-16/runtime/validator-live/README.md)
- [Pinned mainnet rehearsal](evidence/ml-dsa-release-2026-10-16/runtime/mainnet/README.md)
- [RPC isolation and pending simulation repair](evidence/ml-dsa-release-2026-10-16/rpc-followup/README.md)
- [Original five-localnet shutdown verification](evidence/ml-dsa-release-2026-10-16/local/all-localnets-shutdown.json)
- [Final follow-up shutdown verification](evidence/ml-dsa-release-2026-10-16/rpc-followup/shutdown.json)

Local preflight, live test results, static review, cryptographic vectors and a
mainnet state rehearsal are different evidence classes. None can establish that
users can never lose funds. The disclosed, unregistered, bare-funded account
case is a concrete counterexample. Mainnet activation must remain **blocked**
until the remaining runtime defect and applicable external assurance requirements
are resolved; completing the local audit does not erase its findings.
