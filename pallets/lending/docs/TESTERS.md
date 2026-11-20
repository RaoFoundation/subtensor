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

## Fixed-term expiry (30-day max) and roll

New opens expire after `max_loan_duration` (**216_000** blocks ≈ **30 days** at
12 s by default; terms v2; root-settable). Increases and partial repay **do not**
extend the clock; an increase at or after `expires_at` returns `DeadlinePassed`.

**`btcli lending roll --netuid N`** (extrinsic `roll`) renews a live loan onto a
fresh clock under the same gates as open. Opens-off (`Enabled=false`) disables
rolls. Rejected when expired, out of EMA band, LTV fails at current price, or
the side book already exceeds the current side cap. Interest is charged to now;
principal / collateral / lift / young-short age (`last_exposure_added_at`) are
unchanged. After settle, open a fresh position for a new clock.

**`btcli lending params`** dumps live `Enabled` + `LiftParamsStore` (LTV, APR
long/short, duration, deferral, young-short window, min loan, expiry drain,
floors/caps/ramp, EMA band). Root changes them with `set_lift_params` /
`set_enabled` — no runtime upgrade.

At `expires_at` the position is closed automatically at spot through the pool:
expiry == a default close, not an EMA settlement. There is no wallet top-up and
no slippage limit (MEV at expiry is accepted), and the owner's free TAO is never
touched. Alpha a close would return (long collateral left after covering `P`,
short buyback dust) is sold at spot and paid as TAO because expiry has no hotkey.
An underwater short or insolvent long reuses the collateral-exhaustion
`Lending.Forfeited` path instead of recording a shortfall: lift + escrowed
proceeds are re-added (weight-preserving), remaining collateral is burned,
nothing returns to the borrower, and the k-neutral burn runs as usual. Solvent
positions emit `Lending.Expired` (`rate` = spot before the closing trades).
Every expiry close/forfeit also emits `Lending.LoanExpired`. Neither path loops
in the retry queue once settled. Up to **16** expiry settles run in
`on_initialize` each block; remainder continues in later blocks / `on_idle`.

## Cards and list columns

Dry-run shows the Loan / Repayment / Raise card (quoted payment, refund,
shortfall). List columns include status (underwater), escrow, expires_at, due,
and runway capped at the remaining max_loan_duration term. JSON amounts are rao
integers.

## Locked V1 economics (testers)

- Short interest / opening value = `min(lending EMA, spot)` (F16, Jacob 2026-11-03).
  A pumped spot or pumped EMA cannot inflate the TAO coupon.
- Hard max loan duration: **216_000 blocks (~30 days)** default; renew via
  `roll` only (terms v2). Root can change duration for **new opens / rolls**
  without rewriting live `expires_at`.
- Caps: floor 33%, thin floor 15% below 3k τ, 50/50 split → 7.5% per side on thin
  pools (to be raised after launch). LTV 25% (15% thin longs). APR long/short
  split (default 100% each). All of these are root-settable via `set_lift_params`.
- Volatility-targeting controller is **not** in V1.
- Never-deepen burn: **per-loan on every readd** (not idle-book last-close).
  Emission growth between open and close is not burned.
- Max settlement deferral: **50_400 blocks (~7 days)** default (N28). Band gate
  lifts then; settle runs on the next collector retry. Interest still stops at
  `expires_at`.
- Curve independence: **HOLD**.
- Dark launch: `Enabled=false` until root `set_enabled(true)`.

## Known issues / residual risk

- **N28 residual:** a large pump held the full 7-day max-deferral window through
  forced settle can still cost the pool (504 clone: ~3k τ pump on a thin SN86
  book → forfeit buyback under-recovered alpha, `k` down, attacker PnL positive).
  Longer deadline raises the cost of holding the pump; it does not eliminate it.
- Global `lending-stats` alpha totals are mixed-subnet sums labeled plain `α`
  (not a single-subnet glyph). Prefer `--netuid` for per-subnet alpha.
