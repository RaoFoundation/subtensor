"use strict";

// One row per root-controlled chain-wide value, not per netuid. All balances
// are raw rao. Fixed-point values use their raw integer representation: this
// avoids converting decimals into consensus integers. Enum values are TOML
// strings. Composite calls are laid out under [group.call] and [group.storage].
const params = [
  ["default_take", "MaxDelegateTake", "defaultTake"],
  ["tx_rate_limit", "TxRateLimit", "txRateLimit"],
  ["network_rate_limit", "NetworkRateLimit", "rateLimit"],
  ["network_immunity_period", "NetworkImmunityPeriod", "immunityPeriod"],
  ["network_min_lock_cost", "NetworkMinLockCost", "lockCost"],
  ["subnet_limit", "SubnetLimit", "maxSubnets"],
  ["lock_reduction_interval", "NetworkLockReductionInterval", "interval"],
  ["stake_threshold", "StakeThreshold", "minStake"],
  ["tx_delegate_take_rate_limit", "TxDelegateTakeRateLimit", "txRateLimit"],
  ["min_delegate_take", "MinDelegateTake", "take"],
  ["dissolve_network_schedule_duration", "DissolveNetworkScheduleDuration", "duration"],
  ["evm_chain_id", "ChainId", "chainId", { pallet: "EVMChainId" }],
  ["subnet_moving_alpha", "SubnetMovingAlpha", "alpha"],
  ["commit_reveal_version", "CommitRevealWeightsVersion", "version"],
  ["ck_burn", "CKBurn", "burn"],
  ["admin_freeze_window", "AdminFreezeWindow", "window"],
  ["owner_hparam_rate_limit", "OwnerHyperparamRateLimit", "epochs"],
  ["tao_flow_cutoff", "TaoFlowCutoff", "flowCutoff"],
  ["tao_flow_normalization_exponent", "FlowNormExponent", "exponent"],
  ["emission_gate_exponent", "EmissionGateExponent", "exponent"],
  ["tao_flow_smoothing_factor", "FlowEmaSmoothingFactor", "smoothingFactor"],
  ["net_tao_flow_enabled", "NetTaoFlowEnabled", "enabled"],
  ["max_mechanism_count", "MaxMechanismCount", "maxMechanismCount"],
  ["start_call_delay", "StartCallDelay", "delay"],
  ["coldkey_swap_announcement_delay", "ColdkeySwapAnnouncementDelay", "duration"],
  ["coldkey_swap_reannouncement_delay", "ColdkeySwapReannouncementDelay", "duration"],
  ["basket_min_trade_tao", "BasketMinTradeTao", "minTradeRao"],
  ["basket_concentration_cap", "BasketConcentrationCap", "cap"],
  ["basket_trading_enabled", "BasketTradingEnabled", "enabled"],
  ["basket_daily_turnover_cap", "BasketDailyTurnoverCap", "cap"],
  ["basket_liquidity_cap", "BasketLiquidityCap", "cap"],
  ["hashed_accounts_enabled", "HashedAccountsEnabled", "enabled", { pallet: "AdminUtils", callPallet: "AdminUtils" }],
  ["max_epochs_per_block", "MaxEpochsPerBlock", "maxEpochsPerBlock"],
  ["gamma_enabled", "GammaEnabled", "enabled"],
  ["hashed_account_registration_deposit", "HashedAccountRegistrationDeposit", "value", { pallet: "HashedAccounts" }],
  ["basket_trade_max_slippage_bps", "BasketTradeMaxSlippageBps", "bps"],
  ["basket_trade_refill_blocks", "BasketTradeRefillBlocks", "blocks"],
  ["basket_trade_price_reference", "BasketTradePriceReferenceMode", "reference"],
  ["subnet_emission_weights_rate_limit", "SubnetEmissionWeightsRateLimit", "rateLimit"],
  ["pending_childkey_cooldown", "PendingChildKeyCooldown", "cooldown", { callPallet: "SubtensorModule", call: "set_pending_childkey_cooldown" }],
  ["tx_childkey_take_rate_limit", "TxChildkeyTakeRateLimit", "txRateLimit", { callPallet: "SubtensorModule" }],
  ["min_childkey_take", "MinChildkeyTake", "take", { callPallet: "SubtensorModule" }],
  ["max_childkey_take", "MaxChildkeyTake", "take", { callPallet: "SubtensorModule" }],
  ["nominator_min_required_stake_factor", "NominatorMinRequiredStake", "minStake", {
    call: "sudo_set_nominator_min_required_stake",
    reportOnly: "The setter can sweep the entire staking map; exclude it until cleanup is bounded and weighted.",
  }],
  ["emission_bar_quantile", "EmissionBarQuantile", "quantile"],
  ["emission_bar_rank", "EmissionBarRank", "rank"],
  ["tao_usd_price_oracle", "TaoUsdPriceOracle", "oracle", { optional: true }],
].map(([name, storage, argument, options = {}]) => ({
  name,
  pallet: options.pallet || "SubtensorModule",
  storage,
  callPallet: options.callPallet || "AdminUtils",
  call: options.call || `sudo_set_${name}`,
  argument: options.argument || argument,
  optional: options.optional || false,
  reportOnly: options.reportOnly || null,
}));

const groups = {
  basket_claim_dust: {
    callPallet: "AdminUtils",
    call: "sudo_set_basket_claim_dust",
    callArguments: ["rowCapRao", "rowBps", "sliceRao", "forfeitCapRao"],
    storage: {
      row_cap_rao: ["SubtensorModule", "BasketClaimRowDustCapTao"],
      row_bps: ["SubtensorModule", "BasketClaimRowDustBps"],
      slice_rao: ["SubtensorModule", "BasketClaimSliceDustTao"],
      forfeit_cap_rao: ["SubtensorModule", "BasketClaimForfeitCapTao"],
    },
  },
};

module.exports = { params, groups };
