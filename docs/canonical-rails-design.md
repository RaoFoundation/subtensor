# Canonical Rails: USD → TAO → Alpha Settlement Layer

**Status:** Draft for review
**Date:** 2026-08-21
**Scope:** Runtime pallets, EVM precompiles, external token contracts, bridge topology, CLI/SDK surface

---

## 1. Summary

Bittensor's subnet AMMs already clear roughly **$38M/day** of alpha trading volume. Meanwhile, every external product that tried to connect outside dollars to that demand has failed at the same point: distribution shipped, liquidity never did (TaoFi: ~$78K TVL; the official Solana TAO mint: ~$16K of pool depth; total stablecoins on Bittensor EVM: ~$500).

This document specifies the **canonical rails**: a chain-owned settlement layer that connects external dollars to TAO and alpha, such that:

1. Every dollar that enters becomes TAO buy pressure, then alpha buy pressure, by construction.
2. The chain provides first-class, permanent liquidity that no third party has ever supplied.
3. Alpha exposure is exported to other chains as yield-bearing tokens with keyless custody.
4. Subnet service revenue settles through the same route, making revenue → token demand mechanical and auditable.

The design principle throughout: **be the mint and the redemption window — never the storefront, never the wire.** The chain owns the token contracts, the swap venue, and the trust; it rents bridges as rate-limited couriers and leaves consumer UX to partners.

---

## 2. Topology

```text
        [Bittensor runtime — the hub]                      [Base — spoke #1]
        ┌─────────────────────────────────────┐            ┌──────────────────┐
  in →  │ USDC_bridged ⇄ tUSD  (PSM: caps,    │            │  USDC  (origin)  │
        │                       haircuts)     │            │                  │
        │ tUSD ⇄ TAO   (canonical pool,       │◄─Hyperlane─►  TAO (1:1)       │
        │               protocol-owned depth) │  validator │  wALPHA_i (share)│
        │ TAO ⇄ α_i    (existing subnet AMMs) │  quorum    │  wBETA_v (share) │
        │ α_i → shares (v441 basket engine)   │  ISM       │                  │
        └─────────────────────────────────────┘            └──────────────────┘
          registry · per-block supply attestation · rate limits · Gateway
```

- **Mint path (demand):** USDC on Base → bridge → PSM → tUSD → TAO → alpha/basket → share.
- **Redeem path (exit):** share → alpha → TAO → tUSD → USDC delivered back on Base.
- Both directions cross the chain-owned tUSD/TAO pool. There is no other door.

### 2.1 The narrow waist

Many things above (Base, later Solana/other chains, CEX market makers, ETP custodians), many things below (128+ subnets, validator baskets, a future index). One thin interface in the middle that everything must pass through. This gives:

- **Universality** — an integrator needs two function calls and a registry lookup.
- **The design law enforced by construction** — a symmetric, freely-arbitraged USD↔alpha market drains TAO (arbitrage after alpha buys runs USD→alpha→TAO→USD, selling TAO). Because the atomic entry point is the only way in, every USD→alpha path physically routes through the TAO pool.
- **Fee and burn capture at one chokepoint** every flow crosses.

---

## 3. Components

### 3.1 tUSD and the PSM (`pallets/usd-psm`, new)

tUSD is the runtime's internal dollar **unit of account**. It is deliberately **not a product**:

- **Non-transferable and non-holdable.** tUSD exists only inside atomic transactions (PSM → pool → stake). Users never hold a tUSD balance. There is no ERC-20 view of it and it never leaves the runtime.
- Why: a holdable tUSD would inevitably be paired against alpha tokens on the chain's own EVM, creating the symmetric USD↔alpha bypass internally — the exact structure this design exists to prevent. Non-holdable tUSD makes the one-door invariant airtight. It also means the chain has no on-chain dollar parking lot: de-risking means leaving (exit to USDC on Base), not sitting in stables inside the ecosystem.

The **PSM** (Peg Stability Module, after Maker's pattern) is the front door for dollars:

- A governance-managed registry of accepted USD assets (bridged USDC variants; Circle-native USDC via CCTP when available).
- Per-asset **cap** (maximum exposure to that bridge/issuer) and **haircut** (conversion discount for less-trusted assets).
- Converts accepted assets 1:1 (minus haircut) into internal tUSD; holds reserves 1:1. Reserves are never rehypothecated into the pool — the dollar leg must not become TAO-correlated.

### 3.2 The canonical tUSD/TAO pool (`pallets/swap`, extended)

One new pair in the existing swap pallet:

- **Protocol-owned liquidity (POL):** permanent, never withdrawn, fee-compounding.
- **Policy hooks as parameters, not code:** published asymmetric fee curve (cheaper in than out), protected TWAP execution for large exits, all tunable by governance in storage.
- Quotes come from **one runtime API** consumed identically by the precompile, RPC, and CLI.

### 3.3 Atomic entry/exit (`pallets/subtensor`, extended)

Two composition extrinsics (plus invoice settlement, §3.8):

- `swap_usd_and_stake(usd_asset, amount, netuid, hotkey, min_alpha_out)` — PSM → pool → existing `add_stake` / basket-deposit path.
- `unstake_and_swap_to_usd(...)` → tUSD → PSM redemption → outbound bridge message; USDC arrives at the user's address on the spoke chain.

These reuse the shipped staking paths and the v441 basket engine (slippage-aware NAV, keyless escrow, full-NAV share minting, pro-rata claims).

### 3.4 Exported tokens (chain-owned contracts on spokes)

Standard **ERC-20** tokens on Base/Ethereum, with three extensions:

| Extension | Purpose |
|---|---|
| ERC-2612 (permit) | Gasless approvals |
| xERC-20 (ERC-7281) | Only registered bridges may mint, each within a rate limit; owner (the chain) can add/revoke minters |
| ERC-4626 read surface (shares only) | DeFi integrations can price the share; actual mint/redeem is cross-chain |

Three token classes:

- **TAO (spoke ticker: `TAO`)** — plain 1:1 wrapper; the money leg, collateral leg, and the migration target for the legacy single-operator wTAO.
- **wALPHA_i** — appreciating share of staked alpha in subnet *i*. Escrowed alpha remains staked; emissions accrue to escrow; each share redeems for a growing amount of alpha (wstETH model — balances never rebase).
- **wBETA_v** — appreciating share of validator *v*'s v441 basket (diversified subnet exposure). Because baskets are per-validator, any validator's basket can be promoted to a wBETA — third parties join the canonical rail natively.

Deployment rules: one audited implementation per VM; **CREATE2** so the address is identical on every EVM chain; **9 decimals for every asset on every chain** (matching rao — no conversion math means no conversion bugs); supply invariant: Σ spoke supply ≤ hub escrow, published per block (§3.6).

### 3.5 Bridge topology: rent transport, supply the trust

- **Verification is never built in-house.** Bridge hacks are unbounded (Ronin $625M, Wormhole $326M, Nomad $190M); a rented, rate-limited bridge failing costs at most its cap.
- **Day-one corridor: Hyperlane** (permissionless at every layer; already live on Bittensor EVM). The destination **ISM quorum is the chain's own validator roster**: validators run the stock Hyperlane validator agent as a **pinned sidecar** in the validator deployment (compose profile — never inside the node process, never as an off-chain worker). Canonical mints on Base therefore require Bittensor's own validators' signatures — no new trust assumption beyond chain consensus itself.
- **Accountability on-chain:** validators register their agent's signing key against their hotkey; signed checkpoints that diverge from finalized state are portable fraud proofs; the runtime **slashes TAO** on proof. Roster changes propagate through the control plane.
- **Aggregation:** the ISM requires the validator quorum AND one external validator set (code diversity against agent/infra bugs — the KelpDAO lesson: $292M lost through one compromised 1-of-1 verifier).
- **Second courier (phase 2):** a chain-operated **LayerZero DVN** (the endpoint is already live on Subtensor EVM, EID 30374), making the validator roster a mandatory co-signer on the second rail too.
- **Inbound (money arriving):** tiered confirmation. Small deposits clear on the rented rail alone; deposits above a threshold additionally require a native validator-observer quorum (off-chain workers polling operator-run foreign nodes — never third-party RPCs alone). Upgrade path: Snowbridge-style Ethereum light client pallet, which replaces committee trust with Ethereum's own cryptography.
- **Control plane:** admin operations (limits, minters, roster) originate as extrinsics, are delivered to spokes by **two independent bridges**, and obey an asymmetric timelock — **48h to escalate, instant to revoke**.

### 3.6 Registry and supply attestation (`pallets/token-registry`, new)

The registry is what makes "canonical" a queryable fact instead of a marketing claim:

- Per asset, per chain: canonical contract address, deployment standard, status (`canonical | migrating | deprecated → successor`), registered minters with live rate-limit windows and pending timelocked changes.
- **Per-block supply attestation:** hub-escrowed amount vs. sum of spoke supplies. Proof-of-reserve as a chain primitive — "check the invariant" replaces "trust the issuer."
- Exposed via precompile and public API; wallets and aggregators resolve it instead of guessing between competing mints.

### 3.7 The Gateway (chain-owned EVM contract)

The single Hyperlane recipient on Bittensor EVM. Its message envelope is the system's wire format:

```rust
struct GatewayEnvelope {
  version: u8,                // adaptability lives here
  asset: AssetId,             // non-exhaustive enum: Tao | TUsd | Alpha(netuid) | BasketShare(hotkey) | ...
  amount: u64,                // 9 decimals, always
  dest: AccountId32,          // the "account pairing": just a field, no ceremony
  action: GatewayAction,      // CreditClaim | SwapToTao | Stake{netuid, hotkey, min} | PayInvoice{...}
  nonce: u64,                 // replay protection via processed-set
}
```

**The Gateway never reverts.** If the requested action cannot execute (bad netuid, rate limit hit, pool paused), delivery still lands as a **claim record** — non-transferable, resolvable three ways: retry the action, convert to TAO, or bridge back out as USDC (with optional auto-return after N days). This collapses the cross-chain failure space to two states: *undelivered* (retryable; funds safe in origin escrow) or *delivered* (the user holds something spendable). "No stuck money" is a type-level invariant, inherited by every UI.

Outbound, the runtime dispatches Mailbox calls itself from a keyless system account (pallet-evm call) — users sign one Substrate extrinsic and never need an EVM key.

### 3.8 Service payments (`PayInvoice`)

Subnets price services in dollars; buyers pay in USDC (or ETH etc., swapped origin-side); settlement runs the full route:

```text
USDC (Base) → PSM → tUSD → TAO (canonical pool) → alpha (subnet AMM)
            → split: payee hotkey (100 − burn_bps) / burn (burn_bps)
```

- `burn_bps` is subnet-owner-configurable and public — the guaranteed net-demand slice; both swap legs additionally pay fees into protocol-owned liquidity.
- The receipt event `{invoice_id, usd_amount, alpha_delivered, block}` is watched by the subnet's API to unlock service — and doubles as **auditable per-subnet revenue**, enabling revenue-weighted baskets/indices and giving issuers a number auditors can verify.
- Designed for machine payments (x402 pattern): AI agents holding USDC pay subnet APIs programmatically.
- **One settlement path only.** Direct USDC-to-subnet payment lanes are explicitly out: they recreate the off-chain status quo where revenue never touches TAO.

---

## 4. Liquidity: how the chain funds the USD side

The chain never sells TAO for dollars. Three sources, in sequence:

1. **Treasury TAO seeds the TAO side.** Day one the pool is deep for buyers, shallow for sellers — acceptable for a demand rail; exits run through protected TWAP until the USD side matures.
2. **Selling claims on yield, never the base asset:**
   - **Alpha bootstrap auctions** — protocol-held alpha auctioned for USDC; existing alpha demand funds the dollar depth.
   - **Share bonds** — wALPHA/wBETA sold at a small discount for upfront USDC, vesting over weeks; forward-selling the product itself.
3. **Organic fill** — every inbound deposit swaps tUSD → TAO, accumulating tUSD (i.e., USD reserves) in the pool; usage builds the exit liquidity.

"First-class" means: **permanent** (POL is never withdrawn), **compounding** (fees recycle into depth), **published** (weekly depth/slippage scoreboard; the launch announcement is the depth number).

---

## 5. User experience

### 5.1 End user (btcli)

Five verbs. The bridge is invisible; the EVM layer never appears.

```text
$ btcli bridge deposit --from base --amount 1000 --then stake --netuid 64 --hotkey 5F...
  1/3 Base tx confirmed     0x8a2f…
  2/3 Hyperlane in transit  msg 0x77c1…  (~90s)
  3/3 Executed              ✓ staked 47.2 α on SN64

$ btcli exit --amount 5 --to base:0xYourAddr        # TAO → USDC lands on Base
$ btcli wrap alpha --netuid 64 --amount 50 --dest base --to 0x…   # one Substrate signature
$ btcli bridge status 0x77c1…                        # one state machine, human-readable next steps
$ btcli registry list | attestation                  # canonical addresses, live backing proof
```

Failure UX is a product guarantee: every failed action ends as a resolvable claim, never a stuck balance.

### 5.2 Integrator (storefronts, wallets, issuers)

Resolve the registry → call `swapUsdAndStake` / `unstakeAndSwapToUsd` (versioned precompile; released selectors are permanent) → poll one `transferStatus` object. Issuers additionally get in-kind create/redeem against shares and the attestation feed.

### 5.3 Validator operator

`compose up` brings up node + pinned Hyperlane agent sidecar; register the signing key once; a stipend pays for the duty; the watchdog and slashing enforce it.

---

## 6. Locked decisions and invariants

| Decision | Consequence |
|---|---|
| One door: the atomic extrinsic is the only entry/exit | No second pipe, ever, including for the chain itself |
| tUSD is non-holdable, non-transferable, internal-only | No on-chain USD↔alpha bypass; no stablecoin product surface |
| Gateway never reverts; fallback = claim record | "No stuck money" as a type-level invariant |
| 9 decimals, every asset, every chain | No conversion math anywhere (permanent commitment) |
| Versioned envelope; append-only ABIs; non-exhaustive enums | New assets/actions are one-variant diffs, never breaking changes |
| One rate-limit algorithm (linear-refill window) in PSM and xERC-20 | Auditors verify one algorithm, not two dialects |
| Token contracts immutable; parameters and periphery evolve | Holders trust bytecode; Gateway v2 deploys beside v1 |
| Escalate slow (48h timelock), revoke instant | Safety asymmetry in the control plane |
| Neutrality: policy is published rules, never discretion | The chain formally renounces front-running its own flow; neutrality is why everyone routes through the waist |
| Hub-and-spoke; spokes never mint against each other | Simplicity over mesh topology (permanent commitment) |

**Renounced (as load-bearing as the builds):** no in-house verification layer; no consumer swap UI; no tUSD export as a stablecoin; no emissions-paid mercenary liquidity; no PSM reserve rehypothecation.

---

## 7. Build plan (monorepo)

| ID | Component | Location |
|---|---|---|
| A1 | tUSD ledger + PSM | `pallets/usd-psm` (new; modeled on `pallets/alpha-assets`) |
| A2 | tUSD/TAO pair + POL + policy hooks | `pallets/swap` (extend) |
| A3 | Atomic extrinsics (incl. `PayInvoice` settlement) | `pallets/subtensor` |
| A4 | Registry + supply attestation + hub escrow | `pallets/token-registry` (new; escrow per `claim_root.rs` pattern) |
| B1 | USD action precompile + ERC-20 custody adapter | `precompiles/src/usd.rs` (+ `.sol`/`.abi`) |
| B2 | Registry read precompile | `precompiles/src/token_registry.rs` |
| C1 | xERC-20 suite, ShareToken, Lockbox, MigrationSwap, CREATE2 scripts | `contracts/evm` (new Foundry workspace) |
| C2 | Gateway (Hyperlane recipient, action router) | `contracts/evm` |
| D1 | Hyperlane corridor: warp config, validator-quorum ISM, sidecar packaging | `contracts/evm` + validator compose profile |
| D2 | Control plane (2-bridge-attested admin messages, asymmetric timelock) | `pallets/token-registry` + relayer script |
| E1 | E2E harness: localnet + anvil + local Hyperlane; golden CLI transcripts; failure drills | `ts-tests`, `justfile` |
| E2 | SDK client + btcli verbs | `sdk/`, btcli plugin |
| E3 | CI, invariant fuzzing, external audits | workflows |

**Milestones:** M1 kernel (A1→A2→A3→B1; localnet demo of one-call USDC→basket) → M2 registry+hub → M3 export layer + corridors (parallel once the standard freezes) → M4 hardening + audits, which **gates** liquidity seeding and any announcement.

**Scaffold order:** shared types in `primitives/` first (every phase imports them); walking skeleton second (a do-nothing envelope traversing Base → Gateway → runtime event); golden transcripts as CI fixtures third; then logic fills the skeleton.

The entire pipeline is locally testable with zero external dependencies: localnet subtensor + anvil (as Base, with free ETH) + Hyperlane core deployed and operated locally via its CLI + a mock USDC. Failure drills (rate-limit breach, minter revocation, relayer outage, forged checkpoint) run in the same rig.

---

## 8. What we cannot build (external dependencies)

| Dependency | Nature | Strategy |
|---|---|---|
| Native USDC (CCTP) | Circle's code and compliance process | Open the conversation at M1 — longest lead time, no substitute |
| Bridge committees' honesty | External organizations | Cap (rate limits), diversify (two corridors), watch (attestation), slash (own roster) |
| Cross-chain atomicity | Physics of two consensus systems | Hide it: interchain accounts, claims, later intent/solver fast-fills |
| Wallet/aggregator adoption | Phantom/Jupiter/MetaMask product decisions | Earn with depth; registry makes canonical status provable |

---

## 9. Rollout

1. **Depth before noise.** No announcement until the pool absorbs a $250K order under 1% slippage.
2. **One corridor, one storefront.** Base only; partner storefront (e.g., TaoFi) rebased onto canonical rails, keeping its own fees.
3. **The yield product.** wALPHA for a handful of top-revenue subnets + wBETA baskets. Mint/redeem permissionless so secondary markets anywhere pin to NAV and all net flow settles through the hub.
4. **Index + institutions.** Revenue-weighted index (fed by PayInvoice receipts); in-kind creation/redemption for ETP issuers.
5. **Weekly public scoreboard:** stablecoin float, pool depth, net USD→TAO flow, TAO burned, canonical vault TVL.

Cheap wins alongside: migration path for the legacy wTAO ($23M single-operator honeypot — risk removal and launch TVL in one move); consolidation of the Solana dual-mint when the Solana corridor opens.

---

## 10. Risks

- **The chain becomes the headline risk.** Mitigation: minimal owned surface (~6 audited artifacts), immutable tokens, formal verification of the supply invariant, published attestation.
- **Provider sell-back neutralizes PayInvoice demand.** Mitigation: burn_bps floor + both-leg fees to POL; honest accounting in all public materials.
- **Thin exit liquidity early.** Mitigation: TWAP-protected exits, published depth targets, bootstrap instruments (§4).
- **Regulatory surface of yield shares.** Mitigation: issuers (ETPs) package the regulated wrapper; the chain ships neutral infrastructure with in-kind redemption.
- **Bridge compromise.** Bounded by rate limits; detected by attestation; recovered by instant revoke + claims process.

---

## 11. Open questions

1. Validator roster size and stipend level for the ISM quorum (proposal: 9–15 named operators, quarterly rotation).
2. burn_bps default and bounds for PayInvoice (proposal: default 5%, range 0–20%).
3. Which subnets get wALPHA at launch (proposal: top 3–5 by verifiable revenue once receipts exist; baskets first).
4. Exit-side fee curve parameters and the USD-depth threshold that retires TWAP protection.
5. Timing and terms of the legacy wTAO wind-down negotiation.
