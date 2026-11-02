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
needed). If free balance is short, expect `CloseTopUpNeeded` with that number
via the quote — or use `--repay-from-wallet --hotkey …`.

## Cards and list columns

Dry-run shows the Loan / Repayment / Raise card (quoted payment, refund,
shortfall). List columns include status (underwater), escrow, expires_at, due,
and runway capped at the one-year term. JSON amounts are rao integers.

## Intentionally unchanged

Short interest still uses `min(reference, spot)` (E2E F16 — design question).
