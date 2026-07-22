'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  Tooltip,
  Legend,
  type Plugin,
} from 'chart.js';
import { Bar } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import { AXIS_BORDER, GRAPH_FONT, GRID, INK, INK_FAINT, axisTitle, baseTicks } from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend);

// Two validators, two miners. V1 splits its weight; V2 goes all-in on M1.
const WEIGHTS = [
  [0.5, 0.5], // V1
  [1.0, 0.0], // V2
];

function normalize(v: number[]): number[] {
  const sum = v.reduce((a, b) => a + b, 0);
  return sum > 0 ? v.map((x) => x / sum) : v;
}

// Steady state assumed (EMA bonds have converged onto the weights), which isolates
// the structural difference between the two dividend paths in run_epoch.rs.
function dividends(stakes: number[], yuma3: boolean): number[] {
  const incentive = normalize(
    WEIGHTS[0].map((_, j) => stakes.reduce((acc, s, i) => acc + s * WEIGHTS[i][j], 0)),
  );

  // Classic: bonds are W ∘ S column-normalized, so stake enters the bond matrix.
  // Yuma3: bonds are the validator's own weight proportions (get_bonds_fixed_proportion),
  // column-normalized, and stake scales the dividend once at the end.
  const bond = (i: number, j: number) => (yuma3 ? WEIGHTS[i][j] : WEIGHTS[i][j] * stakes[i]);
  const colSums = WEIGHTS[0].map((_, j) => stakes.reduce((acc, _s, i) => acc + bond(i, j), 0));
  const perValidator = stakes.map((_s, i) =>
    incentive.reduce((acc, inc, j) => acc + (colSums[j] > 0 ? bond(i, j) / colSums[j] : 0) * inc, 0),
  );

  return normalize(yuma3 ? perValidator.map((d, i) => d * stakes[i]) : perValidator);
}

// Direct in-plot labels above the V1 bar pair (which always has the most
// headroom of the two groups), replacing the Chart.js legend. Positions are
// read from live bar metadata at draw time, so no stale-closure ref is needed.
const barLabelPlugin: Plugin<'bar'> = {
  id: 'yuma3BarLabels',
  afterDatasetsDraw(chart) {
    const { ctx, chartArea } = chart;
    const labels: { datasetIndex: number; text: string; color: string }[] = [
      { datasetIndex: 0, text: 'CLASSIC', color: INK_FAINT },
      { datasetIndex: 1, text: 'YUMA3', color: INK },
    ];
    ctx.save();
    ctx.font = GRAPH_FONT;
    ctx.textAlign = 'center';
    for (const { datasetIndex, text, color } of labels) {
      const bar = chart.getDatasetMeta(datasetIndex)?.data?.[0];
      if (!bar) continue;
      ctx.fillStyle = color;
      ctx.fillText(text, bar.x, Math.max(bar.y - 6, chartArea.top + 10));
    }
    ctx.restore();
  },
};

export function HyperparamYuma3Chart() {
  const [bigStake, setBigStake] = useState(0.8);

  const stakes = useMemo(() => [1 - bigStake, bigStake], [bigStake]);
  const classic = useMemo(() => dividends(stakes, false), [stakes]);
  const yuma3 = useMemo(() => dividends(stakes, true), [stakes]);

  const data = useMemo(
    () => ({
      labels: ['V1 (splits 50/50)', 'V2 (all-in on M1)'],
      datasets: [
        {
          label: 'classic dividends (yuma3 off)',
          data: classic,
          backgroundColor: 'rgba(41, 41, 41, 0.3)',
          borderColor: INK,
          borderWidth: 1,
        },
        {
          label: 'Yuma3 dividends (yuma3 on)',
          data: yuma3,
          backgroundColor: 'rgba(41, 41, 41, 0.85)',
          borderColor: INK,
          borderWidth: 1,
        },
      ],
    }),
    [classic, yuma3],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label ?? ''}: ${(ctx.parsed.y * 100).toFixed(1)}%`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks(),
        },
        y: {
          min: 0,
          max: 1,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({
            maxTicksLimit: 5,
            callback: (v: string | number) => `${Math.round(Number(v) * 100)}%`,
          }),
          title: axisTitle('share of validator dividends'),
        },
      },
    }),
    [],
  );

  return (
    <ExplainerPanel
      title="yuma3_enabled: classic vs Yuma3 dividends"
      caption={
        <>
          Two validators, two miners: V1 splits its weight 50/50, V2 puts everything on M1.
          Classic (off) column-normalizes ΔB = W ∘ S, so stake is baked into each miner&apos;s
          bond column. Yuma3 (on) keeps bonds as each validator&apos;s own weight proportions
          (
          <a
            href="/code/pallets/subtensor/src/epoch/run_epoch.rs#L1230-L1238"
            className="underline"
          >
            get_bonds_fixed_proportion
          </a>
          ), sums normalized bonds × incentive per validator, then scales by stake once. Bonds
          shown at steady state.
        </>
      }
    >
      <div className="h-56">
        <Bar data={data} options={options} plugins={[barLabelPlugin]} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="V1 dividends"
          value={`${(classic[0] * 100).toFixed(1)}% → ${(yuma3[0] * 100).toFixed(1)}%`}
          hint="classic → Yuma3"
        />
        <ExplainerStat
          label="V2 dividends"
          value={`${(classic[1] * 100).toFixed(1)}% → ${(yuma3[1] * 100).toFixed(1)}%`}
          hint="classic → Yuma3"
        />
        <ExplainerStat
          label="Stake split (V1 / V2)"
          value={`${((1 - bigStake) * 100).toFixed(0)}% / ${(bigStake * 100).toFixed(0)}%`}
          hint="dividends match stake only when weights agree"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="V2 stake share"
          value={bigStake}
          min={0.05}
          max={0.95}
          step={0.05}
          display={`${(bigStake * 100).toFixed(0)}% of total stake`}
          onChange={setBigStake}
        />
      </div>
    </ExplainerPanel>
  );
}
