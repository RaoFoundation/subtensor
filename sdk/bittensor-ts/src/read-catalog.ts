// Read names and parameter contracts mirror sdk/python/bittensor/reads.
export const READ_CATALOG = [
  {
    "name": "accepts_locked_alpha",
    "category": "Locks & conviction",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Whether a coldkey accepts incoming locked alpha transfers."
  },
  {
    "name": "alpha_price",
    "category": "Prices & swaps",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Current spot alpha price for a subnet, as TAO per alpha."
  },
  {
    "name": "alpha_prices",
    "category": "Prices & swaps",
    "params": {},
    "defaults": {},
    "summary": "Spot alpha price for every subnet, as TAO per alpha keyed by netuid."
  },
  {
    "name": "associated_evm_key",
    "category": "Neurons & registration",
    "params": {
      "netuid": "integer",
      "uid": "integer"
    },
    "defaults": {},
    "summary": "The EVM address associated with a neuron (by netuid + uid) and the block it was set."
  },
  {
    "name": "auto_stake",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "The hotkey a coldkey auto-stakes its rewards to on a subnet, or None if unset."
  },
  {
    "name": "auto_stake_all",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every auto-stake destination configured for a coldkey."
  },
  {
    "name": "balance",
    "category": "Accounts & keys",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Free TAO balance of a coldkey address."
  },
  {
    "name": "balances",
    "category": "Accounts & keys",
    "params": {
      "coldkey_ss58s": "array"
    },
    "defaults": {},
    "summary": "Free TAO balance for several coldkey addresses in one batched request."
  },
  {
    "name": "basket_position",
    "category": "Staking",
    "params": {
      "hotkey_ss58": "string",
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A staker's beta token position in one validator's basket."
  },
  {
    "name": "basket_trading_status",
    "category": "Staking",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A validator's `swap_basket` trading status: gates and the turnover bucket."
  },
  {
    "name": "block_info",
    "category": "Chain",
    "params": {
      "block": "integer"
    },
    "defaults": {},
    "summary": "A block's hash, timestamp, and extrinsics (as module.function summaries)."
  },
  {
    "name": "block_time",
    "category": "Chain",
    "params": {},
    "defaults": {},
    "summary": "Seconds per block, detected from the chain (12.0 mainnet, 0.25 fast-blocks)."
  },
  {
    "name": "blocks_since_last_step",
    "category": "Epochs & timing",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Blocks since the subnet's last epoch step ran."
  },
  {
    "name": "blocks_since_last_update",
    "category": "Epochs & timing",
    "params": {
      "netuid": "integer",
      "uid": "integer"
    },
    "defaults": {},
    "summary": "Blocks since a neuron last set or committed weights on a subnet"
  },
  {
    "name": "blocks_until_next_epoch",
    "category": "Epochs & timing",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Blocks until the subnet's next epoch fires, or None if tempo is 0."
  },
  {
    "name": "bonds",
    "category": "Weights & bonds",
    "params": {
      "netuid": "integer",
      "mechid": "integer"
    },
    "defaults": {
      "mechid": 0
    },
    "summary": "Validator bond rows as ``{validator_uid: {miner_uid: bond}}``, scaled to 0..1."
  },
  {
    "name": "burn",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Current burn (recycle) cost to register on a subnet."
  },
  {
    "name": "childkey_threshold_suspended",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Whether a parent hotkey's child relations are currently inert."
  },
  {
    "name": "children",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Child hotkeys of a parent on a subnet, as (proportion, child_ss58) pairs."
  },
  {
    "name": "coldkey_lock",
    "category": "Locks & conviction",
    "params": {
      "coldkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Lock state for a coldkey on a subnet, or None if no lock exists."
  },
  {
    "name": "coldkey_swap_announcement",
    "category": "Accounts & keys",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A coldkey's swap state (execute block, new-key hash, dispute, recovery lock), or None."
  },
  {
    "name": "collateral_policy",
    "category": "Mining & collateral",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "A subnet's collateral configuration."
  },
  {
    "name": "commit_reveal_enabled",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Whether commit-reveal weights are enabled on a subnet."
  },
  {
    "name": "commitment",
    "category": "Identity & commitments",
    "params": {
      "netuid": "integer",
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "The commitment a hotkey has published on a subnet, or None."
  },
  {
    "name": "commitments",
    "category": "Identity & commitments",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Every commitment on a subnet, newest first: hotkey, uid, content, block, age, reveal state."
  },
  {
    "name": "crowdloan",
    "category": "Leases & crowdloans",
    "params": {
      "crowdloan_id": "integer"
    },
    "defaults": {},
    "summary": "A crowdloan's state (creator, deposit, raised, cap, end, target/call), or None."
  },
  {
    "name": "crowdloan_contributors",
    "category": "Leases & crowdloans",
    "params": {
      "crowdloan_id": "integer"
    },
    "defaults": {},
    "summary": "Contributors and amounts for a crowdloan, with amounts in TAO."
  },
  {
    "name": "crowdloans",
    "category": "Leases & crowdloans",
    "params": {},
    "defaults": {},
    "summary": "All crowdloans on chain (id and summary fields)."
  },
  {
    "name": "delegate",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Delegate info for one hotkey, or None if it is not a delegate."
  },
  {
    "name": "delegate_take",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A hotkey's delegate take (emission fraction it keeps) with the allowed min/max."
  },
  {
    "name": "delegated",
    "category": "Delegation",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every nomination a coldkey holds: (delegate, netuid, stake) per position."
  },
  {
    "name": "delegates",
    "category": "Delegation",
    "params": {},
    "defaults": {},
    "summary": "Every delegate hotkey on the network, with take and registrations."
  },
  {
    "name": "difficulty",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Current PoW registration difficulty for a subnet."
  },
  {
    "name": "epoch_status",
    "category": "Epochs & timing",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Where a subnet is in its epoch cycle, in one block-pinned read."
  },
  {
    "name": "existential_deposit",
    "category": "Accounts & keys",
    "params": {},
    "defaults": {},
    "summary": "Minimum balance an account must keep to stay alive."
  },
  {
    "name": "hotkey_conviction",
    "category": "Locks & conviction",
    "params": {
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Conviction metrics for a hotkey on a subnet."
  },
  {
    "name": "hotkey_identities",
    "category": "Identity & commitments",
    "params": {
      "hotkey_ss58s": "array"
    },
    "defaults": {},
    "summary": "On-chain identities for hotkeys (via each hotkey's owner coldkey), keyed by hotkey."
  },
  {
    "name": "hotkey_owner",
    "category": "Neurons & registration",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "The coldkey that owns a hotkey, or None if unowned."
  },
  {
    "name": "identity",
    "category": "Identity & commitments",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "The on-chain identity of a coldkey (name, links, description), or None."
  },
  {
    "name": "immunity_period",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Blocks a newly registered neuron is immune from deregistration."
  },
  {
    "name": "is_delegate",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Whether a hotkey is a delegate."
  },
  {
    "name": "is_fast_blocks",
    "category": "Chain",
    "params": {},
    "defaults": {},
    "summary": "Whether the chain runs fast blocks (0.25s slots, local/e2e testing mode)."
  },
  {
    "name": "lease",
    "category": "Leases & crowdloans",
    "params": {
      "lease_id": "integer"
    },
    "defaults": {},
    "summary": "A subnet lease by id (beneficiary, emissions share, end block, netuid, cost), or None."
  },
  {
    "name": "leases",
    "category": "Leases & crowdloans",
    "params": {},
    "defaults": {},
    "summary": "Every subnet lease on the network."
  },
  {
    "name": "locks_for_coldkey",
    "category": "Locks & conviction",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every lock a coldkey holds (one per subnet), rolled forward to now."
  },
  {
    "name": "locks_for_hotkey",
    "category": "Locks & conviction",
    "params": {
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Every coldkey's lock targeting a hotkey on a subnet, rolled forward to now."
  },
  {
    "name": "max_weight_limit",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Normalized-fraction cap on any single weight: after the chain normalizes the submitted vector, no weight may exceed max_weight_limit/65535 of the total. Not a raw u16 ceiling per weight."
  },
  {
    "name": "mechanism_count",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Current mechanism count configured for a subnet."
  },
  {
    "name": "mechanism_emission_split",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Emission split between mechanisms on a subnet."
  },
  {
    "name": "metagraph",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "The full metagraph for a subnet in one call (stakes, ranks, emissions, axons, ...)."
  },
  {
    "name": "mev_shield_next_key",
    "category": "Chain",
    "params": {},
    "defaults": {},
    "summary": "The MEV Shield ML-KEM-768 public key (0x-hex) used to encrypt shielded txs, or None."
  },
  {
    "name": "min_allowed_weights",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Minimum number of weights a validator must set on a subnet. A pure self-weight (a single entry pointing at the caller's own uid) bypasses the minimum, and the subnet owner bypasses validator-permit rules."
  },
  {
    "name": "miner_collateral",
    "category": "Mining & collateral",
    "params": {
      "netuid": "integer",
      "hotkey_ss58": "string",
      "coldkey_ss58": "string"
    },
    "defaults": {
      "coldkey_ss58": null
    },
    "summary": "A `(hotkey, coldkey)` stake position's standing collateral, or None."
  },
  {
    "name": "most_convicted_hotkey",
    "category": "Locks & conviction",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Hotkey with the highest conviction on a subnet, if any."
  },
  {
    "name": "multisig",
    "category": "Accounts & keys",
    "params": {
      "account_ss58": "string",
      "call_hash": "string"
    },
    "defaults": {},
    "summary": "A pending multisig operation (opening timepoint, approvals, depositor), or None."
  },
  {
    "name": "netuids_for_hotkey",
    "category": "Neurons & registration",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every subnet a hotkey is registered on, as a sorted list of netuids."
  },
  {
    "name": "neurons",
    "category": "Neurons & registration",
    "params": {
      "netuid": "integer",
      "lite": "boolean"
    },
    "defaults": {
      "lite": true
    },
    "summary": "Every neuron on a subnet in ONE runtime-API call (the metagraph fast path)."
  },
  {
    "name": "next_epoch_start_block",
    "category": "Epochs & timing",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Block at which the subnet's next epoch is expected to fire, or None if tempo is 0."
  },
  {
    "name": "owned_hotkeys",
    "category": "Neurons & registration",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every hotkey owned by a coldkey (the reverse of `hotkey_owner`)."
  },
  {
    "name": "parents",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Parent hotkeys of a child on a subnet, as (proportion, parent_ss58) pairs."
  },
  {
    "name": "pending_children",
    "category": "Delegation",
    "params": {
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Proposed child hotkeys of a parent still in cooldown, and when they apply."
  },
  {
    "name": "proxies",
    "category": "Accounts & keys",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Proxy delegations of an account: who may sign on its behalf, and the reserved deposit."
  },
  {
    "name": "quote_stake",
    "category": "Prices & swaps",
    "params": {
      "netuid": "integer",
      "amount_tao": "number"
    },
    "defaults": {},
    "summary": "Simulate staking ``amount_tao`` TAO into a subnet: alpha out, fee, and slippage."
  },
  {
    "name": "quote_unstake",
    "category": "Prices & swaps",
    "params": {
      "netuid": "integer",
      "amount_alpha": "number"
    },
    "defaults": {},
    "summary": "Simulate unstaking ``amount_alpha`` alpha from a subnet: TAO out, fee, and slippage."
  },
  {
    "name": "reveal_period",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Commit-reveal reveal window, in epochs, for a subnet."
  },
  {
    "name": "revealed_commitment",
    "category": "Identity & commitments",
    "params": {
      "netuid": "integer",
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "The revealed (timelock-decrypted) commitment for a hotkey on a subnet, or None."
  },
  {
    "name": "root_basket_owed",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Total TAO a coldkey would realize by claiming its root dividends now."
  },
  {
    "name": "root_basket_owed_breakdown",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A coldkey's pending root dividends, itemized per validator hotkey."
  },
  {
    "name": "root_basket_portfolio",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A coldkey's basket portfolio: beta tokens held per validator, with values."
  },
  {
    "name": "root_basket_total_nav",
    "category": "Staking",
    "params": {},
    "defaults": {},
    "summary": "Network-wide total basket NAV across all validators, in TAO."
  },
  {
    "name": "root_baskets",
    "category": "Staking",
    "params": {},
    "defaults": {},
    "summary": "Basket summaries for every validator with an active basket."
  },
  {
    "name": "root_claim_threshold",
    "category": "Staking",
    "params": {},
    "defaults": {},
    "summary": "The minimum TAO payout for a root dividend claim."
  },
  {
    "name": "stake",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string",
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Alpha staked by a coldkey to a hotkey on a subnet (TAO when netuid is 0)."
  },
  {
    "name": "stake_availability",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Free vs locked stake for a coldkey on one subnet."
  },
  {
    "name": "stake_availability_for_coldkey",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string",
      "netuids": "array"
    },
    "defaults": {},
    "summary": "Free vs locked stake for a coldkey across many subnets (one runtime call)."
  },
  {
    "name": "stake_for_coldkey",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every stake position held by a coldkey, across all hotkeys and subnets."
  },
  {
    "name": "stake_for_coldkeys",
    "category": "Staking",
    "params": {
      "coldkey_ss58s": "array"
    },
    "defaults": {},
    "summary": "Every stake position for several coldkeys at once, in one runtime call."
  },
  {
    "name": "stake_value_for_coldkey",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A coldkey's stake marked to TAO at spot prices (excludes slippage/fees), block-pinned."
  },
  {
    "name": "stake_value_for_coldkeys",
    "category": "Staking",
    "params": {
      "coldkey_ss58s": "array"
    },
    "defaults": {},
    "summary": "Spot-price stake valuation for several coldkeys at once, at one block."
  },
  {
    "name": "staking_hotkeys",
    "category": "Staking",
    "params": {
      "coldkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Every hotkey associated with a coldkey's staking or root-basket state."
  },
  {
    "name": "subnet",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Tempo, burn, and neuron count for one subnet (the three reads run concurrently)."
  },
  {
    "name": "subnet_collateral",
    "category": "Mining & collateral",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Every `(hotkey, coldkey)` position with standing collateral on a subnet."
  },
  {
    "name": "subnet_convictions",
    "category": "Locks & conviction",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Every hotkey with locked stake on a subnet, rolled forward to now."
  },
  {
    "name": "subnet_emission_enabled",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Root-controlled pool-side TAO emission flag for a subnet (1 = enabled). New registrations start enabled. When 0, the subnet earns no TAO emission share even while subnet_is_active is true; only root can flip it (see the set_subnet_emission_enabled intent)."
  },
  {
    "name": "subnet_emission_weights",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "The subnet's emission weight vector ``[burn, recycle, owner, validator,"
  },
  {
    "name": "subnet_hyperparameters",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "All hyperparameters for a subnet, as a flat name -> raw value mapping."
  },
  {
    "name": "subnet_identity",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "The identity metadata of a subnet, or None."
  },
  {
    "name": "subnet_names",
    "category": "Subnets",
    "params": {},
    "defaults": {},
    "summary": "Registered name of every subnet that published an identity, keyed by netuid."
  },
  {
    "name": "subnet_registration_cost",
    "category": "Subnets",
    "params": {},
    "defaults": {},
    "summary": "Current cost to register a new subnet."
  },
  {
    "name": "subnet_start_schedule",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "When a registered subnet can call ``start_call``."
  },
  {
    "name": "subnet_state",
    "category": "Subnets",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Reporting lifecycle for one subnet, or None after cleanup/nonexistence."
  },
  {
    "name": "subnet_states",
    "category": "Subnets",
    "params": {},
    "defaults": {},
    "summary": "Reporting lifecycle for every active or dissolving subnet."
  },
  {
    "name": "subnet_tao_flows",
    "category": "Subnets",
    "params": {},
    "defaults": {},
    "summary": "Per-subnet EMA of net TAO flow, in rao per block."
  },
  {
    "name": "subnets",
    "category": "Subnets",
    "params": {},
    "defaults": {},
    "summary": "Info for every subnet, fetched in four batched map queries rather than"
  },
  {
    "name": "timelocked_weight_commits",
    "category": "Weights & bonds",
    "params": {
      "netuid": "integer",
      "mechid": "integer"
    },
    "defaults": {
      "mechid": 0
    },
    "summary": "Pending (still-encrypted) commit-reveal weight commits, grouped by epoch."
  },
  {
    "name": "timestamp",
    "category": "Chain",
    "params": {},
    "defaults": {},
    "summary": "Current chain time (from the Timestamp pallet) and the block it was read at."
  },
  {
    "name": "token_symbols",
    "category": "Subnets",
    "params": {},
    "defaults": {},
    "summary": "Chain-registered token symbol of every subnet, keyed by netuid."
  },
  {
    "name": "total_alpha_staked",
    "category": "Staking",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Total alpha currently staked on a subnet, across all hotkeys (TAO when netuid is 0)."
  },
  {
    "name": "tx_rate_limit",
    "category": "Chain",
    "params": {},
    "defaults": {},
    "summary": "Global transaction rate limit in blocks."
  },
  {
    "name": "uid",
    "category": "Neurons & registration",
    "params": {
      "hotkey_ss58": "string",
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "UID of a hotkey on a subnet, or None if not registered there."
  },
  {
    "name": "validator_basket",
    "category": "Staking",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A validator's basket holdings: per subnet, the alpha held and its TAO value."
  },
  {
    "name": "validator_basket_nav",
    "category": "Staking",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "A validator's basket net asset value in TAO (realizable quote)."
  },
  {
    "name": "validator_basket_summary",
    "category": "Staking",
    "params": {
      "hotkey_ss58": "string"
    },
    "defaults": {},
    "summary": "Everything about one validator's basket, in a single call."
  },
  {
    "name": "weights",
    "category": "Weights & bonds",
    "params": {
      "netuid": "integer",
      "mechid": "integer"
    },
    "defaults": {
      "mechid": 0
    },
    "summary": "Validator weight rows as ``{validator_uid: {miner_uid: fraction}}``, each row summing to 1."
  },
  {
    "name": "weights_rate_limit",
    "category": "Hyperparameters",
    "params": {
      "netuid": "integer"
    },
    "defaults": {},
    "summary": "Blocks a hotkey must wait between weight sets on a subnet."
  }
] as const

export type ReadAddress = string | { ss58Address?: string; ss58_address?: string; address?: string; coldkeypub?: ReadAddress | Promise<ReadAddress>; hotkeypub?: ReadAddress | Promise<ReadAddress> }

export interface ReadParams {
  accepts_locked_alpha: { coldkey_ss58: ReadAddress }
  alpha_price: { netuid: number }
  alpha_prices: {  }
  associated_evm_key: { netuid: number; uid: number }
  auto_stake: { coldkey_ss58: ReadAddress; netuid: number }
  auto_stake_all: { coldkey_ss58: ReadAddress }
  balance: { coldkey_ss58: ReadAddress }
  balances: { coldkey_ss58s: ReadAddress[] }
  basket_position: { hotkey_ss58: ReadAddress; coldkey_ss58: ReadAddress }
  basket_trading_status: { hotkey_ss58: ReadAddress }
  block_info: { block: number }
  block_time: {  }
  blocks_since_last_step: { netuid: number }
  blocks_since_last_update: { netuid: number; uid: number }
  blocks_until_next_epoch: { netuid: number }
  bonds: { netuid: number; mechid?: number }
  burn: { netuid: number }
  childkey_threshold_suspended: { hotkey_ss58: ReadAddress }
  children: { hotkey_ss58: ReadAddress; netuid: number }
  coldkey_lock: { coldkey_ss58: ReadAddress; netuid: number }
  coldkey_swap_announcement: { coldkey_ss58: ReadAddress }
  collateral_policy: { netuid: number }
  commit_reveal_enabled: { netuid: number }
  commitment: { netuid: number; hotkey_ss58: ReadAddress }
  commitments: { netuid: number }
  crowdloan: { crowdloan_id: number }
  crowdloan_contributors: { crowdloan_id: number }
  crowdloans: {  }
  delegate: { hotkey_ss58: ReadAddress }
  delegate_take: { hotkey_ss58: ReadAddress }
  delegated: { coldkey_ss58: ReadAddress }
  delegates: {  }
  difficulty: { netuid: number }
  epoch_status: { netuid: number }
  existential_deposit: {  }
  hotkey_conviction: { hotkey_ss58: ReadAddress; netuid: number }
  hotkey_identities: { hotkey_ss58s: ReadAddress[] }
  hotkey_owner: { hotkey_ss58: ReadAddress }
  identity: { coldkey_ss58: ReadAddress }
  immunity_period: { netuid: number }
  is_delegate: { hotkey_ss58: ReadAddress }
  is_fast_blocks: {  }
  lease: { lease_id: number }
  leases: {  }
  locks_for_coldkey: { coldkey_ss58: ReadAddress }
  locks_for_hotkey: { hotkey_ss58: ReadAddress; netuid: number }
  max_weight_limit: { netuid: number }
  mechanism_count: { netuid: number }
  mechanism_emission_split: { netuid: number }
  metagraph: { netuid: number }
  mev_shield_next_key: {  }
  min_allowed_weights: { netuid: number }
  miner_collateral: { netuid: number; hotkey_ss58: ReadAddress; coldkey_ss58?: ReadAddress | null }
  most_convicted_hotkey: { netuid: number }
  multisig: { account_ss58: ReadAddress; call_hash: string }
  netuids_for_hotkey: { hotkey_ss58: ReadAddress }
  neurons: { netuid: number; lite?: boolean }
  next_epoch_start_block: { netuid: number }
  owned_hotkeys: { coldkey_ss58: ReadAddress }
  parents: { hotkey_ss58: ReadAddress; netuid: number }
  pending_children: { hotkey_ss58: ReadAddress; netuid: number }
  proxies: { coldkey_ss58: ReadAddress }
  quote_stake: { netuid: number; amount_tao: number | string }
  quote_unstake: { netuid: number; amount_alpha: number | string }
  reveal_period: { netuid: number }
  revealed_commitment: { netuid: number; hotkey_ss58: ReadAddress }
  root_basket_owed: { coldkey_ss58: ReadAddress }
  root_basket_owed_breakdown: { coldkey_ss58: ReadAddress }
  root_basket_portfolio: { coldkey_ss58: ReadAddress }
  root_basket_total_nav: {  }
  root_baskets: {  }
  root_claim_threshold: {  }
  stake: { coldkey_ss58: ReadAddress; hotkey_ss58: ReadAddress; netuid: number }
  stake_availability: { coldkey_ss58: ReadAddress; netuid: number }
  stake_availability_for_coldkey: { coldkey_ss58: ReadAddress; netuids: number[] }
  stake_for_coldkey: { coldkey_ss58: ReadAddress }
  stake_for_coldkeys: { coldkey_ss58s: ReadAddress[] }
  stake_value_for_coldkey: { coldkey_ss58: ReadAddress }
  stake_value_for_coldkeys: { coldkey_ss58s: ReadAddress[] }
  staking_hotkeys: { coldkey_ss58: ReadAddress }
  subnet: { netuid: number }
  subnet_collateral: { netuid: number }
  subnet_convictions: { netuid: number }
  subnet_emission_enabled: { netuid: number }
  subnet_emission_weights: { netuid: number }
  subnet_hyperparameters: { netuid: number }
  subnet_identity: { netuid: number }
  subnet_names: {  }
  subnet_registration_cost: {  }
  subnet_start_schedule: { netuid: number }
  subnet_state: { netuid: number }
  subnet_states: {  }
  subnet_tao_flows: {  }
  subnets: {  }
  timelocked_weight_commits: { netuid: number; mechid?: number }
  timestamp: {  }
  token_symbols: {  }
  total_alpha_staked: { netuid: number }
  tx_rate_limit: {  }
  uid: { hotkey_ss58: ReadAddress; netuid: number }
  validator_basket: { hotkey_ss58: ReadAddress }
  validator_basket_nav: { hotkey_ss58: ReadAddress }
  validator_basket_summary: { hotkey_ss58: ReadAddress }
  weights: { netuid: number; mechid?: number }
  weights_rate_limit: { netuid: number }
}
export type ReadName = keyof ReadParams
