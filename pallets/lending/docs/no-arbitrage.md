# No-arbitrage for lift lending

Companion to [`no-arbitrage.tex`](./no-arbitrage.tex) / [`no-arbitrage.pdf`](./no-arbitrage.pdf). **Pallet-first rewrite** (Sept 2026 cross-check only).

## Notation (own)

| Symbol | Meaning |
|--|--|
| $\varphi$ | **Lift fraction** $P/x$ (short) — **not** a fee |
| $w_q$, $w_b$ | Quote (TAO) / base (alpha) Balancer weights |
| $\varepsilon$ | Swap fee on input; **base case $0$**; production $33/65535$ |
| $L$ | $x^{w_b} y^{w_q}$ |
| $\Delta$ | Combined attacker free PnL (profit if $>0$) |

Clone report’s printed `w1` is our $w_q$.

## Key identity (fee-free short open)

After lift $\varphi$ + force-sell principal $P=\varphi x$:

$$
y_s = y\,(1-\varphi)^{1/w_q},\qquad L_s=(1-\varphi)L.
$$

## SN29 prediction (script: `scripts/no_arbitrage_derive.py`)

**a13** ($y=7449.956$, $w_q=0.471657$, $\varphi\approx0.04493$, pump $9000$, buy $2363$):

| model | $\Delta$ free | $w_q$ after |
|--|--|--|
| clone report | **+289.269** | 0.450646 |
| pallet, $\varepsilon=33/65535$, buggy readd | **+289.304** | 0.450646 |
| pallet, keep weights | −281.96 | 0.471657 |

**a15** ($\varphi\approx0.033$, pump $7000$): report **+121.699**; fee-0 buggy **+121.794**; keep-weights **−117.8**.

## μ Sept 2026 cross-check

After our derivation: μ’s $\varphi$ = lift fraction, $w_2=w_q$, fees ignored ($\varepsilon=0$). The shared identity is $y_s/y=(1-\varphi)^{1/w_q}$. We do not import μ’s $F$ as an assumption; the SN29 drain is the post-pump weight re-derivation on readd, which that fee-free $F$ does not encode.

## Fix

`readd_protocol_liquidity` = weight-preserving credit (design a). Not Uni v2.

## Pass/fail (keep-weights)

**PASS** iff no self-contained sequence of at most $N=8$ actions (multi-coldkey and trade-split variants included) yields attacker net TAO $\Delta>0$ for any lift fraction $\varphi$ up to the side-cap $\varphi_{\max}$ on live reserves.

Live finney grid (SN29, SN64 Chutes, SN51, SN4, SN119 thin): keep-weights **PASS** ($20\,087$ cells, max $\Delta\approx -0.01\,\tau$); buggy profitable on $16\,619$ cells. Script: `scripts/no_arbitrage_cycle_search.py`.

Theorem status: SN29 drain identity proved for observed clone books; general closed-form for every keep-weights cycle still open — the bounded search is the shipping criterion.

## Slippage / volatility

While a lift $\varphi$ is outstanding, reserves are scaled by $(1-\varphi)$ at unchanged spot, so small-trade slippage $\approx 1/(1-\varphi)$. Chart: [`no-arbitrage-slippage.pdf`](./no-arbitrage-slippage.pdf) (caps marked).
