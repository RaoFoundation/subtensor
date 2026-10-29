# No-arbitrage for lift lending (working note)

Companion to [`no-arbitrage.tex`](./no-arbitrage.tex) / [`no-arbitrage.pdf`](./no-arbitrage.pdf). **Increment 3c** for public review on PR #3268.

## Answer to μ: we do not abandon Balancer / this is not Uni v2

**Stake / unstake** already run on fixed stored $(w_1,w_2)$ — they never re-derived weights. Main re-derives weights only on **asymmetric liquidity adds** (emissions via `adjust_protocol_liquidity`) so those adds do not move spot.

**Fixed weights ≠ Uniswap v2.** Uni v2 is the special case $w_1=w_2=\tfrac12$. With $w_1\neq w_2$ held fixed (SN29 ~0.47), the pool is still a weighted Balancer CPMM. Design (a) does not force 50/50.

**While a lift is outstanding**, swaps are fixed-weight Balancer — same as every swap already was. What (a) changes is only **lending readd after skew**: no price-neutral asymmetric join.

**What is lost:** price-neutrality of lending readd (spot may jump; $L$ may drift). Emissions / stake / unstake unchanged. **Acceptable** as the emergency inverse: readd restores protocol inventory; crystallising a pumped mark into $w$ was the drain.

## Designs (keep re-derivation for emissions)

| | Readd rule | Emissions re-derive? | Notes |
|--|--|--|--|
| **(a) shipped** | exact snapshot deltas, keep $w$ | yes | quiet-path inverse; readd can move spot; search-clean |
| **(b) next if needed** | restore $\Delta L$ at **current** ratio; MTM residual swap | yes (unused on readd) | price-neutral join; $L$ restored; more code |
| (c) re-derive at lift-time ratio | proportional legs ⇒ weight update is a **no-op** (= a) | — | forcing spot back to lift-time price: do not |
| (d) target EMA on readd | join/reweight toward EMA | yes | settlement policy later; not the drain root |
| (e) keep current-mark reweight + fee | still rewrites $w$ | yes | rejected — ratchet remains |

## Recommendation for Jacob

1. **Ship (a) now** — not Uni v2; kills a13/a15; emissions still re-derive.
2. **Never** restore current-mark weight update on lending readd.
3. **Optional hardening:** (b) if (a) ever counterexampled.
4. (c)≈(a) for proportional legs; skip (d)/(e) as root fix.
5. Search-backed, not theorem-closed.

## Bounded cycle search

[`scripts/no_arbitrage_cycle_search.py`](./scripts/no_arbitrage_cycle_search.py): 672 cells — buggy 318 profitable (max $F=1.88$); **(a)/(b) zero** profitable (max $F<1$).

## Clone reproduction (spec 493 / tip `4d3dc1585`)

| seq | path | attacker | SubnetTAO | $w_1$ |
|-----|------|----------|-----------|-------|
| a13 | short 1500 → pump 9000 → wallet repay | +289 τ | −298 τ | 0.4717→0.4506 |
| a15 | short 1200 → self-pump 7000 → buyback | +122 τ | −128 τ | 0.4507→0.4389 |
| a14 | wallet repay, no pump | −0.61 τ | +0.11 τ | — |
