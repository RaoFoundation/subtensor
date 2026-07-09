# Subtensor EVM Precompiles

This document describes all precompiles registered in
[`precompiles/src/lib.rs`](../precompiles/src/lib.rs).

Precompile addresses are derived from a numeric index with:

```text
address = H160::from_low_u64_be(index)
```

For example, index `2048` is `0x0000000000000000000000000000000000000800`.

> [!NOTE-1]
> Solidity interface files and ABIs for Subtensor-specific precompiles live under
> [`precompiles/src/solidity/`](../precompiles/src/solidity/).

> [!NOTE-2]
> For some known precompile contracts, the addresses differ from the Ethereum standard. For example, Blake2F. The contract developers should check the table to get correct address.
---

## Address Overview

| Index | Address | Category | Precompile |
|------:|---------|----------|------------|
| 1 | `0x…0001` | Ethereum | ECRecover |
| 2 | `0x…0002` | Ethereum | SHA-256 |
| 3 | `0x…0003` | Ethereum | RIPEMD-160 |
| 4 | `0x…0004` | Ethereum | Identity |
| 5 | `0x…0005` | Ethereum | ModExp |
| 6 | `0x…0006` | Frontier | Dispatch |
| 7 | `0x…0007` | Ethereum | Bn128Mul |
| 8 | `0x…0008` | Ethereum | Bn128Pairing |
| 9 | `0x…0009` | Ethereum | Bn128Add |
| 10 | `0x…000a` | Ethereum (Cancun) | PointEvaluation (EIP-4844) |
| 1024 | `0x…0400` | Frontier | SHA3-FIPS256 |
| 1025 | `0x…0401` | Frontier | ECRecoverPublicKey |
| 1026 | `0x…0402` | Crypto | Ed25519Verify |
| 1027 | `0x…0403` | Crypto | Sr25519Verify |
| 1028 | `0x…0404` | Ethereum | Blake2F |
| 2048 | `0x…0800` | Subtensor | BalanceTransfer |
| 2049 | `0x…0801` | Subtensor | Staking (v1) |
| 2050 | `0x…0802` | Subtensor | Metagraph |
| 2051 | `0x…0803` | Subtensor | Subnet |
| 2052 | `0x…0804` | Subtensor | Neuron |
| 2053 | `0x…0805` | Subtensor | Staking (v2) |
| 2054 | `0x…0806` | Subtensor | UidLookup |
| 2055 | `0x…0807` | Subtensor | StorageQuery |
| 2056 | `0x…0808` | Subtensor | Alpha |
| 2057 | `0x…0809` | Subtensor | Crowdloan |
| 2058 | `0x…080a` | Subtensor | Leasing |
| 2059 | `0x…080b` | Subtensor | Proxy |
| 2060 | `0x…080c` | Subtensor | AddressMapping |
| 2061 | `0x…080d` | Subtensor | VotingPower |

---

## Ethereum Precompiles

These are standard EVM built-ins from Frontier / Ethereum.

### 1. ECRecover — `0x…0001`

Recovers an Ethereum address from a secp256k1 signature.

| Item | Value |
|------|-------|
| Input | 128 bytes: `hash (32) \|\| v (32) \|\| r (32) \|\| s (32)` |
| Output | 32-byte left-padded address |

### 2. SHA-256 — `0x…0002`

Computes SHA-256 of arbitrary input.

| Item | Value |
|------|-------|
| Input | any length |
| Output | 32-byte digest |

### 3. RIPEMD-160 — `0x…0003`

Computes RIPEMD-160 of arbitrary input.

| Item | Value |
|------|-------|
| Input | any length |
| Output | 32-byte left-padded digest |

### 4. Identity — `0x…0004`

Returns input unchanged (data copy precompile).

| Item | Value |
|------|-------|
| Input | any length |
| Output | same as input |

### 5. ModExp — `0x…0005`

Modular exponentiation (`EIP-198`).

| Item | Value |
|------|-------|
| Input | length-prefixed base / exponent / modulus |
| Output | modular exponentiation result |

### 6. Dispatch — `0x…0006`

Frontier-specific precompile that dispatches a Substrate runtime call from EVM.

| Item | Value |
|------|-------|
| Input | SCALE-encoded runtime call |
| Output | empty on success |

> [!IMPORTANT]
> In this runtime, address `0x06` is **Dispatch**, not `Bn128Add`. Ethereum’s
> usual bn128 layout is remapped below.

### 7. Bn128Mul — `0x…0007`

Elliptic-curve scalar multiplication on alt_bn128 (`EIP-196`).

### 8. Bn128Pairing — `0x…0008`

Elliptic-curve pairing check on alt_bn128 (`EIP-197`).

### 9. Bn128Add — `0x…0009`

Elliptic-curve point addition on alt_bn128 (`EIP-196`).

### 10. PointEvaluation — `0x…000a`

KZG point evaluation precompile from **EIP-4844** (Cancun).

| Item | Value |
|------|-------|
| Gas | `50_000` |
| Input | exactly **192 bytes**: `versioned_hash (32) \|\| z (32) \|\| y (32) \|\| commitment (48) \|\| proof (48)` |
| Output on success | `FIELD_ELEMENTS_PER_BLOB (4096)` \|\| `BLS_MODULUS` (64 bytes total) |

Source: [`precompiles/src/point_evaluation.rs`](../precompiles/src/point_evaluation.rs)

---

## Frontier / Crypto Precompiles

### 1024. SHA3-FIPS256 — `0x…0400`

FIPS-compatible SHA3-256 digest.

### 1025. ECRecoverPublicKey — `0x…0401`

Like ECRecover, but returns the recovered public key instead of the address.

### 1026. Ed25519Verify — `0x…0402`

Verifies an Ed25519 signature.

| Item | Value |
|------|-------|
| Index | `1026` |
| Gas base | `6000` |
| Input layout | `0x00..0x04` padding / selector region + `msg (32)` + `pubkey (32)` + `signature (64)` (minimum 132 bytes) |
| Output | 32 bytes; `1` in the last byte if valid, otherwise `0` |

Solidity reference: [`ed25519Verify.sol`](../precompiles/src/solidity/ed25519Verify.sol)

### 1027. Sr25519Verify — `0x…0403`

Verifies an sr25519 signature (Substrate native).

| Item | Value |
|------|-------|
| Index | `1027` |
| Gas base | `6000` |
| Input layout | same shape as Ed25519Verify |
| Output | 32 bytes; `1` in the last byte if valid, otherwise `0` |

Solidity reference: [`sr25519Verify.sol`](../precompiles/src/solidity/sr25519Verify.sol)

### 1028. Blake2F — `0x…0404`

BLAKE2 compression function (`EIP-152`).

---

## Subtensor Precompiles

Most Subtensor-specific precompiles:

- use Solidity ABI selectors generated by `precompile-utils`
- map the EVM caller through `HashedAddressMapping` before pallet dispatch
- may require admin enablement via `PrecompileEnum` (except `StorageQuery` and a few always-on cases)

`bytes32` arguments typically represent Substrate `AccountId32` values
(hotkeys / coldkeys).

---

### 2048. BalanceTransfer — `0x…0800`

Transfer the call’s `msg.value` (in TAO, after EVM↔substrate balance conversion) to a Substrate account.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `transfer(bytes32)` | yes | Transfer TAO to the given `AccountId32` |

Solidity: [`balanceTransfer.sol`](../precompiles/src/solidity/balanceTransfer.sol)

---

### 2049. Staking (v1) — `0x…0801`

Legacy staking interface.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `addStake(bytes32,uint256)` | yes | Add stake to hotkey on `netuid` (value from `msg.value`) |
| `removeStake(bytes32,uint256,uint256)` | yes | Remove stake amount from hotkey/netuid |
| `getTotalColdkeyStake(bytes32)` | no | Total coldkey stake |
| `getTotalHotkeyStake(bytes32)` | no | Total hotkey stake |
| `getStake(bytes32,bytes32,uint256)` | no | Stake for hotkey/coldkey/netuid |
| `addProxy(bytes32)` | yes | Add staking proxy |
| `removeProxy(bytes32)` | yes | Remove staking proxy |

Solidity: [`staking.sol`](../precompiles/src/solidity/staking.sol)

---

### 2050. Metagraph — `0x…0802`

Read-only metagraph / neuron metrics.

| Method | Description |
|--------|-------------|
| `getUidCount(uint16)` | Number of UIDs on subnet |
| `getStake(uint16,uint16)` | Stake at UID |
| `getRank(uint16,uint16)` | Rank |
| `getTrust(uint16,uint16)` | Trust |
| `getConsensus(uint16,uint16)` | Consensus |
| `getIncentive(uint16,uint16)` | Incentive |
| `getDividends(uint16,uint16)` | Dividends |
| `getEmission(uint16,uint16)` | Emission |
| `getVtrust(uint16,uint16)` | Validator trust |
| `getValidatorStatus(uint16,uint16)` | Whether UID is a validator |
| `getLastUpdate(uint16,uint16)` | Last update block |
| `getIsActive(uint16,uint16)` | Active status |
| `getAxon(uint16,uint16)` | Axon endpoint info |
| `getHotkey(uint16,uint16)` | Hotkey for UID |
| `getColdkey(uint16,uint16)` | Coldkey for UID |

Solidity: [`metagraph.sol`](../precompiles/src/solidity/metagraph.sol)

---

### 2051. Subnet — `0x…0803`

Subnet registration and owner hyperparameter get/set.

#### Network registration

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `registerNetwork(bytes32)` | yes | Register network with hotkey |
| `registerNetwork(bytes32,string,string,string,string,string,string,string)` | yes | Register with identity fields |
| `getNetworkRegistrationBlock(uint16)` | no | Block when network was registered |

#### Common hyperparameters

Methods follow `getX(uint16)` / `setX(uint16, …)` patterns for:

- serving rate limit
- min / max difficulty
- weights version key & set rate limit
- adjustment alpha
- max weight limit
- immunity period
- min allowed weights
- kappa / rho / alpha sigmoid steepness
- activity cutoff & factor
- network registration / PoW registration allowed
- min / max burn
- difficulty
- bonds moving average
- commit-reveal weights enabled & interval
- liquid alpha / Yuma3 / bonds reset toggles
- alpha values
- toggle transfers

Solidity: [`subnet.sol`](../precompiles/src/solidity/subnet.sol)

---

### 2052. Neuron — `0x…0804`

Neuron registration, weights, and serving endpoints.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `setWeights(uint16,uint16[],uint16[],uint64)` | yes | Set weights |
| `commitWeights(uint16,bytes32)` | yes | Commit weights hash |
| `revealWeights(uint16,uint16[],uint16[],uint16[],uint64)` | yes | Reveal committed weights |
| `burnedRegister(uint16,bytes32)` | yes | Burned registration |
| `registerLimit(uint16,bytes32,uint64)` | yes | Registration with limit |
| `serveAxon(uint16,uint32,uint128,uint16,uint8,uint8,uint8,uint8)` | yes | Publish axon endpoint |
| TLS axon serve overload | yes | Publish axon with TLS certificate |
| `servePrometheus(uint16,uint32,uint128,uint16,uint8)` | yes | Publish Prometheus endpoint |

Solidity: [`neuron.sol`](../precompiles/src/solidity/neuron.sol)

---

### 2053. Staking (v2) — `0x…0805`

Current staking interface with limits, alpha burn, allowances, and stake movement.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `addStake(bytes32,uint256,uint256)` | yes | Add stake amount on netuid |
| `removeStake(bytes32,uint256,uint256)` | yes | Remove stake amount |
| `removeStakeFull(bytes32,uint256)` | yes | Fully remove stake |
| `removeStakeFullLimit(bytes32,uint256,uint256)` | yes | Fully remove with price limit |
| `moveStake(bytes32,bytes32,uint256,uint256,uint256)` | yes | Move stake between hotkeys / nets |
| `transferStake(bytes32,bytes32,uint256,uint256,uint256)` | yes | Transfer stake between coldkeys |
| `burnAlpha(bytes32,uint256,uint256)` | yes | Burn alpha stake |
| `addStakeLimit(...)` | yes | Add stake with max price / allow partial |
| `removeStakeLimit(...)` | yes | Remove stake with min price / allow partial |
| `getTotalColdkeyStake(bytes32)` | no | Total coldkey stake |
| `getTotalHotkeyStake(bytes32)` | no | Total hotkey stake |
| `getStake(bytes32,bytes32,uint256)` | no | Stake lookup |
| `getAlphaStakedValidators(bytes32,uint256)` | no | Alpha-staked validators |
| `getTotalAlphaStaked(bytes32,uint256)` | no | Total alpha staked |
| `getNominatorMinRequiredStake()` | no | Minimum nominator stake |
| `getTotalColdkeyStakeOnSubnet(bytes32,uint256)` | no | Coldkey stake on subnet |
| `addProxy(bytes32)` / `removeProxy(bytes32)` | yes | Proxy management |
| `approve(address,uint256,uint256)` | yes | Approve stake allowance |
| `allowance(address,address,uint256)` | no | Read allowance |
| `increaseAllowance(...)` / `decreaseAllowance(...)` | yes | Adjust allowance |
| `transferStakeFrom(...)` | yes | Transfer stake using allowance |

Solidity: [`stakingV2.sol`](../precompiles/src/solidity/stakingV2.sol)

---

### 2054. UidLookup — `0x…0806`

Look up a subnet UID associated with an EVM address / associated key.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `uidLookup(uint16,address,uint16)` | no | Returns UID + association metadata for `netuid` |

Solidity: [`uidLookup.sol`](../precompiles/src/solidity/uidLookup.sol)

---

### 2055. StorageQuery — `0x…0807`

Raw Substrate storage reader for allowlisted pallet key prefixes.

| Item | Value |
|------|-------|
| Input | full storage key bytes (must start with an authorized 16-byte twox128 pallet prefix) |
| Output | raw SCALE storage value, or empty if missing |
| Allowlist | `SubtensorModule`, `Swap`, `Balances`, `Proxy`, `Scheduler`, `Drand`, `Crowdloan`, `Sudo`, `Multisig`, `Timestamp` |

> [!WARNING]
> Keys outside the allowlist are rejected with `"Invalid key"`.

Source: [`storage_query.rs`](../precompiles/src/storage_query.rs)

---

### 2056. Alpha — `0x…0808`

Subnet AMM / alpha economics views and swap simulations.

| Method | Description |
|--------|-------------|
| `getAlphaPrice(uint16)` | Current alpha price |
| `getMovingAlphaPrice(uint16)` | Moving alpha price |
| `getTaoInPool(uint16)` | TAO reserve |
| `getAlphaInPool(uint16)` | Alpha-in reserve |
| `getAlphaOutPool(uint16)` | Alpha-out |
| `getAlphaIssuance(uint16)` | Alpha issuance |
| `getTaoWeight()` | Global TAO weight |
| `getCKBurn()` | CK burn value |
| `simSwapTaoForAlpha(uint16,uint64)` | Simulate TAO → alpha |
| `simSwapAlphaForTao(uint16,uint64)` | Simulate alpha → TAO |
| `getSubnetMechanism(uint16)` | Mechanism id |
| `getRootNetuid()` | Root netuid |
| `getEMAPriceHalvingBlocks(uint16)` | EMA halving period |
| `getSubnetVolume(uint16)` | Subnet volume |
| `getTaoInEmission(uint16)` | TAO-in emission |
| `getAlphaInEmission(uint16)` | Alpha-in emission |
| `getAlphaOutEmission(uint16)` | Alpha-out emission |
| `getSumAlphaPrice()` | Sum of alpha prices |

Solidity: [`alpha.sol`](../precompiles/src/solidity/alpha.sol)

---

### 2057. Crowdloan — `0x…0809`

Crowdloan create / contribute / admin flows.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `getCrowdloan(uint32)` | no | Crowdloan details |
| `getContribution(uint32,bytes32)` | no | Contributor amount |
| `create(uint64,uint64,uint64,uint32,address)` | yes | Create crowdloan |
| `contribute(uint32,uint64)` | yes | Contribute |
| `withdraw(uint32)` | yes | Withdraw contribution |
| `finalize(uint32)` | yes | Finalize capped crowdloan |
| `refund(uint32)` | yes | Refund contributors |
| `dissolve(uint32)` | yes | Dissolve crowdloan |
| `updateMinContribution(uint32,uint64)` | yes | Update min contribution |
| `updateEnd(uint32,uint32)` | yes | Update end block |
| `updateCap(uint32,uint64)` | yes | Update cap |

Solidity: [`crowdloan.sol`](../precompiles/src/solidity/crowdloan.sol)

---

### 2058. Leasing — `0x…080a`

Subnet leasing via crowdloan.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `getLease(uint32)` | no | Lease details |
| `getContributorShare(uint32,bytes32)` | no | Contributor share |
| `getLeaseIdForSubnet(uint16)` | no | Lease id for subnet |
| `createLeaseCrowdloan(uint64,uint64,uint64,uint32,uint8,bool,uint32)` | yes | Create lease crowdloan |
| `terminateLease(uint32,bytes32)` | yes | Terminate ended lease |

Solidity: [`leasing.sol`](../precompiles/src/solidity/leasing.sol)

---

### 2059. Proxy — `0x…080b`

Subtensor proxy management and proxied calls from EVM.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `createPureProxy(uint8,uint32,uint16)` | yes | Create pure proxy |
| `killPureProxy(bytes32,uint8,uint16,uint32,uint32)` | yes | Kill pure proxy |
| `proxyCall(bytes32,uint8[],uint8[])` | yes | Execute call through proxy |
| `addProxy(bytes32,uint8,uint32)` | yes | Add proxy |
| `removeProxy(bytes32,uint8,uint32)` | yes | Remove proxy |
| `removeProxies()` | yes | Remove all proxies |
| `pokeDeposit()` | yes | Refresh proxy deposit |
| `getProxies(bytes32)` | no | List proxies for account |

Solidity: [`proxy.sol`](../precompiles/src/solidity/proxy.sol)

---

### 2060. AddressMapping — `0x…080c`

Maps an EVM `address` to its Substrate `AccountId32` under the runtime
`HashedAddressMapping`.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `addressMapping(address)` | no | Returns 32-byte Substrate account id |

Solidity: [`addressMapping.sol`](../precompiles/src/solidity/addressMapping.sol)

---

### 2061. VotingPower — `0x…080d`

Read voting-power tracking state for subnets.

| Method | Mutating | Description |
|--------|:--------:|-------------|
| `getVotingPower(uint16,bytes32)` | no | Voting power for account on subnet |
| `isVotingPowerTrackingEnabled(uint16)` | no | Whether tracking is enabled |
| `getVotingPowerDisableAtBlock(uint16)` | no | Disable-at block |
| `getVotingPowerEmaAlpha(uint16)` | no | EMA alpha parameter |
| `getTotalVotingPower(uint16)` | no | Total tracked voting power |

Source: [`voting_power.rs`](../precompiles/src/voting_power.rs)

---

## Notes for Integrators

1. **Address format**  
   Always call the 20-byte `H160` derived from the index (left-padded with zeros).

2. **ABI vs raw input**  
   - Ethereum / Frontier crypto precompiles generally use raw byte layouts.  
   - Subtensor precompiles use Solidity ABI (`precompile-utils` selectors).

3. **Account mapping**  
   EVM callers are mapped to Substrate accounts via `HashedAddressMapping`
   (`BlakeTwo256`). Use `AddressMapping` (`0x…080c`) to inspect the mapping.

4. **Decimals**  
   EVM balances are scaled relative to Substrate TAO (commonly ×10⁹ in this
   codebase). Prefer the Solidity helpers / existing ABIs when converting amounts.

5. **Enablement**  
   Many Subtensor precompiles go through `try_execute` and may be gated by
   `PrecompileEnum` admin settings. If a call reverts unexpectedly, check that
   the corresponding precompile feature is enabled on-chain.

---

## Source Map

| Precompile | Source |
|------------|--------|
| PointEvaluation | `precompiles/src/point_evaluation.rs` |
| Ed25519Verify | `precompiles/src/ed25519.rs` |
| Sr25519Verify | `precompiles/src/sr25519.rs` |
| BalanceTransfer | `precompiles/src/balance_transfer.rs` |
| Staking v1 / v2 | `precompiles/src/staking.rs` |
| Metagraph | `precompiles/src/metagraph.rs` |
| Subnet | `precompiles/src/subnet.rs` |
| Neuron | `precompiles/src/neuron.rs` |
| UidLookup | `precompiles/src/uid_lookup.rs` |
| StorageQuery | `precompiles/src/storage_query.rs` |
| Alpha | `precompiles/src/alpha.rs` |
| Crowdloan | `precompiles/src/crowdloan.rs` |
| Leasing | `precompiles/src/leasing.rs` |
| Proxy | `precompiles/src/proxy.rs` |
| AddressMapping | `precompiles/src/address_mapping.rs` |
| VotingPower | `precompiles/src/voting_power.rs` |
| Registry | `precompiles/src/lib.rs` |
