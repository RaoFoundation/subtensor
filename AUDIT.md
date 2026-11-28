# Canonical rails: audit guide

This file is for the auditor of the canonical rails. It lists what to review, whom the
system trusts, what each privileged party can do, what must always be true, and the
risks that are known and accepted.

The design is in [`docs/design/canonical-rails.md`](docs/design/canonical-rails.md). Read
it first. This file uses its words: hub, spoke, portal, vault, gateway, bridge, order,
answer.

## 1. Scope

| Part | Files | Lines |
| --- | --- | --- |
| tUSD | `pallets/tusd/src/lib.rs` | 470 |
| Canonical pool | `pallets/canonical-pool/src/{lib,math}.rs` | 1,010 |
| Hub | `pallets/rails/src/{lib,router,types}.rs` | 1,980 |
| Index hotkey | `pallets/subtensor/src/staking/index_hotkey.rs` | 230 |
| Staking for the hub | `pallets/subtensor/src/staking/rails.rs` | 120 |
| Credits for a bridged payer | `do_spend_tao_at_service` in `pallets/subtensor/src/subnets/gamma.rs` | 50 |
| Shared types | `common/src/rails.rs` | 385 |
| Precompile and wire format | `precompiles/src/{rails,rails_wire}.rs` | 1,660 |
| Runtime wiring | `runtime/src/lib.rs`, `runtime/src/proxy_filters/` | small |
| Contracts | `contracts/evm/src/**` | 1,300 |

The counts include comments and, for the wire format, its unit tests.

Changes to existing code are small. Review them as a diff against `main`:

- `pallets/subtensor`: the index hotkey's exceptions in the childkey, take, registration
  and hotkey-swap paths; the credits path; the cleanup at dissolution; a refresh of one
  index in `on_idle`.
- `pallets/swap`: one line. The `balancer` module is public, so that the pool can call it.
- `pallets/admin-utils`: one new variant of the precompile enum.
- `runtime` and `precompiles`: the wiring, the proxy call groups and the registry entry.

Not in scope: the swap page, btcli, the local rig, Hyperlane itself, OpenZeppelin, the
swap pallet's balancer math, and the gamma pallet code that exists on `main`.

## 2. The system in short

```text
 spoke (Base)                                hub (Bittensor)
 ------------                                ---------------
 payer -> Vault.deposit --+
 payer -> Token.sell -----+-> Portal --[transport]--> Gateway -> precompile 0x815
                                                                    |
                                               pallet-rails  <------+
                                               |  tusd: mint and burn per bridge
                                               |  canonical-pool: tUSD <-> TAO
                                               |  subtensor: stake at the index hotkey,
                                               |             or burn alpha for credits
                                               v
 Vault.release  <--+                         one queue of answers for each bridge
 Token.mint  <-----+-- Portal <--[transport]-- Gateway.flush
```

Value moves on one route: stablecoin, tUSD, TAO, alpha, credits. Each order is settled
in one extrinsic, or it is refunded in full.

## 3. Trust assumptions

Each line is something that the system assumes. If it is false, section 5 says how
much can be lost.

1. **The transport's security module is honest in both directions.** A message that
   the module accepts is taken as authentic. On Bittensor the gateway names its module.
   On the spoke the portal names its module. With Hyperlane's multisig module, this is
   the honesty of a threshold of the validators that the owner chose.
2. **Root on Bittensor is honest.** Root is the admin origin of the three pallets. Root
   can also upgrade the runtime, so this adds no trust to what Bittensor has already.
3. **The owner of the contracts is honest, or is seen in time.** The owner is a
   timelock of 48 hours behind a multisig. Every change is public for 48 hours before
   it takes effect.
4. **The guardians act in time.** A guardian can only pause. If no guardian acts, the
   limits of section 5 still hold.
5. **The stablecoin is worth a dollar and its contract is honest.** tUSD is one for one
   with the units in the vault. The rails do not insure a depeg, a freeze or a
   blacklist by the issuer.
6. **The root TAO/USD oracle is honest and live.** It is the same feed that gamma uses
   on `main`. It prices credits and the pool's first deposit. Section 6.4 bounds what
   a wrong feed can do.
7. **Bittensor's EVM runs the precompile as written**, and the spoke runs the EVM as
   written. The contracts need the Cancun opcodes.
8. **Someone keeps the transport running.** A relayer delivers messages. A keeper
   asks the gateway to hand the hub's messages to the transport: no message leaves
   the hub without that call. An operator pays the answer fees into the accounts of
   the bridges on the hub. All three are needed for liveness only, not for safety.
   Anyone can be a relayer or a keeper.

The hub does not trust the spoke's accounting. The hub keeps its own count of every
canonical token that it let each spoke mint, and takes a burn only up to that count.

## 4. Privileged parties

### 4.1 On Bittensor

| Party | Can | Cannot |
| --- | --- | --- |
| Root | Add a bridge with its route, and change its mint cap and mint window (`Tusd.set_bridge`). Enable a bridge. Set the gateway account and the guardian. Enable the pool. Set the pool's fee from 0.05% to 1%, and its swap limits. Open and list markets, set the caps. Set the message reserve and ceiling of a bridge. Register and correct registry entries. Skip an order. Defer the message at the head of a queue. Pause and resume. | Mint tUSD without a bridge. Move user balances by a call of these pallets. Change the route of a bridge: its chain, transport domain, portal and stablecoin are fixed when it is added. Remove a message of the hub: a deferred message stays in the hub until the gateway sends it. |
| Guardian (one account, named by root) | Pause the hub. Disable a bridge. Disable the pool. | Resume anything. Change any setting. Move funds. |
| Gateway account (the mapped account of the gateway contract) | Call `Rails.execute`, `Rails.ack_outbound` and `Rails.defer_outbound`. Take a message out of the hub: the head of a queue, or a deferred message. Say what the transport charges for it, and receive that much from the account of the message's bridge. Defer the head of a queue. | Choose the bridge of an order: the hub derives it from the transport's origin and sender. Settle an order out of sequence, or a second time. Receive more for a message than the hub's limit: the reserve of the bridge and what its account can spare, up to the ceiling. Receive anything without taking a message out of the hub. Take a message that is behind the head of its queue and not deferred. |
| TAO/USD oracle (one account, named by root) | Feed the TAO price. | Raise the accepted price faster than about 2% per hour. |
| Anyone | Swap, add and remove liquidity in the pool. Transfer tUSD. Pay through `Rails.bridge_out`. Pay TAO into the message account of a bridge (`Rails.fund_messages`). | Take TAO out of such an account. |

Root adding a bridge with a hostile portal is the same power as minting tUSD up to
that bridge's cap. The cap and the window are the bound.

### 4.2 On Bittensor's EVM: `Gateway`

| Party | Can | Cannot |
| --- | --- | --- |
| Owner (timelock) | Upgrade the contract. Set the security module and the guardian. Resume. | Act in less than 48 hours. |
| Guardian | Pause. | Resume. Change a setting. |
| The transport's Mailbox | Deliver a message to `handle`. | Have the gateway hand a message to the transport: `handle` only settles the order. |
| Anyone | Call `flush`: hand to the transport what the hub queued for one bridge. Call `send`: hand over one deferred message. With both, pay what the transport charges above what the hub pays. Send TAO to the contract: it pays for no message. | Change what is sent, or where. Spend the hub's TAO beyond the hub's limit for a message. Defer a message that the hub pays for. Remove a message. |

An upgrade of the gateway can call `Rails.execute` with any order from a registered
portal's address. This is the largest power in the system on the hub side. Its bounds
are the hub's caps (section 5).

The gateway holds no TAO between transactions. The TAO that pays the transport is in
the hub's accounts, one for each bridge. An upgraded gateway could ask the hub for its
limit for each message that has not left and keep the TAO. The bound is one ceiling
for each such message, and never more than the accounts hold. That TAO pays for
messages. It is not payers' money. An upgraded gateway could also take a message out
of the hub and not hand it to the transport. Then the payment that the message
orders does not arrive on the spoke until the owner corrects it. What backs that
payment stays where it is: the stablecoin in the vault, the TAO in the hub's reserve,
the stake in the hub's vault.

### 4.3 On a spoke: `Portal`, `Vault`, tokens

| Party | Can | Cannot |
| --- | --- | --- |
| Portal owner (timelock) | Upgrade the portal. Set the gateway address, the security module, the guardian, the answer fee and its recipient. Resume. | Act in less than 48 hours. |
| Fee recipient | Receive the answer fees when anyone calls `sweepFees`. | Anything else. |
| Portal guardian | Pause the portal: no new orders and no answers. | Resume. |
| Vault owner (timelock, fixed at deployment) | Set the release limit and the guardian. Resume releases. | Upgrade the vault. Withdraw. Change the portal or the stablecoin. |
| Vault guardian | Stop releases. | Resume. Stop deposits: these follow the portal's pause. |
| Portal (as a contract) | Release from the vault inside the rate limit. Mint the canonical tokens. Create alpha tokens. | Take tokens from a holder. Move a payer's stablecoin: approvals are to the vault, which pulls only in `deposit`, from the caller. |
| Token holder | Transfer. Sell own tokens (`sell`). | |
| Anyone | Call `Portal.claim(address)`: have the vault release, inside its limit, what the portal owes that address. Call `sweepFees`. | Direct a payment to another address. |

The vault and the tokens are not upgradeable and have no owner functions beyond the
table.

An upgrade of the portal can mint canonical tokens on the spoke without limit, and can
release the vault's stablecoin at the rate limit. Tokens that the hub did not count
cannot be redeemed at the hub (invariant I5), so they are worthless there. They could
still be sold to third parties on the spoke before the fraud is seen. This is why the
owner is a timelock and why the guardian exists.

## 5. What a failure can cost

| Failure | Worst case | Bound |
| --- | --- | --- |
| The security module on Bittensor accepts forged orders from a real portal's address | tUSD minted without backing, then sold in the pool or bridged out | The bridge's mint cap and mint window. The pool's 2% swap limit and 20% flow limit. The guardian can disable the bridge and the pool. |
| The security module on the spoke accepts forged answers | Stablecoin released from the vault. Canonical tokens minted on the spoke. Forged debts on record in the portal | The vault's release limit per period, then the guardian's pause. Tokens that the hub did not count do not redeem. The owner clears forged debts by an upgrade before the vault resumes (risk 10). |
| The gateway is upgraded with hostile code | As forged orders | As above, after 48 hours of notice |
| The portal is upgraded with hostile code | As forged answers | As above, after 48 hours of notice |
| One bridge is compromised in any way | That bridge's cap of tUSD. That bridge's vault | Other bridges: a bridge burns only its own outstanding tUSD, so it cannot release another vault's stablecoin |
| The oracle feeds a wrong TAO price | Credits are valued wrong | The lower of the feed and the pool's time-weighted price counts. The feed can rise only about 2% per hour. Too few credits: the buyer's minimum refunds the order |
| A subnet's pool is pushed up before a credits order | Fewer credits for the buyer | Alpha counts at the lower of the slow price and the spot price. The buyer's minimum refunds the order |
| The canonical pool is pushed before an order | A worse price for the payer | The order's minimum. The swap limits make a large push take many blocks |
| A person sends many orders that must be refunded, or many small exits | The message account of that bridge pays the transport for their answers | Each order pays the answer fee on the spoke, and each exit pays the reserve on the hub. The hub takes an order only if the account holds the reserve for its answer. An order that is not taken waits at the transport, and its funds stay locked on the spoke. No other bridge is touched |
| The message account of a bridge runs low | That bridge takes no new order. Every message that the hub holds already can still leave: its reserve is in the account | Anyone pays in (`Rails.fund_messages`). The other bridges go on |
| The transport charges more for a message than the reserve of its bridge | The hub pays the difference from what the bridge's account can spare, up to the ceiling. If the account has nothing to spare, or the charge is above the ceiling, the message is deferred: it stays in the hub with its reserve, and the messages behind it go on. The bridge's orders still settle while the account holds reserves for their answers and fewer than 1,024 of its messages have not left | Anyone pays into the account (`Rails.fund_messages`), and a keeper sends the deferred messages. Anyone calls `Gateway.send` and pays the difference. Root raises the ceiling. The hub never pays more than the ceiling for one message, and never uses the reserve of another message |
| The transport refuses the messages to a spoke, or fails on the way out | The messages of that bridge wait in the hub. Its orders still settle, until 1,024 of its messages have not left. Then its orders wait at the transport, and their funds stay locked on the spoke | No order is undone: an order and its answer never share a transaction. The messages to other spokes are in other queues. Root can defer a message that the transport does not take, so that the ones behind it go on |
| No keeper runs | No message leaves the hub: answers, exits and token registrations wait | The hub holds each message with its reserve. Anyone can be the keeper: `Gateway.flush` has no access control, and `btcli bridge flush` calls it |
| Root names a new gateway account | Nothing | The TAO for messages is in the hub's accounts. A new gateway is paid from them as the old one was |
| A spoke is to move to a new portal | Nothing | The route of a bridge cannot change, so its tUSD stays backed by the vault of its portal and its queued answers go to that portal. The new portal is a new bridge with its own cap |
| A sale or a refund pays more USDC than the vault's allowance | The payee gets a part now and waits for the rest | The portal records the debt and pays it in parts (`Portal.claim`). The release limit still bounds what a forged answer can take in a period |

## 6. Invariants

Each invariant names where the code keeps it and where a test checks it.

### 6.1 tUSD (`pallets/tusd`)

- **I1.** Total issuance equals the sum of balances and equals the sum of what the
  bridges have outstanding. `do_try_state`; unit tests after each operation.
- **I2.** A mint never takes a bridge above its mint cap, nor above what its window
  allows. `Pallet::mint`.
- **I3.** A bridge burns at most what it has outstanding. `Pallet::burn`.
- **I4.** An account holds zero or at least the minimum balance. `credit`, `debit`.
- **I34.** The route of a bridge never changes after the bridge is added
  (`set_bridge`). So what a bridge has outstanding stays backed by the vault of its
  portal, and the answers in its queue go to that portal. Test
  `the_route_of_a_bridge_is_set_once`.

Across the chains: the vault of a bridge holds at least that bridge's outstanding
tUSD, in the stablecoin's units. The hub mints only for a deposit that the portal
reported, and tells the vault to release only what it burned. Checked end to end by
`scripts/rails/e2e.py` and by the invariant test
`invariant_TheVaultCoversEveryDollarOwed`.

### 6.2 Canonical pool (`pallets/canonical-pool`)

- **I5a.** The pool account holds at least the recorded reserves. `do_try_state`.
- **I6.** A swap never lowers the product of the reserves. Checked in the swap itself
  after the balancer computes the amounts.
- **I7.** Adding or removing liquidity never lowers either reserve per share. Share
  math rounds against the provider.
- **I8.** After the first deposit, both reserves stay at or above the minimum reserve
  and the share supply stays above zero.

### 6.3 Hub (`pallets/rails`)

- **I9.** The reserve account holds at least the sum of canonical TAO that the hub
  let all spokes mint. `do_try_state`.
- **I10.** The shares of a market equal the sum of the alpha tokens that the hub let
  the spokes mint for it. `do_try_state`.
- **I5.** The hub takes a burn of a canonical token only up to the supply that it
  counted for that spoke (`ensure_backed`, `collect`, `redeem_shares`). An order that
  claims more is rejected and is not refunded.
- **I11.** Each order nonce of a bridge is settled at most once, in sequence
  (`NextOrder`). Each answer has a nonce that is issued once (`NextMessage`).
- **I12.** An order changes state only if its whole route succeeds. Otherwise the only
  changes are the nonce, the receipt and one refund answer (`settle`, a storage
  layer).
- **I13.** An order leaves nothing in the hub account. Every step moves an amount that
  it was given, never a balance that it read. Unit tests assert an empty hub account
  after each order. This is not in `try_state`, because anyone can send funds to
  that account.
- **I29.** The message account of a bridge holds, on top of the existential deposit, at
  least the bridge's reserve for each message of the bridge that has not left: in its
  queue or deferred. `do_try_state` checks it. Four places keep it: a message is
  queued only if it still holds (`ensure_room`), a message that leaves takes at most
  its reserve and what the account holds beyond the reserves of the messages that
  have not left (`message_limit`, `ack_outbound`), a message that is deferred moves no
  TAO (`defer_outbound`), and the reserve rises only if the account covers those
  messages at the new reserve (`set_message_reserve`). The hub checks it before it
  takes an order, because every order has an answer if it fails. Tests
  `a_bridges_account_holds_the_reserve_for_each_queued_message`,
  `a_dear_message_never_takes_the_reserve_of_another`,
  `a_deferred_message_keeps_its_reserve_and_the_ones_behind_it_leave` and
  `the_reserve_rises_only_if_the_account_covers_the_queue`.
- **I30.** TAO leaves the message account of a bridge only when the gateway takes a
  message of that bridge out of the hub: what the gateway says the transport charges,
  at most the bridge's ceiling (`ack_outbound`). So one bridge never spends what was
  paid in for another, and the hub pays a bounded price for each message. Tests
  `one_bridge_never_spends_what_was_paid_in_for_another`,
  `the_hub_pays_what_the_transport_charges_up_to_the_reserve`,
  `a_charge_above_the_reserve_is_paid_from_what_the_account_can_spare`,
  `a_new_gateway_is_paid_from_the_same_accounts` and
  `message_accounts_hold_under_random_operations`: 6,000 random steps on two bridges,
  with messages that are deferred and sent later, against a separate count of what
  each account can pay. The e2e checks on real chains that the account paid exactly
  what the transport charged, and that the gateway's balance did not change.
- **I35.** The hub never drops a message. A message leaves `Outbox` only in
  `ack_outbound`: in the transaction in which the gateway pays the transport and
  hands it the message. A deferred message stays in `Outbox`, and `Deferred` counts
  it. The gateway can take the head of a queue and the deferred messages, and nothing
  else. `do_try_state` checks that every message from the head of a queue on is
  there, that the messages below the head are as many as `Deferred` counts, and that
  a bridge has at most `MaxOutbox` messages in all. Tests
  `a_deferred_message_keeps_its_reserve_and_the_ones_behind_it_leave`,
  `the_admin_defers_a_message_that_the_transport_does_not_take`,
  `deferred_messages_count_as_waiting` and, through the precompile,
  `the_gateway_defers_a_message_and_sends_it_later`.
- **I14.** The alpha that a sale takes from a vault is never more than the vault
  holds, and both share formulas round against the trader (`alpha_for_shares`,
  `shares_for_alpha`).

### 6.4 Index hotkey and credits (`pallets/subtensor`)

- **I15.** An index hotkey has no key. Its account is derived from the pallet id, a
  tag and the netuid, and it is recognised without a storage read
  (`index_hotkey_netuid`).
- **I16.** Nobody can register an index hotkey, swap a hotkey to it, or set it as a
  child. Its take is zero whatever storage holds.
- **I17.** Only an index hotkey can have more than 5 children, and at most 8. Each
  child gets at most 25%.
- **I18.** The children of an index hotkey are a pure function of the subnet's last
  epoch: validator permits, dividends, childkey takes and parent counts.
- **I19.** The gamma of a bridged payment is never more than the dollars of TAO paid,
  at the lower of the oracle price and the price cap that the hub passes
  (`do_spend_tao_at_service`).

### 6.5 Contracts (`contracts/evm`)

- **I20.** The portal carries out each answer at most once (`_processed`), whatever
  the transport or its security module does.
- **I21.** The portal obeys only the Mailbox, and only messages whose origin is the
  hub's domain and whose sender is the gateway.
- **I22.** The portal holds no stablecoin and no tokens. Invariant test
  `invariant_ThePortalHoldsNothing`. It holds the answer fees, in the chain's coin,
  and they can leave only to the fee recipient.
- **I23.** Canonical tokens are minted only by the portal that created them. A token
  is burned only by its holder, in `sell`.
- **I24.** The vault releases only on the portal's call, at most its allowance, and
  never while paused. A deposit counts what arrived by the change of the vault's
  balance, and `deposit` and `release` cannot be entered again while one runs.
- **I31.** The portal owes each address the stablecoin that the hub told it to pay,
  less what the vault released to that address (`owed`). `totalOwed` is the sum. The
  portal lowers the debt before it calls the vault, and a release that fails reverts
  the whole call (`_pay`). The vault holds at least what is owed. Invariant tests
  `invariant_WhatIsOwedIsOwedToSomeone` and `invariant_TheVaultCoversEveryDollarOwed`,
  and the fuzz test `testFuzz_APaymentInPartsAddsUpToThePayment`.
- **I32.** For each message, the gateway reads the transport's charge, has the hub
  hand it that charge up to the hub's limit, takes the rest from the caller, and pays
  the transport, in one transaction (`_charge`, `_handOver`). It spends nothing else:
  it ends each transaction with what it began with, and TAO that is sent to it stays
  there. (The hub rounds a charge up to its own unit, so less than one rao for a
  message can stay in the gateway: risk 8.) If the hub and the caller do not cover
  the charge, nothing is paid: `flush` defers the message, and `send` reverts. Fuzz
  test `testFuzz_TheGatewayPaysOnExactlyWhatItIsHanded`, and
  `test_ACallerThatComesBackInWithItsChangeSpendsOnlyItsOwn` for a caller that
  enters the gateway again when its change arrives.
- **I33.** No contract catches a failed call. A transaction that has too little gas,
  or whose payment is refused, fails as a whole. So a caller that estimates gas gets
  a true estimate, no order or payment is half done, and no caller can make a step
  fail by its choice of gas. Where a step may wait without a fault, the code decides
  it and records it: a message that is not paid for is deferred (`MessageDeferred`),
  and a payment above the vault's allowance leaves a debt (`owed`).
- **I36.** The gateway settles an order whatever the transport does on the way out.
  `handle` calls the hub's `execute` and makes no call to the transport. Tests
  `test_AnOrderSettlesAndItsAnswerWaitsForAFlush` and
  `test_AnOrderSettlesWhateverTheTransportDoesOnTheWayOut`.
- **I37.** The spoke takes the hub's messages in any order. Each is carried out once
  (I20). A payment of an alpha token reverts until the token exists, and the
  transport delivers it again. An alpha token keeps the rate of the newest message
  that reported one (`rateNonce`), and counts new tokens at the rate that came with
  them. Test `test_AnOlderReportThatArrivesLateDoesNotReplaceANewerOne`. The
  invariant tests deliver the hub's messages in a random order.
- **I25.** The supply of each canonical token equals what the hub counted, less burns
  that the hub has not read, plus mints that the portal has not carried out.
  Invariant test `invariant_TokensMatchTheHubsSupply`.
- **I27.** Nothing is ordered for the zero address. The portal refuses the order
  before it takes funds (`Wire.check`). The hub refuses it too and refunds the payer
  (`ensure_payable`).
- **I28.** A payout of the stablecoin is a whole number of the stablecoin's units, is
  not zero, and is at least the order's minimum (`payable_stable`).
- **I26.** An order has exactly one encoding. The hub decodes strictly: wrong length,
  a nonzero unused field or a value that does not fit its type is refused. Golden
  vectors are shared by `precompiles/src/rails_wire.rs` and
  `contracts/evm/test/Wire.t.sol`.

## 7. Known risks and accepted trade-offs

1. **The weights are not measured.** `weights.rs` of the three pallets holds
   placeholders. They must be measured on reference hardware before a release.
   `Rails.execute` adds the staking weight that `RailsStaking::weight` reports.
2. **Liveness depends on keepers.** A relayer must deliver messages. A keeper must call
   `Gateway.flush`: no message leaves the hub without that call, and that includes the
   answer to an order. Nobody is paid to be a keeper. The call costs gas only, anyone
   can make it, and `btcli bridge flush` does. Funds are safe while messages wait: the
   hub holds each message with its reserve. The answer fees collect on the spoke, in
   the spoke's coin. The operator has to move their value into the message account of
   the bridge on the hub. If the operator does not, or if a fee is set below the
   transport's charge, the account runs low. Then the bridge takes no new order until
   someone pays in. Orders that wait keep their funds locked on the spoke. Messages
   that the hub holds already can still leave. The gateway takes a queue in order. A
   message that the transport refuses holds the ones behind it in the same queue,
   until the transport takes it or root defers it. It holds no message of another
   bridge. A message that is not paid for holds nothing: it is deferred.
3. **Head-of-line blocking.** The hub takes the orders of a bridge in sequence. An order
   that the gateway cannot decode blocks the orders behind it until root skips it. The
   payer's funds of a skipped order stay on the spoke, and the owner has to return them
   by governance. A portal that follows `Wire.sol` cannot produce such an order.
4. **A payment of USDC can arrive in parts.** The vault releases a limited amount in a
   period. A payment above what is left of that amount is paid in part, and the rest is
   on record in the portal (`owed`). Someone must call `Portal.claim` for the rest,
   after the allowance refilled: the payee, or anyone else. The swap page and
   `btcli bridge claim` do this. Set the limit above the largest sale that should pass in one
   payment.
5. **Refunds use the release limit too.** Many large refunds can use the allowance that
   sales need. Then sales are paid in parts. While debts are on record, the allowance
   goes to the payee whose `claim` comes first: there is no queue. All debts are paid in
   the end, at the rate of the limit. A release period of zero turns the limit into a
   cap for each release, with no limit on the rate: do not set it.
6. **A recipient that cannot receive.** The zero address is refused. Other addresses are
   not checked. If the stablecoin's issuer blocks the payee, the release reverts each
   time the transport tries the answer, and `claim` reverts for a debt that is on
   record. Other payments go on: the portal carries answers out in any order. The hub
   has settled the order, so the stablecoin stays in the vault until the block ends or
   the owner acts.
7. **The reserve and the ceiling are root's to set.** With a reserve below the
   transport's charge, every message needs TAO to spare from the account of its bridge.
   With a ceiling below the charge, the messages of the bridge are deferred until
   someone pays the difference with `Gateway.send`, the charge falls, or root raises
   the ceiling. With a reserve far above the charge, the account must hold more TAO for
   the same messages, and an exit pays more.
8. **TAO that is sent to the gateway contract stays there.** It pays for no message, and
   it cannot be taken out in version 1. The same holds for what is left in the message
   account of a bridge that is retired. Pay in only with `Rails.fund_messages`. The
   message account of each bridge and the gateway's account keep the chain's existential
   deposit, 500 rao. The hub counts in rao and rounds a charge up, and the EVM pays in
   wei: a charge that is not a whole number of rao leaves less than one rao in the
   gateway.
9. **The gateway reports the charge.** The hub hands the gateway the amount that the
   gateway contract names, up to the hub's limit for the message. The contract reads it
   from the transport in the same call in which it pays. A gateway that is upgraded with
   wrong code could name the limit for each message that has not left and keep the
   difference. The bound is one ceiling for each such message, and never more than the
   accounts of the bridges hold.
10. **A debt on record outlives a pause.** If the security module on a spoke accepts a
    forged answer, the portal can record a debt to the forger that is larger than the
    vault's allowance. The vault's limit still bounds what leaves in a period, and the
    guardian can stop the releases. But the debt stays on record, and `claim` would pay
    it when the releases resume. The portal has no call that removes a debt. So before
    the owner resumes the vault after such a fault, the owner must upgrade the portal to
    clear the forged debts. Both steps go through the 48-hour timelock.
11. **A bridge can have at most 1,024 messages that have not left**, in its queue or
    deferred. A person can reach that number with small exits. Each exit pays the
    reserve for its own message. At that number the bridge takes no order and no exit.
    Anyone hands the queue to the transport with `Gateway.flush`, for gas only.
    Deferred messages count too: if nobody pays what the transport charges for 1,024
    messages of a bridge, the bridge takes no order until someone does (risk 7). The
    messages of other bridges are not touched.
12. **An exit pays the reserve, not the charge.** The hub cannot know what the transport
    will charge when the exit is made. An exit pays the reserve of its bridge into the
    bridge's message account. What the transport does not charge stays there and pays
    for later messages. It does not go back to the account that exits.
13. **The route of a bridge is fixed.** A move to a new portal, or to a transport that
    names the spoke with another domain, needs a new bridge with a new id. The old
    bridge then winds down by its own means: while it is enabled, its tokens sell and
    its tUSD redeems through the old portal. Nothing moves what one bridge owes to
    another bridge. A route that is wrong when the bridge is added stays wrong: add the
    bridge again under another id, and never enable the first.
14. **Sandwiching.** An order settles at the hub's prices when it arrives, some blocks
    after the payer signed. The order's minimum bounds the loss. btcli defaults to 5%
    and the swap page to 1%. The pool's swap limits make a large push slow.
15. **Credits are final.** They cannot be sold, moved or refunded. They belong to one
    address at one service on one subnet. A subnet that is dissolved clears them.
16. **Credits can be fewer than the dollars paid.** They count the alpha at the lower of
    the slow price and the spot price, after two pool fees. The quote shows the number
    before the payer signs.
17. **The time-weighted price needs history.** Credits orders are refunded until the
    pool has 300 blocks of history, and whenever the oracle feed is older than 300
    blocks.
18. **The time-weighted price can be moved by a party that holds the pool's price for a
    long time.** The party pays the pool's fee and the arbitrage against it for the
    whole period. Only a lower price matters, and a lower price gives fewer credits, not
    more.
19. **An index hotkey earns on less than all of its stake if fewer than 4 validators
    qualify.** Each child gets at most 25%.
20. **The index rule can be gamed at its edges.** A validator earns a place among the 8
    by its dividend of the last epoch. A validator at the edge can change its behaviour
    around the refresh. The place moves stake weight, never the stake.
21. **A child keeps its childkey take** of what it earns on the index's stake. The rule
    skips children whose take is above the highest delegate take.
22. **A market is for ever.** A netuid has one market. After the subnet is dissolved the
    token only redeems TAO. If the netuid is registered again, the new subnet cannot
    have a token in version 1.
23. **Dissolution pays in TAO at the chain's terms.** Holders of the alpha token share
    what the vault received. They do not choose the time.
24. **Virtual shares keep a trace of the earnings.** `V / (shares + V)` of a vault's
    earnings stay in the vault without an owner. For a vault of 1,000 alpha that is one
    millionth.
25. **Dust.** The vault account keeps the existential deposit from its first stake. The
    stablecoin has 6 decimals and the hub 9: the part of a release below one millionth
    of a dollar stays in the vault. A sale of canonical TAO that would leave the reserve
    account between zero and the existential deposit is refunded.
26. **Gifts.** Anyone can send TAO to the hub account, the reserve account or a vault
    account, and can transfer stake to a vault. No order reads those balances, except
    two: the reserve check (a gift only helps) and the wind-down of a dissolved market
    (a gift goes to the holders). Stake that is given to a vault raises the rate for all
    holders and counts toward the market's cap, so a gift can make purchases fail until
    root raises the cap.
27. **A wrong address in the registry.** Root passes the token's address when it lists a
    market. The portal creates the token at its own CREATE2 address in any case and
    emits `RegistryMismatch` if the two differ. Root corrects the registry with
    `Rails.register_token`. Until then the registry names a wrong address.
28. **Token names are fixed at creation.** A subnet that changes its name keeps the old
    token name and symbol. Two subnets can have the same symbol: the registry and the
    netuid tell them apart.
29. **Unpause is slow by design.** Only the owner resumes a contract, through the
    48-hour timelock.
30. **The rig is not a security model.** It uses one validator, a threshold of one, a
    timelock of zero seconds and public development keys.
31. **The fee baselines of the runtime do not cover the new calls**, and `spec_version`
    is not raised in this change. Both are release steps.
32. **MEV shield.** btcli sends `swap_pool` and `bridge_out` through the MEV shield by
    default. A payment from a spoke cannot be shielded: it is public on the spoke before
    it arrives.
33. **A deferred message arrives late.** Its payee waits until the hub pays more, the
    charge falls, or someone pays the difference. The payee can pay it: `Gateway.send`
    is open to all. The messages arrive in another order than the hub made them. The
    spoke is built for that (I37).
34. **Anyone can have an unpaid message deferred.** `Gateway.flush` defers the head of
    a queue if the transport's charge is above what the hub pays at that moment and the
    caller adds nothing. So a short rise of the charge can leave messages deferred. They
    lose their place in line and nothing else: a keeper sends each one as soon as the
    hub pays for it. A message that the hub pays for cannot be deferred through the
    gateway.
35. **A new gateway address strands the messages in flight.** A portal obeys only the
    gateway address that it knows (I21). A message that the old gateway handed to the
    transport, and that has not arrived when the portal's owner names a new gateway,
    is refused on arrival. The hub has already let it go. So upgrade the gateway in
    place. If a new address cannot be avoided: pause the hub, wait until the transport
    has delivered every message, and change the address on both sides after that.
36. **An order that the hub does not take waits at the transport.** That is the case
    while the hub is paused, while the order's bridge is disabled, and while the
    bridge's account cannot pay for one more message. The payer's funds stay locked on
    the spoke for that time. The hub does not refund such an order, because it does
    not know whether the portal is at fault. If a bridge is never enabled again, the
    owner of its portal has to return those funds by an upgrade.

## 8. Tests

| What | Where | Run |
| --- | --- | --- |
| tUSD: 18 unit tests, benchmarks as tests | `pallets/tusd/src/tests.rs` | `cargo test -p pallet-tusd --all-features` |
| Pool: 34 unit tests, benchmarks as tests | `pallets/canonical-pool/src/tests.rs` | `cargo test -p pallet-canonical-pool --all-features` |
| Hub: 62 unit tests, 15 benchmarks as tests | `pallets/rails/src/tests.rs` | `cargo test -p pallet-rails --all-features` |
| Index hotkey: 22 tests on the real pallet | `pallets/subtensor/src/tests/index_hotkey.rs` | `cargo test -p pallet-subtensor --all-features index_hotkey` |
| Credits for a bridged payer: 7 tests | `pallets/subtensor/src/tests/gamma_credits.rs` | `cargo test -p pallet-subtensor --all-features gamma_credits` |
| Staking for the hub, with a real dissolution: 8 tests | `pallets/subtensor/src/tests/rails_staking.rs` | `cargo test -p pallet-subtensor --all-features rails_staking` |
| The production runtime through the precompile: 5 tests | `runtime/tests/rails.rs` | `cargo test -p node-subtensor-runtime --all-features --test rails` |
| Precompile and wire format, with golden vectors | `precompiles/src/{rails,rails_wire}.rs` | `cargo test -p subtensor-precompiles --all-features rails` |
| Contracts: 96 tests. Unit, fuzz (512 runs) and 5 invariants (64 runs of 128 calls) | `contracts/evm/test` | `just rails-contracts` |
| Both chains and the transport, end to end | `scripts/rails/e2e.py`, `scripts/rails/e2e_cli.py` | `just rails-up && just rails-e2e` |

The unit tests of the hub call `try_state` of the three pallets after each operation.

## 9. Static analysis of the contracts

`forge lint` passes with four lints excluded in `contracts/evm/foundry.toml`. Each was
reviewed:

- `reentrancy-events`: the external calls go to the transport's Mailbox, to the hub's
  precompile, or to this system's own vault and tokens. State is written before the
  calls, and answers are marked as carried out first.
- `calls-loop`: `Gateway.flush` takes a bounded number of messages.
- `arbitrary-send-eth`: the transport's fee goes to the Mailbox and the change goes
  back to the payer.
- `missing-zero-check`: zero has a meaning for a guardian (none) and for a security
  module (the transport's default).

## 10. Questions that deserve the auditor's time

1. Can any sequence of orders, refunds and reordered answers make a spoke's tokens or
   a vault's releases differ from the hub's counts (I5, I25)?
2. Is every failure after `NextOrder` moves either a full rollback with a refund, or a
   revert of the whole call (I12)? Look at `settle`, `enqueue` and a full outbox.
3. Does any exemption of the index hotkey reach an ordinary hotkey (I16, I17)?
4. Is the share arithmetic safe at the extremes: an empty vault, a vault that lost
   alpha, a gift of stake, the first buyer (I14)?
5. Is the strict decoder in `rails_wire.rs` complete, and do the two sides agree on
   every byte (I26)?
6. What can an upgrade of the gateway or the portal do that sections 4 and 5 do not
   list?
7. Can a message of one bridge be paid for with the TAO of another, can TAO leave the
   message account of a bridge in any way other than `ack_outbound`, or can a queued
   message be left without its reserve (I29, I30)?
8. Can what the portal owes differ from what the hub ordered less what the vault
   released (I31)? Look at `Portal._pay`.
