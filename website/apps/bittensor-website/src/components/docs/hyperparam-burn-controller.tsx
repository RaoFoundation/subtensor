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

const BLOCKS_PER_DAY = 7200; // 12s blocks

interface Preset {
  minBurn: number;
  maxBurn: number;
  mult: number;
  halfLife: number;
  regsPerDay: number;
  startBurn: number;
  windowBlocks: number;
  sampleEvery: number;
  /** Scheduled burst of registrations (used by the burn_increase_mult scenario). */
  burst?: { startBlock: number; spacing: number; count: number };
}

const DEFAULT_PRESET: Preset = {
  minBurn: 0.0005,
  maxBurn: 100,
  mult: 1.26,
  halfLife: 360,
  regsPerDay: 20,
  startBurn: 0.1, // chain's InitialBurn (100_000_000 rao)
  windowBlocks: BLOCKS_PER_DAY,
  sampleEvery: 12,
};

const PRESETS: Record<string, Preset> = {
  // No demand: a moderate spike decays all the way down and rests on the floor.
  min_burn: {
    ...DEFAULT_PRESET,
    minBurn: 0.005,
    regsPerDay: 0,
    startBurn: 5,
  },
  // A rush drives the price into the ceiling, where it pins.
  max_burn: {
    ...DEFAULT_PRESET,
    maxBurn: 5,
    regsPerDay: 150,
  },
  // A short window with a burst of registrations so each ×mult step is visible.
  burn_increase_mult: {
    ...DEFAULT_PRESET,
    regsPerDay: 0,
    windowBlocks: 1440,
    sampleEvery: 2,
    burst: { startBlock: 60, spacing: 30, count: 8 },
  },
  // A big spike decaying, with markers at each half-life.
  // sampleEvery must divide the half-life slider step (30) so markers land on samples.
  burn_half_life: {
    ...DEFAULT_PRESET,
    regsPerDay: 0,
    startBurn: 10,
    sampleEvery: 30,
  },
};

const CAPTIONS: Record<string, string> = {
  min_burn:
    'One simulated day with no registrations: the price decays exponentially until it lands on the min_burn floor (dashed line) and rests there. min_burn is the resting price of an idle subnet — drag it to move the floor.',
  max_burn:
    'One simulated day under a registration rush (150/day): each registration multiplies the price up until it pins at the max_burn ceiling (dashed line). While pinned, every registration costs exactly max_burn.',
  burn_increase_mult:
    'A burst of 8 registrations, one every 30 blocks (~6 min): each multiplies the price by burn_increase_mult, stacking into a staircase that the half-life decay then unwinds. Drag the multiplier to steepen or flatten the stairs.',
  burn_half_life:
    'A τ10 spike decaying with no registrations. Each marker sits one burn_half_life after the previous, at half its price, until the min_burn floor. Drag the half-life to stretch or compress the cooldown.',
};

const DEFAULT_CAPTION =
  'One simulated day. Every block the burn decays by f where f^burn_half_life = 1/2; every registration multiplies it by burn_increase_mult; the result is always clamped to [min_burn, max_burn].';

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

function formatBlocksElapsed(blocks: number, windowBlocks: number): string {
  if (windowBlocks <= 1800) return `${blocks * 12 >= 3600 ? `${((blocks * 12) / 3600).toFixed(1)}h` : `${Math.round((blocks * 12) / 60)}m`}`;
  return `${(blocks / 300).toFixed(1)}h`;
}

export function HyperparamBurnController({ focus }: { focus?: string }) {
  const preset = PRESETS[focus ?? ''] ?? DEFAULT_PRESET;
  const [minBurn, setMinBurn] = useState(preset.minBurn);
  const [maxBurn, setMaxBurn] = useState(preset.maxBurn);
  const [mult, setMult] = useState(preset.mult);
  const [halfLife, setHalfLife] = useState(preset.halfLife);
  const [regsPerDay, setRegsPerDay] = useState(preset.regsPerDay);
  const [manualRegs, setManualRegs] = useState(0);

  const { windowBlocks, sampleEvery, burst, startBurn } = preset;

  const sim = useMemo(() => {
    const effMax = Math.max(maxBurn, minBurn);
    const decay = Math.pow(0.5, 1 / halfLife);
    const clamp = (v: number) => Math.min(Math.max(v, minBurn), effMax);

    let burn = clamp(startBurn);
    // "Register now" bumps land at the start of the window.
    for (let i = 0; i < manualRegs; i++) burn = clamp(burn * mult);

    const xs: number[] = [0];
    const ys: number[] = [burn];
    let peak = burn;
    let low = burn;
    let regsSoFar = 0;

    for (let b = 1; b <= windowBlocks; b++) {
      burn = clamp(burn * decay);
      const due = Math.floor((b * regsPerDay) / BLOCKS_PER_DAY);
      while (regsSoFar < due) {
        burn = clamp(burn * mult);
        regsSoFar++;
      }
      if (burst) {
        const offset = b - burst.startBlock;
        if (offset >= 0 && offset % burst.spacing === 0 && offset / burst.spacing < burst.count) {
          burn = clamp(burn * mult);
        }
      }
      if (burn > peak) peak = burn;
      if (burn < low) low = burn;
      if (b % sampleEvery === 0) {
        xs.push(b);
        ys.push(burn);
      }
    }

    return { xs, ys, peak, low, final: burn };
  }, [minBurn, maxBurn, mult, halfLife, regsPerDay, manualRegs, windowBlocks, sampleEvery, burst, startBurn]);

  // Rate at which per-registration bumps exactly cancel the decay:
  // mult * 0.5^(gap / half_life) = 1  =>  gap = half_life * log2(mult).
  const breakEven = mult > 1 ? BLOCKS_PER_DAY / (halfLife * Math.log2(mult)) : Infinity;

  const data = useMemo(() => {
    // Only draw a clamp line when it is near the curve (or is the focused
    // parameter), so it does not stretch the log axis into empty space.
    const showMinLine = focus === 'min_burn' || sim.low <= minBurn * 2;
    const showMaxLine = focus === 'max_burn' || sim.peak >= maxBurn * 0.5;

    const datasets: ChartData<'line', (number | null)[]>['datasets'] = [
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
    ];

    if (showMinLine) {
      datasets.push({
        label: 'min_burn',
        data: sim.ys.map(() => minBurn),
        borderColor: focus === 'min_burn' ? 'rgb(41, 41, 41)' : 'rgba(41, 41, 41, 0.3)',
        borderWidth: focus === 'min_burn' ? 1.5 : 1,
        borderDash: focus === 'min_burn' ? [6, 4] : [2, 3],
        pointRadius: 0,
        fill: false,
      });
    }
    if (showMaxLine) {
      datasets.push({
        label: 'max_burn',
        data: sim.ys.map(() => Math.max(maxBurn, minBurn)),
        borderColor: focus === 'max_burn' ? 'rgb(41, 41, 41)' : 'rgba(41, 41, 41, 0.3)',
        borderWidth: focus === 'max_burn' ? 1.5 : 1,
        borderDash: focus === 'max_burn' ? [6, 4] : [2, 3],
        pointRadius: 0,
        fill: false,
      });
    }

    if (focus === 'burn_half_life') {
      // Mark the curve at 1, 2, 3, 4 half-lives; each is half the previous
      // while decay is uninterrupted and the floor has not been reached.
      const markers = sim.ys.map((y, i) => {
        const b = sim.xs[i] ?? 0;
        const isHalfLifeMultiple = b > 0 && b % halfLife === 0 && b / halfLife <= 4;
        return isHalfLifeMultiple && y > minBurn * 1.01 ? y : null;
      });
      datasets.push({
        label: 'half-life markers',
        data: markers,
        borderColor: 'rgb(41, 41, 41)',
        backgroundColor: 'rgb(41, 41, 41)',
        pointRadius: 4,
        pointStyle: 'rectRot' as const,
        showLine: false,
        fill: false,
      });
    }

    return {
      labels: sim.xs.map((b) => formatBlocksElapsed(b, windowBlocks)),
      datasets,
    };
  }, [sim, focus, minBurn, maxBurn, halfLife, windowBlocks]);

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: {
        legend: { display: false },
        tooltip: {
          filter: (item: { datasetIndex: number }) => item.datasetIndex === 0,
          callbacks: {
            title: (items: { dataIndex: number }[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `${formatBlocksElapsed(sim.xs[idx] ?? 0, windowBlocks)} in`;
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
    [sim.xs, windowBlocks],
  );

  return (
    <ExplainerPanel
      title="Registration burn controller"
      caption={CAPTIONS[focus ?? ''] ?? DEFAULT_CAPTION}
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
        <ExplainerStat
          label={windowBlocks === BLOCKS_PER_DAY ? 'After 24h' : `After ~${Math.round((windowBlocks * 12) / 3600 * 10) / 10}h`}
          value={formatTao(sim.final)}
        />
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
