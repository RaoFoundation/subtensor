---
title: "Existing Ethereum receipt fee discrepancy"
description: "Confirmed localnet receipt accounting mismatch under Subtensor's existing base-fee-only policy, separate from the pending RPC availability repair."
---

The 32-transfer pending RPC regression exposed a **confirmed pre-existing Ethereum receipt correctness issue**. Subtensor charges only the block base fee, while `effectiveGasPrice` in the receipt reports the submitted legacy gas price. This is not a passing receipt-accuracy test, and the discrepancy remains unfixed outside the RPC availability repair.

The initial fixture submitted 32 ordinary ECDSA value transfers at 10 gwei to a new recipient. All 32 succeeded in canonical Ethereum block 14 (`0x89a231b60222a5631d8aad6e531fe836694f392c02a39e8b53cf7f94668a8828`), whose base fee was 5 gwei. Archive recovery verified their receipts and balances after Substrate finality reached block 15:

| Observation | Result |
| --- | --- |
| Initial native sender balance | 100,000,000,000 rao |
| Total transferred value | 32,000,000 rao |
| Final native sender balance | 99,963,740,000 rao |
| Actual total fee debit | **4,260,000 rao** |
| Sum of receipt `gasUsed × effectiveGasPrice` | **8,520,000 rao** |
| Sum of receipt `gasUsed × block baseFeePerGas` | **4,260,000 rao** |
| Final sender nonce | 32 |
| Final recipient native balance | 32,000,000 rao |

The measured sender debit matches the runtime's established base-fee-only policy exactly. This observation does **not** demonstrate unauthorized extra debit, lost transfer value, or an ML-DSA signing failure. Consumers calculating paid fees from receipts receive an inflated value when the submitted price exceeds the base fee. The same source mismatch exists for type-2 fee calculations when priority fees would increase the reported price.

### Source and provenance

At checkout HEAD `2d5089da3bcb292cbf557d9b22ff34582e23b3f4`, with the local RPC repair applied:

- `vendor/frontier/frame/evm/src/runner/stack.rs:305–314` explicitly replaces the priority fee with `None` and charges `base_fee`. Line 351 also exposes `base_fee` to the `GASPRICE` opcode. Lines 448–449 derive the final charge from effective gas and this fee.
- `vendor/frontier/client/rpc/src/eth/transaction.rs:319–335` instead uses the submitted `gas_price` for legacy/type-1 receipts; lines 287–315 calculate type-2/type-4 price using base fee plus priority fee.
- Neither file was modified by this local RPC availability repair. `git blame` attributes the cited policy and receipt lines to the repository history boundary `b5d06a5bcd` (2026-07-10), so that blame date is not asserted as the original introduction date.
- Both behaviors are independently present in the locally available `origin/main` commit `6a972693ae9580541225bd37696ac2fee26b8ae9`: runner lines 258, 266, and 303, and receipt lines 313, 320, and 321. The runner policy is also present in the local `codex/hashed-accounts` base `b2ebaeef436b1b5288da066bf9d0a9244fdb1908`. These are inspected local refs, not a claim that the remote tips were refreshed.

### Evidence status

The original `ethereum_pending_transfers.initial.py`, `ethereum-pending-results.initial.json`, and `ethereum-live.initial.log.gz` are retained in this evidence directory (the raw files also remain under `/tmp/ms-release-validation/rpc-followup/`). The original result is **failed** at its final receipt-based fee assertion; its earlier 32-entry pending nonce/value observations passed. Its SHA-256 is `1d8ce9d58350885e43fff9aef752f22d0f108484ba8f81882d8ba2791fe7cf4a`.

`recover_initial_receipts.py` reads the retained archive without submitting transactions. `ethereum-initial-recovered.json` contains all 32 receipts, the canonical block, native balances, and a status of `passed_with_confirmed_preexisting_receipt_mismatch`: recovery and base-fee accounting succeeded; receipt accuracy did not.

The corrected transfer fixture persists receipts before accounting assertions, verifies exact actual debits against each block's base fee, and separately asserts and reports the existing receipt discrepancy. The contract fixture likewise expects the established base-fee `GASPRICE` behavior while exercising typed pending call/create, access-list warming, and rollback. These fixture corrections preserve the non-MS finding rather than treating it as resolved.
