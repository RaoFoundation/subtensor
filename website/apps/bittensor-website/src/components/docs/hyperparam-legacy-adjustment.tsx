'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  LogarithmicScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
} from 'chart.js';
import type { ChartData } from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(
  CategoryScale,
  LinearScale,
  LogarithmicScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
);

const WINDOW_BLOCKS = 1800; // ~6h at 12s blocks
const BURST_END = 600; // demand for the first ~2h, then quiet
const REGS_PER_INTERVAL_OF_100 = 12; // demand rate during the burst
const TARGET = 2; // legacy setpoint (chain default for target_regs_per_interval)
const SAMPLE_EVERY = 4;
const MIN_BURN = 0.0005;
const MAX_BURN = 100;
const START_BURN = 0.5;
const NEW_MULT = 1.26; // current burn_increase_mult default
const NEW_HALF_LIFE = 360; // current burn_half_life default

const CAPTIONS: Record<string, string> = {
  adjustment_alpha:
    'The same demand burst (~2h of registrations, then quiet) priced two ways. Then (dotted steps): once per interval the price jumped to an EMA-blended value — adjustment_alpha is the weight on the old price, so higher alpha means slower steps. Now (solid): every registration bumps the price and it decays every block; alpha changes nothing on this curve.',
  adjustment_interval:
    'The same demand burst (~2h of registrations, then quiet) priced two ways. Then (dotted steps): the price only moved once per adjustment_interval — drag it to make the steps wider or narrower. Now (solid): the price reacts per registration and decays per block; the interval no longer affects it.',
};

function formatTao(value: number): string {
  if (value >= 1000) return `τ${value.toFixed(0)}`;
  if (value >= 1) return `τ${value.toFixed(2)}`;
  return `τ${value.toPrecision(3)}`;
}

/** Blocks at which a registration lands, evenly spread through the burst. */
function registrationBlocks(): number[] {
  const total = Math.round((BURST_END / 100) * REGS_PER_INTERVAL_OF_100);
  const blocks: number[] = [];
  for (let i = 0; i < total; i++) {
    blocks.push(Math.round(((i + 1) * BURST_END) / total));
  }
  return blocks;
}

export function HyperparamLegacyAdjustment({ focus }: { focus?: string }) {
  const [alpha, setAlpha] = useState(0);
  const [interval, setInterval_] = useState(100);

  const sim = useMemo(() => {
    const clamp = (v: number) => Math.min(Math.max(v, MIN_BURN), MAX_BURN);
    const regBlocks = new Set(registrationBlocks());

    // "Then": price frozen within each interval; at the boundary,
    // proposed = price × (regs + target) / (2 × target), blended by alpha.
    let thenPrice = START_BURN;
    let regsThisInterval = 0;

    // "Now": per-registration ×mult bump, per-block half-life decay.
    let nowPrice = START_BURN;
    const decay = Math.pow(0.5, 1 / NEW_HALF_LIFE);

    const xs: number[] = [0];
    const thenYs: number[] = [thenPrice];
    const nowYs: number[] = [nowPrice];

    for (let b = 1; b <= WINDOW_BLOCKS; b++) {
      const hasReg = regBlocks.has(b);

      if (hasReg) regsThisInterval++;
      if (b % interval === 0) {
        const proposed = (thenPrice * (regsThisInterval + TARGET)) / (2 * TARGET);
        thenPrice = clamp(alpha * thenPrice + (1 - alpha) * proposed);
        regsThisInterval = 0;
      }

      nowPrice = clamp(nowPrice * decay);
      if (hasReg) nowPrice = clamp(nowPrice * NEW_MULT);

      if (b % SAMPLE_EVERY === 0) {
        xs.push(b);
        thenYs.push(thenPrice);
        nowYs.push(nowPrice);
      }
    }

    return { xs, thenYs, nowYs };
  }, [alpha, interval]);

  const data = useMemo(() => {
    const datasets: ChartData<'line', number[]>['datasets'] = [
      {
        label: 'now: continuous bump + decay',
        data: sim.nowYs,
        borderColor: 'rgb(41, 41, 41)',
        backgroundColor: 'rgba(41, 41, 41, 0.08)',
        fill: true,
        tension: 0,
        pointRadius: 0,
        borderWidth: 1.5,
      },
      {
        label: 'then: interval EMA steps',
        data: sim.thenYs,
        borderColor: 'rgba(41, 41, 41, 0.45)',
        borderDash: [4, 3],
        stepped: true,
        pointRadius: 0,
        borderWidth: 1.25,
        fill: false,
      },
    ];
    return {
      labels: sim.xs.map((b) => `${((b * 12) / 3600).toFixed(1)}h`),
      datasets,
    };
  }, [sim]);

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            title: (items: { dataIndex: number }[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `${(((sim.xs[idx] ?? 0) * 12) / 3600).toFixed(1)}h in`;
            },
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label}: ${formatTao(ctx.parsed.y)}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { maxTicksLimit: 8, font: { family: 'FiraCode, monospace', size: 10 } },
        },
        y: {
          type: 'logarithmic' as const,
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: {
            maxTicksLimit: 6,
            font: { family: 'FiraCode, monospace', size: 10 },
            callback: (value: string | number) => formatTao(Number(value)),
          },
          title: { display: true, text: 'burn (τ, log)', font: { size: 11 } },
        },
      },
    }),
    [sim.xs],
  );

  return (
    <ExplainerPanel
      title="Then vs now: registration pricing"
      caption={CAPTIONS[focus ?? ''] ?? CAPTIONS.adjustment_interval}
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Then (dotted)"
          value="steps per interval"
          hint={`next = α·old + (1−α)·old·(regs+${TARGET})/${2 * TARGET}`}
        />
        <ExplainerStat
          label="Now (solid)"
          value="moves every block"
          hint={`×${NEW_MULT} per registration, half-life ${NEW_HALF_LIFE} blocks`}
        />
        <ExplainerStat
          label="These sliders affect"
          value="only the dotted line"
          hint="On the current chain both values are stored but unused."
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <div
          className={
            focus === 'adjustment_alpha'
              ? 'border-l-2 border-[var(--bt-fg)] pl-3'
              : 'border-l-2 border-transparent pl-3'
          }
        >
          <ExplainerSlider
            label="adjustment_alpha (legacy)"
            value={alpha}
            min={0}
            max={0.95}
            step={0.05}
            display={alpha.toFixed(2)}
            onChange={setAlpha}
          />
        </div>
        <div
          className={
            focus === 'adjustment_interval'
              ? 'border-l-2 border-[var(--bt-fg)] pl-3'
              : 'border-l-2 border-transparent pl-3'
          }
        >
          <ExplainerSlider
            label="adjustment_interval (legacy)"
            value={interval}
            min={50}
            max={600}
            step={50}
            display={`${interval} blocks (~${Math.round((interval * 12) / 60)}m)`}
            onChange={setInterval_}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
