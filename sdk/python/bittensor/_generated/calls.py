"""Generated from runtime metadata by codegen. DO NOT EDIT BY HAND.

Regenerate with: python -m codegen <ws-endpoint>
Spec version: 428
"""
from typing import Any, NamedTuple


class Call(NamedTuple):
    """A composed call target: (module, function, params).

    A typed 3-tuple, so calls are trivially inspectable and testable.
    """

    module: str
    function: str
    params: dict[str, Any]


class System:
    """Call builders for the System pallet."""

    @staticmethod
    def apply_authorized_upgrade(code) -> Call:
        "Provide the preimage (runtime binary) `code` for an upgrade that has been authorized.  If the authorization required a version check, this call will ensure the spec name remains unchanged and that the spec version has increased.  Depending on the runtime's `OnSetCode` configuration, this function may directly apply the new `code` in the same block or attempt to schedule the upgrade.  All origins are allowed."
        return Call('System', 'apply_authorized_upgrade', {'code': code})

    @staticmethod
    def authorize_upgrade(code_hash) -> Call:
        'Authorize an upgrade to a given `code_hash` for the runtime. The runtime can be supplied later.  This call requires Root origin.'
        return Call('System', 'authorize_upgrade', {'code_hash': code_hash})

    @staticmethod
    def authorize_upgrade_without_checks(code_hash) -> Call:
        'Authorize an upgrade to a given `code_hash` for the runtime. The runtime can be supplied later.  WARNING: This authorizes an upgrade that will take place without any safety checks, for example that the spec name remains the same and that the version number increases. Not recommended for normal use. Use `authorize_upgrade` instead.  This call requires Root origin.'
        return Call('System', 'authorize_upgrade_without_checks', {'code_hash': code_hash})

    @staticmethod
    def kill_prefix(prefix, subkeys) -> Call:
        'Kill all storage items with a key that starts with the given prefix.  **NOTE:** We rely on the Root origin to provide us the number of subkeys under the prefix we are removing to accurately calculate the weight of this function.'
        return Call('System', 'kill_prefix', {'prefix': prefix, 'subkeys': subkeys})

    @staticmethod
    def kill_storage(keys) -> Call:
        'Kill some items from storage.'
        return Call('System', 'kill_storage', {'keys': keys})

    @staticmethod
    def remark(remark) -> Call:
        'Make some on-chain remark.  Can be executed by every `origin`.'
        return Call('System', 'remark', {'remark': remark})

    @staticmethod
    def remark_with_event(remark) -> Call:
        'Make some on-chain remark and emit event.'
        return Call('System', 'remark_with_event', {'remark': remark})

    @staticmethod
    def set_code(code) -> Call:
        'Set the new runtime code.'
        return Call('System', 'set_code', {'code': code})

    @staticmethod
    def set_code_without_checks(code) -> Call:
        'Set the new runtime code without doing any checks of the given `code`.  Note that runtime upgrades will not run if this is called with a not-increasing spec version!'
        return Call('System', 'set_code_without_checks', {'code': code})

    @staticmethod
    def set_heap_pages(pages) -> Call:
        "Set the number of pages in the WebAssembly environment's heap."
        return Call('System', 'set_heap_pages', {'pages': pages})

    @staticmethod
    def set_storage(items) -> Call:
        'Set some items of storage.'
        return Call('System', 'set_storage', {'items': items})


class Timestamp:
    """Call builders for the Timestamp pallet."""

    @staticmethod
    def set(now) -> Call:
        "Set the current time.  This call should be invoked exactly once per block. It will panic at the finalization phase, if this call hasn't been invoked by that time.  The timestamp should be greater than the previous one by the amount specified by [`Config::MinimumPeriod`].  The dispatch origin for this call must be _None_.  This dispatch class is _Mandatory_ to ensure it gets executed in the block. Be aware that changing the complexity of this call could result exhausting the resources in a block to execute any other calls.  ## Complexity - `O(1)` (Note that implementations of `OnTimestampSet` must also be `O(1)`) - 1 storage read and 1 storage mutation (codec `O(1)` because of `DidUpdate::take` in `on_finalize`) - 1 event handler `on_timestamp_set`. Must be `O(1)`."
        return Call('Timestamp', 'set', {'now': now})


class Grandpa:
    """Call builders for the Grandpa pallet."""

    @staticmethod
    def note_stalled(delay, best_finalized_block_number) -> Call:
        'Note that the current authority set of the GRANDPA finality gadget has stalled.  This will trigger a forced authority set change at the beginning of the next session, to be enacted `delay` blocks after that. The `delay` should be high enough to safely assume that the block signalling the forced change will not be re-orged e.g. 1000 blocks. The block production rate (which may be slowed down because of finality lagging) should be taken into account when choosing the `delay`. The GRANDPA voters based on the new authority will start voting on top of `best_finalized_block_number` for new finalized blocks. `best_finalized_block_number` should be the highest of the latest finalized block of all validators of the new authority set.  Only callable by root.'
        return Call('Grandpa', 'note_stalled', {'delay': delay, 'best_finalized_block_number': best_finalized_block_number})

    @staticmethod
    def report_equivocation(equivocation_proof, key_owner_proof) -> Call:
        'Report voter equivocation/misbehavior. This method will verify the equivocation proof and validate the given key ownership proof against the extracted offender. If both are valid, the offence will be reported.'
        return Call('Grandpa', 'report_equivocation', {'equivocation_proof': equivocation_proof, 'key_owner_proof': key_owner_proof})

    @staticmethod
    def report_equivocation_unsigned(equivocation_proof, key_owner_proof) -> Call:
        'Report voter equivocation/misbehavior. This method will verify the equivocation proof and validate the given key ownership proof against the extracted offender. If both are valid, the offence will be reported.  This extrinsic must be called unsigned and it is expected that only block authors will call it (validated in `ValidateUnsigned`), as such if the block author is defined it will be defined as the equivocation reporter.'
        return Call('Grandpa', 'report_equivocation_unsigned', {'equivocation_proof': equivocation_proof, 'key_owner_proof': key_owner_proof})


class Balances:
    """Call builders for the Balances pallet."""

    @staticmethod
    def burn(value, keep_alive) -> Call:
        "Burn the specified liquid free balance from the origin account.  If the origin's account ends up below the existential deposit as a result of the burn and `keep_alive` is false, the account will be reaped.  Unlike sending funds to a _burn_ address, which merely makes the funds inaccessible, this `burn` operation will reduce total issuance by the amount _burned_."
        return Call('Balances', 'burn', {'value': value, 'keep_alive': keep_alive})

    @staticmethod
    def force_adjust_total_issuance(direction, delta) -> Call:
        'Adjust the total issuance in a saturating way.  Can only be called by root and always needs a positive `delta`.  # Example'
        return Call('Balances', 'force_adjust_total_issuance', {'direction': direction, 'delta': delta})

    @staticmethod
    def force_set_balance(who, new_free) -> Call:
        'Set the regular balance of a given account.  The dispatch origin for this call is `root`.'
        return Call('Balances', 'force_set_balance', {'who': who, 'new_free': new_free})

    @staticmethod
    def force_transfer(source, dest, value) -> Call:
        'Exactly as `transfer_allow_death`, except the origin must be root and the source account may be specified.'
        return Call('Balances', 'force_transfer', {'source': source, 'dest': dest, 'value': value})

    @staticmethod
    def force_unreserve(who, amount) -> Call:
        'Unreserve some balance from a user by force.  Can only be called by ROOT.'
        return Call('Balances', 'force_unreserve', {'who': who, 'amount': amount})

    @staticmethod
    def transfer_all(dest, keep_alive) -> Call:
        'Transfer the entire transferable balance from the caller account.  NOTE: This function only attempts to transfer _transferable_ balances. This means that any locked, reserved, or existential deposits (when `keep_alive` is `true`), will not be transferred by this function. To ensure that this function results in a killed account, you might need to prepare the account by removing any reference counters, storage deposits, etc...  The dispatch origin of this call must be Signed.  - `dest`: The recipient of the transfer. - `keep_alive`: A boolean to determine if the `transfer_all` operation should send all of the funds the account has, causing the sender account to be killed (false), or transfer everything except at least the existential deposit, which will guarantee to keep the sender account alive (true).'
        return Call('Balances', 'transfer_all', {'dest': dest, 'keep_alive': keep_alive})

    @staticmethod
    def transfer_allow_death(dest, value) -> Call:
        "Transfer some liquid free balance to another account.  `transfer_allow_death` will set the `FreeBalance` of the sender and receiver. If the sender's account is below the existential deposit as a result of the transfer, the account will be reaped.  The dispatch origin for this call must be `Signed` by the transactor."
        return Call('Balances', 'transfer_allow_death', {'dest': dest, 'value': value})

    @staticmethod
    def transfer_keep_alive(dest, value) -> Call:
        'Same as the [`transfer_allow_death`] call, but with a check that the transfer will not kill the origin account.  99% of the time you want [`transfer_allow_death`] instead.  [`transfer_allow_death`]: struct.Pallet.html#method.transfer'
        return Call('Balances', 'transfer_keep_alive', {'dest': dest, 'value': value})

    @staticmethod
    def upgrade_accounts(who) -> Call:
        'Upgrade a specified account.  - `origin`: Must be `Signed`. - `who`: The account to be upgraded.  This will waive the transaction fee if at least all but 10% of the accounts needed to be upgraded. (We let some not have to be upgraded just in order to allow for the possibility of churn).'
        return Call('Balances', 'upgrade_accounts', {'who': who})


class SubtensorModule:
    """Call builders for the SubtensorModule pallet."""

    @staticmethod
    def add_stake(hotkey, netuid, amount_staked) -> Call:
        "--- Adds stake to a hotkey. The call is made from a coldkey account. This delegates stake to the hotkey.  Note: the coldkey account may own the hotkey, in which case they are delegating to themselves.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller's coldkey.  * 'hotkey' (T::AccountId): - The associated hotkey account.  * 'netuid' (u16): - Subnetwork UID  * 'amount_staked' (u64): - The amount of stake to be added to the hotkey staking account.  # Event: * StakeAdded; - On the successfully adding stake to a global account.  # Raises: * 'NotEnoughBalanceToStake': - Not enough balance on the coldkey to add onto the global account.  * 'NonAssociatedColdKey': - The calling coldkey is not associated with this hotkey.  * 'BalanceWithdrawalError': - Errors stemming from transaction pallet."
        return Call('SubtensorModule', 'add_stake', {'hotkey': hotkey, 'netuid': netuid, 'amount_staked': amount_staked})

    @staticmethod
    def add_stake_burn(hotkey, netuid, amount, limit) -> Call:
        '--- The extrinsic is a combination of add_stake(add_stake_limit) and burn_alpha. We buy alpha token first and immediately burn the acquired amount of alpha (aka Subnet buyback).'
        return Call('SubtensorModule', 'add_stake_burn', {'hotkey': hotkey, 'netuid': netuid, 'amount': amount, 'limit': limit})

    @staticmethod
    def add_stake_limit(hotkey, netuid, amount_staked, limit_price, allow_partial) -> Call:
        "--- Adds stake to a hotkey on a subnet with a price limit. This extrinsic allows to specify the limit price for alpha token at which or better (lower) the staking should execute.  In case if slippage occurs and the price shall move beyond the limit price, the staking order may execute only partially or not execute at all.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller's coldkey.  * 'hotkey' (T::AccountId): - The associated hotkey account.  * 'netuid' (u16): - Subnetwork UID  * 'amount_staked' (u64): - The amount of stake to be added to the hotkey staking account.  * 'limit_price' (u64): - The limit price expressed in units of RAO per one Alpha.  * 'allow_partial' (bool): - Allows partial execution of the amount. If set to false, this becomes fill or kill type or order.  # Event: * StakeAdded; - On the successfully adding stake to a global account.  # Raises: * 'NotEnoughBalanceToStake': - Not enough balance on the coldkey to add onto the global account.  * 'NonAssociatedColdKey': - The calling coldkey is not associated with this hotkey.  * 'BalanceWithdrawalError': - Errors stemming from transaction pallet."
        return Call('SubtensorModule', 'add_stake_limit', {'hotkey': hotkey, 'netuid': netuid, 'amount_staked': amount_staked, 'limit_price': limit_price, 'allow_partial': allow_partial})

    @staticmethod
    def announce_coldkey_swap(new_coldkey_hash) -> Call:
        'Announces a coldkey swap using BlakeTwo256 hash of the new coldkey.  This is required before the coldkey swap can be performed after the delay period.  It can be reannounced after a delay of `ColdkeySwapReannouncementDelay` following the first valid execution block of the original announcement.  The dispatch origin of this call must be the original coldkey that made the announcement.  - `new_coldkey_hash`: The hash of the new coldkey using BlakeTwo256.  The `ColdkeySwapAnnounced` event is emitted on successful announcement.'
        return Call('SubtensorModule', 'announce_coldkey_swap', {'new_coldkey_hash': new_coldkey_hash})

    @staticmethod
    def associate_evm_key(netuid, evm_key, block_number, signature) -> Call:
        'Attempts to associate a hotkey with an EVM key.  The signature will be checked to see if the recovered public key matches the `evm_key` provided.  The EVM key is expected to sign the message according to this formula to produce the signature: ```text keccak_256(hotkey ++ keccak_256(block_number)) ```  # Arguments * `origin` - The origin of the transaction, which must be signed by the `hotkey`. * `netuid` - The netuid that the `hotkey` belongs to. * `evm_key` - The EVM key to associate with the `hotkey`. * `block_number` - The block number used in the `signature`. * `signature` - A signed message by the `evm_key` containing the `hotkey` and the hashed `block_number`.  # Errors Returns an error if: * The transaction is not signed. * The hotkey does not belong to the subnet identified by the netuid. * The EVM key cannot be recovered from the signature. * The EVM key recovered from the signature does not match the given EVM key.  # Events May emit a `EvmKeyAssociated` event on success'
        return Call('SubtensorModule', 'associate_evm_key', {'netuid': netuid, 'evm_key': evm_key, 'block_number': block_number, 'signature': signature})

    @staticmethod
    def batch_commit_weights(netuids, commit_hashes) -> Call:
        '--- Allows a hotkey to commit weight hashes for multiple netuids as a batch.  # Args: * `origin`: (<T as frame_system::Config>Origin): - The caller, a hotkey who wishes to set their weights.  * `netuids` (Vec<Compact<u16>>): - The network uids we are setting these weights on.  * `commit_hashes` (Vec<H256>): - The commit hashes to commit.  # Event: * WeightsSet; - On successfully setting the weights on chain. * BatchWeightsCompleted; - On success of the batch. * BatchCompletedWithErrors; - On failure of any of the weights in the batch. * BatchWeightItemFailed; - On failure for each failed item in the batch.'
        return Call('SubtensorModule', 'batch_commit_weights', {'netuids': netuids, 'commit_hashes': commit_hashes})

    @staticmethod
    def batch_reveal_weights(netuid, uids_list, values_list, salts_list, version_keys) -> Call:
        '---- The implementation for batch revealing committed weights.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The signature of the revealing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `uids_list` (`Vec<Vec<u16>>`): - A list of uids for each set of weights being revealed.  * `values_list` (`Vec<Vec<u16>>`): - A list of values for each set of weights being revealed.  * `salts_list` (`Vec<Vec<u16>>`): - A list of salts used to generate the commit hashes.  * `version_keys` (`Vec<u64>`): - A list of network version keys.  # Raises: * `CommitRevealDisabled`: - Attempting to reveal weights when the commit-reveal mechanism is disabled.  * `NoWeightsCommitFound`: - Attempting to reveal weights without an existing commit.  * `ExpiredWeightCommit`: - Attempting to reveal a weight commit that has expired.  * `RevealTooEarly`: - Attempting to reveal weights outside the valid reveal period.  * `InvalidRevealCommitHashNotMatch`: - The revealed hash does not match any committed hash.  * `InvalidInputLengths`: - The input vectors are of mismatched lengths.'
        return Call('SubtensorModule', 'batch_reveal_weights', {'netuid': netuid, 'uids_list': uids_list, 'values_list': values_list, 'salts_list': salts_list, 'version_keys': version_keys})

    @staticmethod
    def batch_set_weights(netuids, weights, version_keys) -> Call:
        '--- Allows a hotkey to set weights for multiple netuids as a batch.  # Args: * `origin`: (<T as frame_system::Config>Origin): - The caller, a hotkey who wishes to set their weights.  * `netuids` (Vec<Compact<u16>>): - The network uids we are setting these weights on.  * `weights` (Vec<Vec<(Compact<u16>, Compact<u16>)>): - The weights to set for each network. [(uid, weight), ...]  * `version_keys` (Vec<Compact<u64>>): - The network version keys to check if the validator is up to date.  # Event: * WeightsSet; - On successfully setting the weights on chain. * BatchWeightsCompleted; - On success of the batch. * BatchCompletedWithErrors; - On failure of any of the weights in the batch. * BatchWeightItemFailed; - On failure for each failed item in the batch.'
        return Call('SubtensorModule', 'batch_set_weights', {'netuids': netuids, 'weights': weights, 'version_keys': version_keys})

    @staticmethod
    def burn_alpha(hotkey, amount, netuid) -> Call:
        'Burns alpha from a cold/hot key pair without reducing `AlphaOut`  # Arguments * `origin` - The origin of the call (must be signed by the coldkey) * `hotkey` - The hotkey account * `amount` - The amount of alpha to burn * `netuid` - The subnet ID  # Events Emits a `TokensBurned` event on success.'
        return Call('SubtensorModule', 'burn_alpha', {'hotkey': hotkey, 'amount': amount, 'netuid': netuid})

    @staticmethod
    def burned_register(netuid, hotkey) -> Call:
        'User register a new subnetwork via burning token'
        return Call('SubtensorModule', 'burned_register', {'netuid': netuid, 'hotkey': hotkey})

    @staticmethod
    def claim_root(subnets) -> Call:
        "--- Claims the root emissions for a coldkey. # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller's coldkey.  # Event: * RootClaimed; - On the successfully claiming the root emissions for a coldkey.  # Raises:"
        return Call('SubtensorModule', 'claim_root', {'subnets': subnets})

    @staticmethod
    def clear_coldkey_swap_announcement() -> Call:
        'Clears a coldkey swap announcement after the reannouncement delay if it has not been disputed.  The `ColdkeySwapCleared` event is emitted on successful clear.'
        return Call('SubtensorModule', 'clear_coldkey_swap_announcement', {})

    @staticmethod
    def commit_crv3_mechanism_weights(netuid, mecid, commit, reveal_round) -> Call:
        '---- Used to commit encrypted commit-reveal v3 weight values to later be revealed.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The committing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `commit` (`Vec<u8>`): - The encrypted compressed commit. The steps for this are: 1. Instantiate [`WeightsTlockPayload`] 2. Serialize it using the `parity_scale_codec::Encode` trait 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336] to produce a [`TLECiphertext<TinyBLS381>`] type. 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.  * reveal_round (`u64`): - The drand reveal round which will be avaliable during epoch `n+1` from the current epoch.  # Raises: * `CommitRevealV3Disabled`: - Attempting to commit when the commit-reveal mechanism is disabled.  * `TooManyUnrevealedCommits`: - Attempting to commit when the user has more than the allowed limit of unrevealed commits.  ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed for mechanisms.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The committing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `mecid` (`u8`): - The u8 mechanism identifier.  * `commit` (`Vec<u8>`): - The encrypted compressed commit. The steps for this are: 1. Instantiate [`WeightsTlockPayload`] 2. Serialize it using the `parity_scale_codec::Encode` trait 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336] to produce a [`TLECiphertext<TinyBLS381>`] type. 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.  * reveal_round (`u64`): - The drand reveal round which will be avaliable during epoch `n+1` from the current epoch.  # Raises: * `CommitRevealV3Disabled`: - Attempting to commit when the commit-reveal mechanism is disabled.  * `TooManyUnrevealedCommits`: - Attempting to commit when the user has more than the allowed limit of unrevealed commits.'
        return Call('SubtensorModule', 'commit_crv3_mechanism_weights', {'netuid': netuid, 'mecid': mecid, 'commit': commit, 'reveal_round': reveal_round})

    @staticmethod
    def commit_mechanism_weights(netuid, mecid, commit_hash) -> Call:
        '---- Used to commit a hash of your weight values to later be revealed for mechanisms.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The signature of the committing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `mecid` (`u8`): - The u8 mechanism identifier.  * `commit_hash` (`H256`): - The hash representing the committed weights.  # Raises: * `CommitRevealDisabled`: - Attempting to commit when the commit-reveal mechanism is disabled.  * `TooManyUnrevealedCommits`: - Attempting to commit when the user has more than the allowed limit of unrevealed commits.'
        return Call('SubtensorModule', 'commit_mechanism_weights', {'netuid': netuid, 'mecid': mecid, 'commit_hash': commit_hash})

    @staticmethod
    def commit_timelocked_mechanism_weights(netuid, mecid, commit, reveal_round, commit_reveal_version) -> Call:
        '---- Used to commit timelock encrypted commit-reveal weight values to later be revealed for a mechanism.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The committing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `mecid` (`u8`): - The u8 mechanism identifier.  * `commit` (`Vec<u8>`): - The encrypted compressed commit. The steps for this are: 1. Instantiate [`WeightsTlockPayload`] 2. Serialize it using the `parity_scale_codec::Encode` trait 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336] to produce a [`TLECiphertext<TinyBLS381>`] type. 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.  * reveal_round (`u64`): - The drand reveal round which will be avaliable during epoch `n+1` from the current epoch.  * commit_reveal_version (`u16`): - The client (bittensor-drand) version'
        return Call('SubtensorModule', 'commit_timelocked_mechanism_weights', {'netuid': netuid, 'mecid': mecid, 'commit': commit, 'reveal_round': reveal_round, 'commit_reveal_version': commit_reveal_version})

    @staticmethod
    def commit_timelocked_weights(netuid, commit, reveal_round, commit_reveal_version) -> Call:
        '---- Used to commit timelock encrypted commit-reveal weight values to later be revealed.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The committing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `commit` (`Vec<u8>`): - The encrypted compressed commit. The steps for this are: 1. Instantiate [`WeightsTlockPayload`] 2. Serialize it using the `parity_scale_codec::Encode` trait 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336] to produce a [`TLECiphertext<TinyBLS381>`] type. 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.  * reveal_round (`u64`): - The drand reveal round which will be avaliable during epoch `n+1` from the current epoch.  * commit_reveal_version (`u16`): - The client (bittensor-drand) version'
        return Call('SubtensorModule', 'commit_timelocked_weights', {'netuid': netuid, 'commit': commit, 'reveal_round': reveal_round, 'commit_reveal_version': commit_reveal_version})

    @staticmethod
    def commit_weights(netuid, commit_hash) -> Call:
        '---- Used to commit a hash of your weight values to later be revealed.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The signature of the committing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `commit_hash` (`H256`): - The hash representing the committed weights.  # Raises: * `CommitRevealDisabled`: - Attempting to commit when the commit-reveal mechanism is disabled.  * `TooManyUnrevealedCommits`: - Attempting to commit when the user has more than the allowed limit of unrevealed commits.'
        return Call('SubtensorModule', 'commit_weights', {'netuid': netuid, 'commit_hash': commit_hash})

    @staticmethod
    def decrease_take(hotkey, take) -> Call:
        "--- Allows delegates to decrease its take value.  # Args: * 'origin': (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * 'hotkey' (T::AccountId): - The hotkey we are delegating (must be owned by the coldkey.)  * 'netuid' (u16): - Subnet ID to decrease take for  * 'take' (u16): - The new stake proportion that this hotkey takes from delegations. The new value can be between 0 and 11_796 and should be strictly lower than the previous value. It T is the new value (rational number), the the parameter is calculated as [65535 * T]. For example, 1% would be [0.01 * 65535] = [655.35] = 655  # Event: * TakeDecreased; - On successfully setting a decreased take for this hotkey.  # Raises: * 'NotRegistered': - The hotkey we are delegating is not registered on the network.  * 'NonAssociatedColdKey': - The hotkey we are delegating is not owned by the calling coldkey.  * 'DelegateTakeTooLow': - The delegate is setting a take which is not lower than the previous."
        return Call('SubtensorModule', 'decrease_take', {'hotkey': hotkey, 'take': take})

    @staticmethod
    def disable_voting_power_tracking(netuid) -> Call:
        'Schedules disabling of voting power tracking for a subnet.  This function can be called by the subnet owner or root. Voting power tracking will continue for 14 days (grace period) after this call, then automatically disable and clear all VotingPower entries for the subnet.  # Arguments: * `origin` - The origin of the call, must be subnet owner or root. * `netuid` - The subnet to schedule disabling voting power tracking for.  # Errors: * `SubnetNotExist` - If the subnet does not exist. * `NotSubnetOwner` - If the caller is not the subnet owner or root. * `VotingPowerTrackingNotEnabled` - If voting power tracking is not enabled.'
        return Call('SubtensorModule', 'disable_voting_power_tracking', {'netuid': netuid})

    @staticmethod
    def dispute_coldkey_swap() -> Call:
        'Dispute a coldkey swap.  This will prevent any further actions on the coldkey swap until triumvirate step in to resolve the issue.  - `coldkey`: The coldkey to dispute the swap for.'
        return Call('SubtensorModule', 'dispute_coldkey_swap', {})

    @staticmethod
    def dissolve_network(coldkey, netuid) -> Call:
        "Remove a user's subnetwork The caller must be the owner of the network"
        return Call('SubtensorModule', 'dissolve_network', {'coldkey': coldkey, 'netuid': netuid})

    @staticmethod
    def enable_voting_power_tracking(netuid) -> Call:
        'Enables voting power tracking for a subnet.  This function can be called by the subnet owner or root. When enabled, voting power EMA is updated every epoch for all validators. Voting power starts at 0 and increases over epochs.  # Arguments: * `origin` - The origin of the call, must be subnet owner or root. * `netuid` - The subnet to enable voting power tracking for.  # Errors: * `SubnetNotExist` - If the subnet does not exist. * `NotSubnetOwner` - If the caller is not the subnet owner or root.'
        return Call('SubtensorModule', 'enable_voting_power_tracking', {'netuid': netuid})

    @staticmethod
    def increase_take(hotkey, take) -> Call:
        "--- Allows delegates to increase its take value. This call is rate-limited.  # Args: * 'origin': (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * 'hotkey' (T::AccountId): - The hotkey we are delegating (must be owned by the coldkey.)  * 'take' (u16): - The new stake proportion that this hotkey takes from delegations. The new value can be between 0 and 11_796 and should be strictly greater than the previous value. T is the new value (rational number), the the parameter is calculated as [65535 * T]. For example, 1% would be [0.01 * 65535] = [655.35] = 655  # Event: * TakeIncreased; - On successfully setting a increased take for this hotkey.  # Raises: * 'NotRegistered': - The hotkey we are delegating is not registered on the network.  * 'NonAssociatedColdKey': - The hotkey we are delegating is not owned by the calling coldkey.  * 'DelegateTakeTooHigh': - The delegate is setting a take which is not greater than the previous."
        return Call('SubtensorModule', 'increase_take', {'hotkey': hotkey, 'take': take})

    @staticmethod
    def lock_stake(hotkey, netuid, amount) -> Call:
        "Locks stake on a subnet to a specific hotkey, building conviction over time.  If no lock exists for (coldkey, subnet), a new one is created. If a lock exists, the destination hotkey must match the existing lock's hotkey. Top-up adds to the locked amount after rolling the lock state forward.  # Arguments * `origin` - Must be signed by the coldkey. * `hotkey` - The hotkey to lock stake to. * `netuid` - The subnet on which to lock. * `amount` - The alpha amount to lock."
        return Call('SubtensorModule', 'lock_stake', {'hotkey': hotkey, 'netuid': netuid, 'amount': amount})

    @staticmethod
    def move_lock(destination_hotkey, netuid) -> Call:
        'Moves an existing lock for a coldkey on a subnet from one hotkey to another.  The lock is rolled forward to the current block before switching the associated hotkey, preserving the decayed locked mass. The conviction is reset to zero.  # Arguments * `origin` - Must be signed by the coldkey that owns the lock. * `destination_hotkey` - The hotkey the lock should target after the move. * `netuid` - The subnet on which the lock exists. # Errors: * `Error::<T>::NoExistingLock` - If no lock exists for the given coldkey and subnet.'
        return Call('SubtensorModule', 'move_lock', {'destination_hotkey': destination_hotkey, 'netuid': netuid})

    @staticmethod
    def move_stake(origin_hotkey, destination_hotkey, origin_netuid, destination_netuid, alpha_amount) -> Call:
        "---- The implementation for the extrinsic move_stake: Moves specified amount of stake from a hotkey to another across subnets.  # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * `origin_hotkey` (T::AccountId): - The hotkey account to move stake from.  * `destination_hotkey` (T::AccountId): - The hotkey account to move stake to.  * `origin_netuid` (T::AccountId): - The subnet ID to move stake from.  * `destination_netuid` (T::AccountId): - The subnet ID to move stake to.  * `alpha_amount` (T::AccountId): - The alpha stake amount to move."
        return Call('SubtensorModule', 'move_stake', {'origin_hotkey': origin_hotkey, 'destination_hotkey': destination_hotkey, 'origin_netuid': origin_netuid, 'destination_netuid': destination_netuid, 'alpha_amount': alpha_amount})

    @staticmethod
    def recycle_alpha(hotkey, amount, netuid) -> Call:
        'Recycles alpha from a cold/hot key pair, reducing AlphaOut on a subnet  # Arguments * `origin` - The origin of the call (must be signed by the coldkey) * `hotkey` - The hotkey account * `amount` - The amount of alpha to recycle * `netuid` - The subnet ID  # Events Emits a `TokensRecycled` event on success.'
        return Call('SubtensorModule', 'recycle_alpha', {'hotkey': hotkey, 'amount': amount, 'netuid': netuid})

    @staticmethod
    def register(netuid, block_number, nonce, work, hotkey, coldkey) -> Call:
        "---- Registers a new neuron to the subnetwork.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the calling hotkey.  * 'netuid' (u16): - The u16 network identifier.  * 'block_number' ( u64 ): - Block hash used to prove work done.  * 'nonce' ( u64 ): - Positive integer nonce used in POW.  * 'work' ( Vec<u8> ): - Vector encoded bytes representing work done.  * 'hotkey' ( T::AccountId ): - Hotkey to be registered to the network.  * 'coldkey' ( T::AccountId ): - Associated coldkey account.  # Event: * NeuronRegistered; - On successfully registering a uid to a neuron slot on a subnetwork.  # Raises: * 'MechanismDoesNotExist': - Attempting to register to a non existent network.  * 'TooManyRegistrationsThisBlock': - This registration exceeds the total allowed on this network this block.  * 'HotKeyAlreadyRegisteredInSubNet': - The hotkey is already registered on this network.  * 'InvalidWorkBlock': - The work has been performed on a stale, future, or non existent block.  * 'InvalidDifficulty': - The work does not match the difficulty.  * 'InvalidSeal': - The seal is incorrect."
        return Call('SubtensorModule', 'register', {'netuid': netuid, 'block_number': block_number, 'nonce': nonce, 'work': work, 'hotkey': hotkey, 'coldkey': coldkey})

    @staticmethod
    def register_leased_network(emissions_share, end_block) -> Call:
        "Register a new leased network.  The crowdloan's contributions are used to compute the share of the emissions that the contributors will receive as dividends.  The leftover cap is refunded to the contributors and the beneficiary.  # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * `emissions_share` (Percent): - The share of the emissions that the contributors will receive as dividends.  * `end_block` (Option<BlockNumberFor<T>>): - The block at which the lease will end. If not defined, the lease is perpetual."
        return Call('SubtensorModule', 'register_leased_network', {'emissions_share': emissions_share, 'end_block': end_block})

    @staticmethod
    def register_limit(netuid, hotkey, limit_price) -> Call:
        'User register a new subnetwork via burning token, but only if the on-chain burn price for this block is <= `limit_price`.  `limit_price` is expressed in the same TaoCurrency/u64 units as `Burn`.'
        return Call('SubtensorModule', 'register_limit', {'netuid': netuid, 'hotkey': hotkey, 'limit_price': limit_price})

    @staticmethod
    def register_network(hotkey) -> Call:
        'User register a new subnetwork'
        return Call('SubtensorModule', 'register_network', {'hotkey': hotkey})

    @staticmethod
    def register_network_with_identity(hotkey, identity) -> Call:
        'User register a new subnetwork'
        return Call('SubtensorModule', 'register_network_with_identity', {'hotkey': hotkey, 'identity': identity})

    @staticmethod
    def remove_stake(hotkey, netuid, amount_unstaked) -> Call:
        "Remove stake from the staking account. The call must be made from the coldkey account attached to the neuron metadata. Only this key has permission to make staking and unstaking requests.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller's coldkey.  * 'hotkey' (T::AccountId): - The associated hotkey account.  * 'netuid' (u16): - Subnetwork UID  * 'amount_unstaked' (u64): - The amount of stake to be added to the hotkey staking account.  # Event: * StakeRemoved; - On the successfully removing stake from the hotkey account.  # Raises: * 'NotRegistered': - Thrown if the account we are attempting to unstake from is non existent.  * 'NonAssociatedColdKey': - Thrown if the coldkey does not own the hotkey we are unstaking from.  * 'NotEnoughStakeToWithdraw': - Thrown if there is not enough stake on the hotkey to withdwraw this amount."
        return Call('SubtensorModule', 'remove_stake', {'hotkey': hotkey, 'netuid': netuid, 'amount_unstaked': amount_unstaked})

    @staticmethod
    def remove_stake_full_limit(hotkey, netuid, limit_price) -> Call:
        'Removes all stake from a hotkey on a subnet with a price limit. This extrinsic allows to specify the limit price for alpha token at which or better (higher) the staking should execute. Without limit_price it remove all the stake similar to `remove_stake` extrinsic'
        return Call('SubtensorModule', 'remove_stake_full_limit', {'hotkey': hotkey, 'netuid': netuid, 'limit_price': limit_price})

    @staticmethod
    def remove_stake_limit(hotkey, netuid, amount_unstaked, limit_price, allow_partial) -> Call:
        "--- Removes stake from a hotkey on a subnet with a price limit. This extrinsic allows to specify the limit price for alpha token at which or better (higher) the staking should execute.  In case if slippage occurs and the price shall move beyond the limit price, the staking order may execute only partially or not execute at all.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller's coldkey.  * 'hotkey' (T::AccountId): - The associated hotkey account.  * 'netuid' (u16): - Subnetwork UID  * 'amount_unstaked' (u64): - The amount of stake to be added to the hotkey staking account.  * 'limit_price' (u64): - The limit price expressed in units of RAO per one Alpha.  * 'allow_partial' (bool): - Allows partial execution of the amount. If set to false, this becomes fill or kill type or order.  # Event: * StakeRemoved; - On the successfully removing stake from the hotkey account.  # Raises: * 'NotRegistered': - Thrown if the account we are attempting to unstake from is non existent.  * 'NonAssociatedColdKey': - Thrown if the coldkey does not own the hotkey we are unstaking from.  * 'NotEnoughStakeToWithdraw': - Thrown if there is not enough stake on the hotkey to withdwraw this amount."
        return Call('SubtensorModule', 'remove_stake_limit', {'hotkey': hotkey, 'netuid': netuid, 'amount_unstaked': amount_unstaked, 'limit_price': limit_price, 'allow_partial': allow_partial})

    @staticmethod
    def reset_coldkey_swap(coldkey) -> Call:
        'Reset a coldkey swap by clearing the announcement and dispute status.  The dispatch origin of this call must be root.  - `coldkey`: The coldkey to reset the swap for.'
        return Call('SubtensorModule', 'reset_coldkey_swap', {'coldkey': coldkey})

    @staticmethod
    def reveal_mechanism_weights(netuid, mecid, uids, values, salt, version_key) -> Call:
        '---- Used to reveal the weights for a previously committed hash for mechanisms.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The signature of the revealing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `mecid` (`u8`): - The u8 mechanism identifier.  * `uids` (`Vec<u16>`): - The uids for the weights being revealed.  * `values` (`Vec<u16>`): - The values of the weights being revealed.  * `salt` (`Vec<u16>`): - The salt used to generate the commit hash.  * `version_key` (`u64`): - The network version key.  # Raises: * `CommitRevealDisabled`: - Attempting to reveal weights when the commit-reveal mechanism is disabled.  * `NoWeightsCommitFound`: - Attempting to reveal weights without an existing commit.  * `ExpiredWeightCommit`: - Attempting to reveal a weight commit that has expired.  * `RevealTooEarly`: - Attempting to reveal weights outside the valid reveal period.  * `InvalidRevealCommitHashNotMatch`: - The revealed hash does not match any committed hash.'
        return Call('SubtensorModule', 'reveal_mechanism_weights', {'netuid': netuid, 'mecid': mecid, 'uids': uids, 'values': values, 'salt': salt, 'version_key': version_key})

    @staticmethod
    def reveal_weights(netuid, uids, values, salt, version_key) -> Call:
        '---- Used to reveal the weights for a previously committed hash.  # Args: * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`): - The signature of the revealing hotkey.  * `netuid` (`u16`): - The u16 network identifier.  * `uids` (`Vec<u16>`): - The uids for the weights being revealed.  * `values` (`Vec<u16>`): - The values of the weights being revealed.  * `salt` (`Vec<u16>`): - The salt used to generate the commit hash.  * `version_key` (`u64`): - The network version key.  # Raises: * `CommitRevealDisabled`: - Attempting to reveal weights when the commit-reveal mechanism is disabled.  * `NoWeightsCommitFound`: - Attempting to reveal weights without an existing commit.  * `ExpiredWeightCommit`: - Attempting to reveal a weight commit that has expired.  * `RevealTooEarly`: - Attempting to reveal weights outside the valid reveal period.  * `InvalidRevealCommitHashNotMatch`: - The revealed hash does not match any committed hash.'
        return Call('SubtensorModule', 'reveal_weights', {'netuid': netuid, 'uids': uids, 'values': values, 'salt': salt, 'version_key': version_key})

    @staticmethod
    def root_dissolve_network(netuid) -> Call:
        'Remove a subnetwork The caller must be root'
        return Call('SubtensorModule', 'root_dissolve_network', {'netuid': netuid})

    @staticmethod
    def root_register(hotkey) -> Call:
        'Register the hotkey to root network'
        return Call('SubtensorModule', 'root_register', {'hotkey': hotkey})

    @staticmethod
    def schedule_swap_coldkey(new_coldkey) -> Call:
        'Schedules a coldkey swap operation to be executed at a future block.  WARNING: This function is deprecated, please migrate to `announce_coldkey_swap`/`coldkey_swap`'
        return Call('SubtensorModule', 'schedule_swap_coldkey', {'new_coldkey': new_coldkey})

    @staticmethod
    def serve_axon(netuid, version, ip, port, ip_type, protocol, placeholder1, placeholder2) -> Call:
        "Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller.  * 'netuid' (u16): - The u16 network identifier.  * 'version' (u64): - The bittensor version identifier.  * 'ip' (u64): - The endpoint ip information as a u128 encoded integer.  * 'port' (u16): - The endpoint port information as a u16 encoded integer.  * 'ip_type' (u8): - The endpoint ip version as a u8, 4 or 6.  * 'protocol' (u8): - UDP:1 or TCP:0  * 'placeholder1' (u8): - Placeholder for further extra params.  * 'placeholder2' (u8): - Placeholder for further extra params.  # Event: * AxonServed; - On successfully serving the axon info.  # Raises: * 'MechanismDoesNotExist': - Attempting to set weights on a non-existent network.  * 'NotRegistered': - Attempting to set weights from a non registered account.  * 'InvalidIpType': - The ip type is not 4 or 6.  * 'InvalidIpAddress': - The numerically encoded ip address does not resolve to a proper ip.  * 'ServingRateLimitExceeded': - Attempting to set prometheus information withing the rate limit min."
        return Call('SubtensorModule', 'serve_axon', {'netuid': netuid, 'version': version, 'ip': ip, 'port': port, 'ip_type': ip_type, 'protocol': protocol, 'placeholder1': placeholder1, 'placeholder2': placeholder2})

    @staticmethod
    def serve_axon_tls(netuid, version, ip, port, ip_type, protocol, placeholder1, placeholder2, certificate) -> Call:
        "Same as `serve_axon` but takes a certificate as an extra optional argument. Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.  # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller.  * 'netuid' (u16): - The u16 network identifier.  * 'version' (u64): - The bittensor version identifier.  * 'ip' (u64): - The endpoint ip information as a u128 encoded integer.  * 'port' (u16): - The endpoint port information as a u16 encoded integer.  * 'ip_type' (u8): - The endpoint ip version as a u8, 4 or 6.  * 'protocol' (u8): - UDP:1 or TCP:0  * 'placeholder1' (u8): - Placeholder for further extra params.  * 'placeholder2' (u8): - Placeholder for further extra params.  * 'certificate' (Vec<u8>): - TLS certificate for inter neuron communitation.  # Event: * AxonServed; - On successfully serving the axon info.  # Raises: * 'MechanismDoesNotExist': - Attempting to set weights on a non-existent network.  * 'NotRegistered': - Attempting to set weights from a non registered account.  * 'InvalidIpType': - The ip type is not 4 or 6.  * 'InvalidIpAddress': - The numerically encoded ip address does not resolve to a proper ip.  * 'ServingRateLimitExceeded': - Attempting to set prometheus information withing the rate limit min."
        return Call('SubtensorModule', 'serve_axon_tls', {'netuid': netuid, 'version': version, 'ip': ip, 'port': port, 'ip_type': ip_type, 'protocol': protocol, 'placeholder1': placeholder1, 'placeholder2': placeholder2, 'certificate': certificate})

    @staticmethod
    def serve_prometheus(netuid, version, ip, port, ip_type) -> Call:
        "---- Set prometheus information for the neuron. # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the calling hotkey.  * 'netuid' (u16): - The u16 network identifier.  * 'version' (u16): -  The bittensor version identifier.  * 'ip' (u128): - The prometheus ip information as a u128 encoded integer.  * 'port' (u16): - The prometheus port information as a u16 encoded integer.  * 'ip_type' (u8): - The ip type v4 or v6."
        return Call('SubtensorModule', 'serve_prometheus', {'netuid': netuid, 'version': version, 'ip': ip, 'port': port, 'ip_type': ip_type})

    @staticmethod
    def set_activity_cutoff_factor(netuid, factor_milli) -> Call:
        '`set_activity_cutoff_factor`. Per-mille (1/1000) units; `cutoff_blocks = (factor × tempo) / 1000`. Validates `[MinActivityCutoffFactorMilli, MaxActivityCutoffFactorMilli]`. Callable by the subnet owner (rate-limited via `OwnerHyperparamUpdate`, respects the admin freeze window) or by root (bypasses both).'
        return Call('SubtensorModule', 'set_activity_cutoff_factor', {'netuid': netuid, 'factor_milli': factor_milli})

    @staticmethod
    def set_auto_parent_delegation_enabled(hotkey, enabled) -> Call:
        '--- Allows a root validator to toggle auto parent delegation for new subnets owner hotkey'
        return Call('SubtensorModule', 'set_auto_parent_delegation_enabled', {'hotkey': hotkey, 'enabled': enabled})

    @staticmethod
    def set_childkey_take(hotkey, netuid, take) -> Call:
        'Sets the childkey take for a given hotkey.  This function allows a coldkey to set the childkey take for a given hotkey. The childkey take determines the proportion of stake that the hotkey keeps for itself when distributing stake to its children.  # Arguments: * `origin` (<T as frame_system::Config>::RuntimeOrigin): - The signature of the calling coldkey. Setting childkey take can only be done by the coldkey.  * `hotkey` (T::AccountId): - The hotkey for which the childkey take will be set.  * `take` (u16): - The new childkey take value. This is a percentage represented as a value between 0 and 10000, where 10000 represents 100%.  # Events: * `ChildkeyTakeSet`: - On successfully setting the childkey take for a hotkey.  # Errors: * `NonAssociatedColdKey`: - The coldkey does not own the hotkey. * `InvalidChildkeyTake`: - The provided take value is invalid (greater than the maximum allowed take). * `TxChildkeyTakeRateLimitExceeded`: - The rate limit for changing childkey take has been exceeded.'
        return Call('SubtensorModule', 'set_childkey_take', {'hotkey': hotkey, 'netuid': netuid, 'take': take})

    @staticmethod
    def set_children(hotkey, netuid, children) -> Call:
        "Set a single child for a given hotkey on a specified network.  This function allows a coldkey to set a single child for a given hotkey on a specified network. The proportion of the hotkey's stake to be allocated to the child is also specified.  # Arguments: * `origin` (<T as frame_system::Config>::RuntimeOrigin): - The signature of the calling coldkey. Setting a hotkey child can only be done by the coldkey.  * `hotkey` (T::AccountId): - The hotkey which will be assigned the child.  * `child` (T::AccountId): - The child which will be assigned to the hotkey.  * `netuid` (u16): - The u16 network identifier where the childkey will exist.  * `proportion` (u64): - Proportion of the hotkey's stake to be given to the child, the value must be u64 normalized.  # Events: * `ChildAddedSingular`: - On successfully registering a child to a hotkey.  # Errors: * `MechanismDoesNotExist`: - Attempting to register to a non-existent network. * `RegistrationNotPermittedOnRootSubnet`: - Attempting to register a child on the root network. * `NonAssociatedColdKey`: - The coldkey does not own the hotkey or the child is the same as the hotkey. * `HotKeyAccountNotExists`: - The hotkey account does not exist.  # Detailed Explanation of Checks: 1. **Signature Verification**: Ensures that the caller has signed the transaction, verifying the coldkey. 2. **Root Network Check**: Ensures that the delegation is not on the root network, as child hotkeys are not valid on the root. 3. **Network Existence Check**: Ensures that the specified network exists. 4. **Ownership Verification**: Ensures that the coldkey owns the hotkey. 5. **Hotkey Account Existence Check**: Ensures that the hotkey account already exists. 6. **Child-Hotkey Distinction**: Ensures that the child is not the same as the hotkey. 7. **Old Children Cleanup**: Removes the hotkey from the parent list of its old children. 8. **New Children Assignment**: Assigns the new child to the hotkey and updates the parent list for the new child."
        return Call('SubtensorModule', 'set_children', {'hotkey': hotkey, 'netuid': netuid, 'children': children})

    @staticmethod
    def set_coldkey_auto_stake_hotkey(netuid, hotkey) -> Call:
        "Set the autostake destination hotkey for a coldkey.  The caller selects a hotkey where all future rewards will be automatically staked.  # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * `hotkey` (T::AccountId): - The hotkey account to designate as the autostake destination."
        return Call('SubtensorModule', 'set_coldkey_auto_stake_hotkey', {'netuid': netuid, 'hotkey': hotkey})

    @staticmethod
    def set_identity(name, url, github_repo, image, discord, description, additional) -> Call:
        "---- Set prometheus information for the neuron. # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the calling hotkey.  * 'netuid' (u16): - The u16 network identifier.  * 'version' (u16): -  The bittensor version identifier.  * 'ip' (u128): - The prometheus ip information as a u128 encoded integer.  * 'port' (u16): - The prometheus port information as a u16 encoded integer.  * 'ip_type' (u8): - The ip type v4 or v6."
        return Call('SubtensorModule', 'set_identity', {'name': name, 'url': url, 'github_repo': github_repo, 'image': image, 'discord': discord, 'description': description, 'additional': additional})

    @staticmethod
    def set_mechanism_weights(netuid, mecid, dests, weights, version_key) -> Call:
        "--- Sets the caller weights for the incentive mechanism for mechanisms. The call can be made from the hotkey account so is potentially insecure, however, the damage of changing weights is minimal if caught early. This function includes all the checks that the passed weights meet the requirements. Stored as u16s they represent rational values in the range [0,1] which sum to 1 and can be interpreted as probabilities. The specific weights determine how inflation propagates outward from this peer.  Note: The 16 bit integers weights should represent 1.0 as the max u16. However, the function normalizes all integers to u16_max anyway. This means that if the sum of all elements is larger or smaller than the amount of elements * u16_max, all elements will be corrected for this deviation.  # Args: * `origin`: (<T as frame_system::Config>Origin): - The caller, a hotkey who wishes to set their weights.  * `netuid` (u16): - The network uid we are setting these weights on.  * `mecid` (`u8`): - The u8 mechnism identifier.  * `dests` (Vec<u16>): - The edge endpoint for the weight, i.e. j for w_ij.  * 'weights' (Vec<u16>): - The u16 integer encoded weights. Interpreted as rational values in the range [0,1]. They must sum to in32::MAX.  * 'version_key' ( u64 ): - The network version key to check if the validator is up to date.  # Event: * WeightsSet; - On successfully setting the weights on chain.  # Raises: * 'MechanismDoesNotExist': - Attempting to set weights on a non-existent network.  * 'NotRegistered': - Attempting to set weights from a non registered account.  * 'WeightVecNotEqualSize': - Attempting to set weights with uids not of same length.  * 'DuplicateUids': - Attempting to set weights with duplicate uids.  * 'UidsLengthExceedUidsInSubNet': - Attempting to set weights above the max allowed uids.  * 'UidVecContainInvalidOne': - Attempting to set weights with invalid uids.  * 'WeightVecLengthIsLow': - Attempting to set weights with fewer weights than min.  * 'MaxWeightExceeded': - Attempting to set weights with max value exceeding limit."
        return Call('SubtensorModule', 'set_mechanism_weights', {'netuid': netuid, 'mecid': mecid, 'dests': dests, 'weights': weights, 'version_key': version_key})

    @staticmethod
    def set_pending_childkey_cooldown(cooldown) -> Call:
        'Sets the pending childkey cooldown (in blocks). Root only.'
        return Call('SubtensorModule', 'set_pending_childkey_cooldown', {'cooldown': cooldown})

    @staticmethod
    def set_perpetual_lock(netuid, enabled) -> Call:
        "Sets or clears the caller's perpetual lock flag for a subnet.  Locks decay by default. When enabled, the caller's individual lock does not unlock through locked-mass decay. Passing `false` returns the caller's lock to normal decay."
        return Call('SubtensorModule', 'set_perpetual_lock', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def set_reject_locked_alpha(enabled) -> Call:
        'Sets or clears whether the caller rejects incoming locked alpha.  Coldkeys reject locked alpha by default. Passing `false` opts the caller into receiving locked alpha from stake transfers or coldkey swaps.'
        return Call('SubtensorModule', 'set_reject_locked_alpha', {'enabled': enabled})

    @staticmethod
    def set_root_claim_type(new_root_claim_type) -> Call:
        "--- Sets the root claim type for the coldkey. # Args: * 'origin': (<T as frame_system::Config>Origin): - The signature of the caller's coldkey.  # Event: * RootClaimTypeSet; - On the successfully setting the root claim type for the coldkey."
        return Call('SubtensorModule', 'set_root_claim_type', {'new_root_claim_type': new_root_claim_type})

    @staticmethod
    def set_subnet_identity(netuid, subnet_name, github_repo, subnet_contact, subnet_url, discord, description, logo_url, additional) -> Call:
        '---- Set the identity information for a subnet. # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the calling coldkey, which must be the owner of the subnet.  * `netuid` (u16): - The unique network identifier of the subnet.  * `subnet_name` (Vec<u8>): - The name of the subnet.  * `github_repo` (Vec<u8>): - The GitHub repository associated with the subnet identity.  * `subnet_contact` (Vec<u8>): - The contact information for the subnet.'
        return Call('SubtensorModule', 'set_subnet_identity', {'netuid': netuid, 'subnet_name': subnet_name, 'github_repo': github_repo, 'subnet_contact': subnet_contact, 'subnet_url': subnet_url, 'discord': discord, 'description': description, 'logo_url': logo_url, 'additional': additional})

    @staticmethod
    def set_tempo(netuid, tempo) -> Call:
        'Owner-side `set_tempo`. Validates `[MinTempo, MaxTempo]`, applies a fixed `MinTempo`-block cooldown via `TransactionType::TempoUpdate`, respects the admin freeze window, and resets the cycle (`LastEpochBlock = current_block`) on success.'
        return Call('SubtensorModule', 'set_tempo', {'netuid': netuid, 'tempo': tempo})

    @staticmethod
    def set_weights(netuid, dests, weights, version_key) -> Call:
        "--- Sets the caller weights for the incentive mechanism. The call can be made from the hotkey account so is potentially insecure, however, the damage of changing weights is minimal if caught early. This function includes all the checks that the passed weights meet the requirements. Stored as u16s they represent rational values in the range [0,1] which sum to 1 and can be interpreted as probabilities. The specific weights determine how inflation propagates outward from this peer.  Note: The 16 bit integers weights should represent 1.0 as the max u16. However, the function normalizes all integers to u16_max anyway. This means that if the sum of all elements is larger or smaller than the amount of elements * u16_max, all elements will be corrected for this deviation.  # Args: * `origin`: (<T as frame_system::Config>Origin): - The caller, a hotkey who wishes to set their weights.  * `netuid` (u16): - The network uid we are setting these weights on.  * `dests` (Vec<u16>): - The edge endpoint for the weight, i.e. j for w_ij.  * 'weights' (Vec<u16>): - The u16 integer encoded weights. Interpreted as rational values in the range [0,1]. They must sum to in32::MAX.  * 'version_key' ( u64 ): - The network version key to check if the validator is up to date.  # Event: * WeightsSet; - On successfully setting the weights on chain.  # Raises: * 'MechanismDoesNotExist': - Attempting to set weights on a non-existent network.  * 'NotRegistered': - Attempting to set weights from a non registered account.  * 'WeightVecNotEqualSize': - Attempting to set weights with uids not of same length.  * 'DuplicateUids': - Attempting to set weights with duplicate uids.  * 'UidsLengthExceedUidsInSubNet': - Attempting to set weights above the max allowed uids.  * 'UidVecContainInvalidOne': - Attempting to set weights with invalid uids.  * 'WeightVecLengthIsLow': - Attempting to set weights with fewer weights than min.  * 'MaxWeightExceeded': - Attempting to set weights with max value exceeding limit."
        return Call('SubtensorModule', 'set_weights', {'netuid': netuid, 'dests': dests, 'weights': weights, 'version_key': version_key})

    @staticmethod
    def start_call(netuid) -> Call:
        'Initiates a call on a subnet.  # Arguments * `origin` - The origin of the call, which must be signed by the subnet owner. * `netuid` - The unique identifier of the subnet on which the call is being initiated.  # Events Emits a `FirstEmissionBlockNumberSet` event on success.'
        return Call('SubtensorModule', 'start_call', {'netuid': netuid})

    @staticmethod
    def sudo_set_max_childkey_take(take) -> Call:
        'Sets the maximum allowed childkey take.  This function can only be called by the root origin.  # Arguments: * `origin` - The origin of the call, must be root. * `take` - The new maximum childkey take value.  # Errors: * `BadOrigin` - If the origin is not root.'
        return Call('SubtensorModule', 'sudo_set_max_childkey_take', {'take': take})

    @staticmethod
    def sudo_set_min_childkey_take(take) -> Call:
        'Sets the minimum allowed childkey take.  This function can only be called by the root origin.  # Arguments: * `origin` - The origin of the call, must be root. * `take` - The new minimum childkey take value.  # Errors: * `BadOrigin` - If the origin is not root.'
        return Call('SubtensorModule', 'sudo_set_min_childkey_take', {'take': take})

    @staticmethod
    def sudo_set_num_root_claims(new_value) -> Call:
        '--- Sets root claim number (sudo extrinsic). Zero disables auto-claim.'
        return Call('SubtensorModule', 'sudo_set_num_root_claims', {'new_value': new_value})

    @staticmethod
    def sudo_set_root_claim_threshold(netuid, new_value) -> Call:
        '--- Sets root claim threshold for subnet (sudo or owner origin).'
        return Call('SubtensorModule', 'sudo_set_root_claim_threshold', {'netuid': netuid, 'new_value': new_value})

    @staticmethod
    def sudo_set_tx_childkey_take_rate_limit(tx_rate_limit) -> Call:
        'Sets the transaction rate limit for changing childkey take.  This function can only be called by the root origin.  # Arguments: * `origin` - The origin of the call, must be root. * `tx_rate_limit` - The new rate limit in blocks.  # Errors: * `BadOrigin` - If the origin is not root.'
        return Call('SubtensorModule', 'sudo_set_tx_childkey_take_rate_limit', {'tx_rate_limit': tx_rate_limit})

    @staticmethod
    def sudo_set_voting_power_ema_alpha(netuid, alpha) -> Call:
        'Sets the EMA alpha value for voting power calculation on a subnet.  This function can only be called by root (sudo). Higher alpha = faster response to stake changes. Alpha is stored as u64 with 18 decimal precision (1.0 = 10^18).  # Arguments: * `origin` - The origin of the call, must be root. * `netuid` - The subnet to set the alpha for. * `alpha` - The new alpha value (u64 with 18 decimal precision).  # Errors: * `BadOrigin` - If the origin is not root. * `SubnetNotExist` - If the subnet does not exist. * `InvalidVotingPowerEmaAlpha` - If alpha is greater than 10^18 (1.0).'
        return Call('SubtensorModule', 'sudo_set_voting_power_ema_alpha', {'netuid': netuid, 'alpha': alpha})

    @staticmethod
    def swap_coldkey(old_coldkey, new_coldkey, swap_cost) -> Call:
        "Performs an arbitrary coldkey swap for any coldkey.  Only callable by root as it doesn't require an announcement and can be used to swap any coldkey."
        return Call('SubtensorModule', 'swap_coldkey', {'old_coldkey': old_coldkey, 'new_coldkey': new_coldkey, 'swap_cost': swap_cost})

    @staticmethod
    def swap_coldkey_announced(new_coldkey) -> Call:
        'Performs a coldkey swap if an announcement has been made.  The dispatch origin of this call must be the original coldkey that made the announcement.  - `new_coldkey`: The new coldkey to swap to. The BlakeTwo256 hash of the new coldkey must be the same as the announced coldkey hash.  The `ColdkeySwapped` event is emitted on successful swap.'
        return Call('SubtensorModule', 'swap_coldkey_announced', {'new_coldkey': new_coldkey})

    @staticmethod
    def swap_hotkey(hotkey, new_hotkey, netuid) -> Call:
        '---- The extrinsic for user to change its hotkey in subnet or all subnets.  # Arguments * `origin` - The origin of the transaction (must be signed by the coldkey). * `hotkey` - The old hotkey to be swapped. * `new_hotkey` - The new hotkey to replace the old one. * `netuid` - Optional subnet ID. If `Some`, swap only on that subnet; if `None`, swap on all subnets. is transferred to the new hotkey.'
        return Call('SubtensorModule', 'swap_hotkey', {'hotkey': hotkey, 'new_hotkey': new_hotkey, 'netuid': netuid})

    @staticmethod
    def swap_hotkey_v2(hotkey, new_hotkey, netuid, keep_stake) -> Call:
        '---- The extrinsic for user to change its hotkey in subnet or all subnets. This extrinsic is similar to swap_hotkey, but with keep_stake parameter bo be able to keep the stake when swapping a root key to a child key  # Arguments * `origin` - The origin of the transaction (must be signed by the coldkey). * `hotkey` - The old hotkey to be swapped. * `new_hotkey` - The new hotkey to replace the old one. * `netuid` - Optional subnet ID. If `Some`, swap only on that subnet; if `None`, swap on all subnets. * `keep_stake` - If `true`, stake remains on the old hotkey and the rest metadata is transferred to the new hotkey.'
        return Call('SubtensorModule', 'swap_hotkey_v2', {'hotkey': hotkey, 'new_hotkey': new_hotkey, 'netuid': netuid, 'keep_stake': keep_stake})

    @staticmethod
    def swap_stake(hotkey, origin_netuid, destination_netuid, alpha_amount) -> Call:
        'Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.  # Arguments * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`. * `hotkey` - The hotkey whose stake is being swapped. * `origin_netuid` - The network/subnet ID from which stake is removed. * `destination_netuid` - The network/subnet ID to which stake is added. * `alpha_amount` - The amount of stake to swap.  # Errors Returns an error if: * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails). * Either `origin_netuid` or `destination_netuid` does not exist. * The hotkey does not exist. * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`. * The swap amount is below the minimum stake requirement.  # Events May emit a `StakeSwapped` event on success.'
        return Call('SubtensorModule', 'swap_stake', {'hotkey': hotkey, 'origin_netuid': origin_netuid, 'destination_netuid': destination_netuid, 'alpha_amount': alpha_amount})

    @staticmethod
    def swap_stake_limit(hotkey, origin_netuid, destination_netuid, alpha_amount, limit_price, allow_partial) -> Call:
        'Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.  # Arguments * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`. * `hotkey` - The hotkey whose stake is being swapped. * `origin_netuid` - The network/subnet ID from which stake is removed. * `destination_netuid` - The network/subnet ID to which stake is added. * `alpha_amount` - The amount of stake to swap. * `limit_price` - The limit price expressed in units of RAO per one Alpha. * `allow_partial` - Allows partial execution of the amount. If set to false, this becomes fill or kill type or order.  # Errors Returns an error if: * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails). * Either `origin_netuid` or `destination_netuid` does not exist. * The hotkey does not exist. * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`. * The swap amount is below the minimum stake requirement.  # Events May emit a `StakeSwapped` event on success.'
        return Call('SubtensorModule', 'swap_stake_limit', {'hotkey': hotkey, 'origin_netuid': origin_netuid, 'destination_netuid': destination_netuid, 'alpha_amount': alpha_amount, 'limit_price': limit_price, 'allow_partial': allow_partial})

    @staticmethod
    def terminate_lease(lease_id, hotkey) -> Call:
        "Terminate a lease.  The beneficiary can terminate the lease after the end block has passed and get the subnet ownership. The subnet is transferred to the beneficiary and the lease is removed from storage.  **The hotkey must be owned by the beneficiary coldkey.**  # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * `lease_id` (LeaseId): - The ID of the lease to terminate.  * `hotkey` (T::AccountId): - The hotkey of the beneficiary to mark as subnet owner hotkey."
        return Call('SubtensorModule', 'terminate_lease', {'lease_id': lease_id, 'hotkey': hotkey})

    @staticmethod
    def transfer_stake(destination_coldkey, hotkey, origin_netuid, destination_netuid, alpha_amount) -> Call:
        'Transfers a specified amount of stake from one coldkey to another, optionally across subnets, while keeping the same hotkey.  # Arguments * `origin` - The origin of the transaction, which must be signed by the `origin_coldkey`. * `destination_coldkey` - The coldkey to which the stake is transferred. * `hotkey` - The hotkey associated with the stake. * `origin_netuid` - The network/subnet ID to move stake from. * `destination_netuid` - The network/subnet ID to move stake to (for cross-subnet transfer). * `alpha_amount` - The amount of stake to transfer.  # Errors Returns an error if: * The origin is not signed by the correct coldkey. * Either subnet does not exist. * The hotkey does not exist. * There is insufficient stake on `(origin_coldkey, hotkey, origin_netuid)`. * The transfer amount is below the minimum stake requirement.  # Events May emit a `StakeTransferred` event on success.'
        return Call('SubtensorModule', 'transfer_stake', {'destination_coldkey': destination_coldkey, 'hotkey': hotkey, 'origin_netuid': origin_netuid, 'destination_netuid': destination_netuid, 'alpha_amount': alpha_amount})

    @staticmethod
    def trigger_epoch(netuid) -> Call:
        'Owner-side `trigger_epoch`. Schedules an epoch to fire after `AdminFreezeWindow` blocks. Rate-limited via the existing `OwnerHyperparamUpdate` pattern.'
        return Call('SubtensorModule', 'trigger_epoch', {'netuid': netuid})

    @staticmethod
    def try_associate_hotkey(hotkey) -> Call:
        'Attempts to associate a hotkey with a coldkey.  # Arguments * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`. * `hotkey` - The hotkey to associate with the coldkey.  # Note Will charge based on the weight even if the hotkey is already associated with a coldkey.'
        return Call('SubtensorModule', 'try_associate_hotkey', {'hotkey': hotkey})

    @staticmethod
    def unstake_all(hotkey) -> Call:
        "---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.  # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * `hotkey` (T::AccountId): - The associated hotkey account.  # Event: * StakeRemoved; - On the successfully removing stake from the hotkey account.  # Raises: * `NotRegistered`: - Thrown if the account we are attempting to unstake from is non existent.  * `NonAssociatedColdKey`: - Thrown if the coldkey does not own the hotkey we are unstaking from.  * `NotEnoughStakeToWithdraw`: - Thrown if there is not enough stake on the hotkey to withdraw this amount.  * `TxRateLimitExceeded`: - Thrown if key has hit transaction rate limit"
        return Call('SubtensorModule', 'unstake_all', {'hotkey': hotkey})

    @staticmethod
    def unstake_all_alpha(hotkey) -> Call:
        "---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.  # Args: * `origin` - (<T as frame_system::Config>::Origin): - The signature of the caller's coldkey.  * `hotkey` (T::AccountId): - The associated hotkey account.  # Event: * StakeRemoved; - On the successfully removing stake from the hotkey account.  # Raises: * `NotRegistered`: - Thrown if the account we are attempting to unstake from is non existent.  * `NonAssociatedColdKey`: - Thrown if the coldkey does not own the hotkey we are unstaking from.  * `NotEnoughStakeToWithdraw`: - Thrown if there is not enough stake on the hotkey to withdraw this amount.  * `TxRateLimitExceeded`: - Thrown if key has hit transaction rate limit"
        return Call('SubtensorModule', 'unstake_all_alpha', {'hotkey': hotkey})

    @staticmethod
    def update_symbol(netuid, symbol) -> Call:
        'Updates the symbol for a subnet.  # Arguments * `origin` - The origin of the call, which must be the subnet owner or root. * `netuid` - The unique identifier of the subnet on which the symbol is being set. * `symbol` - The symbol to set for the subnet.  # Errors Returns an error if: * The transaction is not signed by the subnet owner. * The symbol does not exist. * The symbol is already in use by another subnet.  # Events Emits a `SymbolUpdated` event on success.'
        return Call('SubtensorModule', 'update_symbol', {'netuid': netuid, 'symbol': symbol})


class Utility:
    """Call builders for the Utility pallet."""

    @staticmethod
    def as_derivative(index, call) -> Call:
        'Send a call through an indexed pseudonym of the sender.  Filter from origin are passed along. The call will be dispatched with an origin which use the same filter as the origin of this call.  NOTE: If you need to ensure that any account-based filtering is not honored (i.e. because you expect `proxy` to have been used prior in the call stack and you do not want the call restrictions to apply to any sub-accounts), then use `as_multi_threshold_1` in the Multisig pallet instead.  NOTE: Prior to version *12, this was called `as_limited_sub`.  The dispatch origin for this call must be _Signed_.'
        return Call('Utility', 'as_derivative', {'index': index, 'call': call})

    @staticmethod
    def batch(calls) -> Call:
        'Send a batch of dispatch calls.  May be called from any origin except `None`.  - `calls`: The calls to be dispatched from the same origin. The number of call must not exceed the constant: `batched_calls_limit` (available in constant metadata).  If origin is root then the calls are dispatched without checking origin filter. (This includes bypassing `frame_system::Config::BaseCallFilter`).  ## Complexity - O(C) where C is the number of calls to be batched.  This will return `Ok` in all circumstances. To determine the success of the batch, an event is deposited. If a call failed and the batch was interrupted, then the `BatchInterrupted` event is deposited, along with the number of successful calls made and the error of the failed call. If all were successful, then the `BatchCompleted` event is deposited.'
        return Call('Utility', 'batch', {'calls': calls})

    @staticmethod
    def batch_all(calls) -> Call:
        'Send a batch of dispatch calls and atomically execute them. The whole transaction will rollback and fail if any of the calls failed.  May be called from any origin except `None`.  - `calls`: The calls to be dispatched from the same origin. The number of call must not exceed the constant: `batched_calls_limit` (available in constant metadata).  If origin is root then the calls are dispatched without checking origin filter. (This includes bypassing `frame_system::Config::BaseCallFilter`).  ## Complexity - O(C) where C is the number of calls to be batched.'
        return Call('Utility', 'batch_all', {'calls': calls})

    @staticmethod
    def dispatch_as(as_origin, call) -> Call:
        'Dispatches a function call with a provided origin.  The dispatch origin for this call must be _Root_.  ## Complexity - O(1).'
        return Call('Utility', 'dispatch_as', {'as_origin': as_origin, 'call': call})

    @staticmethod
    def dispatch_as_fallible(as_origin, call) -> Call:
        'Dispatches a function call with a provided origin.  Almost the same as [`Pallet::dispatch_as`] but forwards any error of the inner call.  The dispatch origin for this call must be _Root_.'
        return Call('Utility', 'dispatch_as_fallible', {'as_origin': as_origin, 'call': call})

    @staticmethod
    def force_batch(calls) -> Call:
        "Send a batch of dispatch calls. Unlike `batch`, it allows errors and won't interrupt.  May be called from any origin except `None`.  - `calls`: The calls to be dispatched from the same origin. The number of call must not exceed the constant: `batched_calls_limit` (available in constant metadata).  If origin is root then the calls are dispatch without checking origin filter. (This includes bypassing `frame_system::Config::BaseCallFilter`).  ## Complexity - O(C) where C is the number of calls to be batched."
        return Call('Utility', 'force_batch', {'calls': calls})

    @staticmethod
    def if_else(main, fallback) -> Call:
        'Dispatch a fallback call in the event the main call fails to execute. May be called from any origin except `None`.  This function first attempts to dispatch the `main` call. If the `main` call fails, the `fallback` is attemted. if the fallback is successfully dispatched, the weights of both calls are accumulated and an event containing the main call error is deposited.  In the event of a fallback failure the whole call fails with the weights returned.  - `main`: The main call to be dispatched. This is the primary action to execute. - `fallback`: The fallback call to be dispatched in case the `main` call fails.  ## Dispatch Logic - If the origin is `root`, both the main and fallback calls are executed without applying any origin filters. - If the origin is not `root`, the origin filter is applied to both the `main` and `fallback` calls.  ## Use Case - Some use cases might involve submitting a `batch` type call in either main, fallback or both.'
        return Call('Utility', 'if_else', {'main': main, 'fallback': fallback})

    @staticmethod
    def with_weight(call, weight) -> Call:
        'Dispatch a function call with a specified weight.  This function does not check the weight of the call, and instead allows the Root origin to specify the weight of the call.  The dispatch origin for this call must be _Root_.'
        return Call('Utility', 'with_weight', {'call': call, 'weight': weight})


class Sudo:
    """Call builders for the Sudo pallet."""

    @staticmethod
    def remove_key() -> Call:
        'Permanently removes the sudo key.  **This cannot be un-done.**'
        return Call('Sudo', 'remove_key', {})

    @staticmethod
    def set_key(new) -> Call:
        'Authenticates the current sudo key and sets the given AccountId (`new`) as the new sudo key.'
        return Call('Sudo', 'set_key', {'new': new})

    @staticmethod
    def sudo(call) -> Call:
        'Authenticates the sudo key and dispatches a function call with `Root` origin.'
        return Call('Sudo', 'sudo', {'call': call})

    @staticmethod
    def sudo_as(who, call) -> Call:
        'Authenticates the sudo key and dispatches a function call with `Signed` origin from a given account.  The dispatch origin for this call must be _Signed_.'
        return Call('Sudo', 'sudo_as', {'who': who, 'call': call})

    @staticmethod
    def sudo_unchecked_weight(call, weight) -> Call:
        'Authenticates the sudo key and dispatches a function call with `Root` origin. This function does not check the weight of the call, and instead allows the Sudo user to specify the weight of the call.  The dispatch origin for this call must be _Signed_.'
        return Call('Sudo', 'sudo_unchecked_weight', {'call': call, 'weight': weight})


class Multisig:
    """Call builders for the Multisig pallet."""

    @staticmethod
    def approve_as_multi(threshold, other_signatories, maybe_timepoint, call_hash, max_weight) -> Call:
        'Register approval for a dispatch to be made from a deterministic composite account if approved by a total of `threshold - 1` of `other_signatories`.  Payment: `DepositBase` will be reserved if this is the first approval, plus `threshold` times `DepositFactor`. It is returned once this dispatch happens or is cancelled.  The dispatch origin for this call must be _Signed_.  - `threshold`: The total number of approvals for this dispatch before it is executed. - `other_signatories`: The accounts (other than the sender) who can approve this dispatch. May not be empty. - `maybe_timepoint`: If this is the first approval, then this must be `None`. If it is not the first approval, then it must be `Some`, with the timepoint (block number and transaction index) of the first approval transaction. - `call_hash`: The hash of the call to be executed.  NOTE: If this is the final approval, you will want to use `as_multi` instead.  ## Complexity - `O(S)`. - Up to one balance-reserve or unreserve operation. - One passthrough operation, one insert, both `O(S)` where `S` is the number of signatories. `S` is capped by `MaxSignatories`, with weight being proportional. - One encode & hash, both of complexity `O(S)`. - Up to one binary search and insert (`O(logS + S)`). - I/O: 1 read `O(S)`, up to 1 mutate `O(S)`. Up to one remove. - One event. - Storage: inserts one item, value size bounded by `MaxSignatories`, with a deposit taken for its lifetime of `DepositBase + threshold * DepositFactor`.'
        return Call('Multisig', 'approve_as_multi', {'threshold': threshold, 'other_signatories': other_signatories, 'maybe_timepoint': maybe_timepoint, 'call_hash': call_hash, 'max_weight': max_weight})

    @staticmethod
    def as_multi(threshold, other_signatories, maybe_timepoint, call, max_weight) -> Call:
        'Register approval for a dispatch to be made from a deterministic composite account if approved by a total of `threshold - 1` of `other_signatories`.  If there are enough, then dispatch the call.  Payment: `DepositBase` will be reserved if this is the first approval, plus `threshold` times `DepositFactor`. It is returned once this dispatch happens or is cancelled.  The dispatch origin for this call must be _Signed_.  - `threshold`: The total number of approvals for this dispatch before it is executed. - `other_signatories`: The accounts (other than the sender) who can approve this dispatch. May not be empty. - `maybe_timepoint`: If this is the first approval, then this must be `None`. If it is not the first approval, then it must be `Some`, with the timepoint (block number and transaction index) of the first approval transaction. - `call`: The call to be executed.  NOTE: Unless this is the final approval, you will generally want to use `approve_as_multi` instead, since it only requires a hash of the call.  Result is equivalent to the dispatched result if `threshold` is exactly `1`. Otherwise on success, result is `Ok` and the result from the interior call, if it was executed, may be found in the deposited `MultisigExecuted` event.  ## Complexity - `O(S + Z + Call)`. - Up to one balance-reserve or unreserve operation. - One passthrough operation, one insert, both `O(S)` where `S` is the number of signatories. `S` is capped by `MaxSignatories`, with weight being proportional. - One call encode & hash, both of complexity `O(Z)` where `Z` is tx-len. - One encode & hash, both of complexity `O(S)`. - Up to one binary search and insert (`O(logS + S)`). - I/O: 1 read `O(S)`, up to 1 mutate `O(S)`. Up to one remove. - One event. - The weight of the `call`. - Storage: inserts one item, value size bounded by `MaxSignatories`, with a deposit taken for its lifetime of `DepositBase + threshold * DepositFactor`.'
        return Call('Multisig', 'as_multi', {'threshold': threshold, 'other_signatories': other_signatories, 'maybe_timepoint': maybe_timepoint, 'call': call, 'max_weight': max_weight})

    @staticmethod
    def as_multi_threshold_1(other_signatories, call) -> Call:
        'Immediately dispatch a multi-signature call using a single approval from the caller.  The dispatch origin for this call must be _Signed_.  - `other_signatories`: The accounts (other than the sender) who are part of the multi-signature, but do not participate in the approval process. - `call`: The call to be executed.  Result is equivalent to the dispatched result.  ## Complexity O(Z + C) where Z is the length of the call and C its execution weight.'
        return Call('Multisig', 'as_multi_threshold_1', {'other_signatories': other_signatories, 'call': call})

    @staticmethod
    def cancel_as_multi(threshold, other_signatories, timepoint, call_hash) -> Call:
        'Cancel a pre-existing, on-going multisig transaction. Any deposit reserved previously for this operation will be unreserved on success.  The dispatch origin for this call must be _Signed_.  - `threshold`: The total number of approvals for this dispatch before it is executed. - `other_signatories`: The accounts (other than the sender) who can approve this dispatch. May not be empty. - `timepoint`: The timepoint (block number and transaction index) of the first approval transaction for this dispatch. - `call_hash`: The hash of the call to be executed.  ## Complexity - `O(S)`. - Up to one balance-reserve or unreserve operation. - One passthrough operation, one insert, both `O(S)` where `S` is the number of signatories. `S` is capped by `MaxSignatories`, with weight being proportional. - One encode & hash, both of complexity `O(S)`. - One event. - I/O: 1 read `O(S)`, one remove. - Storage: removes one item.'
        return Call('Multisig', 'cancel_as_multi', {'threshold': threshold, 'other_signatories': other_signatories, 'timepoint': timepoint, 'call_hash': call_hash})

    @staticmethod
    def poke_deposit(threshold, other_signatories, call_hash) -> Call:
        'Poke the deposit reserved for an existing multisig operation.  The dispatch origin for this call must be _Signed_ and must be the original depositor of the multisig operation.  The transaction fee is waived if the deposit amount has changed.  - `threshold`: The total number of approvals needed for this multisig. - `other_signatories`: The accounts (other than the sender) who are part of the multisig. - `call_hash`: The hash of the call this deposit is reserved for.  Emits `DepositPoked` if successful.'
        return Call('Multisig', 'poke_deposit', {'threshold': threshold, 'other_signatories': other_signatories, 'call_hash': call_hash})


class Preimage:
    """Call builders for the Preimage pallet."""

    @staticmethod
    def ensure_updated(hashes) -> Call:
        'Ensure that the bulk of pre-images is upgraded.  The caller pays no fee if at least 90% of pre-images were successfully updated.'
        return Call('Preimage', 'ensure_updated', {'hashes': hashes})

    @staticmethod
    def note_preimage(bytes) -> Call:
        'Register a preimage on-chain.  If the preimage was previously requested, no fees or deposits are taken for providing the preimage. Otherwise, a deposit is taken proportional to the size of the preimage.'
        return Call('Preimage', 'note_preimage', {'bytes': bytes})

    @staticmethod
    def request_preimage(hash) -> Call:
        'Request a preimage be uploaded to the chain without paying any fees or deposits.  If the preimage requests has already been provided on-chain, we unreserve any deposit a user may have paid, and take the control of the preimage out of their hands.'
        return Call('Preimage', 'request_preimage', {'hash': hash})

    @staticmethod
    def unnote_preimage(hash) -> Call:
        'Clear an unrequested preimage from the runtime storage.  If `len` is provided, then it will be a much cheaper operation.  - `hash`: The hash of the preimage to be removed from the store. - `len`: The length of the preimage of `hash`.'
        return Call('Preimage', 'unnote_preimage', {'hash': hash})

    @staticmethod
    def unrequest_preimage(hash) -> Call:
        'Clear a previously made request for a preimage.  NOTE: THIS MUST NOT BE CALLED ON `hash` MORE TIMES THAN `request_preimage`.'
        return Call('Preimage', 'unrequest_preimage', {'hash': hash})


class Scheduler:
    """Call builders for the Scheduler pallet."""

    @staticmethod
    def cancel(when, index) -> Call:
        'Cancel an anonymously scheduled task.'
        return Call('Scheduler', 'cancel', {'when': when, 'index': index})

    @staticmethod
    def cancel_named(id) -> Call:
        'Cancel a named scheduled task.'
        return Call('Scheduler', 'cancel_named', {'id': id})

    @staticmethod
    def cancel_retry(task) -> Call:
        'Removes the retry configuration of a task.'
        return Call('Scheduler', 'cancel_retry', {'task': task})

    @staticmethod
    def cancel_retry_named(id) -> Call:
        'Cancel the retry configuration of a named task.'
        return Call('Scheduler', 'cancel_retry_named', {'id': id})

    @staticmethod
    def schedule(when, maybe_periodic, priority, call) -> Call:
        'Anonymously schedule a task.'
        return Call('Scheduler', 'schedule', {'when': when, 'maybe_periodic': maybe_periodic, 'priority': priority, 'call': call})

    @staticmethod
    def schedule_after(after, maybe_periodic, priority, call) -> Call:
        'Anonymously schedule a task after a delay.'
        return Call('Scheduler', 'schedule_after', {'after': after, 'maybe_periodic': maybe_periodic, 'priority': priority, 'call': call})

    @staticmethod
    def schedule_named(id, when, maybe_periodic, priority, call) -> Call:
        'Schedule a named task.'
        return Call('Scheduler', 'schedule_named', {'id': id, 'when': when, 'maybe_periodic': maybe_periodic, 'priority': priority, 'call': call})

    @staticmethod
    def schedule_named_after(id, after, maybe_periodic, priority, call) -> Call:
        'Schedule a named task after a delay.'
        return Call('Scheduler', 'schedule_named_after', {'id': id, 'after': after, 'maybe_periodic': maybe_periodic, 'priority': priority, 'call': call})

    @staticmethod
    def set_retry(task, retries, period) -> Call:
        "Set a retry configuration for a task so that, in case its scheduled run fails, it will be retried after `period` blocks, for a total amount of `retries` retries or until it succeeds.  Tasks which need to be scheduled for a retry are still subject to weight metering and agenda space, same as a regular task. If a periodic task fails, it will be scheduled normally while the task is retrying.  Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic clones of the original task. Their retry configuration will be derived from the original task's configuration, but will have a lower value for `remaining` than the original `total_retries`."
        return Call('Scheduler', 'set_retry', {'task': task, 'retries': retries, 'period': period})

    @staticmethod
    def set_retry_named(id, retries, period) -> Call:
        "Set a retry configuration for a named task so that, in case its scheduled run fails, it will be retried after `period` blocks, for a total amount of `retries` retries or until it succeeds.  Tasks which need to be scheduled for a retry are still subject to weight metering and agenda space, same as a regular task. If a periodic task fails, it will be scheduled normally while the task is retrying.  Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic clones of the original task. Their retry configuration will be derived from the original task's configuration, but will have a lower value for `remaining` than the original `total_retries`."
        return Call('Scheduler', 'set_retry_named', {'id': id, 'retries': retries, 'period': period})


class Proxy:
    """Call builders for the Proxy pallet."""

    @staticmethod
    def add_proxy(delegate, proxy_type, delay) -> Call:
        'Register a proxy account for the sender that is able to make calls on its behalf.  The dispatch origin for this call must be _Signed_.  Parameters: - `proxy`: The account that the `caller` would like to make a proxy. - `proxy_type`: The permissions allowed for this proxy account. - `delay`: The announcement period required of the initial proxy. Will generally be zero.'
        return Call('Proxy', 'add_proxy', {'delegate': delegate, 'proxy_type': proxy_type, 'delay': delay})

    @staticmethod
    def announce(real, call_hash) -> Call:
        'Publish the hash of a proxy-call that will be made in the future.  This must be called some number of blocks before the corresponding `proxy` is attempted if the delay associated with the proxy relationship is greater than zero.  No more than `MaxPending` announcements may be made at any one time.  This will take a deposit of `AnnouncementDepositFactor` as well as `AnnouncementDepositBase` if there are no other pending announcements.  The dispatch origin for this call must be _Signed_ and a proxy of `real`.  Parameters: - `real`: The account that the proxy will make a call on behalf of. - `call_hash`: The hash of the call to be made by the `real` account.'
        return Call('Proxy', 'announce', {'real': real, 'call_hash': call_hash})

    @staticmethod
    def create_pure(proxy_type, delay, index) -> Call:
        "Spawn a fresh new account that is guaranteed to be otherwise inaccessible, and initialize it with a proxy of `proxy_type` for `origin` sender.  Requires a `Signed` origin.  - `proxy_type`: The type of the proxy that the sender will be registered as over the new account. This will almost always be the most permissive `ProxyType` possible to allow for maximum flexibility. - `index`: A disambiguation index, in case this is called multiple times in the same transaction (e.g. with `utility::batch`). Unless you're using `batch` you probably just want to use `0`. - `delay`: The announcement period required of the initial proxy. Will generally be zero.  Fails with `Duplicate` if this has already been called in this transaction, from the same sender, with the same parameters.  Fails if there are insufficient funds to pay for deposit."
        return Call('Proxy', 'create_pure', {'proxy_type': proxy_type, 'delay': delay, 'index': index})

    @staticmethod
    def kill_pure(spawner, proxy_type, index, height, ext_index) -> Call:
        'Removes a previously spawned pure proxy.  WARNING: **All access to this account will be lost.** Any funds held in it will be inaccessible.  Requires a `Signed` origin, and the sender account must have been created by a call to `create_pure` with corresponding parameters.  - `spawner`: The account that originally called `create_pure` to create this account. - `index`: The disambiguation index originally passed to `create_pure`. Probably `0`. - `proxy_type`: The proxy type originally passed to `create_pure`. - `height`: The height of the chain when the call to `create_pure` was processed. - `ext_index`: The extrinsic index in which the call to `create_pure` was processed.  Fails with `NoPermission` in case the caller is not a previously created pure account whose `create_pure` call has corresponding parameters.'
        return Call('Proxy', 'kill_pure', {'spawner': spawner, 'proxy_type': proxy_type, 'index': index, 'height': height, 'ext_index': ext_index})

    @staticmethod
    def poke_deposit() -> Call:
        'Poke / Adjust deposits made for proxies and announcements based on current values. This can be used by accounts to possibly lower their locked amount.  The dispatch origin for this call must be _Signed_.  The transaction fee is waived if the deposit amount has changed.  Emits `DepositPoked` if successful.'
        return Call('Proxy', 'poke_deposit', {})

    @staticmethod
    def proxy(real, force_proxy_type, call) -> Call:
        'Dispatch the given `call` from an account that the sender is authorised for through `add_proxy`.  The dispatch origin for this call must be _Signed_.  Parameters: - `real`: The account that the proxy will make a call on behalf of. - `force_proxy_type`: Specify the exact proxy type to be used and checked for this call. - `call`: The call to be made by the `real` account.'
        return Call('Proxy', 'proxy', {'real': real, 'force_proxy_type': force_proxy_type, 'call': call})

    @staticmethod
    def proxy_announced(delegate, real, force_proxy_type, call) -> Call:
        'Dispatch the given `call` from an account that the sender is authorized for through `add_proxy`.  Removes any corresponding announcement(s).  The dispatch origin for this call must be _Signed_.  Parameters: - `real`: The account that the proxy will make a call on behalf of. - `force_proxy_type`: Specify the exact proxy type to be used and checked for this call. - `call`: The call to be made by the `real` account.'
        return Call('Proxy', 'proxy_announced', {'delegate': delegate, 'real': real, 'force_proxy_type': force_proxy_type, 'call': call})

    @staticmethod
    def reject_announcement(delegate, call_hash) -> Call:
        'Remove the given announcement of a delegate.  May be called by a target (proxied) account to remove a call that one of their delegates (`delegate`) has announced they want to execute. The deposit is returned.  The dispatch origin for this call must be _Signed_.  Parameters: - `delegate`: The account that previously announced the call. - `call_hash`: The hash of the call to be made.'
        return Call('Proxy', 'reject_announcement', {'delegate': delegate, 'call_hash': call_hash})

    @staticmethod
    def remove_announcement(real, call_hash) -> Call:
        'Remove a given announcement.  May be called by a proxy account to remove a call they previously announced and return the deposit.  The dispatch origin for this call must be _Signed_.  Parameters: - `real`: The account that the proxy will make a call on behalf of. - `call_hash`: The hash of the call to be made by the `real` account.'
        return Call('Proxy', 'remove_announcement', {'real': real, 'call_hash': call_hash})

    @staticmethod
    def remove_proxies() -> Call:
        'Unregister all proxy accounts for the sender.  The dispatch origin for this call must be _Signed_.  WARNING: This may be called on accounts created by `create_pure`, however if done, then the unreserved fees will be inaccessible. **All access to this account will be lost.**'
        return Call('Proxy', 'remove_proxies', {})

    @staticmethod
    def remove_proxy(delegate, proxy_type, delay) -> Call:
        'Unregister a proxy account for the sender.  The dispatch origin for this call must be _Signed_.  Parameters: - `proxy`: The account that the `caller` would like to remove as a proxy. - `proxy_type`: The permissions currently enabled for the removed proxy account.'
        return Call('Proxy', 'remove_proxy', {'delegate': delegate, 'proxy_type': proxy_type, 'delay': delay})

    @staticmethod
    def set_real_pays_fee(delegate, pays_fee) -> Call:
        'Set whether the real account pays transaction fees for proxy calls made by a specific delegate.  The dispatch origin for this call must be _Signed_ and must be the real (delegator) account that has an existing proxy relationship with the delegate.  Parameters: - `delegate`: The proxy account for which to set the fee payment preference. - `pays_fee`: If `true`, the real account will pay fees for proxy calls made by this delegate. If `false`, the delegate pays (default behavior).'
        return Call('Proxy', 'set_real_pays_fee', {'delegate': delegate, 'pays_fee': pays_fee})


class Commitments:
    """Call builders for the Commitments pallet."""

    @staticmethod
    def set_commitment(netuid, info) -> Call:
        'Set the commitment for a given netuid'
        return Call('Commitments', 'set_commitment', {'netuid': netuid, 'info': info})

    @staticmethod
    def set_max_space(new_limit) -> Call:
        'Sudo-set MaxSpace'
        return Call('Commitments', 'set_max_space', {'new_limit': new_limit})


class AdminUtils:
    """Call builders for the AdminUtils pallet."""

    @staticmethod
    def schedule_grandpa_change(next_authorities, in_blocks, forced) -> Call:
        'A public interface for `pallet_grandpa::Pallet::schedule_grandpa_change`.  Schedule a change in the authorities.  The change will be applied at the end of execution of the block `in_blocks` after the current block. This value may be 0, in which case the change is applied at the end of the current block.  If the `forced` parameter is defined, this indicates that the current set has been synchronously determined to be offline and that after `in_blocks` the given change should be applied. The given block number indicates the median last finalized block number and it should be used as the canon block when starting the new grandpa voter.  No change should be signaled while any change is pending. Returns an error if a change is already pending.'
        return Call('AdminUtils', 'schedule_grandpa_change', {'next_authorities': next_authorities, 'in_blocks': in_blocks, 'forced': forced})

    @staticmethod
    def sudo_set_activity_cutoff(netuid, activity_cutoff) -> Call:
        'The extrinsic sets the activity cutoff for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the activity cutoff.'
        return Call('AdminUtils', 'sudo_set_activity_cutoff', {'netuid': netuid, 'activity_cutoff': activity_cutoff})

    @staticmethod
    def sudo_set_adjustment_alpha(netuid, adjustment_alpha) -> Call:
        'The extrinsic sets the adjustment alpha for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the adjustment alpha.'
        return Call('AdminUtils', 'sudo_set_adjustment_alpha', {'netuid': netuid, 'adjustment_alpha': adjustment_alpha})

    @staticmethod
    def sudo_set_adjustment_interval(netuid, adjustment_interval) -> Call:
        'The extrinsic sets the adjustment interval for a subnet. It is only callable by the root account, not changeable by the subnet owner. The extrinsic will call the Subtensor pallet to set the adjustment interval.'
        return Call('AdminUtils', 'sudo_set_adjustment_interval', {'netuid': netuid, 'adjustment_interval': adjustment_interval})

    @staticmethod
    def sudo_set_admin_freeze_window(window) -> Call:
        'Sets the admin freeze window length (in blocks) at the end of a tempo. Only callable by root.'
        return Call('AdminUtils', 'sudo_set_admin_freeze_window', {'window': window})

    @staticmethod
    def sudo_set_alpha_sigmoid_steepness(netuid, steepness) -> Call:
        '# Arguments * `origin` - The origin of the call, which must be the root account. * `netuid` - The unique identifier for the subnet. * `steepness` - The Steepness for the alpha sigmoid function. (range is 0-int16::MAX, negative values are reserved for future use)  # Errors * `BadOrigin` - If the caller is not the root account. * `SubnetDoesNotExist` - If the specified subnet does not exist. * `NegativeSigmoidSteepness` - If the steepness is negative and the caller is root. # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_alpha_sigmoid_steepness', {'netuid': netuid, 'steepness': steepness})

    @staticmethod
    def sudo_set_alpha_values(netuid, alpha_low, alpha_high) -> Call:
        'Sets values for liquid alpha'
        return Call('AdminUtils', 'sudo_set_alpha_values', {'netuid': netuid, 'alpha_low': alpha_low, 'alpha_high': alpha_high})

    @staticmethod
    def sudo_set_bonds_moving_average(netuid, bonds_moving_average) -> Call:
        'The extrinsic sets the bonds moving average for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the bonds moving average.'
        return Call('AdminUtils', 'sudo_set_bonds_moving_average', {'netuid': netuid, 'bonds_moving_average': bonds_moving_average})

    @staticmethod
    def sudo_set_bonds_penalty(netuid, bonds_penalty) -> Call:
        'The extrinsic sets the bonds penalty for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the bonds penalty.'
        return Call('AdminUtils', 'sudo_set_bonds_penalty', {'netuid': netuid, 'bonds_penalty': bonds_penalty})

    @staticmethod
    def sudo_set_bonds_reset_enabled(netuid, enabled) -> Call:
        'Enables or disables Bonds Reset for a given subnet.  # Parameters - `origin`: The origin of the call, which must be the root account or subnet owner. - `netuid`: The unique identifier for the subnet. - `enabled`: A boolean flag to enable or disable Bonds Reset.  # Weight This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.'
        return Call('AdminUtils', 'sudo_set_bonds_reset_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_set_burn_half_life(netuid, burn_half_life) -> Call:
        'Set BurnHalfLife for a subnet. It is only callable by root and subnet owner.'
        return Call('AdminUtils', 'sudo_set_burn_half_life', {'netuid': netuid, 'burn_half_life': burn_half_life})

    @staticmethod
    def sudo_set_burn_increase_mult(netuid, burn_increase_mult) -> Call:
        'Set BurnIncreaseMult for a subnet. It is only callable by root and subnet owner.'
        return Call('AdminUtils', 'sudo_set_burn_increase_mult', {'netuid': netuid, 'burn_increase_mult': burn_increase_mult})

    @staticmethod
    def sudo_set_ck_burn(burn) -> Call:
        'Sets the childkey burn for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the childkey burn.'
        return Call('AdminUtils', 'sudo_set_ck_burn', {'burn': burn})

    @staticmethod
    def sudo_set_coldkey_swap_announcement_delay(duration) -> Call:
        'Sets the announcement delay for coldkey swap.'
        return Call('AdminUtils', 'sudo_set_coldkey_swap_announcement_delay', {'duration': duration})

    @staticmethod
    def sudo_set_coldkey_swap_reannouncement_delay(duration) -> Call:
        'Sets the coldkey swap reannouncement delay.'
        return Call('AdminUtils', 'sudo_set_coldkey_swap_reannouncement_delay', {'duration': duration})

    @staticmethod
    def sudo_set_commit_reveal_version(version) -> Call:
        'Sets the commit-reveal weights version for all subnets'
        return Call('AdminUtils', 'sudo_set_commit_reveal_version', {'version': version})

    @staticmethod
    def sudo_set_commit_reveal_weights_enabled(netuid, enabled) -> Call:
        'The extrinsic enabled/disables commit/reaveal for a given subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the value.'
        return Call('AdminUtils', 'sudo_set_commit_reveal_weights_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_set_commit_reveal_weights_interval(netuid, interval) -> Call:
        'Sets the commit-reveal weights periods for a specific subnet.  This extrinsic allows the subnet owner or root account to set the duration (in epochs) during which committed weights must be revealed. The commit-reveal mechanism ensures that users commit weights in advance and reveal them only within a specified period.  # Arguments * `origin` - The origin of the call, which must be the subnet owner or the root account. * `netuid` - The unique identifier of the subnet for which the periods are being set. * `periods` - The number of epochs that define the commit-reveal period.  # Errors * `BadOrigin` - If the caller is neither the subnet owner nor the root account. * `SubnetDoesNotExist` - If the specified subnet does not exist.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_commit_reveal_weights_interval', {'netuid': netuid, 'interval': interval})

    @staticmethod
    def sudo_set_default_take(default_take) -> Call:
        'The extrinsic sets the default take for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the default take.'
        return Call('AdminUtils', 'sudo_set_default_take', {'default_take': default_take})

    @staticmethod
    def sudo_set_difficulty(netuid, difficulty) -> Call:
        'The extrinsic sets the difficulty for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the difficulty.'
        return Call('AdminUtils', 'sudo_set_difficulty', {'netuid': netuid, 'difficulty': difficulty})

    @staticmethod
    def sudo_set_dissolve_network_schedule_duration(duration) -> Call:
        'Sets the duration of the dissolve network schedule.  This extrinsic allows the root account to set the duration for the dissolve network schedule. The dissolve network schedule determines how long it takes for a network dissolution operation to complete.  # Arguments * `origin` - The origin of the call, which must be the root account. * `duration` - The new duration for the dissolve network schedule, in number of blocks.  # Errors * `BadOrigin` - If the caller is not the root account.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_dissolve_network_schedule_duration', {'duration': duration})

    @staticmethod
    def sudo_set_ema_price_halving_period(netuid, ema_halving) -> Call:
        '# Arguments * `origin` - The origin of the call, which must be the root account. * `ema_alpha_period` - Number of blocks for EMA price to halve  # Errors * `BadOrigin` - If the caller is not the root account.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_ema_price_halving_period', {'netuid': netuid, 'ema_halving': ema_halving})

    @staticmethod
    def sudo_set_evm_chain_id(chain_id) -> Call:
        'Sets the EVM ChainID.  # Arguments * `origin` - The origin of the call, which must be the subnet owner or the root account. * `chainId` - The u64 chain ID  # Errors * `BadOrigin` - If the caller is neither the subnet owner nor the root account.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_evm_chain_id', {'chain_id': chain_id})

    @staticmethod
    def sudo_set_immunity_period(netuid, immunity_period) -> Call:
        'The extrinsic sets the immunity period for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the immunity period.'
        return Call('AdminUtils', 'sudo_set_immunity_period', {'netuid': netuid, 'immunity_period': immunity_period})

    @staticmethod
    def sudo_set_kappa(netuid, kappa) -> Call:
        'The extrinsic sets the kappa for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the kappa.'
        return Call('AdminUtils', 'sudo_set_kappa', {'netuid': netuid, 'kappa': kappa})

    @staticmethod
    def sudo_set_liquid_alpha_enabled(netuid, enabled) -> Call:
        'Enables or disables Liquid Alpha for a given subnet.  # Parameters - `origin`: The origin of the call, which must be the root account or subnet owner. - `netuid`: The unique identifier for the subnet. - `enabled`: A boolean flag to enable or disable Liquid Alpha.  # Weight This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.'
        return Call('AdminUtils', 'sudo_set_liquid_alpha_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_set_lock_reduction_interval(interval) -> Call:
        'The extrinsic sets the lock reduction interval for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the lock reduction interval.'
        return Call('AdminUtils', 'sudo_set_lock_reduction_interval', {'interval': interval})

    @staticmethod
    def sudo_set_max_allowed_uids(netuid, max_allowed_uids) -> Call:
        'The extrinsic sets the maximum allowed UIDs for a subnet. It is only callable by the root account and subnet owner. The extrinsic will call the Subtensor pallet to set the maximum allowed UIDs for a subnet.'
        return Call('AdminUtils', 'sudo_set_max_allowed_uids', {'netuid': netuid, 'max_allowed_uids': max_allowed_uids})

    @staticmethod
    def sudo_set_max_allowed_validators(netuid, max_allowed_validators) -> Call:
        'The extrinsic sets the maximum allowed validators for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the maximum allowed validators.'
        return Call('AdminUtils', 'sudo_set_max_allowed_validators', {'netuid': netuid, 'max_allowed_validators': max_allowed_validators})

    @staticmethod
    def sudo_set_max_burn(netuid, max_burn) -> Call:
        'The extrinsic sets the maximum burn for a subnet. It is only callable by root and subnet owner. The extrinsic will call the Subtensor pallet to set the maximum burn.'
        return Call('AdminUtils', 'sudo_set_max_burn', {'netuid': netuid, 'max_burn': max_burn})

    @staticmethod
    def sudo_set_max_difficulty(netuid, max_difficulty) -> Call:
        'The extrinsic sets the maximum difficulty for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the maximum difficulty.'
        return Call('AdminUtils', 'sudo_set_max_difficulty', {'netuid': netuid, 'max_difficulty': max_difficulty})

    @staticmethod
    def sudo_set_max_epochs_per_block(max_epochs_per_block) -> Call:
        'Sets the per-block cap on subnet epochs (dynamic tempo throttle).'
        return Call('AdminUtils', 'sudo_set_max_epochs_per_block', {'max_epochs_per_block': max_epochs_per_block})

    @staticmethod
    def sudo_set_max_mechanism_count(max_mechanism_count) -> Call:
        'Sets the global maximum number of mechanisms in a subnet'
        return Call('AdminUtils', 'sudo_set_max_mechanism_count', {'max_mechanism_count': max_mechanism_count})

    @staticmethod
    def sudo_set_max_registrations_per_block(netuid, max_registrations_per_block) -> Call:
        'The extrinsic sets the maximum registrations per block for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the maximum registrations per block.'
        return Call('AdminUtils', 'sudo_set_max_registrations_per_block', {'netuid': netuid, 'max_registrations_per_block': max_registrations_per_block})

    @staticmethod
    def sudo_set_mechanism_count(netuid, mechanism_count) -> Call:
        'Sets the desired number of mechanisms in a subnet'
        return Call('AdminUtils', 'sudo_set_mechanism_count', {'netuid': netuid, 'mechanism_count': mechanism_count})

    @staticmethod
    def sudo_set_mechanism_emission_split(netuid, maybe_split) -> Call:
        'Sets the emission split between mechanisms in a subnet'
        return Call('AdminUtils', 'sudo_set_mechanism_emission_split', {'netuid': netuid, 'maybe_split': maybe_split})

    @staticmethod
    def sudo_set_min_allowed_uids(netuid, min_allowed_uids) -> Call:
        'The extrinsic sets the minimum allowed UIDs for a subnet. It is only callable by the root account.'
        return Call('AdminUtils', 'sudo_set_min_allowed_uids', {'netuid': netuid, 'min_allowed_uids': min_allowed_uids})

    @staticmethod
    def sudo_set_min_allowed_weights(netuid, min_allowed_weights) -> Call:
        'The extrinsic sets the minimum allowed weights for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the minimum allowed weights.'
        return Call('AdminUtils', 'sudo_set_min_allowed_weights', {'netuid': netuid, 'min_allowed_weights': min_allowed_weights})

    @staticmethod
    def sudo_set_min_burn(netuid, min_burn) -> Call:
        'The extrinsic sets the minimum burn for a subnet. It is only callable by root and subnet owner. The extrinsic will call the Subtensor pallet to set the minimum burn.'
        return Call('AdminUtils', 'sudo_set_min_burn', {'netuid': netuid, 'min_burn': min_burn})

    @staticmethod
    def sudo_set_min_childkey_take_per_subnet(netuid, take) -> Call:
        'The extrinsic sets the minimum childkey take for a subnet. It is callable by root or the subnet owner. The subnet minimum can only make the global minimum stricter.'
        return Call('AdminUtils', 'sudo_set_min_childkey_take_per_subnet', {'netuid': netuid, 'take': take})

    @staticmethod
    def sudo_set_min_delegate_take(take) -> Call:
        'The extrinsic sets the minimum delegate take. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the minimum delegate take.'
        return Call('AdminUtils', 'sudo_set_min_delegate_take', {'take': take})

    @staticmethod
    def sudo_set_min_difficulty(netuid, min_difficulty) -> Call:
        'The extrinsic sets the minimum difficulty for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the minimum difficulty.'
        return Call('AdminUtils', 'sudo_set_min_difficulty', {'netuid': netuid, 'min_difficulty': min_difficulty})

    @staticmethod
    def sudo_set_min_non_immune_uids(netuid, min) -> Call:
        'Sets the minimum number of non-immortal & non-immune UIDs that must remain in a subnet'
        return Call('AdminUtils', 'sudo_set_min_non_immune_uids', {'netuid': netuid, 'min': min})

    @staticmethod
    def sudo_set_net_tao_flow_enabled(enabled) -> Call:
        'Enables or disables net TAO flow (protocol cost deduction from emission shares). When enabled, emission shares use net flow = user flow - protocol cost. When disabled, emission shares use gross user flow only (current behavior).'
        return Call('AdminUtils', 'sudo_set_net_tao_flow_enabled', {'enabled': enabled})

    @staticmethod
    def sudo_set_network_immunity_period(immunity_period) -> Call:
        'The extrinsic sets the immunity period for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the immunity period for the network.'
        return Call('AdminUtils', 'sudo_set_network_immunity_period', {'immunity_period': immunity_period})

    @staticmethod
    def sudo_set_network_min_lock_cost(lock_cost) -> Call:
        'The extrinsic sets the min lock cost for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the min lock cost for the network.'
        return Call('AdminUtils', 'sudo_set_network_min_lock_cost', {'lock_cost': lock_cost})

    @staticmethod
    def sudo_set_network_pow_registration_allowed(netuid, registration_allowed) -> Call:
        'The extrinsic sets the network PoW registration allowed for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the network PoW registration allowed.'
        return Call('AdminUtils', 'sudo_set_network_pow_registration_allowed', {'netuid': netuid, 'registration_allowed': registration_allowed})

    @staticmethod
    def sudo_set_network_rate_limit(rate_limit) -> Call:
        'The extrinsic sets the network rate limit for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the network rate limit.'
        return Call('AdminUtils', 'sudo_set_network_rate_limit', {'rate_limit': rate_limit})

    @staticmethod
    def sudo_set_network_registration_allowed(netuid, registration_allowed) -> Call:
        'The extrinsic sets the network registration allowed for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the network registration allowed.'
        return Call('AdminUtils', 'sudo_set_network_registration_allowed', {'netuid': netuid, 'registration_allowed': registration_allowed})

    @staticmethod
    def sudo_set_nominator_min_required_stake(min_stake) -> Call:
        'The extrinsic sets the minimum stake required for nominators. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the minimum stake required for nominators.'
        return Call('AdminUtils', 'sudo_set_nominator_min_required_stake', {'min_stake': min_stake})

    @staticmethod
    def sudo_set_owner_cut_auto_lock_enabled(netuid, enabled) -> Call:
        'Set whether subnet owner cut is auto-locked for a subnet. It is only callable by root and subnet owner.'
        return Call('AdminUtils', 'sudo_set_owner_cut_auto_lock_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_set_owner_cut_enabled(netuid, enabled) -> Call:
        'Set whether the subnet owner cut is enabled for a subnet. It is only callable by root and subnet owner.'
        return Call('AdminUtils', 'sudo_set_owner_cut_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_set_owner_hparam_rate_limit(epochs) -> Call:
        'Sets the owner hyperparameter rate limit in epochs (global multiplier). Only callable by root.'
        return Call('AdminUtils', 'sudo_set_owner_hparam_rate_limit', {'epochs': epochs})

    @staticmethod
    def sudo_set_owner_immune_neuron_limit(netuid, immune_neurons) -> Call:
        'Sets the number of immune owner neurons'
        return Call('AdminUtils', 'sudo_set_owner_immune_neuron_limit', {'netuid': netuid, 'immune_neurons': immune_neurons})

    @staticmethod
    def sudo_set_rao_recycled(netuid, rao_recycled) -> Call:
        'The extrinsic sets the recycled RAO for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the recycled RAO.'
        return Call('AdminUtils', 'sudo_set_rao_recycled', {'netuid': netuid, 'rao_recycled': rao_recycled})

    @staticmethod
    def sudo_set_recycle_or_burn(netuid, recycle_or_burn) -> Call:
        'Set the behaviour of the "burn" UID(s) for a given subnet. If set to `Burn`, the miner emission sent to the burn UID(s) will be burned. If set to `Recycle`, the miner emission sent to the burn UID(s) will be recycled.  # Parameters - `origin`: The origin of the call, which must be the root account or subnet owner. - `netuid`: The unique identifier for the subnet. - `recycle_or_burn`: The desired behaviour of the "burn" UID(s) for the subnet.'
        return Call('AdminUtils', 'sudo_set_recycle_or_burn', {'netuid': netuid, 'recycle_or_burn': recycle_or_burn})

    @staticmethod
    def sudo_set_rho(netuid, rho) -> Call:
        'The extrinsic sets the rho for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the rho.'
        return Call('AdminUtils', 'sudo_set_rho', {'netuid': netuid, 'rho': rho})

    @staticmethod
    def sudo_set_serving_rate_limit(netuid, serving_rate_limit) -> Call:
        'The extrinsic sets the serving rate limit for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the serving rate limit.'
        return Call('AdminUtils', 'sudo_set_serving_rate_limit', {'netuid': netuid, 'serving_rate_limit': serving_rate_limit})

    @staticmethod
    def sudo_set_sn_owner_hotkey(netuid, hotkey) -> Call:
        'Sets or updates the hotkey account associated with the owner of a specific subnet.  This function allows either the root origin or the current subnet owner to set or update the hotkey for a given subnet. The subnet must already exist. To prevent abuse, the call is rate-limited to once per configured interval (default: one week) per subnet.  # Parameters - `origin`: The dispatch origin of the call. Must be either root or the current owner of the subnet. - `netuid`: The unique identifier of the subnet whose owner hotkey is being set. - `hotkey`: The new hotkey account to associate with the subnet owner.  # Returns - `DispatchResult`: Returns `Ok(())` if the hotkey was successfully set, or an appropriate error otherwise.  # Errors - `Error::SubnetNotExists`: If the specified subnet does not exist. - `Error::TxRateLimitExceeded`: If the function is called more frequently than the allowed rate limit.  # Access Control Only callable by: - Root origin, or - The coldkey account that owns the subnet.  # Storage - Updates [`SubnetOwnerHotkey`] for the given `netuid`. - Reads and updates [`LastRateLimitedBlock`] for rate-limiting. - Reads [`DefaultSetSNOwnerHotkeyRateLimit`] to determine the interval between allowed updates.  # Rate Limiting This function is rate-limited to one call per subnet per interval (e.g., one week).'
        return Call('AdminUtils', 'sudo_set_sn_owner_hotkey', {'netuid': netuid, 'hotkey': hotkey})

    @staticmethod
    def sudo_set_stake_threshold(min_stake) -> Call:
        'The extrinsic sets the weights min stake. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the weights min stake.'
        return Call('AdminUtils', 'sudo_set_stake_threshold', {'min_stake': min_stake})

    @staticmethod
    def sudo_set_start_call_delay(delay) -> Call:
        'Sets the delay before a subnet can call start'
        return Call('AdminUtils', 'sudo_set_start_call_delay', {'delay': delay})

    @staticmethod
    def sudo_set_subnet_emission_enabled(netuid, enabled) -> Call:
        'Enables or disables subnet pool-side emission for a subnet.  This does not remove the subnet from emission share calculation and does not change `alpha_out`, owner cut, root proportion, pending server emission, or pending validator emission. It only zeros the pool-side `alpha_in`, `tao_in`, and `excess_tao` chain-buy paths.'
        return Call('AdminUtils', 'sudo_set_subnet_emission_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_set_subnet_limit(max_subnets) -> Call:
        'The extrinsic sets the subnet limit for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the subnet limit.'
        return Call('AdminUtils', 'sudo_set_subnet_limit', {'max_subnets': max_subnets})

    @staticmethod
    def sudo_set_subnet_moving_alpha(alpha) -> Call:
        '# Arguments * `origin` - The origin of the call, which must be the root account. * `alpha` - The new moving alpha value for the SubnetMovingAlpha.  # Errors * `BadOrigin` - If the caller is not the root account.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_subnet_moving_alpha', {'alpha': alpha})

    @staticmethod
    def sudo_set_subnet_owner_cut(subnet_owner_cut) -> Call:
        'The extrinsic sets the subnet owner cut for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the subnet owner cut.'
        return Call('AdminUtils', 'sudo_set_subnet_owner_cut', {'subnet_owner_cut': subnet_owner_cut})

    @staticmethod
    def sudo_set_subtoken_enabled(netuid, subtoken_enabled) -> Call:
        'Enables or disables subtoken trading for a given subnet.  # Arguments * `origin` - The origin of the call, which must be the root account. * `netuid` - The unique identifier of the subnet. * `subtoken_enabled` - A boolean indicating whether subtoken trading should be enabled or disabled.  # Errors * `BadOrigin` - If the caller is not the root account.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_set_subtoken_enabled', {'netuid': netuid, 'subtoken_enabled': subtoken_enabled})

    @staticmethod
    def sudo_set_tao_flow_cutoff(flow_cutoff) -> Call:
        'Sets TAO flow cutoff value (A)'
        return Call('AdminUtils', 'sudo_set_tao_flow_cutoff', {'flow_cutoff': flow_cutoff})

    @staticmethod
    def sudo_set_tao_flow_normalization_exponent(exponent) -> Call:
        'Sets TAO flow normalization exponent (p)'
        return Call('AdminUtils', 'sudo_set_tao_flow_normalization_exponent', {'exponent': exponent})

    @staticmethod
    def sudo_set_tao_flow_smoothing_factor(smoothing_factor) -> Call:
        'Sets TAO flow smoothing factor (alpha)'
        return Call('AdminUtils', 'sudo_set_tao_flow_smoothing_factor', {'smoothing_factor': smoothing_factor})

    @staticmethod
    def sudo_set_target_registrations_per_interval(netuid, target_registrations_per_interval) -> Call:
        'The extrinsic sets the target registrations per interval for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the target registrations per interval.'
        return Call('AdminUtils', 'sudo_set_target_registrations_per_interval', {'netuid': netuid, 'target_registrations_per_interval': target_registrations_per_interval})

    @staticmethod
    def sudo_set_tempo(netuid, tempo) -> Call:
        'The extrinsic sets the tempo for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the tempo.'
        return Call('AdminUtils', 'sudo_set_tempo', {'netuid': netuid, 'tempo': tempo})

    @staticmethod
    def sudo_set_toggle_transfer(netuid, toggle) -> Call:
        'Enable or disable atomic alpha transfers for a given subnet.  # Parameters - `origin`: The origin of the call, which must be the root account or subnet owner. - `netuid`: The unique identifier for the subnet. - `enabled`: A boolean flag to enable or disable Liquid Alpha.  # Weight This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.'
        return Call('AdminUtils', 'sudo_set_toggle_transfer', {'netuid': netuid, 'toggle': toggle})

    @staticmethod
    def sudo_set_total_issuance(total_issuance) -> Call:
        'DEPRECATED'
        return Call('AdminUtils', 'sudo_set_total_issuance', {'total_issuance': total_issuance})

    @staticmethod
    def sudo_set_tx_delegate_take_rate_limit(tx_rate_limit) -> Call:
        'The extrinsic sets the rate limit for delegate take transactions. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the rate limit for delegate take transactions.'
        return Call('AdminUtils', 'sudo_set_tx_delegate_take_rate_limit', {'tx_rate_limit': tx_rate_limit})

    @staticmethod
    def sudo_set_tx_rate_limit(tx_rate_limit) -> Call:
        'The extrinsic sets the transaction rate limit for the network. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the transaction rate limit.'
        return Call('AdminUtils', 'sudo_set_tx_rate_limit', {'tx_rate_limit': tx_rate_limit})

    @staticmethod
    def sudo_set_weights_set_rate_limit(netuid, weights_set_rate_limit) -> Call:
        'The extrinsic sets the weights set rate limit for a subnet. It is only callable by the root account. The extrinsic will call the Subtensor pallet to set the weights set rate limit.'
        return Call('AdminUtils', 'sudo_set_weights_set_rate_limit', {'netuid': netuid, 'weights_set_rate_limit': weights_set_rate_limit})

    @staticmethod
    def sudo_set_weights_version_key(netuid, weights_version_key) -> Call:
        'The extrinsic sets the weights version key for a subnet. It is only callable by the root account or subnet owner. The extrinsic will call the Subtensor pallet to set the weights version key.'
        return Call('AdminUtils', 'sudo_set_weights_version_key', {'netuid': netuid, 'weights_version_key': weights_version_key})

    @staticmethod
    def sudo_set_yuma3_enabled(netuid, enabled) -> Call:
        'Enables or disables Yuma3 for a given subnet.  # Parameters - `origin`: The origin of the call, which must be the root account or subnet owner. - `netuid`: The unique identifier for the subnet. - `enabled`: A boolean flag to enable or disable Yuma3.  # Weight This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.'
        return Call('AdminUtils', 'sudo_set_yuma3_enabled', {'netuid': netuid, 'enabled': enabled})

    @staticmethod
    def sudo_toggle_evm_precompile(precompile_id, enabled) -> Call:
        'Toggles the enablement of an EVM precompile.  # Arguments * `origin` - The origin of the call, which must be the root account. * `precompile_id` - The identifier of the EVM precompile to toggle. * `enabled` - The new enablement state of the precompile.  # Errors * `BadOrigin` - If the caller is not the root account.  # Weight Weight is handled by the `#[pallet::weight]` attribute.'
        return Call('AdminUtils', 'sudo_toggle_evm_precompile', {'precompile_id': precompile_id, 'enabled': enabled})

    @staticmethod
    def sudo_trim_to_max_allowed_uids(netuid, max_n) -> Call:
        'Trims the maximum number of UIDs for a subnet.  The trimming is done by sorting the UIDs by emission descending and then trimming the lowest emitters while preserving temporally and owner immune UIDs. The UIDs are then compressed to the left and storage is migrated to the new compressed UIDs.'
        return Call('AdminUtils', 'sudo_trim_to_max_allowed_uids', {'netuid': netuid, 'max_n': max_n})

    @staticmethod
    def swap_authorities(new_authorities) -> Call:
        'The extrinsic sets the new authorities for Aura consensus. It is only callable by the root account. The extrinsic will call the Aura pallet to change the authorities.'
        return Call('AdminUtils', 'swap_authorities', {'new_authorities': new_authorities})


class SafeMode:
    """Call builders for the SafeMode pallet."""

    @staticmethod
    def enter() -> Call:
        "Enter safe-mode permissionlessly for [`Config::EnterDuration`] blocks.  Reserves [`Config::EnterDepositAmount`] from the caller's account. Emits an [`Event::Entered`] event on success. Errors with [`Error::Entered`] if the safe-mode is already entered. Errors with [`Error::NotConfigured`] if the deposit amount is `None`."
        return Call('SafeMode', 'enter', {})

    @staticmethod
    def extend() -> Call:
        "Extend the safe-mode permissionlessly for [`Config::ExtendDuration`] blocks.  This accumulates on top of the current remaining duration. Reserves [`Config::ExtendDepositAmount`] from the caller's account. Emits an [`Event::Extended`] event on success. Errors with [`Error::Exited`] if the safe-mode is entered. Errors with [`Error::NotConfigured`] if the deposit amount is `None`.  This may be called by any signed origin with [`Config::ExtendDepositAmount`] free currency to reserve. This call can be disabled for all origins by configuring [`Config::ExtendDepositAmount`] to `None`."
        return Call('SafeMode', 'extend', {})

    @staticmethod
    def force_enter() -> Call:
        'Enter safe-mode by force for a per-origin configured number of blocks.  Emits an [`Event::Entered`] event on success. Errors with [`Error::Entered`] if the safe-mode is already entered.  Can only be called by the [`Config::ForceEnterOrigin`] origin.'
        return Call('SafeMode', 'force_enter', {})

    @staticmethod
    def force_exit() -> Call:
        'Exit safe-mode by force.  Emits an [`Event::Exited`] with [`ExitReason::Force`] event on success. Errors with [`Error::Exited`] if the safe-mode is inactive.  Note: `safe-mode` will be automatically deactivated by [`Pallet::on_initialize`] hook after the block height is greater than the [`EnteredUntil`] storage item. Emits an [`Event::Exited`] with [`ExitReason::Timeout`] event when deactivated in the hook.'
        return Call('SafeMode', 'force_exit', {})

    @staticmethod
    def force_extend() -> Call:
        'Extend the safe-mode by force for a per-origin configured number of blocks.  Emits an [`Event::Extended`] event on success. Errors with [`Error::Exited`] if the safe-mode is inactive.  Can only be called by the [`Config::ForceExtendOrigin`] origin.'
        return Call('SafeMode', 'force_extend', {})

    @staticmethod
    def force_release_deposit(account, block) -> Call:
        'Force to release a deposit for an account that entered safe-mode at a given historical block.  This can be called while safe-mode is still entered.  Emits a [`Event::DepositReleased`] event on success. Errors with [`Error::Entered`] if safe-mode is entered. Errors with [`Error::NoDeposit`] if the payee has no reserved currency at the specified block.  Can only be called by the [`Config::ForceDepositOrigin`] origin.'
        return Call('SafeMode', 'force_release_deposit', {'account': account, 'block': block})

    @staticmethod
    def force_slash_deposit(account, block) -> Call:
        'Slash a deposit for an account that entered or extended safe-mode at a given historical block.  This can only be called while safe-mode is entered.  Emits a [`Event::DepositSlashed`] event on success. Errors with [`Error::Entered`] if safe-mode is entered.  Can only be called by the [`Config::ForceDepositOrigin`] origin.'
        return Call('SafeMode', 'force_slash_deposit', {'account': account, 'block': block})

    @staticmethod
    def release_deposit(account, block) -> Call:
        'Permissionlessly release a deposit for an account that entered safe-mode at a given historical block.  The call can be completely disabled by setting [`Config::ReleaseDelay`] to `None`. This cannot be called while safe-mode is entered and not until [`Config::ReleaseDelay`] blocks have passed since safe-mode was entered.  Emits a [`Event::DepositReleased`] event on success. Errors with [`Error::Entered`] if the safe-mode is entered. Errors with [`Error::CannotReleaseYet`] if [`Config::ReleaseDelay`] block have not passed since safe-mode was entered. Errors with [`Error::NoDeposit`] if the payee has no reserved currency at the block specified.'
        return Call('SafeMode', 'release_deposit', {'account': account, 'block': block})


class Ethereum:
    """Call builders for the Ethereum pallet."""

    @staticmethod
    def transact(transaction) -> Call:
        'Transact an Ethereum transaction.'
        return Call('Ethereum', 'transact', {'transaction': transaction})


class EVM:
    """Call builders for the EVM pallet."""

    @staticmethod
    def call(source, target, input, value, gas_limit, max_fee_per_gas, max_priority_fee_per_gas, nonce, access_list, authorization_list) -> Call:
        'Issue an EVM call operation. This is similar to a message call transaction in Ethereum.'
        return Call('EVM', 'call', {'source': source, 'target': target, 'input': input, 'value': value, 'gas_limit': gas_limit, 'max_fee_per_gas': max_fee_per_gas, 'max_priority_fee_per_gas': max_priority_fee_per_gas, 'nonce': nonce, 'access_list': access_list, 'authorization_list': authorization_list})

    @staticmethod
    def create(source, init, value, gas_limit, max_fee_per_gas, max_priority_fee_per_gas, nonce, access_list, authorization_list) -> Call:
        'Issue an EVM create operation. This is similar to a contract creation transaction in Ethereum.'
        return Call('EVM', 'create', {'source': source, 'init': init, 'value': value, 'gas_limit': gas_limit, 'max_fee_per_gas': max_fee_per_gas, 'max_priority_fee_per_gas': max_priority_fee_per_gas, 'nonce': nonce, 'access_list': access_list, 'authorization_list': authorization_list})

    @staticmethod
    def create2(source, init, salt, value, gas_limit, max_fee_per_gas, max_priority_fee_per_gas, nonce, access_list, authorization_list) -> Call:
        'Issue an EVM create2 operation.'
        return Call('EVM', 'create2', {'source': source, 'init': init, 'salt': salt, 'value': value, 'gas_limit': gas_limit, 'max_fee_per_gas': max_fee_per_gas, 'max_priority_fee_per_gas': max_priority_fee_per_gas, 'nonce': nonce, 'access_list': access_list, 'authorization_list': authorization_list})

    @staticmethod
    def disable_whitelist(disabled) -> Call:
        return Call('EVM', 'disable_whitelist', {'disabled': disabled})

    @staticmethod
    def set_whitelist(new) -> Call:
        return Call('EVM', 'set_whitelist', {'new': new})

    @staticmethod
    def withdraw(address, value) -> Call:
        'Withdraw balance from EVM into currency/balances pallet.'
        return Call('EVM', 'withdraw', {'address': address, 'value': value})


class BaseFee:
    """Call builders for the BaseFee pallet."""

    @staticmethod
    def set_base_fee_per_gas(fee) -> Call:
        return Call('BaseFee', 'set_base_fee_per_gas', {'fee': fee})

    @staticmethod
    def set_elasticity(elasticity) -> Call:
        return Call('BaseFee', 'set_elasticity', {'elasticity': elasticity})


class Drand:
    """Call builders for the Drand pallet."""

    @staticmethod
    def set_beacon_config(config_payload, signature) -> Call:
        'allows the root user to set the beacon configuration generally this would be called from an offchain worker context. there is no verification of configurations, so be careful with this.  * `origin`: the root user * `config`: the beacon configuration'
        return Call('Drand', 'set_beacon_config', {'config_payload': config_payload, 'signature': signature})

    @staticmethod
    def set_oldest_stored_round(oldest_round) -> Call:
        'allows the root user to set the oldest stored round'
        return Call('Drand', 'set_oldest_stored_round', {'oldest_round': oldest_round})

    @staticmethod
    def write_pulse(pulses_payload, signature) -> Call:
        'Verify and write a pulse from the beacon into the runtime'
        return Call('Drand', 'write_pulse', {'pulses_payload': pulses_payload, 'signature': signature})


class Crowdloan:
    """Call builders for the Crowdloan pallet."""

    @staticmethod
    def contribute(crowdloan_id, amount) -> Call:
        'Contribute to an active crowdloan.  The contribution will be transferred to the crowdloan account and will be refunded if the crowdloan fails to raise the cap. If the contribution would raise the amount above the cap, the contribution will be set to the amount that is left to be raised.  The dispatch origin for this call must be _Signed_.  Parameters: - `crowdloan_id`: The id of the crowdloan to contribute to. - `amount`: The amount to contribute.'
        return Call('Crowdloan', 'contribute', {'crowdloan_id': crowdloan_id, 'amount': amount})

    @staticmethod
    def create(deposit, min_contribution, cap, end, call, target_address) -> Call:
        'Create a crowdloan that will raise funds up to a maximum cap and if successful, will either transfer funds to the target address or dispatch the call (using creator origin). Exactly one of call or target address must be provided. Providing both, or providing neither, is rejected.  The initial deposit will be transferred to the crowdloan account and will be refunded in case the crowdloan fails to raise the cap. Additionally, the creator will pay for the execution of the call.  The dispatch origin for this call must be _Signed_.  Parameters: - `deposit`: The initial deposit from the creator. - `min_contribution`: The minimum contribution required to contribute to the crowdloan. - `cap`: The maximum amount of funds that can be raised. - `end`: The block number at which the crowdloan will end. - `call`: The call to dispatch when the crowdloan is finalized. - `target_address`: The address to transfer the raised funds to.'
        return Call('Crowdloan', 'create', {'deposit': deposit, 'min_contribution': min_contribution, 'cap': cap, 'end': end, 'call': call, 'target_address': target_address})

    @staticmethod
    def dissolve(crowdloan_id) -> Call:
        "Dissolve a crowdloan.  The crowdloan will be removed from the storage. All contributions must have been refunded before the crowdloan can be dissolved (except the creator's one).  The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to dissolve."
        return Call('Crowdloan', 'dissolve', {'crowdloan_id': crowdloan_id})

    @staticmethod
    def finalize(crowdloan_id) -> Call:
        'Finalize crowdloan that has reached the cap.  The call will either transfer the raised amount to the configured target address or dispatch the configured call using the creator origin. The stored crowdloan must contain exactly one of target address or call; if both or neither are set, finalization fails before transfer or dispatch.  When dispatching a call, the CurrentCrowdloanId will be set to the crowdloan id being finalized so the dispatched call can access it temporarily by accessing the `CurrentCrowdloanId` storage item.  The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to finalize.'
        return Call('Crowdloan', 'finalize', {'crowdloan_id': crowdloan_id})

    @staticmethod
    def refund(crowdloan_id) -> Call:
        "Refund contributors of a non-finalized crowdloan.  The call will try to refund all contributors (excluding the creator) up to the limit defined by the `RefundContributorsLimit`. If the limit is reached, the call will stop and the crowdloan will be marked as partially refunded. It may be needed to dispatch this call multiple times to refund all contributors.  The dispatch origin for this call must be _Signed_ and doesn't need to be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to refund."
        return Call('Crowdloan', 'refund', {'crowdloan_id': crowdloan_id})

    @staticmethod
    def set_max_contribution(crowdloan_id, new_max_contribution) -> Call:
        'Set or clear the maximum cumulative contribution allowed per contributor for a non-finalized crowdloan.  The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to update the maximum contribution of. - `new_max_contribution`: The new optional maximum contribution.'
        return Call('Crowdloan', 'set_max_contribution', {'crowdloan_id': crowdloan_id, 'new_max_contribution': new_max_contribution})

    @staticmethod
    def update_cap(crowdloan_id, new_cap) -> Call:
        'Update the cap of a non-finalized crowdloan.  The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to update the cap of. - `new_cap`: The new cap.'
        return Call('Crowdloan', 'update_cap', {'crowdloan_id': crowdloan_id, 'new_cap': new_cap})

    @staticmethod
    def update_end(crowdloan_id, new_end) -> Call:
        'Update the end block of a non-finalized crowdloan.  The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to update the end block of. - `new_end`: The new end block.'
        return Call('Crowdloan', 'update_end', {'crowdloan_id': crowdloan_id, 'new_end': new_end})

    @staticmethod
    def update_min_contribution(crowdloan_id, new_min_contribution) -> Call:
        'Update the minimum contribution of a non-finalized crowdloan.  If a maximum contribution is configured, the new minimum contribution must not exceed it.  The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.  Parameters: - `crowdloan_id`: The id of the crowdloan to update the minimum contribution of. - `new_min_contribution`: The new minimum contribution.'
        return Call('Crowdloan', 'update_min_contribution', {'crowdloan_id': crowdloan_id, 'new_min_contribution': new_min_contribution})

    @staticmethod
    def withdraw(crowdloan_id) -> Call:
        'Withdraw a contribution from an active (not yet finalized or dissolved) crowdloan.  Only contributions over the deposit can be withdrawn by the creator.  The dispatch origin for this call must be _Signed_.  Parameters: - `crowdloan_id`: The id of the crowdloan to withdraw from.'
        return Call('Crowdloan', 'withdraw', {'crowdloan_id': crowdloan_id})


class Swap:
    """Call builders for the Swap pallet."""

    @staticmethod
    def add_liquidity(hotkey, netuid, tick_low, tick_high, liquidity) -> Call:
        'DEPRECATED'
        return Call('Swap', 'add_liquidity', {'hotkey': hotkey, 'netuid': netuid, 'tick_low': tick_low, 'tick_high': tick_high, 'liquidity': liquidity})

    @staticmethod
    def disable_lp() -> Call:
        'DEPRECATED'
        return Call('Swap', 'disable_lp', {})

    @staticmethod
    def modify_position(hotkey, netuid, position_id, liquidity_delta) -> Call:
        'DEPRECATED'
        return Call('Swap', 'modify_position', {'hotkey': hotkey, 'netuid': netuid, 'position_id': position_id, 'liquidity_delta': liquidity_delta})

    @staticmethod
    def remove_liquidity(hotkey, netuid, position_id) -> Call:
        'DEPRECATED'
        return Call('Swap', 'remove_liquidity', {'hotkey': hotkey, 'netuid': netuid, 'position_id': position_id})

    @staticmethod
    def set_fee_rate(netuid, rate) -> Call:
        'Set the fee rate for swaps on a specific subnet (normalized value). For example, 0.3% is approximately 196.  Only callable by the admin origin'
        return Call('Swap', 'set_fee_rate', {'netuid': netuid, 'rate': rate})

    @staticmethod
    def toggle_user_liquidity(netuid, enable) -> Call:
        'DEPRECATED'
        return Call('Swap', 'toggle_user_liquidity', {'netuid': netuid, 'enable': enable})


class Contracts:
    """Call builders for the Contracts pallet."""

    @staticmethod
    def call(dest, value, gas_limit, storage_deposit_limit, data) -> Call:
        'Makes a call to an account, optionally transferring some balance.  # Parameters  * `dest`: Address of the contract to call. * `value`: The balance to transfer from the `origin` to `dest`. * `gas_limit`: The gas limit enforced when executing the constructor. * `storage_deposit_limit`: The maximum amount of balance that can be charged from the caller to pay for the storage consumed. * `data`: The input data to pass to the contract.  * If the account is a smart-contract account, the associated code will be executed and any value will be transferred. * If the account is a regular account, any value will be transferred. * If no account exists and the call value is not less than `existential_deposit`, a regular account will be created and any value will be transferred.'
        return Call('Contracts', 'call', {'dest': dest, 'value': value, 'gas_limit': gas_limit, 'storage_deposit_limit': storage_deposit_limit, 'data': data})

    @staticmethod
    def call_old_weight(dest, value, gas_limit, storage_deposit_limit, data) -> Call:
        'Deprecated version if [`Self::call`] for use in an in-storage `Call`.'
        return Call('Contracts', 'call_old_weight', {'dest': dest, 'value': value, 'gas_limit': gas_limit, 'storage_deposit_limit': storage_deposit_limit, 'data': data})

    @staticmethod
    def instantiate(value, gas_limit, storage_deposit_limit, code_hash, data, salt) -> Call:
        'Instantiates a contract from a previously deployed wasm binary.  This function is identical to [`Self::instantiate_with_code`] but without the code deployment step. Instead, the `code_hash` of an on-chain deployed wasm binary must be supplied.'
        return Call('Contracts', 'instantiate', {'value': value, 'gas_limit': gas_limit, 'storage_deposit_limit': storage_deposit_limit, 'code_hash': code_hash, 'data': data, 'salt': salt})

    @staticmethod
    def instantiate_old_weight(value, gas_limit, storage_deposit_limit, code_hash, data, salt) -> Call:
        'Deprecated version if [`Self::instantiate`] for use in an in-storage `Call`.'
        return Call('Contracts', 'instantiate_old_weight', {'value': value, 'gas_limit': gas_limit, 'storage_deposit_limit': storage_deposit_limit, 'code_hash': code_hash, 'data': data, 'salt': salt})

    @staticmethod
    def instantiate_with_code(value, gas_limit, storage_deposit_limit, code, data, salt) -> Call:
        'Instantiates a new contract from the supplied `code` optionally transferring some balance.  This dispatchable has the same effect as calling [`Self::upload_code`] + [`Self::instantiate`]. Bundling them together provides efficiency gains. Please also check the documentation of [`Self::upload_code`].  # Parameters  * `value`: The balance to transfer from the `origin` to the newly created contract. * `gas_limit`: The gas limit enforced when executing the constructor. * `storage_deposit_limit`: The maximum amount of balance that can be charged/reserved from the caller to pay for the storage consumed. * `code`: The contract code to deploy in raw bytes. * `data`: The input data to pass to the contract constructor. * `salt`: Used for the address derivation. See [`Pallet::contract_address`].  Instantiation is executed as follows:  - The supplied `code` is deployed, and a `code_hash` is created for that code. - If the `code_hash` already exists on the chain the underlying `code` will be shared. - The destination address is computed based on the sender, code_hash and the salt. - The smart-contract account is created at the computed address. - The `value` is transferred to the new account. - The `deploy` function is executed in the context of the newly-created account.'
        return Call('Contracts', 'instantiate_with_code', {'value': value, 'gas_limit': gas_limit, 'storage_deposit_limit': storage_deposit_limit, 'code': code, 'data': data, 'salt': salt})

    @staticmethod
    def instantiate_with_code_old_weight(value, gas_limit, storage_deposit_limit, code, data, salt) -> Call:
        'Deprecated version if [`Self::instantiate_with_code`] for use in an in-storage `Call`.'
        return Call('Contracts', 'instantiate_with_code_old_weight', {'value': value, 'gas_limit': gas_limit, 'storage_deposit_limit': storage_deposit_limit, 'code': code, 'data': data, 'salt': salt})

    @staticmethod
    def migrate(weight_limit) -> Call:
        "When a migration is in progress, this dispatchable can be used to run migration steps. Calls that contribute to advancing the migration have their fees waived, as it's helpful for the chain. Note that while the migration is in progress, the pallet will also leverage the `on_idle` hooks to run migration steps."
        return Call('Contracts', 'migrate', {'weight_limit': weight_limit})

    @staticmethod
    def remove_code(code_hash) -> Call:
        'Remove the code stored under `code_hash` and refund the deposit to its owner.  A code can only be removed by its original uploader (its owner) and only if it is not used by any contract.'
        return Call('Contracts', 'remove_code', {'code_hash': code_hash})

    @staticmethod
    def set_code(dest, code_hash) -> Call:
        'Privileged function that changes the code of an existing contract.  This takes care of updating refcounts and all other necessary operations. Returns an error if either the `code_hash` or `dest` do not exist.  # Note  This does **not** change the address of the contract in question. This means that the contract address is no longer derived from its code hash after calling this dispatchable.'
        return Call('Contracts', 'set_code', {'dest': dest, 'code_hash': code_hash})

    @staticmethod
    def upload_code(code, storage_deposit_limit, determinism) -> Call:
        'Upload new `code` without instantiating a contract from it.  If the code does not already exist a deposit is reserved from the caller and unreserved only when [`Self::remove_code`] is called. The size of the reserve depends on the size of the supplied `code`.  If the code already exists in storage it will still return `Ok` and upgrades the in storage version to the current [`InstructionWeights::version`](InstructionWeights).  - `determinism`: If this is set to any other value but [`Determinism::Enforced`] then the only way to use this code is to delegate call into it from an offchain execution. Set to [`Determinism::Enforced`] if in doubt.  # Note  Anyone can instantiate a contract from any uploaded code and thus prevent its removal. To avoid this situation a constructor could employ access control so that it can only be instantiated by permissioned entities. The same is true when uploading through [`Self::instantiate_with_code`].  Use [`Determinism::Relaxed`] exclusively for non-deterministic code. If the uploaded code is deterministic, specifying [`Determinism::Relaxed`] will be disregarded and result in higher gas costs.'
        return Call('Contracts', 'upload_code', {'code': code, 'storage_deposit_limit': storage_deposit_limit, 'determinism': determinism})


class MevShield:
    """Call builders for the MevShield pallet."""

    @staticmethod
    def announce_next_key(enc_key) -> Call:
        "Rotate the key chain and announce the current author's ML-KEM encapsulation key.  Called as an inherent every block. `enc_key` is `None` on node failure, which removes the author from future shielded tx eligibility.  Key rotation order (using pre-update AuthorKeys): 1. CurrentKey  ← PendingKey 2. PendingKey  ← NextKey 3. NextKey     ← next-next author's key  (user-facing) 4. AuthorKeys[current] ← announced key"
        return Call('MevShield', 'announce_next_key', {'enc_key': enc_key})

    @staticmethod
    def set_max_extrinsic_weight(value) -> Call:
        'Set the maximum weight allowed for a single extrinsic during on_initialize processing. Extrinsics exceeding this limit are removed from the queue. Rejects values exceeding the absolute limit.'
        return Call('MevShield', 'set_max_extrinsic_weight', {'value': value})

    @staticmethod
    def set_max_pending_extrinsics_number(value) -> Call:
        'Set the maximum number of pending extrinsics allowed in the queue.'
        return Call('MevShield', 'set_max_pending_extrinsics_number', {'value': value})

    @staticmethod
    def set_on_initialize_weight(value) -> Call:
        'Set the maximum weight allowed for on_initialize processing. Rejects values exceeding the absolute limit (half of total block weight).'
        return Call('MevShield', 'set_on_initialize_weight', {'value': value})

    @staticmethod
    def set_stored_extrinsic_lifetime(value) -> Call:
        'Set the extrinsic lifetime (max blocks between submission and execution).'
        return Call('MevShield', 'set_stored_extrinsic_lifetime', {'value': value})

    @staticmethod
    def store_encrypted(encrypted_call) -> Call:
        'Store an encrypted extrinsic for later execution in on_initialize.'
        return Call('MevShield', 'store_encrypted', {'encrypted_call': encrypted_call})

    @staticmethod
    def submit_encrypted(ciphertext) -> Call:
        'Users submit an encrypted wrapper.  Client‑side:  1. Read `NextKey` (ML‑KEM encapsulation key bytes) from storage. 2. Sign your extrinsic so that it can be executed when added to the pool, i.e. you may need to increment the nonce if you submit using the same account. 3. Encrypt:  plaintext = signed_extrinsic key_hash = xxhash128(NextKey) kem_len = Length of kem_ct in bytes (u16) kem_ct = Ciphertext from ML‑KEM‑768 nonce = Random 24 bytes used for XChaCha20‑Poly1305 aead_ct = Ciphertext from XChaCha20‑Poly1305  with ML‑KEM‑768 + XChaCha20‑Poly1305, producing  ciphertext = key_hash || kem_len || kem_ct || nonce || aead_ct'
        return Call('MevShield', 'submit_encrypted', {'ciphertext': ciphertext})


class LimitOrders:
    """Call builders for the LimitOrders pallet."""

    @staticmethod
    def cancel_order(order) -> Call:
        "Register a cancellation intent for an order.  Must be called by the order's signer. The full `Order` payload is provided so the pallet can derive the `OrderId`. Once marked Cancelled, the order can never be executed."
        return Call('LimitOrders', 'cancel_order', {'order': order})

    @staticmethod
    def execute_batched_orders(netuid, orders) -> Call:
        'Execute a batch of signed limit orders for a single subnet using aggregated (netted) pool interaction.  Unlike `execute_orders`, which hits the pool once per order, this extrinsic:  1. Validates all orders (bad signature / expired / already processed / price-not-met orders are skipped and emit `OrderSkipped`). 2. Fetches the current price once. 3. Aggregates all valid buy inputs (TAO) and sell inputs (alpha). 4. Nets the two sides: only the residual amount touches the pool in a single swap, minimising price impact. 5. Distributes outputs pro-rata: - Dominant-side orders split the pool output proportionally to their individual net amounts. - Offset-side orders are filled internally at the current price (no pool interaction for them). 6. Collects protocol fees (TAO for buy orders, alpha → TAO for sell orders) and routes them to `FeeCollector`.  All orders in the batch must target `netuid`. Orders for a different subnet are skipped.'
        return Call('LimitOrders', 'execute_batched_orders', {'netuid': netuid, 'orders': orders})

    @staticmethod
    def execute_orders(orders, should_fail) -> Call:
        'Execute a batch of signed limit orders. Admin-gated.  The `should_fail` flag controls how individual order failures are handled:  - When `false` (best-effort): orders whose price condition is not yet met are silently skipped so that a single stale order cannot block the rest of the batch. Orders that fail for any other reason (expired, bad signature, etc.) are also skipped; the admin is expected to filter these off-chain. - When `true` (all-or-nothing): the first order failure aborts the whole batch by returning the underlying error, reverting any orders already executed in this call.'
        return Call('LimitOrders', 'execute_orders', {'orders': orders, 'should_fail': should_fail})

    @staticmethod
    def set_pallet_status(enabled) -> Call:
        'Set a status for the limit orders pallet  Must be called by root It allows disabling or enabling the pallet true means enabling, false means disabling'
        return Call('LimitOrders', 'set_pallet_status', {'enabled': enabled})


