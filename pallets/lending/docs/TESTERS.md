# Lending — tester notes (PR #3268)

Hands-on checklist for the escrowed-raise / lift lending surface. Spec detail
lives in [`lift-lending-spec.md`](./lift-lending-spec.md).

## Commands

```text
btcli lending open|close|repay|raise|list|show
```

- `show` / `quote` prints the position plus a close quote (payment, refund, shortfall).
- `repay` is the partial-repay path (`Lending.repay`); full close remains `close`.
- `list` defaults to the wallet coldkey; pass `--all` for every position on the subnet.

## Slippage and deadlines

- `--max-slippage` must be **> 0** (default 1%). Zero is rejected client-side: interest
  and emissions move price one block after the quote.
- Caps / floors (`max_collateral_alpha`, `min_borrow_tao`, `min_escrow_alpha`,
  `min_tao_out`, `min_proceeds`, `max_payment`, `min_refund`) are derived from the
  live quote and `--max-slippage`.
- Raise: omit `--valid-until` → current block + 50. SDK `RaiseLoan` with
  `valid_until=None` resolves the same way in `build()`.

## Hotkeys

Pass an explicit `--hotkey` (registered validator stake destination) whenever
alpha moves:

- long open (collateral source)
- long close (alpha refund)
- short `--repay-from-wallet` / partial repay from wallet
- raise (collateral + sale)

Short default close is TAO-only; surplus buyback dust is sold for TAO when the
wallet hotkey is unregistered.

## Underwater shorts

Default close tops up free TAO for the buyback shortfall (bounded by
`max_payment` / `--max-slippage`). The quote exposes `shortfall` (exact TAO
needed). If free balance is short, expect `InsufficientEscrow` and use the
quote's shortfall — or `--repay-from-wallet --hotkey …`.

## Fixed-term expiry

At `expires_at` the position is closed automatically at spot through the pool:
expiry == a default close, not an EMA settlement. There is no wallet top-up and
no slippage limit (MEV at expiry is accepted), and the owner's free TAO is never
touched. Alpha a close would return (long collateral left after covering `P`,
short buyback dust) is sold at spot and paid as TAO because expiry has no hotkey.
An underwater short or insolvent long reuses the collateral-exhaustion
`Lending.Forfeited` path instead of recording a shortfall: lift + escrowed
proceeds are re-added (weight-preserving), remaining collateral is burned,
nothing returns to the borrower, and the k-neutral burn runs as usual. Solvent
positions still emit `Lending.Expired` (`rate` = spot before the closing trades).
Neither path loops in the retry queue.

## Cards and list columns

Dry-run shows the Loan / Repayment / Raise card (quoted payment, refund,
shortfall). List columns include status (underwater), escrow, expires_at, due,
and runway capped at the one-year term. JSON amounts are rao integers.

## Intentionally unchanged

Short interest still uses `min(reference, spot)` (E2E F16 — design question).
