'use client';

import {useId, useMemo, useState} from 'react';

const INK = 'rgb(41, 41, 41)';
const INK_FAINT = 'rgba(41, 41, 41, 0.45)';
const GRID = 'rgba(41, 41, 41, 0.12)';
const ACCENT = '#d15168';

const MONO = {fontFamily: 'FiraCode, monospace', fontSize: 10, fill: INK} as const;

// Finney SubnetMovingPrice snapshot, 2026-07-27. Price-proportional demand
// shares for all 126 subnets (netuid 1-128 minus root), sorted descending,
// with every subnet treated as emission-enabled.
const SHARES = [
  0.0648777, 0.0483803, 0.0439025, 0.0424682, 0.0392578, 0.0357883, 0.0268929, 0.0263278,
  0.0217363, 0.0203214, 0.0181265, 0.0175383, 0.0154783, 0.0147082, 0.0134228, 0.0117943,
  0.0117212, 0.0104459, 0.0102057, 0.0098919, 0.0095797, 0.0095514, 0.009441, 0.0093479,
  0.0089458, 0.0088688, 0.0088141, 0.0080852, 0.0080827, 0.0080605, 0.0080219, 0.0079442,
  0.0078232, 0.007781, 0.0076949, 0.0076822, 0.0075456, 0.007481, 0.0071565, 0.006929,
  0.0068142, 0.0066749, 0.0066701, 0.0064523, 0.0061145, 0.0061047, 0.0060396, 0.0058941,
  0.005812, 0.00561, 0.0055986, 0.0053491, 0.0053475, 0.0053437, 0.0053102, 0.0052276,
  0.0051107, 0.0050131, 0.0049943, 0.0049684, 0.004919, 0.0048043, 0.0048023, 0.0046012,
  0.0045255, 0.0044887, 0.0044578, 0.0044469, 0.0044395, 0.0043641, 0.0043289, 0.0043194,
  0.0042668, 0.0042606, 0.0041039, 0.0040412, 0.0039663, 0.0039358, 0.0038849, 0.0038357,
  0.0037019, 0.0035701, 0.0035237, 0.0033517, 0.0033442, 0.0033007, 0.0032311, 0.0032205,
  0.0031774, 0.0031548, 0.0031153, 0.003101, 0.0030963, 0.0030443, 0.0030437, 0.0029951,
  0.0029807, 0.0029709, 0.0029628, 0.0029474, 0.0029176, 0.0029128, 0.002882, 0.0028696,
  0.0028474, 0.0028247, 0.0027107, 0.0026314, 0.0026313, 0.0026037, 0.0025952, 0.0025854,
  0.0025842, 0.0025629, 0.0025602, 0.0025505, 0.0025436, 0.0025425, 0.0025187, 0.0024875,
  0.0024866, 0.0024089, 0.0023074, 0.0022328, 0.0021629, 0.0018404,
];

const N = SHARES.length;

// θ is the q-mass bar: the demand share at which the sorted cumulative
// distribution crosses q. Subnets above the bar collectively carry q of demand.
function qMassBar(q: number): {theta: number; barRank: number} {
  let cum = 0;
  for (let i = 0; i < N; i++) {
    cum += SHARES[i];
    if (cum >= q) return {theta: SHARES[i], barRank: i + 1};
  }
  return {theta: SHARES[N - 1], barRank: N};
}

function gatedShares(theta: number, h: number): number[] {
  const weighted = SHARES.map((s) => {
    const sh = Math.pow(s, h);
    return (s * sh) / (sh + Math.pow(theta, h));
  });
  const z = weighted.reduce((a, b) => a + b, 0);
  return weighted.map((w) => w / z);
}

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
const pct = (v: number, d = 1) => `${(v * 100).toFixed(d)}%`;

const Slider = ({
  label,
  value,
  min,
  max,
  step,
  display,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  display: string;
  onChange: (value: number) => void;
}) => {
  const id = useId();
  return (
    <div style={{flex: 1, minWidth: 220}}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 6,
        }}
      >
        <label
          htmlFor={id}
          style={{
            fontFamily: 'FiraCode, monospace',
            fontSize: 10,
            textTransform: 'uppercase',
            letterSpacing: '0.08em',
            color: INK_FAINT,
          }}
        >
          {label}
        </label>
        <span style={{fontFamily: 'FiraCode, monospace', fontSize: 11, color: INK}}>{display}</span>
      </div>
      <input
        id={id}
        type='range'
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        style={{width: '100%', accentColor: ACCENT, cursor: 'pointer'}}
      />
    </div>
  );
};

const StatCell = ({label, before, after}: {label: string; before: string; after: string}) => (
  <div>
    <p
      style={{
        fontFamily: 'FiraCode, monospace',
        fontSize: 9,
        textTransform: 'uppercase',
        letterSpacing: '0.08em',
        color: INK_FAINT,
        margin: 0,
      }}
    >
      {label}
    </p>
    <p style={{fontFamily: 'FiraCode, monospace', fontSize: 12, margin: '4px 0 0', color: INK}}>
      {before} <span style={{color: INK_FAINT}}>→</span> <span style={{color: ACCENT}}>{after}</span>
    </p>
  </div>
);

export const EmissionGateExplorer = () => {
  const [q, setQ] = useState(0.61);
  const [h, setH] = useState(3);

  const {theta, barRank, after, belowBefore, belowAfter, top8Before, top8After, effNBefore, effNAfter} =
    useMemo(() => {
      const {theta, barRank} = qMassBar(q);
      const after = gatedShares(theta, h);
      return {
        theta,
        barRank,
        after,
        belowBefore: sum(SHARES.filter((s) => s < theta)),
        belowAfter: sum(after.filter((_, i) => SHARES[i] < theta)),
        top8Before: sum(SHARES.slice(0, 8)),
        top8After: sum(after.slice(0, 8)),
        effNBefore: 1 / sum(SHARES.map((s) => s * s)),
        effNAfter: 1 / sum(after.map((s) => s * s)),
      };
    }, [q, h]);

  // Plot geometry.
  const L = 52;
  const R = 738;
  const T = 34;
  const B = 296;
  const yMax = Math.max(after[0], SHARES[0]) * 1.08;
  const x = (rank: number) => L + ((rank - 1) / (N - 1)) * (R - L);
  const y = (share: number) => B - (share / yMax) * (B - T);
  const path = (data: number[]) =>
    data.map((s, i) => `${i === 0 ? 'M' : 'L'} ${x(i + 1).toFixed(1)} ${y(s).toFixed(1)}`).join(' ');

  const yTicks = [0, 0.02, 0.04, 0.06, 0.08].filter((t) => t <= yMax);
  const xTicks = [1, 16, 32, 48, 64, 96, 126];

  return (
    <div>
      <svg
        viewBox='0 0 760 330'
        role='img'
        style={{width: '100%', height: 'auto', marginTop: 8}}
        aria-label={`Emission share by demand rank for ${N} subnets. A muted line shows today's price-proportional emission; a red line shows gated emission. The bar sits at rank ${barRank}; below it emission collapses smoothly toward zero.`}
      >
        {yTicks.map((t) => (
          <g key={t}>
            <line x1={L} y1={y(t)} x2={R} y2={y(t)} stroke={GRID} strokeWidth='1' />
            <text {...MONO} x={L - 8} y={y(t) + 3} textAnchor='end' fill={INK_FAINT}>
              {(t * 100).toFixed(0)}%
            </text>
          </g>
        ))}
        {xTicks.map((r) => (
          <text key={r} {...MONO} x={x(r)} y={B + 18} textAnchor='middle' fill={INK_FAINT}>
            {r}
          </text>
        ))}
        <text {...MONO} x={(L + R) / 2} y={B + 34} textAnchor='middle' fill={INK_FAINT}>
          SUBNETS RANKED BY DEMAND SHARE
        </text>
        <text
          {...MONO}
          x={16}
          y={(T + B) / 2}
          textAnchor='middle'
          fill={INK_FAINT}
          transform={`rotate(-90 16 ${(T + B) / 2})`}
        >
          EMISSION SHARE
        </text>

        {/* the bar */}
        <line
          x1={x(barRank)}
          y1={T}
          x2={x(barRank)}
          y2={B}
          stroke={ACCENT}
          strokeWidth='1'
          strokeDasharray='4 4'
        />
        <text {...MONO} x={x(barRank) + 6} y={T + 10} fill={ACCENT}>
          BAR · RANK {barRank}
        </text>
        <line
          x1={L}
          y1={y(theta)}
          x2={R}
          y2={y(theta)}
          stroke={INK_FAINT}
          strokeWidth='1'
          strokeDasharray='2 4'
        />
        <text {...MONO} x={R} y={y(theta) - 5} textAnchor='end' fill={INK_FAINT}>
          θ = {pct(theta, 2)}
        </text>

        <path d={path(SHARES)} fill='none' stroke={INK_FAINT} strokeWidth='1.5' />
        <path d={path(after)} fill='none' stroke={ACCENT} strokeWidth='1.8' />

        <text {...MONO} x={x(10)} y={y(SHARES[9]) - 10} fill={INK_FAINT}>
          BEFORE · ∝ PRICE
        </text>
        <text {...MONO} x={x(4) + 8} y={y(after[3])} fill={ACCENT}>
          AFTER · GATED
        </text>
      </svg>

      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '16px 32px',
          justifyContent: 'space-between',
          marginTop: 16,
          padding: '12px 0',
          borderTop: `1px solid ${GRID}`,
          borderBottom: `1px solid ${GRID}`,
        }}
      >
        <StatCell
          label={`Below-bar emission (${N - barRank + 1} subnets)`}
          before={pct(belowBefore)}
          after={pct(belowAfter)}
        />
        <StatCell label='Top-8 emission' before={pct(top8Before)} after={pct(top8After)} />
        <StatCell
          label='Effective subnets (1/Σs²)'
          before={effNBefore.toFixed(0)}
          after={effNAfter.toFixed(0)}
        />
        <StatCell label='Hard-zeroed' before='0' after='0' />
      </div>

      <div style={{display: 'flex', flexWrap: 'wrap', gap: '16px 40px', marginTop: 16}}>
        <Slider
          label='q · demand mass above the bar'
          value={q}
          min={0.5}
          max={0.95}
          step={0.01}
          display={`q = ${q.toFixed(2)}`}
          onChange={setQ}
        />
        <Slider
          label='h · gate sharpness'
          value={h}
          min={1}
          max={6}
          step={0.5}
          display={`h = ${h.toFixed(1)}`}
          onChange={setH}
        />
      </div>
    </div>
  );
};
