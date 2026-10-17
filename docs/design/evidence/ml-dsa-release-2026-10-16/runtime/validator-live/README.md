---
title: "Live MS validator weights and commitments"
description: "Finalized fixed and rotating ML-DSA validator operations, corrected fixture history, and independent canonical receipt checks."
---

The corrected local-chain suite passed both cases in 723.21 seconds. Each case used three freshly generated MS keys, a separate subnet, and actual MS-signed transactions. Code 8 is fixed MS (subnet 14); code 7 is rotating MS (subnet 15). Each has 14 successful finalized receipts, recorded in [fixed MS](validator-live-corrected/validator-paths-8.json) and [rotating MS](validator-live-corrected/validator-paths-7.json).

Both cases successfully exercised:

- Alice-sponsored atomic descriptor registration and funding, followed by MS subnet creation and miner burn registration.
- Non-self SDK Yuma `SetWeights`, checked as the exact finalized `(miner_uid, 65535)` value.
- Actual stake, owner-signed Null consensus selection, a validator permit selected from that stake, and exact raw-u16 Null weights of 9.
- Plain commitment storage and a timelocked metadata commitment decrypted by the chain into the exact expected plaintext.
- SDK encrypted `CommitWeights`, followed by automatic on-chain reveal into exact raw weights of 13.
- Legacy salt-based `commit_weights` and SDK `RevealWeights`, using the stored commit epoch, with exact final weights of 65535 and an empty legacy commit queue.

The chain retained normal 12-second block timing. Supported per-subnet administration set tempo 6, reveal period 1, zero validator weight-call rate limit, and registration/weight parameters needed by the fixture. No global activation switch, Sudo owner, runtime timing constant, reference weight, or storage injection was changed by this suite. No `start_call` delay was bypassed. The epoch scheduler advances these subnets before emissions start. Alice's funding and root transactions held the shared sponsor lock through finalization; test wallets had ample disposable local balances.

This used the existing activated local validation runtime with its recorded weight overlay, not the production Wasm. [Run identity](validator-live-run-manifest.json) pins local Wasm SHA-256 `4ff98b40b7a38d636bbb1f857103ff3881d6ebb758d030dcb206ec541a10931b`, genesis, spec 480, SDK native-extension identity, and executed test-source hashes. Measured production weights remain outside scope.

[Independent post-verification](validator-live-post-verification.json) checked all 48 receipts from both attempts against canonical finalized blocks through block 302, exact extrinsic indexes and hashes, `System.ExtrinsicSuccess`, and successful nested results, including every Sudo setup call. It also independently checked both attempts' revealed plaintext and the corrected cases' final weights and empty legacy queues. The [verification script](verify-validator-live.py) preserves the exact read-only checks.

The first attempt had 20 successful finalized receipts, but both tests stopped when a test helper tried to decode already revealed UTF-8 plaintext as hexadecimal. This was a fixture error, not a chain or ML-DSA failure. The [original executed source](validator-live-first-attempt-test.py), [first log](validator-live.log.gz), and [first fixed](validator-live/validator-paths-8.json) and [first rotating](validator-live/validator-paths-7.json) records are retained. Python's failure display reread the edited source lines; the preserved original source and hash identify the code actually executed. The helper now follows the existing timelock test's hex-or-UTF8 conversion. The [corrected log](validator-live-corrected.log.gz) preserves both passes.

The executed test is `sdk/python/tests/e2e/test_mldsa_validator_paths.py`, SHA-256 `e74d6434bdde194a9d08ed6735f4a17b2d341d88303b7d9965cf259d190b6edc`. It ran with `PYTHONPATH=/tmp/ms-cleanup-native:$PWD/sdk/python`, the repository SDK virtual environment, `E2E_WALLET_ENDPOINT=ws://127.0.0.1:20244`, the shared Alice lock, `-n 2 -vv -s`, and separate evidence directories for each attempt. Public evidence includes no private wallet keys.

These successful validator paths close the specific missing live weight/commitment coverage. They do not resolve the separately documented hostile pre-registration account obstruction or pending-RPC admission issue, and they do not prove every possible state or argument combination safe.
