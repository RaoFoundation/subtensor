# No-arbitrage for lift lending (working note)

Companion to [`no-arbitrage.tex`](./no-arbitrage.tex) / [`no-arbitrage.pdf`](./no-arbitrage.pdf). **Increment 3b** for public review on PR #3268.

## Answer to μ: we do not abandon Balancer

**Stake / unstake stay on the Balancer weighted pool** with stored $(w_1,w_2)$. Emissions still use `adjust_protocol_liquidity` → `update_weights_for_added_liquidity`. **Only lending lift/readd** bypasses that weight update.

While a position is open, the live book is a thinner Balancer pool at the same weights and spot — pricing is coherent.

## Two readd designs

| | (a) snapshot / restore exact deltas | (b) settle in invariant units |
|--|--|--|
| Readd | credit exact $(\Delta y,\Delta x)$, **weights untouched** | proportional join restoring $\Delta L$ at **current** ratio; MTM custody residual |
| No intervening swaps | exact inverse ($p$, $L$ restored) | same |
| After swaps | $w$ fixed; $L$ need not equal $L_0$; spot may jump on credit | $w$ fixed; $L$ restored by construction; residual swap surface |
| Shipped? | **yes** (root fix) | no (search-clean, but needs residual accounting) |

**Buggy path** (old): credit deltas **and** rewrite weights to keep post-credit spot — emissions tool, not lift inverse → a13/a15 drain.

## Bounded cycle search

[`scripts/no_arbitrage_cycle_search.py`](./scripts/no_arbitrage_cycle_search.py) → [`no_arbitrage_cycle_search.json`](./scripts/no_arbitrage_cycle_search.json).

Expanded grid ($w_1\in\{0.35,0.47,0.50,0.65\}$, $y\in\{2\mathrm{k},8\mathrm{k},50\mathrm{k}\}$, short/long × buyback/wallet × pump/dump):

| design | cells | profitable | $\max F_{\mathrm{cyc}}$ |
|--|--|--|--|
| buggy | 672 | 318 | 1.88 |
| (a) | 672 | **0** | 0.999 |
| (b) | 672 | **0** | 0.999 |

This is a **bounded search**, not a closed-form proof. Conjecture “all TAO-pure cycles have $F\le 1$ under (a)” remains open. We ship (a) because it is the lift inverse, kills the reproduced drain, and clears the search.

## Clone reproduction (spec 493 / tip `4d3dc1585`)

| seq | path | attacker | SubnetTAO | $w_1$ |
|-----|------|----------|-----------|-------|
| a13 | short 1500 → pump 9000 → wallet repay | +289 τ | −298 τ | 0.4717→0.4506 |
| a15 | short 1200 → self-pump 7000 → buyback | +122 τ | −128 τ | 0.4507→0.4389 |
| a14 | wallet repay, no pump | −0.61 τ | +0.11 τ | — |

## Earlier increments (1–2)

μ’s fee-only $F\gtrsim 1$ on production fees; Cycle A with weight-**update** has $F_A>1$ for $h\gtrsim 0.031$; EMA/capacity only bound $h$, do not flip the sign. Keep-weights flips every listed row to $F_A<1$. See tex §§numeric / h-sweep / protections.
