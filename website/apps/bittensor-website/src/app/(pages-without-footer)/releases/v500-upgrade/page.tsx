import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import snapshot from '../../../../../public/catalog/root-reborn-snapshot.json';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V500 Upgrade — Curated Beta',
  description:
    'V500 enables set_root_weights. Validators curate their dividend baskets under a ' +
    '1/16 concentration cap, and the chain itself now computes the basket index and ' +
    'every fund’s display price, staker yield, and positions — one canonical number ' +
    'for btcli, explorers, and contracts. btcli root list, allocate, claim, and ' +
    'weights are the working surface.',
  alternates: {canonical: '/releases/v500-upgrade'},
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

/**
 * Aggregate USD value of all outstanding β (every validator basket's NAV in τ, marked
 * hourly, times the TAO/USD close of the same hour), sampled every 3 hours since the
 * Root Reborn launch. Sources: TaoMarketCap root-basket aggregate NAV and TAO/USD
 * hourly candles, 2026-08-26.
 */
const USD_M = [
  0.267, 0.29, 0.311, 0.33, 0.363, 0.379, 0.406, 0.43, 0.449, 0.466, 0.493, 0.514,
  0.52, 0.542, 0.56, 0.582, 0.602, 0.619, 0.628, 0.65, 0.671, 0.696, 0.714, 0.732,
  0.749, 0.774, 0.79, 0.801, 0.833, 0.85, 0.874, 0.907, 0.934, 0.955, 0.974, 1.002,
  1.021, 1.066, 1.093, 1.123, 1.145, 1.174, 1.203, 1.215, 1.228, 1.227, 1.262, 1.284,
  1.301, 1.33, 1.313, 1.329, 1.342, 1.348, 1.368, 1.386, 1.412, 1.437, 1.454, 1.462,
  1.484, 1.522, 1.573, 1.586, 1.596, 1.625, 1.604, 1.64, 1.664, 1.683, 1.686, 1.72,
  1.743, 1.76, 1.813, 1.814, 1.861, 1.882, 1.893, 1.895, 1.909, 1.906, 1.876, 1.914,
  1.954, 1.987, 2.017, 2.017, 2.039, 2.05, 2.08, 2.098, 2.118, 2.13, 2.146, 2.168,
  2.188, 2.215, 2.248, 2.26, 2.279, 2.282, 2.326, 2.332, 2.339, 2.372, 2.4, 2.399,
  2.441, 2.452, 2.394, 2.433, 2.465, 2.483, 2.494, 2.509, 2.507, 2.548, 2.56, 2.581,
  2.63, 2.652, 2.708, 2.78, 2.88, 2.927, 2.921, 2.954, 3.057, 3.072, 3.051, 3.146,
  3.136, 3.198, 3.28, 3.378, 3.494, 3.51, 3.642, 3.65, 3.647, 3.754, 3.977, 3.854,
  3.807, 3.725, 3.694, 3.833, 3.837, 3.815, 3.789, 3.751, 3.869, 3.938, 4.127, 4.15,
  4.355, 4.265, 4.279, 4.293, 4.267, 4.388, 4.472, 4.47, 4.538, 4.547, 4.546, 4.634,
  4.517, 4.377, 4.365, 4.405, 4.304, 4.357, 4.472, 4.489, 4.539, 4.583, 4.431, 4.515,
  4.51,
];
const USD_STEP_HOURS = 3;

/** Total β value in USD since launch — the dollar footprint of the basket system. */
const BasketUsdChart = () => {
  const x0 = 80;
  const y0 = 44;
  const w = 560;
  const h = 300;
  const axis = y0 + h;

  const days = USD_M.map((_, i) => (i * USD_STEP_HOURS) / 24);
  const maxDay = days[days.length - 1];
  const hi = Math.max(...USD_M);
  const yHi = hi * 1.08;

  const xFor = (d: number) => x0 + (d / maxDay) * w;
  const yFor = (v: number) => axis - (v / yHi) * h;
  const path = USD_M.map(
    (v, i) => `${i === 0 ? 'M' : 'L'} ${xFor(days[i]).toFixed(1)} ${yFor(v).toFixed(1)}`,
  ).join(' ');

  const yTicks = [0, 1, 2, 3, 4].filter((v) => v <= yHi);
  const xTicks = [0, 7, 14, 21].filter((d) => d <= maxDay + 0.5);
  const last = USD_M[USD_M.length - 1];

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 840 400'
      role='img'
      aria-label={`Total USD value of all outstanding beta across validator baskets over the ${Math.round(
        maxDay,
      )} days since the Root Reborn launch, growing from roughly $270 thousand to $${last.toFixed(
        1,
      )} million.`}
    >
      <text {...GRAPH_TEXT} x='420' y='28' textAnchor='middle' fill={MUTED} fontSize={12}>
        TOTAL β VALUE · USD · ALL FUNDS · SINCE LAUNCH
      </text>
      <line x1={x0} y1={y0} x2={x0} y2={axis} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={axis} x2={x0 + w} y2={axis} stroke={INK} strokeWidth='1' />
      {yTicks.map((v) => (
        <g key={v}>
          <line
            x1={x0}
            y1={yFor(v)}
            x2={x0 + w}
            y2={yFor(v)}
            stroke={FAINT}
            strokeWidth='1'
            strokeDasharray='4 3'
          />
          <text {...GRAPH_TEXT} x={x0 - 8} y={yFor(v) + 3} textAnchor='end' fill={MUTED}>
            {`$${v}M`}
          </text>
        </g>
      ))}
      {xTicks.map((d) => (
        <text key={d} {...GRAPH_TEXT} x={xFor(d)} y={axis + 20} textAnchor='middle' fill={MUTED}>
          {d}
        </text>
      ))}
      <text {...GRAPH_TEXT} x={x0 + w} y={axis + 38} textAnchor='end' fill={MUTED}>
        DAYS SINCE LAUNCH →
      </text>
      <path d={path} fill='none' stroke={INK} strokeWidth='1.5' />
      <text
        {...GRAPH_TEXT}
        x={x0 + w + 10}
        y={yFor(last) + 3}
        fontSize={11}
        fill={GOLD}
      >
        {`$${last.toFixed(2)}M`}
      </text>
    </svg>
  );
};

/** Live fund prices since launch, index-spliced — the same numbers btcli shows. */
const BasketPricesChart = () => {
  const chart = snapshot.basketChart;
  const x0 = 80;
  const y0 = 44;
  const w = 560;
  const h = 340;
  const axis = y0 + h;

  const days = chart.blocks.map((b) => (b - chart.epochBlock) / 7200);
  const maxDay = days[days.length - 1];
  const values = [
    ...chart.index,
    ...chart.funds.flatMap((f) => f.series),
  ].filter((v): v is number => v != null);
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const pad = (hi - lo) * 0.12 || 0.01;
  const yLo = lo - pad;
  const yHi = hi + pad;

  const xFor = (d: number) => x0 + (d / maxDay) * w;
  const yFor = (v: number) => axis - ((v - yLo) / (yHi - yLo)) * h;
  const pathFor = (series: (number | null)[]) =>
    series
      .map((v, i) =>
        v == null
          ? null
          : `${i === 0 || series[i - 1] == null ? 'M' : 'L'} ${xFor(days[i]).toFixed(1)} ${yFor(v).toFixed(1)}`,
      )
      .filter(Boolean)
      .join(' ');

  const yTicks: number[] = [];
  for (let v = Math.ceil(yLo / 0.05) * 0.05; v <= yHi; v += 0.05) {
    yTicks.push(Number(v.toFixed(2)));
  }
  const xTicks = [0, 7, 14, 21].filter((d) => d <= maxDay + 0.5);

  const labeled = [
    {
      name: 'INDEX',
      y: yFor(chart.index[chart.index.length - 1]),
      index: true,
    },
    ...chart.funds.flatMap((fund) => {
      const last = [...fund.series].reverse().find((v): v is number => v != null);
      return last == null
        ? []
        : [
            {
              name: (fund.name ?? `${fund.hotkey.slice(0, 8)}…`).toUpperCase(),
              y: yFor(last),
              index: false,
            },
          ];
    }),
  ].sort((a, b) => a.y - b.y);
  for (let i = 1; i < labeled.length; i++) {
    if (labeled[i].y - labeled[i - 1].y < 16) labeled[i].y = labeled[i - 1].y + 16;
  }

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 840 440'
      role='img'
      aria-label={`The ${chart.funds.length} largest validator baskets priced against the basket index over the ${Math.round(
        maxDay,
      )} days since launch. Every fund's displayed beta rate starts at the index level of its launch day, so distance above the gold index line is cumulative outperformance versus the average basket.`}
    >
      <text {...GRAPH_TEXT} x='420' y='28' textAnchor='middle' fill={MUTED} fontSize={12}>
        LIVE FUND PRICES · τ PER β · INDEX-SPLICED · SINCE LAUNCH
      </text>
      <line x1={x0} y1={y0} x2={x0} y2={axis} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={axis} x2={x0 + w} y2={axis} stroke={INK} strokeWidth='1' />
      {yTicks.map((v) => (
        <g key={v}>
          <line
            x1={x0}
            y1={yFor(v)}
            x2={x0 + w}
            y2={yFor(v)}
            stroke={FAINT}
            strokeWidth='1'
            strokeDasharray='4 3'
          />
          <text {...GRAPH_TEXT} x={x0 - 8} y={yFor(v) + 3} textAnchor='end' fill={MUTED}>
            {v.toFixed(2)}
          </text>
        </g>
      ))}
      {xTicks.map((d) => (
        <text key={d} {...GRAPH_TEXT} x={xFor(d)} y={axis + 20} textAnchor='middle' fill={MUTED}>
          {d}
        </text>
      ))}
      <text {...GRAPH_TEXT} x={x0 + w} y={axis + 38} textAnchor='end' fill={MUTED}>
        DAYS SINCE LAUNCH →
      </text>
      {chart.funds.map((fund) => (
        <path
          key={fund.hotkey}
          d={pathFor(fund.series)}
          fill='none'
          stroke={INK}
          strokeWidth='1'
          opacity='0.55'
        />
      ))}
      <path
        d={pathFor(chart.index)}
        fill='none'
        stroke={GOLD}
        strokeWidth='2'
        strokeDasharray='6 4'
      />
      {labeled.map((l) => (
        <text
          key={l.name}
          {...GRAPH_TEXT}
          x={x0 + w + 10}
          y={l.y + 3}
          fontSize={l.index ? 11 : 10}
          fill={l.index ? GOLD : MUTED}
        >
          {l.name}
        </text>
      ))}
    </svg>
  );
};

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V500 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Curated Beta · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <BasketPricesChart />
          <p className={styles.graph_caption}>
            Largest funds by NAV since launch. Gold is the index (average basket). Height above it is cumulative outperformance versus the average basket.
          </p>
        </section>

        <section className={styles.section}>
          <BasketUsdChart />
          <p className={styles.graph_caption}>
            The same system in dollars: every fund&apos;s NAV marked hourly in τ, times the
            TAO/USD price of the same hour. Three weeks in, outstanding β across all
            validator baskets is worth about $4.5M.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>500</strong> turns on <code>set_root_weights</code>. From the upgrade
            block a root validator can choose how its dividend stream (the yield from root stake) is re-deployed across subnet
            alpha.
          </p>
          <p>
            A new <code>RootWeightsCap</code> of 1/16 is added which limits concentration of the validator's dividend stream. Validators must spread across
            at least 16 destinations. <code>btcli root</code> is the working surface: list,
            allocate, claim, and weights.
          </p>
          <p>
            The scoreboard itself also moves on chain: the runtime now computes the basket
            index and every fund&apos;s index-spliced display price, staker yield, and
            positions, so btcli, explorers, and contracts all read the same canonical
            numbers instead of each interpreting raw state. This release supersedes the
            unshipped 449 tag; everything proposed there ships here.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Allocate, accrue, claim</h2>
          <p className={styles.graph_caption}>
            As with the Root Reborn update, root principal stays as root stake in TAO, yield accrues as beta (a share in a basket of alpha tokens).
            These can be 'claimed' into TAO which folds the yield back into stake.
          </p>

          <div className={styles.step}>
            <p className={styles.step_title}>1 · List</p>
            <p>
              <code>btcli root list</code> shows each root validator's basket, sorted
              by NAV.
            </p>
            <pre className={styles.step_code}>
              {`btcli root list`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>2 · Allocate</p>
            <p>
              Deploys τ from free balance into the chosen validator&apos;s basket and credits
              β immediately.
            </p>
            <pre className={styles.step_code}>
              {`btcli root allocate`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>3 · Claim</p>
            <p>
              Sells your accrued β back to the fund and folds the TAO into your root stake on
              that validator (
              <DocLink href='/docs/tx/claim-root-with-hotkey'>
                <code>claim_root_with_hotkey</code>
              </DocLink>
              ). Principal is never touched.
            </p>
            <pre className={styles.step_code}>
              {`btcli root claim`}
            </pre>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Setting weights</h2>
          <p className={styles.graph_caption}>
            <code>btcli root weights</code> writes an equal-weight vector. A 16-way split sits
            on the cap; add a 17th and it renormalizes to 1/17. Netuid 0 is a valid destination
            — that slice is held as TAO.
          </p>

          <div className={styles.step}>
            <p className={styles.step_title}>1 · Register</p>
            <p>
              Burn-based seat. This upgrade writes ``MinBurn`` and
              ``ImmunityPeriod`` on root. A full senate evicts the
              lowest-staked non-immune member.
            </p>
            <pre className={styles.step_code}>
              {`btcli subnets burn-cost 0
btcli root register`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>2 · Stake</p>
            <p>
              Root principal on your hotkey. Needed to keep the seat and to set weights.
            </p>
            <pre className={styles.step_code}>
              {`btcli stake add --netuid 0 --amount 1000 --hotkey <your hotkey>`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>3 · Set</p>
            <p>
              Replace the allocation. Hotkey signs. 16 destinations, 1/16 each (
              <DocLink href='/docs/tx/set-root-weights'>
                <code>set_root_weights</code>
              </DocLink>
              ).
            </p>
            <pre className={styles.step_code}>
              {`btcli root weights set --netuids 0,1,3,4,5,8,9,11,13,19,21,23,34,51,64,77`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>4 · Add</p>
            <p>17 destinations, 1/17 each.</p>
            <pre className={styles.step_code}>
              {`btcli root weights add --netuid 88`}
            </pre>
          </div>

          <div className={styles.step}>
            <p className={styles.step_title}>5 · Remove</p>
            <p>Drop one, renormalize.</p>
            <pre className={styles.step_code}>
              {`btcli root weights remove --netuid 8`}
            </pre>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>One price, defined by the chain</h2>
          <p>
            Raw fund prices (<code>NAV / β supply</code>) carry arbitrary historical
            baselines, so they are not comparable across funds of different ages. Until now
            the fix — splicing every fund onto a common index at its birth — lived in a
            frozen table inside the SDK: an interpretation only btcli shared. V500 makes
            that convention chain state.
          </p>
          <p>
            Every fund gets a frozen <code>BetaBaseline</code>, stamped once at its first
            share mint. The stamp marks both the fund&apos;s own price and the index level
            at <em>realizable</em> quotes — what selling would actually fetch, bounded by
            pool depth — so a baseline cannot be poisoned by briefly pumping a thin
            pool&apos;s spot price. A migration seeds the baselines the SDK has been
            displaying, so every number is continuous through the upgrade. Baselines live
            exactly as long as their fund: fully claimed out means retired, and a revival
            stamps fresh.
          </p>
          <p>
            The runtime also keeps <code>BasketTwr</code>, a per-fund total-return
            accumulator that compounds with every dividend mint. Staker return over any
            window is a pure ratio of two samples — the canonical answer to &quot;if I
            staked τ1 here, what did I earn?&quot;
          </p>
          <p>
            Five new <code>betaBasket</code> runtime APIs serve the whole surface:{' '}
            <code>get_all_beta_pricing</code> (the leaderboard in one call, every fund
            marked against one index sweep), <code>get_beta_pricing</code>,{' '}
            <code>get_beta_index</code> (the live bag and stake index levels), and{' '}
            <code>get_beta_position</code> / <code>get_beta_portfolio</code> (a
            staker&apos;s holdings in display units, where{' '}
            <code>display_beta × display_price</code> is the position&apos;s value). On
            v500 nodes, <code>btcli root list</code> is a pass-through of these numbers;
            its local math remains only as a fallback for older nodes and pre-upgrade
            history.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changed on chain</h2>
          <p>
            The migration sets <code>RootWeightSettingEnabled</code> to true and writes{' '}
            <code>RootWeightsCap</code> at 4096/65535. The cap is checked on the submitted
            values; stored vectors from before the upgrade are left alone until the next write.
            Governance can move the cap (
            <code>AdminUtils::sudo_set_root_weights_cap</code>) or switch curation back off.
            After that, <code>RootWeightSettingDisabled</code> means the switch is off, not that
            the launch gate is still closed. On a chain with fewer destinations than the cap
            demands, the check is skipped.
          </p>
          <p>
            The upgrade also adds the standardized pricing layer above:{' '}
            <code>BetaBaseline</code> and <code>BasketTwr</code> storage, baseline stamping
            at first mint, a migration seeding the SDK&apos;s historical baselines, and the
            five <code>betaBasket</code> pricing APIs (runtime API v3).
          </p>
          <p>
            SDK 11.3.0 ships with the runtime. That is the version that has{' '}
            <code>btcli root list</code>, <code>allocate</code>, <code>claim</code>, and{' '}
            <code>weights</code>, plus the index-spliced β rate — read straight from the
            chain&apos;s pricing APIs on upgraded nodes. Upgrade:
          </p>
          <pre className={styles.code_block}>{`pip install -U bittensor`}</pre>
          <p>
            <code>SetRootWeights</code> preflights the cap on the quantized values it will
            submit. Fund reads:{' '}
            <DocLink href='/docs/query/root-baskets'>
              <code>root-baskets</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/validator-basket-summary'>
              <code>validator-basket-summary</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/basket-position'>
              <code>basket-position</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/root-basket-portfolio'>
              <code>root-basket-portfolio</code>
            </DocLink>
            . Guide:{' '}
            <DocLink href='/docs/guides/root-reborn'>Root Reborn</DocLink>.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
