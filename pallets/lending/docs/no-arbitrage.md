# No-arbitrage for lift lending (working note)

Companion to [`no-arbitrage.tex`](./no-arbitrage.tex) / [`no-arbitrage.pdf`](./no-arbitrage.pdf). **Increment 3** for public review on PR #3268. Follows μ’s method: TAO-pure cycles → factor $F_{\mathrm{cyc}}(h;\text{LP};\Theta)$ → min/max over parameter space.

## Theorem status (one paragraph)

Under **buggy** re-add (`readd_protocol_liquidity` → `adjust_protocol_liquidity` → weight update at the post-swap ratio), the claim “no TAO-pure cycle has attacker return $>1$” is **false**. Clone tip `4d3dc1585` (spec 493): short→foreign pump→wallet repay **+289 τ** / SubnetTAO **−298** with $w_1:0.4717\to0.4506$, $L:174\mathrm{k}\to203\mathrm{k}$; self-pump buyback **+122 τ**; no-pump control **−0.61 τ**. Cycle A sketches agree ($F_A\approx 1.018$–$1.054$). μ’s fee-only $F$ stays $\gtrsim 1$, so the drain is the **weight ratchet**, not fees.

**Root fix:** `readd_protocol_liquidity` is the weight-preserving Balancer inverse of `lift_protocol_liquidity` (no `adjust` / no weight write). Lending `readd_and_credit` and `_preserve_weights` both use that path. Regressions: `test_readd_after_skew_preserves_weights`, `sn29_short_self_pump_buyback_close_*`, `sn29_short_foreign_pump_wallet_repay_*` (no attacker profit, $w_1$ unchanged, $L$ conserved). Full closed-form proof for every TAO-pure cycle remains open; clone sequences are covered.

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

### Cycle A sweep (buggy weight-update path, production fee)
On SN29-like pool, weight-**update** path breakeven at $h\approx 0.031$; above that $F_A>1$ (e.g. +0.43% at EMA cap $h\approx 0.072$, +2.71% at $h=0.165$). Weight-**keep** path is negative for every listed $h$. JSON: [`scripts/no_arbitrage_h_sweep.json`](./scripts/no_arbitrage_h_sweep.json).

### Do protections force EV ≤ 0 on the buggy path?
**No.** EMA band (~7.2%), side caps (7.5–16.5%), and `PrincipalExceedsTaoIn/AlphaIn` only bound $h$. Inside the band, Cycle A still has +EV for $h\gtrsim 0.031$.

## Increment 3 — clone drain + lift/readd inverse

| seq | path | attacker | SubnetTAO | $w_1$ |
|-----|------|----------|-----------|-------|
| a13 | short 1500 → pump 9000 → wallet repay | +289 τ | −298 τ | 0.4717→0.4506 |
| a15 | short 1200 → self-pump 7000 → buyback | +122 τ | −128 τ | 0.4507→0.4389 |
| a14 | wallet repay, no pump | −0.61 τ | +0.11 τ | — |

**Fix:** `readd_protocol_liquidity` ≡ weight-preserving credit (exact inverse of lift on the Balancer side). Do not re-derive weights from the current ratio after intervening swaps.

## Next increments

Closed forms for $F_B,F_C,F_D$ under fixed readd; grid extrema; ≤6-step search; call-flow doc.
