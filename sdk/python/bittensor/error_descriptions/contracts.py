"""Chain error descriptions declared (first) by the `Contracts` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "CannotAddSelfAsDelegateDependency": (
        "A contract called `lock_delegate_dependency` with its own code hash, which is not "
        "permitted. Check the code hash argument passed to the delegate dependency API against "
        "the contract's own code hash."
    ),
    "CodeInUse": (
        "`remove_code` was refused because at least one contract instance still references the "
        "code hash. Check the code's reference count and terminate or `set_code` the contracts "
        "using it before removal."
    ),
    "CodeInfoNotFound": (
        "No `CodeInfoOf` entry exists for the supplied code hash, so its owner and deposit "
        "metadata cannot be read. Verify the code hash argument and that the code was uploaded "
        "and not since removed."
    ),
    "CodeNotFound": (
        "No uploaded WASM binary exists under the supplied code hash. Verify the `code_hash` "
        "argument used in instantiation, `set_code`, or a delegate call, and that the code was "
        "uploaded via `upload_code` and not removed."
    ),
    "CodeRejected": (
        "The uploaded WASM failed validation, most often because it imports a host API the node "
        "does not support, e.g. newer ink! against an older node. Rerun the node with "
        "`-lruntime::contracts=debug` to see the detailed rejection reason."
    ),
    "CodeTooLarge": (
        "The code blob passed to `instantiate_with_code` or `upload_code` exceeds the maximum "
        "code length in the pallet's schedule. Compare the WASM binary size against the "
        "schedule's code length limit and shrink the contract."
    ),
    "ContractNotFound": (
        "No contract instance exists at the destination address; the account has no "
        "`ContractInfoOf` entry. Verify the `dest` address and that the contract was "
        "instantiated and has not been terminated."
    ),
    "ContractReverted": (
        "The contract ran to completion but returned with the REVERT flag set, rolling back its "
        "state changes; only extrinsics surface this as an error. Dry-run the call via RPC and "
        "decode the returned output data for the contract's error value."
    ),
    "ContractTrapped": (
        "The contract aborted with a WASM trap, e.g. a panic, unreachable instruction, or "
        "memory violation, instead of returning normally. Dry-run the call with debug messages "
        "enabled and check the input data against the contract's expectations."
    ),
    "DecodingFailed": (
        "Input bytes passed to a contract API host function could not be SCALE-decoded into the "
        "expected type. Check the encoding of the call's input data or the argument bytes the "
        "contract passes to the runtime API."
    ),
    "DelegateDependencyAlreadyExists": (
        "The contract called `lock_delegate_dependency` for a code hash it has already locked. "
        "Check the contract's recorded delegate dependencies before adding, and unlock the old "
        "entry first if replacing it."
    ),
    "DelegateDependencyNotFound": (
        "`unlock_delegate_dependency` was called for a code hash that is not among the "
        "contract's locked delegate dependencies. Check the code hash argument against the "
        "dependencies recorded in the contract's info."
    ),
    "DuplicateContract": (
        "Instantiation would create a contract at an address already occupied by an existing "
        "contract. Check the derived contract address and vary the `salt` argument to obtain a "
        "fresh address."
    ),
    "Indeterministic": (
        "Code flagged as non-deterministic (e.g. using floating point) was used where "
        "determinism is enforced, such as on-chain instantiation or calls. Check the "
        "determinism mode the code was uploaded with and rebuild the contract "
        "deterministically."
    ),
    "InputForwarded": (
        "The contract forwarded its input to a callee via `seal_call` with the FORWARD_INPUT "
        "flag and then tried to read or forward it again. Check the call flags used; use "
        "CLONE_INPUT when the input is still needed afterwards."
    ),
    "InvalidCallFlags": (
        "The flags bitmask given to `seal_call` or `seal_delegate_call` contains an unknown or "
        "disallowed combination; delegate calls accept only a restricted flag set. Check the "
        "flags argument against the supported `CallFlags` bit values."
    ),
    "InvalidSchedule": (
        "The pallet's schedule is misconfigured, e.g. a zero weight or zero `ref_time_by_fuel` "
        "for a basic operation, making gas conversion impossible. This is a runtime "
        "configuration issue; inspect the `Schedule` constant rather than call arguments."
    ),
    "MaxCallDepthReached": (
        "A nested contract call would exceed the maximum call stack depth defined in the pallet "
        "schedule. Inspect the cross-contract call chain for deep or unbounded recursion and "
        "flatten it or raise the configured depth."
    ),
    "MaxDelegateDependenciesReached": (
        "`lock_delegate_dependency` failed because the contract already holds the maximum "
        "number of delegate dependencies allowed by the runtime. Check the "
        "`MaxDelegateDependencies` config value and unlock unused dependencies first."
    ),
    "MigrationInProgress": (
        "A multi-block storage migration of the contracts pallet is still running, so other "
        "extrinsics of the pallet are rejected until it completes. Check the migration status "
        "in storage and retry once done, or submit `migrate` calls to advance it."
    ),
    "NoChainExtension": (
        "The contract invoked `call_chain_extension` but this runtime registers no chain "
        "extension; such code is normally rejected at upload. Check that the target chain "
        "provides the chain extension the contract was built against."
    ),
    "NoMigrationPerformed": (
        "A `migrate` call ran but no migration step executed, either because no migration is "
        "pending or the supplied `weight_limit` is too small for one step. Check the "
        "in-progress migration status and increase the weight limit argument."
    ),
    "OutOfBounds": (
        "A pointer and length pair passed to a contract API host function references memory "
        "outside the contract's sandbox. Check the buffer pointers and lengths the contract "
        "passes to seal functions; this usually indicates a low-level contract bug."
    ),
    "OutOfGas": (
        "The contract exhausted the gas limit supplied for this execution before completing. "
        "Increase the `gas_limit` argument on `call` or `instantiate`; dry-run the call via RPC "
        "to estimate the required weight."
    ),
    "OutOfTransientStorage": (
        "A write would exceed the per-execution byte limit for transient storage. Check how "
        "much data the contract places in transient storage during the call against the "
        "runtime's transient storage limit."
    ),
    "OutputBufferTooSmall": (
        "The output buffer the contract supplied to an API call is smaller than the data the "
        "runtime needs to write back. Check the output length pointer the contract passes and "
        "enlarge the buffer; usually a contract-side bug."
    ),
    "RandomSubjectTooLong": (
        "The subject buffer given to the deprecated `seal_random` API exceeds the schedule's "
        "`subject_len` limit. Shorten the randomness subject the contract passes or check the "
        "schedule's limits section."
    ),
    "ReentranceDenied": (
        "A call tried to re-enter a contract already on the call stack without reentrancy being "
        "allowed, or contract code called back into the contracts pallet through the runtime. "
        "Check the callee address against the current call stack and the ALLOW_REENTRY call "
        "flag."
    ),
    "StateChangeDenied": (
        "The contract invoked a state-modifying host function, such as a storage write, "
        "transfer, or value-bearing call, while executing in read-only mode. Check whether the "
        "enclosing call was made with the read-only flag or from a static context."
    ),
    "StorageDepositLimitExhausted": (
        "The execution created more storage than the caller's `storage_deposit_limit` allows to "
        "be charged. Raise the `storage_deposit_limit` argument or reduce storage usage; a "
        "dry-run reports the required `storage_deposit`."
    ),
    "StorageDepositNotEnoughFunds": (
        "The origin's free balance cannot cover the storage deposit limit specified or required "
        "for this call. Check the caller's withdrawable balance against the "
        "`storage_deposit_limit` argument and the dry-run's reported deposit."
    ),
    "TerminatedInConstructor": (
        "The contract called `seal_terminate` inside its constructor, self-destructing during "
        "instantiation, which is forbidden. Inspect the constructor logic; termination is only "
        "allowed in regular message calls."
    ),
    "TerminatedWhileReentrant": (
        "`seal_terminate` was called on a contract that appears more than once on the call "
        "stack, so termination was refused. Check the call chain for reentrant calls into the "
        "contract being terminated."
    ),
    "TooManyTopics": (
        "The number of topics passed to `seal_deposit_event` exceeds the schedule's "
        "`event_topics` limit. Reduce the number of indexed topics in the contract's event "
        "definition or compare against the schedule limit."
    ),
    "TransferFailed": (
        "A balance transfer performed during the contract call failed, most likely because the "
        "sender lacks enough free balance. Check the transferring account's free balance "
        "against the `value` being sent, accounting for the existential deposit."
    ),
    "ValueTooLarge": (
        "A value written to contract storage or emitted as event data exceeds the "
        "`MaxValueSize` limit. Compare the size of the stored value or event payload against "
        "the runtime's maximum value size constant."
    ),
    "XCMDecodeFailed": (
        "The bytes the contract passed to `xcm_execute` or `xcm_send` could not be decoded as a "
        "versioned XCM message. Check the XCM encoding and version the contract produces "
        "against what the runtime supports."
    ),
}
