---
title: "Local release, compatibility and receipt evidence"
description: "Accepted local-chain cases, exact production artifacts, block decoding, preflight checks and public receipts."
---

[Root live results](root-live-results.json) identify **17 release safety cases and
8 operation-family cases** that passed. Earlier fixture failures and interrupted
attempts are retained in the compressed logs and are not counted as full-suite
passes. The unchanged first eight recovery cases and two contract cases are
combined with their explicitly selected successful follow-up runs.

[Finalized receipt verification](root-finalized-receipt-verification.json)
independently checks the recorded block hashes, canonical heights, extrinsic
indices and System outcomes. Nested dispatch events are retained. The live tests
separately assert the business state and expected partial-write/rollback effects;
an expected failed inner dispatch is not mistaken for a successful operation.
[Exact proof rejection evidence](release-receipts/strict-proof-rejections.json)
requires actual bad-signature errors, excluding capacity, timeout, fee and other
unrelated failures.

[Chain artifacts](chain-artifacts.json) distinguish the earlier native node,
intermediate corrected node, final production node and separately built local
weight overlay. [Five-chain health](five-chain-health.json) captures all isolated
chains while active with normal 12-second slots. It is a historical observation,
not a claim that those nodes remain running. Shutdown records document cleanup.

The [full preflight result](preflight-results.json),
[ignored-test list](preflight-ignored.log.gz), [production artifact hashes](production-artifacts.json)
and [exact final production verifier results](production-verifier-results.json)
are separate from activated localnet evidence. The all-features workspace run
passed 3,568 tests and ignored 47; the complete SDK unit run passed 3,115 tests
with one unrelated legacy-encryption fixture skipped. A later directory-fsync
regression is recorded in the separate crypto package.

[Codec results](live-codecs-results.json) exercise Polkadot.js and PAPI against a
finalized 44-extrinsic mixed-wallet block at metadata versions 14, 15 and 16.
[API compatibility](api-compatibility.json) compares legacy encodings against
fresh mainnet metadata. These are codec checks, not proof of every external UI.

[Pending-RPC isolation](live-pending-isolation-results.json) reproduces the
remaining availability defect; a passing reproduction is not a repair. The
parent release report also retains the hostile-registration fund-safety blocker
and independent cryptographic audit requirement.

Raw logs are losslessly gzip-compressed. Public account addresses and test
receipts are retained; private wallet files, CLI creation output, snapshots and
binaries are excluded. The manifest records retained bytes and original log
hashes. Reproduction scripts preserve the exact local paths used and require an
equivalent environment and active test chain.
