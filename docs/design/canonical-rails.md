---
title: Canonical rails
description: Design of the canonical cross-chain rails. A stablecoin on Base becomes tUSD, TAO, a subnet's alpha token or service credits on one route through Bittensor, and back.
---

# Canonical rails

This document is the design of the canonical rails as they are built in this repository.
It replaces `docs/canonical-rails-design.md` of the earlier demo (PR 3110).
[`AUDIT.md`](https://github.com/RaoFoundation/subtensor/blob/main/AUDIT.md) lists the
trust assumptions, the admin powers, the invariants and the known risks.

## 1. Purpose

A person who holds USDC and an Ethereum wallet can use Bittensor without a Bittensor
wallet. That person can:

- get **canonical TAO**;
- get the **alpha token** of a subnet, for example `CHUTES` or `LIUM`;
- buy **service credits** at a subnet's service, for example "USDC to LIUM credits";
- sell a token and get USDC back.

Every dollar takes one route. No other route exists.

```text
USDC  <->  tUSD  <->  TAO  <->  subnet alpha  ->  service credits
(spoke)   (hub)     (hub)      (hub)              (hub, burned)
```

A dollar reaches alpha only through TAO. TAO is bought only in the canonical pool. There
are no tUSD/alpha pools.

### In version 1

| Part | Where |
| --- | --- |
| tUSD, a native asset | `pallets/tusd` |
| The canonical tUSD/TAO pool | `pallets/canonical-pool` |
| The hub: orders, vaults, reserve, registry | `pallets/rails` |
| Index hotkeys and credits paid through a bridge | `pallets/subtensor` |
| The hub's interface for contracts | `precompiles/src/rails.rs`, address `0x815` |
| Gateway, portal, vault, tokens | `contracts/evm` |
| The local rig and the end-to-end tests | `scripts/rails`, `just rails-up` |
| btcli `bridge` and `pool` | `sdk/python` |
| The swap page | `website/apps/bittensor-website`, route `/swap` |

### Not in version 1

- A validator basket (beta) token.
- A gamma token that users hold.
- `add_stake_for`: stake for an account on Bittensor from another chain. Section 9 says
  how the design leaves room for it.
- Migration of legacy wTAO.
- A second spoke. The design has many spokes. Only Base is deployed first, then Ethereum.

## 2. Words

- **Hub**: Bittensor. All value is settled here.
- **Spoke**: another chain, for example Base.
- **Transport**: the system that carries messages between the chains. It is Hyperlane.
- **Security module** (ISM): the contract of the transport that decides if a message is
  authentic.
- **Portal**: the contract on a spoke that sends orders and carries out the hub's answers.
- **Vault** (on a spoke): the contract that holds the USDC. It cannot be upgraded.
- **Gateway**: the contract on Bittensor's EVM that passes messages between the
  transport and the hub.
- **Bridge**: one portal that the hub approved, with its own limits. A bridge has an id.
  The id of Base is 8453.
- **Order**: a message from a portal to the hub: "I took this, deliver that".
- **Answer**: a message from the hub to a portal: "pay this to that address".
- **Keeper**: a program that asks the gateway to hand the hub's messages to the
  transport. Anyone can run one. It can only send what the hub decided.
- **Deferred message**: a message of the hub that left its queue, because the transport
  charged more for it than was paid. It stays in the hub until someone sends it.
- **Congested bridge**: a bridge with 256 or more deferred messages. The hub makes no
  new payment on its spoke until some of them have left.
- **tUSD**: the dollar on Bittensor. One tUSD exists for each dollar of stablecoin that a
  vault holds.
- **Canonical TAO**: a token on a spoke. One token is one TAO in the hub's reserve.
- **Alpha token**: a token on a spoke. One token is one share of the hub's vault of a
  subnet.
- **Vault** (on the hub): the account that holds the staked alpha of one subnet for all
  holders of that subnet's alpha token.
- **Rate**: the alpha that one alpha token is worth. It grows. The number of tokens does
  not change.
- **Index hotkey**: a hotkey without a private key. It lends its stake to the best
  validators of its subnet by a fixed rule.
- **Credits**: gamma. One gamma is one dollar of a subnet's alpha, burned at a service.
- **Guardian**: an account that can pause, and can do nothing else.

All amounts on the hub and all canonical tokens have 9 decimals. USDC has 6. The portal
converts. A part of a deposit that is smaller than one millionth of a dollar cannot
exist on the spoke, so no conversion loses value in that direction. In the other
direction, the part of an amount below one millionth of a dollar stays in the vault.

## 3. The parts

### 3.1 tUSD (`pallets/tusd`)

tUSD is a native balance. It is transferable. It is not an ERC-20 token on Bittensor's
EVM, and it does not leave Bittensor.

- tUSD is minted only when an approved bridge reports a deposit. It is burned only when
  that bridge releases the stablecoin. No extrinsic mints or burns.
- Each bridge has a **mint cap**: the most tUSD it can have outstanding.
- Each bridge has a **mint window**: a rate limit that refills linearly. An empty
  window is full again after its period.
- A bridge can burn only what it has outstanding. A bridge cannot release the
  stablecoin of another bridge.
- Root adds a bridge and sets its terms. A new bridge starts disabled. Root or the
  guardian disables a bridge. Only root enables it.
- The route of a bridge is set one time: its chain, its transport domain, its portal
  and its stablecoin. Root can change the mint cap and the mint window later, and
  nothing else. What a bridge has outstanding is in the vault of its portal, and what
  the hub still has to pay out for it is paid by that portal. Another portal could do
  neither. A portal is upgraded in place, at the same address. A new portal with a
  new vault is a new bridge, with a new id.
- An account holds zero or at least 0.01 tUSD.

Many bridges can mint tUSD. A bridge that is compromised can mint at most its own cap,
and can drain at most its own vault.

### 3.2 Canonical pool (`pallets/canonical-pool`)

The pool is the one place where tUSD and TAO trade.

- **Math.** The pool reuses the swap pallet's balancer math (`balancer.rs`) at weights
  of one half each. That is a constant-product pool. The pallet does not contain a
  second copy of the math. It adds the fee, the limits and the share accounting.
- **Off at the start.** The pool ships disabled. Root enables it with
  `CanonicalPool.set_enabled`. Root or the guardian disables it. A disabled pool
  refuses swaps and deposits. It always lets a provider remove liquidity.
- **Open liquidity.** Anyone adds TAO and tUSD in the current ratio of the pool and
  gets shares. A share redeems its part of both reserves at any time. There is no
  fixed seed.
- **The first deposit sets the price.** It must be within 2% of the TAO price of the
  root oracle. The first deposit locks 1,000 shares for ever, so the share supply
  never returns to zero.
- **Fee.** 0.3% by default. Root can set it between 0.05% and 1%. The fee stays in the
  pool, so the liquidity providers earn it.
- **Limits that loosen with depth.** One swap pays in at most 2% of the reserve it
  pays into. All swaps in one direction pay in at most 20% of that reserve in a
  window of 300 blocks that refills linearly. Both limits are fractions of the
  reserve: they grow as the pool grows. Root can change them.
- **Time-weighted price.** The pool keeps a price accumulator. `twap_price` is the
  average price over at least 300 blocks (about one hour). It does not exist until
  the pool has that much history.

### 3.3 The hub (`pallets/rails`)

The hub settles orders. It holds no user balances between orders: the hub account is
empty before and after each order.

**Orders.** Only the gateway contract can call `Rails.execute`. The hub then:

1. finds the bridge of the portal that sent the order. It refuses a portal that it does
   not know.
2. takes the order only if its nonce is the next nonce of that bridge. Each order
   settles one time, in sequence.
3. runs the route in a storage layer. If a step fails, the hub discards all changes and
   queues an answer that returns to the payer exactly what the portal took.
4. writes a receipt: settled, refunded, rejected or skipped.

An order fails as a whole, and stays with the transport for a new attempt, only if the
hub is paused, the portal is unknown, its bridge is disabled, the order is out of
sequence, or the hub could not queue the order's answer: the queue of the bridge is
full, or the account of the bridge does not hold the reserve for one more message (see
"Who pays for a message"). In each case what the payer locked stays on the spoke.
Deferred messages do not fill the queue, so they never keep an order out (see "A
congested bridge").

Nothing that happens to an answer on its way out can fail an order. The gateway
settles an order in one transaction, and hands answers to the transport in others (see
"The outbox").

**What an order can deliver.**

| Destination | What the hub does |
| --- | --- |
| Stablecoin | burns tUSD of the bridge, tells the vault to release USDC |
| Canonical TAO | moves TAO to the reserve account, tells the portal to mint |
| Alpha token | stakes TAO in the subnet's vault, tells the portal to mint shares |
| Credits | buys alpha, burns it, spends the gamma at the service. No answer |
| tUSD on Bittensor | transfers tUSD to an account |
| TAO on Bittensor | transfers TAO to an account |

**Refunds.** A refund returns the asset that the payer paid. A deposit of USDC returns
as USDC. Burned tokens are minted back. A refund is whole: the hub takes nothing from
it.

**What cannot be paid is not settled.** An order for the zero address would settle on
the hub and then never pay out, because a token cannot be minted to that address. The
portal refuses such an order before it takes any funds. The hub refuses it too and
refunds the payer.

**The stablecoin is paid in whole units.** USDC has 6 decimals and the hub counts in 9.
The hub tells the vault to release whole units of the stablecoin, and the order's
minimum counts those units. The part of the tUSD below one unit is burned with the
rest, and its stablecoin stays in the vault.

**Minimum and deadline.** Each order carries `minOut` and `deadline`. If the route
would deliver less than `minOut`, or the hub sees the order after `deadline`, the hub
refunds it.

**The reserve.** One account holds the TAO behind all canonical TAO. The hub tracks the
supply of each spoke and never lets a spoke burn more than the hub let it mint. Each
spoke has a cap.

**Vaults and alpha tokens.** Each subnet with a market has a vault account. The vault
stakes at the subnet's index hotkey. An alpha token is a share of the vault:

```text
shares minted = alpha staked * (shares + V) / (alpha in vault + V)
alpha paid    = shares burned * (alpha in vault + V) / (shares + V)
```

`V` is 0.001 of a unit of each. It is a guard against the first-depositor attack on
share vaults. Both divisions round against the trader. The staked alpha earns, and the
number of shares does not change. So one token is worth more alpha with time, as
wstETH is worth more stETH. The token does not rebase.

Each market has a cap on the alpha in its vault.

**A subnet that is dissolved.** The chain pays the stake of a dissolved subnet to its
stakers in TAO. The vault account receives that TAO. From then on the alpha token can
only be sold: each share gets its part of the TAO in the vault. In version 1 a netuid
has one market for ever. If the netuid is registered again for another subnet, the old
market still only winds down, and the new subnet cannot have a market.

**The outbox.** The hub cannot call a contract. It puts each message for a spoke in a
queue. Each bridge has its own queue. The messages leave when someone calls
`Gateway.flush(bridge, limit)`. A keeper does this, and anyone can: the call has no
access control, and it sends only what the hub decided. The gateway takes the message
at the head of the queue out of the hub, pays the transport and hands it the message,
in one transaction. Then it takes the next one. The queue of a bridge holds at most
1,024 messages.

Orders and answers never share a transaction. The transaction that settles an order
hands nothing to the transport. So a transport that fails on the way out, or that
charges more than is paid, cannot undo an order or hold it up.

**A deferred message.** If the transport charges more for a message than the hub pays
and the caller adds (see "Who pays for a message"), the gateway defers the message
(`Rails.defer_outbound`). The message leaves the queue, and the messages behind it go
on. It stays in the hub, and its reserve stays in the account of the bridge. Anyone
can call `Gateway.send(bridge, nonce)` to hand it to the transport later: when the hub
pays more, when the charge falls, or with the difference paid by the caller. A keeper
does that when the hub pays the whole charge. The hub never drops a message.

The spoke takes the hub's messages in any order. Each message is carried out one
time. A payment of an alpha token waits for the message that creates the token. An
alpha token keeps the rate that the newest message reported.

**A congested bridge.** A deferred message takes no place in the queue. It holds its
reserve in the account of the bridge, as every message does that has not left, and
that is the bound on their number. So a bridge whose messages are deferred still takes
orders. But the hub does not add payments to a backlog that nobody pays for. A bridge
is congested from 256 deferred messages on. Then the hub makes no new payment on that
spoke:

- It takes an order that asks for a payment on the spoke, and refunds it. The receipt
  gives the reason `Congested`. The refund is a message too, and it can be deferred as
  the others are. The payer, or anyone, can send it and pay the difference.
- It refuses a payment through `Rails.bridge_out`. The call fails as a whole, and the
  account keeps its funds.
- It settles an order that pays out on Bittensor. Such an order needs no message.
- A quote for the spoke gives `Congested`, so a client can know before anyone pays.

The bridge makes payments again when fewer than 256 of its messages are deferred.
That happens when someone pays the differences (`Gateway.send`), or when someone pays
into the account or root raises the ceiling and a keeper sends the messages.

**A message that the transport refuses.** If the transport refuses a message, the
whole call fails, and the message stays at the head of its queue. It holds only the
messages to the same spoke: the messages to other spokes are in other queues. Orders
of that bridge still settle, until its queue is full. The message leaves when the
transport takes it again. A keeper sees this state: its call of `flush` would fail.
The rig's keeper raises an alert that names the message. Root can then defer the
message (`Rails.defer_outbound(bridge, nonce)`), so that the messages behind it go on.
The gateway does not defer such a message itself, because it would have to catch the
failed call (see "No call fails in silence"). Section 11 of `AUDIT.md` lists what to
watch and who acts.

**From Bittensor.** An account on Bittensor pays TAO or tUSD with `Rails.bridge_out`
and receives the stablecoin, canonical TAO or an alpha token on a spoke. The call
fails as a whole if it would deliver less than its minimum.

**Who pays for a message.** The transport charges the gateway for each message. An
order that is refunded has an answer too. If answers were free, a person could send
many orders that must be refunded and use up the TAO that pays for the answers of
other payers. So each payment pays for its own message where it starts, and the hub
sets the TAO for a message aside before it takes the order:

- **The account of a bridge.** The TAO that pays the transport is the hub's. Each
  bridge has its own account on the hub (`Rails.message_account(bridge)`). Anyone pays
  into it with `Rails.fund_messages(bridge, amount)`. Nothing takes TAO back out of
  it. The gateway contract holds no TAO between transactions.
- **The reserve and the ceiling.** Root sets two amounts for a bridge with
  `Rails.set_message_reserve`. The reserve is what the hub sets aside for each message
  to that spoke. Set it above what the transport charges. The ceiling is the most that
  the hub pays for one message. It is at least the reserve. A bridge takes no order
  before these are set.
- **Before a message is queued**, the account of the bridge must hold the reserve for
  each message of the bridge that has not left, in the queue or deferred, and for the
  new one. If it does not, the hub does not take the next order of the bridge. The
  order stays with the transport and is delivered again later. Its funds stay locked
  on the spoke. So the hub never settles an order whose answer it could not pay for.
- **When a message leaves**, the gateway tells the hub what the transport charges for
  it. The hub moves that amount from the account of the bridge to the gateway. The
  gateway pays the transport with it in the same transaction. What the message does
  not use of its reserve stays in the account.
- **If the transport charges more than the reserve**, the hub pays the difference
  from what the account holds beyond the reserves of the messages that have not left,
  up to the ceiling. It never uses the reserve of another message. So messages that a
  higher charge holds up leave when the account has TAO to spare, and anyone can pay
  TAO in. The ceiling bounds what a wrong price from the transport can take for one
  message.
- **If the hub does not pay the whole charge**, because the account has nothing to
  spare or the charge is above the ceiling, the caller of `Gateway.flush` or
  `Gateway.send` can pay the difference with the call. What the caller sent and was
  not needed comes back. If nobody pays the difference, the message is deferred. An
  order of the bridge still settles in that time.
- **An order on a spoke** pays the portal's **answer fee**, in the spoke's coin, on top
  of the transport's fee for the order. The owner sets it. The fees collect in the
  portal, and anyone can send them on to the fee recipient: the operator that pays
  them into the account of the bridge on the hub.
- **A payment through `Rails.bridge_out`** pays the reserve in TAO into the account of
  its bridge, on top of what it sends. What the transport does not charge of it stays
  in that account.

So a message that the hub queued can always leave at a charge up to the reserve,
whatever the messages before it cost. A shortage of one bridge stops only the new
orders of that bridge. It cannot stop the payments of another bridge, and it cannot
take the funds of a payer: an order that the hub does not take stays whole on the
spoke.

An example with a reserve of 0.001 TAO and a ceiling of 0.003 TAO. Three messages wait,
and the account holds 0.003 TAO: the three reserves. The transport's charge rises to
0.002 TAO. A keeper calls `flush`. No message is paid for: its reserve is 0.001 TAO,
and the account has nothing to spare. The three messages are deferred. Someone pays
0.003 TAO into the account. Now the keeper sends each message at 0.002 TAO: 0.001 TAO
from its reserve and 0.001 TAO from what was paid in.

**The registry.** `Rails.Tokens` lists, for each bridge and asset, the address, the
name and the symbol of the canonical token. Wallets and integrators read it to tell
the canonical token from a copy. `btcli bridge tokens` shows it.

### 3.4 Index hotkeys (`pallets/subtensor/src/staking/index_hotkey.rs`)

The vault's alpha must be staked somewhere. If a person chose the validator, that
person would direct a large stake. So nobody chooses.

Each subnet with a market has an **index hotkey**: an account of the pallet, derived
from the netuid. It has no private key, so nobody can sign for it. It is not
registered and has no UID. Staking to it is ordinary staking.

The index hotkey lends all of its stake weight to children. A fixed rule picks them
from the last epoch of the subnet:

1. take the validators that have a validator permit and a dividend above zero;
2. order them by dividend, highest first, then by UID;
3. look at the first 32 at most. Keep a validator if its childkey take is not above
   the highest delegate take, and if it can accept one more parent;
4. stop at 8;
5. give each the same part, at most 25%.

With 4 children or more, all of the stake weight is lent. With fewer, each gets 25%
and the rest earns nothing until more validators qualify.

The rule runs again for one index per block at most, and for each index at most once
per childkey cooldown: the time an ordinary parent waits to change its children. The
children take effect at once, because the rule reads consensus results and no owner
chooses.

An index hotkey differs from an ordinary hotkey in four ways only:

- it can have 8 children, not 5;
- its take is 0 and cannot change, so its stakers keep all it earns;
- the protocol sets its children. No signature and no stake threshold applies;
- nobody can register it, swap to it, or make it a child.

All other validators keep the normal limits. What a child earns on the lent stake
flows back to the index hotkey's stakers, less the child's own childkey take.

Anyone can stake to an index hotkey with `add_stake`, as to any hotkey. The vault's
stake is its own position, counted by its coldkey.

### 3.5 Credits

A credits order runs USDC, tUSD, TAO, alpha, gamma, and then burns. The hub:

1. buys TAO in the canonical pool;
2. buys alpha in the subnet's pool and burns it;
3. values the burned alpha in dollars, as a gamma mint does, and spends that gamma at
   the service for the buyer's address.

Nothing is left to hold or redeem. There is no redemption path.

**Pricing that a pool cannot bend.** Three values price a credit. Each is conservative:

- the alpha is valued at the lower of the subnet's slow gamma price and its spot
  price. A buyer who pushes the spot price up gets no more credit;
- a TAO counts for no more dollars than the lower of the root oracle's price and the
  canonical pool's time-weighted price. A pool that is pushed cheap for a moment
  cannot turn one dollar into more than one dollar of credit;
- the order's `minOut` is in credits. If the buyer would get less, the order is
  refunded in USDC.

A credit is recorded in three places for the service to read:

- storage `SubtensorModule.GammaSpentByPayer(netuid, service, payer)`: a total that
  only grows;
- the event `GammaSpentV2`, with the payer's address, the origin chain and the
  order's nonce;
- the event `CreditsPurchased(beneficiary, netuid, service, origin, nonce, gamma)` of
  the gateway contract, indexed by the buyer's Ethereum address. A supplier that
  watches Bittensor's EVM filters on its own service and credits the address.

A supplier must credit from finalized blocks, and can use the storage total to check
that it missed no event.

### 3.6 The precompile (`0x815`)

Contracts reach the hub through the Rails precompile. Its state-changing functions
dispatch the pallet's calls with the caller as the signer, so the pallet's own checks
decide. Only the gateway passes them.

| Function | Use |
| --- | --- |
| `execute(origin, sender, order)` | settle an order, and learn its bridge. Gateway only |
| `ackOutbound(bridge, nonce, paid)` | take a message out of the hub: the head of the queue of a bridge, or a deferred message. The hub moves `paid` from the account of that bridge to the gateway: at most the limit that `nextOutbound` and `outbound` return. Gateway only |
| `deferOutbound(bridge, nonce)` | defer the head of the queue of a bridge. Gateway only |
| `nextOutbound(bridge)` | the head of the queue of a bridge, encoded for the portal, and the most that the hub pays for it now |
| `outbound(bridge, nonce)` | one message that the gateway can take now, the head of the queue or a deferred message, encoded for the portal, and the most that the hub pays for it now |
| `quote(bridge, order)` | what an order would deliver now, or why it would be refunded |
| `receipt(bridge, nonce)` | what the hub did with an order |
| `nextOrder(bridge)` | the next nonce the hub takes |
| `alphaRate(netuid)` | the live rate of an alpha token |
| `creditsOf(netuid, service, payer)` | the credits bought for an address |

`quote` runs the real route and discards the result. It is exact for the current
block.

### 3.7 Contracts (`contracts/evm`)

**On Bittensor's EVM**

- `Gateway` (UUPS, upgradeable). It passes what the transport delivers to the hub
  (`handle`), and the hub's messages to the transport (`flush`, `send`). It decides
  nothing. It emits `CreditsPurchased`. It holds no TAO between transactions: the hub
  hands it what it pays the transport for each message.

**On each spoke**

- `Vault` (not upgradeable). It holds the USDC. A payer deposits here, so an approval
  of USDC is only given to code that cannot change. The portal can do one thing at
  the vault: release USDC, inside a rate limit that only the owner changes.
- `Portal` (UUPS, upgradeable). It holds no stablecoin and no tokens, and it takes no
  approvals. It numbers the orders, sends them, and carries out the hub's answers.
  Each answer is carried out one time. It holds only the answer fees, in the chain's
  coin, until `sweepFees` sends them to their recipient.
- `CanonicalTao` and `AlphaToken` (not upgradeable). Only the portal mints, when the
  hub says so. Only a holder burns, by calling `sell` on the token. The portal cannot
  take tokens from anyone.
- `TimelockController` (OpenZeppelin). It owns the gateway, the portal and the vault's
  settings.

**A payment of USDC can arrive in parts.** The vault releases a limited amount in a
period of time. This limit bounds what a fault can take. The portal records each
payment of USDC that the hub orders (`owed`) and releases what the limit allows at
that moment. The rest stays on record. Anyone can call `Portal.claim(address)` later:
the vault then releases more to that address, as its allowance refills. The USDC goes
only to the address that is owed. The same holds while the vault is stopped: the debt
stays on record.

**No call fails in silence.** The gateway and the portal never catch a failed call.
If the transport fails when the gateway hands it a message, the whole call fails, and
the message stays in the hub. If the stablecoin refuses a payment, the whole
transaction fails, and the transport delivers the hub's message again later. A
contract that caught such a failure would succeed with too little gas. A relayer that
estimates gas would then send transactions that do only half the work, and a caller
could choose the gas so that a step fails. Where a step can wait without a fault, the
contracts decide that in code: a message that is not paid for is deferred, and a
payment above the vault's allowance leaves a debt on record.

**Token names.** When root lists a market, the hub reads the subnet's identity from
the chain and sends the name and the symbol to the portal. The symbol is the first run
of letters and digits of the name, in upper case, at most 11 characters: "Chutes"
gives `CHUTES`, "lium.io" gives `LIUM`. A subnet without a usable name is `SN64`. The portal creates the token
with CREATE2, with the netuid as the salt and with creation code that takes no
arguments. So the address of a subnet's token is known before the token exists, and
the hub's registry can list it. A name is fixed when the token is created.

**What the alpha token shows.** `rate` is the rate that the hub reported with the last
mint. `principalOf(holder)` is the alpha that the holder's tokens were worth when the
holder got them. The swap page reads the live rate from the hub and shows "worth X
alpha, +Y% since bought".

### 3.8 The transport

Hyperlane carries the messages. The gateway and the portal each name their own
security module. Zero selects the transport's default. A production deployment sets a
multisig module with validators that the owner chooses.

The transport delivers a message at least one time, in any order. The rails do not
depend on more:

- the hub takes orders strictly in sequence, so a replay or a reordering fails and is
  tried again later;
- the portal carries out each answer one time, by its nonce, in any order.

### 3.9 The wire format

Both directions use the Solidity ABI encoding. The contracts never read SCALE.
`precompiles/src/rails_wire.rs` is the only code that converts. An order is 12 words
and has exactly one valid encoding: a field that its kind does not use must be zero.
The first word is a version. The Rust and the Solidity tests share golden vectors.

## 4. Flows

### Buy an alpha token

1. The payer approves the vault for USDC, one time.
2. The payer calls `Vault.deposit(amount, destination, minOut, deadline)`. The vault
   takes the USDC and tells the portal.
3. The portal sends order `n` through the transport.
4. The transport's security module verifies the message on Bittensor. The gateway
   calls the precompile.
5. The hub mints tUSD, swaps it for TAO in the canonical pool, stakes the TAO at the
   subnet's index hotkey, counts the new shares, and queues the answer.
6. A keeper calls `Gateway.flush` for the bridge. The gateway pays the transport from
   the account of the bridge and hands it the answer.
7. The portal mints the alpha token to the payer.

On the local rig this takes about 10 seconds.

### Sell an alpha token

1. The holder calls `sell` on the token. The token burns and the portal sends the
   order.
2. The hub unstakes the alpha of the shares, swaps TAO for tUSD, burns the tUSD, and
   answers.
3. The vault releases USDC to the holder.

### Buy credits

Steps 1 to 4 are the same. Then the hub buys TAO, pays for the gamma from the
subnet's vault account, and records the credit. The gateway emits `CreditsPurchased`.
No message returns to the spoke.

### A refund

If the route fails at any step, the hub discards the changes and answers with a
refund. The vault releases the same USDC to the payer. The receipt names the reason.

### Leave Bittensor

1. An account calls `Rails.bridge_out` with tUSD or TAO. It also pays the reserve for
   one message into the account of the bridge.
2. The hub settles the route and queues the answer.
3. The answer leaves when a keeper, or anyone, calls `Gateway.flush` for the bridge.

### A message that is not paid for

1. The transport charges more for a message than the hub pays for one.
2. A keeper calls `Gateway.flush`. The gateway defers the message. The messages behind
   it leave, if they are paid for.
3. Someone pays into the account of the bridge, or root raises the ceiling. Or the
   charge falls. Then a keeper calls `Gateway.send` for the message, and it leaves.
4. Or anyone calls `Gateway.send` for the message and pays the difference.
5. If nobody does any of this and 256 messages of the bridge are deferred, the bridge
   is congested. The hub refunds the next orders for the spoke. It settles orders
   again when deferred messages have left.

### A payment larger than the vault's allowance

1. The hub's answer arrives. The portal records the full amount as owed.
2. The vault releases what its allowance covers. The payee has that part.
3. Later, anyone calls `Portal.claim(payee)`. The vault releases more. This repeats
   until nothing is owed.

## 5. Limits and switches

| Limit | Default | Who sets it |
| --- | --- | --- |
| tUSD mint cap of a bridge | set per bridge | root |
| tUSD mint window of a bridge | set per bridge | root |
| Canonical TAO cap of a bridge | 0 until set | root |
| Alpha cap of a market | set when opened | root |
| Pool fee | 0.3%, from 0.05% to 1% | root |
| Pool swap limit | 2% of the reserve | root |
| Pool flow limit | 20% of the reserve per 300 blocks | root |
| Vault release limit (spoke) | set at deployment | the timelock |
| Answer fee of an order (spoke) | set at deployment, no default | the timelock |
| Message reserve and ceiling of a bridge (hub) | not set: the bridge takes no order | root |
| Route of a bridge (hub) | set when the bridge is added | nobody, after that |
| TAO in the message account of a bridge (hub) | none until someone pays in | anyone pays in |
| Queue of a bridge | 1,024 messages. Deferred messages do not count | runtime constant |
| Deferred messages from which a bridge is congested | 256 | runtime constant |

| Switch | Who turns it off | Who turns it on |
| --- | --- | --- |
| The hub (`Rails.set_paused`) | root, guardian | root |
| A bridge (`Tusd.set_bridge_enabled`) | root, guardian | root |
| The pool (`CanonicalPool.set_enabled`) | root, guardian | root |
| The gateway contract | owner, guardian | owner (timelock) |
| The portal contract | owner, guardian | owner (timelock) |
| Releases of the vault | owner, guardian | owner (timelock) |

## 6. Who controls what

- On Bittensor, the admin origin of the three pallets is root.
- The guardian is one account that root names with `Rails.set_guardian`. It can only
  pause.
- Each contract's owner is a timelock with a delay of 48 hours. A multisig proposes
  and executes through it. Each contract has a guardian that can only pause.
- The vault and the tokens cannot be upgraded.

Jacob holds the multisig and the guardian keys at the start.

## 7. Decisions of 27 November (section 10.10 of the draft)

| Decision | How it is built |
| --- | --- |
| D3a. A new `pallet-canonical-pool` that reuses the balancer math. It ships off. Anyone adds liquidity. No fixed seed. Limits loosen with depth | section 3.2 |
| D3b. Fee 0.05% or more, 0.3% by default, paid to the liquidity providers | section 3.2 |
| D3d. Credits have a minimum. Refunds return in USDC | sections 3.3 and 3.5 |
| D3e. Upgradeable contracts: multisig, 48 hour timelock, guardian that only pauses. The USDC is in a vault that cannot be upgraded | sections 3.7 and 6 |
| D3f. Base first | the bridge id is the chain id. A second spoke is a second bridge |
| D3g. Canonical TAO. Legacy wTAO is not handled | section 3.3 |
| D3h. Tokens do not rebase. The site shows "worth X alpha" | sections 3.3 and 3.7 |
| D3i. `add_stake_for` later | section 9 |
| D3j. TAO is the only route into alpha | sections 1 and 3.2 |
| Index hotkey: no key, no registration, 8 children and 0 take for it alone | section 3.4 |
| Frontend: the demo page becomes a static route of the Next.js app | `/swap` |

## 8. Decisions that the draft did not cover

1. **Base branch.** The work is based on `main` at `24e1fc7b1` (spec 497). That
   commit has the swap pallet with the balancer math, and the gamma push model
   (`spend_gamma`, `Services`, the root TAO/USD oracle).
2. **Balancer reuse.** The pool calls `pallet_subtensor_swap::balancer::Balancer` and
   adds a check that a swap never lowers the product of the reserves.
3. **Liquidity is two-sided.** A provider adds both assets in the pool's ratio. A
   one-sided deposit is a swap and a deposit, and it would need its own limits.
4. **The first deposit** must be within 2% of the oracle price.
5. **Limits as fractions.** 2% for one swap, 20% for each direction in 300 blocks.
6. **Credits use the lower of the oracle price and the pool's time-weighted price.**
   The period is 300 blocks.
7. **tUSD minimum balance:** 0.01 tUSD.
8. **Index rule:** the best 8 by dividend of the last epoch, equal parts, each at most
   25%, refreshed once per childkey cooldown, closed at dissolution.
9. **One vault account for each subnet.** It owns the index stake and pays for stakes
   and credits. Staking keeps the existential deposit in the paying account, so the
   first order of a subnet uses that much less, one time.
10. **One market for each netuid for ever** in version 1.
11. **Virtual shares** of 0.001 against the first-depositor attack.
12. **Typed orders, strict order nonces on the hub, once-only answers on the spoke.**
13. **The outbox is pulled.** The gateway sends the hub's messages when it is asked
    to. `flush` and `send` are open to all. Each bridge has its own queue.
14. **Refunded, rejected, skipped.** An order that claims to have burned more than
    the hub let the spoke mint is rejected with no refund: it shows a broken spoke.
    Root can skip an order that cannot be decoded, so that later orders can settle.
15. **The precompile is at `0x815`** and emits no logs. The gateway contract emits
    the events.
16. **The vault on the spoke is the entry point and cannot change.** A payment that
    is larger than the vault's allowance is paid in parts (decision 26).
17. **Holders sell at the token.** No approval is given to the upgradeable portal.
18. **The portal creates the alpha tokens** at its own CREATE2 address. If the hub's
    registry lists another address, the portal reports the mismatch and does not
    fail.
19. **The alpha token has no permit.** Canonical TAO has.
20. **Unpause is the owner's alone**, through the timelock, on each side.
21. **No restricted proxy type** can make the new calls.
22. **The swap page** reads its addresses from a config file. It is not in the site's
    header until the rails are live. In local mode it has a faucet and a test
    account.
23. **Each payment pays for its message, and the hub sets the TAO for a message aside
    before it takes the order.** An order pays an answer fee on the spoke. An exit
    pays the reserve on the hub. Each bridge has its own account on the hub, which
    always holds the reserve for each message that has not left. The hub pays the transport's
    charge from it. Without this, refunds could use up the TAO that pays for the
    answers of other payers and other bridges, and a settled order could wait for
    someone to pay for its answer.
24. **Nothing is ordered for the zero address.** The portal refuses, and the hub
    refunds.
25. **The stablecoin minimum counts whole units** of the stablecoin, not the hub's 9
    decimals.
26. **USDC is paid in parts when the vault's limit is reached.** The portal keeps
    what it owes each address, and anyone can have it paid as the allowance refills.
    The other choice was to refuse a deposit that could later exceed the limit. But
    the hub cannot know at deposit time what a later sale will pay.
27. **The answer fee has no default at deployment.** The deployer must decide it.
28. **No contract catches a failed call.** The first build sent the answers in the
    order's transaction, and caught a failed hand-over so that the order still
    settled. But a transaction with too little gas for the hand-over then succeeds,
    and a relayer that estimates gas sends such transactions. See decision 33 for
    what the gateway does now.
29. **The gateway holds no TAO.** The second build kept the TAO for messages in the
    gateway's account. A new gateway would then start with none of it. Now accounts
    of the hub hold it, and the hub hands the gateway what it pays the transport,
    message by message. A change of the gateway moves nothing. The gateway's account
    must hold the existential deposit before root names it.
30. **The hub pays at most the ceiling for a message, and above the reserve only from
    spare TAO.** Each message has its reserve. If the transport charges more, the hub
    adds the difference from what the bridge's account holds beyond the reserves of
    the messages that have not left, up to the ceiling. So messages that a higher
    charge holds up leave while the account has TAO to spare, and a wrong price from
    the transport cannot take more than the ceiling for a message, nor the reserve of
    another message. Above that, whoever asks the gateway to send the message pays
    the difference, or the message is deferred (decision 34).
31. **An exit pays the reserve, not the charge.** The hub cannot know the charge when
    the exit is made. What is not used stays in the account of the bridge.
32. **The route of a bridge is fixed.** The first build let root change the portal of
    a bridge. But the stablecoin behind the bridge's tUSD is in the vault of the old
    portal, and the answers in the bridge's queue are for the old portal. So a bridge
    keeps its route for ever. A new portal is a new bridge.
33. **Orders and answers do not share a transaction.** Earlier builds sent the
    answers of a bridge in the transaction that settled an order of that bridge. Then
    a transport that failed on the way out also failed the order. Now `handle` only
    settles, and a keeper calls `flush`. The cost is that an answer needs a keeper.
    Exits and token registrations start on Bittensor and always needed one. Anyone
    can be a keeper: `btcli bridge flush` is one call.
34. **A message that is not paid for is deferred, not held.** Earlier builds kept such
    a message at the head of its queue, and the messages behind it waited. Now it
    leaves the queue and keeps its reserve, and anyone can send it later. Only the
    gateway defers, and only a message that the hub and the caller do not pay for. A
    message that the transport refuses is not deferred by the gateway: the gateway
    would have to catch the failure (decision 28). Root can defer it.
35. **The spoke takes the hub's messages in any order.** The transport gives no order,
    and a deferred message can arrive late. Each message is carried out once. An
    alpha token keeps the rate of the newest message that reported one, and counts
    new tokens at the rate that came with them.
36. **Deferred messages fill no queue, and a congested bridge refunds.** An earlier
    build counted deferred messages against the queue's 1,024 places. Then a backlog
    that nobody paid for could keep every new order out, with its funds locked on the
    spoke. Now only the messages in the queue count. A deferred message is bounded by
    its reserve: the account of the bridge holds one for each. From 256 deferred
    messages on, the hub adds no payment to the backlog. It takes an order and
    refunds it, so the payer has a message that returns the funds, and can pay for it
    to be sent. It refuses an exit, which costs the account nothing. The number 256 is
    a quarter of the queue.
37. **Root defers a message that the transport refuses. Nobody else can.** A caller
    of the gateway cannot be given this, because a caller chooses the gas, and with
    too little gas any hand-over fails. So the state is watched: a keeper raises an
    alert, and root acts.

## 9. Room for what is not built

- **`add_stake_for`.** An order would name a hotkey and a coldkey on Bittensor, and the
  hub would stake for that coldkey and send no answer. The order has a version
  field, so a second layout can carry two accounts. The staking interface
  (`RailsStaking`) gets one more function. Nothing else changes.
- **More spokes.** A spoke is a bridge id, a portal and a vault. The hub's limits are
  per bridge already.
- **A validator basket token and a gamma token.** Each is one more asset kind in the
  order and in the registry.

## 10. Run it

```bash
just rails-up        # the localnet, anvil as Base, Hyperlane, contracts, configuration
just rails-e2e       # the rails end to end, then every btcli bridge and pool command
just rails-page      # the swap page at http://localhost:3000/swap
just rails-down      # stop
just rails-contracts # the contracts' unit, fuzz and invariant tests
```

## 11. Before mainnet

- Measure the weights of the three pallets on reference hardware. The weights in this
  repository are placeholders.
- Raise `spec_version`.
- Decide the fee baselines for the new calls.
- Finish an external audit.
- Choose the validators of the security modules and deploy the modules.
- Deploy the timelocks, the gateway, the vault and the portal, and transfer nothing to
  a plain account.
- Set the caps low at the start. Enable the bridge, then the pool.
- For each bridge, set the reserve on the hub above the transport's charge for one
  message, the ceiling above the highest charge that the hub should pay without
  anyone's decision, and the answer fee on the spoke to at least the worth of the
  charge. Pay into the account of the bridge before the bridge is enabled: the
  reserves for the queue, and TAO to spare for charges above the reserve.
- Check the route of each bridge before it is added. It cannot change.
- Send the gateway's account the existential deposit before root names it as the
  gateway.
- Run a keeper and the oracle feed. Without a keeper no message leaves the hub: the
  keeper calls `Gateway.flush` when messages wait, and `Gateway.send` for a deferred
  message when the hub pays for it. Watch what the keeper reports: deferred messages,
  and a message that the gateway cannot hand over. Section 11 of `AUDIT.md` says who
  acts on each. Pay the answer fees into the account of their bridge
  (`Rails.fund_messages`). Do not send TAO to the gateway contract: it pays for no
  message.
