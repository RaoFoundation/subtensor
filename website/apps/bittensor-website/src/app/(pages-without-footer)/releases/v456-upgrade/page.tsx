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
    'subnet’s own pool. One position per subnet and one call to add to it, take from it, ' +
    'flip it, or roll it; a 90-day term. No synthetic tokens, nothing minted or burned, one ' +
    'per-day borrow fee for both sides paid to the pool. btcli deriv short, long, list, ' +
    'closable, and close are the working surface.',
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
  // First whole-percent move at which the long's payout is zero: near a halving at 2x.
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
      aria-label='Value returned for a 100 TAO cushion, plotted against the alpha price move from minus 100 to plus 100 percent. The short line, at 1x, rises as alpha falls and reaches zero near a doubling. The long line, at 2x, rises twice as fast as alpha rises and reaches zero near a halving. Both cross 100 TAO at no move.'
    >
      <text {...GRAPH_TEXT} x='420' y='28' textAnchor='middle' fill={MUTED} fontSize={12}>
        WHAT 100 τ COMES BACK AS · SHORT 1x · LONG 2x · CLOSED AFTER ONE DAY
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
        LONG · 2x
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
        HOLD · NO EXPIRY · FEE ACCRUES
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
            alpha rises. Both hand back the cushion at no move, minus the day of fee booked at
            the add. Once the
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
            by a TAO deposit, for a 90-day term. A short profits when alpha falls; a long profits
            when alpha rises.
          </p>
          <p>
            You hold one position per subnet, and you move it with one call. Add on the side you
            hold and it grows. Add on the other side and that much comes off, paid out at
            today&apos;s price. Add more than you hold and it flips. Add on your side after the
            term is up and it rolls: settled at today&apos;s price and reopened for another 90
            days. <code>close</code> settles all of it.
          </p>
          <p>
            There are no synthetic tokens and no order book. Every position is built from the
            subnet pool&apos;s own reserves: the chain lifts a slice of the pool sized from your
            deposit (one times it for a short, two times for a long), trades that slice through
            the ordinary staking swap, and reverses the trade when you settle. Nothing is minted,
            nothing is burned. The pool earns a borrow fee fixed per day for each slice when it
            is added: 0.05% of the slice&apos;s TAO exposure a day, the same law on both sides.
          </p>
          <p>
            <code>btcli deriv</code> is the working surface: <code>short</code>,{' '}
            <code>long</code>, <code>list</code>, <code>closable</code>, <code>close</code>, and{' '}
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
              cushion at 2x would lift 2%.) Both sides shrink by the same share, so the price
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
              At any block — by you, or by anyone once the position has run its 90 days or can
              no longer pay a day of fee — the trade is reversed: a short
              buys its 2,000 α back, a long sells its alpha and repays the 100 τ. The slice goes
              home together with the fee, added to the pool without moving the price. You get
              your cushion back, plus or minus the move, minus the fee. Adding the other side
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
            sale started, and the pool is 0.05 τ richer for each day the position was open
            (0.05% a day of the 100 τ this short has in play).
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
              Whether each side is enabled, the leverage ceiling per side, the pool cap,
              the fee rate, the term, and the minimum deposit (
              <DocLink href='/docs/query/derivatives-params'>
                <code>derivatives-params</code>
              </DocLink>
              ). With <code>--netuid</code>, also whether root has paused, re-capped, or
              re-priced that one subnet (
              <DocLink href='/docs/query/derivatives-subnet-override'>
                <code>derivatives-subnet-override</code>
              </DocLink>
              ).
            </p>
            <pre className={styles.step_code}>{`btcli deriv params --netuid 7 --json`}</pre>
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
              the rest opens on the new side. Same side after the 90 days: the position is
              settled at today&apos;s price and a new one opens from the amount, a roll.
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv short --netuid 7 --amount 100 -w my_coldkey                # open a short
btcli deriv short --netuid 7 --amount 50 -w my_coldkey                 # add to it
btcli deriv long  --netuid 7 --amount 30 -w my_coldkey                 # take 30 τ off it
btcli deriv long  --netuid 7 --amount 300 --leverage 2 -w my_coldkey   # flip to a long
btcli deriv long  --netuid 7 --amount 100 --leverage 2 -w my_coldkey   # after day 90: roll it`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>3 · Watch</p>
            <p>
              Fee so far, days to expiry, estimated equity, and health (owner-only, or closable
              by anyone) (
              <DocLink href='/docs/query/derivative-positions'>
                <code>derivative-positions</code>
              </DocLink>
              ). Equity prices the closing leg on a constant-product curve; the chain&apos;s own
              quote decides. <code>closable</code> lists every position on a subnet that anyone
              may close, expired or unhealthy, lowest equity first.
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv list -w my_coldkey
btcli deriv closable --netuid 7`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>4 · Close</p>
            <p>
              Atomic: reverse the trade, repay the pool, collect the fee, pay you, return the
              slice — or the whole call reverts (
              <DocLink href='/docs/tx/close-derivative'>
                <code>close</code>
              </DocLink>
              ). Once a position has expired, anyone may close it with <code>--owner</code> and
              is paid one day of fee; the owner gets the rest. Once its equity no longer covers
              one day of fee, anyone may close it and is paid the fee owed, at least one day of
              it; the owner gets nothing. Add cushion before that to keep it healthy.
            </p>
            <pre className={styles.step_code}>
              {`btcli deriv close --netuid 7 -w my_coldkey
btcli deriv close --netuid 7 --owner <their-ss58> -w my_coldkey`}
            </pre>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What bounds it</h2>
          <p>
            <strong>Leverage you choose on each add, under a ceiling root sets: 1x on shorts,
            2x on longs.</strong> At 1x a short&apos;s exposure equals its cushion, so a 20%
            move in alpha moves a 100 τ short by about 20 τ and a doubling wipes it. At 2x a
            long&apos;s exposure is twice its cushion: a 20% move is worth about 40 τ and a
            halving wipes it. In general a position at leverage L is wiped by a move of 1/L
            against it. The long ceiling is 2x because at 1x a long can never cost the pool
            anything and does nothing a spot buy does not; at 2x it is a real instrument whose
            worst case for the pool, a halving, is as rare as a doubling is for shorts. Both
            ceilings are dials; the protocol is built for higher ones later. Your cushion is the
            most you can lose, and it is TAO unless root switches alpha cushions on for a side
            (they ship off): a subnet team cannot post alpha it minted to itself as collateral.
            If the closing trade cannot repay what the position borrowed, the
            position is underwater: you are paid nothing, whatever the pallet still holds goes to
            the pool, and the pool carries the remaining shortfall. That rule is enforced at
            settlement, not inferred from swap quotes.
          </p>
          <p>
            <strong>10% pool cap.</strong> All open positions of one side on one subnet may
            borrow at most 10% of the reserve they lend from. Above that, opens fail with{' '}
            <code>PoolCapExceeded</code> until others close. This keeps the pool&apos;s worst
            case — every position on one side blowing through its cushion — small relative to the
            pool. Root can raise or lower the cap, pause a side chain-wide, or do either for a
            single subnet without touching the rest.
          </p>
          <p>
            <strong>A 90-day term.</strong> Every position expires <code>lifetime_blocks</code>{' '}
            (648,000 blocks, 90 days) after its first add. Adding does not move it. After it,
            anyone may close the position for one day of its fee and the owner is paid the rest
            as at any close; the owner&apos;s own add on the same side rolls it instead, settling
            at today&apos;s price and reopening from the new deposit for another 90 days, in one
            transaction. Expiry is a forced mark to market, not a penalty: what it buys the pool
            is that no slice of liquidity can be held out for good, and every position must
            re-enter through the cap and the ceilings in force that day.
          </p>
          <p>
            <strong>Health inside the term.</strong> A position&apos;s equity is what a close
            now would pay: cushion plus proceeds, less the debt at the pool&apos;s quote, less
            the fee owed. While that covers one more day of fee the position is healthy and
            owner-only. Below it, anyone may close it and is paid the fee owed plus whatever is
            left after the pool is repaid, topped up by the pool to one day of fee if less. The
            buffer is what makes the liquidation pay for itself: at the moment a position
            becomes closable it still holds about a day of fee, so the bounty comes out of the
            position, and the pool only pays the floor when a price jump takes a position
            straight to underwater. The chain runs no sweep; closing what is expired or unhealthy
            is permissionless work. Adding cushion restores health.
          </p>
          <p>
            <strong>One fee, both sides.</strong> Each add fixes a per-day rate for its slice,
            books one day of it at once, and from then on the position&apos;s summed rate
            accrues per block; every settlement pays what is owed. The rate is{' '}
            <code>rate_per_day</code>, 0.05%, times the slice&apos;s TAO exposure, whichever
            side: a 100 τ short at 1x pays 0.05 τ a day, 4.5 τ over its term; a 100 τ long at 2x
            has 200 τ in play and pays 0.1 τ a day, 9 τ over its term. Leverage costs in
            proportion to what it borrows, and pool size and pool share do not enter. The fee is
            a rent on the pool&apos;s liquidity, not an option premium; what protects the pool
            from a position that turns dangerous is the cap, the ceilings, and the term. Root can
            set a different rate for one subnet whose pool the flat rate underprices. Profit comes
            out of the pool; loss goes into it.
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
            <strong>Dissolution.</strong> If a subnet is dissolved with positions open, settling
            them is the first cleanup phase, before any staker is paid. Dissolution is a forced
            close of every position at that block&apos;s price, done as one atomic swap: the
            alpha all shorts owe is netted against the alpha all longs hold, and only the
            difference is quoted against the pool, exactly as its swap would price it. That one
            price is fixed before the first position settles and every position settles at it.
            A short&apos;s alpha debt is charged at that price, a long&apos;s alpha is credited
            at it, the fee is paid, and the rest is yours. A short that is in the money is paid
            its gain first; with a lone position open, dissolution pays what <code>close</code>{' '}
            would have paid in that block.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changed on chain</h2>
          <p>
            <code>pallet-derivatives</code> is added at index 33 with two user calls —{' '}
            <code>add</code>, which takes a <code>side</code> of short or long, an amount and a{' '}
            <code>leverage_percent</code>, and <code>close</code> — plus two root-only calls:{' '}
            <code>sudo_set_params</code>, which rejects a zero leverage ceiling, pool share,
            fee rate, or lifetime, and <code>sudo_set_subnet_override</code>, which pauses a
            side or replaces the cap or the fee rate on one subnet. Its parameters ship at:
            shorts and longs enabled, alpha cushions off on both sides,{' '}
            <code>max_short_leverage_percent</code> 100, <code>max_long_leverage_percent</code>{' '}
            200, <code>max_pool_share</code> 10%, <code>rate_per_day</code> 0.05%,{' '}
            <code>lifetime_blocks</code> 648,000, <code>min_deposit_tao</code> 0.1 τ. Every one
            is a dial root can turn later; a position keeps the fee rate and the expiry it has,
            and its next add is checked against the new values. A position is one record per
            coldkey and subnet, every field a sum over its adds, with a fee ledger of{' '}
            <code>fee_per_day</code> and <code>fee_accrued</code> and an{' '}
            <code>expires_at</code>. The cushion is a <code>Cushion</code> of TAO, alpha, and
            the hotkey the alpha returns to; the <code>add</code> call takes a{' '}
            <code>Deposit</code> of <code>Tao</code> or <code>Alpha</code>, and the alpha path
            is switched off until root turns it on per side. Existing positions can always be
            closed, whatever is paused.
          </p>
          <p>
            The subtensor pallet gains a small pool interface for the derivatives pallet:
            price-neutral <code>lift_liquidity</code> and <code>return_liquidity</code>, internal
            buy and sell through the existing balancer swap, and exact-output swaps for the
            buyback. Subnet dissolution gains a <code>DerivativesSettle</code> phase that runs
            first and closes every position as one net swap at one price, emitted as{' '}
            <code>DissolutionPriced</code>. The emission price EMA now reads <code>get_emission_alpha_price</code>, the spot
            price with the long-side footprint added back to the alpha reserve; with no longs
            open it is the spot price exactly.
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
