"""Chain error descriptions declared (first) by the `SubtensorModule` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AccountRejectsLockedAlpha": (
        "Locked alpha was being transferred to a coldkey whose `AccountFlags` do not have the "
        "accept-locked-alpha bit set, e.g. during a lock transfer or coldkey swap of locks. "
        "Check the destination coldkey's `AccountFlags` storage and have the recipient opt in "
        "to receiving locked alpha before retrying."
    ),
    "ActiveLockExists": (
        "The destination coldkey already holds a lock with nonzero locked mass on that subnet, "
        "so a new or transferred lock cannot be created there. Inspect the `Lock` storage for "
        "the (coldkey, netuid, hotkey) triple and wait for the existing lock to unlock or "
        "remove it first."
    ),
    "ActivityCutoffFactorMilliOutOfBounds": (
        "The `factor_milli` argument to set the activity-cutoff factor was outside the allowed "
        "1000-50000 per-mille range (1 to 50 tempos). Adjust the argument to fall within those "
        "bounds before resubmitting."
    ),
    "ActivityCutoffTooLow": (
        "An admin tried to set the subnet's activity cutoff below the chain-wide minimum. "
        "Compare the requested value against the `MinActivityCutoff` storage item and current "
        "`activity_cutoff` in `btcli sudo get --netuid <n>`."
    ),
    "AddStakeBurnRateLimitExceeded": (
        "The add-stake-and-burn operation was submitted again before its per-key rate-limit "
        "window elapsed. Wait some blocks and retry; no active raise site exists in current "
        "code, so this mainly appears on older runtimes."
    ),
    "AdminActionProhibitedDuringWeightsWindow": (
        "An owner or admin hyperparameter change was attempted inside the protected freeze "
        "window just before the subnet's epoch runs. Check `AdminFreezeWindow` and the blocks "
        "remaining until the next epoch (subnet tempo), then retry after the epoch fires."
    ),
    "AllNetworksInImmunity": (
        "Creating a new subnet required pruning an existing one, but every candidate subnet is "
        "still inside its network immunity period so none can be dissolved. Check "
        "`NetworkImmunityPeriod` and each subnet's `NetworkRegisteredAt`, and retry once a "
        "subnet leaves immunity."
    ),
    "AlphaHighTooLow": (
        "The `alpha_high` argument to set liquid-alpha values was below the minimum of roughly "
        "0.025 (1638/65535 in u16 units). Raise `alpha_high` in the `sudo_set_alpha_values` "
        "call; current values are in the `AlphaValues` storage per netuid."
    ),
    "AlphaLowOutOfRange": (
        "The `alpha_low` argument to set liquid-alpha values was below the ~0.025 minimum "
        "(1638/65535) or greater than `alpha_high`. Choose alpha_low within that range and not "
        "exceeding alpha_high; current settings are in the `AlphaValues` storage for the "
        "netuid."
    ),
    "AmountTooLow": (
        "A stake, unstake, move or swap amount was zero or its TAO equivalent fell below the "
        "minimum stake threshold after fees and slippage. Compare the amount against the "
        "`DefaultMinStake` storage item and the subnet's alpha price before retrying with a "
        "larger amount."
    ),
    "AnnouncedColdkeyHashDoesNotMatch": (
        "The `new_coldkey` passed to `swap_coldkey_announced` (`btcli wallet swap-coldkey`) "
        "hashes to a different value than the hash committed in the earlier "
        "`announce_coldkey_swap` (`btcli wallet announce-coldkey-swap`). Use the same new "
        "coldkey you announced; check status with `btcli wallet swap-check`."
    ),
    "AutoEpochAlreadyImminent": (
        "`trigger_epoch` was called when the next automatic epoch is closer than the "
        "`AdminFreezeWindow`, so a manual trigger would have no effect. Check the subnet's "
        "tempo and blocks until the next epoch, and simply wait for it to fire."
    ),
    "BalanceWithdrawalError": (
        "The requested TAO could not be withdrawn from the coldkey's free balance, typically "
        "due to insufficient funds, the existential deposit, or frozen/reserved balance. Check "
        "the coldkey's balance with `btcli wallet balance` and reduce the amount or top up."
    ),
    "BasketHasNoWeights": (
        "Retired on current runtimes: a basket deposit into a validator with no usable root "
        "weight vector is now held as the fund's root (TAO cash) slot instead of erroring. "
        "Seeing this error means the chain is running an older runtime — have the validator "
        "set root weights with `btcli weights set-root` (or `set_root_weights`) first."
    ),
    "BeneficiaryDoesNotOwnHotkey": (
        "When ending a subnet lease, the hotkey passed for the ownership handover is not owned "
        "by the lease's beneficiary coldkey. Check the `Owner` storage for that hotkey and pass "
        "a hotkey the beneficiary coldkey actually owns."
    ),
    "BetaBasketSeedInProgress": (
        "The `migrate_seed_beta_basket_v2` seed has not completed (it normally finishes "
        "inside the upgrade block, so this only appears if that run was interrupted). Basket "
        "deposits, claims, coldkey / root-touching hotkey swaps, and root stake "
        "add/remove/transfer/swap are paused until it finishes. Wait until the migration "
        "cursor clears (`HasMigrationRun` for the seed) and retry."
    ),
    "CallDisabled": (
        "The extrinsic has been switched off in the current runtime and cannot be dispatched. "
        "There is no active raise site in current code; if seen, check release notes for "
        "whether the call was re-enabled in a newer runtime version."
    ),
    "CanNotSetRootNetworkWeights": (
        "`set_weights` was called with netuid 0, the root network, where normal weight setting "
        "is not allowed. Use a non-root `netuid` argument; root weights are handled by a "
        "separate mechanism."
    ),
    "CannotAffordLockCost": (
        "The coldkey's free balance cannot cover the current dynamic subnet-creation lock cost. "
        "Compare `btcli subnets create-cost` (or `btcli query subnet-registration-cost`) against "
        "the coldkey balance from `btcli wallet balance` before registering a subnet."
    ),
    "CannotBurnOrRecycleOnRootSubnet": (
        "`recycle_alpha` or `burn_alpha` was called with netuid 0, and TAO on the root subnet "
        "cannot be burned or recycled. Pass a non-root `netuid` argument for the subnet whose "
        "alpha you want to recycle or burn."
    ),
    "CannotUseSystemAccount": (
        "The hotkey supplied for registration, hotkey swap, or subnet-owner-hotkey assignment "
        "is a reserved subnet system account. Use a regular user-generated hotkey instead; "
        "system accounts are derived per-subnet and rejected by `is_subnet_account_id`."
    ),
    "ChildParentInconsistency": (
        "A `set_children` or parent-delegation call would make the same hotkey appear as both a "
        "child and a parent, or referenced a child missing from the proposed mapping. Inspect "
        "the `ChildKeys` and `ParentKeys` storage for the hotkeys involved and remove the "
        "overlap."
    ),
    "ColdKeyAlreadyAssociated": (
        "The destination coldkey of a coldkey swap already has staking hotkeys associated with "
        "it, so it cannot receive the swapped identity. Check the `StakingHotkeys` storage for "
        "the new coldkey and swap to a fresh, unused coldkey instead."
    ),
    "ColdkeyCollateralIncomplete": (
        "A coldkey swap could not fully migrate miner collateral: after migrating every "
        "indexed collateral hotkey, the old coldkey's ColdkeyMinerCollateral aggregate "
        "was still non-zero. This is a fail-closed invariant — retry after investigating "
        "orphaned MinerCollateral rows for that coldkey, or contact runtime maintainers."
    ),
    "ColdkeyCollateralPositionsFull": (
        "This coldkey already has the maximum number of distinct hotkeys with miner "
        "collateral on the subnet. Drain or consolidate existing bonds before adding "
        "another collateral position."
    ),
    "ColdkeySwapAlreadyDisputed": (
        "`dispute_coldkey_swap` was called for a coldkey whose pending swap announcement is "
        "already under dispute. Check the `ColdkeySwapDisputes` storage for the coldkey; no "
        "further dispute action is needed."
    ),
    "ColdkeySwapAnnounced": (
        "The coldkey has a pending swap announcement, so all but a small allow-list of "
        "extrinsics are blocked until the swap completes or is cleared. Check status with "
        "`btcli wallet swap-check`, then finish with `btcli wallet swap-coldkey` or clear "
        "via `btcli tx clear-coldkey-swap-announcement`."
    ),
    "ColdkeySwapAnnouncementNotFound": (
        "`swap_coldkey_announced`, `dispute_coldkey_swap`, or `clear_coldkey_swap_announcement` "
        "was called for a coldkey with no pending announcement. Announce first with "
        "`btcli wallet announce-coldkey-swap` (`btcli wallet swap-check` shows status)."
    ),
    "ColdkeySwapClearTooEarly": (
        "The swap announcement cannot be cleared until the reannouncement delay after the "
        "announcement's execution block has passed. Compare the current block with the `when` "
        "stored in `ColdkeySwapAnnouncements` plus `ColdkeySwapReannouncementDelay` and retry "
        "later."
    ),
    "ColdkeySwapDisputed": (
        "All extrinsics from this coldkey are blocked because its pending coldkey swap is under "
        "dispute. Check the `ColdkeySwapDisputes` storage for the coldkey; the dispute must be "
        "resolved by root before the account can transact."
    ),
    "ColdkeySwapReannouncedTooEarly": (
        "`announce_coldkey_swap` was called again before the reannouncement delay after the "
        "previous announcement's execution block elapsed. Compare the current block with the "
        "stored announcement time plus `ColdkeySwapReannouncementDelay` and retry later."
    ),
    "ColdkeySwapTooEarly": (
        "`swap_coldkey_announced` (`btcli wallet swap-coldkey`) was executed before the "
        "announcement delay had elapsed since `announce_coldkey_swap`. Check remaining "
        "blocks with `btcli wallet swap-check` and wait until then."
    ),
    "CommitRevealDisabled": (
        "A weight commit or reveal was submitted on a subnet where commit-reveal is turned off. "
        "Check the `commit_reveal_weights_enabled` hyperparameter for the netuid "
        "(`btcli sudo get --netuid <n>`); use plain `set_weights` instead when it is disabled."
    ),
    "CommitRevealEnabled": (
        "Plain `set_weights` was called on a subnet where commit-reveal is enabled, which "
        "requires the commit/reveal flow instead. Check the `commit_reveal_weights_enabled` "
        "hyperparameter for the netuid and switch to `commit_weights`/`reveal_weights`."
    ),
    "CommittingWeightsTooFast": (
        "The neuron committed weights again before the per-UID rate limit elapsed since its "
        "last commit on that subnet. Compare blocks since the last commit against the "
        "`weights_rate_limit` hyperparameter (`btcli sudo get --netuid <n>`) and wait."
    ),
    "DelegateTakeTooHigh": (
        "The `take` argument exceeds the maximum delegate take allowed by the chain (18% by "
        "default). Compare the requested value against the `MaxDelegateTake` storage item and "
        "lower it."
    ),
    "DelegateTakeTooLow": (
        "The `take` argument was below the `MinDelegateTake` minimum, or "
        "`increase_take`/`decrease_take` was not strictly increasing/decreasing relative to the "
        "current take. Check the hotkey's current take in the `Delegates` storage and the "
        "`MinDelegateTake` storage item."
    ),
    "DelegateTxRateLimitExceeded": (
        "The delegate changed its take again before the per-hotkey take-change rate limit "
        "elapsed. Compare blocks since the hotkey's last take transaction against the "
        "`TxDelegateTakeRateLimit` storage item and retry later."
    ),
    "Deprecated": (
        "The extrinsic has been removed and always fails, e.g. `schedule_swap_coldkey`, the "
        "swap pallet's user-liquidity calls, or `sudo_set_total_issuance`. Migrate to the "
        "replacement call noted in the deprecation (for coldkey swaps, "
        "`btcli wallet announce-coldkey-swap` then `btcli wallet swap-coldkey`)."
    ),
    "DisabledTemporarily": (
        "The operation has been temporarily switched off in the runtime, usually as a hotfix "
        "measure. There is no active raise site in current code; if encountered, check the "
        "runtime version and release notes for when the feature is re-enabled."
    ),
    "DuplicateChild": (
        "The children list passed to `set_children` contains the same child hotkey more than "
        "once. Deduplicate the `children` argument; current relations are visible in the "
        "`ChildKeys` storage for the parent hotkey and netuid."
    ),
    "DuplicateUids": (
        "The `uids` vector passed to `set_weights` (or a reveal) contains the same UID more "
        "than once. Deduplicate the uids/values pairs before submitting; each target neuron may "
        "appear only once per weight vector."
    ),
    "DynamicTempoBlockedByCommitReveal": (
        "`trigger_epoch` is refused while commit-reveal is enabled on the subnet, because an "
        "out-of-band epoch would desync the CRv3 reveal window from the Drand schedule and drop "
        "committed weights. Check the `commit_reveal_weights_enabled` hyperparameter; disable "
        "it before manually triggering epochs."
    ),
    "EpochTriggerAlreadyPending": (
        "`trigger_epoch` was called while a previously triggered epoch is still queued for this "
        "subnet. Check the `PendingEpochAt` storage for the netuid and wait for the pending "
        "epoch to fire before triggering again."
    ),
    "EvmKeyAssociateRateLimitExceeded": (
        "`associate_evm_key` was called again before the per-UID rate limit since the last "
        "association elapsed. Compare blocks since the association recorded in the "
        "`AssociatedEvmAddress` storage against the `EvmKeyAssociateRateLimit` runtime constant "
        "and retry later."
    ),
    "EvmKeyAssociationLimitExceeded": (
        "The EVM address is already associated with the maximum number of UIDs allowed on this "
        "subnet. Inspect the `AssociatedUidsByEvmAddress` storage for the (netuid, evm_key) "
        "pair and free a slot or use a different EVM address."
    ),
    "ExpectedBeneficiaryOrigin": (
        "A lease operation such as terminating a subnet lease was signed by an account other "
        "than the lease's beneficiary coldkey. Check the beneficiary recorded in the "
        "`SubnetLeases` storage for the lease id and sign with that coldkey."
    ),
    "ExpiredWeightCommit": (
        "The hash supplied to `reveal_weights` matches a commit whose reveal window has already "
        "passed, so it can no longer be revealed. Check the `commit_reveal_period` "
        "hyperparameter and reveal within the allowed epochs after committing; re-commit and "
        "reveal on time."
    ),
    "FaucetDisabled": (
        "The `faucet` extrinsic was called on a runtime built without the pow-faucet feature, "
        "i.e. any real network. The faucet only works on local test chains compiled with that "
        "feature; use a funded wallet or testnet TAO instead."
    ),
    "FirstEmissionBlockNumberAlreadySet": (
        "`btcli sudo start` was issued for a subnet whose emissions have already been started. "
        "Check with `btcli sudo check-start`; if the subnet is already emitting, no action is "
        "needed."
    ),
    "HotKeyAccountNotExists": (
        "The hotkey has no on-chain account, meaning it was never created through registration, "
        "so staking or delegation operations cannot reference it. Check the `Owner` storage for "
        "the hotkey or `btcli wallet overview`; register the hotkey on a subnet first."
    ),
    "HotKeyAlreadyDelegate": (
        "`become_delegate` was called for a hotkey that is already a delegate. Check the "
        "`Delegates` storage for the hotkey; if it has a take entry it is already delegating "
        "and no action is needed."
    ),
    "HotKeyAlreadyRegisteredInSubNet": (
        "A registration or hotkey swap targeted a hotkey that already holds a UID on the subnet "
        "(or, for a swap without a netuid, on any subnet). Check the `Uids` storage for the "
        "(netuid, hotkey) pair or `btcli wallet overview`; use a different hotkey or netuid."
    ),
    "HotKeyNotRegisteredInNetwork": (
        "The hotkey is not registered on the relevant subnet, raised by "
        "`serve_axon`/`serve_prometheus` for the serving netuid or by identity calls requiring "
        "registration on any subnet. Verify registration with `btcli query uid --netuid <n>` "
        "(or `btcli query netuids-for-hotkey`) and register via `btcli subnets register` first."
    ),
    "HotKeyNotRegisteredInSubNet": (
        "The hotkey holds no UID on the given netuid, so weight setting, commits, or UID "
        "lookups fail there. Check the `Uids` storage for the (netuid, hotkey) pair or "
        "`btcli query uid --netuid <n>`; confirm the netuid argument and register the hotkey if "
        "needed."
    ),
    "HotKeySetTxRateLimitExceeded": (
        "The coldkey attempted a hotkey set/swap before `TxRateLimit` blocks had passed since "
        "its last such transaction. Check the coldkey's last transaction block against the "
        "`TxRateLimit` storage value and wait the remaining blocks."
    ),
    "HotKeySwapOnSubnetIntervalNotPassed": (
        "A hotkey swap on a subnet was attempted before `HotkeySwapOnSubnetInterval` blocks "
        "passed since the coldkey's last swap on that netuid. Compare `LastHotkeySwapOnNetuid` "
        "for the coldkey with the current block and retry after the interval."
    ),
    "IncorrectCommitRevealVersion": (
        "The `commit_reveal_version` argument does not match the chain's current commit-reveal "
        "weights version. Query the `CommitRevealWeightsVersion` storage item and upgrade or "
        "configure the client to commit with that version."
    ),
    "IncorrectWeightVersionKey": (
        "The `version_key` supplied with set_weights is older than the subnet's required "
        "weights version. Compare it against the `WeightsVersionKey` hyperparameter "
        "(`btcli sudo get --netuid <n>`) and update the validator software or the key."
    ),
    "InputLengthsUnequal": (
        "A batch weights call passed vectors of different lengths, e.g. netuids vs commit "
        "hashes, or uids vs values, salts and version_keys in batch reveal. Check that every "
        "parallel vector argument in the batch extrinsic has the same length."
    ),
    "InsufficientAlphaBalance": (
        "A stake decrease asked to debit more alpha than the hotkey-coldkey pair holds on that "
        "subnet. Compare the requested amount against the pair's current stake on the netuid "
        "(`btcli stake list` or the `Alpha` storage entry)."
    ),
    "InsufficientLiquidity": (
        "The pool cannot absorb the operation: the swap simulation failed, reserves are smaller "
        "than the payout, or the amount exceeds the pool's supported input. Check the subnet "
        "pool reserves `SubnetTAO`, `SubnetAlphaIn` and `SubnetAlphaOut` against the amount."
    ),
    "InsufficientStakeForLock": (
        "The requested lock amount exceeds the coldkey's total alpha stake on that subnet "
        "(existing locked mass included). Compare the amount against the coldkey's stake on the "
        "netuid, e.g. via `btcli stake list`, and lock less or add stake first."
    ),
    "InsufficientTaoBalance": (
        "The coldkey's free TAO balance is below the amount a TAO-side operation needs: a "
        "transfer between coldkeys, a burn or recycle, or a subnet-registration lock. Check "
        "the coldkey's balance with `btcli wallet balance` against the amount being moved."
    ),
    "InvalidChild": (
        "The children or parents list includes the pivot hotkey itself (a self-loop), including "
        "during a hotkey swap when the new hotkey is already a child or parent of the old one. "
        "Check the `children` argument and `ChildKeys`/`ParentKeys` for the hotkeys involved."
    ),
    "InvalidChildkeyTake": (
        "The childkey take is outside the allowed range for the subnet: below the effective "
        "minimum or above `MaxChildkeyTake`. Query `MinChildkeyTake` and `MaxChildkeyTake` and "
        "pick a `take` value within those bounds."
    ),
    "InvalidDifficulty": (
        "The submitted proof-of-work hash does not meet the required difficulty (the faucet "
        "uses a fixed 1,000,000; PoW registration uses the subnet's difficulty). Check the "
        "`Difficulty` storage for the netuid and regenerate work against the current block."
    ),
    "InvalidIdentity": (
        "The submitted coldkey or subnet identity failed validation, typically a field "
        "exceeding its allowed byte length or malformed data. Check each identity field's "
        "length against the limits enforced by the chain before calling set_identity or "
        "set_subnet_identity."
    ),
    "InvalidIpAddress": (
        "The `ip` argument to serve_axon or serve_prometheus is not a valid address for the "
        "declared `ip_type` (prometheus additionally rejects the zero address). Verify the IP "
        "encodes correctly as IPv4 or IPv6 and matches the `ip_type` passed."
    ),
    "InvalidIpType": (
        "The `ip_type` argument to serve_axon or serve_prometheus is not 4 or 6. Check the "
        "value being sent by the miner or client; only IPv4 (4) and IPv6 (6) are accepted."
    ),
    "InvalidLeaseBeneficiary": (
        "The account registering a leased network is not the creator of the crowdloan currently "
        "being finalized. Check the crowdloan's `creator` field in the crowdloan pallet storage "
        "and submit the call from that coldkey."
    ),
    "InvalidPort": (
        "The `port` argument to serve_axon or serve_prometheus is 0, which is rejected. Check "
        "the miner or client axon configuration and serve on a non-zero port."
    ),
    "InvalidRecoveredPublicKey": (
        "The EVM key association signature recovered to a public key whose keccak hash does not "
        "equal the supplied `evm_key`. Verify the signature was produced by the claimed EVM key "
        "over the hotkey plus block hash message (EIP-191 format)."
    ),
    "InvalidRevealCommitHashNotMatch": (
        "The revealed uids, values, salt and version_key hash to a value that matches none of "
        "the hotkey's pending non-expired commits. Check that the reveal parameters and salt "
        "exactly match what was committed, and inspect `WeightCommits` for the hotkey and "
        "netuid."
    ),
    "InvalidRevealRound": (
        "A timelocked weights commit specified a `reveal_round` older than the latest stored "
        "DRAND round, so it could be decrypted immediately. Query the drand pallet's "
        "`LastStoredRound` and commit with a future round number."
    ),
    "InvalidRootClaimThreshold": (
        "The value passed to set the root claim threshold exceeds the chain's maximum allowed "
        "threshold. Check the `new_value` argument against the `MAX_ROOT_CLAIM_THRESHOLD` "
        "constant and the current `RootClaimableThreshold` for the netuid."
    ),
    "InvalidSeal": (
        "The seal hash recomputed from the supplied `block_number`, `nonce` and key does not "
        "equal the submitted `work`. Verify the PoW solver built the seal for the same key and "
        "block it submits, and that the work bytes were not corrupted in transit."
    ),
    "InvalidValue": (
        "A generic out-of-range parameter on an admin or sudo call, e.g. mechanism counts, "
        "emission splits summing away from 65535, max UIDs or take bounds. Check the specific "
        "argument against the min/max storage items the extrinsic validates (e.g. "
        "`MinAllowedUids`, `MaxMechanismCount`)."
    ),
    "InvalidVotingPowerEmaAlpha": (
        "The alpha passed to set the voting power EMA exceeds 10^18, which represents 1.0. "
        "Check the `alpha` argument to sudo_set_voting_power_ema_alpha; it must be at most "
        "10^18, and the current value is in `VotingPowerEmaAlpha` per netuid."
    ),
    "InvalidWorkBlock": (
        "The `block_number` in the proof-of-work submission is in the future or more than 3 "
        "blocks old, so the work is stale. Compare the submitted block number with the current "
        "chain height and regenerate the PoW against a fresh block."
    ),
    "KeepStakeBlockedByCollateral": (
        "A hotkey swap with keep_stake=true was refused because the old hotkey still has "
        "standing miner registration collateral. keep_stake leaves stake on the old key "
        "while the UID moves, which would strand the bond. Retry with keep_stake=false so "
        "collateral migrates with the UID; on-chain hotkey lineage maps track the rename "
        "for blacklist continuity. Or drain the bond through earned emission first."
    ),
    "LeaseCannotEndInThePast": (
        "The `end_block` supplied when registering a leased network is not after the current "
        "block. Check the current chain height and pass an `end_block` in the future, or omit "
        "it for a perpetual lease."
    ),
    "LeaseDoesNotExist": (
        "The `lease_id` argument does not correspond to any stored lease. Query the "
        "`SubnetLeases` storage map to confirm the lease id and whether it was already "
        "terminated."
    ),
    "LeaseHasNoEndBlock": (
        "The lease being terminated is perpetual (its `end_block` is None), so it can never be "
        "ended this way. Check the lease's `end_block` field in `SubnetLeases` for the given "
        "lease id."
    ),
    "LeaseHasNotEnded": (
        "The lease termination was attempted before the lease's `end_block` was reached. "
        "Compare the current block height with the `end_block` stored in `SubnetLeases` for the "
        "lease id and retry after it passes."
    ),
    "LeaseNetuidNotFound": (
        "After registering the leased network, no subnet owned by the lease's derived coldkey "
        "could be found, so the netuid lookup failed. Inspect `SubnetOwner` entries for the "
        "lease coldkey; this usually indicates the registration did not complete."
    ),
    "LiquidAlphaDisabled": (
        "Setting `alpha_low`/`alpha_high` was attempted while liquid alpha is disabled on the "
        "subnet. Check the `LiquidAlphaOn` hyperparameter (`btcli sudo get --netuid <n>`) and "
        "have the subnet owner enable liquid alpha first."
    ),
    "LockHotkeyMismatch": (
        "A conviction lock on this subnet is bound to a different hotkey than the one in the "
        "call. One coldkey may lock to only one hotkey per subnet: topping up `lock_stake` "
        "must reuse that hotkey (`btcli lock move` to retarget), and a stake transfer that "
        "moves locked alpha must land on the receiver's existing lock hotkey "
        "(`btcli stake transfer --destination-hotkey <lock-hotkey>`). Check the lock with "
        "`btcli lock show --netuid <n>` / `btcli stake list` (locked · free · lock → hotkey)."
    ),
    "LockIdOverFlow": (
        "The global network-registration lock id counter reached its u32 maximum while queueing "
        "a subnet registration, so no new lock could be created. Check the "
        "`NetworkRegistrationLockId` storage value; this indicates lock id exhaustion, not a "
        "balance problem."
    ),
    "MaxWeightExceeded": (
        "After normalization, one of the submitted weights exceeds the subnet's maximum weight "
        "limit (self-weight is exempt). Check the `MaxWeightsLimit` hyperparameter "
        "(`btcli sudo get --netuid <n>`) and flatten the weight vector before setting."
    ),
    "MechanismDoesNotExist": (
        "The target subnet or its sub-mechanism does not exist: the netuid is unknown, the "
        "mechanism index is at or above `MechanismCountCurrent`, or a non-dynamic `mechid` was "
        "requested. Check the netuid with `btcli subnets list` and the mechanism count for that "
        "subnet."
    ),
    "NeedWaitingMoreBlocksToStarCall": (
        "The subnet owner ran `btcli sudo start` before enough blocks had passed since the "
        "subnet was registered. Check readiness with `btcli sudo check-start` and retry once "
        "the window opens."
    ),
    "NetworkDissolveAlreadyQueued": (
        "The subnet is already in the dissolve cleanup queue, so it cannot be queued for "
        "dissolution again. Check the `DissolveCleanupQueue` storage value for the netuid "
        "before submitting another dissolve request."
    ),
    "NetworkTxRateLimitExceeded": (
        "The coldkey attempted register_network again before the network registration rate "
        "limit elapsed since its previous registration. Check the coldkey's last "
        "register-network block against the `NetworkRateLimit` storage value and wait the "
        "remaining blocks."
    ),
    "NeuronNoValidatorPermit": (
        "The hotkey tried to set weights on other neurons without holding a validator permit on "
        "that subnet. Check `ValidatorPermit` for the neuron's uid (e.g. via the metagraph or "
        "`btcli subnets metagraph`) and whether its stake ranks it as a validator."
    ),
    "NewColdKeyIsHotkey": (
        "The proposed new coldkey in a coldkey swap is already an existing hotkey account, "
        "which is not allowed. Check the `Owner` storage for the candidate key to confirm it is "
        "not registered as a hotkey, and pick a fresh coldkey."
    ),
    "NewHotKeyIsSameWithOld": (
        "swap_hotkey was called with `new_hotkey` equal to the current hotkey, so there is "
        "nothing to swap. Check the extrinsic arguments and supply a different destination "
        "hotkey."
    ),
    "NewHotKeyNotCleanForRootSwap": (
        "The destination hotkey has pending root claimable dividends, non-zero root stake, or "
        "root-claimed history, so root accounting cannot merge safely. Check `RootClaimable` "
        "and root-subnet stake for the new hotkey; claim or clear them, or use a fresh hotkey."
    ),
    "NoExistingLock": (
        "move_lock was called but no conviction lock exists for the signing coldkey on that "
        "subnet. Check the lock storage for the coldkey and netuid, and create a lock before "
        "attempting to move it to another hotkey."
    ),
    "NoNeuronIdAvailable": (
        "Registration could not obtain a uid: the subnet's `MaxAllowedUids` is 0, or the subnet "
        "is full and every neuron is immune from pruning. Check `SubnetworkN` versus "
        "`MaxAllowedUids` for the netuid and retry after immunity periods expire."
    ),
    "NoWeightsCommitFound": (
        "A weights reveal was submitted but no pending (non-expired) commit exists for the "
        "hotkey and netuid, possibly because it already expired. Query the `WeightCommits` "
        "storage map for the hotkey and check the commit hasn't passed the reveal window."
    ),
    "NonAssociatedColdKey": (
        "The signing coldkey does not own the hotkey it is trying to operate on (stake, swap, "
        "serve, children or take changes). Check the `Owner` storage entry for the hotkey, e.g. "
        "via `btcli wallet overview`, and sign with the coldkey that registered it."
    ),
    "NotEnoughAlphaOutToRecycle": (
        "A recycle or burn of alpha requested more than the subnet's outstanding alpha supply. "
        "Compare the amount against the `SubnetAlphaOut` storage value for the netuid and "
        "reduce the recycle amount."
    ),
    "NotEnoughBalanceToPaySwapColdKey": (
        "The coldkey's free TAO balance cannot cover the coldkey swap cost, which is recycled "
        "when the swap executes. Check the balance with `btcli wallet balance` against the swap "
        "cost and top up before `btcli wallet swap-coldkey`."
    ),
    "NotEnoughBalanceToPaySwapHotKey": (
        "The coldkey's free TAO balance is below the hotkey swap cost (a per-subnet cost "
        "applies when swapping on a single netuid). Check `btcli wallet balance` against the "
        "key swap cost and fund the coldkey before retrying."
    ),
    "NotEnoughBalanceToStake": (
        "The coldkey's free balance is less than the TAO required, either the stake amount in "
        "add_stake or the burn cost of a registration. Check `btcli wallet balance` against the "
        "amount or the current registration burn (`Burn` storage for the netuid)."
    ),
    "NotEnoughStake": (
        "The caller's hotkey holds less stake than the action requires; a generic "
        "insufficient-stake failure on staking-related calls. Check the hotkey's stake on the "
        "relevant subnet, e.g. `btcli stake list`, against the amount the extrinsic needs."
    ),
    "NotEnoughStakeToSetChildkeys": (
        "Raised by `set_children` when the parent hotkey's total stake is below "
        "`StakeThreshold` and it is not the subnet owner hotkey. Compare the hotkey's total "
        "stake (`btcli stake list`) against the `StakeThreshold` storage value."
    ),
    "NotEnoughStakeToSetWeights": (
        "Setting or committing weights failed because the hotkey's stake weight on the subnet "
        "is below `StakeThreshold` (the weights-min-stake floor); the subnet owner hotkey is "
        "exempt. Check the hotkey's stake on that netuid against `StakeThreshold`."
    ),
    "NotEnoughStakeToWithdraw": (
        "An unstake, stake move, swap, or transfer requested more alpha than the hotkey-coldkey "
        "pair holds on that subnet. Compare the requested amount against that specific "
        "position (`btcli stake list`) — conviction locks are coldkey-wide, so stake may sit "
        "on a different hotkey than the lock target; move stake onto the origin hotkey first "
        "or pass the hotkey that actually holds the balance."
    ),
    "NotRootSubnet": (
        "A call that only operates on the root network, such as setting root network weights, "
        "was given a non-root netuid. Check the netuid argument; root operations must target "
        "netuid 0."
    ),
    "NotSubnetOwner": (
        "The signing coldkey is not the recorded owner of the subnet it tried to administer "
        "(e.g. setting subnet identity or owner-only hyperparameters). Compare the caller "
        "against the `SubnetOwner` storage entry for that netuid."
    ),
    "Overflow": (
        "A checked arithmetic operation overflowed, e.g. incrementing `NextSubnetLeaseId` when "
        "registering a leased network, or adding to a crowdloan's `raised` amount or "
        "contributor count. Internal guard; inspect the amounts involved as this should not "
        "occur with realistic values."
    ),
    "ProportionOverflow": (
        "The child proportions passed to `set_children` sum to more than u64::MAX. Reduce the "
        "per-child proportion values so their total fits in a u64; each proportion is a "
        "fraction of u64::MAX."
    ),
    "RegistrationNotPermittedOnRootSubnet": (
        "A neuron registration or child-hotkey operation (`register`, `burned_register`, "
        "`set_children`) was called with the root netuid, where these calls are invalid. Check "
        "the netuid argument; use a regular subnet, or `root_register` for root membership."
    ),
    "RegistrationPriceLimitExceeded": (
        "`burned_register` with a price limit failed because the subnet's current registration "
        "burn cost exceeds the supplied `limit_price`. Check the current burn via `Burn` "
        "storage or `btcli subnets list` and raise the limit or wait for the cost to decay."
    ),
    "RevealPeriodTooLarge": (
        "`set_reveal_period` was given a commit-reveal period above the compiled-in maximum "
        "number of epochs. Lower the `reveal_period` argument; the current setting is readable "
        "from `RevealPeriodEpochs` for the netuid."
    ),
    "RevealPeriodTooSmall": (
        "`set_reveal_period` was given a commit-reveal period below the compiled-in minimum "
        "number of epochs. Raise the `reveal_period` argument; the current setting is readable "
        "from `RevealPeriodEpochs` for the netuid."
    ),
    "RevealTooEarly": (
        "A weight reveal was submitted before the commit's reveal window: the current epoch "
        "must equal the commit epoch plus the reveal period. Check the commit in "
        "`WeightCommits` and the subnet's `RevealPeriodEpochs`, then wait for the reveal epoch."
    ),
    "RootNetworkDoesNotExist": (
        "Root registration or root stake claiming found no root network in chain state, which "
        "only happens on misconfigured or freshly bootstrapped chains. Verify netuid 0 exists "
        "in `NetworksAdded`."
    ),
    "RootWeightSettingDisabled": (
        "`set_root_weights` is disabled network-wide: Root Reborn launched gated, so every "
        "fund runs the null strategy (dividends accumulate in place on their origin subnet) "
        "until weight setting is switched on by governance or a later upgrade. No action "
        "available — wait for the enable; dividends keep accruing meanwhile."
    ),
    "RootStakeLocked": (
        "A root (netuid 0) exit was attempted before `RootStakeUnlockInterval` blocks "
        "elapsed since the coldkey/hotkey's last root stake add, remove, or claim. Applies to "
        "`remove_stake` and to move/swap/transfer off root. Check "
        "`LastColdkeyHotkeyStakeBlock` against the current block and wait out the hold window "
        "(or leave the interval at 0 via sudo if the lock is not intended)."
    ),
    "SameAutoStakeHotkeyAlreadySet": (
        "The coldkey tried to set its auto-stake destination on a subnet to the hotkey that is "
        "already configured. Read `AutoStakeDestination` for the coldkey and netuid before "
        "calling; only a different hotkey is accepted."
    ),
    "SameNetuid": (
        "A stake swap or move where coldkey, hotkey, and subnet are all unchanged, so "
        "`origin_netuid` equals `destination_netuid` with nothing to transition. Check the call "
        "arguments; at least the subnet or one of the keys must differ."
    ),
    "ServingRateLimitExceeded": (
        "`serve_axon` or `serve_prometheus` was called again before enough blocks passed since "
        "the neuron's last serving update. Check the axon's last update block in `Axons` "
        "against the `ServingRateLimit` (serving_rate_limit hyperparameter) and wait."
    ),
    "SettingWeightsTooFast": (
        "The neuron set weights again before `WeightsSetRateLimit` blocks elapsed since its "
        "last weight update on that subnet. Check the weights_rate_limit hyperparameter and the "
        "neuron's `LastUpdate` entry, then wait the remaining blocks."
    ),
    "SlippageTooHigh": (
        "A stake, unstake, or move would move the price past the slippage-protection limit "
        "(default 5% tolerance). Retry, raise `--rate-tolerance`, or disable protection with "
        "`--no-slippage-protection`; check the current price with `btcli subnets price`."
    ),
    "StakeTooLowForRoot": (
        "Retired. `root_register` used to raise this when the root network was full and the "
        "hotkey's netuid-0 stake did not exceed the lowest-staked root member; root admission "
        "is now burn-based (the coldkey pays the root burn price), so this error is no longer "
        "raised."
    ),
    "StakeUnavailable": (
        "An unstake or same-subnet stake transfer would dip into stake that is still reserved: "
        "the requested alpha exceeds the coldkey's free balance on that subnet after subtracting "
        "conviction `Lock` and any miner registration collateral locked against hotkeys the "
        "coldkey owns. Check `Lock` and `MinerCollateral` for the netuid; only total stake minus "
        "those reservations can move."
    ),
    "StakingRateLimitExceeded": (
        "Staking operations (add_stake, remove_stake, and similar) were submitted faster than "
        "the per-block staking rate limit allows for the hotkey-coldkey pair. Space the "
        "transactions out and retry in a later block."
    ),
    "StartCallNotReady": (
        "`btcli sudo start` was run before `StartCallDelay` blocks elapsed since the subnet "
        "was registered. Check readiness with `btcli sudo check-start` and wait for the "
        "remainder."
    ),
    "SubNetRegistrationDisabled": (
        "Neuron registration is switched off: either the subnet's `NetworkRegistrationAllowed` "
        "flag is false, or network creation has not opened yet (`NetworkRegistrationStartBlock` "
        "is in the future). Check the network_registration_allowed hyperparameter for the "
        "netuid."
    ),
    "SubnetBuybackRateLimitExceeded": (
        "A subnet buyback operation (staking TAO and immediately burning the acquired alpha, "
        "e.g. via `add_stake_burn`) was repeated within its rate-limit window. Wait for the "
        "window to pass before retrying the buyback."
    ),
    "SubnetLimitReached": (
        "`register_network` failed because the subnet count is at the network limit and no "
        "existing subnet is eligible to be pruned. Check the number of registered subnets "
        "(`btcli subnets list`) against the subnet limit and retry once a subnet becomes "
        "prunable."
    ),
    "SubnetNotExists": (
        "The netuid passed to the call does not correspond to a registered subnet. Verify the "
        "netuid argument against `NetworksAdded` or `btcli subnets list`; the subnet may also "
        "have been dissolved."
    ),
    "SubtokenDisabled": (
        "The subnet's alpha token is not yet enabled, so staking, swapping, and trading on it "
        "are blocked until the owner runs `btcli sudo start` after registration. Check "
        "readiness with `btcli sudo check-start`."
    ),
    "SymbolAlreadyInUse": (
        "The token symbol requested for the subnet is already assigned to another subnet. Scan "
        "`TokenSymbol` across netuids and pick a symbol that is not taken."
    ),
    "SymbolDoesNotExist": (
        "The requested token symbol is not in the chain's predefined symbol table, so it cannot "
        "be assigned to a subnet. Check the symbol argument against the chain's built-in "
        "`SYMBOLS` list and choose a valid entry."
    ),
    "TempoOutOfBounds": (
        "The subnet owner gave `sudo_set_tempo` a tempo outside the allowed MIN_TEMPO to "
        "MAX_TEMPO range (360-50,400 blocks). Check the tempo argument against those chain "
        "constants and pick a value inside the bounds; only root may set a tempo outside them."
    ),
    "TooManyChildren": (
        "`set_children` was called with more than 5 child hotkeys for a parent on the subnet. "
        "Trim the children list to at most 5 entries."
    ),
    "TooManyRegistrationsThisBlock": (
        "Registrations in the current block already reached the subnet's per-block cap "
        "(`MaxRegistrationsPerBlock`, the max_regs_per_block hyperparameter); root registration "
        "enforces the same cap on netuid 0. Retry in the next block."
    ),
    "TooManyRegistrationsThisInterval": (
        "Registrations in the current interval reached the cap of three times "
        "`TargetRegistrationsPerInterval` for the subnet. Compare `RegistrationsThisInterval` "
        "against that hyperparameter and wait for the next interval to start."
    ),
    "TooManyUIDsPerMechanism": (
        "Setting max UIDs or mechanism count would make max_uids times mechanism_count exceed "
        "the chain default of 256 UIDs per subnet. Check `MaxAllowedUids` and the subnet's "
        "mechanism count so their product stays within the limit."
    ),
    "TooManyUnrevealedCommits": (
        "`commit_weights` (or a CRv3 commit) failed because the hotkey already has 10 "
        "unrevealed commits queued on the subnet. Inspect `WeightCommits` for the hotkey and "
        "reveal or let old commits expire before committing again."
    ),
    "TransactorAccountShouldBeHotKey": (
        "The extrinsic must be signed by the hotkey itself, but a different account (typically "
        "the coldkey) was the origin. Check which key signs the transaction; calls like axon "
        "serving expect the hotkey as origin."
    ),
    "TransferDisallowed": (
        "A stake transfer or cross-subnet move was attempted while the origin or destination "
        "subnet has stake transfers switched off. Check the `TransferToggle` storage for both "
        "netuids involved; the subnet owner must enable transfers first."
    ),
    "TrimmingWouldExceedMaxImmunePercentage": (
        "Trimming the subnet's UIDs cannot proceed because immune neurons would make up at "
        "least the maximum immune share (80%) of the reduced slot count. Check neurons still in "
        "`ImmunityPeriod` and retry after their immunity lapses or with a higher max UID "
        "target."
    ),
    "TxChildkeyTakeRateLimitExceeded": (
        "`set_childkey_take` was called again for the hotkey on this subnet before its "
        "rate-limit window elapsed. Check the block of the last childkey-take change against "
        "`TxChildkeyTakeRateLimit` and wait out the remainder."
    ),
    "TxRateLimitExceeded": (
        "An owner or admin transaction (e.g. `set_children`, tempo or hyperparameter updates, "
        "setting the owner hotkey) was repeated within its rate-limit window for that key and "
        "subnet. Check when the same transaction type last succeeded and wait for the limit to "
        "pass."
    ),
    "UidMapCouldNotBeCleared": (
        "During a UID reshuffle or trim, clearing the subnet's `Uids` map left residual entries "
        "(the storage clear returned a cursor). Internal state inconsistency rather than a "
        "caller error; inspect the `Uids` storage for the netuid and report it."
    ),
    "UidVecContainInvalidOne": (
        "The weight submission includes a UID that is not registered on the subnet, i.e. at "
        "least one entry is not below `SubnetworkN`. Check the `uids` argument against the "
        "subnet's neuron count in the metagraph (`btcli subnets metagraph`)."
    ),
    "UidsLengthExceedUidsInSubNet": (
        "The weight submission contains more UID entries than there are neurons registered on "
        "the subnet. Compare the length of the `uids` argument against `SubnetworkN` for the "
        "netuid and trim the vector."
    ),
    "UnableToRecoverPublicKey": (
        "While associating an EVM key, the secp256k1 public key recovered from the supplied "
        "signature could not be parsed. Check that the signature was produced by signing the "
        "expected EIP-191 message (hotkey plus block hash) with the EVM private key."
    ),
    "UnlockAmountTooHigh": (
        "An unlock requested more alpha than remains locked for the coldkey on that subnet. "
        "Check the lock's remaining decaying locked mass in the `Lock` storage entry and "
        "request at most that amount."
    ),
    "VotingPowerTrackingNotEnabled": (
        "Disabling voting power tracking was requested on a subnet where tracking is not "
        "currently active. Check the `VotingPowerTrackingEnabled` storage flag for the netuid "
        "before calling the disable extrinsic."
    ),
    "WaitingForDissolvedSubnetCleanup": (
        "The operation is blocked while a dissolved subnet's storage is still being torn down "
        "in the background. Check the `DissolveCleanupQueue` and retry after the on-idle "
        "cleanup for that netuid completes."
    ),
    "WeightVecLengthIsLow": (
        "The weight submission has fewer entries than the subnet's minimum (setting only a "
        "self-weight is the one exception). Compare the vector length against the "
        "`MinAllowedWeights` (min_allowed_weights hyperparameter) for the netuid."
    ),
    "WeightVecNotEqualSize": (
        "The `uids` and `values` vectors passed to a weight-setting call have different "
        "lengths, so they cannot be paired. Check the call arguments; both vectors must have "
        "exactly one value per UID."
    ),
    "ZeroBalanceAfterWithdrawn": (
        "Withdrawing TAO from the coldkey (e.g. paying a registration burn or adding stake) "
        "would leave the account at zero, below what keeps it alive. Check the coldkey's free "
        "balance and leave at least the existential deposit after the amount withdrawn."
    ),
}
