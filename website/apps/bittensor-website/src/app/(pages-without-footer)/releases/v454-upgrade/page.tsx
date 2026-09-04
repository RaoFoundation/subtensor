import FadeInWrapper from '@/app/components/FadeInWrapper';
import {CUSHION, OPEN_PRICE, payout, simulate, type Side} from '@/lib/derivatives-math';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V454 Upgrade — Longs and Shorts',
  description:
    'V454 adds pallet-derivatives: 30-day longs and shorts on any subnet’s alpha, borrowed ' +
    'from the subnet’s own pool. No synthetic tokens, nothing minted or burned, a per-day ' +
    'borrow fee fixed at open and paid to the pool. btcli deriv short, long, list, roll, and ' +
    'close are the working surface.',
  alternates: {canonical: '/releases/v454-upgrade'},
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
  const yHi = 220;

  const xFor = (m: number) => x0 + ((m + 100) / 200) * w;
  const yFor = (v: number) => axis - (Math.min(v, yHi) / yHi) * h;
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
      aria-label='Value returned for a 100 TAO cushion at 1x, plotted against the alpha price move from minus 100 to plus 100 percent. The short line rises as alpha falls and reaches zero near a doubling. The long line rises as alpha rises and reaches zero as alpha approaches zero. Both cross 100 TAO at no move.'
    >
      <text {...GRAPH_TEXT} x='420' y='28' textAnchor='middle' fill={MUTED} fontSize={12}>
        WHAT 100 τ COMES BACK AS · 1x · CLOSED AFTER ONE DAY
      </text>
      <line x1={x0} y1={y0} x2={x0} y2={axis} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={axis} x2={x0 + w} y2={axis} stroke={INK} strokeWidth='1' />
      {[0, 50, 100, 150, 200].map((v) => (
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
        SHORT
      </text>
      <text {...GRAPH_TEXT} x={xFor(60)} y={yFor(payout('long', 60)) - 10} textAnchor='middle' fill={GOLD} fontSize={11}>
        LONG
      </text>
      <circle cx={xFor(-100)} cy={axis} r='3.5' fill={RED} />
      <circle cx={xFor(100)} cy={axis} r='3.5' fill={RED} />
      <text {...GRAPH_TEXT} x={xFor(-100)} y={axis + 38} fill={RED} fontSize={9}>
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
    {t: 0.88, label: 'SLICE + FEE RETURN', dy: 34},
  ];

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 840 360'
      role='img'
      aria-label='Pool price through one short with no market move. Lifting the slice does not change the price. Selling the lifted alpha dips it from 0.0500 to 0.0490. Buying the alpha back at close returns it to 0.0500. Returning the slice and fee does not move it.'
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
        HOLD · UP TO 30 DAYS · FEE ACCRUES
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
          <h1 className={styles.paper_title}>The V454 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Longs and shorts · September 2026
          </p>
        </section>

        <section className={styles.section}>
          <PayoffChart />
          <p className={styles.graph_caption}>
            Put in 100 τ. A short (ink) pays more as alpha falls; a long (gold) pays more as
            alpha rises. Both hand back the cushion at no move, minus one day of fee. Once the
            cushion is spent the line stops at zero: settlement pays you nothing, hands whatever
            is left to the pool, and the pool carries the remaining shortfall. You owe nothing
            more.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>454</strong> adds <code>pallet-derivatives</code>. Anyone can now open
            a <strong>long</strong> or a <strong>short</strong> on a subnet&apos;s alpha for up
            to 30 days, backed by a TAO deposit. A short profits when alpha falls; a long profits
            when alpha rises.
          </p>
          <p>
            There are no synthetic tokens and no order book. Every position is built from the
            subnet pool&apos;s own reserves: the chain lifts a slice of the pool the same size as
            your deposit, trades that slice through the ordinary staking swap, and reverses the
            trade when you close. Nothing is minted, nothing is burned. The pool earns a borrow
            fee fixed per day at open: 5 τ a day per 100% of the pool a short lifts, 0.02% of
            exposure a day on a long.
          </p>
          <p>
            <code>btcli deriv</code> is the working surface: <code>short</code>,{' '}
            <code>long</code>, <code>list</code>, <code>close</code>, and <code>params</code>.
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
              Your 100 τ cushion is 1% of the pool&apos;s TAO, so the pallet lifts 1% of both
              reserves — 100 τ and 2,000 α — out of the pool. Both sides shrink by the same
              share, so the price does not move.
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
              At any block within 30 days — or by anyone after — the trade is reversed: a short
              buys its 2,000 α back, a long sells its alpha and repays the 100 τ. The slice goes
              home together with the fee, added to the pool without moving the price. You get
              your cushion back, plus or minus the move, minus the fee.
            </p>
          </div>
        </section>

        <section className={styles.section}>
          <FootprintChart />
          <p className={styles.graph_caption}>
            The pool&apos;s view of the same short. Only the two swaps move the price; the lift
            and the return are neutral. With no market move the buyback lands exactly where the
            sale started, and the pool is 0.05 τ richer for each day the position was open (a 100
            τ short lifts 1% of this 10,000 τ pool; 5 τ × 1% = 0.05 τ a day).
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Open, watch, close</h2>
          <p className={styles.graph_caption}>
            Replace netuid 7 with your target subnet. <code>--amount</code> is the cushion, in
            TAO, taken from your coldkey balance.
          </p>

          <div className={styles.step}>
            <p className={styles.step_title}>1 · Read the parameters</p>
            <p>
              Whether each side is enabled, the leverage, the pool cap, the lifetime, the fee
              rate, and the minimum deposit (
              <DocLink href='/docs/query/derivatives-params'>
                <code>derivatives-params</code>
              </DocLink>
              ).
            </p>
            <pre className={styles.step_code}>{`btcli deriv params --json`}</pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>2 · Open</p>
            <p>
              One position per coldkey, subnet, and side. A long and a short on the same subnet
              are independent. Both go through the pallet&apos;s single <code>open</code> call
              with a <code>side</code> argument; the SDK exposes the two sides as{' '}
              <DocLink href='/docs/tx/open-short'>
                <code>open_short</code>
              </DocLink>{' '}
              and{' '}
              <DocLink href='/docs/tx/open-long'>
                <code>open_long</code>
              </DocLink>
              .
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv short --netuid 7 --amount 100 -w my_coldkey
btcli deriv long  --netuid 7 --amount 100 -w my_coldkey`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>3 · Watch</p>
            <p>
              Fee so far, blocks to expiry, and an estimated close value at spot (
              <DocLink href='/docs/query/derivative-positions'>
                <code>derivative-positions</code>
              </DocLink>
              ). The real settlement pays slippage on the closing leg, so expect slightly less.
            </p>
            <pre className={styles.step_code}>{`btcli deriv list -w my_coldkey`}</pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>4 · Close</p>
            <p>
              Atomic: reverse the trade, repay the pool, collect the fee, pay you, return the
              slice — or the whole call reverts (
              <DocLink href='/docs/tx/close-derivative'>
                <code>close</code>
              </DocLink>
              ). Past expiry, anyone may close a position with <code>--owner</code>.
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv close --netuid 7 --side short -w my_coldkey
btcli deriv close --netuid 7 --side long --owner <their-ss58> -w my_coldkey`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>4b · Or roll</p>
            <p>
              To stay in past the 30 days: settle at today&apos;s price and reopen in the same
              transaction (
              <DocLink href='/docs/tx/roll-derivative'>
                <code>roll</code>
              </DocLink>
              ). Loss or profit so far is realized, the fee so far is paid, and what comes back
              is the cushion of a fresh position with a full lifetime. <code>--add</code> puts
              more cushion in. A position is never extended without being marked to market.
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv roll --netuid 7 --side short -w my_coldkey
btcli deriv roll --netuid 7 --side short --add 50 -w my_coldkey`}
            </pre>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What bounds it</h2>
          <p>
            <strong>1x leverage, TAO cushions.</strong> Exposure equals your cushion, so a 20%
            move in alpha moves a 100 τ position by about 20 τ. Your cushion is the most you can
            lose, and it is TAO only: a subnet team cannot post alpha it minted to itself as
            collateral. If the closing trade cannot repay what the position borrowed, the
            position is underwater: you are paid nothing, whatever the pallet still holds goes to
            the pool, and the pool carries the remaining shortfall. That rule is enforced at
            settlement, not inferred from swap quotes.
          </p>
          <p>
            <strong>10% pool cap.</strong> All open positions of one side on one subnet may
            borrow at most 10% of the reserve they lend from. Above that, opens fail with{' '}
            <code>PoolCapExceeded</code> until others close. This keeps the pool&apos;s worst
            case — every position on one side blowing through its cushion — small relative to the
            pool.
          </p>
          <p>
            <strong>30-day expiry.</strong> A position may live for 216,000 blocks. After that
            the chain sweeps it in <code>on_idle</code>, up to 32 per block, and settles it like
            any close; anyone may also close it by hand. You cannot extend — open a new position
            to stay in.
          </p>
          <p>
            <strong>Fee to the pool.</strong> Fixed per day at open, one-day minimum, paid at
            close. A short pays <code>5 τ × phi</code> per day, where <code>phi</code> is the
            share of the pool it lifted: 1% of any pool costs 0.05 τ a day, 1.5 τ over 30 days.
            A long pays 0.02% of its TAO exposure per day: 0.6 τ over 30 days on 100 τ. The two
            sides differ because the pool&apos;s risk differs. A short exposes the pool to a pump,
            and in a constant-product pool the cost of a pump scales with one over the TAO reserve,
            so a fixed TAO amount per unit of pool share is the fair form. A long exposes the pool
            to a crash, which does not depend on pool size. Both constants are about 1.5–2.5×
            the pool&apos;s measured expected loss over a year of Finney prices. Profit comes out
            of the pool; loss goes into it. Over time the fee is what the pool earns for lending.
          </p>
          <p>
            <strong>Dissolution.</strong> If a subnet is dissolved with positions open, settling
            them is the first cleanup phase. Positions are unwound, not settled: the slice goes
            back in kind, your cushion comes back, and no fee is charged.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changed on chain</h2>
          <p>
            <code>pallet-derivatives</code> is added at index 33 with three user calls —{' '}
            <code>open</code>, which takes a <code>side</code> of short or long,{' '}
            <code>close</code>, and <code>roll</code> — plus a root-only{' '}
            <code>sudo_set_params</code> that rejects a zero leverage, pool share, or lifetime.
            Its parameters ship at: shorts and longs enabled, <code>leverage_percent</code> 100,{' '}
            <code>max_pool_share</code> 10%, <code>lifetime_blocks</code> 216,000,{' '}
            <code>short_fee_per_day</code> 5 τ, <code>long_rate_per_day</code> 0.02%,{' '}
            <code>min_deposit_tao</code> 0.1 τ. Root can switch either side off; existing
            positions can always be closed.
          </p>
          <p>
            The subtensor pallet gains a small pool interface for the derivatives pallet:
            price-neutral <code>lift_liquidity</code> and <code>return_liquidity</code>, internal
            buy and sell through the existing balancer swap, and exact-output swaps for the
            buyback. Subnet dissolution gains a <code>DerivativesSettle</code> phase that runs first.
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
            . SDK intents <code>OpenShort</code>, <code>OpenLong</code>,{' '}
            <code>RollPosition</code>, and <code>ClosePosition</code> back the btcli commands. Upgrade the SDK to get{' '}
            <code>btcli deriv</code>:
          </p>
          <pre className={styles.code_block}>{`pip install -U bittensor`}</pre>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
