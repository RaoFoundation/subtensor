import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import snapshot from '../../../../../public/catalog/root-reborn-snapshot.json';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'Root Reborn — The V441 Upgrade',
  description:
    'TAO is a productive asset. Root Reborn turns root yield from passive into managed — ' +
    'validator-curated baskets of subnet alpha, held until claimable.',
  alternates: {canonical: '/releases/v441-upgrade'},
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

const fmt = new Intl.NumberFormat('en-US', {maximumFractionDigits: 0});
const pct = (x: number, digits = 1) => `${(x * 100).toFixed(digits)}%`;
const taoUsd = snapshot.taoUsd;

/** Compact USD for stock figures: $1.02B, $188M, $184k. */
const usd = (tao: number) => {
  const dollars = tao * taoUsd;
  const abs = Math.abs(dollars);
  if (abs >= 1e9) return `$${(dollars / 1e9).toFixed(2)}B`;
  if (abs >= 1e6) return `$${(dollars / 1e6).toFixed(0)}M`;
  if (abs >= 1e3) return `$${(dollars / 1e3).toFixed(0)}k`;
  return `$${dollars.toFixed(0)}`;
};

const rootDividendsPerDay = fmt.format(snapshot.rootDividendsTaoPerDay);
/** The root stream measured against fresh daily pool emission — the scale of the drain. */
const streamVsInject = pct(snapshot.rootDividendsTaoPerDay / snapshot.taoPerDayIntoPools, 0);

/** Redistribution neutralizes the drain: net sell flow removed equals the stream itself. */
const sellFlowRemovedPerDay = snapshot.rootDividendsTaoPerDay;

const GOLD_SOFT = 'rgba(224, 165, 63, 0.14)';
const FAINT = 'rgba(41, 41, 41, 0.12)';

/** Cumulative-yield frontier: force-sold linear ceiling vs compounding managed curves. */
const YieldFrontierDiagram = () => {
  const x0 = 70;
  const y0 = 40;
  const w = 560;
  const h = 200;
  const baseline = y0 + h;
  const yMax = 0.2;
  const r = snapshot.rootYieldApr;
  // Illustrative annualized allocation premiums, not a projection of any subnet.
  const premiums = [0.05, 0.1];

  const xFor = (t: number) => x0 + t * w;
  const yFor = (v: number) => baseline - (v / yMax) * h;
  const compounded = (rate: number, t: number) => Math.exp(rate * t) - 1;
  const curvePoints = (rate: number) => {
    const pts: string[] = [];
    for (let i = 0; i <= 48; i++) {
      const t = i / 48;
      pts.push(`${xFor(t).toFixed(1)} ${yFor(compounded(rate, t)).toFixed(1)}`);
    }
    return pts;
  };
  const pathFor = (rate: number) =>
    curvePoints(rate)
      .map((p, i) => `${i === 0 ? 'M' : 'L'} ${p}`)
      .join(' ');
  // Skill band: area between the top managed curve and the force-sold line.
  const topPremium = premiums[premiums.length - 1];
  const bandPath = `${pathFor(r + topPremium)} L ${xFor(1).toFixed(1)} ${yFor(r).toFixed(
    1,
  )} L ${x0} ${baseline} Z`;

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 300'
      role='img'
      aria-label={`Cumulative yield per TAO staked on root over twelve months. The before line is today's ${pct(
        r,
      )} run-rate, force-sold daily, and acts as a ceiling. Managed basket curves compound the same stream inside subnet alpha; illustrative allocation premiums lift the twelve-month outcome above the old ceiling, and the shaded region between them is what allocation skill plays for.`}
    >
      <text {...GRAPH_TEXT} x='380' y='28' textAnchor='middle' fill={MUTED}>
        CUMULATIVE YIELD PER τ ON ROOT · 12 MONTHS · SAME DIVIDEND STREAM
      </text>

      <path d={bandPath} fill={GOLD_SOFT} />

      {/* Axes */}
      <line x1={x0} y1={y0} x2={x0} y2={baseline} stroke={INK} strokeWidth='1' />
      <line x1={x0} y1={baseline} x2={x0 + w} y2={baseline} stroke={INK} strokeWidth='1' />
      {[0.1, 0.2].map((v) => (
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
            {pct(v, 0)}
          </text>
        </g>
      ))}
      <text {...GRAPH_TEXT} x={x0 - 8} y={baseline + 3} textAnchor='end' fill={MUTED}>
        0
      </text>
      {[0, 0.5, 1].map((t) => (
        <text
          key={t}
          {...GRAPH_TEXT}
          x={xFor(t)}
          y={baseline + 20}
          textAnchor='middle'
          fill={MUTED}
        >
          {t * 12}
        </text>
      ))}
      <text {...GRAPH_TEXT} x={x0 + w} y={baseline + 38} textAnchor='end' fill={MUTED}>
        MONTHS →
      </text>

      {/* Before: force-sold daily, linear, a ceiling */}
      <line
        x1={x0}
        y1={baseline}
        x2={xFor(1)}
        y2={yFor(r)}
        stroke={MUTED}
        strokeWidth='1.5'
      />
      <circle cx={xFor(1)} cy={yFor(r)} r='3.5' fill='none' stroke={MUTED} strokeWidth='1.5' />
      <text {...GRAPH_TEXT} x={xFor(1) + 10} y={yFor(r) + 3} fill={MUTED}>
        {pct(r)} · BEFORE
      </text>
      <text {...GRAPH_TEXT} x={xFor(1) + 10} y={yFor(r) + 16} fill={MUTED}>
        SOLD DAILY · CEILING
      </text>

      {/* Managed basket curves */}
      {premiums.map((g, i) => {
        const last = i === premiums.length - 1;
        return (
          <g key={g}>
            <path
              d={pathFor(r + g)}
              fill='none'
              stroke={GOLD}
              strokeWidth={last ? 2 : 1.25}
            />
            <circle cx={xFor(1)} cy={yFor(compounded(r + g, 1))} r='3.5' fill={GOLD} />
            <text
              {...GRAPH_TEXT}
              x={xFor(1) + 10}
              y={yFor(compounded(r + g, 1)) + 3}
              fill={GOLD}
            >
              {pct(compounded(r + g, 1), 0)} · +{pct(g, 0)} SKILL
            </text>
          </g>
        );
      })}
      <text {...GRAPH_TEXT} x={xFor(0.66)} y={yFor(0.062)} textAnchor='middle' fill={GOLD}>
        ALLOCATION SKILL
      </text>
      <text
        {...GRAPH_TEXT}
        x={xFor(0.66)}
        y={yFor(0.062) + 14}
        textAnchor='middle'
        fill='rgba(224, 165, 63, 0.75)'
      >
        COMPOUNDS IN α · REALIZED ON CLAIM
      </text>
    </svg>
  );
};

/** Signed daily net flow at subnet pools: the root drain is neutralized, not reversed. */
const FlowNeutralityDiagram = () => {
  const inject = snapshot.taoPerDayIntoPools;
  const rootStream = snapshot.rootDividendsTaoPerDay;
  const netBefore = inject - rootStream;
  const netAfter = inject; // root stream sold once, rebought across subnets: net ≈ 0

  const zeroY = 160;
  const k = 0.055; // px per τ/day
  const yFor = (v: number) => zeroY - v * k;
  const barW = 64;

  const before = {cx: 200, injectX: 120, rootX: 216, lineX1: 110, lineX2: 300};
  const after = {cx: 540, injectX: 460, rootX: 556, lineX1: 450, lineX2: 640};
  const halfW = 28;

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 300'
      role='img'
      aria-label={`Daily net TAO flow at the subnet pools, before and after the upgrade. Emission inject of ${fmt.format(
        inject,
      )} tao per day is unchanged. Before, the root stream of ${fmt.format(
        rootStream,
      )} tao per day sits below the zero axis as net sell flow, leaving a net of ${fmt.format(
        netBefore,
      )      }. After, the drain is removed: at launch dividends accrue in place and nothing is sold; a curated fund, shown here, sells its stream once and immediately rebuys across subnets. Either way the net at the pools is approximately zero — the ${fmt.format(
        sellFlowRemovedPerDay,
      )} tao per day drain is removed and the net rises to the full inject of ${fmt.format(
        netAfter,
      )}.`}
    >
      <text {...GRAPH_TEXT} x='380' y='28' textAnchor='middle' fill={MUTED}>
        DAILY NET TAO FLOW AT THE SUBNET POOLS · τ / DAY · THE DRAIN GOES TO ZERO
      </text>

      {/* Gridlines and y labels */}
      {[1000, -1000].map((v) => (
        <g key={v}>
          <line
            x1='60'
            y1={yFor(v)}
            x2='700'
            y2={yFor(v)}
            stroke={FAINT}
            strokeWidth='1'
            strokeDasharray='4 3'
          />
          <text {...GRAPH_TEXT} x='52' y={yFor(v) + 3} textAnchor='end' fill={MUTED}>
            {v > 0 ? `+${v / 1000}k` : `−${-v / 1000}k`}
          </text>
        </g>
      ))}
      <line x1='60' y1={zeroY} x2='700' y2={zeroY} stroke={INK} strokeWidth='1.25' />
      <text {...GRAPH_TEXT} x='52' y={zeroY + 3} textAnchor='end'>
        0
      </text>

      <text {...GRAPH_TEXT} x={before.cx} y='48' textAnchor='middle' fill={MUTED}>
        BEFORE · ROOT STREAM EXITS
      </text>
      <text {...GRAPH_TEXT} x={after.cx} y='48' textAnchor='middle' fill={MUTED}>
        AFTER · ROOT STREAM REDISTRIBUTED
      </text>

      {/* Before: inject up, root stream down, net well below inject */}
      <rect
        x={before.injectX}
        y={yFor(inject)}
        width={barW}
        height={zeroY - yFor(inject)}
        fill='none'
        stroke={INK}
        strokeWidth='1.5'
      />
      <text
        {...GRAPH_TEXT}
        x={before.injectX + barW / 2}
        y={yFor(inject) - 8}
        textAnchor='middle'
      >
        +{fmt.format(inject)} INJECT
      </text>
      <rect
        x={before.rootX}
        y={zeroY}
        width={barW}
        height={yFor(-rootStream) - zeroY}
        fill='rgba(41, 41, 41, 0.08)'
        stroke={MUTED}
        strokeWidth='1.5'
      />
      <text
        {...GRAPH_TEXT}
        x={before.rootX + barW / 2}
        y={yFor(-rootStream) + 16}
        textAnchor='middle'
        fill={MUTED}
      >
        −{fmt.format(rootStream)} SOLD, EXITS
      </text>
      <line
        x1={before.lineX1}
        y1={yFor(netBefore)}
        x2={before.lineX2}
        y2={yFor(netBefore)}
        stroke={INK}
        strokeWidth='1'
        strokeDasharray='3 3'
      />
      <text {...GRAPH_TEXT} x={before.lineX2 + 6} y={yFor(netBefore) + 3} fill={MUTED}>
        NET +{fmt.format(netBefore)}
      </text>

      {/* After: inject up, root stream sold and rebought — a cancelled pair */}
      <rect
        x={after.injectX}
        y={yFor(inject)}
        width={barW}
        height={zeroY - yFor(inject)}
        fill='none'
        stroke={INK}
        strokeWidth='1.5'
      />
      <text
        {...GRAPH_TEXT}
        x={after.injectX + barW / 2}
        y={yFor(inject) - 8}
        textAnchor='middle'
      >
        +{fmt.format(inject)} INJECT
      </text>
      <rect
        x={after.rootX}
        y={zeroY}
        width={halfW}
        height={yFor(-rootStream) - zeroY}
        fill='rgba(41, 41, 41, 0.08)'
        stroke={MUTED}
        strokeWidth='1.5'
      />
      <text
        {...GRAPH_TEXT}
        x={after.rootX + halfW / 2}
        y={yFor(-rootStream) + 16}
        textAnchor='middle'
        fill={MUTED}
      >
        SOLD
      </text>
      <rect
        x={after.rootX + halfW + 8}
        y={yFor(rootStream)}
        width={halfW}
        height={zeroY - yFor(rootStream)}
        fill={GOLD_SOFT}
        stroke={GOLD}
        strokeWidth='1.5'
      />
      <text
        {...GRAPH_TEXT}
        x={after.rootX + halfW + 8 + halfW / 2}
        y={yFor(rootStream) - 8}
        textAnchor='middle'
        fill={GOLD}
      >
        REBOUGHT
      </text>
      <text
        {...GRAPH_TEXT}
        x={after.rootX + halfW + 4}
        y={yFor(-rootStream) + 36}
        textAnchor='middle'
        fill={GOLD}
      >
        NET ≈ 0 · REDISTRIBUTED PER ROOT WEIGHTS
      </text>
      <line
        x1={after.lineX1}
        y1={yFor(netAfter)}
        x2={after.lineX2}
        y2={yFor(netAfter)}
        stroke={GOLD}
        strokeWidth='1'
        strokeDasharray='3 3'
      />
      <text {...GRAPH_TEXT} x={after.lineX2 + 6} y={yFor(netAfter) + 3} fill={GOLD}>
        NET +{fmt.format(netAfter)}
      </text>

      {/* The drain removed: from −983 back to the zero axis */}
      <line
        x1='688'
        y1={yFor(-rootStream)}
        x2='688'
        y2={zeroY + 4}
        stroke={GOLD}
        strokeWidth='1.5'
      />
      <polygon points={`688,${zeroY + 2} 684,${zeroY + 10} 692,${zeroY + 10}`} fill={GOLD} />
      <text {...GRAPH_TEXT} x='698' y={yFor(-rootStream / 2) - 4} fill={GOLD}>
        {fmt.format(sellFlowRemovedPerDay)} τ/DAY
      </text>
      <text {...GRAPH_TEXT} x='698' y={yFor(-rootStream / 2) + 10} fill={GOLD}>
        NET SELLING
      </text>
      <text {...GRAPH_TEXT} x='698' y={yFor(-rootStream / 2) + 24} fill={GOLD}>
        REMOVED
      </text>

      <text {...GRAPH_TEXT} x='380' y='288' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
        EMISSION INJECT UNCHANGED · CURATED FLOW SHOWN — AT LAUNCH NOTHING IS SOLD AT ALL
      </text>
    </svg>
  );
};

/** Bipartite graph: validators publish weight vectors; edges carry dividends to subnets. */
const GuidingHandsDiagram = () => {
  const validatorYs = [100, 180, 260, 340];
  const goldValidator = 1;
  const hexX = 130;
  const hexR = 30;

  // Right side: real top root-dividend subnets from the snapshot, plus the TAO slot.
  const subnetNodes = snapshot.topRootDividendSubnets
    .slice(0, 6)
    .map((s, i) => ({label: `${s.name.toUpperCase()} · SN ${s.netuid}`, y: 70 + i * 45}));
  const taoNode = {label: 'NETUID 0 · TAO SLOT', y: 70 + 6 * 45};
  const nodes = [...subnetNodes, taoNode];
  const rectX = 540;
  const rectW = 190;
  const rectH = 26;

  // Illustrative weight vectors; destinations are the real subnets above.
  const edges: Array<{v: number; s: number; w: number}> = [
    {v: 0, s: 0, w: 0.5},
    {v: 0, s: 1, w: 0.3},
    {v: 0, s: 4, w: 0.2},
    {v: 1, s: 1, w: 0.4},
    {v: 1, s: 5, w: 0.3},
    {v: 1, s: 6, w: 0.2},
    {v: 1, s: 3, w: 0.1},
    {v: 2, s: 3, w: 0.4},
    {v: 2, s: 2, w: 0.3},
    {v: 2, s: 6, w: 0.3},
    {v: 3, s: 5, w: 0.5},
    {v: 3, s: 0, w: 0.25},
    {v: 3, s: 4, w: 0.25},
  ];

  const x1 = hexX + hexR * 0.866 + 4;
  const x2 = rectX - 4;
  const edgePath = (vy: number, sy: number) => {
    const midX = (x1 + x2) / 2;
    return `M ${x1} ${vy} C ${midX} ${vy}, ${midX} ${sy}, ${x2} ${sy}`;
  };

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 400'
      role='img'
      aria-label={`Bipartite graph of root validators and subnets. Four validator nodes on the left hold the network's ${fmt.format(
        snapshot.rootStakeTao,
      )} tao of delegated stake. Each publishes an on-chain weight vector, drawn as edges to subnet nodes on the right — real top root-dividend subnets plus the netuid zero TAO slot. Edge thickness is the share of the validator's root dividend flowing to that destination; one validator's vector is highlighted with its weights. Vectors shown are illustrative.`}
    >
      <text {...GRAPH_TEXT} x={hexX} y='32' textAnchor='middle' fill={MUTED}>
        ROOT VALIDATORS
      </text>
      <text {...GRAPH_TEXT} x={hexX} y='46' textAnchor='middle' fill={MUTED}>
        {fmt.format(snapshot.rootStakeTao)} τ DELEGATED
      </text>
      <text {...GRAPH_TEXT} x={rectX + rectW / 2} y='32' textAnchor='middle' fill={MUTED}>
        SUBNETS · {snapshot.liveSubnets} LIVE
      </text>
      <text {...GRAPH_TEXT} x={rectX + rectW / 2} y='46' textAnchor='middle' fill={MUTED}>
        α DESTINATIONS
      </text>

      {/* Edges under nodes */}
      {edges.map(({v, s, w}) => {
        const gold = v === goldValidator;
        return (
          <path
            key={`${v}-${s}`}
            d={edgePath(validatorYs[v], nodes[s].y)}
            fill='none'
            stroke={gold ? GOLD : 'rgba(41, 41, 41, 0.22)'}
            strokeWidth={2 + w * 6}
          />
        );
      })}
      {/* Weight labels on the highlighted vector */}
      {edges
        .filter(({v}) => v === goldValidator)
        .map(({s, w}) => (
          <text
            key={`w-${s}`}
            {...GRAPH_TEXT}
            x={x2 - 8}
            y={nodes[s].y - 7}
            textAnchor='end'
            fill={GOLD}
          >
            {pct(w, 0)}
          </text>
        ))}

      {/* Validator hexes */}
      {validatorYs.map((y, i) => {
        const gold = i === goldValidator;
        return (
          <g key={y}>
            <polygon
              points={hexPoints(hexX, y, hexR)}
              fill={gold ? GOLD_SOFT : 'none'}
              stroke={gold ? GOLD : INK}
              strokeWidth='1.5'
            />
            <text
              {...GRAPH_TEXT}
              x={hexX}
              y={y - 2}
              textAnchor='middle'
              fill={gold ? GOLD : INK}
            >
              VALI {String.fromCharCode(65 + i)}
            </text>
            <text
              {...GRAPH_TEXT}
              x={hexX}
              y={y + 12}
              textAnchor='middle'
              fill={gold ? 'rgba(224, 165, 63, 0.75)' : MUTED}
            >
              VECTOR
            </text>
          </g>
        );
      })}

      {/* Subnet rects */}
      {nodes.map((n, i) => {
        const isTao = i === nodes.length - 1;
        return (
          <g key={n.label}>
            <rect
              x={rectX}
              y={n.y - rectH / 2}
              width={rectW}
              height={rectH}
              fill='none'
              stroke={INK}
              strokeWidth='1.5'
            />
            {isTao && (
              <rect x={rectX + 6} y={n.y - 4} width={8} height={8} fill={GOLD} />
            )}
            <text {...GRAPH_TEXT} x={rectX + (isTao ? 20 : 8)} y={n.y + 4}>
              {n.label}
            </text>
          </g>
        );
      })}

      <text {...GRAPH_TEXT} x='380' y='392' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
        EVERY EDGE IS ON-CHAIN, PUBLIC · WIDTH = SHARE OF THE VALIDATOR&apos;S ROOT DIVIDEND
      </text>
    </svg>
  );
};

/** Sorted delegation distributions: incumbency curve vs a challenger breaking it on yield. */
const RootCompetitionDiagram = () => {
  // Illustrative delegation shares, sorted by size (not a live snapshot).
  const beforeHeights = [120, 95, 75, 60, 48, 38, 30, 24];
  const afterHeights = [92, 88, 72, 58, 104, 38, 30, 24];
  const challenger = 4;
  const incumbentLoss = beforeHeights[0] - afterHeights[0];

  const barW = 26;
  const gap = 9;
  const baseline = 220;
  const beforeX = 56;
  const afterX = 432;
  const barX = (originX: number, i: number) => originX + i * (barW + gap);

  const incumbentCx = barX(afterX, 0) + barW / 2;
  const challengerCx = barX(afterX, challenger) + barW / 2;
  const arcTop = baseline - beforeHeights[0] - 26;

  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 300'
      role='img'
      aria-label='Two sorted delegation distributions. Before: bars decrease smoothly from a dominant incumbent — stake gravitates to size and entry requires out-staking the floor. After: registration is burn-based, and a gold challenger in the middle of the ranking breaks the monotone curve by posting a better realized yield, pulling delegation away from the incumbent, whose bar shows the ghost of the stake it lost. Distributions are illustrative.'
    >
      <text {...GRAPH_TEXT} x='190' y='32' textAnchor='middle' fill={MUTED}>
        BEFORE · ENTRY BY STAKE
      </text>
      <text {...GRAPH_TEXT} x='570' y='32' textAnchor='middle' fill={MUTED}>
        AFTER · ENTRY BY BURN, RANK BY YIELD
      </text>

      {/* Before: smooth incumbency curve */}
      {beforeHeights.map((h, i) => (
        <rect
          key={i}
          x={barX(beforeX, i)}
          y={baseline - h}
          width={barW}
          height={h}
          fill={i === 0 ? 'rgba(41, 41, 41, 0.08)' : 'none'}
          stroke={INK}
          strokeWidth='1.5'
        />
      ))}
      <text
        {...GRAPH_TEXT}
        x={barX(beforeX, 0) + barW / 2}
        y={baseline - beforeHeights[0] - 10}
        textAnchor='middle'
      >
        INCUMBENT
      </text>
      <line
        x1={beforeX - 8}
        y1={baseline}
        x2={barX(beforeX, beforeHeights.length)}
        y2={baseline}
        stroke={INK}
        strokeWidth='1'
      />
      <text {...GRAPH_TEXT} x='190' y={baseline + 22} textAnchor='middle' fill={MUTED}>
        DELEGATION SORTS BY SIZE · SIZE COMPOUNDS
      </text>

      {/* After: challenger breaks the curve */}
      {afterHeights.map((h, i) => {
        const gold = i === challenger;
        return (
          <rect
            key={i}
            x={barX(afterX, i)}
            y={baseline - h}
            width={barW}
            height={h}
            fill={gold ? GOLD_SOFT : 'none'}
            stroke={gold ? GOLD : INK}
            strokeWidth='1.5'
          />
        );
      })}
      {/* Ghost of the stake the incumbent lost */}
      <rect
        x={barX(afterX, 0)}
        y={baseline - beforeHeights[0]}
        width={barW}
        height={incumbentLoss}
        fill='none'
        stroke={MUTED}
        strokeWidth='1'
        strokeDasharray='3 3'
      />
      {/* Delegation flows from incumbent to challenger */}
      <path
        d={`M ${incumbentCx} ${baseline - afterHeights[0] - 8} C ${incumbentCx} ${arcTop}, ${challengerCx} ${arcTop}, ${challengerCx} ${
          baseline - afterHeights[challenger] - 8
        }`}
        fill='none'
        stroke={GOLD}
        strokeWidth='1.5'
      />
      <polygon
        points={`${challengerCx},${baseline - afterHeights[challenger] - 6} ${
          challengerCx - 4
        },${baseline - afterHeights[challenger] - 14} ${challengerCx + 4},${
          baseline - afterHeights[challenger] - 14
        }`}
        fill={GOLD}
      />
      <text
        {...GRAPH_TEXT}
        x={(incumbentCx + challengerCx) / 2}
        y={arcTop - 8}
        textAnchor='middle'
        fill={GOLD}
      >
        DELEGATION FOLLOWS REALIZED YIELD
      </text>
      <text
        {...GRAPH_TEXT}
        x={challengerCx}
        y={baseline + 22}
        textAnchor='middle'
        fill={GOLD}
      >
        NEW ENTRANT
      </text>
      <text
        {...GRAPH_TEXT}
        x={challengerCx}
        y={baseline + 36}
        textAnchor='middle'
        fill='rgba(224, 165, 63, 0.75)'
      >
        BURNED IN · NO STAKE
      </text>
      <line
        x1={afterX - 8}
        y1={baseline}
        x2={barX(afterX, afterHeights.length)}
        y2={baseline}
        stroke={INK}
        strokeWidth='1'
      />

      <text {...GRAPH_TEXT} x='380' y='285' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
        SCOREBOARD: WHO MADE THEIR STAKERS THE MOST TAO
      </text>
    </svg>
  );
};

const hexPoints = (cx: number, cy: number, r: number) => {
  const dx = r * 0.866;
  return [
    [cx, cy - r],
    [cx + dx, cy - r / 2],
    [cx + dx, cy + r / 2],
    [cx, cy + r],
    [cx - dx, cy + r / 2],
    [cx - dx, cy - r / 2],
  ]
    .map(([x, y]) => `${x},${y}`)
    .join(' ');
};

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>Root Reborn</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            The V441 Upgrade · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p>
            TAO is not just a utility token — it is a productive asset. Holding it earns a
            share of the inflation of all {snapshot.registeredSubnets} competitively
            optimized subnets. We call that share <strong>root proportion</strong>: the
            fraction of Bittensor&apos;s underlying subnets owed to TAO itself. The exchange
            is a fair one — in return, the subnets exclusively receive TAO&apos;s own
            inflation, the very issuance that dilutes TAO holders (above all those staked on
            root).
          </p>
          <p className={styles.headline_number}>{fmt.format(snapshot.rootStakeTao)} τ</p>
          <p className={styles.headline_label}>
            {usd(snapshot.rootStakeTao)} staked on root — {pct(snapshot.rootShareOfIssuance)} of
            every TAO ever minted
          </p>
          <p>
            Nearly half of all TAO in existence sits on root —{' '}
            {pct(snapshot.rootShareOfStake, 0)} of all staked TAO. Until now that capital has
            been sterile, because the chain enforced a bias on how its owed inflation was
            used: sold into TAO the moment it arrived. This upgrade changes the nature of
            root yield, from passive to managed. The validator a root staker attaches to now
            chooses a distribution allocation — where the rewards are deployed — so that
            yield stays allocated inside the network and, instead of being auto-sold, is held
            until claimable inside{' '}
            <DocLink href='/docs/guides/root-reborn'>baskets</DocLink>. In effect, every root
            validator runs an escrowed basket of subnet alpha, curated by its root weights,
            and stakers redeem their entitlement when they choose.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Why we are doing this</p>
          <p>
            <strong>1 · Optimize the yield we pay TAO holders.</strong> The subnets pay root
            roughly {rootDividendsPerDay} τ every day ({usd(snapshot.rootDividendsTaoPerDay)}
            /day) — {pct(snapshot.rootDividendsPctOfEmission, 0)} of all new emission — and
            have paid {fmt.format(snapshot.cumulativeRootRevenueTao)} τ (
            {usd(snapshot.cumulativeRootRevenueTao)}) since dTAO went live. At today&apos;s
            run-rate that is a {pct(snapshot.rootYieldApr)}/yr base yield in TAO, and under
            the old machinery it was a ceiling as much as a floor: every unit was force-sold
            the block it arrived, at a price and a time nobody chose. Root Reborn hands that
            stream to the people best positioned to manage it. A validator that allocates
            well compounds its stakers&apos; yield inside subnet alpha every epoch instead of
            realizing it daily — {pct(snapshot.rootYieldApr)} becomes the starting point, not
            the story, and we expect the effective yield distributed to TAO holders to rise
            with allocation skill.
          </p>
          <YieldFrontierDiagram />
          <p className={styles.graph_caption}>
            The same dividend stream under both regimes. Force-sold daily, today&apos;s
            run-rate of {pct(snapshot.rootYieldApr)}/yr is a ceiling; compounding it inside a
            managed basket makes it a floor. Premium curves are illustrative allocation
            outcomes, not projections of any subnet.
          </p>
          <p>
            <strong>2 · Make root proportion neutral for subnets.</strong> Root&apos;s share
            of subnet inflation has been structural sell pressure: {rootDividendsPerDay} τ of
            subnet alpha marked and sold out of the pools daily, mechanically, regardless of
            conviction — a drain equal to {streamVsInject} of the roughly{' '}
            {fmt.format(snapshot.taoPerDayIntoPools)} τ of fresh emission entering all subnet
            pools each day. Root Reborn removes that drain outright. At launch every fund
            runs the null strategy: the dividend is not sold at all — it accrues in place as
            subnet alpha held by the fund, touching no pool. Once curation opens, a curated
            dividend is sold once and its proceeds immediately rebought across subnet pools
            per the validator&apos;s weights. Either way, root&apos;s net flow at the pools
            goes from{' '}
            <strong>
              −{rootDividendsPerDay} τ/day ({usd(sellFlowRemovedPerDay)}) to ≈ 0
            </strong>
            : a redistribution among subnets rather than a tax out of them. Subnets still pay
            root its proportion; root now holds it — and, curated, recycles it — inside the
            subnets its validators believe in.
          </p>
          <FlowNeutralityDiagram />
          <p className={styles.graph_caption}>
            Daily net flow at the subnet pools, measured in τ. The emission inject is
            untouched. At launch nothing is sold — dividends accrue in place, net 0 by
            construction; a curated fund (shown) sells its dividend once and rebuys across
            subnets, still net ≈ 0. The −{rootDividendsPerDay} τ/day drain is removed either
            way, and the pools keep the full inject.
          </p>
          <p>
            <strong>3 · Re-engage validators as guiding hands.</strong> Root validators sit
            on the largest aggregated positions in the network —{' '}
            {fmt.format(snapshot.rootStakeTao)} τ of delegated conviction — and until now had
            no way to express a view with them. Root Reborn makes capital allocation part of
            validation: each validator publishes an on-chain weight vector, and its
            stakers&apos; dividends flow per that judgment across {snapshot.liveSubnets} live
            subnets. (Weight setting ships gated off; curation opens in a follow-up upgrade
            so the null-strategy baseline is established first.) The parties with the
            deepest visibility into the ecosystem are, for the
            first time, paid to curate it — their vectors are public signals of where value
            is being created.
          </p>
          <GuidingHandsDiagram />
          <p className={styles.graph_caption}>
            The allocation layer as a bipartite graph: validators on the left, subnet alpha
            destinations on the right — the top root-dividend payers today, plus the netuid 0
            TAO slot. Every edge is a published, on-chain weight; the highlighted validator
            shows one full vector. Vectors are illustrative, destinations are real.
          </p>
          <p>
            <strong>4 · Break the monopolies on root.</strong> Root delegation has been won
            by incumbency: stake gravitates to size, and size compounds. This upgrade changes
            what competition on root is about. Registration is now burn-based — no prior
            stake required — and the scoreboard is the simplest in finance: who made their
            stakers the most TAO. A new entrant with a sharper read on the subnets can
            out-allocate an incumbent, post a better realized yield, and pull delegation away
            from it. Intelligent allocation behaviour, not entrenchment, becomes how position
            on root is earned.
          </p>
          <RootCompetitionDiagram />
          <p className={styles.graph_caption}>
            Delegation across root validators, sorted by size. Before, the distribution is a
            pure incumbency curve — stake gravitates to stake. After, a burn-registered
            entrant with a better realized yield breaks the curve and pulls delegation from
            the head; the dashed ghost is what the incumbent lost. Distributions are
            illustrative.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>How it works</p>
          <p>
            The machinery is one fund per root validator, five moving parts, no action
            required from stakers:
          </p>
          <ul className={styles.list}>
            <li>
              <strong>Validators publish a vector — once curation opens.</strong>{' '}
              <DocLink href='/docs/tx/set-root-weights'>
                <code>set_root_weights</code>
              </DocLink>{' '}
              (call index 146) takes relative weights over netuid 0 and existing subnets — at
              least 8 positive destinations (softened when fewer networks exist),
              rate-limited like other weight calls.{' '}
              <strong>Weight setting launches gated off network-wide</strong> (calls fail
              with <code>RootWeightSettingDisabled</code>): every fund starts on the same
              null strategy, and curation is switched on in a later upgrade.
            </li>
            <li>
              <strong>Each epoch, the dividend lands in the basket.</strong> With no vector
              — every fund at launch — the dividend simply accrues in place: the subnet
              alpha is credited straight into the fund&apos;s holding on the subnet that
              paid it, trade-free (no sell, no swap fees, no slippage). Once a validator
              curates, its dividend is instead sold to TAO once, split per the vector, and
              each slice buys that destination&apos;s alpha into the basket; weight on
              netuid 0 keeps its slice as pure TAO.
            </li>
            <li>
              <strong>Holdings sit in chain-owned escrow.</strong> Basket positions are real
              stake entries under a pallet sub-account with no private key — they cannot be
              moved or signed away, and because they are real stake they keep earning every
              epoch.
            </li>
            <li>
              <strong>Stakers accrue entitlement at NAV.</strong> When a dividend lands, each
              staker is credited a fraction of the whole fund, priced at its{' '}
              <strong>realizable</strong> TAO quote — what selling the holdings would
              actually fetch at current pool depth, never a spot mark. Existing holders are
              neither diluted nor gifted.
            </li>
            <li>
              <strong>Claims are per validator, whenever you choose.</strong>{' '}
              <DocLink href='/docs/tx/claim-root-with-hotkey'>
                <code>claim_root_with_hotkey</code>
              </DocLink>{' '}
              takes the validator hotkey: it redeems your owed fraction pro-rata from that
              basket only, and pays it as TAO staked straight back to root on the same
              validator. The legacy{' '}
              <DocLink href='/docs/tx/claim-root'>
                <code>claim_root(subnets)</code>
              </DocLink>{' '}
              call still decodes for old clients (the subnet set is ignored; it claims
              coldkey-wide). Nothing is realized — and nothing is taxable — until you call
              it.
            </li>
          </ul>
          <p>
            Past accruals migrate automatically at upgrade: the old per-subnet claimable
            state becomes basket holdings and entitlement on the same validator, so no one
            loses a rao and no one needs to act. Here is what each side actually runs.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>For root validators</p>
          <p>
            Joining root is now <strong>burn-based</strong>: <code>root_register</code>{' '}
            charges the coldkey the demand-priced root burn (τ1 at rest, bumped
            ~×1.26 per registration and decaying back to the floor) instead of requiring
            stake up front. A full network still prunes the lowest-staked member on entry,
            so subscribe stake to your own hotkey to hold the seat.
          </p>
          <pre className={styles.code_block}>
            {`btcli subnets burn-cost 0                        # current root registration price
btcli subnets register --netuid 0 -w my_coldkey -H my_hotkey  # register; no prior stake needed`}
          </pre>
          <p>
            No curation is needed to earn: a validator with no weight vector accrues
            automatically — each subnet&apos;s dividend accumulates in place in that
            subnet&apos;s alpha, trade-free. <strong>Weight setting is gated off at
            launch</strong>: <code>set_root_weights</code> fails with{' '}
            <code>RootWeightSettingDisabled</code> network-wide, so every fund starts on
            this same null strategy and the upgrade&apos;s effect is uniform and visible.
            Curation will be enabled in a later upgrade; once it is, set your vector with
            your hotkey (or weight netuid 0 to keep part of a curated basket in TAO):
          </p>
          <pre className={styles.code_block}>
            {`btcli root set-weights --weights "0:0.2,4:0.3,8:0.5" -w my_wallet
btcli root get-weights --hotkey 5F...
btcli root show --hotkey 5F...         # your fund: weights, holdings, NAV`}
          </pre>
          <p>
            Weights are relative and normalized before submission (
            <DocLink href='/docs/tx/set-root-weights'>
              <code>set-root-weights</code>
            </DocLink>
            , call index 146); every destination must be netuid 0 or an existing subnet, and
            the root weights rate limit applies. The SDK surfaces the same path as{' '}
            <code>bt.SetRootWeights</code>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>For root stakers</p>
          <p>
            Nothing to configure — subscribe TAO on root and yield accrues automatically.{' '}
            <code>btcli</code> shows root positions in TAO: staked τ is principal,
            accrued τ is fund yield.
          </p>
          <pre className={styles.code_block}>
            {`btcli root list                            # staked + accrued τ, per validator
btcli root subscribe --amount 100 --hotkey 5F...  # assets in
btcli root claim                                  # pick wallet → validator → claim / withdraw
btcli root claim --hotkey 5F... --amount all      # withdraw full position
btcli root claim --hotkey 5F...                   # claim accrued into stake only`}
          </pre>
          <p>
            <code>btcli root claim</code> is the exit path: omit{' '}
            <code>--amount</code> to realize accrued yield into root stake, or pass{' '}
            <code>--amount</code> / <code>all</code> to withdraw to free balance (accrued
            yield is claimed first when needed via{' '}
            <DocLink href='/docs/tx/claim-root-with-hotkey'>
              <code>claim_root_with_hotkey</code>
            </DocLink>
            ). Legacy <code>claim_root(subnets)</code> still works for old clients (subnets
            ignored; claims every validator). Per-validator payouts below the claim
            threshold (default 500,000 rao; read it with{' '}
            <DocLink href='/docs/query/root-claim-threshold'>
              <code>root-claim-threshold</code>
            </DocLink>
            ) are skipped and keep accruing — there is no deadline and nothing expires.
          </p>
          <p>
            The same claim from the SDK — preview what is owed, then redeem one
            validator&apos;s accrued yield:
          </p>
          <pre className={styles.code_block}>
            {`import bittensor as bt
from bittensor.wallet import Wallet

wallet = Wallet(name="my_coldkey")
sub = bt.Subtensor()

# Preview: TAO owed per validator hotkey, at current pool prices
owed = sub.read(
    "root_basket_owed_breakdown",
    coldkey_ss58=wallet.coldkeypub.ss58_address,
)

# Claim one validator's accrued yield (staked back to root)
result = sub.execute(bt.ClaimRootWithHotkey(hotkey_ss58="5F..."), wallet)

# Or claim across every validator you stake to (compat; subnets ignored)
result = sub.execute(bt.ClaimRoot(subnets=[0]), wallet)`}
          </pre>
          <p>
            And the raw chain calls, for anyone signing directly or building against
            metadata:
          </p>
          <pre className={styles.code_block}>
            {`SubtensorModule.claim_root_with_hotkey        call index 148
  origin: signed(coldkey)
  args:   hotkey: AccountId32      # validator whose accrued yield to redeem

SubtensorModule.claim_root                    call index 121
  origin: signed(coldkey)
  args:   subnets: BTreeSet<u16>   # IGNORED — claims are fund-level;
                                   # walks every validator you stake to`}
          </pre>
          <p>
            A claim redeems your owed fraction of the validator&apos;s basket pro-rata
            (alpha sold to TAO at current pool depth) and stakes the proceeds back to root
            on the same validator. Fees are charged by work actually done — the declared
            weight is a cap, refunded post-dispatch. Prefer{' '}
            <code>claim_root_with_hotkey</code>: a coldkey staked to many validators pays
            for the full walk under bare <code>claim_root</code>, while the per-hotkey call
            prices exactly one fund.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Breaking changes</p>
          <ul className={styles.list}>
            <li>
              <code>claim_root</code> (call index 121) keeps the{' '}
              <code>subnets: BTreeSet&lt;NetUid&gt;</code> argument so old clients still
              decode, but the set is <strong>ignored</strong> — claims are fund-level across
              every validator the coldkey stakes to. New{' '}
              <code>claim_root_with_hotkey</code> (148) claims one validator.
            </li>
            <li>
              <code>set_root_claim_type</code> (122) and{' '}
              <code>sudo_set_num_root_claims</code> (123) are <strong>removed</strong>.
              Payouts are always TAO staked back to root — the Swap / Keep / KeepSubnets
              setting and the automatic per-block claim sweep are gone. To hold subnet
              alpha, stake on the subnet directly.
            </li>
            <li>
              The <code>RootClaimable</code> / <code>RootClaimed</code> per-subnet state is{' '}
              <strong>migrated into baskets</strong> at upgrade: previously accrued claimable
              alpha becomes basket holdings and entitlement on the same validator. Nothing is
              lost, and no user action is needed for past accruals.
            </li>
            <li>
              Hotkey and coldkey swaps carry basket state (holdings, entitlements, claim
              watermarks) to the new key; a hotkey with a live basket is not
              &quot;clean&quot; for reuse. Subnet dissolution converts that subnet&apos;s
              basket holdings into the TAO slot of each affected fund.
            </li>
            <li>
              <code>root_register</code> (62) now charges the root burn price and no longer
              requires the hotkey to out-stake the lowest root member — the{' '}
              <code>StakeTooLowForRoot</code> error is retired. Tooling that pre-funded root
              hotkeys with stake before registering can drop that step; coldkeys need free
              balance for the burn instead.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>New runtime APIs</p>
          <p>
            A <code>BetaBasketRuntimeApi</code> ships with the release, mirrored as{' '}
            <code>betaBasket_*</code> RPC methods and as named SDK reads (available under{' '}
            <code>btcli query</code> and <code>client.read</code>):
          </p>
          <pre className={styles.code_block}>
            {`betaBasket_getStakerOwed(coldkey)            root-basket-owed
betaBasket_getStakerValidatorOwed(hk, ck)    root-basket-owed-breakdown
betaBasket_getValidatorBasket(hotkey)        validator-basket
betaBasket_getValidatorNav(hotkey)           validator-basket-nav
betaBasket_getTotalNav()                     root-basket-total-nav
betaBasket_getValidatorWeights(hotkey)       validator-root-weights`}
          </pre>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <p>
            Operators should wait for the on-chain <code>spec_version</code> to move to 441,
            then upgrade nodes and clients. SDK users should pull the matching bittensor
            release once the train publishes it.
          </p>
          <p>
            <strong>Root validators:</strong> nothing to do at launch — weight setting is
            gated off network-wide and every fund runs the null strategy, with dividends
            accumulating in place per subnet. When curation is enabled in a later upgrade,
            set your vector with <code>btcli root set-weights</code>.{' '}
            <strong>Stakers:</strong> use{' '}
            <code>btcli root subscribe</code> and <code>btcli root claim</code> — list shows
            staked and accrued τ; claim realizes yield and optionally withdraws. The retired{' '}
            <code>btcli stake set-claim</code> / <code>process-claim</code> commands are
            replaced by the <code>btcli root</code> suite.
          </p>
          <p>
            <strong>Indexers and integrators:</strong> regenerate metadata for new{' '}
            <code>claim_root_with_hotkey</code> (148) and <code>set_root_weights</code>{' '}
            (146 — same name as the retired call-index-8 emission vote, new args and
            semantics); <code>claim_root</code> (121) keeps its <code>subnets</code> arg
            (ignored).
            Drop the retired 122/123 calls, and add the <code>RootWeightsSet</code>,{' '}
            <code>BasketDeposited</code>, <code>BasketClaimed</code>, <code>RootClaimed</code>
            , and <code>BasketHoldingConverted</code> events plus the{' '}
            <code>betaBasket_*</code> RPC namespace. Note that{' '}
            <code>set_root_weights</code> is gated at launch — it fails with the new{' '}
            <code>RootWeightSettingDisabled</code> error until governance flips the flag via{' '}
            <code>AdminUtils.sudo_set_root_weight_setting_enabled</code> (103, emitting{' '}
            <code>RootWeightSettingToggled</code>). The claim threshold is root-settable via{' '}
            <code>sudo_set_root_claim_threshold</code> (124), wrapped as{' '}
            <DocLink href='/docs/tx/set-root-claim-threshold'>
              <code>set-root-claim-threshold</code>
            </DocLink>
            .
          </p>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v441 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The network, in raw numbers</p>
          <table className={styles.metrics_table}>
            <tbody>
              <tr>
                <td>TAO price</td>
                <td>${taoUsd.toFixed(2)}</td>
              </tr>
              <tr>
                <td>TAO issued to date</td>
                <td>
                  {fmt.format(snapshot.totalIssuanceTao)} τ · {usd(snapshot.totalIssuanceTao)}
                </td>
              </tr>
              <tr>
                <td>Staked on the root network</td>
                <td>
                  {fmt.format(snapshot.rootStakeTao)} τ · {usd(snapshot.rootStakeTao)} ·{' '}
                  {pct(snapshot.rootShareOfIssuance)} of issuance
                </td>
              </tr>
              <tr>
                <td>Subnets live and earning</td>
                <td>
                  {snapshot.liveSubnets} of {snapshot.registeredSubnets}
                </td>
              </tr>
              <tr>
                <td>
                  Chain-held TAO in subnet pools
                  <br />
                  <span style={{opacity: 0.55, fontSize: 11}}>
                    from TAO→α buys — sits until deregister
                  </span>
                </td>
                <td>
                  {fmt.format(snapshot.taoInSubnetPools)} τ · {usd(snapshot.taoInSubnetPools)}
                </td>
              </tr>
              <tr>
                <td>Subnet alpha market cap</td>
                <td>
                  {fmt.format(snapshot.alphaMarketCapTao)} τ · {usd(snapshot.alphaMarketCapTao)}
                </td>
              </tr>
              <tr>
                <td>Root dividend stream (protocol revenue, 7-day avg)</td>
                <td>
                  {rootDividendsPerDay} τ / day · {usd(snapshot.rootDividendsTaoPerDay)} / day ·{' '}
                  {pct(snapshot.rootDividendsPctOfEmission, 0)} of emission
                </td>
              </tr>
              <tr>
                <td>Cumulative protocol revenue since dTAO</td>
                <td>
                  {fmt.format(snapshot.cumulativeRootRevenueTao)} τ ·{' '}
                  {usd(snapshot.cumulativeRootRevenueTao)}
                </td>
              </tr>
              <tr>
                <td>Base root yield at that run-rate</td>
                <td>{pct(snapshot.rootYieldApr)} / yr in TAO</td>
              </tr>
              <tr>
                <td>Paid to miners for useful work</td>
                <td>
                  {fmt.format(snapshot.minersTaoPerDay)} τ / day ·{' '}
                  {usd(snapshot.minersTaoPerDay)} / day
                </td>
              </tr>
              <tr>
                <td>Root dividend gate (Σ subnet EMA prices)</td>
                <td>
                  {snapshot.emaPriceSum.toFixed(2)} ·{' '}
                  {snapshot.rootDividendGateOpen ? 'open' : 'closed'}
                </td>
              </tr>
            </tbody>
          </table>
          <p className={styles.data_note}>
            finney block {fmt.format(snapshot.block)} · july 24, 2026 · ${taoUsd.toFixed(2)}/τ ·
            sources: taomarketcap api (subnets + protocol revenue + market candles) + chain
            storage
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/root-reborn'>Read the Root Reborn guide</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
