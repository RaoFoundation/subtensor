import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'Conviction Normalization',
  description:
    'The subnet ownership gate now measures one hotkey alone against an 18% conviction ' +
    'threshold, restoring the TAO cost of a takeover to above pre-v446 levels.',
  alternates: {canonical: '/releases/conviction-normalization'},
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

// Cost, in TAO, to buy and lock enough alpha to take over each mainnet subnet
// that is at least one year old (88 subnets, Finney block 8,844,344). Each
// curve sorts its subnets from cheapest to dearest; y is log-scale. Costs are
// real AMM quotes (fee and slippage included) via sim_swap_tao_for_alpha.
const COST_PATHS = {
  pre446:
    'M 70.0 235.3 L 77.8 233.2 L 85.5 230.6 L 93.3 227.6 L 101.1 224.6 L 108.8 222.9 L 116.6 217.3 L 124.4 216.5 L 132.1 213.3 L 139.9 212.9 L 147.6 210.8 L 155.4 210.0 L 163.2 210.0 L 170.9 206.1 L 178.7 204.7 L 186.5 203.4 L 194.2 203.2 L 202.0 203.1 L 209.8 202.8 L 217.5 202.4 L 225.3 202.1 L 233.1 202.0 L 240.8 201.1 L 248.6 200.4 L 256.4 199.9 L 264.1 196.9 L 271.9 196.6 L 279.6 194.9 L 287.4 193.6 L 295.2 193.4 L 302.9 191.7 L 310.7 191.1 L 318.5 190.3 L 326.2 190.1 L 334.0 189.5 L 341.8 188.9 L 349.5 186.1 L 357.3 184.9 L 365.1 182.6 L 372.8 182.6 L 380.6 182.2 L 388.4 181.2 L 396.1 178.6 L 403.9 178.3 L 411.6 177.2 L 419.4 175.7 L 427.2 173.9 L 434.9 172.6 L 442.7 170.4 L 450.5 170.1 L 458.2 168.5 L 466.0 165.4 L 473.8 164.3 L 481.5 160.1 L 489.3 159.1 L 497.1 158.9 L 504.8 157.9 L 512.6 153.3 L 520.4 152.5 L 528.1 151.3 L 535.9 149.3 L 543.6 149.2 L 551.4 148.8 L 559.2 147.7 L 566.9 145.6 L 574.7 143.4 L 582.5 140.0 L 590.2 139.9 L 598.0 134.6 L 605.8 128.5 L 613.5 120.3 L 621.3 118.0 L 629.1 117.4 L 636.8 116.8 L 644.6 112.8 L 652.4 108.4 L 660.1 102.4 L 667.9 97.1 L 675.6 96.8 L 683.4 92.9 L 691.2 87.8 L 698.9 87.6 L 706.7 77.0 L 714.5 69.1 L 722.2 45.1 L 730.0 39.7',
  v446: 'M 70.0 270.4 L 77.7 264.9 L 85.3 261.4 L 93.0 257.8 L 100.7 249.9 L 108.4 235.3 L 116.0 232.8 L 123.7 227.8 L 131.4 226.5 L 139.1 225.1 L 146.7 223.2 L 154.4 220.8 L 162.1 216.9 L 169.8 213.3 L 177.4 212.9 L 185.1 212.8 L 192.8 211.5 L 200.5 211.3 L 208.1 210.0 L 215.8 210.0 L 223.5 210.0 L 231.2 209.8 L 238.8 207.1 L 246.5 205.2 L 254.2 204.5 L 261.9 203.4 L 269.5 203.3 L 277.2 203.1 L 284.9 202.8 L 292.6 202.4 L 300.2 200.9 L 307.9 200.4 L 315.6 199.9 L 323.3 196.6 L 330.9 194.9 L 338.6 194.7 L 346.3 194.5 L 354.0 193.4 L 361.6 193.0 L 369.3 191.1 L 377.0 188.9 L 384.7 188.3 L 392.3 187.3 L 400.0 186.1 L 407.7 184.9 L 415.3 182.6 L 423.0 182.2 L 430.7 179.5 L 438.4 178.3 L 446.0 177.3 L 453.7 175.9 L 461.4 174.3 L 469.1 173.9 L 476.7 172.6 L 484.4 170.4 L 492.1 170.1 L 499.8 169.6 L 507.4 168.5 L 515.1 165.4 L 522.8 163.8 L 530.5 157.9 L 538.1 153.3 L 545.8 149.3 L 553.5 149.2 L 561.2 148.8 L 568.8 145.6 L 576.5 143.4 L 584.2 140.0 L 591.9 139.9 L 599.5 134.6 L 607.2 128.5 L 614.9 121.8 L 622.6 120.3 L 630.2 119.3 L 637.9 117.4 L 645.6 116.8 L 653.3 108.7 L 660.9 108.4 L 668.6 106.7 L 676.3 102.4 L 684.0 97.1 L 691.6 92.9 L 699.3 87.8 L 707.0 87.6 L 714.7 69.1 L 722.3 45.1 L 730.0 39.7',
  next: 'M 70.0 236.4 L 80.0 229.8 L 90.0 229.8 L 100.0 212.6 L 110.0 210.1 L 120.0 202.9 L 130.0 198.4 L 140.0 196.4 L 150.0 194.5 L 160.0 192.9 L 170.0 182.7 L 180.0 182.6 L 190.0 180.3 L 200.0 180.1 L 210.0 179.3 L 220.0 177.9 L 230.0 175.6 L 240.0 175.5 L 250.0 174.6 L 260.0 173.7 L 270.0 172.5 L 280.0 171.9 L 290.0 171.3 L 300.0 170.0 L 310.0 169.1 L 320.0 169.0 L 330.0 168.5 L 340.0 165.7 L 350.0 163.8 L 360.0 163.2 L 370.0 162.5 L 380.0 162.1 L 390.0 160.8 L 400.0 157.3 L 410.0 155.0 L 420.0 152.7 L 430.0 152.3 L 440.0 151.8 L 450.0 151.7 L 460.0 150.8 L 470.0 146.8 L 480.0 145.2 L 490.0 139.6 L 500.0 139.0 L 510.0 137.6 L 520.0 137.3 L 530.0 137.3 L 540.0 135.8 L 550.0 134.2 L 560.0 133.3 L 570.0 131.2 L 580.0 128.9 L 590.0 127.2 L 600.0 120.1 L 610.0 109.0 L 620.0 107.5 L 630.0 104.7 L 640.0 101.0 L 650.0 96.6 L 660.0 92.9 L 670.0 87.4 L 680.0 85.2 L 690.0 72.6 L 700.0 67.5 L 710.0 62.5 L 720.0 55.5 L 730.0 45.1',
} as const;

const TakeoverCostGraph = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='Per-subnet takeover cost in TAO under three rule sets, sorted from cheapest to dearest on a log scale. The v446 curve sits lowest, the pre-446 curve above it, and the new 18% single-hotkey curve highest.'
  >
    {/* Axes */}
    <line x1='70' y1='30' x2='70' y2='290' stroke='rgb(41, 41, 41)' strokeWidth='1' />
    <line x1='70' y1='290' x2='730' y2='290' stroke='rgb(41, 41, 41)' strokeWidth='1' />
    <text {...GRAPH_TEXT} x='730' y='310' textAnchor='end'>
      SUBNETS, CHEAPEST TO DEAREST
    </text>
    <text {...GRAPH_TEXT} x='62' y='293' textAnchor='end'>
      100τ
    </text>
    <text {...GRAPH_TEXT} x='62' y='210' textAnchor='end'>
      1kτ
    </text>
    <text {...GRAPH_TEXT} x='62' y='127' textAnchor='end'>
      10kτ
    </text>
    <text {...GRAPH_TEXT} x='62' y='44' textAnchor='end'>
      100kτ
    </text>

    {/* Log gridlines */}
    <line
      x1='70'
      y1='206.7'
      x2='730'
      y2='206.7'
      stroke='rgba(41, 41, 41, 0.15)'
      strokeWidth='1'
      strokeDasharray='2 4'
    />
    <line
      x1='70'
      y1='123.3'
      x2='730'
      y2='123.3'
      stroke='rgba(41, 41, 41, 0.15)'
      strokeWidth='1'
      strokeDasharray='2 4'
    />

    {/* v446 rules (today): sum gate on eligible alpha */}
    <path
      d={COST_PATHS.v446}
      fill='none'
      stroke='rgba(41, 41, 41, 0.45)'
      strokeWidth='1'
      strokeDasharray='4 3'
    />
    <text {...GRAPH_TEXT} x='120' y='282' fill='rgba(41, 41, 41, 0.6)'>
      V446 (TODAY)
    </text>

    {/* pre-446 rules: sum gate on outstanding alpha */}
    <path d={COST_PATHS.pre446} fill='none' stroke='rgba(41, 41, 41, 0.45)' strokeWidth='1' />
    <text {...GRAPH_TEXT} x='76' y='224' fill='rgba(41, 41, 41, 0.6)'>
      PRE-446
    </text>

    {/* this upgrade: 18% on one hotkey alone */}
    <path d={COST_PATHS.next} fill='none' stroke='#d15168' strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='96' y='112' fill='#d15168'>
      THIS UPGRADE: 18% ON ONE HOTKEY
    </text>
    <text {...GRAPH_TEXT} x='500' y='34' fill='#d15168'>
      +21 SUBNETS OFF THE CHART
    </text>
  </svg>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>Conviction Normalization</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            The 18% single-hotkey ownership gate · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            The <DocLink href='/releases/v446-upgrade'>v446 upgrade</DocLink> changed how the
            conviction ownership gate is calculated. It based the threshold on{' '}
            <em>eligible alpha</em> — outstanding alpha minus protocol-owned and burned alpha —
            instead of outstanding alpha. The accounting is more faithful, but it had a side
            effect: the denominator shrank, so the gate fell. The TAO a challenger needed to take
            over a subnet dropped on most networks, and on some it dropped a lot.
          </p>
          <p>
            This upgrade brings the expected TAO cost of a takeover back to — and above — its
            pre-446 level. It does so by turning exactly two parameters:
          </p>
          <ol>
            <li>
              <strong>The gate measures one hotkey alone.</strong> Previously the threshold was
              compared against the <em>sum</em>{' '}
              of all conviction on the subnet, while the winner
              was simply the hotkey with the most. Anyone&apos;s locks — including the
              owner&apos;s own defensive locks — counted toward a challenger&apos;s quorum. Now
              the winning hotkey must clear the bar with its own conviction only.
            </li>
            <li>
              <strong>The threshold rises from 10% to 18%.</strong>
            </li>
          </ol>
          <Code
            language='text'
            code={`eligible alpha = SubnetAlphaOut - SubnetProtocolAlpha - AlphaBurned

ownership transfers when, on a subnet at least one year old,
the highest-conviction hotkey's OWN conviction > 18% × eligible alpha`}
          />
          <p>
            Coalitions still work the natural way: backers who want to support a challenger lock
            toward the challenger&apos;s hotkey, and their conviction lands in that hotkey&apos;s
            aggregate. What no longer works is winning with a small position because unrelated
            lockers happened to push the subnet-wide total over the line.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Why 18%</h2>
          <p>
            18% is already the network&apos;s ownership number. The{' '}
            <DocLink href='/docs/concepts/emissions'>subnet owner cut</DocLink>{' '}
            — the share of a subnet&apos;s emissions paid to its owner — is 18%. The bar to claim
            ownership now matches the reward of holding it: to earn the owner&apos;s 18% of
            emissions, you must first hold 18% of the subnet&apos;s eligible alpha as matured
            conviction on a single hotkey. One number describes both sides of the trade, instead
            of a 10% figure with no economic anchor.
          </p>
          <p>
            The symmetry also gives owners a clean defense rule. To win, a challenger must clear
            18% <em>and</em>{' '}
            out-hold the owner&apos;s own conviction. An owner&apos;s locks mature
            instantly while a challenger&apos;s take about 43 days, so an owner who watches their
            subnet can always lock enough to stay ahead before a challenge matures.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What this does to takeover cost</h2>
          <p>
            We priced a takeover of every mainnet subnet that has passed the one-year age gate —
            88 subnets — under three rule sets: pre-446, v446 as it runs today, and this upgrade.
            Each cost is a real AMM quote: the TAO needed to buy enough alpha from the pool, with
            fees and slippage, so that one fresh hotkey both beats the current conviction leader
            and clears the gate.
          </p>
          <TakeoverCostGraph />
          <p className={styles.graph_caption}>
            Takeover cost per subnet under each rule set, sorted from cheapest to dearest,
            log scale. Finney block 8,844,344.
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Rule set</th>
                <th>Median cost</th>
                <th>Subnets under 1,000 τ</th>
                <th>Not fillable in one AMM buy</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Pre-446 (10% of outstanding, sum)</td>
                <td>2,182 τ</td>
                <td>13</td>
                <td>2</td>
              </tr>
              <tr>
                <td>v446 today (10% of eligible, sum)</td>
                <td>1,765 τ</td>
                <td>23</td>
                <td>1</td>
              </tr>
              <tr>
                <td>This upgrade (18% of eligible, one hotkey)</td>
                <td>3,915 τ</td>
                <td>5</td>
                <td>21</td>
              </tr>
            </tbody>
          </table>
          <p>
            The v446 curve sits below the pre-446 curve across the board — that is the accidental
            lowering this upgrade corrects. The new curve sits above both. The median cost roughly
            doubles against pre-446, the number of subnets takeable for under 1,000 TAO falls from
            23 to 5, and on 21 subnets the required 18% position cannot be assembled from the pool
            at all: buying that much alpha in one market exhausts what the AMM will sell at any
            price.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>If you own a subnet</h2>
          <p>
            Nothing about your locks or conviction resets, and no takeover becomes easier — every
            change in this upgrade raises the bar. For your subnet to change hands, all of the
            following must now hold at once:
          </p>
          <ul>
            <li>The subnet is at least one year old.</li>
            <li>
              A single hotkey holds more than 18% of eligible alpha as conviction — locked to that
              one hotkey, not spread across the subnet.
            </li>
            <li>
              That conviction has matured. A challenger&apos;s conviction approaches its locked
              mass on the{' '}
              <DocLink href='/code/pallets/subtensor/src/lib.rs#L1654'>MaturityRate</DocLink>{' '}
              timescale, about 43 days to most of full weight — the position is visible on chain
              through <DocLink href='/docs/query/subnet-convictions'>subnet-convictions</DocLink>{' '}
              the entire time.
            </li>
            <li>
              The challenger out-holds you. Your locks mature instantly, so out-locking a
              challenger&apos;s position at any point before it matures ends the attempt.
            </li>
          </ul>
          <p>
            In practice this delays the effects of conviction. On every mainnet subnet past the
            age gate today, the conviction leader is the owner&apos;s own hotkey — no challenger
            leads anywhere, let alone above 18%. A challenge that starts now must buy against the
            pool, then wait out maturity in full view. Conviction remains the path by which a
            committed, majority-scale backer can eventually earn a neglected subnet — that is by
            design — but it is no longer a lever that a small, well-timed position can pull.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Upgrade checklist</h2>
          <ul>
            <li>
              Recompute takeover dashboards: the gate is per-hotkey conviction versus 18% of
              eligible alpha; subnet-wide totals no longer gate anything.
            </li>
            <li>
              Owners who want a standing defense: keep more conviction locked on the owner hotkey
              than any challenger holds. Owner locks mature instantly, so topping up at any point
              before a challenge matures ends it.
            </li>
            <li>
              The <DocLink href='/docs/query/subnet-convictions'>subnet-convictions</DocLink> read
              now projects each hotkey against the 18% bar directly.
            </li>
          </ul>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
