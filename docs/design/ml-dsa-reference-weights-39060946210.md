---
title: "ML-DSA reference weights and transaction fees"
description: "Selective reference-weight import from benchmark run 39060946210 with validated native fees in TAO and USD."
---

Retained **16 measured weight methods across five pallets**, using [benchmark run 39060946210](https://github.com/RaoFoundation/subtensor/actions/runs/39060946210/job/117478623351?pr=3259). All 11 benchmark pallets completed. The action exited 2 because it found drift and produced a patch; there was no failed measurement. The artifact SHA-256 is `5aa10aa331f894533d29c71c2191b36d4d2394ffba717c69d2e60975fe3baad5`.

The benchmark measured commit `437053f80d5ebebce950602b168901bfe6591483` on AMD EPYC 9V45 reference hardware with compiled Wasm, 50 steps and 20 repeats. It was applied locally to `8d6104b58aa5df1f53011cf8fbc81ac6b86f37ec`; the intervening commit contains tests/evidence and no runtime, pallet, primitive or benchmark-source changes. No commit or push was made.

Only the nine ML-DSA/derived/capability methods in HashedAccounts, four authenticated-order methods, protected pure proxy, protected derivative, and large Shield carrier were selected. Generic and RocksDB fallback implementations retain the exact measured formulas, database counts and proof sizes. An independent structural comparison checked all **156 trait/implementation methods**, verifying all selected formulas and preserving all unrelated functions. The rest of the action's broad drift patch was not applied.

The subsequent activation cleanup removed the redundant ML-DSA setter, its two flags, event, benchmark and weight method. AdminUtils is now the single activation control. The remaining 16 methods retain their measured formulas, including the conservative cost of the removed flag reads. The table omits the deleted setter; the original benchmark artifact and validation snapshots below record the earlier 17-method import. They remain historical evidence, not validation of subsequent edits.

**Fee chart**

USD uses **1 TAO = $271.935**, fetched from [Coinbase spot](https://api.coinbase.com/v2/prices/TAO-USD/spot) at **2026-10-17 14:04:08 PDT**. These are pre-dispatch inclusion quotes from the actual production runtime, including base, encoded-length, call and transaction-extension fees. Assumptions: nonce 0, immortal era, generation 0, zero tip and fee multiplier 1.0. A pinned finalized Finney read confirmed that multiplier. They are candidate-runtime estimates, not fees already deployed on mainnet or a claim that dummy order/ciphertext fixtures execute successfully. Ordinary SR envelopes use a dummy signature for size; MS envelopes use actual signed proofs.

MS rows include the larger envelope and the maximum of measured valid/invalid authorization costs. Fixed and rotating MS use the same weight selector and fixed-size proof at this nonce. Plain transfers target an already registered account by SS58; a descriptor-aware SDK batch can add registration/check and wrapper fees. Sovereign derived accounts require an authorized wrapper: both the inner-call-only sponsored quote and a complete final MS approval through 2-of-2 multisig are shown; the first approval's separate fee is not included.

| Operation / scenario | Encoded bytes | Fee quote (TAO) | USD |
|---|---:|---:|---:|
| register MS (SR sponsor) | 174 | 0.000894830 | $0.243336 |
| check_registered MS (SR sponsor) | 174 | 0.000077164 | $0.020984 |
| register_derived (2-of-2 descriptor) | 175 | 0.008283107 | $2.252467 |
| register_derived_sponsored (inner call only) | 207 | 0.008593616 | $2.336905 |
| check_derived (2-of-2 descriptor) | 175 | 0.000080382 | $0.021859 |
| install_capability (MS; unrestricted trading) | 5,525 | 0.000381259 | $0.103678 |
| revoke_capability (MS) | 5,445 | 0.000365616 | $0.099424 |
| create_protected_pure (MS) | 5,451 | 0.007120700 | $1.936368 |
| as_protected_derivative (MS; empty remark) | 5,449 | 0.007114422 | $1.934660 |
| authorize_order (MS; V1 terms) | 5,592 | 0.000387370 | $0.105339 |
| cancel_order (MS; authorized V1 terms) | 5,592 | 0.000391109 | $0.106356 |
| transfer 1 TAO (MS; registered destination) | 5,481 | 0.000332326 | $0.090371 |
| transfer 1 TAO (SR comparison) | 144 | 0.000101874 | $0.027703 |
| execute_orders (1 MS proof; SR relayer) | 5,530 | 0.004224706 | $1.148845 |
| execute_batched_orders (1 MS proof; SR relayer) | 5,531 | 0.004114221 | $1.118801 |
| execute_orders (8 MS proofs; SR relayer) | 43,479 | 0.005031507 | $1.368243 |
| execute_batched_orders (8 MS proofs; SR relayer) | 43,480 | 0.005023919 | $1.366179 |
| Shield carrier (8192 bytes; SR) | 8,301 | 0.000116745 | $0.031747 |
| Shield carrier (8193 bytes; SR) | 8,302 | 0.000120584 | $0.032791 |
| Shield carrier (32768 bytes; SR) | 32,881 | 0.000132873 | $0.036133 |
| Shield carrier (8192 bytes; MS) | 13,638 | 0.000351829 | $0.095675 |
| Shield carrier (8193 bytes; MS) | 13,639 | 0.000355668 | $0.096719 |
| Shield carrier (32768 bytes; MS) | 38,218 | 0.000381912 | $0.103855 |
| register_derived_sponsored (2-of-2 final MS approval) | 5,600 | 0.008922953 | $2.426463 |

Successful transactions may refund unused declared weight. Shield rows are **carrier fees only**; the decrypted inner transaction has its own fee. Values for 8,192/8,193 bytes demonstrate selection of the small/large carrier tier.

Authenticated order weight uses `n = max(actual order count, 8)` and authenticated count `p` from 1 to 8, preserving the measured ranges. Thus a one-proof order batch still pays the eight-order baseline. The 16 weight methods do not represent 17 public dispatchables: `authorize_mldsa65` and its invalid-proof counterpart are authorization overhead, and several methods select variants of the existing registration/order/Shield calls. They are already included in the applicable table rows and must not be charged twice.

**Deposits are separate**

| New account / first-time operation | Reserved TAO | USD equivalent |
|---|---:|---:|
| Classical hashed account | 0.200 | $54.39 |
| MS account (fixed or rotating) | 1.144 | $311.09 |
| Protected derived account | 8.312 | $2260.32 |
| Protected pure proxy, total first-time reserve | 8.405 | $2285.61 |

These values come from runtime constants, not the benchmark. Descriptor registration reserves its permanent deposit from the sponsor; it is not a transaction fee. Protected pure creation adds the ordinary 0.093 TAO proxy reserve to the 8.312 TAO permanent derived-registration deposit. Protected derivative creation reserves the derived deposit on first registration; checking/reusing an existing registration does not reserve it again. Multisig also has its normal temporary deposit, released on completion/cancellation, separate from these permanent registration deposits. Order economic fees, transferred value, stake and tips are not included in the transaction-fee chart.

**Validation**

Before activation-control consolidation, the full local preflight passed: **3,584 Rust tests passed**, zero failed, 47 ignored. It includes both CI Clippy configurations, formatting, feature propagation, generated SDK/docs checks, TypeScript checks, the release-node build and metadata drift. No migration changed, so the gate correctly skipped another mainnet migration rehearsal. Lockfiles are unchanged.

That validation also included two default-feature integration tests use production weight wiring: one obtained and round-tripped 25 sized envelopes and checks block-weight bounds; the other registers an MS account, admits and executes its transfer through Executive, and verifies balance movement, fee debit, nonce and generation advancement. They do not use the unit-test weight overrides. Registration and Shield boundary tests now assert the measured selectors rather than obsolete `Weight::MAX` placeholders. The former full localnet/audit report remains historical evidence; this weight-only follow-up is not another exhaustive localnet run or a production-throughput certification.

All temporary local listeners and owned test processes are stopped. The benchmark monitor is paused because this result has been handled.

[Current CSV with fee components](evidence/ml-dsa-reference-weights-39060946210/activation-fees.csv) · [Machine-readable validation and hashes](evidence/ml-dsa-reference-weights-39060946210/validation-summary.json) · [Original benchmark artifact](evidence/ml-dsa-reference-weights-39060946210/artifact.zip)


**Activation cleanup validation**

After consolidating activation under AdminUtils, the full preflight passed again:
3,583 Rust tests, both Clippy configurations, generated SDK/docs checks, release
build and metadata drift. The focused SDK suite passed 1,127 tests plus five CLI
regressions. Two default-feature production-weight tests passed and all 24
remaining fee quotes exactly match the earlier import. A fresh archival local
chain with normal 12-second blocks and production weights passed the root-only
toggle test and the complete all-profile SDK pause/resume test, including ordinary
SR/ED operation while disabled. All owned nodes and test runners were stopped.

Call index 6 remains unused and `AuthorizedFixed` retains event index 8. Account
identities, permanent protections, aliases, and replay/epoch counters are unchanged.
No further duplicate feature extrinsics were found; registration guards, explicit
sponsored registration, and protected child creation have distinct authorization
and deposit semantics. The spec version remains 480; deployment still requires a
version newer than the target chain or the explicitly approved version exception.
No commit, push, version bump or label change was performed.

[Cleanup results and hashes](evidence/ml-dsa-reference-weights-39060946210/activation-validation.json)
