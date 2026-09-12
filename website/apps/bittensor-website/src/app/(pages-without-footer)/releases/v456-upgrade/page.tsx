import FadeInWrapper from '@/app/components/FadeInWrapper';
import {CUSHION, OPEN_PRICE, payout, simulate, type Side} from '@/lib/derivatives-math';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V456 Upgrade — Longs and Shorts',
  description:
    'V456 adds pallet-derivatives: longs and shorts on any subnet’s alpha, borrowed from the ' +
    'subnet’s own pool. One position per subnet and one call to add to it, take from it, or ' +
    'flip it; no expiry. No synthetic tokens, nothing minted or burned: the pool lends out at ' +
    'most 25% of itself at 25% a year, the same for both sides, paid to the pool. btcli deriv ' +
    'short, long, list, and close are the working surface.',
  alternates: {canonical: '/releases/v456-upgrade'},
};

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

const GRAPH_TEXT = {
  fontFamily: 'FiraCode',
  fontSize: 10,
  fill: 'rgb(41, 41, 41)',
} as const;

const INK = 'rgb(41, 41, 41)';
const MUTED = 'rgba(41, 41, 41, 0.45)';
const GOLD = '#e0a53f';
const FAINT = 'rgba(41, 41, 41, 0.12)';
const RED = '#c0392b';

// Plot geometry shared by both charts: the plot area is centered in the 840-wide viewBox.
const VIEW_W = 840;
const PLOT_W = 560;
const PLOT_X0 = (VIEW_W - PLOT_W) / 2;

/** Both payoff lines on one axis: what 100 τ comes back as, against the alpha price move. */
const PayoffChart = () => {
  const x0 = PLOT_X0;
  const y0 = 44;
  const w = PLOT_W;
  const h = 300;
  const axis = y0 + h;
  const yHi = 320;

  const xFor = (m: number) => x0 + ((m + 100) / 200) * w;
  const yFor = (v: number) => axis - (Math.min(v, yHi) / yHi) * h;
  // First whole-percent move at which the long's payout is zero: near a two-thirds fall at 1.5x.
  const longGone = Array.from({ length: 101 }, (_, i) => -100 + i).find((m) => payout('long', m) > 0) ?? -50;
  const line = (side: Side) => {
    const parts: string[] = [];
    for (let m = -100; m <= 100; m += 1) {
      parts.push(`${m === -100 ? 'M' : 'L'} ${xFor(m).toFixed(1)} ${yFor(payout(side, m)).toFixed(1)}`);
    }
    return parts.join(' ');
  };

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 840 400'
      role='img'
      aria-label='Value returned for a 100 TAO cushion, plotted against the alpha price move from minus 100 to plus 100 percent. The short line, at 1x, rises as alpha falls and reaches zero near a doubling. The long line, at 1.5x, rises half again as fast as alpha rises and reaches zero near a fall of two thirds. Both cross 100 TAO at no move.'
    >
      <text {...GRAPH_TEXT} x='420' y='28' textAnchor='middle' fill={MUTED} fontSize={12}>
        WHAT 100 τ COMES BACK AS · SHORT 1x · LONG 1.5x · CLOSED THE SAME BLOCK
      </text>
      <line x1={x0} y1={y0} x2={x0} y2={axis} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={axis} x2={x0 + w} y2={axis} stroke={INK} strokeWidth='1' />
      {[0, 100, 200, 300].map((v) => (
        <g key={v}>
          <line
            x1={x0}
            y1={yFor(v)}
            x2={x0 + w}
            y2={yFor(v)}
            stroke={v === CUSHION ? MUTED : FAINT}
            strokeWidth='1'
            strokeDasharray='4 3'
          />
          <text {...GRAPH_TEXT} x={x0 - 8} y={yFor(v) + 3} textAnchor='end' fill={MUTED}>
            {`${v} τ`}
          </text>
        </g>
      ))}
      {[-100, -50, 0, 50, 100].map((m) => (
        <text key={m} {...GRAPH_TEXT} x={xFor(m)} y={axis + 20} textAnchor='middle' fill={MUTED}>
          {m > 0 ? `+${m}%` : `${m}%`}
        </text>
      ))}
      <text {...GRAPH_TEXT} x={x0 + w / 2} y={axis + 38} textAnchor='middle' fill={MUTED}>
        ALPHA PRICE MOVE →
      </text>
      <line x1={xFor(0)} y1={y0} x2={xFor(0)} y2={axis} stroke={FAINT} strokeWidth='1' strokeDasharray='2 3' />
      <text {...GRAPH_TEXT} x={xFor(0) + 6} y={y0 + 12} fill={MUTED} fontSize={9}>
        OPEN PRICE
      </text>
      <text {...GRAPH_TEXT} x={x0 + w - 4} y={yFor(CUSHION) - 6} textAnchor='end' fill={MUTED}>
        YOUR CUSHION, UNCHANGED
      </text>
      <path d={line('short')} fill='none' stroke={INK} strokeWidth='1.5' />
      <path d={line('long')} fill='none' stroke={GOLD} strokeWidth='2' strokeDasharray='6 4' />
      <text {...GRAPH_TEXT} x={xFor(-60)} y={yFor(payout('short', -60)) - 10} textAnchor='middle' fill={INK} fontSize={11}>
        SHORT · 1x
      </text>
      <text {...GRAPH_TEXT} x={xFor(40)} y={yFor(payout('long', 40)) - 10} textAnchor='middle' fill={GOLD} fontSize={11}>
        LONG · 1.5x
      </text>
      <circle cx={xFor(longGone)} cy={axis} r='3.5' fill={RED} />
      <circle cx={xFor(100)} cy={axis} r='3.5' fill={RED} />
      <text {...GRAPH_TEXT} x={xFor(longGone)} y={axis + 38} textAnchor='middle' fill={RED} fontSize={9}>
        LONG CUSHION GONE
      </text>
      <text {...GRAPH_TEXT} x={xFor(100)} y={axis + 38} textAnchor='end' fill={RED} fontSize={9}>
        SHORT CUSHION GONE
      </text>
    </svg>
  );
};

/**
 * Pool price through one short with no market move: the open sale dips it, the close
 * buyback brings it back, and the slice going home does not move it at all.
 */
const FootprintChart = () => {
  const x0 = PLOT_X0;
  const y0 = 44;
  const w = PLOT_W;
  const h = 260;
  const axis = y0 + h;

  const openPrice = OPEN_PRICE;
  const dipped = simulate('short', 0).priceOpen;
  const yLo = 0.0485;
  const yHi = 0.0515;
  const yFor = (v: number) => axis - ((v - yLo) / (yHi - yLo)) * h;
  const xFor = (t: number) => x0 + t * w;

  const phases = [
    {t: 0.0, v: openPrice},
    {t: 0.18, v: openPrice},
    {t: 0.18, v: dipped},
    {t: 0.72, v: dipped},
    {t: 0.72, v: openPrice},
    {t: 1.0, v: openPrice},
  ];
  const path = phases
    .map((p, i) => `${i === 0 ? 'M' : 'L'} ${xFor(p.t).toFixed(1)} ${yFor(p.v).toFixed(1)}`)
    .join(' ');

  const marks = [
    {t: 0.18, label: 'OPEN · LIFT + SELL α', dy: 20},
    {t: 0.72, label: 'CLOSE · REBUY α', dy: 20},
    {t: 0.88, label: 'SLICE + INTEREST RETURN', dy: 34},
  ];

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 840 360'
      role='img'
      aria-label='Pool price through one short with no market move. Lifting the slice does not change the price. Selling the lifted alpha dips it from 0.0500 to 0.0490. Buying the alpha back at close returns it to 0.0500. Returning the slice and interest does not move it.'
    >
      <text {...GRAPH_TEXT} x='420' y='28' textAnchor='middle' fill={MUTED} fontSize={12}>
        POOL PRICE THROUGH ONE SHORT · τ PER α · NO MARKET MOVE
      </text>
      <line x1={x0} y1={y0} x2={x0} y2={axis} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={axis} x2={x0 + w} y2={axis} stroke={INK} strokeWidth='1' />
      {[0.049, 0.05, 0.051].map((v) => (
        <g key={v}>
          <line x1={x0} y1={yFor(v)} x2={x0 + w} y2={yFor(v)} stroke={FAINT} strokeWidth='1' strokeDasharray='4 3' />
          <text {...GRAPH_TEXT} x={x0 - 8} y={yFor(v) + 3} textAnchor='end' fill={MUTED}>
            {v.toFixed(4)}
          </text>
        </g>
      ))}
      {marks.map((m) => (
        <g key={m.label}>
          <line x1={xFor(m.t)} y1={y0} x2={xFor(m.t)} y2={axis} stroke={FAINT} strokeWidth='1' strokeDasharray='2 3' />
          <text {...GRAPH_TEXT} x={xFor(m.t)} y={axis + m.dy} textAnchor='middle' fill={MUTED} fontSize={9}>
            {m.label}
          </text>
        </g>
      ))}
      <text {...GRAPH_TEXT} x={xFor(0.45)} y={yFor(dipped) + 18} textAnchor='middle' fill={MUTED} fontSize={9}>
        HOLD · NO EXPIRY · INTEREST ACCRUES
      </text>
      <path d={path} fill='none' stroke={INK} strokeWidth='1.5' />
      <text {...GRAPH_TEXT} x={xFor(0.18) - 8} y={yFor(dipped) + 3} textAnchor='end' fill={GOLD} fontSize={11}>
        {dipped.toFixed(4)}
      </text>
    </svg>
  );
};

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V456 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Longs and shorts · September 2026
          </p>
        </section>

        <section className={styles.section}>
          <PayoffChart />
          <p className={styles.graph_caption}>
            Put in 100 τ. A short (ink) pays more as alpha falls; a long (gold) pays more as
            alpha rises. Both hand back the cushion at no move, less a little slippage. Once the
            cushion is spent the line stops at zero: settlement pays you nothing, hands whatever
            is left to the pool, and the pool carries the remaining shortfall. You owe nothing
            more.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>456</strong> adds <code>pallet-derivatives</code>. Anyone can now take
            a <strong>long</strong> or a <strong>short</strong> on a subnet&apos;s alpha, backed
            by a TAO deposit, for as long as it pays its interest. A short profits when alpha falls;
            a long profits when alpha rises.
          </p>
          <p>
            You hold one position per subnet, and you move it with one call. Add on the side you
            hold and it grows. Add on the other side and that much comes off, paid out at
            today&apos;s price. Add more than you hold and it flips. There is no expiry to watch
            and nobody can close you out: the chain collects the interest from your cushion
            once a week, and only an empty cushion ends a position. <code>close</code> settles
            all of it.
          </p>
          <p>
            There are no synthetic tokens and no order book. Every position is built from the
            subnet pool&apos;s own reserves: the chain lifts a slice of the pool sized from your
            deposit (one times it for a short, two times for a long), trades that slice through
            the ordinary staking swap, and reverses the trade when you settle. Nothing is minted,
            nothing is burned. The pool lends out at most 25% of itself per side, at 25% a year
            of the slice&apos;s TAO exposure, the same for both sides, fixed for each slice when
            it is added and accrued per block. Those two numbers are the whole design, and root
            sets both.
          </p>
          <p>
            <code>btcli deriv</code> is the working surface: <code>short</code>,{' '}
            <code>long</code>, <code>list</code>, <code>close</code>, and{' '}
            <code>params</code>.
            The full walk-through, with an animated slide deck of one position from open to
            close, is in the{' '}
            <DocLink href='/docs/guides/derivatives'>Longs and shorts</DocLink> guide.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>How a position is built</h2>
          <p className={styles.graph_caption}>
            Three moves, all against the subnet pool. The example is a 100 τ short on a
            10,000 τ / 200,000 α pool.
          </p>

          <div className={styles.step}>
            <p className={styles.step_title}>1 · Lift</p>
            <p>
              At 1x your 100 τ cushion sizes a slice worth 1% of the pool&apos;s TAO, so the
              pallet lifts 1% of both reserves — 100 τ and 2,000 α — out of the pool. (The same
              cushion at 1.5x would lift 1.5%.) Both sides shrink by the same share, so the price
              does not move.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>2 · Trade</p>
            <p>
              A short sells the 2,000 α straight back into the pool for about 99 τ. A long does
              the mirror: it spends the 100 τ on about 1,980 α. This is a real swap with real
              slippage, so a short nudges the price down at open and a long nudges it up. The
              other half of the slice waits in escrow. Your position now holds the proceeds and
              owes the pool what it borrowed.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>3 · Close</p>
            <p>
              At any block, by you and only you, the trade is reversed: a short buys its
              2,000 α back, a long sells its alpha
              and repays the 100 τ. The slice goes home together with the interest, added to the pool
              without moving the price. You get your cushion back, plus or minus the move, minus
              the interest. Adding the other side
              does the same thing to a fraction of the position: a 30 τ long against this 100 τ
              short buys back 600 α, returns 30 τ of escrow, and pays out 30 τ of cushion plus
              or minus the move.
            </p>
          </div>
        </section>

        <section className={styles.section}>
          <FootprintChart />
          <p className={styles.graph_caption}>
            The pool&apos;s view of the same short. Only the two swaps move the price; the lift
            and the return are neutral. With no market move the buyback lands exactly where the
            sale started, and the pool is about 0.068 τ richer for each day the position was open
            (25% a year on the 100 τ this short has in play).
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Add, watch, close</h2>
          <p className={styles.graph_caption}>
            Replace netuid 7 with your target subnet. <code>--amount</code> is the TAO the add
            is sized by; it is deposited as cushion when the add is on your side, and only sizes
            the reduction when it is against it. <code>--leverage</code> is the exposure as a
            multiple of it (default 1), up to the side&apos;s ceiling.
          </p>

          <div className={styles.step}>
            <p className={styles.step_title}>1 · Read the parameters</p>
            <p>
              The pool share and the yearly interest, plus the fixed limits: the leverage ceiling
              per side and the minimum deposit (
              <DocLink href='/docs/query/derivatives-params'>
                <code>derivatives-params</code>
              </DocLink>
              ). A pool share of zero means root has paused new positions.
            </p>
            <pre className={styles.step_code}>{`btcli deriv params --json`}</pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>2 · Add</p>
            <p>
              One position per coldkey and subnet; its side is the sign of what you hold.{' '}
              <code>short</code> and <code>long</code> are the pallet&apos;s single{' '}
              <DocLink href='/docs/tx/add-derivative'>
                <code>add</code>
              </DocLink>{' '}
              call with the side fixed. Same side: another slice is lifted and folded in. Other side: that share is settled
              at today&apos;s price and paid out. More than you hold: the position closes and
              the rest opens on the new side. Nothing expires; add whenever you like.
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv short --netuid 7 --amount 100 -w my_coldkey                # open a short
btcli deriv short --netuid 7 --amount 50 -w my_coldkey                 # add to it
btcli deriv long  --netuid 7 --amount 30 -w my_coldkey                 # take 30 τ off it
btcli deriv long  --netuid 7 --amount 300 --leverage 2 -w my_coldkey   # flip to a long`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>3 · Watch</p>
            <p>
              Interest so far, runway, and estimated equity (
              <DocLink href='/docs/query/derivative-positions'>
                <code>derivative-positions</code>
              </DocLink>
              ). Runway is how many days the cushion keeps paying interest at the current rate;
              at zero the chain forfeits the position to the pool. Equity prices the closing leg
              on a constant-product curve; the chain&apos;s own quote decides.
            </p>
            <pre className={styles.step_code}>{`btcli deriv list -w my_coldkey`}</pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>4 · Close</p>
            <p>
              Atomic: reverse the trade, repay the pool, collect the interest, pay you, return the
              slice — or the whole call reverts (
              <DocLink href='/docs/tx/close-derivative'>
                <code>close</code>
              </DocLink>
              ). Only you can close your position. To keep it open longer, add cushion.
            </p>
            <pre className={styles.step_code}>{`btcli deriv close --netuid 7 -w my_coldkey`}</pre>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What bounds it</h2>
          <p>
            <strong>Leverage you choose on each add, under a fixed ceiling: 1x on shorts, 1.5x
            on longs.</strong> At 1x a short&apos;s exposure equals its cushion, so a 20% move
            in alpha moves a 100 τ short by about 20 τ and a doubling wipes it. At 1.5x a
            long&apos;s exposure is half again its cushion: a 20% move is worth about 30 τ and
            a fall of two thirds wipes it. In general a position at leverage L is wiped by a
            move of 1/L against it. At 1x a long can never cost the pool anything and does
            nothing a spot buy does not, so the ceiling is above it. It is 1.5x, not higher,
            because of one attack: the largest long the cap admits could dump alpha it holds
            outside into its own lifted price and walk away from the debt, and that pays once
            the leverage is above 1 + sqrt(1 − pool_share), 1.87x at a 25% share. 1.5x is
            under that line for every share up to 25% and every weight the pool drifts to.
            The ceilings are runtime constants, changed only by an upgrade. Your cushion is TAO,
            and it is the most you can lose. If the pool&apos;s own quote says the closing trade
            could not repay what the position borrowed plus the interest, the position is
            underwater and <em>nothing is traded</em>: you are paid nothing, everything the
            pallet holds goes to the pool as it is, and the pool carries the remaining shortfall.
            Once you are underwater a swap could only cost the pool more, and a market order
            announced in advance is something anyone can trade against; a close that does not
            trade gives them nothing.
          </p>
          <p>
            <strong>Liquidity is not re-added into a pushed price.</strong> Returning a slice
            to the pool is price-neutral, which is exactly wrong when the price was just
            pushed: the pool would deepen at the pushed price and whoever pushed it would
            sell back into that depth. So before handing anything back the pallet compares the
            spot price with the subnet&apos;s moving price. If they are more than 5% apart the
            pair is <em>parked</em> in the pallet, outside the pool, and <code>on_idle</code>{' '}
            re-adds it once the spot is back within 5%. An honest close on a quiet pool parks
            nothing. Parked alpha counts as outstanding for the emission price, and a
            dissolution returns any parked pair to the reserves before settling anything.
          </p>
          <p>
            <strong>25% pool share.</strong> All open positions of one side on one subnet may
            borrow at most 25% of the reserve they lend from. Above that, opens fail with{' '}
            <code>PoolCapExceeded</code> until others close. This keeps the pool&apos;s worst
            case — every position on one side blowing through its cushion — bounded relative to
            the pool. Root can raise or lower the share; setting it to zero pauses new positions
            while every open one can still be reduced or closed. Slices and the cap are sized
            against the subnet&apos;s moving-average price as well as the live pool, taking the
            tighter of the two, so a spot swap in the same block cannot buy a bigger slice or
            more room. The cap is checked on what the pool actually lost, proceeds plus escrow
            after the opening swap, so a pool whose balancer weights have drifted cannot lend
            one side more than its share.
          </p>
          <p>
            <strong>No expiry, no liquidation.</strong> A position has no term. It lives until
            you close it or its cushion runs out. Nothing forces a mark to market on a date, so
            nothing has to be rolled and no add is ever refused for being late. And nobody else
            can close it: a position that anyone could close once the price moved against it
            would invite a squeeze — pump the price, close the shorts, sell into the buybacks the
            closes force — so there is no such door. A price move never takes your position. What
            keeps a slice from being held out of the pool for good is the interest: every block
            open costs the same, the chain collects it from your cushion once a week, and a
            cushion that runs dry ends the position. That is the design: the pool lends out at most{' '}
            <code>pool_share</code> of itself, at <code>interest_rate</code>. Root picks those two
            numbers, and that is all.
          </p>
          <p>
            <strong>One interest rate, both sides, collected weekly as buy pressure.</strong>{' '}
            Each add fixes a yearly interest for its slice, <code>interest_rate</code>, 25%,
            times the slice&apos;s TAO exposure, and the position&apos;s summed interest accrues
            per block from then on. Nothing is charged up front. A position is given a due block
            one week after it opens and listed in a queue under it; when the chain reaches that
            block it takes the interest accrued out of the cushion, buys alpha from the pool with
            it, and recycles the alpha, the same way a registration burn does. The pool keeps the
            TAO, the alpha leaves circulation, and the price ticks up: interest on a short and on
            a long alike is a buy of the subnet&apos;s alpha. The position is then booked again a
            week later; its trade, leverage, and exposure are not touched. Each block does only
            the collections that fall on it. A 100 τ short at 1x pays 25 τ a year, about 2.1 τ a
            month or 0.068 τ a day; a 100 τ long at 1.5x has 150 τ in play and pays half again
            that.
            Leverage costs in proportion to what it borrows, and pool size and pool share do not
            enter.
          </p>
          <p>
            <strong>Runway and forfeit.</strong> <code>btcli deriv list</code> shows how many
            days your cushion keeps paying at the current rate: four years for a 1x position,
            two years and eight months for a 1.5x long. When a collection finds a cushion that
            cannot cover the interest due, the position is starved and forfeited: every TAO and
            every alpha the pallet holds for it goes back to the pool in kind, with no swap. The
            pool gets its slice back plus the cushion; you get nothing; no price moves. A
            collection leaves a position on a dissolving subnet alone; the settlement takes the
            interest instead. To extend the runway, add cushion. A
            position that is underwater on price can still be held: the pool is not out of
            pocket while it holds your cushion and the slice, and if the price comes back so
            does your equity. The interest is what the pool charges for its liquidity, not an
            option premium; what protects the pool from a position that turns dangerous is the
            share cap and the ceilings. Profit comes out of the pool; loss goes into it.
          </p>
          <p>
            <strong>Longs do not earn emission.</strong> Emission is weighted by each
            subnet&apos;s moving price, and a long lifts the spot price for as long as it is
            open. So the price the emission EMA tracks is computed with every open long&apos;s
            alpha counted back into the pool: a long leaves the pool&apos;s TAO where it was and
            takes alpha out, so adding that alpha back gives the price the pool would show with
            no longs at all. A team cannot long its own subnet to be paid more. Shorts are left
            in; a short lowers the price on purpose, and the emission follows it. Swaps and the
            pool itself use the real reserves; only the emission weight is adjusted.
          </p>
          <p>
            <strong>When the subnet is deregistered: cash settlement at fixed prices.</strong>{' '}
            If a subnet is dissolved with positions open, settling them is the first cleanup
            phase, before stakes are converted or any staker is paid. The chain reads the
            pool&apos;s spot price and the subnet&apos;s moving price once, stores them, and
            emits <code>DissolutionPriced</code>; shorts are charged at the higher of the two
            and longs credited at the lower, with no swap and no netting of one position
            against another. A short&apos;s alpha debt is converted to TAO at the short price,
            rounded up, and repaid from the cushion plus proceeds; the interest owed is taken;
            the rest is paid to you in TAO. A long pays its TAO debt and interest from its
            cushion first and the rest in alpha at the long price, rounded up; the alpha it
            still holds is then bought by the pool for TAO out of the reserve, as far as the
            reserve goes, and any alpha the reserve cannot buy stays yours as stake, paid out
            with every other stake in the later phases. So no long is paid nothing because
            another drew the reserve first. An underwater position pays you nothing and its
            remainder goes to the pool. Everything the pool is owed, and any parked liquidity,
            returns to its reserves, which is what the stakers are paid from next. Settlement
            never blocks dissolution: each block settles as many positions as its weight
            budget allows and resumes in the next, with no cap on how many positions a subnet
            may have; a transfer that fails is logged and the cleanup moves on. Because nothing
            is bought back, nothing climbs the curve back either: a short that drove the price
            down in the block the subnet died is charged the moving price it could not move,
            and loses its own impact.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>How positions close and how interest works, in short</h2>
          <p className={styles.graph_caption}>
            The same rules as above, reduced to the facts a holder needs. The full steps are in
            the <DocLink href='/docs/guides/derivatives#how-a-position-closes'>guide</DocLink>.
          </p>

          <div className={styles.step}>
            <p className={styles.step_title}>Who can close</p>
            <p>
              Only you. <code>close</code> settles the position of the account that signs it and
              no other. There is no liquidator, no expiry date, and no price that forces a
              close. A position ends in one of four ways, named in the{' '}
              <code>PositionClosed</code> event: <code>Owner</code> (you closed it),{' '}
              <code>Underwater</code> (you closed it, but the pot could not repay the debt, so
              nothing was traded and everything went to the pool as it was),{' '}
              <code>Starved</code> (its cushion could not pay a weekly interest collection), or{' '}
              <code>Dissolution</code> (the subnet was removed). A price move against you never
              ends the position on its own; the pool carries that exposure until you close or the
              cushion runs dry.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>Closing a short</p>
            <p>
              The pot is your cushion plus the TAO the borrowed alpha was sold for. The pool is
              asked what buying back the alpha owed would cost; if that plus the interest is
              more than the pot, the position is underwater, nothing is traded, the pot and the
              escrow go to the pool and you are paid nothing. Otherwise the pot buys back
              exactly the alpha owed, interest comes out of what is left, and the rest is yours,
              in TAO.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>Closing a long</p>
            <p>
              The mirror. The pool is asked what the held alpha would sell for; if your cushion
              plus that is less than the TAO owed plus the interest, the position is underwater,
              nothing is traded, the cushion and the alpha go to the pool as they are and you
              are paid nothing. Otherwise the alpha is sold, the pot repays the TAO owed,
              interest comes out of what is left, and the rest is yours, in TAO.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>Closing part of it</p>
            <p>
              Add on the other side. Less than you hold settles that fraction and pays it out;
              nothing is deposited. Your whole position or more closes all of it, and any deposit
              past the flip point of at least 0.1 τ opens the other side.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>Weekly interest</p>
            <p>
              Two root-set numbers: <code>pool_share</code> (25%), the most one side of one
              subnet may borrow; and <code>interest_rate</code> (25% a year), charged on each
              slice&apos;s TAO exposure, the same for shorts and longs, fixed when the slice is
              added. What you owe grows every block. The chain collects it every 50,400 blocks,
              about 7 days, at the start of a block, up to twenty positions per block, and books
              the next collection a week later. Adding on your own side does not reset that
              clock. The collected TAO buys alpha from the pool and the alpha is recycled, so the
              interest reaches the pool as buy pressure: the pool keeps the TAO, the alpha leaves
              circulation, and the price ticks up.
            </p>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>When the cushion runs dry</p>
            <p>
              The check happens only at a weekly collection. A cushion that cannot cover the
              interest owed then is starved: everything the pallet holds for the position goes
              back to the pool as it is, with no swap, you are paid nothing, and no price moves.
              To keep a position alive, add cushion on your own side.
            </p>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changed on chain</h2>
          <p>
            <code>pallet-derivatives</code> is added at index 33 with two user calls —{' '}
            <code>add</code>, which takes a <code>side</code> of short or long, a TAO deposit
            and a <code>leverage_percent</code>, and <code>close</code> — plus one root-only
            call, <code>sudo_set_params</code>, which sets the two parameters:{' '}
            <code>pool_share</code> (25%) and <code>interest_rate</code> (25%). Both are dials
            root can turn later; a position keeps the rate it has, and its next add is checked
            against the new share. Three limits are runtime constants:{' '}
            <code>MaxShortLeverage</code> 100, <code>MaxLongLeverage</code> 150,{' '}
            <code>MinDeposit</code> 0.1 τ. A position is one record per coldkey and subnet,
            every field a sum over its adds: the TAO cushion, the legs, the exposure, the yearly
            interest, the interest carried since it was last touched, and the block it is next
            collected. A <code>Due</code> queue lists positions by that block; the pallet&apos;s{' '}
            <code>on_initialize</code> collects the ones due, up to twenty a block, walking the
            queue from <code>NextDue</code> so nothing is skipped after a crowded slot or a
            stall, and forfeits any whose cushion cannot pay. Its <code>on_idle</code> re-adds
            liquidity parked in <code>Parked</code> once a subnet&apos;s spot price is back
            within 5% of its moving price. Existing positions can always be closed by their
            owner, whatever the share is set to.
          </p>
          <p>
            The subtensor pallet gains a small pool interface for the derivatives pallet:
            price-neutral <code>lift_liquidity</code> and <code>return_liquidity</code>, internal
            buy and sell through the existing balancer swap, exact-output swaps for the
            buyback, and quotes for both so an underwater close can be recognised without
            trading. Subnet dissolution gains a <code>DerivativesSettle</code> phase that runs
            first and cash-settles every position, shorts at the higher of spot and moving
            price and longs at the lower, emitted as <code>DissolutionPriced</code>. The
            emission price EMA now reads <code>get_emission_alpha_price</code>, the spot price
            with the long-side footprint and any parked alpha added back to the alpha reserve;
            with no longs open and nothing parked it is the spot price exactly.
          </p>
          <p>
            New runtime reads:{' '}
            <DocLink href='/docs/query/derivative-position'>
              <code>derivative-position</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/derivative-positions'>
              <code>derivative-positions</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/derivatives-params'>
              <code>derivatives-params</code>
            </DocLink>
            . SDK intents <code>AddPosition</code> and <code>ClosePosition</code> back the
            btcli commands. Upgrade the SDK to get{' '}
            <code>btcli deriv</code>:
          </p>
          <pre className={styles.code_block}>{`pip install -U bittensor`}</pre>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
