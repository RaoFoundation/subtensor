# Rules for exposing state and runtime constants

This file lists concrete state variables, maps, and runtime constants and
classifies them as one of three classes:

1. Safe to expose directly, as is, or 
2. Need some type-safe wrapping, or
3. Internal, do not need to be exposed, or already known to be deprecated soon

The class 1 items are not anticipated to change significantly. Even if they do,
their exposed values should remain honestly reproducible with no greater than
O(1) complexity.

The class 2 items are temporary, use unstable internal representations, or
express complex formulas and need to be safely wrapped.

## Runtime constants

Inventory every public runtime constant declared by or supplied to the
configuration of an in-scope pallet. Expose it directly or through a coherent
typed grouped view, reading the authoritative runtime source rather than
copying its literal value into the precompile.

Preserve semantic types and units when converting Rust values to Solidity.
Treat fixed-point values, balances, block numbers, bounded sizes, and other
representation-specific constants as type-safe wrapping cases when their Rust
representation is not a suitable permanent ABI.

This requirement covers deterministic client-facing runtime configuration. It
does not cover generated weights, compiler/build constants, or private
implementation details that are not part of the pallet's public behavior.

## Safe to expose directly

### Pallet subtensor

- Delegation and childkeys: Delegates, ChildkeyTake, PendingChildKeys, ChildKeys, ParentKeys, PendingChildKeyCooldown, minimum/maximum delegate and childkey takes, and MinChildkeyTakePerSubnet.

- Ownership and account relationships: OwnedHotkeys, AutoStakeDestination, AutoStakeDestinationColdkeys, HotkeySuccessor, HotkeyRoot, ColdkeySuccessor, ColdkeyRoot, coldkey-swap announcements/disputes/delays, and LastHotkeySwapOnNetuid. Owner is only indirectly available when the caller already knows a subnet UID, so arbitrary hotkey ownership is only partially covered.

- Subnet identity and configuration: TokenSymbol, SubnetOwner, SubnetOwnerHotkey, Tempo, RecycleOrBurn, BondsPenalty, MaxAllowedUids, MaxAllowedValidators, AdjustmentInterval, TargetRegistrationsPerInterval, OwnerCutEnabled, ImmuneOwnerUidsLimit, MechanismCountCurrent, MechanismEmissionSplit, BurnHalfLife, BurnIncreaseMult, TransferToggle, MinAllowedUids, MinNonImmuneUids, and numerous global network limits.

- Emission and economic accounting: BlockEmission, Subtensor TotalIssuance, TotalStake, AlphaDividendsPerSubnet, RootAlphaDividendsPerSubnet, LastHotkeyEmissionOnNetuid, SubnetMovingAlpha, RootProp, SubnetEmissionEnabled, SubnetExcessTao, SubnetRootSellTao, SubnetProtocolAlpha, flow/EMA maps, emission gate configuration, pending emission/cut maps, MinerBurned, and RAORecycledForRegistration.

- Neuron state: Uids, IsNetworkMember, Weights, Bonds, BlockAtRegistration, NeuronCertificates, Prometheus, IdentitiesV2, SubnetIdentitiesV3, LoadedEmission, transaction-rate timestamps, and all weight-commit maps and versions.

- Collateral and leasing: MinerCollateral, ColdkeyMinerCollateral, ColdkeyCollateralHotkeys, CollateralLockShare, CollateralDrainRatio, NextSubnetLeaseId, and AccumulatedLeaseDividends.

- EVM associations: Forward view for AssociatedEvmAddress(netuid, uid).

### Pallet balances

TotalIssuance

### Pallet Proxy

proxy deposit, Announcements, LastCallResult, RealPaysFee

### Pallet Swap

FeeRate, SwapBalancer, BalancerTaoReservoir, BalancerAlphaReservoir, HasMigrationRun

## Need some type-safe wrapping

### Pallet subtensor

- StakingHotkeys: expose through an offset-based typed view that reads the existing vector once and
  slices it in memory. The response is capped, while the accepted full-vector decode is recorded in
  [Reviewed exceptions](exceptions.md).

### Pallet Swap

PalSwapInitialized and its successors should be exposed as just generic "IsSwapInitialized", non-specific to palswap / balancer.

## Do not expose

### Pallet subtensor

- Root claims: RootClaimableThreshold, RootClaimable, RootClaimed, RootClaimType.

### Pallet balances

InactiveIssuance, the reserved, frozen, and flags portions of Account: Locks, Reserves, Holds, Freezes

### Pallet swap 

ScrapReservoirAlpha
