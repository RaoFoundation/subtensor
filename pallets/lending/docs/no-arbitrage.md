# No-arbitrage for lift lending

Companion to [`no-arbitrage.tex`](./no-arbitrage.tex) / [`no-arbitrage.pdf`](./no-arbitrage.pdf).

## The shogting cycle

**Definition:** open short → stake/pump → close → unstake (single-wallet, TAO-pure). Budget $V=h\,y$ with $h=\mathrm{budget}/\texttt{tao\_in}$.

## Fresh-fork base case (not contaminated $w_q\approx 0.47$)

Finney SN29 at lending-clone fork block **#9,378,125**:

| | |
|--|--|
| $y$ | $7897.952\,\tau$ |
| $x$ | $3\,153\,658\,\alpha$ |
| $w_q$ | $\approx\tfrac12$ |

Under **fixed** (keep-weights) readd, $w_q$ stays $\tfrac12$. Under **unfixed** readd, $w_q$ moves (e.g. $0.473$ at $h=0.5$).

Contaminated a13/a15 books ($w_q=0.4717$, $0.4507$) were sequential drains on one chain — appendix only.

## Closed-form ROI (fee-free, $w=\tfrac12$)

\[
\mathrm{ROI}_{\mathrm{fix}}(h)=\frac{E-S-B+\Delta y^{\mathrm{fix}}}{V},\qquad
\mathrm{ROI}_{\mathrm{unfix}}(h)=\frac{E-S-B+\Delta y^{\mathrm{unfix}}}{V}.
\]

At $h=0.5$ on the fork book: unfixed **+3.10%**, fixed **−2.94%**.

## ROI plot

[`no-arbitrage-shogting-roi.png`](./no-arbitrage-shogting-roi.png) — % ROI vs $h\in[0.01,10]$ (log), unfixed/fixed × $\varepsilon=0$ / production fee.

Script: `scripts/no_arbitrage_shogting.py`.

## Relation to μ

Lift identity $y_s/y=(1-\varphi)^{1/w_q}$ **reduces to** μ’s $(1-\varphi)^{1/w_2}$. Shogting ROI **differs** from μ’s $F$ under unfixed readd (weight ratchet = SN29 drain). Under keep-weights + $\varepsilon=0$, $\mathrm{ROI}\le 0$ matches μ’s $F\ge 1$ (reciprocal conventions).

## Root fix

Keep-weights readd (fixed shogting path). No undefined “ship (a)” / “design (b)” labels.

## K-neutrality burn (Jacob 2026-10-30)

Keep-weights stops the SN29 weight ratchet, but a shogting cycle can still raise $L=x^{w_b}y^{w_q}$ (cycle loss stays in the pool). On the last readd when the lift book returns to idle, burn a common fraction of both reserves so $L$ returns to the pre-lending baseline $L^\star$ snapshotted at first open:

$$\sigma=(x^\star/x')^{w_b}(y^\star/y')^{w_q},\quad\text{keep }\sigma\text{ of each reserve (spot fixed)}.$$

Floors at pre-credit reserves: never burn user funds or escrow. Works for any weights; dissolution skips.
