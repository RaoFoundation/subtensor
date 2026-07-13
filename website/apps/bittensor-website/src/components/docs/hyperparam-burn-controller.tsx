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

const BLOCKS_PER_DAY = 7200; // 12s blocks
const SIM_BLOCKS = BLOCKS_PER_DAY; // 24h window
const SAMPLE_EVERY = 12;
const INITIAL_BURN_TAO = 0.1; // chain's InitialBurn (100_000_000 rao)

function formatTao(value: number): string {
  if (value >= 1000) return `τ${value.toFixed(0)}`;
  if (value >= 1) return `τ${value.toFixed(2)}`;
  return `τ${value.toPrecision(3)}`;
}

function sliderClass(focused: boolean): string {
  return focused
    ? 'border-l-2 border-[var(--bt-fg)] pl-3'
    : 'border-l-2 border-transparent pl-3';
}

export function HyperparamBurnController({ focus }: { focus?: string }) {
  const [minBurn, setMinBurn] = useState(0.0005);
  const [maxBurn, setMaxBurn] = useState(100);
  const [mult, setMult] = useState(1.26);
  const [halfLife, setHalfLife] = useState(360);
  const [regsPerDay, setRegsPerDay] = useState(20);
  const [manualRegs, setManualRegs] = useState(0);

  const sim = useMemo(() => {
    const effMax = Math.max(maxBurn, minBurn);
    const decay = Math.pow(0.5, 1 / halfLife);
    const clamp = (v: number) => Math.min(Math.max(v, minBurn), effMax);

    let burn = clamp(INITIAL_BURN_TAO);
    // "Register now" bumps land at the start of the window.
    for (let i = 0; i < manualRegs; i++) burn = clamp(burn * mult);

    const xs: number[] = [0];
    const ys: number[] = [burn];
    let peak = burn;
    let regsSoFar = 0;

    for (let b = 1; b <= SIM_BLOCKS; b++) {
      burn = clamp(burn * decay);
      const due = Math.floor((b * regsPerDay) / BLOCKS_PER_DAY);
      while (regsSoFar < due) {
        burn = clamp(burn * mult);
        regsSoFar++;
      }
      if (burn > peak) peak = burn;
      if (b % SAMPLE_EVERY === 0) {
        xs.push(b);
        ys.push(burn);
      }
    }

    return { xs, ys, peak, final: burn };
  }, [minBurn, maxBurn, mult, halfLife, regsPerDay, manualRegs]);

  // Rate at which per-registration bumps exactly cancel the decay:
  // mult * 0.5^(gap / half_life) = 1  =>  gap = half_life * log2(mult).
  const breakEven = mult > 1 ? BLOCKS_PER_DAY / (halfLife * Math.log2(mult)) : Infinity;

  const data = useMemo(
    () => ({
      labels: sim.xs.map((b) => `${(b / 300).toFixed(1)}h`),
      datasets: [
        {
          label: 'Burn cost (τ)',
          data: sim.ys,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgba(41, 41, 41, 0.08)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
      ],
    }),
    [sim],
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
            title: (items: { dataIndex: number }[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `${((sim.xs[idx] ?? 0) / 300).toFixed(1)}h in`;
            },
            label: (ctx: { parsed: { y: number } }) => formatTao(ctx.parsed.y),
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
      title="Registration burn controller"
      caption="One simulated day. Every block the burn decays by f where f^burn_half_life = 1/2; every registration multiplies it by burn_increase_mult; the result is always clamped to [min_burn, max_burn]."
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={() => setManualRegs((n) => n + 1)}
          className="border border-line bg-bg px-3 py-1.5 font-mono text-xs hover:bg-panel"
        >
          Register now (+1)
        </button>
        {manualRegs > 0 && (
          <button
            type="button"
            onClick={() => setManualRegs(0)}
            className="border border-line bg-bg px-3 py-1.5 font-mono text-xs text-mute hover:bg-panel"
          >
            Reset ({manualRegs})
          </button>
        )}
        <span className="text-[0.75rem] text-mute">
          Each click adds a registration at the start of the window.
        </span>
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat label="Peak in window" value={formatTao(sim.peak)} />
        <ExplainerStat label="After 24h" value={formatTao(sim.final)} />
        <ExplainerStat
          label="Break-even rate"
          value={Number.isFinite(breakEven) ? `${breakEven.toFixed(0)} regs/day` : 'none'}
          hint="Above this, the price climbs; below it, decay wins."
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <div className={sliderClass(focus === 'min_burn')}>
          <ExplainerSlider
            label="min_burn"
            value={minBurn}
            min={0.0005}
            max={1}
            step={0.0005}
            display={formatTao(minBurn)}
            onChange={setMinBurn}
          />
        </div>
        <div className={sliderClass(focus === 'max_burn')}>
          <ExplainerSlider
            label="max_burn"
            value={maxBurn}
            min={0.1}
            max={500}
            step={0.1}
            display={formatTao(maxBurn)}
            onChange={setMaxBurn}
          />
        </div>
        <div className={sliderClass(focus === 'burn_increase_mult')}>
          <ExplainerSlider
            label="burn_increase_mult"
            value={mult}
            min={1}
            max={3}
            step={0.01}
            display={`×${mult.toFixed(2)}`}
            onChange={setMult}
          />
        </div>
        <div className={sliderClass(focus === 'burn_half_life')}>
          <ExplainerSlider
            label="burn_half_life"
            value={halfLife}
            min={30}
            max={7200}
            step={30}
            display={`${halfLife} blocks (~${Math.round((halfLife * 12) / 60)}m)`}
            onChange={setHalfLife}
          />
        </div>
        <div className={sliderClass(false)}>
          <ExplainerSlider
            label="registrations / day"
            value={regsPerDay}
            min={0}
            max={240}
            step={1}
            display={`${regsPerDay}`}
            onChange={setRegsPerDay}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
