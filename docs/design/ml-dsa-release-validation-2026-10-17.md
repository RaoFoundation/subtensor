---
title: "ML-DSA and hashed-account local release validation"
description: "Completed validation of commit 437053f80d, including 91 live cases, cryptographic cross-checks, client decoding and mainnet-state rehearsals."
---

**All 91 accepted localnet cases passed. No new production-code defect was found within the validated scope.** The candidate is **`437053f80d5ebebce950602b168901bfe6591483`**, captured from a clean checkout. Changes from this run are tests, fixture corrections and evidence; production sources and all five governed lockfiles are unchanged. Nothing was committed or pushed.

This completes the requested local validation excluding reference weights and production capacity. Production `Weight::MAX` gates remain. Functional activation used an explicitly recorded five-file test-weight overlay; this does not justify deploying those gates unchanged. The previously accepted rule to finalize descriptor registration before public disclosure or bare-address funding remains part of the operating model.

The run used **11 fresh isolated chains, three validators each, normal 12-second slots, and archived blocks/state**. RPC checks confirmed distinct genesis hashes, a 6,000 ms minimum timestamp period, and the expected runtime code. The untouched production runtime separately rejected both MS registration profiles with `ExhaustsResources`, even after their flags were enabled. All test chains, owned runners and temporary listeners were subsequently shut down and independently checked.

**Executed live coverage**

| Suite | Cases | Result |
|---|---:|---|
| Release gates, rollback, proof binding, Shield nonce boundaries | 17 | Passed |
| MS orders, capabilities, native EVM and derived authority | 8 | Passed |
| Staking, locks, identity, children, serving, hotkey swap, utility, preimage, contracts, crowdloan | 8 | Passed |
| Yuma/Null validator eligibility, direct and committed weights | 2 | Passed |
| All six wallet lifecycles, recovery and coldkey migration | 6 | Passed |
| All six CLI wallet modes and import/signing paths | 6 | Passed |
| MS proxy roles, mixed quorum, cancellation and deposits | 3 | Passed |
| Restricted EVM proxy, delayed execution and revoke/regrant | 4 | Passed |
| Consent, failed reserve, account drain/refill and seed recovery | 4 | Passed |
| Protected multisig/proxy composition | 1 | Passed |
| Permitted weight setting, forbidden transfer and revocation | 2 | Passed |
| Mixed multisig receiving/signatory paths and simulated Vault | 24 | Passed |
| Shield across three validators and all six wallet modes | 6 | Passed |
| **Total** | **91** | **Passed** |

There are **14 newly added regression cases**: three extended proxy/multisig cases, four proxy-boundary cases, four sponsorship cases, one nested-authority case and two validator-proxy cases. The other expansion is 24 existing mixed/Vault cases beyond the previous 53-case baseline. Superseded fixture attempts are retained separately and are not added to the passing total.

The new cases verify permitted operations and named denials, including restricted-proxy EVM escalation, delay enforcement, old-announcement invalidation after revoke/regrant, mixed three-of-four quorum execution, cancellation by every member, nested multisig/proxy use, and weight-setting through a restricted proxy. Sponsorship coverage checks absent/declined consent, insufficient caps, runtime reserve failure without partial registration, exact-cap first-payment sweeps, actual fee/deposit conservation, and restored-seed spending after draining and refilling the account. Proxy/multisig first-payment attempts to an unregistered descriptor fail before submission; the same wrapped payment succeeds after separate finalized registration.

The two validator-proxy cases independently verified finalized non-self weights, rejection of a transfer, unchanged weights after revocation, deposit release, and principal/delegate generation and nonce attribution. The separate validator suite exercises real Null permit selection as well as owner-authorized Yuma paths.

An independent RPC audit verified **765 unique recorded receipts** against canonical finalized blocks, extrinsic indices/hashes and System/wrapper events: **706 successes**, **20 successful outer calls with expected failed inner dispatch**, and **39 expected System failures**. Its exact scope is 11 receipt-bearing suites/61 cases across eight chains. Mixed multisig, cross-validator Shield and RPC tests have their own evidence. The receipt transcript includes retained setup/superseded-attempt transactions; it is not an inventory of every CLI/Ethereum transaction or an embedded consensus proof.

The two long CLI multisig scenarios additionally have independent finalized-state/event proofs: exact 8/9 TAO recipient balances, temporary deposit release, signing counters, intended inner failure and recovery, and denied/allowed proxy outcomes. [CLI state verification](evidence/ml-dsa-release-2026-10-17/proofs/mixed/cli-state-verification.json).

**Build, source and cryptographic validation**

- The full mandatory local preflight passed: **3,582 Rust tests passed, zero failed, 47 ignored**, across 88 targets. CI formatting, Clippy configurations, zepter, generated SDK/docs drift, and the other required gate checks passed. The ignored tests are classified in the evidence; none is an ML-DSA test and none is counted as passed.
- **3,561 distinct offline SDK cases passed**: 3,560 in the broad run plus the separately rerun optional legacy Fernet case. Live cases skipped by that offline invocation are counted only through their actual live runs above. Final collection found **3,658 tests**. Fresh ruff, formatting, collection, configured CI type checking and diff checks passed; the type-check policy permits warnings, which remain in its retained log.
- WASM core/binding checks, `wasm-pack` and JavaScript smoke checks passed. A separate default-feature Executive regression verified the EVM association cooldown at block 7,199 (rejected) and 7,200 (successful) for all six wallet modes. Live lifecycle tests preserved that cooldown and checked early rejection; they did not wait a full day for it.
- Static tracing rechecked descriptor identity, profile/domain/call binding, authorization consumption, nonce and generation advancement, persistent protection, mixed quorum eligibility, proxy filters, EVM aliases, consent before signing, and seed recovery. The initial source review and final test-assertion review are retained. The focused unit results overlap the broad SDK count and are not added again.
- Fresh independent ML-DSA cross-checks passed **8,003 cases** against pinned liboqs C/x86-64 implementations: key/signature equality, bidirectional verification, wrong messages and contexts, signature/public-key mutations, malformed hints, payload boundaries, descriptor identity, HKDF generations, authorization headers and message/order capabilities. Current production primitive and reference-library hashes are recorded. This is interoperability and adversarial-input evidence, not formal proof or side-channel certification.

The Executive matrix inventories 325 production dispatch variants across 24 callable pallets; the all-features faucet adds one. Two GRANDPA variants are uninhabited, and `Ethereum.transact` is separately rejected as a native signed wrapper. The remaining **323 variants × four signing modes = 1,292 pool/dispatch cases**, plus four Ethereum-wrapper rejections, are included in the Rust total. The modes are plain SR, plain ED, rotating MS and fixed MS; hashed SR/ED are exercised elsewhere. Minimal fixtures establish authentication/dispatch parity with eight explicit policy exceptions, not successful business semantics for every possible state and argument combination.

The inventory reconciles **3,658 collected = 3,561 offline passes + 91 selected live passes + six unselected live cases**. The six standalone live cases are explicitly not counted as passed: two emission-allocation SDK/CLI flows, the complete Null/Yuma/Null emissions cycle, and separate activation, PoW and timelock fixtures. The MS-sensitive activation/PoW/timelock paths have related executed coverage above; that is not a claim that every assertion in the omitted fixtures was repeated. [Exact omissions and related evidence](evidence/ml-dsa-release-2026-10-17/proofs/preflight/unexecuted-live-test-classification.json).

**Client compatibility, mainnet state and RPCs**

All six wallet modes appeared successfully in one finalized block containing 12 extrinsics. Polkadot.js 16.5.6 and the selected PAPI decoder passed metadata V14/V15/V16 decoding and exact round trips. The current native library and a preserved native build without current ML-DSA key-type exports passed V14/V15; both lack V16 support. The preserved binary is hashed, but its source revision was not established. Decoder provenance, block bytes and hashes are retained. No graphical explorer UI or physical Vault/phone was exercised; the Vault adapter uses a simulated external signer response.

Both production-weight mainnet rehearsals passed upgrade checks `All`, idempotency, spec compatibility and **ten synthetic post-upgrade blocks each**, with state-root/signature checks, try-state `All` and full-state decoding. The second build includes `try-runtime,metadata-hash`, covering the release feature combination plus instrumentation. Both use the authenticated October 15 snapshot at block **9,290,353**, approximately 49 hours old at execution. A fresh read-only query found mainnet spec 479; candidate spec 480 is newer. There were no mainnet writes, new clone workflows or reference benchmark runs. These checks do not claim a fresh snapshot, full historical replay or bit-identical Srtool build.

Production-runtime RPC tests with MS disabled passed admission isolation under malformed/unknown runtime calls, pending overlays and rollback, 32 queued Ethereum transfers with actual finalized debit/nonce checks, and contract call/create simulation, revert, access-list and supported state-override behavior. The pre-existing Ethereum `effectiveGasPrice` reporting discrepancy was reproduced and remains explicitly separate from actual-debit correctness and this PR's regression assessment. These tests establish the exercised semantics, not production throughput.

**Changes and deployment limits**

Only tests and evidence changed. Older fixtures now provide explicit sponsorship caps, serialize shared sponsors through finalization, load canonical events when normalized failed receipts omit them, include `metadata-hash` for Vault, and check the SCALE `Ok` variant without assuming one unit representation. No production authorization or balance invariant was weakened to make a test pass.

Reference weights/capacity remain excluded. The master toggle controls direct protected authentication, registration and capabilities; it is not a global freeze of every previously authorized mixed-multisig/order effect completed by ordinary relayers. Raw Ethereum RLP continues to use secp256k1; MS uses native EVM call/create routes. The tested outcomes cannot guarantee safety for every future input or rule out every implementation error.

**Evidence and cleanup**

The [machine-readable summary](evidence/ml-dsa-release-2026-10-17/validation-summary.json) contains exact source/artifact hashes, accepted case identities, both mainnet rehearsals, decoder results, RPC results, final checks and shutdown verification. Its portable file manifest indexes retained JUnit, compressed logs, source snapshots, codec capture and canonical receipt transcript. Wallet keyfiles, binaries, node databases and the large snapshot are excluded from that portable bundle. Raw local evidence and archived chain state remain in `/tmp/ms-head-validation-20261017`.

All 66 validation RPC/P2P ports and temporary preflight RPC port 9977 were closed, with no owned test chain or runner remaining. No commit or push was made.
