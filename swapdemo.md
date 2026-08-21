# Subnet tokens swap demo

Buy and sell subnet tokens from MetaMask with USDC. Each token is an ERC-20
on a local fake-Base chain, backed 1:1 by real staked alpha in an on-chain
escrow on a local Bittensor chain. Balances rebase upward as the escrow
earns emissions.

## What you need

- **Rust toolchain** — builds the `node-subtensor` binary (first run only,
  slow).
- **Foundry** (`anvil`, `forge`, `cast`) — runs the fake Base chain and
  deploys contracts.
- **Node.js + pnpm** — runs the rig scripts (`cd ts-tests && pnpm install`
  once).
- **Docker** — runs the Hyperlane agents (two validators, one relayer) that
  carry messages between the two chains.
- **jq**, **just** — script plumbing.
- **MetaMask** in your browser.

## Start the rig

```bash
just rails-up
```

This starts, in order: the Bittensor localnet (3 nodes), anvil (fake Base,
chain id 84530), the Hyperlane message layer, and the rails contracts. It
then creates 8 demo subnets (Apex, Targon, Vanta, blockmachine, lium.io,
Gradients, Ridges, Chutes), sets their real mainnet identities on-chain
(name, description, logo), and deploys one ERC-20 per subnet on fake Base.

Takes ~5 minutes. It is done when it prints `rig is up` and the manifest
path. Keep the terminal open: closing it stops the chains.

## Open the demo

```bash
just rails-demo
```

This serves the page at <http://127.0.0.1:8666/demo/> and opens it.

1. Click **Connect MetaMask**. The page adds the "Base rails localnet"
   network for you and tops your account up with gas ETH automatically.
2. Pick a subnet in the dropdown. Name, logo, description, live pool price
   and hub escrow all come from the chain.
3. Click **Faucet USDC** for test money (no signature needed).
4. Enter an amount and click **Buy**. One approve + one buy transaction.
   The hub mints tUSD against your USDC, swaps to TAO, and stakes into the
   subnet's escrow. The token lands in ~20–60 seconds.
5. Click **Watch in MetaMask** to see the token in your wallet. The balance
   ticks upward by itself as emissions accrue (the index heartbeat pushes
   every ~20 blocks).
6. **Sell** any amount (blank = all). The hub unstakes, swaps back, and
   USDC returns to your address.

The log at the bottom of the page narrates every step and prints wallet
errors verbatim.

## Check it from the terminal

```bash
just rails-ping        # end-to-end message round trip
cd ts-tests && pnpm exec moonwall test rails   # full e2e suite (12 tests)
```

## Stop it

```bash
just rails-down            # stop chains + agents, keep state
just rails-down --purge    # also wipe state (next up is a fresh rig)
```

## Known localnet quirks

- MetaMask cannot show fiat prices or charts for a custom chain; the page's
  "Price (pool quote)" row is the real execution price.
- After a `--purge` restart, MetaMask's cached nonce can go stale. The page
  detects the stuck transaction and heals it automatically (watch the log).
- If MetaMask refuses the network add, add it manually: RPC
  `http://localhost:8545`, chain id `84530`, currency ETH.
