# No-arbitrage for lift lending (working note)

Companion to [`no-arbitrage.tex`](./no-arbitrage.tex) / [`no-arbitrage.pdf`](./no-arbitrage.pdf). **Increment 1** for public review on PR #3268. Follows μ’s method: TAO-pure cycles → factor $F_{\mathrm{cyc}}(h;\text{LP};\Theta)$ → min/max over parameter space.

## Theorem status (one paragraph)

Under **today’s wallet-repay re-add** (Balancer weights updated to keep post-trade spot), the claim “no TAO-pure cycle has attacker return $>1$” is **false**. Cycle A (open long → wallet repay → sell escrow $A$) yields $F_A\approx 1.018$–$1.054$ on an SN29-like pool ($y=10^5\,\tau$, $p\approx 0.00279$, $P=0.165\,y$), with $w_1$ and $\sqrt{k}$-proxy rising. μ’s September fee-only $F$ stays $\gtrsim 1$ (no fee-only arb), so the drain is **not** explained by fees: it is the **weight ratchet** on asymmetric re-add. With weight-**preserving** re-add, the same paths give $F_A<1$. Full proof under preserve-weights is open.

## μ’s September $F$ (transcription checked)

$$
F=(f_h+1)\left(\frac{(1-\varphi)^{1/w_2}}{(1-\varphi)^{1/w_2}+f_h-\varphi}\right)^{E},
$$
$$
E=\frac{f_h+1}{(f_h+1+\varphi)\left(1-\frac{1-(1-\varphi)^{1/w_2}}{f_h-\varphi+1}\right)^{w_2/(1-w_2)}}.
$$

μ’s convention: profitable when $F<1$. This note’s $F_{\mathrm{cyc}}$ is attacker terminal/initial TAO (profit when $>1$).

| Symbol | Provisional map into current lift math |
|--------|------------------------------------------|
| $w_2$ | Balancer quote weight $w_q$ (TAO leg) |
| $\varphi$ | Swap fee ($\approx 33/65535$) |
| $f_h$ | Size as fraction of pool TAO (e.g. $P/y$ or LTV-scaled) |

At $w_2=\tfrac12$, $\varphi=33/65535$: $F\in[1.00011,1.00049]$ for $f_h\in[0.01,0.25]$ — **never** $<1$. Script: [`scripts/no_arbitrage_F.py`](./scripts/no_arbitrage_F.py).

## Cycle A numbers (SN29-like)

| dump | weights | $F_A$ | profit (τ) | $w_1$ after |
|------|---------|-------|------------|-------------|
| 0% | update | 1.0176 | +291 | 0.515 |
| 0% | keep | 0.9631 | −609 | 0.500 |
| 35% | update | 1.0413 | +681 | 0.528 |
| 35% | keep | 0.9411 | −972 | 0.500 |
| 50% | update | 1.0539 | +889 | 0.535 |
| 50% | keep | 0.9297 | −1160 | 0.500 |

## Increment 2 — $h$-sweep and protections

### μ’s returns graph (eyeball verified)
Return % vs pool size in units of trader TAO budget (log x, 10%–1000%): ≈+1% at 10%; peak ≈+4.5% near 100%; zero near ~300%; ≈−39% at 1000%. Profitable when budget ≳ ⅓ of pool TAO (large-trade regime).

### Cycle A sweep (current design, production fee)
On SN29-like pool, weight-**update** path breakeven at $h\approx 0.031$; above that $F_A>1$ (e.g. +0.43% at EMA cap $h\approx 0.072$, +2.71% at $h=0.165$). Weight-**keep** path is negative for every listed $h$. JSON: [`scripts/no_arbitrage_h_sweep.json`](./scripts/no_arbitrage_h_sweep.json).

### Do protections force EV ≤ 0?
**No.** EMA band (~7.2%), side caps (7.5–16.5%), and `PrincipalExceedsTaoIn/AlphaIn` only bound $h$. Inside the band, Cycle A still has +EV for $h\gtrsim 0.031$. Splits at $h=0.05$ earn ~+0.15% each while $w_1$ ratchets; a pre-dump then long-to-band realises the large $h$ profits of increment 1. Multi-coldkey does not remove the ratchet.

## Next increments

Closed forms for $F_B,F_C,F_D$; grid extrema; ≤6-step search; call-flow doc.
